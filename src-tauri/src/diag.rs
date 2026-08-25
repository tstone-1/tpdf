//! Diagnostics that outlive the run.
//!
//! Everything the shipped application says about itself --- a worker killed on
//! its deadline, a crashed worker replaced under a reader who saw nothing, a
//! pre-spawn that failed, a print that never presented --- was an `eprintln!`
//! and nothing else. A GUI process started by double-clicking a document has no
//! stderr, so the lines this codebase words most carefully were exactly the ones
//! a user could never send back: "printing did nothing" and "my document
//! flickers" arrive with no evidence, and the second of those is a
//! crash-replacement loop.
//!
//! Four decisions here are deliberate, and all four look like omissions.
//!
//! **This is a second sink, not a redirect.** Every wired site still writes the
//! bytes it wrote before to stderr, unchanged, because stderr is the channel the
//! harnesses read --- `viewer_check.py`, `worker-probe` and `backend-probe` all
//! capture it, and several parse it. A logging change that moved a line off
//! stderr would be a silent regression in checks that have nothing to do with
//! logging. The file gets a copy with a timestamp in front of it.
//!
//! **One `write_all` of one `String` per line, under a lock.** Rust's stderr is
//! unbuffered and `write_fmt` issues a separate write per format piece, so
//! `eprintln!` is several writes that anything sharing the handle can interleave
//! between --- `docs/TRAPS.md` records a bare `[worker] ` fragment that read as a
//! worker dying with an empty reason, which is a plausible bug report about
//! something that never happened. The same hazard applies to the file with
//! several threads in one process, and the lock is what answers it there.
//!
//! **A failed append is swallowed, and counted.** A diagnostics channel that can
//! fail a request, or recurse into itself trying to report its own failure, is
//! worse than no channel at all. So the error is dropped --- but silently
//! dropping it would leave a file with a hole in it that reads as a quiet
//! period, so the count of lines the file did not take is carried and written
//! out ahead of the next line that succeeds. A gap is then visible as a gap.
//!
//! **The worker children never start a sink**, and nothing here enforces that
//! beyond where `start` is called from: a worker is this executable re-exec'd
//! with a marker argument and `worker_child::main` never returns, so it does not
//! reach the setup hook. A worker's own dying words still go to the stderr it
//! inherited from the parent, and on a GUI launch that is still nowhere. Routing
//! those needs the parent to read the children's pipes, which is a larger change
//! than this one --- see residual risk 13 in `docs/THREAT-MODEL.md`.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

/// Bytes the current file may reach before it is rotated.
///
/// With the one kept previous file, the whole channel is bounded at twice this.
/// Small on purpose: what it holds is one line per worker death, deadline kill
/// or failed print, so a quarter of a megabyte is thousands of them --- far more
/// than any session produces, and still nothing a user would notice on disk.
pub const MAX_BYTES: u64 = 256 * 1024;

/// Where the diagnostics go, once the application has resolved a place.
///
/// Unset in every other process that runs this code: worker children, the
/// benchmarks, the probes, and the unit tests. There the sink is absent and
/// [`note`] is exactly the `eprintln!` it replaced.
static SINK: OnceLock<Sink> = OnceLock::new();

/// Starts persisting diagnostics to `path`.
///
/// First caller wins and later ones are ignored rather than refused: there is
/// one application per process, and a second call would be a bug in the caller
/// rather than something a diagnostics channel should have an opinion about.
pub fn start(path: PathBuf) {
    let _ = SINK.set(Sink::new(path));
}

/// Says one already-formatted line, on stderr and --- if a sink was started ---
/// in the file.
///
/// Takes the finished line rather than a format string, so the formatting
/// happens once at the call site and what reaches both sinks is the same
/// `String`. See the module comment on why that matters for stderr.
pub fn note(line: &str) {
    note_to(SINK.get(), line);
}

/// [`note`], with the sink named rather than looked up.
///
/// Split out so the no-sink case is testable: `SINK` is a `OnceLock` and can be
/// set once per process, so a test that asserted "nothing is written before
/// `start`" through the global would be a test whose result depended on which
/// other test in the binary ran first.
fn note_to(sink: Option<&Sink>, line: &str) {
    to_stderr(line);
    if let Some(sink) = sink {
        sink.append(line);
    }
}

