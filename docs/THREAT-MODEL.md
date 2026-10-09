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

Five principals, each trusting only what is below it in the table; the command-line tool sits beside the coordinator rather than above it.

| Principal | Authority it holds | Authority it does not |
|---|---|---|
| **Webview** (Svelte) | Draws, receives tiles, issues commands — fifteen of which write files on its behalf (§T6.1), drives the updater's optional launch check and can ask for the process to be ended and started again once an update is applied (§T9), can ask for a document web link to be opened (§T8), reads signature images explicitly selected through its file input (§T6.17), can ask for a second PDF to be opened for reading so its pages can be inserted (§T6.20), can ask for the command-line tool's link in `/usr/local/bin` to be made or removed (§T6.23) and whether it is there, which changes nothing (§T6.38), and can ask for tpdf to be made the default application for PDFs (§T6.27) | No general filesystem access, no network reach of its own and no PDF parsing. It can name an address only in two ways: a document web link the reader confirms (§T8), and the timestamp authority `sign_document` and `sign_resume` are handed --- any `http` or `https` host without credentials, loopback and private addresses included on purpose (§T10) |
| **Coordinator** (Rust, the Tauri process) | Opens files the user chose, owns the window, spawns and kills workers, owns every shared mapping; asks the OS key store to sign one digest when the reader signs a document (§T6.21); asks the timestamp authority the reader chose for a token over that signature, when they chose one, and the certificate authorities for revocation data, when they also asked for long-term data (§T10) | Parses no PDF syntax on the *viewing* path — with one exception, printing, described below; holds no private key, and parses no part of a document it signs |
| **Command-line tool** (`tpdf-cli`, the same crate) | A coordinator without a window, run by the reader's own account: opens the files named on its command line, spawns and kills the same workers, asks the OS key store to sign one digest, writes signed, filled or redacted copies, page-operation outputs or a document's text, and reads a document's password from an environment variable it is told the name of (§T6.23) | The coordinator's limits exactly --- parses no part of a document, holds no private key --- and no webview, no updater, and no network but the timestamp authority `sign --timestamp` names and the certificate authorities `--long-term` asks (§T10) |
| **Worker** (Rust + PDFium) | Parses and renders whatever bytes it is handed | No path to the document and cannot create a file, on both platforms; no filesystem and no network on **macOS** — on Windows, no writes, and reads and sockets are the disclosed ceiling |
| **Disk** | Holds the document and tpdf's output | — |

**That first row said "No filesystem" flatly until 2026-08-17, and §T6.1 had contradicted it
since 2026-08-16.** The webview holds no filesystem *plugin* permission — the granted list is
`core:default`, `dialog:allow-open`, `dialog:allow-save`, `dialog:allow-message`,
`updater:default` and `process:allow-restart` (§T9, since 26.9.17), plus
`core:window:allow-destroy`. The last permission closes the
window after the frontend has checked every open tab for unsaved edits. Tabs retain
their document handles, worker pools and passwords until closed; only the active tab
mounts a viewer. Resource limits remain per worker, not an aggregate limit across tabs.
The dialog permissions open panels and write nothing; the message
permission provides the image-only redaction confirmation. But it can issue `save_copy`,
`save_document`, `extract_pages`, `split_document`, `merge_documents`, `print_document`,
`redact_copy`, `redact_document`, `redact_raster_copy`, `ocr_copy`, `protect_copy`, `images_to_pdf`, `compress_copy`, `sign_document` and `sign_resume`, and all fifteen write a file at the
process's authority with a path the caller chose.
<!-- writers: save_copy save_document extract_pages split_document merge_documents print_document redact_copy redact_document redact_raster_copy ocr_copy protect_copy images_to_pdf compress_copy sign_document sign_resume --> So the accurate statement is that the webview cannot touch the
filesystem *itself* and can ask for fifteen specific writes; the flat version reads as the
stronger claim, and a reader who stops at this table gets the wrong answer. §T6.1 has the worked-out version and says why neither path checks its argument
against the document actually open.

**`page_import_prepare` is not a tenth, and the list is right to leave it out.** It opens a
file the reader chose, in the render service like any other document, and writes nothing; what
it adds is read authority over one more path — §T6.20. `page_import` and `page_import_cancel`,
which place the file's pages or release it, take no path at all. Its pages reach the disk only through the
writers above, which is where §T6.19 checks them.

**It said "four" until 2026-08-24, and `merge_documents` had been the fifth since 2026-08-24.**
That is the same drift this paragraph was written to record, one writer later: the count is a
number in prose and the list of commands is the thing that changes, so the count is wrong from
the moment a command is added until somebody reads this sentence again. The list is now the
claim and the number follows it — and the standing rule that a count in prose has nothing
checking it applies here as much as it does to the trap index. A new file-writing command
belongs in this list, in §T6.1, and in the coordinator-parsing entry at residual risk 18.

⚠ **And it happened again, in both halves at once, found by the release checklist on
2026-08-30.** The row said **six** while the list beneath it named **five**, so the two
disagreed with each other on adjacent lines — and the row was not the wrong one for the
reason it looked: the true count is **eight**. `split_document` had never been added to the
list, and `redact_copy` and `redact_document` were in neither. All three are named elsewhere
in this document — the two redaction writers at §T6.11, the split at §T6.9 and at residual
risk 18 — so nothing was undisclosed; what was wrong was the one place a reader goes to find
out *how many* ways the webview can cause a write, and it was wrong in the direction that
under-claims. **"The list is the claim and the number follows it" is a rule that needs
somebody to apply it**, and three commands landed without anybody doing so. The check that
would have caught it is mechanical and does not exist: enumerate the registered commands that
reach a writer, and diff that set against this list.

**"No network" was wrong in the same way and for longer.** `updater:default` is granted to
this window, and it is the *frontend* that spends it: `App.svelte` imports
`@tauri-apps/plugin-updater` and calls `check()`, which issues the one request this
application makes. The row has said "no network" since before the updater landed in `26.8.2`
and nothing moved it. What the webview does not have is network reach of its own — no
`fetch` to an arbitrary host, because the CSP is `default-src 'self'` — which is a real and
different property, and the one the row now states. §T9 is the worked-out version.
**Since 2026-09-28 the updater's check is not the one request either**: a signing the reader
asks to be timestamped makes a second, from the coordinator, to the authority they chose
(§T10). The webview names that authority as a string and gains no network reach for it --- no
capability, the CSP unchanged --- which is the same distinction again.
**And the row's last clause was false for the signing commands from the day they took an
authority** (found by the 26.9.22 release audit). It said the webview had "no way to name an
address that no document open in this process contains", while `sign_document` and
`sign_resume` accept any `http` or `https` address the webview passes: the coordinator judges
the scheme and refuses credentials (`tsa::authority`), and nothing else. A compromised webview
can therefore make the coordinator send a timestamp request --- a hash and a nonce --- to any
host, a loopback or private address included, and read nothing back beyond whether a token
that checks out came. Private and loopback addresses are allowed on purpose: a company's own
timestamp authority lives on its network. What long-term signing then fetches is bounded by
§T10's gate, not by this row.

Both corrections are the failure the release checklist's step 6 exists for: a row in a
summary table that stopped agreeing with the section beneath it, in the direction that
over-claims. Neither could go red, and neither was found by a probe.

**The worker row was wrong the same way, and it is a security claim rather than a summary of
one.** It read "No filesystem, no network" flatly until 2026-09-01. That is macOS: the profile
denies reads, writes and socket binds, and §T4 measures it. Windows is a job object plus a
low-integrity token — it denies writes and it denies reaching into the app process, and it
denies neither reads nor sockets. Residual risk 4 has carried the read half since 2026-08-02;
nothing carried the socket half until §T4 gained it on 2026-09-01, and that half is still a
reading of `sandbox_win` rather than a measurement. What holds on both platforms is the rest of
the row: the worker is never handed a path and cannot create a file, which is why the document
and the output arrive as descriptors, and is what makes the Windows read ceiling narrower than
it sounds. Found by an external review reading this row against §6 — the third row in a
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

- `save_document` → `save::append_bytes` or `save::stage_in_place` (`commands/save.rs`), on every save
  over the open file.
- `save_copy` → `save::write_copy`, on every Save a copy.
- `extract_pages` → `save::write_copy` again, on every extraction.
- `merge_documents` → `save::write_merged`, on every merge — and this one parses **more**
  than the open document: every file going in is loaded with `lopdf` here, so a merge of four
  documents is four parses of bytes nothing has rendered. Residual risk 18 carries that.

**The first of those moved the same day.** A save that only *adds* marks — the ordinary
"keep my highlights" — is prepared by `save::append_update`, which is a pure function of the
document's bytes and the plan: it opens nothing, names no path, and knows none exists. It runs
as `Request::Append` in the worker that already holds the document, under the same sandbox,
deadline, resource limits and restart as every render, in the process that has already parsed
that document with `lopdf` for its comments, links and properties. What crosses back is an
update section and two numbers.

The split is where the authority is, not where the code is convenient. `save::append_ready`
stays in the coordinator and asks only questions about a *path* — has this file changed since
it was opened, how long is it — which need filesystem authority and no parser.
`save::appended` then refuses an update built against a different number of bytes than the
caller measured, which is a check that did not exist and could not: the two lengths were one
number by construction while one function did both halves.

`Plan::opened_as` is `#[serde(skip)]`, so the fingerprint cannot cross in either direction —
and the compiler is what enforces that rather than the attribute alone, since `Fingerprint`
implements neither `Serialize` nor `Deserialize`. `Request`'s standing property holds: it names
nothing the worker could act on.

⚠ **Only half of it moved that day, and the other half moved on 2026-08-26.** Preparing the
update is one parse; *verifying* what was written is another, and `save::append_in_place`
re-read the whole file and parsed it here. It is `save::Reread` now — a seam taking the
written file's handle, its length and the password — and `save::InWorker` maps that handle
read-only into a sandboxed child, asks `Request::Reread` and drops it. So the append is out of
the coordinator in both directions, and it gained the deadline and the memory bound this
section says need a process. Residual risk 18 has the full account, including what stayed.

Evidence, external to our own account of it: `worker-probe` builds an update section through a
real contained worker and appends it to the fixture, then re-parses the result — **865 bytes
on a 775-page document, re-read as 775 pages**, with the length it was built against compared
against the file's own (macOS, 2026-08-22, 17/17). Four more checks since 2026-08-26 put the
same worker on the *verification* side: it and the coordinator are asked the identical question
about identical bytes and have to agree in both directions, the refusal has to be `lopdf`'s
rather than PDFium's at open — which the first draft of that check got wrong while reading as
a pass — and a fourth asks for something only the worker path needs, since two readers
agreeing says nothing about whether a worker was involved at all (23/23).

**The rewrite moved on 2026-08-28 and the copy paths, Split and the working-document print job
on 2026-09-01**, all through `save::Rewriter` and an output channel that is a descriptor rather
than a reply — residual risk 18 has the mechanism and what it costs. What the memory
measurement of 2026-08-22 decided was not *whether* a rewrite could move but how large a
document may be **appended to** inside a worker: a worker holding the 337 MB scan reaches
1029.8 MB of footprint after answering an append, 667 MB of which the append added, against a
1024 MB Windows commit cap. `save::APPEND_MAX_BYTES` is that bound. `docs/PLAN.md` §3 has the
table, the three designs the measurement re-ranks, and the one open question it leaves.

**That cap applies to the append this section is about**, which is worth saying plainly rather
than leaving in the plan. On Windows the document's mapping is file-backed and not commit, so
the number to compare is the 667 rather than the 1029.8, and that leaves a margin — by
reasoning, not by measurement. Nobody has run `worker-probe` against a large document on
Windows. If the cap is reached the worker is killed and the save is refused, which is
containment behaving as designed and a save the reader cannot complete; it is not data loss,
since nothing has been written at that point.

**The merge followed on 2026-09-01, and with it every writing path is out.** It was the
widest of them — the only operation that parses documents tpdf never opened — and it moved
on the same seam through `worker_proto::Request::Merge`, with the incoming files handed over
as a second read-only mapping (`worker::IN_FD`). What the coordinator does now is *read* those
files: it copies their bytes into the mapping and never asks what they mean.

⚠ **One `lopdf` parse remained in the coordinator after that, and it was a reader rather
than a writer: `verify::scan`.** The redaction verification re-reads the file that was just
written and parsed it here, on the blocking pool. Its bytes derive from the reader's document,
so it was the same exposure the writers had. **It was missed for exactly the reason
`print::build` was** — this section, residual risk 18 and `scripts/check_writers.py` are all
keyed on *writing*, and a verification writes nothing. `docs/TRAPS.md` has that under *A risk
and a gate both keyed on writing cannot see the path that only reads*.

**Closed 2026-09-01, on the seam the read-back already had.** `save::Verifier` is the third
member of `save::Outside`, beside `Reread` and `Rewriter` and for the same reason; the
coordinator opens the file it wrote, hands the **handle** and its length to `save::InWorker`,
which maps it read-only, spawns a sandboxed child, asks `worker_proto::Request::Verify` and
drops it. What crosses back is a `verify::Report` — a set of needles found and two lists of
reasons — and never a byte of the document.

**So on both shipped platforms no `lopdf` parse of a document happens in the coordinator at
all.** That is a stronger statement than this section has been able to make before, and it is
worth stating with its exception: `save::Here` still parses here, it is what a platform with
no sandbox gets, and `render::UNSANDBOXED_MARK` is what keeps such a run distinguishable.

Two properties of the move are not guessable from the feature. The scan needs the reader's
**password**, because a redacted copy of an encrypted document is re-encrypted and a worker
without the key parses no objects and finds nothing — an absence that reads exactly like a
clean file; `verify::scan` refuses to certify that, which makes the failure safe, and
`Request::Unlock` before the ask is what makes it answerable. And a report is now a **reply**,
read under `MAX_REPLY_BYTES`, so `verify::MAX_OBJECT_REASONS` bounds its per-object lists at a
thousand and counts the rest in one further line — otherwise a file with a few hundred
thousand undecodable objects produces a report that will not fit down the pipe, and the reader
meets a verification that *failed* rather than a file with a great deal wrong with it. The
verdict is unchanged by the shortening; only the enumeration is.

**The other reply the same bound has to hold is the one whose size the caller chooses.** Since
26.9.2 a search asks about a run of pages rather than one, so the answer is the sum of several
pages' hits and the number of pages is picked by the frontend — which is the quantity that
does not predict the size. `render::run_search_range` therefore stops when its answers reach
3 MB, a tenth of `MAX_REPLY_BYTES`, and hands back a **prefix** of what was asked for; the
caller reads how many pages came back rather than assuming the run completed, because a walk
that advanced by the number it asked for would skip the pages the budget cut off and report no
hits on them.

What follows describes the exposure those parses had while they were here, and is kept because
`save::Here` still has it.

That parse ran under `tauri::async_runtime::spawn_blocking`, and it is worth being exact
about what that does and does not buy, because the name invites the wrong reading: it moves
the work off the async runtime's threads. It does not move it out of the process holding the
window, the edit journal and the user's filesystem authority. "Off the async thread" is not
"out of the trusted process", and the two were being treated as the same thing.

Four things bound how much that is worth:

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
*prototype* worker, not the shipped one — `latency-bench` puts the **production** `Worker`
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
other request kinds have. Only a tile can be withdrawn — `Withdraw` names a tile's
request id and nothing else — so **every other request kind** holds a service thread until
it answers, and there are `pool + 2` of those *shared across every open document*. That read
"`Text`, `Search`, `Outline` and `Open`" until 2026-09-06, which was the whole set when it was
written and is now four of the twenty-one `worker_proto::Request` variants; the property is the
absence of a withdrawal, not the list, and the list is what went stale — so one page that never finished parsing stopped the viewer
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

**The plan is an input too, and until 2026-09-02 nothing in this section said so.** Every
input named above is document-shaped — a stream, a page, an object graph — and a worker serving
a save receives a second one: a `Plan`, carrying page sizes, crops, quads and stroke points as
`f32` and `f64`. It is not the reader's typing. It crosses the worker boundary, it is restored
from a session, and `fuzz_targets/save_rewrite_update.rs` exists precisely because a plan can
reach `rewrite_update` that was computed against a different revision of the file. The guards
for its geometry were in `edits.rs`, where the *command* receives it, which is the app process.

What that cost, measured: a **2,937-byte** input reached **6.2 GB of allocation in a single
pass** through `save::rewrite_update`. `draw_wave` steps a squiggle across a quad one
half-period at a time, so its trip count is `width / half` with `half` derived from the quad's
*height* — a ratio of two numbers each of which is unremarkable alone. 200,049 segments, at
about thirty bytes of `String` each. A guard already sat immediately above that loop
(`half <= 0.0 || high <= low`) and could not have helped: it asks whether the arithmetic yields
a stroke at all, and every one of those segments was arithmetically fine. Two bounds exist now
— `MAX_PAGE_POINTS` refuses a made page outside PDF 1.7's own 14,400-point limit, and
`MAX_WAVE_SEGMENTS` widens the period rather than stepping at whatever the aspect ratio asks
for. Either alone leaves a live path: for an unturned page `from_device` carries the quad's own
width and height straight through, so the extreme ratio is reachable on a perfectly legal page.

**The audit behind that pair is narrow, and saying so is the point of putting it here.** Every
drawing routine in `save/marks.rs` was read for loops: `draw_wash`, `draw_line`, `draw_outline`,
`draw_ellipse`, `draw_text`, `draw_stamp` and `draw_path` all iterate a *collection* — quads,
strokes, lines — whose length the plan's own byte size bounds, and `draw_wave` was the only one
computing a trip count from geometry. That is a statement about the drawing routines on
2026-09-02 and about nothing else. The rest of the save path was not swept for the same shape,
which is why this is a residual rather than a closed item: the class is *a quantity derived
from plan geometry*, not *the squiggle loop*.

**Residual.** One pathological page still occupies its process's single PDFium thread and
starves every other render there. Note this is our own threading choice, forced by the
fact that concurrent PDFium calls crash — `pdfium-render`'s `thread_safe` feature does
not serialize them, whatever its README says (AGENTS.md). It no longer does so
indefinitely: one deadline is the bound, since a request killed for exceeding it is the one
death `Workers::with_worker` does **not** retry — retrying would spend a second deadline of
a service thread to learn what the first established. The deadline is still a coarse
instrument, in that the process dies and every partial render goes with it.

Memory is the larger residual and it is unbounded, not merely coarsely bounded: neither the
kernel's limit (refused) nor ours (unwired) applies to a worker on macOS today. Bounding the
*inputs* — decompressed stream size, tile dimensions, pages per request — is the layer that
would catch a sub-interval burst even with the poll running. This read "only the tile bound
exists" until 2026-09-02; there are **four** as of 2026-09-06, and the two added on 2026-09-02
(`MAX_PAGE_POINTS`, `MAX_WAVE_SEGMENTS`) bound a **plan's** geometry rather than a document's —
an input this section had not counted at all. **The fourth is `save::MAX_MERGE_BYTES`
(1 GiB), and it corrects the sentence that stood here**: the tile bound is no longer the only
one refused before a worker is asked. A merge's total is checked against the incoming files'
own handles at `save.rs:1063`, before the mapping is created and before a byte is read, so the
reader meets a refusal naming the limit rather than an allocation failure — the tile bound's
own shape (`protocol.rs`), arriving on the one input the coordinator holds all of at once.

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
`_KILL_ON_JOB_CLOSE` and `_DIE_ON_UNHANDLED_EXCEPTION`, and an integrity level — read the
file, there is no network call in it. The Windows mechanism that gates network *capability*
is AppContainer, which this is not. **Nothing here has measured a socket bind from a
contained Windows worker**, so this is a ceiling read off the code rather than a result, and
the honest statement is that the network is not denied there rather than that it is reachable.
The measurement is one rung on `examples/win_sandbox_probe.rs`, which already re-execs a
contained child and compares its work against an uncontained control; a bind of a TCP and a
UDP socket in that child is the Windows twin of `worker-bench --mode authority`.

Until 2026-09-01 the evidence above was the whole of this section, so a macOS result read as
covering both platforms — the same shape as the README sentence corrected the same day, and
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

**Since 2026-10-05 that holds for every redaction.** The rewrite sweeps whenever the plan
carries a region. Before, it swept only when an annotation, an outline entry, a field or a
picture went, so a text or drawing redaction on a page whose `/Contents` is several streams
left the old streams in the file, unreachable, with the removed glyphs in them, and the needle
scan answered verified when no single string held the whole needle. A page's content is now
replaced by `redact::replace_page_content`, which reads the page back and refuses unless it
holds exactly what was written; `/Contents` as a reference to an array is rewritten like any
other. A content stream that does not parse to its end is refused by every removal, so a
partial parse is never written back. The needle scan also compares each needle with every
string's decoded text (UTF-16BE or PDFDocEncoding), so a note or a bookmark quoting a removed
line in either encoding is found.

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
a path; it writes nothing. The write is `save_copy`, and its authority is the process's —
which is to say the reader's, since nothing here is sandboxed on the app side. So the honest
statement is: **a caller able to reach `save_copy` can write a PDF anywhere the reader can
write**, without a panel and without a prompt — and the *source* path is the frontend's
too, so the same caller can read any PDF the reader can read. Neither end is checked against
the document the render service actually opened, which it could be. It is not, because
`print_document` has had exactly the same shape since 2026-07-28 and tightening one of the
two would leave a consistent surface looking inconsistent; if this is closed, close both.

**`page_import` is not a write, added 2026-09-19.** It reaches the render service's open, not
a writer, and is recorded at §T6.20 rather than here; §3 says why it is not in the list. The
open moved to `page_import_prepare` the same day, when the reader could choose which pages;
it is still a read, and still not in the list.

**`extract_pages` is the same verb with a selection, added 2026-08-17**, and it is recorded
here rather than given a section because it adds no authority: same write path, same
caller-supplied source and destination, same absence of a check against the open document.
The only thing it adds is a `slots` argument, which `plan_subset` refuses when it is empty,
out of range, repeated or descending — so the worst a bad selection produces is a refusal,
not a wider write. **The count of commands that write a file is three now**, not two: the
boundary table's §3 row says "two", and it is corrected in the same commit; a number in a
summary row is exactly the thing that stops agreeing with the section beneath it. (It is
**four** as of 2026-08-19, when `save_document` landed — see §T6.7. The sentence is left as
it was written rather than silently re-pointed, because what it is about is a count in a
summary going stale, and re-pointing it every time would erase its own evidence.)

**The current list lives in §3 and is the authority; do not count from this section.** It is
**nine** as of 2026-09-07 with `redact_raster_copy`, **ten** as of 2026-09-26 with
`sign_document` (§T6.21), and **eleven** as of 2026-09-28 with `sign_resume`, which writes the
signature `sign_document` made and held when its timestamp did not come, to the path that call
named (§T10), and **twelve** as of 2026-10-03 with `ocr_copy` (§T6.28), and **thirteen** the same day with `protect_copy` (§T6.30), and **fourteen** with `images_to_pdf` (§T6.31), and **fifteen** with `compress_copy` (§T6.32). It reached eight on 2026-08-30 without anybody adding three of them here or
there: `split_document`, `redact_copy` and `redact_document` were each disclosed in their own
entries and absent from the one place that answers *how many*. That is this paragraph's own
subject arriving a third time, which is the argument for the mechanical check §3 now names —
enumerate the registered commands reaching a writer and diff the set — rather than for
another sentence telling the next person to remember.

`redact_raster_copy` keeps the source and creates a fresh image-only PDF in a
sandboxed rewriting worker. A digest-checked anonymous snapshot binds the marked
regions to the opened bytes. Every page is rendered at 300 dpi, and marked pixels
are replaced before lossless image encoding. Only generated page objects, drawing
commands and masked RGB strips enter the output; source text, annotations, links,
metadata and revision history do not. Encryption is preserved. Before the staged
file replaces its destination, the worker checks the serialized object inventory
and image digests with lopdf and renders the masked regions back with PDFium.
The result proves removal within the marked regions, not absence of the same
information elsewhere in the document. Failures leave the destination untouched.
The parent bounds input snapshots to 512 MiB and ends the worker after 180 seconds;
the worker additionally bounds page pixels, total pixels, encoded bytes and elapsed
time. Unsupported unsandboxed platforms refuse this operation.

**What bounds that is the same thing that bounds `spike_exit`, and no more.** The CSP is
`default-src 'self'` with no `'unsafe-inline'`, so the only script that runs is the one that
shipped — residual risk 7, and the T8 invariant that keeps document text from becoming
script. The marginal authority over what was already reachable is real but small: a caller
that can reach `save_copy` can already reach `open_document` and the print path. It is
recorded here rather than left implicit because it is the first *write*, and a write is a
different kind of verb from the ones this surface had before.

**Three refusals, and each is a correctness property rather than a security one:**

- An **encrypted** source keeps its encryption, and one that nobody unlocked is refused.
  `lopdf` drops `/Encrypt` on save without a word, so a copy of a restricted document would
  come out unrestricted and look identical — exactly the T5 shape, a false assurance,
  pointed at the document's own protection rather than at ours. Until 2026-08-28 the answer
  to that was to refuse every encrypted source; since then `save::rewrite` puts the file's
  own state back with `Document::encrypt` as its last step, so the copy is as restricted as
  the original. The refusal that remains is the one no key can satisfy: a document still
  locked parses to nothing, and is declined with a message naming the lock.
- A **page count that disagrees with the model** is refused, which is the only part of §5's
  external-modification story that exists yet.
- **Writing over the source** is refused, compared by canonical path so that two spellings
  of one file are one file.

**The write is atomic** — sibling temporary file, rename — so an interrupted save leaves
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
about *tpdf's own leavings* — the objects this rewrite made unreachable — and not about
the document's: an orphan the source arrived with is still carried forward. Extract pages and
Split go through this same `rewrite`, which is why the distinction matters more than it
sounds: their names state an exclusion the file has to honour. Residual risks 15 and 16.

**And since 2026-10-02 a rewrite that dropped a page also unlinks what no remaining page
uses** (`unused::prune`, then a second sweep). The sweep alone was not enough for a document
whose pages all name one resource dictionary: a dropped page's pictures and fonts stayed
reachable through a page that was kept, so two pages extracted from five still held the other
three pages' pictures. An `/XObject` or `/Font` entry goes when no remaining page's content
mentions its name. The pass leaves a group of pages untouched when it cannot know what they
use: content that does not decode completely under the text editor's strict decoder, a
dictionary something other than those pages reaches, or a form, Type 3 font, pattern or
annotation appearance that carries no `/Resources` of its own. In those cases the file is as
it was before this pass, so the exclusion is then the sweep's and no stronger. Other resource
categories (`/ExtGState`, `/ColorSpace`, `/Pattern`, `/Shading`, `/Properties`) are not
pruned.

#### T6.2 — Deleting and moving a page, added 2026-08-17

**Nothing new crosses the boundary.** `page_delete` and `page_move` each take a document
handle and one or two page identities and mutate a `HashMap` in the app process; they open
no file, write none, and reach no worker. Their authority is the same as `page_rotate`'s,
which is to say the ability
to make the reader's *unsaved* document differ from the file on disk — reversible with
undo, and never written until the reader names a file. The commands that write are still
`save_copy` and `print_document`, and their authority is unchanged and stated above.

