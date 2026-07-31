//! Spike 0.5: is the three-process shape of PLAN §3 actually buildable, and
//! what does the boundary cost?
//!
//! The plan asserts that parsing and rendering happen in worker processes with
//! no filesystem or network authority, under resource limits, restartable on
//! crash --- and that this is the only route to parallelism, because concurrent
//! in-process Pdfium calls are undefined behaviour (spike 0.9 later showed they
//! segfault, and that `thread_safe` does not serialize them as its README
//! claims). All of that is load-bearing and none of it had been measured. Retrofitting a
//! process boundary is a rewrite, so it is Phase 0 work.
//!
//! The binary is both halves. `worker-bench worker` is the child; every other
//! invocation is the parent, which spawns copies of itself via
//! `current_exe()` --- the same trick Chrome uses, and the one that works when
//! the executable is inside a signed `.app` bundle.
//!
//! Three design commitments are being tested, not assumed:
//!
//! * **The document reaches the worker as memory, never as a path.** The parent
//!   opens the file, maps it, and passes the *descriptor* to the child, which
//!   maps it at a fixed fd. A worker that never opens a path can be denied
//!   `file-read*` outright.
//! * **Pixels come back through shared memory, not a pipe.** Pdfium renders
//!   directly into the shared mapping via `PdfBitmap::from_bytes`, so there is
//!   no copy on the worker side at all. The pipe carries only a JSON line.
//! * **The control channel is line-delimited JSON on stdin/stdout,** so a dead
//!   worker is an EOF rather than a hang.
//!
//! Modes, each answering one question:
//!
//! * `latency`   --- what a tile costs in-process, over a pipe, and over shm
//! * `parallel`  --- does K workers give K times the throughput (§10 q5)
//! * `crash`     --- is a worker death contained, and what does recovery cost
//! * `timeout`   --- can a runaway render be cancelled by killing the worker
//! * `limits`    --- does `setrlimit` actually bound a worker on this platform
//! * `authority` --- can the worker be denied files and network and still render
//! * `footprint` --- if the kernel will not bound memory, can the parent
//! * `engine`    --- what the bound Pdfium build is capable of at all
//!
//! Usage:
//!   worker-bench <file.pdf>
//!       [--mode latency|parallel|crash|timeout|limits|authority|footprint|engine]
//!       [--rounds N] [--reps N] [--pages N] [--workers 1,2,4,8]
//!       [--page N] [--scale F] [--tile N] [--lib DIR]
//!       [--profiles 'none;targeted;worker;(version 1)...']
//!       [--budget-mb N] [--poll-ms 0,1,5,20]
//!
//! `--profiles` is semicolon-separated and takes either a built-in name or raw
//! SBPL beginning with `(`, so a policy can be bisected from the shell without a
//! rebuild. That is how `PROFILE_WORKER` below was arrived at.

use std::path::Path;

/// Runs the one mode that needs no worker, or refuses and says what is missing.
///
/// `--mode engine` reads the library file and never spawns anything, so it is the
/// only mode that has any business running here. It was unreachable off unix
/// purely because it lived inside a `#[cfg(unix)]` module, and the threat-model
/// claim it checks is the most load-bearing one in the project --- so being
/// unable to run it on a platform meant the claim was untested there rather than
/// merely unmeasured.
///
/// The refusal for every other mode names the model that was actually built.
/// It previously cited "restricted tokens" and "named section objects", both of
/// which `examples/win_sandbox_probe.rs` measured *out* of the design, and a wrong
/// reason on a refusal is worse than a vague one: it reads as a design
/// instruction, and someone building the spike from it would have built the two
/// rejected things.
#[cfg(not(unix))]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value_of = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };
    if value_of("--mode").as_deref() == Some("engine") {
        let Some(dir) = value_of("--lib") else {
            eprintln!(
                "[ERROR] --mode engine needs --lib DIR: it reads the library, it does not bind it"
            );
            std::process::exit(2);
        };
        if let Err(e) = engine_report(Path::new(&dir)) {
            eprintln!("[ERROR] {e}");
            std::process::exit(1);
        }
        return;
    }
    eprintln!(
        "{}",
        concat!(
            "[ERROR] worker-bench is a POSIX harness: its own worker, dup2 handover, ",
            "socket pair and SBPL profile bisection, none of which has a Windows ",
            "counterpart. Only --mode engine runs here, and it does.
",
            "The Windows model is a low-integrity token inside a job object, with ",
            "anonymous sections inherited by handle; see examples/win_sandbox_probe.rs for ",
            "why it is not a restricting SID.
",
            "The rest needs its own spike, not a port, and as of 2026-07-31 it has one: ",
            "latency-bench measures the per-tile overhead decomposition here, which was ",
            "the one thing this refusal named as uncovered. It drives the production ",
            "worker rather than a private one, so it builds on both platforms and this ",
            "harness can be cross-checked against it on macOS -- a run that has not happened yet.
",
            "Parallel scaling is pool-bench, the authority rungs are win-sandbox-probe, ",
            "crash and timeout are backend-probe, and limits and footprint are answered ",
            "by the job object capping commit in the kernel.",
        )
    );
    std::process::exit(2);
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// What the bound Pdfium build is *capable* of, read out of the binary.
///
/// "Document JavaScript is disabled by default" is the single most
/// load-bearing sentence in the threat model, and as a statement about
/// configuration it is weak --- a default can be changed, and a call that
/// enables it is one line. As a statement about the build it is much
/// stronger, and it is checkable: a Pdfium without V8 linked cannot run a
/// script whatever it is asked to do.
///
/// **JavaScript cannot be tested behaviourally.** A document whose script does
/// nothing looks exactly like a document whose script never ran, so the absence
/// of an effect is not evidence of the absence of an engine. The symbol table is
/// the only thing that discriminates, which is why the controls below decide
/// whether this method works on a given binary at all.
///
/// **And on Windows it does not.** Moved to file scope 2026-07-30 so it can run
/// off unix --- it needs no worker, it reads a file --- and the first Windows run
/// established that the shipped `pdfium.dll` carries **no local C++ symbols**:
/// `CPDF_Document` is absent, so `v8::` and `CXFA_` being absent means nothing.
/// The second control catches that and the verdict is `[NOT VERIFIED]`, which is
/// correct and is the point --- but it means `docs/THREAT-MODEL.md`'s promotion of
/// "JavaScript is disabled" to "there is no engine to disable" is established on
/// **macOS only**. That is recorded there rather than left implied.
///
/// The export table is reported beside the scan because it is the one dimension
/// that survives stripping. Read it as *surface*, not as a verdict: the Windows
/// DLL exports four XFA-named functions, and three of them
/// (`FPDF_GetXFAPacket{Count,Name,Content}`) read the `/XFA` streams out of an
/// AcroForm dictionary and need no XFA implementation behind them. Whether
/// `FPDF_LoadXFA` is a stub is the open question; see the note printed at the end.
pub fn engine_report(dir: &Path) -> Result<(), String> {
    let path = ["libpdfium.dylib", "pdfium.dll", "libpdfium.so"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.exists())
        .ok_or_else(|| format!("no Pdfium library in {}", dir.display()))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{} ({} bytes)", path.display(), bytes.len());
    println!();

    // Two controls, because a scan that cannot fail proves nothing. The
    // first says the file really is Pdfium; the second says its local
    // symbols survived, without which every absence below is "not
    // verified" rather than "not present" --- the same rule §6 applies to a
    // carrier a redaction verifier cannot decode.
    let is_pdfium = contains(&bytes, b"FPDF_LoadDocument");
    let has_locals = contains(&bytes, b"CPDF_Document");
    println!("  control: exported Pdfium symbols  {}", yes_no(is_pdfium));
    println!("  control: local C++ symbols kept   {}", yes_no(has_locals));
    if !is_pdfium {
        println!("\n[FAIL] this is not a Pdfium library; nothing below means anything.");
        return Ok(());
    }

    // Before the stripped-binary exit, not after. The export table is the only
    // dimension that survives stripping, so the run that *cannot* do the symbol
    // scan is exactly the run where it is worth having --- and printing it after
    // the early return meant Windows saw nothing at all.
    println!();
    exported_surface(&bytes);

    if !has_locals {
        println!(
            "\n[NOT VERIFIED] the binary is stripped of local symbols, so the absence \
             of a JavaScript engine or of XFA cannot be established this way. What is \
             above is surface, not a verdict."
        );
        println!(
            "to settle XFA on a stripped binary the test is behavioural and needs a \
             control: a document carrying an /XFA packet, where FPDF_GetXFAPacketCount > 0 \
             proves the packet reader works, so FPDF_LoadXFA returning false then means the \
             implementation is absent rather than the document empty. Not written --- that \
             fixture does not exist yet."
        );
        return Ok(());
    }

    let v8 = contains(&bytes, b"_ZN2v8") || contains(&bytes, b"v8::");
    let real_js = contains(&bytes, b"_ZN11CJS_Runtime");
    let stub_js = contains(&bytes, b"CJS_RuntimeStub");
    let xfa = contains(&bytes, b"CXFA_");
    println!();
    println!("  V8 engine linked                  {}", yes_no(v8));
    println!("  CJS_Runtime (real JS bridge)      {}", yes_no(real_js));
    println!("  CJS_RuntimeStub (no-op bridge)    {}", yes_no(stub_js));
    println!("  XFA implementation (CXFA_*)       {}", yes_no(xfa));
    println!();

    let js_absent = !v8 && !real_js && stub_js;
    println!(
        "document JavaScript: {}",
        if js_absent {
            "[OK] cannot run --- there is no engine to run it, only the stub"
        } else if v8 || real_js {
            "[FAIL] AN ENGINE IS PRESENT; policy is now the only thing stopping it"
        } else {
            "[NOT VERIFIED] neither an engine nor the stub was found"
        }
    );
    println!(
        "XFA forms:           {}",
        if xfa {
            "[FAIL] PRESENT; §6's XFA refusal is a policy, not a property"
        } else {
            "[OK] not built in"
        }
    );
    println!();
    println!(
        "note this is a property of the vendored build, not of Pdfium, so it has to be \
         re-checked after every bump --- and it says nothing about the code paths that \
         ARE present. `FPDFDOC_InitFormFillEnvironment` runs on every document open in \
         pdfium-render, so the form-fill machinery is reachable surface even with no \
         engine behind it."
    );
    Ok(())
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

/// The library's exported names, and the XFA-named ones among them.
///
/// The one dimension that survives stripping: exports are always named, because
/// a loader has to find them. So where the symbol scan above goes
/// `[NOT VERIFIED]` this still says something --- just something narrower.
///
/// **A count, not only matches.** `AGENTS.md` records a probe that reported
/// "found nothing" when it had in fact enumerated nothing, which is the same
/// answer for opposite reasons. If the table cannot be parsed, that is said.
///
/// Windows-shaped, and deliberately not generalised: a Mach-O export trie needs
/// different code, and macOS has local symbols so this dimension buys nothing
/// there. Off Windows it says so rather than printing a zero.
fn exported_surface(bytes: &[u8]) {
    let Some(names) = pe_exports(bytes) else {
        println!("  exported names: not a PE image, so the export table is not read here");
        return;
    };
    let xfa: Vec<&String> = names.iter().filter(|n| n.contains("XFA")).collect();
    println!("  exported functions                {}", names.len());
    println!("  ... XFA-named among them          {}", xfa.len());
    for name in &xfa {
        println!("      {name}");
    }
    if !xfa.is_empty() {
        println!(
            "  (surface, not a verdict: FPDF_GetXFAPacket* read /XFA streams out of an \
             AcroForm dict and need no XFA engine behind them)"
        );
    }
}

/// Every name in a PE image's export directory, or `None` if it is not one.
///
/// Hand-rolled rather than a crate, for the reason every dependency here is
/// weighed: this reads ~40 bytes of headers and walks one array, and a PE parser
/// is a licence and a supply-chain surface for that. It refuses rather than
/// guesses on anything it does not recognise --- `None` means "not read", and the
/// caller prints that instead of a count.
fn pe_exports(bytes: &[u8]) -> Option<Vec<String>> {
    /// Reads a little-endian `u32` at `at`, if it is in bounds.
    fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// Reads a little-endian `u16` at `at`, if it is in bounds.
    fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(bytes, 0x3C)? as usize;
    if bytes.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = u16_at(bytes, coff + 2)? as usize;
    let opt_size = u16_at(bytes, coff + 16)? as usize;
    let opt = coff + 20;
    // The data-directory offset is the only thing that differs between PE32 and
    // PE32+, and getting it wrong reads a neighbouring field as an address.
    let dirs = match u16_at(bytes, opt)? {
        0x20b => opt + 112,
        0x10b => opt + 96,
        _ => return None,
    };
    let export_rva = u32_at(bytes, dirs)?;
    if export_rva == 0 {
        return Some(Vec::new());
    }

    // Section table, so an RVA can be turned into a file offset. A directory
    // address is a virtual address; reading it as a file offset lands in
    // unrelated bytes and yields plausible garbage.
    let table = opt + opt_size;
    let mut spans = Vec::with_capacity(sections);
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(bytes, entry + 8)? as usize;
        let virtual_address = u32_at(bytes, entry + 12)? as usize;
        let raw_size = u32_at(bytes, entry + 16)? as usize;
        let raw = u32_at(bytes, entry + 20)? as usize;
        spans.push((virtual_address, virtual_size.max(raw_size), raw));
    }
    let offset_of = |rva: usize| -> Option<usize> {
        spans
            .iter()
            .find(|(va, size, _)| rva >= *va && rva < va.saturating_add(*size))
            .map(|(va, _, raw)| raw + (rva - va))
    };

    let directory = offset_of(export_rva as usize)?;
    let count = u32_at(bytes, directory + 24)? as usize;
    let names_rva = u32_at(bytes, directory + 32)? as usize;
    let names = offset_of(names_rva)?;
    let mut found = Vec::with_capacity(count);
    for index in 0..count {
        let rva = u32_at(bytes, names + index * 4)? as usize;
        let at = offset_of(rva)?;
        let end = bytes.get(at..)?.iter().position(|b| *b == 0)? + at;
        found.push(String::from_utf8_lossy(&bytes[at..end]).into_owned());
    }
    Some(found)
}

