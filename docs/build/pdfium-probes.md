# PDFium compatibility probes

The checks a digest cannot stand in for, run after a pin change: each command, what it should print, and why it exists. Run them from the repository root after generating the fixtures (`BUILD.md`, *Test fixtures*). Moved here verbatim from `BUILD.md` on 2026-10-23, which indexes this file.

Run these from the repository root, after generating the fixtures below. Each exits
non-zero on failure.

With pdfium-render 0.9.4, removal cases `a` and `c` both passed on Windows on
2026-09-10, against both 7881 and 8044. That does not establish whether the
earlier macOS ownership crash is fixed; retain the spike's workaround until
the destroy case is also rerun there.

```
# The FPDFPageObj_Destroy ownership segfault. Case `c` (leak) must pass; if case
# `a` (destroy) ever stops crashing, the upstream bug is fixed.
cargo run --release --manifest-path src-tauri/Cargo.toml --example remove-probe -- \
    testdata/text-truetype.pdf c

# Extraction order and search must agree with the independently authored text.
# The 7881 -> 8044 update changed Arabic word order; symbol and pixel probes
# cannot see that regression. Generate both fixtures before running these.
cargo run --release --manifest-path src-tauri/Cargo.toml --example search-probe -- \
    --file testdata/multilingual.pdf
cargo run --release --manifest-path src-tauri/Cargo.toml --example search-probe -- \
    --file testdata/encodings.pdf

# The V8 and XFA symbol scan. This mode reads the library rather than binding it,
# so --lib is required even though every other mode defaults it -- and the directory
# is platform-shaped: lib/ on macOS, bin/ on Windows, where the loadable DLL lives.
# macOS is the only platform where this can currently answer: the Windows DLL is
# stripped of local symbols and the check correctly reports [NOT VERIFIED]. Run both;
# the Windows one still reports the export surface, which stripping cannot hide.
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    testdata/text-heavy.pdf --mode engine --lib vendor/pdfium/lib   # macOS
cargo run --release --manifest-path src-tauri/Cargo.toml --example worker-bench -- \
    --mode engine --lib vendor/pdfium/bin                           # Windows

# Progressive rendering still agrees with the safe path, byte for byte. Slow:
# roughly 20 s, because the point is the page that takes seconds to render.
cargo run --release --manifest-path src-tauri/Cargo.toml --example progressive-probe -- \
    testdata/vector-heavy.pdf --mode identity --slices 0

# Form widgets take a second PDFium pass after the progressive render. The
# fixture has a value and deliberately has no stored appearance stream, so
# omitting FPDF_FFLDraw changes 4,587 bytes rather than passing by construction.
cargo run --release --manifest-path src-tauri/Cargo.toml --example progressive-probe -- \
    testdata/form.pdf --mode identity --slices 0

# AcroForm editing: synthetic shared fields, saved appearances and the real UI.
# Use a scratch directory; the application check edits only its own copies.
TPDF_FORM_FIXTURE=/tmp/tpdf-form-fixture.pdf TPDF_FORM_PROBE=/tmp/tpdf-filled-form.pdf \
    cargo test --locked --manifest-path src-tauri/Cargo.toml --lib forms::tests::forms_round_trip_values_and_every_shared_widget_appearance
uv run scripts/tabs_check.py <checks-binary> /tmp/tpdf-form-fixture.pdf --phase forms

# Making a form in the window: placing a text field, a checkbox, a dropdown and
# two radio buttons of one group, a placed field given a limit, a tooltip, a
# text size and a default value in the properties panel,
# picking three with Shift and aligning them, one undo for the arrangement, a
# save, then a saved field dragged, the page picture read at its old and its
# new place before the save, the field given a tooltip, required,
# right-aligned, a text size and a default value in the properties panel, and
# saved again; then the saved dropdown answered and given new choices, and the
# controls read before any save; then a saved radio button given a new value and
# its group a tooltip, and a third save; an empty signature field is placed on
# the way, too low a drag for one refused, and after the last save its Sign
# button is pressed and the signing's chooser opened and cancelled; nothing is
# signed and no key is asked for. The driver reads the saved
# file with the command-line tool and fills it. Any document with a page will do.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase fields
# A field on a turned page: placed on a page the file turns, saved, the page turned a
# quarter more by the reader, a second field placed and saved. The driver reads the turn
# each field declares and fills both. Needs a document whose first page the file turns a
# quarter and whose pages have no field; `testdata/make_rotated_pdf.py testdata` writes it.
uv run scripts/tabs_check.py <checks-binary> testdata/rotated-90.pdf --phase fields-turned
# macOS independent reader; the optional directory receives page PNGs.
swift scripts/form_pdfkit_check.swift /tmp/tpdf-filled-form.pdf /tmp/tpdf-form-render

# A form made by tpdf against PDFKit, the engine Preview reads and saves with,
# in both directions: PDFKit reads the form and the answers tpdf wrote, then
# answers the form itself and tpdf reads that back. macOS only. The optional
# directory keeps the three files and PDFKit's drawing of two of them.
uv run scripts/made_form_check.py /tmp/tpdf-made-form

# pypdf on what `tpdf redact` writes: matched strings absent from page text, form values,
# appearances and orphan objects, with the controls kept. Prove the independent checker first.
uv run --with pypdf scripts/redact_pdf_check.py --self-test
# TPDF_REDACT_PROBE leaves input/output pairs and their expectations, including one field
# whose widgets and distinct appearances are on two pages but only one widget is covered.
TPDF_REDACT_PROBE=/tmp/tpdf-redact cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli
uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/contacts-redacted.pdf \
    /tmp/tpdf-redact/contacts-expected.json
uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/contacts.pdf \
    /tmp/tpdf-redact/contacts-expected.json --expect-fail
uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/multi-widget-redacted.pdf \
    /tmp/tpdf-redact/multi-widget-expected.json
uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/multi-widget.pdf \
    /tmp/tpdf-redact/multi-widget-expected.json --expect-fail

# The same readers on what `tpdf fill` writes. TPDF_FILL_PROBE leaves the filled files, and
# their inputs as the negative controls, in a scratch directory.
TPDF_FILL_PROBE=/tmp/tpdf-fill cargo test --manifest-path src-tauri/Cargo.toml --test cli
uv run --with pypdf scripts/fill_pdf_check.py /tmp/tpdf-fill/every-control-filled.pdf \
    /tmp/tpdf-fill/every-control-answers.json
uv run --with pypdf scripts/fill_pdf_check.py /tmp/tpdf-fill/every-control.pdf \
    /tmp/tpdf-fill/every-control-answers.json --expect-fail
swift scripts/form_pdfkit_check.swift /tmp/tpdf-fill/acme-filled.pdf /tmp/tpdf-fill/render
swift scripts/form_pdfkit_check.swift /tmp/tpdf-fill/acme-filled-signed.pdf
swift scripts/form_pdfkit_check.swift /tmp/tpdf-fill/acme.pdf   # control: must FAIL

# Visual signatures: synthetic colour/alpha quadrants on cropped, rotated pages.
TPDF_SIGNATURE_PROBE=/tmp/tpdf-signatures \
    cargo test --locked --manifest-path src-tauri/Cargo.toml --lib signature_pixels_alpha_and_placement
uv run --with pypdfium2 --with pillow scripts/signature_pdf_check.py /tmp/tpdf-signatures
swift scripts/signature_pdfkit_check.swift /tmp/tpdf-signatures
uv run scripts/tabs_check.py <checks-binary> /tmp/tpdf-signatures/signature-source-0.pdf --phase signatures
# On Windows, set TPDF_SIGNATURE_PROBE with $env:TPDF_SIGNATURE_PROBE and use a local scratch path.
# Use an isolated TAURI_CONFIG identifier if another instance of tpdf is running.

# Pages inserted from another file (`edit.insertPages`, past its dialog): the palette's
# page question dismissed (nothing placed, the file released), answered `2-N` (exactly
# those pages, in order), then left blank for every page --- drawn from the other file's
# own handle, its text read and searched there, one undo and one redo, then saved into
# the disposable copy. The other file needs three pages whose text differs from each
# other and from the opened file's, or the checks on *which* page answered cannot fail;
# links.pdf has eight. The roles were the other way round until the range question.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase import --other testdata/links.pdf

# The sentence a reader is shown after a redaction, read off the message area
# rather than out of the reply that produced it. `verify.rs` and `redact.rs`
# assert the report, and `redact_import_check.py` asserts it across the worker;
# between them and the screen sit `recovery.ts`'s `afterRedaction` and
# `App.svelte`, which no gate reaches. Four passes over four disposable
# copies, each marking regions on page 1 and confirming at the warning:
# a word still on the page that was marked, one surviving only on a page that
# was not, and one in a form object both pages draw, which has no page at all.
# The fourth word is marked in every pass, occurs nowhere else, and must be
# named nowhere in any verdict -- the control that says the other three are
# about the fixture rather than about a scan that reports whatever it is given.
# The fourth pass marks only that word and must come back *verified*: the other
# three expect "not verified", which is also every verdict when the OCR gate
# cannot read, so without it this phase passed on macOS 27 before the Vision fix.
# The first redaction on a machine or after a macOS update waits ~25 s while
# Vision compiles its models.
# `file.redactCopy` is the sibling command and cannot be driven: it opens a
# native save panel. Both report through the same sentence.
# `<checks-binary>`, not an ordinary build: every tabs_check phase is reached
# through `src/lib/harness.ts`, which a normal build strips -- so a release
# binary launches, ignores TPDF_OPENCHECK and is killed by the timeout, which
# reads as a hang rather than as the wrong binary.
python3 testdata/make_redact_pages_pdf.py
uv run scripts/tabs_check.py <checks-binary> testdata/redact-pages.pdf --phase redact-pages

# Recognise text and save as, through the window (`src/lib/recognisecheck.ts`).
# The script makes the scan itself: the fixture's first page rendered by
# tpdf-cli (taken from beside the checks binary) and put on a page as a picture,
# once and six times over. The save panel is answered by `saveanswer.ts`.
# 15 checks in the window and 3 on disk afterwards: the copy is written, the
# stopped one is not, nothing is left staged. Needs the system's recogniser, so
# macOS or Windows with an OCR language installed; about 10 s once the engine's
# models are compiled, up to 30 s more on the first run after an OS update.
#
# The Stop step presses the button the moment it is offered, which is before
# `ocr_copy` has been sent. That is deliberate: it found a stop that arrived
# before its command being cleared by it (`docs/TRAPS.md`).
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase recognise

# The two password commands in the window (`src/lib/protectcheck.ts`): the
# new-password dialog before the save panel, a repeat that differs refused in
# the dialog, the copy, opening it through the password prompt, and removing
# the password again. The script then reads both copies with the tool: the
# protected one must give no text without the password. RAN 2026-10-03 on
# macOS: 15/15 and 6 disk checks.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase protect

# New document from pictures in the window (`src/lib/picturescheck.ts`), with
# no document open: two pictures become two pages and the document is opened;
# a file that is not a picture is refused by name. The script then reads the
# document with the tool.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase pictures

# Save a smaller copy in the window (`src/lib/compresscheck.ts`): the dialog
# fills each choice with its size, the smallest shows two loaded pictures of
# the page, a number that is no number is refused, and two copies are written.
# The script then checks each is smaller than the one before and has the
# page. RAN 2026-10-03 on macOS: 17/17 and 4 disk checks.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase compress

# Signing with a certificate, through the window a reader signs in
# (`src/lib/signingcheck.ts`). NEVER in scripts/gates.py or in CI, and never
# started by an agent: it signs with a real key from the login keychain, so
# macOS asks the person at the machine, and it asks DigiCert and Sectigo over
# the network. RAN 2026-09-28 with the owner at the machine,
# macOS arm64, the test identity: 46/46, three keychain prompts, the key
# requests +1, +1, +0 (Sign without long-term data), +1, +0 (Cancel), and on disk
# digicert.pdf and sectigo.pdf written, cancelled.pdf not. The first run that day
# was 45/47, both failures in the phase itself: the message area's text carries a
# trailing space, and two checks shared a name; fixed in signingcheck.ts and
# signphase.ts.
#   NEEDS: a checks bundle; the test identity (A TEST SIGNING IDENTITY, under
#   the signing probes above) or another self-issued one --- the refusal it
#   expects at step 4 is the one a certificate that publishes no revocation data
#   gets; the network; an unlocked, visible screen.
#   --identity is the certificate's SHA-256, lower-case hex; there is no default
#   and the phase refuses to start without one. `tpdf-cli identities --json`
#   prints it as `id`.
#   WHAT THE PERSON CLICKS: the keychain's "tpdf Checks wants to sign using
#   key..." prompt, three times --- steps 2, 4 and the Cancel path each make a
#   new signature. Click "Allow" each time: a FOURTH prompt is a second key
#   request, the defect step 5 exists to catch, and the phase's count goes red
#   with it. "Always Allow" answers all three at once and leaves only the count.
#   Nothing else: every other click is the phase's.
#   THE STEPS, all through the palette opened from the toolbar, the chooser's
#   own controls and the question's own buttons; only the save panel is
#   answered by a path (`src/lib/saveanswer.ts`), because no phase drives one.
#     1  text-base14 copy open; Sign document...; the identity; Invisible.
#     2  Sectigo preselected; long-term disabled under No timestamp; DigiCert,
#        long-term unticked; the lock question with no answer, Sign refused
#        with the reason, then No, I am the last to sign; Sign. The closing sentence names the file, intact, the
#        timestamp attested by DigiCert, the authority trusted, and no
#        revocation. The save panel would have suggested <name>-signed.pdf.
#     3  the saved copy's Document properties: intact; timestamp attested by
#        DigiCert; authority trusted; signer not trusted, its chain ending at an
#        untrusted root, judged at the time the timestamp attests; every
#        revocation row "not checked".
#     4  the original again: DigiCert remembered; Sectigo, long-term ticked;
#        refused with the question naming the unpublished revocation data and
#        offering Try again / Sign without long-term data / Cancel; the
#        signature held, waiting for its long-term data.
#     5  Sign without long-term data: written, closing sentence as in 2 for
#        Sectigo, nothing held.
#     6  Sign document... again: Sectigo and long-term remembered. Then the
#        Cancel path: refused again, Cancel, "Not signed: nothing was written.",
#        nothing held.
#   THE KEY COUNT. `sign_record` answers how many times this process asked the
#   OS to sign (`keystore::KEY_REQUESTS`, counted by `keystore::Counted` around
#   the identity `sign_document` signs with) and what `Pending` holds. Checked
#   exactly at every step: +1 at 2, at 4 and on the Cancel path, +0 at 5 and at
#   Cancel. The +1 steps are the control: a count that stopped counting fails
#   step 2 before step 5's +0 can mean anything.
#   AFTERWARDS tabs_check.py reads the disk itself: digicert.pdf and
#   sectigo.pdf written, cancelled.pdf not.
#   FAILURE SHAPES. "the identity ... is not in the chooser" --- the keychain
#   holds no such identity, or it expired (the test one is good for 30 days).
#   A hang to the 900 s timeout at step 2 or 4 --- the keychain prompt was never
#   answered, or is behind another window. "refused with a question, not a
#   message" red with a network sentence --- the authority did not answer; run
#   it again. Everything else red is a finding.
#   PROVED ABLE TO FAIL without a window: what the phase concludes is
#   `signphase.ts` (five mutations in mutate_frontend.py, each red in
#   signphase.test.ts), the save answer `saveanswer.ts` (one), the counter
#   `keystore::Counted` and the held state (two in mutate_rust.py). By hand,
#   in the window, not yet run: `sign_document` handing `sign_cms::sign` the
#   identity instead of the counted one turns step 2's +1 red; `signing.ts`
#   answering "without" after long-term data with `shell.sign` rather than
#   `shell.resume` turns step 5's +0 red and raises a fourth prompt;
#   `App.svelte` dropping `discard` from the shell turns "Cancel drops the held
#   signature" red.
uv run scripts/tabs_check.py <checks-binary> testdata/text-base14.pdf --phase sign \
    --identity 2a144cdb0facc6919f9163d7776c3c2f7f35bbd23021c002d61c29c7f0819c74

# Character boxes still land on the ink they describe. Run it on a *small* text
# fixture: on testdata/text-heavy.pdf the wrong convention also scores 70%, so
# that page cannot discriminate and the probe fails rather than reporting a pass.
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-probe -- \
    testdata/text-marked.pdf --mode align

# Reading order taken from a document's own tags rather than from geometry. The
# assertion that carries the weight is not that an order came back but that it is
# the *tagged* one: page 1's margin note reads third by geometry and last by the
# tags, and the manifest states both. Page 2 is the control, tagged in the order
# geometry would infer anyway, and text-base14 is the other control -- an untagged
# page must report no runs rather than an order it inferred.
cargo run --release --example structure-probe -- \
    --library ../vendor/pdfium/lib --file ../testdata/tagged.pdf \
    --untagged ../testdata/text-base14.pdf

# The outline walk terminates, resolves and refuses. Run BOTH: the hostile
# fixture proves the bounds fire, and the ordinary one proves they do not fire
# when they should not, which is the half that catches a walk bounding
# everything.
cargo run --release --manifest-path src-tauri/Cargo.toml --example outline-probe -- \
    testdata/outline-simple.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example outline-probe -- \
    testdata/outline-hostile.pdf --mode check

# The comment scan reads what a reviewer wrote and refuses what it cannot. Run
# ALL THREE: the corpus proves the bodies, encodings, dates, replies and bounds
# (26/26); the rotated one-pager proves rectangles come back in display space
# (5/5, one skip); and the `clean` control on a document with no annotations
# proves the scan is not simply returning nothing for everything -- without which
# every "the hostile page was cut short" assertion passes on a scan that found
# nothing anywhere.
cargo run --release --manifest-path src-tauri/Cargo.toml --example comments-probe -- \
    testdata/comments.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example comments-probe -- \
    testdata/comments-rotated.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example comments-probe -- \
    testdata/text-base14.pdf --mode clean

# Links: the rectangles a reader clicks. Run ALL FOUR, and `agree` is the one to
# read. It compares the two destination resolvers tpdf has -- `outline.rs` through
# PDFium, `links.rs` through lopdf -- on a fixture whose outline points at the same
# places its links do. That mode found a defect on its first run
# (`FPDFDest_GetLocationInPage` answers only for /XYZ, so every /FitH outline
# entry had been landing at the top of its page since outline.rs was written) and
# it is the only check here that can fail for a reason neither module's own tests
# can reach. `clean` is the control: without it, every "the hidden link is not
# listed" assertion passes on a scan that found nothing anywhere.
#   links.pdf --mode check   27/27
#   links.pdf --mode agree    9/9   (6 shared destinations, its control, and the
#                                    manifest-free outline differential)
#   links-rotated --mode check 7/7, 2 skipped
#   text-base14 --mode clean  2/2
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links.pdf --mode agree
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links-rotated.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/text-base14.pdf --mode clean

# A locked document: can a reader actually open one?
# Everything that decides the answer is on the other side of a process boundary
# -- the load in worker_child::serve, the retry in worker_child::unlock, the
# password on the worker's stdin, and the pool replaying it in
# Workers::spawn_into -- so no unit test in the app process can reach it. This
# drives a real RenderService in worker mode. It takes no arguments: the two
# fixtures it needs are named in the file, because the properties are about
# encryption rather than about content.
#
# Proved able to fail by ten mutations, each reddening exactly the check it
# belongs to. The first is the one worth reading, because its failure is the
# defect a naive implementation ships:
#
#   spawn_into skips the unlock       -> "8 served, then: This document is
#                                        locked" -- the first worker's tiles come
#                                        back and the pool's second worker
#                                        refuses. Every other check stayed green,
#                                        including "a tile renders with ink".
#   Response::locked never sets it    -> both "refused as locked" checks
#   unlock does not reword a retry    -> "the second refusal is worded
#                                        differently from the first"
#   serve never enters the unlock loop-> 4 failed, 2 skipped
#
# And six more for the password's onward hops, added 2026-08-23. Each is a
# one-line edit, restored afterwards, with the file digest checked before and
# after so a mutation that did not land cannot read as a survivor:
#
#   RawDocument::password -> None      -> 4 red: properties (locked=true), links
#                                        (2 pages unaccounted for), mapping (2
#                                        truncated), and the save refused
#   docinfo::scan drops it             -> the properties check, alone
#   links::scan drops it               -> the links check, alone
#   encoding::scan drops it            -> the mapping check, alone
#   annots::scan drops it              -> the comments check, alone -- and it
#                                        reddened NOTHING until that check was
#                                        written, because the fixture has no
#                                        comments and a count of them cannot tell
#                                        "none" from "could not look"
#   Workers::password -> None          -> the save: "the service holds None"
#
# WHAT IT NEEDS. testdata/incr-encrypted-pw.pdf, which pyhanko writes -- it was
# built with qpdf until 2026-08-23, and qpdf is not on a hosted runner, so this
# whole probe printed [SKIP]s there. It is in scripts/ci_fixtures.py's --signed
# group now and both workflows already install pyhanko. Without the fixture this
# still prints twelve [SKIP]s naming the reason rather than passing.
#
#   macOS arm64, 2026-08-23   12/12, 0 skipped
#                             the save check reports: 986 bytes appended to 2346,
#                             still AES-256, 2 pages
cargo run --release --manifest-path src-tauri/Cargo.toml --example password-probe

# Structural soundness: does an independent VALIDATOR accept what tpdf writes?
# PLAN.md section 6 step 5 asks for a parser that did not write a rewrite to
# re-check it, and measuring that on 2026-08-26 gave an uncomfortable answer.
# Given a rewrite whose /Size claims more objects than the file holds --
# spike 0.4's defect -- lopdf's loader says "OK, 8 pages", PDFKit says "OK, 8
# pages", and only `qpdf --check` objects:
#
#   reported number of objects (142) is not one plus the highest object number (101)
#
# So the shipped check (verify::structure) is deliberately narrow -- a header,
# one %%EOF, no trailing data, a startxref inside the file -- and this is where
# the missing half is exercised. qpdf is not a dependency and is not on a hosted
# runner; it is on a development machine, so run this by hand before a release
# and after anything that touches how a document is serialised.
#
# Every fixture goes through the REAL writer, save::write_copy -- the same call
# Save a copy, Extract pages and Split reach -- with two plans each: keep every
# page, and drop one, which is the plan that makes rewrite() run the sweep.
#
# BOTH DIRECTIONS ARE FAILURES, and the second is the one to expect:
#   * qpdf refuses what we passed  -> the rewrite shipped a broken file.
#   * we refuse what qpdf passed   -> over-refusal, which is worse than no
#     check: it would refuse to save a document a reader had just edited. The
#     first draft of a /Size rule did exactly that.
#
# TWO CONTROLS, and the probe is worthless without them, because a sweep that
# reports "nothing found" looks identical whether the oracle ran or never did:
#   * a planted stale /Size must be REFUSED BY QPDF. It also re-measures the
#     gap: verify::structure is expected to pass that file, and a run where it
#     suddenly catches it contradicts its own doc comment -- read that first.
#   * planted trailing bytes must be REFUSED BY US, so a run where
#     verify::structure was never called is distinguishable from a clean one.
#
# A finding is compared against the SOURCE's own verdict. A rewrite faithfully
# carries a defect the input already had, and the first run of this reported
# outline-hostile.pdf for a loop in its /Outlines tree -- which is what that
# fixture is for.
#
# WITHOUT QPDF it prints one [SKIP] and exits 2 rather than 0: the caller wanted
# a verdict and there is none. `brew install qpdf`.
#
#   macOS arm64, 2026-08-26   66 rewrites checked, 3 plans refused by the
#                             writer, 0 findings, both controls fired
cargo run --release --manifest-path src-tauri/Cargo.toml --example qpdf-probe

# Redaction: does removing a region remove the words, and ONLY those words?
# src/redact.rs is asserted against hand-built content streams, which is right
# for "which operator gets deleted" and says nothing about a real document -- a
# fixture agrees with whatever its author had in mind. This is the corpus
# control: the same two functions, real files, through PDFium, with verify::scan
# asked whether the words left the FILE rather than the page.
#
# Five checks, and the three that assert LIMITS are the valuable ones:
#
#   text-base14.pdf   the account number is removed, and "Sphinx of black
#                     quartz" on another line survives -- the over-redaction
#                     control, without which emptying the page would pass.
#   links.pdf         eight pages drawing the same words, so the needle names
#                     its page. The first run of this probe marked a word that
#                     lives on every page, removed it from one, and correctly
#                     reported it still in the file.
#   text-marked.pdf   the same line, held SIX times: as /ActualText on a
#                     marked-content span, on the structure element that span
#                     belongs to, in two annotations, in /Info, and in an
#                     outline entry whose title is a substring of it -- of which
#                     only the annotation away from the region survives a
#                     redaction, which is what redact-apply-probe measures. Its
#                     outline is FOUR entries with the carrier in the middle of
#                     the sibling chain, which is the shape that catches a
#                     removal that drops the object without splicing.
#                     Since 2026-08-27 the span's copy is cleared by the removal
#                     itself, so the check reads the carriers apart rather than
#                     asking whether the secret is anywhere in the file: the key
#                     must be gone from the page's content stream, with a control
#                     proving it was there, while the scan must still find the
#                     word -- which by then can only be the annotations and
#                     /Info. Asking one whole-file question could not say WHICH
#                     copy went, and that is why the check that promised to go
#                     red on this very day did not; see TRAPS.md.
#   hostile-scan.pdf  a region over a /DCTDecode image reports an INCOMPLETE
#                     plan naming each object it cannot remove. Deny by default:
#                     taking the words and leaving a picture of the words is the
#                     confident lie section 6 opens by forbidding.
#   text-cid.pdf      the blind spot, asserted in both directions -- PDFium
#                     extracts the account number and verify::scan cannot see
#                     it, because Identity-H stores glyph ids. A run where the
#                     scan DOES find it means the instrument grew a capability
#                     its own documentation denies.
#
# Route B eats the line: PLAN.md section 6 removes the whole text-showing
# operation containing any redacted glyph, so a word beside the target goes with
# it. Every control word is on a different line for that reason, and the run
# prints how many of the page's operators went.
#
# It needs the fonttools fixtures (text-base14, text-marked, text-cid), which a
# hosted runner does not have -- see scripts/ci_fixtures.py. Without them the
# cases print [SKIP] and the run reports that nothing was checked rather than
# passing.
#
#   macOS arm64, 2026-08-26   2 cases ran, 0 failures, all three limits asserted
#   Windows x64, 2026-09-02   3 cases ran, 0 failures, all three limits asserted
#
# The third case is hostile-ocg.pdf, added 2026-09-02, and unlike the two above
# it a hosted runner CAN build it -- make_hostile_pdf.py is dependency-free. It
# reaches the one branch of the marked-content handler nothing else here does:
# a span written `/OC /MC0 BDC`, whose property list is a NAME into the page's
# `/Properties` rather than an inline dictionary, resolved to a real `/Type /OCG`
# dictionary a parser produced. Shown to be the only reacher rather than assumed:
# forcing the shared-list refusal reddens that case and leaves the other two
# green, because both of them write inline dictionaries.
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-probe

# Redaction, end to end: does the whole path actually remove the words?
# redact-probe proves the primitive -- given ordinals, the operators go. This
# proves the PATH: a rectangle built from the character boxes becomes a plan
# against PDFium's own object list, becomes ordinals in a save plan, becomes a
# written file, and the words are not in it. Everything between the drag and the
# file except the dialog and the command's own glue.
#
# TWO READERS, and the control is the point. The needle must be gone and a word
# on another line must survive, asserted through verify::scan over the bytes AND
# through PDFium re-extracting the written file. A scan that finds nothing
# because it cannot look is the failure this repository has recorded from
# several directions; the survivor is what says it can see the file at all.
#
# The region deliberately overlaps a path -- make_text_pdf.py draws four
# unrelated non-text objects -- so the plan is INCOMPLETE and the probe asserts
# that too. A rule under a line of text is what almost every real document has,
# which is why the command writes the file and reports it as unproven rather
# than refusing; see PLAN.md section 6.
#
# THE ANNOTATION CARRIER is the second phase, on text-marked.pdf, added
# 2026-08-27. A comment about a passage quotes the passage, so an annotation over
# a redacted region goes with the words -- popup and replies included. Three
# assertions and the middle one is the control: ANNOT-OVER must go, ANNOT-AWAY
# must stay (a reader's other comments are not theirs to lose), and the secret
# itself must STILL be found, because /Info /Title and the surviving annotation
# both hold it and this command touches neither. If that last one flips, the
# document-level carriers are being cleared and this probe needs rewriting.
# THE STRUCTURE CARRIER is the same row of the carrier table in its other home,
# asserted the same way: STRUCT-CARRIER (the element owning the redacted line's
# /MCID) and STRUCT-ANCESTOR (the element above it, which restates what was
# removed) must go, while STRUCT-OTHER -- the element for a line nobody marked --
# must stay. A rule that stripped the whole tree would pass the first two.
#
# THE DOCUMENT'S OWN DESCRIPTION goes whole: /Info and the XMP packet, asserted
# through the fixture's /Info /Producer string, which appears nowhere else in the
# file. The title is not used for this -- the title IS the secret, so its going
# would be indistinguishable from the page's own copy going.
#
# A check ahead of all of them asserts every marker is in the fixture to begin
# with, without which no direction could fail.
#
# THE FORM is three checks on the same written file, and its fixture is built so
# that the VALUE rule is the only thing that can decide either field.
# FIELD-CARRIER holds the redacted line's own account number and its widget sits
# at the far corner of the page, so the annotation pass leaves the widget and the
# field can only be taken by what it says; WIDGET-UNDER-CARRIER then has to come
# with it, or the page keeps an annotation whose /Parent is gone. FIELD-KEEP
# holds somebody else's answer and is the over-removal control. Both widgets are
# HIDDEN (/F 2) -- not decoration: a visible widget would be drawn by PDFium's
# form-fill environment and move every pixel comparison this corpus makes, and a
# hidden field holding the answer is the more honest shape anyway, since it is
# exactly the leak this carrier is about.
#
# THE OUTLINE is read back through outline::read rather than out of the bytes,
# because a byte scan cannot answer this carrier's question: an entry spliced out
# of the chain but still an object is neither present nor absent by a grep. It is
# also the point -- outline::read is what feeds the sidebar, so a title it still
# returns is a title a reader still sees. Four checks: the carrier gone, its
# child gone, OUTLINE-BEFORE surviving, and OUTLINE-AFTER still REACHABLE. The
# last is what the fixture's shape is for -- see TRAPS.md on forgetting a node in
# a linked list, and note that deleting the splice leaves the first three green.
#
# IN PLACE is the last phase and it is the same removal pointed at the reader's
# own file: stage a sibling, check the source has not moved, rename over it, and
# read back the path rather than the buffer. It works on a COPY of
# text-base14.pdf made into a file of its own -- pointing it at the fixture
# would leave every later run of every other probe reading a redacted one. Four
# checks and two are controls: the needle gone from the reader's own path, KEEP
# still there so a scan that cannot look would fail the first, the file still
# opening in PDFium with every page it had, and the staged sibling gone -- which
# is not tidiness, since a temporary left beside a redacted document holds the
# unredacted bytes.
#
# Its last section is text inside a Form XObject, on form-xobject.pdf: PDFium
# enumerates a form as ONE page object, so remove_shows has no ordinal that names
# what is inside it -- 9,310 of 154,095 realistic regions across 41 real
# documents, the largest carrier a redaction could not take that is made of
# ordinary text. Nine checks, and the discriminating ones are the third and
# fourth: the marked line goes and the line BESIDE IT IN THE SAME FORM stays, or
# a removal that emptied the whole stream would pass everything else here. Then a
# form the document draws twice is refused and no file is written.
#
# Its image section is the same shape on image-region.pdf, 8 checks, and the one
# that matters greps the written bytes for the picture's OWN PIXELS rather than
# asking what the page draws. Those read almost the same and are not: deleting
# the `Do` stops the page drawing it and leaves every byte in the file. That is
# why the fixture stores its images uncompressed, and it is what caught the
# rewrite's sweep condition not listing image removals.
#
# It needs text-base14.pdf and text-marked.pdf, which a hosted runner does not
# have. Without them the run prints [SKIP] and says so rather than passing.
# form-xobject.pdf and image-region.pdf a runner CAN build -- both are pure
# Python with no system font -- so those sections run there.
#
#   macOS arm64, 2026-08-27   48 checks, 0 failures
#   Windows x64, 2026-09-02   52 checks, 0 failures, 0 skipped
#
# The count moved 48 -> 52 on 2026-09-02, when the structure-element section
# gained `/Alt` and `/E` beside its `/ActualText` pair. `redact.rs`'s SHADOW_TEXT
# names all three keys and only the first had a fixture any redaction probe
# opened, so the other two were exercised by hand-built Rust dictionaries alone
# and the loader had never produced them. Proved by A/B rather than asserted:
# truncating SHADOW_TEXT to `[b"ActualText"]` leaves the STRUCT-CARRIER check
# GREEN and turns exactly the two new ones red.
#
# The macOS line above predates those four checks and has not been re-run.
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-apply-probe

# --survey answers the one question that decides whether the feature works on
# real files: how often does the correspondence guard REFUSE? redact.rs removes
# by position and refuses when the show operators lopdf decodes disagree with
# the text objects PDFium counted, because nothing connects the two lists but
# order. Spike 0.3 measured 4:4 on four fixtures built for it and said a TJ
# split across objects, or a Form XObject contributing from another stream,
# breaks it -- without saying how often.
#
# It asserts nothing. A page that disagrees is a fact about the corpus rather
# than a defect, and the pages that disagree are printed because those are the
# ones worth reading.
#
#   macOS arm64, 2026-08-26   48 files, 1720 pages, 0 disagreements
#
# Since 2026-10-03 it asks the same of paths, which `remove_paths` addresses by
# position: the paths lopdf finds painted against PDFium's path objects.
#
#   macOS arm64, 2026-10-03   67 files, 1755 pages, 355,266 paths, 0 disagreements
#
# Read that with its limit: testdata/ is mostly fixtures this project generates,
# so it is not a sample of the wild. It does include the hostile set, the signed
# contracts and the multi-column and multilingual pages. What the number
# supports is "the guard did not fire once across everything here", not "it
# never fires".
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-apply-probe -- --survey

# Signatures: does PDFium agree with us about the same signatures?
# `docinfo.rs` walks /AcroForm /Fields with lopdf; PDFium implements that walk
# in C++ and exports the result. Neither knows about the other, which is what
# makes this the instrument links-probe --mode agree is, for the subsystem where
# being wrong means naming the wrong signer. Seven comparisons per signature:
# the count, /SubFilter, /Reason, /M digit for digit, the DocMDP level, the
# signed byte count, and -- the one that matters -- the certificate parsed out
# of EACH READER'S OWN /Contents blob, compared by subject and serial.
#
# `clean` is the control and is not optional: two readers that both find nothing
# agree perfectly. `agree` REFUSES an unsigned document (exit 1) and `clean`
# refuses a signed one, so neither can report a vacuous pass.
#
# Proved able to fail by five mutations of docinfo.rs, each reddening exactly
# the check it belongs to: summing the byte-range offsets (11357 against 3869),
# never reporting a DocMDP level (0 against 2), reading /Filter as the subfilter
# ("Adobe.PPKLite" against "adbe.pkcs7.detached"), misspelling the /Contents key
# (docinfo read none, PDFium's blob read one), and not recognising /FT /Sig at
# all (0 signed of 0 fields against 1). Restored and re-run green each time.
# What it CANNOT catch, measured rather than reasoned about: a bug inside
# parse_certificate is invisible, because both sides of the certificate
# comparison use it. PDFium hands over the /Contents bytes and no view of the
# certificate set, so replacing the signer match with `certificates[0]` leaves
# the probe at 13/13 on incr-two-signers.pdf -- both sides pick the same wrong
# element. This is a differential over WHICH BLOB, not over what the blob says;
# the unit tests own the second half.
#   all five incr-* signed fixtures --mode agree   7/7 each, 35 comparisons
#   incr-ber                        --mode agree   7/7, and every one of the 7 is
#                                                 a comparison over a BER blob
#   incr-two-signers                --mode agree  13/13, the only fixture where
#                                                 the per-signature pairing can
#                                                 fail (reversing docinfo's field
#                                                 order reddens 4 of the 13)
#   tagged / comments / links       --mode clean   3/3 each
#
# The certificate comparison's (None, None) arm was hard-coded to pass until
# 2026-08-21 -- literally `report.check(..., true, ...)` -- so a document neither
# reader could read a certificate from printed "7 passed, 0 failed". It is a
# FAILURE now: every signature reaching that loop is one both readers found, so
# two empty answers mean the blob defeated both parsers. Proved able to fail by
# stubbing parse_certificate to refuse everything: incr-signed.pdf goes 6/1.
#
# incr-timestamped.pdf is the only fixture whose signature carries an RFC 3161
# token. genTime is PINNED by the generator (2026-08-21 12:00:00 UTC) so tests
# assert the instant rather than its shape; the TSA is pyhanko's offline dummy,
# so the structure is real and the trust is nil.
#
# That fixture is no longer the only evidence. Until 2026-08-21 one real signed
# document to hand carried a timestamp and tpdf read NOTHING from it -- its
# /Contents is BER with indefinite lengths, which `der` refuses by design.
# ber::to_definite_length walks the blob first, and the same document now reads
# its certificate, the key usage it states, and a timestamp from a real TSA one
# second after the signing time it claims. The control was the change stashed
# and the probe rebuilt: cert="(no certificate)" before, named after.
#
# incr-ber.pdf is the fixture for that path, and it is incr-signed.pdf with
# every constructed value in its blob rewritten in indefinite form and NOTHING
# else changed -- same length, byte-identical outside the /Contents span. The
# pair is what makes the check discriminate: two blobs that come out of the walk
# equal can only have done so by the length form being normalised away. It is
# built by rewriting rather than by signing, because pyHanko emits DER and has
# no switch for this.
#
# --mode read also prints the timestamp, when a signature carries one, and what
# each certificate states its key is for. Nothing
# compares that against PDFium, which exposes no extension accessor at all --
# the oracle is `openssl x509 -text` on the same blob, which is what
# `the_usage_a_real_certificate_states_is_the_usage_openssl_reads` is written
# against. For incr-signed.pdf both read "Digital signature, Non-repudiation",
# no extended key usage, and CA:FALSE.
for f in incr-signed incr-certified-1 incr-certified-2 incr-certified-3 \
         incr-certified-3-indirect incr-two-signers incr-ber; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
      "testdata/$f.pdf" --mode agree
done
cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
    testdata/tagged.pdf --mode clean

# --mode nested asserts a DISAGREEMENT, and is the odd one out here on purpose.
# /AcroForm /Fields is a tree; PDFium's signature enumeration reads the array and
# stops, while docinfo.rs recurses. So a field under /Kids gives PDFium 0 and us
# 1, and --mode agree on that fixture reports a count mismatch that reads like a
# defect in us. Established by control: the same document with the leaf flat
# instead of nested gives PDFium 1, same signature dictionary byte for byte, and
# qpdf --check passes both. The mode says in its own output "if this is 1, PDFium
# now recurses and this mode is obsolete", so the limitation expires loudly.
#   signed-nested-field --mode nested   3/3
cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
    testdata/signed-nested-field.pdf --mode nested

# --mode integrity holds the INTEGRITY VERDICT (integrity.rs, 2026-09-26) against
# pyHanko, through `testdata/check_signature.py --json` run by `uv`. PDFium
# verifies nothing, so no other mode reaches the verdict. pyHanko's answer is
# mapped onto tpdf's words: a coverage other than the whole file or a whole
# revision -> unchecked/range; valid=no -> broken (the signature is judged
# before the digest); intact=no -> altered; CRYPTO_CONSTRAINTS_FAILURE -> weak
# (SHA-1); otherwise intact, with the digest names compared too. Needs `uv`; the
# oracle not running is [FAIL], never a pass. macOS arm64, 2026-09-26, 14/14
# files green:
#   incr-signed, incr-certified-{1,2,3,3-indirect}, incr-timestamped,
#   signed-p256, -p384, -pss                      3/3 each   intact
#   incr-two-signers                              5/5        intact, intact
#   signed-sha1                                   3/3        weak
#   signed-altered                                3/3        altered
#   signed-broken                                 3/3        broken
#   signed-nested-field                           2/2        unchecked, range
# plus three real signed documents to hand (an Acrobat Sign certification, a
# SHA-1 supplier certificate, a BER CAdES contract): intact, weak, intact, all
# agreeing. incr-ber.pdf is left OUT on purpose and exits 1 if run: pyHanko
# refuses its indefinite lengths, and OpenSSL disagrees with tpdf about it for a
# reason that is the fixture's -- see the trap "A fixture rewritten to reach one
# parser leaves the standard".
#
# Proved able to fail by mutating integrity.rs, rebuilding, and restoring it
# byte-identical (digest checked before and after):
#   never test the signature value   -> signed-broken 2/3 (tpdf intact)
#   report every match as altered    -> incr-signed, signed-p256 2/3, sha1 2/3
#   treat SHA-1 as strong            -> signed-sha1 2/3 (tpdf intact)
#   skip the hole-is-this-value rule -> SURVIVED on signed-nested-field, because
#                                       its invented range fails the `<`...`>`
#                                       framing first; only the unit test
#                                       reaches that rule
#
# TIMESTAMPS (Phase 6 step 3, increment A, 2026-09-28). The same mode compares a
# signature's RFC 3161 token too: check_signature.py --json reports pyHanko's
# TimestampSignatureStatus, and a document timestamp is read with
# validate_pdf_timestamp. Mapped as the signature is, but pyHanko's token
# `intact` also covers the imprint, so intact=no -> altered. Two disagreements
# are expected and are NOT on these fixtures: pyHanko calls an SHA-1 imprint
# intact (tpdf: weak) and accepts a token with no ESS binding (tpdf: unchecked,
# binding; OpenSSL refuses it too) -- docs/TRAPS.md has both. 2026-09-28:
#   incr-timestamped       6/6   signature intact; token intact, same time
#   incr-doc-timestamped   4/4   the document timestamp intact
for f in incr-signed incr-certified-1 incr-certified-2 incr-certified-3 \
         incr-certified-3-indirect incr-timestamped incr-doc-timestamped \
         incr-two-signers signed-p256 signed-p384 signed-pss signed-sha1 \
         signed-altered signed-broken signed-nested-field; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
      "testdata/$f.pdf" --mode integrity
done

# TIMESTAMP TOKENS AGAINST OPENSSL. The minted tokens' verdicts, held against
# `openssl ts -verify`. The ignored unit test writes each fault's token, the
# data they are over and the authority's root; OpenSSL then checks each. It
# insists on an ESS binding and on an authority certificate whose extended key
# usage is timeStamping, critical and alone, and accepts SHA-1. 2026-09-28,
# OpenSSL 3.6.3, every line tpdf's verdict or standing except the two SHA-1
# tokens (tpdf: weak; OpenSSL: OK):
#   intact OK | sha1-imprint OK | sha1-signature OK | wrong-imprint "message
#   imprint mismatch" | corrupt-signature "signature failure" | altered-content
#   "digest failure" | no-binding "missing signing certificate attribute" |
#   other-binding "ess cert id wrong order" | no-purpose "unsuitable
#   certificate purpose"
T=$(mktemp -d)
TPDF_TOKEN_DIR="$T" cargo test --manifest-path src-tauri/Cargo.toml --lib -- \
    --ignored write_tokens_for_openssl --nocapture
openssl x509 -inform DER -in "$T/root.der" -out "$T/root.pem"
for f in intact sha1-imprint sha1-signature wrong-imprint corrupt-signature \
         altered-content no-binding other-binding; do
  echo "--- $f"
  openssl ts -verify -data "$T/data.bin" -in "$T/$f.tst" -token_in -CAfile "$T/root.pem"
done
openssl x509 -inform DER -in "$T/no-purpose-root.der" -out "$T/no-purpose-root.pem"
openssl ts -verify -data "$T/data.bin" -in "$T/no-purpose.tst" -token_in \
    -CAfile "$T/no-purpose-root.pem"

# SIGNING (Phase 6 step 2, 2026-09-26): does what tpdf signs verify under readers
# that did not write it? sign-probe signs through the production path ---
# sign_prepare::prepare (what the worker runs), sign_cms::finish,
# save::write_signed --- with the key held by `openssl` and the value made by
# `openssl pkeyutl`, so no key is ever in tpdf and the signer shares no code with
# any verifier. Then three readers: tpdf's integrity.rs (every signature intact,
# one more than before), pyHanko via check_signature.py (intact and valid; ours
# covering ENTIRE_FILE, every earlier one ENTIRE_REVISION), and
# `openssl cms -verify -binary -noverify` over the covered pieces. And three
# controls that must turn every reader that can see them: the value spliced two
# digits late, a signature over the wrong digest (written past finish's own
# check, which is asserted to refuse it too), and one covered byte changed.
# Needs `openssl` 3.x and `uv`; either missing is [FAIL]. The second argument is
# a scratch directory it writes the key, the certificate and the outputs into.
# Both workflows run the incr-signed:rsa and incr-two-signers:p256 rows on the
# macOS leg since 2026-10-05, with TPDF_PYHANKO_PYTHON naming the interpreter
# that holds the pinned pyHanko, which the probe then uses in place of `uv`.
# The other rows, and everything below with --visible or --timestamp, still run
# by hand: text-base14.pdf cannot exist on a runner, and a timestamp needs a
# network authority.
# macOS arm64, 2026-09-27, all green (one check more than 2026-09-26's run:
# every signature's pyHanko summary line must now say docmdp=ok, which the probe
# had printed and not asserted --- see the trap "A verdict the probe printed and
# did not assert passed a DocMDP violation as green"):
#   text-base14 --key rsa|p256|p384        16/16 each
#   incr-signed --key rsa                  18/18
#   incr-two-signers --key p256            20/20
#   incr-certified-2 --key p384            18/18
#   incr-certified-3 --key rsa             18/18 (pyHanko logs that its diff
#                                          policy was not designed for /P 3)
#   incr-xrefstream --key p256             16/16 (an xref-stream update)
# pyHanko classes every earlier signature's appended change FORM_FILLING with
# DocMDP satisfied. incr-certified-1 (/P 1) and incr-encrypted-open exit 2 with
# tpdf's refusal, which is the answer: neither can be signed.
for f in text-base14:rsa text-base14:p256 text-base14:p384 incr-signed:rsa \
         incr-two-signers:p256 incr-certified-2:p384 incr-certified-3:rsa \
         incr-xrefstream:p256; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example sign-probe -- \
      "testdata/${f%%:*}.pdf" "/tmp/tpdf-sign-probe/${f%%:*}-${f##*:}" --key "${f##*:}"
done
# --visible signs through sign_prepare::prepare_visible instead: a rectangle on
# page 1 (--rect left,top,right,bottom in display points, 40,40,280,120 by
# default) and a synthetic 64 x 32 signature image, so the appearance, its image
# and its font are inside the signed revision. Every check above still runs, and
# then two renderers that share no code say WHERE THE INK WENT, each rendering
# the original and the signed file at 2 px/pt and counting changed pixels:
#   PDFium, through progressive::render as the viewer renders a tile;
#   PDFKit, through scripts/sign_visible_pdfkit.swift (macOS only; elsewhere it
#     prints [SKIP], never a pass).
# Ink inside the rectangle (at least 2% of it) and none outside, past a
# one-pixel antialiasing band. The control: the same document signed
# INVISIBLY renders under PDFium with no pixel changed. On a TURNED page PDFKit
# rasterises no /Sig widget at all (the trap "PDFKit rasterises no `/Sig`
# widget on a turned page..."), so there the probe asserts that limitation ---
# it goes red the day PDFKit changes --- and checks a copy with the new
# revision's /Subtype/Widget overwritten by /Subtype/Stamp (same length).
# macOS arm64, 2026-09-27, all green; changed pixels inside the 76,800-px
# rectangle, none outside and none on the edge in any run:
#   text-base14 --key rsa|p256|p384        21/21 each  PDFium ~10,000, PDFKit ~10,500
#   incr-signed --key rsa                  23/23       earlier: FORM_FILLING, docmdp ok
#   incr-two-signers --key p256            25/25
#   incr-xrefstream --key p256             21/21
#   rotated-90 --key rsa                   22/22       PDFKit: widget 0, as a stamp 10,516
#   inherited --key p256                   22/22       PDFKit: widget 0, as a stamp 10,537
# incr-certified-2 and -3 exit 2 with tpdf's refusal: pyHanko reads a visible
# field after a certification as a DocMDP violation (trap "A visible signature
# field after a certification is a violation to pyHanko...").
# Proved able to fail by mutating sign_prepare.rs, rebuilding, and restoring it
# byte-identical (SHA-256 checked):
#   no /AP on the widget          -> both renderers' ink-inside check red
#   /Rect moved 100 pt, /BBox not -> ink-inside and none-outside red for both,
#                                    the stamp copy included on inherited.pdf
for f in text-base14:rsa text-base14:p256 text-base14:p384 incr-signed:rsa \
         incr-two-signers:p256 incr-xrefstream:p256 rotated-90:rsa inherited:p256; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example sign-probe -- \
      "testdata/${f%%:*}.pdf" "/tmp/tpdf-sign-probe/visible-${f%%:*}-${f##*:}" \
      --key "${f##*:}" --visible
done
# The appearance's options (2026-09-27): --lines label,name,date (a comma list;
# all three by default), --reason TEXT, --location TEXT and --no-image. With
# --visible the probe now also asserts (a) pyHanko's reading of /Reason and
# /Location --- the text written, trimmed, or null when none was given --- via
# check_signature.py --json, which prints both; and (b) that the appearance
# panel's preview, asked for through render::run_signature_preview with the same
# Visible and time, agrees with the signed page inside the rectangle on every
# pixel where the original page is paper (at most 1 in 1,000 may differ), with
# the unsigned page as the control that must disagree. Five checks more than the
# table above. macOS arm64, 2026-09-27, all green, 0 preview pixels differing in
# every run, turned pages included:
#   the eight fixtures above                       26-30 each
#   text-base14 --reason "Geprüft und freigegeben" --location "Köln-Mülheim"   26/26
#   text-base14 --reason "þÿ ok" --location "ï»¿ ok"  26/26 (UTF-16 in the file,
#                                                   read back by pyHanko as typed)
#   text-base14 --lines date --no-image             26/26
#   text-base14 --lines ""   (the image alone)      26/26
#   rotated-90 --reason Approved --location Hamburg 27/27
#   inherited --reason Approved --lines name        27/27
#   incr-signed --reason Freigabe --location Berlin 28/28
# Proved able to fail by mutating sign_prepare.rs, rebuilding, and restoring it
# byte-identical (SHA-256 checked), each run with --reason Geprüft --location Köln:
#   /Reason never written          -> "pyHanko: /reason reads" red (Null), alone
#   preview ignores the options    -> "the signed page shows what the preview
#                                     showed" red, 3,724 of 40,320 px, alone
cargo run --release --manifest-path src-tauri/Cargo.toml --example sign-probe -- \
    testdata/text-base14.pdf /tmp/tpdf-sign-probe/visible-reason --key rsa --visible \
    --reason "Geprüft und freigegeben" --location "Köln-Mülheim"
# signature-probe --mode agree on a visibly signed incr-two-signers: 19/19, PDFium
# reading the same three signatures; --mode integrity 7/7; qpdf --check passes.
#
# --timestamp (Phase 6 step 3, increment B): the signature stamped by a REAL
# timestamp authority through tsa::stamp and Made::seal, the window's and the
# tool's path, network included --- a measurement run by hand, never a gate.
# Four checks more: tpdf reads the token intact and attested, its time within
# five minutes of this clock, its authority trusted by the system store; pyHanko
# intact and valid; `openssl ts -verify -data <value octets>` OK, anchored at the
# system root the token's own chain names (every system root is printed too and
# not counted: Sectigo's ESS lists a cross-signed root, and OpenSSL given every
# root builds the other chain --- docs/TRAPS.md); and the same token over other
# bytes refused. macOS arm64, 2026-09-28, text-base14 --key rsa, 22/22 each:
#   digicert    token 6,006 B, CMS with it 7,440 B, authority trusted, openssl OK
#   sectigo     token 6,633 B, CMS with it 8,067 B, authority trusted, openssl OK
#               anchored at USERTrust RSA (every root: ess cert id not found)
#   globalsign  token 7,658 B, CMS with it 9,092 B, authority trusted, openssl OK
# The time it prints for the request includes building the HTTP client, which
# from target/debug/examples is 3 to 28 s of CoreFoundation listing that folder
# (docs/TRAPS.md); from a folder of its own, DigiCert's whole request was 0.43 s.
for srv in digicert sectigo globalsign; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example sign-probe -- \
      testdata/text-base14.pdf "/tmp/tpdf-sign-probe/timestamp-$srv" --key rsa --timestamp "$srv"
done
#
# --lock (2026-10-13): the signature locks the document (sign_prepare.rs, "The
# lock"). Six rows more than the same run without it: tpdf reads level 1 on the
# new signature and on no other; pyHanko reads level 1 (check_signature.py's
# `permits`); tpdf refuses to sign the written file again and still builds a
# document timestamp's revision on it; and then THE CHANGE --- testdata/sign_again.py
# has pyHanko's own writer add a second signature --- must read docmdp=VIOLATED
# for the locked signature AND docmdp=ok for the same document signed without
# the lock, which the probe signs for this purpose. The second is the control:
# docs/TRAPS.md has the two changes that turned neither or both.
# macOS arm64, 2026-10-13, pyHanko 0.36.2, all green:
#   text-base14 --key rsa --lock                       22/22
#   incr-signed --key rsa --lock                       24/24 (the earlier signature docmdp=ok)
#   incr-two-signers --key p256 --lock                 26/26
#   text-base14 --key p256 --visible --lock            32/32
#   text-base14 --key rsa --timestamp digicert --lock  28/28
#   incr-certified-2 --key p384 --lock                 exits 2 with tpdf's refusal:
#                                                      a certified document is not locked
# The thirteen guards (the refusal, the two places the lock is written, the
# stricter-of-two reading, the read-back) are rows of scripts/mutate_rust.py,
# named "sign lock: ...", "lock read: ..." and "lock read-back: ..."; each turned
# its named unit test red when run by hand on 2026-10-13.
for f in text-base14:rsa incr-signed:rsa incr-two-signers:p256; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example sign-probe -- \
      "testdata/${f%%:*}.pdf" "/tmp/tpdf-sign-probe/lock-${f%%:*}" --key "${f##*:}" --lock
done
# Long-term data after a lock: the instrument below also writes b-lt-locked.pdf, a
# locking signature with its /DSS and archive timestamp appended. pyHanko must say
# of its Signature1: modification=LTA_UPDATES docmdp=ok (measured 2026-10-13):
#   uv run --with pyhanko testdata/check_signature.py /tmp/tpdf-long-term/b-lt-locked.pdf
# check_signature.py's summary could not read any file with a document timestamp
# until that day: it handed the timestamp to the signature validator, which raises.
#
# REVOCATION (Phase 6 step 3, increment C1). Nothing here goes online in a gate;
# two ignored instruments let OpenSSL and pyHanko judge what tpdf judges. First,
# minted OCSP responses and lists, written with the answer tpdf gives each:
TPDF_REVOCATION_OUT=/tmp/tpdf-revocation cargo test --manifest-path src-tauri/Cargo.toml \
    --lib write_minted_data_for_openssl -- --ignored --nocapture
cd /tmp/tpdf-revocation
for n in good revoked unknown delegated unauthorised corrupt wrong-certid; do
  openssl ocsp -respin ocsp-$n.der -issuer ca.pem -cert subject.pem -CAfile ca.pem \
      -verify_other ca.pem -no_nonce -validity_period 999999999
done
for n in empty listed corrupt wrong-issuer; do
  openssl crl -inform DER -in crl-$n.der -CAfile ca.pem -noout
done
# macOS arm64, OpenSSL 3.6.3, 2026-09-28: all eleven agree (good/revoked/unknown/
# delegated OK with that status; unauthorised "missing ocspsigning usage";
# corrupt "signature failure"; wrong CertID "No Status found" = tpdf none; a list
# naming another issuer "unable to get issuer certificate" = tpdf none). pyHanko's
# own verify_ocsp_response / verify_crl agree on the same files once the
# responder carries id-pkix-ocsp-nocheck (docs/TRAPS.md).
#
# Second, real data, once: ask each authority above for a token (openssl ts
# -query, curl), split the certificates out of it, fetch an OCSP response about
# the timestamping certificate and the list it names (openssl ocsp -url, curl),
# and lay each out as <dir>/<authority>/{subject,issuer,ocsp,crl}.der. Then:
TPDF_REVOCATION_REAL=<dir> cargo test --manifest-path src-tauri/Cargo.toml \
    --lib judge_real_data -- --ignored --nocapture
# 2026-09-28: good on all six (three responses, three lists), as OpenSSL said
# given the system roots; one byte flipped in a response and a list, unchecked
# signature from tpdf and a verify failure from OpenSSL. docs/PLAN.md §9 has the
# sizes and the issue times that decided the freshness rule.
#
# incr-lt.pdf is the B-LT fixture another tool wrote (make_incremental_pdf.py,
# pyHanko with cryptography's responses); pyHanko's own reading of it is
# validate_pdf_signature at its token's time with the /DSS as the only
# revocation data, which says INTACT:TRUSTED with the root it carries anchored.
# LONG-TERM VALIDATION DATA (Phase 6 step 3, increment C2). Offline first: an
# ignored instrument writes B-LT files made against the fake PKI (OCSP, lists, an
# intermediate), with every /DSS entry and the roots beside them.
TPDF_LONG_TERM_OUT=/tmp/tpdf-long-term cargo test --manifest-path src-tauri/Cargo.toml \
    --lib write_b_lt_documents_for_other_readers -- --ignored --nocapture
cd /tmp/tpdf-long-term
for f in b-lt-*.pdf; do qpdf --check "$f"; done
openssl x509 -inform DER -in ocsp-root.der -out root.pem
openssl x509 -inform DER -in ocsp-signer.der -out signer.pem
openssl ocsp -respin ocsp-ocsp-0.der -issuer root.pem -cert signer.pem -CAfile root.pem -no_nonce
# pyHanko, no fetching, revocation hard-fail, the test roots: the trust must come
# from the /DSS. Save as lt.py and run `uv run --with pyhanko python lt.py <name> <pdf>`;
# the control is the same file cut at its second-to-last %%EOF (no /DSS), which
# must read UNTRUSTED.
#   import sys
#   from asn1crypto import x509, ocsp as O, crl as C
#   from pyhanko.pdf_utils.reader import PdfFileReader
#   from pyhanko.sign.validation import (validate_pdf_signature, DocumentSecurityStore,
#                                        KeyUsageConstraints)
#   from pyhanko_certvalidator import ValidationContext
#   name, path = sys.argv[1:]
#   load = lambda p: x509.Certificate.load(open(p, 'rb').read())
#   roots = [load(f'{name}-root.der'), load(f'{name}-tsa-root.der')]
#   r = PdfFileReader(open(path, 'rb'))
#   try:
#       d = DocumentSecurityStore.read_dss(r)
#       certs = [x509.Certificate.load(x.get_object().data) for x in d.certs.values()]
#       ocsps = [O.OCSPResponse.load(x.get_object().data) for x in d.ocsps]
#       crls = [C.CertificateList.load(x.get_object().data) for x in d.crls]
#   except Exception:
#       certs, ocsps, crls = [], [], []
#   vc = lambda: ValidationContext(trust_roots=roots, allow_fetching=False,
#       revocation_mode='hard-fail', ocsps=ocsps, crls=crls, other_certs=certs)
#   st = validate_pdf_signature(r.embedded_signatures[0], signer_validation_context=vc(),
#       ts_validation_context=vc(), key_usage_settings=KeyUsageConstraints(key_usage=set()))
#   print(st.summary())
# 2026-09-28, pyHanko 0.37: all three INTACT:TRUSTED,TIMESTAMP_TOKEN<INTACT:TRUSTED>,
# EXTENDED_WITH_LTA_UPDATES,ACCEPTABLE_MODIFICATIONS; the control INTACT:UNTRUSTED.
# (The test signer states no key usage, hence the empty KeyUsageConstraints.)
#
# Then the real certificate authorities behind each timestamp authority, once, by
# hand. The probe's signer is self-made, so the whole path must REFUSE it (checked),
# and the authority's half is measured alone: its chain's real OCSP responders,
# each request timed and sized, the /DSS appended and read back good. From a
# folder of its own (the CoreFoundation trap under the timestamp block):
cargo build --release --manifest-path src-tauri/Cargo.toml --example sign-probe
mkdir -p /tmp/probe && cp src-tauri/target/release/examples/sign-probe /tmp/probe/
for srv in digicert sectigo globalsign; do
  /tmp/probe/sign-probe testdata/text-base14.pdf "/tmp/tpdf-sign-probe/lt-$srv" \
      --key rsa --timestamp "$srv" --long-term
done
# 2026-09-28, 26/26 each: DigiCert 2 OCSP (727 B each), Sectigo 2 (766 B each),
# GlobalSign 3 (1,737 / 1,713 / 1,691 B), 0.12 to 0.16 s gathered, a /DSS revision
# of 7,034 / 7,066 / 12,450 B. pyHanko with the system roots (security
# find-certificate -a -p /System/Library/Keychains/SystemRootCertificates.keychain),
# no fetching, hard-fail, over authority-lt.pdf: the timestamp INTACT:TRUSTED, and
# UNTRUSTED over the probe's B-T copy; openssl ocsp -respin over each dss-ocsp-<n>.der
# with -CAfile the system roots and -verify_other the dss-cert-<n>.der: all seven
# "Response verify OK", good. docs/PLAN.md says what this did not measure.
# Since PAdES B-LTA the probe then asks the same authority for an archive timestamp
# over authority-lt.pdf and writes authority-lta.pdf: 2026-09-28, 28/28 each, archive
# tokens of 6,005 / 6,633 / 7,657 B in 0.36 / 2.00 / 0.38 s, the signature's authority
# judged trusted_at_timestamp at the archive's time. pyHanko over each archive, the
# system roots, no fetching:
#   r = PdfFileReader(open('authority-lta.pdf', 'rb'))
#   st = validate_pdf_timestamp(r.embedded_signatures[-1],
#       validation_context=ValidationContext(trust_roots=roots, allow_fetching=False))
# INTACT:TRUSTED, coverage ENTIRE_FILE, on all three; qpdf --check clean.
#
# The window check that would show a visible signature placed and written in the
# real application does not exist: `tabs_check.py --phase sign` (above, with the
# window checks) signs invisibly only. By hand: File > Sign document..., choose Visible, choose
# what it shows in the Signature appearance panel (the preview redraws on each
# change), drag a rectangle, save; then signature-probe <copy> --mode integrity,
# uv run --with pyhanko testdata/check_signature.py --json <copy> for the reason
# and location, and look.
#
# A TEST SIGNING IDENTITY for doing that by hand (macOS). A developer's usual
# identity is often an Apple Developer ID, which is code signing only and which
# sign_cms::usable rightly refuses to offer. Make a self-signed one that states a
# document purpose, outside the repository, and import it yourself: an agent's
# import into the login keychain is refused as persistence, correctly.
#   cat > id.cnf <<'CNF'
#   [req]
#   distinguished_name = dn
#   prompt = no
#   x509_extensions = ext
#   [dn]
#   CN = tpdf TEST SIGNER - not a real identity
#   O = tpdf test
#   [ext]
#   basicConstraints = critical, CA:FALSE
#   keyUsage = critical, digitalSignature, nonRepudiation
#   extendedKeyUsage = emailProtection, 1.3.6.1.5.5.7.3.36
#   subjectKeyIdentifier = hash
#   CNF
#   openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 30 -config id.cnf
#   export PW=$(openssl rand -hex 16)
#   openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -name "tpdf TEST SIGNER" \
#       -out id.p12 -passout env:PW
#   security import id.p12 -k ~/Library/Keychains/login.keychain-db -f pkcs12 -P "$PW"
# OpenSSL 3 (Homebrew's; /usr/bin/openssl is LibreSSL). `-legacy` because
# `security import` does not read OpenSSL 3's default PKCS#12 encryption.
# `security find-identity -v` will NOT list it --- `-v` is trusted identities
# only --- and tpdf still offers it: its search does not ask for trusted ones.
# First used 2026-09-27: a visible signature over incr-signed.pdf read intact for
# both signatures under tpdf, pyHanko and `openssl cms -verify`, trust `untrusted`
# / `root`, and PDFKit drew the appearance. Remove it with
#   security delete-identity -c "tpdf TEST SIGNER - not a real identity"
# The OS key store, which no gate can reach as a store. What the gates do run on
# macOS: keystore::tests::the_os_signs_a_revision_that_the_verifier_calls_intact
# hands SecKeyCreateSignature a key made in memory by SecKeyCreateWithData ---
# no keychain touched --- through the same function the shipped path calls.
# Two more are ignored and run by hand. On macOS, the chain API (the first
# SecTrust evaluation in a process measured 5.4 s and 19.2 s):
cargo test --manifest-path src-tauri/Cargo.toml --lib keystore::tests::the_chain_api -- --ignored
# On Windows, the store end to end: a CNG certificate created in CurrentUser\My,
# found by identities(), used to sign, and removed with its key. First run
# 2026-09-27 on MOTHERSHIP (Windows 11, pwsh 7): RSA and ECDSA P-256 both signed
# through CNG and read intact. The first attempt failed in its CLEANUP and left
# both probe certificates, keys included, in the store: PowerShell 7 knows
# `-DeleteKey` only once the Cert:\ path is bound, so the path now comes first,
# and every removal is attempted before any failure is reported. After a run,
# `Get-ChildItem Cert:\CurrentUser\My | ? Subject -like 'CN=tpdf-keystore-probe-*'`
# must list nothing.
cargo test --manifest-path src-tauri/Cargo.toml --lib keystore::tests::the_windows_store -- --ignored --nocapture
# There is no macOS identity-store test, and the reason is recorded rather than
# worked around: creating a keychain file (`security create-keychain`,
# SecKeychainCreate) adds it to the user's keychain search list.
#
# The window check that drives signing end to end is `tabs_check.py --phase
# sign`, above with the other window checks: it signs with an identity already
# in the reader's store, named by --identity, and the person at the machine
# answers the keychain. Built and run green 2026-09-28 (46/46). By hand, on a machine
# whose store already holds a signing identity: a build of the app, any PDF,
# File > Sign document..., then
#   cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
#       <the signed copy> --mode integrity
# which holds tpdf's verdict on the written file against pyHanko's.

