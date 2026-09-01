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
| **Webview** (Svelte) | Draws, receives tiles, issues commands --- eight of which write files on its behalf (§T6.1), and drives the updater's one request per launch (§T9) | No *direct* filesystem access, no network reach of its own, no PDF parsing |
| **Coordinator** (Rust, the Tauri process) | Opens files the user chose, owns the window, spawns and kills workers, owns every shared mapping | Parses no PDF syntax on the *viewing* path — with one exception, printing, described below |
| **Worker** (Rust + PDFium) | Parses and renders whatever bytes it is handed | No path to the document and cannot create a file, on both platforms; no filesystem and no network on **macOS** --- on Windows, no writes, and reads and sockets are the disclosed ceiling |
| **Disk** | Holds the document and tpdf's output | — |

**That first row said "No filesystem" flatly until 2026-08-17, and §T6.1 had contradicted it
since 2026-08-16.** The webview holds no filesystem *plugin* permission --- the granted list is
`core:default`, `dialog:allow-open`, `dialog:allow-save` and `updater:default`, and the two
dialog permissions open panels and write nothing. But it can issue `save_copy`,
`save_document`, `extract_pages`, `split_document`, `merge_documents`, `print_document`,
`redact_copy` and `redact_document`, and all eight write a file at the process's authority
with a path the caller chose.
<!-- writers: save_copy save_document extract_pages split_document merge_documents print_document redact_copy redact_document --> So the accurate statement is that the webview cannot touch the
filesystem *itself* and can ask for eight specific writes; the flat version reads as the
stronger claim, and a reader who stops at this table gets the wrong answer. §T6.1 has the worked-out version and says why neither path checks its argument
against the document actually open.

**It said "four" until 2026-08-24, and `merge_documents` had been the fifth since 2026-08-24.**
That is the same drift this paragraph was written to record, one writer later: the count is a
number in prose and the list of commands is the thing that changes, so the count is wrong from
the moment a command is added until somebody reads this sentence again. The list is now the
claim and the number follows it --- and the standing rule that a count in prose has nothing
checking it applies here as much as it does to the trap index. A new file-writing command
belongs in this list, in §T6.1, and in the coordinator-parsing entry at residual risk 18.

⚠ **And it happened again, in both halves at once, found by the release checklist on
2026-08-30.** The row said **six** while the list beneath it named **five**, so the two
disagreed with each other on adjacent lines --- and the row was not the wrong one for the
reason it looked: the true count is **eight**. `split_document` had never been added to the
list, and `redact_copy` and `redact_document` were in neither. All three are named elsewhere
in this document --- the two redaction writers at §T6.11, the split at §T6.9 and at residual
risk 18 --- so nothing was undisclosed; what was wrong was the one place a reader goes to find
out *how many* ways the webview can cause a write, and it was wrong in the direction that
under-claims. **"The list is the claim and the number follows it" is a rule that needs
somebody to apply it**, and three commands landed without anybody doing so. The check that
would have caught it is mechanical and does not exist: enumerate the registered commands that
reach a writer, and diff that set against this list.

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

**The worker row was wrong the same way, and it is a security claim rather than a summary of
one.** It read "No filesystem, no network" flatly until 2026-09-01. That is macOS: the profile
denies reads, writes and socket binds, and §T4 measures it. Windows is a job object plus a
low-integrity token --- it denies writes and it denies reaching into the app process, and it
denies neither reads nor sockets. Residual risk 4 has carried the read half since 2026-08-02;
nothing carried the socket half until §T4 gained it on 2026-09-01, and that half is still a
reading of `sandbox_win` rather than a measurement. What holds on both platforms is the rest of
the row: the worker is never handed a path and cannot create a file, which is why the document
and the output arrive as descriptors, and is what makes the Windows read ceiling narrower than
it sounds. Found by an external review reading this row against §6 --- the third row in a
four-row table to drift from the section under it, which is now the strongest argument this
document has for the checklist step that re-reads it.

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
  whenever the reader has rotated the view. **Moved 2026-09-01**, see below.

Two of the three cannot move. `NSPrintOperation` needs the application's own window and its
`NSPrintInfo`, so the panel is in the coordinator by construction; PDFKit is also the parser
the print system will use itself, which is the whole argument for reading the job back with
it (`print_macos`). What *could* move was `print::build`'s `lopdf` rewrite, and it has:
`print::build_update` is a pure function of the document's bytes and the page range, run
through `Request::PrintRange` in the same sandboxed worker every other rewrite uses, and
`save::print_range_bytes` is the coordinator half that owns the scratch file and no parse.
The verification read stays, deliberately, and is the whole point of it: it is the platform's
own parser, asked whether it can read what we built. Reaching *that* needs no more than ⌘P on
an open document, and it is disclosed rather than closed.

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

**Printing is not the only exception, and this document said it was until 2026-08-22.** An
outside review found it: every path that *writes* a document parsed it in the coordinator
too, through `lopdf`, and each is one menu item away.

- `save_document` → `save::append_bytes` or `save::stage_in_place` (`lib.rs`), on every save
  over the open file.
- `save_copy` → `save::write_copy`, on every Save a copy.
- `extract_pages` → `save::write_copy` again, on every extraction.
- `merge_documents` → `save::write_merged`, on every merge --- and this one parses **more**
  than the open document: every file going in is loaded with `lopdf` here, so a merge of four
  documents is four parses of bytes nothing has rendered. Residual risk 18 carries that.

**The first of those moved the same day.** A save that only *adds* marks --- the ordinary
"keep my highlights" --- is prepared by `save::append_update`, which is a pure function of the
document's bytes and the plan: it opens nothing, names no path, and knows none exists. It runs
as `Request::Append` in the worker that already holds the document, under the same sandbox,
deadline, resource limits and restart as every render, in the process that has already parsed
that document with `lopdf` for its comments, links and properties. What crosses back is an
update section and two numbers.

The split is where the authority is, not where the code is convenient. `save::append_ready`
stays in the coordinator and asks only questions about a *path* --- has this file changed since
it was opened, how long is it --- which need filesystem authority and no parser.
`save::appended` then refuses an update built against a different number of bytes than the
caller measured, which is a check that did not exist and could not: the two lengths were one
number by construction while one function did both halves.

`Plan::opened_as` is `#[serde(skip)]`, so the fingerprint cannot cross in either direction ---
and the compiler is what enforces that rather than the attribute alone, since `Fingerprint`
implements neither `Serialize` nor `Deserialize`. `Request`'s standing property holds: it names
nothing the worker could act on.

⚠ **Only half of it moved that day, and the other half moved on 2026-08-26.** Preparing the
update is one parse; *verifying* what was written is another, and `save::append_in_place`
re-read the whole file and parsed it here. It is `save::Reread` now --- a seam taking the
written file's handle, its length and the password --- and `save::InWorker` maps that handle
read-only into a sandboxed child, asks `Request::Reread` and drops it. So the append is out of
the coordinator in both directions, and it gained the deadline and the memory bound this
section says need a process. Residual risk 18 has the full account, including what stayed.

Evidence, external to our own account of it: `worker-probe` builds an update section through a
real contained worker and appends it to the fixture, then re-parses the result --- **865 bytes
on a 775-page document, re-read as 775 pages**, with the length it was built against compared
against the file's own (macOS, 2026-08-22, 17/17). Four more checks since 2026-08-26 put the
same worker on the *verification* side: it and the coordinator are asked the identical question
about identical bytes and have to agree in both directions, the refusal has to be `lopdf`'s
rather than PDFium's at open --- which the first draft of that check got wrong while reading as
a pass --- and a fourth asks for something only the worker path needs, since two readers
agreeing says nothing about whether a worker was involved at all (23/23).

**The rewrite moved on 2026-08-28 and the copy paths, Split and the working-document print job
on 2026-09-01**, all through `save::Rewriter` and an output channel that is a descriptor rather
than a reply --- residual risk 18 has the mechanism and what it costs. What the memory
measurement of 2026-08-22 decided was not *whether* a rewrite could move but how large a
document may be **appended to** inside a worker: a worker holding the 337 MB scan reaches
1029.8 MB of footprint after answering an append, 667 MB of which the append added, against a
1024 MB Windows commit cap. `save::APPEND_MAX_BYTES` is that bound. `docs/PLAN.md` §3 has the
table, the three designs the measurement re-ranks, and the one open question it leaves.

**That cap applies to the append this section is about**, which is worth saying plainly rather
than leaving in the plan. On Windows the document's mapping is file-backed and not commit, so
the number to compare is the 667 rather than the 1029.8, and that leaves a margin --- by
reasoning, not by measurement. Nobody has run `worker-probe` against a large document on
Windows. If the cap is reached the worker is killed and the save is refused, which is
containment behaving as designed and a save the reader cannot complete; it is not data loss,
since nothing has been written at that point.

**The merge followed on 2026-09-01, and with it every writing path is out.** It was the
widest of them --- the only operation that parses documents tpdf never opened --- and it moved
on the same seam through `worker_proto::Request::Merge`, with the incoming files handed over
as a second read-only mapping (`worker::IN_FD`). What the coordinator does now is *read* those
files: it copies their bytes into the mapping and never asks what they mean.

⚠ **One `lopdf` parse remains in the coordinator, and it is a reader rather than a writer:
`verify::scan`.** The redaction verification re-reads the file that was just written and
parses it here, on the blocking pool. Its bytes derive from the reader's document, so this is
the same exposure the writers had. **It was missed for exactly the reason `print::build` was**
--- this section, residual risk 18 and `scripts/check_writers.py` are all keyed on *writing*,
and a verification writes nothing. `docs/TRAPS.md` has that under *A risk and a gate both
keyed on writing cannot see the path that only reads*.

It is the narrowest of the parses that have been here: the bytes are one tpdf wrote seconds
ago rather than the file as it arrived, and its own load is bounded. It is not moved, and the
route to moving it is the one the others took --- `verify::scan` is already a pure function of
bytes and needles.

That parse runs under `tauri::async_runtime::spawn_blocking`, and it is worth being exact
about what that does and does not buy, because the name invites the wrong reading: it moves
the work off the async runtime's threads. It does not move it out of the process holding the
window, the edit journal and the user's filesystem authority. "Off the async thread" is not
"out of the trusted process", and the two were being treated as the same thing.

Four things bound how much that is worth, and they are why this is a `[NOT MOVED]` rather
than a blocker:

- **`lopdf` is safe Rust.** T1 is about memory corruption in a C++ parser; that threat does
  not transfer to this path. What does transfer is T3 — a document that makes the parser
  allocate or spin.
- **Every load on these paths is bounded.** All three pass `max_decompressed_size:
  Some(MAX_DECODE)` (64 MB), and the two recursive graph walks refuse past
  `sweep::MAX_NESTING` rather than descending until the stack runs out.
- **A panic is caught rather than fatal**, and that is a property of how this is built rather
  than a hope: the crate unwinds (no `panic = "abort"` in any profile), and a panic inside
  `spawn_blocking` reaches the caller as a `JoinError` that each of these commands turns into
  a refusal. `a_panic_in_a_blocking_task_is_reported_rather_than_fatal` in `lib.rs` pins it,
  so setting `panic = "abort"` would turn a gate red rather than silently making a parser
  panic close the reader's document.
- **The bytes are the reader's own file**, opened deliberately, which is the same standing as
  the printing path and weaker than a drive-by.

