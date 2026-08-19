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
| **Webview** (Svelte) | Draws, receives tiles, issues commands --- four of which write files on its behalf (§T6.1), and drives the updater's one request per launch (§T9) | No *direct* filesystem access, no network reach of its own, no PDF parsing |
| **Coordinator** (Rust, the Tauri process) | Opens files the user chose, owns the window, spawns and kills workers, owns every shared mapping | Parses no PDF syntax on the *viewing* path — with one exception, printing, described below |
| **Worker** (Rust + PDFium) | Parses and renders whatever bytes it is handed | No filesystem, no network, no path to the document, cannot create a file |
| **Disk** | Holds the document and tpdf's output | — |

**That first row said "No filesystem" flatly until 2026-08-17, and §T6.1 had contradicted it
since 2026-08-16.** The webview holds no filesystem *plugin* permission --- the granted list is
`core:default`, `dialog:allow-open`, `dialog:allow-save` and `updater:default`, and the two
dialog permissions open panels and write nothing. But it can issue `save_copy`,
`save_document`, `extract_pages` and `print_document`, and all four write a file at the
process's authority with a path the caller chose. So the accurate statement is that the
webview cannot touch the filesystem *itself* and can ask for four specific writes; the flat version reads as the stronger claim, and a reader
who stops at this table gets the wrong answer. §T6.1 has the worked-out version and says why
neither path checks its argument against the document actually open.

**"No network" was wrong in the same way and for longer.** `updater:default` is granted to
this window, and it is the *frontend* that spends it: `App.svelte` imports
`@tauri-apps/plugin-updater` and calls `check()`, which issues the one request this
application makes. The row has said "no network" since before the updater landed in `26.8.2`
and nothing moved it. What the webview does not have is network reach of its own --- no
`fetch` to an arbitrary host, because the CSP is `default-src 'self'` --- which is a real and
different property, and the one the row now states. §T9 is the worked-out version.

Both corrections are the failure the release checklist's step 6 exists for: a row in a
summary table that stopped agreeing with the section beneath it, in the direction that
over-claims. Neither could go red, and neither was found by a probe.

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

#### T6.1 — Saving a copy, added 2026-08-16

**What changed.** `save_copy` is the first command that writes a file the reader names, and
`dialog:allow-save` is the first new capability the application has taken since the updater.
Both are narrower than they look and one of them is not as narrow as it should be.

**The capability is inert on its own.** `dialog:allow-save` opens a native panel and returns
a path; it writes nothing. The write is `save_copy`, and its authority is the process's ---
which is to say the reader's, since nothing here is sandboxed on the app side. So the honest
statement is: **a caller able to reach `save_copy` can write a PDF anywhere the reader can
write**, without a panel and without a prompt --- and the *source* path is the frontend's
too, so the same caller can read any PDF the reader can read. Neither end is checked against
the document the render service actually opened, which it could be. It is not, because
`print_document` has had exactly the same shape since 2026-07-28 and tightening one of the
two would leave a consistent surface looking inconsistent; if this is closed, close both.

**`extract_pages` is the same verb with a selection, added 2026-08-17**, and it is recorded
here rather than given a section because it adds no authority: same write path, same
caller-supplied source and destination, same absence of a check against the open document.
The only thing it adds is a `slots` argument, which `plan_subset` refuses when it is empty,
out of range, repeated or descending --- so the worst a bad selection produces is a refusal,
not a wider write. **The count of commands that write a file is three now**, not two: the
boundary table's §3 row says "two", and it is corrected in the same commit; a number in a
summary row is exactly the thing that stops agreeing with the section beneath it. (It is
**four** as of 2026-08-19, when `save_document` landed --- see §T6.7. The sentence is left as
it was written rather than silently re-pointed, because what it is about is a count in a
summary going stale, and re-pointing it every time would erase its own evidence.)

**What bounds that is the same thing that bounds `spike_exit`, and no more.** The CSP is
`default-src 'self'` with no `'unsafe-inline'`, so the only script that runs is the one that
shipped --- residual risk 7, and the T8 invariant that keeps document text from becoming
script. The marginal authority over what was already reachable is real but small: a caller
that can reach `save_copy` can already reach `open_document` and the print path. It is
recorded here rather than left implicit because it is the first *write*, and a write is a
different kind of verb from the ones this surface had before.

**Three refusals, and each is a correctness property rather than a security one:**