# TRUST (2026-09-27): what the operating system's trust store says about each
# intact or weak signature's signer, through the REAL system store, offline ---
# and the same scan again in a child under worker::SANDBOX_PROFILE, which must
# first fail to read its own input file (the proof its sandbox is live) and then
# print identical lines. That second half is the measurement trust.rs rests on:
# the worker can reach trustd. Prints signatures by number, never by field name,
# because it is meant for real documents. macOS arm64, 2026-09-27: the fixtures
# read untrusted (their pyHanko test roots are in no store); the three real signed
# documents step 1 used read intact/untrusted(root), weak/untrusted(root),
# intact/untrusted(root), identical under the sandbox, and `security verify-cert
# -p basic -L` over the same certificates agrees (CSSMERR_TP_NOT_TRUSTED). On
# Windows the sandboxed half is [SKIP]: a Windows worker's containment is applied
# by its parent, where this process cannot put itself.
cargo run --release --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
    testdata/incr-signed.pdf --mode trust
# The unreachable-service case, which the gates hold only as a code mapping: run
# the probe with trustd denied. Expect `Unchecked / Some(Unavailable)`, and the
# sandboxed-half comparison [FAIL] (a nested sandbox_init refuses); what matters
# is the first line --- `untrusted` there is the defect in docs/TRAPS.md.
sandbox-exec -p '(version 1)(allow default)(deny mach-lookup (global-name "com.apple.trustd") (global-name "com.apple.trustd.agent"))' \
    src-tauri/target/release/examples/signature-probe testdata/incr-signed.pdf --mode trust