What is **not** bounded is time or memory. There is no deadline on these parses and no
resource limit, because both need a process to enforce them against — which is exactly what
`docs/PLAN.md` §3's surgery worker is for and it is not built. A document crafted to make
`lopdf` spin presents as an application that has stopped responding, not as a contained
worker failure. See residual risk 18.

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

**Windows answers half of this, and the half it does not answer is in this section's own
title.** §6's containment is a job object plus a low-integrity token, and neither restricts
sockets: `sandbox_win` sets `JOB_OBJECT_LIMIT_ACTIVE_PROCESS`, `_PROCESS_MEMORY`,
`_KILL_ON_JOB_CLOSE` and `_DIE_ON_UNHANDLED_EXCEPTION`, and an integrity level --- read the
file, there is no network call in it. The Windows mechanism that gates network *capability*
is AppContainer, which this is not. **Nothing here has measured a socket bind from a
contained Windows worker**, so this is a ceiling read off the code rather than a result, and
the honest statement is that the network is not denied there rather than that it is reachable.
The measurement is one rung on `examples/win_sandbox_probe.rs`, which already re-execs a
contained child and compares its work against an uncontained control; a bind of a TCP and a
UDP socket in that child is the Windows twin of `worker-bench --mode authority`.

Until 2026-09-01 the evidence above was the whole of this section, so a macOS result read as
covering both platforms --- the same shape as the README sentence corrected the same day, and
the same shape as §6's own inverted error, in the other direction.

**Residual.** On **macOS**, a hostile document can still learn which paths exist; it cannot
read one, write one, or open a socket. On **Windows** it cannot write and cannot
`OpenProcess`, and it can read anything the user can (residual risk 4) and, on the reading of
the code above, open a socket. `sandbox_init` denials also do not appear in the unified log
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

**The current list lives in §3 and is the authority; do not count from this section.** It is
**eight** as of 2026-08-30, and it reached eight without anybody adding three of them here or
there: `split_document`, `redact_copy` and `redact_document` were each disclosed in their own
entries and absent from the one place that answers *how many*. That is this paragraph's own
subject arriving a third time, which is the argument for the mechanical check §3 now names ---
enumerate the registered commands reaching a writer and diff the set --- rather than for
another sentence telling the next person to remember.

**What bounds that is the same thing that bounds `spike_exit`, and no more.** The CSP is
`default-src 'self'` with no `'unsafe-inline'`, so the only script that runs is the one that
shipped --- residual risk 7, and the T8 invariant that keeps document text from becoming
script. The marginal authority over what was already reachable is real but small: a caller
that can reach `save_copy` can already reach `open_document` and the print path. It is
recorded here rather than left implicit because it is the first *write*, and a write is a
different kind of verb from the ones this surface had before.

**Three refusals, and each is a correctness property rather than a security one:**

- An **encrypted** source keeps its encryption, and one that nobody unlocked is refused.
  `lopdf` drops `/Encrypt` on save without a word, so a copy of a restricted document would
  come out unrestricted and look identical --- exactly the T5 shape, a false assurance,
  pointed at the document's own protection rather than at ours. Until 2026-08-28 the answer
  to that was to refuse every encrypted source; since then `save::rewrite` puts the file's
  own state back with `Document::encrypt` as its last step, so the copy is as restricted as
  the original. The refusal that remains is the one no key can satisfy: a document still
  locked parses to nothing, and is declined with a message naming the lock.
- A **page count that disagrees with the model** is refused, which is the only part of §5's
  external-modification story that exists yet.
- **Writing over the source** is refused, compared by canonical path so that two spellings
  of one file are one file.

**The write is atomic** --- sibling temporary file, rename --- so an interrupted save leaves
either the old file or the new one. The redaction path above needs the same property for a
different reason and states it separately; this one is not that, and does not claim to be:
**a saved copy is a serialisation, not a sanitation.** Nothing here removes a prior
incremental revision, and a copy that dropped no page garbage-collects nothing, so a copy of
a document carries forward whatever the original carried. That is correct for "save a copy"
and would be wrong for a redaction, and the two must not be confused when the redaction path
is built on it.

**One thing is collected, and stating the difference is the point.** Since 2026-08-26 a
rewrite that **dropped or moved a page** runs `sweep::collect` over what it produced, so the
content of a page the reader removed does not travel on inside the file. That is a promise
about *tpdf's own leavings* --- the objects this rewrite made unreachable --- and not about
the document's: an orphan the source arrived with is still carried forward. Extract pages and
Split go through this same `rewrite`, which is why the distinction matters more than it
sounds: their names state an exclusion the file has to honour. Residual risks 15 and 16.

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
sanitation (§T6.1, residual risk 16).

**A pending redaction reaches no writer, and that is carried by the type rather than by a
filter** (2026-08-26). Marking a region puts a `Redaction` in a table of its own with an id
space of its own, and `Plan::marks` is built from `EditState::marks` --- a list a redaction
cannot be in. So the failure this arrangement exists to prevent, tpdf writing a reader's
*pending* redactions into a saved file as annotations, is unexpressible rather than guarded
against: an outline drawn over words that are still there, in a document that has been handed
on, is a confident lie of exactly the kind §6 of `docs/PLAN.md` opens by refusing. The
alternative design --- one mark kind with an exclusion in `save.rs` --- would have been a rule
to remember on the day the next kind is added. Two tests pin it: the plan of a document with a
redaction equals the plan of the same document without one, and the reply carries it while the
plan does not.

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
  now names the kind, because a reader chooses between several. What it has *not* stopped
  being is the property that matters --- read the amendment rather than this bullet, which
  names the current set rather than counting it.

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
markup. The route by which it becomes *somebody else's* string is unchanged: it goes into
`/Contents`, and comes back through `annots.rs` into the comment panel and the comment
popup, which have treated a body that way since they were written.

**A second display route landed on 2026-08-20 and this said there was none.** The sentence
here read *"the note is displayed nowhere else while the document is open"*, which the marks
panel made false: `marklist.ts` puts every mark's note on screen, from `edits::MarkView`
rather than from `annots.rs`. The **mitigation is unchanged** --- the row's text is assigned
through `textContent` and nothing else, and the `sinks` gate scans the whole frontend, so it
covered the new file the day it appeared without anyone adding it to a list. What was wrong
was the scope claim, and the cost of leaving it would have been an auditor asking *"where
does a mark's note reach the DOM?"*, reading two file names, and missing a third.

**And the model's notes are this session's, which is narrower than the paragraph above
allows.** `Edits::open` builds `Doc::open(pages)` --- a fresh model with no marks --- so no
`MarkView` ever carries bytes read back out of a file; a reopened document's annotations
arrive as *comments*, through `annots.rs`, into the panel that has always treated them as
attacker-chosen. `MarkView::note`'s own doc comment claims the stronger thing, and it is
left claiming it: a string that is handled as data either way costs nothing to over-declare,
and the narrower reading is one feature away from being wrong --- restoring an edit journal
across an open would make it so without touching a line of this file.

#### T6.8 — What a document says about itself, added 2026-08-21

> Everything from here to T6.5 is about **reading**, not saving, and it accumulated under
> T6.3's heading — *Highlighting a selection* — one route at a time until the block was longer
> than the section holding it. The heading is added rather than the block moved, because moving
> a hundred and fifty lines to fix a filing error is the larger risk; the parent number stays
> wrong for the same reason. `AGENTS.md` cited **§T6.4** for the certificate bounds, which is a
> different subsection about marks, and now cites this one.

**A third display route landed on 2026-08-21, and it is the widest one yet.** The properties
dialog puts a document's `/Info` strings on screen --- `/Title`, `/Author`, `/Producer`, and
any custom key the document invented --- together with a signature's stated name, reason and
location. Every one of those is a string a stranger wrote, and the custom keys mean the
*label* is attacker-chosen too, which no previous route had: a comment's fields are named by
us, and here `properties.fields[n].name` is whatever the document put in its dictionary.

**The mitigation is the same one and needed no new mechanism.** `propertiesdialog.ts` assigns
every name and every value through `textContent`, creates no URL-bearing element, and sets no
attribute from a document string --- so the `sinks` gate covered the file the day it appeared,
exactly as it covered `marklist.ts`. What is new is worth naming rather than leaving implicit:
`docinfo::Properties` has **no field that could carry a URL or an action**, in the way
`outline::Target` deliberately has none, so there is nothing for the frontend to be tempted
by. `no_signature_field_may_carry_a_verdict` matches `Signature` exhaustively for a related
reason --- adding a field there is a compile error rather than a review question.

**One honest limit, and it is the same seam residual risk 7 names.** The values are bounded
in *length* (`MAX_VALUE_CHARS`) and in *count* (`MAX_FIELDS`), and both bounds are reported
rather than silent --- but nothing constrains what a value *says*. A `/Producer` reading
"This document is valid and verified" is shown as written, because it is what the document
claims and hiding it would be its own lie; what is prevented is tpdf appearing to agree, and
`properties.test.ts` asserts that against exactly that input.

**A fourth route, and this one changed what parses hostile bytes rather than what displays
them: tpdf reads certificates as of 2026-08-21.** A signature's `/Contents` is a DER blob the
document chose, and `docinfo::parse_certificate` now hands it to `cms` and `x509-cert`. So
there is a second ASN.1 parser in the trust boundary beside PDFium's, on input just as
attacker-controlled, and three things bound it rather than one.

- **It runs in the worker, not in the app process.** `docinfo::scan` is reached through
  `Request::Properties`, so the new parser sits behind the same sandbox as everything else that
  reads a document. This is the property T1 exists for and it needed no new mechanism, which is
  the argument for having built the boundary before it was needed rather than after.
- **The blob is bounded before the parser sees it.** `MAX_SIG_BLOB` is 1 MiB against a real
  blob of tens of kilobytes, and exceeding it is *reported* through `Limits::certificates_unread`
  rather than passed off as a document with no certificate. The bound has a test that can fail,
  which took two attempts --- see the trap; the first version could not distinguish refusing a
  blob from parsing one and failing.

  **This sentence was true of two parsers out of three until 2026-08-24.** `ber.rs` walks the
  same attacker-chosen bytes as `cms` and `der` and ran *before* the bound, so a 200 MB
  `/Contents` was measured, re-measured once per constructed level and copied into an
  allocation its own size, and only the result was compared against `MAX_SIG_BLOB`. It is a
  parser like the other two and is now bounded like them, on its **input**, at twice the
  bound --- the factor is what makes the check refuse nothing the output check would have
  accepted, since definite-length rewriting can shrink a value by at most half. The guard has
  no outcome a test can see, for exactly that reason; what its test pins is the factor.
- **Both crates are `no_std`-shaped pure-Rust decoders returning `Result`.** No `unsafe`, no
  allocation driven by a declared length the input chose, and every failure path here maps to
  `None` plus a counted limit. Nine packages, all `Apache-2.0 OR MIT` bar `flagset` which is
  `Apache-2.0`, swept over the whole tree rather than read off a README.

**Reaching a signature is bounded too, and it was not until 2026-08-24.**
`docinfo::read_signatures` walks the form's field tree, and it bounded the *depth* of that
walk and the number of signatures it would report --- neither of which stops **fan-out**. A
group node carries no `/FT`, so it emits no signature and `MAX_SIGNATURES` never fires; a node
whose `/Kids` names itself sixty-four times therefore costs 64^8 pops inside a depth bound of
eight, on a file of a few kilobytes. `MAX_FIELD_NODES` (4,096) bounds the pops themselves,
which is the shape `links.rs`'s `MAX_TREE_NODES` had already taken for its own tree walk.
Hitting it is reported through `Limits::signatures_dropped` --- the same counter the signature
bound reports through, because to a reader they are one event: this scan stopped looking, and
what it says about signatures is incomplete.

