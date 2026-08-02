# tpdf — Threat model

Phase 0's last item (`docs/PLAN.md` §9). The architecture it describes was already
committed to and largely measured; what was missing was the document that says what the
architecture is *for*, which claims rest on evidence, and which rest on nothing yet.

**The rule this document follows:** every mitigation below is either measured — with the
spike that measured it named — or marked untested. A control that has never been shown to
fire is indistinguishable from one that keeps passing, and this repository has been bitten
by that twice already (a crash test the optimizer deleted, a stray-file check that was
inert on macOS for months). An unmarked assertion here would be a third.

Written 2026-07-26. Reviewed against the code 2026-08-02 (`BUILD.md` release step 6).

**That review found seven claims that had drifted, and six of them drifted in the direction
this document did not warn about.** The rule above guards against a mitigation *claimed* and
absent; the fourth consecutive review found mostly the inverse — mitigations present and
disclaimed, because the sections describing them were written before they were wired and
nothing re-read them afterwards. §6 called Windows uncontained four days after it was
contained, §7.4 carried the matching residual, §T8 rested on a premise the sidebar had
falsified, §7.7 called a narrowed CSP a scaffold default, §5 named a copy of the sandbox
profile that nothing ships, and §8's re-verification commands stopped being runnable when the
spikes became `[[example]]` targets. Only one — `JOB_OBJECT_LIMIT_JOB_TIME`, claimed by §6's
table and set nowhere — was the over-claim the rule anticipates.

An under-claim is the quieter failure and the more expensive one. An over-claim is corrected
the first time someone checks it; an under-claim reads as diligence, and it is what a reader
budgets their remaining work against. So the rule needs a second half: **a mitigation marked
untested must be re-read when the thing it describes is built, and the commit that wires a
control is the commit that owes this document a line.**

---

## 1. What is being defended, in priority order

1. **Everything on the machine that is not the document.** Files, keychains, other
   applications' data. A PDF viewer is a program that runs attacker-controlled input
   through a C++ parser on demand; this is the asset that matters.
2. **The user's identity and network position.** Credentials, tokens, and the ability to
   reach an internal network the attacker cannot.
3. **The confidentiality tpdf claims to have delivered.** Unique to an editor with
   redaction in it: a document reported sanitized that is not is worse than no redaction
   feature at all, because the user acts on the claim. This asset has no analogue in a
   pure viewer and it is the one tpdf is most likely to lose.
4. **Availability.** A document that hangs the application. Lowest priority — annoying,
   not dangerous — but it is where resource limits earn their place.

## 2. Who the adversary is

**The document.** Every PDF tpdf opens is assumed hostile, whatever its provenance: opened
by hand, arrived by mail, dropped on the dock, opened by file association, pulled from a
network share, extracted from another document's attachments, or pasted in as a page from
a second file. There is no trusted-source path and there should never be one — the whole
value of the isolation is that it does not depend on knowing where a file came from.

Secondary adversaries, defended against but not the design driver:

- **The distribution channel** — a tampered download or update.
- **The dependency graph** — a compromised crate, npm package, or PDFium build.

Explicitly **not** defended against, and out of scope:

- A local attacker already executing code as the user.
- A modified build of tpdf itself.
- Physical access to an unlocked machine.
- Traffic analysis, timing side channels, and anything requiring the attacker to observe
  the machine's hardware.

## 3. Trust boundaries

Four principals, each trusting only what is below it in the table.

| Principal | Authority it holds | Authority it does not |
|---|---|---|
| **Webview** (Svelte) | Draws, receives tiles, issues commands | No filesystem, no network, no PDF parsing |
| **Coordinator** (Rust, the Tauri process) | Opens files the user chose, owns the window, spawns and kills workers, owns every shared mapping | Parses no PDF syntax on the *viewing* path — with one exception, printing, described below |
| **Worker** (Rust + PDFium) | Parses and renders whatever bytes it is handed | No filesystem, no network, no path to the document, cannot create a file |
| **Disk** | Holds the document and tpdf's output | — |

Two consequences of that table are load-bearing and worth stating separately.

**The document reaches the worker as memory, never as a path.** The coordinator opens the
file and maps it; the worker receives the *descriptor*, `dup2`'d to a fixed number before
`exec`. A descriptor has no name to guess and survives a policy that forbids opening files
at all — which is what makes a `(deny file-read*)` worker possible in the first place.
Measured in spike 0.5: a worker under that policy opens a 775-page document and renders it
pixel-identically to an unsandboxed one.

**The coordinator's "never parses PDF syntax itself" became true on 2026-07-28 on macOS and
2026-07-29 on Windows**, and not before. Until then the boundary existed and was measured,
but the viewer's own render path still opened documents in the app process; this table
described the architecture rather than the running program. `Backend::default_here` now
returns `Backend::Worker` on both — one `cfg!(any(target_os = "macos", windows))`, which is
the line that keeps this row.

What says so is not a comment, and the two platforms are attested differently:

- **macOS**: `backend-probe` reads the **dynamic linker's** image table and finds no
  `libpdfium` mapped in a process that has just opened a 775-page document and rendered a
  tile from it — then starts the in-process backend, watches the image appear, and so proves
  the scan can see one.
- **Windows**: `scripts/win_modules.py` reads the app's module list through Toolhelp from
  *outside* the process, which is stronger evidence in kind — a milestone we record says what
  our code believes it did. It was run **before** the flip and reported the parser mapped
  (47 modules at peak, `[FAIL]`); that control is why the pass after it means anything.

Everything below the first row of that table was already true of the worker; this is the row
above it catching up.

**Printing is the exception, and it is a real one.** Added 2026-07-28, and the row above
said "never parses PDF syntax itself" for two days while it did. Three call paths parse
attacker-controlled bytes inside the coordinator:

- `print_macos::read` — PDFKit, i.e. CoreGraphics — on **every** print, including the
  passthrough case where the bytes are the untrusted file verbatim.
- `print_macos::present` — the same parser again, on the **main thread**, inside AppKit's
  run loop.
- `print::build` — `lopdf` — whenever the job is not a passthrough, which today means
  whenever the reader has rotated the view.

Two of the three cannot move. `NSPrintOperation` needs the application's own window and its
`NSPrintInfo`, so the panel is in the coordinator by construction; PDFKit is also the parser
the print system will use itself, which is the whole argument for reading the job back with
it (`print_macos`). What *can* move is `print::build`'s `lopdf` rewrite and the verification
read, and neither has. Reaching this needs no more than ⌘P on an open document.

