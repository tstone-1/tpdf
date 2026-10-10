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
Cancellation reaches no writing IPC. The warning does not consult the integrity
verdict the same read now carries (`integrity.rs`, 2026-09-26): saving can break
an intact signature and cannot repair an altered one, so it is shown for every
signed document alike. The verdict is shown in the properties dialog, where it
says what it checked and that the key's owner was not. The read that feeds this
warning therefore also hashes each signature's range, under the per-document
`integrity::MAX_HASHED` budget, once per document.

**Signing with a certificate (Phase 6 step 2, 2026-09-26)** is split across the boundary:
the worker builds the revision (`sign_prepare.rs`, `Request::PrepareSignature`), the app
process reads the file as bytes, re-derives the digest, has the OS sign it through
`keystore.rs` and splices the CMS in (`sign_cms.rs`), `save::write_signed` writes a new file,
and a fresh worker reads it back (`save::Verifier::signatures`). The worker never holds a key
and the app process never parses the document. `signing.ts` carries the sequence and every
sentence, except the verdict on the read-back, which is Rust's and the same for the window and
the command-line tool (`commands::sign::finish` and `read_back`); `App.svelte` supplies the chooser, the save panel and the message area. The
signed-save warning is not shown: signing appends one revision and writes no byte of the
earlier ones, which `sign-probe` shows pyHanko agreeing with. Unsaved edits, encrypted
documents and DocMDP `/P 1` are refused. `docs/PLAN.md` §9 has the decisions and the
measurements; `BUILD.md` has `sign-probe` and the keystore tests.

**A timestamp when signing (2026-09-28)** sits between the signature and the write:
`sign_cms::sign` makes the signature into a `Made` without writing it, `tsa::stamp` asks the
authority the reader chose (app process only; the worker keeps no network) and checks the token
with `integrity::token`, `Made::stamped` adds it as an unsigned attribute, and `Made::seal`
splices and checks the signature **and** the token in the finished bytes before anything is
written. When the request fails the window holds the `Made` in `commands::sign::Pending` and
`sign_resume` writes it with another try or with none, without asking the OS again;
`tpdf sign --timestamp` exits 3. `docs/PLAN.md` §9, *Adding a timestamp when signing*, and
`docs/THREAT-MODEL.md` §T10.

**Long-term validation data when signing (2026-09-28)** follows the seal: `longterm::plan`
finds every certificate to ask about in the timestamped CMS, `longterm::gather` fetches OCSP or
a list for each through `tsa::fetch` (app process only) and judges it with `revocation::judge`,
and a worker spawned over a snapshot of the unwritten bytes appends the `/DSS` and reads the
result (`sign_dss.rs`, `Request::AppendValidation`, `save::Verifier::validation`);
`longterm::check` refuses unless both revocations read `good`. On failure the window holds the
sealed bytes as `Pending`'s `Sealed` stage; `tpdf sign --long-term` exits 3. `docs/PLAN.md` §9,
*Long-term validation data when signing*.

**Long-term validation data for a document that is already signed (2026-10-10)** is the same
second half, started from a file somebody else signed: *Add long-term validation data…*
(`file.addValidationData`, `validationdata.ts`, `add_validation_data`) and `tpdf long-term`
(`cli/long_term.rs`), which share `commands::validation::finish`. A worker reads the file and
hands over each signed field's `/Contents` beside its scan (`sign_dss::survey`,
`Request::SurveySignatures`, `save::Verifier::survey`); `longterm::existing::add` plans from
those --- every signature's signer chain and every timestamp's authority chain, through
`longterm::walked`, the signing's own walk --- gathers with `longterm::gather`, has a worker
append the `/DSS` (`sign_dss::extend`) and ends with `longterm::archived`, the signing's
document timestamp, from the authority the reader chose. What is its own: **every leaf must
chain to a root the OS trusts before anything is fetched for it** (`existing::plan_one`,
`longterm::vouched_for`), since every certificate here is the document's; **all of the
document's signatures are covered or nothing is written**; a certification with no changes
permitted, an encrypted document and a survey that is not complete are refused before anybody
is asked; and the worker's reading of the result is held against its reading of the original
before the copy is written and again after (`existing::covered`, `existing::read_back`). The
survey's counts and sizes are held again in the app process before any of it is parsed
(`existing::bounded`); a `/DSS` with no room left for what a run adds is refused before
anything is fetched (`existing::room`), and `sign_dss::append` writes no entry the `/DSS`
already holds. A document that needs a password is the encrypted refusal, also where the
worker only answers that it is locked. A later run works while the certificates are still
valid, and is refused once one has expired. No key
is used, so nothing is held between tries, and the signed-save warning is not shown, for
signing's reason. `docs/PLAN.md` §9, *Validation data for a document that is already signed*,
and `docs/THREAT-MODEL.md` §T10.