#[cfg(unix)]
fn main() {
    imp::run();
}

#[cfg(unix)]
mod imp {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::path::PathBuf;
    use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use pdfium_render::prelude::*;
    use serde::{Deserialize, Serialize};

    /// Where the worker finds the mapped document.
    const DOC_FD: RawFd = 3;
    /// Where the worker finds the tile buffer it renders into.
    const TILE_FD: RawFd = 4;

    /// Largest tile the shared buffer can hold. 2048x2048 BGRA, which §4 found
    /// to be the upper end of the useful tile range.
    const TILE_CAPACITY: usize = 2048 * 2048 * 4;

    // ---------------------------------------------------------------- protocol

    /// A request from parent to worker, one JSON object per line on stdin.
    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "op", rename_all = "kebab-case")]
    enum Request {
        /// Parse the mapped document. Never takes a path.
        Open,
        /// Render one tile.
        Tile {
            page: u16,
            scale: f32,
            x: i32,
            y: i32,
            width: u16,
            height: u16,
            /// `false` sends the pixels back down the pipe as well, for the
            /// transport comparison.
            shm_only: bool,
        },
        /// Report what authority this process still holds.
        Probe,
        /// Die on purpose, so supervision can be measured.
        Crash { how: String },
        /// Allocate without bound, so a memory budget can be measured.
        ///
        /// Stands in for a decompression bomb or a pathological page: the
        /// worker is not misbehaving, it is doing exactly what the document
        /// asked for, and nothing inside it knows when to stop.
        Balloon {
            /// How much to take per allocation.
            chunk_kb: usize,
            /// A ceiling so a supervision bug cannot push the machine into
            /// swap. Reaching it means the parent failed.
            cap_mb: usize,
        },
        /// Answer immediately; used to time a warm round trip with no work in it.
        Ping,
    }

    /// A reply from worker to parent, one JSON object per line on stdout,
    /// optionally followed by exactly `bytes` raw bytes.
    #[derive(Serialize, Deserialize, Debug, Default)]
    struct Response {
        ok: bool,
        #[serde(default)]
        error: String,
        /// Raw bytes following this line on stdout. Zero for the shm path.
        #[serde(default)]
        bytes: usize,
        /// Time inside Pdfium.
        #[serde(default)]
        render_us: u64,
        /// Time converting Pdfium's BGRA to the RGBA a canvas wants.
        #[serde(default)]
        swizzle_us: u64,
        #[serde(default)]
        note: String,
    }

    impl Response {
        fn err(message: impl Into<String>) -> Self {
            Self {
                ok: false,
                error: message.into(),
                ..Default::default()
            }
        }
    }

    // --------------------------------------------------------- shared memory

    /// A shared anonymous mapping, created in the parent and inherited by fd.
    ///
    /// Deliberately not `shm_open`: a POSIX shm object lives in a global name
    /// space, so a second process can find it by guessing the name, and the
    /// worker would need that name space to remain reachable under a sandbox. A
    /// temp file unlinked immediately after creation has neither problem --- the
    /// descriptor is the only handle that exists, and a descriptor survives a
    /// policy that denies opening files.
    struct Shm {
        file: std::fs::File,
        ptr: *mut libc::c_void,
        len: usize,
    }

    // The pointer is an ordinary mapping; moving it between threads is fine.
    // Aliasing is disciplined by the protocol: exactly one side writes a given
    // buffer at a time, and the parent only reads after the worker's reply.
    unsafe impl Send for Shm {}

    impl Shm {
        /// Creates a mapping of `len` bytes backed by an unlinked temp file.
        fn create(len: usize) -> Result<Self, String> {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let mut path = std::env::temp_dir();
            path.push(format!(
                "tpdf-shm-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));

            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("shm create failed: {e}"))?;
            std::fs::remove_file(&path).map_err(|e| format!("shm unlink failed: {e}"))?;
            file.set_len(len as u64)
                .map_err(|e| format!("shm resize failed: {e}"))?;

            Self::map(file, len)
        }

        /// Adopts a descriptor the parent passed in, and maps it.
        fn from_fd(fd: RawFd, len: usize) -> Result<Self, String> {
            // SAFETY: the parent dup2'd a real descriptor to this number before
            // exec, and nothing else in this process owns it.
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            Self::map(file, len)
        }

        fn map(file: std::fs::File, len: usize) -> Result<Self, String> {
            // SAFETY: len is non-zero and the descriptor is a regular file of at
            // least that size.
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    file.as_raw_fd(),
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err(format!(
                    "mmap of {len} bytes failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(Self { file, ptr, len })
        }

        fn as_slice(&self) -> &[u8] {
            // SAFETY: the mapping is valid for `len` bytes for as long as `self`.
            unsafe { std::slice::from_raw_parts(self.ptr as *const u8, self.len) }
        }

        fn as_mut_slice(&mut self) -> &mut [u8] {
            // SAFETY: as above, and `&mut self` excludes concurrent readers here.
            unsafe { std::slice::from_raw_parts_mut(self.ptr as *mut u8, self.len) }
        }

        /// Reborrows the mapping for the process lifetime.
        ///
        /// Only sound because the worker leaks its `Shm`, which it does: Pdfium
        /// holds the document bytes for as long as the document is open, and the
        /// worker's document is open until it exits.
        fn as_static(&self) -> &'static [u8] {
            // SAFETY: caller guarantees the `Shm` is never dropped.
            unsafe { std::slice::from_raw_parts(self.ptr as *const u8, self.len) }
        }
    }

    impl Drop for Shm {
        fn drop(&mut self) {
            // SAFETY: unmapping exactly what was mapped.
            unsafe { libc::munmap(self.ptr, self.len) };
        }
    }

    // --------------------------------------------------------------- pdfium

    /// Binds Pdfium, from `--lib` or from the conventional bundle location.
    fn bind(library_dir: &Option<PathBuf>) -> Result<&'static Pdfium, String> {
        let dir = library_dir.clone().unwrap_or_else(default_library_dir);
        let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
        let bindings = Pdfium::bind_to_library(&path)
            .map_err(|e| format!("could not load Pdfium from {}: {e}", path.display()))?;
        Ok(Box::leak(Box::new(Pdfium::new(bindings))))
    }

    fn default_library_dir() -> PathBuf {
        // The dylib sits next to the built binaries.
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// What to render: one page, one scale, one window onto it.
    #[derive(Clone, Copy)]
    struct TileSpec {
        page: u16,
        scale: f32,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
    }

    /// Renders one tile into `buffer` as RGBA, returning (render, swizzle) times.
    ///
    /// The buffer is Pdfium's own render target --- `PdfBitmap::from_bytes` hands
    /// it the pointer --- so when the buffer is a shared mapping there is no copy
    /// on this side of the boundary at all. Pdfium writes BGRA; the swizzle to
    /// RGBA is timed separately because it is a full pass over the tile and the
    /// convenience method `as_rgba_bytes()` hides it inside a second allocation.
    fn render_into(
        doc: &PdfDocument<'_>,
        spec: TileSpec,
        buffer: &mut [u8],
    ) -> Result<(u64, u64), String> {
        let TileSpec {
            page,
            scale,
            x,
            y,
            width,
            height,
        } = spec;
        let page_ref = doc
            .pages()
            .get(page as PdfPageIndex)
            .map_err(|e| format!("no such page {page}: {e}"))?;

        let full_width = (page_ref.width().value * scale).round() as i32;
        let full_height = (page_ref.height().value * scale).round() as i32;
        let needed = width as usize * height as usize * 4;
        let capacity = buffer.len();
        let target = buffer
            .get_mut(..needed)
            .ok_or_else(|| format!("tile buffer holds {capacity} bytes, need {needed}"))?;

        let mut bitmap = PdfBitmap::from_bytes(
            width as Pixels,
            height as Pixels,
            PdfBitmapFormat::BGRA,
            target,
        )
        .map_err(|e| format!("could not wrap tile buffer: {e}"))?;

        let config = PdfRenderConfig::new()
            .set_target_width(full_width)
            .set_target_height(full_height)
            .set_origin(-x, -y);

        let t0 = Instant::now();
        page_ref
            .render_into_bitmap_with_config(&mut bitmap, &config)
            .map_err(|e| format!("render failed: {e}"))?;
        let render_us = t0.elapsed().as_micros() as u64;
        drop(bitmap);

        let t1 = Instant::now();
        swizzle_bgra_to_rgba(&mut buffer[..needed]);
        let swizzle_us = t1.elapsed().as_micros() as u64;

        Ok((render_us, swizzle_us))
    }

    /// Swaps the red and blue channels in place.
    fn swizzle_bgra_to_rgba(pixels: &mut [u8]) {
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    }

    /// Fraction of pixels that are not pure white, i.e. how much ink is on the
    /// tile.
    ///
    /// Distinguishes the two ways a font lookup can fail: a page that renders
    /// with a substituted face still has roughly the original amount of ink on
    /// it, while a page that lost its glyphs entirely has none.
    fn ink_fraction(pixels: &[u8]) -> f64 {
        let mut inked = 0usize;
        let total = pixels.len() / 4;
        for px in pixels.chunks_exact(4) {
            if px[0] != 0xFF || px[1] != 0xFF || px[2] != 0xFF {
                inked += 1;
            }
        }
        inked as f64 / total.max(1) as f64
    }

    /// Folds a tile so the parent demonstrably touches every delivered byte.
    ///
    /// Every variant pays this, so it cancels out of the comparison --- but
    /// leaving it out would let the shm variant "win" by never reading the
    /// pixels it claims to have received.
    fn checksum(bytes: &[u8]) -> u64 {
        let mut acc = 0u64;
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            acc = acc.wrapping_add(u64::from_le_bytes(word));
        }
        acc
    }

    // --------------------------------------------------------------- sandbox

    #[cfg(target_os = "macos")]
    extern "C" {
        fn sandbox_init(
            profile: *const libc::c_char,
            flags: u64,
            errorbuf: *mut *mut libc::c_char,
        ) -> libc::c_int;
    }

    /// A profile that denies everything not explicitly allowed.
    ///
    /// This is what a production worker should run under. Whether Pdfium
    /// survives it is exactly what the `authority` mode measures.
    const PROFILE_STRICT: &str = "(version 1)\n\
         (deny default)\n\
         (allow sysctl-read)\n\
         (allow mach-lookup)\n\
         (allow signal (target self))\n";

    /// A profile that keeps the process otherwise unrestricted but removes the
    /// two authorities PLAN §3 actually names.
    const PROFILE_TARGETED: &str = "(version 1)\n\
         (allow default)\n\
         (deny file-read* file-write*)\n\
         (deny network*)\n";

    /// `targeted` with the one hole a render worker actually needs.
    ///
    /// Arrived at by bisection, not by guessing, because the obvious version of
    /// it is wrong. Denying `file-read*` and allowing `file-read*` back on the
    /// font directories still renders a base-14 page differently --- the
    /// substitution is driven by *metadata* reads across the filesystem, so
    /// clawing back read access to the font directories alone does not restore
    /// it. Allowing `file-read-metadata` globally and `file-read-data` only
    /// under the font directories does, on every fixture.
    ///
    /// The residual is that a hostile document could still learn which paths
    /// exist. It cannot read one, write one, or open a socket.
    const PROFILE_WORKER: &str = "(version 1)\n\
         (allow default)\n\
         (deny network*)\n\
         (deny file-write*)\n\
         (deny file-read*)\n\
         (allow file-read-metadata)\n\
         (allow file-read-data\n\
           (subpath \"/System/Library/Fonts\")\n\
           (subpath \"/Library/Fonts\"))\n";

    /// Applies a seatbelt profile to this process, irrevocably.
    #[cfg(target_os = "macos")]
    fn apply_sandbox(profile: &str) -> Result<(), String> {
        let c_profile = std::ffi::CString::new(profile).map_err(|e| e.to_string())?;
        let mut error: *mut libc::c_char = std::ptr::null_mut();
        // SAFETY: a valid NUL-terminated profile and a place to put the error.
        let rc = unsafe { sandbox_init(c_profile.as_ptr(), 0, &mut error) };
        if rc == 0 {
            return Ok(());
        }
        // SAFETY: on failure sandbox_init sets `error` to a NUL-terminated string.
        let message = unsafe {
            if error.is_null() {
                "unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(error)
                    .to_string_lossy()
                    .into_owned()
            }
        };
        Err(message)
    }

    #[cfg(not(target_os = "macos"))]
    fn apply_sandbox(_profile: &str) -> Result<(), String> {
        Err("no seatbelt on this platform; Linux would use seccomp-bpf + namespaces".into())
    }

    /// Sets one limit and says what the kernel made of it.
    fn set_one_rlimit(resource: libc::c_int, label: &str, value: u64) -> String {
        let limit = libc::rlimit {
            rlim_cur: value as libc::rlim_t,
            rlim_max: value as libc::rlim_t,
        };
        // SAFETY: a well-formed rlimit for a valid resource number.
        let rc = unsafe { libc::setrlimit(resource, &limit) };
        if rc != 0 {
            return format!("{label}: REFUSED ({})", std::io::Error::last_os_error());
        }
        // Read it back. A kernel may accept a call and store something else,
        // and a limit that is not what was asked for is not a bound.
        let mut read_back: libc::rlimit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: a valid out pointer for a valid resource number.
        unsafe { libc::getrlimit(resource, &mut read_back) };
        if read_back.rlim_cur == value {
            format!("{label}: accepted")
        } else {
            format!("{label}: accepted but reads back as {}", read_back.rlim_cur)
        }
    }

    /// A process's physical footprint in bytes, or `None` if it is gone.
    ///
    /// `ri_phys_footprint` is the number Activity Monitor shows as Memory and
    /// the one jetsam bounds: dirty plus compressed pages, excluding clean
    /// file-backed mappings. That last exclusion is why it, and not resident
    /// size, is the number a budget should be written in --- a worker with a
    /// 300 MB document mapped is not using 300 MB of anything scarce, and a
    /// bound on RSS would kill it for reading its own input.
    ///
    /// `RUSAGE_INFO_V0` is the oldest flavour carrying the field, so it is the
    /// one least likely to shift under a macOS update.
    #[cfg(target_os = "macos")]
    fn phys_footprint(pid: u32) -> Option<u64> {
        // SAFETY: every field is an integer, so an all-zero value is valid.
        let mut info: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
        // `rusage_info_t` is itself `void *`, so the declared `*mut
        // rusage_info_t` reads as a pointer to a pointer and is not one --- the
        // struct's own address goes here, exactly as every caller in the SDK
        // writes it. Passing the address of a pointer instead type-checks
        // cleanly, returns 0, and has the kernel write the whole struct over
        // whatever follows that pointer on the stack. It did, and the only
        // symptom was a footprint that read as zero.
        let rc = unsafe {
            libc::proc_pid_rusage(
                pid as libc::c_int,
                libc::RUSAGE_INFO_V0,
                std::ptr::addr_of_mut!(info).cast::<libc::rusage_info_t>(),
            )
        };
        (rc == 0).then_some(info.ri_phys_footprint)
    }

    #[cfg(not(target_os = "macos"))]
    fn phys_footprint(_pid: u32) -> Option<u64> {
        None
    }

    /// Applies memory and CPU-time limits to this process.
    ///
    /// `RLIMIT_AS` and `RLIMIT_DATA` are both attempted, because they are not
    /// the same bound and not every kernel honours both --- and a limit the
    /// kernel silently ignores is worse than none, since it reads in the source
    /// as a bound that exists.
    fn apply_rlimits(mem_mb: Option<u64>, cpu_s: Option<u64>, nofile: Option<u64>) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(mb) = mem_mb {
            let bytes = mb * 1024 * 1024;
            notes.push(set_one_rlimit(
                libc::RLIMIT_AS,
                &format!("RLIMIT_AS {mb} MB"),
                bytes,
            ));
            notes.push(set_one_rlimit(
                libc::RLIMIT_DATA,
                &format!("RLIMIT_DATA {mb} MB"),
                bytes,
            ));
        }
        if let Some(secs) = cpu_s {
            notes.push(set_one_rlimit(
                libc::RLIMIT_CPU,
                &format!("RLIMIT_CPU {secs}s"),
                secs,
            ));
        }
        if let Some(count) = nofile {
            notes.push(set_one_rlimit(
                libc::RLIMIT_NOFILE,
                &format!("RLIMIT_NOFILE {count}"),
                count,
            ));
            // A worker that may not create files cannot be talked into writing
            // one by a crafted document, whatever else it is talked into.
            notes.push(set_one_rlimit(libc::RLIMIT_FSIZE, "RLIMIT_FSIZE 0", 0));
        }
        notes
    }

    // ---------------------------------------------------------------- worker

    /// The child half: map, bind, restrict, then serve requests until stdin ends.
    fn worker_main(args: Vec<String>) -> ! {
        let mut doc_len = 0usize;
        let mut library_dir = None;
        let mut sandbox = None;
        let mut rlimit_as = None;
        let mut rlimit_cpu = None;
        let mut rlimit_nofile = None;

        let mut it = args.into_iter();
        while let Some(flag) = it.next() {
            let value = it.next().unwrap_or_default();
            match flag.as_str() {
                "--doc-len" => doc_len = value.parse().unwrap_or(0),
                "--lib" => library_dir = Some(PathBuf::from(value)),
                "--sandbox" => sandbox = Some(value),
                "--rlimit-as" => rlimit_as = value.parse().ok(),
                "--rlimit-cpu" => rlimit_cpu = value.parse().ok(),
                "--rlimit-nofile" => rlimit_nofile = value.parse().ok(),
                other => {
                    eprintln!("[worker] unknown flag {other}");
                    std::process::exit(2);
                }
            }
        }

        // Order matters and is the whole point: map the shared buffers and bind
        // Pdfium *first*, because both need authority the worker is about to
        // give up. Everything after this line runs with no way to open a file.
        let doc_shm = match Shm::from_fd(DOC_FD, doc_len) {
            Ok(s) => Box::leak(Box::new(s)),
            Err(e) => {
                eprintln!("[worker] {e}");
                std::process::exit(3);
            }
        };
        let mut tile_shm = match Shm::from_fd(TILE_FD, TILE_CAPACITY) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[worker] {e}");
                std::process::exit(3);
            }
        };
        let pdfium = match bind(&library_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[worker] {e}");
                std::process::exit(3);
            }
        };

        let mut startup_notes = apply_rlimits(rlimit_as, rlimit_cpu, rlimit_nofile);
        if let Some(profile) = &sandbox {
            // A value starting with "(" is raw SBPL, so a profile can be
            // bisected from the shell without a rebuild.
            let text = match profile.as_str() {
                raw if raw.starts_with('(') => raw,
                "strict" => PROFILE_STRICT,
                "targeted" => PROFILE_TARGETED,
                "worker" => PROFILE_WORKER,
                other => {
                    eprintln!("[worker] unknown sandbox profile {other}");
                    std::process::exit(2);
                }
            };
            startup_notes.push(match apply_sandbox(text) {
                Ok(()) => format!("sandbox {profile}: applied"),
                Err(e) => format!("sandbox {profile}: FAILED ({e})"),
            });
        }

        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();
        let mut document: Option<PdfDocument<'static>> = None;
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => std::process::exit(0),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[worker] stdin: {e}");
                    std::process::exit(4);
                }
            }
            let request: Request = match serde_json::from_str(line.trim()) {
                Ok(r) => r,
                Err(e) => {
                    reply(
                        &mut writer,
                        &Response::err(format!("bad request: {e}")),
                        &[],
                    );
                    continue;
                }
            };

            let mut payload: &[u8] = &[];
            let response = match request {
                Request::Ping => Response {
                    ok: true,
                    note: std::mem::take(&mut startup_notes).join("; "),
                    ..Default::default()
                },
                Request::Open => {
                    let t0 = Instant::now();
                    match pdfium.load_pdf_from_byte_slice(doc_shm.as_static(), None) {
                        Ok(doc) => {
                            let pages = doc.pages().len();
                            document = Some(doc);
                            Response {
                                ok: true,
                                render_us: t0.elapsed().as_micros() as u64,
                                note: format!("{pages} pages, from shared memory"),
                                ..Default::default()
                            }
                        }
                        Err(e) => Response::err(format!("open failed: {e}")),
                    }
                }
                Request::Tile {
                    page,
                    scale,
                    x,
                    y,
                    width,
                    height,
                    shm_only,
                } => match &document {
                    None => Response::err("no document open"),
                    Some(doc) => {
                        let spec = TileSpec {
                            page,
                            scale,
                            x,
                            y,
                            width,
                            height,
                        };
                        match render_into(doc, spec, tile_shm.as_mut_slice()) {
                            Ok((render_us, swizzle_us)) => {
                                let n = width as usize * height as usize * 4;
                                if !shm_only {
                                    payload = &tile_shm.as_slice()[..n];
                                }
                                Response {
                                    ok: true,
                                    bytes: payload.len(),
                                    render_us,
                                    swizzle_us,
                                    ..Default::default()
                                }
                            }
                            Err(e) => Response::err(e),
                        }
                    }
                },
                Request::Probe => Response {
                    ok: true,
                    note: probe_authority(),
                    ..Default::default()
                },
                Request::Crash { how } => {
                    // Reply first, so the parent's failure is an EOF on the next
                    // request rather than on this one. That is the realistic
                    // case: a worker dies partway through a queue.
                    reply(
                        &mut writer,
                        &Response {
                            ok: true,
                            ..Default::default()
                        },
                        &[],
                    );
                    match how.as_str() {
                        "abort" => std::process::abort(),
                        "segv" => {
                            // The address goes through `black_box` on purpose. A
                            // plain `null_mut().write()` is UB the optimizer is
                            // entitled to delete, and it does: the first version
                            // of this arm compiled away and the process exited
                            // normally, so the harness "proved" containment of a
                            // crash that never happened.
                            let wild = std::hint::black_box(0usize) as *mut u8;
                            // SAFETY: none. That is the point.
                            unsafe { wild.write_volatile(1) };
                            std::process::exit(10)
                        }
                        _ => std::process::exit(9),
                    }
                }
                Request::Balloon { chunk_kb, cap_mb } => {
                    // Acknowledge first. The parent has to be free to poll while
                    // this runs, and a worker that is busy taking memory is not
                    // going to answer anything again.
                    reply(
                        &mut writer,
                        &Response {
                            ok: true,
                            ..Default::default()
                        },
                        &[],
                    );
                    let chunk = chunk_kb.max(1) * 1024;
                    let cap = cap_mb * 1024 * 1024;
                    let mut held: Vec<Vec<u8>> = Vec::new();
                    let mut taken = 0usize;
                    while taken < cap {
                        let mut block = vec![0u8; chunk];
                        // Touch every page. An allocation nothing has written to
                        // is not resident and does not count against the
                        // footprint, so a balloon that only reserved address
                        // space would be invisible to the very number being
                        // tested --- which is the honest shape of the threat
                        // anyway: bombs materialise their bytes.
                        let mut offset = 0;
                        while offset < block.len() {
                            // SAFETY: `offset` is within the block.
                            unsafe { block.as_mut_ptr().add(offset).write_volatile(1) };
                            offset += 4096;
                        }
                        taken += block.len();
                        held.push(block);
                    }
                    eprintln!("[worker] balloon hit its {cap_mb} MB cap unnoticed");
                    std::process::exit(BALLOON_CAPPED);
                }
            };
            reply(&mut writer, &response, payload);
        }
    }

    /// Writes one response line plus its optional raw payload.
    fn reply(out: &mut impl Write, response: &Response, payload: &[u8]) {
        let mut line = serde_json::to_string(response).unwrap_or_else(|e| {
            format!("{{\"ok\":false,\"error\":\"unserializable: {e}\",\"bytes\":0}}")
        });
        line.push('\n');
        let _ = out.write_all(line.as_bytes());
        if !payload.is_empty() {
            let _ = out.write_all(payload);
        }
        let _ = out.flush();
    }

    /// Tries each authority the worker is supposed to have lost, and reports.
    fn probe_authority() -> String {
        let mut results = Vec::new();

        results.push(match std::fs::read("/etc/hosts") {
            Ok(b) => format!("read /etc/hosts: ALLOWED ({} bytes)", b.len()),
            Err(e) => format!("read /etc/hosts: denied ({})", e.kind()),
        });

        let scratch = std::env::temp_dir().join(format!("tpdf-probe-{}", std::process::id()));
        results.push(match std::fs::write(&scratch, b"x") {
            Ok(()) => {
                let _ = std::fs::remove_file(&scratch);
                "write temp file: ALLOWED".to_string()
            }
            Err(e) => format!("write temp file: denied ({})", e.kind()),
        });

        results.push(match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(_) => "bind tcp socket: ALLOWED".to_string(),
            Err(e) => format!("bind tcp socket: denied ({})", e.kind()),
        });

        results.push(match std::net::UdpSocket::bind("127.0.0.1:0") {
            Ok(_) => "bind udp socket: ALLOWED".to_string(),
            Err(e) => format!("bind udp socket: denied ({})", e.kind()),
        });

        results.join("; ")
    }

    // ---------------------------------------------------------------- parent

    /// A supervised worker: the child, its two pipes, and its tile mapping.
    struct Worker {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        tile: Shm,
    }

    /// Everything needed to respawn a worker identically.
    #[derive(Clone)]
    struct Spawn {
        doc_fd: RawFd,
        doc_len: usize,
        library_dir: Option<PathBuf>,
        sandbox: Option<String>,
        rlimit_as: Option<u64>,
        rlimit_cpu: Option<u64>,
        rlimit_nofile: Option<u64>,
        /// Inherit the child's stderr, so a sandbox denial or a signal is visible.
        show_stderr: bool,
    }

    impl Worker {
        fn spawn(spec: &Spawn) -> Result<Self, String> {
            let tile = Shm::create(TILE_CAPACITY)?;
            let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;

            let mut cmd = Command::new(exe);
            cmd.arg("worker")
                .arg("--doc-len")
                .arg(spec.doc_len.to_string());
            if let Some(dir) = &spec.library_dir {
                cmd.arg("--lib").arg(dir);
            }
            if let Some(profile) = &spec.sandbox {
                cmd.arg("--sandbox").arg(profile);
            }
            if let Some(mb) = spec.rlimit_as {
                cmd.arg("--rlimit-as").arg(mb.to_string());
            }
            if let Some(secs) = spec.rlimit_cpu {
                cmd.arg("--rlimit-cpu").arg(secs.to_string());
            }
            if let Some(count) = spec.rlimit_nofile {
                cmd.arg("--rlimit-nofile").arg(count.to_string());
            }
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(if spec.show_stderr {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                });

            let doc_fd = spec.doc_fd;
            let tile_fd = tile.file.as_raw_fd();
            // SAFETY: only dup/dup2/close run between fork and exec, all of which
            // are async-signal-safe. Both sources are dup'd to fresh descriptors
            // first, because either may already occupy the target number --- the
            // parent's own mapping files typically land on fd 3 and 4.
            unsafe {
                cmd.pre_exec(move || {
                    let d = libc::dup(doc_fd);
                    let t = libc::dup(tile_fd);
                    if d < 0 || t < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::dup2(d, DOC_FD) < 0 || libc::dup2(t, TILE_FD) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if d != DOC_FD {
                        libc::close(d);
                    }
                    if t != TILE_FD {
                        libc::close(t);
                    }
                    Ok(())
                });
            }

            let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
            let stdin = child.stdin.take().ok_or("no stdin")?;
            let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
            Ok(Self {
                child,
                stdin,
                stdout,
                tile,
            })
        }

        /// Sends a request and reads the reply. `Err` means the worker is gone.
        fn call(&mut self, request: &Request) -> Result<Response, String> {
            let mut line = serde_json::to_string(request).map_err(|e| e.to_string())?;
            line.push('\n');
            self.stdin
                .write_all(line.as_bytes())
                .and_then(|()| self.stdin.flush())
                .map_err(|e| format!("worker stdin closed: {e}"))?;

            let mut reply = String::new();
            let n = self
                .stdout
                .read_line(&mut reply)
                .map_err(|e| format!("worker stdout: {e}"))?;
            if n == 0 {
                return Err("worker closed its output".into());
            }
            serde_json::from_str(&reply).map_err(|e| format!("bad reply {reply:?}: {e}"))
        }

        /// Reads a pipe-transport payload into `buf`.
        fn read_payload(&mut self, buf: &mut Vec<u8>, len: usize) -> Result<(), String> {
            buf.resize(len, 0);
            self.stdout
                .read_exact(buf)
                .map_err(|e| format!("payload read failed: {e}"))
        }

        /// Blocks until the worker has something to say, or the deadline passes.
        fn wait_readable(&self, timeout: Duration) -> bool {
            let mut fds = libc::pollfd {
                fd: self.stdout.get_ref().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd.
            let rc = unsafe { libc::poll(&mut fds, 1, timeout.as_millis() as libc::c_int) };
            rc > 0
        }

        /// How the child died, in words.
        fn epitaph(&mut self) -> String {
            match self.child.wait() {
                Ok(status) => match status.signal() {
                    Some(sig) => format!("killed by signal {sig}"),
                    None => format!("exited with code {}", status.code().unwrap_or(-1)),
                },
                Err(e) => format!("wait failed: {e}"),
            }
        }
    }

    // ------------------------------------------------------------------ args

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Latency,
        Parallel,
        Crash,
        Timeout,
        Limits,
        Authority,
        Footprint,
        Engine,
    }

    struct Args {
        file: PathBuf,
        mode: Mode,
        rounds: usize,
        reps: usize,
        pages: usize,
        workers: Vec<usize>,
        page: u16,
        scale: f32,
        tile: u16,
        /// Tiles across, when the work list walks one page instead of many.
        grid: Option<usize>,
        library_dir: Option<PathBuf>,
        /// Sandbox profiles to compare in `authority` mode. A name, raw SBPL, or
        /// `none`.
        profiles: Vec<String>,
        /// The footprint a worker may reach before the parent kills it.
        budget_mb: u64,
        /// Polling intervals to compare in `footprint` mode, in milliseconds.
        poll_ms: Vec<u64>,
    }

    impl Args {
        /// The `index`-th unit of work.
        ///
        /// Two shapes, because they answer different questions. Without
        /// `--grid` the work list is one tile from each of `--pages` pages,
        /// which is what spike 0.5 measured --- and it can only be run on a
        /// document that has that many pages. With `--grid N` it is instead a
        /// walk across an N-wide grid of tiles on a *single* page, which is the
        /// shape a viewport actually asks for and the only shape a one-page
        /// document can be asked for at all.
        fn work(&self, index: usize) -> TileSpec {
            let Some(cols) = self.grid else {
                return self.tile_spec(index as u16);
            };
            TileSpec {
                page: self.page,
                scale: self.scale,
                x: (index % cols) as i32 * self.tile as i32,
                y: (index / cols) as i32 * self.tile as i32,
                width: self.tile,
                height: self.tile,
            }
        }

        /// The `index`-th unit of work, as a request to a worker.
        fn work_request(&self, index: usize, shm_only: bool) -> Request {
            let TileSpec {
                page,
                scale,
                x,
                y,
                width,
                height,
            } = self.work(index);
            Request::Tile {
                page,
                scale,
                x,
                y,
                width,
                height,
                shm_only,
            }
        }

        /// The tile this run renders, for a given page.
        fn tile_spec(&self, page: u16) -> TileSpec {
            TileSpec {
                page,
                scale: self.scale,
                x: 0,
                y: 0,
                width: self.tile,
                height: self.tile,
            }
        }

        /// The same tile as a request to a worker.
        fn tile_request(&self, page: u16, shm_only: bool) -> Request {
            let TileSpec {
                page,
                scale,
                x,
                y,
                width,
                height,
            } = self.tile_spec(page);
            Request::Tile {
                page,
                scale,
                x,
                y,
                width,
                height,
                shm_only,
            }
        }
    }

    fn parse_args(mut it: impl Iterator<Item = String>) -> Result<Args, String> {
        let file = it
            .next()
            .ok_or("usage: worker-bench <file.pdf> [options]")?;
        let mut args = Args {
            file: PathBuf::from(file),
            mode: Mode::Latency,
            rounds: 5,
            reps: 20,
            pages: 64,
            workers: vec![1, 2, 4, 8],
            page: 0,
            scale: 1.5,
            tile: 1024,
            grid: None,
            library_dir: None,
            profiles: vec![
                "none".into(),
                "targeted".into(),
                "strict".into(),
                "worker".into(),
            ],
            budget_mb: 512,
            poll_ms: vec![0, 1, 5, 20],
        };
        while let Some(flag) = it.next() {
            let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--mode" => {
                    args.mode = match value.as_str() {
                        "latency" => Mode::Latency,
                        "parallel" => Mode::Parallel,
                        "crash" => Mode::Crash,
                        "timeout" => Mode::Timeout,
                        "limits" => Mode::Limits,
                        "authority" => Mode::Authority,
                        "footprint" => Mode::Footprint,
                        "engine" => Mode::Engine,
                        other => return Err(format!("bad --mode {other}")),
                    }
                }
                "--rounds" => args.rounds = value.parse().map_err(|_| "bad --rounds")?,
                "--reps" => args.reps = value.parse().map_err(|_| "bad --reps")?,
                "--pages" => args.pages = value.parse().map_err(|_| "bad --pages")?,
                "--workers" => {
                    args.workers = value
                        .split(',')
                        .map(|s| s.parse().map_err(|_| "bad --workers"))
                        .collect::<Result<_, _>>()?
                }
                "--page" => args.page = value.parse().map_err(|_| "bad --page")?,
                "--scale" => args.scale = value.parse().map_err(|_| "bad --scale")?,
                "--tile" => args.tile = value.parse().map_err(|_| "bad --tile")?,
                "--grid" => args.grid = Some(value.parse().map_err(|_| "bad --grid")?),
                "--lib" => args.library_dir = Some(PathBuf::from(value)),
                "--profiles" => args.profiles = value.split(';').map(str::to_string).collect(),
                "--budget-mb" => args.budget_mb = value.parse().map_err(|_| "bad --budget-mb")?,
                "--poll-ms" => {
                    args.poll_ms = value
                        .split(',')
                        .map(|s| s.parse().map_err(|_| "bad --poll-ms"))
                        .collect::<Result<_, _>>()?
                }
                other => return Err(format!("unknown flag {other}")),
            }
        }
        Ok(args)
    }

    // ------------------------------------------------------------------ main

    pub fn run() {
        let mut argv = std::env::args().skip(1);
        let first = argv.next();
        if first.as_deref() == Some("worker") {
            worker_main(argv.collect());
        }

        let args = match parse_args(first.into_iter().chain(argv)) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[ERROR] {e}");
                std::process::exit(2);
            }
        };

        let bytes = match std::fs::read(&args.file) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[ERROR] could not read {}: {e}", args.file.display());
                std::process::exit(2);
            }
        };
        let mut doc_shm = match Shm::create(bytes.len()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[ERROR] {e}");
                std::process::exit(2);
            }
        };
        doc_shm.as_mut_slice().copy_from_slice(&bytes);

        println!(
            "worker-bench  {}  {} bytes  tile {}x{} @ {}x",
            args.file.display(),
            bytes.len(),
            args.tile,
            args.tile,
            args.scale
        );
        println!();

        let spec = Spawn {
            doc_fd: doc_shm.file.as_raw_fd(),
            doc_len: bytes.len(),
            library_dir: args.library_dir.clone(),
            sandbox: None,
            rlimit_as: None,
            rlimit_cpu: None,
            rlimit_nofile: None,
            show_stderr: false,
        };

        let result = match args.mode {
            Mode::Latency => mode_latency(&args, &spec),
            Mode::Parallel => mode_parallel(&args, &spec),
            Mode::Crash => mode_crash(&args, &spec),
            Mode::Timeout => mode_timeout(&args, &spec),
            Mode::Limits => mode_limits(&args, &spec),
            Mode::Authority => mode_authority(&args, &spec),
            Mode::Footprint => mode_footprint(&args, &spec),
            Mode::Engine => mode_engine(&args, &spec),
        };

        if let Err(e) = result {
            eprintln!("[ERROR] {e}");
            std::process::exit(1);
        }
    }

    // -------------------------------------------------------- mode: latency

    /// One variant's timings for one round, all per-tile means in milliseconds.
    struct Row {
        round: usize,
        variant: &'static str,
        wall: f64,
        render: f64,
        swizzle: f64,
        fold: f64,
    }

    impl Row {
        /// Everything the tile cost that was not rendering it or reading it ---
        /// serialization, the pipe, the worker's own loop.
        fn transport(&self) -> f64 {
            self.wall - self.render - self.swizzle - self.fold
        }
    }

    /// One tile, four ways, interleaved across rounds.
    fn mode_latency(args: &Args, spec: &Spawn) -> Result<(), String> {
        let pdfium = bind(&args.library_dir)?;
        let doc = pdfium
            .load_pdf_from_file(&args.file, None)
            .map_err(|e| format!("parent open failed: {e}"))?;
        let mut local = vec![0u8; TILE_CAPACITY];

        let mut worker = Worker::spawn(spec)?;
        let opened = worker.call(&Request::Open)?;
        if !opened.ok {
            return Err(opened.error);
        }
        println!("worker opened the document: {}", opened.note);
        println!();

        let n = args.tile as usize * args.tile as usize * 4;
        let mut payload = Vec::with_capacity(n);
        let mut rows: Vec<Row> = Vec::new();

        // `ping` carries no pixels at all, so it isolates the cost of the
        // control channel itself: write a line, wake the worker, read a line.
        // Whatever the other variants cost beyond it is attributable to moving
        // the tile, not to crossing the boundary.
        const VARIANTS: [&str; 4] = ["ping", "inproc", "pipe", "shm"];

        for round in 0..args.rounds {
            // Interleaved within each round and compared pairwise, because wall
            // clock on this machine drifts more than the effect being measured.
            for variant in VARIANTS {
                let t0 = Instant::now();
                let mut render_us = 0u64;
                let mut swizzle_us = 0u64;
                let mut fold_us = 0u64;
                let mut sink = 0u64;

                for _ in 0..args.reps {
                    match variant {
                        "ping" => {
                            worker.call(&Request::Ping)?;
                        }
                        "inproc" => {
                            let (r, s) = render_into(&doc, args.tile_spec(args.page), &mut local)?;
                            render_us += r;
                            swizzle_us += s;
                            let t = Instant::now();
                            sink = sink.wrapping_add(checksum(&local[..n]));
                            fold_us += t.elapsed().as_micros() as u64;
                        }
                        transport => {
                            let shm_only = transport == "shm";
                            let response = worker.call(&Request::Tile {
                                page: args.page,
                                scale: args.scale,
                                x: 0,
                                y: 0,
                                width: args.tile,
                                height: args.tile,
                                shm_only,
                            })?;
                            if !response.ok {
                                return Err(response.error);
                            }
                            render_us += response.render_us;
                            swizzle_us += response.swizzle_us;
                            if shm_only {
                                let t = Instant::now();
                                sink = sink.wrapping_add(checksum(&worker.tile.as_slice()[..n]));
                                fold_us += t.elapsed().as_micros() as u64;
                            } else {
                                worker.read_payload(&mut payload, response.bytes)?;
                                let t = Instant::now();
                                sink = sink.wrapping_add(checksum(&payload));
                                fold_us += t.elapsed().as_micros() as u64;
                            }
                        }
                    }
                }

                let reps = args.reps as f64;
                rows.push(Row {
                    round,
                    variant,
                    wall: t0.elapsed().as_secs_f64() * 1000.0 / reps,
                    render: render_us as f64 / 1000.0 / reps,
                    swizzle: swizzle_us as f64 / 1000.0 / reps,
                    fold: fold_us as f64 / 1000.0 / reps,
                });
                std::hint::black_box(sink);
            }
        }

        println!(
            "{:>5}  {:<7} {:>11} {:>10} {:>10} {:>10} {:>11}",
            "round", "variant", "end-to-end", "render", "swizzle", "parent fold", "transport"
        );
        for r in &rows {
            println!(
                "{:>5}  {:<7} {:>10.3}ms {:>9.3}ms {:>9.3}ms {:>9.3}ms {:>10.3}ms",
                r.round,
                r.variant,
                r.wall,
                r.render,
                r.swizzle,
                r.fold,
                r.transport()
            );
        }
        println!();

        // Round 0 is a consistent warm-up outlier; excluded, but printed above
        // rather than quietly dropped.
        let steady: Vec<&Row> = rows.iter().filter(|r| r.round > 0).collect();
        let mean = |variant: &str, f: fn(&Row) -> f64| {
            let picked: Vec<f64> = steady
                .iter()
                .filter(|r| r.variant == variant)
                .map(|r| f(r))
                .collect();
            picked.iter().sum::<f64>() / picked.len() as f64
        };

        println!(
            "means over rounds 1..{} (round 0 excluded as warm-up):",
            args.rounds - 1
        );
        for variant in VARIANTS {
            println!(
                "  {variant:<7} {:>7.3} ms end to end = {:.3} render + {:.3} swizzle + \
                 {:.3} parent fold + {:.3} transport",
                mean(variant, |r| r.wall),
                mean(variant, |r| r.render),
                mean(variant, |r| r.swizzle),
                mean(variant, |r| r.fold),
                mean(variant, Row::transport),
            );
        }
        println!();
        println!(
            "  bare round trip, no pixels            {:>7.3} ms",
            mean("ping", |r| r.wall)
        );
        println!(
            "  moving {} KB down the pipe             {:>7.3} ms",
            n / 1024,
            mean("pipe", Row::transport) - mean("ping", |r| r.wall)
        );
        println!(
            "  moving {} KB through shared memory     {:>7.3} ms",
            n / 1024,
            mean("shm", Row::transport) - mean("ping", |r| r.wall)
        );
        println!();
        println!(
            "note: every pixel variant folds all {} KB in the parent, timed separately \
             above, so none of them can look cheap by never reading what it received.",
            n / 1024
        );

        let _ = worker.call(&Request::Ping);
        Ok(())
    }

    // ------------------------------------------------------- mode: parallel

    /// Does K workers actually give K times the throughput?
    ///
    /// Measured twice, because the answer differs and the difference is the
    /// point. With `fold` on, the parent reads every delivered tile --- which is
    /// what a coordinator does, and which competes for the same cores as the
    /// workers. With it off, only Pdfium's own scaling is visible. Reporting
    /// only the first understates Pdfium; reporting only the second promises a
    /// throughput no coordinator can absorb.
    fn mode_parallel(args: &Args, spec: &Spawn) -> Result<(), String> {
        match args.grid {
            Some(cols) => println!(
                "rendering {} tiles of page {} in a {cols}-wide grid, {}x{} at {}x, \
                 {} rounds interleaved\n",
                args.pages, args.page, args.tile, args.tile, args.scale, args.rounds
            ),
            None => println!(
                "rendering {} pages to {}x{} tiles at {}x, {} rounds interleaved\n",
                args.pages, args.tile, args.tile, args.scale, args.rounds
            ),
        }

        let n = args.tile as usize * args.tile as usize * 4;
        let pdfium = bind(&args.library_dir)?;
        let doc = pdfium
            .load_pdf_from_file(&args.file, None)
            .map_err(|e| format!("parent open failed: {e}"))?;
        let mut local = vec![0u8; TILE_CAPACITY];
        // The first render of a document pays one-off costs that would otherwise
        // be charged entirely to whichever variant ran first.
        render_into(&doc, args.work(0), &mut local)?;

        // Workers are spawned once and reused across every round, so process
        // startup is never inside a timed section.
        let mut pools: Vec<(usize, Vec<Worker>)> = Vec::new();
        for &k in &args.workers {
            let mut pool = Vec::new();
            for _ in 0..k {
                let mut w = Worker::spawn(spec)?;
                let opened = w.call(&Request::Open)?;
                if !opened.ok {
                    return Err(opened.error);
                }
                w.call(&args.work_request(0, true))?;
                pool.push(w);
            }
            pools.push((k, pool));
        }

        for fold in [true, false] {
            // Variant order is fixed within a round and rounds are repeated, so
            // drift shows up as spread across rounds rather than as a slope
            // through the table.
            let mut baseline = Vec::new();
            let mut measured: Vec<(usize, Vec<f64>)> =
                pools.iter().map(|(k, _)| (*k, Vec::new())).collect();

            for _ in 0..args.rounds {
                let t0 = Instant::now();
                let mut sink = 0u64;
                for index in 0..args.pages {
                    render_into(&doc, args.work(index), &mut local)?;
                    if fold {
                        sink = sink.wrapping_add(checksum(&local[..n]));
                    }
                }
                std::hint::black_box(sink);
                baseline.push(t0.elapsed().as_secs_f64());

                for (index, (_, pool)) in pools.iter_mut().enumerate() {
                    let elapsed = run_pool(pool, args, n, fold)?;
                    measured[index].1.push(elapsed);
                }
            }

            let best = |v: &[f64]| v.iter().cloned().fold(f64::INFINITY, f64::min);
            let base = best(&baseline);
            println!(
                "parent reads every tile: {}",
                if fold { "yes" } else { "no" }
            );
            println!(
                "  in-process, 1 thread    {:>7.3} s   {:>7.1} pages/s   (baseline)",
                base,
                args.pages as f64 / base
            );
            for (k, samples) in &measured {
                let elapsed = best(samples);
                println!(
                    "  {k:>2} worker process{:<2}  {elapsed:>7.3} s   {:>7.1} pages/s   \
                     {:>5.2}x baseline",
                    if *k == 1 { "" } else { "es" },
                    args.pages as f64 / elapsed,
                    base / elapsed
                );
            }
            println!();
        }

        for (_, pool) in pools {
            for mut w in pool {
                let _ = w.child.kill();
                let _ = w.child.wait();
            }
        }

        println!(
            "best of {} rounds, because the interference here is other processes on the \
             machine and it is one-sided --- a round can only be slowed down.",
            args.rounds
        );
        println!(
            "the fold stands in for whatever the coordinator does with a tile, and it \
             understates it: §3 measured 3.0 ms to hand the same 4 MB to the webview, \
             against {:.2} ms to fold it.",
            0.86
        );
        Ok(())
    }

    /// Runs `pages` renders across a pool, one thread per worker, work-stealing.
    fn run_pool(
        pool: &mut [Worker],
        args: &Args,
        tile_bytes: usize,
        fold: bool,
    ) -> Result<f64, String> {
        let next = AtomicUsize::new(0);
        let total = args.pages;
        let t0 = Instant::now();
        let outcome: Result<(), String> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for worker in pool.iter_mut() {
                let next = &next;
                handles.push(scope.spawn(move || -> Result<u64, String> {
                    let mut acc = 0u64;
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= total {
                            return Ok(acc);
                        }
                        let response = worker.call(&args.work_request(index, true))?;
                        if !response.ok {
                            return Err(response.error);
                        }
                        if fold {
                            acc = acc.wrapping_add(checksum(&worker.tile.as_slice()[..tile_bytes]));
                        }
                    }
                }));
            }
            let mut acc = 0u64;
            for handle in handles {
                acc = acc.wrapping_add(handle.join().map_err(|_| "worker thread panicked")??);
            }
            std::hint::black_box(acc);
            Ok(())
        });
        outcome?;
        Ok(t0.elapsed().as_secs_f64())
    }

    // ---------------------------------------------------------- mode: crash

    /// Is a worker death contained, and what does recovery cost?
    fn mode_crash(args: &Args, spec: &Spawn) -> Result<(), String> {
        for how in ["abort", "segv", "exit"] {
            let mut spec = spec.clone();
            spec.show_stderr = false;
            let mut worker = Worker::spawn(&spec)?;
            worker.call(&Request::Open)?;
            let good = worker.call(&args.tile_request(args.page, true))?;
            if !good.ok {
                return Err(good.error);
            }

            // The worker acknowledges, then dies. So the failure surfaces on the
            // *next* request, which is the realistic shape: a worker dies with a
            // queue behind it, not while answering the request that killed it.
            let ack = worker.call(&Request::Crash { how: how.into() })?;
            if !ack.ok {
                return Err(ack.error);
            }

            let t0 = Instant::now();
            let detected = worker.call(&args.tile_request(args.page, true));
            let detect_ms = t0.elapsed().as_secs_f64() * 1000.0;
            let failure = match detected {
                Ok(r) => format!("NO FAILURE REPORTED (ok={})", r.ok),
                Err(e) => e,
            };
            let epitaph = worker.epitaph();

            let t1 = Instant::now();
            let mut fresh = Worker::spawn(&spec)?;
            fresh.call(&Request::Open)?;
            let retried = fresh.call(&args.tile_request(args.page, true))?;
            let recover_ms = t1.elapsed().as_secs_f64() * 1000.0;

            println!("crash by {how}:");
            println!("  child               {epitaph}");
            println!("  parent noticed      {failure} after {detect_ms:.1} ms");
            println!(
                "  respawn + reopen + first tile   {recover_ms:.1} ms   (tile ok = {})",
                retried.ok
            );
            println!("  parent still running: yes");
            println!();

            let _ = fresh.child.kill();
            let _ = fresh.child.wait();
        }
        Ok(())
    }

    // -------------------------------------------------------- mode: timeout

    /// Can a render that is already running be abandoned?
    fn mode_timeout(args: &Args, spec: &Spawn) -> Result<(), String> {
        let deadline = Duration::from_millis(250);
        let mut worker = Worker::spawn(spec)?;
        worker.call(&Request::Open)?;

        // Ask for something known to take seconds --- the whole point is that
        // the request cannot be withdrawn once Pdfium is inside it.
        let request = args.tile_request(args.page, true);
        let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        line.push('\n');
        let t0 = Instant::now();
        worker
            .stdin
            .write_all(line.as_bytes())
            .and_then(|()| worker.stdin.flush())
            .map_err(|e| e.to_string())?;

        let answered = worker.wait_readable(deadline);
        let waited = t0.elapsed().as_secs_f64() * 1000.0;
        if answered {
            println!(
                "worker answered in {waited:.1} ms, inside the {} ms deadline --- pick a \
                 heavier page or a larger --scale to exercise the kill path.",
                deadline.as_millis()
            );
            let _ = worker.child.kill();
            let _ = worker.child.wait();
            return Ok(());
        }

        println!(
            "deadline of {} ms passed with the worker still inside Pdfium.",
            deadline.as_millis()
        );
        let t1 = Instant::now();
        worker.child.kill().map_err(|e| format!("kill: {e}"))?;
        let epitaph = worker.epitaph();
        let kill_ms = t1.elapsed().as_secs_f64() * 1000.0;

        let t2 = Instant::now();
        let mut fresh = Worker::spawn(spec)?;
        fresh.call(&Request::Open)?;
        let ready_ms = t2.elapsed().as_secs_f64() * 1000.0;

        println!("  kill to reaped      {kill_ms:.1} ms   ({epitaph})");
        println!("  respawn to ready    {ready_ms:.1} ms");
        println!();
        println!(
            "so a runaway render costs one process, not the application --- but note the \
             kill discards work that cannot be resumed. Pdfium's progressive API \
             (IFSDK_PAUSE) is the cooperative alternative and is not exercised here."
        );

        let _ = fresh.child.kill();
        let _ = fresh.child.wait();
        Ok(())
    }

    // --------------------------------------------------------- mode: limits

    /// Does `setrlimit` actually bound a worker on this platform?
    fn mode_limits(args: &Args, spec: &Spawn) -> Result<(), String> {
        for (label, as_mb, cpu_s, nofile, renders) in [
            ("memory, 256 MB", Some(256u64), None, None, 1),
            ("cpu time, 1 s", None, Some(1u64), None, 1),
            (
                "descriptors, 8 and no file writes",
                None,
                None,
                Some(8u64),
                1,
            ),
            // Three renders under a limit one render does not reach, to show
            // whether the budget is per-request or per-process lifetime.
            ("cpu time, 3 s, three renders", None, Some(3u64), None, 3),
        ] {
            let mut spec = spec.clone();
            spec.rlimit_as = as_mb;
            spec.rlimit_cpu = cpu_s;
            spec.rlimit_nofile = nofile;
            spec.show_stderr = true;

            let mut worker = match Worker::spawn(&spec) {
                Ok(w) => w,
                Err(e) => {
                    println!("{label}: worker would not start: {e}\n");
                    continue;
                }
            };
            let ping = match worker.call(&Request::Ping) {
                Ok(p) => p,
                Err(e) => {
                    println!("{label}: worker died before serving anything: {e}");
                    println!("  {}\n", worker.epitaph());
                    continue;
                }
            };
            println!("{label}:");
            println!("  at startup          {}", ping.note);

            let opened = worker.call(&Request::Open);
            match opened {
                Ok(r) if r.ok => println!("  open                ok ({})", r.note),
                Ok(r) => println!("  open                refused: {}", r.error),
                Err(e) => {
                    println!("  open                worker died: {e}");
                    println!("  {}\n", worker.epitaph());
                    continue;
                }
            }

            let mut alive = true;
            for attempt in 1..=renders {
                let t0 = Instant::now();
                let tile = worker.call(&args.tile_request(args.page, true));
                let elapsed = t0.elapsed().as_secs_f64() * 1000.0;
                match tile {
                    Ok(r) if r.ok => {
                        println!("  render {attempt}            completed in {elapsed:.1} ms")
                    }
                    Ok(r) => println!("  render {attempt}            refused cleanly: {}", r.error),
                    Err(e) => {
                        println!(
                            "  render {attempt}            worker died after {elapsed:.1} ms: {e}"
                        );
                        println!("  {}", worker.epitaph());
                        alive = false;
                        break;
                    }
                }
            }
            println!();

            if !alive {
                continue;
            }

            let _ = worker.child.kill();
            let _ = worker.child.wait();
        }

        println!(
            "a limit that is accepted by setrlimit but never fires is worse than no \
             limit, because it reads as a bound that exists."
        );
        Ok(())
    }

    // ------------------------------------------------------ mode: authority

    /// Can the worker be denied files and network and still render?
    ///
    /// "The render returned ok" is not evidence, so every profile's tile is
    /// folded and compared against the unrestricted one. A sandboxed Pdfium
    /// that cannot reach `/System/Library/Fonts` will substitute or drop glyphs
    /// and report success either way --- which is the same silent-substitution
    /// failure mode §6 already found in `set_text()`.
    fn mode_authority(args: &Args, spec: &Spawn) -> Result<(), String> {
        let tile_bytes = args.tile as usize * args.tile as usize * 4;
        let mut reference: Option<u64> = None;

        for name in &args.profiles {
            let profile = if name == "none" {
                None
            } else {
                Some(name.as_str())
            };
            let label = if name.starts_with('(') {
                "custom"
            } else {
                name.as_str()
            };
            let mut spec = spec.clone();
            spec.sandbox = profile.map(str::to_string);
            spec.show_stderr = true;

            println!("sandbox profile: {label}");
            let mut worker = match Worker::spawn(&spec) {
                Ok(w) => w,
                Err(e) => {
                    println!("  worker would not start: {e}\n");
                    continue;
                }
            };
            match worker.call(&Request::Ping) {
                Ok(p) if !p.note.is_empty() => println!("  startup             {}", p.note),
                Ok(_) => println!("  startup             unrestricted"),
                Err(e) => {
                    println!("  died before serving anything: {e}");
                    println!("  {}\n", worker.epitaph());
                    continue;
                }
            }

            match worker.call(&Request::Probe) {
                Ok(p) => {
                    for line in p.note.split("; ") {
                        println!("  {line}");
                    }
                }
                Err(e) => {
                    println!("  probe               worker died: {e}");
                    println!("  {}\n", worker.epitaph());
                    continue;
                }
            }

            match worker.call(&Request::Open) {
                Ok(r) if r.ok => println!("  open from shm       ok ({})", r.note),
                Ok(r) => println!("  open from shm       FAILED: {}", r.error),
                Err(e) => {
                    println!("  open from shm       worker died: {e}");
                    println!("  {}\n", worker.epitaph());
                    continue;
                }
            }

            match worker.call(&args.tile_request(args.page, true)) {
                Ok(r) if r.ok => {
                    let pixels = &worker.tile.as_slice()[..tile_bytes];
                    let digest = checksum(pixels);
                    let ink = ink_fraction(pixels);
                    let verdict = match reference {
                        None => {
                            reference = Some(digest);
                            "reference".to_string()
                        }
                        Some(want) if want == digest => {
                            "pixel-identical to unsandboxed".to_string()
                        }
                        Some(_) => "[FAIL] PIXELS DIFFER FROM UNSANDBOXED".to_string(),
                    };
                    println!(
                        "  render              ok, {:.2} ms in Pdfium, ink {:.4}%, {verdict}",
                        r.render_us as f64 / 1000.0,
                        ink * 100.0
                    );
                }
                Ok(r) => println!("  render              FAILED: {}", r.error),
                Err(e) => {
                    println!("  render              worker died: {e}");
                    println!("  {}", worker.epitaph());
                }
            }
            println!();

            let _ = worker.child.kill();
            let _ = worker.child.wait();
        }

        println!(
            "the document never reaches the worker as a path, so a denial of file-read \
             is not something the render path has to work around --- but a document \
             whose fonts are NOT embedded sends Pdfium to the system font files, which \
             is a path. Run this against a base-14 fixture, not only an embedded one."
        );
        Ok(())
    }

    // --------------------------------------------------------- mode: engine

    /// Does `haystack` contain `needle` anywhere?
    fn mode_engine(args: &Args, _spec: &Spawn) -> Result<(), String> {
        let dir = args
            .library_dir
            .clone()
            .ok_or("--lib DIR is required: this mode reads the library, it does not bind it")?;
        super::engine_report(&dir)
    }

    // ------------------------------------------------------ mode: footprint

    /// One supervised runaway.
    struct BalloonRow {
        round: usize,
        poll_ms: u64,
        /// `false` when the child finished its whole burst and exited between
        /// two samples, so the parent never saw it cross at all.
        caught: bool,
        detect_ms: f64,
        peak_mb: f64,
        overshoot_mb: f64,
        growth_mb_s: f64,
        kill_ms: f64,
        epitaph: String,
    }

    fn median(values: &[f64]) -> f64 {
        if values.is_empty() {
            return f64::NAN;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        if sorted.len() % 2 == 0 {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    }

    const MB: f64 = 1024.0 * 1024.0;

    /// What a ballooning worker exits with when it reaches its own ceiling ---
    /// i.e. when supervision failed to stop it.
    const BALLOON_CAPPED: i32 = 11;

    /// If the kernel will not bound a worker's memory, can the parent?
    ///
    /// `limits` mode established that macOS refuses `RLIMIT_AS`, `RLIMIT_DATA`
    /// and `RLIMIT_RSS` outright, which leaves the worker's memory unbounded by
    /// anything --- a real gap, since a decompression bomb or a pathological
    /// page is a plausible document rather than an exotic one. The remaining
    /// mechanism is supervision: the parent samples the child's footprint and
    /// kills it over budget.
    ///
    /// Whether that is a *bound* depends on a number, not on an argument. A
    /// poll interval buys overshoot at the rate the child can take memory, so
    /// the question this mode answers is how much a worker can take between two
    /// samples, and it answers it against a child allocating as fast as the
    /// machine allows --- faster than any document would, which is the point.
    fn mode_footprint(args: &Args, spec: &Spawn) -> Result<(), String> {
        let me = std::process::id();
        if phys_footprint(me).is_none() {
            println!(
                "proc_pid_rusage is unavailable here, so this mode measures nothing. \
                 Windows would use a job object with JOB_OBJECT_LIMIT_PROCESS_MEMORY, \
                 which is a real kernel bound and needs no polling at all."
            );
            return Ok(());
        }

        // What a sample costs, because the poll interval is only choosable if
        // sampling itself is free.
        let probes = 2000;
        let t0 = Instant::now();
        for _ in 0..probes {
            std::hint::black_box(phys_footprint(me));
        }
        let per_sample_us = t0.elapsed().as_secs_f64() * 1e6 / f64::from(probes);
        println!("one proc_pid_rusage sample: {per_sample_us:.2} us (over {probes} calls)");
        println!();

        // Ground the budget. A threshold picked without knowing what honest
        // work costs is not a limit, it is a coin toss.
        {
            let mut worker = Worker::spawn(spec)?;
            let pid = worker.child.id();
            worker.call(&Request::Ping)?;
            let at_rest = phys_footprint(pid).unwrap_or(0);
            worker.call(&Request::Open)?;
            let after_open = phys_footprint(pid).unwrap_or(0);
            for _ in 0..args.reps {
                worker.call(&args.tile_request(args.page, true))?;
            }
            let after_tiles = phys_footprint(pid).unwrap_or(0);
            println!("what legitimate work costs a worker:");
            println!(
                "  at rest, Pdfium bound        {:>8.1} MB",
                at_rest as f64 / MB
            );
            println!(
                "  document open                {:>8.1} MB   (+{:.1})",
                after_open as f64 / MB,
                (after_open - at_rest) as f64 / MB
            );
            println!(
                "  after {} tiles of {}x{}     {:>8.1} MB   (+{:.1})",
                args.reps,
                args.tile,
                args.tile,
                after_tiles as f64 / MB,
                (after_tiles - after_open) as f64 / MB
            );
            println!(
                "  budget for the runs below    {:>8.1} MB",
                args.budget_mb as f64
            );
            println!(
                "  (the tile mapping is not in any of these: a footprint excludes clean \
                 file-backed pages, so both the mapped document and the shared tile \
                 buffer are invisible here. Both are the parent's allocations and are \
                 bounded where they are made.)"
            );
            println!();
            let _ = worker.child.kill();
            let _ = worker.child.wait();
        }

        let budget = args.budget_mb * 1024 * 1024;
        let cap_mb = (args.budget_mb * 4) as usize;
        let deadline = Duration::from_secs(10);
        let mut rows: Vec<BalloonRow> = Vec::new();

        // Intervals interleaved inside rounds, so a thermal or scheduling drift
        // over the run hits every interval equally instead of the last one.
        for round in 0..args.rounds {
            for &poll_ms in &args.poll_ms {
                let mut worker = Worker::spawn(spec)?;
                let pid = worker.child.id();
                worker.call(&Request::Open)?;
                let ack = worker.call(&Request::Balloon {
                    chunk_kb: 1024,
                    cap_mb,
                })?;
                if !ack.ok {
                    return Err(ack.error);
                }

                let started = Instant::now();
                let first = phys_footprint(pid).unwrap_or(0);
                let mut peak = first;
                let mut crossed = None;
                let mut gone = None;
                loop {
                    if let Some(now) = phys_footprint(pid) {
                        peak = peak.max(now);
                        if now >= budget {
                            crossed = Some((started.elapsed(), now));
                            break;
                        }
                    }
                    // A burst is bounded: the child may take everything it wants
                    // and exit before the next sample, in which case supervision
                    // did not overshoot, it never engaged. That is a different
                    // outcome and averaging it in as "0 MB overshoot" would read
                    // as the best result rather than the worst.
                    if let Ok(Some(status)) = worker.child.try_wait() {
                        gone = Some((started.elapsed(), status));
                        break;
                    }
                    if started.elapsed() > deadline {
                        break;
                    }
                    if poll_ms > 0 {
                        std::thread::sleep(Duration::from_millis(poll_ms));
                    }
                }

                // The exit code matters: BALLOON_CAPPED means the child really
                // did take everything it asked for and the parent simply never
                // looked while it was happening. Any other code means it died
                // for an unrelated reason, and a "miss" that is really an early
                // death would read as the worst result while proving nothing.
                let (kill_ms, epitaph) = match gone {
                    Some((_, status)) => (
                        0.0,
                        match status.code() {
                            Some(BALLOON_CAPPED) => {
                                format!("took all {cap_mb} MB and exited, unnoticed")
                            }
                            Some(code) => format!("[FAIL] exited with code {code}, not the cap"),
                            None => format!("[FAIL] {}", status),
                        },
                    ),
                    None => {
                        let t_kill = Instant::now();
                        let _ = worker.child.kill();
                        let epitaph = worker.epitaph();
                        (t_kill.elapsed().as_secs_f64() * 1000.0, epitaph)
                    }
                };

                let caught = crossed.is_some();
                let (detect, at) = crossed.unwrap_or_else(|| {
                    (
                        gone.map_or_else(|| started.elapsed(), |(at, _)| at),
                        peak.max(first),
                    )
                });
                rows.push(BalloonRow {
                    round,
                    poll_ms,
                    caught,
                    detect_ms: detect.as_secs_f64() * 1000.0,
                    peak_mb: peak as f64 / MB,
                    overshoot_mb: (at.saturating_sub(budget)) as f64 / MB,
                    growth_mb_s: (at.saturating_sub(first)) as f64
                        / MB
                        / detect.as_secs_f64().max(1e-9),
                    kill_ms,
                    epitaph,
                });
            }
        }

        println!(
            "runaway worker against a {} MB budget, bursting to at most {cap_mb} MB, \
             {} rounds interleaved:",
            args.budget_mb, args.rounds
        );
        println!(
            "{:>5}  {:>7}  {:>11}  {:>10}  {:>11}  {:>12}  {:>10}  child",
            "round", "poll", "detected", "peak", "overshoot", "growth", "kill+reap"
        );
        for r in &rows {
            println!(
                "{:>5}  {:>5}ms  {:>9.2}ms  {:>7.1}MB  {:>8.1}MB  {:>7.0}MB/s  {:>8.2}ms  {}",
                r.round,
                r.poll_ms,
                r.detect_ms,
                r.peak_mb,
                r.overshoot_mb,
                r.growth_mb_s,
                r.kill_ms,
                r.epitaph
            );
        }
        println!();

        // Median overshoot understates the bound and worst observed understates
        // it too, because both depend on where the crossing happens to fall
        // between two samples. What the interval actually guarantees is
        // interval x growth rate, so that is printed alongside and is the number
        // a budget has to be set from.
        println!("by poll interval --- medians, worst seen, and what the interval bounds:");
        for &poll_ms in &args.poll_ms {
            // Missed rows are excluded from every statistic and counted
            // separately. A miss is not a small overshoot.
            let pick = |f: fn(&BalloonRow) -> f64| {
                median(
                    &rows
                        .iter()
                        .filter(|r| r.poll_ms == poll_ms && r.caught)
                        .map(f)
                        .collect::<Vec<_>>(),
                )
            };
            let attempts = rows.iter().filter(|r| r.poll_ms == poll_ms).count();
            let missed = rows
                .iter()
                .filter(|r| r.poll_ms == poll_ms && !r.caught)
                .count();
            if missed == attempts {
                println!(
                    "  poll {poll_ms:>2} ms   never saw the burst at all: {missed}/{attempts} \
                     runs finished between two samples"
                );
                continue;
            }
            let worst = rows
                .iter()
                .filter(|r| r.poll_ms == poll_ms && r.caught)
                .map(|r| r.overshoot_mb)
                .fold(0.0f64, f64::max);
            let growth = pick(|r| r.growth_mb_s);
            let cpu = if poll_ms == 0 {
                100.0
            } else {
                per_sample_us / (poll_ms as f64 * 1000.0) * 100.0
            };
            println!(
                "  poll {poll_ms:>2} ms   overshoot {:>6.1} MB median, {worst:>6.1} MB worst, \
                 {:>6.0} MB bounded   growth {growth:>6.0} MB/s   kill+reap {:>5.2} ms   \
                 poll costs {cpu:>6.3}% of a core   missed {missed}/{attempts}",
                pick(|r| r.overshoot_mb),
                growth * poll_ms as f64 / 1000.0,
                pick(|r| r.kill_ms),
            );
        }
        println!();

        let caught: Vec<&BalloonRow> = rows.iter().filter(|r| r.caught).collect();
        let killed = caught.iter().all(|r| r.epitaph.contains("signal 9"));
        let missed = rows.len() - caught.len();
        println!(
            "every worker the parent caught was killed by it: {}",
            if killed { "[OK] yes" } else { "[FAIL] no" }
        );
        println!(
            "bursts that completed between two samples: {missed} of {} {}",
            rows.len(),
            if missed == 0 {
                "[OK]"
            } else {
                "--- supervision never engaged on those"
            }
        );
        if rows.iter().any(|r| r.epitaph.contains("[FAIL]")) {
            println!(
                "[FAIL] a child died for a reason other than its cap, so at least one \
                 'missed' row proves nothing"
            );
        }
        println!();
        println!(
            "the child here takes memory as fast as the allocator hands it over, which no \
             document does, so the growth rate is a ceiling. Two things follow. A pool of \
             N workers must be budgeted at (per-worker budget + bounded overshoot) x N, \
             and the overshoot term is exactly the price of having no kernel limit. And a \
             poll interval of zero is not free supervision --- it burns a core, and the \
             lower overshoot it shows here is partly bought by starving the child of the \
             CPU it was allocating with."
        );
        Ok(())
    }
}