- An **encrypted** source is refused outright. `lopdf` drops `/Encrypt` on save without a
  word, so a copy of a restricted document would come out unrestricted and look identical.
  This is exactly the T5 shape --- a false assurance --- pointed at the document's own
  protection rather than at ours.
- A **page count that disagrees with the model** is refused, which is the only part of §5's
  external-modification story that exists yet.
- **Writing over the source** is refused, compared by canonical path so that two spellings
  of one file are one file.

**The write is atomic** --- sibling temporary file, rename --- so an interrupted save leaves
either the old file or the new one. The redaction path above needs the same property for a
different reason and states it separately; this one is not that, and does not claim to be:
**a saved copy is a serialisation, not a sanitation.** Nothing here garbage-collects an
unreachable object or removes a prior incremental revision, so a copy of a document carries
forward whatever the original carried. That is correct for "save a copy" and would be wrong
for a redaction, and the two must not be confused when the redaction path is built on it.

#### T6.2 — Deleting and moving a page, added 2026-08-17

**Nothing new crosses the boundary.** `page_delete` and `page_move` each take a document
handle and one or two page identities and mutate a `HashMap` in the app process; they open
no file, write none, and reach no worker. Their authority is the same as `page_rotate`'s,
which is to say the ability
to make the reader's *unsaved* document differ from the file on disk --- reversible with
undo, and never written until the reader names a file. The commands that write are still
`save_copy` and `print_document`, and their authority is unchanged and stated above.

**One thing did change on the write side, and it is worth stating precisely rather than as a
narrowing.** `print_document` takes the open document's handle now, and the *edits* in a job
--- which pages the reader kept and how each is turned --- are read from the model rather
than accepted from the frontend. The explicit page range is unchanged and still comes from
the caller; it is what a print panel's "pages 2 to 4" will be, and it carries no edits. So a
caller can still name any readable path and any range of its pages --- the §T6.1 shape,
unchanged --- and cannot invent an edit the model does not hold.

**What a deletion does to the parsing surface.** A page dropped from a saved copy is dropped
by the same page-tree pass the print path has used since 2026-07-28 (`pagetree::drop_pages`),
which walks the object graph under `sweep::MAX_NESTING` and refuses rather than stopping
early --- a partial pass would leave a page tree naming an object that is gone, which is a
document that opens and prints blank pages. One refusal is a correctness property in the
§T6.1 sense: a page two page numbers share cannot be half-deleted, because removing it means
removing one entry from a `/Kids` array rather than one object, and a pass that removed
neither would hand back a copy with the page the reader deleted still in it.

**What a reorder does to it.** A moved page cannot be written in place --- the four
inheritable page attributes belong to the tree node a page hangs under, not to the page ---
so `pagetree::reorder_pages` writes those attributes onto each page and rebuilds the tree one
level deep. It runs **only** when the reader's order differs from the file's, which is a
correctness property rather than a saving: a rebuild reparents every page of every document,
and doing that to one nobody rearranged is a rewrite with no request behind it. The
abandoned tree nodes stay in the file as unreachable objects, exactly as a deleted page's
content does, and for the same stated reason --- a saved copy is a serialisation, not a
sanitation (§T6.1, residual risk 15).

**Dragging a thumbnail adds nothing here**, checked rather than assumed when it landed on
2026-08-17. It registers no command, takes no capability and reaches no new sink: the gesture
ends in `page_move`, which is the command above. What it does add to the webview is pointer
listeners and a `setPointerCapture` on the strip's own panel, neither of which parses markup
or builds a URL-bearing element --- the `sinks` gate is what says so mechanically, and §T8 is
where that invariant lives.

**The outline is dropped whole from a copy that lost pages**, and that is a *smaller* claim
than repairing it would be: what survives a repair is only as sound as the resolver that did
it, and what survives this is nothing. Stated in the changelog as a real loss rather than
hidden as a detail.

#### T6.3 — Highlighting a selection, added 2026-08-18

**The first thing tpdf adds to a document rather than rearranging, and it adds no
authority.** `annot_highlight` and `annot_remove` take a document handle, a page identity and
a list of numbers, and mutate a `HashMap` in the app process. They open no file, write none
and reach no worker --- the T6.2 shape exactly, and the commands that write are still
`save_copy`, `extract_pages` and `print_document`.

**Two things the frontend cannot say, and both are deliberate.**

- **The timestamp.** `edits::NewMark` has no field for it; `lib.rs` reads the clock when the
  command arrives. What a mark claims about when it was made is the application's statement,
  and a `made` on the wire would be one more attacker-chosen string in a file tpdf signs its
  name to.
