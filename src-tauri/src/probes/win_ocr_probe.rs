//! The body of `examples/win_ocr_probe.rs`. See that file for what this answers.

use std::os::windows::io::{AsRawHandle, FromRawHandle};

use tpdf_lib::ocr::{Options, Pixels, Recogniser};
use tpdf_lib::ocr_gate::MIN_CONTROL_PX;
use tpdf_lib::ocr_windows::{rgba_to_bgra_opaque, WindowsOcr};
use tpdf_lib::sandbox_win::{self, Containment, Stdio};
use windows::core::HSTRING;
use windows::Media::Ocr::OcrEngine;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
    GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HBITMAP, HGDIOBJ, OUT_TT_PRECIS, TRANSPARENT,
};

/// The bitmap the control is drawn into. Bigger than a gate band needs, so that a
/// failure to read is about the engine rather than about a control too small for
/// it --- `docs/TRAPS.md` has an entry per direction of that mistake.
const PROBE_W: i32 = 640;
const PROBE_H: i32 = 160;
/// The two character heights every string is read at.
///
/// **Two, because one would be a control easier than the check.** The first is
/// far above anything the gate has to render --- it asks whether the engine works
/// at all, and a blank reading there is a broken probe rather than a finding. The
/// second is [`MIN_CONTROL_PX`] itself, the floor the gate will not render a
/// control below, which is where its own recent measurements say the engine goes
/// silent. Borrowed from `ocr_gate` rather than copied: nothing is asserted
/// against it, so there is no check to make unfalsifiable, and a second copy of a
/// constant is how the floor and the probe of the floor drift apart.
const SIZES_PX: [i32; 2] = [44, MIN_CONTROL_PX as i32];

/// A word, and a string no dictionary holds.
///
/// The pair is the point. If both come back verbatim the engine is not
/// second-guessing what it read; if the first survives and the second arrives as
/// something else, `Windows.Media.Ocr` cannot honour
/// `ocr::Options::language_correction`, and a verdict from it means something
/// different from a verdict from Vision.
const REAL_WORD: &str = "REDACTED";
const NON_WORD: &str = "qwrtzp";

/// How this binary re-execs itself as the contained child.
const CONTAINED_ARGV: &str = "--contained-child";

/// What the child answers when it was not contained after all.
const NOT_CONTAINED_EXIT: u32 = 3;

/// One reading, printed whatever it says.
fn say(label: &str, value: &str) {
    println!("  {label:<28} {value}");
}

/// One string, at one size, and what came back.
///
/// Data rather than a printed line, because the contained child takes the same
/// readings in another process and the parent has to *compare* them. A probe that
/// printed on both sides and left the reader to diff two tables would be one
/// where a difference is noticed by whoever is paying attention.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Reading {
    px: i32,
    text: String,
    got: String,
    verdict: String,
}

impl Reading {
    fn label(&self) -> String {
        format!("{:>3} px  read {:?}", self.px, self.text)
    }

    fn value(&self) -> String {
        format!("{:?}  {}", self.got, self.verdict)
    }
}

/// Every string at every size, as data.
fn take_readings(engine: &WindowsOcr) -> Vec<Reading> {
    let mut out = Vec::new();
    for px in SIZES_PX {
        for text in [REAL_WORD, NON_WORD] {
            let (got, verdict) = match read_back(engine, text, px) {
                Ok(got) => {
                    // Three outcomes, not two. Nothing read is what the gate's
                    // own silent regions look like, and folding it into DIFFERS
                    // would report a size the engine cannot see as a correction
                    // it made.
                    let verdict = if got.trim().is_empty() {
                        "NOTHING READ"
                    } else if got.split_whitespace().any(|w| w == text) {
                        "VERBATIM"
                    } else {
                        "DIFFERS"
                    };
                    (got, verdict.to_string())
                }
                Err(why) => (String::new(), format!("failed: {why}")),
            };
            out.push(Reading {
                px,
                text: text.to_string(),
                got,
                verdict,
            });
        }
    }
    out
}

/// The installed recogniser languages, as `(tag, display name)`.
///
/// Prints nothing, and neither does [`make_engine`]. The contained child's stdout
/// **is** the answer channel, so a helper that reported by printing would corrupt
/// the reply the parent parses --- and it would do so only in the contained case,
/// which is the one nobody runs by hand.
fn languages() -> Result<Vec<(String, String)>, String> {
    let list = OcrEngine::AvailableRecognizerLanguages()
        .map_err(|e| format!("AvailableRecognizerLanguages: {e}"))?;
    Ok(list
        .into_iter()
        .map(|lang| {
            (
                lang.LanguageTag()
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                lang.DisplayName()
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            )
        })
        .collect())
}

