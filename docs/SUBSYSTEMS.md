# Subsystems: signatures, forms, tabs, and the worker's writers and readers

Moved verbatim from `AGENTS.md` on 2026-09-24, where it sat under *Stack* and was loaded
before every task. `AGENTS.md` keeps one index line per topic. Code comments and other
documents that say "`AGENTS.md` records ..." about these subsystems mean this file.

## Signatures, forms and tabs

Visual signatures use a bounded RGBA raster (`signature.rs`, `signature.ts`) on
`MarkKind::Signature`; PNG/JPEG decoding stays in the webview. Pixels are shared by
`Arc` in the journal, limited to 512x256 pixels (including rotated equivalents)
per image and 4 MiB across retained marks. The worker writes a PDF Stamp appearance
with an RGB image and alpha soft mask. Placement stores inverse-rotated pixels in
the mark's original display space, so later page turns rotate the image with it.
`SignatureDialog` remembers pixels only on explicit opt-in, in macOS Keychain or
user-scoped Windows DPAPI storage. `signaturestore.ts` migrates the legacy
localStorage entry only after protected readback matches. PNG/JPEG headers are
validated before decoding: at most 10 MiB compressed, 8 megapixels and 8192 pixels
per dimension; animations and changing dimensions are refused.
`tabs_check.py --phase signatures` checks the real application; the independent
PDFium and PDFKit pixel readers and fixture commands are in `BUILD.md`.
This creates visual marks only; it does not create certificate-based signatures.

Before any save/copy writing command, the application reads digital-signature
metadata from the worker and asks for consent if signatures/certification exist
or enumeration was incomplete. DocMDP permission to fill forms does not bypass
the warning: the current rewrite cannot preserve that cryptographic history.
Cancellation reaches no writing IPC. This is a warning, not signature validation.

AcroForm filling uses `forms.rs` inside the document worker, with shared field
answers in the edit journal and `Plan.forms`. Every save carrying answers takes
an explicit-appearance rewrite; ordinary save, copy, print and raster redaction
share that writer. Text fields, checkboxes, radio groups, dropdowns (including
editable choices), and single/multiple-selection lists are supported. XFA,
read-only, password, file-select, comb and rich-text controls are not editable.
Text uses Helvetica with the same supported character set as `textbox.rs`.
`FormLayer` commits and drains validation before a tab transition or save.
`tabs_check.py --phase forms` drives the application on disposable synthetic forms;
`form_pdfkit_check.swift` independently reads and renders the saved result.
When writing inherited fields, materialise `/FT` and `/Ff` on the terminal field:
PDFKit reads `/V` through the parent chain but did not render our fixture's text
when `/FT` existed only on its grandparent. The unit test pins this compatibility
requirement. See `BUILD.md` for the fixture-generation and check commands.

Choice answers use option indices in the journal, preserving distinct labels
that share an export value. The writer saves `/V`, `/I` and explicit appearances
together; editable custom text removes stale `/I`. Radio indices use widget
object order, so page moves cannot retarget a pending answer, and `/AS` is updated
on every sibling while preserving its authored artwork. `TPDF_CHOICE_FIXTURE`
and `TPDF_CHOICE_PROBE` on the mixed-form Rust test generate synthetic inputs
and saved output for `tabs_check.py --phase forms` and `choice_pdf_check.py`.
The latter uses the independent `pypdf` parser and must reject the unedited input.
PDFKit displays the first label for duplicate dropdown exports despite a saved
`/I` selecting the second. `TPDF_CHOICE_UNIQUE_PROBE` generates the distinct-export
control for `choice_pdfkit_check.swift`, which asserts the selected dropdown label
and radio states. PDFKit's thumbnail also omits list-selection shading; the
independent parser checks the saved list values, indices and shaded appearances.

Document tabs retain backend handles and edit journals in `src/lib/documenttabs.ts`.
Only the active tab mounts a Viewer and Sidebar; switching commits open note fields,
drains pending edits, and keeps the reading position, search scope and sidebar choice.
Save/reload replaces the handle in the same tab. File writes block tab transitions;
tab closure releases its handle, and window closure checks all tabs for unsaved work.
`scripts/tabs_check.py <binary> <fixture.pdf>` exercises the application on disposable
copies. On Windows, run an isolated build (`TAURI_CONFIG` with a distinct `identifier`)
when the installed app is running, since single-instance forwarding otherwise absorbs it.
The restart session still restores the most recent document, not the full tab list.
Restoration keeps the saved absolute point until the target page's lazy geometry
arrives; ordinary scrolling over estimated pages still keeps its relative position.
Fit uses the visible sheet, which can differ from the page at the viewport's top.
`tabs_check.py --phase tabs-position` checks repeated tab switches at nonzero offsets
and at the final page under fit-page, fit-width and fixed zoom.
View/page rotations and page reordering retain the fit target across layout changes;
`tabs_check.py --phase tabs-rotation` checks the native view-rotation commands
against an explicit return to the same sheet and refit.