- **The subtype.** `MarkKind` has one variant and `save.rs` maps it with a `match`, so the
  `/Subtype` written is a literal of ours. A document cannot choose it, and neither can the
  frontend --- the same property `annots.rs` keeps on the way *in*, where `Kind` is an enum of
  our own literals rather than the document's `/Subtype` string.

  ⚠ **The second half of that stopped being true later the same day** (§T6.5): the frontend
  now names the kind, because a reader chooses between three. What it has *not* stopped being
  is the property that matters --- read the amendment rather than this bullet.

**A mark's note is attacker-controlled the moment a saved file is reopened**, which is the
one genuinely new surface. The reader types it, tpdf writes it, and `annots.rs` reads it back
out of a file that may by then have been edited by anything --- so it is treated exactly as a
comment body already is: it reaches the DOM as text, it may carry no URL, and §T8's invariant
is what makes that checkable. `edits::MarkView` says so at its declaration and the `sinks`
gate is what enforces it mechanically. Today the note is always empty, because nothing types
one; the field exists because the write path needs it and the reading path already has it.

**Something types one as of 2026-08-18** (§T6.4), and the paragraph above is what it was
written against --- so the surface is the one already described rather than a new one. The
box a reader types in is a `<textarea>`, whose `value` is text by construction and parses no
markup; the note is displayed nowhere else while the document is open. The route by which it
becomes *somebody else's* string is unchanged: it goes into `/Contents`, and comes back
through `annots.rs` into the comment panel and the comment popup, which have treated a body
that way since they were written.

#### T6.5 — The frontend names the mark's kind, added 2026-08-18

**A reader can now choose Highlight, Underline or Strike out, so the kind travels on the
wire** --- `MarkKind` is a field on `edits::NewMark`. T6.3's bullet said the frontend cannot
choose the subtype; that is now the wrong sentence for the right property, and the property
survives intact.

**What the frontend chooses is a variant, not a string.** `MarkKind` is a Rust enum with
three variants and serde names, so an unknown name is a *deserialisation failure at the
command boundary* --- the command never runs. The `/Subtype` bytes are still literals in
`save.rs`'s `match`, reachable only by naming one of the three, and that `match` is still
what makes a fourth variant a compile error rather than a mark written as something else. So
the closed set moved from "one variant, nothing to choose" to "three variants, chosen by
name", and at no point is a caller's string written into the file.

**The colour is the field a caller does choose freely**, and it did before this too: three
floats that reach `/C` and the appearance stream's `rg` operator. They are clamped by
nothing, which is worth stating rather than discovering --- a value outside 0..=1 is a
malformed colour in a file tpdf wrote. It is not an escape: `format!` writes a number, PDF
readers clamp, and the surrounding operators are ours. Bounding it is a correctness question
rather than a security one, and it is not done.

**No new authority.** The command is the renamed `annot_highlight` --- one path for all three
kinds rather than three commands, which is a smaller surface and not a larger one. It still
takes a document handle, a page identity and a list of numbers, still mutates a `HashMap` in
the app process, and still opens no file, writes none and reaches no worker.

#### T6.4 — A note on a mark, and taking one off, added 2026-08-18

**`annot_note` and `annot_remove` add no authority either**, for the same reason `annot_mark`
(called `annot_highlight` when this was written) does not: both take a document handle and an identity, both mutate a `HashMap` in the app
process, and neither opens a file, writes one or reaches a worker. `annot_note` additionally
takes a string, which is the reader's own and is not interpreted by anything on the way in ---
`save.rs` encodes it as a PDF text string when a copy is written, and that encoding is the
same one the author field already goes through.

**Two bounds it does not have**, stated because their absence is a decision rather than an
oversight:

- **No length limit.** A note is as long as the reader makes it. The memory it costs is one
  copy per journalled version, in a process that already holds the document, and the file it
  produces is one the reader asked for. `annots.rs` *does* bound what it reads back, so a
  note longer than that clip is written whole and reported clipped on reopen --- visible to a
  reader as a truncated note in the panel, which is a display limit and not a loss of the
  file's bytes.
- **No content rules.** Control characters, right-to-left overrides and anything else a
  keyboard can produce go through. They are the reader's own bytes in the reader's own file;
  the place where such a string becomes dangerous is where it is *read*, and that path is
  §T8's.