pub fn main() {
    if std::env::args().any(|a| a == CONTAINED_ARGV) {
        contained_child();
    }

    println!("[win-ocr-probe] Windows.Media.Ocr, on this machine");

    let tags = match languages() {
        Ok(tags) => tags,
        Err(why) => {
            // Not exit 1. Failing to *ask* is a different fact from an empty
            // answer, and folding the two together is what makes an absent
            // capability indistinguishable from a broken probe.
            eprintln!("[FAIL] {why}");
            std::process::exit(2);
        }
    };
    for (tag, name) in &tags {
        say("recogniser language", &format!("{tag}  ({name})"));
    }
    say("languages installed", &tags.len().to_string());

    // THE GATING LINE. Greppable on purpose: this is the reading the ranking in
    // `docs/PLAN.md` §9.10 turns on, and it should not have to be inferred from
    // the absence of rows above.
    println!(
        "[verdict] language packs on a stock install: {}",
        if tags.is_empty() { "NONE" } else { "present" }
    );
    if tags.is_empty() {
        println!("[verdict] the in-box engine cannot ship as-is; nothing below could run");
        return;
    }

    let engine = match WindowsOcr::new() {
        Ok(engine) => {
            // The shipping engine, not a second copy of the WinRT calls. What CI
            // exercises here is `ocr_windows::WindowsOcr::recognise` itself ---
            // bitmap construction, the word walk and the coordinate conversion
            // --- rather than a probe that agrees with itself.
            say("engine", &engine.id().to_string());
            engine
        }
        Err(why) => {
            eprintln!("[FAIL] {why}");
            std::process::exit(2);
        }
    };

    // Restored 2026-08-29 after the containment rung's restructure dropped both
    // of them: they were `say` calls inline in `main`, and the extraction of
    // `languages`/`make_engine` replaced the region they sat in. The run still
    // printed a healthy-looking report and `BUILD.md` still carried the number,
    // so nothing anywhere disagreed. See the trap of that name.
    match OcrEngine::MaxImageDimension() {
        // A real bound on `ocr::Pixels`: the gate composites a probe image and
        // hands it over whole, so a limit below a page at render scale is a
        // constraint on the caller rather than a detail of the binding.
        Ok(max) => say("max image dimension", &max.to_string()),
        Err(e) => say("max image dimension", &format!("unreadable ({e})")),
    }

    let here = take_readings(&engine);
    for reading in &here {
        say(&reading.label(), &reading.value());
    }

    println!(
        "[verdict] a non-word that DIFFERS means Options::language_correction cannot be \
         honoured here; at {} px that is the gate's own floor, where a corrector bites \
         hardest",
        SIZES_PX[1]
    );

    contained_rung(&here);
}

/// Runs the same readings again in a child under the containment that ships.
///
/// **The rung the ranking actually turns on.** Everything above ran at whatever
/// integrity the shell gave it, and a real engine would run where the parser
/// worker runs. macOS answered the mirror of this with *no*: Vision is killed by
/// SIGTRAP under `SANDBOX_PROFILE` and needs general `file-read`, which is why
/// OCR is a separate process under `OCR_SANDBOX_PROFILE`. If the same is true
/// here, an in-box Windows engine needs a second containment story rather than a
/// line in the worker.
///
/// **Through `sandbox_win` rather than a ladder of its own.** `win_sandbox_probe`
/// built six rungs to find out which one PDFium survives; that question is
/// answered, and the answer is what `Containment::default()` implements. Asking
/// it again here would be a second copy of security-critical code, and the copy
/// that drifts is the one nobody ships.
fn contained_rung(control: &[Reading]) {
    match run_contained() {
        Ok(there) => {
            for reading in &there {
                say(&format!("contained {}", reading.label()), &reading.value());
            }
            // Compared as data. The interesting outcome is not "the child failed"
            // but "the child read something *different*", which is what a
            // substituted font or a denied resource looks like from here --- and
            // `docs/TRAPS.md` records a sandboxed PDFium returning `ok` while
            // silently substituting a typeface, which is the same shape.
            let same = there == control;
            println!(
                "[verdict] under the containment that ships (job + low integrity): {}",
                if same {
                    "reads IDENTICALLY to uncontained"
                } else {
                    "reads DIFFERENTLY -- compare the rows above"
                }
            );
        }
        Err(why) => {
            // Loud, and not an exit code. The uncontained readings above did
            // happen and are worth keeping; what failed is one rung.
            println!("[verdict] the contained rung could not be measured: {why}");
        }
    }
}