**One thing did change on the write side, and it is worth stating precisely rather than as a
narrowing.** `print_document` takes the open document's handle now, and the *edits* in a job
— which pages the reader kept and how each is turned — are read from the model rather
than accepted from the frontend. The explicit page range is unchanged and still comes from
the caller; it is what a print panel's "pages 2 to 4" will be, and it carries no edits. So a
caller can still name any readable path and any range of its pages — the §T6.1 shape,
unchanged — and cannot invent an edit the model does not hold.

**What a deletion does to the parsing surface.** A page dropped from a saved copy is dropped
by the same page-tree pass the print path has used since 2026-07-28 (`pagetree::drop_pages`),
which walks the object graph under `sweep::MAX_NESTING` and refuses rather than stopping
early — a partial pass would leave a page tree naming an object that is gone, which is a
document that opens and prints blank pages. One refusal is a correctness property in the
§T6.1 sense: a page two page numbers share cannot be half-deleted, because removing it means
removing one entry from a `/Kids` array rather than one object, and a pass that removed
neither would hand back a copy with the page the reader deleted still in it.

**What a reorder does to it.** A moved page cannot be written in place — the four
inheritable page attributes belong to the tree node a page hangs under, not to the page —
so `pagetree::reorder_pages` writes those attributes onto each page and rebuilds the tree one
level deep. It runs **only** when the reader's order differs from the file's, which is a
correctness property rather than a saving: a rebuild reparents every page of every document,
and doing that to one nobody rearranged is a rewrite with no request behind it. The
abandoned tree nodes stay in the file as unreachable objects, exactly as a deleted page's
content does, and for the same stated reason — a saved copy is a serialisation, not a
sanitation (§T6.1, residual risk 16).

**A pending redaction reaches no writer, and that is carried by the type rather than by a
filter** (2026-08-26). Marking a region puts a `Redaction` in a table of its own with an id
space of its own, and `Plan::marks` is built from `EditState::marks` — a list a redaction
cannot be in. So the failure this arrangement exists to prevent, tpdf writing a reader's
*pending* redactions into a saved file as annotations, is unexpressible rather than guarded
against: an outline drawn over words that are still there, in a document that has been handed
on, is a confident lie of exactly the kind §6 of `docs/PLAN.md` opens by refusing. The
alternative design — one mark kind with an exclusion in `save.rs` — would have been a rule
to remember on the day the next kind is added. Two tests pin it: the plan of a document with a
redaction equals the plan of the same document without one, and the reply carries it while the
plan does not.

**Dragging a thumbnail adds nothing here**, checked rather than assumed when it landed on
2026-08-17. It registers no command, takes no capability and reaches no new sink: the gesture
ends in `page_move`, which is the command above. What it does add to the webview is pointer
listeners and a `setPointerCapture` on the strip's own panel, neither of which parses markup
or builds a URL-bearing element — the `sinks` gate is what says so mechanically, and §T8 is
where that invariant lives.

**The outline of a copy that lost pages is repaired, since 2026-10-07, and the claim is
bounded by what the repair refuses.** Until then it was dropped whole, which was a smaller
claim: what survives a repair is only as sound as the resolver that did it, and what
survived the drop was nothing. `outline_repair.rs` now removes the entries that lead to a
deleted page and keeps the rest, and three things keep that from being a larger claim than
it can carry. **No title of a deleted page survives**: an entry whose page went is removed
even when entries under it stay, and those move up — the first version kept it as a
heading, which would have put the name of a chapter that was left out into an extract.
**Anything the resolver cannot vouch for takes the old path**: an outline that is not a
tree, an entry naming its page by number, a name the bounded walk gives up on. And **the
removed entries leave the file**, not only the chain, so their titles are not in the copy
as unreachable objects. What is not claimed: a heading with no destination of its own is
kept while anything is left under it, and its title is whatever the author wrote there.
Deleting a page is not redaction, and a title that must go is a job for *Redact*.

#### T6.3 — Highlighting a selection, added 2026-08-18

**The first thing tpdf adds to a document rather than rearranging, and it adds no
authority.** `annot_highlight` and `annot_remove` take a document handle, a page identity and
a list of numbers, and mutate a `HashMap` in the app process. They open no file, write none
and reach no worker — the T6.2 shape exactly, and the commands that write are still
`save_copy`, `extract_pages` and `print_document`.

**Two things the frontend cannot say, and both are deliberate.**

- **The timestamp.** `edits::NewMark` has no field for it; `commands/edit.rs` reads the
  clock when the command arrives. What a mark claims about when it was made is the application's statement,
  and a `made` on the wire would be one more attacker-chosen string in a file tpdf signs its
  name to.
- **The subtype.** `MarkKind` has one variant and `save.rs` maps it with a `match`, so the
  `/Subtype` written is a literal of ours. A document cannot choose it, and neither can the
  frontend — the same property `annots.rs` keeps on the way *in*, where `Kind` is an enum of
  our own literals rather than the document's `/Subtype` string.

  ⚠ **The second half of that stopped being true later the same day** (§T6.5): the frontend
  now names the kind, because a reader chooses between several. What it has *not* stopped
  being is the property that matters — read the amendment rather than this bullet, which
  names the current set rather than counting it.

**A mark's note is attacker-controlled the moment a saved file is reopened**, which is the
one genuinely new surface. The reader types it, tpdf writes it, and `annots.rs` reads it back
out of a file that may by then have been edited by anything — so it is treated exactly as a
comment body already is: it reaches the DOM as text, it may carry no URL, and §T8's invariant
is what makes that checkable. `edits::MarkView` says so at its declaration and the `sinks`
gate is what enforces it mechanically. Today the note is always empty, because nothing types
one; the field exists because the write path needs it and the reading path already has it.

**Something types one as of 2026-08-18** (§T6.4), and the paragraph above is what it was
written against — so the surface is the one already described rather than a new one. The
box a reader types in is a `<textarea>`, whose `value` is text by construction and parses no
markup. The route by which it becomes *somebody else's* string is unchanged: it goes into
`/Contents`, and comes back through `annots.rs` into the comment panel and the comment
popup, which have treated a body that way since they were written.

**A second display route landed on 2026-08-20 and this said there was none.** The sentence
here read *"the note is displayed nowhere else while the document is open"*, which the marks
panel made false: `marklist.ts` puts every mark's note on screen, from `edits::MarkView`
rather than from `annots.rs`. The **mitigation is unchanged** — the row's text is assigned
through `textContent` and nothing else, and the `sinks` gate scans the whole frontend, so it
covered the new file the day it appeared without anyone adding it to a list. What was wrong
was the scope claim, and the cost of leaving it would have been an auditor asking *"where
does a mark's note reach the DOM?"*, reading two file names, and missing a third.

**And the model's notes are this session's, which is narrower than the paragraph above
allows.** `Edits::open` builds `Doc::open(pages)` — a fresh model with no marks — so no
`MarkView` ever carries bytes read back out of a file; a reopened document's annotations
arrive as *comments*, through `annots.rs`, into the panel that has always treated them as
attacker-chosen. `MarkView::note`'s own doc comment claims the stronger thing, and it is
left claiming it: a string that is handled as data either way costs nothing to over-declare,
and the narrower reading is one feature away from being wrong — restoring an edit journal
across an open would make it so without touching a line of this file.

#### T6.8 — What a document says about itself, added 2026-08-21

> Everything from here to T6.5 is about **reading**, not saving, and it accumulated under
> T6.3's heading — *Highlighting a selection* — one route at a time until the block was longer
> than the section holding it. The heading is added rather than the block moved, because moving
> a hundred and fifty lines to fix a filing error is the larger risk; the parent number stays
> wrong for the same reason. `AGENTS.md` cited **§T6.4** for the certificate bounds, which is a
> different subsection about marks, and now cites this one.

**A third display route landed on 2026-08-21, and it is the widest one yet.** The properties
dialog puts a document's `/Info` strings on screen — `/Title`, `/Author`, `/Producer`, and
any custom key the document invented — together with a signature's stated name, reason and
location. Every one of those is a string a stranger wrote, and the custom keys mean the
*label* is attacker-chosen too, which no previous route had: a comment's fields are named by
us, and here `properties.fields[n].name` is whatever the document put in its dictionary.

**The mitigation is the same one and needed no new mechanism.** `propertiesdialog.ts` assigns
every name and every value through `textContent`, creates no URL-bearing element, and sets no
attribute from a document string — so the `sinks` gate covered the file the day it appeared,
exactly as it covered `marklist.ts`. What is new is worth naming rather than leaving implicit:
`docinfo::Properties` has **no field that could carry a URL or an action**, in the way
`outline::Target` deliberately has none, so there is nothing for the frontend to be tempted
by. `no_signature_field_may_carry_a_verdict` matches `Signature` exhaustively for a related
reason — adding a field there is a compile error rather than a review question.

**One honest limit, and it is the same seam residual risk 7 names.** The values are bounded
in *length* (`MAX_VALUE_CHARS`) and in *count* (`MAX_FIELDS`), and both bounds are reported
rather than silent — but nothing constrains what a value *says*. A `/Producer` reading
"This document is valid and verified" is shown as written, because it is what the document
claims and hiding it would be its own lie; what is prevented is tpdf appearing to agree, and
`properties.test.ts` asserts that against exactly that input.

**A fourth route, and this one changed what parses hostile bytes rather than what displays
them: tpdf reads certificates as of 2026-08-21.** A signature's `/Contents` is a DER blob the
document chose, and `certificate::parse_certificate` hands it to `cms` and `x509-cert`. So
there is a second ASN.1 parser in the trust boundary beside PDFium's, on input just as
attacker-controlled, and three things bound it rather than one.

- **It runs in the worker, not in the app process.** `docinfo::scan` is reached through
  `Request::Properties`, so the new parser sits behind the same sandbox as everything else that
  reads a document. This is the property T1 exists for and it needed no new mechanism, which is
  the argument for having built the boundary before it was needed rather than after.
- **The blob is bounded before the parser sees it.** `MAX_SIG_BLOB` is 1 MiB against a real
  blob of tens of kilobytes, and exceeding it is *reported* through `Limits::certificates_unread`
  rather than passed off as a document with no certificate. The bound has a test that can fail,
  which took two attempts — see the trap; the first version could not distinguish refusing a
  blob from parsing one and failing.

  **This sentence was true of two parsers out of three until 2026-08-24.** `ber.rs` walks the
  same attacker-chosen bytes as `cms` and `der` and ran *before* the bound, so a 200 MB
  `/Contents` was measured, re-measured once per constructed level and copied into an
  allocation its own size, and only the result was compared against `MAX_SIG_BLOB`. It is a
  parser like the other two and is now bounded like them, on its **input**, at twice the
  bound — the factor is what makes the check refuse nothing the output check would have
  accepted, since definite-length rewriting can shrink a value by at most half. The guard has
  no outcome a test can see, for exactly that reason; what its test pins is the factor.
- **Both crates are `no_std`-shaped pure-Rust decoders returning `Result`.** No `unsafe`, no
  allocation driven by a declared length the input chose, and every failure path here maps to
  `None` plus a counted limit. Nine packages, all `Apache-2.0 OR MIT` bar `flagset` which is
  `Apache-2.0`, swept over the whole tree rather than read off a README.

**Reaching a signature is bounded too, and it was not until 2026-08-24.**
`docinfo::read_signatures` walks the form's field tree through `fields::walk`, and it
bounded the *depth* of that walk and the number of signatures it would report — neither of which stops **fan-out**. A
group node carries no `/FT`, so it emits no signature and `MAX_SIGNATURES` never fires; a node
whose `/Kids` names itself sixty-four times therefore costs 64^8 pops inside a depth bound of
eight, on a file of a few kilobytes. `MAX_FIELD_NODES` (4,096) bounds the pops themselves —
it is the `nodes` field of the `fields::Bounds` this caller passes, and `redact::covered_fields`
passes 20,000 through the same loop, deliberately: a field a redaction does not reach is a value
left in a redacted document, where one the properties panel does not reach is a line missing from
a list. That is the shape `links.rs`'s `MAX_TREE_NODES` had already taken for its own tree walk.
Hitting it is reported through `Limits::signatures_dropped` — the same counter the signature
bound reports through, because to a reader they are one event: this scan stopped looking, and
what it says about signatures is incomplete.

**And the honest limit, which is the part a reader would get wrong.** Parsing a certificate is
not verifying one. tpdf builds no chain, holds no trust store and consults no revocation list
— so a document can name itself anything and tpdf will show it. Since 2026-09-26 it does check
the signature against the bytes it covers (below, *checking a signature*), and that check says
whether the certificate's key made the signature, never whose key it is. What the certificate buys is a second, differently-sourced
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
satisfiable only by leaking them — an allocation an attacker sizes and chooses the count of,
one per extension per signature, in the process the sandbox exists to contain. The bound that
is actually correct is `for<'a> Decode<'a>`, which the three owned types satisfy and which
borrows for the length of the call. Nothing would have gone red: a leak is not a crash, the
gates were 16/16 with the leaking version in the tree, and clippy has no lint for it. The trap
of that name carries it.

A malformed extension is **counted**, not read as an absent one, because those are opposite
claims: an absent key usage places no limit on the key, and a malformed one places an unknown
limit. Absent is the reassuring branch, which is the direction a silent failure would fall.
And what an extension states is still the issuer's word — the constraint binds the key, and
only a chain to a trusted issuer makes it mean anything, so `NOT_CHECKED` now says that too.

**Timestamps, same day, and they add no parser.** An RFC 3161 token is itself a CMS
`SignedData`, so reading one exercises the crates already described here on bytes already
bounded by `MAX_SIG_BLOB` — the token sits *inside* the signature blob. The only new decoding
is `TSTInfo`'s, and it is deliberately positional: four opaque values skipped, the fifth
required to parse as a `GeneralizedTime`. That last requirement is the bound. A structure
malformed enough to shift the fields yields **no** time rather than a time read out of the wrong
field, which matters because the output is attributed to an authority — a plausible wrong
instant presented as a third party's attestation is worse than silence, and it is what a
positional walk with no type check would produce.

Two refusals beside it, each with a test that reaches it: an attribute carrying more than one
value is refused rather than guessed at, and a CMS whose `eContentType` is not `id-ct-TSTInfo`
is not read as a timestamp however well-formed its content is.

**That limitation is closed, and closing it added a parser of our own.** A `/Contents` blob
encoded in **BER with indefinite lengths** was refused outright by `der`, so tpdf read no
certificate and no timestamp from it — one of ten real signed documents to hand, and the class
affected is CAdES, which is where timestamping is routine. `ber::to_definite_length` walks the
blob and hands the crates a definite-length value.

It is roughly 150 lines rather than a dependency, and it is the only code here that reads
attacker-chosen bytes without a third party between us and them, so its bounds are the point:

- **Nesting is capped at `MAX_DEPTH` (64)** against a real signature's twenty-five, and there is
  exactly **one** copy of that bound. `emit` runs only after `measure` walked the same bytes and
  returned, so it carries no second guard — two copies would each refuse the blob alone, and a
  mutation of either would have survived.
- **A length field is capped at `MAX_LENGTH_BYTES` (4)** and a tag at `MAX_TAG_BYTES` (5). X.690
  reserves `0xff` as a length-of-length and this refuses it by the same rule.
- **Every read goes through `get`, never an index.** The two offsets built from a length the
  document chose — past a header, past a value — go through `checked_add`; the rest are a
  cursor plus at most five, and a cursor never exceeds the slice's length, so they cannot wrap.
  A value claiming more bytes than it has, a child overrunning the length its parent declared,
  and an indefinite value that never terminates are each refused rather than trusted, each with
  a test whose input reaches only that rule.
- **Output growth is bounded by input.** An indefinite header plus its marker is four bytes and
  a definite one is at most six, so the rewrite can grow a blob by at most half, and
  `MAX_SIG_BLOB` still bounds what reaches the parsers.
- **It refuses rather than repairs.** DER constrains more than the length form — `SET OF`
  ordering, a canonical `BOOLEAN`, primitive strings — and none of that is touched. A blob
  violating one is refused by the parser after it and counted as unread, which is the same
  outcome as before for every case this does not fix.

The walk is also what decides where the blob **ends**, replacing a scan for the last non-zero
byte. That is a security-relevant change as much as a correctness one: the old rule handed the
parser however many padding bytes preceded the last non-zero one, and the new one hands it
exactly one value. A blob that will not walk is counted through `certificates_unread` — by its
own mechanism, with its own test, because it and the parser's counter can produce the same
number and one input reaches only one of them.

**Checking a signature, 2026-09-26: the first cryptography run on attacker-supplied bytes.**
`integrity::check` recomputes the digest over the `/ByteRange`, compares it with the signed
`messageDigest`, and verifies the signature over the signed attributes under the signer's
public key --- RSA PKCS#1 v1.5 and PSS through `rsa`, ECDSA over P-256 and P-384 through
`p256`/`p384`. Every input is the document's: the range, the blob, the certificate, the key,
the signature value. What bounds it:

- **The worker, again.** It is called from `docinfo::scan_from`, so it runs where every other
  parser does and needed no new mechanism. No private key exists anywhere in tpdf; `rsa`'s
  RUSTSEC-2023-0071 is a timing leak in private-key operations, which this never performs, and
  is accepted in `.cargo/audit.toml` on that ground. Signing (§T6.21) did not change that: the
  OS signs, and `rsa` signs only in the test build, with keys the tests make.
- **The blob is the one `signature_contents` already prepared**, under `MAX_SIG_BLOB` and the
  BER walk's bounds, so no new byte reaches a decoder the certificate reader did not already
  reach. The signed attributes are located by walking the same, already-decoded structure.
- **The range is validated before a byte is hashed**: exactly two pieces from zero, within the
  file, and a hole that is `<`, hex and `>` decoding to this signature's own `/Contents`. The
  numbers are the document's and are converted, never cast; their sum cannot overflow a `u64`
  because each is under 2^63. A range failing any of it gets no verdict, because a digest over
  a range that leaves something else uncovered is the signature-wrapping attack's success
  condition, not a check.
- **Hashing is budgeted per document**: `MAX_HASHED`, one gigabyte across all signatures,
  charged before the bytes are read. A range is nearly the whole file and there may be
  thirty-two signatures, so without it a document chooses how many times the worker hashes
  itself inside its thirty-second deadline.
- **Key sizes are capped**: an RSA modulus over 8,192 bits is refused, and `rsa` refuses an
  exponent over 2^33 on its own, so a document cannot make one public-key operation expensive.

**What a verdict claims, stated as the threat model needs it.** `intact` means the covered bytes
are the ones signed and the key in the certificate the signature names made the signature. It
does not mean the signer is who the certificate says, that the certificate chains to anything,
was unrevoked, or was in date, and it says nothing about revisions appended after the signed
range --- a later revision can change every page a reader sees while an earlier signature stays
`intact`, which is why the dialog states the appendix beside the verdict. Since 2026-10-05 the
command-line gate has the counterpart: `verify --strict` fails a document when what was appended
after its last intact signature or document timestamp could not be read, or touches a page other
than by listing a signature or timestamp field among its annotations. A page counts as touched
when its object or anything it draws from was added or replaced; until that date only `/Page`
objects were counted, so a replaced content stream read as no page. Not judged: a change that
reaches no page (a catalog entry such as form defaults, optional-content configuration or
document JavaScript), and a change between two signatures, which the later one covers. A
document signature that carries the ESS `signingCertificateV2` attribute is held to it: one
naming another certificate than the one it carries is `unchecked`, reason `binding`. A signature tpdf
cannot fully check is `unchecked` with its reason and is never shown as `intact`; a SHA-1 match
is `weak`, because a chosen-prefix collision makes it forgeable by whoever prepared the
document. The attacker this does not stop is the one with their own key: anybody can make a
certificate naming anybody, sign, and be `intact`. Whether an issuer the OS trusts vouches for
the key is a second verdict since 2026-09-27 (§T6.22), and whether the document's own
revocation data says the key was withdrawn a third since 2026-09-28 (§T6.25).

**A fifth route, and a third parser: tpdf reads XMP as of 2026-08-21.** The catalog's
`/Metadata` is an RDF/XML packet the document chose, and `xmp::scan` hands it to `quick-xml`.
That crate was **already in the tree** through Tauri's `plist` dependency, so this compiled no
new code into the binary — but it is newly reachable from attacker-chosen bytes, which is the
only question that matters here. Four bounds, and the fourth is the one worth reading:

- **It runs in the worker.** Reached through `Request::Properties` like everything else that
  parses a document, so it is behind the T1 boundary and needed no new mechanism.
- **The packet is capped** at `xmp::MAX_PACKET` (1 MiB against real packets of 0.4--40 kB),
  nesting at 64 levels, and each value at 4 KiB — with the value bound applied **while the
  value accumulates**, not to the finished string, since clipping at the end means holding
  whatever the document sent first. Every one of those is *reported* through `Xmp::unread`
  rather than answered with a packet that claimed nothing.
- **The stream is decompressed under the document's existing `MAX_DECODE`**, so a compressed
  `/Metadata` is no different from any other stream bomb.
- **Entity expansion is structurally impossible, not merely bounded.** `quick-xml` delivers
  every `&...;` as its own `GeneralRef` event and expands nothing; `unescape` resolves the five
  predefined names and character references, and refuses everything else. Nothing here calls
  `unescape_with`, which is the only door a custom entity could come through. So a
  billion-laughs declaration costs a dropped `DocType` event — asserted by a test that
  distinguishes *not expanded* from *expanded quickly*, since a test asserting only that the
  parse terminated would pass on both.

**And the honest limit.** A conformance claim is a claim. tpdf does not validate a document
against PDF/A, PDF/UA or PDF/X, and the string shown is copied out of the packet — so a
document may write anything it likes there, including the word *valid*. The row says *the
document's own claim, which tpdf does not check*, and `properties.test.ts` asserts that a
hostile conformance string cannot put a verdict word into a label tpdf wrote. Same posture as
the signature rows, same reason.

#### T6.5 — The frontend names the mark's kind, added 2026-08-18

**A reader can now choose Highlight, Underline or Strike out, so the kind travels on the
wire** — `MarkKind` is a field on `edits::NewMark`. T6.3's bullet said the frontend cannot
choose the subtype; that is now the wrong sentence for the right property, and the property
survives intact.

**What the frontend chooses is a variant, not a string.** `MarkKind` is a Rust enum with
serde names, so an unknown name is a *deserialisation failure at the command boundary* — the
command never runs. The `/Subtype` bytes are still literals in `save.rs`'s `match`, reachable
only by naming one of them, and that `match` is still what makes a new variant a compile error
rather than a mark written as something else. So the closed set moved from "one variant,
nothing to choose" to "a closed set, chosen by name", and at no point is a caller's string
written into the file.

**Five variants as of 2026-08-19, and the sentence above deliberately no longer counts them.**
It said "three" until a comment bubble and a box were added, and the number was never the
property — a count in prose goes stale the next time the set grows, which is the failure this
repository already records about its own trap tally. The two new kinds are the ones a reader
*places* rather than selects, which changes nothing here: both are still variants named on the
wire, both map to a `/Subtype` literal through the same `match`, and the box additionally
carries an appearance stream built entirely from numbers of ours. Ask `MarkKind` in
`docmodel.rs` for the current set.

**The colour is the field a caller does choose freely**, and it did before this too: three
floats that reach `/C` and the appearance stream's `rg` operator. They are clamped by
nothing, which is worth stating rather than discovering — a value outside 0..=1 is a
malformed colour in a file tpdf wrote. It is not an escape: `format!` writes a number, PDF
readers clamp, and the surrounding operators are ours. Bounding it is a correctness question
rather than a security one, and it is not done.

**No new authority.** The command is the renamed `annot_highlight` — one path for all three
kinds rather than three commands, which is a smaller surface and not a larger one. It still
takes a document handle, a page identity and a list of numbers, still mutates a `HashMap` in
the app process, and still opens no file, writes none and reaches no worker.

#### T6.4 — A note on a mark, and taking one off, added 2026-08-18

**`annot_note` and `annot_remove` add no authority either**, for the same reason `annot_mark`
(called `annot_highlight` when this was written) does not: both take a document handle and an identity, both mutate a `HashMap` in the app
process, and neither opens a file, writes one or reaches a worker. `annot_note` additionally
takes a string, which is the reader's own and is not interpreted by anything on the way in —
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
mark, appended to the page's `/Annots` — and one refusal that is a correctness property in
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
app process — it opens no file, writes none and reaches no worker, exactly as
`page_rotate` and `annot_mark` do. `page_content_box` and `page_geometry` **do reach a
worker**: the first renders the page to find where its ink is, the second loads it to report
what size a crop makes it. That is the first pair of commands added since the viewer's own
that parse a document, and it is worth saying plainly rather than folding into the sentence
above.

**A third joined them on 2026-08-23**: `page_crop_box` loads the page to read its `/Rotate`
and its `/CropBox`, so that a rectangle a reader dragged on screen can be turned into the box
the model holds. It is the same shape as the two above and adds the same nothing — a handle
the frontend has, a page it knows, four numbers, and a parse in the worker that renders every
tile. It exists because the frontend is deliberately never told a page's `/Rotate`, so the
one place that can undo it is the backend.

What they add is nothing. Both take the document handle the frontend already has and a page
position in the file it already knows; neither takes a path, and the parse happens in the
same sandboxed worker that renders every tile a reader has already caused. A caller able to
reach them can already reach the tile protocol, which renders any page of the same document
on demand. The marginal authority is a render nobody asked for — a denial of service on
the render thread, which residual risk 7 bounds the same way it bounds `spike_exit`.

**A crop is four numbers off the wire, and they are checked in three places for three
different reasons.** `docmodel::Rect::is_proper` refuses a rectangle enclosing no area,
including any corner that is not a number, so a `NaN` cannot reach the model. `protocol.rs`
refuses a tile URL carrying **three** of the four corners rather than completing the
rectangle from the page — three numbers plus a default is a rectangle nobody asked for,
drawn plausibly and in the wrong place, which is what that parser exists to prevent — and
refuses a non-finite or degenerate one before a render is allocated for it. `pagetree`
refuses a crop that shares no area with the sheet, which is a different question and can only
be asked where the media box is known.

**The save path gains one mutation of the object graph**, and it is narrower than the others
on this surface. `apply_crops` writes `/CropBox` on the **page object** and never on an
ancestor: the box is inheritable, so a write onto a `/Pages` node crops every page hanging
under it, which for a document whose pages share one node is the whole file from a reader who
cropped one page. It intersects with `/MediaBox` per §14.11.2 rather than trusting the value,
and a crop the intersection empties is refused rather than written — a page that renders as
nothing is not an outcome a reader asked for.

**Residual, and it is a §T5 shape rather than a §T6 one: a crop hides, it does not remove.**
Everything outside the box is still in the file, still extractable, still searchable in any
reader — and tpdf's own search still finds it, because a crop moves character *boxes* and
not character *indices*. That is what `/CropBox` means and it is the right behaviour for a
crop. It is listed because it is the second operation on this surface where a reader could
plausibly believe otherwise, after deleting a page (risk 15), and because "crop" is a word
that sounds like removal in a way "rotate" and "move" do not. The operation that makes hidden
mean gone is `docs/PLAN.md` §6, and it is not built.