**Whether the OS trusts a signer (2026-09-27)** is asked in the worker, inside
`docinfo::scan_from`, of every signature whose integrity verdict is intact or weak, and
nowhere else: `trust.rs` hands the signer's certificate and the rest of the signature's set,
re-encoded and bounded, to the system store (`SecTrust` on macOS, `CertGetCertificateChain`
on Windows) with the network off, and `judge` reads the answer --- now; again at the
certificate's own last or first moment when it is out of date; then its extended key usage
against `sign_cms::DOCUMENT_PURPOSES`. `Signature::trust` carries it; `integrity.ts`'s
`trustRow` words it, naming the store; since 2026-09-28 revocation is a row of its own,
`revocationRow`, from the data the document carries (`revocation.rs`), and a signature whose
timestamp is intact from a trusted authority is judged at the time it attests (`judge_at`);
`properties.ts` puts it directly under the integrity row, whose "owner not checked" sentence
it then replaces. The store construction and the offline flags are shared with
`keystore.rs`'s signing chain through `trust::platform`. Tests never touch a real store:
`Anchors::Only` gives one evaluation in-memory roots. `signature-probe --mode trust` is the
real-store instrument and the sandbox measurement; `docs/THREAT-MODEL.md` §T6.22 the
residuals.

**A visible appearance (2026-09-27)** is chosen in the same chooser, invisible by default. The
placement reuses the viewer's crop drag as `ArmedTool` `place` (`Viewer.armPlacement`, one
rectangle, answered `null` by Escape, another tool or the viewer going); `signing.ts` reads the
saved visual signature first (`loadSignature`, none meaning words alone) and sends page id,
display rectangle and pixels with `sign_document`. The app process reads the signer's name from
the certificate and maps the page id to the file's page; the **worker** checks both and writes
the `/AP` form, image, soft mask and Helvetica into the revision it signs
(`sign_prepare/appearance.rs`, through `save::Upright` on a turned page), so the appearance is
covered by the signature. Refused: names outside Latin-1 (never substituted), rectangles off the
page or under 24 points a side, and any certified document, where pyHanko reads a new visible
field as a DocMDP violation. PDFKit does not rasterise a `/Sig` widget on a turned page;
`sign-probe --visible` checks that page's appearance through a stamp copy.

**What it shows (2026-09-27)** is chosen in a panel between the chooser and the drag
(`signappearance.ts`, wired by `signing.ts`): the image --- saved, drawn or imported now through
`SignatureDialog` (`APPEARANCE_WORDS`), or none --- the three lines each switchable, and a reason
and location, which `sign_prepare.rs` also writes as `/Reason` and `/Location` text strings
(`text_string`: PDFDocEncoding where it equals Latin-1 and does not begin like a byte-order mark,
UTF-16BE otherwise). They travel as `sign_prepare::Options` inside `Placement` and `Visible`,
defaulted so an older request means the three lines. The **preview** is `sign_preview`: the app
process reads the name from the certificate, and the document's worker runs
`sign_prepare::preview` --- the signing's own `prepare_visible` over a blank page the size of the
rectangle --- and renders it as PNG (`render::run_signature_preview`); the panel paints it on a
canvas. The form is drawn at the origin, so the preview's stream is the one the signing writes.
*Remember as my default* keeps the choices, never the pixels, in `localStorage`
(`tpdf.signatureAppearance`); anything unexpected there reads as the defaults.