/// Spawns this binary contained, and reads back what it measured.
fn run_contained() -> Result<Vec<Reading>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    // Quoted by hand, and the naive rule is correct *here* where it is not in
    // general: `worker_argv::command_line` doubles a run of backslashes before
    // the closing quote, because `--lib C:\Program Files\tpdf\` would otherwise
    // escape it and swallow the next argument. This command line is an executable
    // path and a literal flag; a path ending in `.exe` has no trailing backslash
    // and `"` is not a legal filename character, so the case that rule exists for
    // is unreachable. That function is `pub(crate)` and an example is another
    // crate, which is what forces the choice rather than a preference.
    let command = format!("\"{}\" {CONTAINED_ARGV}", exe.display());

    // Two pipes as `Worker::spawn_contained_worker` builds them, even though only
    // the reply half carries anything: `Stdio` wants three valid handles, and
    // giving the child a stdin it can read to EOF is cheaper than reasoning about
    // what a worker does with an invalid one.
    let (requests_read, requests_write) = sandbox_win::pipe()?;
    let (replies_read, replies_write) = sandbox_win::pipe()?;
    // SAFETY: four fresh handles from `CreatePipe`, owned by nothing else.
    // Wrapped now rather than after the spawn, so that an error on any branch
    // added later closes them --- `File`'s drop is the cleanup that cannot be
    // forgotten.
    let (_stdin, mut stdout, child_stdin, child_stdout) = unsafe {
        (
            std::fs::File::from_raw_handle(requests_write.cast()),
            std::fs::File::from_raw_handle(replies_read.cast()),
            std::fs::File::from_raw_handle(requests_read.cast()),
            std::fs::File::from_raw_handle(replies_write.cast()),
        )
    };

    let stdio = Stdio::with_inherited_stderr(
        child_stdin.as_raw_handle().cast(),
        child_stdout.as_raw_handle().cast(),
    )?;
    let handles: [windows_sys::Win32::Foundation::HANDLE; 0] = [];
    let contained =
        sandbox_win::spawn_contained(&command, &handles, &Containment::default(), Some(&stdio))?;

    // Closed in the parent *before* the child runs, for the reason the worker
    // states: while this process holds a copy of the reply pipe's write end that
    // pipe never reaches EOF, so a child that died reads as one still thinking.
    drop(child_stdin);
    drop(child_stdout);

    contained.resume()?;

    let mut answer = String::new();
    let read = std::io::Read::read_to_string(&mut stdout, &mut answer);
    let code = contained.wait()?;

    // The exit code first, because a child that died has no answer to parse and
    // the parse error would name the wrong thing. `describe_exit` is what turns
    // an NTSTATUS into a sentence --- and macOS's lesson is exactly this: the
    // engine may abort its host rather than refuse, so dying IS a result.
    if code == NOT_CONTAINED_EXIT {
        return Err(format!(
            "the child was not contained, so it measured nothing: {}",
            answer.trim()
        ));
    }
    if code != 0 {
        return Err(format!(
            "the contained child exited {} ({}); it said {:?}",
            code,
            sandbox_win::describe_exit(code),
            answer.trim()
        ));
    }
    read.map_err(|e| format!("reading the child's answer: {e}"))?;
    serde_json::from_str(answer.trim())
        .map_err(|e| format!("parsing the child's answer {:?}: {e}", answer.trim()))
}

/// The other half: measure under containment and answer on stdout.
fn contained_child() -> ! {
    // **Before measuring, not after.** A child that quietly ran uncontained would
    // report that the engine survives containment, which is the direction that
    // costs something --- `docs/TRAPS.md` on a control that cannot fail. This is
    // a verification where macOS has an application: by the time this runs the
    // decision was taken by whoever spawned us, and all we can do is check.
    if let Err(why) = sandbox_win::assert_contained() {
        println!("{why}");
        std::process::exit(NOT_CONTAINED_EXIT as i32);
    }
    let engine = match WindowsOcr::new() {
        Ok(engine) => engine,
        Err(why) => {
            println!("no engine under containment: {why}");
            std::process::exit(2);
        }
    };
    let readings = take_readings(&engine);
    match serde_json::to_string(&readings) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            println!("serialising readings: {e}");
            std::process::exit(2);
        }
    }
    std::process::exit(0);
}