#### T6.7 — Saving over the open document, added 2026-08-19

**No new authority, and one new verb.** `save_document` takes a document handle and a
source path and writes the working document over that path. Its authority is `save_copy`'s
— the process's, which is the reader's — and the path is the frontend's in exactly the
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
when the document was opened** — `fingerprint.rs`, and `docs/PLAN.md` §5. Pointing this at
an unrelated document is now refused unless that document is byte-identical to the one the
reader opened, which is a considerably narrower gap than "happens to have the same number of
pages".

**That is a real narrowing and it is still not a guarantee, and the difference is worth
being exact about.** It is not a check that the path names the open document; it is a check
that the file at that path is unchanged since *some* document was opened, and the two
coincide only because the frontend passes the path it opened. A caller free to choose the
path could pass a *different* file it had first arranged to be fingerprinted. So the absence
§T6.1 records is unchanged — the source path is still the frontend's, unchecked against
what the render service opened — and this remains the command where checking it would
matter most. What the fingerprint removes is the accident, not the adversary.

**Fail closed, which is the part that is a security property rather than a correctness
one.** A fingerprint that could not be taken refuses the save rather than permitting it, so
"could not look" never reads as "looked, and it was fine". `save_copy` deliberately does
not, because a copy risks a bad new file beside an intact original and the refusal above
names Save a copy as the way out.

**Since 2026-10-01 the window also asks whether the open file changed on disk**, once a second
while it is visible: `document_stamp` and `document_differs` in `commands/document.rs`. Both
take the path from the frontend, as the save does, and neither checks it against what the
render service opened. What a caller naming another path can learn is that file's length and
modification time, and whether its bytes equal the opened document's. Neither command returns
content, writes, or parses the file: the app process calls `stat` and hashes. That is less
than `open_document` already grants a caller who can reach it. A reload that follows is
`file.reload`, which opens the file again through a worker like any other open; in the
automatic mode nobody is asked first, so a file another program put there is parsed without a
click, inside the same sandbox as a file the reader chose.

**The modification time is deliberately not part of the deep comparison, and that narrows
what this paragraph may claim.** A file whose mtime moved and whose bytes did not is
*accepted*: `cp -p` preserves a timestamp across a rewrite and a `touch` moves one without
changing a byte, so the timestamp is evidence about neither. The digest is the comparison,
and the sentence above should be read as length-and-contents rather than as three
independent locks — an attacker was never going to be stopped by a timestamp, and a
reader whose backup tool ran was being stopped by one. The mtime is still compared in the
one place nothing better is affordable: the look between staging and the rename, which
compares against what **staging** read rather than against what was opened, so that window
is milliseconds rather than the whole session.

**The document is closed before the file is replaced, and that is a correctness property
with a security-shaped tell.** A `rename` over a memory-mapped file succeeds on macOS and
leaves the worker serving the inode that is no longer at that path — measured, and in
`docs/TRAPS.md`. Nothing about that is exploitable; what it is, is a reader looking at a
document that disagrees with their own file while everything reports success, which is the
§T5 false-assurance shape pointed at the save rather than at a redaction. Windows refuses
the rename instead, so the order is what makes the two platforms agree.

**Two refusals, distinguished on the wire, which is unusual enough to state.**
`SaveFailure` carries `reopen`: false means nothing was touched and the reader still has
their document, true means it is closed whatever became of the file. It is `failure::Failure`
since 2026-09-06 and the wire is unchanged — the two booleans are *derived* from one
`failure::Action` at serialisation time, so the type and the wire cannot come to disagree about
a refusal. The reason it is a
field rather than a wording is the T8 reason one level down — a frontend that decided by
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
  mapped into that process — that is what the handover is — and a key to bytes you are
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
every worker after the first — the one the pool grows under contention, and every
replacement for one that crashed — because each maps the same bytes and meets the same
encryption. A design that unlocked only the first worker would render the page a reader
is looking at and refuse the next.

**What that costs, stated plainly: the password is in this process's memory while the
document is open.** So is every decrypted page of it, which is the more revealing of the
two, and neither is defended against something that can read this process's memory — an
adversary who has that has already won. It is not written to disk, does not reach the
session file, and goes when the slot does. What is *not* done, and would be the next rung
if it were worth one, is zeroing the buffer on drop: `String` does not, the value is
copied by every `clone` on the way to a worker, and a partial job here would read as a
guarantee.

**The refusal is structured for a reason that is a security one as much as a usability
one.** `Refusal::locked` travels as a field, so nothing downstream matches on a message to
decide whether to prompt. A frontend deciding *"show a password box"* by looking for the
word "password" in a backend string is one wording change away from prompting for the
wrong refusal — and the strings themselves are chosen in `progressive.rs` and
`worker_child.rs`, never taken from the document, so no failure path can become a route
for text a stranger wrote. That is the T8 property, in the one place a new error channel
was added.

**Two more places hold it as of 2026-08-23, and both are inside a boundary that already had
it.** `RawDocument::password` is the worker's own copy, kept because every question PDFium
cannot answer — comments, links, properties, the character mapping, the update section a
save appends — is a second parse of the same bytes with `lopdf`, and `lopdf` needs the same
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
`App.svelte` stores it. Every later use — pool growth, crash replacement, a save — is
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
— and each was already free to make that read. What it does add is copies, which is the
zeroing paragraph above becoming a little more true: there are more of them, none is zeroed,
and a partial job would read as a guarantee.

**One deliberate non-extension: printing an edited encrypted document.** `save::print_bytes`
takes the reader's password since 2026-08-30 — for a refusal, not for a job. With the key it
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
is a worker request rather than coordinator work — the same argument `Request::Append`
records. It names nothing the worker could act on: rectangles in, a count and some sentences
out, and nothing is removed by answering it. The removal itself happens in the coordinator,
inside the rewrite that already parses that file with `lopdf`, which is residual risk 18 and
is not widened by this.

**The claim is the exposure, and it is bounded by the type rather than by care.**
`redact::Applied` cannot carry `verified` without an empty `why`, and every object the removal
could not take becomes a reason. So the failure mode this section would otherwise have — a
reader told a file is clean when it is not — needs a defect in `verify::scan` rather than an
omission at a call site.

**Residual, and it is large enough to state plainly.** This reaches five rows of
`docs/PLAN.md` §6's carrier table and no more, all of it added 2026-08-27. The page's own
content: the show operators, and the shadow text (`/ActualText`, `/Alt`, `/E`) in **both** of
that row's homes — the marked-content property list the glyphs sit inside, and the structure
element that span belongs to, reached by `/MCID` through the parent tree, together with its
ancestors. An **annotation whose `/Rect` overlaps a region**, with its popup and its replies,
removed together with every reference to it rather than unlinked from the page. And the
document's own description of itself — `/Info` and the catalog's `/Metadata` — taken whole,
because a title that paraphrases a redacted line is reachable by no rule that matches text.
And the **outline entries whose title names what went**, with the subtree under each: a
bookmark title *is* the heading it points at, measured at 163 of 165 verbatim page text
against a 4% cross-document control, which is what licenses a string rule here where one was
refused for metadata. Entry by entry rather than the whole outline, so one redacted heading
does not cost a reader their table of contents. And the **form fields whose answer went** —
by either of two rules, that a widget under the field has gone, or that its value or its
`/DV` default is text that went (page text, or the answer of a widget the region took),
which is that row's *widgets outside the redacted rectangle* stated as a property rather
than as a location. A field loses all its widgets with its answer, and the answers under the
regions are verification needles, so a copy that survives is reported *not verified*.
Until 2026-09-26 the first rule needed *every* widget gone and a widget's answer never reached
the second: a field with a second widget outside the region kept its `/V`, drew it there, and
the redaction reported itself verified.

**An XFA form is refused rather than half redacted**, which is the one place this subsystem
answers a carrier by declining the operation. An XFA packet is a complete XML copy of every
answer, so taking the field values and leaving it removes nothing a reader could not recover;
a rule that reached inside it would be a second form implementation. The refusal is in the
pre-flight, before anything is touched, and is keyed on the redaction — an ordinary copy of
an XFA form still works, because a serialisation makes no claim for the packet to falsify.

Everything else in that table survives: an annotation *away* from every region, an outline
entry naming something else, and a form field naming an answer that did not go (all three
deliberate — a reader's other comments, their other bookmarks and the rest of their form are
not theirs to lose), page labels, embedded files, and any prior
incremental revision, since a copy is a serialisation rather than a sanitation (§T6.1). So does any structure element the parent-tree walk could not reach,
which is reported as unverified rather than passed over.

**The metadata strip, the outline removal and the field removal are properties of the
redaction and of nothing else.** A copy, an extract, a split, a merge and a print job all still
carry `/Info`, XMP, every bookmark and every answer across untouched, which is §T6.1's position
and is held by **one** condition guarding all three — so one mutation of it reddens all three
controls, which is why only one of them names it. What a region takes of a picture or a
vector drawing, and what it leaves and reports, is §T6.29. A CID-encoded document cannot be
scanned at the byte level at all, which `verify::scan` reports as a blind spot rather than as
a pass. None of that makes the answer *wrong* — it makes the answer *not verified*, which is
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
this command can overwrite any PDF the reader can write with a redacted version of itself —
which is what `save_document` can already do with an edited version of itself, so what this
adds over the existing surface is the removal rather than the write. The CSP is what bounds
it, as it bounds every command on this surface (residual risk 7).

**The order is what keeps a failure from being a loss.** Stage a sibling, fingerprint the
source, close the document, check the source has not moved, rename. Every refusal `save.rs`
states arrives while the reader still has their document and their marks, so the only window
in which content can be lost is between the rename and the read-back — and a rename is
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
no undo reaches across the removal. Nothing was built for this — it is what an in-place
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
wire boundary and before the model sees it — which is the check that matters, because a
`NaN` reaching a `/Rect` is written into a content stream by `format!` as the literal
`inf`, and this repository has already paid for that once with an unchecked `f32`.

The **page clamp** is deliberately not there. `docmodel` cannot bound the move — the
page's size in points is the renderer's answer and not the model's — so the viewer clamps
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

#### T6.17 — Form answers and visual signatures, added 2026-09-11

AcroForm fields are read inside the document worker. Form JavaScript remains
disabled; XFA and unsupported field types are not editable. Answers belong to
the document's edit journal and pass validation before save or a tab transition.
The writing worker emits explicit appearances and preserves encryption through
the existing rewrite path. The application forms check covers shared answers,
choice controls, tab isolation and reopening; independent PDFKit and pypdf checks
cover the saved values and appearances.

A visual signature is a PDF stamp, with no certificate, identity verification or
cryptographic assurance. The webview can read a PNG/JPEG explicitly selected in
its file input; it still has no general filesystem plugin permission. It rejects
files larger than 10 MiB and validates PNG/JPEG headers before decoding: at most
8 megapixels and 8192 pixels per dimension. Animated PNGs, additional JPEG frames
and JPEG dimension changes are refused. The decoded dimensions must match the
header. These limits bound image dimensions, not all native decoder allocations;
this input remains outside the PDF worker boundary described in T8.
The dialog always states that the image supplies no identity or certificate-based
verification, and the command is named "Place signature image".

Before IPC, signatures are reduced to at most 131,072 RGBA pixels. Rust validates
dimensions, exact byte length and nonzero opacity, and bounds retained signature
rasters, including undo history, to 4 MiB per document. The writing worker emits
RGB image data and a separate alpha mask; the coordinator decodes no image file.
Pixel comparisons through PDFium and PDFKit cover saved stamps on rotated and
cropped pages, with unedited inputs as controls.

Remembering a signature is explicit opt-in. macOS uses Keychain; Windows writes
only user-scoped DPAPI ciphertext to application-local storage. The IPC cannot
choose the storage path or service. The legacy localStorage value is removed only
after an identical protected copy has been read back; failures retain it and
report the problem. "Forget saved signature" removes protected and legacy values,
but cannot remove stamps already saved in PDFs. An active webview can still ask
the application to load the saved pixels, and same-user compromise remains outside
this protection. This is a visual image, not an authentication credential.

Saving a signed/certified PDF, including a form whose DocMDP policy permits filling,
now requires an explicit warning confirmation. Incomplete signature metadata also
requires confirmation; cancellation sends no writing command. This closes silent
invalidation in the normal UI, not the absence of incremental form updates or
cryptographic validation. The existing writer can still invalidate signatures
when a reader elects to continue.

#### T6.18 — Editing existing text, added 2026-09-13

Text discovery, embedded-font parsing, preview rewriting and saved rewriting run
inside document workers. The coordinator retains bounded changes in the edit
journal and sends them over the existing protocol; no new file-writing command
or filesystem authority is added. Preview serialisation is limited to 64 MiB.
The parser limits decoded page content to 1 MiB and 16,384 operations, with at most
128 pending text changes. Embedded font streams use the same 1 MiB decode bound.

Editing accepts a conservative grammar, not arbitrary PDF text. Font mappings,
metrics and glyph outlines are validated before use. Legacy replacements fit the
original line width and use existing glyphs. Explicit layouts allow a bounded
box (0.1 to 14,400 points per axis), font size (1 to 512 points), and at most 128
wrapped lines. Four bundled OFL Noto Sans styles and regular/bold Noto Sans CJK SC
provide fallback glyphs without filesystem access. An installed copy of a document's own
font can come before them since 2026-09-30; the worker still opens no file, and §T6.26
has what crosses the boundary instead. Of the bundled programs only the CJK ones pass
through the subsetter; an installed copy, TrueType or CFF, does too (§T6.26). Saved subsets
retain embedding rights and
must satisfy the existing embedded-font bounds. CJK programs are shared by style
and glyph set, and Latin programs by style; bounded
CIDToGIDMap streams map validated glyphs and ToUnicode streams are capped at
128 KiB. Layout restores the authored font, spacing, line matrix and cursor,
retains clipping, and refuses page overflow and new collisions with other text.
Draft previews are PNG crops of at most 1024 by 512 pixels from the same writer.
Supported
tag trees are checked in both directions for page and MCID ownership, bounded
to 256 content items per page and 4,096 per document, and preserved. An independent
4,096-node bound includes empty elements. Alternate text and unsupported structure
semantics are refused or retained read-only. Matching single-fragment ActualText
spans are editable and update their logical text together with their visible text.
Alternate logical text remains read-only. Untouched content operands retain their
original bytes. Skewed, mirrored and pattern-filled text can remain read-only on
an editable page; collision checks still account for its ink bounds.

Uncolored Type 3 fonts admit only bounded d1 vector glyph programs with validated
widths, Unicode mappings and ink bounds. Glyphs cannot invoke resources, images,
text or other glyphs. Per-glyph streams are capped at 64 KiB; a font shares a
1 MiB decoded budget and 16,384 operations. Supported axial gradient patterns
are validated and preserved without rewriting their resources.

Preserved Form XObjects remain read-only. Traversal is capped at eight levels,
32 calls, 1 MiB of decoded content and 16,384 operations, with cycle detection.
External forms, soft-mask graphics states and pattern colours are refused.
Forms containing text reserve their transformed BBox against layout expansion.
Page images, preserved forms and the images those forms draw share a 32 MiB
byte budget (`MAX_IMAGES`); each top-level form's tree is also bounded to 8 MiB
of decoded content and 524,288 operators. Indexed images validate
palette length and every sample before preservation. Tagged artifacts and
unmarked text remain read-only and retain collision bounds.

The editing grammar also validates embedded CFF/Type1C programs, CID-keyed CFF
programs and opaque image XObjects inside the worker. The CID-keyed reader
(`fonts/cff/cid.rs`) parses only closed DICT key lists, at most 256 font dicts,
4,096 glyphs and 48 charstring operands, and executes no charstring beyond the
operands before its first stack-clearing operator; outlines go through
ttf-parser as for every other CFF program. JPEG preservation uses zune-jpeg after checking
marker framing, at most 2 MiB of encoded data, at most 64 scans, and dimensions
no greater than 8192 per axis; decoded samples share the page image budget.
Decoder success does not prove every entropy sample is valid, and no image
sample is rewritten. Tagged grouping and table structures use separate depth,
container and identifier bounds. Linked headers must resolve to the same table;
the original structure graph remains unchanged. Fixed refusal messages never
include unknown document keys or operand values.

A text change is not a redaction: duplicate text in metadata, annotations or
other pages is not searched or removed. Pending text edits and redaction marks
are mutually exclusive; save and reopen before marking redactions. Saving takes
the existing full rewrite path, including encryption preservation and the normal
signed-document warning. Independent parser and PDFKit readback cover structure,
text and pixels outside the edited line on synthetic producer exports. These
examples establish compatibility for those inputs, not all output of a producer.

#### T6.19 — Pages from another file, at save, added 2026-09-19

**What changed.** A plan may place pages of a second document (`PageSource::Imported`), and
every rewriting save of such a plan now parses that document too. The model and the writer
landed first, when nothing in the webview could create an imported page; `page_import` has
reached it since later the same day (§T6.20). §3's list of writers is unchanged either way:
the new exposure is a **second input to a writer that already existed**, not a new writer.

**Where the parse happens.** In the same sandboxed worker as the rewrite, on the merge's input
channel: `save::staged_rewrite` reads each file the plan names into one read-only mapping and
`save_outside::InWorker::write` spawns the child with it on `worker::IN_FD`. The coordinator
reads those bytes and does not parse them, which is §3's rule for a merge. The worker loads
each with `lopdf` under `MAX_DECODE`, and `merge::import` walks only the pages asked for,
with the pages left behind as a wall (`docs/PLAN.md`, *The importer takes a selection*).