# trust::tests needs no store and no login session: Anchors::Only gives each
# evaluation in-memory roots (SecTrustSetAnchorCertificatesOnly on macOS, an
# exclusive-root chain engine on Windows), and the one system-store control reads
# a root out of SecTrustCopyAnchorCertificates, read-only. None is ignored. The
# Windows half first ran on the Windows CI job (9ad0355, 2026-09-27): all 14
# trust tests passed there, the exclusive-root engine included.

# Marks: does a highlight a reader makes land on the words they made it from?
# Run ALL FOUR modes, and run them on BOTH geometry fixtures -- that is not
# thoroughness, it is the only way two of the checks can fail at all. Measured by
# mutation: dropping the crop-box origin from the write path reddens
# `links-cropped` and NOTHING else, and mapping with no rotation reddens
# `rotated-90` and nothing else. An upright, uncropped page cannot tell either
# mistake from correct behaviour, and `--mode roundtrip` says so in its output.
#
#   roundtrip  writes a mark, reads it back through `annots.rs` -- a separate
#              implementation of the inverse mapping -- and compares. Also pins
#              the `/QuadPoints` corner order against the bytes.  9/9
#              Since 2026-08-18 the note is *typed* through `renote` rather than
#              passed at creation, so this covers the route a reader takes: a
#              highlight made with nothing to say, and the words added after.
#              Both routes end in the same `/Contents`.
#   ink        renders the saved page and counts wash per quad, with the SOURCE
#              page as the control. 90-96% of each quad across the corpus.  3/3
#   noap       the same with the appearance stream stripped, so the wash is the
#              renderer's own, from `/QuadPoints`. Nothing else reads those
#              numbers: a mutation reordering every corner passed every other
#              mode.  3/3
#   legible    the glyphs survive the wash. Removing `/Multiply` leaves 0 of
#              2,744 ink pixels on `text-base14`.  2/2
#   rule       where a line kind's rule actually lands, in rendered pixels. The
#              check no file-level assertion can make: PDFium generates its own
#              appearance for a markup annotation that has none, so this is what
#              says it honours ours. Two assertions, and the second is what
#              tells the kinds apart -- an underline puts every pixel in the
#              bottom third of the quad and NONE in the middle, a strikeout the
#              other way round. Refuses `--kind highlight`, which fills its quad
#              and is what `--mode legible` measures.  4/4 per kind
#   outline    that a box is a frame and not a filled rectangle, in pixels. The
#              one measurement no file-level assertion can make: a stroked box
#              and a solid block of colour satisfy the subtype, the rectangle,
#              the absent quads and the presence of an /AP equally, and a solid
#              block hides the figure the box was drawn around. Three readings
#              -- the source page as control, the whole quad, the middle inset
#              clear of the stroke -- plus the thinner of the two horizontal
#              edges' thickness, which is what says the stroke was not clipped
#              in half by the /BBox. Renders at 4x whatever --scale says, and
#              prints that it did: at 2x a full stroke is 3 px against a
#              clipped 1.5 and antialiasing swallows the difference. Refuses
#              every kind but square.  4/4
#   refuse     the refusals, with a control proving a real mark is still taken.
#
# `--kind highlight|underline|strikeout|note|square` chooses what to write, and
# every mode that writes a mark takes it: `--mode roundtrip --kind strikeout`
# re-runs the whole file check against a `/StrikeOut`, whose subtype, appearance
# geometry and opacity all differ and whose quads do not. The last two are the
# kinds a reader places rather than selects, and they are what makes the quad
# count a real assertion rather than a formality: both expect ZERO, in the same
# run where the three markup kinds expect one, so a writer that stopped emitting
# quads for everything is not mistaken for one that correctly omits them.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/rotated-90.pdf --mode roundtrip
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/links-cropped.pdf --mode roundtrip
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/rotated-90.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/links-cropped.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode noap
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/columns.pdf --mode noap
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode legible
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode refuse
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind underline
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind strikeout
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode rule --kind underline
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode rule --kind strikeout
# `--mode rule` on every turn. Which third of the quad is "under" has four
# answers and the mode reads them off a table; `rotated.pdf` carries /Rotate 0,
# 90, 180 and 270 on pages 0 to 3, so one sweep exercises all of it. Until
# 2026-08-20 this mode was only ever pointed at an upright page and split the
# quad down the screen regardless, which reported 330/330/332 and TWO FAILURES
# on a sideways underline that was drawn correctly. 4/4 on each of the eight.
for page in 0 1 2 3; do for kind in underline strikeout; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/rotated.pdf --page $page --mode rule --kind $kind
done; done
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind note
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind square
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode outline --kind square
# The ellipse, through the same mode and the same three readings -- plus a fourth
# that is the whole reason it takes this kind at all. `--kind square` above and
# `--kind ellipse` below are a PAIR: the corner check asserts emptiness for the
# ellipse and INK for the box, so running only one of them leaves the other
# direction untested, and an emptiness assertion whose control never runs cannot
# tell "the corner is clear" from "the renderer drew nothing".
#
# Everything else in this mode passes for a rectangle drawn in place of an
# ellipse -- measured, by mutating `Paint::Ellipse` to `Paint::Outline`: whole
# quad, inner half and edge thickness all stay green and only the corner fires.
# 5/5 on each of the two.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode outline --kind ellipse
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind ellipse
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind ellipse
# Whether a foreign renderer reads a comment icon's `/C` at all, which settles the
# unchecked half of `docs/PLAN.md` open question 8. Two files differing only in
# `/C`, rendered by both readers, compared byte for byte -- no hue, no threshold.
# 5/5, and the numbers are the finding: PDFKit moves 439 px and PDFium moves 0,
# so Preview shows the reader's colour and tpdf's own renderer does not.
#
# THE FIRST CHECK IS A CONTROL AND IS NOT OPTIONAL. It runs the same comparison
# on a HIGHLIGHT, whose appearance stream carries the colour, so both readers
# must move -- 3379 and 3546. Without it the PDFium reading is an emptiness
# assertion with nothing proving the instrument looked: sending one colour twice
# leaves that check GREEN while three others go red, which is what proves the
# pair has to be read together.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode iconcolor --kind note
# Reference run: PDFKit 439 px moved, PDFium 0, controls 3379/3546 on a highlight.
#
# `hidden-probe` is the sequel and asks the question the ranked overlay work
# turns on: does PDFium honour /F bit 2, Hidden, PER ANNOTATION? It does. The
# fixture wants a highlight and a comment a hundred points apart on one page,
# which `--mode preview --kind note --out` builds on top of an already-marked
# file; the note's /Rect is then moved by an equal-length byte edit so the xref
# stays valid.
#
#   src-tauri/target/release/examples/hidden-probe both.pdf hidden.pdf \
#       --source testdata/text-base14.pdf \
#       --note-rect 50,200,110,245 --quad-rect 65,112,300,124
#
# 4/4 on 2026-08-31: 3919 px for the fixture's two marks, 373 moved by the flag,
# 2815 still in the highlight's quad, 0 left in the comment's rectangle. Its
# control is to pass the VISIBLE file twice, which reddens the two live checks at
# 0 and 373 -- and 373 in the icon's rectangle is also what proves that rectangle
# is aimed at the icon rather than at blank paper.
#
# `--out <path>` keeps the four files it writes --- two notes and two highlights,
# each pair differing only in `/C` --- under the temporary directory as
# `tpdf-iconcolor-<pid>-<Kind>-<blue|red>.pdf`. That is how a reader this probe
# cannot drive gets measured: on 2026-08-31 those files went into Adobe Acrobat
# DC, whose window was captured at its own accessibility bounds and diffed by
# region. Acrobat honours `/C` --- 873 px in the icon, 0 beside it, 0 for the same
# file opened twice, 24,642 for the highlight. See docs/PLAN.md §10 q8; and note
# that a whole-window diff is wrong here, because the tab title carries the
# filename and differs for that reason alone.
# The stamp. `--mode stamp` exists because `--mode outline` CANNOT FAIL for this
# kind: a stamp is a box with a word in it, so every reading that mode takes of a
# box is satisfied by a stamp except the one it has backwards -- it requires an
# empty middle and a stamp's middle carries its word. The new mode reads three
# bands, and each is a different way of drawing a stamp wrong: the whole quad
# (nothing was drawn), the middle third (a box), and the top edge (a text box).
# 4/4; the reference run reads 11,309 px in the quad, 717 in the middle and 513
# on the top edge, against 0 on the source page.
#
# `--mode preview` is the strongest of the three and PDFKit is why: an
# independent parser reads the annotation as `Stamp` and an independent RENDERER
# draws 1,306 px across its rectangle, neither of them ours.
#
# `--stamp <name>` picks which of the four; it defaults to `approved`.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode stamp --kind stamp
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind stamp
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind stamp --stamp draft
# The squiggle and its control, and they are a PAIR for the corner check's exact
# reason: `--mode wave` asserts that a squiggle puts ink in the strip above where
# an underline's rule stops, and that an underline leaves that strip EMPTY.
# Running only the squiggle leaves the emptiness untested; running only the
# underline is an emptiness assertion that "the renderer drew nothing at all"
# satisfies just as well.
#
# `--mode rule` is run for the squiggle too and passes -- its ink is under the
# baseline -- but it CANNOT tell a squiggle from an underline, because thirds of
# a quad put both kinds in the same one. That is why `--mode wave` exists.
#
# Upright pages only: `--mode wave` refuses a turned page rather than repeating
# `--mode rule`'s four-row turn table for a second mode. 3/3 on each of the two.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode wave --kind squiggly
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode wave --kind underline
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind squiggly
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode rule --kind squiggly
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind squiggly
# The text box, whose /AP holds words rather than a shape. `--mode rule` and
# `--mode outline` both refuse it and should: its ink is wherever its words fall,
# which depends on how many there are, so thirds of a quad do not describe it at
# all rather than describing it coarsely.
#
# `--mode preview` is where it is measured, and for this kind that check asserts
# something the others cannot: the drawn line is as wide as `textbox::advance`
# predicts (110.0 pt against 109.4). That is the Helvetica widths table checked
# through PDFKit, and `helvetica-probe` below checks it through PDFium.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind textbox
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode preview --kind textbox
# The Helvetica widths, against what PDFium actually draws. **The only evidence
# that table is right**: it is 95 numbers written out by hand, a wrong entry
# still draws and still wraps, and any unit test would compare the table against
# itself. Needs no fixture -- it writes its own page.
#
# Every string must come in UNDER its predicted advance and none may exceed it:
# ink runs from the first glyph's left edge to the last one's right, and an
# advance includes the trailing side bearing. 8/8.
cargo run --release --manifest-path src-tauri/Cargo.toml --example helvetica-probe
# Freehand ink. `--mode strokes`, NOT `--mode ink` --- that name was taken nine
# months earlier by the coverage measurement above and means something else
# entirely, which is the collision `MarkKind::Ink` walked into.
#
# The fixture is two strokes along the run with a wide gap, and the gap must be
# EMPTY: a writer that flattened `/InkList` into one path joins the first stroke
# to the second with a diagonal straight through it. The two outer bands are
# read as well, because a writer that emitted only the first stroke also leaves
# the gap empty. Renders at 4x whatever `--scale` says, and says so.
#
# SEVEN checks, not five, since 2026-08-20: the two added are how LONG each
# stroke is. The other five passed on `rotated-90` while every stroke came out
# at a nineteenth of its length -- 545 px against 10200 -- because two stubs at
# the ends of a rectangle put ink in both outer thirds and none in the middle,
# exactly as two full-length strokes do. Expect `249.0 pt of 249.2` on the
# sideways page and `255.5 of 255.8` on the upright one; the bound is 80%, so a
# green run has about a fifth of the rectangle in hand rather than the two
# hundredths of a point this mode's band arithmetic once stood on.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode roundtrip --kind ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode strokes --kind ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/rotated-90.pdf --mode strokes --kind ink