**What the write adds to a saved copy** is one annotation object and one form XObject per
mark, appended to the page's `/Annots` --- and one refusal that is a correctness property in
the §T6.1 sense: a mark on a page object that two page numbers share is refused, because an
annotation hangs off the *object* and would appear on both pages. That is the same shape as
the half-deletion refusal above, one level on, and it is scoped to the marked page rather
than to the file: a document with one malformed page must not become unmarkable everywhere.

**The appearance stream is ours rather than the reader's**, and that is a security-adjacent
choice worth stating: the content stream `save.rs` writes is built from numbers, with no
string from the document or from the reader in it. Nothing about a mark's appearance depends
on text anyone typed.

#### T6.6 — Cropping a page, added 2026-08-18

**The model command is the T6.2 shape, and two commands beside it are not.** `page_crop`
takes a document handle, a page identity and four numbers, and mutates a `HashMap` in the
app process --- it opens no file, writes none and reaches no worker, exactly as
`page_rotate` and `annot_mark` do. `page_content_box` and `page_geometry` **do reach a
worker**: the first renders the page to find where its ink is, the second loads it to report
what size a crop makes it. That is the first pair of commands added since the viewer's own
that parse a document, and it is worth saying plainly rather than folding into the sentence
above.

What they add is nothing. Both take the document handle the frontend already has and a page
position in the file it already knows; neither takes a path, and the parse happens in the
same sandboxed worker that renders every tile a reader has already caused. A caller able to
reach them can already reach the tile protocol, which renders any page of the same document
on demand. The marginal authority is a render nobody asked for --- a denial of service on
the render thread, which residual risk 7 bounds the same way it bounds `spike_exit`.

**A crop is four numbers off the wire, and they are checked in three places for three
different reasons.** `docmodel::Rect::is_proper` refuses a rectangle enclosing no area,
including any corner that is not a number, so a `NaN` cannot reach the model. `protocol.rs`
refuses a tile URL carrying **three** of the four corners rather than completing the
rectangle from the page --- three numbers plus a default is a rectangle nobody asked for,
drawn plausibly and in the wrong place, which is what that parser exists to prevent --- and
refuses a non-finite or degenerate one before a render is allocated for it. `pagetree`
refuses a crop that shares no area with the sheet, which is a different question and can only
be asked where the media box is known.

**The save path gains one mutation of the object graph**, and it is narrower than the others
on this surface. `apply_crops` writes `/CropBox` on the **page object** and never on an
ancestor: the box is inheritable, so a write onto a `/Pages` node crops every page hanging
under it, which for a document whose pages share one node is the whole file from a reader who
cropped one page. It intersects with `/MediaBox` per §14.11.2 rather than trusting the value,
and a crop the intersection empties is refused rather than written --- a page that renders as
nothing is not an outcome a reader asked for.

**Residual, and it is a §T5 shape rather than a §T6 one: a crop hides, it does not remove.**
Everything outside the box is still in the file, still extractable, still searchable in any
reader --- and tpdf's own search still finds it, because a crop moves character *boxes* and
not character *indices*. That is what `/CropBox` means and it is the right behaviour for a
crop. It is listed because it is the second operation on this surface where a reader could
plausibly believe otherwise, after deleting a page (risk 15), and because "crop" is a word
that sounds like removal in a way "rotate" and "move" do not. The operation that makes hidden
mean gone is `docs/PLAN.md` §6, and it is not built.

#### T6.7 — Saving over the open document, added 2026-08-19

**No new authority, and one new verb.** `save_document` takes a document handle and a
source path and writes the working document over that path. Its authority is `save_copy`'s
--- the process's, which is the reader's --- and the path is the frontend's in exactly the
same way, unchecked against the document the render service actually opened. So the §T6.1
statement stands unchanged and now covers four commands rather than three.

**What is new is that this one replaces a file rather than creating one.** `save_copy`
refuses the source outright; this is the command that is *for* the source. A caller able to
reach it can therefore overwrite any PDF the reader can write, without a panel and without a
prompt, and the marginal difference from `save_copy` is that a file already there is gone
rather than a second file appearing beside it. The bound is the same and no stronger:
`default-src 'self'` with no `'unsafe-inline'`, residual risk 7, and the fact that a caller
who can reach this can already reach `open_document` and the print path.

**One check narrows what a wrong path can do, and it is a correctness check rather than a
security one.** The page count of the file named has to match the plan's baseline, so
pointing this at an unrelated document is refused unless that document happens to have the
same number of pages --- at which point the reader's edits are applied to it and written.
That is not a guarantee and is not offered as one; it is the same absence §T6.1 records, and
if the source path is ever checked against the open document, this is the command where it
matters most.