**And the honest limit, which is the part a reader would get wrong.** Parsing a certificate is
not verifying one. tpdf builds no chain, holds no trust store, consults no revocation list, and
never checks the signature against the bytes it covers --- so a document can name itself
anything and tpdf will show it. What the certificate buys is a second, differently-sourced
claim about who signed, next to the `/Name` the signer typed; `properties.ts` shows both and
says when they disagree. `NOT_CHECKED` states all four omissions and is shown wherever a
signature is, and `no_certificate_field_may_carry_a_verdict` makes adding a field to
`docinfo::Certificate` a compile error rather than a review question. The one unhedged
statement the certificate rows make is `self_issued`, which compares two byte strings and is
deliberately not rendered as a warning: every root in every trust store is self-issued.

**Extensions are decoded as of the same day, and the interesting part is what the bound on
them nearly was.** `decode_extension` reads key usage, extended key usage and basic
constraints, all inside the already-capped blob, so no new byte reaches a parser. The obvious
signature for it is `T: der::Decode<'static>`, which compiles, and which on borrowed bytes is
satisfiable only by leaking them --- an allocation an attacker sizes and chooses the count of,
one per extension per signature, in the process the sandbox exists to contain. The bound that
is actually correct is `for<'a> Decode<'a>`, which the three owned types satisfy and which
borrows for the length of the call. Nothing would have gone red: a leak is not a crash, the
gates were 16/16 with the leaking version in the tree, and clippy has no lint for it. The trap
of that name carries it.

A malformed extension is **counted**, not read as an absent one, because those are opposite
claims: an absent key usage places no limit on the key, and a malformed one places an unknown
limit. Absent is the reassuring branch, which is the direction a silent failure would fall.
And what an extension states is still the issuer's word --- the constraint binds the key, and
only a chain to a trusted issuer makes it mean anything, so `NOT_CHECKED` now says that too.

**Timestamps, same day, and they add no parser.** An RFC 3161 token is itself a CMS
`SignedData`, so reading one exercises the crates already described here on bytes already
bounded by `MAX_SIG_BLOB` --- the token sits *inside* the signature blob. The only new decoding
is `TSTInfo`'s, and it is deliberately positional: four opaque values skipped, the fifth
required to parse as a `GeneralizedTime`. That last requirement is the bound. A structure
malformed enough to shift the fields yields **no** time rather than a time read out of the wrong
field, which matters because the output is attributed to an authority --- a plausible wrong
instant presented as a third party's attestation is worse than silence, and it is what a
positional walk with no type check would produce.

Two refusals beside it, each with a test that reaches it: an attribute carrying more than one
value is refused rather than guessed at, and a CMS whose `eContentType` is not `id-ct-TSTInfo`
is not read as a timestamp however well-formed its content is.

**That limitation is closed, and closing it added a parser of our own.** A `/Contents` blob
encoded in **BER with indefinite lengths** was refused outright by `der`, so tpdf read no
certificate and no timestamp from it --- one of ten real signed documents to hand, and the class
affected is CAdES, which is where timestamping is routine. `ber::to_definite_length` walks the
blob and hands the crates a definite-length value.

It is roughly 150 lines rather than a dependency, and it is the only code here that reads
attacker-chosen bytes without a third party between us and them, so its bounds are the point:

- **Nesting is capped at `MAX_DEPTH` (64)** against a real signature's twenty-five, and there is
  exactly **one** copy of that bound. `emit` runs only after `measure` walked the same bytes and
  returned, so it carries no second guard --- two copies would each refuse the blob alone, and a
  mutation of either would have survived.
- **A length field is capped at `MAX_LENGTH_BYTES` (4)** and a tag at `MAX_TAG_BYTES` (5). X.690
  reserves `0xff` as a length-of-length and this refuses it by the same rule.
- **Every read goes through `get`, never an index.** The two offsets built from a length the
  document chose --- past a header, past a value --- go through `checked_add`; the rest are a
  cursor plus at most five, and a cursor never exceeds the slice's length, so they cannot wrap.
  A value claiming more bytes than it has, a child overrunning the length its parent declared,
  and an indefinite value that never terminates are each refused rather than trusted, each with
  a test whose input reaches only that rule.
- **Output growth is bounded by input.** An indefinite header plus its marker is four bytes and
  a definite one is at most six, so the rewrite can grow a blob by at most half, and
  `MAX_SIG_BLOB` still bounds what reaches the parsers.
- **It refuses rather than repairs.** DER constrains more than the length form --- `SET OF`
  ordering, a canonical `BOOLEAN`, primitive strings --- and none of that is touched. A blob
  violating one is refused by the parser after it and counted as unread, which is the same
  outcome as before for every case this does not fix.

The walk is also what decides where the blob **ends**, replacing a scan for the last non-zero
byte. That is a security-relevant change as much as a correctness one: the old rule handed the
parser however many padding bytes preceded the last non-zero one, and the new one hands it
exactly one value. A blob that will not walk is counted through `certificates_unread` --- by its
own mechanism, with its own test, because it and the parser's counter can produce the same
number and one input reaches only one of them.

**A fifth route, and a third parser: tpdf reads XMP as of 2026-08-21.** The catalog's
`/Metadata` is an RDF/XML packet the document chose, and `xmp::scan` hands it to `quick-xml`.
That crate was **already in the tree** through Tauri's `plist` dependency, so this compiled no
new code into the binary --- but it is newly reachable from attacker-chosen bytes, which is the
only question that matters here. Four bounds, and the fourth is the one worth reading:

- **It runs in the worker.** Reached through `Request::Properties` like everything else that
  parses a document, so it is behind the T1 boundary and needed no new mechanism.
- **The packet is capped** at `xmp::MAX_PACKET` (1 MiB against real packets of 0.4--40 kB),
  nesting at 64 levels, and each value at 4 KiB --- with the value bound applied **while the
  value accumulates**, not to the finished string, since clipping at the end means holding
  whatever the document sent first. Every one of those is *reported* through `Xmp::unread`
  rather than answered with a packet that claimed nothing.
- **The stream is decompressed under the document's existing `MAX_DECODE`**, so a compressed
  `/Metadata` is no different from any other stream bomb.
- **Entity expansion is structurally impossible, not merely bounded.** `quick-xml` delivers
  every `&...;` as its own `GeneralRef` event and expands nothing; `unescape` resolves the five
  predefined names and character references, and refuses everything else. Nothing here calls
  `unescape_with`, which is the only door a custom entity could come through. So a
  billion-laughs declaration costs a dropped `DocType` event --- asserted by a test that
  distinguishes *not expanded* from *expanded quickly*, since a test asserting only that the
  parse terminated would pass on both.

**And the honest limit.** A conformance claim is a claim. tpdf does not validate a document
against PDF/A, PDF/UA or PDF/X, and the string shown is copied out of the packet --- so a
document may write anything it likes there, including the word *valid*. The row says *the
document's own claim, which tpdf does not check*, and `properties.test.ts` asserts that a
hostile conformance string cannot put a verdict word into a label tpdf wrote. Same posture as
the signature rows, same reason.

#### T6.5 — The frontend names the mark's kind, added 2026-08-18

**A reader can now choose Highlight, Underline or Strike out, so the kind travels on the
wire** --- `MarkKind` is a field on `edits::NewMark`. T6.3's bullet said the frontend cannot
choose the subtype; that is now the wrong sentence for the right property, and the property
survives intact.

**What the frontend chooses is a variant, not a string.** `MarkKind` is a Rust enum with
serde names, so an unknown name is a *deserialisation failure at the command boundary* --- the
command never runs. The `/Subtype` bytes are still literals in `save.rs`'s `match`, reachable
only by naming one of them, and that `match` is still what makes a new variant a compile error
rather than a mark written as something else. So the closed set moved from "one variant,
nothing to choose" to "a closed set, chosen by name", and at no point is a caller's string
written into the file.

**Five variants as of 2026-08-19, and the sentence above deliberately no longer counts them.**
It said "three" until a comment bubble and a box were added, and the number was never the
property --- a count in prose goes stale the next time the set grows, which is the failure this
repository already records about its own trap tally. The two new kinds are the ones a reader
*places* rather than selects, which changes nothing here: both are still variants named on the
wire, both map to a `/Subtype` literal through the same `match`, and the box additionally
carries an appearance stream built entirely from numbers of ours. Ask `MarkKind` in
`docmodel.rs` for the current set.

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

- ~~**No length limit.** A note is as long as the reader makes it.~~ **False since
  2026-08-25**, and it stood here for four days after that: `edits::too_long` refuses a note
  over `textbox::MAX_NOTE_CHARS` — 64 Ki characters — before the lock is taken, on
  `annot_mark` and `annot_note` alike. Found on 2026-08-29 while writing §T6.13, by reading
  the code the new command shares rather than the paragraph describing it. The rest of the
  bullet is still true and is why the bound is generous rather than tight: the memory a note
  costs is one copy per journalled version, in a process that already holds the document, and
  `annots.rs` bounds what it reads *back* independently — so a note longer than that clip is
  written whole and reported clipped on reopen, which is a display limit and not a loss of the
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

**A third joined them on 2026-08-23**: `page_crop_box` loads the page to read its `/Rotate`
and its `/CropBox`, so that a rectangle a reader dragged on screen can be turned into the box
the model holds. It is the same shape as the two above and adds the same nothing --- a handle
the frontend has, a page it knows, four numbers, and a parse in the worker that renders every
tile. It exists because the frontend is deliberately never told a page's `/Rotate`, so the
one place that can undo it is the backend.

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

**Two checks narrow what a wrong path can do, and both are correctness checks rather than
security ones.** The page count of the file named has to match the plan's baseline, and
since 2026-08-19 its **length, modification time and SHA-256 have to match what was recorded
when the document was opened** --- `fingerprint.rs`, and `docs/PLAN.md` §5. Pointing this at
an unrelated document is now refused unless that document is byte-identical to the one the
reader opened, which is a considerably narrower gap than "happens to have the same number of
pages".

**That is a real narrowing and it is still not a guarantee, and the difference is worth
being exact about.** It is not a check that the path names the open document; it is a check
that the file at that path is unchanged since *some* document was opened, and the two
coincide only because the frontend passes the path it opened. A caller free to choose the
path could pass a *different* file it had first arranged to be fingerprinted. So the absence
§T6.1 records is unchanged --- the source path is still the frontend's, unchecked against
what the render service opened --- and this remains the command where checking it would
matter most. What the fingerprint removes is the accident, not the adversary.

**Fail closed, which is the part that is a security property rather than a correctness
one.** A fingerprint that could not be taken refuses the save rather than permitting it, so
"could not look" never reads as "looked, and it was fine". `save_copy` deliberately does
not, because a copy risks a bad new file beside an intact original and the refusal above
names Save a copy as the way out.

**The modification time is deliberately not part of the deep comparison, and that narrows
what this paragraph may claim.** A file whose mtime moved and whose bytes did not is
*accepted*: `cp -p` preserves a timestamp across a rewrite and a `touch` moves one without
changing a byte, so the timestamp is evidence about neither. The digest is the comparison,
and the sentence above should be read as length-and-contents rather than as three
independent locks --- an attacker was never going to be stopped by a timestamp, and a
reader whose backup tool ran was being stopped by one. The mtime is still compared in the
one place nothing better is affordable: the look between staging and the rename, which
compares against what **staging** read rather than against what was opened, so that window
is milliseconds rather than the whole session.

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