**What is checked, and where.** In the coordinator, the SHA-256 of the bytes handed over
against the digest recorded when the reader chose the file (`docmodel::SourceFile`), and a
file never fingerprinted is refused rather than trusted --- so a file replaced between the
insert and the save cannot put somebody else's pages into the reader's document. In the
worker, before the graph is touched: a file `lopdf` will not read, an **encrypted** one
(both `was_encrypted` and `is_encrypted`, for §T6.9's empty-password reason), and a page past
its end are refusals. The total size is bounded by the merge's `MAX_MERGE_BYTES`.

**What is refused rather than handled.** ~~Redaction anywhere in a document holding an
imported page, and inserting while regions are marked~~ (narrowed 2026-09-20, see below);
~~text replacement on an imported page~~ (2026-09-20); and a merge of a document that holds
one.

**Redaction beside an imported page, narrowed 2026-09-20.** What is refused is now a region
on **the imported page itself** (`Refusal::RedactionOnImportedPage`); a region on one of the
opened document's own pages is served. Nothing about the parse changed: the removal is
`save::apply_redactions` against baseline page objects, in the same worker, in the same
rewrite that already parsed the incoming files --- so this adds no input, no parser and no
channel. The verification is the part with a security reading, and it is unchanged in what
it does and sharper in what it says. `verify::scan` reads the whole written file and never
attributed a hit to a page; a word the reader removed from their own page and the file also
carries elsewhere has always come back as *"still in the file"*, with the verdict *not
verified*. An imported page is one more place it can be carried, not a new kind of blindness
--- measured on a document with none
(`verify::tests::a_needle_on_another_page_reads_as_still_in_the_file`). The direction of the
error is the one §6 requires: the file is reported **less** certifiable than it may be, never
more. `redact::inserted_pages_note` states the ambiguity beside the finding rather than
resolving it, and adds nothing when the scan found nothing. ~~**Residual: a redaction report
cannot say which page a surviving word is on.**~~ **Closed 2026-09-21** --- see *Attributing a
surviving word to a page* below; what the residual called for is what was built. The
image-only copy remains refused for such a document,
by `raster_redact::rewrite`, because `render::run_rewrite` hands that path no incoming bytes
and every output page is drawn through the opened document's own engine.

**Text replacement on an imported page, added 2026-09-20.** The writer now edits each
incoming document before `merge::import` walks it, so `textedit::write` --- the content-stream
parser and rewriter --- runs over a **second** attacker-chosen document in the same call. It
is the same code on the same kind of input in the same place: inside the sandboxed worker, on
a document loaded from the merge's read-only input mapping under `MAX_DECODE`, with
`textedit`'s own bounds (1 MiB decoded content, 16,384 operators, 32 MiB of images) applying
per document rather than across them. No new parser, no new channel, and nothing comes back:
the merged document goes out the rewrite's existing output channel as before.

What is checked before any of it: each replacement names a file the plan lists, and a page of
it that exactly one position places (`save::text_by_document`). A plan reaches the writer from
outside the process, so those are the model's refusals restated where a forged plan meets
them --- and the third case is why they are refusals rather than filters: editing the file and
importing a different page of it would write nothing and report a save.

**Residual.** The imported pages' own annotations, fonts and images come across as objects
and are written as they were --- this is an import, not a sanitation, exactly as a merge is
not. A `/Dest` from an imported page to one left behind dangles rather than importing it.
And a replacement is now validated by the **inserted file's own render worker**
(`document_text_runs` routed by `page_text_address`), which is a second worker parsing
attacker-chosen bytes for an edit --- the same pool `page_import_prepare` already opened for
drawing and searching those pages (§T6.20), asked one more kind of question.

**Attributing a surviving word to a page, added 2026-09-21.** `verify::scan` now walks, per
page of the file it just read, that page's reachable objects --- content streams, annotations
and their appearance streams, resources and the forms and fonts inside them --- and reports
each surviving needle as placed on a page set, shared between pages, or reachable from none.
**It adds no input, no parser and no channel**: it is the same `lopdf` document the scan
already loaded, inside the same sandboxed worker (`save::Verifier`, `Request::Verify`), read
and never written, and the report crosses the same pipe it did before with one bounded field
added.

What it needs bounding is the walk's own cost and the reply's size, both against a document
an attacker chose: `MAX_REACH_DEPTH` (32) on the reference chain, `MAX_REACH_OBJECTS`
(100,000) per page, `MAX_REACH_STEPS` (4,000,000) across the document including the direct
values inside one object, `MAX_CARRIERS` (1,000) per needle, and `MAX_LOCATED_PAGES` (64)
pages named per answer with the rest counted. The first three withhold **every** answer when
they trip rather than shortening one, which is the safe direction here and not the obvious
one: a truncated walk leaves an object it would have reached from a later page recorded as an
earlier page's alone, so it does not lose an answer, it manufactures a wrong one. A file with
more than one `%%EOF` withholds them too, for §6's revision reason.

**The failure that matters is a wrong page, not a missing one**, because a page number is
what a reader compares against the pages they marked. So the traversal is a deny list
(`verify::NOT_CONTENT`) of every key that leaves a page --- `/Parent`, `/Kids`, `/P`,
`/Dest`, `/A`, the outline chain, the structure tree, the form --- and where it is unsure it
reaches too little: a word it does not reach is reported unplaceable, never placed on a
guess. **Residual: an appearance stream under `/D` is not reached**, since that key is
shared with actions, so a word living only there is reported as unplaceable rather than
found on its page. The verdict is unchanged in every case --- a word still in the file is
still a leak, and this only says where.

#### T6.20 — Opening a second file to insert its pages, added 2026-09-19

**What changed.** `page_import(doc, after, path)` opens a PDF the reader picked in the open
panel and places every page of it in the working document. It is the command §T6.19's writer
was waiting for, and it is a **read**: nothing is written until one of §3's writers is asked
to, and that writer checks the file again by digest.

**Where the parse happens.** In the render service, through the same `open_handed` the
reader's own documents take — a worker pool of its own, sandboxed exactly as §5 and §6
describe, with the file handed over as a mapping and never as a path. The coordinator opens
the file, hashes it through the same handle it hands the service (`Fingerprint::of_open`, for
`open_document`'s reason), and asks the pool one more question before the model sees
anything: `document_properties`, whose parse is the worker's too. An encrypted file is
refused there, because the save refuses one (§T6.19) and a reader should hear so before
arranging the pages rather than after.

**Split in two the same day, when the reader could choose which pages.**
`page_import_prepare(doc, path)` does everything above --- the open, the encryption check,
the fingerprint --- and holds the file for the importing document without placing anything;
`page_import(doc, pending, after, pages)` places the pages named, and `page_import_cancel`
releases the file. Neither of the second pair takes a path, so the read authority is still
exactly one path per prepare. The fingerprint is still taken through the mapped handle, at
prepare, so what a save checks is the bytes the reader was shown the count of.

**The authority it adds.** The same as `open_document`'s: a caller able to reach it can have
any PDF the reader can read parsed in a sandboxed worker, and see its pages' pixels and text
through the tile and text commands. `dialog:allow-open` already granted the panel and
`open_document` already granted the read, so this is not new reach; it is a second route to
the same one, and it is recorded because it is a second *document* per tab.

**Who owns the handle.** The importing document's model --- `edits::Open::pending` while the
reader names the pages, `edits::Open::sources` once they are placed --- never the
webview, which is told the handle so it can draw and never closes it —
`docs/TRAPS.md`'s *A resource whose only owner is on the other side of a boundary* is what
that rules out. `close_document`, the in-place save and the redaction's close hand every
handle back to be released with the document, a waiting one included; `release_documents`
sweeps them with everything else. A waiting file also ends when a second prepare replaces it,
when the palette's question is dismissed (`page_import_cancel`), and when the model refuses
the pages named. A webview that never answers holds one pool per document at most, until the
document closes. Undo does not release one, because redo draws the same pages. A second
import of the same bytes into the same document reuses the first handle and releases the new
one; two tabs importing one file hold two pools, deliberately (see `page_import`).

**Residual.** Every file imported is one more worker pool for the life of the document that
imported it — the per-worker limits hold, and there is still no aggregate limit across the
pools of one tab, which is the tabs' own residual (§3) arriving a second way. A file that is
truncated under its mapping reports the document gone, and the viewer then stops asking for
tiles for the whole document rather than for that file's pages. Links found on an imported
page are followed only to pages that were imported with it; nothing reads the other file's
outline.

**A web address on an imported page is followed too, since 26.9.16, and it adds no authority.**
That sentence read "shown and refused, because its token indexes the other file's list" until
then, and the reason was a fact about the *frontend*: a token indexes one scan's list, the
working document's links are built from several scans, and the translated link had nowhere to
record which one it came from. The list itself was never out of reach — `document_links` is
asked of the imported file through its own handle and `webopen::Registry::adopt` takes the
answer under that handle, exactly as for the opened document — so those addresses were already
held and already nameable by anything that could call `open_web_link`. What changed is that the
frontend now carries the handle on the target (`Target`'s `doc`, which is never on the wire) and
hands it back, so the reader reaches an address of the file the page came from rather than
whichever address sits at the same index in the opened document's list. §T8's first fact is
restated for it.

**The addresses die with the document that imported them, on every path that closes one.**
`Registry::forget` takes the document *and* its sources, so the three closing paths cannot omit
them; two of them did, and that is fixed in the same increment. A handle goes back to the render
service to be reused, so a list left under one would answer the next file's clicks with this
one's addresses.

#### T6.21 — Signing with a certificate the reader has, added 2026-09-26

**What changed.** `sign_document(doc, source, identity, path)` signs the open document and
writes the result to a new file, and `sign_identities` lists the certificates in the reader's
store that have a private key. It is Phase 6 step 2 (`docs/PLAN.md` §9), and it is the first
feature that reaches the reader's **keys** --- the second thing §1 puts first after the files.

**The split is the security design, and each half is refused the other's authority.**

- **The worker parses and holds no key.** `sign_prepare::prepare` runs where every other
  parse runs (`Request::PrepareSignature`, answered by the document's own pool) and returns an
  incremental update with a zeroed `/Contents` hole, its `/ByteRange` and a SHA-256. No key
  and no certificate crosses to it. The request carries a time, and for a visible signature
  also the placement, the chosen image and the signer's name, which the app process reads out
  of the certificate to draw it (`worker_proto::Request::PrepareSignature`,
  `sign_prepare::Visible`). Corrected at the 26.9.21 release audit, when this said the
  request carried a time and nothing else --- true until the visible signature landed.
- **The app process holds the authority and parses no document.** `sign_cms.rs` reads the
  file's bytes and does arithmetic on them: the update must have been built against exactly
  this length, the range must frame exactly the reserved hole, the hole must be empty, and
  **the digest is recomputed here over the bytes that will be written** and compared with the
  worker's. A worker that described one document and built another is refused before the OS
  is asked anything. The CMS is assembled from the reader's certificate and the digest; the
  OS signs `SHA-256(signed attributes)` (`SecKeyCreateSignature`; `NCryptSignHash` through
  `CryptAcquireCertificatePrivateKey` with CNG only); the value is spliced into the hole by
  position. Before anything is written, `integrity::check` --- pure arithmetic and CMS over
  bytes this process assembled --- must call the result `intact`.
- **A worker reads the written file back.** `save::Verifier::signatures` maps the new file's
  handle into a fresh worker and asks `Request::Properties`, so the verdict the reader is shown
  is computed where the properties dialog's is. That is the worker backend, which is the
  default; under `TPDF_BACKEND=in-process` the read-back runs `docinfo::scan` in the app
  process, like every other parse on that fallback (`save::Here::signatures`).

**The key never leaves the OS, and tpdf never sees a PIN.** There is no API here that reads a
key or accepts one: `sign_cms::Key` has one method, which signs a digest. Keychain access
confirmations, smart-card PINs and Windows' key-protection dialogs are the operating system's
own, raised during the one call that signs; that is correct and expected, and nothing in the
webview or the app process can observe what is typed into them. A prompt appearing when the
reader asked to sign is the system working; a prompt appearing at any other time is not ours.

**What the certificate listing reads, and what it does not.** Certificates from the reader's
own store are parsed in the app process (`x509-cert`) to decide whether to offer them: key
kind, validity at the current time, key usage, and extended key usage --- a certificate that
states one must name a purpose a document signature serves (`sign_cms::DOCUMENT_PURPOSES`), so a
code-signing, TLS or login certificate is listed with its reason and not offered. **The
same rule is applied again where the key is used**: `sign_cms::finish` calls `usable` at the
signing time before it asks the OS for anything, because the window's command takes the
identity from the webview and, for an invisible signature, went from the store to the key with
no check until the 26.9.21 release audit found it --- an expired or code-signing certificate
named by its hash would have signed. `signing_refuses_a_certificate_the_listing_would_not_offer`
holds it, and a mutation removing the call proves the test can fail. Added
2026-09-26, after the only identity in the owner's keychain, an Apple Developer ID code-signing
certificate, turned out to pass the first three. These are the reader's certificates, not the
document's. The chain placed in the CMS is whatever the OS chain API assembles **without the
network** --- `SecTrust` with fetching disallowed, `CertGetCertificateChain` cache-only with AIA
disabled --- so building the chain adds no network authority. Fetching intermediates or
revocation data is still not done; asking a timestamp authority for a token, when the reader
chooses one, is, and is the application's second network authority (§T10). Building the chain is
not a trust decision here and nothing is concluded from it.

**What is written, and what is refused.** The original is never modified: the signed copy is
the original's bytes followed by one revision, written through the same staging and rename as
every other copy (`save::write_signed`), and naming the original is refused. Refused before the
OS is asked: a document with unsaved edits (a signature over the file would not be over what
the reader sees); an **encrypted** document, because the writer would encrypt the signature's
own value; a certification that permits **no** change (DocMDP `/P 1`); and a file over
`save::APPEND_MAX_BYTES`, the bound a marks-only append already works under (§T3).

**What a signature made here claims.** That these bytes, as written, were signed by the key in
this certificate, at the time the reader's clock gave --- PAdES B-B has no timestamp, so `/M` is
the machine's word. It does not claim the certificate is trusted by anybody, and a reader's
verifier decides that. Earlier signatures are left intact because their bytes are the new
file's prefix; `sign-probe` shows pyHanko reading each earlier one as covering its entire
revision with the appended change classed as form filling.

**A visible signature's appearance, configured (2026-09-27).** `sign_preview(doc, identity,
size, image, options)` draws the appearance before anything is signed. It reads the chosen
certificate's subject in the app process, as `sign_document` does, and touches no key; it
reads nothing of the open document. The document's **worker** builds a one-page file of the
preview's size, runs `sign_prepare::prepare_visible` over it and renders it with PDFium
(`Request::SignaturePreview`), so the only parser to meet the reader's text and pixels is the
one already contained, and the app process still never maps PDFium. What crosses back is a
bounded PNG (at most 720 points a side at two pixels a point). The reason and location the
reader types are written into the signed revision as `/Reason` and `/Location` --- covered by
the signature like the rest of the dictionary --- and kept, if asked, in `localStorage` with
the other per-machine preferences; a signature image's pixels are not.

#### T6.22 — Whether the OS trusts a signer, added 2026-09-27

**What changed.** `docinfo::scan_from` now asks the operating system's trust store, for every
signature whose integrity verdict is `intact` or `weak`, whether the signer's certificate
chains to a root it trusts (`trust.rs`; `docs/PLAN.md` §9, Phase 6). It is the first time a
document's bytes are handed to an **OS service** rather than to a parser tpdf links: the chain
builder is `trustd` on macOS and CryptoAPI on Windows.

**Where it runs, and why there.** In the worker, beside the integrity check, because the
certificates are attacker-chosen. That was a measurement, not an assumption: `signature-probe
--mode trust` repeats the scan in a child under `worker::SANDBOX_PROFILE` --- proved live by
the child failing to read its own input file --- and on macOS the verdict lines are identical
to the unsandboxed ones on every document tried. The profile's `(allow default)` is what admits
the Mach lookup to `trustd`; `(deny network*)` does not reach it.

**Windows containment measured 2026-09-29, Windows 11 build 26200.**
`trust::tests::platform_tests::windows_worker_trust_matches_uncontained_controls` starts a
child through `sandbox_win::spawn_contained` with production defaults. The child observes
low integrity (4096), job membership, and a denied write in a directory the parent can write.
It reads the CurrentUser and LocalMachine ROOT views without write access; the certificate
and trust-verdict digests for every root agree with the uncontained control. The measured
views held 71 and 70 roots, of which 58 and 57 were currently trusted for document signing.
Synthetic anchored, untrusted, expired, future, missing-intermediate, wrong-purpose, CMS
and timestamp-authority controls pass in both processes. Starting the child without
containment fails the integrity assertion. No trust stores or keys are changed.

The uncontained control was an elevated SSH process. A separate installed-application
run later on 2026-09-29 measured the 26.9.22 GUI at medium integrity (8192) in the ordinary
user's desktop session, with low-integrity (4096), job-contained renderer children loading
only the packaged PDFium. A disposable non-exportable RSA-2048 CNG key signed invisible,
visible and DigiCert-timestamped copies through that GUI; independent pyHanko readback
validated the signatures and trusted the timestamp offline. The self-signed signer remained
untrusted as expected. `BUILD.md`, *Installed Windows signing*, records the cancellation,
tamper and rendering controls. Successful long-term signing with a CA-issued signer and
hardware-token/PIN behavior remain unmeasured. Neither run is a cold-cache measurement.
Deliberately denied store-service access also remains unmeasured: an API error
is classified as `unchecked`, but a store failure presented by Windows as an incomplete chain
could still read as untrusted. The new test establishes access under the measured containment
and store configuration, not under every possible account or store ACL.

**What the OS receives.** Not the document's bytes as written: the CMS blob is bounded by
`MAX_SIG_BLOB` as before, each certificate is decoded by `x509-cert` and **re-encoded** as DER,
at most `trust::MAX_CERTIFICATES` (16) of them and each under `MAX_CERTIFICATE_BYTES` (64 KiB);
a set outside either bound is `unchecked` rather than truncated. So the platform parser meets
canonical DER that a Rust decoder already accepted, which narrows but does not remove its
exposure: the values inside --- names, extensions, keys --- are still the document's.

**Residual: a second native parser of attacker-chosen certificates.** On macOS the evaluation
happens in `trustd` --- measured: with the Mach lookup to `trustd` denied under `sandbox-exec`,
`SecTrustEvaluateWithError` fails with `errSecInternalError` --- so the certificates are parsed
there, **outside** the worker's sandbox, as well as by Security.framework inside the worker when
each `SecCertificate` is made. So a memory-safety defect in Apple's certificate
parsing reachable from a document now runs with `trustd`'s authority rather than the
worker's. The same is true of every process on the machine that verifies a certificate an
attacker supplied --- a mail client, a browser --- and it is bounded by the re-encoding above,
but it is a new reach from a document and is listed as residual risk 25. On Windows the parse
is in-process, inside the worker's containment.

**And, since 26.9.22, certificates from the network reach the OS in the coordinator.** A
signing with long-term data hands the timestamp token's certificates --- bytes the authority,
or anybody on an `http://` path, chose --- to `trust::platform` in the app process or
`tpdf-cli`, not a worker, twice: for the gate that the authority is trusted for timestamping
before anything is fetched for it (`longterm::vouched`, the reader's own
`trust::of_blob_for`, so a set over 16 certificates or one over 64 KiB is `unchecked` and the
signing refused), and for the offline chain the gathering looks for issuers in
(`longterm::os_chain`, which hands over at most 16 others, each under 64 KiB). Each is decoded
by `x509-cert` and re-encoded before the OS sees it, as in the worker. On macOS the evaluation
is still `trustd`'s; what is new is that Security.framework's certificate parsing, and on
Windows CryptoAPI's, meet network-supplied certificates in an uncontained process. Residual
risk 25 carries it.

**No network authority is added.** Fetching is off on both platforms, with the same flags the
signing chain uses, now shared from `trust::platform`: no intermediate is downloaded, no OCSP
responder or CRL is asked, and on Windows no root is auto-updated. The network authorities are
the updater's (§T9) and, since 2026-09-28, the timestamp client's (§T10); trust asks neither. The cost is in the answer and stated there: **revocation is judged only from
data the document carries** (§T6.25, since 2026-09-28; before that it was not judged at all), a
missing intermediate reads as a missing link unless the document's `/DSS` carries it, and a root
Windows would fetch on demand reads as untrusted until the machine has it.

**What a standing claims.** `trusted` means the OS store's rules accept a chain from the
signer's certificate to a root it trusts, **now**, and that the certificate names a purpose a
document signature serves. It does not mean the certificate was unrevoked, that it was in force
when the signature was made, or anything under Adobe's list --- a signature made against an
AATL-only root reads as ending at a root this computer does not trust, and the sentence says
so. A certificate that has run out since is `expired`, never `trusted`. The attacker this still
does not stop is one who holds a key a trusted issuer certified for them, or who controls the
reader's own trust settings; both are outside a document's reach.

#### T6.23 — The command-line tool, added 2026-09-27

**What changed.** A second executable, `tpdf-cli` (`src/bin/tpdf-cli.rs`, `src/cli.rs`),
ships in both bundles: `Contents/MacOS/tpdf-cli` on macOS and `tpdf-cli.exe` beside
`tpdf.exe` on Windows. `tpdf verify` reads every signature in the documents it is given,
`tpdf sign` signs one with a certificate from the OS store (§T6.21), and `tpdf identities`
lists those certificates. It is a new **entry point**, not a new capability: it reaches the
same code the window does, and nothing the window cannot.

**Who can invoke it.** Only the reader's own account, or something already running as it: it
is an ordinary program on disk, with no listener, no service, no URL scheme and no IPC. So it
widens nothing an attacker at the account's level did not have --- such an attacker can run
the application too --- and the assets in §1 are reached exactly as the window reaches them.
It contains no updater (§T9), and makes network requests only when `sign --timestamp`
names an authority, all from this process (§T10): the timestamp request; and, with
`--long-term`, up to 16 revocation requests to the certificate authorities
(`longterm::MAX_REQUESTS`) --- made only once the authority is one this computer trusts --- and
then the archive-timestamp request, a second request to the same authority. Every other
command, `verify` included, makes none.

**The control on the key is the operating system's prompt, and nothing works around it.**
`tpdf sign` asks the store through `keystore.rs`, the application's module, and the OS decides
whether this program may use that key: on macOS the keychain item's access control names the
programs allowed to use it, and `tpdf-cli` is a different program from `tpdf`, so the first
signing with any key raises the system's prompt, which the reader answers --- *Always Allow*
is what unattended use rests on, and is the reader's decision. A smart card or token asks for
its PIN in its own dialog. The tool supplies no password, sets no access control and
disables no prompt. A visible signature reads the saved signature image from the same
protected store (`signature_store.rs`) under the application's service name, which is a second
item and the OS may ask about it separately; `--no-image` does not read it, and neither
does `--image <file>`, which reads the named PNG or JPEG instead. That file is input like any
other: the tool opens it, holds its size to 10 MB, and hands it to a worker as a second
read-only mapping, where `signature_import` reads its header by a fixed grammar and then
decodes it (`png`, `zune-jpeg`) under an 8-megapixel bound. The tool's own process parses no
image. The worker is given nothing to write. A refusal there ends the command before the
store is asked for a certificate. A key file
(`.p12`) option is deliberately absent in this version: it would put a private key in a file
this process reads, which the whole design avoids, and is an explicit later decision.

**The same worker boundary, proved rather than assumed.** A document named on the command
line is attacker-controlled input exactly as a double-clicked one is. The tool parses none of
it: `verify` and the read-back after `sign` ask `Request::Properties`, and the revision is built
by `Request::PrepareSignature`, each in a worker spawned by `save::InWorker` --- which re-executes
the tool itself with `worker::WORKER_ARGV`, so `cli::main` answers that marker before anything
else, as the application's `run` does. Measured 2026-09-27 on macOS arm64 by `tests/cli.rs`:
with `DYLD_PRINT_LIBRARIES` set, the built tool's own process loads no PDFium while a worker it
spawned does (the positive control); in the test process acting as the tool's coordinator, the
dynamic linker's image list has no PDFium after `cli::run` has verified a document, and does the
moment PDFium is bound there (the control that the list can show it); and `sandbox_check`
reports a worker started by `Worker::spawn_shared` --- the call `InWorker` makes --- as
sandboxed, against the test process as the unsandboxed control. The PDFium library is found by
the application's own rule (`library_dir_among`), from the bundle's resource directory beside
the executable, never from the working directory: when the tool cannot tell where its own
executable is, it refuses to start rather than let the shared search fall back to `.`, where a
worker would have loaded whatever `libpdfium` the reader's current folder held (fixed at the
26.9.21 release audit).

**What it writes.** `sign` writes one file, the path after `-o`, through `save::write_signed` ---
the application's writer, staged and renamed, refusing the input under any name --- and only
after `sign_cms::finish` has refused anything `integrity::check` does not call intact. It
refuses an output that exists unless `--force` is given, and never modifies the input. `verify`
and `identities` write nothing. Paths come from the command line and are used at the account's
own authority, which is what a command-line program is for; there is no webview between them
and the reader.

**`info` and `text`, and a document's password** (added the same day). Two more commands
that ask the workers nothing new: `info` asks a worker `Request::Properties`, `Request::Open` and
`Request::Form`, `text` asks `Request::Open`, `Request::Mapping` and `Request::Text` per page ---
all questions the window already asks --- through `save_outside::Session`, one worker per
document with the 30 s deadline on every question; the reading order is computed in this process
from the character codes and boxes the worker returned --- values the document influences, but
parsed by nobody here: the ordering sorts and bands numbers, its recursion stops at twelve
levels as `reading.ts`'s does, and `text` refuses to produce more than 64 MiB rather than
growing without bound. `tests/cli.rs` holds both to the containment check above under
`DYLD_PRINT_LIBRARIES`. `text -o` writes one file, refuses an existing one without `--force`,
and refuses the input under any name. The one new input is a **password**, and it never
appears on the command line: `--password-env VAR` names an environment variable, because argv is
readable by every process on the machine and lands in shell history, while another process can
read this one's environment only at the same account's level, where it could equally read the
reader's files. The tool reads the variable, sends the value to the worker as `Request::Unlock`
over its stdin --- §T6.9's route, unchanged --- and prints it nowhere; `tests/cli.rs` asserts it
is absent from the output. The workers inherit the tool's environment, so the variable is
visible inside them too; that widens nothing, since a worker already holds the document the
password decrypts and receives the password on stdin regardless.

**`fields` and `fill`** (added the same day). `fields` asks one worker `Request::Form` ---
`forms::scan`, the window's form reader --- and groups its answer by field in this process.
`fill` asks one worker `Request::Properties`, `Request::Open` and `Request::Form`; checks the
answers here, against the widgets the worker returned, with `forms::check`, the window's rules
for a typed answer (values the document influences --- names, options, rectangles --- parsed by
nobody here: the answers file is JSON from the reader's own account, bounded at 16 MiB, and each
answer is held to `forms.rs`'s 16 KB); then writes through `save::write_copy`, the window's
*Save As*: the plan crosses to a writing worker, which re-validates every answer against its own
scan before `forms::write` touches the object graph, and the copy is staged in a directory beside `-o`.
A fresh worker reads the staged form back, and only a copy that agrees is given the output's
name; an output `--force` would have replaced is left as it was otherwise. Plain output passes
every document-derived string through `cli::printable`, so a title or a field name cannot send
a terminal control sequence. `tests/cli.rs` holds both commands to the containment check above --- `fill`'s three
workers each map PDFium and the tool's process none. `fill` writes one file, refuses the input
and the answers file under any name as the output, and refuses an existing output without
`--force`. **It refuses a signed or certified document**, or one whose signatures could not all
be enumerated, because the writer is a full rewrite that would invalidate them --- the window
asks before doing the same, and a command line has nobody to ask. An encrypted document is
written re-encrypted with its own passwords, as the window's save writes it.

**JSON edit API** (`edit`, `comments`, `text-runs`, added 2026-09-29) uses the
same worker readers and copy writer. The coordinator reads at most 1 MiB of edit
JSON, rejects unknown fields and schema versions, and applies at most 1,000
operations to the GUI's `Edits` model. Text replacements are validated by a worker
against the inspected revision before entering that model. Original comment
identities must belong to the selected source page. No PDF parser is added to
the coordinator. Plans cannot name worker file descriptors, inject internal save
plans or enable document JavaScript. A failed operation publishes no file.

Publication shares the page commands' staging, input-alias refusal, signature
consent and encryption preservation. The dry run validates the model and text
requests without saving; it cannot establish that the later write will succeed.

`render` uses the same contained worker to read and rasterize one page. The
coordinator accepts raw RGBA tiles only after their length equals the requested
geometry and fits the shared mapping; it encodes PNG without parsing PDF or
decoding an image from the worker. Allocation is capped at 16,777,216 pixels,
each side at 8192 pixels, and each worker request retains its 30-second deadline.
Output staging and input fingerprint checks precede publication. Rendering
makes no signature or redaction-verification claim.

The Python client uses argument arrays and stdin JSON, never a shell. Passwords
are child-environment values. Calls have deadlines and attempt owned-process-tree
cleanup; a timeout is not a rollback. There is no new network listener or authority.

**Page operations** (`merge`, `extract`, `split`, `rotate`, `crop`, added 2026-09-29)
use `save::write_merged`, `save::write_split` and `save::write_copy` with `InWorker`.
The coordinator constructs plans from page counts and geometry returned by workers;
crop coordinates are converted through `Request::CropBox`, never by parsing PDF here.
Every input is inspected for signatures before writing; signed or incompletely inspected
inputs require `--invalidate-signatures`. Merge preserves the first input's encryption
and refuses encrypted additional inputs. Source fingerprints are checked before writing
and again before publishing. This detects changes across the operation, but is not a
filesystem snapshot against concurrent writers.

Outputs go to an exclusively created staging directory beside the destination. Fresh
workers check page counts, displayed dimensions and encryption before publication.
Without `--force`, a hard link publishes each file without replacing a concurrent
arrival. `--force` uses the save path's replacement; input aliases, symbolic-link
outputs and directories are refused. A split checks all destinations and stages all
parts before publishing; a publication failure can leave earlier parts, which are
listed in its JSON report. This is per-file atomicity, not a multi-file transaction.
The output filesystem must support hard links for no-replacement publication.
`tests/cli/pages.rs` exercises the built tool and checks content/order, rotation and
crop boxes independently of its JSON reports. Page-range expansion is bounded to
100,000 selected pages, and splits to 10,000 output files.

**`redact`, a second entry point to redaction** (added the same day). `tpdf redact` reaches
the window's *Redact and save as…* with no window: it opens the document in a
`render::RenderService` on the **worker backend, always** --- whatever `TPDF_BACKEND` says ---
whose workers are the tool re-executed with `worker::WORKER_ARGV`, as `Env::worker`'s are; runs
the viewer's search (`search.rs`) over each page's codes in this process, which reads numbers
and code points a worker returned and parses nothing; marks what it found on an
`edits::Edits`, the window's model, through `Edits::redact`; and hands the result to
`commands::redact::ask_redactions` and `redact_copy_asked`, the body of the window's command
(§T6.11) --- plans from a worker, the rewrite by `save::write_copy` in a writing worker, the byte
scan in a scanning worker, the OCR gate in its own worker under `OCR_SANDBOX_PROFILE` (§5.1),
and the black fill. `cli::main` answers the OCR worker's marker as well as the parser's, as the
application's `run` does. `tests/cli.rs` holds `redact` to the containment check above, dry run
and real run: the tool's own process maps no PDFium; the processes beside its PDFium workers
that map none are the OCR worker, which reads pixels, and a pre-spawned spare that ended
without a document.

**The report is the claim, and it is the window's.** `verified` is `true` only when
`redact::Applied` is --- every list of reasons empty --- and the tool adds two readers that can
only take that away: a search of the written, unfilled file for every `--text` and `--pattern`
(the check a reader makes by hand), and a reason for each match whose characters have no
position and so could not be marked, which the window's scan would not look for because it
looks for what was *taken*. The sentence printed is `recovery.ts`'s `afterRedaction`, held to
it by `cliwording.test.ts`, so a script is never told more than a reader of the window. **Exit
code 1 means written and not proved clean**: the file is kept, as the window keeps it
(§T6.11's *never claim clean, not never write*), and the reasons are the reader's to act on ---
a script that treats exit 1 as success has discarded the one thing this subsystem produces.
Exit 4 removes what was written. A `redact` report holds the removed words, in `hits` and
`taking`; it is the reader's to store as they would the original. **A signed document is
refused** unless `--invalidate-signatures`, for `fill`'s reason; with it, the report counts the
signatures the copy no longer carries intact. XFA is refused before anything is asked, in
`save.rs`'s words (`redact::XFA_REDACTION`).

**Residual, and it was the gate --- closed 2026-09-27.** On the machine this was built on (macOS
27.0, build 26A428) the OCR gate read nothing: Vision refused every image inside the OCR worker
with `__objc2.missingError`, so every redaction --- window and tool alike --- was *not verified*,
correctly, and `tpdf redact` exited 1 for every document it wrote. The cause was the profile
refusing Vision's first-use model cache write, and the fix readies the engine before the profile
comes down without widening it (§5.1). Measured after it: `tpdf redact` on
`testdata/text-base14.pdf --text WWWWW` exits 0 with `verified: true` on a cold cache, and the
same build with the warm-up removed exits 1 with the OCR reason (`docs/TRAPS.md`, *On macOS 27
the OCR worker's Vision refuses every image*).

**The link.** *Install command-line tool…* (`command_line_tool`) makes `/usr/local/bin/tpdf` a
symbolic link to the bundled tool, and its sibling removes it. (What the window reads to grey
the two commands is §T6.38; it is not what this decides by.) The webview names no path: the
link and the target are both fixed in `clitool.rs`, so the widest thing it can ask for is that
one link made or removed. A file at that path that is not a link to a `tpdf-cli` in the
`Contents/MacOS` of a folder named `*.app` is never replaced or removed --- the bundle's name
has been required since the 26.9.21 release audit, before which any folder laid out that way
counted. The decision is made on what `plan` read; the privileged `ln -sfn` or `rm -f` acts
on the path moments later, so a local process that swaps the file in that interval is not
excluded --- one able to write `/usr/local/bin` could replace the link without tpdf anyway. When the directory is not writable --- the
ordinary case, and on a new Apple silicon Mac it does not exist --- the change goes through
AppleScript's `do shell script ... with administrator privileges`, which is the system's own
authorization dialog: tpdf never sees the password, and the tool's path reaches the shell as an
argument through `quoted form of`, never as script text. The answer shown is the link read back
afterwards. A link from a translocated copy (macOS App Translocation) is refused, because it
would dangle at the next launch.

**The `PATH` entry (Windows, added 2026-10-01).** There is no link on Windows. The same two
commands, `tpdf-cli path --add` and `--remove`, and the per-user installer's hooks add the
folder `tpdf-cli.exe` is in to the user's own `PATH` (`HKCU\Environment\Path`) or take it out
(`userpath.rs`). The webview and the command line name no folder: it is the folder of the
running executable. The value is read whole through the registry API and written only when the
read succeeded, the value is text, and the list changed; its type is kept and every other entry
is kept as written. Nothing under `HKLM` is touched and no administrator is asked for. The
installer's script reads and writes no `PATH` itself, because an NSIS string is cut at a fixed
length and a cut value written back would drop the reader's own entries. What the entry grants
is what the link grants: whoever can replace the files in that folder decides what `tpdf-cli`
runs, and the per-user install folder is writable by the user's own account, as the
application beside it already is.

**Residual.** A link in `/usr/local/bin` points into the application bundle, so replacing the
bundle replaces what `tpdf` runs --- which is the same authority an attacker who can replace the
bundle already has over the application, and is why the link is not a copy. The Windows tool
**first ran on a desktop 2026-09-27** (MOTHERSHIP): `identities` read the real
`CurrentUser\My`, `verify` gave the macOS verdicts fixture for fixture, and a temporary
document-signing certificate signed invisibly and visibly, read intact by tpdf, pyHanko and
`openssl cms -verify`. `sandbox_check` has no Windows counterpart --- a Windows worker's
containment is its parent's --- so `scripts/win_modules.py` was the instrument: sampled from
outside through 800 verifications, the tool's own process never had `pdfium.dll` mapped (15
modules), against a positive control in which a process that loads the DLL read as loading it.
The workers refused the module query after their first moments, which is consistent with their
token and is not evidence about what they map.

#### T6.24 — Checking a timestamp token, added 2026-09-28

**What changed.** A signature's RFC 3161 token (the unsigned attribute
1.2.840.113549.1.9.16.2.14) and a document timestamp's `/Contents` were read for their time
and their authority's certificate and nothing else. `integrity/token.rs` now checks them:
the token's own CMS signature, its `messageDigest` against its `TSTInfo`, its ESS binding to the
authority's certificate, and its `messageImprint` against the signature's value octets or a
document timestamp's range (`docs/PLAN.md` §9, *Is the timestamp intact*). Its authority is
then asked about through the OS store (§T6.22) with the timestamping purpose.

**Where it runs, and what bounds it.** In the worker, from `docinfo::scan_from`, on bytes that
went through the same bounded preparation as the signature's: the token is inside the
`/Contents` blob `signature_contents` already capped at `MAX_SIG_BLOB` and walked to definite
length, or it *is* that blob for a document timestamp. New parsing on attacker-chosen bytes,
all of it in the worker: the token's `SignedData` (`cms`), the `TSTInfo`'s first three fields
read positionally with each type checked, and the ESS attributes (`der`'s derived decoders). The
public-key operations are the ones `integrity.rs` already performs, under the same key-size
ceilings. Hashing is charged to the document's `MAX_HASHED` before it happens --- a document
timestamp's imprint is over nearly the whole file --- so a token cannot make the worker hash
more than a signature could. A document timestamp's range passes `integrity::covered` before
anything is hashed, the rule that refuses the wrapping attack's shape for a signature.

**What reaches the OS store.** For a token that is `intact` or `weak` only, its certificate
set, re-encoded and bounded exactly as a signer's is --- so residual 25 now covers a second
set of certificates per timestamped signature, not a new kind of exposure.

**What a verdict claims.** `intact` and `weak` make the token's `genTime` an attested time:
the authority whose key the token names said this signature (or these bytes) existed then.
`attested` is set in the worker and read by the dialog and the command-line tool, so a broken
token's time --- which is the token's word and nothing more --- is never shown as attested. It
does not claim the authority is anybody in particular: that is the standing, `trusted` only
when the store vouches for its certificate **for timestamping** and **now**. It does not claim
the signer's certificate was in force at `genTime`: the signer is still judged at the present.

**No network authority is added.** Checking reads only what the file carries. Requesting a
token is Phase 6 step 3's increment B, which is where that changes: §T10.

**Residual: an attacker who holds a key a trusted issuer certified for timestamping** can mint
a token stating any time, for any signature, and it reads `intact` and `trusted`. That is what
trusting a timestamp authority means, and the same is true of every reader of RFC 3161 tokens;
listed as residual risk 28 together with the moment the authority is judged at.

#### T6.25 — Reading revocation data a document carries, added 2026-09-28

**What changed.** A document made for long-term validation carries the revocation data it
was signed against: a `/DSS` dictionary (`/Certs`, `/OCSPs`, `/CRLs`, and `/VRI` entries naming
more of the same), the Adobe `adbe-revocationInfoArchival` signed attribute, and the CMS
`crls` set. `revocation.rs` now reads all three and judges, for the signer's certificate and
a timestamp authority's, whether that data says the certificate was revoked (`docs/PLAN.md`
§9, *Revocation, from the document's own data*). A signature whose timestamp is intact and
whose authority this computer trusts is then judged at the time the token attests.

**No network authority is added, and that is the decision.** Reading and verifying never
fetch. Asking an OCSP responder or downloading a list while a document is open would tell each
certificate authority which signed documents the reader opens, and would put a network
authority in the read path, which runs in a worker with `(deny network*)`. So a document
carrying no revocation data reads *not checked* for revocation, which is what it is.

**Where it runs, and what bounds it.** In the worker, from `docinfo::scan_from`, beside the
integrity and trust checks. New attacker-chosen input, all of it parsed there: the `/DSS`
streams, decoded through `lopdf`'s bounded `decompressed_content_with_limit` at each kind's
size bound --- a filtered stream that will not decode inside it is counted, never read raw;
`OCSPResponse` and `BasicOCSPResponse` (`x509-ocsp`, one new package); `CertificateList`
(`x509-cert`); the Adobe attribute, walked by tag. Counts and sizes are named constants in
`revocation.rs` --- 32 responses of at most 64 KiB, 8 lists of at most 8 MiB, 32 `/DSS`
certificates of at most 64 KiB, 64 `/VRI` entries --- and anything dropped at a bound or
unreadable is counted in `Limits::revocation_dropped` / `revocation_unread`, and turns every
answer that would have been *good* or *none* into *not checked*: what was not read might have
said revoked. **The counts bound the decoding, not only what is kept** (since 26.9.22, found by
its release audit): an indirect object is decoded once however many arrays and `/VRI` entries
name it, and at most as many `/DSS` streams of each kind are decoded as are kept --- the next is
counted dropped without being decoded (`docinfo::read_dss_with`). Before, every array item was
inflated first and the counts applied afterwards, so a `/CRLs` array naming one small stream
that inflates to 8 MiB a thousand times inflated 8 GB in the worker to keep one list; now the
most a `/DSS` can make the worker inflate is its count bounds times its size bounds, about
68 MiB. Every signature over a response, a list or a delegated responder's certificate
is checked by `integrity::signed_by`, the arithmetic a signature's own verdict rests on, and
charged to the document's `MAX_HASHED` before it is hashed; a list's check is memoised per
issuer, so eight megabytes are hashed once however many certificates ask.

**What reaches the OS store.** The `/DSS` certificates, as extra candidate issuers for the
chain the OS builds --- at most sixteen beside the signature's own set, each under 64 KiB and
re-encoded from a successful decode. Residual 25 now covers them too. Revocation data never
reaches the OS: tpdf judges it itself, because the OS evaluation it asks is offline and would
not.

**What an answer claims.** `good`: checked data --- signed by the certificate's issuer, or
for OCSP by a responder the issuer authorised with `id-kp-OCSPSigning`, about this
certificate by `CertID` or by the list's issuer and scope --- says it was not revoked, and its
`nextUpdate` is after the moment judged (EN 319 102-1 §5.2.5.4's default freshness).
`revoked`, with the date and reason; after an attested moment, which does not undo the
signature. `unknown`, `none`, and `unchecked` with a reason. Which moment, and whose clock it
is, is stated in the sentence every time.

**The attested moment moves the signer's trust question, and is earned.** Only an `intact`
token (not `weak`), from an authority the store trusts for timestamping *now*, not shown
revoked by the document's own data, with `genTime` inside the authority's certificate's dates.
Anything less and the signer is judged now, as before.

**Residuals** 30 and 31, below; 28 is narrowed.

**The whole chain, added the same day.** The same data is now asked about every certificate
from the signer's --- and the authority's --- up to its root, at the moment the leaf is judged
(`revocation/chain.rs`, `docs/PLAN.md` §9 *The whole chain*). A certificate above the leaf that
the document shows revoked before that moment revokes the chain: `verify --strict` fails it, and
on the authority's side it earns no attested moment. **No new input and no new authority**: the
walk reads the certificates already parsed --- the signature's, its token's, the `/DSS`'s and
those inside the responses --- finds each issuer by name *and* key with the same
`issuer_of`, and judges with the same `judge`, so every signature it checks is charged to the
same `MAX_HASHED`. **Bounded**: at most eight certificates judged per chain
(`chain::MAX_CHAIN`), the rest counted to the walk's end and turning a chain that would read
*good* or *none* into *not checked*; a certificate met twice ends the walk, so two authorities
certifying each other cannot loop it; the steps are at most the candidates, which the bounds
above already count. The chain the OS assembles is **not** offered as candidates --- a revocation
answer stays a property of the file, the same on every computer --- which is residual 34.

**Archive timestamps, added the same day (PAdES B-LTA).** A document timestamp later in the file
now fixes the moment the timestamp authorities before it are judged at (`docinfo::archive_moment`,
`docs/PLAN.md` §9 *Archive timestamps*). **The party under question still does not choose its
moment**: the archive is another authority's token --- or the same one's, later --- judged by the
same rules as any token (intact, not weak; trusted for timestamping now or at a still later
archive's moment; nothing on its chain revoked; its time inside its certificate), and only a
range reaching past the earlier one's counts. No new parse: the fields were already read, now in
a different order, and each document timestamp was already checked over its range.

#### T6.26 — An installed copy of the document's font, added 2026-09-30

**What is new is one input crossing into the worker and one file read in the app
process.** When automatic font mode needs a character the document's embedded subset
lacks, the worker names the PostScript name it would try (`textedit::Preview::wants`), the
app process asks the operating system where a font of exactly that name is installed and
reads the file (`sysfont.rs`), and the next request carries the bytes to the worker as
`textedit::Layout::installed`. Nothing else in the application reads a font file.

- **The app process parses nothing.** CoreText (macOS) or DirectWrite (Windows) answers
  where the font is; the file is read whole, bounded by the read itself at
  `MAX_INSTALLED`, 32 MiB, with one byte over refused, so a file that grows between two
  calls cannot outrun the bound. A name that is not 1--63 printable ASCII characters
  without PDF delimiters is never looked up. The webview cannot supply the bytes: every
  change from it is stripped of `installed` before the batch is built
  (`sysfont::batch`), so the only font a worker is handed is one this process read or a
  subset a worker built.
- **The worker trusts none of it** (`textedit/fonts/installed.rs::accept`). The face must
  carry the requested name in every name record that decodes, which also catches an OS
  lookup that answered with a substitute and the wrong face of a collection; TrueType or
  CFF outlines, no CFF2, variable or colour font; OS/2 rights of 0 or editable (0x8) only;
  every width the document's subset declares must agree within 1/1000 em; the supplied
  bytes are bounded again on deserialisation, before decoding, and in `accept`. The
  program is parsed by `ttf-parser` and cut down by `subsetter`, the same two crates and the
  same process that already handle the bundled fonts, and the subset is held to
  `MAX_CONTENT` and parsed back before it is used.
- **CFF outlines, added 2026-09-30, widen what the worker parses in an installed file.**
  `ttf-parser` reads the CFF table and `subsetter` desubroutinizes the charstrings of the
  glyphs kept, a code path the bundled TrueType fonts never reach. Its subroutine recursion
  is bounded (depth 64), but the expansion is not bounded before its output exists: a
  hostile installed font whose subroutines call each other several times at each level can
  make the output large before `MAX_CONTENT` refuses it. On Windows the worker's commit cap
  refuses the allocation; on macOS a worker's memory has no bound (see *Memory has no kernel
  bound on macOS* above), so this is the same exposure a document's own decompression bomb
  has there, reached only through a font the reader installed. What is embedded is the subset's bare CFF with one string added to its Top DICT
  (`fonts/cff/rights.rs`, which patches five-byte offsets in place and refuses anything
  else), and it is parsed back by `cff::cid::parse`, the reader any document's CID-keyed
  CFF already goes through, with every glyph width compared, before it is written.
- **What a document can make happen.** The name comes from the document, so a document can
  make the app process read the file of any font the reader has installed, and the worker
  parse it. The reach ends at the worker, which has no network (§T4), and at a subset of that
  font's glyphs inside the edited copy the reader chooses to save --- the glyphs of the
  characters they typed and of the document's own subset, never the whole program. A font
  whose rights forbid embedding or subsetting is refused. The file's bytes are the reader's
  own installed font, not attacker-chosen data, except where the reader installed a hostile
  font, which then reaches the same parser every document font already reaches, in the same
  sandbox.
- **What the journal carries.** After an Apply the journal holds the worker's subset rather
  than the file, so the bytes riding on every tile, outline and save request are kilobytes,
  and each of those requests puts the subset through `accept` again rather than trusting the
  worker that built it.

The same edit can come out differently on a computer without the font, where Noto is used;
that was accepted with the decision and is residual risk 35 rather than a defect.

#### T6.27 — Making tpdf the default application, added 2026-10-02

*Make tpdf the default PDF app* (`default_pdf_app`) is the one place tpdf touches which
application opens a PDF. The webview passes nothing: the application is the bundle the
running executable is inside (`defaultapp::bundle_of`), so the widest thing the webview can
ask for is that this copy of tpdf becomes the default. Nothing runs it at start, and tpdf
never asks whether it is the default.

On macOS the change is `NSWorkspace`'s, and the system shows its own confirmation to the
person at the machine; declining leaves the setting as it was. The answer shown is the
application the system names afterwards, never the call's result: the older Launch Services
call returned success and changed nothing on macOS 27. On Windows an application cannot set
the default, so the command opens Settings at Default apps (`ms-settings:defaultapps`, a
constant) and the reader chooses there.

A compromised webview could therefore raise the system's question, or open Settings, without
the reader having asked. It cannot answer the question, and it cannot name another
application.

#### T6.28 — A text layer from recognised words, added 2026-10-03

`tpdf ocr` writes a copy in which pages without text carry the words a recogniser read off
them, as invisible text. The window's *Recognise text and save as* does the same through
`ocr_copy`, which is the twelfth command that writes a file (§3). Its path is the one the
reader chose in a save panel, and like the other eleven the command does not check that.
`ocr_cancel` sets a flag the running recognition reads between pages; it takes no argument
and writes nothing.

**No new process and no new authority.** The page is rendered by the parser worker under its
own profile. The pixels go to the OCR worker of §5.1, under `OCR_SANDBOX_PROFILE`, which
already existed for the redaction gate. What is new is how much that worker is shown: a whole
page where the gate showed it strips. Its input is still an RGBA buffer the coordinator
assembled, of a size the coordinator chose (`ocr_layer::render_size`, at most 16 MiB and
8,192 px a side), with no format to parse. The words then go to a parser worker inside the
plan and are written by `lopdf` in `save::rewrite`, the path every other rewrite takes.

**The recognised text is attacker-influenced, and it is data all the way.** A document's
author chooses what its picture shows, and so what the recogniser returns. Each word reaches
the content stream as a hex string of UTF-16 code units (`textlayer::show`), so no character
of it can end a string or begin an operator; control characters are dropped. A layer is at
most 20,000 words a page and a word at most 512 code units, refused whole above that. The
rectangle is four finite numbers or the word is left out.

**What the layer claims, and what it does not.** The copy now answers a search with what a
recogniser believed the page says. A picture can be made to be read as words it does not
show to a person, and a recogniser misreads on its own. tpdf checks one thing: each page
given a layer reads back with the characters that were recognised, so the layer written is
the layer recognised. The window's copy is read back while it is still the staged file
(`save::write_checked_copy`), so one that does not read back never gets the reader's name. It does not check the recognition, and the documentation says so.
This is the caller `ocr.rs` describes as wanting recall, and nothing downstream may treat a
layer's words as verified. In particular the redaction gate does not: it reads pixels
through its own control (§5.1), never this layer.

**Residual.** A reader who searches a recognised copy for a word and finds nothing has
learned that the recogniser did not report it, not that the page does not show it. And a
redaction by search on such a copy marks only what was recognised.

#### T6.29 — A drawing inside a region is removed, added 2026-10-03

A path whose bounds lie wholly inside a marked region is taken out of the page's content
(`redact::remove_paths`), outline included. No new command, process or authority: the plan is
still made in the worker from PDFium's object list, and the removal is still the coordinator's
`lopdf` rewrite (§T6.11).

**Addressed by position, behind the guard text has.** The k-th path object PDFium enumerates
is taken to be the k-th path the content stream paints. `painted_paths` restates PDFium's rule
for which paths become objects, and the two counts agreed on all 1,755 fixture pages
(355,266 paths). A page where they disagree is not removed from by position: the planner
reports its paths as `unplaced-path`, and `remove_paths` refuses. A document built to make
the counts agree while the *order* differs would have the wrong path removed; PDFium
enumerates in content order, and no such document is known.

**A region with no text in it is checked at the page's size, and says so (2026-10-03).** The
OCR gate proves a region unreadable by reading back a control no larger than the smallest
text the removal took. A region over a drawing or a picture took no text, and until this
date that was a refusal: *not verified*, whatever was removed. The control is now sized from
the smallest word left on the page that is long enough to be read back, and a clean verdict
carries a note with that size. What this gives up: text set as outlines, or shown in a
picture, that is smaller than every word left on the page could survive a failed removal
unread, and the verdict would be clean. The note is the disclosure; a reader for whom that
matters uses the image-only copy. Regions that did hold text are judged exactly as before.

**What `verified` says about a drawing: nothing more than before.** The byte scan looks for
the words a removal took, and the OCR gate reads the rendered region for legible text. A
removed scribble is in neither. So `verified` on a region that held a drawing means the words
are gone and nothing legible is left there, which covers text set as outlines and does not
cover a signature's strokes. The removal of the strokes rests on the operators having been
deleted, which the unit and integration tests check on fixtures, not on a read-back of each
written file. Pictures have stood the same way since 2026-08-27.

**A straight rule or a rectangle that crosses the region is cut at its edge (2026-10-03).**
`pathcut.rs` splits it from its own operators: a filled rectangle loses the region, and a
horizontal or vertical stroke is cut where the region covers its whole thickness. The part
inside is not in the written content. This rests on a model of what a stroke inks (half the
width either side, half a width past a corner and past a round or projecting cap), which
errs outward, and on the graphics state read from the content stream. It is checked on
fixtures by unit tests and by rendering the written copy, not by a read-back of each written
file: the copy paints its own mark over the region, so the pixels there cannot say what is
under it. A wrong model would leave a sliver of a rule under the mark, not text.

**Residual.** A drawing that reaches beyond the region and is not such a rule or rectangle
stays whole, the part inside the region included, and the result is *not verified* with that
named as the reason: a curve, a dashed or hairline stroke, a shape filled and stroked
together, a line the region covers only part of the thickness of. The same holds for a
drawing that also sets the clip and for shadings. A reader who needs those gone uses the
image-only copy.

**Inside a Form XObject the same rules apply, one level down (2026-10-03).** Text, pictures
and drawings a page draws through a form are addressed by their place in the form's own
content, behind the same count guard: `remove_form_shows`, `remove_form_images` and
`take_form_paths` refuse when PDFium's count for that form and `lopdf`'s disagree. A cut
inside a form is worked out in the state the form's content starts in, which is the page's
at the `Do` with the form's `/Matrix` applied (`pathcut::drawings_in_form`). A form the
document draws more than once is not changed: its content is one stream, and editing it
would change places nobody marked. It is left and the result is *not verified*. A form
drawn inside a form is not followed and is reported the same way.

**A picture drawn in several places leaves the marked page and stays in the file
(2026-10-03).** Its draw on the marked page is removed. The image object stays while any
other page or form draws it, so its bytes are still in the written file, and a clean
verdict does not say otherwise: the report's `notes` and the review panel name the picture
and how many times the document draws it. This is the one case where `verified` is given
for a file that still holds the bytes of something a region covered, and it is given
because nothing the region covers is drawn on that page any more. A reader who needs the
picture out of the file marks it on every page that draws it; the last removal drops the
resource name and the sweep takes the object.

#### T6.30 — Setting and removing a password, added 2026-10-03

`tpdf protect` and `tpdf unprotect` write a copy with a new password or with none, and the
window's *Save a copy with a password* and *Save a copy without its password* do the same
through `protect_copy`, the thirteenth command that writes a file (§3). The write is
`save::write_copy`, the path every copy takes, with one field more in the plan
(`protect::Protection`).

**What a new password is.** AES-256 under the PDF 2.0 handler (`V 5`, `R 6`), built by
`lopdf`. One password is both the user and the owner password and no permission is
withheld, because a permission bit binds no reader. The 32-byte file key and the
identifier of a file that had none come from the operating system's generator
(`getrandom`), read inside the sandboxed worker, which needs no file or network authority
for it; `lopdf` draws its own salts the same way. Measured on macOS under the shipped
profile. **On Windows the low-integrity worker has not been run through this path.**

**Where the new password travels.** From the dialog's two fields to `protect_copy` as an
argument, into the plan, and over the worker's request pipe with the rest of the plan. It
is never on a command line: the tool reads it from an environment variable, as it reads
the one that opens the input. `Protection`'s `Debug` form prints no password, because a
plan is printed in refusals and in test output. It is not stored. A reader who loses it
has lost the copy, and the dialog and the README say so.

**What is checked before the copy has its name.** The writing worker loads the bytes it
built twice with `lopdf`: without a password, where a protected copy must stay locked and
an unprotected one must open with no trace of encryption, and with the new password,
where the page count must be the one written. That is the writer reading its own output.
The tool adds a reader that shares no code with it: the staged file is opened in fresh
PDFium workers before it is published, and `tests/cli/protect.rs` asks `qpdf` as a third.
The window has the first check only.

**Removal is refused for a document that opens without a password.** Such a file carries
restrictions somebody else chose, and `lopdf` authenticates it with the empty password by
itself, so a removal would drop them for a reader who was never asked for anything. The
rule is asked of the bytes, by loading them with no password, because a wrong password
given to such a document still arrives decrypted. tpdf itself does not enforce those
restrictions while the document is open; what it will not do is write a file without them.

**A defect this found, in what was already shipped.** `lopdf` leaves the key length out of
each crypt filter it writes. `qpdf` and PDFium assume it from the method; CoreGraphics does
not, so Preview took the password of any encrypted document tpdf had rewritten since
2026-08-28 and showed its pages blank. `protect::finish` now writes the length after every
`Document::encrypt`: the rewrite, the merge and the image-only redaction. Measured with
PDFKit before and after. PDFKit also refuses a password with a character outside ASCII,
on a file `qpdf` wrote as well as on tpdf's; that is PDFKit's, and the dialog warns of it.

#### T6.31 — A document made from pictures, added 2026-10-03

`tpdf images` and the window's *New document from pictures* write a PDF with one page for
each PNG or JPEG file, through `images_to_pdf`, the fourteenth command that writes a file
(§3). It is the first writer that starts from no document, and the second input after a
merge's that is a file tpdf has never opened.

**The pictures are hostile input and are decoded in a worker.** The coordinator reads the
files into one read-only mapping, as it does a merge's, and parses nothing. A worker is
started over a document, so this one is started over the warm-up document tpdf ships,
which the request never reads; the profile and the authority are every writing worker's.
`Request::Images` names where each picture is in the mapping, and the spans are checked
against it in the worker.

**What the decoders are given.** A PNG's size is read from its header before the `png`
crate is given any room, and the room is the pixels that size implies. A JPEG's header is
walked by hand for its size, component count, EXIF orientation and JFIF density; the walk
reads nothing past a segment's stated length, and an EXIF directory is read for one tag.
The picture is then decoded once by `zune-jpeg` in strict mode, bounded to the header's
size, and the result is thrown away: the bytes written are the file's own, under
`DCTDecode`. The decode refuses a file whose picture data runs out; it forgives a missing
end marker and a few missing bytes, measured on the 664-byte fixture. Limits are 40 megapixels and 30,000 pixels a side for a picture, 500 pictures
and a gigabyte in all for a document. A CMYK, lossless or arithmetic-coded JPEG is refused
by name.

**The JPEG's bytes are passed through, so they are parsed again by every reader of the
result.** That is what a PDF with a photograph in it is, and tpdf's own renderer is PDFium
in a worker. It also means everything else in the file goes with it: EXIF data, including
a camera's position, is in the document that is written. The README says so.

**What is checked.** The tool opens the staged document in a fresh PDFium worker and
publishes it only if it has one page for each picture; `tests/cli/images.rs` renders the
page and compares it with the picture pixel for pixel, and reads a turned photograph's
corner. The window's document is opened in the viewer, where the reader sees it.

#### T6.32 — A smaller copy, added 2026-10-03

`tpdf compress` and the window's *Save a smaller copy* write a copy made smaller, through
`compress_copy`, the fifteenth command that writes a file (§3). The write is
`save::write_copy`, the path every copy takes, with one field more in the plan
(`compress::Compress`). `compress_estimate` writes nothing.

**New decoding, in the worker.** Shrinking a picture decodes it: `FlateDecode` through
`lopdf`, bounded to the bytes the picture's own width, height and components imply, and
`DCTDecode` through `zune-jpeg` in strict mode, bounded to the same size and refused when
its component count is not the colour space's. A picture of more than 40 megapixels is not
decoded. The walk that finds where pictures are drawn follows Form XObjects eight deep and
stops after 200,000 `Do` operations, and each content stream is bounded as every other
decode is. Deflating a stream again inflates it first, to at most 64 MiB, and a stream
past that is left. The JPEG encoder is the `image` crate's, which was already in the tree
and now has a caller; it is given pixels this process decoded and writes into memory.

**The estimate opens a second document in the worker.** `run_shrink` serialises the
smaller copy and opens those bytes with PDFium beside the document the worker holds, to
draw one page of each. The bytes are tpdf's own serialisation of a document the worker has
already parsed with both engines, and the profile and limits are unchanged; what is new is
that a worker holds two documents for the length of one request.

**What reaches the webview.** Two PNGs of one part of one page, as `data:` URLs, and
numbers. They are shown in `img` elements and dropped when the dialog closes.

**A copy that loses pictures cannot be undone, so the source is never the target.** Both
routes write a copy, the tool refuses an output that names its input, and the window's
document stays the one that was opened. A signed document is asked about first, as for
every rewrite.

**What is checked.** The tool opens the staged copy in a fresh PDFium worker and publishes
it only when it has the source's pages, is encrypted exactly when the source was, and is
smaller. `tests/cli/compress.rs` renders a lossless copy and compares it with the source
pixel for pixel, and reads the text of a lossy one. Over 65 documents every lossless copy
rendered identically to its source (`docs/PLAN.md` *A smaller copy*). **Not checked:** how
a lossy copy looks is not judged by anything but the reader, which is what the preview is
for; a picture in a colour space this leaves alone is simply left, so a document of CMYK
or indexed pictures shrinks less than its size suggests and nothing says why.

### T7 — Distribution and update

**The threat.** A tampered download, a tampered update, or a compromised dependency —
including the PDFium source, toolchain and resulting binary.

**What stops it, and what does not yet.** The application ships through notarized
macOS distribution and signed updater payloads. PDFium is built by the read-only
`.github/workflows/pdfium.yml` from pinned source, dependency and packaging-patch
revisions. Since `pdfium-8066-tpdf.1` (2026-09-25) the source is built unpatched: upstream
fixed the RTL regression TPDF used to patch, so there is one engine per platform rather than
a control and a candidate. Each must reproduce the exact RTL observation
`scripts/pdfium_verify.py` pins — every ordinary fixture's authored text, and the two known
limitations' exact wrong text — and pass upstream's text tests before an archive is emitted.
Archives carry source/toolchain provenance and licences; `scripts/fetch_pdfium.py` pins their
SHA-256 before extraction. The notices gate checks permissive licensing, and
`.github/workflows/audit.yml` checks the Rust and npm dependency trees against the advisory
databases on every push and weekly, failing on any advisory `.cargo/audit.toml` does not
list with its reason. Build artifacts do not publish themselves or update the production pin.

These checks do not establish that a compiler or dependency is uncompromised, nor
prove general PDFium correctness. Host SDK/CRT versions are recorded rather than
hermetically supplied. Compatibility probes are rerun when the pin changes; the
current source-build evidence and remaining RTL limitations are in `BUILD.md`.

**Tested 2026-08-03, and this read "Untested" until then.** The signing and notarization
path for a bundled dylib (§10 q7) was the open question here, and it bit `screenpick`'s
release path before. It is answered: the `.app` notarizes `Accepted`, the DMG notarizes and
staples, and both the app and `libpdfium.dylib` carry a Developer ID Application signature
chaining to Apple Root CA with the hardened runtime. Confirmed from **outside** the workflow
as well as by it — the DMG was downloaded from the draft and checked on a machine that had
not built it, where `spctl -a -t open` reports `source=Notarized Developer ID` and the
stapled ticket validates. That distinction is not pedantry here: on the run before, every
one of those properties held while the workflow's own verification step failed for a reason
of its own, so the workflow's verdict and the artifact's state are separate facts.

What is **still** untested is distribution over time rather than at the moment of release:
nothing re-checks that a published artifact still validates once the signing certificate
expires (2031-07-26) or if it were revoked. **There is an update channel to carry a fix as
of 26.8.2** — see §T9, which is where the residual for it lives; this paragraph read "there
is no update channel ... since tpdf ships no updater" until then.

#### T6.33 — Adding form fields, added 2026-10-03

`tpdf form` and the window's *Add a form field* commands write new AcroForm fields: a text
field, a text field on several lines, a checkbox, or a dropdown with its list of choices
(at most 1,000, each at most 255 characters, checked by `formfields::options_problem`). No new process or authority: the
fields are part of the plan the coordinator's `lopdf` rewrite already carries
(`formfields::place`). A field's name is checked before anything is written
(`formfields::name_problem`: not empty, no space at either end, no period, no control
character, at most 255 characters; and not a name the document or the same call already
uses), one call adds at most 1,000, and a box
smaller than the minimum is refused. A field on a page that is turned is refused rather
than placed wrongly. What is written is a widget with a default appearance string and the
standard Helvetica resource; no script, action or calculation is ever attached. A document
that carries a digital signature gets the same warning before the write as any other
change.

**Changing fields the document already has (2026-10-04).** `formedit::apply` moves or
resizes a widget, renames a field, or removes a widget, on the same rewrite path and with
no new process or authority. Every change is checked against the scanned form before the
first is made: the widget must exist, a name must be one a field may have and one no
field beside it will have, and a rectangle must fit its page and the least size of its
kind. A signature field is refused. A removed field is unlinked from its page, the field
tree and the calculation order, and the rewrite's sweep then takes its objects, its
answer included; `save/tests.rs` looks for the removed field in the written file. Nothing
is added to a field: no script, action or calculation is written, and one the field
already carries is neither read nor changed.

**A field's properties (2026-10-04).** The same call writes a field's tooltip (`/TU`),
its required and read-only flags (`/Ff` bits 2 and 1), a text field's `/MaxLen`, its
`/Q` and a choice field's `/Opt`. Each is a string, a number or a list of strings on the
field's own dictionary. The strings are a reader's: a tooltip is held to 1024 characters
with no control character but a line break, a choice to the rules a placed dropdown has,
and both are written as PDF text strings, never into a content stream. A choice's label
does reach the appearance stream when the field is redrawn, through the same hex-string
path an answer takes, which admits only characters of the font's encoding. The scan now
also reads `/TU` (held to 16 KB before decoding) and `/Q`, and the window shows the
tooltip through an element's `title`, which is text. A field placed in the session
carries the same parts in the journal (`annot_field_props`), held to the same bounds
when they are set and again when the save writes the field.

**Radio buttons (2026-10-04).** A button's value becomes a PDF name, the key of its
appearance state, and later the group's `/V`. It is a reader's text held to the rules of
a dropdown's choice (255 characters, no control character, not `Off`), and `lopdf`
writes a name with every character outside the regular set escaped; a test writes a
value with a space and reads the file back. The appearances are fixed drawings of a
ring and a dot, with no text in them.

#### T6.34 — Reopening every tab at launch, added 2026-10-03

Off unless the reader turns it on. When on, the session record holds the paths of the
documents open as tabs and which one was showing, beside the reading places it already
held for recent documents; it is the same local file, and it holds paths and no content.
At launch each path is opened through the ordinary open path, so each document is parsed
in a worker like any other, and one that will not open does not stop the others. The
record has no field for a password, so a protected document asks for it again.

#### T6.35 — The blank window lists recent documents, added 2026-10-05

The window with no document open shows the newest eight reading places of the session
record: file name, folder and page. The rows are drawn from the record alone. No file is
opened, read or looked for to draw them, so a path in the record reaches no parser until
the reader picks its row, and then through the ordinary open path. Paths are written
into the page as text.

The webview can now remove from the record as well as add to it: `session_forget` drops
the place of one path, and `session_clear_places` drops every place. Both compare the
path as a string and open nothing; neither can change the preferences or the list of
open tabs, and neither can write anywhere but the session file. `session_load` now also
answers the home folder's path, which the page uses to write a folder under it as `~`.
The webview already held absolute paths under that folder; the home path is not written
to the session file.

#### T6.36 — A text box's words on the page while they are typed, added 2026-10-05

The page draws a text box's words, and a placed field's name, as the reader types them
in the box beside the mark. The model still hears the note once, from `annot_note` when
that box closes (§T6.4), so nothing about what is journalled or saved has changed.

One command is new, `annot_draft_lines`. It takes a string and two numbers, the left
and right edge of the box, and answers the lines `textbox.rs` would break the string
into. It names no document, takes no lock, stores nothing, opens no file and reaches no
worker. The string is refused past `textbox::MAX_NOTE_CHARS` by `edits::too_long`, the
function that bounds a note, and the wrap is linear in its length. The numbers decide a
width that is floored at one point, so no width makes the wrap run without end.

The string is what is in the note field. That is the reader's typing, or the note the
mark already had, which for a mark read back from a saved file is the file's (§T8). It
reaches the page as the note itself does: drawn on the overlay canvas with `fillText`,
never assigned to the DOM.

#### T6.37 — The language text is recognised in, added 2026-10-05

*Recognise text: language* lets the reader name one language for the recogniser, or leave
the choice to it. Two commands are new and one takes a new argument.

`ocr_languages` takes nothing and answers the language tags this machine's recogniser
offers, with whether more can be installed. It names no document, takes no lock, opens no
file and reaches no worker. It is the one question put to the recogniser **in the app
process**: on macOS `VNRecognizeTextRequest.supportedRecognitionLanguages`, on Windows
`OcrEngine::AvailableRecognizerLanguages`. No image and no document goes with it, so
nothing a file supplied is processed outside the OCR worker (§5.1); recognition itself
still happens only there. Measured 2026-10-05 on macOS 26A434: 33 tags in 15 ms. The
Windows call is the one the OCR worker already made at start; it is compiled for the app
process by `scripts/check_windows.py` and has not been run there.

`session_set_ocr_language` takes a tag or nothing and stores it in the session file beside
the other preferences, through the same lock. A value that is not shaped like a language
tag (`ocr_layer::is_language_tag`: 2 to 35 letters, digits and hyphens, starting with a
letter) is stored as nothing, and the same rule is applied when the file is read, because
the file is on disk where anything can edit it.

`ocr_copy` takes the tag as `language`, and `ocr_layer::choose` holds it against what the
machine offers at that moment: a tag that is offered goes to the engine in the engine's own
spelling, and one that is not offered goes nowhere, whatever it is. The engine then chooses, and the reply names the tag so the window can say it.
So the only strings that reach `setRecognitionLanguages:` or `Language::CreateLanguage`
from the window are ones the platform itself listed.

A compromised webview can therefore set a preference and make a recognition read in a
language the reader did not choose, which makes the recognised text worse. It gains no
file, no path and no process.

#### T6.38 — Asking whether the command-line tool is installed, added 2026-10-06

*Install command-line tool…* and *Uninstall command-line tool…* are each greyed when they
have nothing to do, so the window has to know what is installed. One command is new,
`command_line_tool_state`, and it is the first request about the tool that changes nothing.

It takes no argument. On macOS it reads what the two fixed paths `/usr/local/bin/tpdf` and
`/usr/local/bin/tpdf-cli` hold --- `symlink_metadata` and `read_link` on each, through
`clitool::plan`, the same reading the two commands decide by --- and compares a link's
target with the tool beside the running executable. On Windows it reads the user's own
`PATH` (`HKCU\Environment\Path`) through `userpath::stored` and looks for the folder of the
running executable in it. Since this change that value is read through a handle opened for
`KEY_QUERY_VALUE` alone, and written, by the two commands only, through a second handle
opened for `KEY_SET_VALUE`. Elsewhere it reads nothing. It names no document, opens no
file's contents, reaches no worker and starts no process.

The answer is two booleans, or nothing where there is nothing to read: whether this copy's
tool is what a terminal gets, and whether either path holds anything (on Windows both are
whether the folder is on the `PATH`). No path, link target or `PATH` entry crosses to the
webview. The window asks once after launch, after either command finishes, and each time it
comes to the front.

**The answer decides nothing.** `command_line_tool` reads the filesystem or the `PATH` again
when it runs and acts on what it finds then (§T6.23), and nothing the webview holds is passed
to it. A compromised webview could ask this as often as it likes and learn the two booleans;
it could also ignore the answer and offer a greyed command, which then runs exactly as it
did before this change. A failed read, or one that has not answered, greys nothing.

**Measured, and not.** The reading over the two paths is held by
`clitool::tests::the_state_is_read_from_both_links_and_reading_changes_neither`, which lists
a scratch folder before and after every read, and by
`a_path_that_cannot_be_read_greys_neither_command`; the real links on the machine this was
written on read as installed (`docs/VERIFICATION.md`, *The command-line tool's two commands
are greyed by what is installed*). That a Windows read writes nothing is
`userpath::tests::asking_whether_a_folder_is_stored_writes_nothing`, which compares the
registry key's last-write time across the read, and
`the_handle_a_reading_holds_cannot_write`; the same record says where they ran. The command
has not been driven from a window on Windows.

#### T6.39 — `tpdf hidden`, text a page does not show, added 2026-10-07

A command of the command-line tool that reads a document and writes nothing. It adds no
authority and no parser. For each selected page it asks the worker session every reading
command already opens for two answers that existed before it, `Request::Text` and
`Request::Tile`, and compares them in the tool's own process (`hidden.rs`): the boxes of the
characters against the pixels of the rendered page. The tool's process handles a list of
numbers and a pixel buffer whose size it checks; it does not read the document. The one
bound that is new is on the image, at most 8,192 pixels a side and 16,777,216 in all, which
is `tpdf render`'s, and a page that does not fit at one pixel a point is reported as not
compared.

What the command prints is text from the document. The plain output goes through `say`,
which is what keeps a control sequence in a document from reaching a terminal, and `--json`
through the one writer every report uses.

**The claim is one-sided, and that is the security property.** A passage the command lists
is in the file and is not shown by the page. Listing nothing is not a statement that the
document is clean: a page without text is not compared, so a scan with black bars drawn
over it passes unseen; a cover that is not one flat colour hides its text from the
comparison; and comments, form values, attachments, metadata and earlier revisions are
outside it. The report carries `without_text`, `not_compared` and `unjudged`, and the plain
output's last sentence says them beside the result. The exit code is 1 on a finding and 0
otherwise; 0 means "nothing found", and `README.md` words it so. This is a different claim
from `tpdf redact`'s `verified`, which is about a file tpdf wrote and names every carrier
it read back.

**In the window since 2026-10-08**, as *Find text the pages do not show*. It is the same
walk (`hidden/survey.rs`), with the render service answering for a page where the tool asks
a worker session of its own, so the document is parsed wherever the viewer's own rendering
parses it and nowhere else. Two commands. `hidden_text` takes the document's handle and a
run number and answers the passages, the walk's last sentence and whether the journal holds
changes that are not saved; `hidden_text_cancel` takes a run number. Neither takes a path
and neither writes a file. The comparison runs in the application process on what the
service answered: character boxes, and pixels that are refused tile by tile when a tile is
not the size that was asked for. The words of a passage are document text that reaches the
webview, where `hiddenlist.ts` sets them as `textContent`. The file is what is checked,
not the journal, and the reply says when the two differ. Not measured: no probe reads the
application's image table while a check runs; `backend-probe` does that for the render
path the check goes through.

#### T6.40 — Taking part of a line, added 2026-10-07

A redaction used to delete the whole show operator that drew any glyph under a region.
It now cuts those glyphs out of the operator where it can prove which they are, and
leaves a gap as wide as they were (`redact/glyph_cut.rs`; `docs/PLAN.md` §6, *Route A, for
the case it can be proved in*). This is a rewrite of the bytes a redaction exists to
remove, so what it rests on is stated here.

**What would leak, and what stops it.**

- *The wrong glyphs are cut and the marked ones stay.* A glyph is addressed by its place
  among the codes of a string, and that place is PDFium's count against `lopdf`'s. The cut
  is made only when both counts are equal, and PDFium's characters at one pen position are
  one glyph, so it never has more glyphs than codes. Then the written file is searched for
  every string a removal took, as before: marked text that stayed is found and the copy is
  reported as not verified.
- *A marked glyph is called outside the region.* A glyph goes when the region covers more
  than a tenth of its box. One covered a tenth or less stays, and is at least nine tenths
  visible beside the fill. A glyph PDFium gives no box for cannot be placed, so a show
  with one that is not white space is not cut and goes whole.
- *A copy of the line elsewhere keeps the words.* A bookmark or a form answer that repeats
  a line is compared with the whole line a cut came out of, not with the glyphs that went,
  and the alternate text of every marked-content span around a cut show is cleared.

**What is not covered.** Text inside a Form XObject is not cut; it goes by the whole
operator. The position of what stays is measured (a twentieth of a point, on eight ways of
writing a line and on one real invoice) and is not checked on each save: a gap of the
wrong width would move the rest of a line and leak nothing. A font whose glyph for a code
changes with its neighbours would draw the kept glyphs differently once a neighbour is
gone; none was met and nothing looks for one.

### T9 — The updater

**The threat.** The updater is the only code path in tpdf that fetches bytes and then
*executes* them, and it is the highest-authority feature the application has. A compromised
endpoint, a downgrade to a version with a known defect, or an unsigned payload accepted by
mistake, each ends with attacker-chosen code running as the user — outside every boundary
the rest of this document builds, because the replacement binary *is* the boundary.

It also changes a property that held until 26.8.2: **tpdf made no network request at all.**
That was worth something and it is now spent. It is spent narrowly, and the narrowness is
the mitigation rather than a footnote — see below.

**Web links, from 2026-09-07, are a second thing that causes traffic, and they are not a
second network authority — the distinction is worth stating rather than blurring.** tpdf
opens no socket for them: `opener.rs` hands the address to `NSWorkspace openURL:` or
`ShellExecuteW`, and the request is made by the reader's browser, in the browser's own
process, with the browser's own sandbox and cookie jar. So this section's inventory of what
*this application* speaks to is unchanged — one endpoint, at most one automatic check per launch — and
what changed is that tpdf can now cause a request somewhere else. **That inventory changed on
2026-09-28**: a signing the reader asks to be timestamped speaks to the authority they chose,
once per attempt, from the coordinator --- a second network authority, with its own section,
§T10. The three things bounding
that are in §T8: the scheme allowlist, the per-link confirmation, and the token that keeps a
caller to addresses the document already held.

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
  By default, `checkOnLaunch()` checks once. The tpdf menu on macOS and command palette can
  disable it across launches; manual checking remains available. A missing preference
  keeps the default, while an unreadable or malformed preference skips the check.
  Changing this setting starts no request and cannot cancel one already running.
  The preference is one non-sensitive boolean in WebView localStorage, independent
  of the reading-session file. Downloading and applying require the reader to click.
  `update.ts` carries why this is not silent.
- **A failed check is reported and forgotten.** Nothing retries, so an endpoint that is
  hostile, slow or absent cannot turn into a loop that keeps dialling out.

**Restarting is a new authority the webview holds as of 26.9.17, and it is narrower than it
sounds.** Finishing an update on macOS needs a relaunch — the plugin replaces the `.app` on
disk and leaves the running process on the old code — so `tauri-plugin-process` is linked and
the webview can ask for one. Four things bound it, and the first is the one that decides the
shape:

- **It chooses nothing.** `AppHandle::request_restart` ends the event loop and
  `tauri::process::restart` starts `current_binary()` again — on macOS by reading
  `CFBundleExecutable` out of the bundle's own `Info.plist`. No path, argument or command
  crosses the boundary, so the whole of the authority is *this application, again*.
- **Only `process:allow-restart` is granted**, not `process:default`, which would also hand
  over `exit`. Restarting reopens the reader's session; quitting is a different act and
  nothing here needs it.
- **No network, no filesystem, nothing unpacked.** This is not a second updater. It adds one
  crate, which is two commands over `AppHandle`.
- **It is reachable only from the applied state.** The command's guard is `updateReady()` and
  `update.ts`'s `finishUpdate` refuses any state but `ready`, so a press that arrives one
  frame behind the state does nothing rather than ending the process with nothing installed.

**And the question a restart must not skip is asked in the webview, which is where the
residual is.** `request_restart` does **not** close the window: it sends `ExitRequested` with
its own exit code straight at the run-event loop, so the `onCloseRequested` handler that
counts dirty tabs and asks before discarding them never runs. `finishUpdate` therefore asks
that question itself, in the same words the close dialog uses, before it calls `relaunch()` —
and the same call gates the Windows *install*, which ends the process just as surely
(`Update::install_inner` there hands over to the installer and calls `exit(0)`).

**Residual, and there are six.**

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
4. **The unsaved-work question before a restart lives on the webview's side of the
   boundary.** The coordinator does not know whether a tab is dirty — the edit models are
   Rust state, but the settle that makes `dirty` current runs in the frontend — so
   `finishUpdate` asks and `relaunch()` obeys. A webview compromised through T8 could
   therefore end the process without the reader answering, losing unsaved edits. That is the
   same authority a compromised webview already has through `core:window:allow-destroy`, and
   it is a denial rather than a disclosure: the restart runs the binary already installed at
   the application's own path, and the reading position and recovery record are written
   continuously rather than at exit. Moving the question into Rust would mean moving the
   settle with it, which is the frontend's job for reasons that have nothing to do with
   updates.
5. **Untested against a real endpoint.** `update.test.ts` fakes the plugin, and the tests
   there cover the state machine rather than signature verification, TLS, or the real
   `latest.json`. The first genuine end-to-end proof is the first update applied from one
   published release to the next, and `BUILD.md` schedules it as a manual step because it
   cannot exist until two signed releases do. **Nothing below claims otherwise.**
6. **The Windows signing login is held by the release workflow and read by a program that is
   not Certum's**, since 2026-10-07. The Authenticode key is in Certum's SimplySign service
   and cannot be exported; what the workflow holds is the account's e-mail address and the
   seed its one-time codes are computed from, and those two are enough to sign any file as
   *Open Source Developer Timo Stein* until the certificate is revoked or expires on
   2027-10-07. They are secrets of the GitHub environment `signing`, which accepts the
   `main` branch and `v*` tags; `release.yml` and `sign-rehearsal.yml` are the two workflows
   that name it, and neither runs for a pull request. The program that reads them is
   `ssign`, built from one pinned commit of its source: 2,800 lines read on
   2026-10-07, in which the only hosts are `cloudsign.webnotarius.pl` and `time.certum.pl`.
   Its dependencies are not pinned by anything here beyond its own lock file and were not
   read. Since 2026-10-08 the built program is kept in a GitHub Actions cache whose key is
   that commit, and a job builds it only when the cache has none. A release runs on a tag
   and reads a cache saved by a run on `main` or by itself, so the copy that reads the
   login is one a run on `main` built; a pull request from a fork cannot write there.
   Somebody who can change a workflow on `main` can replace it. So could code that merely
   runs in a job on `main` without being able to change anything, such as a dependency that
   `ci.yml` installs: saving a cache needs no more than that. Since 2026-10-09 a restored
   copy is therefore used only when its SHA-256 is the one recorded in the workflow, and the
   release job restores no Rust build cache at all (`BUILD.md`, *Signing with the Certum
   certificate*). Whether a step on today's hosted runners can in fact save a cache under a
   key of its choosing was not tested; the check does not depend on the answer. This is a second key beside the updater's and it protects a different thing: an
   installed copy accepts an update by the minisign key alone, so a stolen Certum login
   lets somebody sign their own program under this name and does not let them update
   anybody's tpdf. The protocol is reverse-engineered, so Certum can end it without notice;
   the cost of that is a release that fails to build, not one that ships unsigned, because
   the Windows leg reads every signature back.

### T10 — Asking a timestamp authority, added 2026-09-28

**What changed.** A signing can carry an RFC 3161 timestamp (PAdES B-T): after the OS has made
the signature, tpdf asks a timestamp authority for a token over it and writes the token into the
signature as an unsigned attribute (`tsa.rs`, `sign_cms::Made`; `docs/PLAN.md` §9, *Adding a
timestamp when signing*). It is the application's **second network authority** beside the
updater (§T9), and the first that sends anything derived from a document.

**When a request is made.** Only when the reader asks, for one signing: a server picked in the
*Sign document* chooser --- none is preselected, and the reader's choice is remembered in the
webview's `localStorage` as a per-viewer convenience --- or `tpdf sign --timestamp`. One request
per attempt, made after the OS has signed and before anything is written; another only when the
reader presses *Try again*. Nothing is asked on launch, on open, on verify, or by a worker. Every
gate and test is offline: the tests' authority is a fake on 127.0.0.1.

**Who makes it.** The coordinator, on a blocking thread, or the command-line tool's process ---
**never a worker**, whose profile keeps `(deny network*)`, and **never the webview**, which gains
no capability and whose CSP is unchanged (`connect-src 'self' tile: http://tile.localhost ipc:
http://ipc.localhost`). The webview hands `sign_document` a string; the coordinator parses it with
the `url` crate and asks only `http` and `https`, with a host and without a user name or
password, before anything else --- the same judgement `--timestamp` gets at parse time, exit 2.
Redirects are not followed. The system's proxy settings apply, as for the updater.

**What is sent.** An HTTP `POST` of `application/timestamp-query`: a `TimeStampReq` carrying
SHA-256 of **the new signature's value octets** (RFC 3161 Appendix A), a fresh 128-bit nonce from
the OS random source (`ring` through `rustls`'s provider), and `certReq TRUE`. Nothing of the
document, its name, the signer's certificate or the reader's identity. Around it: the reader's
IP address, the authority's host name in a DNS query, and `reqwest`'s default headers --- tpdf
sets no `User-Agent`. `tsa::tests::the_request_is_the_der_rfc_3161_describes` pins the bytes, and
`openssl ts -query -text` reads the same vector as version 1, sha256, the nonce, certificate
required.

**What is accepted, parsed where.** The answer is attacker-chosen bytes parsed **in the
coordinator** --- the one parse in this feature the worker boundary does not cover, and the
reason it is narrow: at most 64 KiB is read (a longer answer is refused, whatever
`Content-Length` said), a connect limit of 10 s and a total of 30 s, and only `der`, which is
memory-safe and refuses non-canonical encodings, reads it. The token's public-key checks are
`integrity.rs`'s pure-Rust ones. `tsa::accept` then refuses unless the status is granted, the
token's verdict under increment A's reader is `intact` (not `weak`), its imprint is SHA-256 of
this value, and its nonce is the one sent. `Made::seal` checks the token again in the finished
bytes. Only then is anything written.

**What an attacker can do, over `http://`.** Two of the three listed authorities serve nothing
else, so this is the common case:

- **Read the request**: that somebody at this IP address signed something at that moment, and a
  hash of the signature value. The hash identifies the signature to anybody who later has the
  signed file; it reveals nothing of the document to somebody who does not.
- **Deny service**: drop, delay or garble the answer. The reader is told, nothing is written, and
  the signature already made can be written without a timestamp as an explicit second choice.
- **Replay an earlier answer**: refused, because its nonce is not this request's.
- **Forge a token from the chosen authority**: not possible without that authority's key --- the
  token is a signature, checked before anything is written.
- **Substitute a token from an authority of its own**: possible, and written. The attacker sees
  the imprint and the nonce in the request, so it can mint a sound token over both with its own
  key; tpdf's checks before writing are of the token's arithmetic, not of who its authority is.
  **What stops it being mistaken for the chosen one** is that the authority's standing is part of
  every answer: the closing sentence after signing names the authority and says whether this
  computer trusts it, the properties dialog and `tpdf verify` do the same, and a substituted
  authority reads *not trusted*. That is the plain B-T signing; a signing that also asks for
  long-term data is refused at this point, before anything is fetched (below). Residual 29.

**What a malicious authority can do** is what any trusted authority can: state any time for any
imprint it is sent (residual 28). It cannot make tpdf write more than the reserved span
(`Made::stamped` refuses past `RESERVED`), cannot change a byte of what the key signed (the token
is an unsigned attribute, and `stamping_changes_nothing_the_key_signed` holds that byte for
byte), and cannot keep a signing waiting past the total limit.

**What is kept while the reader decides.** When the request fails, the made signature --- the
whole file as read, up to `save::APPEND_MAX_BYTES`, and the CMS --- is held in the coordinator's
memory (`commands::sign::Pending`), one at a time, reached only by the number it was handed out
with, and dropped by *Cancel*, by the next signing, or at exit. It holds no key: the OS made the
value already, which is why neither *Try again* nor *Sign without a timestamp* asks the OS again.

**Measured**, macOS arm64, 2026-09-28: DigiCert, Sectigo and GlobalSign each granted a token
through this path, read `intact`, attested and `trusted` by tpdf, `intact` and `valid` by pyHanko,
and *Verification: OK* by `openssl ts -verify` over the value octets (`docs/PLAN.md` §9).

**Residual.** Residual 29. And the coordinator now parses bytes from the network: bounded as
above, in `der` and pure-Rust public-key code, which is the smallest parse that can accept a
token --- the worker cannot make the request, and sending the answer to a worker to parse would
add a process round trip for the one parse whose bytes the coordinator must itself splice.

#### Revocation data fetched while signing, added 2026-09-28

**What changed.** A timestamped signing can also carry long-term validation data (PAdES B-LT):
after the signature is sealed, the coordinator (or the command-line tool) asks the certificate
authorities about the signer's certificate, the timestamp authority's, and every certificate
above either that is not a root, and a worker appends what they answered, with the
certificates, as a `/DSS` revision (`longterm.rs`, `sign_dss.rs`; `docs/PLAN.md` §9, *Long-term
validation data when signing*). The same network authority as the timestamp, widened to the
certificate authorities the certificates name --- not a third one with its own switch: it is
asked only when the reader ticks the box, or gives `--long-term`, and only together with a
timestamp. **Reading and verifying still never go online** (§T6.25).

**Who is asked, and what is sent.** For each certificate, the OCSP responders its
`authorityInfoAccess` names, then --- only when no responder gave an answer --- the revocation
lists its `cRLDistributionPoints` names; `http` and `https` only, judged by the `url` crate with
no credentials, and every other scheme (`ldap:` above all) skipped, never asked. An OCSP
request is a `POST` of one `CertID`: SHA-1 hashes of the issuer's name and key, and **the
certificate's serial number**. A list is a plain `GET`. So each authority learns that **a
certificate it issued is being used, at this moment, from this IP address** --- for the signer's
CA, that its holder is signing now. Nothing of the document, the signature or the reader's
other certificates is sent. Most public responders and list servers are plain HTTP (DigiCert,
Sectigo and GlobalSign all serve their timestamping certificates' data over `http://`), so an
observer on the path learns the same. No nonce is sent (the public responders serve
pre-produced answers and ignore one); freshness is the response's own dates.

**What is accepted, parsed where.** Answers are parsed **in the coordinator**, like the token:
each read under a bound before it is parsed --- 64 KiB for an OCSP answer, 4 MiB for a list and
for everything gathered together --- with a connect limit of 10 s and a total of 30 s per request,
at most 16 requests and 90 s for the whole gathering, no redirect. Each answer is judged by
increment C1's own reader (`revocation::judge`, `der`, `x509-cert`, `x509-ocsp`, the signature
arithmetic `integrity.rs` uses) before it is kept: signed by the issuer or a responder it
authorised, about this certificate, fresh. `good` is kept; **`revoked` refuses the signing
outright** --- nothing is written, and nothing is held that could be written without the data;
`unknown`, and an answer that does not check out, refuse it too. A responder that gives no
answer (a transport failure, an HTTP error, `tryLater`, bytes that are not a response) is the
only thing the list is asked after: an answer that fails its checks is never replaced by
shopping for another from the same CA over the same path.

**Only for an authority this computer trusts, since 26.9.22.** Before anything is fetched, the
timestamp authority's certificate must chain to a root the OS store trusts for timestamping,
now and offline (`longterm::vouched`: `trust::of_blob_for` with `Purpose::Timestamping`, the
reader's own rule). Otherwise the signing is refused --- *the timestamp authority X is not
trusted by this computer, so tpdf will not fetch revocation data for it* --- with nothing
written and nothing asked, and the window holds the signature as for any other long-term
refusal (*Try again*, *Sign without long-term data*). The signer's certificate needs no such
gate: it comes from the reader's own keychain or certificate store, not from the network. The
gate is where certificates from the network are first handed to the OS in the coordinator:
the token's set, re-encoded, at most 16 of them each under 64 KiB, as the worker hands a
document's (§T6.22); the offline chain the gathering then asks the OS for is held to the same
bounds (`longterm::os_chain`).

**Who writes, and what is checked before writing.** A worker, spawned over a snapshot of the
signed bytes that are not written yet, builds the `/DSS` revision with `lopdf` and reads the
result with `docinfo::scan`; the coordinator writes nothing unless that reading says the new
signature is intact, its timestamp intact, and the signer's and the authority's revocation
`good`. The existing read-back after writing then runs as before.

**What an attacker can do.** On the path: read which certificates are being checked (above);
deny service (nothing is written, and the reader may sign without the data); **replay an
earlier response** still inside its validity window --- one signed before a revocation, saying
`good` (residual 33). **Choose the addresses asked, no longer**: the addresses come from the
certificates, and the timestamp authority's certificates come from the network. Until 26.9.22
an attacker on an `http://` path to the authority could substitute a token from an authority of
its own (above) whose certificates named any `http` or `https` address --- loopback, the
reader's LAN --- and the gathering would send up to 16 requests there from the reader's machine,
reading back only whether an answer checked out: a blind request forgery. The gate above closes
it: fetching happens only for an authority whose chain the OS trusts, and **a trusted chain is
what makes those addresses the certificate authority's**. **That held only for the token's
signer until 2026-10-05.** The gathering walked every certificate the token carries, and a CMS
`certificates` set is outside the signature, so a certificate added to a genuine token --- with
the issuing authority's name and key, an issuer of the attacker's own and addresses of their
choosing --- was walked and asked about (measured: SecTrust still vouched for the authority with
the twin present, and the twin's address was fetched). Now every link above the authority's
certificate must be on the chain the OS vouched for, and every link above the signer's on the
signature's own set and what the OS builds from it; anything else is refused before any fetch,
and the token's certificates are no longer handed to the OS for the signer's chain. Loopback and private addresses are
still allowed, on purpose: a company's own PKI publishes its responder on its network, and a
CA the store trusts is trusted to name its own. Not: forge a response, which is signed by the issuer or a responder it
authorised and checked twice, here and in the worker's reading. A malicious or compromised CA
can answer `good` for a revoked certificate, as it can for every relying party.

**Residual.** Residuals 32 and 33. And the coordinator parses more network bytes than for the
token alone, with the same bound-then-parse rule and the same memory-safe parsers.

**The archive timestamp, added the same day (PAdES B-LTA).** Every long-term signing now ends
with one more request to **the same timestamp authority the reader chose** --- never a second,
unchosen one --- for a token over the whole file, validation data included (`longterm::archived`,
`tsa::ask_over_range`). What it learns is what the first request told it: a SHA-256 and a nonce,
nothing of the document. The token is judged by increment B's rules over the covered range
before it is kept, and the sealed file is read back before it is returned; a worker builds the
revision over a snapshot of bytes not yet written, as for the `/DSS`. **No new authority, no
new parser**: one request more under the same limits, and a refusal writes nothing, like any
other long-term failure.

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
— `marklist.ts` assigns it through `textContent` like everything else, and the invariant
below is what makes that sufficient rather than a promise about this one call site.

**A fifth source the same day, and it is the widest: the properties dialog.** A document's
`/Info` strings, its custom keys — where the *label* is attacker-chosen too — and a
signature's stated name, reason and location all reach the DOM through
`propertiesdialog.ts`. §T6.8 is the worked-out version and is not repeated here; what
matters at this level is that it needed no new mechanism, because the invariant below is a
property of the frontend rather than a promise about each call site.

**What does *not* belong on that list, checked rather than assumed: the sentence a document
that will not open now shows.** `progressive::open_failure` returns one of five literals we
wrote, chosen by PDFium's error code and carrying no byte of the document, and `refuse`
answers every later request with that same string. It is the same shape as
`outline::Target::Refused`, and it is worth stating because the obvious next edit — naming
the file, or passing PDFium's own message through — would put attacker-chosen text on a
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
81 files, 36,029 lines and 59 calls — the frontend had grown by more than half and the
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

The backend half is enforced by the type. `outline.rs` refuses `/Launch` and `/GoToR` into
`Target::Refused { action }`, whose string is one of five literals chosen in that file;
`no_target_variant_may_carry_a_url` matches `Target` exhaustively, so adding a URL-bearing
variant is `error[E0004]` rather than a test failure. What is *not* enforced is the link
between the two halves — see residual risk 7.

**`/URI` is the one that changed, on 2026-09-07, and it changed the shape of this section's
argument rather than merely adding a case.** Web links are followed now (`docs/PLAN.md` §11),
so a document-written string *does* cross into the webview: `Target::Web` carries a `host` and
a `rest`. Until then the sentence above could say no document-derived URL crossed at all, and
that sentence is retired.

What replaces it is narrower and is stated as three separate facts, because each fails
differently:

1. **What crosses is not an address.** `Target::Web` carries `token`, `host` and `rest`. The
   URL itself never leaves the app process: `links::Links::urls` holds it, `document_links`
   **drains** that list into `webopen::Registry` before the reply is serialised, and
   `open_web_link` takes a token and looks it up. So the widest thing a compromised webview
   can ask for is *an address a document open in this process already contained*, which an
   attacker who wrote that document had anyway. Since a page can be inserted from another
   file (§T6.20) "a document" is more than the one the reader opened, and it has been since
   that landed: the imported file is scanned through its own handle and adopted under it, so
   its addresses were reachable before 26.9.16 made the reader able to reach one. What that
   release changed is which of them a *click* reaches — the target now carries the handle its
   token was numbered by — not what the command can be asked for. That matters more than it does for the other commands because a
   URL is an outbound request to a host of the caller's choosing — an exfiltration channel
   that printing and saving are not.
2. **What crosses cannot be rendered as anything but text.** The sink argument above is
   unchanged and now covers one more route: `check_webview_sinks.py` gained a rule for calls
   that navigate without an element — `window.open`, `location.assign`, `location.replace` —
   which every earlier rule was blind to because they are neither an attribute nor a property
   assignment. Proved to fire by planting a `window.open` and reading the exit code.
3. **The two halves are linked, for this arm.** The gap admitted above — a Rust change cannot
   turn the frontend check red — is closed for the one arm where it now matters: the same
   gate reads `src/lib/outline.ts`'s `Target` union and fails if the `web` arm declares a
   field named `url`, `uri`, `href` or `address`, **and** fails if the arm cannot be found at
   all. Both directions were proved by mutation. The rest of residual risk 7 stands.

**What is not enforced anywhere is the confirmation.** `weblinkdialog.ts` asks before
`open_web_link` is called, and a script running in the webview can call the command directly
without showing anybody a dialog. That is residual risk 7's shape rather than a new hole — the
same script can already print and save — and it is why the scheme allowlist lives in
`weburl.rs`, on the backend side of the boundary, rather than beside the dialog. A caller that
skips the dialog still cannot open a `javascript:` URL, and still cannot open an address that
was not in the document.

**The dialog is itself a surface, and the three rules it follows are in `weblinkdialog.ts`'s
header.** The short version: the host is shown as punycode because rendering the Unicode form
is drawing the homoglyph attack on the attacker's behalf; the path is secondary and truncated
because that is where a stranger writes something reassuring; and the confirmation is per
link, never per domain, because a domain grant established from a document a stranger sent is
a standing capability nobody is asked about again.

**Comments raised the stakes on all of this on 2026-08-16 without changing the argument.** A
document's annotations are the largest body of attacker-chosen prose tpdf has ever put on
screen — bodies, authors, subjects, several paragraphs each — and they reach the DOM through
`commentlist.ts` and `commentpopup.ts`. The reader's *own* marks reach it through
`marklist.ts` as of 2026-08-20, on the same terms and with a narrower origin — §T6.4 has
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

Editable redaction verifies the uncovered output before adding its opaque appearance,
black unless the reader chose white or red (`redaction_fill::Fill`). The colour is the
only thing the choice changes; a white fill hides that a redaction was made, from a
reader of the copy, and hides nothing the removal left.
The final `RedactionFill` worker job adds opaque, printable appearance streams; it
does not perform or certify content removal. Both the structural scan and OCR gate are
evaluated first, and their failures remain failures after filling. A fingerprint taken before
the checks pins the final rewrite to that output, and an immutable worker snapshot
plus a second fingerprint check before replacement rejects changed files. The
existing path-based verification race described above remains: these checks do not
hold one file handle throughout scanning and OCR. Remaining text stays selectable;
removing a whole text-show operation can also remove text outside the marked region.
This is separate from image-only redaction.

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

**On macOS 27 the engine is readied before the profile, and the profile is unchanged.**
Measured 2026-09-27 on macOS 27.0 (26A428), M5: Vision compiles its text models on first use in
a process and writes them to `~/Library/Caches/<name>/com.apple.e5rt.e5bundlecache` --- the
bundle identifier inside an app bundle, the executable's name outside one --- (three `.bundle` directories, 136 KB, about **23.4 s** of compile on a cold cache). Inside the
profile that write is refused, and the kernel's own report --- `log stream` on
`sender == "Sandbox"` while a cold worker ran --- names it and nothing else:

```
deny(1) file-write-create /Users/<user>/Library/Caches/<executable>
```

No mach-lookup, IOKit, sysctl or read denial appeared for the OCR worker; the profile's
`(allow default)` already grants those. Vision then fails every image with
`__objc2.missingError`, and the gate reports every redaction *not verified*.

The remedy asks for no authority. `ocr_worker::enter_boundary` runs one recognition on a
constant 64 x 64 blank image (`ocr_vision::Vision::warm`) **before** `apply_sandbox`, so the
models are compiled, cached and loaded while the process may still write, and nothing the
sandboxed process does afterwards asks to. The warm-up image is a constant --- nothing from a
document is processed outside the boundary, and the profile still goes on before the first
request is read. With it, a cold worker logs **no denial at all** across the seven fixtures of
`redact-gate-probe` (8/8 each), and the boundary still refuses what it is for:
`ocr-sandbox-probe`'s `ocr` rung, which crosses it through `enter_boundary` itself, has a file
write and a loopback connect both refused with `PermissionDenied`. Removing the warm-up turns
that rung and the gate red again (5/8) on a cold cache, which is the control for the claim.

What was weighed and not needed: a writable per-worker cache directory (Vision's cache follows
`CFFIXED_USER_HOME`, measured, but `HOME` and `TMPDIR` do not move it) would have added a write
grant; no mach service needed allowing. What the warm-up costs is the pre-sandbox write itself,
which is Vision's own behaviour in any unsandboxed process on this OS, to the cache directory
of the executable's name --- and one recognition of a blank square per worker spawn, which
measured no slower per save than before (warm cache: 110--120 ms per gate call against
133--160 ms without it). The one-time compile made the first reply slower than
`REPLY_DEADLINE`'s 30 s allows a slower Mac to be trusted with, so a worker's first reply now
has `FIRST_REPLY_DEADLINE`, 120 s; every later reply is held to 30 s.

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

**The control is held to script as well as size**, since 2026-10-05. A clean verdict stands only
when every script in the words the region covered is one the control word is written in;
otherwise the result is `NotVerified` with the cause `ScriptUnproven`. The gate also asks Vision
with `automaticallyDetectsLanguage` on, where the request has the selector (macOS 13 and later).
Measured 2026-10-05 on build 26A434 in the sandboxed worker, 12 pt at 2x: asked plainly, Vision
returns the control and nothing else for Chinese, Japanese and Thai, which is what the script
rule now refuses; asked for any script, it reads Chinese, Japanese, Korean and Thai inside the
profile. Arabic and Devanagari fail there with `CRImageReaderError error 1`, because their models
compile on first detection and the profile refuses the cache write; Hebrew is read in neither
mode. All three end `NotVerified`. The warm-up runs both request kinds.

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
`OcrWorker::spawn` returns `NO_ENGINE`, which becomes one sentence in
`Applied::why` rather than one per region, and `verified` is false. A skipped check that read
as a clean answer would be this document's own T5 failure arriving through the platform gate.

⚠ **This sentence named Windows as that platform until 2026-09-06, eight days after it
stopped being one.** The refusing arm is `#[cfg(not(any(target_os = "macos", windows)))]`
(`ocr_worker.rs`, `OcrWorker::spawn` and `serve`), so both shipped platforms run the gate and
`NO_ENGINE` is reachable only where neither engine exists. Risk 19 recorded the closure on
2026-08-29 and struck its own half of it; this paragraph, four lines above the Windows arm it
contradicts, did not move — the same summary-drifting-from-the-section-beneath-it failure
§3 records three times over, arriving inside one section instead of between two.

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
child is created suspended and has executed no instruction: a low-integrity token inside a job
object (`sandbox_win::Containment`, `Job::create`, `low_integrity_token`). The job is
assigned by `PROC_THREAD_ATTRIBUTE_JOB_LIST` inside process creation, so parent
termination cannot fall between child creation and a later job assignment. Spawning is
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
(`ERROR_NOT_ENOUGH_QUOTA`). At that time `KILL_ON_JOB_CLOSE` was only claimed;
the parent-death regression below now exercises it from an external test process.

⚠ **And on 2026-08-25 the outcome it is supposed to prevent was observed.** A viewer
mutation run stalled on a live `tpdf.exe --render-worker --prespawn --tile-handle 2028`
whose parent pid had nothing behind it: the app had exited and the warmed pre-spawn was
still there twenty-nine minutes later, idle, holding the stdout and stderr it had
inherited. What that establishes is the *outcome*, not the mechanism — whether the job
object was assigned to that worker at all, whether its handle was closed early, or
whether the exit path leaks a handle that keeps the job alive, is **not established**,
and no probe then could say. At that point memory and process creation were
measured, while orphan cleanup had a counterexample. `docs/TRAPS.md` carries the entry.

**The creation gap is reproduced and closed on Windows, 2026-09-12.** Another native
form check left five suspended renderer workers after app exit. The spawn path
created a suspended process and assigned its job in a separate call. Terminating a
helper parent immediately after `CreateProcess` reproduced an orphan on that
ordering. `PROC_THREAD_ATTRIBUTE_JOB_LIST` now assigns the job during creation;
the same test passes for both ordinary and low-integrity launches, without running
parent destructors. All 23 sandbox tests and 45 worker-boundary checks passed.
Nine native application exits (five form runs, tabs, signatures, and two signed-save
choices) left no surviving test workers. `scripts/win_worker_exit.py` checks the
process table externally after these UI checks, distinguishes terminated process
objects from live children, and fails when enumeration cannot establish an answer.
This closes the demonstrated creation race; it does not retroactively establish
the mechanism of the August incident.

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
   are the second layer, and there are **four** as of 2026-09-06 rather than the one this entry
   claimed — see risk 22, which is where the plan's geometry is named, and risk 18 for
   `save::MAX_MERGE_BYTES`, the one bound besides the tile's that is refused before a worker is
   asked at all.
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
   that is still scaffold — and it has grown twice since that sentence was written, which the
   sentence did not record. `src-tauri/capabilities/default.json` grants four permissions as of
   2026-09-06: `core:default`, `dialog:allow-open`, `dialog:allow-save` (2026-08-16) and
   `updater:default` (`26.8.2`), still unpared. §3's boundary table carries the same list, and
   this entry read `core:default` plus `dialog:allow-open` alone until today — a residual
   describing a narrower grant than the one that ships, which is the over-claiming direction.
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
    never send back. Nineteen parent-process sites go through `diag::note`, counted 2026-09-06 — it was
    nine when this landed on 2026-08-02, and a count in prose is the thing this document
    records as drifting, so the authority is `grep -rn 'diag::note(' src-tauri/src` and not
    this number. It
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
    that reached a reader was the coordinator's epitaph — `worker stopped answering (exited
    with 1 (0x00000001))` — for every document, by every route. It now answers requests with
    the reason instead, over the reply pipe the protocol already has, which is not a logging
    channel and needs no writable path. The coordinator's open path also notes the failure
    through `diag::note`, which it did not before: a session in which nothing could be opened
    left an empty log, byte-identical to a session with nothing wrong. What is unchanged is
    the general case above — a worker that *crashes*, or dies after the document is open,
    still says nothing a reader can send back.
14. **`save_copy` writes a PDF anywhere the reader can write** (§T6.1), added 2026-08-16 —
    the first command on this surface that creates a file, and its authority is the app
    process's rather than a panel's. The path comes from the frontend, so a native save panel
    is the *interface* and not the bound. What bounds it is residual risk 7: the CSP admits
    only the script that shipped. The marginal authority is small — a caller that can reach
    this can already reach `open_document` and the print path — and it is listed because a
    write is a different verb from the ones this surface had, not because the CSP is believed
    to be weaker than it was yesterday.
15. **A saved copy is a serialisation and not a sanitation** (§T6.1), **narrowed
    2026-08-26**. Nothing on that path drops a prior incremental revision, and a copy that
    dropped no page collects nothing — so whatever the source carried, the copy carries.
    That is right for "save a copy" and wrong for a redaction, and the redaction path must
    not be built on it by assuming otherwise. **The narrowing**: a save that dropped or moved
    a page now collects what *that* made unreachable (risk 16), which is a promise about
    tpdf's own leavings and not about the document's.
16. ~~**A copy that lost a page keeps the deleted page's content in every place that is not
    the page tree**~~ (§T6.2), added 2026-08-17, **closed 2026-08-26**. `pagetree::drop_pages`
    removed the page object and every reference to it, and the mark-and-sweep that collects
    what those references *held* — the content stream, the fonts, an embedded image — ran
    on the print path and not on the save path. `save::rewrite` runs it now, whenever the plan
    dropped or moved a page, and two checks pin it in opposite directions: the content of a
    page that went is absent from the file, and the content of every page that stayed is
    still there.

    **What the entry got wrong is worth more than what it got right, because the wording is
    what kept it from being found.** It named the *deletion*, on the reasoning that deleting
    is the first operation where a reader could plausibly believe otherwise. Extract pages
    was already shipped on the same `planned_bytes` -> `rewrite` path and is a stronger case
    in every respect — the command's own name states the exclusion, and the leak is total
    rather than partial. Measured on `links.pdf` before the fix: extracting page 1 of 8
    produced a file reporting **one** page and carrying **all eight** content streams, 4,139
    decodable bytes each. Split, added in 26.8.11, joined the same path afterwards and was
    covered by nothing. `docs/TRAPS.md` has the entry.

    **What is not closed**, and it is the larger half: this collects what *this rewrite*
    orphaned. A document that arrived with orphans in it still comes back with them (that is
    §T6.1's position, and risk 15 above), and nothing here touches the carriers `docs/PLAN.md`
    §6 lists — an annotation's appearance stream, a form field's value, a thumbnail, and the
    structure tree's own copy of a page's alternate text. "Removed" means removed *from the
    page tree and everything only it held*, not yet from the document. (`/ActualText` inside a
    content stream is cleared by *Redact and save as* since 2026-08-27. That is a different
    command on a different path, and it does not make a save a sanitation.)

17. **A cropped page hides content and does not remove it** (§T6.6), added 2026-08-18.
    Everything outside the crop box is still in the saved file, still extractable, and still
    found by tpdf's own search — a crop moves character boxes, not character indices. That
    is what `/CropBox` means, and it is the right behaviour for a crop. It is listed
    separately from risks 14 and 15 because "crop" is a word that sounds like removal in a
    way "rotate" and "move" do not, and because it is now the *second* operation a reader
    could plausibly believe removes something. Redaction is `docs/PLAN.md` §6 and is not
    built.

18. **Every parse of a document is out of the coordinator; `save::Here` is what is left**
    (§3), added 2026-08-22 after an outside review found this document naming printing as the
    only coordinator-side parser while three edit writers had joined it. **Narrowed the same
    day**: a save that only adds marks is *prepared* in the worker now (`Request::Append`).
    The writers left after that were the rewriting save — a deletion, a move, a turn, a crop
    — and the two copy paths, and `lopdf` read the source bytes in the app process on those,
    under `spawn_blocking`, which moves the work off the async runtime and not out of the
    process.

    ⚠ **Every one of those writers closed on 2026-09-01, and what this entry was then named
    for was a *reader* it never listed.** `verify::scan` re-reads the file a redaction
    has just written and parsed it here, on the blocking pool, to decide whether the removal
    was genuine. Its bytes derive from the reader's document, so it is the same exposure the
    writers had — and it was invisible to this entry, to §3 and to
    `scripts/check_writers.py` alike, because all three enumerate the operations that
    **write**. A verification writes nothing. That is the second time this month an
    instrument keyed on writing hid a parse: `print::build` was the first, found by an outside
    review a day earlier. `docs/TRAPS.md` has it under *A risk and a gate both keyed on
    writing cannot see the path that only reads*.

    Two things bounded it while it was here. The bytes are ones tpdf wrote seconds earlier
    rather than the file as it arrived, so a hostile construction has to survive our own
    serialiser first; and the load is bounded like every other.

    ⚠ **It closed the same day, and the last paragraph of this entry predicted how
    correctly** — `verify::scan` was already a pure function of bytes and needles, and the
    file it reads is one the coordinator had just created and could hand over as a descriptor.
    That is exactly the move: `save::Verifier` is the third member of `save::Outside`,
    `save::InWorker::scan` maps the handle and asks `worker_proto::Request::Verify`, and
    `Reply::Verified` carries the report back with no bytes in it. **With that, the title of
    this entry is no longer a coordinator-side parse of any kind** — what is left is
    `save::Here`, the fallback a platform with no sandbox gets, marked by
    `render::UNSANDBOXED_MARK` rather than silent.

    Three consequences worth stating, because none of them follows from the feature. The
    coordinator must send the **password** first: a redacted copy of an encrypted document is
    re-encrypted, so a worker without the key parses no objects and finds no needles, and
    finding nothing is what a clean file looks like — `verify::scan` is built to refuse
    certifying that, so the failure is safe rather than silent, and the ask is what makes it
    answerable. A report is now read under `MAX_REPLY_BYTES`, so
    `verify::MAX_OBJECT_REASONS` bounds the per-object lists at a thousand and adds one line
    counting the rest; without it a file with a few hundred thousand undecodable objects
    produces a report that will not fit, and the reader is told the verification *failed*
    rather than that the file is unaccountable. And `commands::redact::scan_written_file` takes
    `&dyn save::Verifier` rather than the `&dyn save::Outside` its callers hold, so the
    read-back cannot reach a writer — a trait upcast, which costs nothing and is what lets
    the test double be a verifier and no more.

    **Evidence.** `worker-probe` scans one document through both halves of the seam and
    compares the reports field by field, with two controls: the report must name a needle
    that is in every PDF and not name one that is in none, and it must have reached objects
    — without which "they agree" is satisfied by two reports that looked at nothing. A third
    check points the worker path at a directory with no PDFium, where it fails while the
    coordinator path still answers, which is what says a child was involved at all. In the
    unit suite `the_redaction_read_back_does_not_parse_the_file_it_wrote` hands the read-back
    a file that is not a PDF and a verifier that says it is fine, and requires the verifier's
    answer to come back unaltered — red on the code this replaced — with
    `a_read_back_of_a_file_that_is_not_there_is_an_error` as the control that a missing file
    is an error rather than an empty report, which would certify a file nobody looked at.
    Four mutations, each killed by the test named for it.

    ⚠ **The append is not off this list, and this entry said it was until 2026-08-23.** Its
    *preparation* moved; its **verification** did not. `save::append_in_place` re-reads the
    whole file it has just written and parses it with `lopdf` in the app process, to check
    the cross-reference chained and the page count survived — and the previous revision of
    that file is the attacker's bytes verbatim, so this is a coordinator-side parse of
    untrusted input on every append, which is the commonest save there is. It is bounded by
    the same `MAX_DECODE`. It was **also not** under `spawn_blocking` — the `match` that
    calls it ran directly on the async runtime, unlike the three writers above, so a
    document engineered to make the read-back spin stalled the runtime rather than a
    blocking pool.

    **That half is fixed the same day.** The whole `landed` match is on the blocking pool
    now, which moves the rewrite's own work with it: `verify_before_commit` reads the
    source's metadata and renames, and it was on the runtime too.

    ⚠ **This said `verify_before_commit` "hashes every byte of the file" until 2026-08-31,
    and it never has.** That function has compared **length and modification time only**
    since 2026-08-19 — `Fingerprint::agrees_shallowly`, deliberately, and `save.rs` says
    so where it is defined. The digest runs earlier: `rewrite_ready` compares length and a
    SHA-256 of every byte against `Plan::opened_as`, before anything is staged. Both are on
    the blocking pool, which is the claim this paragraph is about and the one part of it
    that was true. The distinction is not cosmetic for a reader of this document: the last
    look before the rename cannot see a replacement that preserved both fields, and the
    reason it is the cheap check — the window between staging and the rename is measured
    in milliseconds — is on `verify_before_commit` itself.

    ⚠ **And the process half closed 2026-08-26, so the append is off this list entirely.**
    The read-back is `save::Reread`, a seam taking the written file's **handle**, a length
    and the password; `save::InWorker` maps that handle read-only, spawns a sandboxed child
    on it, asks `Request::Reread` and drops it. The coordinator no longer holds the bytes,
    so there is nothing there to parse — carried by the type rather than by anyone
    remembering, which is what makes it checkable: `the_coordinator_does_not_parse_the_file_
    it_wrote` writes a file that does not parse, hands over a verifier that says it is fine,
    and requires the save to succeed. It goes red on the code this replaced.

    Three things about that are worth stating rather than implying. **It gains the bounds
    the coordinator could not offer** — the deadline and the memory bound this entry says
    need a separate process, which the append's read-back now has along with `MAX_DECODE`.
    **The obstacle `docs/PLAN.md` recorded was not the real one**: it said the worker "holds
    a mapping of the file as it was", and `save_document` closes the document before the
    write, so there is no such mapping — the real constraint was that a child has to be
    started, at one spawn per in-place append. **And `lopdf` is deliberately still the
    parser**, where `Request::Open` already answers a page count: what is being tested is
    whether the cross-reference *chained*, and PDFium is lenient about exactly that.
    Measured on the day, not inherited — `worker-probe` plants a trailer pointing at
    offset 999999999, PDFium opens it without complaint, and `lopdf` names the
    cross-reference table.

    ⚠ **And the rewriting save closed 2026-08-28, which is what the last paragraph of this
    entry said it needed.** `save::rewrite_update` is the whole rewrite as a pure function
    of the document's bytes and the plan — the split `save::append_update` already had —
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
    and `File::create` on any path is refused with `EPERM` — which is the control saying
    the policy was in force, and without it the run is equally consistent with a sandbox
    that never came on. So the policy stops a worker *opening* a path for writing and does
    not stop a write through a descriptor the parent opened. That is the same asymmetry
    `DOC_FD` already rests on in the other direction. The usual explanation — the check is
    at `open` rather than per write — is the standard account and is not what was measured;
    the rule to act on is the pair of outcomes.

    **What it costs is one spawn, measured.** On `comments.pdf` the rewrite is 2.4 ms in the
    coordinator and 11.4 ms in a worker — +9.0 ms, best of five interleaved — which is the
    process start plus PDFium's initialisation and is therefore fixed rather than
    proportional to the document. On a file where the parse is the cost it disappears; this
    fixture is close to the worst case for it.

    **What the coordinator can still check, and it is exactly one thing.** It never sees
    the bytes, so it compares two numbers arrived at independently: the length the worker
    reports and the length the staged file has. A short write, a reply built for another
    request, or a second rewrite appending to the first all disagree there. Neither number
    is derived from the other, which is what makes it a check rather than a restatement.

    **Evidence.** `worker-probe` writes the same document twice — once through
    `save::Here` and once through `save::InWorker` — and compares them **byte for byte**:
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
    is not a PDF and requires it to succeed — red on the code this replaced.

    **What is not closed.** `save::Here` still parses in the coordinator, and it is what a
    platform with no sandbox gets — refusing would make such a platform useless rather
    than uncontained, which is the rule `Backend::default_here` already follows, and
    `render::UNSANDBOXED_MARK` is what keeps the two runs distinguishable. Beyond that,
    **two paths remain and they are named rather than counted**: `save::write_merged`, and
    `print::build` on the page-range print route.

    ⚠ **The copy paths, Split and the working-document print job closed 2026-09-01.**
    `save::write_copy`, `save::write_split` and `save::print_bytes` take the same
    `save::Rewriter` the rewrite took, and the shape is the rewrite's: the source's
    **handle** goes in, a staging file's handle goes in, and a length comes back. The
    question this entry left open — *handing a worker a descriptor to a file it did not
    create is a decision this entry has not made yet* — turned out not to arise: what the
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
    `save::Job` — one value carrying both of the ways a print job differs from a save, the
    reader's view rotation and this refusal — travelling on `Request::Rewrite` and decided
    in `save::rewrite_update`. Leaving it behind would have meant shipping a decrypted copy
    of the reader's document out of the sandbox in order to refuse it.

    ⚠ **`print::build` was a coordinator-side parse this entry never listed, and it closed
    on 2026-09-01 — the day after it was written down.** The page-range route — a range
    the reader typed, or any print with no document open — called it, and it loaded the
    file with `lopdf`, walked the page tree and serialised, all here. It was the same
    exposure as the one above by a different function, and it was missed for the same reason
    the 2026-08-30 correction records: this entry has enumerated **commands**, and the
    property is one of **functions**.

    **The gate has the same blind spot, and that is the part worth keeping.**
    `scripts/check_writers.py` derives its list from the terminal writers in `save.rs`, so a
    path that parses the reader's document and *writes nothing* is invisible to it by
    construction — a print job goes to a printer. Every instrument here was keyed on
    writing; the property is parsing. See `docs/TRAPS.md`, *A risk and a gate both keyed on
    writing cannot see the path that only reads*.

    It is closed by `crate::print::build_update`, the pure half, run through
    `worker_proto::Request::PrintRange` in the same sandboxed worker as every other rewrite,
    with `save::print_range_bytes` owning the scratch file and doing no parse. It answers
    with `Reply::Rewrote` deliberately: the fact is the same one — N bytes down the output
    channel — and the coordinator compares it against the staged file's own size through
    the same `save::landed_is` a rewrite uses. `worker-probe` was 37 checks when this was written, the three new
    ones being the differential, the needs-a-worker control and the scratch cleanup.

    **No password crosses on this request**, and that is not an omission:
    `print::build_update` refuses an encrypted document whether or not the key is held,
    because `lopdf`'s full serialiser emits every object in the clear and a selection cannot
    be appended. Sending the key would buy a decrypted copy and nothing else.

    **What it costs is one spawn per operation, measured.** On `testdata/text-base14.pdf`,
    best of five interleaved and three consecutive runs: the copy is 4.0 ms here and 11.9 ms
    in a worker (+8.0), the print job 0.2 -> 7.2 (+7.0), and the rewrite 0.2 -> 7.2 (+7.1).
    `text-wide.pdf` reads +7.2, +7.0 and +6.9. All three are the same fixed cost — a
    process start plus PDFium's initialisation — rather than anything proportional to the
    document. `worker-probe` was 40/40 on macOS when these were taken, and prints all three numbers.

    **The Windows half was measured on 2026-09-01, and it took no new probe.** The mechanism
    there is a `DuplicateHandle` of the staging file into the child's table, named in argv on
    `--out-handle` — the same route the document's section already takes, and the granted
    access travels with the handle rather than being re-checked against the low-integrity
    token. That was the expected behaviour, and expected behaviour is what this document had
    instead of a reading for as long as the sentence here said *unmeasured*. `worker-probe`
    is a step of both CI legs now rather than a run somebody remembers to make, and on run
    33501693368 it reported **34/34 checks passed, 0 not applicable to this platform** on
    `windows-2025` — so the copy, the split, the print job and the rewrite's output channel
    are each watched working there, not described.

    **Every check count in this entry is a dated reading, not the current number.** Three of
    them said *is N checks now* and all three went stale inside a fortnight, which is the trap
    this project already records about a count written into prose. The authority is the probe's
    own last line; `BUILD.md` carries the running account of what was added when.

    ⚠ **The read and socket ceiling of the Windows boundary is unchanged by that**, and is
    residual risk 4. A measured output channel says the descriptor handover works; it says
    nothing about what a compromised worker could still reach.

    The general shape is the one this entry already records — **a mitigation that moved
    half a path reads exactly like one that moved the path**, and the half that stayed is
    the one nobody writes down.

    ⚠ **Merge documents widens this, on purpose, and by a different axis: it parses files
    the reader chose that tpdf never opened.** Added 2026-08-24. Every other writer on this
    list reads the *open* document — one file, already parsed by a worker, already
    rendered on screen. `save::write_merged` loads each incoming file with `lopdf` in the
    coordinator, before anything about it is known, so a merge of four documents is four
    coordinator-side parses of bytes the application has never seen. Each is bounded by the
    same `MAX_DECODE`, the graph walk in `merge.rs` uses `sweep::MAX_NESTING`, and the whole
    command is on the blocking pool — so what it adds is exposure to more attacker-chosen
    input on the existing path, not a new kind of access.

    **Closed 2026-09-01, and the paragraph above was wrong about how.** It said the incoming
    files "could go through a worker on the way in, since what has to come back per file is a
    page count and an object graph — which is the whole file, so it has the rewrite's
    problem after all". That reasoning had the direction backwards: nothing has to come back
    *per file*. The merge is one operation with one answer, so the files go **in** and the
    merged document comes out down the output channel the rewrite already had.

    They go in as one read-only mapping — every incoming file concatenated, with
    `save::Incoming` naming where each begins, how long it is and what to call it — on
    `worker::IN_FD`. One mapping rather than one per file because the descriptor shuffle
    between `fork` and `exec` may not allocate, so a descriptor per file would need a
    compile-time cap, and a cap on how many documents a reader may merge is a product limit
    invented to suit a shuffle. `Reply::Merged` carries two numbers, which is why it is a
    variant of its own rather than the `Reply::Rewrote` a page-range print reuses: the page
    count can only be taken where the merged document is.

    The coordinator's remaining part is to **read** those files, and reading is not parsing:
    `save::concatenated` reads those files straight into the mapping the worker is handed
    — one copy rather than the two it used to make — and never asks what they mean. It is
    also the one place a merge's *size* is bounded: `MAX_MERGE_BYTES` (1 GiB) is checked against
    the running total from the files' own handles before a byte is read, because the coordinator
    holds every incoming document at once and the worker maps the same segment. A reader who
    picks a folder of scans gets a refusal naming the limit rather than an allocation failure
    (2026-09-06).

    `worker-probe` was 40 checks when this was written. Three of them are this: the coordinator and the worker
    merge a document with itself and produce byte-identical output with the same page count,
    that count is the merged document's rather than any plan's, and the merge refuses when
    there is no worker to be had.

    Decompression is bounded at
    `MAX_DECODE`, graph recursion at `sweep::MAX_NESTING`, and a panic is reported rather
    than fatal (pinned by a test, so the property cannot be lost to a profile change) — but
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
    word, and those are reported *not verified* — which is the safe answer and is also the
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
    — bytes nobody read, reported. And ~~on a platform with no engine there is no gate at
    all: Windows gets one sentence saying so, which is honest and is not a mitigation.~~
    **Closed 2026-08-29**: `ocr_windows.rs` drives `Windows.Media.Ocr` behind
    `ocr::Recogniser` and `OcrWorker::spawn` has a Windows arm, so both platforms run the
    gate. The remaining no-engine sentence is now reachable only on a platform this project
    does not target — and the test that covered it is compiled by neither, which
    `ocr_worker.rs` says out loud rather than leaving as an apparent coverage.

20. **The Windows engine cannot be told not to correct what it read**, added 2026-08-29 with
    the engine. `ocr::Options::language_correction` is documented as off for verification
    *always*, because a corrector turns marks it cannot read into plausible words — which is
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
    capture — a control composited beside real page ink, at the document's own contrast.

    So a *not verified* from this engine means the same as one from Vision, and a *clean* rests
    on a control the engine may in principle have reconstructed rather than read. The
    instrument that would narrow it is the corpus sweep `redact-reach-probe` already does on
    macOS, run against real documents on Windows; it has not been, as of 2026-09-01. Since
    that date the run is named in `BUILD.md`'s release checklist at **step 8**, with the flags
    it needs there — `--no-gate` off, because the gate is the half under test — rather than
    living only in this sentence, which nothing reads before a tag.

21. **A 358-byte document ends the process that parses it, and no guard we can write will
    stop it**, added 2026-09-01. A cross-reference stream declares the byte widths of its own
    fields in `/W`, and `lopdf` multiplies them out and asks for a zeroed buffer of the result
    without checking it: `/W [1 4 3333333333333333332]` gives `memory allocation of
    3333333333333333332 bytes failed`. **That is `handle_alloc_error`, so it is an abort and
    not a panic** — `catch_unwind` cannot see it, and there is no point in the tpdf code
    where a check could go, because the code that would have to check is `lopdf`'s own
    cross-reference parser. The threshold is sharp: `W[2] = 2^45` completes, `2^46` aborts.

    **It is on the load path**, so every reader of the object graph reaches it: `annots::scan`,
    `links::scan`, `docinfo::scan`, `encoding::scan` and `save::rewrite_update`, verified by
    feeding one file to each. What bounds the damage is where those parses run, which since
    2026-08-28 and 2026-09-01 is a worker for all of them: a reader sees a panel that never
    fills and a save that refuses, and the pool restarts a process. That is the entry about a
    pool replacing a dead worker with the same bytes and faulting again — correct behaviour
    with an unhelpful shape, rather than a compromise. Before those moves it would have taken
    the application down from `commands::print::print_job`. **The last route by which it still could —
    `save::write_merged` — closed later the same day**, so every `lopdf` load of the reader's
    document now happens where an abort takes a worker rather than the window. **The last one —
    `verify::scan`, the redaction read-back — closed later the same day through
    `worker_proto::Request::Verify`**, so on both shipped platforms there is no
    coordinator-side `lopdf` parse of a document for this to reach. `save::Here` is the
    exception and is what a platform with no sandbox gets.

    **`testdata/abort/xref-bomb.pdf` is the reproducer, generated like every other fixture**
    (`testdata/make_xref_bomb_pdf.py`) — in a subdirectory of its own, because every sweep
    over `testdata/*.pdf` would otherwise load it and die, and `worker-probe` hands it to a rewrite through a real
    worker: the worker dies, the coordinator is told so in words, and the probe carries on. The
    coordinator arm is deliberately not run against it — it would take the probe with it,
    which is the finding rather than a test.

    **Rechecked with lopdf 0.45 on Windows, 2026-09-10:** the generated xref-bomb
    is now rejected with `could not parse the document: couldn't parse input`
    instead of aborting. The worker probe requires refusal and no output; its
    separate explicit worker-kill check still proves crash reporting. The wider
    fuzz corpus has not been rerun against 0.45, so this closes the reproducer's
    observed abort, not every possible allocation failure described here.

    Nothing here is a memory-safety defect and nothing is exploitable beyond availability: the
    allocation is refused, not made. The fixes available are upstream in `lopdf`, or a
    pre-parse of the cross-reference stream's `/W` before handing the bytes over, which means
    writing a second cross-reference parser to protect the first. The upstream
    change above removes the observed abort for this reproducer.
    Found 2026-09-01 by coverage-guided fuzzing of `lopdf` through our own entry points,
    independently by **three** targets — `lopdf_load`, `encoding_scan` and, on 2026-09-02
    after 8,051,057 executions, `annots_scan`. Five artifacts carry it at five magnitudes,
    from 6.7e15 to 4.6e16 bytes; the numbers differ and the defect does not. That third target
    is why `src-tauri/fuzz/run.py` now forks **every** whole-document reader rather than the
    ones observed to stop: this entry already says all five reach the abort, so a target left
    unforked is one waiting to lose its run to something already known to be reachable from it.

22. **A worker trusts the geometry in the plan it is given**, added 2026-09-02. Every input
    this document counted was document-shaped; a save hands the worker a second one. A `Plan`
    carries page sizes, crops, quads and stroke points as raw `f32` and `f64`, and it arrives
    from outside the app process — across the worker boundary, out of a restored session, or
    computed against a revision of the file that has since been replaced. `edits.rs` refuses a
    non-finite page size and the model refuses one enclosing no area; **both of those run in
    the coordinator**, and the same argument that moved every `lopdf` parse into a worker
    (risk 18) says a guard there is not a guard here.

    **Measured rather than argued.** `fuzz_targets/save_rewrite_update.rs` reached **6.2 GB of
    allocation from a 2,937-byte input, in one pass** — against 54 MB for a size-matched file
    from the same corpus and 52 MB for an empty one. Two mechanisms, and each needed its own
    fix: `rewrite_update` accepted a made page 1.3e190 points wide, and `draw_wave`'s trip
    count is `width / half` where `half` comes from the quad's *height*, so an unremarkable
    width over an unremarkable height gave 200,049 line segments. Bounding the page does not
    close the second — for an unturned page `from_device` carries the quad's own dimensions
    straight through, so the ratio is reachable on a perfectly legal page. `MAX_PAGE_POINTS`
    and `MAX_WAVE_SEGMENTS` are the two bounds; three mutations cover them and are caught by
    the tests named for them.

    **What is residual is the class, not those two.** The audit that produced them read every
    drawing routine in `save/marks.rs` for loops, and found that all the others iterate a
    collection whose length the plan's byte size already bounds. That is the extent of it: the
    rest of the save path has not been swept for a quantity derived from plan geometry, and
    the plan's other numeric fields — crops, stroke coordinates, note lengths — have not been
    put to the same question. On macOS nothing catches the next one if there is one, because
    the memory poll of risk 2 has no caller; on Windows the job object's committed-memory cap
    refuses the allocation before a byte of it exists, which is the asymmetry §T3 describes.

    Availability only. Nothing here is a memory-safety defect and no allocation succeeds that
    should not have; the reachable harm is a worker dying, or on macOS the machine paging while
    it tries.
23. **A font program that declares no embedding rights is edited as unrestricted**, added
    2026-09-18. Text editing reuses glyphs already embedded in the document and never
    extracts, installs or copies a font program elsewhere; where a program states its rights
    (a TrueType or OpenType OS/2 `fsType`, a Type 1 `FSType`, a CFF PostScript `/FSType`),
    anything but installable or editable embedding keeps new text out of that font: since
    2026-09-26 its text is still read and can be replaced or deleted, but a replacement is set
    in the bundled OFL Noto Sans and a request to write it in the original font is refused,
    so no glyph of a restricted program is ever used for text it did not already show. A
    CID-keyed CFF program with such rights still refuses the edit. A program with no such
    declaration was accepted for Type 1, CFF and Apple TrueType, and since this date for
    OpenType-style TrueType too, because Typst drops OS/2 from every TrueType subset and ISO
    32000-1 does not require the table in an embedded program. The residue is a licence
    question, not a security one: a subsetter that dropped a restrictive table (a "preview
    and print" font) leaves nothing in the file to say so, and the edit goes ahead. Nothing
    inside the document can close that; inferring rights from a font's name would be a guess
    presented as a check.

24. **A compromised worker chooses the revision the reader signs** (§T6.21), added 2026-09-26.
    The app process checks the update's arithmetic --- its length, its range, its empty hole
    and the digest over the bytes written --- and deliberately does not parse it, so it cannot
    say *what* the new revision contains. A worker the document has taken over could write a
    revision that also replaces a page's content, and the reader would sign that. The read-back
    by a second worker and the reader's own verifier see the change as part of the signed
    revision, not as an alteration after it. Bounded by the worker being the process that
    already renders the document to the reader --- a worker that can lie here can lie about
    every pixel --- and not closed: closing it needs an independent reader of the update in the
    app process, which is the parse the split exists to keep out.

25. **A document's certificates reach `trustd`, outside the worker's sandbox** (§T6.22), added
    2026-09-27. The trust check asks the OS chain builder about the signer's certificate and the
    rest of the signature's set, and on macOS that parse happens in the system daemon rather than
    in the contained worker. Bounded by what is handed over --- at most sixteen certificates,
    each re-encoded from a successful `x509-cert` decode and under 64 KiB --- and by `trustd`
    being the component every other program on the machine hands attacker-supplied
    certificates to. Not closed: closing it means not asking the OS, which is the decision the
    trust verdict rests on. Windows trust-store access under containment was measured
    2026-09-29 (§T6.22); its chain builder runs in the worker process.
    **Widened 2026-09-28, at the 26.9.22 release audit** (§T6.22, §T10): a signing with
    long-term data hands the OS certificates from the network --- the timestamp token's set ---
    in the coordinator or `tpdf-cli`, which is not contained on either platform: once to judge
    whether the authority is trusted before anything is fetched for it, and once for the
    offline chain the gathering looks for issuers in. Held to the same bounds, sixteen
    certificates each re-encoded and under 64 KiB, and asked only when the reader asks for
    long-term data on one signing.

26. **A key the reader allowed the command-line tool to use signs for anything running as
    them** (§T6.23), added 2026-09-27. *Always Allow* in the macOS keychain prompt is granted to
    the program, not to an occasion, so once given, any process in the reader's account can run
    `tpdf sign` with that key and no prompt appears --- which is exactly what unattended signing
    needs and exactly what a script the reader did not mean to run would use. Bounded by the OS:
    the grant is the reader's, per key, visible and revocable in Keychain Access, and a key that
    asks for a PIN or confirmation every time (a smart card, a key marked so) keeps asking. Not
    closed, and not closable by tpdf without overriding the decision the OS asked the reader to
    make; the README says what the choice means.
27. **A script that treats `tpdf redact`'s exit code 1 as success ships a copy nobody proved
    clean** (§T6.23), added 2026-09-27. The tool keeps a copy it could not verify, as the window
    does, and says so with exit 1 and every reason; what it cannot do is make a caller read
    them. On macOS 27.0 (26A428), until the OCR gate was fixed on 2026-09-27 (§5.1), *every*
    copy exited 1 --- so a script written against that build that learned to accept 1 accepts
    everything. Bounded by the contract: 0 is
    the only code that means proved clean, the README's table says so, and the report's
    `verified` is `null`, `false` or `true`, never absent. Not closable by tpdf.

28. **A timestamp's time is attested by whoever holds a timestamping key the store trusts, and
    the authority is judged now** (§T6.24), added 2026-09-28. A token that checks out is the
    authority's statement, and the authority can state any time; that is what a timestamp is.
    tpdf judges the authority's certificate at the present moment, with no revocation data, so
    a timestamping key compromised and revoked since reads the same as one that never was, and
    an authority whose certificate has expired since reads `expired` even when the token was
    made well inside its dates. Bounded by the standing's own words --- judged now --- and by the
    time being called attested only beside a sound token. **Narrowed 2026-09-28** (§T6.25): a
    document carrying revocation data for the authority's certificate is now judged against it
    at the time the token states, and a revoked authority no longer attests a moment for the
    signer. Not closed for the ordinary document, which carries none: that reads *not checked*,
    and fetching it would need the network in the read path, which was decided against.
    **Narrowed again with archive timestamps (PAdES B-LTA, 2026-09-28):** a document timestamp
    later in the file, sound and from an authority the store trusts, is the moment the
    authorities before it are judged at, so an authority whose certificate has expired since is
    judged inside its dates rather than read `expired`. The last archive's authority is still
    judged now, and so is every authority in a document without one.
29. **Over `http://`, an attacker on the path can substitute a timestamp from an authority of its
    own** (§T10), added 2026-09-28. It sees the imprint and the nonce in the request and can mint
    a token over both; the token checks out and is written. It cannot forge one from the
    authority the reader chose. Bounded by the authority's standing being said wherever the
    timestamp is --- the closing sentence after signing, the properties dialog, `tpdf verify` ---
    so a substituted authority reads *not trusted* rather than as the one chosen; and closed for
    the one listed authority that serves HTTPS, Sectigo, which is asked over it. **Closed for
    long-term signing at the 26.9.22 release audit**: with long-term data, the authority must be
    trusted for timestamping by the OS store before anything is fetched, so a substituted token
    is refused, nothing written, and its certificates choose no address tpdf connects to
    (§T10). That price was paid there --- trust is now asked in the coordinator on network
    bytes (residual 25), and a real authority whose root a Windows machine has not yet fetched
    is refused for long-term data until it has. Still open for a plain timestamped signing: a
    substituted token without long-term data is written, and reads *not trusted* wherever it
    is shown --- the decision left open in `docs/PLAN.md` §9, Phase 6's open questions.
30. **A signer judged at an attested moment is judged with today's trust store** (§T6.25),
    added 2026-09-28. The OS store is asked whether the chain ended at a root it trusts *at*
    `genTime`, using the roots it holds now: a root distrusted since reads as not trusted, and a
    root added since as trusted --- the chain model EN 319 102-1 describes, with current anchors.
    And the moment is the authority's, trusted *now*: an authority compromised without its
    revocation reaching the document can attest any time. Bounded by the sentence naming both
    the moment and whose clock it is. **Narrowed 2026-09-28**: archive timestamps are read (PAdES
    B-LTA, §T6.25), so an authority followed by a sound archive from an authority the store
    trusts is judged at that archive's moment rather than now. Not closed: every moment is
    still judged with today's roots, and the last archive's authority is trusted *now*.
31. **A delegated OCSP responder's own revocation is not asked** (§T6.25), added 2026-09-28.
    tpdf checks that the issuer issued the responder's certificate for `id-kp-OCSPSigning` and
    that it was in force when it answered; it does not look for revocation data about the
    responder itself. Real responders' certificates carry `id-pkix-ocsp-nocheck`, which says
    not to (RFC 6960 §4.2.2.2.1), and pyHanko, which does ask when that extension is absent,
    was the one oracle to disagree before the test responder carried it. Bounded by responder
    certificates being short-lived by practice. Not closed.
32. **A B-LT signing tells each certificate authority that its certificate is in use** (§T10),
    added 2026-09-28. Each OCSP request carries a serial number the responder can tie to its
    holder, from the reader's IP address, over plain HTTP for most public responders. Bounded by
    being asked only when the reader ticks the box for one signing, or gives `--long-term`, and
    by the README saying so. Not closable by tpdf: it is what an OCSP request is.
33. **A response replayed inside its validity window reads as current** (§T10), added
    2026-09-28. No nonce is sent and the public responders ignore one, so an attacker on the
    path can answer with an earlier `good` response signed before a revocation, as long as its
    `nextUpdate` has not passed; tpdf writes it, and a later reader judges the same response.
    Bounded by the response's own validity window (7 days on the test responders; hours to days
    on the public ones, `docs/PLAN.md` §9) and by responses being signed. Not closed.
34. **A chain the document leaves unfinished reads not checked, even where this computer's store
    would finish it** (§T6.25), added 2026-09-28. The walk up from the signer's certificate
    takes issuers only from the document, so a B-LT document from another writer whose `/DSS`
    stops at a cross-certificate, or leaves out an intermediate the OS holds, reads *not
    checked* for the certificate where the walk stopped, however good the rest. The direction
    is the safe one --- nothing reads *good* that the document does not show --- and tpdf's own
    writer carries every issuer and the self-issued anchor. The cost is a warning a reader of
    such a document sees and a stricter reader might not. Not closed: offering the OS chain's
    roots as anchors only, never as issuers of checked data, would close the cross-certificate
    half without making the answer depend on the computer's data.

35. **An edit's font depends on what the computer has installed** (§T6.26), added 2026-09-30.
    Automatic mode uses an installed copy of the document's own font for characters its
    subset lacks, so the same edit sets in that font on one computer and in Noto on another,
    and a preview taken on one is not a promise about the other. Accepted with the decision:
    the preview names the font it used and marks it *(installed)*, and once applied the
    journal carries the subset, so saving on the computer that previewed it writes what was
    previewed. CFF-outline installed fonts are refused rather than used.
36. **The redaction gate's script rule reads the text layer, and a region without words has
    none** (§5.1), added 2026-10-05. The scripts a region held come from the page's character
    codes, so a page whose codes claim one script while its glyphs draw another is judged by
    the codes. A region that held no words (a drawing, a scan) has no script to hold the
    control to: detection catches Chinese, Japanese, Korean and Thai there, Arabic and
    Devanagari fail closed with an engine error, and Hebrew or any script Vision does not
    detect can still certify. Below macOS 13 and on Windows there is no detection, so only the
    script rule protects. A Latin name under a Japanese control, or the reverse, is now
    `NotVerified`: the control chooser does not prefer a control in the covered script.

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

# §5.1 as shipped, each rung in a fresh process with a fresh, cold Vision cache (~47 s).
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-sandbox-probe -- \
    testdata/text-base14.pdf
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