Windows workers join their cleanup job during process creation through
`PROC_THREAD_ATTRIBUTE_JOB_LIST`. Do not restore a separate post-creation
assignment: parent termination in that interval leaves a suspended orphan.
The sandbox regression kills a helper parent at that boundary. Native tab,
form, signature and signed-save checks also verify worker exit externally with
`scripts/win_worker_exit.py`; its controls run in Windows CI and release validation.

Windows orphaned test workers can hold the release executable open while CIM
returns no executable path for them. Restart Manager with that exact executable
registered as a resource identifies its holders. Never clean up by image name:
the installed application may be running alongside the test build.

## What the worker writes and reads, and the password

**Since 2026-08-22 the worker also *writes* with `lopdf`.** `Request::Append` builds the update
section for a save that only adds marks, because doing so is a pure function of the document's
bytes and the plan — and those bytes are the attacker's. It runs where every other parse of
them runs, which narrowed `docs/THREAT-MODEL.md` residual risk 18 from every writing path to
the rewriting ones. The split is by authority: `save::append_ready` asks the coordinator's
questions about a path, `save::append_update` asks none.

**Since 2026-08-28 the in-place *rewrite* runs there too**, which is what took *delete a page
and press ⌘S* off that risk. The obstacle was never the input: it was that a rewrite's answer
is the whole document, against a 32 MB reply limit and files ten times that. So the worker is
handed an **output channel** — the staging file's own descriptor, on `worker::OUT_FD`, given
at `exec` — and writes down it. `save::rewrite_update` is the pure half, `save::Rewriter` the
seam, and `save::Outside` names the one choice both seams read. The coordinator holds neither
the document's bytes nor the new file's; what crosses back is a length, which it compares
against the staged file's own size.

That the channel survives the sandbox was measured rather than assumed: the profile says
`(deny file-write*)`, which denies *opening* a path and not a descriptor handed over before
it. `worker-probe` writes one document both ways and compares byte for byte.

**The copy paths, the split and the print job followed on 2026-09-01**, on the same seam —
`Job` carries what a print does not share with a save, so `staged_rewrite` is one function.
**The page-range print and the merge followed on 2026-09-01**, through
`Request::PrintRange` and `Request::Merge`. The merge is the widest of them — it parses files
the reader picked in a dialog that tpdf never opened — and the obstacle recorded for it was
wrong in a way worth keeping: the threat model said each incoming file's object graph would
have to come *back*, when nothing comes back per file. They go **in**, as one read-only
mapping on `worker::IN_FD` with `save::Incoming` naming each, and the merged document goes out
down the channel a rewrite already had.

⚠ **The last one was a *reader* residual risk 18 never listed: `verify::scan`.** The
redaction verification parses the file it just wrote, and it was invisible to that risk, to
`docs/THREAT-MODEL.md` §3 and to `scripts/check_writers.py` alike, because all three enumerate
what **writes** — the same blind spot that hid `print::build`, twice in two days. The index
has the trap.

**It moved on 2026-09-01, through `save::Verifier` and `Request::Verify`, so on both shipped
platforms no `lopdf` parse of a document happens in the coordinator at all.** `save::Here`
remains the exception and is what a platform with no sandbox gets. Two properties of that move
are not guessable from the feature: the scan needs the reader's password, because a redacted
copy of an encrypted document is re-encrypted and a worker without the key parses no objects
and finds nothing — an absence that reads exactly like a clean file; and a report is now a
reply read under `MAX_REPLY_BYTES`, so `verify::MAX_OBJECT_REASONS` bounds its per-object lists
and counts the rest. The index has that second one too.

**Windows is wired the same way and is measured**: `worker-probe` is a step of both CI legs,
and it reported **45/45 with 0 not applicable to this platform** on run 33626718480, which is
the current `main`. That supersedes the 34/34 recorded here from run 33501693368, and closes
the caveat that stood beside it — the checks added since have now been through a Windows leg.
Read the count from the run rather than from this line: it is a fact about a build, and the
sentence it sits in has been stale once already.

**Since 2026-08-23 a reader can open a document behind a password.** Until then an encrypted
PDF could be chosen from the file dialog and then not opened by any route — `open_failure`
said so, in a sentence ending *"and tpdf cannot ask for one yet"*, which is a to-do that
reads as a decision. The worker asks and retries the load **in place**, which is legal
because a failed load poisons nothing: measured on `testdata/incr-encrypted-pw.pdf`, four
loads of one buffer in one process open on both correct passwords and refuse on both others.

Two consequences are not guessable from the feature. **PDFium answers the same error for a
document given no password and one given the wrong password**, so the sentence a reader sees
on a retry is chosen in `worker_child::unlock`, the only place that knows one was tried. And
**the password is held for the document's lifetime on `Held::password`**, because every
worker after the first — pool growth and crash replacement alike — maps the same bytes and
meets the same encryption; without it a locked document renders the page a reader is looking
at and refuses the next. `docs/THREAT-MODEL.md` §T6.9 states what holding it costs.