**The document is closed before the file is replaced, and that is a correctness property
with a security-shaped tell.** A `rename` over a memory-mapped file succeeds on macOS and
leaves the worker serving the inode that is no longer at that path --- measured, and in
`docs/TRAPS.md`. Nothing about that is exploitable; what it is, is a reader looking at a
document that disagrees with their own file while everything reports success, which is the
§T5 false-assurance shape pointed at the save rather than at a redaction. Windows refuses
the rename instead, so the order is what makes the two platforms agree.

**Two refusals, distinguished on the wire, which is unusual enough to state.**
`SaveFailure` carries `reopen`: false means nothing was touched and the reader still has
their document, true means it is closed whatever became of the file. The reason it is a
field rather than a wording is the T8 reason one level down --- a frontend that decided by
matching on message text would be parsing a string the backend is free to reword.

**The write is atomic and is still a serialisation, not a sanitation.** Everything §T6.1
says about that applies here and is more consequential: a copy carrying forward an
unreachable object leaves the original untouched beside it, and this one does not. Saving
over a document does not remove a prior incremental revision, garbage-collect anything, or
make hidden content gone. `docs/PLAN.md` §6 is the operation that does, and it is not built.

### T7 — Distribution and update

**The threat.** A tampered download, a tampered update, or a compromised dependency —
including the PDFium binary itself, which is prebuilt from a third party.

**What stops it, and what does not yet.** Direct notarized distribution with a signed
update payload is the intended shape, following `screenpick`. Two things are settled by
policy rather than by code today: the PDFium build's provenance (pinned build, re-checked
after every bump — see T2, and the `remove_probe` standing regression) and permissive-only
licensing, which keeps the dependency set small enough to audit.

**Tested 2026-08-03, and this read "Untested" until then.** The signing and notarization
path for a bundled dylib (§10 q7) was the open question here, and it bit `screenpick`'s
release path before. It is answered: the `.app` notarizes `Accepted`, the DMG notarizes and
staples, and both the app and `libpdfium.dylib` carry a Developer ID Application signature
chaining to Apple Root CA with the hardened runtime. Confirmed from **outside** the workflow
as well as by it --- the DMG was downloaded from the draft and checked on a machine that had
not built it, where `spctl -a -t open` reports `source=Notarized Developer ID` and the
stapled ticket validates. That distinction is not pedantry here: on the run before, every
one of those properties held while the workflow's own verification step failed for a reason
of its own, so the workflow's verdict and the artifact's state are separate facts.

What is **still** untested is distribution over time rather than at the moment of release:
nothing re-checks that a published artifact still validates once the signing certificate
expires (2031-07-26) or if it were revoked. **There is an update channel to carry a fix as
of 26.8.2** — see §T9, which is where the residual for it lives; this paragraph read "there
is no update channel ... since tpdf ships no updater" until then.

### T9 — The updater

**The threat.** The updater is the only code path in tpdf that fetches bytes and then
*executes* them, and it is the highest-authority feature the application has. A compromised
endpoint, a downgrade to a version with a known defect, or an unsigned payload accepted by
mistake, each ends with attacker-chosen code running as the user — outside every boundary
the rest of this document builds, because the replacement binary *is* the boundary.

It also changes a property that held until 26.8.2: **tpdf made no network request at all.**
That was worth something and it is now spent. It is spent narrowly, and the narrowness is
the mitigation rather than a footnote — see below.

**What stops it.**

- **The payload is verified before it is unpacked.** `tauri-plugin-updater` checks a
  minisign signature against `plugins.updater.pubkey`, compiled into the binary at build
  time, and only then extracts. That ordering is what keeps the archive parsers the plugin
  brings in (`zip`, `tar`) from ever seeing bytes an attacker chose — they are as much a
  parsing surface as PDFium is, and they run **in the app process**, not in a worker.
- **The signing key is not the Apple key and is held only by CI.** It exists as
  `TAURI_SIGNING_PRIVATE_KEY` (+ password) on the repository, and GitHub secrets cannot be
  read back out. Compromising the release workflow is therefore the whole attack; a stolen
  Developer ID would not help, and neither would write access to the releases page, since an
  unsigned or wrongly-signed payload is refused by the installed copy.
- **The endpoint is a single pinned HTTPS URL** —
  `github.com/tstone-1/tpdf/releases/latest/download/latest.json` — which resolves only to a
  *published* release. Draft releases are invisible to it, so the human act of publishing is
  what offers an update to anybody, and a failed release run offers nothing.