#### T6.9 — A reader's password, added 2026-08-23

**A new kind of value crosses the boundary, and it is the first secret.** Every earlier
`Request` variant names page numbers, geometry and a reader's search query; `Unlock`
carries a password. Three things are worth stating rather than leaving to be noticed.

- **It grants the worker no authority it did not have.** The document's bytes are already
  mapped into that process --- that is what the handover is --- and a key to bytes you are
  holding is not a new reach. What it changes is that they stop being noise. A worker that
  cannot read them is a worker that renders nothing, so this widens nothing an attacker
  who had already compromised a worker could reach.
- **It travels on stdin and never in argv.** The pipe is the parent's, private to the two
  processes; a command line is readable from the process table by anything running as this
  user. That is the whole of the difference and it is why `Request::Unlock` is a message
  rather than a spawn argument.
- **It is not logged.** No diagnostic prints a request body, and the field's own doc
  comment says so, which is the only thing standing between it and the next person who
  adds a trace of the protocol.

**It is held in the app process for the document's lifetime, and that is a requirement
rather than a convenience.** `Held::password` is what `Workers::spawn_into` replays to
every worker after the first --- the one the pool grows under contention, and every
replacement for one that crashed --- because each maps the same bytes and meets the same
encryption. A design that unlocked only the first worker would render the page a reader
is looking at and refuse the next.

**What that costs, stated plainly: the password is in this process's memory while the
document is open.** So is every decrypted page of it, which is the more revealing of the
two, and neither is defended against something that can read this process's memory --- an
adversary who has that has already won. It is not written to disk, does not reach the
session file, and goes when the slot does. What is *not* done, and would be the next rung
if it were worth one, is zeroing the buffer on drop: `String` does not, the value is
copied by every `clone` on the way to a worker, and a partial job here would read as a
guarantee.

**The refusal is structured for a reason that is a security one as much as a usability
one.** `Refusal::locked` travels as a field, so nothing downstream matches on a message to
decide whether to prompt. A frontend deciding *"show a password box"* by looking for the
word "password" in a backend string is one wording change away from prompting for the
wrong refusal --- and the strings themselves are chosen in `progressive.rs` and
`worker_child.rs`, never taken from the document, so no failure path can become a route
for text a stranger wrote. That is the T8 property, in the one place a new error channel
was added.

**Two more places hold it as of 2026-08-23, and both are inside a boundary that already had
it.** `RawDocument::password` is the worker's own copy, kept because every question PDFium
cannot answer --- comments, links, properties, the character mapping, the update section a
save appends --- is a second parse of the same bytes with `lopdf`, and `lopdf` needs the same
key. That is the sandboxed process, holding a key to bytes it is already holding. And
`save_document` asks the service for it through `Job::Password`, once, for the arm that
appends: the read-back that checks the written cross-reference has to parse the file, and
without the key it would count zero pages and roll a correct save back. The value is a local
in that function and goes when it returns.

**Neither adds a hop the password had not already made.** It reaches the worker over stdin
on `Unlock` and is kept in the app process on `Held::password`; these two are reads of those
two, in the same processes. What they do change is the number of copies, which is the
paragraph above's point about zeroing on drop: there are now more of them, and none is
zeroed.

**What is deliberately *not* done: the frontend does not keep it.** `unlock.ts` holds the
typed password in a local for the duration of the retry loop and drops it, and nothing in
`App.svelte` stores it. Every later use --- pool growth, crash replacement, a save --- is
served from Rust. The webview is the least trusted place in the application (residual risk
7), so a password parked in component state for a document's lifetime would be the one hop
worth avoiding, and it is avoided.

**Seven commands ask for it as of 2026-08-28, where one did, and the count is the change
rather than the shape.** A rewrite used to refuse every encrypted document, so `save_document`
asked only for the arm that appends. A rewrite now re-encrypts what it wrote with the state
the load recorded, which needs the key twice: `lopdf` parses no objects at all without it, and
`Document::encrypt` puts the encryption back. So `save_copy`, `extract_pages`,
`split_document`, `merge_documents`, `redact_copy`, `redact_document`, `save_document` and
`print_document` each call `password_for`, which is one ask on `Job::Password` and a local
that goes when the command returns.

**This adds no hop and no lifetime.** Every one of those is the same read, from the same
`Held::password`, into the same process that already holds it, for the length of one command
--- and each was already free to make that read. What it does add is copies, which is the
zeroing paragraph above becoming a little more true: there are more of them, none is zeroed,
and a partial job would read as a guarantee.

**One deliberate non-extension: printing an edited encrypted document.** `save::print_bytes`
takes the reader's password since 2026-08-30 --- for a refusal, not for a job. With the key it
can tell an encrypted document apart from one it cannot read, so the refusal a reader meets
names the escape that exists (*print the whole document instead*, which routes the encrypted
bytes through untouched) instead of claiming the document could not be unlocked while it is
open on their screen. The job over an edited encrypted document is still refused: the bytes a
print job produces go to `NSPrintOperation` or `Windows.Data.Pdf`, which would need the key
themselves, so making it work means handing the platform a decrypted copy of a document whose
author encrypted it. That is a different decision from *let the rewrite work*, and it has not
been measured. Since 2026-09-01 the refusal is made in the worker rather than here: it travels
as `save::Job::Print` on `Request::Rewrite` and is decided in `save::rewrite_update`, because
the parse it depends on moved and the alternative is shipping the decrypted document out of the
sandbox in order to refuse it. (This paragraph said "`print_bytes` passes `None`" until 2026-08-31, two days
after it stopped being the mechanism; the outcome it described never changed.)

#### T6.11 — Redacting, added 2026-08-26

**No new capability, and one genuinely new kind of claim.** `redact_copy` is §T6.1's verb:
the same `dialog:allow-save` panel, the same `save::write_copy`, the same caller-supplied
source and destination, unchecked against the document the render service actually opened.
Everything that section says about the authority of a write applies here unchanged and is not
repeated. What is new is that this command **destroys content**, and that its reply asserts a
security property about the file it wrote.

**`Request::RedactPlans` is the parse, and it is on the right side.** Deciding what a removal
takes means reading the page's object list, which is a reading of attacker-chosen bytes, so it
is a worker request rather than coordinator work --- the same argument `Request::Append`
records. It names nothing the worker could act on: rectangles in, a count and some sentences
out, and nothing is removed by answering it. The removal itself happens in the coordinator,
inside the rewrite that already parses that file with `lopdf`, which is residual risk 18 and
is not widened by this.

**The claim is the exposure, and it is bounded by the type rather than by care.**
`redact::Applied` cannot carry `verified` without an empty `why`, and every object the removal
could not take becomes a reason. So the failure mode this section would otherwise have --- a
reader told a file is clean when it is not --- needs a defect in `verify::scan` rather than an
omission at a call site.

**Residual, and it is large enough to state plainly.** This reaches five rows of
`docs/PLAN.md` §6's carrier table and no more, all of it added 2026-08-27. The page's own
content: the show operators, and the shadow text (`/ActualText`, `/Alt`, `/E`) in **both** of
that row's homes --- the marked-content property list the glyphs sit inside, and the structure
element that span belongs to, reached by `/MCID` through the parent tree, together with its
ancestors. An **annotation whose `/Rect` overlaps a region**, with its popup and its replies,
removed together with every reference to it rather than unlinked from the page. And the
document's own description of itself --- `/Info` and the catalog's `/Metadata` --- taken whole,
because a title that paraphrases a redacted line is reachable by no rule that matches text.
And the **outline entries whose title names what went**, with the subtree under each: a
bookmark title *is* the heading it points at, measured at 163 of 165 verbatim page text
against a 4% cross-document control, which is what licenses a string rule here where one was
refused for metadata. Entry by entry rather than the whole outline, so one redacted heading
does not cost a reader their table of contents. And the **form fields whose answer went** ---
by either of two rules, that every widget under the field has gone, or that its value or its
`/DV` default is text that went, which is that row's *widgets outside the redacted rectangle*
stated as a property rather than as a location.

**An XFA form is refused rather than half redacted**, which is the one place this subsystem
answers a carrier by declining the operation. An XFA packet is a complete XML copy of every
answer, so taking the field values and leaving it removes nothing a reader could not recover;
a rule that reached inside it would be a second form implementation. The refusal is in the
pre-flight, before anything is touched, and is keyed on the redaction --- an ordinary copy of
an XFA form still works, because a serialisation makes no claim for the packet to falsify.

Everything else in that table survives: an annotation *away* from every region, an outline
entry naming something else, and a form field naming an answer that did not go (all three
deliberate --- a reader's other comments, their other bookmarks and the rest of their form are
not theirs to lose), page labels, embedded files, and any prior
incremental revision, since a copy is a serialisation rather than a sanitation (§T6.1). So does any structure element the parent-tree walk could not reach,
which is reported as unverified rather than passed over.

**The metadata strip, the outline removal and the field removal are properties of the
redaction and of nothing else.** A copy, an extract, a split, a merge and a print job all still
carry `/Info`, XMP, every bookmark and every answer across untouched, which is §T6.1's position
and is held by **one** condition guarding all three --- so one mutation of it reddens all three
controls, which is why only one of them names it. An image or a
vector drawing inside the region is reported and left. A CID-encoded document cannot be
scanned at the byte level at all, which `verify::scan` reports as a blind spot rather than as
a pass. None of that makes the answer *wrong* --- it makes the answer *not verified*, which is
what the reader is told.

#### T6.12 — Redacting the reader's own file, added 2026-08-27

**No new capability and no new parse.** `redact_document` is §T6.11's command with
`save::stage_in_place` where `save::write_copy` was, so it is §T6.7's write with §T6.11's
claim; both sections apply unchanged and neither is repeated. It takes **no destination**,
which makes its authority narrower than its sibling's rather than wider: the only file it can
write is the one named as the source, and `save_document` has been able to write that since
2026-08-19.

**One authority is genuinely new, and it is the reader's rather than an attacker's.** Until
today a redaction could only produce a file; now it can destroy one. A caller able to reach
this command can overwrite any PDF the reader can write with a redacted version of itself ---
which is what `save_document` can already do with an edited version of itself, so what this
adds over the existing surface is the removal rather than the write. The CSP is what bounds
it, as it bounds every command on this surface (residual risk 7).

**The order is what keeps a failure from being a loss.** Stage a sibling, fingerprint the
source, close the document, check the source has not moved, rename. Every refusal `save.rs`
states arrives while the reader still has their document and their marks, so the only window
in which content can be lost is between the rename and the read-back --- and a rename is
atomic, so what is in that window is a file that is either the old one or the new one.

**A file that could not be proved clean is still written**, which is §T6.11's decision
arriving where it costs more. §6's rule is *never claim clean*, not *never write*: rolling
back would hand the reader the words they asked to destroy while reporting a failure. What
they get instead is the redacted file and the reasons it could not be shown clean, which is
the same answer the copy gives.

**The journal is spent, not truncated, and that is the stronger property.** §6 asks for the
journal to be truncated at the apply. Truncating leaves every earlier command undoable, so a
reader could step back to a state whose regions were still pending while the file no longer
holds the words. The close drops the model entirely and the reader reopens from the path, so
no undo reaches across the removal. Nothing was built for this --- it is what an in-place
write already does.

