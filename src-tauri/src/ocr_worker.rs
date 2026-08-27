//! Running the OCR engine in a process of its own.
//!
//! [`crate::ocr`] measured why this exists and `examples/ocr_sandbox_probe.rs`
//! now re-measures it on demand: under the parser worker's profile Vision is
//! **killed by SIGTRAP**, and it needs general read authority to run at all. The
//! second half is why it cannot share the parser's boundary --- a process holding
//! a hostile document must not be able to read the user's files. The first half
//! is why it must not share *any* process whose loss matters: a library that
//! takes its host down cannot sit next to unsaved annotations.
//!
//! ## Both ends of the wire are in this one file, deliberately
//!
//! `worker.rs` and `worker_child.rs` are split because they are 1,400 and 900
//! lines. This is neither, and `docs/TRAPS.md` records what the split cost there:
//! *one untyped reply carrier, and the two ways serde refuses to replace it* ---
//! a payload's shape living in two processes with nothing holding the copies
//! together, which had already produced one wrong measurement. [`Ask`] and
//! [`Said`] are written once and both ends name the same types.
//!
//! ## What this process is not
//!
//! It maps **no PDF parser**. The parser worker exists because PDF is
//! attacker-authored structure --- an object graph, filters, fonts, a
//! decompressor. This one consumes a fixed-size RGBA buffer that *we* rendered:
//! no format to parse, no lengths to trust, no recursion. That is a categorically
//! smaller surface, and it is why a laxer profile here is a considered trade
//! rather than a concession.

// `BufRead`, `BufReader`, `Command` and `Stdio` are used only by the macOS arms
// below, and importing them here made three compile gates red on Windows and
// green on a Mac --- `docs/TRAPS.md` records that an unused-import warning on one
// platform is not an unused import. They are imported where they are used.
use std::io::Write;
use std::process::{Child, ChildStdin};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::ocr::{EngineId, Options, Pixels, RecogniseError, RecognisedItem};
use crate::worker_shm::Shm;

/// The marker argument that turns this executable into an OCR worker.
pub const OCR_WORKER_ARGV: &str = "--ocr-worker";

/// The descriptor the pixel buffer arrives on in the child.
///
/// Three, the first free number after the standard streams, and the same choice
/// [`crate::worker::DOC_FD`] makes for the same reason.
pub const PIXELS_FD: i32 = 3;

/// How large a probe image the buffer will hold.
///
/// Sixteen megabytes, which is [`crate::worker::TILE_CAPACITY`]'s figure and is
/// picked here for a different reason: a redaction region plus a control strip
/// is a fraction of a page, so this is a bound rather than a working size, and a
/// caller that exceeds it is refused rather than silently truncated.
pub const PIXELS_CAPACITY: usize = 16 * 1024 * 1024;

/// How long the parent waits for a reply before giving up on the process.
///
/// **The engine's own `deadline_ms` is not a deadline.** `Options` carries one
/// and the Vision binding does not pass it to anything --- `VNImageRequestHandler`
/// has no timeout --- so an engine that wedges wedges the child, and a parent
/// reading a line would wait for ever. `docs/TRAPS.md` has *a check whose failure
/// mode is a wait cannot fail*; this is the same defect one layer out, and the
/// only place it can be fixed is here, because only the parent survives it.
pub const REPLY_DEADLINE: Duration = Duration::from_secs(30);

/// What the parent asks for.
///
/// The pixels are not in it --- they are in the shared mapping, and this says how
/// to read them. Sending them down the pipe would be a base64 of megabytes per
/// call for a buffer both processes can already see.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ask {
    /// Width in pixels of the image at the front of the mapping.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels per point, carried through to [`RecognisedItem::rect`].
    pub scale: f32,
    /// What to ask the engine for.
    pub options: Options,
}