# `--out PATH` keeps the marked copy instead of removing it, for opening in
# Preview or Acrobat by hand. Worth doing once per release: the probe proves the
# geometry and the pixels PDFium draws, and what it cannot prove is that somebody
# else's reader shows the mark at all.
#
# `--mode preview` IS THIS STEP NOW, and the paragraph below is the by-hand run
# that came first. It opens the saved file with PDFKit and asks eight questions
# of it; run it over every kind before a release:
for kind in highlight underline strikeout note square ink; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/text-base14.pdf --mode preview --kind $kind
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/rotated-90.pdf --mode preview --kind $kind
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/links-cropped.pdf --mode preview --kind $kind
done
# 18 runs, all green: 8/8 on an upright page, 7/8 plus one [SKIP] on the turned
# one, and 9/9 for a note, which asks two questions instead of one about its
# rectangle. macOS only -- Windows.Data.Pdf renders but exposes no annotation
# object model, so the mode refuses there rather than half-running.
#
# THE WINDOWS COUNTERPART IS `--mode winreader`, and it asks a strictly smaller
# question. The sentence above is about METADATA -- the subtype, the author, the
# note, the rectangle -- and it is right about those. It is not about the pixels:
# a renderer draws our mark or it does not, whether or not it will answer
# questions about it. So this is a before-and-after inside ONE reader rather than
# a differential between two. Windows only; on macOS it refuses and names
# `--mode preview` as the mode that answers there.
#
# Five checks per kind, and TWO of them are controls. The renderer must draw the
# same page identically -- in PIXELS, not in bytes, because WinRT's BMP encoder
# is not reproducible and a byte comparison would condemn a correct renderer.
# And the mark's rectangle must be a MINORITY of the page, or "the difference is
# inside the rectangle" is true by construction.
#
# All nine kinds, 5/5 each, on `text-base14.pdf` at one pixel per point.
# Reference run: highlight 2,973 px changed with 100% inside its own rectangle,
# ink 1,536, ellipse 1,205, square 802, squiggly 576, text box 448, underline and
# strikeout 254 each. A run whose px counts differ by a few is antialiasing; one
# whose INSIDE share drops below 100% for anything but a note is not.
#
# THE NOTE IS THE EXCEPTION AND IT IS NOT A DEFECT. `Windows.Data.Pdf` replaces a
# /Text rectangle with its own icon, as PDFKit does -- and CENTRES it where PDFKit
# anchors it to the top-left corner. A 254x14 rectangle at 60,111 changes an
# 18x19 box at 178,109. So 84.4% of its pixels are inside and that is correct;
# the mode asks whether the icon is small and sits on the rectangle instead.
# `docs/TRAPS.md` has it under "The second reader substitutes the same icon".
for kind in highlight underline strikeout squiggly note ink textbox square ellipse; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/text-base14.pdf --mode winreader --kind $kind
done
#
# ACROBAT IS THE THIRD READER AND THE ONLY ONE THAT NEEDS A PERSON. It exposes no
# automation interface in the copy installed here -- Adobe Acrobat 26.001.21789,
# Reader mode, no `AcroExch.*` COM classes -- so the instrument is a folder of
# files and a pair of eyes. Done 2026-08-31; `docs/PLAN.md` has the results. Five
# minutes, and it is the only check that can see a repair dialog, which is what
# the strictest structural reader in circulation says about an incremental save
# written by `lopdf` over a file it did not create.
#
# DERIVE THE EXPECTATIONS FROM THE FILE, do not write them from memory. The
# handover for the first run described the mark as covering "about `...jumps
# ove`" when 40 characters of Helvetica end after `lazy `, so a byte-exact render
# was reported back as a possible defect. Wrong the other way it would have been
# reported as a confirmation. `docs/TRAPS.md`: "A handover telling a person what
# to expect is a second implementation".
DROP="$HOME/Desktop/acrobat-check-$(date +%F)"; mkdir -p "$DROP"
cp testdata/text-base14.pdf "$DROP/00-CONTROL-unmarked.pdf"
for kind in highlight underline strikeout squiggly note square ellipse textbox ink stamp; do
  cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
      testdata/text-base14.pdf --mode roundtrip --kind $kind --out "$DROP/$kind.pdf"