AcroForm filling uses `forms.rs` inside the document worker, with shared field
answers in the edit journal and `Plan.forms`. Every save carrying answers takes
an explicit-appearance rewrite; ordinary save, copy, print and raster redaction
share that writer. Text fields, checkboxes, radio groups, dropdowns (including
editable choices), and single/multiple-selection lists are supported. XFA,
read-only, password, file-select, comb and rich-text controls are not editable.
Fields are made by `formfields.rs`: `Plan.new_fields` carries them to the same
rewrite, which adds them before it writes answers. A new field is one object that is
both field and widget, with its own appearance, appended to the page's annotations and
the form's field list together; the form dictionary and its `Helv` font are made when
the document has none. Text fields and checkboxes only, on pages the document does not
turn. `tpdf form` is one caller. The other is the window: a field a reader drags is a
mark of kind `Field` in the edit journal, with its name as the mark's note and its kind
in `Mark::field`, so that it is moved, renamed, removed and undone as every mark is.
`save/marks.rs` hands such a mark to `formfields::place`, after the pages are in their
final order, and attaches the widget where it attaches every annotation. A plan holding
one is a rewrite. `scripts/tabs_check.py --phase fields` drives it in the application.
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
The strip's paths and the active tab are written to the session file on every change
(`TabRecorder` in `src/lib/tabrestore.ts`); with *At launch: reopen all tabs* chosen,
`launchPlan` shows the tab that was in front and `openBehind` opens the others without
a viewer once the first page is drawn. `scripts/session_check.py --only tabs` drives it.
`scripts/tabs_check.py <binary> <fixture.pdf>` exercises the application on disposable
copies. On Windows, run an isolated build (`TAURI_CONFIG` with a distinct `identifier`)
when the installed app is running, since single-instance forwarding otherwise absorbs it.
The restart session still restores the most recent document, not the full tab list.
With no document open the window lists the session's newest places (`src/lib/startpage.ts`):
its rows are what the palette's `file.recent.N` commands are built from, a row runs its
command through the registry, and `session_forget` and `session_clear_places` remove
places without touching the tabs or the preferences.
Restoration keeps the saved absolute point until the target page's lazy geometry
arrives; ordinary scrolling over estimated pages still keeps its relative position.

Two documents side by side are three modules and one rule. `src/lib/panes.ts` holds which
tab is on which side, which is in front and which side is focused, and `Panes.plan` says
what has to be mounted or torn down; a side is not a place on screen, so a side that
empties hands its page area to the other and *Switch sides* rebuilds nothing.
`src/lib/livedocument.ts` names the variables `App.svelte` keeps about a mounted
document (`LiveDocument`) and moves them as a whole: the variables hold the focused
document, and `Stage` holds the other one's record. `App.svelte` carries the plan out in
`showPanes` and changes side in `focusSide`, which is synchronous because the viewer in
the side pressed handles the same press next.

The `views` gate (`scripts/check_view_after_await.py`) holds the rule below as text: a
variable of the focused document read after an `await` outside `asDocument`,
`documentTasks.run` and `opens.run` fails, as does `applyEdit` after a wait without its
view, and every top-level `let` in `App.svelte` is one of those variables or is named in
the script as the window's. It reads text and does not follow calls. Code that must stop
when the reader has left calls `stillIn(view, model)`, and `applyEdit` answers the state
the view adopted. A frame led by the unfocused side runs inside a lend, and `Stage.within`
keeps the lender's record so that the focused viewer's own callbacks run from in there.

`openDocument` is a sequence of steps. What it decides is `src/lib/documentopen.ts` (the
page table, the place to resume, the second view of a document, the side, the wording of a
failure), and `buildSidebar`, `adoptModel`, `buildViewer`, `showModel` and
`readAfterFirstPaint` in `App.svelte` are its wiring, called from it alone. A form's
controls are built by `buildFormLayer`, with every callback bound to its view. Inverted
page colours and the pen nib are the window's and each viewer holds a copy, so
`toggleInvert` and `chooseNib` set them through `Stage.each` on every mounted viewer.
What the reader is told after a failed open has gone back to a tab the window already had
is `toldAfterFallback` in `documentopen.ts`: `openDocument` answers what it said and
`showPanes` says it again after the mounts, which put back each tab's own message. A tab
keeps a search's scope with the page ids it was taken on (`KeptScope`), and
`scopeToRestore` puts it back only while the order is the same; a mounted viewer drops it
in `setPages`.
What a failed open takes out of the tabs, and that the row is drawn after the last of
them, is `dropAbandoned` in `documentopen.ts`; `abandonOpen` calls it on every path. A form
control is placed by `controlRect` (`savedfields.ts`), asked through `asDocument(view,
...)`: a control lays itself out from its own key handler, and the reader may be in the
other side then. `tabs_check.py --phase form-beside` drives a form on the unfocused side,
and `--phase answers` the outline, comments and links a document is asked for after its
first paint.