/// The line on stderr, in one write, byte for byte as `eprintln!` wrote it.
///
/// Not `eprintln!`, for the interleaving reason in the module comment --- and
/// the error is dropped rather than panicked on, which `eprintln!` does. A GUI
/// process whose stderr has been closed must not be taken down by a diagnostic.
fn to_stderr(line: &str) {
    let mut out = String::with_capacity(line.len() + 1);
    out.push_str(line);
    out.push('\n');
    let _ = io::stderr().write_all(out.as_bytes());
}

/// Lines the file did not take since the last one it did.
#[derive(Default)]
struct State {
    dropped: u64,
}

/// A bounded file that several threads may append whole lines to.
struct Sink {
    /// The current file. Its predecessor sits beside it as `<name>.old`.
    path: PathBuf,
    /// Bytes the current file may reach before rotation. [`MAX_BYTES`] in the
    /// application; a test builds a sink with a small one so that rotation is
    /// reachable without writing a quarter of a megabyte to prove it.
    cap: u64,
    /// What serializes the append. A `parking_lot::Mutex` rather than the
    /// standard one because it does not poison: a panic anywhere near this would
    /// otherwise disable logging for the rest of the run, silently, at exactly
    /// the moment there is something to log.
    state: Mutex<State>,
    /// A test's hook, run between the two halves of a line.
    ///
    /// It exists because the lock is otherwise not provable. A whole line is one
    /// `write_all` to a file opened for append, and the kernel makes that atomic
    /// on both platforms --- so removing the lock would leave the lines intact
    /// and the test green, which is a check that cannot fail. Splitting the
    /// write is what makes the lock the only thing keeping a line whole.
    #[cfg(test)]
    midline: Option<fn()>,
}

impl Sink {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            cap: MAX_BYTES,
            state: Mutex::new(State::default()),
            #[cfg(test)]
            midline: None,
        }
    }

    /// Appends one line, and swallows every reason it could not.
    fn append(&self, line: &str) {
        let stamped = format!("{} {line}", stamp(SystemTime::now()));
        let mut state = self.state.lock();
        if self.write(&state, &stamped).is_ok() {
            state.dropped = 0;
        } else {
            state.dropped = state.dropped.saturating_add(1);
        }
    }

    /// One line, plus the account of any that were lost before it.
    ///
    /// Both go into a single buffer and a single write, so a failure leaves the
    /// count standing rather than reporting a gap and then falling into it.
    fn write(&self, state: &State, stamped: &str) -> io::Result<()> {
        let mut buffer = String::new();
        if state.dropped > 0 {
            buffer.push_str(&format!(
                "{} [diag] {} line(s) could not be written\n",
                stamp(SystemTime::now()),
                state.dropped
            ));
        }
        buffer.push_str(stamped);
        buffer.push('\n');

        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                fs::create_dir_all(dir)?;
            }
        }
        self.rotate_if_full(u64::try_from(buffer.len()).unwrap_or(u64::MAX));

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.write_pieces(&mut file, buffer.as_bytes())
    }

    /// Renames the current file out of the way when the next line would not fit.
    ///
    /// Best effort, and deliberately not an error: a rotation that fails leaves
    /// the file one line over its bound and tries again on the next line, where
    /// treating it as a write failure would throw away the diagnostic in hand to
    /// enforce a size limit.
    ///
    /// The size is read from the filesystem rather than counted, so a file
    /// truncated or replaced underneath the process is measured as it is.
    fn rotate_if_full(&self, incoming: u64) {
        let Ok(meta) = fs::metadata(&self.path) else {
            return;
        };
        if meta.len() > 0 && meta.len().saturating_add(incoming) > self.cap {
            let _ = fs::rename(&self.path, previous(&self.path));
        }
    }

    /// The bytes, in one write --- or in two when a test has asked for two.
    ///
    /// See [`Sink::midline`] for why the seam exists. Nothing outside the tests
    /// compiles the split arm.
    fn write_pieces(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        if let Some(midline) = self.midline {
            let (head, tail) = bytes.split_at(bytes.len() / 2);
            file.write_all(head)?;
            midline();
            return file.write_all(tail);
        }
        file.write_all(bytes)
    }
}

/// The name the current file is rotated to.
///
/// Built by appending rather than through `with_extension`, which would replace
/// `.log` instead of adding to it --- the same reasoning, and the same shape, as
/// `temp_beside` in `session.rs`.
fn previous(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("tpdf.log"), OsString::from);
    name.push(".old");
    path.with_file_name(name)
}