done
# The twelfth file is the one that found something: `square.pdf` with its /AP
# stripped, 15 bytes smaller and otherwise identical. Acrobat draws it.
cargo run --release --manifest-path src-tauri/Cargo.toml --example annot-probe -- \
    testdata/text-base14.pdf --mode noap --kind square --out "$DROP/square-no-appearance.pdf"
#
# ASK FOUR THINGS of each file: a repair or damage dialog on open; the mark drawn
# on line 1 in the colour written; the Comments pane listing it as `annot-probe`
# with the body `written by annot-probe`; and NOTHING of either on the control.
#
# WHAT IT CANNOT CATCH is in the mode's own doc comment as a measured table, and
# is worth reading before trusting a green run: every check is between two
# READERS, so a writer that moves something legally moves it for both. A /Rect
# shifted three points sideways passes here and fails --mode roundtrip; a
# /Subtype written as the wrong one passes here and fails a unit test; a missing
# /AP passes here because PDFKit draws its own.
#
# DONE BY HAND FIRST, 2026-08-20, and this is that record -- a by-hand step that
# leaves no record is a step nobody can tell was skipped. All six kinds were written to
# `text-base14.pdf` and opened with PDFKit, which is what Preview is, through a
# throwaway Swift harness reporting the annotation list and diffing the render
# against the unmarked original. Every kind came back with the right /Subtype,
# author and note, at the /Rect it was written at, painting pixels the source
# page does not: highlight 81% of its own box, note 77%, ink 37%, box 27%,
# strikeout 9%, underline 8%. The control -- the original against itself -- is
# 0 annotations and 0 pixels changed.
#
# THIS IS PHASE 2's EXIT CRITERION and it has no standing check. Two things a
# repeat run must know, both measured the same day and both able to produce a
# confident wrong answer:
#
#   * On a /Rotate page PDFKit draws the content ROTATED into an UNROTATED
#     frame. `page.bounds(for: .mediaBox)` answers 612x792 for a page poppler
#     renders at 792x612, and six of rotated-90's twelve lines are clipped off
#     the side. Meanwhile `annotation.bounds` returns the raw /Rect, unrotated.
#     So the annotation layer and the content layer sit in different frames, and
#     a coverage figure "inside its own bounds" reads 0.0% for a mark that is
#     drawn correctly. Do the positional half on an upright page only.
#   * poppler's `pdftoppm` honours /Rotate properly and draws annotations, so it
#     is the better oracle for a turned page -- and it is how the transposed ink
#     above was found. Not a dependency and not on every machine: a spike tool,
#     not a gate.

