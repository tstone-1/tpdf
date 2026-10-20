//! Drives the whole Windows print path to a real spooler, without paper.
//!
//! `print_win.rs` ends at `present`, which opens a dialog --- so everything from
//! the dialog inward was unverifiable by anything automatic, which is the same gap
//! `print_macos.rs` records and accepts. On Windows it does not have to be
//! accepted: **"Microsoft Print to PDF" is a real printer with a real driver and a
//! real spooler**, and naming an output file in `DOCINFOW.lpszOutput` makes it
//! write there instead of raising a save dialog. So the pipeline can be driven end
//! to end by opening a printer DC directly and skipping only the panel.
//!
//! What that buys is the check no unit test can make. `print.rs` proves the *job*
//! is right and `print_win`'s own tests prove a page rasterises to the right size;
//! neither can say that GDI accepted the DIB, that the driver consumed the pages,
//! or that anything arrived. This asks the printer.
//!
//! **Ink, not page count.** A blank page is the failure mode that matters here: a
//! wrong `BITMAPINFO`, a DC in the wrong mapping mode or a bad `StretchDIBits`
//! rectangle all produce the right number of perfectly empty sheets, and a check
//! that counted pages would pass on every one of them. So each printed page is
//! rendered back and its ink counted, with the source pages' ink as the control ---
//! `AGENTS.md` records that a comparison whose baseline is never itself checked is
//! the shape of a check that cannot fail, and "both zero" is exactly how this one
//! would silently pass.
//!
//! ```text
//! cargo run --release --example print-probe
//! cargo run --release --example print-probe -- testdata/rotated.pdf
//! cargo run --release --example print-probe -- testdata/rotated.pdf "Microsoft Print to PDF"
//! ```
//!
//! The body is an inner `mod imp` rather than a `#![cfg]` at the crate root, for the
//! reason `win_sandbox_probe.rs` gives: a crate-root `#![cfg]` empties a `[[bin]]`
//! including its `main`, and cargo then reports a missing entry point rather than a
//! deliberately skipped target. Kept in this file rather than a `print_probe/`
//! subdirectory --- a directory under `src/bin/` becomes a phantom binary in the
//! Windows installer, which is a trap of its own.

// Nothing here has a macOS counterpart to run: the print path there is
// `NSPrintOperation`, which paginates PDF bytes itself and has no DC, no DIB and no
// spooler file to inspect. A probe that could only report their absence should say
// so rather than print a table.
#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("print-probe drives a Win32 printer DC or a CUPS queue; Windows and Linux only");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    // The read-back worker is this executable started again.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == tpdf_lib::worker::WORKER_ARGV) {
        tpdf_lib::worker_child::main(&args);
    }
    linux::main();
}