/// A UTC timestamp for an instant, to the millisecond.
///
/// A clock before the epoch reads as the epoch. That is a machine whose clock is
/// wrong by decades, and refusing to write the diagnostic over it would lose the
/// thing worth having.
fn stamp(at: SystemTime) -> String {
    let ms = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    stamp_ms(u64::try_from(ms).unwrap_or(u64::MAX))
}

/// A UTC timestamp for a count of milliseconds since the epoch.
///
/// UTC and not local time, and written out in full rather than left as an epoch
/// count: the reader of this file is someone comparing it against when they saw
/// something go wrong, possibly in another timezone, and an epoch would make
/// them fetch a converter first.
///
/// No dependency for this. The civil-date arithmetic is Howard Hinnant's
/// days-to-`(y, m, d)`, which is a dozen lines and is pinned by a table of
/// known instants below --- a leap day among them, since that is the one input
/// a wrong version of this gets wrong.
fn stamp_ms(ms: u64) -> String {
    let (year, month, day) = civil_from_days(ms / 86_400_000);
    let rest = ms % 86_400_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        (rest / 60_000) % 60,
        (rest / 1_000) % 60,
        rest % 1_000
    )
}

/// Days since 1970-01-01 to the calendar date, proleptic Gregorian.
///
/// `pub(crate)` for [`crate::save::pdf_date`], which needs the same arithmetic
/// to stamp an annotation. It is a dozen lines and it would have been copied;
/// the table of known instants below --- a leap day among them --- is what pins
/// it, and a second copy would be a second implementation with no table.
pub(crate) fn civil_from_days(days: u64) -> (u64, u64, u64) {
    // Shifted so the era starts on 0000-03-01, which puts the leap day at the
    // end of the year and makes every month length regular.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, note, note_to, previous, stamp_ms, Sink, MAX_BYTES};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::testutil::TempDir;

    /// The log file inside a scratch directory.
    ///
    /// An extension rather than a method on [`TempDir`]: the name `tpdf.log`
    /// belongs to this module, not to a general-purpose temp directory.
    trait LogFile {
        fn file(&self) -> PathBuf;
    }

    impl LogFile for TempDir {
        fn file(&self) -> PathBuf {
            self.join("tpdf.log")
        }
    }

    fn read(path: &PathBuf) -> String {
        std::fs::read_to_string(path).expect("the log is readable")
    }

    /// The part of a written line after the timestamp and its separating space.
    ///
    /// Split on the first space rather than by a fixed offset, because a parser
    /// that depends on a column width breaks the day the format changes and does
    /// it silently --- `AGENTS.md` records that from the other end of a harness.
    fn payload(line: &str) -> &str {
        line.split_once(' ').expect("a stamp and a payload").1
    }

    /// Turns the test below into the child half of itself.
    const HELPER: &str = "TPDF_DIAG_STDERR_HELPER";

    #[test]
    fn the_line_still_reaches_stderr_exactly_as_it_did() {
        // The property the whole design rests on, and the one nothing else here
        // can see: this is a second sink and not a redirect, because stderr is
        // what `viewer_check.py`, `worker-probe` and `backend-probe` capture and
        // several of them parse. A line quietly moved off stderr would be a
        // regression in checks that have nothing to do with logging, and it
        // would show up as a harness finding about something else entirely.
        //
        // Through a child process because there is no way to read this one's
        // stderr from inside it. The child is this same test binary, running
        // this same test, with the variable that makes it take the first branch.
        if std::env::var_os(HELPER).is_some() {
            note("[render] the line a harness greps for");
            return;
        }

        let out = std::process::Command::new(std::env::current_exe().expect("this test binary"))
            // `--nocapture`, or libtest swallows the child's stderr and the
            // check reads as a line that was never written.
            .args([
                "diag::tests::the_line_still_reaches_stderr_exactly_as_it_did",
                "--exact",
                "--nocapture",
            ])
            .env(HELPER, "1")
            .output()
            .expect("the helper ran");

        let said = String::from_utf8_lossy(&out.stderr);
        assert!(
            said.contains("[render] the line a harness greps for\n"),
            "the line did not reach stderr: {said:?}"
        );
    }

    #[test]
    fn a_line_is_appended_with_a_timestamp_in_front_of_the_text_verbatim() {
        let dir = TempDir::new("append");
        let sink = Sink::new(dir.file());
        sink.append("[render] worker 4711: no reply in 30 s; killing it");
        sink.append("[print] the print job could not be read back");

        let written = read(&dir.file());
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 2, "both lines should be there: {written:?}");
        assert_eq!(
            payload(lines[0]),
            "[render] worker 4711: no reply in 30 s; killing it",
            "the existing line must survive verbatim -- the prefixes are the taxonomy"
        );
        assert_eq!(
            payload(lines[1]),
            "[print] the print job could not be read back"
        );
        // And the stamp really is one: a sink that wrote the line with no
        // timestamp would satisfy `payload` above by handing back everything
        // after the first space of the line itself.
        assert!(
            lines[0].starts_with('2') && lines[0][..24].ends_with('Z'),
            "no timestamp in front of {:?}",
            lines[0]
        );
    }

    #[test]
    fn the_timestamp_is_utc_to_the_millisecond() {
        // A table rather than a shape assertion, and a leap day in it: the
        // civil-date arithmetic is the only part of this module that can be
        // wrong on a particular day and right on every other one, so a check on
        // "looks like a date" would pass for a version that is off by one from
        // March onwards in every leap year.
        for (ms, expected) in [
            (0_u64, "1970-01-01T00:00:00.000Z"),
            (1_000, "1970-01-01T00:00:01.000Z"),
            (951_782_400_000, "2000-02-29T00:00:00.000Z"),
            (1_583_020_800_000, "2020-03-01T00:00:00.000Z"),
            (1_754_146_867_311, "2025-08-02T15:01:07.311Z"),
        ] {
            assert_eq!(stamp_ms(ms), expected, "at {ms} ms");
        }
        // The month boundary the shifted era makes regular, taken directly:
        // day 59 is the first of March, so a February counted as 29 days in a
        // non-leap year would land a day early here.
        assert_eq!(civil_from_days(59), (1970, 3, 1));
    }

    #[test]
    fn a_full_file_is_rotated_and_exactly_one_previous_is_kept() {
        let dir = TempDir::new("rotate");
        let mut sink = Sink::new(dir.file());
        // A small cap rather than the real one, so rotation is reachable without
        // writing a quarter of a megabyte. What the cap *is* has its own check
        // below; what this one is about is that the comparison happens at all.
        sink.cap = 200;

        // Six lines of a fixed 41 bytes against a 200-byte cap is exactly one
        // rotation, which is what lets the two halves below be asserted
        // separately: what was carried, and what was kept.
        for n in 0..6 {
            sink.append(&format!("[render] line {n}"));
        }

        assert_eq!(
            dir.names(),
            vec!["tpdf.log".to_string(), "tpdf.log.old".to_string()],
            "rotation should leave exactly two files"
        );
        let current = read(&dir.file());
        let carried = read(&previous(&dir.file()));
        assert!(
            current.contains("[render] line 5") && !current.contains("[render] line 0"),
            "the current file should hold the newest lines: {current:?}"
        );
        assert!(
            carried.contains("[render] line 0") && !carried.contains("[render] line 5"),
            "the previous file should hold the oldest lines: {carried:?}"
        );
        // One rotation loses nothing, which is the difference between rotating
        // and truncating -- a cap enforced by throwing the file away would
        // satisfy every assertion above about the current file.
        for n in 0..6 {
            let line = format!("[render] line {n}");
            assert!(
                current.contains(&line) || carried.contains(&line),
                "{line} survived neither rotation"
            );
        }

        // And it stays bounded across many rotations rather than only the first,
        // which is the property the cap exists for.
        for n in 6..200 {
            sink.append(&format!("[render] line {n}"));
        }
        assert_eq!(dir.names().len(), 2, "a third file appeared");
        for file in [dir.file(), previous(&dir.file())] {
            let len = u64::try_from(read(&file).len()).unwrap_or(u64::MAX);
            assert!(len <= sink.cap, "{file:?} is past the cap at {len} bytes");
        }
    }

    #[test]
    fn the_cap_a_sink_is_built_with_is_the_documented_bound() {
        // The rotation test above runs on a cap of its own, so without this one
        // the shipped bound is asserted nowhere and could be any number at all.
        assert_eq!(Sink::new(PathBuf::from("tpdf.log")).cap, MAX_BYTES);
    }

    /// Set by the first thread once it is halfway through its line.
    static PAUSED: AtomicBool = AtomicBool::new(false);
    /// Set by the second thread once its own line has been written.
    static SECOND_WROTE: AtomicBool = AtomicBool::new(false);
    /// Cleared by the first call, so only one line is ever held open.
    static ARMED: AtomicBool = AtomicBool::new(true);

    /// Holds the first thread mid-line until the second has written, or until a
    /// bound says it never will.
    ///
    /// The bound is what makes the check decide rather than hang: with the lock
    /// in place the second thread *cannot* write, so this waits out its whole
    /// 500 ms exactly once and the run is a pass. Without the lock the second
    /// thread writes immediately and this returns in microseconds, having let it
    /// land in the middle of the first thread's line.
    fn rendezvous() {
        if !ARMED.swap(false, Ordering::SeqCst) {
            return;
        }
        PAUSED.store(true, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_millis(500);
        while !SECOND_WROTE.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::yield_now();
        }
    }

    #[test]
    fn two_threads_cannot_interleave_one_line() {
        let dir = TempDir::new("interleave");
        let mut sink = Sink::new(dir.file());
        sink.midline = Some(rendezvous);
        let sink = Arc::new(sink);

        let first = {
            let sink = Arc::clone(&sink);
            std::thread::spawn(move || sink.append(&"A".repeat(120)))
        };

        let until = Instant::now() + Duration::from_secs(5);
        while !PAUSED.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < until,
                "the first thread never reached the middle of its line"
            );
            std::thread::yield_now();
        }
        sink.append(&"B".repeat(40));
        SECOND_WROTE.store(true, Ordering::SeqCst);
        first.join().expect("the first thread finished");

        let written = read(&dir.file());
        let mut payloads: Vec<String> = written.lines().map(|l| payload(l).to_owned()).collect();
        payloads.sort();
        assert_eq!(
            payloads,
            vec!["A".repeat(120), "B".repeat(40)],
            "a line was written into the middle of another: {written:?}"
        );
    }

    #[test]
    fn nothing_is_written_where_no_sink_has_been_started() {
        // The state every other process running this code is in: the worker
        // children, the probes, the benchmarks and the rest of this test binary.
        // What it rules out is a `note` that falls back to a location of its own
        // -- which would put a file somewhere nobody chose, in processes that
        // were never meant to write one.
        let dir = TempDir::new("unstarted");
        // The text names itself, because `note_to` writes to stderr either way
        // and two unexplained `[render]` lines in the middle of a green suite
        // read as a failure that did not happen.
        let line = "[diag] this line is one test's own output and means nothing";
        note_to(None, line);
        assert_eq!(dir.names(), Vec::<String>::new(), "something was written");

        // The control, and without it the assertion above is satisfied by a
        // `note_to` that never writes at all -- which would disable the whole
        // file sink with nothing going red.
        note_to(Some(&Sink::new(dir.file())), line);
        assert_eq!(dir.names(), vec!["tpdf.log".to_string()]);
    }

    #[test]
    fn an_append_that_failed_is_counted_and_said_out_loud_on_the_next_one() {
        // A gap in a diagnostics file reads as a quiet period, which is the one
        // thing it must not do -- a run with three worker deaths and none of them
        // recorded looks exactly like a healthy run.
        let dir = TempDir::new("dropped");
        // A directory where the file goes: opening it for append fails on both
        // platforms, which is a write failure without needing a full disk.
        std::fs::create_dir_all(dir.file()).expect("an obstruction");
        let sink = Sink::new(dir.file());
        sink.append("[render] lost one");
        sink.append("[render] lost two");
        assert_eq!(sink.state.lock().dropped, 2, "the losses were not counted");

        std::fs::remove_dir(dir.file()).expect("clearing the obstruction");
        sink.append("[render] this one lands");

        let written = read(&dir.file());
        assert!(
            written.contains("[diag] 2 line(s) could not be written"),
            "the gap was not accounted for: {written:?}"
        );
        assert!(written.contains("[render] this one lands"));
        assert_eq!(
            sink.state.lock().dropped,
            0,
            "the count should be cleared once it has been reported"
        );
    }
}