/// What the child answers.
///
/// Externally tagged, which is serde's default and is the only encoding of the
/// three that is safe here: `docs/TRAPS.md` records internal tagging refusing a
/// bare payload at runtime and untagged silently swapping two variants of the
/// same shape.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Said {
    /// The engine ran, and this is what it read --- possibly nothing.
    Read {
        /// Which engine, as the process that ran it reports itself.
        engine: Named,
        /// The spans, in the convention [`RecognisedItem`] documents.
        items: Vec<RecognisedItem>,
    },
    /// The engine did not produce a result.
    Failed(RecogniseError),
}

/// An [`EngineId`] as it survives a wire.
///
/// [`EngineId::name`] is a `&'static str`, so it cannot be deserialised from
/// arbitrary text. Carried as a `String` and resolved against the names this
/// build knows: a name it does not recognise is a **refusal**, because the only
/// way to see one is a child from a different build, and an engine identity that
/// cannot be resolved is one nothing downstream can invalidate against.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Named {
    /// The short stable name, one of [`KNOWN_ENGINES`].
    pub name: String,
    /// The build string, which moves with the OS.
    pub build: String,
}

/// The engine names this build can resolve.
pub const KNOWN_ENGINES: [&str; 2] = ["vision", "windows-ocr"];

impl Named {
    /// The [`EngineId`] this stands for.
    ///
    /// # Errors
    ///
    /// A name outside [`KNOWN_ENGINES`].
    pub fn resolve(&self) -> Result<EngineId, String> {
        let name = KNOWN_ENGINES
            .iter()
            .find(|known| **known == self.name)
            .ok_or_else(|| {
                format!(
                    "an OCR worker reported the engine {:?}, which this build does not know; \
                     an identity that cannot be resolved is one nothing can be invalidated \
                     against",
                    self.name
                )
            })?;
        Ok(EngineId {
            name,
            build: self.build.clone(),
        })
    }
}

/// The message a platform with no engine is refused with.
pub const NO_ENGINE: &str = "no OCR engine is implemented on this platform, so nothing here can \
                             be shown to have been read";

/// Whether an image can be handed over at all, and why not.
///
/// A free function rather than two `if`s inside [`OcrWorker::recognise`], for the
/// reason `docs/TRAPS.md` gives about a guard written inline with an FFI call:
/// the only way to reach one there is to have a live child, so the check that
/// stops a malformed image ever reaching another process would be tested by
/// spawning one. Here it is tested by calling it.
///
/// **Two rules and two messages**, deliberately. An image whose buffer is not its
/// own dimensions and an image the mapping cannot hold are different mistakes
/// with different remedies, and one outcome that two mechanisms produce cannot
/// test either one.
///
/// # Errors
///
/// A buffer that does not match the dimensions it is described by, or an image
/// larger than `capacity`.
fn room_for(pixels: &Pixels<'_>, capacity: usize) -> Result<(), RecogniseError> {
    if !pixels.is_consistent() {
        return Err(RecogniseError::MalformedInput(format!(
            "{} byte(s) is not {}x{} of RGBA",
            pixels.rgba.len(),
            pixels.width,
            pixels.height
        )));
    }
    if pixels.rgba.len() > capacity {
        return Err(RecogniseError::MalformedInput(format!(
            "the image is {} bytes and the buffer holds {capacity}",
            pixels.rgba.len()
        )));
    }
    Ok(())
}

/// A process holding an OCR engine.
///
/// **Not a [`crate::ocr::Recogniser`]**, and the difference is not tidiness. That
/// trait answers `id()` before it has run anything; a worker cannot, because the
/// identity that matters is the one the process that actually read reports, and
/// before the first reply there is nothing to report. So [`recognise`](Self::recognise)
/// returns the id beside the items rather than promising one in advance.
pub struct OcrWorker {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<std::io::Result<String>>,
    pixels: Shm,
    /// Set once the process has been given up on, so a second call says why
    /// rather than waiting another deadline for a process that is gone.
    dead: Option<String>,
}

impl OcrWorker {
    /// Starts one.
    ///
    /// # Errors
    ///
    /// Creating the mapping or spawning; and on any platform with no engine,
    /// always --- see [`NO_ENGINE`].
    #[cfg(not(target_os = "macos"))]
    pub fn spawn() -> Result<Self, String> {
        Err(NO_ENGINE.into())
    }