# geometry-probe: the page PDFium lays out, against the page the document describes.
#
# `FPDFPage_GetMediaBox` does not walk `/Parent`, so a page inheriting its box
# from an ancestor gets no answer from PDFium -- and `FPDF_GetPageWidthF` then
# reports `width x width` for one that also carries a quarter turn. That is a
# document laid out square with its content clipped off the sheet, and nothing
# errors. `RawDocument::page` repairs it by handing PDFium the box
# `pagetree::displayed_boxes` derived; this is what says so.
#
#   size    the displayed width and height PDFium reports equal the page tree's.
#           `400.0 x 400.0 against 600.0 x 400.0` before the repair.
#   box     `crop_pt`, which every coordinate on the page is measured from, is
#           that rectangle in the page's OWN space. Not implied by the size
#           check: before the repair this page answered `[0 0 600 400]`, the
#           right size in the wrong convention.
#   ink     the page draws something -- the reader-visible half, and the one no
#           structural check makes. 1, 3 and 0 inked pixels of ~26,600 before,
#           1013, 1062 and 1317 after. The floor is 0.1%, and the margin is
#           printed either way.
#   cost    the page tree was parsed IFF some page needed it. Every number above
#           is identical whether the parse happened or not, so a repair that
#           parsed every document would be invisible here and would put a whole
#           `lopdf` pass on the path a reader waits on.
#
cargo run --release --manifest-path src-tauri/Cargo.toml --example geometry-probe -- \\
    testdata/inherited.pdf --lib vendor/pdfium/lib