- **Nothing is fetched unasked beyond the check itself, and nothing is applied unasked.**
  One `check()` per launch, issued after every spike and check entry point has returned, so
  every harness in this repository still runs entirely offline. Downloading and applying
  require the reader to click. `update.ts` carries why this is not silent.
- **A failed check is reported and forgotten.** Nothing retries, so an endpoint that is
  hostile, slow or absent cannot turn into a loop that keeps dialling out.

**Residual, and there are four.**

1. **The release workflow is the single point of trust.** Anyone who can run it can sign a
   payload every installed copy will accept. That is the same exposure as any signed
   auto-update and it is not reduced by anything here; it is bounded only by GitHub account
   security and by the workflow being tag-triggered and unreachable from a fork PR (§7.6).
2. **No downgrade protection of our own.** The plugin compares versions and offers only
   newer ones, but that decision is made from `latest.json` — an attacker who could serve
   that file could not forge a signature, but could withhold an update indefinitely. Nothing
   here detects being pinned to an old version.
3. **The archive parsers run in the app process.** Signature verification comes first, so
   this only matters if the signing key is compromised — at which point it is the least of
   the problems. Recorded because the boundary claim elsewhere in this document is about
   *PDFium*, and this is a second parser family that the app process now links.
4. **Untested against a real endpoint.** `update.test.ts` fakes the plugin, and the tests
   there cover the state machine rather than signature verification, TLS, or the real
   `latest.json`. The first genuine end-to-end proof is the first update applied from one
   published release to the next, and `BUILD.md` schedules it as a manual step because it
   cannot exist until two signed releases do. **Nothing below claims otherwise.**

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

**It is enforced by a gate as of 2026-08-02**, and was a convention for the few hours between
that discovery and this one. `scripts/check_webview_sinks.py` is the `sinks` gate, and it
pins the narrow checkable invariant rather than the broad one:

> there is no markup-parsing sink anywhere in the frontend

That is **sufficient**, not merely necessary, and the reason is what lets a grep answer a
question about taint. If no sink exists, a string reaching the DOM has only `textContent`,
`createTextNode`, `value` and `setAttribute` left to travel by, and none of those parses
markup — so the check never has to decide *which* strings came from a document, which is the
part no grep could do. It scans the whole frontend for nine patterns
(`innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`,
`createContextualFragment`, `srcdoc`, `{@html`, plus `eval(` and `new Function(`, which the
CSP would turn into a production-only failure).

`setAttribute` is the one of those four that *can* be a sink — with `href`, `src` or an event
handler — so the gate carries a second rule: **every `.setAttribute(` call must name its
attribute with a string literal.** Every one does, or the gate is red.

**The gate prints its own population, and this section deliberately no longer does.** It said
"58 files and 22,073 lines" and "all 45" from 2026-08-02 until 2026-08-17, against an actual
81 files, 36,029 lines and 59 calls --- the frontend had grown by more than half and the
sentence describing what was covered had not moved, which is a count in prose with nothing
able to go red about it. Read it from the run instead:

```
python3 scripts/check_webview_sinks.py   # scanned N files, N lines, ... N setAttribute call(s)
```

Four failure modes were proved by mutation before the gate was trusted, each singly and with
a byte-digest restore: an outline title assigned by `innerHTML` (caught, named to the line), a
computed attribute name (caught), an empty scan population (refused, since a scan that
examined nothing reports exactly what a clean one reports), and **`.setAttribute(` occurring
nowhere at all** (refused — a pattern that stops occurring passes identically to one that
finds nothing). The exemption marker `webview-sink-ok:` works and every use of it is counted
and printed, so it cannot silence the check quietly.

`setAttribute` is not the only route a *non-markup* string can still navigate or execute by,
so three more rules close the rest: a dangerous literal attribute name (`href`, `src`,
`onclick`, …), an assignment to a navigating property, and the blunt one that makes the
others nearly moot — **no URL-bearing element is ever created**. With no `<a>`, `<img>` or
`<iframe>` in existence there is nothing for a URL to be assigned to. Each was proved to fire,
and `this.onChange = onChange` — an ordinary field, not a DOM handler — was proved *not* to,
which is what says the rule discriminates rather than matching everything.

The backend half is enforced by the type. `outline.rs` refuses `/URI`, `/Launch` and `/GoToR`
into `Target::Refused { action }`, whose string is one of five literals chosen in that file;
`no_target_variant_may_carry_a_url` matches `Target` exhaustively, so adding a URL-bearing
variant is `error[E0004]` rather than a test failure. What is *not* enforced is the link
between the two halves — see residual risk 7.