**Residual.** Everything §T6.11 lists survives here identically, and one thing more is worth
saying because the in-place form is the one that suggests otherwise: **this does not sanitize
what is outside the file.** A backup, a versioned snapshot, a sync client's copy in the cloud
and the sectors the old file occupied are all untouched, and the reader's original is now the
only copy they had. §T6's own residual says this; it is repeated here because a command that
overwrites the original is the one where a reader will assume it has been dealt with.

#### T6.10 — Moving a mark, added 2026-08-23

**No new authority, and it is the T6.2 shape.** `annot_move` takes a document handle, a mark
identity and two numbers, mutates a `HashMap` in the app process, and opens no file, writes
none and reaches no worker. `page_crop_box`, added in the same window, is the exception and
is covered in §T6.6 rather than here, because what it does is a *crop* question.

**Two numbers off the wire, and the two checks on them are in different places for
different reasons.** `edits::displace` refuses a `dx` or `dy` that is not finite, at the
wire boundary and before the model sees it --- which is the check that matters, because a
`NaN` reaching a `/Rect` is written into a content stream by `format!` as the literal
`inf`, and this repository has already paid for that once with an unchecked `f32`.

The **page clamp** is deliberately not there. `docmodel` cannot bound the move --- the
page's size in points is the renderer's answer and not the model's --- so the viewer clamps
before it sends, exactly as it clamps the geometry of a mark being placed, and both layers
say so in their doc comments. A caller bypassing the frontend can therefore move a mark off
its page. That is a correctness defect in the file it produces rather than a reach: the
value goes into a `/Rect` as a finite number, and a rectangle outside the media box renders
as nothing in any reader. Listed rather than fixed because the fix belongs where the page
size is known, and residual risk 7 already bounds who can call this at all.

**Which marks it will move is a product rule and not a security one.** `isMovable` refuses
the four kinds anchored to words a reader selected, because a wash dragged off its line
marks nothing; the model will move any mark, and that asymmetry is deliberate and stated in
`markband.ts`.

#### T6.13 — Editing a comment the file came with, added 2026-08-29

**One genuinely new thing, and it is not the string.** `annot_rewrite` takes a document
handle, a page identity, a body and — unlike every command before it — **an object number
out of the document's own graph**. Every other write command names something this
application issued and numbered; this one names something the *file* numbered, because
`annots::Comment::id` is a position in one scan and the identity has to survive a round trip
through the webview and back into a worker.

So the argument selects a target inside the document, and the question is what a wrong or
hostile one can reach. Three bounds, in the order they apply:

- **Object 0 is refused at the wire boundary**, in `edits::rewrite`, before the model sees
  it. It is the head of the free list and can never be an indirect object, so a plan naming
  it is a defect in tpdf rather than a file that changed.
- **The page is refused by the model.** A rewrite names a page as well as an object, and one
  naming a page that does not exist or has been deleted is refused there — which is also
  what makes the edit die with its page rather than reaching a writer as an instruction about
  a comment the written file does not contain.
- **`save::set_note` refuses anything that is not an annotation**, against the bytes, in the
  worker. This is the bound that matters: without it a plan naming a page object would write
  `/Contents` onto the page, where the key means the page's content stream, and the save
  would report success over a destroyed document. It is one function shared by both writers
  precisely so that adding the second caller could not lose it.

**No new authority otherwise.** The command mutates a `HashMap` in the app process, opens no
file, writes none and reaches no worker; the write happens later, on the save path already
covered by §T6.1 and §T6.7, and in the worker for the reason residual risk 18 gives.

**The body is the reader's own string**, treated exactly as §T6.4's is and bounded by the
same `edits::too_long` — 64 Ki characters, refused before the lock is taken. It is checked
here rather than inherited because this is a second route into `/Contents`, and a bound
enforced on one of two routes is not enforced.

**What it does not do is read the comment first.** The model has never parsed the document,
so it cannot tell the reader's new body from the old one, cannot know whether a rewrite
changes anything, and cannot restore the file's own text. Undo is what restores it, by
replaying without the command — which is the same mechanism every other edit uses, and is why
there is no "revert" command with a document read behind it.

#### T6.15 — Deleting a comment the file came with, added 2026-08-30

**The same new thing §T6.13 names, and the same bound.** `annot_discard` takes a document
handle, a page identity and **an object number out of the document's own graph** — the second
command to do so. It is bounded the same way: the model checks only the page, and the writer
refuses an object it cannot find or that is not an annotation, so a plan naming a font, a page
or the catalog is refused rather than acted on. There is no new authority, no file is opened
and no worker is reached; the deletion is a `BTreeMap` entry in the app process until a save.

**What is genuinely different is that this one removes bytes.** Every write command before it
adds. `pagetree::forget` takes the annotation's dictionary and every reference to it, and the
rewrite's mark-and-sweep then collects what the annotation was the only reference *to* — its
appearance stream, which for a comment is a drawing of the words. Without the sweep those
bytes stay in the file with nothing pointing at them, which is exactly the leftover
§T6.11's picture case records; the condition the sweep runs on gained a clause for it and a
test greps the written bytes rather than asking what the page draws.

**It is not a redaction and must not be read as one.** What is promised is narrow and is the
one thing a reader can believe: *tpdf does not leave behind what it was told to remove*. A
comment's words may still be elsewhere in the document — quoted in another annotation, in the
page's own text, in an XMP packet — and nothing here looks for them. Whole-document sanitation
is `docs/PLAN.md` §6 and is a different promise.

**A deletion forces a full rewrite**, which is a security-relevant consequence rather than
only a performance one: the previous revision does not survive, so a signed revision that an
append would have left intact for a validator is gone. §T6.7's residual already states that
trade for the rewrite path; a deletion is the one edit that cannot choose the other side of
it.

**Residual.** A comment one of the reader's own replies answers is refused, so `/IRT` cannot
be left dangling by this command. A reply naming an object the *file* does not have is still
the writer's check, unchanged.

#### T6.14 — The nib a reader picks, added 2026-08-30

**No new command and no new authority.** Choosing a nib calls no Tauri command at all: it
sets a field on the viewer, and the number reaches the backend as one more field on the
`annot_mark` payload §T6.2 already covers. Nothing here opens a file, writes one or reaches a
worker.

**One new number off the wire, and it is the T6.10 shape exactly.** `Mark::width` is written
into an appearance stream by `format!("{width} w")`, which is the route `displace` refuses a
non-finite offset for and `channel` clamps a colour for: JSON has no `Infinity` literal, but
`1e40` is valid JSON and is `f64::INFINITY` by the time it is in Rust, and `format!` spells
that `inf` — three letters in the middle of a content stream, which is a syntax error. tpdf
would write a file no reader can parse and sign its name to it. `edits::nib` is the fourth
guard of that family, at the wire boundary and before the model sees the value.

**Clamped rather than refused**, which is `channel`'s choice and not `displace`'s. The
distinction is what the number means: an offset and a coordinate say *where* a mark is, and a
mark silently moved is not the mark the reader drew; a width says how heavy it is, and a line
a quarter point off what was asked for is still that line in that place. A non-finite value
has no clamped meaning — `f64::NAN.clamp(a, b)` is `NaN` — so it becomes the default, which
is also what a caller sending no width at all gets.

**The bound is a range and not the table.** `NIB_MIN` and `NIB_MAX` in `docmodel.rs` are what
the wire is held to; `marknibs.ts` is what a reader can pick, and a frontend test asserts
every entry sits inside the range. A caller bypassing the frontend can therefore ask for any
width in `0.25..=24` — which is a correctness question about a file rather than a reach, and
residual risk 7 already bounds who can call `annot_mark` at all.

#### T6.16 — The gesture an erasure belongs to, added 2026-08-30

**No new command and no new authority.** Grouping an eraser sweep adds no Tauri command:
`annot_erase` and `annot_remove` each gain one field, and both already existed with the reach
§T6.4 states. Nothing here opens a file, writes one or reaches a worker.

**One new number off the wire, and it is the smallest of the family.** `sweep` is a `u64` the
frontend mints, one per release of the pointer. Unlike `Mark::width` it reaches no content
stream and no file: nothing is written from it, no table is keyed by it, and no lookup uses
it. The model's only question is whether two *adjacent* journal entries carry the same value.
`edits::gesture` is the door, and what it buys is a type: `SweepId` is a `NonZeroU64`, so
*belongs to a gesture* and *is gesture number nothing* cannot be the same value, and zero is
the wire's spelling of the second.

**What a hostile caller gets is worth stating exactly, because it is nearly nothing.** Sending
an arbitrary number groups commands it issued back to back into one press of undo — which it
could equally have achieved by not issuing them. The number cannot reach a command it did not
send, because grouping is adjacency in the journal and the journal only holds what was
accepted. The worst outcome is a reader pressing undo once and losing more of their own recent
editing than they meant to, recoverable with redo, and residual risk 7 already bounds who can
call these commands at all.

**Why the frontend mints it rather than the backend.** The backend sees a sequence of
commands and cannot see where a gesture ends; a backend-minted grouping would have to be a
timer, which is a decision about the clock rather than about what the reader did. The trust
this places in the webview is the trust already placed in it to send the commands at all.

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

**"Raw pixels rather than anything parsed" is true of the format the viewer asks for and not
of every format the protocol serves.** `tile://` takes `?fmt=raw|png` (`protocol.rs`), and on
`png` the app process encodes the worker's pixels with the `png` crate and the webview decodes
them with `createImageBitmap` (`tiles.ts`) — the platform's own image decoder, in the process
that holds the `invoke` surface. So there is a second hop where bytes that began in the worker
reach a parser on this side of the boundary, and it is not the one this section is about.
Three things bound it, and they are the reason this is a sentence rather than a residual risk.
**Nothing in the viewer asks for `png`**: `autobench.ts` is the only caller that sets it, so a
reader never takes this route. The bytes are **not the document's** — they are a re-encode of
a rendered RGBA buffer, so reaching the decoder with anything chosen requires a PDFium exploit
first, which is T1. And the encode is ours (`render::encode_png`), not a passthrough of
anything the worker framed. What would change the answer is the viewer ever asking for `png`
in earnest, which is a decision to re-take here rather than in `tiles.ts`.

**This section said "today none of it reaches the UI at all — the frontend renders tiles and
nothing else" until 2026-08-02, and that stopped being true when the sidebar and search
landed.** Outline titles reach the DOM (`sidebar.ts`, `title.textContent = row.title ||
"(untitled)"`) and so does every search result (`results.ts`), including the matched
substring the query is highlighted in.

**A fourth source landed 2026-08-21: the words a mark covers.** The marks panel lists a
highlight nobody typed a note on by the phrase it sits on, taken off the page by
`selectionQuadsByPage`, so a row of that panel can now be document text where before it was
either the reader's own note or the literal "No note". Nothing about the mitigation changes
--- `marklist.ts` assigns it through `textContent` like everything else, and the invariant
below is what makes that sufficient rather than a promise about this one call site.

**A fifth source the same day, and it is the widest: the properties dialog.** A document's
`/Info` strings, its custom keys --- where the *label* is attacker-chosen too --- and a
signature's stated name, reason and location all reach the DOM through
`propertiesdialog.ts`. §T6.8 is the worked-out version and is not repeated here; what
matters at this level is that it needed no new mechanism, because the invariant below is a
property of the frontend rather than a promise about each call site.