**Since 2026-08-23 a reader can also save a mark onto one, and since 2026-08-28 a rewrite
too.** An append never touches the previous revision, and `IncrementalDocument::save_to`
encrypts each appended object with the key the load recorded; a rewrite goes through
`lopdf`'s full serialiser, which writes every object in the clear and drops the `/Encrypt`
dictionary with it — so `save::rewrite` takes the encryption state off the document before
it touches anything, and calls `Document::encrypt` back on as its **last** step, after the
sweep and after everything that adds an object. `examples/password_probe.rs` runs the append
end to end (986 bytes appended to a 2,346-byte AES-256 document, reopened afterwards with the
same password and refused without it); `examples/encrypted_rewrite_probe.rs` is the rewrite's,
through `qpdf` rather than through the writer that produced it.

⚠ **This paragraph said the opposite for a day, and the stack table in `AGENTS.md` said the truth —
one file with two accounts of one fact, which is the failure this file's own Quality-gates
section names.** It read *"a rewrite is refused and always will be through that writer"*. The
*always* was the load-bearing word and it was wrong: `Document::encrypt` is public, and the
capability sat closed for months behind a sentence that read as a decision rather than as a
to-do. What is genuinely refused is narrower and worth stating exactly: a document **nobody
unlocked** cannot be rewritten, because there is no state to put back; and a **print job over
part of** an encrypted document is refused, because re-encrypting gives the printer something
it cannot read and not re-encrypting gives it a decrypted copy of a document somebody
encrypted deliberately (`save::print_bytes`, which takes the reader's password precisely so
that *that* refusal is the one they meet).

The password reaches `save::append_update` because the worker holds it, and reaches
`save::append_in_place` because the app process does. That second hop is not optional: the
append re-reads the file it wrote to check the cross-reference chained, and `lopdf` parses no
objects at all without the key, so the check would count zero pages against the two it
expects and roll a correct save back.

**Every one of those `lopdf` parses takes the reader's password too, since 2026-08-23.** It
is one field on `RawDocument` and five call sites, and it is not a nicety: without the key
`lopdf` parses **no objects at all** and returns a `Document` that loads cleanly and reports
zero pages, so a document behind a real password would open, render and search while its
comments, links, properties and character mapping all came back empty — and empty is the
reassuring answer. `links.rs` and `annots.rs` already carried a `pages_missed` count for
exactly that, which is what `password-probe` asserts against; the comments check exists
because taking the password away from `annots::scan` reddened nothing without it.

**Comments, links and a document's own properties are read through `lopdf`, not through
PDFium, and that is a measurement rather than a preference.** `FPDFPage_GetAnnot` and friends work — checked on a fixture before
anything was written — but every one of them needs an `FPDF_PAGE`, and `FPDF_LoadPage`
re-parses each time at up to 44 ms on a complex page. The panel's question is about the whole
document, so through PDFium it is a page load per page; through the object graph it is one
parse the file already needs for `encoding.rs`, at 0.1 ms on a small document and 11.9 ms on
the 337 MB scan. Since 26.9.2 it is one parse in fact as well as in cost:
`docgraph::DocumentGraph` holds it and six readers share it, and `DocumentGraph::parses` is
the observable that says so. `pdfium-render` also does not expose `/IRT` at all, so a reply arrives there
as an unrelated second note by another author.

**Links take the same route, and it costs a second destination resolver** —
`outline.rs` asks PDFium because a bookmark is a PDFium object, `links.rs` reads the
destination array itself. That is the drift trap this file's index names, so `links.pdf`
gives its outline entries the same destinations as its links and `links-probe --mode agree`
compares them, both against the manifest rather than against each other. The properties
readout in `docinfo.rs` takes the `lopdf` route too, and since 2026-08-21 also parses the signer's
certificate — a second ASN.1 parser on attacker-chosen bytes, bounded and sandboxed
accordingly (`docs/THREAT-MODEL.md` §T6.8). `examples/signature_probe.rs` is the differential
against PDFium's own reading of the same file; `BUILD.md` has the invocations.

Two things in that module are traps rather than choices, both in the index: `lopdf::decrypt`
removes the `/Encrypt` trailer entry, so the encryption has to be read **before** it; and the
permission bits do not mean the same thing at every revision, because bits 9 to 12 are
reserved under revision 2 and a negative `/P` sets all four.

**What PDFium does supply is the marks themselves**, and that is why no drawing was added.
`progressive.rs` renders with `FPDF_ANNOT`, so a sticky note's icon and a highlight's wash are
painted inside the tiles — measured on a fixture carrying no appearance streams at all,
where PDFium generates them: the note icon fills 637 of the 756 pixels in its own rectangle,
the highlight 6,690 of 9,436, and a `/Popup` correctly draws nothing. What no reader could
reach before `annots.rs` was the *text*.