**Comments raised the stakes on all of this on 2026-08-16 without changing the argument.** A
document's annotations are the largest body of attacker-chosen prose tpdf has ever put on
screen — bodies, authors, subjects, several paragraphs each — and they reach the DOM through
`commentlist.ts` and `commentpopup.ts`. Every one of those assignments is `textContent`, so
the sufficiency argument above covers them unchanged: with no markup-parsing sink in the
frontend there is nothing for the text to be parsed as.

Three things were done rather than assumed, because "more of the same text" is exactly when a
mitigation quietly stops being sufficient. `annots.rs` gives `Comment` the same treatment
`Target` has and `no_comment_field_may_carry_a_url` destructures it exhaustively, so a new
field is a compile error here too. `Kind` is an enum of ours, not the document's `/Subtype`
string, so the one value that would otherwise flow from the file into a class name or a label
cannot. And a date is *rebuilt* from parsed digits rather than passed through — a `/M` entry
is a string like any other, and `<script>alert(1)` is a legal one.

The popup's body uses `white-space: pre-wrap` to keep a comment's paragraphs. That is a style,
not a parse: the newlines are in the character data, and no markup is involved in rendering
them.

**Links, the same day, went the other way — and it is worth saying why the direction differs.**
A link annotation carries a URL, which is the one kind of attacker-controlled string this whole
section exists to keep away from the DOM. So `links.rs`'s `Link` has **no string field at all**:
a rectangle, a page, and a `Target` whose only string is the five-literal `action` chosen in
`outline.rs`. `no_link_field_may_carry_a_url` destructures it exhaustively, so a field added
later is a compile error rather than a leak, and there is no `textContent` argument to make
because there is nothing to display.

**The accessibility tree marks them up, and the gate decided the element.** A cross-reference
is announced as a link through a `<span role="link">` — never an `<a>`, because the `sinks` gate
refuses the creation of any URL-bearing element anywhere in the frontend, which is exactly what
lets the argument above claim sufficiency from a grep. A span carrying a role is announced as a
link by every screen reader and can hold no URL, so the constraint and the accessible outcome
want the same element rather than trading against each other. The only attribute the document
influences is `aria-disabled`, and it is set from `Target`'s variant rather than from anything
the file wrote; the destination is carried as a page *number*. `a11y.test.ts` asserts on the
built DOM that no `a` or `iframe` exists there, and `viewer_check.py` asserts the same in a real
webview — the gate's claim from the other end.

That is also why **a refused link does not show where it pointed**. The obvious courtesy — "this
opens https://…, follow it?" — would put an attacker-chosen string into a prompt whose whole
purpose is to be trusted, which is a better phishing surface than no message at all. The reader
is told what *kind* of action was declined and nothing else. Whether tpdf should ever open a web
link, and what displaying one safely would require, is `docs/PLAN.md` §10 question 11 — a change
to this boundary rather than a feature, which is why it is a question and not a backlog item.

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
7. **The webview invariant is enforced on both sides, and nothing links the two sides**
   (§T8). Both halves landed 2026-08-02 and each is proved:

   - **Frontend** — `scripts/check_webview_sinks.py`, the `sinks` gate. No markup sink, no
     computed attribute name, no dangerous literal attribute (`href`, `src`, `on*`), no
     URL-bearing element created, no element created from a computed name without a stated
     reason, no assignment to a navigating property — each read in its namespaced spelling
     too, since `.setAttribute(` does not match `.setAttributeNS(` and the gate reported
     `[OK]` on a planted `setAttributeNS(null, "href", <document text>)` until 2026-08-02.
     Every rule shown to fire by mutation, with a control (`this.onChange`, an ordinary
     field) shown *not* to. One exemption: `a11y.ts` builds a heading or a paragraph from
     the document's structure tag through `elementFor`, a total whitelist of `p` and
     `h1`..`h6`.
   - **Backend** — `outline.rs`'s `no_target_variant_may_carry_a_url`. `Target` has no
     URL-bearing variant, and adding one **fails to compile**: `error[E0004]:
     non-exhaustive patterns: Target::Uri { .. } not covered`. That is the strongest verdict
     a mutation can get — not caught, but unmakeable.

   The residual is the seam. The frontend gate's sufficiency depends on the backend fact, and
   a grep over TypeScript cannot see Rust — so a Rust change cannot turn the gate red, and
   the two are held together by these paragraphs and two doc comments that name each other.
   That is better than a convention and weaker than a check.

   The first version of the gate, shipped hours earlier, is the reason to state this
   precisely: it enforced only that an attribute *name* be a literal, while the threat model
   claimed sufficiency from "every `setAttribute` passes a constant name, so there is no
   URL-bearing attribute to poison". `setAttribute("href", row.title)` satisfies both the
   check and the sentence. Correct about the tree in front of it, wrong about what it
   guaranteed.

   This entry also said "CSP and Tauri capabilities are scaffold defaults" until 2026-08-02,
   which was wrong about the CSP: `default-src 'self'` with no `'unsafe-inline'` is a
   narrowed policy where the scaffold ships `"csp": null`. The **capability set** is the part
   that is still scaffold — `core:default` plus `dialog:allow-open`, unpared.
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