/// The Linux half: everything after GTK's print dialog, to a real CUPS queue.
///
/// `print_linux.rs` hands the job's PDF to CUPS through a `GtkPrintJob`. The
/// queue used is cups-pdf's (the package `printer-driver-cups-pdf`, installed
/// on the test machine only), which writes what it is sent to a PDF in
/// `~/PDF`, so what came out of CUPS can be read again. The dialog is skipped
/// with `print_linux::print_to`, which reaches the same `submit` as the
/// dialog's Print button.
///
/// The same order of checks as on Windows: the job, the read-back the
/// application makes before the dialog, a control that the pages sent have
/// ink, then the pages that came out of CUPS counted and their ink measured.
/// Needs a display for GTK; under WSL, `xvfb-run`.
///
/// ```text
/// xvfb-run cargo run --release --example print-probe
/// xvfb-run cargo run --release --example print-probe -- testdata/vector-multi.pdf PDF
/// ```
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime};

    use tpdf_lib::document::OpenDocument;
    use tpdf_lib::print::{self, Job, PagePlan, Pages};
    use tpdf_lib::progressive::{self, Bindings, CancelToken, TileSpec};
    use tpdf_lib::{print_linux, save};

    /// cups-pdf's queue, as Ubuntu's package names it.
    const DEFAULT_PRINTER: &str = "PDF";

    /// Device pixels per point for the ink count; a thumbnail settles it.
    const CHECK_SCALE: f32 = 0.5;

    struct Report {
        passed: usize,
        failed: usize,
    }

    impl Report {
        fn check(&mut self, name: &str, ok: bool, detail: &str) {
            if ok {
                self.passed += 1;
                println!("[OK]   {name:<56} {detail}");
            } else {
                self.failed += 1;
                println!("[FAIL] {name:<56} {detail}");
            }
        }
    }

    pub fn main() {
        let mut argv = std::env::args().skip(1);
        let source = argv
            .next()
            .map_or_else(|| PathBuf::from("testdata/vector-multi.pdf"), PathBuf::from);
        let printer = argv.next().unwrap_or_else(|| DEFAULT_PRINTER.to_owned());
        println!("print-probe");
        println!("  document : {}", source.display());
        println!("  printer  : {printer}");
        println!();
        let mut report = Report {
            passed: 0,
            failed: 0,
        };
        if let Err(e) = run(&source, &printer, &mut report) {
            println!("[FAIL] {e}");
            report.failed += 1;
        }
        println!();
        println!("{} passed, {} failed", report.passed, report.failed);
        if report.failed > 0 {
            std::process::exit(1);
        }
    }

    fn run(source: &Path, printer: &str, report: &mut Report) -> Result<(), String> {
        gtk::init().map_err(|e| format!("GTK could not start, so there is no display: {e}"))?;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("repo root")?;
        let library = root
            .join("vendor")
            .join("pdfium")
            .join(tpdf_lib::PDFIUM_SUBDIR);
        let bindings = progressive::bindings_of(progressive::bind(&library)?);

        let original = std::fs::read(source).map_err(|e| format!("reading {source:?}: {e}"))?;
        let available = open(bindings, original.clone())?.page_count();
        let wanted: Vec<u32> = (1..=available.min(2)).collect();
        let expected = wanted.len();
        // A subset and a quarter turn, so the job is a rewrite and not the file.
        let job = Job {
            pages: Pages::Only(
                wanted
                    .iter()
                    .map(|&number| PagePlan { number, turns: 0 })
                    .collect(),
            ),
            turns: 1,
        };
        let bytes =
            print::build_update(&original, &job).map_err(|e| format!("building the job: {e}"))?;
        println!("  pages    : {available} in the source, printing {wanted:?} with a quarter turn");
        println!();

        // The application's own read-back, as `present_job` makes it.
        let (staged, file) = print_linux::stage(&bytes)?;
        let read_back = save::InWorker::at(library.clone()).printed_pages(&file, bytes.len());
        drop(file);
        report.check(
            "the read-back worker finds the pages of the job",
            read_back.as_ref().is_ok_and(|n| *n == expected),
            &format!("{read_back:?}, {expected} expected"),
        );

        let bytes_for_range = bytes.clone();
        let sent = inks(bindings, bytes)?;
        report.check(
            "control: the pages we sent have ink on them",
            !sent.is_empty() && sent.iter().all(|n| *n > 0),
            &format!("non-white pixels per page: {sent:?}"),
        );

        let out = std::env::var_os("TPDF_PRINT_OUT")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("PDF")))
            .ok_or("no folder for cups-pdf's output")?;
        let started = SystemTime::now();
        let printed = print_linux::print_to(
            printer,
            &staged,
            "tpdf print probe",
            None,
            Duration::from_secs(30),
        );
        report.check(
            "GTK sent the job to the CUPS queue",
            printed.is_ok(),
            &format!("{printed:?}"),
        );
        report.check(
            "the job's file is gone once the job let go of it",
            !staged.exists(),
            &staged.display().to_string(),
        );
        printed?;

        let produced = wait_for_output(&out, started, Duration::from_secs(60))?;
        let after = std::fs::read(&produced).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&produced);
        let pages = inks(bindings, after)?;
        report.check(
            "what came out of CUPS has the pages that were sent",
            pages.len() == expected,
            &format!("{} pages at {}", pages.len(), produced.display()),
        );
        report.check(
            "every page that came out has ink, so none is blank",
            !pages.is_empty() && pages.iter().all(|n| *n > 0),
            &format!("non-white pixels per page: {pages:?}"),
        );
        range(bindings, &bytes_for_range, printer, &out, expected, report)
    }

    /// The dialog's page range, through the same `submit` with the settings
    /// the dialog leaves for a range typed into it.
    ///
    /// **This exists because the range was ignored on Linux and the probe could
    /// not see it**: GTK keeps a range on the job, where nothing reads it for a
    /// job sent as a file, so "2-3" printed every page until `print_linux`
    /// handed it to CUPS. **The last page, not the first**, as on Windows: a
    /// range that is ignored prints from the beginning. Which page came out is
    /// read from its text, since CUPS scales the page to the paper and ink
    /// counts change with it; a fixture whose two pages read alike is skipped,
    /// so run it on `testdata/links.pdf` too.
    fn range(
        bindings: Bindings,
        bytes: &[u8],
        printer: &str,
        out: &Path,
        expected: usize,
        report: &mut Report,
    ) -> Result<(), String> {
        const COUNT: &str = "a page range prints only the page it names";
        const WHICH: &str = "a page range prints the page it named, not the first one";
        if expected < 2 {
            println!("[SKIP] {COUNT}: a one-page job has no range to take a subset of");
            return Ok(());
        }
        let last = u32::try_from(expected).map_err(|e| e.to_string())?;
        let (staged, file) = print_linux::stage(bytes)?;
        drop(file);
        let started = SystemTime::now();
        print_linux::print_to(
            printer,
            &staged,
            "tpdf print probe (range)",
            Some((last, last)),
            Duration::from_secs(30),
        )?;
        let produced = wait_for_output(out, started, Duration::from_secs(60))?;
        let after = std::fs::read(&produced).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&produced);
        let printed = open(bindings, after)?;
        report.check(
            COUNT,
            printed.page_count() == 1,
            &format!(
                "asked for page {last} of {expected}, {} came out",
                printed.page_count()
            ),
        );
        let sent = open(bindings, bytes.to_vec())?;
        let (first, wanted) = (text(&sent, 0)?, text(&sent, last - 1)?);
        if printed.page_count() != 1 {
            println!("[SKIP] {WHICH}: the ranged job did not come out as one page");
        } else if first.trim().is_empty() || first == wanted {
            println!("[SKIP] {WHICH}: the first and last pages of this fixture read alike");
        } else {
            // By words rather than by the whole text: the rewrite and CUPS may
            // read a page out in another order. Words only page `last` has must
            // be there, and words only page 1 has must not.
            let got = text(&printed, 0)?;
            let words = |t: &str| t.split(' ').map(str::to_owned).collect::<HashSet<_>>();
            let (got, first_words, wanted_words) = (words(&got), words(&first), words(&wanted));
            let only_wanted: Vec<_> = wanted_words.difference(&first_words).collect();
            let only_first: Vec<_> = first_words.difference(&wanted_words).collect();
            if only_wanted.is_empty() || only_first.is_empty() {
                // Pages with the same words in another order: no reading of
                // the words tells them apart, and a check here could not fail.
                println!(
                    "[SKIP] {WHICH}: the first and last pages of this fixture have the same words"
                );
                return Ok(());
            }
            let has_wanted = only_wanted.iter().filter(|w| got.contains(**w)).count();
            let has_first = only_first.iter().filter(|w| got.contains(**w)).count();
            report.check(
                WHICH,
                has_wanted == only_wanted.len() && has_first == 0,
                &format!(
                    "of the words only page {last} has, {has_wanted}/{} printed; \
                     of those only page 1 has, {has_first}/{}",
                    only_wanted.len(),
                    only_first.len()
                ),
            );
        }
        Ok(())
    }

    /// A page's text, with the whitespace between words made single spaces,
    /// since a rewrite may lay the same words out with other gaps.
    fn text(document: &OpenDocument, index: u32) -> Result<String, String> {
        let codes = document.page_codes(index, None)?;
        let words: String = codes.iter().filter_map(|&c| char::from_u32(c)).collect();
        Ok(words.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    fn open(bindings: Bindings, bytes: Vec<u8>) -> Result<OpenDocument, String> {
        // Leaked: a probe opens a handful of documents and then exits.
        let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        OpenDocument::open_bytes(bindings, bytes, None).map_err(|e| e.reason)
    }

    /// Non-white pixels on each page, at [`CHECK_SCALE`].
    fn inks(bindings: Bindings, bytes: Vec<u8>) -> Result<Vec<usize>, String> {
        let document = open(bindings, bytes)?;
        let mut inks = Vec::new();
        for index in 0..document.page_count() {
            let page = document.page(index)?;
            let width = (page.width_pt() * CHECK_SCALE).ceil().clamp(1.0, 4000.0);
            let height = (page.height_pt() * CHECK_SCALE).ceil().clamp(1.0, 4000.0);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let spec = TileSpec {
                scale: CHECK_SCALE,
                turns: 0,
                x: 0,
                y: 0,
                width: width as u16,
                height: height as u16,
            };
            let (pixels, done) =
                progressive::render_tile(bindings, &page, spec, None, &CancelToken::new())?;
            if !done.outcome.is_done() {
                return Err(format!("page {} did not finish rendering", index + 1));
            }
            inks.push(
                pixels
                    .chunks_exact(4)
                    .filter(|p| p[..3].iter().any(|c| *c < 200))
                    .count(),
            );
        }
        Ok(inks)
    }

    /// The newest PDF in `folder` written after `since`, once its size has
    /// stopped changing: cups-pdf writes it after the job has left GTK.
    fn wait_for_output(
        folder: &Path,
        since: SystemTime,
        within: Duration,
    ) -> Result<PathBuf, String> {
        let until = Instant::now() + within;
        let mut last: Option<(PathBuf, u64)> = None;
        while Instant::now() < until {
            let newest = std::fs::read_dir(folder)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "pdf"))
                .filter_map(|e| {
                    let meta = e.metadata().ok()?;
                    let when = meta.modified().ok()?;
                    (when >= since).then(|| (when, e.path(), meta.len()))
                })
                .max_by_key(|(when, _, _)| *when);
            if let Some((_, path, len)) = newest {
                if len > 0 && last.as_ref() == Some(&(path.clone(), len)) {
                    return Ok(path);
                }
                last = Some((path, len));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(format!(
            "nothing new appeared in {} within {} s; is cups-pdf's queue there and enabled?",
            folder.display(),
            within.as_secs()
        ))
    }
}

#[cfg(windows)]
fn main() {
    // The pages are drawn by a worker, which is this executable started again.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == tpdf_lib::worker::WORKER_ARGV) {
        tpdf_lib::worker_child::main(&args);
    }
    imp::main();
}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};

    use tpdf_lib::print::{self, Job, PagePlan, Pages};
    use tpdf_lib::print_win;
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, HDC};

    /// The printer used when none is named.
    ///
    /// Present on every Windows 10 and 11 installation as an optional feature that
    /// is on by default, and it writes a PDF --- which is what makes its output
    /// readable by the same parser the rest of this checks with.
    const DEFAULT_PRINTER: &str = "Microsoft Print to PDF";

    /// Resolution the *verification* renders at, not the print resolution.
    ///
    /// Low on purpose: this counts ink, and counting it at 300 dpi would mean
    /// twelve megapixels per page to answer a question a thumbnail settles.
    const CHECK_DPI: f32 = 36.0;

    /// Where the worker that draws the pages finds `pdfium.dll`.
    fn library() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|root| {
                root.join("vendor")
                    .join("pdfium")
                    .join(tpdf_lib::PDFIUM_SUBDIR)
            })
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// A check and its outcome, printed as it lands.
    struct Report {
        passed: usize,
        failed: usize,
    }

    impl Report {
        fn new() -> Self {
            Self {
                passed: 0,
                failed: 0,
            }
        }

        /// Records one check. Printed immediately, so a run that dies partway names
        /// the last thing it completed rather than leaving a reader to guess.
        fn check(&mut self, name: &str, ok: bool, detail: &str) {
            if ok {
                self.passed += 1;
                println!("[OK]   {name:<52} {detail}");
            } else {
                self.failed += 1;
                println!("[FAIL] {name:<52} {detail}");
            }
        }

        fn skip(&mut self, name: &str, why: &str) {
            println!("[SKIP] {name:<52} {why}");
        }
    }

    pub fn main() {
        let mut argv = std::env::args().skip(1);
        let source = argv
            .next()
            .map_or_else(|| PathBuf::from("testdata/rotated.pdf"), PathBuf::from);
        let printer = argv.next().unwrap_or_else(|| DEFAULT_PRINTER.to_owned());

        println!("print-probe");
        println!("  document : {}", source.display());
        println!("  printer  : {printer}");
        println!();

        let mut report = Report::new();
        if let Err(e) = run(&source, &printer, &mut report) {
            println!("[FAIL] {e}");
            report.failed += 1;
        }

        println!();
        println!("{} passed, {} failed", report.passed, report.failed);
        if report.failed > 0 {
            std::process::exit(1);
        }
    }

    fn run(source: &Path, printer: &str, report: &mut Report) -> Result<(), String> {
        if !source.exists() {
            return Err(format!(
                "{} is not there; generate the fixtures first (BUILD.md)",
                source.display()
            ));
        }

        // A subset *and* a rotation, so the job is not a passthrough and every part
        // of `print::build` is exercised on the way to the spooler.
        //
        // The subset is sized to the document rather than hardcoded to two pages.
        // It was `Only(vec![1, 2])`, which made every one-page fixture fail with
        // *"page 2 is not in this document"* --- a probe refusing its own input,
        // reported as though the print path were broken. `vector-heavy.pdf` is one
        // A0 page and is exactly the document this most wants to be run on.
        let available = print_win::read(&std::fs::read(source).map_err(|e| e.to_string())?)
            .ok_or("the OS parser refused the source document")?
            .pages
            .len();
        let wanted: Vec<u32> = (1..=available.min(2))
            .map(|n| u32::try_from(n).unwrap_or(1))
            .collect();
        let expected = wanted.len();
        println!(
            "  pages    : {available} in the source, printing {wanted:?} with a quarter turn{}",
            if expected < 2 {
                " (too few to drop one, so this run does not exercise the subset path)"
            } else {
                ""
            }
        );
        println!();
        // `turns: 0` per page, so what reaches the paper is the job's single
        // quarter turn and nothing else --- the probe is about the subset path
        // and the view rotation, and a per-page turn here would compose with
        // that one and make the orientation checks below untestable.
        let job = Job {
            pages: Pages::Only(
                wanted
                    .iter()
                    .map(|&number| PagePlan { number, turns: 0 })
                    .collect(),
            ),
            turns: 1,
        };
        // `print::build_update` rather than `print::build`, which is test-only
        // since 2026-09-01: it takes a *path*, and nothing outside a test may
        // parse the reader's document in this process. Reading the file here is
        // not that --- the bytes go straight to the same pure function a worker
        // runs, and what this probe measures is the spooler downstream of it.
        let original = std::fs::read(source).map_err(|e| format!("reading {source:?}: {e}"))?;
        let bytes =
            print::build_update(&original, &job).map_err(|e| format!("building the job: {e}"))?;

        let reading = print_win::read(&bytes)
            .ok_or("the OS parser refused the job we built, before any printing")?;
        report.check(
            "the job we built is readable by the OS parser",
            reading.pages.len() == expected,
            &format!("{} pages", reading.pages.len()),
        );

        // The control, and it comes first because everything after it is a
        // comparison against it. Source pages with no ink would make the printed
        // pages' ink unfalsifiable.
        let mut sent = Vec::new();
        for index in 0..u32::try_from(reading.pages.len()).unwrap_or(0) {
            let bmp = print_win::render_page(&bytes, index, CHECK_DPI)
                .map_err(|e| format!("rendering source page {index}: {e}"))?;
            sent.push(measure(&bmp)?);
        }
        report.check(
            "control: the pages we sent have ink on them",
            sent.iter().all(|p| p.inked > 0) && !sent.is_empty(),
            &format!(
                "non-white pixels per page: {:?}",
                sent.iter().map(|p| p.inked).collect::<Vec<_>>()
            ),
        );

        let out = std::env::temp_dir().join(format!("tpdf-print-probe-{}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&out);

        let dc = open_printer(printer)?;
        let every: Vec<u32> = (0..u32::try_from(expected).unwrap_or(0)).collect();
        let spooled = stuck_fails(|beat| {
            print_win::spool(
                dc,
                &bytes,
                "tpdf print probe",
                Some(&out.to_string_lossy()),
                &every,
                Some(beat),
                &library(),
            )
        });
        // SAFETY: the DC `open_printer` created, released once, on both paths.
        let _ = unsafe { DeleteDC(dc) };
        let spooled = spooled.map_err(|e| format!("spooling: {e}"))?;
        report.check(
            "the spooler accepted every page",
            spooled as usize == expected,
            &format!("{spooled} pages spooled"),
        );

        // The driver writes asynchronously; a file that is not there yet is not a
        // file that will never be there.
        let printed = wait_for_file(&out, std::time::Duration::from_secs(30))?;
        report.check(
            "the printer produced a file",
            !printed.is_empty(),
            &format!("{} bytes at {}", printed.len(), out.display()),
        );

        // The strongest single check here: what came out of a *driver* is read by
        // the OS parser. Neither `lopdf` nor PDFium is anywhere in that chain.
        let after = print_win::read(&printed)
            .ok_or("the OS parser could not read what the printer produced")?;
        report.check(
            "the printed output has the pages that were sent",
            after.pages.len() == expected,
            &format!("{} pages", after.pages.len()),
        );

        if after.pages.len() == expected {
            let printed = printed_pages(&printed, expected)?;
            report.check(
                "every printed page has ink, so none of them came out blank",
                printed.iter().all(|p| p.inked > 0) && !printed.is_empty(),
                &format!(
                    "non-white pixels per page: {:?}",
                    printed.iter().map(|p| p.inked).collect::<Vec<_>>()
                ),
            );
            // **Extent against a prediction, not amount and not a fixed threshold.**
            // This began as printed-ink versus sent-ink, which is not scale-invariant:
            // it read `0.49` on pages printed at *half physical size* and passed, and
            // `0.01` on one A0 page for no reason but the paper being 16x smaller in
            // area. Replacing it with "ink spans > 0.7 of the sheet" then failed
            // `rotated.pdf` for being **correct** --- this path prints a page at its
            // true size and only shrinks it to fit, so a small page occupying a third
            // of an A4 sheet is the right answer. `predict` is the version that holds
            // for both; see it for why it is independent of `draw_bmp`.
            let want: Vec<(f64, f64)> = sent
                .iter()
                .zip(&printed)
                .map(|(a, b)| predict(a, b))
                .collect();
            let off: Vec<f64> = printed
                .iter()
                .zip(&want)
                .map(|(got, (w, h))| {
                    ((got.width_span - w).abs() / w.max(0.01))
                        .max((got.height_span - h).abs() / h.max(0.01))
                })
                .collect();
            report.check(
                "printed ink lands where the page geometry says it should",
                off.iter().all(|d| *d < 0.25) && !off.is_empty(),
                &format!(
                    "got {}, predicted {} (worst axis off by {})",
                    extents(&printed),
                    want.iter()
                        .map(|(w, h)| format!("{w:.2}x{h:.2}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    off.iter()
                        .map(|d| format!("{:.0}%", d * 100.0))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            );
            // Kept as a printed observation rather than a check: it is real information
            // about how much fine detail a raster print path loses, and it has no
            // threshold --- it depends on the paper size, which is the reason it failed
            // as an assertion.
            let ratios: Vec<f64> = sent
                .iter()
                .zip(&printed)
                .map(|(a, b)| f64::from(b.inked) / f64::from(a.inked.max(1)))
                .collect();
            println!(
                "       {:<52} {ratios:.3?}",
                "... printed/sent ink, for information"
            );
        } else {
            report.skip(
                "printed pages have ink",
                "the page count is already wrong, so per-page ink would not be about ink",
            );
        }

        range(&bytes, printer, expected, &sent, report)?;

        per_sheet(&bytes, printer, expected, report)?;

        turned(&bytes, printer, expected, &sent, report)?;

        marks(printer, report)?;

        stop(&bytes, printer, expected, report)?;

        boundary(report);

        let _ = std::fs::remove_file(&out);
        Ok(())
    }

    /// The panel's page range, driven through the spooler without the panel.
    ///
    /// **This exists because the Pages field was disabled on Windows and nobody
    /// could see it.** `PRINTDLGW` disables the Pages radio whenever `nMinPage`
    /// equals `nMaxPage`, both defaulted to zero, and no check anywhere reached
    /// the dialog --- so the platform where a reader could not print a page range
    /// was also the platform where nothing said so.
    ///
    /// What is verifiable without a person is the half that decides what reaches
    /// paper: `print::sheets` turns a range into indices and `spool` prints those
    /// and no others. The dialog itself is not driven here and cannot be --- see
    /// the note in `BUILD.md` about what that leaves untested.
    ///
    /// **The last sheet, not the first**, because a loop that ignores its range
    /// prints from the beginning: asking for the last one is the request whose
    /// wrong answer is a *different* page rather than the same one.
    fn range(
        bytes: &[u8],
        printer: &str,
        expected: usize,
        sent: &[Page],
        report: &mut Report,
    ) -> Result<(), String> {
        const COUNT: &str = "a page range spools only the sheets it names";
        const WHICH: &str = "a page range spools the sheet it named, not the first one";
        if expected < 2 {
            report.skip(COUNT, "a one-sheet job has no range to take a subset of");
            report.skip(WHICH, "a one-sheet job has no range to take a subset of");
            return Ok(());
        }
        let last = u32::try_from(expected).unwrap_or(1);
        let sheets = print::sheets(Some((last, last)), last)
            .map_err(|e| format!("resolving the range: {e}"))?;

        let out =
            std::env::temp_dir().join(format!("tpdf-print-probe-range-{}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let dc = open_printer(printer)?;
        let spooled = stuck_fails(|beat| {
            print_win::spool(
                dc,
                bytes,
                "tpdf print probe (range)",
                Some(&out.to_string_lossy()),
                &sheets,
                Some(beat),
                &library(),
            )
        });
        // SAFETY: the DC `open_printer` created, released once, on both paths.
        let _ = unsafe { DeleteDC(dc) };
        let spooled = spooled.map_err(|e| format!("spooling the range: {e}"))?;

        let printed = wait_for_file(&out, std::time::Duration::from_secs(30))?;
        let after =
            print_win::read(&printed).ok_or("the OS parser could not read the ranged output")?;
        report.check(
            COUNT,
            spooled == 1 && after.pages.len() == 1,
            &format!(
                "asked for sheet {last} of {expected}, spooled {spooled}, printed {}",
                after.pages.len()
            ),
        );

        // Which sheet it was. A count of one is equally satisfied by the *first*
        // page, which is what a loop ignoring its range would produce, so the
        // reading has to be one only this sheet could give --- and that needs the
        // two candidates to be measurably unalike. Where they are not, this says
        // so rather than passing on a comparison that cannot fail.
        let (first, wanted) = (&sent[0], &sent[expected - 1]);
        let apart = (first.width_span - wanted.width_span)
            .abs()
            .max((first.height_span - wanted.height_span).abs());
        if after.pages.len() != 1 {
            report.skip(WHICH, "the ranged job did not come out as one page");
        } else if apart < 0.05 {
            report.skip(
                WHICH,
                "the first and last sheets of this fixture measure alike, so no reading tells them apart",
            );
        } else {
            let got = printed_pages(&printed, 1)?;
            let ink = got
                .first()
                .ok_or("the ranged output had no page to measure")?;
            let (w, h) = predict(wanted, ink);
            let off = ((ink.width_span - w).abs() / w.max(0.01))
                .max((ink.height_span - h).abs() / h.max(0.01));
            report.check(
                WHICH,
                off < 0.25,
                &format!(
                    "printed {:.2}x{:.2}, sheet {last} predicts {w:.2}x{h:.2} (off by {:.0}%), \
                     sheet 1 measures {:.2}x{:.2}",
                    ink.width_span,
                    ink.height_span,
                    off * 100.0,
                    first.width_span,
                    first.height_span,
                ),
            );
        }

        let _ = std::fs::remove_file(&out);
        Ok(())
    }

    /// Spools `pages` of `bytes` laid out as `layout` says, to a file, and
    /// returns what the printer wrote.
    fn print_laid_out(
        bytes: &[u8],
        printer: &str,
        name: &str,
        pages: &[u32],
        layout: tpdf_lib::print_layout::Layout,
        marks: bool,
    ) -> Result<Vec<u8>, String> {
        let out = std::env::temp_dir().join(format!(
            "tpdf-print-probe-{name}-{}.pdf",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&out);
        let dc = open_printer(printer)?;
        let spooled = stuck_fails(|beat| {
            print_win::spool_laid_out(
                dc,
                bytes,
                &format!("tpdf print probe ({name})"),
                Some(&out.to_string_lossy()),
                pages,
                layout,
                marks,
                Some(beat),
                &library(),
            )
        });
        // SAFETY: the DC `open_printer` created, released once, on both paths.
        let _ = unsafe { DeleteDC(dc) };
        spooled.map_err(|e| format!("spooling {name}: {e}"))?;
        let printed = wait_for_file(&out, std::time::Duration::from_secs(60))?;
        let _ = std::fs::remove_file(&out);
        Ok(printed)
    }

    /// Ink in each quarter of a rendered sheet: top left, top right, bottom
    /// left, bottom right.
    fn quarters(bmp: &[u8]) -> Result<[u32; 4], String> {
        let raster = print_win::Raster::of(bmp)?;
        let (width, height) = (raster.width(), raster.height());
        let mut ink = [0u32; 4];
        for y in 0..height {
            for x in 0..width {
                if raster.inked(x, y) {
                    ink[usize::from(y >= height / 2) * 2 + usize::from(x >= width / 2)] += 1;
                }
            }
        }
        Ok(ink)
    }

    /// Four pages to a sheet: the job's pages land on one sheet, each in its
    /// own quarter, in reading order, and the quarters with no page stay white.
    ///
    /// **Which quarters, not how many sheets.** A layout that drew every page
    /// over the first cell, or that ignored `per_sheet` and drew one page
    /// filling the sheet, would print one sheet with ink on it too; the
    /// empty quarters are what only the right layout leaves empty.
    fn per_sheet(
        bytes: &[u8],
        printer: &str,
        expected: usize,
        report: &mut Report,
    ) -> Result<(), String> {
        const NAME: &str = "four to a sheet puts each page in its own quarter";
        let count = expected.min(4);
        let pages: Vec<u32> = (0..u32::try_from(count).unwrap_or(0)).collect();
        let layout = tpdf_lib::print_layout::Layout {
            per_sheet: 4,
            ..tpdf_lib::print_layout::Layout::PLAIN
        };
        let printed = print_laid_out(bytes, printer, "four", &pages, layout, true)?;
        let after =
            print_win::read(&printed).ok_or("the OS parser could not read the 4-up output")?;
        if after.pages.len() != 1 {
            report.check(
                NAME,
                false,
                &format!("{} sheets, not one", after.pages.len()),
            );
            return Ok(());
        }
        let bmp = print_win::render_page(&printed, 0, CHECK_DPI)
            .map_err(|e| format!("rendering the 4-up sheet: {e}"))?;
        let ink = quarters(&bmp)?;
        // The printable area is not the sheet, so a quarter of it is not
        // exactly a quarter of the sheet; the gap between cells keeps a page
        // inside its own quarter of either.
        let right = ink
            .iter()
            .enumerate()
            .all(|(quarter, &n)| (quarter < count) == (n > 0));
        report.check(
            NAME,
            right,
            &format!("{count} pages, ink per quarter {ink:?}"),
        );
        Ok(())
    }

    /// Turning pages to fit: a landscape page on a portrait sheet prints a
    /// quarter turn round, its ink landing where the *turned* page predicts.
    ///
    /// **By the page's shape, not its ink's.** The first version of this
    /// check expected every sheet's ink to run tall, and failed on
    /// `rotated.pdf`, whose first page is landscape after the job's quarter
    /// turn while its text block runs tall: the page turned, correctly, and
    /// its ink came out wide. `predict` with the page's spans swapped is the
    /// geometry of the turned page, and an unturned page misses it on both
    /// axes wherever its ink is not square.
    fn turned(
        bytes: &[u8],
        printer: &str,
        expected: usize,
        sent: &[Page],
        report: &mut Report,
    ) -> Result<(), String> {
        const NAME: &str = "a landscape page turns to fit a portrait sheet";
        let wide: Vec<bool> = sent
            .iter()
            .map(|p| {
                let (w, h) = p.inches();
                w > h
            })
            .collect();
        if !wide.contains(&true) {
            report.skip(NAME, "no page of this job is landscape, so nothing turns");
            return Ok(());
        }
        let pages: Vec<u32> = (0..u32::try_from(expected).unwrap_or(0)).collect();
        let turning = tpdf_lib::print_layout::Layout {
            rotate: true,
            ..tpdf_lib::print_layout::Layout::PLAIN
        };
        let printed = print_laid_out(bytes, printer, "turned", &pages, turning, true)?;
        let printed = printed_pages(&printed, expected)?;
        let off: Vec<f64> = sent
            .iter()
            .zip(&wide)
            .zip(&printed)
            .map(|((source, &turn), got)| {
                let source = if turn {
                    Page {
                        inked: source.inked,
                        width_span: source.height_span,
                        height_span: source.width_span,
                        width_px: source.height_px,
                        height_px: source.width_px,
                    }
                } else {
                    Page { ..*source }
                };
                let (w, h) = predict(&source, got);
                ((got.width_span - w).abs() / w.max(0.01))
                    .max((got.height_span - h).abs() / h.max(0.01))
            })
            .collect();
        report.check(
            NAME,
            off.iter().all(|d| *d < 0.25) && off.len() == expected,
            &format!(
                "landscape per page {wide:?}, printed {}, worst axis off the turned geometry by {}",
                extents(&printed),
                off.iter()
                    .map(|d| format!("{:.0}%", d * 100.0))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
        Ok(())
    }

    /// Leaving the marks out: the comments of `comments.pdf` do not print,
    /// and the value of `form.pdf`'s field, which only the form overlay draws,
    /// still does.
    ///
    /// **The form is the control.** A render that dropped everything but the
    /// page's own content would pass the first half; the field's value has no
    /// appearance stream, so it survives only if `FPDF_FFLDraw` still runs.
    ///
    /// **Pixels that changed, not the amount of ink.** The comments are about
    /// a twentieth of `comments.pdf`'s ink and the text is the rest, so a
    /// ratio of the totals read 0.945 with every comment gone (2026-10-20,
    /// checked by eye). What leaving the marks out may do is take ink away and
    /// never add it, so the sheets are compared pixel for pixel: the pixels
    /// only the sheet with marks has, and the ones only the other has.
    /// Stop, on a live job: a print stopped while it is drawn ends as
    /// stopped, and the printer writes nothing, because the guard abandons
    /// the job with `AbortDoc` rather than ending it.
    ///
    /// Stop is pressed over and over from another thread for as long as the
    /// print runs, since a print clears a Stop pressed before it began.
    fn stop(
        bytes: &[u8],
        printer: &str,
        expected: usize,
        report: &mut Report,
    ) -> Result<(), String> {
        const NAME: &str = "a stopped print ends as stopped and prints nothing";
        let out =
            std::env::temp_dir().join(format!("tpdf-print-probe-stop-{}.pdf", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let pages: Vec<u32> = (0..u32::try_from(expected).unwrap_or(0)).collect();
        let done = std::sync::atomic::AtomicBool::new(false);
        let dc = open_printer(printer)?;
        let spooled = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(std::sync::atomic::Ordering::SeqCst) {
                    print_win::stop();
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            });
            let spooled = print_win::spool_laid_out(
                dc,
                bytes,
                "tpdf print probe (stop)",
                Some(&out.to_string_lossy()),
                &pages,
                tpdf_lib::print_layout::Layout::PLAIN,
                true,
                None,
                &library(),
            );
            done.store(true, std::sync::atomic::Ordering::SeqCst);
            spooled
        });
        // SAFETY: the DC `open_printer` created, released once.
        let _ = unsafe { DeleteDC(dc) };
        // Time for a driver that was going to write the file to have started.
        std::thread::sleep(std::time::Duration::from_secs(3));
        let written = out.exists();
        let _ = std::fs::remove_file(&out);
        report.check(
            NAME,
            matches!(&spooled, Err(e) if e == print_win::STOPPED) && !written,
            &format!("{spooled:?}, file written: {written}"),
        );
        Ok(())
    }

    fn marks(printer: &str, report: &mut Report) -> Result<(), String> {
        const GONE: &str = "printing without marks leaves the comments out";
        const KEPT: &str = "printing without marks still prints the form's values";
        let layout = tpdf_lib::print_layout::Layout::PLAIN;
        let sheet = |file: &str, marks: bool| -> Result<Vec<u8>, String> {
            let bytes = std::fs::read(file).map_err(|e| format!("reading {file}: {e}"))?;
            let printed = print_laid_out(&bytes, printer, "marks", &[0], layout, marks)?;
            print_win::render_page(&printed, 0, CHECK_DPI)
                .map_err(|e| format!("rendering {file} printed: {e}"))
        };
        for (file, name) in [("testdata/comments.pdf", GONE), ("testdata/form.pdf", KEPT)] {
            if !Path::new(file).exists() {
                report.skip(
                    name,
                    &format!("{file} is not there; generate the fixtures first"),
                );
                continue;
            }
            let (with, without) = (sheet(file, true)?, sheet(file, false)?);
            let (with, without) = (
                print_win::Raster::of(&with)?,
                print_win::Raster::of(&without)?,
            );
            if (with.width(), with.height()) != (without.width(), without.height()) {
                report.check(name, false, "the two sheets came out different sizes");
                continue;
            }
            let (mut inked, mut taken, mut added) = (0u32, 0u32, 0u32);
            for y in 0..with.height() {
                for x in 0..with.width() {
                    let (a, b) = (with.inked(x, y), without.inked(x, y));
                    inked += u32::from(a);
                    taken += u32::from(a && !b);
                    added += u32::from(b && !a);
                }
            }
            // A few pixels either way are the rasteriser's, at a glyph edge.
            let ok = inked > 0
                && added < 50
                && if name == GONE {
                    taken > 500
                } else {
                    taken < 50
                };
            report.check(
                name,
                ok,
                &format!(
                    "{file}: {inked} inked with marks, {taken} of them gone without, {added} added"
                ),
            );
        }
        Ok(())
    }

    /// What this process has mapped after parsing, rendering and printing a PDF.
    ///
    /// **The honest complication in `print_win.rs`'s "no PDFium in this process"
    /// claim, measured rather than argued.** Printing parses a document *in the app
    /// process* on both platforms --- PDFKit on macOS, `Windows.Data.Pdf` here ---
    /// so a PDF parser is mapped in, and pretending otherwise would be the kind of
    /// half-true containment claim `docs/THREAT-MODEL.md` exists to prevent. What
    /// the boundary actually buys is narrower and worth stating exactly: the process
    /// holding the print job never maps **our** PDFium, so a PDFium bug reachable
    /// from a crafted document cannot be reached through printing, and the parser
    /// that *is* mapped is a Microsoft component patched by Windows Update rather
    /// than a library pinned in our `Cargo.lock`.
    ///
    /// Read from the OS's module table by the same Toolhelp route `backend_probe`
    /// uses, and the total is printed beside the verdict for the reason recorded in
    /// `AGENTS.md`: an enumeration that returned nothing looks exactly like an
    /// absence, so a count is what tells the two apart.
    fn boundary(report: &mut Report) {
        let modules = mapped_images();
        if modules.is_empty() {
            report.check(
                "the printing process never mapped our PDF parser",
                false,
                "the module table could not be read, so nothing is established",
            );
            return;
        }
        let ours: Vec<&String> = modules
            .iter()
            .filter(|m| m.to_ascii_lowercase().contains("pdfium"))
            .collect();
        report.check(
            "the printing process never mapped our PDF parser",
            ours.is_empty(),
            &format!(
                "{} modules mapped, none named pdfium{}",
                modules.len(),
                if ours.is_empty() {
                    String::new()
                } else {
                    format!(" --- found {ours:?}")
                }
            ),
        );
        // Not a check, because there is no threshold to assert: it is the other half
        // of the sentence above, and leaving it out would let "no PDFium" be read as
        // "no PDF parser".
        let os: Vec<String> = modules
            .iter()
            .filter_map(|m| {
                let name = m.rsplit('\\').next().unwrap_or(m);
                name.to_ascii_lowercase()
                    .contains("pdf")
                    .then(|| name.to_owned())
            })
            .collect();
        println!(
            "       {:<52} {}",
            "... the OS PDF component it maps instead",
            if os.is_empty() {
                "none yet: WinRT activation is lazy".to_owned()
            } else {
                os.join(", ")
            }
        );
    }

    /// Every module mapped into this process, by full path.
    ///
    /// The same Toolhelp walk as `backend_probe::mapped_images`, and deliberately a
    /// second copy rather than a shared helper: that one lives inside a probe whose
    /// whole purpose is to be the record of a measurement, and `docs/TRAPS.md` notes
    /// that a record which changes when the thing it measured is refactored has
    /// stopped being evidence.
    fn mapped_images() -> Vec<String> {
        use windows::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W,
            TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        };

        // SAFETY: our own pid; the snapshot is closed on every path out.
        let Ok(snapshot) = (unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, std::process::id())
        }) else {
            return Vec::new();
        };
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut found = Vec::new();
        // SAFETY: zeroed is the documented initial state; `dwSize` is set as the API
        // requires and the entry outlives every call it is passed to.
        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = u32::try_from(std::mem::size_of::<MODULEENTRY32W>()).unwrap_or(0);
        // SAFETY: a live snapshot handle and an initialised entry.
        if unsafe { Module32FirstW(snapshot, &raw mut entry) }.is_ok() {
            loop {
                let len = entry
                    .szExePath
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(entry.szExePath.len());
                found.push(String::from_utf16_lossy(&entry.szExePath[..len]));
                // SAFETY: as above.
                if unsafe { Module32NextW(snapshot, &raw mut entry) }.is_err() {
                    break;
                }
            }
        }
        // SAFETY: the snapshot opened above, closed exactly once.
        let _ = unsafe { CloseHandle(snapshot) };
        found
    }

    /// Opens a device context for a printer by name.
    ///
    /// `CreateDCW` with the `WINSPOOL` driver, which is how a DC is obtained
    /// without a dialog. This is the one thing `present` does differently --- it
    /// takes the DC `PD_RETURNDC` hands back --- and everything downstream of it is
    /// the same code.
    fn open_printer(name: &str) -> Result<HDC, String> {
        let driver = HSTRING::from("WINSPOOL");
        let device = HSTRING::from(name);
        // SAFETY: two valid wide strings that outlive the call; the remaining two
        // arguments are documented as optional and null selects the defaults.
        let dc = unsafe { CreateDCW(&driver, &device, None, None) };
        if dc.is_invalid() {
            return Err(format!(
                "could not open printer {name:?}: {}. Named printers on this machine \
                 are listed by `Get-Printer`.",
                std::io::Error::last_os_error()
            ));
        }
        Ok(dc)
    }

    /// Runs a spool under `print::STALL`, and ends the probe with a verdict
    /// when the spooler takes no page for that long: a stuck queue blocks
    /// inside GDI, and until 2026-10-19 the probe waited there until killed.
    fn stuck_fails<T>(work: impl FnOnce(&print::Beat<'_>) -> T) -> T {
        print::watched(
            print::STALL,
            |_| (),
            work,
            |sent| {
                println!(
                    "[FAIL] the spooler took no page for {} s, after {sent} sheet(s): {}",
                    print::STALL.as_secs(),
                    print::stalled_message(sent)
                );
                std::process::exit(1);
            },
        )
    }

    /// Waits for the spooler to finish writing, and returns the bytes.
    ///
    /// Polls for a *stable* size rather than for existence: the driver creates the
    /// file and then fills it, so reading on first sight yields a truncated PDF and
    /// a parser error that looks like a broken print.
    fn wait_for_file(path: &Path, bound: std::time::Duration) -> Result<Vec<u8>, String> {
        let deadline = std::time::Instant::now() + bound;
        let mut last = 0u64;
        let mut stable = 0;
        while std::time::Instant::now() < deadline {
            if let Ok(meta) = std::fs::metadata(path) {
                let size = meta.len();
                if size > 0 && size == last {
                    stable += 1;
                    if stable >= 3 {
                        return std::fs::read(path).map_err(|e| format!("reading the output: {e}"));
                    }
                } else {
                    stable = 0;
                }
                last = size;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Err(format!(
            "the printer produced nothing at {} within {:.0} s",
            path.display(),
            bound.as_secs_f64()
        ))
    }

    /// What one rendered page's ink looks like: how much, and how far it reaches.
    ///
    /// The spans are fractions of the raster's own dimensions, which is what makes
    /// them comparable between a source page and a sheet of a different size --- the
    /// property a raw ink count does not have.
    struct Page {
        inked: u32,
        width_span: f64,
        height_span: f64,
        /// The raster's own size, so physical inches can be derived from `CHECK_DPI`.
        width_px: usize,
        height_px: usize,
    }

    impl Page {
        /// The raster's physical size in inches at the resolution it was rendered at.
        fn inches(&self) -> (f64, f64) {
            #[allow(clippy::cast_precision_loss)]
            (
                self.width_px as f64 / f64::from(CHECK_DPI),
                self.height_px as f64 / f64::from(CHECK_DPI),
            )
        }
    }

    /// `w x h` extents of a page list, for a report line.
    fn extents(pages: &[Page]) -> String {
        pages
            .iter()
            .map(|p| format!("{:.2}x{:.2}", p.width_span, p.height_span))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Where a source page's ink should land on the sheet, as a fraction of it.
    ///
    /// **The prediction that makes the extent check a check.** A fixed threshold does
    /// not work: this path prints a page at its *true physical size* and only scales
    /// down when it does not fit, so a small page legitimately occupies a third of an
    /// A4 sheet and an A0 page legitimately fills it. Asserting "> 0.7" failed on
    /// `rotated.pdf` for being correct.
    ///
    /// Comparing the printed ink extent against the *source* ink extent, scaled by the
    /// page-to-sheet ratio, is the invariant that holds for both --- and margins cancel,
    /// because both sides measure ink rather than page edges. A page with white borders
    /// has a small extent on both sides of the comparison.
    ///
    /// It is not derived from `draw_bmp`: the page's inches come from the source raster
    /// and the sheet's from what the driver produced, both by way of the OS parser, so
    /// the geometry `draw_bmp` is supposed to realise is predicted independently of it.
    /// That is what caught the render-dpi-versus-device-dpi confusion, which put every
    /// page on paper at half size while the old ink-ratio oracle read 0.49 and passed.
    fn predict(source: &Page, sheet: &Page) -> (f64, f64) {
        let (page_w, page_h) = source.inches();
        let (sheet_w, sheet_h) = sheet.inches();
        if page_w <= 0.0 || page_h <= 0.0 || sheet_w <= 0.0 || sheet_h <= 0.0 {
            return (0.0, 0.0);
        }
        // The same "down only, uniform, preserve aspect" rule the drawing follows ---
        // stated here as the expectation rather than read from there.
        let fit = (sheet_w / page_w).min(sheet_h / page_h).min(1.0);
        (
            source.width_span * page_w * fit / sheet_w,
            source.height_span * page_h * fit / sheet_h,
        )
    }

    /// Renders each page of a produced PDF and measures its ink.
    fn printed_pages(pdf: &[u8], count: usize) -> Result<Vec<Page>, String> {
        let mut pages = Vec::with_capacity(count);
        for index in 0..u32::try_from(count).unwrap_or(0) {
            let bmp = print_win::render_page(pdf, index, CHECK_DPI)
                .map_err(|e| format!("rendering printed page {index}: {e}"))?;
            pages.push(measure(&bmp)?);
        }
        Ok(pages)
    }

    /// Non-white pixels in a rendered BMP, and the bounding box they occupy.
    ///
    /// **The decoding is `print_win::Raster`, not a reader of its own.** This
    /// function had one, written from the header offsets by hand, and it was a
    /// second parser of the same bytes with the same stride and row-order rules to
    /// get right --- a second thing to drift, which this repository has more than
    /// one entry about. `Raster` is built on `parse_bmp`, which already refuses
    /// every malformed header both would have had to, and its `y` counts from the
    /// top so the bottom-up flip is decided once rather than per caller.
    fn measure(bmp: &[u8]) -> Result<Page, String> {
        let raster = print_win::Raster::of(bmp)?;
        let (width, height) = (raster.width(), raster.height());

        let mut inked = 0u32;
        // Deliberately initialised inverted, so that a page with no ink at all leaves
        // them crossed and produces a zero span rather than a full-page one. A blank
        // page reporting "ink spans the whole sheet" is the one wrong answer that
        // would make the check above unfalsifiable.
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (width, 0usize, height, 0usize);
        for y in 0..height {
            for x in 0..width {
                if raster.inked(x, y) {
                    inked += 1;
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let span = |lo: usize, hi: usize, total: usize| -> f64 {
            if inked == 0 || total == 0 {
                0.0
            } else {
                (hi + 1 - lo) as f64 / total as f64
            }
        };
        Ok(Page {
            inked,
            width_span: span(min_x, max_x, width),
            height_span: span(min_y, max_y, height),
            width_px: width,
            height_px: height,
        })
    }
}