/// Draws `text` into a bitmap and asks the engine to read it.
fn read_back(engine: &WindowsOcr, text: &str, px: i32) -> Result<String, String> {
    let bgra = draw(text, px)?;

    // GDI writes BGRA and `ocr::Pixels` is documented RGBA, so the bytes are
    // swapped here and swapped back inside `WindowsOcr::recognise`. That reads as
    // waste and is the opposite: routing the probe through the *shipping*
    // conversion is what makes this an end-to-end exercise of the engine rather
    // than a second copy of the WinRT calls agreeing with itself.
    //
    // `rgba_to_bgra_opaque` is its own inverse for the channel exchange, which is
    // why it can be used in this direction --- and note what it means for this
    // probe's evidence: a *missing* swap would be invisible here, because black
    // on white is unchanged by exchanging two channels. The unit test in
    // `ocr_windows` is the instrument for that, and it has to be.
    let rgba = rgba_to_bgra_opaque(&bgra).ok_or("the drawn buffer is not whole pixels")?;

    let width = u32::try_from(PROBE_W).map_err(|_| "probe width is negative")?;
    let height = u32::try_from(PROBE_H).map_err(|_| "probe height is negative")?;
    let items = engine
        .recognise(
            Pixels {
                rgba: &rgba,
                width,
                height,
                // One pixel per point, so a rectangle comes back in the units it
                // was drawn in and a wrong `scale` would be visible rather than
                // divided away.
                scale: 1.0,
            },
            &Options::default(),
        )
        .map_err(|why| why.to_string())?;

    Ok(items
        .into_iter()
        .map(|item| item.text)
        .collect::<Vec<_>>()
        .join(" "))
}

/// Black `text` on white, centred, as top-down BGRA.
fn draw(text: &str, px: i32) -> Result<Vec<u8>, String> {
    // SAFETY: every handle created below is deleted on the way out, and the bits
    // pointer is owned by the DIB section for as long as the bitmap lives --- the
    // copy out happens before it is deleted.
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        if dc.is_invalid() {
            ReleaseDC(None, screen);
            return Err("CreateCompatibleDC".into());
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(0),
                biWidth: PROBE_W,
                // Negative: top-down, so the rows are in the order
                // `SoftwareBitmap` reads them and no flip is needed. A bottom-up
                // DIB here would give the engine an upside-down image, which
                // reads as an engine that cannot read rather than as a caller
                // that handed one over wrong.
                biHeight: -PROBE_H,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
        // Matched rather than `?`: an early return here would be inside the
        // `unsafe` block with two device contexts already open, and `?` runs no
        // cleanup. The probe is called twice, so a leak on the sad path is a leak
        // the second call inherits.
        let bitmap: HBITMAP =
            match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(h) => h,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    ReleaseDC(None, screen);
                    return Err(format!("CreateDIBSection: {e}"));
                }
            };

        let old_bitmap: HGDIOBJ = SelectObject(dc, bitmap.into());

        let len = (PROBE_W * PROBE_H * 4) as usize;
        // White, by writing it rather than by `PatBlt`: the bits are ours and
        // there is no brush to select.
        core::ptr::write_bytes(bits.cast::<u8>(), 0xFF, len);

        let font = CreateFontW(
            -px,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            // The one parameter of the five that really is a bare `u32`; the
            // other four are newtypes and take their constants unwrapped.
            u32::from(DEFAULT_PITCH.0),
            // Ships on every Windows install, so the probe does not depend on a
            // font somebody added.
            &HSTRING::from("Segoe UI"),
        );
        let old_font: HGDIOBJ = SelectObject(dc, font.into());

        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x0000_0000));
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: PROBE_W,
            bottom: PROBE_H,
        };
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
        // GDI batches, and the bits are read directly rather than through another
        // GDI call that would flush on our behalf.
        let _ = GdiFlush();

        let mut out = vec![0u8; len];
        core::ptr::copy_nonoverlapping(bits.cast::<u8>(), out.as_mut_ptr(), len);

        // Every pixel opaque, and this is not tidiness. GDI writes RGB into a
        // 32-bit DIB and leaves the alpha byte alone -- so a glyph drawn in black
        // arrives as 0x00000000, alpha included. `CreateCopyFromBuffer` takes no
        // alpha mode and `SoftwareBitmap::BitmapAlphaMode` is read-only, so the
        // buffer has to be right rather than the declaration: under
        // `Premultiplied`, which is what Bgra8 gets, every glyph pixel would be
        // fully transparent and the engine would be handed a blank image. It
        // would then report no text, honestly, and the reading would be a bug
        // wearing the shape of a finding -- `docs/TRAPS.md` on the reassuring
        // branch. At alpha 255 throughout, premultiplied and straight are the
        // same image, so no mode can be the wrong one.
        for pixel in out.chunks_exact_mut(4) {
            pixel[3] = 0xFF;
        }

        SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        Ok(out)
    }
}