cargo run --release --manifest-path src-tauri/Cargo.toml --example geometry-probe -- \\
    testdata/links.pdf --lib vendor/pdfium/lib
#
# **Run both, and the second is not a formality.** `inherited.pdf` is the only
# fixture in the corpus PDFium has no `/MediaBox` for, so it is the only one
# where the repair does anything -- and therefore the only one where the cost
# check passes for the wrong reason. A document that states its own boxes is
# where "the page tree was not parsed" has to hold, and it is the one mutation
# (`geometry: parse the page tree for every document`) that nothing else can
# catch. Measured 2026-08-24 on Windows (`--lib vendor/pdfium/bin` there):
# 10/10 and 25/25.
#
# Three mutations, all caught, in `scripts/mutate_viewer.py` under `geometry:`;
# four more against the unit tests in `scripts/mutate_rust.py`.
#
# **It runs on any fixture, and the ink check has a precondition it cannot
# assert.** Every other window corpus is green (4/4 to 37/37) except
# `encodings.pdf`, which is 9/10: its page 2 extracts fourteen Japanese
# characters and renders 0 of 56,600 pixels, being `/UniJIS-UCS2-H` over a
# non-embedded `KozMinPro-Regular` that needs a substituted font. Size and box
# pass on that page, so it is a font on this machine and not a box. The probe's
# own header says why no guard is written for it.
#
# **`inherited.pdf` became a window corpus on 2026-08-24**, at 272/272. This
# said it was deliberately not one, on the strength of a single red check the
# agree phase reported at 27x; that turned out to be the turned-mark defect
# `turned-probe` covers, not a text-box one. What is left is a skip rather than
# a failure: on a 400-point page the phase's synthetic text box has no room for
# a line and both renderers correctly draw nothing, so it is left out of the
# comparison with the measurement in its detail line.

# turned-probe: does a mark land where the reader put it, on a turned page?
#
# `save::user_quads` maps a mark out of the reader's frame and into the page's
# own, which is right for the rectangle and wrong for anything drawn inside it
# that has a direction. On `/Rotate 90` an underline came out as a rule down the
# LEFT edge of the words, a strikeout as a vertical line, a squiggle down the
# left, a text box as a column wrapped to the box's height, and a stamp
# sideways at the wrong size. `/Rotate 90` is what a scanner writes.
#
# One mark of each kind on each page of a document whose four pages carry
# `/Rotate 0`, `90`, `180`, `270` and are otherwise identical -- its generator
# says so in as many words, which is the whole design: page 0's reading is the
# reference and the other three must match it, so nothing is predicted and no
# expected number is written down. Each page is rendered before the mark and
# after it, and the pixels that moved are reduced to a coverage and an ink box,
# both as fractions of the box the reader dragged.
#
cargo run --release --manifest-path src-tauri/Cargo.toml --example turned-probe -- \\
    testdata/rotated.pdf --lib vendor/pdfium/lib
#
# 29/29 as measured 2026-08-24 on Windows (`--lib vendor/pdfium/bin` there).
# Four mutations in `scripts/mutate_viewer.py` under `turned:`, all caught;
# seven more against the unit tests in `scripts/mutate_rust.py` under
# `turned marks:`.
#
# **It is the only check on the squiggle anywhere**, that kind being a stroked
# zigzag: there is no `re` operand for a unit test to read and no line count
# that moves, so what it looks like is a question about pixels.
#
# Two things in the output that are not defects. A highlight's COVERAGE differs
# across turns and is deliberately not compared -- `/BM /Multiply` leaves a
# pixel alone wherever the paper is already dark, so its coverage is a reading
# about the page's content, and this fixture's type is in a different part of
# the display at every turn. Its extent is compared instead. And the last check
# is about the whole set rather than one kind: two kinds drawn differently have
# to READ differently, or a run in which every kind drew the same thing would be
# entirely green.

# insert-text-check: a replacement typed on an inserted page, read back by two
# parsers that share no code with tpdf.
#
# `save::import_tests` proves the writer with lopdf on both sides, which cannot
# notice a file it agrees with itself about. This runs the same path through
# `insert-text-probe` and then asks `qpdf --check` about the structure and
# `pypdf` about the words.
#
# The assertion that bites is NOT "the replacement is there". Page n of the
# opened document and page n of the file its pages came from are two different
# pages, and the fixtures make that collision real: page 1 of `text-base14.pdf`
# is inserted after page 1 of `links.pdf`, which has a page 1 of its own with
# different words on it. A save that edited the wrong document would produce a
# valid file, with the right page count, containing the replacement -- on the
# wrong page. So the check reads the opened document's page 1 back too, and
# every other page of it, and counts the pages the replacement appears on.
#
# Proved by control 2026-09-20: routing every replacement to the opened document
# (the mutation `save: import: write every replacement into the opened document`)
# makes it exit 1. Seven checks, all passing on `main`.
#
cargo build --manifest-path src-tauri/Cargo.toml --example insert-text-probe
uv run --with pypdf scripts/insert_text_check.py
#
# The probe alone, for a different pair or a different page:
#
#   insert-text-probe <base.pdf> <other.pdf> <out.pdf> \
#       [--page N] [--after N] [--replacement TEXT]
#