**Windows is the same exposure with a different parser, since 2026-07-30.** `print_win::read`
and `print_win::spool` both parse in the coordinator, using `Windows.Data.Pdf` where macOS uses
PDFKit — and `spool` additionally *rasterises* every page there, because Windows has no in-box
PDF print API and the pages have to reach a printer DC as bitmaps. So the Windows print path
touches attacker-controlled bytes in the coordinator more than the macOS one does, not less.

Three things bound how much that is worth:

- The parser is a Microsoft component serviced by Windows Update, not a library pinned in
  `Cargo.lock`. It is the same trade as PDFKit and it is the reason a third parser is used at
  all: `lopdf` wrote the job and PDFium drew what the reader saw, so neither can attest that
  the output is readable by anything else.
- **PDFium is not there, and that is measured rather than argued.** `examples/print_probe.rs` reads
  its own module table after parsing, rendering and printing a document, and reports 80 modules
  with none named pdfium — with the count printed, so a failed enumeration cannot read as an
  absence. A PDFium bug reachable from a crafted document is therefore not reachable through
  printing on either platform.
- It is still a `[NOT MOVED]`, not a mitigation. The honest statement is that §3's "the
  coordinator parses no PDF syntax" holds on the *viewing* path on both platforms and has never
  held on the printing path on either.

The one thing that does **not** carry over is `print_macos::read_with_text`. `Windows.Data.Pdf`
has no text API at all, so the Windows readback pins page count and rotation only; the check in
`print.rs` that used text to say *which* pages survived skips out loud there rather than
quietly not existing.

What is bounded rather than moved: both graph walks the rewrite performs — `sweep::references`
and `print::forget_in_object` — are recursive and now refuse past `sweep::MAX_NESTING` (256)
rather than descending until the stack runs out. They **refuse** rather than truncate, which
is not a stylistic choice: a mark-and-sweep that stops early has an incomplete reachable set,
so it would delete live objects and hand back a document that still parses and has holes in
it. Decompression was already bounded (§T4). See residual risk 11.

**Every buffer the worker writes into is the coordinator's allocation.** Tiles are rendered
straight into a shared mapping the parent created and sized. The worker cannot enlarge it,
so tile memory is bounded by construction rather than by supervision — which matters,
because supervision turns out to be the weaker of the two (§4, T3).

## 4. Threats

### T1 — Memory corruption in the parser

**The threat.** PDFium is native C++ parsing an attacker-controlled file format with
recursive object graphs, a dozen stream filters, and thirty years of accumulated
compatibility. Chrome sandboxes it in a separate process; the reasoning transfers exactly.

**What stops it.** Parsing and rendering happen only in worker processes with no
filesystem or network authority. A worker that is compromised holds nothing worth having:
it cannot open a path, cannot write a file, cannot bind a socket, and can reach the
document it was already given and one tile buffer.

**Evidence** (spike 0.5, `worker-bench --mode crash`): a worker killed by SIGABRT, by
SIGSEGV, or exiting non-zero is noticed by the coordinator within **0.1–0.6 ms**, as an
EOF on the line-delimited control channel. Respawning, reopening the 775-page document and
rendering the first tile costs **8.5–12.9 ms**. The coordinator is unaffected in every
case. The boundary itself is not a reason to hesitate: a control round trip is **6 µs**
and moving a 4 MB tile through shared memory is **0.11 ms**, against **3.0 ms** to hand
the same tile to the webview. Isolation costs about 1/27th of the UI.