The rule: **code in `App.svelte` that runs without the reader pressing anything in that
document runs through `asDocument(id, ...)`, and again after every `await`.** A frame, a
late reply and the rest of an edit all read the variables, and by then they may be the
other document's. The viewer's and the sidebar's callbacks get this from `scoped`, which
wraps the whole options object; `runEdit`, `firstPaint`, the four replies after first
paint and the two word walkers do it by hand. `refreshMenu` returns while `stage.lent`,
since the menu bar describes the focused document only. A new field of one document goes
in `LiveDocument` and the slot table, which does not compile without it; a tool the
reader armed or a colour they picked is the window's and stays out.
`scripts/tabs_check.py <checks-binary> testdata/text-heavy.pdf --phase sides` drives it;
with the viewer's callbacks unwrapped, *the header reads the focused document's page* fails.

A tab dragged onto a side is `src/lib/tabdrag.ts`: a press is a click until the pointer
has travelled, and `dropSide` says which side a drop lands on. `tabPress` in `App.svelte`
follows the pointer on the window with pointer events; the page's own drag events are not
used, because the shell takes file drops and on Windows that turns them off. The two
sides are written with the tab list (`Sides` in `session.rs`: the paths on the right,
the tab in front of the side the reader is not in, the divider's share), and a launch
that reopens every tab puts them back once the tabs are open: `sidesToRestore` cuts the
record down to the tabs that opened, and `Panes.arrange` exchanges the two page areas
before it would rebuild the viewer already on screen. The recorder is held until then,
so a quit halfway through does not record the tabs as one side.
`scripts/session_check.py --only tabs` drives the record and the relaunch.

One document on both sides is two tabs on one handle and one `Edits`. So a tab, an entry
on a side and a mounted record are named by a view and not by the handle: `ViewId` in
`src/lib/views.ts`, a type of its own because both are numbers, so a handle passed where a
view is wanted does not compile. The first view of a document is named by its handle and
a further one by a number no handle has. `LiveDocument.openView` is what the stage is
keyed by; `openDoc` stays the handle every backend call takes. Three places know that two
tabs can be one document, each through `twinsOf` or `oneEach` in `documenttabs.ts`:
`runEdit` adopts an edit's reply in every mounted view of the model, `closeTab` asks and
releases only for a document's last view, and `openDocument`, when a save or a reload
replaces the handle, tears the other views down first and points their tabs at the new
handle and model, which `openPath` then mounts again. The answers about the file (links,
comments, outline, form) are read by each view for itself.
`scripts/tabs_check.py <checks-binary> testdata/text-heavy.pdf --phase views` drives it.

Scrolling the two sides together is `src/lib/syncscroll.ts`: an offset between two places,
each a page and the share of it above the top of the view, so it holds across two zooms and
two page sizes. `keepInStep` in `App.svelte` runs in every frame of either document and
moves the other with `Viewer.followTo`, which draws before it returns; a follower woken for
the next animation frame would trail by that frame for as long as the reader scrolled.
Three rules keep the two from pulling at each other: a frame that moved nothing leads
nothing (so a document held at its end does not drag the longer one back), a document
being moved does not answer, and after a move or a zoom the follower's place is recorded
as seen. A wheel turned with Alt held scrolls its own side and resets the offset. A zoom
passes across as a factor, and only a zoom the reader set: a fitted side's zoom follows
the window, and both sides see the same resize. The lock is taken again whenever a side
shows a different tab, and `unmountDocument` drops it. The same sides phase drives it; with
the Alt rule ignored, *Alt scrolls one side alone* fails. That check waits for a frame
after the wheel on purpose: read before it, the other side has not moved under any rule.
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
certificate (`certificate.rs`) — a second ASN.1 parser on attacker-chosen bytes, bounded and sandboxed
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

## A document made from pictures (`tpdf images`, *New document from pictures*)

`imagepages.rs` builds the document and is pure: `picture` reads one PNG or JPEG,
`placement` decides the page and the matrix, `document` assembles the pages. It runs in a
worker through `Request::Images`, which is `Request::Merge` with no base: the pictures
arrive concatenated on the inputs mapping, and the worker is started over the warm-up
document because a worker is started over one. `save::write_images` reads the files, stages
and renames; `Rewriter::images` is the seam, with `Here` and `InWorker` behind it.
`cli/images.rs` is the tool's command and `commands/save.rs`'s `images_to_pdf` the window's;
on the frontend the name and the sentence are `src/lib/pictures.ts`.

A JPEG is embedded unchanged under `DCTDecode` and decoded once so that one whose picture data
runs out is refused; the decoder forgives a missing end marker and about twenty missing bytes. Its
EXIF orientation becomes the content matrix, all eight values, and a page takes the shown
shape. The matrices were checked against Quick Look for 5 and 6, and the unit test holds all
eight against the EXIF table. A PNG is expanded to eight-bit gray or RGB and deflated, with an
`/SMask` when any pixel is not opaque. A page is the picture at its stated resolution, 72 DPI
when it states none, held between 3 and 14,400 points a side.

Not done: CMYK JPEG, which needs `/Decode [1 0 ...]` for Adobe's inverted samples; TIFF, HEIC
and WebP; a paper size from the window; and stripping EXIF, which passing a JPEG through
unchanged rules out.

## Setting and removing a password (`tpdf protect`, `tpdf unprotect`, the two *Save a copy* commands)

`protect.rs` is the whole of it on the writing side. `Plan::protection` says what the copy's
password is: `Keep`, which every plan out of the model carries, `Remove` or `Set`. `save::checked`
asks `protect::allowed` before anything is built, `save::rewrite` asks `protect::resolve` for
the state to encrypt with, and `protect::written_as_asked` loads the serialised bytes with and
without the password before they are returned. That last call has no failing input in the
tests: nothing short of a defect in `lopdf`'s writer makes it fire, so a mutation that deletes
it survives. A plan that changes the password is never an append and never the identity.

**`protect::finish` runs after every `Document::encrypt`, also when the password is kept.**
`lopdf` writes each crypt filter without its key length. `qpdf` and PDFium assume the length
from the method; CoreGraphics refuses the filter, takes the password and draws every page
blank. That shipped from 2026-08-28 to 2026-10-03 in the rewrite, the merge and the
image-only redaction, and was found by opening this feature's first output with PDFKit
instead of only with `qpdf`. A new password also gets `/Length 256`, a file identifier when
the source had none, and a header of at least 1.7.

A new password is one string used as both the user and the owner password, with every
permission granted, under `V 5`/`R 6`. `cli/protect.rs` is the tool's two commands, which
open the staged copy in fresh PDFium workers before publishing it; `commands/protect.rs` is
the window's one command, `protect_copy`, where a password sets and its absence removes. On
the frontend the rules and sentences are `src/lib/protect.ts` and the dialog is
`newpassworddialog.ts`; `App.svelte` keeps the two dialogs in order and the `invoke`.

Three limits, each measured. PDFKit does not open a document whose password has a character
outside ASCII, on a file `qpdf` wrote as well as on tpdf's. `lopdf` answers a wrong password
with an error and no password with an empty, locked document, so the two are read
differently in `written_as_asked`. And `lopdf` authenticates with the empty password by
itself, which is why `allowed` decides a removal by loading the bytes with no password and
not by whether one was given.

## A smaller copy (`tpdf compress`, *Save a smaller copy*)

`compress.rs` makes a parsed document smaller before the rewrite encrypts and writes it:
`Plan::compress` is `No`, `Lossless` or `Pictures { dpi, quality, jpeg }`, and
`save::rewrite` calls `compress::apply` after the sweep and before `Document::encrypt`,
then `serialise_packed` in place of `serialise`. A plan that asks for it is neither the
file nor an append.

Lossless deflates plain streams and deflates deflated ones again, keeping what is smaller.
Pictures are found by `drawn`, which walks page content and the blocks it draws for the
matrix at each `Do`; `shrink` scales and re-encodes one picture and leaves it when the new
form is not a tenth smaller. The JPEG encoder is the `image` crate's, already in the tree
through `pdfium-render`; the decoder is `zune-jpeg`, as for `tpdf images`.

The estimate is a worker request, `Request::Shrink`, answered from the graph's one
`lopdf` parse (`DocumentGraph::shrink`), so it is of the file as opened. `render::run_shrink`
adds the sample: it opens the estimate's bytes as a second document in the same worker and
draws one page from each. `commands/compress.rs` turns the sample into two PNG `data:` URLs
for `compressdialog.ts`. `docs/PLAN.md` *A smaller copy* has the measurements and
`docs/THREAT-MODEL.md` §T6.32 what the decoders are given.

## A text layer over a scanned page (`tpdf ocr`, *Recognise text and save as*)

`textlayer.rs` writes recognised words into a page as invisible text; `ocr_layer.rs` decides
which pages get a layer and what the engine is asked; `cli/ocr.rs` is the tool's command and
`commands/ocr.rs` the window's. The two differ in who renders: the tool holds a worker
session and asks for 1024 px tiles, the window asks the render service for the page as one
tile. The window's copy goes through `save::write_checked_copy`, which shows the staged file
to a check before the rename and refuses a source that changed; the check opens the staged
file in the render service and compares each layer with `ocr_layer::reads_back`. Progress is
the event `tpdf://ocr-progress`, and `ocr_cancel` sets a flag read between pages. On the
frontend the sentences are `src/lib/recognise.ts`; `App.svelte` keeps the save panel, the
Stop button and the open of the copy. The plan
carries the words as `Plan::text_layers`, by baseline page, and `save::rewrite` writes them
before any page is moved or dropped. A plan carrying a layer is never an append and never the
identity.

Four choices in the writer are measurements rather than preferences, and each has an entry in
`docs/TRAPS.md`: the font's one glyph is a **box**, because PDFium takes a character's place
from its outline; the unit is a **word**, because a line's characters spread evenly land up to
33 pt from the type; every word is followed by a **space** just past its box, because a
recogniser's boxes touch; and the layer goes **first** in the page's content, inside its own
`q`/`Q`, so it starts from the default graphics state whatever the page's own content leaves
behind.

The engine is asked for words with its language model on (`ocr_layer::options`), the opposite
of what the redaction gate asks: this caller wants recall and is not a safety check.

**A page the engine will not read.** `ocr_layer::outcome_of` turns each page's answer into a
layer, nothing, or *refused*, and it is the one place that decides which engine errors are a
page's own: `RecogniseError::Rejected` only. `Unavailable`, `Crashed`, `TimedOut` and
`MalformedInput` stop the run in both callers. A refused page is `refused` in the tool's
report and in `Recognised`, the sentence is `ocr_layer::REFUSED_MEANS` with
`recognise.ts` holding the same words, and a document none of whose pages got a layer is
still not written. `tests/cli/ocr.rs` holds the tool and the window to each other on a scan
with an Arabic page, since each walks the pages in code of its own. No page is refused for
the engine's low confidence: `docs/TRAPS.md` has why that rule was measured and not built.

**The language.** The tool takes `--language` any number of times; the window keeps one, or
none for the engine's own choice. `ocr_languages` lists what the machine offers
(`Vision::languages`, `ocr_windows::installed_languages`), asked in the app process because
no image goes with the question. The choice is `Session::ocr_language`, set by
`session_set_ocr_language` and sent with `ocr_copy`, where `ocr_layer::choose` holds it
against the list again: a language the machine has stopped offering is not sent to the
engine, and `Recognised::language_unavailable` names it for the sentence. On the frontend
`src/lib/ocrlanguage.ts` has the choice, the list held while the palette asks, what a typed
answer means and every sentence; `App.svelte` keeps the two `invoke`s. The commands are
`file.recogniseTextLanguage`, which fetches the list, and `file.recogniseTextLanguage.choice`,
the palette question it opens, which is `edit.insertPages`' shape. Names are the webview's
(`Intl.DisplayNames`), not the platform's. On Windows an engine reads one language, so
`WindowsOcr` makes a second engine for a language that is installed and is not its own
(`other_language`), and `Recogniser::id_for` reports it; that path is compiled by
`scripts/check_windows.py` and has not been run.
`docs/PLAN.md` §9 *Cross-cutting* has the measurements and what is not built, and
`BUILD.md`'s `textlayer-probe` is the read-back through PDFium.

## Drawings under a redaction (`redact::remove_paths`)

A redaction takes four kinds of page content, each addressed by its place among its own
kind: text (`remove_shows`), text inside a form (`remove_form_shows`), pictures
(`remove_images`) and, since 2026-10-03, paths (`remove_paths`). `redact::covered` decides
from PDFium's object list which of each a region takes. A picture goes when the region
touches it; a path goes only when the region holds all of it (`objects::contains`), because
a rule or a border that runs on past the region is on a third of all regions.

`painted_paths` is the walk that finds a page's paths in its content stream, and it has to
agree with PDFium about which paths are objects; `docs/PLAN.md` §6 *Drawings a region holds
all of* has the rule and the measurement. Two findings are made while the plan is built and
not by the writer (`leave_unplaced`, through `DocumentGraph::path_clips`): a path that also
sets the clip, and a page whose counts disagree. The count travels as `path_objects` in
`RegionPlan` and `PlannedRedaction`, beside `paths`, the way `image_objects` travels beside
`images`. `docs/THREAT-MODEL.md` §T6.29 says what `verified` does and does not cover.

A path that reaches beyond the region is cut at its edge when it is a straight rule or a
rectangle (`pathcut.rs`). `redact::cut_crossing` decides that in the worker from the path's
operators, after `leave_unplaced`, and lists it in `Plan::cuts`; `aggregate` pairs each cut
with its region in `PlannedRedaction::cuts`, and `redact::take_paths` makes the cuts and the
removals in one pass. `remove_paths` is `take_paths` with no cuts. `pathcut`'s module
documentation lists what is cut and what is not; `docs/PLAN.md` §6 *A rule cut at the
region's edge* has the model of a stroke's ink and the measurement.

## A signature image from a file (`sign --image`)

The signature chooser decodes an imported image with the webview's decoder and scales it on a
canvas (`signature.ts`, `signaturedialog.ts`). The command-line tool has no webview, so
`sign --image <file>` decodes in Rust, in a worker: `signature_import.rs`, asked for by
`Request::SignatureImage`, which reads the worker's second read-only mapping
(`Worker::spawn_reading`, the mapping a merge's inputs use, here with nothing to write) and
nothing of the document the worker was started over.

Shared with the chooser: the limits (10 MB, 8192 pixels a side, 8 megapixels), the header
grammar read before anything is decoded (`dimensions` is `signatureDimensions`, rule for
rule), trimming of transparent margins, the 512 by 256 result, and `Image::valid`. Different:
the decoder (`png` and `zune-jpeg`), the scaling (an alpha-weighted area average), and a
JPEG's EXIF orientation, which is not applied. The chooser's *remove white background* box has
no counterpart. A change to the limits or the grammar belongs in both files, and a test on
each side holds them together: `src-tauri/testdata/signature/headers.json` is 46 image headers
with the size each must read as, or none, and both `signature.test.ts` and
`signature_import/tests.rs` read every one. `headers.py` beside it writes the file.

`cli/sign.rs` reads the image before it asks the store for anything, so a file that cannot be
used ends the run with no certificate listed and no key touched; with `--image` the saved
image is never read. Only `signature_import`'s own two refusals are reported as the image's;
any other refusal from that worker is the document's.

`sign --hide reason,location` sets `Options::hide_reason` and `hide_location`: the text is
written to `/Reason` and `/Location` and left out of the appearance's lines
(`Options::drawn_reason`). Both default to off on the wire, so the application and every
earlier request draw what they drew.