# redact-import-check: a region removed from the reader's own page in a document
# that also holds pages of another file, read back by two parsers that share no
# code with tpdf.
#
# Unlike `insert-text-check` this goes through the SANDBOXED WORKER for both the
# write and the verification (`save::InWorker`, which is what the application
# uses); every other redaction probe passes `save::Here`. It is also the only
# harness that drives a removal and an import in one plan.
#
# Two runs, and the second is the one worth having:
#
#   plain   the other file carries neither word. The scan finds nothing, the
#           file verifies, no note is added -- which is what says the fix is not
#           the old document-wide refusal wearing a different coat.
#   --echo  the probe synthesises the other file so that its one page prints the
#           very word the region covered. `verify::scan` reads the whole file, so
#           it reports that word as still present -- and since 2026-09-21 it also
#           says WHICH PAGE, which is what this run now checks against pypdf
#           saying the same thing from outside: the hit is placed on the inserted
#           page exactly, the control word that survives on the marked page is
#           placed there instead, the reason a reader sees names that page number,
#           and `redact::inserted_pages_note` -- the sentence that existed to
#           disclaim an answer -- is asserted ABSENT.
#
# The `--keep` control must not reach either note. It survives on the marked page
# by construction, so handing it to `redact::marked_pages_note` reports a removal
# that did not take on EVERY run; the probe narrows the report to the removal's
# own needles first. The comment saying so had been written for the other note and
# was not applied to this one -- `docs/TRAPS.md`, *A warning written for one
# consumer of a control is not attached to the control*.
#
# The inserted page goes in FRONT of the marked one, so the marked page's number
# in the base file and its slot in the output are different numbers. A writer
# that addressed the removal by slot would strip the inserted page instead, and
# the file would still be valid, still have the right page count and still be
# missing the word.
#
# 22 checks over the two runs, all passing on `main` 2026-09-21 (17 until the
# attribution landed). Proved by control: the mutation `save: address a removal
# by output slot rather than by baseline page` reddens the unit test that owns
# the same property, and `verify: follow the dictionary keys that leave the page`
# reddens the walk's own.
#
cargo build --manifest-path src-tauri/Cargo.toml --example redact-import-probe
uv run --with pypdf scripts/redact_import_check.py
#
# The probe alone, for a different fixture or word:
#
#   redact-import-probe <base.pdf> <other.pdf> <out.pdf> \
#       [--library DIR] [--needle W] [--keep W] [--echo]
#
# `--echo` writes its own source file as `echo-source.pdf` beside the output; it
# refuses an output named the same, because that path is removed before the save
# and the refusal a reader would then see names a missing insert source.
#
# `--page` is the zero-based page of the OTHER file, `--after` the slot of the
# opened document it lands behind. It prints one JSON object on stdout and
# everything else on stderr. Without `--replacement` it shortens the original
# to its first word, which fits by construction -- a longer one is refused by
# the writer, and that refusal is a correct answer rather than a probe failure.

# merge-probe: a merged document against the two that went into it.
#
# `save::write_merged`'s unit tests are lopdf reading back what lopdf wrote, plus
# a page count from the OS parser. Both say the tree is right. Neither says
# PDFium -- the engine tpdf renders with -- draws page seven, or that the page it
# draws is the page that was merged in. So this compares the merge against its
# SOURCES, per page, three ways:
#
#   size    the page keeps the size it had in its own file. The oracle is
#           `pagetree::displayed_page` reading the source's object graph, NOT
#           PDFium's reading of it -- see the trap about a rotated page whose box
#           is inherited, which PDFium answers `width x width` for. PDFium's own
#           reading is printed beside it so a disagreement is visible.
#   ink     the merged page draws something at all, which is what a lost
#           resource dictionary looks like from outside. A second check compares
#           it against the source's render, and SKIPS with the reason where
#           PDFium reads that source page at the wrong size, since its render is
#           then not a baseline.
#   text    the same code points come back. The one check that needs the fonts
#           as well as the stream: a page whose /Font went missing still renders,
#           because PDFium substitutes, and extracts the wrong code points.
#
cargo run --release --manifest-path src-tauri/Cargo.toml --example merge-probe -- \
    testdata/rotated.pdf testdata/links.pdf --lib vendor/pdfium/lib
cargo run --release --manifest-path src-tauri/Cargo.toml --example merge-probe -- \
    testdata/rotated.pdf testdata/inherited.pdf --lib vendor/pdfium/lib
#
# Measured 2026-08-24 on Windows (`--lib vendor/pdfium/bin` there): 50/50 on the
# first, 30/30 on the second. Four mutations, and the two that survive are as
# informative as the two that do not:
#
#   merge::append shifting by 0                   7/23   caught
#   merge::append grafting nothing                16/21  caught
#   pagetree::detached_page materialising nothing 21/30  caught -- ONLY on the
#                                                 second run; the first stays at
#                                                 50/50, because no page of
#                                                 rotated.pdf or links.pdf
#                                                 inherits anything. That is why
#                                                 `testdata/inherited.pdf` exists.
#   detached_page keeping /Parent                 30/30  SURVIVES, correctly: it
#                                                 drags the source tree in as
#                                                 unreferenced objects and
#                                                 changes nothing a renderer can
#                                                 see. The unit test
#                                                 `the_walk_does_not_leave_the_page_it_started_from`
#                                                 is what covers it.
#
# **The second run was 27/27 with 3 skipped until the geometry repair the same
# day.** The three were the ink comparisons, skipped because PDFium read the
# source page at the wrong size; it does not any more, so they run. The two
# `detached_page` rows were re-measured against the repaired renderer and the
# two `merge::append` rows were not -- they are on the first corpus, which the
# repair cannot reach, and its clean total is unmoved at 50/50.
#
# One figure did NOT reproduce and is recorded rather than explained: the
# materialisation mutation's first-corpus total was written down as 38/38 before
# the repair and measures 50/50 after, so twelve checks that skipped under it no
# longer do. The verdict is the same either way -- it survives on a corpus that
# inherits nothing, which is the whole point of the row -- and chasing the
# denominator was not worth a run. If you are about to rely on that number,
# measure it rather than reading it.
#
# `--emit PATH` keeps the merged file instead of deleting it, which is how you
# hand one to `backend-probe` (40/43, 3 skipped, through the sandboxed pool) or
# open it in the viewer by hand.
#
# A merged document is NOT a window-sweep corpus, and that was measured rather
# than assumed: `viewer_check.py` over a merge of rotated.pdf and links.pdf is
# 297/300, and the three are the mixed-page-size checks -- "the page is laid out
# sideways: wanted 0.8312 for a 612x792 page" -- which derive what they expect
# from page 1's aspect ratio. That is the documented reason `links-rotated` and
# `comments-rotated` are excluded from the sweep. Merging two documents of ONE
# page size would pass, and would have removed the only property that makes a
# merged document different from either input.

# crop-probe: the crop the reader sets, against PDFium rather than against us.
#
# Four modes, and the first is the one the whole design rests on:
#   follows    setting a page's crop box moves EVERYTHING that reads it -- the
#              reported size, the origin every character box is measured from,
#              the render, and the text mapping. Its control is the restore:
#              asking for no crop must put every one of those back, or the page
#              cache turns one request's crop into everyone's. Its FIRST check
#              is the only one here not derived from `crop_pt`: PDFium's page
#              size against `pagetree::displayed_page`'s, read from the same
#              file through lopdf. Every other check would survive a corrupt
#              crop rule, because their before and their after corrupt
#              together.                                          6/6
#   content    the measured content box is inside the page and encloses
#              something.                                        2/2
#   geometry   the crop's rectangle inside the file's page and the cropped
#              page's own reported size must agree -- two derivations, one
#              through `text::to_device`'s rotation table and one from PDFium.
#              Its control is the uncropped case, where the rectangle has to be
#              the whole page at the origin.                      3/3
#   ink        cropping to the content box raises the ink per rendered pixel.
#              The reader-visible claim, and the one no structural check makes.
#              Skips with a stated reason on a page whose ink already reaches
#              its edges, which is the honest control for every other row.
#
# Green on all fourteen corpora. Two rows are worth running by hand after any
# change to the rotation table or the box arithmetic:
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/rotated-90.pdf --mode follows
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/rotated-90.pdf --mode geometry
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/links-cropped.pdf --mode content
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/columns.pdf --mode ink
cargo run --release --manifest-path src-tauri/Cargo.toml --example crop-probe -- \
    testdata/vector-heavy.pdf --mode ink

# `rotated-90.pdf` is not one fixture among fourteen here. It is the page that
# proves `FPDFPage_GetCropBox` and `FPDFPage_SetCropBox` are not inverses: with
# no `/CropBox` of its own the getter answers with the DISPLAYED rectangle, and
# writing that back shrinks a 612x792 page to 612x612. See `docs/TRAPS.md`.
# `vector-heavy.pdf` is the other end -- an A0 drawing with no margins, where the
# right answer is to crop nothing.

# `--mode agree` needs NO manifest since 2026-08-16, which is the point of it:
# it resolves the same outline through PDFium and through lopdf and compares the
# two lists, so any document with an outline is a test. Run it over real files --
# that is where it earns its keep, and the fixture offers 6 entries against 421:
#   find ~/Downloads ~/Desktop -name '*.pdf' -type f | while read -r f; do
#     cargo run -q --manifest-path src-tauri/Cargo.toml --example links-probe -- \
#       "$f" --mode agree 2>&1 | grep -E 'differ:|entries agree|no outline'
#   done
# Measured 2026-08-16 over 44 real documents: 10 agree, 0 differ, 34 have no
# outline, and 1 entry differs only in the reason PDFium cannot see -- a /Dest
# naming a destination that resolves nowhere, which PDFium reports as "no
# destination". That pair is allowed by name; any other difference fails.

# Both whole-document scans now report `pages_missed` -- pages PDFium has that
# `lopdf` could not account for. Worth knowing before reading a zero: swept over
# every fixture on 2026-08-16 it is 0 everywhere, so the two parsers agree about
# page count on every document PDFium will open. `incr-encrypted-pw.pdf` was the
# usual counter-example and was not one then, because PDFium would not open it
# at all -- and it IS one now, since tpdf can ask for a password. Without the key
# `lopdf` reads no objects and reports 0 pages for a document PDFium paginates as
# 2, which is why every one of these readers takes the password. password-probe
# asserts it is 0 once the key arrives and its mutation drives it to 2. To re-run
# the sweep:
#   for f in testdata/*.pdf; do
#     printf '%s: ' "$(basename "$f")"
#     cargo run -q --manifest-path src-tauri/Cargo.toml --example links-probe -- \
#       "$f" --mode read 2>&1 | tail -1
#   done

# The worker boundary is still transparent: the two backends must agree byte for
# byte on tiles, geometry, text, search, outlines and comments, and a worker killed out of
# the OS process table must be replaced by one serving the same document. Run it
# on vector-heavy as well as a text fixture -- it is the only corpus whose render
# is slow enough for the withdrawal and drain checks to apply, and on every other
# one they report [SKIP] with the reason. vector-heavy is the run to read: 44
# check names, 2 skipped on macOS (43 until 26.9.2, when the retirement phase
# gained `growing the pool adopts the warmed spare and re-arms the slot`; 42
# until 2026-08-16, when comments were added
# to the comparison; measured 2026-07-31 at 42, and this said 41/1 then and
# contradicted the "all 42 names" sentence below it -- the prose was right).
cargo run --release --manifest-path src-tauri/Cargo.toml --example backend-probe -- \
    testdata/vector-heavy.pdf
```

The count that matters there is the count of **names**, not the split between passed and
skipped: the split moves with the corpus and with a thumbnail's timing, and chasing a
documented split back to its value is how a condition that keeps a check honest gets
deleted. What holds on every corpus is that all **44** names appear (43 until 26.9.2, when the
retirement phase gained `growing the pool adopts the warmed spare and re-arms the slot`,
which skips with a stated reason on a platform that pre-spawns nothing; 42 until 2026-08-16,
when the comment comparison landed) — diff the name sets
across two fixtures rather than comparing their totals, which is what caught a check that
had stopped existing on one-page documents.

One of the checks **skips itself** rather than passing, and it is the pattern to copy. *"A
search option crosses the worker boundary"* compares a whole-word search on both backends
against an unrestricted one; where the option changes nothing — a page with no extractable
text — it says so and skips, because two backends agreeing on the same result is exactly
what a worker that *dropped* the option would also produce.

**Do not run it under `caffeinate`.** `caffeinate -d -u <utility>` `exec`s the utility in
its own process and leaves a helper behind as that process's *child*, and every observation
of a worker here comes from the process table. The probe filters on the worker's argv for
exactly this reason, so it is now correct either way — but the same trap is waiting for any
new check that counts children, and it presents as a stable, reproducible failure that reads
like a real defect. `AGENTS.md` has the incident.

The worker pool has its own measurement rather than a check, because what it is for is a
number. It is not part of the bump checklist above — run it when the pool, the thread
count, or the tile path changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example pool-bench -- \
    testdata/vector-heavy.pdf --rounds 4 --sizes 1,2,4,6,8
```

It interleaves the sizes across rounds and compares pairwise within a round, discards round
0, and reports the cold regime (the pool growing) separately from the warm one. Quote two
runs, not one: the four-worker figure moves several percent between runs while six barely
moves, and one run would present that as a measurement.

The other half of the same subject — what a grown pool costs to hold and what retiring it
gives back — is a second mode. Run it when the idle timeout, the reaper, or the number of
workers kept changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example pool-bench -- \
    testdata/vector-heavy.pdf --mode retire --rounds 4
```

It reports the pool's footprint at three points and, per round, a warm screenful against
the first one after a retirement. `--idle-ms` sets the timeout it runs at (4 s by default,
so a round does not take half a minute); the app's own default is 30 s. The wait for a
retirement is **bounded and fails the run** if it does not happen — without that, the
second column would quietly be a warm screenful wearing a cold label, which is a number
that looks entirely reasonable.

Three notes on why these are written out in full. The target names are **hyphenated**, and
`--example remove_probe` fails as "no such target", which reads like a missing harness
rather than a wrong name. They are `--example`, not `--bin`, since 2026-07-31 — as
`[[bin]]` they were shipped inside the installer, all seventeen of them — so an older
command fails the same way, and the built artifacts moved from `target/release/` down into
`target/release/examples/`. **Delete any probe executables still sitting in
`target/release/`**: they are left over from before the split, nothing rebuilds them, and a
path copied out of an older document silently runs a frozen binary. And `remove-probe` with
no case argument defaults to case `a`, whose whole purpose is to segfault — so the obvious
invocation of the regression check crashes by design and looks like the bump broke
something.

The progressive checks are why the raw path restates `FPDF_ANNOT`,
`FPDF_REVERSE_BYTE_ORDER` and `FPDFBitmap_BGRA` by value: `pdfium-render` does not
re-export them, and a bump that changed any of them would silently alter every tile. The
run compares progressive output byte-for-byte against the safe path, so it fails if one
does.
