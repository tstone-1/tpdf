//! The worker-process backend: a pool of sandboxed children per document.
//!
//! Split out of `render.rs`, which was 1,958 lines and held the service, both
//! backends, the pool, the spare slot and the reaper at once. Nothing here
//! changed in the move --- it is the same code, and the control for that claim is
//! the test suite plus `backend-probe`, which exercises this file's laziness,
//! capacity, retirement and crash-replacement against the running program.
//!
//! `render.rs` keeps the service, the `Engine` trait both backends implement, and
//! the in-process control this one is compared against. What lives here is
//! everything that only makes sense once documents are parsed somewhere else:
//!
//! - **The pool.** One document, several worker processes, grown lazily under
//!   contention and given back once the scrolling stops ([`DEFAULT_IDLE`]).
//! - **The spare.** One warmed, documentless worker waiting to be adopted, so the
//!   first worker of the *next* document has already paid the link, the sandbox
//!   and the font walk.
//! - **The bookkeeping that keeps those two honest** --- `spawned` against
//!   `idle`, the sender table, and the reservation taken before a spawn.
//! - **The deadline.** A request that never comes back would otherwise hold a
//!   service thread for the life of the process; [`watch_calls`] kills the
//!   worker holding it, which is what turns a wedge into an error.
//!
//! Three properties are worth keeping in mind when changing anything here.
//!
//! - **Growth is lazy.** A document opens with one worker and gains another only
//!   when a request arrives while the first is busy.
//! - **Dequeue order is still FIFO**, because there is still one channel. Only
//!   *execution* overlaps. That is what lets [`Workers::close`] stay correct by
//!   draining rather than by taking a lock over the whole service.
//! - **No lock is held across a render.** Every critical section here is pool
//!   bookkeeping; the render happens in another process entirely.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::annots::Comments;
use crate::links::Links;
use crate::outline::Outline;
use crate::progressive::{CancelToken, Refusal};
use crate::queue::{Claim, SharedQueue};
use crate::render::{
    dispatch, not_open, open_slot_mut, DocumentInfo, Engine, Job, Tile, TileFormat, TileOutcome,
    TileRequest,
};
use crate::search::PageMatches;
use crate::startup::{mark, since_process_start_ms};
use crate::text::PageText;
use crate::worker::{Reply, Request, Response, Shm, WarmWorker, Worker, WorkerSender};

/// How many workers one document may have, unless `TPDF_POOL` says otherwise.
///
/// Six, because that is where the curve flattens on the workload a viewport
/// actually issues. Measured through this service by `examples/pool_bench.rs`, six
/// 1024-square tiles of the A0 sheet, interleaved across rounds (4P+6E machine):
///
/// | workers | 1 | 2 | 4 | 6 | 8 |
/// |---|---|---|---|---|---|
/// | screenful | 3457--3465 ms | 1800--1868 ms | 1263--1299 ms | 830--837 ms | 843--851 ms |
/// | speedup | 1.00x | 1.92--1.94x | 2.67--2.93x | 4.15--4.18x | 4.07--4.12x |
///
/// Past six there is nothing --- eight is *slower*, by less than the spread, so
/// read it as flat rather than as a cost. Note six is neither the core count
/// (10) nor the performance-core count (4): it is where this workload
/// saturates, and `AGENTS.md` records the earlier mistake of carrying a pool
/// size over from a different one.
///
/// Two runs are quoted as a range rather than one as a number, because the
/// four-worker figure moved 2.67--2.93x between them while six moved 0.03x. A
/// single run would have made that look like a measurement.
///
/// The cost of the number is not CPU: every worker holds its own parse of the
/// document, which `worker-probe` measured at 7.8--48.2 MB depending on the
/// corpus, so a fully grown pool on the A0 sheet is about 290 MB. What makes
/// that affordable is that growth is lazy --- a reader turning one page at a
/// time never has more than one worker --- and that it is given back again once
/// the scrolling stops. See [`DEFAULT_IDLE`].
pub const DEFAULT_POOL: usize = 6;

/// How long a worker may sit idle before it is killed.
///
/// The pool exists for a *burst* --- a screenful of tiles, a fast scroll --- and
/// a burst is over long before the reader is. Without this the peak a session
/// ever reached is what it keeps: `pool-bench --mode retire` measures a grown
/// pool on the A0 sheet and what retiring it gives back, and that is the number
/// this constant is chosen against rather than a feeling about tidiness.
///
/// Thirty seconds is a policy choice and both directions of it cost something
/// real, so it is worth stating which. Shorter gives the memory back sooner and
/// charges the *reader* for it: measured, the first screenful after a retirement
/// costs **+65 ms on 811 ms** on the A0 sheet and **+15 ms on 2.5 ms** on the
/// text corpus --- a spawn and a fresh parse per worker, paid concurrently.
/// Longer keeps processes a reader who has stopped scrolling has no use for.
/// Thirty is past any plausible pause inside one gesture and well short of a
/// coffee break.
///
/// **Retirement is polled, and that is sound here in a way it is not elsewhere.**
/// `AGENTS.md` records that polling a child's footprint cannot see a burst
/// smaller than interval x growth rate --- because the thing being watched is an
/// *event*. Idleness is a *state*: a worker that crossed the threshold is still
/// over it at the next sweep, so a coarse interval delays a retirement and can
/// never miss one.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(30);

/// Workers a document keeps however long it idles.
///
/// One, not zero, and the difference is what a reader feels on coming back to a
/// document they left open. Retiring the last worker would give back its 7.8--48.2
/// MB and charge the next page turn a spawn plus a full re-parse --- the
/// stall being paid at exactly the moment someone is watching. Keeping one still
/// returns **242.5 MB of 289.9** on the A0 sheet, which is 84% of it.
///
/// It also keeps an invariant the rest of this module reads as obvious: an open
/// document has a process holding it. Nothing here would break at zero --- the
/// checkout path spawns from `spawned == 0` and the close drain is trivially
/// satisfied by it --- which is precisely why the reason is written down.
const KEPT_WARM: usize = 1;

/// How many threads serve the job queue, for a given per-document pool.
///
/// **More than the pool, deliberately, and it took a mutation to see why.** With
/// one thread per worker the two bounds in [`Workers::checkout`] --- the capacity
/// ceiling and the wait for a free worker --- are both unreachable: `idle` can
/// only be empty when every worker is checked out, which needs one thread each,
/// so a thread arriving to find none free cannot exist. A mutation removing the
/// ceiling entirely survived every check, because the thread count was silently
/// doing the ceiling's job.
///
/// The spare threads are not there to satisfy a test, though. They are what
/// stops one document starving another: with exactly `pool` threads, six tiles
/// of a slow document occupy every one of them, and a request for a *different*
/// document waits behind a render even though its own workers are idle.
pub(crate) fn service_threads(pool: usize) -> usize {
    pool + 2
}

/// How many workers a document may have.
///
/// # Panics
///
/// Never: an unreadable `TPDF_POOL` falls back to the default rather than
/// refusing, because unlike `TPDF_BACKEND` a wrong value here cannot make two
/// measurements silently incomparable --- the size is reported in every place
/// that reports a speedup.
#[must_use]
pub fn pool_size() -> usize {
    std::env::var("TPDF_POOL")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|size| *size > 0)
        .unwrap_or(DEFAULT_POOL)
}