13. **What the coordinator diagnoses now survives the run; what its workers say still does
    not.** A worker killed on its deadline, a crashed worker replaced under a reader who saw
    nothing, a pre-spawn that failed, a print that did not present — every one of those was
    an `eprintln!`, and a GUI process started by double-clicking a PDF has no stderr at all,
    so the diagnostics this codebase words most carefully were exactly the ones a user could
    never send back. Nine parent-process sites go through `diag::note` since 2026-08-02: it
    writes the line to stderr byte for byte as before — that channel is what `viewer_check.py`,
    `worker-probe` and `backend-probe` capture, and a line quietly moved off it would be a
    regression in checks that have nothing to do with logging — and appends a UTC-stamped copy
    to `tpdf.log` in the platform's log directory, `TPDF_LOG_FILE` overriding it. Bounded at
    256 KiB plus one kept predecessor. Serialized by a lock, because `eprintln!` is several
    writes and `docs/TRAPS.md` records a torn one that read as a worker dying with an empty
    reason. A failed append is swallowed — a diagnostics channel that can fail a request is
    worse than none — but counted, and the count is written out ahead of the next line that
    lands, so a hole in the file reads as a hole rather than as a quiet period.

    **The open half is the one nearest the parser.** A worker writes to the stderr it
    inherited from the parent (§T3) and starts no sink of its own, deliberately: a contained
    process holding a writable path outside its own mappings is a hole in §5. So a worker's
    dying words still evaporate on a GUI launch, and closing that means the parent reading
    its children's pipes and re-emitting what arrives — a change to the boundary rather than
    to the logging, and still future work. A crash of the coordinator itself logs nothing
    either, by construction: the process that would write the line is the one that died.
13. **`save_copy` writes a PDF anywhere the reader can write** (§T6.1), added 2026-08-16 ---
    the first command on this surface that creates a file, and its authority is the app
    process's rather than a panel's. The path comes from the frontend, so a native save panel
    is the *interface* and not the bound. What bounds it is residual risk 7: the CSP admits
    only the script that shipped. The marginal authority is small --- a caller that can reach
    this can already reach `open_document` and the print path --- and it is listed because a
    write is a different verb from the ones this surface had, not because the CSP is believed
    to be weaker than it was yesterday.
14. **A saved copy is a serialisation and not a sanitation** (§T6.1). Nothing on that path
    collects an unreachable object or drops a prior incremental revision, so whatever the
    source carried, the copy carries. That is right for "save a copy" and wrong for a
    redaction, and the redaction path must not be built on it by assuming otherwise.
15. **A copy that lost a page keeps the deleted page's content in every place that is not
    the page tree** (§T6.2), added 2026-08-17. `pagetree::drop_pages` removes the page
    object and every reference to it, and the mark-and-sweep that would collect what those
    references *held* --- the content stream, the fonts, an embedded image --- runs on the
    print path and not on the save path. So a reader who deletes a page and sends the copy
    on has removed it from the document and not from the file. That is the same distinction
    as risk 14 and is listed separately because deleting is the first operation where a
    reader could plausibly believe otherwise: `docs/PLAN.md` §6 is where "removed" comes to
    mean removed, and it is not built.

16. **A cropped page hides content and does not remove it** (§T6.6), added 2026-08-18.
    Everything outside the crop box is still in the saved file, still extractable, and still
    found by tpdf's own search --- a crop moves character boxes, not character indices. That
    is what `/CropBox` means, and it is the right behaviour for a crop. It is listed
    separately from risks 14 and 15 because "crop" is a word that sounds like removal in a
    way "rotate" and "move" do not, and because it is now the *second* operation a reader
    could plausibly believe removes something. Redaction is `docs/PLAN.md` §6 and is not
    built.

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