    /// Starts one.
    ///
    /// Returns as soon as `fork`/`exec` has been issued. The child then maps the
    /// pixel buffer, puts the profile in force and waits; none of that is on
    /// anyone's critical path until the first [`recognise`](Self::recognise).
    ///
    /// # Errors
    ///
    /// Creating the mapping, or spawning.
    #[cfg(target_os = "macos")]
    pub fn spawn() -> Result<Self, String> {
        use std::io::{BufRead, BufReader};
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        let pixels = Shm::create(PIXELS_CAPACITY)?;
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let mut command = Command::new(exe);
        command
            .arg(OCR_WORKER_ARGV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, as the parser worker's is: a sandbox denial or a fatal
            // signal has to be visible somewhere.
            .stderr(Stdio::inherit());

        let fd = pixels.raw_fd();
        // SAFETY: only dup/dup2/close run between fork and exec, all of which are
        // async-signal-safe. The source is dup'd first because it may already be
        // sitting on the target number.
        unsafe {
            command.pre_exec(move || {
                let scratch = libc::dup(fd);
                if scratch < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::dup2(scratch, PIXELS_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if scratch != PIXELS_FD {
                    libc::close(scratch);
                }
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        let stdin = child.stdin.take().ok_or("the OCR worker has no stdin")?;
        let stdout = child.stdout.take().ok_or("the OCR worker has no stdout")?;
        let (tx, replies) = std::sync::mpsc::channel();
        // A thread rather than a blocking read, so that `recognise` can bound its
        // wait. There is nowhere else to put the bound: the engine ignores the
        // deadline it is handed, so the child cannot enforce one on itself.
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        Ok(Self {
            child,
            stdin,
            replies,
            pixels,
            dead: None,
        })
    }

    /// The child's process id, for a harness that wants to look at it.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Reads text off an image, in the other process.
    ///
    /// # Errors
    ///
    /// An image the mapping cannot hold or whose buffer does not match its own
    /// dimensions; a child that has died, answered nothing within
    /// [`REPLY_DEADLINE`], or answered something this build cannot read; and
    /// whatever the engine itself reported.
    pub fn recognise(
        &mut self,
        pixels: Pixels<'_>,
        options: &Options,
    ) -> Result<(EngineId, Vec<RecognisedItem>), RecogniseError> {
        if let Some(why) = &self.dead {
            return Err(RecogniseError::Crashed(why.clone()));
        }
        room_for(&pixels, self.pixels.len())?;
        self.pixels.as_mut_slice()[..pixels.rgba.len()].copy_from_slice(pixels.rgba);

        let ask = Ask {
            width: pixels.width,
            height: pixels.height,
            scale: pixels.scale,
            options: options.clone(),
        };
        let line = serde_json::to_string(&ask).map_err(|e| {
            RecogniseError::MalformedInput(format!("the request will not encode: {e}"))
        })?;
        if let Err(e) = writeln!(self.stdin, "{line}").and_then(|()| self.stdin.flush()) {
            return Err(self.give_up(format!("the OCR worker stopped listening: {e}")));
        }

        let reply = match self.replies.recv_timeout(REPLY_DEADLINE) {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => return Err(self.give_up(format!("the OCR worker's reply broke: {e}"))),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(self.give_up("the OCR worker stopped without answering".into()))
            }
            Err(RecvTimeoutError::Timeout) => {
                let why = format!(
                    "the OCR worker did not answer within {} seconds",
                    REPLY_DEADLINE.as_secs()
                );
                let why = self.give_up(why);
                return Err(match why {
                    RecogniseError::Crashed(message) => RecogniseError::TimedOut(message),
                    other => other,
                });
            }
        };

        match serde_json::from_str::<Said>(&reply) {
            Err(e) => Err(self.give_up(format!("the OCR worker's reply did not parse: {e}"))),
            Ok(Said::Failed(why)) => Err(why),
            Ok(Said::Read { engine, items }) => match engine.resolve() {
                Ok(id) => Ok((id, items)),
                Err(why) => Err(self.give_up(why)),
            },
        }
    }

    /// Kills the child and records why, so a later call says so at once.
    fn give_up(&mut self, why: String) -> RecogniseError {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.dead = Some(why.clone());
        RecogniseError::Crashed(why)
    }
}

impl Drop for OcrWorker {
    /// Kills rather than waiting for stdin to close.
    ///
    /// The parser worker's pool retires a worker by dropping its stdin and
    /// letting the child notice; there is no pool here, and an engine mid-call
    /// does not read its pipe. `docs/TRAPS.md` has *a pre-spawned worker outlived
    /// its parent, and the claim that it cannot is untested by design* --- this
    /// is the same hazard with a much shorter answer available.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------- the child

/// The child's entry point. Never returns.
///
/// **The order of the first two steps is the whole of the containment.** The
/// mapping is adopted while the process still has the authority to, and the
/// profile goes on before a single request is read --- so there is no request
/// this process can be asked to serve from outside its boundary.
#[cfg(target_os = "macos")]
pub fn child_main() -> ! {
    let code = match serve() {
        Ok(()) => 0,
        Err(message) => {
            // One write, not `eprintln!`: this stream is inherited and Rust's
            // stderr issues a write per format piece, which interleaves. The
            // parser worker's own `main` records what that cost once.
            let line = format!("[ocr-worker] {message}\n");
            let _ = std::io::stderr().write_all(line.as_bytes());
            1
        }
    };
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

/// Maps the buffer, puts the profile in force, and answers until stdin closes.
#[cfg(target_os = "macos")]
fn serve() -> Result<(), String> {
    use crate::ocr::{Recogniser, OCR_SANDBOX_PROFILE};
    use std::io::{BufRead, BufReader};

    // SAFETY: the parent dup2'd a live descriptor to this number before exec, and
    // nothing else in this process owns it. Read-only: the child never writes
    // pixels, and a mapping it cannot write is one it cannot be made to write.
    let pixels = unsafe { Shm::from_fd(PIXELS_FD, PIXELS_CAPACITY, false)? };

    crate::worker_child::apply_sandbox(OCR_SANDBOX_PROFILE)?;

    let engine = crate::ocr_vision::Vision;
    let id = engine.id();
    let named = Named {
        name: id.name.to_string(),
        build: id.build.clone(),
    };

    // A `BufReader` over stdin is safe here in a way it is not in the parser
    // worker: that one hands the stream on to a password prompt and then to a
    // reader thread, and a private buffer swallows the first request between
    // them. This loop is the only reader this process ever has.
    let input = BufReader::new(std::io::stdin());
    let mut out = std::io::stdout();
    for line in input.lines() {
        let line = line.map_err(|e| format!("reading a request: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let said = match serde_json::from_str::<Ask>(&line) {
            Err(e) => Said::Failed(RecogniseError::MalformedInput(format!(
                "the request did not parse: {e}"
            ))),
            Ok(ask) => answer(&engine, &named, &pixels, &ask),
        };
        let encoded =
            serde_json::to_string(&said).map_err(|e| format!("the reply will not encode: {e}"))?;
        writeln!(out, "{encoded}").map_err(|e| format!("writing a reply: {e}"))?;
        out.flush().map_err(|e| format!("flushing a reply: {e}"))?;
    }
    Ok(())
}

/// One request, against the bytes at the front of the mapping.
#[cfg(target_os = "macos")]
fn answer(engine: &crate::ocr_vision::Vision, named: &Named, pixels: &Shm, ask: &Ask) -> Said {
    use crate::ocr::Recogniser;

    let rgba = match frame_of(pixels.as_slice(), ask) {
        Ok(rgba) => rgba,
        Err(why) => return Said::Failed(why),
    };
    let px = Pixels {
        rgba,
        width: ask.width,
        height: ask.height,
        scale: ask.scale,
    };
    match engine.recognise(px, &ask.options) {
        Ok(items) => Said::Read {
            engine: named.clone(),
            items,
        },
        Err(why) => Said::Failed(why),
    }
}

/// The bytes at the front of the mapping that a request names.
///
/// **The child's own bound, and it is not the parent's.** The parent checks the
/// image it is about to copy in; this checks the numbers that arrived over a
/// pipe against the mapping actually in hand, which is a different question with
/// the same shape. A child that trusted the parent's arithmetic would be a
/// process whose only input is a pipe and whose only guard is on the other side
/// of it.
///
/// # Errors
///
/// Dimensions that do not describe a size at all, or a frame larger than the
/// mapping.
pub fn frame_of<'a>(mapping: &'a [u8], ask: &Ask) -> Result<&'a [u8], RecogniseError> {
    let Some(len) = frame_len(ask.width, ask.height) else {
        return Err(RecogniseError::MalformedInput(format!(
            "{}x{} does not describe an image of any size",
            ask.width, ask.height
        )));
    };
    if len > mapping.len() {
        return Err(RecogniseError::MalformedInput(format!(
            "{}x{} needs {len} bytes and the mapping holds {}",
            ask.width,
            ask.height,
            mapping.len()
        )));
    }
    Ok(&mapping[..len])
}

/// How many bytes an image of these dimensions occupies, or `None` if the answer
/// does not fit.
///
/// **Checked rather than computed**, because `width * height * 4` on `u32` wraps:
/// a request naming 65,536 by 65,536 comes to zero, which would then pass every
/// length comparison below it and hand the engine an empty slice described as a
/// four-gigapixel image. The request comes over a pipe, so its numbers are not
/// this process's to trust.
#[must_use]
pub fn frame_len(width: u32, height: u32) -> Option<usize> {
    let pixels = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    pixels.checked_mul(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_engine_name_resolves_to_the_static_one() {
        let named = Named {
            name: "vision".into(),
            build: "25G83".into(),
        };
        let id = named.resolve().expect("a known engine");
        assert_eq!(id.name, "vision");
        assert_eq!(id.build, "25G83");
    }

    /// A name from a different build is refused rather than carried. An identity
    /// nothing can resolve is one nothing can be invalidated against, which is
    /// the entire purpose of carrying it.
    #[test]
    fn an_unknown_engine_name_is_refused_and_named_in_the_refusal() {
        let named = Named {
            name: "tesseract".into(),
            build: "5".into(),
        };
        let why = named.resolve().expect_err("refused");
        assert!(why.contains("tesseract"), "{why}");
    }

    /// The size arithmetic, with the mistake it avoids written into the test.
    /// Done in `u32` this is **zero**, which passes every length comparison below
    /// it and hands the engine an empty slice described as four gigapixels.
    #[test]
    fn a_four_gigapixel_frame_is_a_number_too_big_rather_than_zero() {
        assert_eq!(
            65536u32.wrapping_mul(65536).wrapping_mul(4),
            0,
            "the u32 arithmetic this exists to avoid"
        );
        assert_eq!(frame_len(65536, 65536), Some(17_179_869_184));
        assert!(frame_len(65536, 65536).is_some_and(|len| len > PIXELS_CAPACITY));
    }

    #[test]
    fn an_ordinary_frame_is_four_bytes_a_pixel() {
        assert_eq!(frame_len(3, 2), Some(24));
        assert_eq!(frame_len(0, 0), Some(0));
    }

    #[test]
    fn an_ask_survives_the_wire() {
        let ask = Ask {
            width: 1200,
            height: 400,
            scale: 2.0,
            options: Options {
                languages: vec!["de-DE".into()],
                language_correction: true,
                deadline_ms: 5_000,
            },
        };
        let line = serde_json::to_string(&ask).expect("encodes");
        assert_eq!(serde_json::from_str::<Ask>(&line).expect("decodes"), ask);
    }

    #[test]
    fn both_answers_survive_the_wire() {
        let read = Said::Read {
            engine: Named {
                name: "vision".into(),
                build: "25G83".into(),
            },
            items: vec![RecognisedItem {
                text: "Beispiel".into(),
                rect: [1.0, 2.0, 3.0, 4.0],
                confidence: Some(0.9),
            }],
        };
        let failed = Said::Failed(RecogniseError::Unavailable("no engine".into()));
        for said in [read, failed] {
            let line = serde_json::to_string(&said).expect("encodes");
            assert_eq!(serde_json::from_str::<Said>(&line).expect("decodes"), said);
        }
    }

    /// The tagging property, asserted rather than trusted. `docs/TRAPS.md`
    /// records untagged silently swapping two variants of the same shape; the
    /// check that the encoding is not untagged is that a bare payload does not
    /// parse.
    #[test]
    fn a_bare_payload_is_not_a_reply() {
        let bare = r#"{"engine":{"name":"vision","build":"1"},"items":[]}"#;
        assert!(serde_json::from_str::<Said>(bare).is_err());
    }

    fn ask(width: u32, height: u32) -> Ask {
        Ask {
            width,
            height,
            scale: 1.0,
            options: Options::default(),
        }
    }

    #[test]
    fn a_frame_the_mapping_holds_is_the_front_of_it() {
        let mapping = vec![7u8; 64];
        let rgba = frame_of(&mapping, &ask(3, 2)).expect("24 of 64");
        assert_eq!(rgba.len(), 24);
        assert!(rgba.iter().all(|b| *b == 7));
    }

    /// The boundary. A frame exactly as large as the mapping is not too large,
    /// and this is the comparison most likely to be written the other way.
    #[test]
    fn a_frame_exactly_as_large_as_the_mapping_fits() {
        let mapping = vec![0u8; 24];
        assert!(frame_of(&mapping, &ask(3, 2)).is_ok());
        let smaller = vec![0u8; 23];
        assert!(frame_of(&smaller, &ask(3, 2)).is_err());
    }

    /// The child's guard is not the parent's. Both exist because the numbers
    /// arrive over a pipe, and a child that trusted the sender would have its
    /// only guard on the other side of its own boundary.
    #[test]
    fn a_frame_larger_than_the_mapping_is_refused_by_the_child() {
        let mapping = vec![0u8; 1024];
        let why = frame_of(&mapping, &ask(2048, 2048)).expect_err("refused");
        assert!(format!("{why}").contains("mapping holds"), "{why}");
    }

    #[test]
    fn an_image_that_is_not_its_own_dimensions_is_refused_before_it_is_copied() {
        let rgba = vec![0u8; 24];
        let px = Pixels {
            rgba: &rgba,
            width: 4,
            height: 2,
            scale: 1.0,
        };
        let why = room_for(&px, PIXELS_CAPACITY).expect_err("refused");
        assert!(format!("{why}").contains("is not 4x2 of RGBA"), "{why}");
    }

    #[test]
    fn an_image_larger_than_the_buffer_is_refused_before_it_is_copied() {
        let rgba = vec![0u8; 24];
        let px = Pixels {
            rgba: &rgba,
            width: 3,
            height: 2,
            scale: 1.0,
        };
        assert!(room_for(&px, 24).is_ok(), "exactly the capacity fits");
        let why = room_for(&px, 23).expect_err("refused");
        assert!(format!("{why}").contains("buffer holds 23"), "{why}");
    }

    /// Off macOS the spawn refuses rather than returning a worker that answers
    /// nothing. A caller that got one would report *no text survived*.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_platform_with_no_engine_refuses_to_spawn() {
        // Matched rather than `expect_err`, which would need `OcrWorker: Debug`
        // --- and a `Debug` on a type holding a live child is a derive that
        // exists only to satisfy a test.
        let why = match OcrWorker::spawn() {
            Err(why) => why,
            Ok(_) => panic!("a platform with no engine handed back a worker"),
        };
        assert!(why.contains("no OCR engine"), "{why}");
    }
}