/// How long a worker may idle, in milliseconds, from `TPDF_IDLE_MS`.
///
/// Zero is **accepted and means zero**: retire at the first sweep. It is not a
/// sentinel for "off", and there deliberately is no spelling for off ---
/// `AGENTS.md` records a field in this repository where a "no value" marker was
/// drawn from the value's own range and collided with a real one the moment the
/// timing was right. A caller that wants no retirement asks for a long timeout,
/// which is a quantity rather than a special case.
///
/// # Panics
///
/// Never: an unreadable value falls back to the default, as `TPDF_POOL` does and
/// for the same reason --- unlike `TPDF_BACKEND`, a wrong value here cannot make
/// two measurements silently incomparable, because every harness that depends on
/// the timeout is handed one explicitly rather than reading it from here.
#[must_use]
pub fn idle_timeout() -> Duration {
    std::env::var("TPDF_IDLE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(DEFAULT_IDLE, Duration::from_millis)
}

/// How long one request may occupy a worker before the worker is killed.
///
/// **This is the per-request CPU bound**, and it exists because nothing else
/// here is one. `AGENTS.md` records that macOS accepts `RLIMIT_CPU` and gives it
/// *lifetime* semantics --- under a 3 s limit a 1.72 s render succeeds and the
/// next dies 1.30 s in --- so it can bound how long a worker lives and cannot
/// bound a request. The parent's own deadline plus a kill is the shape that can,
/// measured at 1.2 ms to kill and reap and 4.8 ms to respawn (spike 0.5).
///
/// Thirty seconds, chosen against what a *legitimate* request costs rather than
/// against what feels responsive. The slowest measured in this repository are an
/// open of the 337 MB scan, which is seconds, and a single tile of the A0 sheet
/// at about 1.5 s; a search or a text extraction is milliseconds. Thirty is more
/// than an order of magnitude above any of them.
///
/// Both directions of the number cost something real. Too short and a document
/// that is merely large cannot be opened at all, with no way for the reader to
/// ask for more time --- the worst failure available here, because it is
/// indistinguishable from a corrupt file. Too long and one pathological page
/// parks a service thread, of which there are `pool + 2`, for that long. The
/// asymmetry is why the default sits far from the fast end.
///
/// What it is **not**: a bound on memory (there is none on macOS --- see
/// `Worker::footprint` and `docs/THREAT-MODEL.md` §T3), and not a cancellation.
/// The worker dies; the work is lost, not paused.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

/// Whether a mapped file has lost bytes the document was relying on.
///
/// Pure, and separate from [`Workers::outlived_its_file`] for one reason: the
/// method around it needs a live pool and a *dead worker* to reach, and a rule
/// that can only be exercised by killing a process is a rule nothing exercises.
/// The arithmetic is the part that can be wrong in a way nobody notices.
///
/// `backing` is `None` where the platform cannot answer --- see
/// [`Shm::backing_len`] on Windows --- and that is deliberately **not** a
/// diagnosis. "I could not look" and "I looked and it shrank" are opposite
/// findings, and treating the first as the second would condemn a document on
/// every ordinary worker crash.
fn shrunk(mapped: u64, backing: Option<u64>) -> Option<String> {
    let backing = backing?;
    // Only *shorter* is a diagnosis. A file that grew is the ordinary case of an
    // incremental save appending a revision, and it takes nothing away: every
    // byte the mapping covers is still there and still says what it said. The
    // reader is looking at the previous revision, which is stale rather than
    // broken --- exactly what a renamed-over file gives them, and that one is
    // not visible here at all, since the mapping's descriptor still names the
    // inode it was made from.
    if backing >= mapped {
        return None;
    }
    Some(format!(
        "{OUTLIVED_MARK}: it was {mapped} bytes when it was opened and is {backing} now, so the \
         part of the document past that point is gone. Open it again to read the new file."
    ))
}

/// Opens every error caused by a document's file being truncated under it.
///
/// One constant with one producer and one consumer, rather than the same phrase
/// written twice: [`Workers::outlived_its_file`] builds the message and
/// `protocol::handle` matches on this to answer **410 Gone** instead of the 400
/// every other failure gets. `AGENTS.md` records what two copies of a
/// distinction cost --- they drift, and a mutation of one survives.
///
/// The frontend deliberately does **not** know this string. It keys on the
/// status code, which is a fact about the protocol rather than a third copy of
/// an English sentence that a reworded message would silently invalidate.
pub const OUTLIVED_MARK: &str = "this file changed on disk while it was open";

/// How long a request may run, in milliseconds, from `TPDF_CALL_MS`.
///
/// Zero is **accepted and means zero**: every outstanding call is overdue at the
/// first sweep. As with `TPDF_IDLE_MS` there is deliberately no spelling for
/// "off" --- a caller that wants no deadline asks for a long one, which is a
/// quantity rather than a special case, and `AGENTS.md` records what a sentinel
/// drawn from a value's own range costs when the timing is right.
///
/// # Panics
///
/// Never: an unreadable value falls back to the default, for the same reason
/// `TPDF_POOL` and `TPDF_IDLE_MS` do.
#[must_use]
pub fn call_deadline() -> Duration {
    std::env::var("TPDF_CALL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(DEFAULT_DEADLINE, Duration::from_millis)
}

/// How often a sweeping thread looks, for a given timeout.
///
/// A quarter of it, so the thing being swept for is acted on between one and
/// one-and-a-quarter timeouts after it became true. Clamped at both ends: the
/// floor keeps a harness's short timeout from spinning a thread, and the ceiling
/// keeps the default's sweep at five seconds rather than seven and a half, which
/// costs nothing --- a sweep is a lock and a subtraction per document.
///
/// **Polling is sound for both callers**, by the argument written out under
/// [`DEFAULT_IDLE`]: what is being swept for is a *state* rather than an event,
/// so a coarse interval delays a kill and can never miss one. An overdue call is
/// a state in exactly the way an idle worker is --- it does not stop being
/// overdue between two samples.
fn sweep_interval(idle_after: Duration) -> Duration {
    (idle_after / 4).clamp(Duration::from_millis(10), Duration::from_secs(5))
}

/// The slot a warmed, documentless worker waits in.
///
/// Shared separately from [`Workers`] so the filling thread needs only this and a
/// library path, rather than a handle to the whole service.
type Spare = Arc<Mutex<SpareSlot>>;

/// A spare worker, and the pid of one that is still warming.
///
/// The pid is recorded the moment the process exists rather than when it becomes
/// usable, and that distinction is load-bearing rather than tidy. A spare is a
/// child process, so anything counting this process's children counts it --- and
/// during the window between `fork` and the readiness notice it would be in the
/// process table while absent from `ready`. `backend-probe` saw exactly that as
/// "2 workers" for a document that had one, which reads as the pool's laziness
/// being broken.
#[derive(Default)]
struct SpareSlot {
    /// Warmed and waiting for a document.
    ///
    /// A `WarmWorker`, and the type is doing work: nothing can be published here
    /// that has not had its readiness line consumed, so the ordering `adopt`
    /// depends on is not a policy this module has to remember.
    ready: Option<WarmWorker>,
    /// Started, not yet warm. Its pid, because there is nothing else to hold.
    warming: Option<u32>,
    /// A spawn is in progress and its pid is not known yet.
    ///
    /// A separate flag rather than a sentinel pid. `AGENTS.md` records a
    /// zero-means-absent field in this repository that collided with a real value
    /// the moment the timing was right, and pid 0 is a real pid.
    ///
    /// It exists because `fork` and "the parent learns the pid" are not the same
    /// instant: `Command::spawn` returns after the child exists, so for a short
    /// window there is a process nothing can name. Anything counting children can
    /// see it, which is how a document with one worker was observed to have two.
    spawning: bool,
}

impl SpareSlot {
    /// Whether every process this slot owns can be named.
    ///
    /// False only inside the fork-to-registration window. An observer counting
    /// child processes should wait for this before drawing conclusions --- not
    /// because the count is wrong, but because it includes a process the slot
    /// cannot yet identify as its own.
    fn settled(&self) -> bool {
        !self.spawning
    }

    /// Every process this slot is responsible for, warm or not.
    fn pids(&self) -> Vec<u32> {
        self.ready
            .as_ref()
            .map(WarmWorker::pid)
            .into_iter()
            .chain(self.warming)
            .collect()
    }
}

/// A request currently inside a worker, and since when.
///
/// The pid is the whole point: the thread that made this entry is blocked in a
/// read on that worker's pipe, so it cannot act on its own timeout, and the
/// supervisor has nothing else to reach the process by. `Worker::pid` says
/// signalling by pid races a reaped child whose number has been reused, and that
/// is exactly right --- what makes it safe *here* is the entry's lifetime.
/// It exists only between the send and the reply, during which the blocked
/// thread owns the `Worker`, and therefore the `Child`, unreaped. The number
/// still names that process, and the entry is gone before the worker can be
/// dropped. Nothing else in this module signals by pid.
///
/// Two gaps in that argument, named rather than left to be discovered.
///
/// The first is *inside* the call: a worker that sends an over-long line is
/// killed and reaped by `Worker::read_reply`, so for the microseconds until this
/// entry is removed the number is free. A sweep landing exactly there would
/// signal a reaped pid --- which names something else only if the pid space wraps
/// inside that window.
///
/// The second is on the other side of the same entry, and it is the one the
/// lifetime argument reads as impossible. [`Workers::kill_overdue`] marks
/// `killed` under the `calls` lock and signals *outside* it. A reply already in
/// the pipe arrives intact --- `Ok` and "killed" is a reachable pair, which
/// [`Workers::watched`] says out loud --- so the blocked thread can finish, take
/// this entry and hand its worker to [`Workers::discard`], which kills and reaps
/// it, all before the signal is sent. From there the number is free for the same
/// microseconds and by the same mitigation.
///
/// Neither is closed here, and the reason is a trade rather than an oversight.
/// Re-checking membership just before the signal only moves the window: the
/// entry can go between the re-check and the syscall exactly as it can now.
/// Closing it means holding `calls` *across* the signal --- and across the
/// `diag::note` that names the kill, which writes a file --- on a lock every
/// request start and end contends on. That is a larger thing to be wrong about
/// than a window a pid space would have to wrap inside.
struct InFlight {
    pid: u32,
    /// When the request was handed over.
    since: Instant,
    /// Set by [`Workers::kill_overdue`] just before it signals.
    ///
    /// The only way the waiting thread can learn *why* its worker stopped: a
    /// child's pipe closes before the child becomes waitable, so asking the
    /// kernel gives "still running" for a process that has just been killed.
    killed: bool,
}

/// A worker waiting in a pool, and since when.
///
/// The timestamp is what retirement is decided on, and it belongs here rather
/// than on `Worker` because it is a fact about the *pool's* use of the process,
/// not about the process: a worker checked out and back is the same child with a
/// new idle time.
struct Idle {
    worker: Worker,
    /// When it was last returned to the pool.
    since: Instant,
}

/// One open document: its bytes, and the pool of processes parsing them.
struct Held {
    /// The document mapping, owned here rather than by any one worker so that
    /// every worker in the pool --- and every replacement for a dead one --- is
    /// handed the same bytes. See [`Worker::spawn_shared`].
    doc: Arc<Shm>,
    /// Workers not currently serving a request, oldest first.
    ///
    /// Pushed at the back and popped from the back, so the *front* is the coldest
    /// end and is where [`Workers::retire_idle`] takes from. Order is a
    /// convenience rather than the mechanism --- each entry carries its own
    /// timestamp and is judged on that --- but it means the survivor of a
    /// retirement is the worker a checkout would have reached for anyway.
    idle: Vec<Idle>,
    /// How many exist at all, idle or checked out. Not `idle.len()`: that would
    /// grow the pool again every time one was busy.
    spawned: usize,
    /// Every live worker's write half, by pid.
    ///
    /// Kept here rather than read off `idle`, because the worker that most needs
    /// a withdrawal is precisely the one that is **checked out** --- it is the
    /// one inside Pdfium. Removed on discard, since the entry holds a clone of
    /// the child's stdin and a stale one is a leaked descriptor.
    senders: Vec<(u32, WorkerSender)>,
    /// Why this document can no longer be served, once that is established.
    ///
    /// Set when a worker dies and the file underneath the mapping turns out to
    /// have been truncated --- see [`Workers::outlived_its_file`]. It is a
    /// *latch* rather than a diagnosis recomputed per request, and it has to be:
    /// once set, every checkout refuses without spawning anything.
    ///
    /// The alternative is what this replaced, and it is worth stating because it
    /// looks like working software. The crash path replaces a dead worker with
    /// one built from the same `doc` mapping --- deliberately, so a replacement
    /// serves the same bytes --- so against a truncated file the replacement
    /// faults exactly where its predecessor did. The reader scrolling into the
    /// missing tail then pays two process spawns and two `SIGBUS` deaths **per
    /// tile**, for a region that can never render, and the only trace is one
    /// line in the log.
    ///
    /// Never cleared. A document whose bytes are gone does not come back, and
    /// the way to read that file again is to open it again --- which builds a
    /// new mapping, in a new slot, with no latch on it.
    outlived: Option<String>,
    /// The password this document was opened with, if it needed one.
    ///
    /// **Held for the document's lifetime because the pool outlives the open.**
    /// Every worker maps the same bytes, so every worker meets the same
    /// encryption --- the second one the pool grows under contention, and every
    /// replacement for one that crashed. Without this, a locked document would
    /// render its first page and then refuse the second, and a crash would end
    /// the session; asking the reader again for each is not a design, it is the
    /// absence of one.
    ///
    /// It lives in the app process rather than in a worker because the workers
    /// are the disposable half. It is not written anywhere, not logged, and goes
    /// when [`Workers::close`] drops the slot.
    ///
    /// `docs/THREAT-MODEL.md` §T6 states what that costs: a password is in this
    /// process's memory while the document is open, which is also true of every
    /// page of the decrypted document, and is not defended against a reader of
    /// this process's memory --- who has already won.
    password: Option<String>,
}

/// Documents parsed in sandboxed child processes, several per document.
///
/// Shared across the service's threads, which is what makes the pool a pool:
/// each thread takes a job, checks a worker out, and renders in that process
/// while the others do the same. Everything here is short critical sections ---
/// no lock is ever held across a render.
pub(crate) struct Workers {
    library_dir: PathBuf,
    /// Indexed by document id, with a hole where one has been closed. See
    /// [`open_slot`].
    docs: Mutex<Vec<Option<Held>>>,
    /// Signalled when a worker returns to a pool, is discarded, or fails to
    /// spawn --- i.e. whenever waiting for one might have become worthwhile.
    returned: Condvar,
    /// The most workers any one document may have.
    capacity: usize,
    /// A worker that is running, sandboxed and font-warmed, with no document.
    ///
    /// One, not a pool of them. It exists for the *first* worker of a document,
    /// which is the one on the critical path to a first page; the pool's later
    /// workers are grown under contention, when a render is already on screen and
    /// several milliseconds are not what anybody is waiting for.
    ///
    /// `examples/prespawn_bench.rs` measures what it removes from an open: **7.9 ms**
    /// on a document that embeds its fonts and **15.3 ms** on one that does not,
    /// because a pre-spawned worker has already paid both the ~6.6 ms link-and-
    /// sandbox floor and the ~7.4 ms system-font walk. What it cannot remove is
    /// the page parse --- the A0 sheet still costs 48 ms of its 56.
    spare: Spare,
    /// How long a worker may sit idle before [`Workers::retire_idle`] kills it.
    ///
    /// Held rather than read from the environment where it is used, so that a
    /// benchmark can run two services at different timeouts in one process ---
    /// the same reason `capacity` is a field. It does **not** apply to the spare
    /// above: that is one process whose entire purpose is to be waiting, and
    /// retiring it would be retiring the mechanism.
    idle_after: Duration,
    /// Requests currently inside a worker, for [`Workers::kill_overdue`].
    ///
    /// A lock of its own rather than a field on [`Held`], and not for
    /// contention: the supervisor has to read this while the thread that wrote
    /// it is blocked on a pipe, and it is not a fact about a document's pool ---
    /// the entry outlives neither the document nor the worker, but it is keyed
    /// on neither. Bounded by the number of service threads, so a `Vec` scanned
    /// linearly is the whole structure.
    calls: Mutex<Vec<InFlight>>,
    /// How long one request may occupy a worker. See [`DEFAULT_DEADLINE`].
    deadline: Duration,
    queue: SharedQueue,
}

impl Workers {
    pub(crate) fn new(
        library_dir: PathBuf,
        queue: SharedQueue,
        capacity: usize,
        idle_after: Duration,
        deadline: Duration,
    ) -> Self {
        Self {
            spare: Spare::default(),
            library_dir,
            docs: Mutex::new(Vec::new()),
            returned: Condvar::new(),
            capacity,
            idle_after,
            calls: Mutex::new(Vec::new()),
            deadline,
            queue,
        }
    }

    /// The document table. Poisoning is recovered from rather than propagated:
    /// a panic in one job must not take every open document with it.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Option<Held>>> {
        self.docs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts a spare worker, unless one is already waiting.
    ///
    /// Called once as the service starts and again after each spare is consumed,
    /// always on a thread nobody is waiting on --- the whole value of this is that
    /// the ~6.6 ms link and the ~7.4 ms font walk happen while the shell is still
    /// coming up, and `AGENTS.md` measures that at ~250 ms of which none is ours.
    ///
    /// Failure is deliberately silent. A spare is an optimisation, and a machine
    /// that could not start one still opens documents by the ordinary path; the
    /// alternative is turning a missed 8 ms into an app that will not launch.
    pub(crate) fn prewarm(&self) {
        // The slot rather than the whole service, so this needs no `Arc<Self>`
        // and the `Engine` methods can keep taking `&self`.
        let slot = self.spare.clone();
        let library_dir = self.library_dir.clone();
        std::thread::Builder::new()
            .name("tpdf-prewarm".into())
            .spawn(move || {
                {
                    // Checked before spawning, so a burst of opens cannot leave a
                    // pile of unused sandboxed children behind. A spare that is
                    // still warming counts: two threads arriving together must not
                    // both decide there is none.
                    let mut current = slot.lock().unwrap_or_else(|e| e.into_inner());
                    if current.ready.is_some() || current.warming.is_some() || current.spawning {
                        return;
                    }
                    // Claimed before the fork, so the window in which a child
                    // exists unnamed is a window this slot admits to being in.
                    current.spawning = true;
                }
                let spawned = Worker::prespawn(&library_dir);
                let pre = match spawned {
                    Ok(pre) => {
                        // Registered before warming, not after.
                        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
                        slot.warming = Some(pre.pid());
                        slot.spawning = false;
                        drop(slot);
                        pre
                    }
                    Err(_) => {
                        // The claim has to be given back, or no spare is ever
                        // started again for the life of the service.
                        slot.lock().unwrap_or_else(|e| e.into_inner()).spawning = false;
                        return;
                    }
                };

                // Warmed here rather than at the point of use. Waiting on this
                // thread is free; waiting on the reader's thread is precisely the
                // cost being avoided.
                let warmed = pre.wait_warm();
                let mut spare = slot.lock().unwrap_or_else(|e| e.into_inner());
                spare.warming = None;
                match warmed {
                    // A second spare arriving while one is already published is
                    // dropped rather than kept, and dropping it kills it ---
                    // `Worker`'s own `Drop`, reached through `WarmWorker`.
                    Ok(worker) if spare.ready.is_none() => spare.ready = Some(worker),
                    _ => {}
                }
            })
            .ok();
    }

    /// The most workers any one document may have on this pool.
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    /// The spare slot, poisoning recovered from as everywhere else here.
    fn spare(&self) -> std::sync::MutexGuard<'_, SpareSlot> {
        self.spare.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The process id of the warmed spare, if one is waiting.
    ///
    /// These three accessors exist so the service can answer a probe's questions
    /// without reaching into [`SpareSlot`]: what a caller needs is "which of my
    /// children are not pool workers", and the three states that answer splits
    /// into --- ready, warming, forking --- are this module's business.
    pub(crate) fn spare_pid(&self) -> Option<u32> {
        self.spare().ready.as_ref().map(WarmWorker::pid)
    }

    /// Every process the spare slot is responsible for, warm or still warming.
    pub(crate) fn spare_pids(&self) -> Vec<u32> {
        self.spare().pids()
    }

    /// Whether every spare process can currently be named.
    pub(crate) fn spares_settled(&self) -> bool {
        self.spare().settled()
    }

    /// Takes the spare worker, if one is ready.
    ///
    /// Only `ready`: a spare that is still warming is left alone, because taking
    /// it would mean waiting out the link and the sandbox on the caller's thread,
    /// which is the entire cost this mechanism exists to move elsewhere.
    fn take_spare(&self) -> Option<WarmWorker> {
        self.spare
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ready
            .take()
    }

    /// Takes a worker out of a document's pool, growing or waiting as needed.
    ///
    /// Growth is **lazy**, and that is the whole reason a pool is affordable: a
    /// reader turning one page at a time never has more than one worker, and so
    /// never pays for more than one parse of the document. A second appears only
    /// when a second request arrives while the first is still rendering --- which
    /// is exactly the case a pool is for.
    fn checkout(&self, doc: u32) -> Result<Worker, String> {
        let mut docs = self.lock();
        loop {
            let held = open_slot_mut(&mut docs, doc)?;
            // Refused before a worker is reached for, because for this document
            // there is no worker that could succeed: every one of them --- and
            // every replacement --- maps the same bytes, and those bytes are the
            // ones that faulted. See [`Held::outlived`].
            if let Some(reason) = &held.outlived {
                return Err(reason.clone());
            }
            if let Some(idle) = held.idle.pop() {
                return Ok(idle.worker);
            }
            if held.spawned < self.capacity {
                // The reservation is taken *before* the lock is released, so two
                // threads arriving together cannot both decide there is room for
                // the last worker.
                held.spawned += 1;
                let bytes = held.doc.clone();
                // Cloned under the lock with the bytes, because the two go
                // together: a worker handed these bytes cannot open them without
                // this. See `Held::password`.
                let password = held.password.clone();
                drop(docs);
                return self.spawn_into(doc, bytes, password);
            }
            // At capacity and all of them busy. Waiting is right rather than
            // queueing another request: this thread has nothing else to do, and
            // the caller's tile cannot start until a process is free anyway.
            docs = self.returned.wait(docs).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Whether a dead worker died because the document's file was truncated.
    ///
    /// Called only after a worker has already been established as dead, so the
    /// question costs an `fstat` on a path that has just lost a process --- not
    /// on every request. It is a *diagnosis*, not a guard: it cannot prevent the
    /// fault, because the file can be shortened between any check and the read
    /// that faults on it. What it prevents is the second fault, and the
    /// hundredth.
    ///
    /// The comparison is against the length the mapping was made at, asked of
    /// the descriptor rather than the path --- see [`Shm::backing_len`], which
    /// carries why that distinction is the whole check rather than a detail.
    ///
    /// `None` on every other cause of death, including a platform that cannot
    /// answer, and the caller then does what it did before: replace the worker
    /// and retry once.
    fn outlived_its_file(&self, doc: u32) -> Option<String> {
        let mut docs = self.lock();
        let held = open_slot_mut(&mut docs, doc).ok()?;
        // Already diagnosed by whichever request got here first. Returned rather
        // than recomputed: two threads can be inside a crash for the same
        // document at once, and the second one's `fstat` is redundant.
        if let Some(reason) = &held.outlived {
            return Some(reason.clone());
        }
        // Read once and passed on, not read again for the message. A second
        // `backing_len` could answer differently --- the writer that shortened
        // the file is still running --- and then the log would name a length the
        // decision was not made on.
        let reason = shrunk(held.doc.len() as u64, held.doc.backing_len())?;
        held.outlived = Some(reason.clone());
        drop(docs);

        // No epitaph, which is why this takes no worker: it runs microseconds
        // after the death, when `try_wait` still answers "still running", so an
        // epitaph here reads *"worker still running; this file changed on disk"*
        // --- a sentence that contradicts itself and sends the next reader
        // looking for a second failure. The deadline branch below omits one for
        // the same reason. The diagnosis is about the file, and names the file.
        crate::diag::note(&format!(
            "[render] document {doc}: {reason} No replacement can serve it, so further \
             requests are refused rather than retried."
        ));
        Some(reason)
    }

    /// Returns a reservation [`checkout`] took, after the worker it was for
    /// failed to arrive.
    ///
    /// Without it the pool shrinks by one every time a spawn or an unlock fails,
    /// and eventually deadlocks at zero with every waiter still waiting.
    fn give_back(&self, doc: u32) {
        let mut docs = self.lock();
        if let Ok(held) = open_slot_mut(&mut docs, doc) {
            held.spawned = held.spawned.saturating_sub(1);
        }
        drop(docs);
        self.returned.notify_all();
    }

    /// Spawns a worker against a reservation already taken by [`checkout`].
    ///
    /// **Every worker after the first goes through here, replacements included**,
    /// which is why the password does: the bytes carry the encryption, so a
    /// second worker meets exactly what the first did. A pool that unlocked only
    /// its first worker would serve page one and refuse page two.
    fn spawn_into(
        &self,
        doc: u32,
        bytes: Arc<Shm>,
        password: Option<String>,
    ) -> Result<Worker, String> {
        // Outside the lock: a spawn is ~12 ms, and holding the table for that
        // would stall every other document as well as this one's other threads.
        let mut worker = match Worker::spawn_shared(bytes, &self.library_dir) {
            Ok(worker) => worker,
            Err(e) => {
                // Give the reservation back, or the pool shrinks by one every
                // time a spawn fails and eventually deadlocks at zero.
                self.give_back(doc);
                return Err(e);
            }
        };

        // Before the worker is published, so nothing can check out one that is
        // still sitting in `worker_child::unlock`. The reservation is given back
        // on failure for the same reason it is above --- an unlock that fails
        // here is a worker that will never serve anything.
        if let Some(password) = password {
            let sent = self
                .watched(&mut worker, |worker| {
                    worker.call(&Request::Unlock { password })
                })
                .0
                .and_then(|response| {
                    if response.ok {
                        Ok(())
                    } else {
                        Err(response.error)
                    }
                });
            if let Err(e) = sent {
                let e = format!("a replacement worker could not unlock the document: {e}");
                self.give_back(doc);
                return Err(e);
            }
        }

        let mut docs = self.lock();
        let Ok(held) = open_slot_mut(&mut docs, doc) else {
            // Closed while this was spawning. Dropping the worker kills it,
            // which is what the close would have done.
            return Err(not_open(doc, true));
        };
        held.senders.push((worker.pid(), worker.sender()));
        Ok(worker)
    }

    /// Returns a worker to its pool, and starts its idle clock.
    fn checkin(&self, doc: u32, worker: Worker) {
        let mut docs = self.lock();
        match open_slot_mut(&mut docs, doc) {
            Ok(held) => held.idle.push(Idle {
                worker,
                since: Instant::now(),
            }),
            // The document was closed while this worker was out. Dropping it
            // kills it --- and `close` is waiting for exactly this, so the
            // notify below is what lets it finish.
            Err(_) => drop(worker),
        }
        drop(docs);
        self.returned.notify_all();
    }

    /// Retires a worker rather than returning it, so a fresh one takes its slot.
    fn discard(&self, doc: u32, worker: Worker) {
        let pid = worker.pid();
        // Dropped first: `Worker`'s own `Drop` kills and reaps, and doing that
        // outside the lock keeps a dying child off the critical section.
        drop(worker);

        let mut docs = self.lock();
        if let Ok(held) = open_slot_mut(&mut docs, doc) {
            held.spawned = held.spawned.saturating_sub(1);
            // The sender holds a clone of the child's stdin, so leaving it here
            // would keep the pipe open for the life of the service --- one
            // descriptor per worker that ever died.
            held.senders.retain(|(other, _)| *other != pid);
        }
        drop(docs);
        self.returned.notify_all();
    }

    /// Kills every worker that has idled past the timeout, down to [`KEPT_WARM`].
    ///
    /// This is what stops the pool's peak being what the session keeps. Growth is
    /// driven by contention and contention is a burst --- a screenful, a fast
    /// scroll --- so without this, one flick through a large document leaves six
    /// processes each holding their own parse for as long as the file is open.
    ///
    /// The bookkeeping has to move together with the processes, and each half is
    /// load-bearing for a different reason:
    ///
    /// - **`spawned` comes down with them**, or the pool believes in workers that
    ///   do not exist: [`Workers::checkout`] would refuse to grow past a ceiling
    ///   nothing is under, and [`Workers::close`] would wait forever for a worker
    ///   that can never come home.
    /// - **The sender goes too.** It holds a *clone* of the child's stdin, so
    ///   dropping the worker does not close the pipe --- that is a descriptor per
    ///   worker ever retired, with no functional symptom at all, because writing
    ///   to a dead pipe fails harmlessly. `discard` learned this the same way.
    ///
    /// Returns how many were killed, for a caller that wants to say so.
    fn retire_idle(&self) -> usize {
        // Collected under the lock and killed outside it. `Worker`'s own `Drop`
        // kills and reaps, measured at 1.2 ms each, and every other document's
        // threads would be waiting on the table for the duration.
        let mut retired: Vec<Worker> = Vec::new();
        {
            let mut docs = self.lock();
            for held in docs.iter_mut().flatten() {
                // Counted against everything alive, not against what is idle: a
                // document with one worker out and one waiting has two, and the
                // waiting one is not the last.
                let mut may_go = held.spawned.saturating_sub(KEPT_WARM);
                let mut index = 0;
                while index < held.idle.len() && may_go > 0 {
                    if held.idle[index].since.elapsed() < self.idle_after {
                        index += 1;
                        continue;
                    }
                    let idle = held.idle.remove(index);
                    held.spawned = held.spawned.saturating_sub(1);
                    held.senders.retain(|(pid, _)| *pid != idle.worker.pid());
                    retired.push(idle.worker);
                    may_go -= 1;
                }
            }
        }

        let count = retired.len();
        drop(retired);
        if count > 0 {
            // A close is waiting on exactly this condition, and a retirement
            // changes both sides of it.
            self.returned.notify_all();
        }
        count
    }

    /// Sends a withdrawal to every worker of every open document.
    ///
    /// Broadcast rather than addressed, because a `rid` is unique for the life
    /// of the process and a worker that has never seen one ignores it. With a
    /// pool that is more useful than before rather than less: the parent does
    /// not know which of a document's workers took the request.
    ///
    /// The senders are cloned under the lock and written to outside it. A
    /// `WorkerSender` is a pipe to a child that is *inside a render*, which is
    /// the case this call exists for --- so the write can block on a full pipe,
    /// and holding the document table across it would stall every other
    /// document's checkouts and check-ins on a withdrawal. Cloning is an `Arc`
    /// bump per worker, and the module note above claims every critical section
    /// here is bookkeeping; this is what keeps that true.
    pub(crate) fn broadcast_withdraw(&self, rid: u64) {
        let senders: Vec<WorkerSender> = {
            let docs = self.lock();
            docs.iter()
                .flatten()
                .flat_map(|held| held.senders.iter().map(|(_, sender)| sender.clone()))
                .collect()
        };
        for sender in senders {
            // A dead worker is not this call's problem: whichever thread is
            // holding it will report that, with an epitaph.
            let _ = sender.withdraw(rid);
        }
    }

    /// The in-flight table, poisoning recovered from as everywhere else here.
    fn calls(&self) -> std::sync::MutexGuard<'_, Vec<InFlight>> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs one exchange with the supervisor watching the clock on it, and says
    /// whether the supervisor killed the worker while it ran.
    ///
    /// Every path that waits on a worker goes through here, which is the point:
    /// only [`Request::Tile`] is withdrawable, so `Text`, `Search`, `Outline`
    /// and `Open` have no way to give up on their own. A page that never
    /// finishes parsing would otherwise hold this thread --- one of `pool + 2`,
    /// shared by *every* open document --- until the process ended.
    ///
    /// The flag is returned rather than folded into the error because the two
    /// are independent: a reply already in the pipe when the deadline expired
    /// arrives intact, so `Ok` and "killed" is a reachable pair rather than a
    /// contradiction, and the answer is worth keeping while the process is not.
    fn watched<T>(
        &self,
        worker: &mut Worker,
        exchange: impl FnOnce(&mut Worker) -> Result<T, String>,
    ) -> (Result<T, String>, bool) {
        let watch = CallWatch::start(self, worker.pid());
        let outcome = exchange(worker);
        (outcome, watch.end())
    }

    /// Kills every worker whose current request has outrun the deadline.
    ///
    /// The kill is the whole mechanism: there is no way to interrupt a thread
    /// blocked in a read, so what ends the wait is the far end of the pipe
    /// closing. `Worker::read_reply` then reports EOF, and
    /// [`Workers::with_worker`] discards the corpse and answers the caller ---
    /// on the strength of the flag set here rather than of the epitaph, which at
    /// that instant still reads "still running". See [`CallWatch::end`].
    ///
    /// Collected under the lock and signalled outside it, as every other kill
    /// here is. Returns how many were killed, for a caller that wants to say so.
    fn kill_overdue(&self) -> usize {
        let now = Instant::now();
        let overdue = {
            let mut calls = self.calls();
            let overdue = overdue(&calls, self.deadline, now);
            // Marked before the signal, and this is what the waiting thread
            // reads: a killed worker cannot be recognised by looking at the
            // process. See [`CallWatch::end`].
            for call in calls.iter_mut() {
                call.killed |= overdue.contains(&call.pid);
            }
            overdue
        };

        for pid in &overdue {
            // Said out loud, because the caller sees only "worker stopped
            // answering" and would otherwise have no way to tell a deadline kill
            // from a crash --- and those are opposite diagnoses: one is a
            // document doing too much, the other is PDFium falling over.
            crate::diag::note(&format!(
                "[render] worker {pid}: no reply in {:.0} s; killing it",
                self.deadline.as_secs_f64()
            ));
            kill_pid(*pid);
        }
        overdue.len()
    }

    /// Runs one exchange with one of a document's workers, replacing it if it
    /// has died.
    ///
    /// Retried exactly once, and the bound is the retry rather than a counter.
    /// A crash the document *causes* reproduces on the retry --- so the reader
    /// pays two crashes for that tile and gets an error, and the next request
    /// pays one more. That is bounded by the requests the reader makes, which is
    /// what makes a restart budget on top of it unreachable defence: there is no
    /// loop here for one to break.
    ///
    /// The trade it does make is that a death caused by the *previous* request,
    /// or by anything outside the document at all, is invisible to the caller.
    /// That is the point of restarting.
    ///
    /// **A deadline kill is the one death that is not retried**, and the
    /// asymmetry is deliberate. A crash costs milliseconds to reproduce, so
    /// trying again is nearly free and hides a death nobody needed to know
    /// about; a request that hung has just spent its whole deadline, and running
    /// it again would spend another one of a service thread to learn what the
    /// first already established. The reader gets the error after one deadline
    /// rather than two.
    fn with_worker<T>(
        &self,
        doc: u32,
        exchange: impl Fn(&mut Worker) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut worker = self.checkout(doc)?;
        // The flag is consulted before anything is decided, because **a deadline
        // kill cannot be recognised by looking at the process.** A child's pipe
        // closes on the way out and it becomes waitable slightly later, so
        // `is_running` says "still running" of a worker `SIGKILL`ed microseconds
        // ago --- observed end to end under `TPDF_CALL_MS=1`, where the epitaph
        // read exactly that for a process the supervisor had just killed.
        // Believing it would put the corpse back in the pool, where it would
        // fail somebody else's request instead of this one.
        let (outcome, killed) = self.watched(&mut worker, &exchange);

        let error = match outcome {
            Ok(value) if !killed => {
                self.checkin(doc, worker);
                return Ok(value);
            }
            // A reply that arrived anyway, from a worker killed for taking too
            // long. The answer is already copied out of the mapping and is worth
            // having; the process it came from is not.
            Ok(value) => {
                self.discard(doc, worker);
                return Ok(value);
            }
            Err(e) => e,
        };

        // Asked of the *file* before anything is concluded from the *process*,
        // and the order is the whole of it. `is_running` is `try_wait`, so a
        // worker that faulted microseconds ago reads as alive --- the trap this
        // file already records for the deadline path, which turns out to reach
        // the crash path too. Placed after that check, this diagnosis was simply
        // skipped on the first failure: the corpse looked alive, was checked
        // back into the pool, and the caller got "worker stopped answering"
        // instead of a reason. Measured, not reasoned about --- `rewrite-probe`
        // reported exactly that, with the diagnosis arriving on some later
        // request once `try_wait` had caught up.
        //
        // Asking first costs an `fstat` on a path that has already failed, and
        // it cannot produce a false diagnosis: a file that has lost bytes the
        // mapping covers is a document that cannot be served, whether the worker
        // that noticed died of it or answered with an ordinary error.
        if let Some(reason) = self.outlived_its_file(doc) {
            self.discard(doc, worker);
            return Err(reason);
        }

        // Only a *dead* worker is worth replacing. A live one that answered with
        // an error answered: restarting on that would hide a bug here behind a
        // process that gets the next question right, and would cost a document
        // reopen per malformed request.
        if !killed && worker.is_running() {
            self.checkin(doc, worker);
            return Err(error);
        }

        if killed {
            // Named rather than given an epitaph, which would read `still
            // running` here for the reason above --- and said out loud, because
            // the error the caller receives is about a pipe rather than about a
            // request that took too long.
            crate::diag::note(&format!(
                "[render] document {doc}: worker killed for exceeding its deadline"
            ));
            self.discard(doc, worker);
            return Err(format!("{error} --- the request exceeded its deadline"));
        }

        // Said out loud, once, because a successful retry makes the death
        // invisible to the caller and a worker that dies quietly is the hardest
        // thing in this design to diagnose.
        crate::diag::note(&format!(
            "[render] document {doc}: worker {}; starting a replacement",
            worker.epitaph()
        ));
        self.discard(doc, worker);

        // The discard freed a slot, so this checkout cannot block: it takes an
        // idle worker if the pool has one and spawns otherwise. Which of the two
        // it does is not something this path needs to care about --- what matters
        // is that it does not wait, because the thread holding the failed request
        // is the one that just made room.
        let mut replacement = self.checkout(doc).map_err(|e| format!("{error} --- {e}"))?;
        let (second, killed) = self.watched(&mut replacement, &exchange);
        // Same rule as above, for the same reason: a worker the supervisor
        // killed does not go back into the pool, whatever it managed to answer
        // on its way out.
        if killed {
            self.discard(doc, replacement);
        } else {
            self.checkin(doc, replacement);
        }
        second
    }

    /// Sends a request that answers with a payload, and reads the answer back.
    ///
    /// Returns the [`Reply`] whole. Which variant belongs to which request is
    /// the caller's to check, and every caller below does it the same way
    /// through [`mismatched`] --- the type says a reply arrived, not that it
    /// answers the question asked.
    fn ask(&self, doc: u32, request: &Request) -> Result<Reply, String> {
        self.with_worker(doc, |worker| {
            let response = worker.call(request)?;
            if !response.ok {
                return Err(response.error);
            }
            response
                .reply
                .ok_or_else(|| "worker replied without a payload".to_string())
        })
    }

    /// Renders through a worker, having already claimed the request.
    fn render(&self, request: &TileRequest, token: &CancelToken) -> Result<TileOutcome, String> {
        self.with_worker(request.doc, |worker| {
            let response = worker.call(&Request::Tile {
                rid: request.rid,
                page: request.page,
                crop: request.crop,
                scale: request.scale,
                turns: request.turns,
                invert: request.invert,
                x: request.x,
                y: request.y,
                width: request.width,
                height: request.height,
                png: request.format == TileFormat::Png,
            })?;

            if !response.ok {
                return Err(response.error);
            }
            if response.abandoned {
                return Ok(TileOutcome::Abandoned);
            }
            // The withdrawal that lost its race to the pipe. The worker rendered
            // the tile because the `Withdraw` arrived before the request it
            // names, and the caller stopped wanting it regardless --- so this
            // side's token, not the worker's answer, is what decides.
            if token.is_cancelled() {
                return Ok(TileOutcome::Abandoned);
            }

            let length = payload_length(&response, request, worker.tile.len())?;
            let bytes = worker.tile.as_slice()[..length].to_vec();
            mark("first tile rendered");

            Ok(TileOutcome::Rendered(Tile {
                bytes,
                width: request.width,
                height: request.height,
                format: request.format,
                render_us: response.render_us,
                encode_us: response.encode_us,
            }))
        })
    }
}

/// Registers a call for the supervisor, and takes it off again.
///
/// A guard rather than a pair of calls, and the asymmetry is the reason: a
/// missing registration costs a request its deadline, where a registration left
/// behind is a pid that will be signalled *after* its `Worker` was dropped and
/// reaped --- by which time the number may name anything on the machine. `?`,
/// an early return and a panic inside the exchange all end the entry here.
struct CallWatch<'a> {
    workers: &'a Workers,
    pid: u32,
}

impl<'a> CallWatch<'a> {
    /// Starts the clock on a request to `pid`.
    fn start(workers: &'a Workers, pid: u32) -> Self {
        workers.calls().push(InFlight {
            pid,
            since: Instant::now(),
            killed: false,
        });
        Self { workers, pid }
    }

    /// Ends the watch, reporting whether the supervisor killed this worker.
    ///
    /// The verdict has to come from here rather than from the process, because
    /// the process cannot answer: a child's pipe closes on its way out and it
    /// becomes waitable slightly later, so a `SIGKILL` sent microseconds ago is
    /// indistinguishable from a worker that is still thinking.
    fn end(self) -> bool {
        // `Drop` runs immediately afterwards and finds nothing to remove, which
        // is why the removal is written as "if present" rather than asserted.
        self.take()
    }

    /// Removes this call's entry, returning whether it had been killed.
    fn take(&self) -> bool {
        // One entry, not every entry with this pid: a worker serves one request
        // at a time, so there is exactly one --- and removing all of them would
        // turn a bookkeeping slip into a silently unsupervised call.
        let mut calls = self.workers.calls();
        match calls.iter().position(|call| call.pid == self.pid) {
            Some(index) => calls.remove(index).killed,
            None => false,
        }
    }
}

impl Drop for CallWatch<'_> {
    fn drop(&mut self) {
        self.take();
    }
}

/// Which outstanding calls have outrun the deadline.
///
/// `now` is a parameter rather than read here, so the decision can be exercised
/// directly instead of by waiting for one. `AGENTS.md` records that a check
/// whose failure mode is a wait cannot fail; a supervisor testable only by
/// hanging a real worker is that check.
///
/// Strictly past the deadline, so a zero deadline is still a deadline and an
/// exact hit is not a kill --- `Instant` ticks at 41.67 ns on this hardware, so
/// "elapsed equals the deadline" is reachable rather than theoretical.
fn overdue(calls: &[InFlight], deadline: Duration, now: Instant) -> Vec<u32> {
    calls
        .iter()
        .filter(|call| now.saturating_duration_since(call.since) > deadline)
        .map(|call| call.pid)
        .collect()
}

/// Ends a worker process, so a read blocked on its pipe fails.
///
/// `SIGKILL` rather than a request to stop: this is the one process in the
/// design that is *assumed hostile*, and a signal it could handle is a signal it
/// could ignore. The same argument [`Workers::close`] makes about not sending a
/// goodbye on the wire.
#[cfg(unix)]
fn kill_pid(pid: u32) {
    // SAFETY: an ordinary signal, to a child of this process that the caller has
    // established is still unreaped. A failure means it is already gone, which
    // is the outcome being asked for.
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
}

/// Ends a worker process, so a read blocked on its pipe fails.
///
/// `TerminateProcess` is the whole mechanism, because Windows has no signals: it
/// is unconditional, the target cannot decline it, and it closes the child's
/// handles --- which is what actually unblocks the thread waiting on the pipe.
/// [`sandbox_win::KILLED_EXIT`] rather than `1` so the corpse can say it did not
/// choose to exit; a code is the only channel left where a signal number would be.
///
/// **This was a no-op until 2026-07-30, and its own comment had predicted the
/// consequence**: *"if a worker ever starts on Windows this has to become
/// `TerminateProcess`, or the deadline silently stops being one."* Workers started
/// on Windows the day before. The failure was worse than an absent deadline ---
/// [`Workers::kill_overdue`] counted the pid, set `killed`, and logged *"worker
/// killed for exceeding its deadline"*, so the caller got a deadline error, the
/// log claimed a kill, and the process went on holding a hung render forever. One
/// leaked worker per hung document, with a line in the log saying otherwise. It is
/// the [`docs/TRAPS.md`] entry *"a guard that degrades to a no-op off its platform
/// stops being a guard"*, fired for real rather than in the abstract.
///
/// No wait, deliberately, and unlike `backend_probe`'s `kill_and_wait`: that one
/// then *counts processes*, so it has to outlast the kernel's asynchrony. Here the
/// only thing wanted is that the blocked read stops blocking, and waiting would
/// put the sweep thread to sleep on a process it has just declared hostile.
///
/// Killing by pid is sound for the same reason [`InFlight`] gives, arriving by a
/// different route: the blocked thread owns the `Worker`, which owns the
/// `Contained`, which holds an open handle to the process --- and Windows will not
/// recycle a pid while any handle to it is open. That is stronger than the unix
/// argument wherever the thread is still blocked.
///
/// It is **not** stronger in the two windows [`InFlight`] names, and the reason is
/// worth stating rather than inferring: in both of them the thread has already
/// dropped the `Worker`, so the handle carrying that guarantee is closed and the
/// pid is as free here as it is on unix. `OpenProcess` then either fails, which
/// is the harmless branch, or names whatever now holds the number. Same
/// mitigation, same size --- the pid space would have to wrap inside a few
/// microseconds --- and no separate one to write here.
#[cfg(windows)]
fn kill_pid(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    // SAFETY: a pid the caller has established still names a live child of this
    // process. A null handle means it is already gone, which is the outcome being
    // asked for --- the same degradation the unix arm gets from a failed `kill`.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return;
    }
    // SAFETY: a live handle opened with PROCESS_TERMINATE, closed on the next line.
    unsafe { TerminateProcess(handle, crate::sandbox_win::KILLED_EXIT) };
    // SAFETY: opened above, closed exactly once, not used again.
    unsafe { CloseHandle(handle) };
}

/// Ends a worker process. Unreachable on a platform that spawns none.
///
/// A silent no-op is safe only because [`Worker::spawn`] refuses where there is no
/// containment to spawn into, so nothing is ever registered to kill. Both
/// platforms that *do* spawn have a real implementation above; if a third one is
/// added, it needs one here before it needs anything else, because the deadline is
/// the only bound on a render that never returns.
#[cfg(not(any(unix, windows)))]
fn kill_pid(_pid: u32) {}

/// How many bytes of the shared mapping a reply is entitled to.
///
/// The worker is our code, and it is also the process holding the attacker's
/// document --- so its replies are the one thing crossing back out of the blast
/// radius, and a length it states is a claim rather than a fact. Reading past
/// the mapping on a claim of 4 GB would be the boundary handing over the
/// authority it exists to withhold.
///
/// For raw pixels the answer is arithmetic rather than a bound: a tile is
/// exactly `width x height x 4` bytes, so anything else is wrong even when it
/// fits. PNG has no such closed form and gets the mapping's size.
fn payload_length(
    response: &Response,
    request: &TileRequest,
    capacity: usize,
) -> Result<usize, String> {
    let stated = response.bytes;
    if stated > capacity {
        return Err(format!(
            "worker claims a {stated}-byte tile and the shared mapping holds {capacity}"
        ));
    }
    if request.format == TileFormat::Raw {
        let expected = request.width as usize * request.height as usize * 4;
        if stated != expected {
            return Err(format!(
                "worker returned {stated} bytes for a {}x{} raw tile, which is {expected}",
                request.width, request.height
            ));
        }
    }
    Ok(stated)
}

/// Reads a worker's refusal back into the type the engine boundary speaks.
///
/// A [`Response`] carries the distinction as a flag and the engine carries it as
/// a field; this is the one place they meet, so nothing else has to know that a
/// refusal has two halves. Deliberately not a `From` impl: a `Response` is a
/// reply of any kind, and most of them are successes.
fn refusal_of(response: Response) -> Refusal {
    Refusal {
        reason: response.error,
        locked: response.locked,
    }
}

impl Engine for Workers {
    /// Spawns the document's first worker and asks it for the geometry.
    ///
    /// One, not `capacity`: the pool grows only under contention, so a document
    /// that is opened and read one page at a time costs exactly one process. The
    /// spawn is on the critical path to the first page and what it costs is
    /// measured rather than assumed --- see PLAN §9.
    fn open(
        &self,
        path: &Path,
        lazy_geometry: bool,
        password: Option<&str>,
    ) -> Result<DocumentInfo, Refusal> {
        let t0 = Instant::now();
        // Mapped here rather than inside the spawn, because this is the copy
        // every later worker for this document will be handed. The file is read
        // once, at open, and never again.
        let doc = Arc::new(Shm::map_file(path)?);
        // A spare, if one warmed in time. It has already paid the link, the
        // sandbox and the font walk, so what remains is the parse of this
        // document -- 0.3 ms on a small file against 15.7 ms cold.
        let mut worker = match self.take_spare() {
            Some(pre) => match pre.adopt(doc.clone()) {
                Ok(worker) => {
                    mark("worker adopted");
                    worker
                }
                // Not fatal. A spare can die while it waits -- it is a process
                // like any other -- and falling back is the difference between a
                // slower open and a document that refuses to open at all. The
                // reason is said out loud because a spare that dies every time
                // would otherwise show up only as the saving quietly vanishing.
                Err(e) => {
                    crate::diag::note(&format!(
                        "[render] a pre-spawned worker could not take the document: {e}"
                    ));
                    let worker = Worker::spawn_shared(doc.clone(), &self.library_dir)?;
                    mark("worker spawned");
                    worker
                }
            },
            None => {
                let worker = Worker::spawn_shared(doc.clone(), &self.library_dir)?;
                mark("worker spawned");
                worker
            }
        };
        // Started before the reply is waited for, so the next document's spare is
        // warming while this one is still parsing.
        self.prewarm();

        // Watched like every pooled request, and this one most of all: the
        // worker is not in a pool yet, so nothing else here would ever notice
        // it, and a parse that never returns is the *first* thing a hostile
        // document can do. A kill lands as a failed open rather than as a
        // permanently occupied thread.
        //
        // The kill flag changes the *message* here and not the control flow,
        // which is worth saying rather than leaving to be inferred from an
        // ignored value. On the failure side the `?` drops the worker, and
        // dropping one kills and reaps it --- but "worker stopped answering
        // (still running)" is what a reader would be shown for a document too
        // large to parse in time, which sends the next person to look for a
        // crash. On the success side --- a reply already in the pipe when the
        // deadline expired --- the document is real, and the dead worker is
        // published into the pool where the first request to take it fails once
        // and is replaced by the path that exists for a crashed worker. There is
        // no third case.
        // The password, before the geometry. A worker that could not open the
        // document is sitting in `worker_child::unlock` answering everything
        // with `locked`, so `Open` would come straight back --- and a worker that
        // opened without needing one accepts this and carries on. One order for
        // both, rather than a branch on what the parent guesses the file is.
        if let Some(password) = password {
            let (response, _) = self.watched(&mut worker, |worker| {
                worker.call(&Request::Unlock {
                    password: password.to_string(),
                })
            });
            let response = response?;
            if !response.ok {
                return Err(refusal_of(response));
            }
        }

        let (response, killed) = self.watched(&mut worker, |worker| {
            worker.call(&Request::Open { lazy_geometry })
        });
        let response = match response {
            Ok(response) => response,
            Err(e) => {
                let e = if killed {
                    format!("{e} --- the document did not open within the deadline")
                } else {
                    e
                };
                // Said out loud, because this is the failure a reader reports and
                // the worker's own account of it goes to a stderr a GUI process
                // does not have. Until this line the log of a session in which no
                // document could be opened at all was empty --- which is what a
                // session with nothing wrong looks like, and it cost a diagnosis.
                crate::diag::note(&format!("[render] opening {}: {e}", path.display()));
                return Err(e.into());
            }
        };
        if !response.ok {
            return Err(refusal_of(response));
        }
        let payload = response.reply.ok_or("worker opened without a payload")?;
        let (pages, page_count, lazy_geometry) = match payload {
            Reply::Open {
                pages,
                page_count,
                lazy_geometry,
            } => (pages, page_count, lazy_geometry),
            other => return Err(mismatched("open", &other).into()),
        };
        let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
        mark("document parsed");

        let mut docs = self.lock();
        let id = docs.len() as u32;
        docs.push(Some(Held {
            doc,
            password: password.map(str::to_string),
            senders: vec![(worker.pid(), worker.sender())],
            spawned: 1,
            idle: vec![Idle {
                worker,
                since: Instant::now(),
            }],
            outlived: None,
        }));
        drop(docs);
        mark("document open complete");

        Ok(DocumentInfo {
            id,
            pages,
            page_count,
            lazy_geometry,
            open_ms,
            at_ms: since_process_start_ms(),
        })
    }

    /// Claims the request here, then renders it there.
    ///
    /// Two queues, and each catches what the other cannot: this one drops a
    /// request that was withdrawn before it ever reached a worker, and the
    /// worker's own reaches a render already inside Pdfium. See
    /// [`RenderService::cancel`].
    ///
    /// The claim is taken before the checkout on purpose. A request withdrawn
    /// while it is waiting for a free worker should not then occupy one.
    fn tile(&self, request: &TileRequest) -> Result<TileOutcome, String> {
        let token = match self.queue.with(|queue| queue.claim(request.rid)) {
            Claim::Start(token) => token,
            Claim::Withdrawn => return Ok(TileOutcome::Abandoned),
        };

        let result = self.render(request, &token);
        self.queue.with(|queue| queue.release(request.rid));
        result
    }

    fn text(&self, doc: u32, page: u32, crop: Option<[f32; 4]>) -> Result<PageText, String> {
        match self.ask(doc, &Request::Text { page, crop })? {
            Reply::Text(text) => Ok(text),
            other => Err(mismatched("text", &other)),
        }
    }

    fn search(
        &self,
        doc: u32,
        page: u32,
        query: &str,
        options: crate::search::Options,
        carry: Option<&crate::search::Carry>,
    ) -> Result<PageMatches, String> {
        let request = Request::Search {
            page,
            query: query.to_string(),
            options,
            carry: carry.cloned(),
        };
        match self.ask(doc, &request)? {
            Reply::Search(matches) => Ok(matches),
            other => Err(mismatched("search", &other)),
        }
    }

    fn content(&self, doc: u32, page: u32) -> Result<Option<[f64; 4]>, String> {
        match self.ask(doc, &Request::Content { page })? {
            Reply::Content(found) => Ok(found),
            other => Err(mismatched("content", &other)),
        }
    }

    fn geometry(
        &self,
        doc: u32,
        page: u32,
        crop: Option<[f32; 4]>,
    ) -> Result<crate::render::CropGeometry, String> {
        match self.ask(doc, &Request::Geometry { page, crop })? {
            Reply::Geometry(size) => Ok(size),
            other => Err(mismatched("geometry", &other)),
        }
    }

    fn crop_box(&self, doc: u32, page: u32, rect: [f32; 4]) -> Result<[f32; 4], String> {
        match self.ask(doc, &Request::CropBox { page, rect })? {
            Reply::CropBox(want) => Ok(want),
            other => Err(mismatched("crop box", &other)),
        }
    }

    fn outline(&self, doc: u32) -> Result<Outline, String> {
        match self.ask(doc, &Request::Outline)? {
            Reply::Outline(outline) => Ok(outline),
            other => Err(mismatched("outline", &other)),
        }
    }

    fn comments(&self, doc: u32) -> Result<Comments, String> {
        match self.ask(doc, &Request::Comments)? {
            Reply::Comments(comments) => Ok(comments),
            other => Err(mismatched("comments", &other)),
        }
    }

    fn links(&self, doc: u32) -> Result<Links, String> {
        match self.ask(doc, &Request::Links)? {
            Reply::Links(links) => Ok(links),
            other => Err(mismatched("links", &other)),
        }
    }

    fn mapping(&self, doc: u32) -> Result<Vec<crate::encoding::PageMapping>, String> {
        match self.ask(doc, &Request::Mapping)? {
            Reply::Mapping(mapping) => Ok(mapping),
            other => Err(mismatched("mapping", &other)),
        }
    }

    fn properties(&self, doc: u32) -> Result<crate::docinfo::Properties, String> {
        match self.ask(doc, &Request::Properties)? {
            Reply::Properties(properties) => Ok(*properties),
            other => Err(mismatched("properties", &other)),
        }
    }

    fn append(&self, doc: u32, plan: &crate::edits::Plan) -> Result<crate::save::Update, String> {
        match self.ask(doc, &Request::Append { plan: plan.clone() })? {
            Reply::Append(update) => Ok(update),
            other => Err(mismatched("append", &other)),
        }
    }

    /// Answered from this process's own record, without asking a worker.
    ///
    /// The pool is where a password lives for the document's lifetime --- see
    /// [`Held::password`] --- so there is nothing to ask a child for, and asking
    /// would be worse than pointless: it would put the value back on a pipe it
    /// has already crossed once.
    fn password(&self, doc: u32) -> Result<Option<String>, String> {
        Ok(open_slot_mut(&mut self.lock(), doc)?.password.clone())
    }

    /// Drops the document, which kills every process holding it.
    ///
    /// **It waits for the pool to come home first**, and that wait is what keeps
    /// the guarantee the single-threaded version got for free. Dequeue order is
    /// still FIFO, so a close is taken off the queue after every request made
    /// before it --- but with several threads those requests may still be
    /// *running*, in workers this is about to kill. Draining first means a
    /// request never loses its worker mid-render.
    ///
    /// No goodbye on the wire. `Worker`'s own `Drop` kills and reaps, and a
    /// request to exit cleanly would be a message the one process in this design
    /// that is *assumed hostile* gets to ignore --- the reader would then wait on
    /// a shutdown that never comes.
    ///
    /// **The pool is killed outside the lock**, as it is in [`Workers::discard`]
    /// and [`Workers::retire_idle`] and for the same reason: a kill and reap is
    /// 1.2 ms each, and a grown pool is six of them, so closing one document
    /// under the table would stall every *other* document's checkouts for the
    /// duration. Emptying the slot first is what makes that safe --- see below.
    fn close(&self, doc: u32) -> Result<(), String> {
        let mut docs = self.lock();
        loop {
            // Looked up every time round, and inside the loop, because a second
            // close of the same id must be an error rather than a wait that
            // never ends. Unlike a withdrawal, a caller here *does* know what it
            // has open.
            let held = open_slot_mut(&mut docs, doc)?;
            if held.idle.len() >= held.spawned {
                break;
            }
            docs = self.returned.wait(docs).unwrap_or_else(|e| e.into_inner());
        }
        // Taken rather than overwritten, so the processes can be killed after
        // the lock is given back. The drain above is what says this is the whole
        // pool: every worker is home, so all of them are in the `idle` this now
        // owns, and nothing can be handed back to a slot that is already a hole
        // --- `checkin` drops a worker whose document has gone, which is exactly
        // what this does to the rest of them.
        let held = docs[doc as usize].take();
        drop(docs);
        drop(held);
        Ok(())
    }

    fn release_all(&self) -> Result<usize, String> {
        // The ids first, under the lock; the closes afterwards, outside it.
        // **Not a style choice**: `close` takes this same lock and then waits on
        // `returned` for the pool to come home, so holding it across the calls
        // would deadlock on the first document with a worker still out --- and
        // deadlock is the one failure mode a *cleanup* path must not have, since
        // it runs while a reader is waiting for their first page.
        let open: Vec<u32> = {
            let docs = self.lock();
            docs.iter()
                .enumerate()
                .filter(|(_, slot)| slot.is_some())
                .map(|(id, _)| id as u32)
                .collect()
        };

        let mut released = 0;
        for doc in open {
            // A slot that went between the read and the close is not an error
            // here, where a caller closing one id twice is. The difference is who
            // is asking: `close` answers a caller that says it has this document,
            // and being wrong about that is worth saying. This is a sweep, and
            // nothing named the ids.
            if self.close(doc).is_ok() {
                released += 1;
            }
        }
        Ok(released)
    }
}

/// A worker answered, with the wrong thing.
///
/// The one failure the [`Reply`] enum does not remove: it makes both ends
/// agree about a payload's *shape*, and nothing in it pairs a reply with the
/// request that asked. A worker sending `Reply::Links` to a `Request::Outline`
/// is a well-formed message and a broken conversation.
///
/// It cannot happen while one process writes the dispatch in
/// `worker_child::answer`, which is a `match` over `Request` -- so this is a
/// guard against that dispatch being edited wrongly, not against the wire.
/// Named so the message says which end is confused: the request that was
/// asked, and the variant that came back.
fn mismatched(asked: &str, got: &Reply) -> String {
    format!("a worker answered {got:?} to a request for {asked}")
}

/// Serves jobs from `threads` threads sharing one receiver.
///
/// The receiver is behind a mutex and each thread holds it only across `recv`,
/// so a thread that has taken a job releases it before doing any work. What that
/// buys is the whole point of the pool: several tiles of the same document are
/// rendered at once, in different processes.
///
/// **Dequeue order is still FIFO** --- one channel, one queue --- and only
/// *execution* overlaps. That is what keeps `close` correct without a lock of its
/// own: a close is taken off the queue after everything queued before it, and
/// drains whatever is still running (see [`Workers::close`]).
/// Returns as soon as the threads are running --- it does not join them. They
/// are detached exactly as the single render thread always was: they end when
/// the last `RenderService` handle is dropped and the channel closes.
pub(crate) fn serve_pooled(rx: Receiver<Job>, engine: Arc<Workers>, threads: usize) {
    let rx = Arc::new(Mutex::new(rx));

    for index in 0..threads {
        let rx = rx.clone();
        let engine = engine.clone();
        std::thread::Builder::new()
            .name(format!("tpdf-render-{index}"))
            .spawn(move || loop {
                let job = {
                    let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                    guard.recv()
                };
                match job {
                    Ok(job) => dispatch(job, engine.as_ref()),
                    // Every sender is gone: the service was dropped.
                    Err(_) => break,
                }
            })
            .expect("failed to spawn a render thread");
    }
}

/// Kills idle workers on a timer, until the service is gone.
///
/// A `Weak`, and that is the whole design of this thread rather than a detail.
/// The reaper must not be what keeps the pool alive: the service's threads end
/// when the last [`RenderService`] handle drops and the channel closes, and their
/// `Arc`s go with them, so a failed upgrade is how this thread learns to stop. A
/// strong handle would keep every worker of every document --- and the documents'
/// mappings --- alive for the life of the process, which is a worse leak than the
/// one this function exists to fix.
///
/// The sleep comes before the upgrade, so the thread outlives the service by at
/// most one interval. That is deliberate rather than accidental: checking first
/// would spend a sweep on a service nobody has used yet.
pub(crate) fn reap_idle(engine: &Arc<Workers>, idle_after: Duration) {
    let weak = Arc::downgrade(engine);
    let interval = sweep_interval(idle_after);
    std::thread::Builder::new()
        .name("tpdf-reaper".into())
        .spawn(move || loop {
            std::thread::sleep(interval);
            // Not `while let`: the upgraded handle must be dropped before the
            // next sleep, or the service stays alive for the whole interval
            // after its last user let go of it.
            let Some(engine) = weak.upgrade() else { return };
            engine.retire_idle();
        })
        // Failure is silent for the same reason a failed prewarm is: retirement
        // is an optimisation, and a machine that cannot spawn this thread still
        // reads documents. What it does not do is grow without bound --- the
        // capacity ceiling is a separate mechanism and is unaffected.
        .ok();
}

/// Kills workers whose request has outrun the deadline, until the service is
/// gone.
///
/// A sibling of [`reap_idle`], holding a `Weak` for exactly the same reason: the
/// supervisor must not be what keeps the pool --- and every document's mapping
/// --- alive, so a failed upgrade is how it learns to stop.
///
/// A thread of its own rather than another job for the reaper, because the two
/// cadences come from unrelated policies. Folding them together would tie a
/// harness's short deadline to how promptly workers retire, and `AGENTS.md`
/// records what it cost the last time two limits in this module were made equal
/// for looking obviously related.
///
/// Failure to spawn is **not** silent, unlike the reaper's. Retirement is an
/// optimisation; this is the only thing standing between a request that never
/// returns and a service thread held for the life of the process, so a service
/// running without it is running without its per-request bound and should say
/// so.
pub(crate) fn watch_calls(engine: &Arc<Workers>, deadline: Duration) {
    let weak = Arc::downgrade(engine);
    let interval = sweep_interval(deadline);
    let spawned = std::thread::Builder::new()
        .name("tpdf-deadline".into())
        .spawn(move || loop {
            std::thread::sleep(interval);
            // Not `while let`, for the reason `reap_idle` gives: the upgraded
            // handle has to be dropped before the next sleep, or the service
            // outlives its last user by a whole interval.
            let Some(engine) = weak.upgrade() else { return };
            engine.kill_overdue();
        });
    if spawned.is_err() {
        crate::diag::note(
            "[render] no deadline supervisor: a request that hangs will hold its thread",
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::{
        overdue, payload_length, shrunk, CallWatch, Held, InFlight, Workers, DEFAULT_IDLE,
        OUTLIVED_MARK,
    };
    use crate::queue::SharedQueue;
    use crate::render::{Engine, TileFormat, TileRequest};
    use crate::worker::Response;

    /// A pool with nothing in it, for exercising the supervisor's bookkeeping.
    ///
    /// Nothing is spawned by construction --- a document has to be opened first
    /// --- so the library path is never read and need not exist.
    fn supervisor(deadline: Duration) -> Workers {
        Workers::new(
            PathBuf::from("/nonexistent"),
            SharedQueue::default(),
            1,
            DEFAULT_IDLE,
            deadline,
        )
    }

    /// A worker's refusal keeps its answerability on the way to the engine.
    ///
    /// Two representations of one distinction meet in `refusal_of`, and that is
    /// exactly the shape `docs/TRAPS.md` records as drifting --- *two copies of a
    /// distinction drift, and a mutation of one survives*. Dropping the flag
    /// there loses nothing visible in the app process except the password
    /// prompt: the reason still arrives, still reads correctly, and the reader
    /// is simply told about a document they can no longer open.
    #[test]
    fn a_locked_reply_reaches_the_engine_as_a_locked_refusal() {
        let locked = super::refusal_of(Response::locked("This document is locked."));
        assert!(locked.locked);
        assert_eq!(locked.reason, "This document is locked.");

        // The control, and the direction that matters more: an ordinary failure
        // must not become a password prompt in front of a broken file.
        let broken = super::refusal_of(Response::err("This file is not a PDF."));
        assert!(!broken.locked);
        assert_eq!(broken.reason, "This file is not a PDF.");
    }

    /// A document over a real mapping, believing in `spawned` workers.
    ///
    /// The mapping has to exist because [`Held`] owns one; nothing here reads a
    /// byte of it. The workers are believed in rather than spawned, which is the
    /// only way to reach the close drain from a unit test --- a real one needs
    /// PDFium, a document and a sandboxed child.
    fn held(name: &str, spawned: usize) -> (std::path::PathBuf, Held) {
        let dir = std::env::temp_dir().join(format!("tpdf-workers-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("doc.bin");
        std::fs::write(&path, vec![0u8; 4096]).expect("write");
        let held = Held {
            doc: std::sync::Arc::new(crate::worker::Shm::map_file(&path).expect("map")),
            idle: Vec::new(),
            spawned,
            senders: Vec::new(),
            outlived: None,
            password: None,
        };
        (dir, held)
    }

    /// An entry that has been outstanding for `age`.
    fn call(pid: u32, age: Duration) -> InFlight {
        InFlight {
            pid,
            since: Instant::now()
                .checked_sub(age)
                .expect("the clock has not been running that briefly"),
            killed: false,
        }
    }

    /// Only a file that lost bytes is a diagnosis, and "cannot tell" is not one.
    ///
    /// Every direction, because they fail to opposite mistakes. A rule that
    /// never fires leaves the reader scrolling into a region that respawns a
    /// sandboxed process and kills it once per tile, silently. A rule that fires
    /// on an unchanged file condemns every document the first time any worker
    /// crashes for any reason --- and a crash is the only path that asks.
    #[test]
    fn only_a_file_that_lost_bytes_is_a_diagnosis() {
        assert!(shrunk(4096, Some(1024)).is_some(), "truncated");
        assert!(
            shrunk(4096, Some(4095)).is_some(),
            "one byte short is short"
        );
        assert!(shrunk(4096, Some(4096)).is_none(), "untouched");
        assert!(shrunk(4096, Some(8192)).is_none(), "an appended revision");
        // The platform that cannot look. Distinct from "it did not shrink" in
        // what it means and identical in what it does, which is the point: the
        // ordinary crash path keeps its retry.
        assert!(shrunk(4096, None).is_none(), "no answer is not a finding");
    }

    /// The message carries the mark the protocol routes on, and both lengths.
    ///
    /// The mark because `protocol::failure` matches on it to answer 410, and a
    /// message that lost it would silently go back to being a retryable 400 ---
    /// visible nowhere, since the reader would see the same blank region either
    /// way. The lengths because they are the whole diagnosis for anyone reading
    /// the log afterwards.
    #[test]
    fn the_reason_says_what_happened_and_carries_the_mark() {
        let reason = shrunk(4096, Some(1024)).expect("a truncation is a diagnosis");
        assert!(reason.contains(OUTLIVED_MARK));
        assert!(reason.contains("4096"));
        assert!(reason.contains("1024"));
    }

    /// A document diagnosed as gone refuses without reserving a worker.
    ///
    /// The refusal is the visible half; **not reserving** is the half that
    /// matters and the one a message check cannot see. Before this, the crash
    /// path replaced the dead worker with one built from the same mapping, which
    /// faulted in the same place --- so a reader scrolling through the missing
    /// tail paid two process spawns and two faults per tile. `spawned` staying
    /// at zero is that, stated as an observable.
    ///
    /// The closed-slot leg is the control: it proves the early return is
    /// specific to the latch rather than a checkout that now refuses everything
    /// with one message.
    #[test]
    fn a_document_whose_file_is_gone_refuses_without_spawning_anything() {
        let pool = supervisor(Duration::from_secs(30));
        let dir = std::env::temp_dir().join("tpdf-workers-gone");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("doc.bin");
        std::fs::write(&path, vec![0u8; 4096]).expect("write");

        {
            let mut docs = pool.lock();
            docs.push(Some(Held {
                doc: std::sync::Arc::new(crate::worker::Shm::map_file(&path).expect("map")),
                idle: Vec::new(),
                spawned: 0,
                senders: Vec::new(),
                outlived: Some("gone: the file was truncated".into()),
                password: None,
            }));
            // A closed document beside it, for the control below.
            docs.push(None);
        }

        // `err()` rather than `expect_err`, which wants the *success* type to be
        // `Debug` and a `Worker` is not one --- it is a live child process.
        let refused = pool.checkout(0).err().expect("a gone document refuses");
        assert_eq!(refused, "gone: the file was truncated");
        assert_eq!(
            pool.lock()[0].as_ref().expect("still held").spawned,
            0,
            "no worker may be reserved for a document that cannot be served"
        );

        let closed = pool
            .checkout(1)
            .err()
            .expect("a closed document refuses too");
        assert_ne!(closed, "gone: the file was truncated");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A close leaves a hole, and closing that hole again is an error.
    ///
    /// The hole is the half that carries the pool out of the lock: `close` takes
    /// the entry and drops it afterwards, so a version that left the slot
    /// occupied would be killing six processes with the document table held.
    /// Both halves are asserted because the second is what makes the first
    /// distinguishable from a close that emptied nothing.
    #[test]
    fn a_close_empties_the_slot_and_a_second_close_is_an_error() {
        let workers = supervisor(Duration::from_secs(30));
        let (dir, held) = held("close-slot", 0);
        workers.lock().push(Some(held));

        assert_eq!(workers.close(0), Ok(()));
        assert!(workers.lock()[0].is_none(), "the slot was not emptied");
        assert!(workers.close(0).is_err(), "a hole cannot be closed again");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Everything held goes, and the count says how much there was.
    ///
    /// **The leak this exists for, demonstrated rather than argued.** Two
    /// documents are open and nothing has closed either --- which is the state a
    /// webview reload leaves behind, because `close_document`'s only caller in
    /// the application is a *successful subsequent open* and a reloaded page has
    /// no id to pass it. Nothing else reclaims them: no timer, no reference
    /// count, no backend-side owner.
    ///
    /// The count is asserted as well as the emptiness, because a caller cannot
    /// see this table. Without it, *nothing was open* and *everything was
    /// released* come back as the same answer, and only one of them means a
    /// webview reloaded.
    #[test]
    fn releasing_everything_empties_every_slot_and_reports_how_many() {
        let workers = supervisor(Duration::from_secs(30));
        let (first, one) = held("release-all-1", 0);
        // Both with a drained pool. `held`'s second argument is `spawned`, not
        // the document id, and a document with a worker still out makes `close`
        // wait for it --- correct behaviour, and it hung the first version of
        // this test, which is the clearest evidence available that `release_all`
        // reuses the drain rather than working around it.
        let (second, two) = held("release-all-2", 0);
        workers.lock().push(Some(one));
        workers.lock().push(Some(two));

        assert_eq!(workers.release_all(), Ok(2), "both were held");
        assert!(
            workers.lock().iter().all(Option::is_none),
            "a slot survived"
        );

        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }

    /// A hole does not count, and neither does an empty table.
    ///
    /// The control on the count, and it is the direction that matters: a sweep
    /// that reported the table's *length* rather than what was in it would say a
    /// fresh start had released something, and the one line this logs exists
    /// precisely to mean "a webview reloaded". `close` leaves holes rather than
    /// removing entries, so a table of holes is the ordinary shape here.
    #[test]
    fn releasing_nothing_reports_nothing_even_with_holes_in_the_table() {
        let workers = supervisor(Duration::from_secs(30));
        assert_eq!(workers.release_all(), Ok(0), "an empty table held nothing");

        let (dir, one) = held("release-all-hole", 0);
        workers.lock().push(Some(one));
        assert_eq!(workers.close(0), Ok(()));
        assert_eq!(
            workers.release_all(),
            Ok(0),
            "a hole is not a document, and the table still has an entry for it"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A close waits for the pool before it takes the slot, not after.
    ///
    /// The order is the whole correctness argument: taking the slot first would
    /// drop --- and therefore kill --- workers that are still inside a render,
    /// which is precisely what the drain exists to prevent. The waiting
    /// direction is asserted first because it is the one a reordering breaks,
    /// and it is asserted as "has not answered yet" rather than by timing the
    /// close, so nothing here depends on how long a wait takes.
    #[test]
    fn a_close_waits_for_the_pool_to_come_home_before_taking_the_slot() {
        let workers = std::sync::Arc::new(supervisor(Duration::from_secs(30)));
        let (dir, held) = held("close-drain", 1);
        workers.lock().push(Some(held));

        let (tx, rx) = std::sync::mpsc::channel();
        let closing = workers.clone();
        std::thread::spawn(move || {
            let _ = tx.send(closing.close(0));
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "the close did not wait for a worker that had not come home"
        );
        assert!(
            workers.lock()[0].is_some(),
            "the slot was taken before the pool was drained"
        );

        // The last worker comes home. Through the count rather than through
        // `checkin`, which wants a `Worker` --- a live sandboxed child.
        workers.lock()[0].as_mut().expect("still held").spawned = 0;
        workers.returned.notify_all();

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(Ok(())),
            "the close never finished"
        );
        assert!(workers.lock()[0].is_none(), "the slot was not emptied");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_call_past_the_deadline_is_named_and_a_younger_one_is_not() {
        // Both directions in one table, because they fail to opposite mistakes:
        // a supervisor that names nothing is a deadline that does not exist,
        // and one that names everything kills every request the moment a sweep
        // runs. Only asserting the first is a check a broken comparison passes.
        let now = Instant::now();
        let calls = [call(11, Duration::from_secs(60)), call(22, Duration::ZERO)];
        assert_eq!(overdue(&calls, Duration::from_secs(30), now), vec![11]);
    }

    #[test]
    fn a_call_exactly_at_the_deadline_is_left_alone() {
        // The boundary from the permitted side. `Instant` ticks at 41.67 ns on
        // this hardware, so an exact hit is reachable rather than theoretical,
        // and the direction that matters is the one that kills a request which
        // was about to answer.
        let now = Instant::now();
        let calls = [InFlight {
            pid: 11,
            since: now,
            killed: false,
        }];
        assert!(overdue(&calls, Duration::ZERO, now).is_empty());
    }

    #[test]
    fn a_finished_call_stops_being_watched() {
        // The property the guard exists for, and its failure is delayed rather
        // than immediate: an entry left behind names a pid whose `Worker` has
        // been dropped and reaped, so the next sweep past the deadline signals
        // whatever now holds that number.
        let workers = supervisor(Duration::from_secs(30));
        assert_eq!(workers.calls().len(), 0);
        {
            let _watch = CallWatch::start(&workers, 4711);
            assert_eq!(workers.calls().len(), 1, "the call was never registered");
        }
        assert_eq!(workers.calls().len(), 0, "the entry outlived its call");
    }

    /// A process that will not end on its own, standing in for a worker whose
    /// request never comes back.
    ///
    /// Not a worker: spawning one needs PDFium and a document, and what is
    /// under test is the supervisor's reach into the process table rather than
    /// anything PDF. What it does share with a worker is the only thing that
    /// matters here --- it is a child this process owns and has not reaped, so
    /// its pid still names it.
    /// Five seconds, not thirty: long enough that the control below cannot race
    /// it, and short enough that a kill which never lands fails the test rather
    /// than hanging the suite --- `wait` blocks, so an unkilled child would
    /// otherwise turn a red into a timeout, and `AGENTS.md` records what a
    /// verdict of "no result" costs a mutation run.
    ///
    /// Un-gated on 2026-07-30, and that is the point of the change rather than a
    /// tidy-up: `kill_pid` was a no-op on Windows, and these three tests were the
    /// ones that would have said so. Being `#[cfg(unix)]` they did not run there,
    /// so the platform where the deadline had stopped working was also the platform
    /// where nothing tested it, and the suite was green. A check that quietly stops
    /// existing on one platform is worse than one that skips out loud.
    fn sleeper() -> std::process::Child {
        #[cfg(unix)]
        let mut command = {
            let mut c = std::process::Command::new("/bin/sleep");
            c.arg("5");
            c
        };
        // No `sleep` on Windows, and `timeout.exe` refuses to run without a console
        // of its own --- it reads the keyboard, so under a redirected stdin it exits
        // immediately with "input redirection is not supported", which would make
        // every assertion below pass for the wrong reason. `ping` waits between
        // packets on a timer and needs no console: six pings to loopback is five
        // intervals, so about five seconds, matching the unix arm.
        #[cfg(windows)]
        let mut command = {
            let mut c = std::process::Command::new("ping.exe");
            c.args(["-n", "6", "127.0.0.1"]);
            c
        };
        command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the platform's sleeper is present")
    }

    /// Whether a status says the process was killed rather than that it finished.
    ///
    /// The two platforms answer in different currencies and neither accepts the
    /// other's: unix has a signal number and no exit code, Windows has an exit code
    /// and no signals. Both directions are checked --- a sleeper that ran to
    /// completion exits 0 on either, so this cannot be satisfied by a kill that
    /// never landed.
    fn killed(status: &std::process::ExitStatus) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            status.signal() == Some(9)
        }
        #[cfg(windows)]
        {
            status.code() == Some(crate::sandbox_win::KILLED_EXIT as i32)
        }
    }

    #[test]
    fn the_supervisor_kills_the_process_holding_an_overdue_call() {
        let workers = supervisor(Duration::from_millis(1));
        let mut child = sleeper();
        let _watch = CallWatch::start(&workers, child.id());
        // Past a 1 ms deadline by a margin that no scheduling delay can close
        // from the wrong side.
        std::thread::sleep(Duration::from_millis(50));

        assert_eq!(workers.kill_overdue(), 1, "nothing was found to kill");
        // And the process really is gone, which is the whole mechanism: a
        // returned count is this function agreeing with itself, where an exit
        // status comes from the kernel. A kill that never landed reaches this
        // assertion five seconds later with a clean exit code, which is a
        // failure rather than a hang --- see `sleeper`.
        let status = child.wait().expect("the child can be reaped");
        assert!(
            killed(&status),
            "the process ended some other way: {status:?}"
        );
    }

    #[test]
    fn a_deadline_kill_reaches_the_log_file() {
        // The wiring, end to end, through the site rather than through the
        // logging module: `kill_overdue` is one of the eight places whose line
        // used to exist only on a stderr that a double-clicked GUI process does
        // not have. What this asserts is that the line the supervisor writes is
        // in the file afterwards -- not that `diag` can write files, which is
        // its own module's business.
        //
        // The one test in this binary that starts the process-wide sink. It is a
        // `OnceLock`, so a second test doing the same would silently get the
        // first one's file and assert against a path nothing writes to.
        let dir = std::env::temp_dir().join("tpdf-diag-workers");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tpdf.log");
        crate::diag::start(path.clone());

        let workers = supervisor(Duration::from_millis(1));
        let mut child = sleeper();
        let pid = child.id();
        let _watch = CallWatch::start(&workers, pid);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(workers.kill_overdue(), 1, "nothing was found to kill");
        child.wait().expect("the child can be reaped");

        let written = std::fs::read_to_string(&path).expect("the log was written");
        assert!(
            written.contains(&format!("[render] worker {pid}: no reply in")),
            "the kill is not in the log: {written:?}"
        );
    }

    #[test]
    fn a_deadline_kill_is_reported_to_the_thread_that_was_waiting() {
        // The half of the mechanism the kernel cannot supply. `is_running` on a
        // worker killed microseconds ago answers "still running" --- measured
        // end to end --- so without this flag the corpse goes back into the pool
        // and the next request to take it fails instead of this one.
        //
        // Deliberately *not* a check on the kill, and the mutation that proved the
        // test above confirmed it: with `kill_pid` reverted to a no-op this one
        // still passes, because the flag is set whether or not the signal lands.
        // That decoupling is not a flaw in the test, it is the shape of the defect
        // the no-op produced --- the caller is told its worker was killed while the
        // process goes on rendering. The kill needs its own assertion, and has one.
        let workers = supervisor(Duration::from_millis(1));
        let mut child = sleeper();
        let watch = CallWatch::start(&workers, child.id());
        std::thread::sleep(Duration::from_millis(50));

        assert_eq!(workers.kill_overdue(), 1);
        assert!(
            watch.end(),
            "the thread was not told why its worker stopped"
        );
        child.wait().expect("the child can be reaped");
    }

    #[test]
    fn a_call_that_ended_on_its_own_is_not_reported_as_killed() {
        // The control, and the direction that matters more: reporting a killed
        // worker that was not killed retires a healthy process on every request
        // and answers the caller with a deadline error it never hit. No process
        // here, because nothing is signalled --- which is the assertion.
        let workers = supervisor(Duration::from_secs(3600));
        let watch = CallWatch::start(&workers, 4711);
        assert_eq!(workers.kill_overdue(), 0);
        assert!(!watch.end());
    }

    #[test]
    fn a_call_inside_its_deadline_is_left_running() {
        // The control. Without it the test above is satisfied by a supervisor
        // that kills every registered call on sight, which is the same as
        // having no deadline at all and is far worse than a wedge.
        let workers = supervisor(Duration::from_secs(3600));
        let mut child = sleeper();
        let _watch = CallWatch::start(&workers, child.id());

        assert_eq!(workers.kill_overdue(), 0);
        assert!(
            matches!(child.try_wait(), Ok(None)),
            "the child was killed inside its deadline"
        );
        child.kill().expect("tidying up the sleeper");
        child.wait().expect("the child can be reaped");
    }

    /// A tile request of a given size and format, with nothing else meaningful.
    fn request(width: u16, height: u16, format: TileFormat) -> TileRequest {
        TileRequest {
            rid: 1,
            doc: 0,
            page: 0,
            crop: None,
            scale: 1.0,
            turns: 0,
            invert: false,
            x: 0,
            y: 0,
            width,
            height,
            format,
        }
    }

    /// A successful reply claiming `bytes` of payload.
    fn reply(bytes: usize) -> Response {
        Response {
            ok: true,
            bytes,
            ..Default::default()
        }
    }

    #[test]
    fn a_raw_tile_of_exactly_the_right_size_is_accepted() {
        // The control: without it every check below passes on a function that
        // refuses everything.
        let req = request(64, 32, TileFormat::Raw);
        assert_eq!(
            payload_length(&reply(64 * 32 * 4), &req, crate::worker::TILE_CAPACITY),
            Ok(64 * 32 * 4)
        );
    }

    #[test]
    fn a_raw_tile_of_the_wrong_size_is_refused_even_though_it_fits() {
        // Well inside the mapping on purpose, so only the arithmetic can catch
        // it --- if this went over capacity too, deleting the arithmetic would
        // still leave the test green and the check unpinned.
        let req = request(64, 32, TileFormat::Raw);
        for stated in [0, 64 * 32 * 4 - 1, 64 * 32 * 4 + 1] {
            assert!(
                payload_length(&reply(stated), &req, crate::worker::TILE_CAPACITY).is_err(),
                "{stated} bytes was accepted for a 64x32 raw tile"
            );
        }
    }

    #[test]
    fn a_payload_larger_than_the_mapping_is_refused() {
        // PNG, which has no expected length, so the capacity bound is the only
        // thing standing between a worker's claim and a read past the mapping.
        let req = request(64, 32, TileFormat::Png);
        assert!(payload_length(&reply(4097), &req, 4096).is_err());
        // And the control, since a compressed tile is legitimately any size.
        assert_eq!(payload_length(&reply(4096), &req, 4096), Ok(4096));
    }
}