**What does *not* belong on that list, checked rather than assumed: the sentence a document
that will not open now shows.** `progressive::open_failure` returns one of five literals we
wrote, chosen by PDFium's error code and carrying no byte of the document, and `refuse`
answers every later request with that same string. It is the same shape as
`outline::Target::Refused`, and it is worth stating because the obvious next edit --- naming
the file, or passing PDFium's own message through --- would put attacker-chosen text on a
path that has none today.

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
`commentlist.ts` and `commentpopup.ts`. The reader's *own* marks reach it through
`marklist.ts` as of 2026-08-20, on the same terms and with a narrower origin --- §T6.4 has
which strings those are, and why a file list is not what makes any of this safe.
Every one of those assignments is `textContent`, so
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

Defined 2026-07-31 in `src-tauri/src/ocr.rs`, built as a process on 2026-08-27
(`src-tauri/src/ocr_worker.rs`) and **running in production since the same day**: every
`redact_copy` and `redact_document` renders the regions it removed from and has an engine read
them, through `src-tauri/src/ocr_gate.rs`.

**This paragraph said "no engine is implemented yet, so nothing below is running in
production" until then**, which was true when written. It is left visible because §6 below
records the same failure at four days' remove and calls this direction the quieter one: a
mitigation present and disclaimed reads as diligence, and is what a reader budgets their
remaining work against.

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

**Windows, since 2026-08-29, and it needs no profile of its own.** `src-tauri/src/ocr_windows.rs`
drives `Windows.Media.Ocr`, and `OcrWorker::spawn` contains its child with
`sandbox_win::Containment::default()` — a job object plus low integrity, which is **the same
containment the parser worker gets**, not a relaxed one. That it is enough was measured before
the engine was written rather than assumed from the parser worker's use of it:
`examples/win_ocr_probe.rs` reads the same strings inside that containment and outside it and
gets identical answers (`BUILD.md`, 2026-08-29). So the table above has no Windows column,
because there was no ladder to climb — the first rung tried was the one that ships.

Three differences from the macOS arm are worth stating rather than leaving to be inferred:

- **A check where macOS has an application.** `apply_sandbox` *causes* the macOS child to lose
  authority and fails loudly if it cannot. By the time the Windows child runs an instruction
  the decision was taken by whoever spawned it, so `serve` calls `sandbox_win::assert_contained`
  instead — which is what turns "the parent is supposed to contain us" into something that
  fails when the parent stopped doing so.
- **The abort risk is inherited reasoning, not a Windows measurement.** The paragraph above —
  an engine that can abort its host must not share a process with unsaved annotations — is why
  this is a separate process on both platforms. On macOS the abort was observed; on Windows it
  has not been, and keeping the process boundary there is a decision to pay for a boundary
  whose necessity is untested rather than to discover it in a crash report.
- **`Options::language_correction` cannot be honoured.** Vision has
  `setUsesLanguageCorrection`; this engine has an internal language model and no switch. It
  matters here because a corrector turns marks it cannot read into plausible words, which is
  the wrong bias when the question is whether anything is readable. Measured 2026-08-29: no
  correction observed at 44 px or at `ocr_gate::MIN_CONTROL_PX`. That is support and not proof
  — at both sizes the engine read clean text exactly, so it was never near its limit, and a
  corrector only shows where a recogniser is struggling. Listed in the residual risks for that
  reason.

**This narrows T5's claim rather than widening it.** OCR is the only check that can speak about
an image carrier, since a byte scan cannot see into a `/DCTDecode` stream. `ocr.rs` therefore
makes "clean" unreachable except through a positive control the engine had to read back from
the same probe image, sized from the smallest box the redaction covered — a control drawn larger
than the redacted text proves only that the engine reads larger text. Every engine failure, and
a missing control, produce `NotVerified`, never `Illegible`.

**What the wiring adds to the trust boundaries, and what it deliberately does not.** The gate
runs in the app process and touches three things:

| what it handles | where it came from | what bounds it |
|---|---|---|
| the written file, reopened to render it | our own writer, from the reader's document | opened through `RenderService` like any other document, so it is parsed **in a parser worker** under §4's profile — the coordinator never parses it |
| tile pixels | a parser worker's mapping | a fixed-size RGBA buffer with no format in it; `Pixels::is_consistent` refuses one whose length disagrees with its dimensions |
| the probe image | assembled here from two strips | `room_for` in the parent and `frame_of` in the child both bound it against `PIXELS_CAPACITY`, so neither end trusts the other's arithmetic |

So the gate adds **no new parse in the coordinator**. What it does add is a second worker per
save and a second file open of a path the app just wrote — the same filesystem authority
`save_copy` already holds.

**On a platform with no engine the gate says so once and the file is not certified.**
`OcrWorker::spawn` returns `NO_ENGINE` on Windows, which becomes one sentence in
`Applied::why` rather than one per region, and `verified` is false. A skipped check that read
as a clean answer would be this document's own T5 failure arriving through the platform gate.

**The coverage this has, stated as a number rather than implied.** A region whose page yields
no qualifying control is `NotVerified`: measured across 41 documents, 45.9% of realistic
regions had no surviving word of at least `MIN_CONTROL_CHARS` characters at or below the size
that was removed. That is a ceiling on this design, and the curve has no flat part to move
to — see `docs/PLAN.md` §6.

**It was called *the* ceiling until 2026-08-27, and it was not the binding one.** The gate
rendered the rows a region's rectangle covers as a **full-width** strip, so the engine was
shown the whole line and read back the neighbouring words the removal was right to leave —
which `adjudicate` then counted as text surviving inside the region. `ocr_gate::mask_columns`
now blanks the strip outside the region's own columns before the control is stacked under it.
On the same 104 regions of the same corpus, with the control's standard untouched, *shown
unreadable* went from **18 to 63** and *still reads as text* from **54 to 6**, every one of
those six inside the region's own columns. So control availability is one of two limits and
was the smaller; the paragraph above named the other as the only one.

The cost is three regions that moved to *could not be checked* — a nearly blank image is
harder to read a control off — which is a wrong *legible* becoming an honest *not verified*.
What makes the masking sound is route B: `redact::covered` marks a text object when it
**overlaps** the region, and a removal takes the whole text-showing operation, so no glyph
overlapping the region survives a correct removal. Everything the mask erases is something the
reader did not mark. If a removal ever splits a text object, this reasoning has to be redone.

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

⚠ **And on 2026-08-25 the outcome it is supposed to prevent was observed.** A viewer
mutation run stalled on a live `tpdf.exe --render-worker --prespawn --tile-handle 2028`
whose parent pid had nothing behind it: the app had exited and the warmed pre-spawn was
still there twenty-nine minutes later, idle, holding the stdout and stderr it had
inherited. What that establishes is the *outcome*, not the mechanism — whether the job
object was assigned to that worker at all, whether its handle was closed early, or
whether the exit path leaks a handle that keeps the job alive, is **not established**,
and no probe here can currently say. Read the row above as: memory and process creation
are measured, orphan cleanup is intended and has one counterexample. The probe that
would settle it does not need to kill itself after all — spawn the pre-spawn from a
*child*, kill the child, and look. `docs/TRAPS.md` carries the entry.