Two qualifications on that 0.11 ms, added 2026-07-31 without changing the conclusion. It is
an **upper bound**: it comes from `worker-bench --mode latency`, whose estimator leaves its
own subtraction error in the answer, and that error is as large as the figure (trap: *"A
baseline that skips the expensive step leaves its noise in the answer"*). And it is the
*prototype* worker, not the shipped one --- `latency-bench` puts the **production** `Worker`
at **0.071--0.103 ms** per tile on macOS and 0.269--0.309 ms on Windows, measured against a
control that holds its residual to 0.001 ms. Every one of those is still one to two orders
of magnitude under the webview hand-off, so "isolation costs a small fraction of the UI"
stands on the better numbers as well as the original ones.

**Residual.** A worker compromise can still lie about what it rendered or extracted. Any
security-relevant answer — above all a redaction verification — must therefore not be
taken on a worker's word alone; see T5.

One class of lie is now refused rather than believed, and only one. A reply states how many
bytes of the shared mapping it wrote, and the coordinator checks that claim before reading:
against the mapping's size, and — for raw pixels, where the answer is arithmetic rather than
a bound — against `width x height x 4` exactly, so a wrong length is refused even when it
fits. A reply *line* is bounded too, at 32 MB, because `read_line` on a pipe is otherwise
unbounded and a worker made to emit an endless one would take the coordinator down with it:
perfect isolation, dead application. Neither bound makes the content trustworthy. They stop
a compromised worker reaching past the buffers it was given, which is a different and much
smaller claim.

### T2 — Execution through the document's own features

**The threat.** PDF carries document-level JavaScript, launch actions, URI actions,
embedded executables, and XFA. These are format features, not bugs, and Acrobat runs
several of them.

**What stops it.** Nothing in tpdf ever invokes them — and, more usefully, the vendored
PDFium build cannot.

**Evidence** (`worker-bench --mode engine`, and a read of `pdfium-render` 0.9.3):

- The macOS build contains **zero `v8::` symbols and no real `CJS_Runtime`** — only
  `CJS_RuntimeStub`, whose `ExecuteScript` disassembles to three instructions that zero
  the output and return. There is no engine to disable.
- It contains **zero `CXFA_` symbols**. XFA is not built in, so §6's XFA refusal is a
  property of the binary rather than a policy that could be forgotten.
- **On Windows neither of those is established, and the check says so.** The shipped
  `pdfium.dll` carries no local C++ symbols — `CPDF_Document` is absent — so `v8::` and
  `CXFA_` being absent from it means nothing, and `worker-bench --mode engine` reports
  `[NOT VERIFIED]` rather than a clean bill. That is the second control doing its job, and
  it is the honest state: on Windows the no-engine property rests on the **asset name and
  the pinned digest** that `scripts/fetch_pdfium.py` asserts, which is a claim about
  *which file was fetched* rather than about what is in it. Weaker, and stated as weaker.
- The Windows DLL **exports four XFA-named functions** — `FPDF_LoadXFA` and
  `FPDF_GetXFAPacket{Count,Name,Content}` — which the export table shows and stripping
  cannot hide. Read as surface, not as a contradiction: the three `GetXFAPacket*` calls
  read the `/XFA` streams out of an AcroForm dictionary and need no XFA implementation
  behind them. Whether `FPDF_LoadXFA` is a stub there is **open**, and it is the one part
  of this section that *is* behaviourally decidable: a fixture carrying an `/XFA` packet
  makes `FPDF_GetXFAPacketCount > 0` a positive control, so `FPDF_LoadXFA` returning false
  on it would mean the implementation is absent rather than the document empty. Not
  written — that fixture does not exist.
- `pdfium-render` never calls any `FORM_Do*` function — not `FORM_DoDocumentOpenAction`,
  not `FORM_DoDocumentJSAction`, not `FORM_DoDocumentAAction`. Those are the only entry
  points through which PDFium executes document script.
- Its `FPDF_FORMFILLINFO` sets `m_pJsPlatform` to null and every callback to `None`, so
  even a fired action has no platform to open a URL, launch a file, mail, upload, or
  download with.
- **There is a second caller as of 2026-07-31, and it takes the same posture.**
  `progressive.rs` builds its own environment, because the raw cancellable path has no
  `pdfium-render` wrapper to inherit one from and interactive widget values are invisible
  without it. Its `FPDF_FORMFILLINFO` is zeroed before `version` and `xfa_disabled` are
  set, so `m_pJsPlatform` is null and every callback is `None` for the same reason rather
  than by copying the same lines. It calls `FORM_OnAfterLoadPage`, `FORM_OnBeforeClosePage`
  and `FPDF_FFLDraw`, and **no `FORM_Do*` function** — not `FORM_DoPageAAction` either,
  which is the page-level counterpart the two document-level ones above do not cover.
  Checked by grep over `src-tauri/src`, which is the whole of it: the string does not
  appear.

**JavaScript** cannot be tested behaviourally. A document whose script does nothing looks
exactly like a document whose script was never run, so the absence of an effect is not
evidence of the absence of an engine. The symbol table is the only thing that
discriminates, which is why the check reads the binary — and why a platform whose binary
has no symbol table to read leaves this at `[NOT VERIFIED]`.

**XFA** is the exception, and the earlier text over-generalised by lumping the two
together. It has a return value and an independent positive control, per the bullet above,
so it can be settled behaviourally where the symbol scan cannot reach.

**Residual, and it is real.** `FPDFDOC_InitFormFillEnvironment` *is* called on every
document open by `pdfium-render`, so the form-fill machinery is reachable attack surface
even with nothing behind it — this is T1 surface, not T2 execution, but it is surface that
a viewer with no form support did not have to expose. And all of the above is a property
of *this* PDFium build: it must be re-checked after every bump, and a build that ships V8
would silently move this threat from "impossible" back to "policy".

### T3 — Resource exhaustion

**The threat.** A decompression bomb, an A0 CAD page, a 25,000-object graph, or a page
that simply takes forever. None of these requires a vulnerability.

**CPU is bounded per request, by the coordinator's own deadline — wired in the app, on both
platforms since 2026-07-30.** A request outstanding longer than `TPDF_CALL_MS` (default
**30 s**) has its worker killed: `workers::watch_calls` sweeps the in-flight table on a timer,
`kill_pid` ends the process, the read blocked on that worker's pipe reaches EOF, and
`Workers::with_worker` discards the corpse and answers the caller with an error. Started by
`RenderService::start_tuned` beside the idle reaper, since 2026-07-29. It covers every request
that waits on a worker, `Open` included — that one is not served through the pool and would
otherwise be watched by nothing.

**This paragraph was false on Windows for a day, and the way it was false is the reason to
record it here rather than only in the trap index.** `kill_pid` was `#[cfg(not(unix))] fn
kill_pid(_pid: u32) {}`, so from the moment workers started on Windows (2026-07-29) until it was
fixed, the platform had **no CPU bound on a request at all** — while this section said it did.
Worse than absent: `kill_overdue` still counted the pid, set the killed flag and logged *"worker
killed for exceeding its deadline"*, so the caller received a deadline error and the log recorded
a kill that had not happened, leaving one process per hung document rendering forever. The three
tests covering the mechanism were `#[cfg(unix)]` as well, so the suite was green.

It is now `OpenProcess` + `TerminateProcess(sandbox_win::KILLED_EXIT)` — a distinct exit code
because Windows has no signal number to carry "did not choose to exit" — with those tests
un-gated and shown to fail against the no-op. The general lesson for this document, which
`BUILD.md`'s review step already states and which this instance is the strongest evidence for:
**a mitigation written in the present tense is a claim about a specific line, and a `cfg` on that
line can retire it without touching the sentence.**

One detail is worth recording because it looks like an implementation choice and is not: the
supervisor marks the request it is about to kill, and the waiting thread reads that mark
rather than asking the kernel. A child's pipe closes on the way out and it becomes waitable
slightly later, so `try_wait` answers *"still running"* for a process `SIGKILL`ed
microseconds earlier — measured, by running the app's own probe under `TPDF_CALL_MS=1`.
Believing it would return the corpse to the pool, where it would fail a different request
than the one that was actually too slow.

The deadline is not a refinement of the withdrawal mechanism, it is the only bound the
other request kinds have. Only a tile can be withdrawn; `Text`, `Search`, `Outline` and
`Open` hold a service thread until they answer, and there are `pool + 2` of those *shared
across every open document* — so one page that never finished parsing stopped the viewer
answering anything at all, and `Workers::close` then hung on its own drain waiting for a
worker that was never coming back.

**`RLIMIT_CPU` is measured and deliberately not set.** It is accepted on macOS and does
fire, but it counts CPU over the *process lifetime*, not per request: under a 3 s limit a
1.72 s render succeeds and the next dies 1.30 s in, at a cumulative 3.0 s (spike 0.5,
`worker-bench --mode limits`). It can bound how long a worker lives and cannot bound a
request, which is the thing that needed bounding, and a lifetime budget on a pooled worker
kills a reader's third page for the sins of the first two. The kill-and-respawn cost the
deadline pays instead is **1.2 ms to kill and reap, 4.8 ms to respawn**.

**This section read "CPU is bounded, in two layers" until 2026-07-29, and neither layer was
in the app.** `setrlimit` was called in the spike binary and nowhere in the shipped path;
"the coordinator's own deadline plus a kill" named a mechanism that did not exist, and gave
the timing of the kill it would have performed. That is precisely the failure this
document's opening rule exists for, arriving from an angle it did not anticipate: not an
unmeasured claim, but a *measured* one — every number in the sentence was real — describing
a mitigation nobody had wired. A measurement reads exactly like a deployment. Every
mitigation below now says which of the two it is.

PDFium's progressive API (`FPDF_RenderPageBitmap_Start` with an `IFSDK_PAUSE` callback) is
the cooperative alternative and is exercised for tiles only (`progressive.rs`) — it is the
mechanism for cancelling without discarding the work, and so for not occupying a worker's
only PDFium thread through a long render. The deadline is the blunt version: it ends the
process, and the work is lost rather than paused.

**Memory *does* have a kernel bound on Windows, and it is measured.** The job object every
worker is created into sets `JOB_OBJECT_LIMIT_PROCESS_MEMORY` with a cap, plus
`ActiveProcessLimit = 1`. Both were claimed by `win_sandbox_probe`'s own table and tested by
nothing until 2026-07-30, which is the shape this document exists to catch: its three authority
probes are all integrity-level properties, so every rung reported on `lowil` and above while the
job's limits went unexercised. Now probed, with the uncontained rung as the control — `bare`
commits 1 GB and starts a second process; every rung with a job is refused with `1455`
(`ERROR_COMMITMENT_LIMIT`) and `1816` (`ERROR_NOT_ENOUGH_QUOTA`).

The asymmetry with macOS is worth stating precisely, because it makes the Windows bound the
*stronger* of the two rather than merely the different one: Windows charges **committed** memory
at `VirtualAlloc` time, so an allocation past the cap is refused before a byte of it exists. A
decompression bomb is stopped one step earlier than any sampling scheme can manage, and the
"polling bounds a leak, not a burst" negative result below does not apply there. It is also why
`Worker::footprint` returning `None` on Windows is not the gap it resembles.

**Memory has no kernel bound on macOS, and the substitute is designed, measured, and NOT
wired.** `setrlimit` refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright with
`EINVAL` (spike 0.5, confirmed independently through Python's `resource` module). The
remaining mechanism is supervision: sample the worker's `ri_phys_footprint` through
`proc_pid_rusage` and kill it over budget. `Worker::footprint` is that sample, and **it has
no caller in the shipped app** — nothing polls it, so no worker's memory is bounded by
anything today. Measured 2026-07-26 (`worker-bench --mode footprint`), against a child
taking memory as fast as the allocator will hand it over (~22 GB/s) and a 128 MB budget:

| poll | overshoot, median | worst seen | what the interval bounds | poll costs | bursts missed |
|---|---|---|---|---|---|
| 0 ms | 0.0 MB | 0.0 MB | — | 100% of a core | 0/5 |
| 1 ms | 16.4 MB | 18.3 MB | 22 MB | 0.033% of a core | 0/5 |
| 5 ms | 22.4 MB | 88.7 MB | 113 MB | 0.007% of a core | 0/5 |
| 20 ms | 225.8 MB | 225.8 MB | 280 MB | 0.002% of a core | 4/5 |
| 50 ms | 368.7 MB | 368.7 MB | 483 MB | 0.001% of a core | 4/5 |

A sample costs **0.33 µs**, so polling is essentially free and the interval is a pure
overshoot-versus-nothing trade. Three things this measurement establishes:

- **Overshoot is interval × growth rate**, and the worst case is what a budget must be set
  from, not the median. Neither the median nor the worst *observed* is the bound, because
  both depend on where the crossing happens to fall between two samples; the arithmetic
  bound is the column that matters.
- **Polling bounds a sustained leak, not a burst.** At 20 ms and above, most runs never saw
  the event at all — the child took its full 512 MB and exited between two samples. This is
  the important negative result: supervision cannot be the only memory defence, because a
  bounded burst can complete inside one sampling gap and never be attributed to anything.
  Bounding the *inputs* — decompressed stream size, tile dimensions, page count per
  request — is the layer that catches those, and it is not optional.
- **A zero interval is not free supervision.** It burns a core, and the low overshoot it
  shows is partly bought by starving the child of the CPU it was allocating with.

A pool of N workers must therefore be budgeted at (per-worker budget + bounded overshoot)
× N. The overshoot term is exactly the price of having no kernel limit — and on Windows it
should disappear, since a job object with `JOB_OBJECT_LIMIT_PROCESS_MEMORY` is a real
kernel bound that needs no polling.

**Why the poll is still unwired, now that a supervisor thread exists to host it.** The
missing piece is not the mechanism, it is the budget. A worker legitimately holds its own
parse of the document (7.8–48.2 MB by corpus), a 16 MB tile mapping, and whatever a single
page's render allocates on the way — and the peak of a *legitimate* worst case, the A0
sheet at high zoom or the 337 MB scan, has never been measured. A budget set below that
kills documents readers are entitled to open, which is a worse failure than the leak it
would bound, and this document's own rule forbids putting a number here that no spike
produced. What a sample costs is known (**0.33 µs**); what it should refuse is not. That
measurement is the work, and the wiring is an afternoon after it.

**Decompression is bounded at the parser.** `lopdf`'s `LoadOptions::max_decompressed_size`
refuses a 1 GiB-inflating stream in 0.3 ms (spike 0.4). Worth remembering why the bound
belongs on the rewriter and not only on a verifier: `qpdf in out` re-encodes stream data
by default and so fully decodes that same stream, costing **1.92 s of CPU at 8.4 MB
resident** — 600× amplification in time, at no cost in memory. A limit expressed in
megabytes would have caught none of it.

**Residual.** One pathological page still occupies its process's single PDFium thread and
starves every other render there. Note this is our own threading choice, forced by the
fact that concurrent PDFium calls crash --- `pdfium-render`'s `thread_safe` feature does
not serialize them, whatever its README says (AGENTS.md). It no longer does so
indefinitely: one deadline is the bound, since a request killed for exceeding it is the one
death `Workers::with_worker` does **not** retry — retrying would spend a second deadline of
a service thread to learn what the first established. The deadline is still a coarse
instrument, in that the process dies and every partial render goes with it.

Memory is the larger residual and it is unbounded, not merely coarsely bounded: neither the
kernel's limit (refused) nor ours (unwired) applies to a worker on macOS today. Bounding the
*inputs* — decompressed stream size, tile dimensions, pages per request — is the layer that
would catch a sub-interval burst even with the poll running, and only the tile bound exists
(`protocol.rs`, refused before a worker is asked).

### T4 — Filesystem and network reach from a compromised worker

**The threat.** T1's payoff. A worker that has been taken over wants to read `~/Documents`,
write a launch agent, or open a socket.

**What stops it.** `sandbox_init` with the profile in §5, applied after the mappings are in
place and PDFium is bound, and irrevocable thereafter. Reads, writes and socket binds are
all denied and the render is unaffected, because the document never arrives as a path.

**Evidence** (spike 0.5, `worker-bench --mode authority`): under the profile, `read
/etc/hosts`, `write temp file`, `bind tcp socket` and `bind udp socket` are all denied, and
the rendered tile is **pixel-identical** to an unsandboxed render on base-14, TrueType, CID
and the 775-page corpus.

**The trap, which cost a day.** A stricter-looking profile renders base-14 documents
*differently* while returning success — PDFium silently substitutes a font face with almost
the same amount of ink. Denying `file-read*` and allowing it back on the font directories
does not fix it, because what the font mapper needs is **metadata** reads across the whole
filesystem, not data reads from the font directories. Hence the shape of the profile below.
The general rule, and it is the third time this shape has appeared in this project: **verify
a sandbox by comparing pixels, never by checking that the render returned `ok`.**

**Residual.** A hostile document can still learn which paths exist. It cannot read one,
write one, or open a socket. `sandbox_init` denials also do not appear in the unified log
without an explicit report clause, so "no log entries" is not evidence that nothing was
denied.

### T5 — False assurance

**The threat.** tpdf tells the user a document is redacted, or that an edit was applied
faithfully, and it is not true. This is the threat this project is most exposed to, because
it is the one the competition fails at and the one a user cannot check.

**Every known instance is a case of a check that cannot see what it claims to certify.**
Collected here because they are one failure, arriving from five directions:

- **A byte scan cannot verify a document with a Type0 font.** Under Identity-H the content
  stream carries glyph ids, not text, so a secret drawn on the page is never present in the
  file as its own bytes. Spike 0.3's own leak scanner called a CID fixture clean while text
  extraction proved the needle was still there.
- **A clean pixel diff is not evidence of a faithful edit.** PDFium regenerates page
  content wholesale on any object edit; spike 0.3 measured marked content and its
  `/ActualText` being discarded while every pixel matched.
- **`set_text()` draws `.notdef`, or codes for glyphs that do not exist, and returns
  success.** In one of the two measured cases displayed text and extracted text disagree —
  a search hit on text nobody can see.
- **An object a prior revision overwrote is reachable by no parser.** It is handed to
  nothing: not a graph walk, not `qpdf --check`, not PDFium. A file with more than one
  revision cannot be certified, only rewritten and then certified.
- **`lopdf` silently drops encryption on save**, and **PDFium accepting a file is not
  evidence the file is well formed** — it rendered a document with a wrong `/Size`
  pixel-identically to a correct one.

**What stops it.** One rule, stated in `docs/PLAN.md` §6 and repeated here because it is the
core of this threat: **a verifier must decode each carrier in that carrier's own encoding,
and a carrier it cannot decode makes the result "not verified", never "clean".** "Grep found
nothing" is not evidence. Verification re-parses with an independent parser, and that
requirement paid for itself the first time it ran, on a bug in tpdf's own object sweep.

**Residual, and it is the largest open risk in the project.** The rule as written refuses
almost every scanned document, because `/DCTDecode`, `/CCITTFaxDecode`, `/JBIG2Decode` and
`/JPXDecode` are all carriers the sanitizer does not decode (§10 q9). Where the line
between "cannot decode" and "is an image and belongs to a different check" sits has not been
established on a real corpus. Until it is, tpdf must refuse rather than reassure.

### T6 — What the save path leaves behind

**The threat.** The bytes a redaction was supposed to remove surviving in the file, or next
to it.

**What stops it.** Applying a redaction is a **full-rewrite barrier**: an incremental save
appends and leaves the original bytes intact, which is exactly what redaction must not do.
But a non-incremental save is not sufficient either — a serializer can carry over
unreachable objects, unused resources and embedded originals, and overwriting in place can
leave trailing bytes past the new `%%EOF`. So redaction writes a **fresh file from a
garbage-collected reachable object graph**, then atomically replaces the target.

**Evidence** (spike 0.4): a collected `lopdf` rewrite reaches the same verdict as QPDF on
all eleven hostile fixtures. Two conditions attach, both measured: `lopdf`'s own
`prune_objects`/`renumber_objects` are quadratic (1.41 s on a 25,583-object graph, against a
mark-and-sweep whose cost is indistinguishable from not collecting at all), and after
sweeping, `max_id` must be lowered by hand or
`/Size` overstates the file — `qpdf --check` rejects that and PDFium does not notice.

**Residual, and it must be said in the UI.** This sanitizes the PDF. It does not sanitize
previous copies, backups, versioned snapshots, or recoverable filesystem sectors. And on a
signed document there is a further limit: spike 0.6 measured that an appended update leaves
a signature cryptographically intact and rejected by difference analysis at **every** DocMDP
level, including an annotation-only edit to a level-3 certified document that the
specification explicitly permits. "The spec permits this edit" and "a validator will accept
it" are different claims, and only the second is what a user sees.

### T7 — Distribution and update

**The threat.** A tampered download, a tampered update, or a compromised dependency —
including the PDFium binary itself, which is prebuilt from a third party.

**What stops it, and what does not yet.** Direct notarized distribution with a signed
update payload is the intended shape, following `screenpick`. Two things are settled by
policy rather than by code today: the PDFium build's provenance (pinned build, re-checked
after every bump — see T2, and the `remove_probe` standing regression) and permissive-only
licensing, which keeps the dependency set small enough to audit.

**Untested.** The signing and notarization path for a bundled dylib (§10 q7) is a known
open question, and it bit `screenpick`'s release path before.

### T8 — The webview

**The threat.** Content injected into the UI layer reaching Tauri's command surface.

**What stops it.** The webview loads no remote content: the frontend is bundled, and tiles
arrive over a custom URI protocol as raw pixels rather than as anything parsed as markup.
Document text that must be displayed — outline entries, search results, form field labels —
is attacker-controlled and must be treated as data at every point.

**This section said "today none of it reaches the UI at all — the frontend renders tiles and
nothing else" until 2026-08-02, and that stopped being true when the sidebar and search
landed.** Outline titles reach the DOM (`sidebar.ts`, `title.textContent = row.title ||
"(untitled)"`) and so does every search result (`results.ts`), including the matched
substring the query is highlighted in.

The mitigation survived the change, and it is a **better** one than the sentence it replaces,
because it is a property of the code rather than an absence of features: every one of those
strings is assigned through **`textContent`**, which sets character data and never parses
markup. There is no markup-parsing sink anywhere in `src/` — no `innerHTML`, no `outerHTML`,
no `insertAdjacentHTML`, no `document.write`, no Svelte `{@html}` — checked by grep over the
whole frontend, which is the whole of it.

Two things follow, and the first is the one that changed:

- It is now a **testable** property rather than a rule to hold to. "No markup sink exists" is
  a grep, and "document strings go through `textContent`" is a grep. Neither is wired as a
  check yet, which makes this the one mitigation in this document enforced by a convention
  and a reading rather than by a line — see residual risk 7.
- A sink added later would not fail anything. `textContent` is the default idiom in this
  frontend rather than a decision recorded anywhere near the code that depends on it.

**CSP is real and is not the scaffold default**, which this document also had wrong.
`tauri.conf.json` sets `default-src 'self'` with `img-src`/`connect-src` widened only to the
tile protocol and the IPC origin, and no `'unsafe-inline'` anywhere. Tauri's scaffold ships
`"csp": null` — no policy at all — so this is a narrowed policy, not an unexamined one. What
is still scaffold is the **capability set**: `core:default` plus `dialog:allow-open`, where
`core:default` is the template's own bundle and has not been pared to what the app calls.

## 5. The sandbox policy

macOS, applied in the worker after the mappings are in place and PDFium is bound, and
irrevocable thereafter. The authoritative copy is **`worker::SANDBOX_PROFILE`**
(`src-tauri/src/worker.rs`), which `worker_child.rs` applies to itself; it is reproduced
here because a threat model that describes a policy without showing it cannot be checked.

**This section named the wrong copy until 2026-08-02**, pointing at `PROFILE_WORKER` in
`src-tauri/examples/worker_bench.rs` — the *spike's* profile, which nothing ships. The two
agree today, and §8 below had the right file the whole time, so the document disagreed with
itself about which text governs. That is worth more than the typo it resembles: the shipped
profile and the bench profile are **two copies of one distinction with nothing asserting
they match**, which is the trap of that name. A bisection run against the bench copy
certifies the bench copy. If they are ever to diverge, the bench is where it will happen
silently, because it is the one with no user.

```
(version 1)
(allow default)
(deny network*)
(deny file-write*)
(deny file-read*)
(allow file-read-metadata)
(allow file-read-data
  (subpath "/System/Library/Fonts")
  (subpath "/Library/Fonts"))
```

It is an allow-by-default profile that removes the three authorities that matter, which is
weaker than a `(deny default)` profile and is what actually works — a deny-default worker
renders base-14 documents with substituted fonts and reports success. The policy was
arrived at by **bisection, not by reasoning**: `worker-bench --profiles` accepts raw SBPL so
a candidate can be narrowed from the shell without a rebuild, and every candidate is judged
by comparing pixels against an unsandboxed render.

Two rules attach to changing it. Verify by pixels, never by a return code. And re-verify on
a base-14 fixture specifically — an embedded-font document is pixel-identical under a
profile that is badly wrong.

### 5.1 A second boundary, for OCR

Defined 2026-07-31 in `src-tauri/src/ocr.rs`; no engine is implemented yet, so nothing below
is running in production. It is recorded here because the *shape* is a trust-boundary
decision and it was measured rather than assumed.

OCR cannot run under the profile above. Measured with `scripts/vision_sandbox_probe.swift`,
which applies a profile to itself post-launch exactly as `worker_child.rs` does — running it
under `sandbox-exec` instead applies the profile before `exec`, and the process then dies in
dyld, which reads as "Vision cannot be sandboxed" when it only means the loader was denied:

| profile | macOS Vision |
|---|---|
| the profile above | **killed, SIGTRAP** |
| `+ file-read-data` on all of `/System/Library` | ran, then failed with `nilError` |
| `+ file-read` allowed entirely | read the control string back |

General `file-read` is exactly what §4's T4 exists to deny a worker, so relaxing this profile
to fit an engine into the parser worker would trade away the containment that worker is for.

It does not need that boundary. The parser is contained because it consumes **attacker-authored
structure**; a recogniser consumes a fixed-size RGBA buffer *we* rendered — no format to parse,
no lengths to trust, no recursion. So a second worker with its own profile keeps the two
authorities that still matter:

```
(version 1)
(allow default)
(deny network*)
(deny file-write*)
```

It stays a separate **process** for a reason unrelated to authority: the first rung above is an
engine aborting its host. Anything that can do that must not share a process with unsaved
annotations, whatever it is allowed to read.

**This narrows T5's claim rather than widening it.** OCR is the only check that can speak about
an image carrier, since a byte scan cannot see into a `/DCTDecode` stream. `ocr.rs` therefore
makes "clean" unreachable except through a positive control the engine had to read back from
the same probe image, sized from the smallest box the redaction covered — a control drawn larger
than the redacted text proves only that the engine reads larger text. Every engine failure, and
a missing control, produce `NotVerified`, never `Illegible`.

## 6. Windows — a policy, and a different one

Contained since 2026-07-29, and it shares no mechanism with §5.

**This section was titled "a gap, not a policy" and said "none of it is wired" until
2026-08-02, four days after it was wired.** It is the largest instance of the failure this
document's review step exists to catch, and the *inverse* of the usual one: not a mitigation
claimed and absent, but a mitigation present and disclaimed. Both are dangerous and this
direction is the quieter of the two — an over-claim gets corrected the first time someone
checks it, while an under-claim reads as diligence and is what a reader budgets their
remaining work against. Anyone planning from this section on 2026-08-01 would have scheduled
a Windows sandbox that already existed, and anyone reasoning about §7.4's residual would have
carried a risk that had been closed.

**The mechanism, and where each half lives.** macOS gets its boundary from `sandbox_init`,
which the child applies *to itself* after `exec` — there is a "before" in which to bind
PDFium. Windows has no counterpart, so the **parent** builds the boundary instead, while the
child is still suspended and has executed no instruction: a low-integrity token inside a job
object (`sandbox_win::Containment`, `Job::create`, `low_integrity_token`). Spawning is
`Worker::spawn`; selecting it is `Backend::default_here`, which returns `Backend::Worker` on
both platforms.

**What low integrity buys is write-denial and process-isolation, not read-denial.** A
contained worker could still read any file the user can. That is why the document and the
output are handed over as **inherited handles** rather than paths — the Windows analogue of
the macOS `dup2`, and a structural necessity here rather than a convenience.

**The stronger rung is not reachable by a flag.** A restricting SID would deny reads too, and
kills the child in the loader with `STATUS_DLL_NOT_FOUND` before `main`, because on Windows
the token is in force from the first instruction. Reaching it needs Chromium's initial-token
/ lockdown-token handover, which is real work rather than a parameter
(`examples/win_sandbox_probe.rs` measured all six rungs).

The shape, with every Windows cell either wired or marked:

| macOS | Windows |
|---|---|
| `sandbox_init` SBPL profile (`worker::SANDBOX_PROFILE`), applied post-`exec` | Job object + low integrity, applied by the parent pre-`resume` (`sandbox_win`) — **wired**; restricting SID blocked on the loader, **not reachable** |
| No memory rlimit; a `proc_pid_rusage` poll is measured and **not wired** (§T3) | `JOB_OBJECT_LIMIT_PROCESS_MEMORY` at `WORKER_MEMORY_CAP` (**1 GiB**) — a real kernel bound, no polling, **wired** |
| Parent deadline per request, **wired** (`workers::watch_calls` + `kill_pid`); `RLIMIT_CPU` measured and not set, being a lifetime budget | Parent deadline, the same one, **wired** — `kill_pid` is `OpenProcess` + `TerminateProcess`. `JOB_OBJECT_LIMIT_JOB_TIME` **not set**, deliberately: see below |
| — | `ActiveProcessLimit = 1`, `KILL_ON_JOB_CLOSE`, `DIE_ON_UNHANDLED_EXCEPTION` — **wired**, no macOS counterpart |
| Unlinked temp file passed by descriptor | Section object, passed as an inherited handle |
| `dup2` to fixed fds before `exec` | `DuplicateHandle` into the suspended child's table, the number named in argv |

**`JOB_OBJECT_LIMIT_JOB_TIME` was claimed by this table and set nowhere**, found in the same
review. It is now marked rather than wired, and the reason is that wiring it would repeat a
mistake this document has already measured its way out of once: job time is a **lifetime** CPU
budget, which is exactly the shape `RLIMIT_CPU` was rejected for on macOS (§T3 — under a 3 s
limit a 1.72 s render succeeds and the next dies 1.30 s in). A lifetime budget on a *pooled*
worker kills a reader's third page for the sins of the first two. The per-request deadline is
the bound that was wanted, it is wired on both platforms, and job time would add a second
mechanism that can only fire on the wrong thing.

**The memory row is the one where Windows is stronger, and it is stronger by construction.**
The kernel charges **committed** memory at `VirtualAlloc` time, so an allocation past the cap
is refused before a byte of it exists — a decompression bomb is stopped one step earlier than
any sampling scheme can reach, and T3's "polling bounds a leak, not a burst" negative result
does not apply here. It is also why `Worker::footprint` returning `None` on Windows is not
the gap it resembles: there is a kernel bound there instead of a poll.

Both job limits were claimed by `win_sandbox_probe`'s own table and **tested by nothing**
until 2026-07-30 — its three authority probes are all integrity-level properties, so every
rung reported on `lowil` and above while the job's limits went unexercised. Now probed, with
the uncontained rung as the control: `bare` commits 1 GB and starts a second process; every
rung with a job is refused with `1455` (`ERROR_COMMITMENT_LIMIT`) and `1816`
(`ERROR_NOT_ENOUGH_QUOTA`). `KILL_ON_JOB_CLOSE` is still only claimed — testing it means
killing the probe itself.

**Evidence that the whole path works, not just the pieces**: `worker-probe` passes 11/11 on
`text-base14`, `text-cid`, `vector-heavy` and `rotated`, including **pixel-identical** tiles
against an in-process render — so the font substitution the macOS sandbox caused did not
recur here, as `win_sandbox_probe` predicted. `backend-probe` passes 38–40/42 across four
corpora with byte-identical name sets. And the module check in §3 is external to the process,
which is what makes it evidence rather than a milestone.

## 7. Residual risk, in one place

1. **Redaction verification refuses too much** — the "cannot decode" rule has not been
   calibrated against a real corpus, and applied literally it fails almost every scan
   (§10 q9). Largest open risk in the project.
2. **A worker's memory is unbounded on macOS**, and this entry understated it until
   2026-07-29 by saying only that a burst *below the polling interval* escapes. There is no
   polling interval: the kernel refuses the three relevant rlimits, and the
   `proc_pid_rusage` poll that would substitute for them is measured in spike 0.5 and has
   no caller in the app (§T3). What is missing before it can be wired is the budget, which
   needs a measurement of a legitimate worker's peak that nothing has taken. Input limits
   are the second layer and only the tile bound exists.
3. **A document's pool multiplies its memory by up to six, while it is being scrolled.**
   Each worker holds its own parse, at 7.8–48.2 MB depending on the corpus, so a fully
   grown pool on the A0 sheet is about 290 MB. Growth is lazy — a reader turning one page
   at a time never has more than one worker — and it is given back: a worker idle for 30 s
   is killed, down to one per document, which returns 242.5 MB of that 290 on the A0 sheet
   (`pool-bench --mode retire`). What remains is the **peak during a burst**, which is not
   bounded by anything smaller than the pool size. Isolation is unaffected: every worker is
   separately sandboxed and separately killable, and one dying costs its document one
   process rather than the document.
4. **A contained Windows worker can still read any file the user can** (§6). This entry read
   "Windows compiles and is entirely uncontained ... nothing uses it, so the risk is
   undiminished" until 2026-08-02, and the containment had been wired since 2026-07-29 — the
   correction is in §6, along with why an under-claim is the more expensive direction to get
   wrong. What remains is the ceiling rather than the gap: low integrity denies writes and
   `OpenProcess`, **not** reads. Closing it needs a restricting SID, which stops the loader
   before `main` and is only reachable through Chromium's initial-token handover. The
   mitigation meanwhile is that the worker is never given a path — document and output arrive
   as inherited handles — so a compromised worker must guess at what to read rather than
   being handed it. A platform with neither mechanism still falls back to in-process, records
   `render::UNSANDBOXED_MARK` and prints a `[WARN]`; no such platform is shipped.
5. **A hostile document can enumerate paths** under the sandbox profile.
6. **The form-fill environment is initialised on every document open**, so that surface is
   exposed before any form feature exists.
7. **Nothing enforces the webview's "data, never markup" rule** (§T8). The rule holds today
   — every document-derived string reaches the DOM through `textContent`, and the frontend
   contains no `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write` or `{@html}`
   — but it holds by convention, and both halves are a grep that nothing runs. This entry
   said "CSP and Tauri capabilities are scaffold defaults" until 2026-08-02, which was wrong
   about the CSP: `default-src 'self'` with no `'unsafe-inline'` is a narrowed policy where
   the scaffold ships `"csp": null`. The **capability set** is the part that is still
   scaffold — `core:default` plus `dialog:allow-open`, unpared.
8. **A compromised worker can lie about what it saw** — no verification result may rest on
   a single worker's word.
9. **Nothing here protects previous copies, backups, or free sectors.**
10. **A document that reliably kills its worker costs a process per attempt.** A crashed
    worker *is* now replaced and the request retried once (`RenderService`, 2026-07-28), so
    a death from anything other than the request in hand is invisible to the reader. The
    bound on the pathological case is the single retry rather than a budget: a page that
    faults deterministically spawns a fresh sandboxed process each time it is asked for,
    which is not free. Verified by `backend-probe`, which kills the worker out of the OS
    process table and asserts the same pixels come back from a different pid.

    **This entry read "bounded by the reader's own requests" until 2026-07-28, and that was
    false** — which is worth keeping visible, because the sentence was doing the work of a
    mitigation while naming a bound nothing enforced. The reader makes one request; the
    *frame loop* made the rest. `Scroller.request()` runs every frame and re-issued any tile
    that was not resident and not in flight, and a failure deleted the in-flight entry
    without recording anything — so a deterministically faulting page had the application
    spawning and killing sandboxed processes at display cadence, indefinitely, with nobody
    touching the machine. The frame loop could not idle out either, because the re-issued
    requests kept `pendingWork` above zero. The real bound is now a per-request exponential
    backoff in `scroller.ts` (250 ms doubling to 8 s), a matching `failed` set in
    `thumbnails.ts`, and a `failed` count carried into `ViewerStatus` so the state is
    visible rather than silent. The general lesson is the one `AGENTS.md` already records
    from the other direction: a bound stated in prose and enforced nowhere reads exactly
    like one that holds.

11. **Printing parses the document inside the coordinator** (§3). PDFKit reads every job in
    the app process and again on the main thread, and `lopdf` rewrites the document there
    whenever the view is rotated. The panel genuinely cannot move — `NSPrintOperation` needs
    the application's window — but the `lopdf` rewrite and the verification read could, and
    have not. A parser bug reached this way lands in the process holding the user's
    filesystem authority, which is asset 1 in §1. Recursion in the two graph walks is
    bounded at `sweep::MAX_NESTING`; nothing else about this is mitigated, and it is reached
    by ⌘P on any open document.

12. **A request that hangs costs a deadline and a worker, and its work is lost.** The
    per-request bound is a kill (§T3), so a page that never finishes parsing holds one of
    `pool + 2` service threads for `TPDF_CALL_MS` — thirty seconds by default — and then
    answers the reader with an error, having spent a process. That is a bound rather than a
    wedge, which is the whole improvement, but a reader looking at a document with such a
    page pays it on every request that reaches it, and nothing remembers that the page is
    bad. The frontend's per-request backoff (§7.10) is what keeps that from repeating at
    frame rate; there is no equivalent for text, search or outline requests.

## 8. How to re-verify any of this

These are **`[[example]]` targets, not `[[bin]]` targets**, since 2026-07-31 — they were moved
out of the installer, which had been shipping all 17 of them including a sandbox prober. The
bare `worker-bench <file.pdf>` form this section carried until 2026-08-02 has not been
runnable since. `cargo run --example worker_bench` is not it either: cargo matches the target
name, which is hyphenated, and the underscored form fails as *"no such target"* — which reads
like a missing harness rather than a misspelling. `BUILD.md` has the same trap.

```
# macOS and Windows both. Add --release or measure a debug build by mistake.
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    <file.pdf> --mode engine     --lib vendor/pdfium/lib
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    <file.pdf> --mode authority  --lib vendor/pdfium/lib   # use a base-14 fixture
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    <file.pdf> --mode footprint  --lib vendor/pdfium/lib --budget-mb 128 \
                                 --poll-ms 0,1,5,20,50
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    <file.pdf> --mode crash      --lib vendor/pdfium/lib
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    <file.pdf> --mode limits     --lib vendor/pdfium/lib

cargo build --release --manifest-path src-tauri/Cargo.toml --example worker-probe
./src-tauri/target/release/examples/worker-probe  <file.pdf>   # the boundary itself
./src-tauri/target/release/examples/backend-probe <file.pdf>   # that the viewer's path uses it

# §5.1, macOS only. Bare first: the control must read its own string back, or the
# sandboxed runs below are unreadable rather than informative.
swiftc -O -o /tmp/vision_probe scripts/vision_sandbox_probe.swift
/tmp/vision_probe
/tmp/vision_probe /tmp/prod.sb            # worker::SANDBOX_PROFILE, extracted from worker.rs
```

**`worker-bench` is macOS-only** and correctly so: it carries its own POSIX worker, fd passing
and SBPL profiles included, and shares no mechanism with the Windows model. That is a
genuine refusal rather than an unported one — `AGENTS.md` records four separate lists of
"Windows blockers" that were wrong by over-reporting, so the distinction is worth stating.
`worker-probe` and `backend-probe` run on both.

**On Windows, `--mode engine` reports `[NOT VERIFIED]` rather than a clean bill** (§T2): the
shipped `pdfium.dll` carries no local C++ symbols, so `v8::` and `CXFA_` being absent from it
means nothing. Do not read that as a pass. What stands there instead is the asset name and
the pinned digest `scripts/fetch_pdfium.py` asserts — a claim about *which file was fetched*
rather than about what is in it.

`--mode engine` and `--mode authority` are the two that must be re-run after every PDFium
bump: the first because the absence of a JavaScript engine is a property of the build, the
second because font handling under the sandbox is a property of the mapper. Windows adds a
third: `win_sandbox_probe`, since the pixel-identity of a contained render is a property of
the font mapper under a token as much as under a profile.