**Evidence that the whole path works, not just the pieces**: `worker-probe` passed 11/11 on
2026-07-29 on `text-base14`, `text-cid`, `vector-heavy` and `rotated`, including
**pixel-identical** tiles
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
4. **A contained Windows worker can still read any file the user can, and nothing in the
   containment denies it a socket** (§6, §T4). This entry read
   "Windows compiles and is entirely uncontained ... nothing uses it, so the risk is
   undiminished" until 2026-08-02, and the containment had been wired since 2026-07-29 — the
   correction is in §6, along with why an under-claim is the more expensive direction to get
   wrong. What remains is the ceiling rather than the gap: low integrity denies writes and
   `OpenProcess`, **not** reads. Closing it needs a restricting SID, which stops the loader
   before `main` and is only reachable through Chromium's initial-token handover. The
   mitigation meanwhile is that the worker is never given a path — document and output arrive
   as inherited handles — so a compromised worker must guess at what to read rather than
   being handed it. **The network half was added on 2026-09-01 and is a reading of the code,
   not a measurement**: `sandbox_win` sets job-object limits and an integrity level and makes
   no network call, and integrity level is not what gates network capability on Windows —
   AppContainer is. §T4 names the one rung of `examples/win_sandbox_probe.rs` that would
   settle it. macOS denies socket binds and that *is* measured (`worker-bench --mode
   authority`). A platform with neither mechanism still falls back to in-process, records
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

    **One class of them stopped evaporating on 2026-08-24, and by a different mechanism.** A
    worker that cannot load PDFium at all used to return `Err` and exit 1, so the only thing
    that reached a reader was the coordinator's epitaph --- `worker stopped answering (exited
    with 1 (0x00000001))` --- for every document, by every route. It now answers requests with
    the reason instead, over the reply pipe the protocol already has, which is not a logging
    channel and needs no writable path. The coordinator's open path also notes the failure
    through `diag::note`, which it did not before: a session in which nothing could be opened
    left an empty log, byte-identical to a session with nothing wrong. What is unchanged is
    the general case above --- a worker that *crashes*, or dies after the document is open,
    still says nothing a reader can send back.
14. **`save_copy` writes a PDF anywhere the reader can write** (§T6.1), added 2026-08-16 ---
    the first command on this surface that creates a file, and its authority is the app
    process's rather than a panel's. The path comes from the frontend, so a native save panel
    is the *interface* and not the bound. What bounds it is residual risk 7: the CSP admits
    only the script that shipped. The marginal authority is small --- a caller that can reach
    this can already reach `open_document` and the print path --- and it is listed because a
    write is a different verb from the ones this surface had, not because the CSP is believed
    to be weaker than it was yesterday.
15. **A saved copy is a serialisation and not a sanitation** (§T6.1), **narrowed
    2026-08-26**. Nothing on that path drops a prior incremental revision, and a copy that
    dropped no page collects nothing --- so whatever the source carried, the copy carries.
    That is right for "save a copy" and wrong for a redaction, and the redaction path must
    not be built on it by assuming otherwise. **The narrowing**: a save that dropped or moved
    a page now collects what *that* made unreachable (risk 16), which is a promise about
    tpdf's own leavings and not about the document's.
16. ~~**A copy that lost a page keeps the deleted page's content in every place that is not
    the page tree**~~ (§T6.2), added 2026-08-17, **closed 2026-08-26**. `pagetree::drop_pages`
    removed the page object and every reference to it, and the mark-and-sweep that collects
    what those references *held* --- the content stream, the fonts, an embedded image --- ran
    on the print path and not on the save path. `save::rewrite` runs it now, whenever the plan
    dropped or moved a page, and two checks pin it in opposite directions: the content of a
    page that went is absent from the file, and the content of every page that stayed is
    still there.

    **What the entry got wrong is worth more than what it got right, because the wording is
    what kept it from being found.** It named the *deletion*, on the reasoning that deleting
    is the first operation where a reader could plausibly believe otherwise. Extract pages
    was already shipped on the same `planned_bytes` -> `rewrite` path and is a stronger case
    in every respect --- the command's own name states the exclusion, and the leak is total
    rather than partial. Measured on `links.pdf` before the fix: extracting page 1 of 8
    produced a file reporting **one** page and carrying **all eight** content streams, 4,139
    decodable bytes each. Split, added in 26.8.11, joined the same path afterwards and was
    covered by nothing. `docs/TRAPS.md` has the entry.

    **What is not closed**, and it is the larger half: this collects what *this rewrite*
    orphaned. A document that arrived with orphans in it still comes back with them (that is
    §T6.1's position, and risk 15 above), and nothing here touches the carriers `docs/PLAN.md`
    §6 lists --- an annotation's appearance stream, a form field's value, a thumbnail, and the
    structure tree's own copy of a page's alternate text. "Removed" means removed *from the
    page tree and everything only it held*, not yet from the document. (`/ActualText` inside a
    content stream is cleared by *Redact and save as* since 2026-08-27. That is a different
    command on a different path, and it does not make a save a sanitation.)

17. **A cropped page hides content and does not remove it** (§T6.6), added 2026-08-18.
    Everything outside the crop box is still in the saved file, still extractable, and still
    found by tpdf's own search --- a crop moves character boxes, not character indices. That
    is what `/CropBox` means, and it is the right behaviour for a crop. It is listed
    separately from risks 14 and 15 because "crop" is a word that sounds like removal in a
    way "rotate" and "move" do not, and because it is now the *second* operation a reader
    could plausibly believe removes something. Redaction is `docs/PLAN.md` §6 and is not
    built.

18. **The redaction verification parses inside the coordinator**
    (§3), added 2026-08-22 after an outside review found this document naming printing as the
    only coordinator-side parser while three edit writers had joined it. **Narrowed the same
    day**: a save that only adds marks is *prepared* in the worker now (`Request::Append`).
    The writers left after that were the rewriting save --- a deletion, a move, a turn, a crop
    --- and the two copy paths, and `lopdf` read the source bytes in the app process on those,
    under `spawn_blocking`, which moves the work off the async runtime and not out of the
    process.

    ⚠ **Every one of those writers is closed as of 2026-09-01, and what this entry is now
    named for is a *reader* it never listed.** `verify::scan` re-reads the file a redaction
    has just written and parses it here, on the blocking pool, to decide whether the removal
    was genuine. Its bytes derive from the reader's document, so it is the same exposure the
    writers had --- and it was invisible to this entry, to §3 and to
    `scripts/check_writers.py` alike, because all three enumerate the operations that
    **write**. A verification writes nothing. That is the second time this month an
    instrument keyed on writing hid a parse: `print::build` was the first, found by an outside
    review a day earlier. `docs/TRAPS.md` has it under *A risk and a gate both keyed on
    writing cannot see the path that only reads*.

    Two things bound it, and neither closes it. The bytes are ones tpdf wrote seconds earlier
    rather than the file as it arrived, so a hostile construction has to survive our own
    serialiser first; and the load is bounded like every other. What would close it is the
    move the others took --- `verify::scan` is already a pure function of bytes and needles,
    and the file it reads is one the coordinator has just created and could hand over as a
    descriptor. Nothing here is built.

    ⚠ **The append is not off this list, and this entry said it was until 2026-08-23.** Its
    *preparation* moved; its **verification** did not. `save::append_in_place` re-reads the
    whole file it has just written and parses it with `lopdf` in the app process, to check
    the cross-reference chained and the page count survived --- and the previous revision of
    that file is the attacker's bytes verbatim, so this is a coordinator-side parse of
    untrusted input on every append, which is the commonest save there is. It is bounded by
    the same `MAX_DECODE`. It was **also not** under `spawn_blocking` --- the `match` that
    calls it ran directly on the async runtime, unlike the three writers above, so a
    document engineered to make the read-back spin stalled the runtime rather than a
    blocking pool.

    **That half is fixed the same day.** The whole `landed` match is on the blocking pool
    now, which moves the rewrite's own work with it: `verify_before_commit` reads the
    source's metadata and renames, and it was on the runtime too.

    ⚠ **This said `verify_before_commit` "hashes every byte of the file" until 2026-08-31,
    and it never has.** That function has compared **length and modification time only**
    since 2026-08-19 --- `Fingerprint::agrees_shallowly`, deliberately, and `save.rs` says
    so where it is defined. The digest runs earlier: `rewrite_ready` compares length and a
    SHA-256 of every byte against `Plan::opened_as`, before anything is staged. Both are on
    the blocking pool, which is the claim this paragraph is about and the one part of it
    that was true. The distinction is not cosmetic for a reader of this document: the last
    look before the rename cannot see a replacement that preserved both fields, and the
    reason it is the cheap check --- the window between staging and the rename is measured
    in milliseconds --- is on `verify_before_commit` itself.

    ⚠ **And the process half closed 2026-08-26, so the append is off this list entirely.**
    The read-back is `save::Reread`, a seam taking the written file's **handle**, a length
    and the password; `save::InWorker` maps that handle read-only, spawns a sandboxed child
    on it, asks `Request::Reread` and drops it. The coordinator no longer holds the bytes,
    so there is nothing there to parse --- carried by the type rather than by anyone
    remembering, which is what makes it checkable: `the_coordinator_does_not_parse_the_file_
    it_wrote` writes a file that does not parse, hands over a verifier that says it is fine,
    and requires the save to succeed. It goes red on the code this replaced.

    Three things about that are worth stating rather than implying. **It gains the bounds
    the coordinator could not offer** --- the deadline and the memory bound this entry says
    need a separate process, which the append's read-back now has along with `MAX_DECODE`.
    **The obstacle `docs/PLAN.md` recorded was not the real one**: it said the worker "holds
    a mapping of the file as it was", and `save_document` closes the document before the
    write, so there is no such mapping --- the real constraint was that a child has to be
    started, at one spawn per in-place append. **And `lopdf` is deliberately still the
    parser**, where `Request::Open` already answers a page count: what is being tested is
    whether the cross-reference *chained*, and PDFium is lenient about exactly that.
    Measured on the day, not inherited --- `worker-probe` plants a trailer pointing at
    offset 999999999, PDFium opens it without complaint, and `lopdf` names the
    cross-reference table.

    ⚠ **And the rewriting save closed 2026-08-28, which is what the last paragraph of this
    entry said it needed.** `save::rewrite_update` is the whole rewrite as a pure function
    of the document's bytes and the plan --- the split `save::append_update` already had ---
    and `save::Rewriter` is the seam that decides where it runs. `save::stage_in_place`
    creates the staging file, opens the source, and hands both **handles** to
    `save::InWorker`, which maps the source read-only, spawns a sandboxed child with the
    staging file's descriptor on `worker::OUT_FD`, asks `Request::Rewrite` and drops it.
    The document's bytes never enter the coordinator and neither do the new file's; what
    crosses back is a length.

    **The output channel is a descriptor, and that it works had to be measured rather than
    assumed.** The profile the worker applies to itself contains `(deny file-write*)`, so
    the obvious reading is that a worker cannot write anything. Measured on macOS 26 with
    `worker::SANDBOX_PROFILE` verbatim: a write through the inherited descriptor succeeds,
    and `File::create` on any path is refused with `EPERM` --- which is the control saying
    the policy was in force, and without it the run is equally consistent with a sandbox
    that never came on. So the policy stops a worker *opening* a path for writing and does
    not stop a write through a descriptor the parent opened. That is the same asymmetry
    `DOC_FD` already rests on in the other direction. The usual explanation --- the check is
    at `open` rather than per write --- is the standard account and is not what was measured;
    the rule to act on is the pair of outcomes.

    **What it costs is one spawn, measured.** On `comments.pdf` the rewrite is 2.4 ms in the
    coordinator and 11.4 ms in a worker --- +9.0 ms, best of five interleaved --- which is the
    process start plus PDFium's initialisation and is therefore fixed rather than
    proportional to the document. On a file where the parse is the cost it disappears; this
    fixture is close to the worst case for it.

    **What the coordinator can still check, and it is exactly one thing.** It never sees
    the bytes, so it compares two numbers arrived at independently: the length the worker
    reports and the length the staged file has. A short write, a reply built for another
    request, or a second rewrite appending to the first all disagree there. Neither number
    is derived from the other, which is what makes it a check rather than a restatement.

    **Evidence.** `worker-probe` writes the same document twice --- once through
    `save::Here` and once through `save::InWorker` --- and compares them **byte for byte**:
    222,667 bytes each on `testdata/comments.pdf` under a plan that turns every page. A
    rewrite is deterministic given one document and one plan, so the two processes have no
    licence to differ, and a comparison of page counts would have passed for a worker that
    dropped the turns. Three checks beside it: both refuse a plan whose baseline is not the
    document, and the worker's refusal names the page counts, so it really parsed; pointed
    at a directory with no PDFium the worker path fails where the coordinator path still
    answers, which is what says a child was involved at all; and a worker started **without**
    an output file refuses the request in words rather than writing a document into
    whichever descriptor happens to be open at that number. In the unit suite,
    `the_coordinator_does_not_parse_the_document_it_rewrites` hands the save a source that
    is not a PDF and requires it to succeed --- red on the code this replaced.

    **What is not closed.** `save::Here` still parses in the coordinator, and it is what a
    platform with no sandbox gets --- refusing would make such a platform useless rather
    than uncontained, which is the rule `Backend::default_here` already follows, and
    `render::UNSANDBOXED_MARK` is what keeps the two runs distinguishable. Beyond that,
    **two paths remain and they are named rather than counted**: `save::write_merged`, and
    `print::build` on the page-range print route.

    ⚠ **The copy paths, Split and the working-document print job closed 2026-09-01.**
    `save::write_copy`, `save::write_split` and `save::print_bytes` take the same
    `save::Rewriter` the rewrite took, and the shape is the rewrite's: the source's
    **handle** goes in, a staging file's handle goes in, and a length comes back. The
    question this entry left open --- *handing a worker a descriptor to a file it did not
    create is a decision this entry has not made yet* --- turned out not to arise: what the
    worker is handed is the staging file `save::stage` creates beside the reader's chosen
    destination, the same file created the same way as an in-place save's, and the rename
    onto the name the reader picked happens in the coordinator. Printing is the one that
    needed an answer of its own, and it is not the output channel that had to change: the
    job's bytes come back **into** this process because `NSPrintOperation` and
    `Windows.Data.Pdf` take bytes rather than a pathname, so the worker writes into a
    scratch file this process created and this process reads it back through the handle that
    wrote it. That read is of bytes tpdf produced a moment ago; the parse of the reader's
    document is gone, and the platform's own parse afterwards is the readback this document
    describes elsewhere and wants.

    **The print refusal moved with the parse, and had to.** An encrypted document may be
    saved and may not be printed in part, and that decision used to be made in the
    coordinator between the two phases of a parse the coordinator was doing. It is now
    `save::Job` --- one value carrying both of the ways a print job differs from a save, the
    reader's view rotation and this refusal --- travelling on `Request::Rewrite` and decided
    in `save::rewrite_update`. Leaving it behind would have meant shipping a decrypted copy
    of the reader's document out of the sandbox in order to refuse it.

    ⚠ **`print::build` was a coordinator-side parse this entry never listed, and it closed
    on 2026-09-01 --- the day after it was written down.** The page-range route --- a range
    the reader typed, or any print with no document open --- called it, and it loaded the
    file with `lopdf`, walked the page tree and serialised, all here. It was the same
    exposure as the one above by a different function, and it was missed for the same reason
    the 2026-08-30 correction records: this entry has enumerated **commands**, and the
    property is one of **functions**.

    **The gate has the same blind spot, and that is the part worth keeping.**
    `scripts/check_writers.py` derives its list from the terminal writers in `save.rs`, so a
    path that parses the reader's document and *writes nothing* is invisible to it by
    construction --- a print job goes to a printer. Every instrument here was keyed on
    writing; the property is parsing. See `docs/TRAPS.md`, *A risk and a gate both keyed on
    writing cannot see the path that only reads*.

    It is closed by `crate::print::build_update`, the pure half, run through
    `worker_proto::Request::PrintRange` in the same sandboxed worker as every other rewrite,
    with `save::print_range_bytes` owning the scratch file and doing no parse. It answers
    with `Reply::Rewrote` deliberately: the fact is the same one --- N bytes down the output
    channel --- and the coordinator compares it against the staged file's own size through
    the same `save::landed_is` a rewrite uses. `worker-probe` is 37 checks now, the three new
    ones being the differential, the needs-a-worker control and the scratch cleanup.

    **No password crosses on this request**, and that is not an omission:
    `print::build_update` refuses an encrypted document whether or not the key is held,
    because `lopdf`'s full serialiser emits every object in the clear and a selection cannot
    be appended. Sending the key would buy a decrypted copy and nothing else.

    **What it costs is one spawn per operation, measured.** On `testdata/text-base14.pdf`,
    best of five interleaved and three consecutive runs: the copy is 4.0 ms here and 11.9 ms
    in a worker (+8.0), the print job 0.2 -> 7.2 (+7.0), and the rewrite 0.2 -> 7.2 (+7.1).
    `text-wide.pdf` reads +7.2, +7.0 and +6.9. All three are the same fixed cost --- a
    process start plus PDFium's initialisation --- rather than anything proportional to the
    document. `worker-probe` is 40/40 on macOS and prints all three numbers.

    **The Windows half was measured on 2026-09-01, and it took no new probe.** The mechanism
    there is a `DuplicateHandle` of the staging file into the child's table, named in argv on
    `--out-handle` --- the same route the document's section already takes, and the granted
    access travels with the handle rather than being re-checked against the low-integrity
    token. That was the expected behaviour, and expected behaviour is what this document had
    instead of a reading for as long as the sentence here said *unmeasured*. `worker-probe`
    is a step of both CI legs now rather than a run somebody remembers to make, and on run
    33501693368 it reported **34/34 checks passed, 0 not applicable to this platform** on
    `windows-2025` --- so the copy, the split, the print job and the rewrite's output channel
    are each watched working there, not described.

    ⚠ **The read and socket ceiling of the Windows boundary is unchanged by that**, and is
    residual risk 4. A measured output channel says the descriptor handover works; it says
    nothing about what a compromised worker could still reach.

    The general shape is the one this entry already records --- **a mitigation that moved
    half a path reads exactly like one that moved the path**, and the half that stayed is
    the one nobody writes down.

    ⚠ **Merge documents widens this, on purpose, and by a different axis: it parses files
    the reader chose that tpdf never opened.** Added 2026-08-24. Every other writer on this
    list reads the *open* document --- one file, already parsed by a worker, already
    rendered on screen. `save::write_merged` loads each incoming file with `lopdf` in the
    coordinator, before anything about it is known, so a merge of four documents is four
    coordinator-side parses of bytes the application has never seen. Each is bounded by the
    same `MAX_DECODE`, the graph walk in `merge.rs` uses `sweep::MAX_NESTING`, and the whole
    command is on the blocking pool --- so what it adds is exposure to more attacker-chosen
    input on the existing path, not a new kind of access.

    **Closed 2026-09-01, and the paragraph above was wrong about how.** It said the incoming
    files "could go through a worker on the way in, since what has to come back per file is a
    page count and an object graph --- which is the whole file, so it has the rewrite's
    problem after all". That reasoning had the direction backwards: nothing has to come back
    *per file*. The merge is one operation with one answer, so the files go **in** and the
    merged document comes out down the output channel the rewrite already had.

    They go in as one read-only mapping --- every incoming file concatenated, with
    `save::Incoming` naming where each begins, how long it is and what to call it --- on
    `worker::IN_FD`. One mapping rather than one per file because the descriptor shuffle
    between `fork` and `exec` may not allocate, so a descriptor per file would need a
    compile-time cap, and a cap on how many documents a reader may merge is a product limit
    invented to suit a shuffle. `Reply::Merged` carries two numbers, which is why it is a
    variant of its own rather than the `Reply::Rewrote` a page-range print reuses: the page
    count can only be taken where the merged document is.

    The coordinator's remaining part is to **read** those files, and reading is not parsing:
    `save::concatenated` copies bytes into a buffer and never asks what they mean.

    `worker-probe` is 40 checks now. Three of them are this: the coordinator and the worker
    merge a document with itself and produce byte-identical output with the same page count,
    that count is the merged document's rather than any plan's, and the merge refuses when
    there is no worker to be had.

    Decompression is bounded at
    `MAX_DECODE`, graph recursion at `sweep::MAX_NESTING`, and a panic is reported rather
    than fatal (pinned by a test, so the property cannot be lost to a profile change) --- but
    there is no deadline and no memory bound, because enforcing either needs a separate
    process. A document that makes the parser spin therefore wedges the application and takes
    the unsaved journal with it, rather than costing a replaceable worker.

    What those four needed was not a second worker but an **output channel**: the append
    moved because its answer is kilobytes and fits in a reply, and a rewrite's answer is the
    whole file. That channel exists as of 2026-08-28 and the in-place rewrite uses it, which
    is what took *deleting a page and pressing ⌘S* off this list. The copies, the split, the
    merge and the print job still reach the paragraph above.

19. **The redaction gate's coverage is a ceiling, not a threshold**, added 2026-08-27 with
    §5.1's wiring. A region can only be certified when its page leaves a word the removal did
    not take, no larger than the smallest box it did take, of at least `MIN_CONTROL_CHARS`
    characters. Measured across 41 real documents, **45.9%** of realistic regions have no such
    word, and those are reported *not verified* --- which is the safe answer and is also the
    answer that was given before any of this existed. Lowering the control's standard is one
    lever on coverage and a poor one, since the measured curve has no flat part: 71.9% at two
    characters, 58.3% at four, 35.5% at eight, and a two-character token is a fragment
    `adjudicate` would match by accident.

    **This entry said that was the only lever, and it was wrong the day it was written.** The
    other one is what the engine is shown, and it was worth more: masking the probe strip to
    the region's own columns rather than rendering the full width of the row took *shown
    unreadable* from 18 to 63 of the same 104 regions, with the control's standard untouched
    (§5.1). A limit stated as the ceiling, in the document whose subject is checks that cannot
    see what they certify, is the shape this file exists to catch.

    Two things narrow it further and neither is closed. **The gate reads the region, not the
    page**, so a `/DCTDecode` image outside every region is still `verify::Report::deferred`
    --- bytes nobody read, reported. And ~~on a platform with no engine there is no gate at
    all: Windows gets one sentence saying so, which is honest and is not a mitigation.~~
    **Closed 2026-08-29**: `ocr_windows.rs` drives `Windows.Media.Ocr` behind
    `ocr::Recogniser` and `OcrWorker::spawn` has a Windows arm, so both platforms run the
    gate. The remaining no-engine sentence is now reachable only on a platform this project
    does not target --- and the test that covered it is compiled by neither, which
    `ocr_worker.rs` says out loud rather than leaving as an apparent coverage.

20. **The Windows engine cannot be told not to correct what it read**, added 2026-08-29 with
    the engine. `ocr::Options::language_correction` is documented as off for verification
    *always*, because a corrector turns marks it cannot read into plausible words --- which is
    the wrong bias when the question is whether anything is readable at all, and it can also
    repair the control token into something else and fail the check for the wrong reason.
    macOS Vision honours it through `setUsesLanguageCorrection`. `Windows.Media.Ocr` has an
    internal language model and no switch, so on that platform the field is documentation
    rather than a setting.

    **Measured, and the measurement is support rather than proof.** `win-ocr-probe` reads a
    word and a non-word at 44 px and again at `ocr_gate::MIN_CONTROL_PX`; all four came back
    verbatim on `windows-2025`, so no correction was observed anywhere it looked. What that
    does not establish is the case the option exists for: at both sizes the engine read clean
    synthetic text *exactly*, so it was never near its limit, and a corrector only shows where
    a recogniser is struggling. What the gate hands an engine is harder in a way size does not
    capture --- a control composited beside real page ink, at the document's own contrast.

    So a *not verified* from this engine means the same as one from Vision, and a *clean* rests
    on a control the engine may in principle have reconstructed rather than read. The
    instrument that would narrow it is the corpus sweep `redact-reach-probe` already does on
    macOS, run against real documents on Windows; it has not been, as of 2026-09-01. Since
    that date the run is named in `BUILD.md`'s release checklist at **step 8**, with the flags
    it needs there --- `--no-gate` off, because the gate is the half under test --- rather than
    living only in this sentence, which nothing reads before a tag.

21. **A 358-byte document ends the process that parses it, and no guard we can write will
    stop it**, added 2026-09-01. A cross-reference stream declares the byte widths of its own
    fields in `/W`, and `lopdf` multiplies them out and asks for a zeroed buffer of the result
    without checking it: `/W [1 4 3333333333333333332]` gives `memory allocation of
    3333333333333333332 bytes failed`. **That is `handle_alloc_error`, so it is an abort and
    not a panic** --- `catch_unwind` cannot see it, and there is no point in the tpdf code
    where a check could go, because the code that would have to check is `lopdf`'s own
    cross-reference parser. The threshold is sharp: `W[2] = 2^45` completes, `2^46` aborts.

    **It is on the load path**, so every reader of the object graph reaches it: `annots::scan`,
    `links::scan`, `docinfo::scan`, `encoding::scan` and `save::rewrite_update`, verified by
    feeding one file to each. What bounds the damage is where those parses run, which since
    2026-08-28 and 2026-09-01 is a worker for all of them: a reader sees a panel that never
    fills and a save that refuses, and the pool restarts a process. That is the entry about a
    pool replacing a dead worker with the same bytes and faulting again --- correct behaviour
    with an unhelpful shape, rather than a compromise. Before those moves it would have taken
    the application down from `lib::print_job`. **The last route by which it still could ---
    `save::write_merged` --- closed later the same day**, so every `lopdf` load of the reader's
    document now happens where an abort takes a worker rather than the window. The one
    coordinator-side `lopdf` parse left is `verify::scan`, which reads a file tpdf wrote
    seconds earlier rather than the document as it arrived; a construction reaching it has to
    survive our own serialiser first.

    **`testdata/abort/xref-bomb.pdf` is the reproducer, generated like every other fixture**
    (`testdata/make_xref_bomb_pdf.py`) --- in a subdirectory of its own, because every sweep
    over `testdata/*.pdf` would otherwise load it and die, and `worker-probe` hands it to a rewrite through a real
    worker: the worker dies, the coordinator is told so in words, and the probe carries on. The
    coordinator arm is deliberately not run against it --- it would take the probe with it,
    which is the finding rather than a test.

    Nothing here is a memory-safety defect and nothing is exploitable beyond availability: the
    allocation is refused, not made. The fixes available are upstream in `lopdf`, or a
    pre-parse of the cross-reference stream's `/W` before handing the bytes over, which means
    writing a second cross-reference parser to protect the first. Neither has been done.
    Found 2026-09-01 by coverage-guided fuzzing of `lopdf` through our own entry points,
    independently by two targets.

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
