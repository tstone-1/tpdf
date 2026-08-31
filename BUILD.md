# BUILD.md --- tpdf

How to get a clean clone building, what the quality gates are, and how a release is cut.

Durable project knowledge lives in [`AGENTS.md`](AGENTS.md); the architecture and roadmap
are in [`docs/PLAN.md`](docs/PLAN.md). This file is only the mechanics.

---

## Prerequisites

| Tool | Notes |
|------|-------|
| Rust (stable, via rustup) | `rustup update`. Do not install a second toolchain through Homebrew. |
| Node 20+ and npm | |
| Python 3.9+ | Only for `scripts/`; not a runtime dependency of tpdf. |
| `uv` | Only for the test fixtures that need `fontTools` or `pyhanko`. |
| `qpdf` | Not needed to build or run tpdf, and **required** for the hostile corpus --- `testdata/make_hostile_pdf.py` shells out to it, so without it there is no `hostile-manifest.json` and `sanitize-rewrite` cannot start. Also the structural oracle for spike 0.4. On Windows the winget package needs elevation; the release's `msvc64.zip` unpacks anywhere and needs none. |

---

## Clean clone

```
npm install
scripts/fetch_pdfium.py
```

`vendor/pdfium/` is gitignored --- a 7.7 MB binary does not belong in the object store --- so
**a fresh clone has no PDFium and every binary fails to bind at runtime until the fetch
script has run.** The script downloads the pinned upstream build, verifies its SHA256
before extracting anything, and refuses a V8 asset.

Verify an existing install without touching the network:

```
scripts/fetch_pdfium.py --check
```

The pin is `chromium/7881`, which is the build every Phase 0 measurement in `AGENTS.md`
and `docs/PLAN.md` was taken against. Bumping it means editing `TAG` and the whole `PINS`
table in `scripts/fetch_pdfium.py` together, then re-running the two checks that a digest
cannot stand in for:

Run these from the repository root, after generating the fixtures below. Each exits
non-zero on failure.

```
# The FPDFPageObj_Destroy ownership segfault. Case `c` (leak) must pass; if case
# `a` (destroy) ever stops crashing, the upstream bug is fixed.
cargo run --release --manifest-path src-tauri/Cargo.toml --example remove-probe -- \
    testdata/text-truetype.pdf c

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
# one they report [SKIP] with the reason. vector-heavy is the run to read: 43
# check names, 2 skipped on macOS (42 until 2026-08-16, when comments were added
# to the comparison; measured 2026-07-31 at 42, and this said 41/1 then and
# contradicted the "all 42 names" sentence below it -- the prose was right).
cargo run --release --manifest-path src-tauri/Cargo.toml --example backend-probe -- \
    testdata/vector-heavy.pdf
```

The count that matters there is the count of **names**, not the split between passed and
skipped: the split moves with the corpus and with a thumbnail's timing, and chasing a
documented split back to its value is how a condition that keeps a check honest gets
deleted. What holds on every corpus is that all **43** names appear (42 until 2026-08-16,
when the comment comparison landed) --- diff the name sets
across two fixtures rather than comparing their totals, which is what caught a check that
had stopped existing on one-page documents.

One of the 42 **skips itself** rather than passing, and it is the pattern to copy. *"A
search option crosses the worker boundary"* compares a whole-word search on both backends
against an unrestricted one; where the option changes nothing --- a page with no extractable
text --- it says so and skips, because two backends agreeing on the same result is exactly
what a worker that *dropped* the option would also produce.

**Do not run it under `caffeinate`.** `caffeinate -d -u <utility>` `exec`s the utility in
its own process and leaves a helper behind as that process's *child*, and every observation
of a worker here comes from the process table. The probe filters on the worker's argv for
exactly this reason, so it is now correct either way --- but the same trap is waiting for any
new check that counts children, and it presents as a stable, reproducible failure that reads
like a real defect. `AGENTS.md` has the incident.

The worker pool has its own measurement rather than a check, because what it is for is a
number. It is not part of the bump checklist above --- run it when the pool, the thread
count, or the tile path changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example pool-bench -- \
    testdata/vector-heavy.pdf --rounds 4 --sizes 1,2,4,6,8
```

It interleaves the sizes across rounds and compares pairwise within a round, discards round
0, and reports the cold regime (the pool growing) separately from the warm one. Quote two
runs, not one: the four-worker figure moves several percent between runs while six barely
moves, and one run would present that as a measurement.

The other half of the same subject --- what a grown pool costs to hold and what retiring it
gives back --- is a second mode. Run it when the idle timeout, the reaper, or the number of
workers kept changes:

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example pool-bench -- \
    testdata/vector-heavy.pdf --mode retire --rounds 4
```

It reports the pool's footprint at three points and, per round, a warm screenful against
the first one after a retirement. `--idle-ms` sets the timeout it runs at (4 s by default,
so a round does not take half a minute); the app's own default is 30 s. The wait for a
retirement is **bounded and fails the run** if it does not happen --- without that, the
second column would quietly be a warm screenful wearing a cold label, which is a number
that looks entirely reasonable.

Three notes on why these are written out in full. The target names are **hyphenated**, and
`--example remove_probe` fails as "no such target", which reads like a missing harness
rather than a wrong name. They are `--example`, not `--bin`, since 2026-07-31 --- as
`[[bin]]` they were shipped inside the installer, all seventeen of them --- so an older
command fails the same way, and the built artifacts moved from `target/release/` down into
`target/release/examples/`. **Delete any probe executables still sitting in
`target/release/`**: they are left over from before the split, nothing rebuilds them, and a
path copied out of an older document silently runs a frozen binary. And `remove-probe` with
no case argument defaults to case `a`, whose whole purpose is to segfault --- so the obvious
invocation of the regression check crashes by design and looks like the bump broke
something.

The progressive checks are why the raw path restates `FPDF_ANNOT`,
`FPDF_REVERSE_BYTE_ORDER` and `FPDFBitmap_BGRA` by value: `pdfium-render` does not
re-export them, and a bump that changed any of them would silently alter every tile. The
run compares progressive output byte-for-byte against the safe path, so it fails if one
does.

### Test fixtures

`testdata/*.pdf` is gitignored and generated. Nothing it produces may be committed or
redistributed --- `make_text_pdf.py` embeds a system font.

```
uv run --with fonttools testdata/make_text_pdf.py testdata
python3 testdata/make_hostile_pdf.py testdata
python3 testdata/make_vector_pdf.py testdata/vector-heavy.pdf
python3 testdata/make_vector_pdf.py testdata/vector-multi.pdf 200000 12
uv run --with pyhanko --with pyhanko-certvalidator --with cryptography \
  testdata/make_incremental_pdf.py testdata
python3 testdata/make_outline_pdf.py testdata
python3 testdata/make_rotated_pdf.py testdata
python3 testdata/make_columns_pdf.py testdata
python3 testdata/make_mixed_pdf.py testdata
python3 testdata/make_tagged_pdf.py testdata
uv run --with fonttools testdata/make_multilingual_pdf.py testdata
uv run --with fonttools testdata/make_encodings_pdf.py testdata
python3 testdata/make_comments_pdf.py testdata
python3 testdata/make_links_pdf.py testdata
python3 testdata/make_form_pdf.py
```

The last two were missing from this list until 2026-08-02, while the corpus table
below told a reader to run the viewer check against both --- so the instruction that
produces a fixture and the instruction that consumes it disagreed, and the failure is
an absent file reported as a broken bundle. `text-heavy.pdf` is deliberately not here:
it is a real document rather than a generated one, and a machine that does not have it
cannot make it.

**What that was quietly costing, found 2026-08-22.** This limitation had been written
down three times and every one of them discusses *corpora* --- a viewer sweep that cannot
run all fourteen, a `prespawn-bench` check that skips, a 109-name re-run taken on six.
All true, and none of them is where it hurt. **Ten `cargo test` tests over the save
path's guards also asked for `text-heavy.pdf`**, and returned at their first line without
it: here, and on both runners, which cannot have it either. A test that returns early is
counted among the `753 passed`, and its `[SKIP]` goes to a stdout libtest discards for a
passing test, so nothing in a green run said so. Every mutation aimed at those guards
SURVIVED, which is the only instrument that could tell.

They use `comments.pdf` now --- generated, appendable, and carrying `/Annots` of its own
so the array-bearing branch is exercised too --- except the one whose control needs a page
listing *nothing*, which takes `rotated.pdf`. All twelve `append` mutations and all 62
`save` ones are now caught by the test named for them. The guards were correct throughout.

The general form is worth carrying to the next "this machine cannot have X" note: the
question is not whether it is true, it is **what else consumes X**. `docs/TRAPS.md` has
the entry.

`make_incremental_pdf.py` writes about **550 MB** on purpose, so that "appending to a
300 MB file is near-instant" can be tested at 300 MB.

**Its signed fixtures did not exist on a hosted runner until 2026-08-21**, so CI tested none
of the signature reader. Both workflows now install pyhanko and call
`scripts/ci_fixtures.py --signed`, which builds the nine of them --- eleven since the two
encrypted fixtures joined the group on 2026-08-23. Two things had to change for
that to be possible, and the first is why it had never worked: `make_incremental_pdf.py` called
**qpdf** with `check=True` and nothing else, so a machine without qpdf died there with a
`FileNotFoundError` naming a program rather than a fixture --- and died *before* every signed
fixture, none of which needs qpdf at all. It skipped that one fixture from then on. The
second is `--scan-pages` with no values, which is the existing switch for not writing 550 MB.

**It calls qpdf for nothing at all since 2026-08-23**, and the skipped fixture is a runner
fixture now. `encrypt_with` writes both encrypted documents with pyhanko --- one behind
`swordfish`, one on an empty user password --- so `incr-encrypted-open.pdf` and
`incr-encrypted-pw.pdf` are in the `--signed` group and CI builds them. That is what makes
`password-probe` run on a runner instead of printing twelve `[SKIP]`s, and it is not a
tidiness fix: the save path's encryption guard had been wrong for four weeks with every gate
green, and the fixture that catches it was the one no runner could build.

Proved both ways before the step was written: with the fixtures moved aside and pyhanko absent,
`ci_fixtures.py --signed` exits **1** with `exited 0 but testdata/incr-signed.pdf does not
exist`; with pyhanko present it exits 0 and writes all nine. **Green on both runners since
2026-08-21**, after three pushes --- the first two failed on assertions that had pinned a value
out of a locally generated fixture, which is one trap and worth reading before adding a test
that reads one. That hard failure is what makes the
tests' own `[SKIP]`-when-absent safe --- a runner that failed to build them goes red at the step
that built them, not green through a suite that skipped.

**The fixtures are not reproducible, and CI generates them fresh every run.** Two consecutive
runs *on one machine* produce nine files of identical size and differing bytes, because pyhanko
mints a new key pair and serial each time. **Across machines the size moves too**: both CI
runners build an `incr-signed.pdf` of **8,097** bytes where this laptop builds **8,128**, on the
same commit. So nothing absolute may be pinned out of one --- not a digest, not a serial, not a
date, and **not a size**, which is the one that looked safe after the local pair agreed and went
red on both runners at the first push. What replaced the pinned numbers is a quantity derived
from the file at test time, by a route `docinfo` does not take.

Three tests reading them ended with `assert!(examined > 0)` until the same day, which is red on
exactly the machines that cannot have the files --- measured by hiding `testdata/incr-*.pdf`:
three failures, each telling a runner to generate what the repository had written down as
deliberately absent. They now assert that **every** named fixture was examined, behind an early
return when none of them exists. Both directions proved: no signed fixture gives 702 passed, 0
failed; hiding exactly one gives two red.

`make_comments_pdf.py` is the only fixture carrying annotations, and it is also one of the
three `scripts/ci_fixtures.py` builds on a hosted runner --- it needs nothing but the standard
library, since the PDF writer it borrows from `make_text_pdf.py` reaches for fonttools only
inside the function that embeds a font. Two things about it are deliberate and easy to undo by
accident: its rotated page's rectangle is **not square**, because a square one maps to itself
under a quarter turn and cannot tell a rotation from an identity; and its three malformed
`/Annots` entries are written **before** the 1,200 notes, because the per-page bound stops the
scan at 1,000 and anything after that is never read. Both are in `docs/TRAPS.md`, both were
found by the fixture failing to discriminate rather than by review.

`make_links_pdf.py` is the fourth `scripts/ci_fixtures.py` builds on a runner, and
dependency-free for the same reason. It writes **three** files, and the third is worth knowing
about before reading a green `text-probe`: `links-cropped.pdf` has a `/CropBox` inset 50 points
from its `/MediaBox`, which is the case PDFium lays out differently from the sheet and which the
scans got wrong until 2026-08-16. Run `text-probe` against it as well as `links-probe` --- the
text half of that fix is covered by the probe rather than by `cargo test`, because it needs a
live PDFium page:

```sh
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-probe -- \
    testdata/links-cropped.pdf     # character boxes land on ink: 96.4%
cargo run --release --manifest-path src-tauri/Cargo.toml --example links-probe -- \
    testdata/links-cropped.pdf --mode check    # 6/6, 2 skipped
```

**That `text-probe` run reports two of its four controls as `[SKIP]`, and it should.** Both
link fixtures are 36 rows of even text, and a dense page of uniform lines cannot detect a
y-flip --- the un-flipped convention reaches 87% and the `/Rotate 180` control 77%, against
0--5% on the fixtures written for this probe. So what a green run proves on these two is
**placement, not orientation**, and the probe now says exactly that in a `[NOTE]` line rather
than reporting an undiscriminating control as a failure. Until 2026-08-16 it failed them and
exited 1, so this documented command was red and this page quoted only its passing line.

**The 96.4% is worth reading against its own control rather than against 100.** Removing the
origin shift in `text.rs` takes it to **74.8%** --- a `[FAIL]`, since the threshold is 95% ---
while `text-base14.pdf`, which has no crop box, stays at 100%. That is the measurement proving
this probe covers the text half of the crop-box fix at all, and it is closer to the threshold
than it looks: a 50 pt inset moves each box by less than a line's height, so on dense text most
still overlap some ink. The `0% before the fix` figure this page used to carry came from a
different and larger error --- the scan then mixed PDFium's *cropped* size with page-space
boxes. A fixture with a bigger inset would give the probe more margin.

96.4% rather than 100% is correct and not a near-miss: the fixture's text runs past the crop
box's bottom edge, so the characters the crop hides have boxes outside the rendered page. A
fixture whose every glyph sat inside the crop would not exercise that. Two things about it carry the same kind of intent as
the comment fixture's. Its **outline points at the same destinations its links do**, which is
what makes `links-probe --mode agree` able to compare tpdf's two destination resolvers at all;
delete the outline and that mode still runs, still prints a count and can no longer fail.
And the rotated page is a **separate file** --- `links-rotated.pdf` --- because a document
that mixes page sizes reddens two of `viewer_check.py`'s rotation checks, which derive what
they expect from page 1's aspect ratio. That is the same split, for the same reason, that
`comments-rotated.pdf` exists.

`make_tagged_pdf.py` is the other side of that coin: the only fixture that carries a
`/StructTreeRoot`, so it says what its own reading order is. Page 1 puts a margin note beside
the first paragraph --- geometry reads it third, the tags read it last --- and page 2 is the
control, tagged in the order geometry would have inferred anyway. A tagged fixture whose tag
order matches geometry tests nothing, so the generator **asserts the discrimination itself**
and refuses to write a fixture that has lost it. Its manifest states both orders, which is what
lets a check say "the tagged answer was used" rather than "an answer was produced".

Its manifest also carries the three fields `viewer_check.py`'s reading-order check reads
(`page`, `name`, `lines`), so this is an ordinary corpus for that harness rather than one it has
to know about --- and what that check then asserts is the **lines**, in tagged order, against a
file a different program wrote.

Worth knowing as external evidence that the fixture is not merely self-consistent: poppler's
`pdftotext` reads page 1 in **geometric** order --- heading, margin note, body --- which is the
wrong answer the tags exist to correct.

**It carries two heading levels, and that is not decoration.** A page with one heading cannot
tell a consumer that uses the document's level from one that announces every heading as `h1`:
the mutation doing exactly that survived against the first version, and the check passed. A
property with one value present is the same as none --- see the trap, whose list of the usual
suspects (one page, one rotation, one font, one column) is worth reading before building any
fixture.

`make_columns_pdf.py` is the only fixture whose *content-stream order* is the point. Its
three pages are two columns emitted column by column, the same two columns emitted line by
line across the gutter, and a heading spanning both over the second of those. The first two
look identical and must read identically, which is an assertion neither page can satisfy by
agreeing with itself. It writes `columns-manifest.json` beside the PDF, and
`viewer_check.py` passes any `<stem>-manifest.json` it finds through to the check --- so
what reading order is compared against is a file a different program wrote.

`make_mixed_pdf.py` is the only fixture whose pages are not all the same size. Every other
document in the corpus is uniform --- `make_rotated_pdf.py` builds a second, uniform file for
exactly that reason --- so until this existed no check could fail on the frontend's largest
layout assumption. It is A4 with an A3-landscape insert (wider, same height, so a failure is
the crop and not the offset), an A5 page (shorter, so a failure is the offset), and an A4
control before and after both. Each page carries a marker at every one of its own edges, and
page 3 carries one just past A4's width, so a cropped render loses a named string rather than
losing something unnamed.

It writes `mixed-geometry.json` rather than `mixed-manifest.json`, because the
`-manifest.json` suffix enrols a fixture in the reading-order check and this one makes no
claim about reading order. `viewer_check.py` binds that sidecar to `TPDF_GEOMETRY_MANIFEST`,
the `geometry_manifest` command hands its contents to the webview, and the three layout checks
assert against it --- against a file a different program wrote, rather than against the backend
the viewer renders through. On every other fixture those three say `[SKIP] no geometry sidecar
for this fixture`.

---

## Quality gates

```
scripts/gates.py
```

That is the whole checklist. **`scripts/gates.py` is the definition of the gates, not a
description of them** --- it holds the commands with their flags, and this file deliberately
does not repeat them. `AGENTS.md` records why: a checklist weaker than the gate it exists
to satisfy is worse than no checklist, and the usual failure is a hand-copied command that
quietly loses a flag. Removing the copy removes the drift.

To see what will run, ask the script rather than this document:

```
scripts/gates.py --list
scripts/gates.py --gate clippy      # run one, repeatable
```

Every gate runs even after an earlier one fails, so one pass reports everything that is
wrong. The exit code is non-zero if any failed.

Two of them are worth understanding rather than just running:

- **`cargo test --locked` is two gates in one.** Besides the unit tests it fails on a
  `Cargo.lock` that was not committed after a `cargo update`, and it compiles the test
  targets, which is where `--all-targets` clippy findings surface. Coverage now reaches
  most of the backend --- the request queue and the `tile://` parser, the worker protocol
  and the pool, rendering, text, search, outlines, printing, session and sweep --- with
  `npm run test` doing the same for the front-end logic beside it. What it deliberately
  leaves to the harnesses under `scripts/` is everything that needs a live webview --- and a
  Windows run is now one of those, `viewer_check.py` having passed there on 2026-07-29,
  rather than something nothing covers at all. What no gate covers is paper: a print job is
  checked by reading its bytes back with PDFKit, a parser independent of the writer but still
  not a printer.
- **`cargo build --locked --bins` is the only gate that links anything.** clippy stops at
  metadata, and `cargo test` links each `[[bin]]` with its `main` replaced by the test
  harness's, so a symbol reachable only from `main` is dropped as dead code rather than
  reported as missing. Without this gate a 7/7 sweep sat beside a failing
  `npm run tauri build`; see the trap.
- **Wrap a batch of benchmark runs in `caffeinate -du`.** `scroll_bench.py` holds one for
  its own lifetime, but the gaps between runs --- and any headless bench running alongside
  it --- are unprotected, and a session that locks mid-batch fails the next frame-rate run
  outright. A locked macOS session cannot be unlocked from a script by design, so this is
  preventable and not recoverable.

- **The `toolchain` gate runs first, and it is what makes the Rust pin real.**
  `rust-toolchain.toml` names the compiler; `RUSTUP_TOOLCHAIN` in the environment overrides
  that file completely and silently, so a pin with nothing asserting it is indistinguishable
  from no pin. The gate compares the running rustc against the file, checks clippy and
  rustfmt came from the same toolchain commit, and prints `RUSTUP_TOOLCHAIN` whether or not
  it is set. Neither workflow uses a toolchain-installing action any more --- both run
  `rustup show`, which installs exactly what the file names.

  To move to a newer Rust: edit `rust-toolchain.toml`, run `scripts/gates.py`, and commit it
  on its own. Expect `-D warnings` to surface new lints; that is the pin working, and dealing
  with them in a dedicated commit is the whole reason it exists.

- **The `workflows` gate compares `ci.yml` and `release.yml`'s `gates` jobs, and only those.**
  They must be the same job: one says a commit is good, the other stops a tag on a broken
  commit producing artifacts, and if the release copy is weaker then every ordinary push is
  checked harder than the thing that actually ships. They had drifted exactly that way ---
  see step 10 of the release checklist for what it cost. The gate compares every `uses:` with
  its pinned SHA and every `run:` body, in order; step *names* are not compared, since two
  identical commands under different labels are still the same job. It deliberately says
  nothing about the `release` job, the triggers or the permissions: those differ on purpose,
  and that difference is the fork threat model rather than drift.

- **The `pdfium` gate is a pin check, not a build step.** It fails if `vendor/pdfium` is
  missing or is not the pinned build --- which is the difference between a benchmark that
  means something and one that does not.

- **The `notices` gate runs last because it reads the build's output.** It derives which
  npm packages ship from `dist/assets/*.js.map` --- the bundler's own account of what it
  emitted --- so it needs the `build` gate above it to have run. Two checks in one command:
  that `THIRD-PARTY-NOTICES.md` still matches the dependency tree, which is the
  binary-distribution obligation; and that no GPL, LGPL or AGPL licence has appeared. Its
  third population is the one nothing else can see --- the C++ libraries inside
  libpdfium, read from `vendor/pdfium/licenses/`, which `cargo metadata` is structurally
  blind to. Regenerate with `scripts/third_party_notices.py` and commit the result; never
  hand-edit the file. On a mismatch it prints the **diff**, not the word "stale" --- a gate
  that fails on a machine you are not sitting at is only actionable if its message carries
  the evidence.

  **After any PDFium pin bump, cross-check the two archives.** They ship the same fifteen
  licence files and nine of them differ --- eight by line endings, and `pdfium.txt` by
  carrying a `//` comment prefix on macOS and none on Windows. A document generated from
  whichever archive is installed is then a function of the platform, which is how this gate
  came to be green on macOS and red on Windows with nothing wrong:

  ```
  scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
  scripts/third_party_notices.py --cross-check /tmp/pdfium-win
  ```

  Note this is doable **from either machine** --- the other platform's archive is a download,
  not a machine you have to be sitting at. Worth reaching for before waiting on a CI round
  trip to diagnose a platform difference.

### CI runs per push and per pull request, and again on a tag

`.github/workflows/ci.yml` runs the gates on `macos-latest` and `windows-2025` for every
push to `main` and every pull request, since 2026-08-02.

This section said "CI runs on a tag, and on nothing else" until then, and the reason it
gave was half wrong in a way worth keeping. The objection was never runner minutes --- it was
that a workflow would be **a second place for the gate list to live**. `ci.yml` does not
restate the commands, it invokes `scripts/gates.py`, so that objection never applied to the
workflow that was eventually written. What changed materially is that the repository went
public and macOS minutes stopped costing 10x against a private allowance. "One machine" was
a description of the circumstances, not an argument.

**What CI cannot cover, and why the harnesses below stay manual.** `viewer_check.py` and
`mutate_viewer.py` drive a real window and need an unlocked, unoccluded screen. On a
headless runner they do not fail, **they hang** --- which is the failure shape this project
reads worst, since a hang and a pass both produce no red. Do not add them to a workflow.

`.github/workflows/release.yml` fires on a CalVer tag. It **invokes `scripts/gates.py`** on
both platforms rather than re-listing commands in YAML, so the checklist and the gate stay
one object instead of two that happen to agree today. Then it builds, signs, notarizes and
publishes a draft release.

It arrived for a reason this document did not predict --- it expected the trigger to be the
repo going public or a second contributor. What actually forced it is notarization: it needs
a Mac, a Developer ID and Apple API credentials, and a signed macOS release should not
depend on which machine is free.

**Its macOS half ran green on 2026-08-03**, and this paragraph said it had never run until
that day. Ported from `screenpick`, whose version is proven; the part with no precedent is
signing the bundled `libpdfium.dylib`, since neither sibling ships a native library.
Notarization requires every Mach-O in the bundle to be Developer ID signed with the hardened
runtime, so the dylib is signed in `vendor/` before the bundler copies it --- correct whether
or not Tauri re-signs nested resources. The verification step fails rather than warns,
because a skipped notarization exits 0.

It took four rehearsal tags and each failed one step later than the last; step 10 of the
release checklist has the sequence, and all three defects are in `docs/TRAPS.md`. What the
green run establishes, beyond that the path works:

- **Signing a bundled native library for notarization works.** The `.app` is `Accepted`, the
  DMG is notarized and stapled, and both the app and the dylib chain **Developer ID
  Application -> Developer ID Certification Authority -> Apple Root CA** with
  `flags=0x10000(runtime)`.
- **Verified from outside the workflow, not only by it.** rc3's DMG was downloaded from the
  draft and checked on a machine that had not built it: `spctl -a -t open` reports
  `source=Notarized Developer ID`, `stapler validate` passes on the DMG and on the `.app`,
  the payload holds exactly one `libpdfium.dylib` and `THIRD-PARTY-NOTICES.md`. Worth doing
  again on any release whose verification step has been touched --- rc3 is the case where
  the artifact was perfect and the checker was broken.
- **The macOS bundle layout for a resource map is settled**, which `pdfium_library_dir`
  records as unverified and the code cannot answer: the engine lands at
  `Contents/Resources/pdfium/libpdfium.dylib`, and the verification step prints the path it
  found.

### Windows runs the viewer, and how it came to be contained

**Read this section as a timeline, not as a status.** It opens with the state before
2026-07-29 --- uncontained, failing open --- because the controls taken then are what make the
later evidence mean anything. The present state is at *Windows no longer fails open* below:
workers are selected there, proved from outside the process. `AGENTS.md` carried the
pre-flip wording in its own gates section for a day after the flip, in flat contradiction of
its own constraints section, which is the hazard this note exists to prevent here.

`scripts/gates.py` reported **8/8 on `x86_64-pc-windows-msvc`** on 2026-07-29 --- a dated
count, and there are twelve gates now, so ask `--list` rather than this line --- and the same
day a Windows build **opened documents and passed the full functional check**. A clean clone bootstraps
with no changes --- `npm install` and `scripts/fetch_pdfium.py` both do the right thing, the
fetch script selects the `win-x64` asset and verifies its digest.

`viewer_check.py` runs unmodified: `webview_guard` already returns early off darwin, and
WebView2 needs no bundle identity, so a plain `target/release/tpdf.exe` is enough where macOS
needs an `.app`. Two things about the invocation, both of which present as something other
than what they are. The binary must come from `cargo build --release --features
tauri/custom-protocol` or the window shows *"localhost refused to connect"* (see the trap ---
the profile is not what embeds the frontend). And **pass it as a backslash path**:
`CreateProcess` does not accept a relative forward-slash path, so
`src-tauri/target/release/tpdf.exe` raises `FileNotFoundError: [WinError 2] The system cannot
find the file specified` for a file that is plainly there, from inside Python's `subprocess`
rather than from anything in this repository.

Four corpora, every one reporting the **86 check names** that were the invariant then, with
splits inside the ranges the table above records. Word and line selection took that to **89**
on 2026-07-30, after this run; the splits below are left as measured rather than adjusted by
arithmetic. A Windows re-run should expect **109** names --- 23 added since, and the macOS
table further down says which of them skip on which document:

| fixture | ran | skipped | failed |
|---|---|---|---|
| `outline-simple.pdf` | 81--82 | 4--5 | 0 |
| `outline-hostile.pdf` | 197 | 37 | 0 |
| `rotated-90.pdf` | 184 | 50 | 0 |
| `vector-heavy.pdf` | 104 | 130 | 0 |

Re-run 2026-07-30 with pre-spawning live, since that changes the app's own behaviour --- every
open now consumes a warmed process and starts another. All four green, no `[WARN]`, 44 modules
at peak with no `pdfium` among them over 27--978 samples. `outline-simple` reported 82/4 that
time against 81/5 before: the **name set** is what is invariant, not the split, and one of
them stopped skipping. A split that moves is information; a name that disappears would not be.

#### The 109-name re-run, measured

Done 2026-07-30 after the reading-order work landed, on the **six** corpora this machine can
generate --- `text-heavy.pdf` is a real document rather than a generated fixture and has never
been on this box, which is the same reason `prespawn-bench` skips one of its checks here.

Every corpus reports the same **109** names and, more usefully than the count, **the same
split as macOS on every single one**:

| fixture | Windows ran / skipped | macOS table | failed |
|---|---|---|---|
| `outline-simple.pdf` | 102 / 7 | 102 / 7 | 0 |
| `outline-hostile.pdf` | 102 / 7 | 102 / 7 | 0 |
| `rotated-90.pdf` | 95 / 14 | 95 / 14 | 0 |
| `vector-heavy.pdf` | 62 / 47 | 62 / 47 | 0 |
| `vector-multi.pdf` | 70 / 39 | 70 / 39 | 0 |
| `columns.pdf` | 93 / 16 | 93 / 16 | 0 |

The name sets were diffed pairwise with the `cut -c8-47` recipe above rather than compared by
count, and all six are byte-identical to each other. Each extracts **110** lines, not 109: the
`the app process never mapped the PDF parser` line is a Windows-only observation printed
outside the check set, exactly as intended, and it is the only difference. 43--45 modules at
peak, no `pdfium` among them, over 32--1324 samples.

**One run of `vector-multi` failed before this and is worth reading rather than discarding.**
`activating a thumbnail goes to its page` reported `from page 1 to 1, wanted 7`, and the three
withdrawal checks beside it skipped --- which looks like two findings and is one, since nothing
navigated, so no new thumbnail was ever requested to be in flight.

It led to a real defect in two classes. The page strip and the outline tree both activated
`focused` --- a **mirror** of the DOM's focus kept by a `focusin` listener --- rather than the
row the key event reached, and a mirror that misses an update sends the reader to whatever it
still names, which is page 1 because it starts at 0. `focusin` is not guaranteed: a document
without system focus moves `activeElement` without delivering focus events. Both now take the
row from `event.target` and keep the mirror only as the fallback for a key that arrived on the
container. Each has a unit test that was shown to go red first, plus a control on the
fallback; `sidebar.ts` had no unit tests before this.

**The intermittent itself was never caught a second time** --- five further corpus runs,
including a replay of the back-to-back loop it came from and one under deliberate concurrent
CPU load, are all green --- so this is an identification by mechanism and symptom, not by
re-observation. Contention was the first guess and was wrong when tested. The check now prints
`activeElement`, whether the strip followed, and `document.hasFocus()`, so a recurrence
settles it. See the trap *A mirror of the DOM's focus goes stale*.

Rendering, scrolling, zoom, pinch, view rotation, text selection, search, the palette, the
accessibility tree, the outline sidebar, thumbnails, inversion and the print command's
refusals all behave as they do on macOS.

**What was missing then was containment, not function** (superseded 2026-07-29 --- see below).
`sandbox_init` is SBPL and macOS-only, so `Worker::spawn` refused off macOS and
`Backend::default_here()` fell back to `Backend::InProcess`. A Windows build parsed
attacker-controlled PDF **in the app process**, which is exactly what `AGENTS.md` and
`docs/THREAT-MODEL.md` forbid. **It failed open**:
`Worker::spawn`'s refusal is asserted by tests, but only a caller that asks for
`TPDF_BACKEND=worker` ever reaches it --- the default selects in-process and renders perfectly
happily, so nothing refuses. A port owes a real containment answer (job objects, a restricted
token, a separate desktop) before Windows can ship. That, and not the viewer, is now the whole
gap.

It is at least **visible**: the uncontained default records `render::UNSANDBOXED_MARK` on the
startup timeline and prints `[WARN] no sandbox on this platform ...` on stderr, and
`viewer_check.py` echoes `[WARN]` lines even on a passing run --- it previously showed stderr
only on failure, which hid the warning from exactly the runs that succeed. Visibility is not
containment, and a mark is deliberately not a refusal: refusing would make Windows useless
rather than uncontained, which is a decision rather than a defect.

**And the fix is now measured rather than guessed.** `cargo run --release --bin
win-sandbox-probe` runs six containment rungs, each rendering the same tile in a re-exec'd
child and compared pixel for pixel against an in-process render, with an uncontained child as
the control over the harness itself:

```
bare        yes   yes   0                       control: what Windows does today
job         yes   yes   0                       memory cap, one process, kill-on-close
lowil       yes   yes   0                       job + low integrity level
noprivs     yes   yes   0                       diagnostic: privileges dropped only
sidonly     no    -     STATUS_DLL_NOT_FOUND    diagnostic: restricting SID only
restricted  no    -     STATUS_DLL_NOT_FOUND    job + restricted token
```

A **job object plus low integrity** renders byte-identically while denying writes to the user
profile and `OpenProcess` on the parent. It does not deny *reads* --- an integrity level
governs writes --- so the child is handed its document and its output as inherited handles
rather than paths. A restricting SID is stronger and unreachable directly: the loader's own
reads are denied and the child dies before `main`, which needs Chromium's initial-token /
lockdown-token handover to get past.

**A worker uses it now** (2026-07-29). `Worker::spawn` builds a contained child on Windows, and
`worker-probe` is the standing proof:

```
cargo build --release --example worker-probe
./src-tauri/target/release/examples/worker-probe.exe testdata/text-base14.pdf
```

**Run it against `incr-scan-40p.pdf` too.** It reports what a save's preparation costs the
worker --- on macOS, 362.7 MB before the request and 1029.8 MB after, so the append itself adds
667 MB on that document.

**Measured on Windows 2026-08-22, and the margin is 4.3% rather than the 35% that was
reasoned.** The first measurement was taken from outside the process, because `[INFO]` could
not print here at all: it was guarded on `Worker::footprint`, which is `phys_footprint`, which
is `None` off macOS --- so a Windows run was told to read a line the build could not emit. That
is fixed, and the fix is the point rather than the convenience. The quantity a job object caps
is **commit**, and `Contained::peak_commit` reads it through the handle the parent already
holds, so `Worker::peak_commit` is to Windows what `footprint` is to macOS and the probe prints
whichever the platform has, named. The footprint check is no longer a `[SKIP]` here: it reads
*"the parent can read what bounds the worker's memory"* and passes on both, so the probe now
reports **17/17 with none not applicable** on either platform.

**Two checks were added on 2026-08-24, so the number is now 19** --- measured **19/19 with none
not applicable on Windows**; the macOS figure was 17/17 before they existed and has not been
re-run. They cover a worker that cannot load PDFium at all: it must **answer** the request with
a reason, rather than exiting 1 the way the shipped 26.8.8 did, and the reason must name the
engine rather than being the parent's epitaph for a dead child. The fixture is a directory with
no PDFium in it, so it needs nothing generated.

**Four more on 2026-08-26, so the number is now 23** --- measured **23/23 with none not
applicable on macOS**; the Windows figure was 19/19 before they existed and has not been
re-run. They put a worker on the save's *verification* side, which nothing exercised until
then: `save::InWorker` was reachable only from `lib.rs`, so every test and every other probe
passed `save::Here` and the shipped verifier was proved by compiling.

What they assert is a differential --- the worker and the coordinator asked the identical
question about identical bytes --- plus the two things a differential cannot say on its own.
That the worker's refusal is **`lopdf`'s and not PDFium's at document-open**: the fixture is a
real document with a trailer pointing at offset 999999999, which PDFium reconstructs and opens
happily while `lopdf` names the cross-reference table, so the two messages differ and the
assertion pins the wording. And that a **worker was involved at all**, which no comparison of
answers can establish, since an `InWorker` delegating to `Here` would agree everywhere; that
one points the verifier at a directory with no PDFium in it, where `Here` still answers and
`InWorker` cannot start a child.

Both of those exist because the first draft got it wrong in the reassuring direction --- it
planted a file that was not a PDF, the worker refused it at open, and the check reported `[OK]`
having never run `lopdf`. `docs/TRAPS.md` has both entries.

**Five more on 2026-08-28, so the number is now 28** --- measured **28/28 with none not
applicable on macOS**; the Windows figure was 19/19 before any of the last nine existed and has
not been re-run. They put a worker on the save's *writing* side, which is the half
`docs/THREAT-MODEL.md` residual risk 18 was still disclosing: the same four shapes as the
verification checks above, plus one that only this path can make.

The differential here is **byte for byte** rather than a number, and it is affordable because a
rewrite of one document under one plan is deterministic --- every date in the output comes from
the plan's own marks and not from the clock. On `testdata/comments.pdf` under a plan that turns
every page, `save::Here` and `save::InWorker` both write 222,667 identical bytes. A comparison
of lengths or page counts would have passed for a worker that dropped the turns.

The extra check is the one that says the **output channel** is real: an ordinary worker,
spawned with no output file, must refuse the rewrite in words rather than writing a document
into whichever descriptor happens to be open at that number. Every other check in the section
would pass just as well if the descriptor were handed over unconditionally and
`worker::OUT_ARGV` did nothing.

**What the move costs is printed beside them.** On `comments.pdf` (4 pages, 238 KB) the
rewrite is **2.4 ms in this process and 11.4 ms in a worker, +9.0 ms** --- best of five
interleaved, minima rather than means, because the question is what the work costs and not
what the machine was doing while it ran. That delta is one process spawn plus PDFium's
initialisation, so it is **fixed rather than proportional**: on a document where the parse and
the serialisation are hundreds of milliseconds it is noise, and this fixture is close to the
worst case for it. Nothing else on the save path got slower --- the parse moved, it did not
happen twice.

⚠ **Run it on Windows before the next release.** The rewrite's output channel there is a
`DuplicateHandle` into the child rather than a `dup2` before `exec`, and no run has yet
exercised it --- so the five checks above are macOS evidence for a mechanism that has two
implementations. `AGENTS.md` records what a single sentence about two platforms costs.

Reverting `worker_child`'s bind arm to `bind(&library_dir)?` turns both red with
`worker stopped answering (exited with 1 (0x00000001))` --- which is the string the reader who
reported it saw, reproduced from the other end. That is the mutation to re-run if either check
is ever in doubt.

The probe's own reading on `incr-scan-40p.pdf`, which is the strongest form of the result
because it is the same three numbers macOS printed:

```
[INFO] the append moved the worker's peak commit 359.5 -> 1027.8 MB (+668.3)
[INFO] that is 95.7% of the 1024 MiB the job object allows, leaving 43.8 MiB
[WARN] 43.8 MiB of headroom against the commit cap --- a larger document cannot
       have its save prepared in the worker
```

macOS reads 362.7 -> 1029.8 (+667.0) for the same fixture. **Baseline, total and delta all
agree**, which is what settles that the two metrics are measuring the same thing and that the
delta was never the term to compare.

The `[WARN]` fires whenever headroom falls under `THIN_HEADROOM_MIB` (128 MiB, roughly what a
42 MB scan costs to prepare). It fires today on the largest fixture in the repository, and that
is correct rather than noise --- it goes quiet when the append stops carrying a discarded copy
of the previous revision, and not before.

**The probe appends a document the application would not.** `save::APPEND_MAX_BYTES` bounds the
production path at 256 MiB, so a 336.6 MB scan is reserialised rather than appended when a
reader saves it. `worker-probe` asks the worker for the append directly and therefore still
measures it, which is deliberate: the bound is a judgement placed under a measured ceiling, and
it can only stay under it if something keeps measuring where the ceiling is. A run whose `[WARN]`
disappears is the signal that the bound can rise.

The sweep that bracketed the ceiling was read from outside the process the first time, through
PSAPI's `PagefileUsage` / `PeakPagefileUsage` over the probe's children. On `MOTHERSHIP`
(x86_64):

```
fixture                    file   peak commit   of the 1 GiB cap   append built?
incr-scan-5p            42.1 MB    134.7 MiB    13.2%              yes, 16/16
incr-scan-20p          168.3 MB    496.9 MiB    48.5%              yes, 16/16
incr-scan-40p          336.6 MB    980.3 MiB    95.7%              yes, 16/16
(41 pages, scratch)    345.0 MB   1004.4 MiB    98.1%              yes, 16/16
(43 pages, scratch)    361.9 MB   1020.7 MiB    99.7%              NO,  12/16
(48 pages, scratch)    404.0 MB   1020.5 MiB    99.7%              NO,  12/16
```

**The reasoning was wrong about which term to compare, not about the mapping.** The mapping
really is file-backed and not commit --- peak working set runs ~343 MB above peak commit on the
40-page scan, which is the document. But macOS `phys_footprint` excludes clean file-backed pages
too, so the mapping is absent from the 1029.8 as well, and the 362.7 MB baseline taken for it is
PDFium's own allocation, which is private commit here. The two metrics measure the same thing:
**980.3 MiB = 1027.9 MB against the macOS 1029.8 MB, 0.2% apart.** The last two rows are their
own control on the reading: commit stops at 1020 MiB and the allocator then fails, so the number
being read is the number the kernel is enforcing.

So `incr-scan-40p.pdf` --- the largest fixture in the repository --- sits **4.3% under the cap**,
and the ceiling is bracketed rather than extrapolated: 345.0 MB saves, 361.9 MB does not. Above
roughly **350 MB an append cannot be built on Windows.** The failure is the safe direction and
worth stating exactly: the allocation fails, the worker aborts with `0xC0000409`, and the append
is prepared *before* `save_document` closes the document --- so it is a `refused`, nothing is
written, and the reader keeps their edits. What they are told is `worker stopped answering
(exited with 3221226505 (0xC0000409))`, which names neither the size nor the cap.

The asymmetry that makes this odd from a reader's chair: only the **append** runs in the worker.
`save::Mode::Rewrite` goes through `spawn_blocking` in the app process, which is under no job
object --- so on a 400 MB scan, highlighting a line cannot be saved while highlighting a line
*and deleting a page* takes the uncapped path. `docs/PLAN.md` §3 carries the ranking this
measurement now speaks to.

**11/11 checks, 1 not applicable**, on `text-base14`, `text-cid`, `vector-heavy` and `rotated`
--- tiles **pixel-identical** to the in-process render, plus text extraction, outlines and
search across the boundary. That is what the run measured on 2026-07-29 and is left as it was
read: the probe gained three checks on 2026-08-22 --- a save's update section built across the
boundary, re-parsed after being appended, and compared against the length it was built for ---
so a current Windows run reports **14 of 14 with one not applicable**, and nobody has taken one.
A count in prose is a dated statement about a dated run; the probe's own output is the
authority, and macOS measured 17/17 that day.

**There is no not-applicable one any more, as of 2026-08-22.** It was the parent's memory poll,
skipped here on the grounds that the job object caps commit in the kernel so there is nothing to
poll --- true, and the wrong conclusion: a kernel bound makes the reading matter *more*, because
what a reader needs is how close the worker came to being refused. What was missing was a way to
look, not a reason. `Contained::peak_commit` is it, and both platforms now report **17/17 with
none not applicable**.

Two things that check does *not* cover, deliberately, because a `cargo test` child is the test
harness and never answers: pipe **direction** and content. Both are the probe's job, measured
by mutating the pipe pair and watching the probe go red --- see the trap *A test whose child
never answers cannot see the pipes being crossed*.

**Windows no longer fails open** (2026-07-29). `Backend::default_here()` selects workers there,
and the evidence is external rather than a mark of our own:

```
python scripts/win_modules.py <pid>          # on its own
python scripts/viewer_check.py <exe> <pdf>   # samples it throughout a real run
```

`viewer_check.py` now launches the app rather than blocking on it, reads the loaded module list
from outside the process while a document is open, and takes the **union** of its samples ---
the parser is mapped only while a document is open, so a single look could miss it in either
direction. The module count is printed beside the verdict, because an enumeration that read
*nothing* reports "not mapped" exactly as containment does; a peak of zero is reported as a
broken observation, never as a pass.

Run **before** the flip it reported `[FAIL] the app process mapped the PDF parser, 47 modules
at peak`. That control is why the pass afterwards means anything. After: four corpora green
with unchanged ran/skipped splits, no `[WARN]`, 44--45 modules at peak, no `pdfium` among them.

That line is printed *outside* the check names on purpose --- those are `viewercheck.ts`'s
and are the cross-platform invariant, and adding a Windows-only name to that set would make the
two platforms look divergent when they are not.

**Outside** means on **stderr**, and the passing direction of it went to stdout until
2026-08-02. `mutate_viewer.py` reads check results from stdout alone for exactly this reason
and its own docstring says so, so on Windows every baseline silently carried a 
"check name" that no mutation could turn red --- and a mutation whose expected name happened
to be a prefix of that line would have been matched against the wrapper rather than against a
check. Both `[FAIL]` forms had been on stderr from the start; only the `[OK]` was not, which is
the direction nobody reads. Same family as the repository's own trap about a wrapper's verdicts
sharing a check's shape, arriving in the harness written after that trap was recorded.

#### Pre-spawning, and what it is worth here

Implemented 2026-07-30, so both platforms start a worker before a file is chosen. Only the
handover differs. A macOS parent sends a descriptor as `SCM_RIGHTS`; a Windows parent
`DuplicateHandle`s the document section **into the running child's handle table** and then sends
a `Handover` line naming the number it wrote. Writing into a low-integrity child is the direction
integrity levels permit, so this crosses the boundary for the same structural reason the macOS
one does. `Handover` is deliberately not a `Request` variant --- a handover is legal exactly once,
and keeping it out of the request vocabulary makes a second one unsayable rather than something
the child has to refuse.

```
cargo run --release --example prespawn-bench -- --rounds 6 \
    text-base14.pdf text-truetype.pdf text-cid.pdf vector-heavy.pdf
```

| fixture | size | spawn now (min/med/max) | pre-spawned | saved |
|---|---|---|---|---|
| `text-base14.pdf` | 888 B | 10.10 / 10.38 / 10.62 ms | 0.69 ms | **+9.64** |
| `text-truetype.pdf` | 20 KB | 8.70 / 8.87 / 9.75 ms | 0.44 ms | **+8.42** |
| `text-cid.pdf` | 22 KB | 8.51 / 8.99 / 9.46 ms | 0.45 ms | **+8.55** |
| `vector-heavy.pdf` | 2 MB | 75.09 / 75.78 / 76.54 ms | 66.77 ms | **+9.15** |

**The shape of the saving is not the macOS one, and that is the finding.** There the interval
splits into a ~6.6 ms floor plus ~7.4 ms of system-font enumeration paid only by documents that
embed nothing. Here the saving is nearly constant at ~9 ms and the font component is **~1.4 ms**
--- `text-base14`, which embeds nothing, costs 10.38 ms against 8.87/8.99 ms for the two that do.
So on Windows pre-spawning buys almost entirely the fixed floor: `CreateProcess`, the loader,
mapping `pdfium.dll`, the token and the job.

Read that 1.4 ms as a between-document comparison, not as the warm/no-warm control. The bench's
own `a warmed worker does not pay the font walk` check needs `text-heavy.pdf`, which this machine
has not generated, and it `[SKIP]`s with that reason rather than quietly not running.

### `backend-probe` on Windows, and the defect it found in itself

```
cargo build --release --example backend-probe
./src-tauri/target/release/examples/backend-probe.exe testdata/text-base14.pdf
./src-tauri/target/release/examples/backend-probe.exe testdata/vector-heavy.pdf
```

| fixture | passed | skipped | failed |
|---|---|---|---|
| `text-base14.pdf` | 38/42 | 4 | 0 |
| `text-cid.pdf` | 38/42 | 4 | 0 |
| `outline-hostile.pdf` | 39/42 | 3 | 0 |
| `vector-heavy.pdf` | 40/42 | 2 | 0 |

**The name total is 43 as of 2026-08-16** --- *"comments return the same list on both"* was
added with the comment layer --- and the four rows above are the Windows measurement at 42,
left as they were taken. macOS re-measured the same four the day the check landed and reports
each row's passed column one higher against the new total: `39/43`, `39/43`, `40/43`, `41/43`,
with the skip counts unchanged. Windows has not been re-run.

That check compares what the two backends *return*, not whether either is right: a defect in
`annots.rs` breaks both identically and it stays green. `comments-probe` is what says the
answer is correct; this says the worker boundary does not change it. Proved to bite before
being trusted --- truncating the worker's reply to three comments turns it red, and restoring
it turns it green, on the same fixture in the same minute.

Re-measured 2026-07-31. **The earlier `41`s were not a missing check**, which is what they
looked like: this table read `37/41 ... 40/41` against macOS's 42, and a handover went out
asking which check was macOS-only and proposing that the flat "all 42 names appear" sentence
above become a per-platform one. Nothing is macOS-only. The 41s were taken at `df1ca61`, and
`9fb728f` --- the very next commit to touch this file --- added *"a search option crosses the
worker boundary"*. Windows has had all 42 ever since; the name sets are byte-identical across
all four corpora here, diffed rather than counted.

Worth keeping as the shape of the error rather than only its answer: **a count taken at one
commit and compared against a count taken at another is not a platform difference**, however
neatly the two platforms line up on either side of it. The cheap discriminator is the one that
settled it in a single command --- grep the *name* out of the source at each commit, rather than
reasoning about which check a platform might lack. The plausible hypothesis on offer was the
parent's memory poll, since `worker-probe` really does skip that one here; it was wrong, and it
was wrong in the direction that would have put a false per-platform caveat into this file.

That added check is also why `vector-heavy` moved from 1 skip to 2 while its passed count stayed
at 40: it skips where the search option changes nothing, which is a corpus with no extractable
text. The other skips are a slow enough render for the three withdrawal checks (only
`vector-heavy` has one) and a second page to confuse a page number with. The boundary, the pixel comparisons, capacity,
crash restart, replacement, retirement, close, descriptor return **and the spare's lifetime** all
pass. Its Windows primitives are Toolhelp for the module list and the process table,
`GetProcessHandleCount` for descriptors, and `TerminateProcess` for a hostile kill from outside
the pool --- deliberately not `Contained::kill`, since the pool has to notice a death it did not
cause.

This is also where the Windows spare is proved end to end, and the detail says more than the
count: `at open: pool [18840], children [2672, 18840], spares [2672]` --- a warmed child exists,
is excluded from the pool rather than miscounted into it, and `opened with 1` beside it keeps the
laziness claim. `a spare does not outlive the service that started it` reports
`its 1 spare process(es) [58096] went with it`.

**It first reported 34/41, and the two failures were the probe's own.** They said a burst grew
the pool to six and 1.2 s into a 4.0 s idle timeout one was left, with **144 handles with one
worker, 144 grown, 144 retired** beside it --- and five extra workers cannot cost zero handles.
Two independent observations agreeing, and the diagnosis drawn from them (created, used and
**destroyed rather than pooled**) was recorded here as an open defect for a day. It was wrong.

Both numbers were honest; neither could say *when* it was taken. `settled_descriptors` waits up
to five seconds for a pre-spawned spare to appear, Windows has none, and the verdict of that wait
was discarded --- so it spent its whole bound on every call, which is longer than the idle timeout
the phase runs at. The instrument retired the pool and then measured it. One worker of six and a
lean handle count are precisely what a correct pool looks like five seconds after a burst. The
pid clause is now asked for only where a spare can exist, and a wait that expires says so with a
`[WARN]`. Nothing in `workers.rs` changed.

Do not "fix" a failure here by relaxing a check --- but do check the clock before believing one.
The pre-fix run remains the red control for both: they were observed failing, are now observed
passing, and `an idle pool is retired down to one worker` is green on both sides, so retirement
was never the thing that broke.

**A second check went red the day pre-spawning landed, and it was the same shape.** `closing
gives back every descriptor opening took` reported *137 quiet, 145 with it open, 142 after
closing it* --- five handles, one spare's worth. Nothing leaked: an `open` consumes the warmed
spare and starts a replacement on another thread, so a raw sample includes one spare or not
depending on how far that thread has got. macOS forks and wins that race; Windows creates a
process, a token, a job and a fresh map of `pdfium.dll`, and does not. Its three samples now go
through `settled_descriptors`, which exists for exactly this and predated them. See the trap ---
the lesson is that passing on one platform was evidence about that platform's timing.

**Of the "four probe binaries that refuse to act as a worker off unix", one did.** That list was
in this file and in `AGENTS.md` for two days and was wrong about two of its four entries, in the
direction a list written by reading always errs --- see the trap. What was actually true:

- `pool-bench`, `prespawn-bench` --- a real `#[cfg(unix)]` gate on the `--render-worker` re-exec,
  dating from before `worker_child` compiled on Windows. Worth understanding before copying it:
  each binary re-execs *itself* as a worker, so gating that made the benchmark **unrunnable**
  rather than degraded. Ported 2026-07-30, along with the hardcoded library path.
- `tile-bench` --- **never refused anything.** It ran on the first try and failed at
  `LoadLibraryExW` on the hardcoded path. Ported the same day; numbers below.
- `worker-bench` --- seven of its eight modes genuinely refuse, and the reason is accurate: it
  carries its own POSIX worker implementation, fd passing and SBPL profile bisection included,
  and shares no mechanism with the job-object model. Those need a spike, not a port, and the
  refusal now says what such a spike would measure that nothing else does (the per-tile overhead
  decomposition of `latency` mode --- parallel scaling is `pool-bench`, the authority rungs are
  `win-sandbox-probe`, crash and timeout are `backend-probe`, and `limits`/`footprint` are
  answered by the job object capping commit in the kernel).

  That last clause was an assertion when it was written and is a measurement as of 2026-07-30:
  `win-sandbox-probe` now probes the job's own two limits, which it had promised in its table
  and never tested. `bare` commits 1 GB and starts a second process; every rung with a job is
  refused with 1455 (commit charge) and 1816 (process quota). So `limits` and `footprint` are
  retired on Windows honestly rather than by hand-waving, and `latency` is the only mode left
  whose question nothing here answers.

  **The eighth mode ran here for the first time, and it does not say what the threat model does.**

```
./src-tauri/target/release/examples/worker-bench.exe --mode engine --lib vendor/pdfium/bin
```

  `--mode engine` spawns nothing --- it reads the library file --- and was unreachable off unix only
  because it sat inside a `#[cfg(unix)]` module. It is at file scope now, and on Windows it
  reports **`[NOT VERIFIED]`**: the shipped `pdfium.dll` carries no local C++ symbols
  (`CPDF_Document` is absent), so `v8::` and `CXFA_` being absent from it means nothing. That is
  the harness's second control working exactly as written --- and it means
  `docs/THREAT-MODEL.md`'s promotion of "JavaScript is disabled" to "there is no engine to
  disable" is established on **macOS only**. On Windows it rests on the asset name and pinned
  digest `fetch_pdfium.py` asserts, which is a claim about which file was fetched rather than
  about what is in it. The threat model now says so.

  It also prints the one dimension that survives stripping, because exports are always named:
  **460 exported functions, four of them XFA-named** --- `FPDF_LoadXFA` and
  `FPDF_GetXFAPacket{Count,Name,Content}`. Surface, not a contradiction: the three
  `GetXFAPacket*` calls read `/XFA` streams out of an AcroForm dictionary and need no XFA engine.
  Whether `FPDF_LoadXFA` is a stub there is open, and unlike JavaScript it is behaviourally
  decidable --- a fixture carrying an `/XFA` packet makes `FPDF_GetXFAPacketCount > 0` a positive
  control, so `FPDF_LoadXFA` returning false on it would mean the implementation is absent rather
  than the document empty. Not written; that fixture does not exist.

  Both numbers were cross-checked against a throwaway Python PE parse before being written down
  --- two independent parsers, same 460 and same four names. Every branch was exercised: a
  non-PDFium file `[FAIL]`s, a file that passes both controls but is not a PE reports "not a PE
  image" rather than a zero, a missing `--lib` exits 2, and another mode still refuses.

**Numbers are macOS arm64 unless a Windows one says so.** The pre-spawn table above and the
tile-bench section below are the sets taken on Windows and are labelled as such; everything else
in this file and in `AGENTS.md` still is not, and the platforms are far enough apart --- a ~1.4 ms
font walk against ~7.4 ms --- that carrying a figure over is a guess, not an estimate.

### `tile-bench` on Windows, and what the render constants cost here

```
cargo build --release --example tile-bench
./src-tauri/target/release/examples/tile-bench.exe testdata/vector-heavy.pdf --mode single --rounds 4
./src-tauri/target/release/examples/tile-bench.exe testdata/text-base14.pdf  --mode single --rounds 4
```

It needed two fixes and neither was a refusal: the hardcoded `vendor/pdfium/lib` (on Windows that
directory exists and holds the *import* library, so it fails at `LoadLibraryExW` rather than at a
missing path --- see the trap), and `peak_rss_mb`, which returned `NaN` off unix. That is
`GetProcessMemoryInfo`/`PeakWorkingSetSize` now, keeping the `NaN`-on-failure contract because a
zero would read as "PDFium allocated nothing". A working set is trimmed under memory pressure, so
it can read below the peak *commit* the same run reached; it is still the right counterpart to
`ru_maxrss` for this question, since both are about pages actually held.

**§9's architectural conclusions hold on Windows, and every constant behind them is worse.**
`vector-heavy` is generated by the committed `make_vector_pdf.py` against the same PDFium pin, so
this row is a fair comparison of *constants* --- across different machines, which is the useful
framing here rather than a CPU verdict:

| | macOS arm64 (`docs/PLAN.md`) | Windows (2026-07-30) |
|---|---|---|
| 256² tile of the A0 page | 0.98 s | **1.35 s** |
| that tile as a share of a full render | 4.3% | **3.8%** |
| full page, 1× | 22.8 s | **35.1 s** |
| full page, 2× | 48.4 s | **88.3 s** |
| fixed cost per render *call* | ~1 s | **~1.3 s** |

So the shape is the same on both --- PDFium culls spatially, a tile is a few percent of a full
render, and there is a hard per-call floor that does not shrink with the request. The magnitudes
are **1.5--1.8× worse**, and the floor about a third worse. The practical consequence: a latency
budget written against the macOS floor is optimistic on Windows by roughly that much, and
`docs/PLAN.md` §4's four consequences now rest on two platforms rather than one.

Independently cross-checked on the same machine before being believed: `backend-probe` measured a
**1536 ms** 512² render of the same document through the worker, against tile-bench's 2203--3073 ms
for that tile size. Same order, differing about as much as a centred tile and a placed one should
--- which is what says the numbers are the document's and not the harness's.

The cheap-page half confirms the asymmetry the plan bets on: `text-base14` is **flat**, 0.6--0.9
ms/Mpixel at every tile size and scale, with no per-call floor at all. Read that as a Windows
result on its own and **not** as a comparison --- macOS measured `text-heavy.pdf`, which this
machine has not generated, so the two cheap-page numbers are different fixtures.

### `pool-bench` on Windows: what a pool buys a screenful

```
cargo run --release --example pool-bench -- testdata/vector-heavy.pdf --tiles 6 --rounds 4 --sizes 1,2,4,6,8
```

Six 1024² tiles of the A0 page, which is what one screenful is. Two runs, so the stable
conclusions can be told from the noisy ones:

| pool | run A | run B | macOS (spike 0.5) |
|---|---|---|---|
| 1 | 5105 ms, 1.00× | 5176 ms, 1.00× | 1.00× |
| 2 | 1.34× | 1.52× | --- |
| 4 | 1.99× | 2.29× | 2.56× |
| 6 | **3.59×** | **3.60×** | **3.22×** |
| 8 | 3.60× | 3.54× | nothing further |

**The shape reproduces exactly**: monotone gains to six and nothing at eight, which is the
capacity ceiling doing its job. Six is stable to within 0.01× across runs and is slightly
*better* than macOS.

**Do not read the middle rows as a platform difference.** Pool 2 moved 1.34 → 1.52× and pool 4
1.99 → 2.29× between two identical runs, and the per-round warm figures span ±20% (pool 2: 2625
to 3894 ms). Only the pool-6 result and the flat pool-8 result are outside that spread. The
per-round table is printed for exactly this reason --- a single speedup column would have made
the intermediate points look like measurements.

Cold and warm are indistinguishable here (5155 vs 5105 ms at pool 1, 1100 vs 1094 at pool 6),
because a ~9 ms spawn is noise against a 5 s screenful. That is a property of this corpus, not
a finding about spawn cost.

Still macOS-shaped, but less than it was: **`open_check.py` runs five of six phases** since the
single-instance plugin closed the document-handover gap. The one that stays macOS-only is the cold
double-click, and that is not a gap --- an Explorer double-click arrives in `argv`, which the
`argv` phase already covers. It skips with its reason rather than disappearing.

`session_check.py` needed no porting at all. It does need a document of **at least eight pages**,
since its target page is 7 and `goToPage` clamps; on a shorter one it now says so as a named check
rather than reporting a wrong page, which is what it used to do. `incr-scan-20p.pdf` is the quick
fixture for it; `text-base14.pdf` (1 page) and `rotated.pdf` (4) are not long enough.

Both now call `clear_strays` before their first launch. That is not tidiness: on Windows a
leftover instance **silently absorbs** every later launch through the single-instance plugin, so
the next phase reports `run timed out` with no output --- which reads as the app hanging. It prints
a `[WARN]` naming the pids when it finds any, because a run that needed it is a run whose earlier
phases are suspect.

`webview_guard` still checks nothing off darwin (see the trap --- Chromium throttles occluded
windows too, so those runs are protected by nothing).

#### What the port changed, so it is not rediscovered

- `worker.rs` now compiles everywhere and refuses off macOS --- which its own module doc had
  claimed since it was written, and which was not true. 38 error sites, all POSIX:
  `std::os::fd`, `mmap`/`munmap`, `File::from_raw_fd`, `ExitStatus::signal`. `Shm` off unix is
  a type with a private field and constructors that refuse.
- `worker_child.rs` was `#[cfg(unix)]`, with the `--render-worker` argv refusing off unix
  rather than falling through. **Both are gone as of 2026-07-29.** The module compiles
  everywhere; three functions know the platform (the two mapping handovers and
  `establish_boundary`) and the rest is shared. The refusal that replaced the `cfg` is
  `establish_boundary` itself, which fails where there is no boundary to establish and does
  so *before* a document is opened --- the deleted one was never the load-bearing guard, and
  keeping it would have suggested otherwise.
- `pdfium_library_dir()` picks `bin/pdfium.dll` on Windows against `lib/libpdfium.dylib` on
  macOS, and now checks for the **library** rather than the directory. See the trap: on
  Windows `vendor/pdfium/lib` genuinely exists and holds the import library, so the old
  existence check passed and the bind failed later.
- `launch.rs`'s percent-decoding test takes a platform-shaped URL. `Url::to_file_path` wants a
  drive letter on Windows and refuses `file:///Users/...`, so the macOS fixture asserted only
  that refusal. Written that way rather than gated off Windows, deliberately --- a check that
  silently stops existing on a platform is the thing this file warns about elsewhere.

#### What running it found, which no gate could

Three defects, none of which any amount of compiling would have surfaced.

- **`npm run tauri build` failed on a tree that gated 7/7.** `backend_probe.rs` called two
  dyld symbols unguarded; clippy never links, and `cargo test` links a `[[bin]]` with `main`
  replaced, which drops them as dead code. There is a **`bins` gate** now, and it was proved
  to fail (5.7 s, debug profile) against the un-gated file before being trusted. The probe
  itself is now a thin entry point over `backend_probe/imp.rs`, refusing off macOS the way
  `fdpass_probe.rs` does --- every claim it makes is about a worker backend that cannot exist
  there.
- **Not one tile was ever painted.** `tiles.ts` fetched `tile://localhost/...`, which WebView2
  cannot resolve; Tauri serves custom protocols at `http://tile.localhost/...` on Windows. The
  origin now comes from Tauri's own `convertFileSrc`, and the CSP names
  `http://tile.localhost` beside `tile:` --- it already named `http://ipc.localhost` beside
  `ipc:`, so the convention was known and applied to one scheme and not the other.
- **`cargo build --release` is not a production build.** It produced a window showing
  *"localhost refused to connect"*: `frontendDist` is embedded by the cargo feature
  `tauri/custom-protocol`, which the Tauri CLI passes and a bare cargo build does not, at any
  optimisation level. Build through `npm run tauri build`, or pass the feature.

**The old version of this section named the wrong blockers**, and the shape of the error is
worth keeping. It listed `sanitize_rewrite.rs` and `tile_bench.rs` as the compile errors;
both were real, but clippy never reached either, because the *library* failed first. A
blocker list assembled by reading code cannot know what fails first --- that is a property of
the build graph. It also said `TPDF_BACKEND=in-process` was "the only thing that runs off
macOS", which was false: `pub mod worker;` was unconditional, so the crate carrying that
control did not compile and nothing ran off macOS at all.

---

## Running it

```
npm run tauri dev -- --release
```

**Never benchmark through `tauri dev` without `--release`.** It shells out to `cargo run`
in the dev profile, and because PDFium arrives as a prebuilt optimized dylib the result is
not uniformly slow but *selectively* slow --- PNG encoding of a tile measured 67 ms in debug
against 1.41 ms in release while the PDFium render beside it moved 1.39 -> 1.36 ms. Ratios
invert rather than merely inflate.

**Startup timing needs a bundle, not just `--release`.** Under `tauri dev` the frontend is
served by Vite over HTTP, so a startup measurement describes Vite's module graph:

```
npm run tauri build -- --bundles app
scripts/startup_bench.py target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf <file.pdf>
```

Run the executable inside the `.app` directly --- that keeps stdout and the environment,
which `open -a` does not. `--purge` gives a genuinely cold page cache and needs a sudoers
entry for `/usr/sbin/purge`.

### Which backend parses the document

Documents are parsed in a sandboxed worker process, one per document. `TPDF_BACKEND`
overrides that:

```
TPDF_BACKEND=worker      # the default on macOS
TPDF_BACKEND=in-process  # the control, and the only thing that runs off macOS
TPDF_POOL=6              # workers one document may have
TPDF_IDLE_MS=30000       # how long one may idle before it is killed
```

`TPDF_IDLE_MS` is a quantity and **zero means zero** --- retire at the first sweep. There is
deliberately no spelling for "off": a "no value" marker taken from the value's own range is
how a sentinel collides with a real value the moment the timing is right, which this
repository has already paid for once. A caller that wants no retirement asks for a long
timeout. Unlike `TPDF_BACKEND`, an unreadable value here falls back to the default rather
than refusing, because it cannot make two measurements silently incomparable --- every
harness that depends on the timeout is handed one explicitly.

Anything else is **refused before the window is created** --- one line on stderr, exit 2. The
variable exists to say which of two implementations ran, so a value that quietly selected
the other one would make any comparison between them meaningless, and `in_process` for
`in-process` is one underscore away.

The refusal is read in `run()` rather than where the backend is used, and that placement is
the whole of its value. `RenderService::start` runs in the Tauri setup hook, which `App::run`
invokes from AppKit's frames --- a panic there is non-unwinding, aborts through a backtrace
with no symbols, and races the watchdog's 30-second report about a page that never ran. A
misspelt variable would be diagnosed as an occluded window.

Two things read differently under the worker: the startup timeline has `worker spawned`
where the in-process one has `pdfium bound`, and a render can now fail because the worker
died rather than only because the document did. `backend-probe` is what says the two agree
about everything else.

A worker that dies is replaced and the request retried once, so a crash usually reaches the
reader as nothing at all --- but it is never silent in the terminal: the parent prints
`[render] document N: worker killed by signal 11; starting a replacement` on stderr, and the
worker's own stderr is inherited. Seeing that line repeatedly on one document means the
document is faulting PDFium on a page the reader keeps asking for, which is the one case a
single retry cannot make cheap.

### `ocr-probe`: does the recogniser work, and is the flip right

macOS only --- it is the Vision binding it exercises. Nothing in it is wired into the viewer;
OCR has interfaces, one engine and a control chooser, and no worker yet.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| fixture | result |
|---|---|
| `text-base14`, `text-marked`, `text-truetype`, `text-cid`, `rotated` | 9/9 |
| `outline-simple` | 8/8, 1 skipped |
| `form` | 7/7, 2 skipped |
| `columns` | 2/2, 4 skipped --- two columns leave no vertically isolated span to use as a control |
| `vector-heavy` | 1/1 against the *inverted* claim: the page has no text, so reading none is correct |
| `links` | **7/8**, 1 skipped --- one expected red, below |
| `encodings` | **7/8**, 1 skipped --- one expected red, below |
| `text-wide` | 9/9 --- the wide-sheet fixture, below |

⚠ **Those counts were two behind on 2026-08-28 and are re-measured here.** The shape sweep's
control was added in the same commit that last touched this table and the row was not moved with
it, so every fixture the sweep runs on read one low before today and two after. `columns` and
`vector-heavy` are unchanged, which is the tell that the drift is the sweep: it is skipped on
both. A count in prose has no gate behind it --- derive it from a run, and re-derive it whenever
a check is added.

**A shape sweep prints above the checks**, added 2026-08-28 and not a check --- it passes and
fails nothing. It exists because the corpus probe's largest remaining bucket is the engine
*answering* and returning no spans at all, and a corpus measurement cannot look at the image it
was handed while a fixture can.

**It sweeps the region strip's height, which is the corpus's own variable**, and every row is a
real `ocr_gate::stack` output rather than a resized one --- so the sweep and any padding change
are the same code path. The control strip is byte-identical in every row. Two earlier drafts got
the construction wrong and both were corrected the same day: the first used the page's tallest
blank band and measured a shape the gate never builds, and the second grew the image with
`Vec::resize`, which appends white *below the bottom margin* rather than widening the image the
way `stack` would.

⚠ **The fixed rows cap the aspect, so the band the corpus goes silent in cannot be built here at
all.** `stack` always writes two margins, the gap and the control strip, and on these fixtures
that is 104 to 117 px of a 1190-wide image --- a ceiling of **10.1:1 to 11.3:1**. Every target at
12:1 and wider prints how many rows short it is instead of a reading, because a shape that was
never built must not read like a shape that was tried and said nothing. Reaching past 16:1 needs a
*shorter control*: the aspect is `width_pt / (tallest + control_pt + padding)` and the scale
cancels, so with `padding` fixed at 24 pt a 595 pt page needs the region and the control together
under about 13 pt.

**Two checks are the controls over the sweep.** The strip has to read at *some* shape, or every
row is a statement about that strip rather than about the proportions; and the token has to read
back at the gate's own shape, or a "no" further out is not evidence about the shape either. The
second was written from `height == real_h` inside the loop, which no swept aspect ever produces
--- it failed on all four fixtures and was right to. It reads the gate's own image explicitly now.

**The `trailing` column separates the aspect from where the padding rows sit**, and it overturned
the reason padding was called a candidate rather than a win. At equal height and equal aspect,
`outline-simple` at 4.0:1 reads the token back when the white is in the region strip and **not**
when it is appended below the bottom margin. The earlier "padding loses the token at 1.9:1" was
about trailing whitespace after the control, not about proportions. Through `stack`'s own
construction the token reads back at every buildable shape on `text-base14`, `outline-simple` and
`encodings`.

**`testdata/text-wide.pdf` is the only fixture that reaches the band the corpus goes silent in**,
and it says the shape is innocent. A 1684 pt sheet with ordinary 14 pt text builds an **18.1:1**
probe image and sweeps to **28.1:1**, where A4 with the same text caps at 10.8:1 --- the lever is
the page's width, so the control strip stays a comfortable 34.5 pt against A4's 30.5. Vision
returns a span and reads the token back at 28.1:1, 24.1:1, 20.0:1, 18.1:1, 16.0:1, 8.0:1 and
4.0:1, and loses only the token (not the span) at 2.0:1. `docs/PLAN.md` §6 has why that kills the
padding repair: on a page of ordinary width a wide probe image *requires* a small control, so the
corpus could never separate the two.

⚠ **The two existing fixtures that come closest cannot answer it, and the sweep's own control says
so.** `text-heavy` and `incr-xrefstream` reach 12.2:1, and on both *the token reads back at the
gate's own shape* fails --- their controls are too small to be read reliably, so no column of
theirs is evidence about shape. Do not read their `no`s as the wide band starting early.

**The control-chooser check**, added 2026-08-27 --- the ninth on a fixture where every check runs, and named here rather than numbered because appending a check renames a number, and it is the only place
`ocr::control_from_page`'s claim meets a real engine. The three gate checks above it take their
control strip out of **Vision's own output**, which is the engine agreeing with itself; this one
chooses from what the *document* says and then asks Vision to read it back. It runs in both
directions: where the engine's reading and the document's text agree the chosen control must
certify, and where they do not it must refuse.

**Two fixtures report one red each, both of them expected, and they are expected for opposite
reasons.** An expected red beside a green run is a bad thing to leave lying around --- this
file has an entry about exactly that --- so here is what each is and what it would take to
remove it.

`encodings.pdf` fails *what it read matches the embedded text*, 0 of 2 words. That is the
fixture doing its job: it has no usable `/ToUnicode`, PDFium returns plausible garbage, and the
check compares the engine's reading against that garbage. Making it green needs a way to tell a
broken engine from a broken text layer, and there is not one --- the check *is* that
comparison. The chooser check reads the same disagreement and reports the refusal as a pass,
which is the honest verdict about the gate rather than about the fixture.

`links.pdf` fails *a blank strip adjudicates Illegible*: the strip control picked the token
`"Donn"` and Vision, handed the same rows inside a composite, read `"Dann 1"`. Nothing is wrong
with the page. It is the weakness the chooser exists for, showing up in the check that predates
it --- a control taken from the engine's own earlier reading is not stable across a second call
on a different image. The same fixture passes the chooser check with `"lantern"`, chosen from
the document. **The fix is to give those three checks the same control source**, which is a
change to three verified checks and is deliberately not in the increment that added the fourth.

**The check that earns its keep is the ordering one.** `normalised_to_points` has unit tests
and they cannot catch the thing that matters, because they assert arithmetic against numbers
the same file wrote --- Vision's `boundingBox` is normalized with the origin bottom-left, and
whether the conversion understands that is a question about a black box. So the probe asserts
content at a position: the word the *document* places highest must come back highest. Removing
the flip reports `read gap -119 pt against 123 pt in the document` and takes both gate checks
with it.

Two limits worth knowing before reading a run. The control band is a strip of the page's own
text rather than a drawn token, so a fixture whose lines are too close together produces no
usable strip and the gate checks `[SKIP]` rather than failing --- `columns` is that case. And
`[SKIP]` here means the harness could not construct the input, never that the gate passed.

### `win-ocr-probe`: can `Windows.Media.Ocr` be the Windows engine at all

Windows only, and it runs **in CI** --- the Windows leg of both `ci.yml` and `release.yml`, as a
step after the gates. That is the point of it: the question is what a machine nobody configured
carries, and the developer machines are all configured. Read the two `[verdict]` lines in the
job log.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example win-ocr-probe
```

**Not a gate.** It measures, and its exit code says whether it could *measure* --- 0 for any
answer including "no language packs", 2 for a call that failed. A probe that reddened CI for
reporting an inconvenient truth is one somebody switches off, and the answer would go with it.
The cost of that choice is the one `AGENTS.md` records about `18/19 gates passed`: a step that
always exits 0 is a step nobody reads, so the verdict lines are written to be grepped.

Four readings, and the last two are why this is not an enumeration:

| reading | what it decides |
|---|---|
| `AvailableRecognizerLanguages` | whether the in-box engine ships at all, which is `docs/PLAN.md` §9.10's ranking |
| `TryCreateFromUserProfileLanguages` | whether the call an implementation would make comes back |
| `MaxImageDimension` | a real bound on `ocr::Pixels`, since the gate hands over a composited image whole |
| a word and a **non-word**, read back | whether `Options::language_correction` can be honoured here |

The last row is the one that reaches the interface rather than the ranking. That option is
documented as off for verification *always*, because a corrector turns marks it cannot read into
plausible words; Vision honours it (`ocr_vision.rs`'s `setUsesLanguageCorrection`) and
`Windows.Media.Ocr` exposes no such switch. A non-word coming back as something else means a
verdict from that engine means something different from a verdict from Vision.

**First reading, `windows-2025`, 2026-08-29** --- the runner image as GitHub ships it, which is
the whole point of taking it there:

| reading | value |
|---|---|
| recogniser languages | **1**, `en-US` (English (United States)) |
| `TryCreateFromUserProfileLanguages` | an engine, `en-US` |
| `MaxImageDimension` | **10000** px |
| 44 px, `"REDACTED"` / `"qwrtzp"` | both **VERBATIM** |
| 16 px, `"REDACTED"` / `"qwrtzp"` | both **VERBATIM** |

So the gating question is answered and answered well: a stock Windows carries a pack, and the
in-box engine is a feature that ships rather than one that needs the machine set up first.

The 16 px row was added the same day because the 44 px one alone is a control easier than the
check: 44 is about 3x `ocr_gate::MIN_CONTROL_PX`, a corrector's effect is largest on marginal
input, and marginal is exactly what this gate hands an engine --- a control sized from the
smallest box a redaction covered. 10000 px is a real ceiling on `ocr::Pixels`, worth knowing
before a page is composited at render scale.

**No correction was observed anywhere the probe looked**, which is the better of the two
answers `Options::language_correction` could have had. It is support rather than proof, and the
gap is specific: at 16 px this engine read clean synthetic text *exactly*, so it was never
operating near its limit, and a corrector only shows where a recogniser is struggling. What the
gate actually hands an engine is harder than this in a way size does not capture --- a control
composited beside real page ink, at whatever contrast the document has. **The remaining risk
therefore moved rather than closed**: it is no longer "the API exposes no switch, so the
contract may be silently broken" but "we have not yet seen this engine read anything it found
difficult". The instrument for that is the corpus sweep the macOS side already has
(`redact-reach-probe`), not another synthetic string.

⚠ **A blank reading for *both* strings is a suspect probe before it is a suspect engine.** GDI
writes RGB into a 32-bit DIB and leaves the alpha byte alone, so the buffer forces alpha to 255
after drawing; if that were wrong every glyph would be transparent and the engine would honestly
report no text. The comment in `draw` says so. Vary the fixture --- a larger `GLYPH_PX`, a
different face --- before concluding anything about `Windows.Media.Ocr`.

**The containment rung, added 2026-08-29.** Everything above runs at whatever integrity the
shell gave the probe, and a real engine would run where the parser worker runs. So the probe
re-execs itself with `--contained-child` through **`sandbox_win::spawn_contained` with
`Containment::default()`** --- the containment that ships, job object plus low integrity --- and
takes the same four readings there. macOS answered the mirror of this with *no*: Vision is
killed by SIGTRAP under `SANDBOX_PROFILE` and needs general `file-read`, which is why OCR is a
separate process under `OCR_SANDBOX_PROFILE`. If the same holds here, an in-box Windows engine
needs a second containment story rather than a line in the worker.

Three things make that rung worth trusting:

- **The child proves it is contained before it measures.** `sandbox_win::assert_contained()`
  first, exiting 3 if not. A child that quietly ran uncontained would report that the engine
  survives containment, which is the direction that costs something.
- **The verdict is a comparison, not a survival check.** The uncontained readings are the
  control and the two lists are compared as data. The outcome to fear is not a child that
  died but one that read something *different* --- a substituted font or a denied resource
  looks exactly like that, and `docs/TRAPS.md` records a sandboxed PDFium returning `ok`
  while silently swapping a typeface.
- **Dying is a result, not an error.** The child's exit code is read before its answer is
  parsed and passed through `sandbox_win::describe_exit`, because macOS's lesson is that this
  class of engine aborts its host rather than refusing.

It reuses `sandbox_win` rather than building a ladder of its own. `win-sandbox-probe` built six
rungs to find which one PDFium survives; that question is answered and the answer is what
`Containment::default()` implements, so a second ladder here would be a second copy of
security-critical code --- and the copy that drifts is the one nobody ships.

**Since 2026-08-29 this drives the shipping engine**, `ocr_windows::WindowsOcr`, rather than
calling WinRT itself. So every CI run exercises `WindowsOcr::recognise` end to end --- bitmap
construction, the word walk, the coordinate conversion --- contained and uncontained, instead of
a parallel copy of the same calls agreeing with itself. One thing it still cannot see: the probe
draws black on white, and exchanging two channels leaves black and white unchanged, so a missing
RGBA-to-BGRA swap is invisible to any reading here. `ocr_windows`'s own unit test is the
instrument for that, and has to be.

**Measured `windows-2025`, 2026-08-29: `reads IDENTICALLY to uncontained`.** All four readings
come back the same under job object plus low integrity as outside it, so `Windows.Media.Ocr`
does **not** repeat what Vision does on macOS --- the engine needs no separate containment story
and can run where the parser worker runs. That is the last thing standing between the interface
and a Windows implementation of `ocr::Recogniser`.

⚠ **That run also revealed a regression this probe had shipped one commit earlier**, and it was
found by diffing the CI output against the previous run rather than by any check: extracting
`languages()` and `make_engine()` replaced a region of `main` that two `say` calls sat in, so
`engine language` and `max image dimension` stopped being printed while `BUILD.md` went on
recording 10000 px as measured. Restored the same day. See the trap about a readings table
outliving the code that produced it, and note what it costs to write a measurement down: the
thing to protect is the call, not the number.

### `ocr-sandbox-probe`: what is left of a process under each profile

macOS only. Three rungs, each a re-exec'd child that renders a page **before** the profile
comes down --- the parser worker maps PDFium first too, and sandboxing earlier would measure a
different program.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-sandbox-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| rung | writes a file | reaches the listener | runs Vision |
|---|---|---|---|
| `bare` --- the control | ok | ok | 4 spans |
| `ocr` --- `OCR_SANDBOX_PROFILE` | PermissionDenied | PermissionDenied | 4 spans |
| `parser` --- `worker::SANDBOX_PROFILE` | --- | --- | killed by signal 5 |

7/7 on OS build 25G83, 2026-08-27. This makes executable the table `ocr.rs` has carried by
hand since 2026-07-31, and it measures something that one did not: the rung that worked there
allowed reads and said nothing about **writes**, while the constant that shipped denies
`file-write*` and `network*`.

**The parent holds a real listener open and passes its port**, and that is not a nicety:
`ConnectionRefused` and a sandbox denial are the same shape from a client's side, so without
something to connect to every rung reports a refusal and the row measures nothing. The `bare`
rung is the control for all three columns --- a machine where nothing works reports a
perfectly contained ladder.

### `ocr-worker-probe`: does the engine work from a process of its own

**Both platforms since 2026-08-29**, and the binary is **its own worker**: `OcrWorker::spawn`
re-execs `current_exe`, so what is under test is the shipped child rather than a copy of it.
Same arrangement `pool-bench` uses.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-worker-probe -- \
    testdata/text-base14.pdf
```

**No `--lib`, and not for brevity**: the default joins `PDFIUM_SUBDIR` --- `bin` on Windows,
where `lib` exists, holds the *import* library and binds to nothing. It hardcoded `lib` until
this became portable, and `only_the_macos_spikes_hardcode_the_library_directory` is the rule
that caught it, which is what that test is for.

**It was macOS-only for one line.** The in-process baseline named `ocr_vision::Vision`
directly; `WindowsOcr` is behind the same `ocr::Recogniser`, so only the engine's
*construction* is per-platform now and everything after it is the trait. That mattered more
than it sounds: this probe measures the **worker**, the Windows worker is the newest thing in
the subsystem, and Windows was the one platform that could not measure it. A spike is
macOS-only when its subject is, never when one line of its scaffolding is.

It runs on both CI legs, at **12.7 s** in the debug build the `bins` gate leaves behind.

| fixture | result |
|---|---|
| `text-base14`, `text-marked`, `rotated`, `links`, `columns`, `encodings` | 12/12 |
| `vector-heavy` | 0/0, 1 skipped --- A0 at scale 2 is 128 MB against a 16 MB buffer |

The check **set** is the invariant, not the total: on Windows the *engine is mapped from launch*
row is absent, because it is a statement about static linkage --- `objc2-vision` links Vision,
while `Windows.Media.Ocr` is WinRT activated through `combase` at the first call. What its
images are and when they arrive is a different question and **unmeasured**; a row asserting a
name nobody has measured would be a guess wearing a check's clothes.

**The baseline is the same program reading the same bytes in-process**, because a worker that
reads nothing and an engine that reads nothing produce identical output. Everything else is a
difference from that row, and the differential is the one that matters on every page: same
engine, same pixels, one process apart, so the text has to be *identical* and a mismatch is
the handover rather than the engine.

Two rows are there because a caller cannot recover from them. An image larger than the shared
mapping must be refused **and leave the worker usable**, or one oversized region costs a whole
document its verification. And a worker killed from outside must report inside its own
deadline rather than block on a pipe nobody will write to --- the engine ignores the
`deadline_ms` it is handed, so the parent is the only place that bound can live.

**What this probe does *not* prove, and the first draft claimed it did:** that the process
which asks never maps the engine. `objc2-vision` links Vision, so every binary linking
`ocr_vision` maps it at launch --- 2 images of 619, before a single call. `backend-probe` can
make that claim about `libpdfium` because `pdfium-render` `dlopen`s it. The check states the
measured fact instead, with an emptiness control beside it. See `docs/TRAPS.md`.

### `redact-reach-probe`: how much of a redaction can be proved, over a corpus

Not a check --- it passes nothing and fails nothing. It is the instrument behind
`docs/PLAN.md` §6's *What a removal can take, re-measured*, and it exists because the 39.1%
that section had quoted since the beginning was measured before the form carrier, before the
image carrier and before there was an OCR gate at all. **A figure that decides which increment
comes next is worth exactly as much as the date on it.**

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- \
    ~/Downloads --pages 3 --regions 40 --no-gate
```

**Counts and shapes only.** Point it at a corpus of real documents: no page text, no
recognised string and no filename beyond the stem leaves it, because a measurement that prints
what it read is one nobody can run twice.

| flag | what it does |
|---|---|
| `--pages N` | pages sampled per document, spread through it rather than off the front |
| `--regions N` | regions sampled per page --- one per word of four characters or more |
| `--max-mb N` | files above this are not opened; a rewrite copies the whole document |
| `--no-gate` | skip the write-and-read-back half, which is 40x the cost |
| `--full-width` | widen every region to the page. A **control** over the gate, not the removal |

The cheap half is 1.8 s over 40 documents and 2,893 regions; with the gate on it is about 12 s
for a twentieth of that sample, which is why the two halves are separable.

**`--full-width` is a control that failed to isolate what it was aimed at, and is kept for
what it found instead.** `ocr_gate::strip` renders the rows a rectangle covers as a
full-width tile, so widening a region leaves the row band identical and should move no
verdict. It moves them a great deal --- 54 *still readable* became 9 on one sample --- because
a wider region covers more words, which changes the control the gate may choose, which
changes the render scale. The region feeds two mechanisms, so varying it isolates neither;
what it establishes is that **the verdict turns heavily on the control choice**, which
nothing else here measures.

The gate half reads `ocr_gate::judge_all` rather than `run`, so it has the engine's own
rectangles and reports how many surviving reads were inside the region's own columns. Since
`ocr_gate::mask_columns` that has been all of them, on 104 regions and again on 448.

**Every *not verified* region is attributed to a step, and the buckets have to close.** Each
prints as its own row --- twelve of them, including the ones that never fired, because an
absent row and a zero are different readings. `NotVerifiedCause` is a type rather than a
substring of the sentence: the version before 2026-08-28 bucketed by
`why.contains("control token")` and discarded the verdict of every page-wide refusal, so it
could attribute one cause of twelve. The `[WARN]` beneath them is the check --- buckets plus
run-refusals must equal the unanswered total, so a region that reached it by a route carrying
no cause is subtracted and named rather than absorbed.

**Two extra axes print under *control not read back*, and only under that one.** It is the sole
cause where the gate got as far as showing the engine something, so it is the only one with a
rendered control to describe. The first bucket is how tall that control landed against
`ocr_gate::MIN_CONTROL_PX`, which is the bound the scale rule exists to clear --- a row below the
floor is the rule missing what it aims at, and on 2026-08-28 that was 34 of 38. The second is how
many characters the token drew, because `ocr::adjudicate` matches by containment and one
recognised span has to hold the whole token. The first prints every bucket including the empty
ones, with a `[WARN]` if they do not sum to the cause's own count.

⚠ **The token axis prints `unread / all` and a rate, and the denominator is not decoration ---
it is what stopped a wrong increment being built.** Read as a numerator alone the bucket says 29
of 33 unread controls drew eight characters or more, which reads as an indictment of
`control_from_page` picking the *longest* qualifying word. With the denominator it says 29 of
**128**: a rate of 22.7% against 33.3% for five-to-seven characters, so long tokens fail *less*
and the obvious repair moves the chooser toward the worse bucket. A count of failures bucketed
by a property is never evidence about that property until the same bucketing is applied to the
population.

**A third axis prints under the same cause: what the engine had actually returned.**
`ocr::Unread` rides on the verdict and carries how many spans came back for the whole probe
image, how many fell in the control band, and how far outside the band the nearest span
*containing the token* sat. Three rows follow --- *read nothing at all*, *read spans, none
holding it*, *read it, outside its band* --- each split by the rendered-height bucket beneath
it. The split is the point: the height rows and the shape rows are two bucketings of one
population, and two marginals bound their overlap without measuring it. Measured 2026-08-28 over
197 refusals at three densities, the outside-the-band row is **0** at every one, and at
`--regions 40` exactly 40 of the 80 silent refusals had a control at or above `MIN_CONTROL_PX`
--- which the marginals alone could only place between 40 and 80.

**A fourth axis, added 2026-08-28: the probe image's own proportions.**
`ocr_gate::geometry_for` reports the shape it planned, and the row prints `unread / all` and a
rate per aspect band, with the silent count beside it. It is a different axis from the control's
rendered height rather than another reading of it, because an aspect is a ratio and the render
scale cancels out of it --- a probe image halved to fit the buffer keeps its shape. Measured over
40 documents at `--regions 12`: **12 / 36 up to 8:1, 28 / 294 between 8:1 and 16:1, 26 / 36 beyond
16:1**, and all 36 silent refusals are in the two tails with none in the middle band that holds
four fifths of the population.

⚠ **That row's denominator has to be counted in the per-region loop, not inside the
`ControlUnread` branch.** Written one scope too low it counts the failures, so every band prints
`N / N 100.0%` --- which happened on 2026-08-28, one increment after the trap about denominators
was written. The `bad / all` form is what makes it visible; a bare percentage would have read as a
finding.

**A sixth and seventh axis, added 2026-08-28: the control's height in points, and which clamp
left it short.** Points is what the aspect turned out to be standing in for, and it is the sharper
single reading: **every control under 2 pt failed**, 24 of 24 and 40 of 40 at the two densities,
against 16.1% for 2 to 6 pt and 7.4% for 6 to 12 pt. That boundary is `MIN_CONTROL_PX /
MAX_SCALE` written as the division rather than as `2.0`, so raising either constant moves the
bucket with it. Crossed with the shape it gives the comparison neither axis could make alone: at a
control of 2 to 6 pt, 517 regions inside 8:1--16:1 are 0% silent and 104 beyond 16:1 are 50%
silent, so the shape matters at a fixed control size.

⚠ **The aspect axis is a description of the corpus, not a lever --- established by building the
lever and measuring it.** Padding every probe image into the 8:1--16:1 band was implemented in
`ocr_gate` (one rule, two callers, six mutations all caught), and at `--regions 40` it moved 120 of
1,469 regions out of the wide band while changing **no verdict at all**: 79 still-readable, 96
unread, 80 silent, 404 provable, identical before and after. At `--regions 12` it left the silent
count at 36 and took *shown unreadable* from 276 to 264. The change was reverted; `docs/PLAN.md`
§6 and the trap entry have why. Read the aspect rows as a property of the population, and do not
rank work off them again.

**An eighth axis, added 2026-08-28: what a higher scale ceiling would do.** For every unread
control the probe computes the scale it would have needed --- `ocr_gate::scale_wanted`, unclamped
--- and whether the probe image fits at it, through `ocr_gate::bytes_at` against the worker's
capacity. Both went public for this; neither is a second copy of anything. Measured: **0 of 24 and
0 of 40 would fit**, worst case asking **31.1x** against a ceiling of 8. So raising `MAX_SCALE`
moves the refusal from *the ceiling could not reach it* to *probe image will not fit* and changes
nothing, which is why it was not written. Rendering the control alone at a generous scale does fit
and is unsound --- a control read in its own kindly rendered image says nothing about the region
strip.

That measurement is what `NotVerifiedCause::ControlTooSmall` came out of: those regions now refuse
with *no scale renders the control legibly* and a message carrying what the page removed, the scale
it would have taken and the ceiling. **No region's outcome changes** --- at `--regions 12`, *control
not read back* goes 66 to 42 with 24 stated, and *shown unreadable* stays at 276 --- and the
evidence that it costs nothing was already printed: the points axis carries its denominator, and
every region with a control under 2 pt went unread.

The clamp row answers a question `ocr_gate.rs` recorded as open --- *"no measurement has separated
them"*. A sub-floor control comes from the `MAX_SCALE` ceiling being unable to reach 16 px, or
from the image being halved to fit the buffer, and the two can hold together, so *both* is its own
row rather than an arm of an ordered chain. Measured: **24 and 40 from the ceiling, 0 from the
halving, 0 short for neither reason.** The `MIN_SCALE` clamp has never fired on real input; re-run
this if `capacity` or the region sampling changes.

**A fifth axis, added 2026-08-28: the two above, crossed.** The height row and the shape row are
marginals of one population, so equal counts on them are not evidence of one set of regions. The
crossing prints `silent / all` per cell, and populated cells only --- an unpopulated cell is not a
zero rate, it is no measurement, and printing it as `0.0%` reads as the former. It answered the
question the marginals could not: at `--regions 12` both rows report a **12**, and the cell
carrying both properties has **no population at all**, so the overlap is 0 and the two tails are
separate defects. `docs/PLAN.md` §6 has the table.

Every row above is guarded, and the guards print nothing when they agree --- deliberately not counted here, because a total in prose has nothing asserting it and the two loops each fire per bucket. The three shapes plus the no-evidence count must equal the
cause's own total; the cross-tabulated total must equal the height-bucket total, since both count
the regions that had a measurable control; and the crossing must reproduce **each** of the two
rows it was derived from, checked per axis rather than over the total. The per-axis split is what
makes a failure readable --- keying the crossing on a constant aspect fires the shape control and
leaves the height control silent, and a constant height does the mirror, so the `[WARN]` names
which axis drifted. A single check over the total goes red for both and names neither. The points
crossing gets the same treatment --- one loop per axis against that axis's own row --- and the clamp
rows have to come to the same total as the height rows, since they partition the same regions. A
non-zero *carried no evidence* is a defect in `ocr::adjudicate` rather than a finding about the
gate: the type says that arm always records one.

⚠ **`--regions N` is not only the sample size; it changes what the gate can do, so every
percentage from this harness has to be quoted with its density.** The regions set `size_pt`
--- the height of the smallest box any of them covers --- and they consume the pool of
surviving words a control may come from, so sampling more of them makes `control_from_page`
harder to satisfy.

⚠ **That also moves which pages reach the shape axis at all, so aspect-band populations are
comparable within a run and not across runs.** A page whose control cannot be chosen contributes
no regions to any denominator here. Between `--regions 12` and `--regions 40`, *no surviving word
is long enough* goes from 60 to 594 and the squarest aspect band empties completely --- which is
the opposite direction from more regions producing a taller image, and is not the capacity rule,
since *probe image will not fit* is 0 in both. Measured over the same 40 documents and the same
three pages each:

| `--regions` | gate regions | shown unreadable | not verified | control not read back | control-selection causes |
|---|---|---|---|---|---|
| 1 | 43 | 69.8% | 25.6% | 10 | 1 |
| 4 | 156 | 67.3% | 26.3% | 33 | 8 |
| 12 | 448 | 56.7% | 38.0% | 64 | 106 |
| 40 | 1,389 | 27.1% | 67.2% | 84 | 850 |

Measured 2026-08-28, after `geometry_for` began choosing the render scale from the control
word's own height rather than from the smallest box a region covered. The four rows before that
change read 65.1 / 64.1 / 55.8 / 26.4 in the third column.

A reader marks a name or a line. **Use `--regions 4` for a figure about the gate and
`--regions 40` for a stress of the control rule**, and say which in the sentence that quotes
it. The *still reads as text* rate is 4.7--6.4% at every density and is the one figure here
that does travel.

⚠ **The `--regions 12` row used to be quoted as reproducing `docs/PLAN.md` §6 to the digit, and
that is the wrong half of the row to make a control out of.** A change to the *gate* is supposed
to move the verdict columns, and this one did. What reproduces exactly across a gate change is
the **left** of the table --- the region counts (43 / 156 / 448 / 1,389) and the
control-selection causes (1 / 8 / 106 / 850), neither of which any scale can touch. Those are
the control over the harness; the verdict columns are the measurement.

### `encrypted-rewrite-probe`: does a rewrite keep an encrypted document's encryption

`docs/PLAN.md` §5 said for months that letting a reader delete a page from an encrypted
document needed QPDF. It needed `lopdf::Document::encrypt`, which `save.rs`'s `rewrite` now
calls with the state `checked` took off the document after a password load.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example encrypted-rewrite-probe
```

Seven checks over the two encrypted fixtures and the locked case; about a second. **The
verdict comes from `qpdf`, not from `lopdf`** --- a reload with the writer's own reader is the
writer agreeing with itself, and here that is worse than usual, because a `lopdf` load
*without* the password parses no objects at all and reports zero pages. The spike this grew
from round-tripped an empty document and printed `[OK]` three times before that was caught, so
every page count here is read back with the password and the encryption comparison is
`qpdf --show-encryption` on the source against the output.

Without `qpdf` installed the three encryption checks `[SKIP]` with that reason and the page
counts still run. The last check is the control and is the one to read first: a rewrite that
dropped the encryption passes both of the others, so the probe scans the written bytes for
`/Encrypt`.

### `redact-gate-probe`: does the redaction gate certify a clean file and refuse a dirty one

`docs/PLAN.md` §6 step 4 is wired into `redact_copy` and `redact_document`, and neither is
reachable from a unit test --- they are Tauri commands, and the join between one and
`ocr_gate::run` is the layer `docs/TRAPS.md` records as *a feature can be inert in the
application while three layers of tests pass*. This drives the real function against a real
render service, a real render worker, a real OCR worker and a real engine. The binary is both
workers, the way `ocr-worker-probe` and `pool-bench` are.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-gate-probe -- \
    testdata/text-base14.pdf
```

**No `--lib`, and not for brevity**: this runs on both platforms, so the default joins
`PDFIUM_SUBDIR` --- `bin` on Windows, where `lib` exists, holds the *import* library and binds
to nothing.

**It runs on both CI legs since 2026-08-29, and it had to.** Until then it was a thing a human
ran by hand on a Mac --- and it is the only instrument on either platform that drives the OCR
gate end to end through a worker process. The `child_main_if_asked` dispatch it needs was
widened to Windows in `lib.rs` alone, so on Windows this probe scored **5/8**: the child found
no marker, fell through into the *parent's* argument parser and exited, and every region came
back `the engine crashed`. Nothing in that sentence is about the engine. The step uses the
debug artifact the `bins` gate already built --- **33.6 s on macOS against 1 s for the release
build**, which is the debug PDFium render and is the price of not building twice.

It is deliberately **not** a `scripts/gates.py` gate: the gate list has to pass on a fresh
checkout, where `testdata/` is empty because the fixtures are generated and gitignored. This
needs a real document, so it belongs after the step that writes one.

| fixture | result |
|---|---|
| `columns`, `text-base14`, `text-marked`, `rotated`, `links`, `text-cid`, `outline-simple` | 8/8 |
| `encodings` | 0/0, 1 skipped --- one text object is every word on the page, so no control survives |

**`columns` ran 0/0 until 2026-08-27 and it is the fixture that matters most.** Its longest
word is `alpha`, five characters, and the target filter was six --- so the one corpus that
puts a *second* text object on the region's own rows was the one this skipped. Every other
fixture draws a line as a single text object, so redacting a word in it takes the whole line
and there is no neighbour left to misread. Lowering the floor to five moves no other corpus,
because the choice is the longest word on the page.

Removing the `ocr_gate::mask_columns` call turns `columns.pdf` red on two checks --- *the
redacted file is certified* and *a word beside the region on its own rows is not reported*
--- and no other corpus on any. That is the control for the mask, and it is the only fixture
where the right rule and the wrong rule disagree.

**The control is the same gate run against the file that was not redacted.** A gate that
certifies everything passes *the redacted file has no reasons* perfectly, so that row on its
own is worth nothing; the source file, with the same regions and the same words, has to come
back **legible** and has to quote the word that is still there. One variable between the two
runs --- which file --- and it is the one under test.

The other three rows: a page the gate knows no words for must be *not verified* rather than
clean, since a page nothing was read on is also a page nothing survived on; the region's own
pixels must differ either side of the write; and on a platform with no engine the gate must
say so **once**, not once per region.

**That pixel row was a byte scan first and it was the wrong instrument.** `verify::scan` for
the removed words goes red on `text-marked.pdf`, where the same line appears four times and one
copy is an annotation the removal is right to keep --- see the trap of that name. A gate about
a region is checked with an instrument about a region.

**Costs, measured on this machine at scale 2.** The gate renders strips rather than pages, and
these are why:

| what | cost |
|---|---|
| `OcrWorker::spawn`, once per save | 1.5 ms |
| one 1190 x 128 probe image through Vision | ~9 ms |
| a whole A4 page through Vision, for comparison | 195 ms |
| a whole page render, warm | 13--48 ms |
| the probe's end-to-end gate run, one region, open included | 200--260 ms |

### `latency-bench`: what one tile costs, decomposed

The last thing `worker-bench` measured that nothing else did. It is a **spike, not a port**:
`worker-bench` carries its own POSIX worker, `dup2` handover, socket pair and SBPL bisection and
cannot run off unix, so this drives the **production** `Worker` instead --- which means it runs on
both platforms. **Measured on Windows 2026-07-30 and on macOS 2026-07-31**, and the point of it
being portable is that macOS can cross-check it against `worker-bench --mode latency`, an
implementation it shares no worker code with. That cross-check has now run, and it is the most
useful thing this harness has produced --- see below.

```
cargo build --release --example latency-bench
./src-tauri/target/release/examples/latency-bench.exe testdata/text-base14.pdf
./src-tauri/target/release/examples/latency-bench.exe testdata/vector-heavy.pdf
```

Four variants, interleaved within each round, round 0 discarded as warm-up and printed anyway:
`inproc` (no boundary), `raw` (`Tile { png: false }`), `png` (`Tile { png: true }`), and
`control` (`Outline`, a round trip carrying no tile). Each row decomposes into render, encode,
parent fold and transport; every pixel-bearing variant folds its whole payload in the parent so
none can look cheap by never reading what it received. It ends with a `N/M checks passed`
summary and exits non-zero on a failure, so a scripted run can see one; a `[WARN]` does not fail
the run, because every warning here says a *derived* figure is untrustworthy rather than that the
measurement broke.

**There is no `pipe` row, and that is a finding.** `worker-bench` compares pixels down the pipe
against pixels through shared memory. Production never does the first --- `Response` documents
that payloads travel through the mapping and never inline --- so a pipe row would measure a route
no tile takes. The same quantity is recovered by differencing `raw` against `png`, two paths that
are both real.

Measured 2026-07-31, Windows, 1024² tile at scale 1:

| fixture | boundary cost | spread over rounds | round trip, no tile | per 100 KB moved |
|---|---|---|---|---|
| `text-base14.pdf` | 0.269 ms | 0.004 ms | 0.040 ms | 0.0055 ms |
| `outline-simple.pdf` | 0.309 ms | 0.016 ms | 0.070 ms | 0.0069 ms |
| `vector-heavy.pdf` | 0.294 ms | 0.150 ms | 0.052 ms | `[SKIP]` --- see below |

And on macOS, 2026-07-31, same tile and scale, three interleaved passes per fixture rather than
one (the fixtures were run round-robin, not in blocks, because wall clock on these Macs drifts
several percent over minutes):

| fixture | boundary cost, 3 passes | within-run spread | round trip, no tile | inproc residual |
|---|---|---|---|---|
| `text-base14.pdf` | 0.103 / 0.079 / 0.071 ms | 0.048 / 0.024 / 0.002 ms | 0.012 ms | 0.001 ms |
| `outline-simple.pdf` | 0.100 / 0.085 / 0.079 ms | 0.003 / 0.007 / 0.010 ms | 0.022 ms | 0.001 ms |
| `vector-heavy.pdf` | 0.150 / 0.200 / 0.194 ms | 0.125 / 0.145 / 0.142 ms | 0.019 ms | 0.002 ms |

Expected shape reproduced exactly: 3/3, 3/3, and 3/4 with 1 skipped on `vector-heavy`, exit 0
throughout, and its `[SKIP]` for payload differencing appears for the documented reason --- png
4027 KB against raw's 4096 KB, so the two variants move nearly the same bytes. That is a property
of the document and it held on both platforms.

**Read the invariance, not the numbers.** The boundary cost is a property of the boundary, so it
should not depend on the document, and across three fixtures that differ by three orders of
magnitude in render time it lands within 0.02 ms of itself. That agreement is the result; any one
of those figures alone would be a single sample.

**The invariance is looser on macOS, and the looseness is confined to one fixture.** The two
light fixtures agree tightly across six runs --- 0.071 to 0.103 ms, overlapping completely --- while
`vector-heavy` sits clear of both at 0.150 to 0.200 ms. Absolute spread across fixtures is
0.137 ms here against 0.040 ms on Windows. Before reading that as a defect, note that
`vector-heavy`'s *own* within-run spread is 0.125--0.145 ms, i.e. as large as its offset from the
others: it is the one fixture where the estimator is near the edge of what it can resolve, which
is exactly what the spread column exists to say. The check is `spread < boundary`, and there it
passes at 0.73--0.83 of its limit against Windows' 0.51 --- so a macOS run of `vector-heavy` is
the plausible place for this to go red first. Three passes did not. Worth knowing rather than
worth acting on.

The absolutes are **~3.5x lower than Windows**, not the 1.5--1.8x the other render constants
differ by. A latency budget written from the Windows figures is conservative on a Mac by more
than the usual factor.

**The cross-check against `worker-bench --mode latency`.** Both harnesses were run on this
machine in one session, and both figures below use the *same* estimator --- the tile variant's
transport column minus `inproc`'s, `inproc` being the variant that renders but crosses nothing:

| | `worker-bench` (private POSIX worker) | `latency-bench` (production `Worker`) |
|---|---|---|
| `text-base14` | 0.006, 0.008 ms | 0.071--0.103 ms |
| `outline-simple` | 0.008 ms | 0.079--0.100 ms |
| `vector-heavy` | **-0.087 ms** | 0.150--0.200 ms |
| in-process residual | 0.013--0.037 ms, and **46.7 ms** on `vector-heavy` | 0.001--0.002 ms |

Two conclusions, and the second is why the cross-check was worth doing:

- **The production worker's per-tile boundary cost is roughly 10x the prototype's** --- ~0.08 ms
  against ~0.007 ms, non-overlapping across nine runs. Both are far below anything that matters
  (the same tile costs 3.0 ms to hand to the webview), so nothing architectural moves, but the
  production protocol is not free the way the spike suggested.
- **`worker-bench`'s latency mode cannot resolve its own answer**, and now says so. Its
  `transport` is a residual and it baselines on `ping`, which never renders, so the render-noise
  floor stays in the figure; on `vector-heavy` the residual is 46.7 ms against a printed 46.6 ms
  and the `inproc`-baselined value goes *negative*. It now prints the residual and the
  `inproc`-baselined figure beside the two `ping`-baselined ones and warns when the error is as
  large as the answer --- which is on **every fixture measured so far**. Read its two headline
  transport figures as upper bounds. Trap: *"A baseline that skips the expensive step leaves its
  noise in the answer"*.

**No sandbox font substitution on macOS.** The handover flagged this as the second reason a Mac
run was worth more than a re-run, since a sandboxed PDFium has previously substituted fonts
silently while still returning `ok`. It did not happen: `inproc` and the worker agree on render
time to within 0.25% on all three fixtures (0.130 vs 0.133 ms, 0.523 vs 0.507 ms, 1670.5 vs
1666.3 ms).

Its four mutations were re-proved here rather than taken on trust --- **4/4 caught**, control
green on all three fixtures first, file restored by bytes and verified by digest against `HEAD`.
Two are caught as `[WARN]` rather than `[FAIL]`, which is by design and worth knowing before
writing a harness around it: a parser that treats `passed = total - skipped` as the failure count
reports those two as broken runs. `passed = checks - failures - skipped - warnings`.

Two things the A0 fixture forced, both of which the small ones hid, and both now traps:

- **The boundary figure is differenced on the transport column, not on end-to-end.** The obvious
  estimator subtracts two ~2.7 s numbers to recover a ~0.3 ms one and reports render noise: it
  read **-265.822 ms** there. The run reports how far the same render varies between variants
  beside the figure, which is the error that estimator would have carried --- on the A0 sheet a
  factor of several hundred.
- **Payload differencing is guarded by materiality, not by ordering.** A dense vector page barely
  compresses --- png 4027 KB against raw 4096 --- so `raw > png` passes on a 68 KB gap and divides
  noise by it. That fixture now reports `[SKIP]` naming both sizes.

The `control` variant subtracts the outline walk the reply reports in `walk_ms`, rather than
warning that the walk is inside the number. Whether that subtraction is sound is cross-checked
two ways --- the entry count and the walk time must agree about whether any work happened --- and
both disagreement branches were shown to fire under mutation. They exist because the first
version trusted the count alone, misparsed an object as an array, and printed *"the document has
no outline"* for `outline-simple.pdf`.

**The boundary check is on reproducibility, not on sign, and the difference was forced by a
mutation that survived.** The first version simply required the figure to be positive --- a
boundary cannot be free --- and restoring the wall-based estimator on the A0 fixture *passed* it,
because -265.822 ms had been one sample of a noisy quantity and the next run of the same broken
arithmetic landed positive. A check that fires only when noise falls one way is decoration on
every run where it does not. It now requires the figure to be positive **and** to repeat across
rounds, which the two estimators differ in enormously: 0.004--0.150 ms of spread on the sound
one, 48 ms on the broken one.

That fix exposed a second defect worth more than the first. The check compared a spread against
a figure that was computed by a *different route*, so the mutation moved the figure and left the
spread sound, and the comparison passed on an estimator that had been broken on purpose. Both
now come from one per-round vector. **Two derivations of one quantity have to be tied together
or their agreement means nothing** --- which is the same lesson as the outline count above,
arriving from the other direction.

**Its checks are proved, not assumed.** Four mutations, four caught, each with the predicted
verdict, restored by bytes and verified by digest: forcing the outline count to zero, forcing it
non-zero, dropping the fold term from the transport formula, and estimating the boundary on
end-to-end again.

### Printing on Windows, and how to check it without paper

`print_win.rs` reads a job back with `Windows.Data.Pdf` --- the OS's own PDF stack, the PDFKit
counterpart --- then rasterises each page onto a printer DC. Windows has no in-box PDF print
API, so rasterising is what every Windows PDF viewer does; the output is **raster at 300 dpi**
where macOS is vector.

`present` opens a modal dialog, so that last step is the one thing no automatic check can
reach --- and since 2026-08-23 there is something behind it worth stating rather than leaving
implied. The panel's **Pages** field was disabled until then (`nMinPage == nMaxPage`, both zero
by default) and now offers a range that `print::sheets` and `spool` honour. The arithmetic is
tested on every platform and the probe proves the spooler sends the named sheets; **whether the
field is enabled, and whether `PD_PAGENUMS` and `nFromPage`/`nToPage` come back holding what the
reader typed, is untested by anything here and cannot be.** The first person to print a range on
Windows is the instrument for that half. `nCopies` is a known second gap of the same shape: it
goes in as 1 and is never read back. Everything before it can be, because **"Microsoft Print to PDF" is a real driver with a
real spooler**, and naming an output file in `DOCINFOW.lpszOutput` stops it raising a save
dialog:

```
cargo run --release --example print-probe
cargo run --release --example print-probe -- testdata/rotated.pdf
cargo run --release --example print-probe -- testdata/vector-multi.pdf "Microsoft Print to PDF"
```

Ten checks, and four of them are the ones worth understanding:

- **Ink, not the page count.** A wrong `BITMAPINFO`, a DC in the wrong mapping mode and a bad
  `StretchDIBits` rectangle all produce the right number of perfectly blank sheets. Proved
  rather than assumed: mutating the blit away leaves *"the printed output has the pages that
  were sent"* green and only the ink red, at `[0, 0]` --- and the output file drops from 598,694
  bytes to 1,183. The pages that were *sent* are the control, because "both zero" would
  otherwise pass on a completely broken path.
- **Ink extent against a predicted geometry, not an ink ratio.** This is the check that found a
  real defect, and the history is the useful part. It began as printed-ink-over-sent-ink with an
  order-of-magnitude band, which read `0.49` and **passed** while every page was going to paper
  at half physical size (a DIB rendered at 300 dpi placed unit-for-unit onto a 600 dpi printer
  DC). The same formula then failed at `0.01` on an A0 page for no reason but the paper being
  16× smaller in area. What holds for both is predicting where the ink should land --- the source
  page's ink extent scaled by the page-to-sheet ratio --- which reports 1% error on the reference
  run and **48%** against the reverted bug. See the trap of the same name.
- **Its own module table**, which is where the boundary claim gets its honest caveat: 80 modules
  mapped, none named pdfium, and `Windows.Data.Pdf.dll` printed beside it as what *is* mapped.
  Printing parses in the app process on both platforms; what the boundary buys is that the
  parser doing it is not ours.
- **A page range spools the sheet it names, not the first one.** Added 2026-08-23 with the range
  itself. A count of one sheet is equally satisfied by the *first* page, which is exactly what a
  loop ignoring its range produces, so the second check compares the printed ink's extent
  against the prediction for the sheet that was asked for. It asks for the **last** sheet for
  the same reason. On a fixture whose first and last sheets measure alike it prints a `[SKIP]`
  saying so, rather than passing on a comparison that cannot fail; `rotated.pdf` is a fixture
  where they differ.

Reference run, `testdata/rotated.pdf`, pages 1--2 with a quarter turn:

```
  pages    : 4 in the source, printing [1, 2] with a quarter turn

[OK]   the job we built is readable by the OS parser        2 pages
[OK]   control: the pages we sent have ink on them          non-white pixels per page: [5012, 5012]
[OK]   the spooler accepted every page                      2 pages spooled
[OK]   the printer produced a file                          598694 bytes
[OK]   the printed output has the pages that were sent      2 pages
[OK]   every printed page has ink ...                       non-white pixels per page: [4777, 6850]
[OK]   printed ink lands where the page geometry says ...   got 0.27x0.28, 0.45x0.27,
                                                            predicted 0.27x0.27, 0.45x0.27
                                                            (worst axis off by 1%, 0%)
       ... printed/sent ink, for information                [0.953, 1.367]
[OK]   the printing process never mapped our PDF parser     80 modules mapped, none named pdfium
       ... the OS PDF component it maps instead             Windows.Data.Pdf.dll
```

`outline-hostile.pdf` is the second shape worth running --- A4-sized pages rather than small ones,
so the predicted extent is 0.72x0.47 instead of 0.27x0.27 and a wrong *fit* would show where a
wrong *scale* does not. Also 8/8.

**Do not run it on `vector-heavy.pdf` or `vector-multi.pdf` casually.** One A0 page of 200,000
vector operations takes **2m51s** end to end, essentially all of it inside
`Windows.Data.Pdf`'s rasteriser and largely independent of resolution. That is a real property of
a raster print path and not a defect --- see the trap *"the OS's PDF rasteriser is not fast"* ---
but it makes those fixtures unsuitable for a quick check.

Beyond the probe, `cargo test --lib print::` runs **18** checks on Windows where it ran 14, because
three of the four third-parser checks are no longer macOS-only. The fourth asserts the page count
and prints a `[SKIP]`: it needs per-page *text* to say which pages survived, and
`Windows.Data.Pdf` has no text API at all. `a_third_parser_checks_a_job_built_from_a_document_we_-`
`did_not_write` covers that property on both platforms instead, using per-page rotation ---
`rotated.pdf` carries 0/90/180/270, so keeping the wrong two pages is a different rotation pair.

### The "reopen its windows" dialog

A development build is killed constantly --- harness timeouts, the deliberate crash probes,
an aborted panic --- and macOS answers each abnormal exit by offering, on the *next* launch,
*"the last time you opened tpdf, it unexpectedly quit while reopening windows. Do you want
to try to reopen its windows again?"* That dialog **blocks the launch** until someone clicks
it, in front of a run that has nothing to do with whatever produced it.

`src-tauri/Info.plist` sets `NSQuitAlwaysKeepsWindows` to false, which is merged into the
bundle by tauri-bundler --- check it with
`plutil -p .../tpdf.app/Contents/Info.plist | grep Quit` after a build rather than assuming
the merge happened. An app that saves no window state cannot be asked to restore it, and
the observable is the mechanism rather than the symptom: hard-kill a running bundle and
`~/Library/Saved Application State/com.timostein.tpdf.savedState` must not appear.

This is also the right *product* behaviour, not only a developer convenience. tpdf reopens
the document you were reading, on the page you were on, through its own session file
(`session.rs`); Cocoa's restoration would be a second mechanism doing the same job, and two
mechanisms agree until they do not.

An existing machine that has already been prompted also wants the user-domain switch, since
the plist only governs bundles built after it:

```
defaults write com.timostein.tpdf ApplePersistenceIgnoreState -bool true
defaults write com.timostein.tpdf NSQuitAlwaysKeepsWindows -bool false
rm -rf ~/Library/"Saved Application State"/com.timostein.tpdf.savedState
```

### Checking the viewer

The reading surface is asserted rather than eyeballed. This opens a document in a real
webview, dispatches real wheel and key events at it, and checks fit-width, fit-page, actual
size, scrolling, End and Home, the zoom ladder, a pinch, resize, text selection and copy,
find-in-document, the
command palette, the screen-reader text layer, the outline sidebar, the page-thumbnail
strip, page inversion, and that the frame loop idles when there is nothing to do:

```
scripts/viewer_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

It is **not** a `gates.py` gate: it needs a built bundle and a generated fixture, neither of
which a gate run has. Run it before a release, and after any change to `viewer.ts`,
`scroller.ts` or the tile protocol.

**One corpus while you are working; the sweep before a push.** The rule above names *files*,
and a file is the wrong unit: `viewer.ts` is 4,400 lines and most changes to it cannot vary
by document. What `viewer_sweep.py` buys over a single run is the name-set invariant across
fourteen corpora, so the question to ask is whether the change could make a check appear,
vanish or skip on *some* documents --- layout, rotation, text extraction, the tile protocol,
anything reading a page's size. A change whose checks drive the DOM and the callbacks
directly cannot, and the sweep is then fourteen runs of the same answer. This is the
portfolio rule about running the owning gate while iterating and the whole suite once, at the
push, applied to the slowest instrument here.

**It requires a bundle, not merely a release build.** A raw `cargo build` binary opens a
window and never executes a line of JavaScript --- WKWebView needs the bundle identity, and
the failure is silent: no error, no crash report, a blank window. Build one with
`npm run tauri build -- --bundles app` and run the executable inside it, which keeps stdout
and the environment that `open -a` does not. The *profile* genuinely does not matter --- the
check asserts behaviour rather than timing it --- so a debug bundle is only slower.

**On a zero exit it now asks whether the run happened**, which it could not until 2026-08-26.
Every guard in it was aimed at a run that failed, and a run that did *nothing* exits 0: on
Windows, single instance makes a second launch forward its argv to a window already open and
exit immediately, so an empty transcript came back as a pass, with the wrapper's own
containment `[OK]` as the only check-shaped line in it. A blank-window bundle failure produces
the same silence, so the paragraph above is no longer describing a failure with no report. The
observable is the `CHECK-NAMES-JSON` roll `checkreport.ts` prints before its summary, and the
refusal names no single cause --- it prints the exit code, the byte count and the number of
`[FAIL]` lines, and lists the four things that look identical from here. Both callers already
guarded themselves (`viewer_sweep.py` on the same roll, `mutate_viewer.py` on the summary),
which is why the layer they share had nothing; neither is made dead by this, since the sweep
looks for the roll before it looks at the exit code.

That guard is a pure function of the transcript, so it can be proved without a screen, a
bundle or a document --- six cases, one of them the acceptance, since a reader that refuses
everything passes all five refusals:

```
scripts/viewer_check.py --self-test
```

It also requires an unlocked screen, for the reason `scroll_bench.py` does: WebKit suspends
a page whose window is not visible, so behind a lock screen the check does not fail, it
stops. Both scripts share that guard (`scripts/webview_guard.py`).

**On a timeout it says which silence it hit**, which it could not until 2026-08-01. A page
WebKit has suspended and a page stuck in a loop both present as no output and a live process,
and they want opposite responses --- one is an occluded window or a screen that locked
*mid-run*, the other a defect in whatever was last changed. `webview_guard.diagnose_silence`
samples the app's CPU **time** twice, two seconds apart, before the kill: suspended uses none,
waiting uses a little, spinning uses a core. The delta is load-bearing --- a single
`ps -o %cpu` is a lifetime average on macOS, so a page that worked hard and then got suspended
reads as busy. `mutate_viewer.py` gets the line for free, since it already forwards
`[FAIL]` stderr lines into its own broken-run verdict.

`session_check.py` and `open_check.py` do **not** have it: they use `subprocess.run`, so the
process is already dead when the timeout is raised, and giving them the diagnosis means
converting four call sites to `Popen`. Worth knowing rather than assuming it is everywhere.

**It does not take focus.** The window appears and has to stay visible, but it will not raise
itself over what you are doing, so the run can sit in the background while you work.
`scroll_bench.py` is the exception and calls `set_focus()` on purpose --- an unfocused window
is throttled, and a frame-rate benchmark would then be measuring the throttle.

**"Visible" is stricter than "unlocked", and the guard does not check it.** A window fully
covered by another --- a full-screen terminal, a different Space --- is *occluded*, and
WebKit suspends the page exactly as it does behind a lock screen. The run then produces no
output, uses no CPU, and stays alive, which reads as a hang in whatever was last changed.
`viewer_sweep.py` and `mutate_viewer.py` take **`--raise`**, and it is **off by default**
as of 2026-08-20. They used to force `TPDF_RAISE=1` on every launch, which on a fourteen-
corpus sweep takes the keyboard away fourteen times in a row --- reported from the machine
as *"these tests are locking up this mac, as every window opens in foreground"*, and it
was never what the checks need. `lib.rs` says so at the call site and keeps a polite
default for exactly this reason: the checks drive behaviour rather than time it, so an
unfocused window costs them nothing. What costs them everything is an **occluded** window,
which is a different property --- and one `webview_guard.py` already detects and names the
remedy for. Use `--raise` when a run produces nothing, not before.

Set `TPDF_RAISE=1` to raise the window when there is nowhere visible to put one:

```
TPDF_RAISE=1 scripts/viewer_check.py <binary> testdata/text-heavy.pdf
```

**An empty transcript file mid-run means nothing, and reading it as a stall is a mistake this
page invited.** `viewer_check.py` collects the app's output with `communicate()` and prints the
whole transcript when the process exits, so a redirected run shows **zero bytes** from the
first second to the last --- on `vector-multi` that is minutes. The "results print as they are
produced" sentence below is about `viewercheck.ts` writing into the pipe, which is what makes a
*timeout's* partial transcript useful; it is not a promise that a live run's log grows. The
liveness signal is CPU time (`ps -o time= -p <pid>`), which is what `diagnose_silence` samples:
a page that never ran accumulates none, and a slow render accumulates seconds.

The watchdog identifies **any** page that never executed, whatever the reason --- an
occluded window and a raw unbundled binary produce exactly the same silence. Every spike
entry point starts by asking Rust for its path, which records a `webview alive` mark; a run
that times out without one is told in full that the page never ran a line of JavaScript.
Confirm independently with `TPDF_STARTUP=<file> <binary>`, which fails the same way in 30 s
and settles "environmental or mine" in one command. Results otherwise print as they are
produced, so a run that stops partway names the last check it completed.

**That was false under a redirect until 2026-07-30, which is how these are always run.** Python
block-buffers stdout the moment it is not a tty, so `open_check.py > out.txt` held **zero bytes**
for a twelve-minute run --- indistinguishable from a script that died at import, and exactly the
ambiguity printing-as-you-go exists to remove. `scripts/live_output.py` makes the three harnesses
line-buffered explicitly; A/B'd at the same four-second mark, 0 bytes against 38. Prefer that over
`python -u`, which is a property of the invocation that every future caller has to remember.

Two of its assertions carry the weight, and both tie a position to specific content rather
than checking that something happened. For **selection**, text dragged near the top of the
page must come from earlier in the page's text than text dragged further down --- a substring
check was tried first and cannot fail, since a selection is a contiguous range of indices
whatever the boxes claim. For **search**, a match's index range must cover the characters
searched for, re-extracted independently; every other search assertion passes just as well
when the indices are off by one.

Run them with **`scripts/viewer_sweep.py <app-exe>`**, which is the list of corpora as well as
the way to run them:

```bash
scripts/viewer_sweep.py --list          # every window corpus, and every fixture excluded, with reasons
scripts/viewer_sweep.py src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
```

> **Not yet re-measured, and the shortfall has grown.** The properties dialog added five
> names to every corpus on 2026-08-21, and the comments panel's covered-words face added a
> sixth later the same day --- so the totals in the table are **six** short of what the next
> run will print, and the invariant the sweep asserts is the *agreement* between corpora
> rather than any particular number. Written down rather than left to be noticed, because the
> table looks measured either way and a stale total reads exactly like a current one.
>
> The current figure, measured 2026-08-23: **342 names**, all distinct --- 324 cutting
> `26.8.7`, plus five for the overlay-against-the-file phase, five for the stamp (its own
> overlay reading and its four commands), six for cropping by dragging (three backend
> checks, its one palette command, and two reading the scrim off the overlay), and two for
> the eraser taking a mark whole (the wash the nib crossed, and the wash beside it that is
> the control). Take the names from the harness's own
> `CHECK-NAMES-JSON` line, never by splitting the printed columns --- this page records that a
> `\s{2,}` split matched 175 of 189 lines, and reaching for it again is what produced a diff
> full of per-corpus *detail* differences that looked like missing names.

#### The overlay against the file, and the one thing it needs from outside

Five of those names are a phase comparing what the overlay draws with what the *saved file*
renders --- the one comparison nothing made, since `viewer_check.py` measured the overlay
against the model's numbers and `annot-probe` measured the file against the same numbers. It
makes nine marks, reads the overlay, saves a copy, opens it, renders the same page and reads
that; the file's ink is isolated by diffing that render against one taken before any mark was
made, so page content cancels and the classifier knows nothing about the colour it is about to
compare. `docs/PLAN.md` has the design and what it measured.

**It needs a writable path, which the webview has none of.** `viewer_check.py` makes one under
the system temp directory, binds it to `TPDF_VIEWERCHECK_SCRATCH`, and removes it at exit; the
`viewercheck_scratch` command hands it to the page. A run that gets nothing there skips all
five with that reason rather than passing. Running the harness by hand without the script is
therefore a run with those five skipped --- which is correct, and worth knowing before reading
a hand-run transcript as a full one.

**The comment is excluded from the colour comparison**, with a measured reason: PDFium draws
its own `/Text` icon and ignores the `/C` we write, so blue reads 224 degrees on screen and 60
in the file, and red reads 0 and 60. See the trap of that name --- the file is right and the
renderer is not ours.

Every run reports the same check names; what differs is how many are `[SKIP]` with a reason,
and a name that goes missing rather than skipping is the bug this arrangement exists to catch.
**The script asserts that**, as a set difference across the corpora, rather than leaving it to
whoever compares two totals --- a check that stopped being printed and a check that started
skipping are the same number. It also prints the table below, so those numbers are measured
rather than transcribed.

> **The list is a gate (`corpora`) because it went wrong the moment it had no home.** On
> 2026-08-16 it lived in a hand-typed shell loop and `links-rotated.pdf` went into a sweep,
> producing eight red checks and three chased diagnoses, none of them a defect --- against the
> paragraph on this page that already says that fixture is separate *because* it reddens two of
> these rotation checks. Every `testdata/*.pdf` is now either a window corpus with a stated
> purpose or excluded with a stated reason, and a fixture matching neither fails the gate.

**The sweep runs on Windows as of 2026-08-19, and did not before.** Two things had to be
fixed, and both are recorded in `docs/TRAPS.md`. It shelled out to `pkill` unconditionally,
which is not a program here: `check=False` swallows a non-zero exit and not a
`FileNotFoundError`, so it died on its first corpus with a traceback and **exit 0**. And
`subprocess.run(text=True)` decodes with the locale codec, which is cp1252 on this machine
— six corpora in, `multilingual.pdf` produced a byte it refuses, the decode raised inside
subprocess's own reader thread, and the failure arrived as a `TypeError` between a `None`
and a string with no mention of an encoding.

Two further things are worth knowing before running it here. A stray `tauri dev` makes
`tauri-plugin-single-instance` forward the launch and exit, so the check reports one line
about the module scan and nothing else — kill leftovers first, which the sweep now does.
And `text-heavy.pdf` is a real document rather than a generated fixture, so a machine that
does not have it cannot run the full sweep at all; `--only` over the other thirteen is the
honest substitute, and the sweep says which corpora it is missing rather than skipping them
quietly.

**`vector-multi` is timing-variable on this machine, and a single red run there is not a
finding.** Measured 2026-08-19 across four runs of the same corpus: **351 s, 496 s, 386 s**
on one build and **384 s** on another --- a 41% spread on identical code. Two of those runs
failed, and they failed **different checks** (`the page already rendered is not rendered
twice`, reporting 11 borrows against 4 draws; and `covers the first screen`, timing out at
sharp=0.0%). Both are checks whose observable is a race between the thumbnail strip and the
viewer, on the only fixture where a thumbnail is slow enough for that race to be real ---
which is precisely what the corpus is *for*.

So: **re-run it before treating a red vector-multi as a regression, and check whether the
same check fails twice.** Two different checks failing is variance; one check failing
repeatedly is a defect. The control that settled it was a `git worktree` at `HEAD` built and
run against the same fixture --- cheap, and it leaves the working tree untouched, which
`git stash` does not.

Measured here on 2026-08-19: thirteen corpora, **276 check names each** (277 once
`app.about` became a driven probe), diffed as sets by the sweep rather than inferred from
the totals agreeing, and **no failing check on any of them**. 629 s in total, of which `vector-multi` is 341 s and `vector-heavy` 145 s; every
other corpus is under 40 s. The ran/skipped splits are the sweep's own output and differ
from the macOS table above only by the checks added since it was taken.

**Every row below was measured on macOS on 2026-08-17**, and printed by the script rather than
transcribed --- the table is the sweep's own output, pasted. Zero failures
anywhere, and **all fourteen corpora report the same 234 check names** --- diffed as sets by
the sweep, not inferred from the totals agreeing.

The link work took the total from 171 to 189, turning a page in the document took it to 204,
and deleting one took it to 218: ten in the viewer, three against the backend, and one command
probe. Of the ten, the one that carries the weight is about **identity** --- the slot below the
gap must now hold the page that was under it, compared by its text --- because a page count one
lower is equally true of a viewer that dropped the wrong page. On a corpus whose pages read
alike that check says so and skips rather than passing on a comparison that cannot fail.

**Moving one took it to 229**: nine checks and two command probes. The nine are where the
deletion ten do not transfer, and the reason is worth stating because it is what made the
frontend defect this phase found invisible. **Every deletion check is built on the page count,
and a move does not change it.** So the length is asserted to be *exactly what it was*, and
every statement that can fail is about identity: the moved page's text is in the slot it was
moved to, the page displaced by it is one slot lower rather than gone, the reader is still
looking at the page they were reading, and both pages keep the sizes they were measured at ---
that last one on `mixed.pdf`, the only corpus where two pages have different sizes, so on every
other one it says so and skips.

**That size check asserted one slot until the mutation aimed at it survived**, which is what
the harness is for. The defect it was written against is a scroller re-indexing its learned
sizes by position, and one comparison catches that. The other way to lose a size is to lose
them all, and then every page falls back to one estimate --- free to land within tolerance of
whichever single shape is being compared, which on this corpus it did. Asserting both slots
makes it arithmetic instead: a shared estimate would have to be within 0.02 of two shapes the
check's own precondition has just established are further apart than that. The same mutation
reddened a *deletion* check the whole time, which reads absolute boxes rather than shapes ---
so the coverage existed and the check named for it was the one that could not fail.

> **The table above is one sweep of the tree as committed**, re-run after the last change to
> any check rather than carried over from before it. That is not free --- this one was **728 s**,
> of which `vector-multi` alone is 349 and `vector-heavy` 205, so two of the fourteen are
> three quarters of the run; `scripts/viewer_sweep.py` prints that breakdown at the end so the
> question does not have to be answered by hand again --- and it is worth it here for a reason this increment
> demonstrated: the run before it went red on four of the fourteen corpora, against ten that
> passed for no better reason than having strips too short to scroll. A table pasted from a
> sweep of a different tree would have been a claim about neither.

**Dragging one took it to 231**, and the two names are the narrowest pair that were worth
paying a window for. The slot arithmetic is two pure functions with unit tests, the gesture's
state machine has nine mutations against a fake DOM, and the edit a drop runs is already
covered by the three backend names below. What none of those can answer is whether a real
WKWebView captures the pointer, keeps delivering moves after it has left the row, and lays out
geometry the gap arithmetic can read --- so the strip's handler here *records* rather than
edits, and the document is never touched. The control is the half that found something: its
first version could not fail either way it was read, and the mutation aimed at it survived
until both clauses were replaced. See the trap of that name.

**Extract took it to 232**, and one name is all it is worth: `file.extractPages runs from the
palette`, driven with a real argument. The arithmetic is 30 unit tests over `parsePageRange`
and `namePages`, the subset plan is ten Rust tests, and the command is four more --- none of
which can say whether a typed value survives the palette's own input and arrives as the slots
the action is handed, which is the only thing the window is asked. The argument is `1-2`
rather than `1` on purpose: a single slot reads the same whether the parser produced one page
or dropped one, and every window corpus has at least two pages.

**The right-click menu took it to 234**, and both names are about the same gesture because the
report that asked for them was: right-clicking a page offered the web view's own menu, whose
one entry reloads the frontend. So one name asserts that menu is *suppressed* --- read off
`defaultPrevented` on a real `contextmenu` event rather than inferred from a screenshot --- and
the other that the slot under the pointer is the one reported. The menu's own behaviour is 18
unit tests over the real class in a fake DOM; what only a window can say is whether a
right-click on a row arrives at all. Three ways of posting a secondary click from *outside* the
process were tried first and every one of them failed silently; `docs/TRAPS.md` has them.

Putting the order back must restore the document exactly, which
is the check a viewer that reordered its own view but not its model passes and a viewer that
lost a page fails. Two of the eleven drive `page_move` for real, including the refusal of a
page moved behind itself, and one is undo.

The three that ask the backend are the `page_delete` round trip: the command is registered,
names a page by identity, refuses a second deletion of that id as *deleted* rather than as
unknown, and undo puts the page back. They leave the model as they found it, asserted rather
than assumed, because every phase after them reads the document.

Of the page-turn ten, three are negative --- a page nobody turned keeps its proportions and its
upright text, and `viewer.rotation` does not move --- because every positive statement about a
turned page is equally true of a view that rotated everything. The tenth is the half turn, and
it exists because a mutation deleting the invalidation that runs *before* the geometry survived
all nine others: a quarter turn changes the page box, so `applySizes` invalidates it either
way, and only 180 degrees leaves the box identical.

The Windows column is *not* carried forward: it was measured at 163 names on 2026-08-02 and
nothing has re-run there since, so it is absent rather than adjusted --- which is what this
page has twice recorded arithmetic in a measurement column costing.

**Two rows are new**, and both earned their place the day they were added. `links.pdf` caught a
destination landing on the page before the one it named --- the only corpus that could, being
the only fixture in the tree with a `/Fit` entry. `links-cropped.pdf` caught two checks whose
control could not be established on a document with a single link, because the check before
them follows it.

**Re-run 2026-08-24 on Windows**, with `inherited.pdf` promoted from an exclusion to a corpus:
**343** names, no failing check anywhere, 731 s. `text-heavy.pdf` is not on this machine ---
no script writes it, it is a real document supplied by hand --- so the run covered **14 of the
15** and its row below is the earlier macOS reading, marked as such. The sweep refuses a
missing fixture outright rather than skipping it, which is why the run had to name the other
fourteen.

| fixture | ran | skipped | what it is there for |
|---|---|---|---|
| `text-heavy.pdf` | 292 | 50 | *(2026-08-23, macOS --- not on the machine that ran the rest)* the dense case, and search across 775 pages |
| `outline-simple.pdf` | 299 | 44 | the only fixture with an ordinary outline |
| `outline-hostile.pdf` | 299 | 44 | the only one with a `/Launch` entry to refuse |
| `vector-heavy.pdf` | 198 | 145 | one page, no extractable text, and no white paper to invert |
| `vector-multi.pdf` | 238 | 105 | twelve A0 pages: the only one where a thumbnail is slow enough to collide with the viewer |
| `rotated-90.pdf` | 278 | 65 | every page at `/Rotate 90`, which nothing else in the corpus has |
| `columns.pdf` | 288 | 55 | the only one whose content-stream order is not its reading order |
| `tagged.pdf` | 263 | 80 | the only one carrying a `/StructTreeRoot`, and the only two-page one |
| `multilingual.pdf` | 280 | 63 | the only one whose text is not Latin: CJK with no word separators, Arabic right-to-left, a decomposed accent, and a code point above the BMP |
| `encodings.pdf` | 281 | 62 | the only one whose character mappings are absent, broken or predefined --- and the only fixture that reaches the replacement-character path at all |
| `mixed.pdf` | 282 | 61 | the only one whose pages are not all the same size, and the only one that exercises the three layout checks at all |
| `comments.pdf` | 306 | 37 | the only one carrying annotations: notes, a reply, a highlight, three text-string encodings, an indirect `/Annots` array and 1,200 marks on one page --- the only corpus where all eight comment checks run |
| `links.pdf` | 308 | 35 | the only one with link annotations, and the only one whose outline is deliberately not in page order --- which is what let it catch a destination landing on the page before the one it named |
| `links-cropped.pdf` | 245 | 98 | the only one whose `/CropBox` is not its `/MediaBox`, so a rectangle placed in media space lands visibly wrong |
| `inherited.pdf` | 272 | 71 | the only one whose pages take their `/MediaBox` from an ancestor while carrying a quarter turn, and the shortest displayed page in the corpus at 400 points |

**The 2026-08-23 sweep read 342 names on macOS, and the difference is not one number.** The
one new name is `file.mergeDocuments runs from the palette`, checked against the run's own
name list rather than inferred from the total. Every row's ran/skipped split also moved by one
or two, and **that is not attributed here**: this run is a different platform *and* four
commits later, so a per-row difference between the two is two variables at once --- the trap of
that name. The invariant the sweep asserts is none of these totals, it is that all fourteen
agree on the *names*, and both runs satisfy it.

The 2026-08-23 run for the record: **342** names on fourteen (the same list with `inherited`
excluded), 721 s, no failing check anywhere, and `vector-multi` and `vector-heavy` 73% of the
time between them. Five of those names are the overlay-against-the-file phase, six are
cropping by dragging, and two are the eraser taking a mark whole.

**Re-run the same day** after the eraser and the crop drag: **329 -> 342**, every corpus
gaining thirteen *runs* and losing none. Two sweeps in one day are worth one note --- the
totals move whenever a check is added, so a row here is a statement about the run that
produced it, and the invariant the sweep asserts is that all fourteen agree.

**Re-run 2026-08-18** with the crop: **267** names on all fourteen, seven more than the 260
below. Every corpus gained seven *runs* and lost none.
All seven are one backend phase, driving `page_content_box`, `page_geometry` and `page_crop`
against the real backend --- which is the only place that can say the three commands are
*registered*, the failure every layer below passes through. Its last check is the control: a
crop whose corners are the wrong way round has to come back as the refusal the model names.

The two palette commands are deliberately **not** driven from the palette, and the reason is
in `viewercheck.ts` beside them: cropping to content is two IPC replies deep --- measure the
ink, then ask what size the page becomes --- and the probe framework's settle is a frame-loop
wait rather than a reply wait. Their wiring is covered by `appcommands.test.ts`'s sweep over
every registered command.

**Re-run 2026-08-18** with the keyboard route to a mark: **260** names on all fourteen,
six more than the 254 below, and the rows above are that sweep's. Every corpus gained six
*runs* and lost none --- the four for the walk and the guard need no selection and no corpus
feature, since the marks are two the harness hands the viewer, and so do the two commands in
the sweep every registered command gets. The two command probes are the only pair in that
table with no `unless`: no fixture carries a mark, because these are the reader's own.

Four of the six are in a real webview for a reason a unit test cannot cover. The guard that
stops a key typed into a note from moving the page is about a key **bubbling** from the field
to the root handler; vitest dispatches at the root with a target of its own choosing, which is
a statement about the handler rather than about the tree it is installed in. Its control is in
the same check --- the same key pressed on the page must still scroll --- because a guard
tested only on its refusal is satisfied by a viewer that ignores everything.

**Re-measured 2026-08-18** with the note on a mark, on all fourteen corpora: every one
reports the same **254** names, and the rows above are that sweep's, pasted from what
`viewer_sweep.py` prints. Fourteen names on top of the 237 the morning's highlight work
left: **nine** for the note box, **four** for the three mark commands driven against the real
backend, and **one** for `edit.removeMark` in the sweep every registered command gets.

**Re-run 2026-08-18** with underline and strike out: **254** names on all fourteen, three
more than the 251 below. Two are the new commands in the sweep that every registered command
gets --- aimed separately, carrying the kind in the expectation, since one action taking a
parameter is where a copy-and-paste gives a reader a Strike out that highlights. The third
drives each kind through `annot_mark` against the real backend and reads the kind back off
the state reply. Every corpus gained three *runs*: they need no selection and no corpus
feature, because the mark is one the harness hands the viewer.

**Re-run earlier the same day** after the page-turn placement fix, and every one of the
twenty-eight numbers then came back **byte-identical**, diffed rather than eyeballed. That is the honest
result and it is worth stating rather than quietly re-pasting the same table: the defect it
fixed --- a comment or a link on a page an edit had turned, drawn in one place and found in
another --- needs a page turn and an annotation *at the same time*, and no fixture in this
corpus has both. The window harness could not have caught it and still cannot. What it does
cover is the primitive underneath: a mutation that turns every rectangle a quarter too far
reddens three of the mark phase's checks, so the one implementation those three subsystems
now share is reached from here. The measurement that found the defect is a differential in
`viewerturns.test.ts`; see `docs/PLAN.md`.

**Every corpus gained runs and no skips**, which is the difference from the highlight
increment and is deliberate. Those checks needed a selection, so the two fixtures with no
extractable text skipped them; these are driven against a mark the harness hands the viewer
itself. The model is tested in `docmodel.rs`, the file in `annot-probe`, and this phase tests
what neither can reach --- a rectangle on screen, a press landing on it, and the box that
opens --- for which a synthetic rectangle is the right input and runs everywhere.

That earlier sweep is worth keeping for the shape of its failure: the first version of the
quad checks left them out of the no-text path entirely, so two corpora reported 235 names
against everything else's 237 --- a name that had *vanished* rather than one that skipped,
which is exactly what the identical-name-sets rule is for.

**`tagged.pdf` runs three of these thirteen and skips ten**, which is the split worth knowing:
the ten that drive the viewer need a middle page to delete and it has two, while the three that
ask the backend need only a page to spare. They are the only checks of this phase that run
there, and skipping them along with everything else would have been a skip for a reason that is
not theirs.

**`vector-multi` is the one with no margin, and the sweep's timeout was raised for it.** It
takes **325 s** of the default 900 (420 until 2026-08-17, which the page-deletion phase went
past before that phase was cut from two delete-and-restore cycles to one). Every other corpus
is minutes. A tight timeout is worse than a generous one here: it fails as *"the run printed no
CHECK-NAMES-JSON line"*, which reads as a crash rather than as a bound.

**A `pkill -f "tpdf.app/Contents/MacOS/tpdf"` goes between runs**, and
`scripts/viewer_sweep.py` does it. A leftover window occludes the next one, WebKit suspends an
occluded page, and the run then produces nothing and uses no CPU --- twice, before that went
into the sweep script. `TPDF_RAISE=1` covers the other half, a window with nowhere visible to
go, and the script sets it.

**`text-heavy.pdf` moved by one between two sweeps an hour apart** --- 177/41 and 176/42, the
same 218 names --- which is the race the notes below describe, not a regression. It is left as
the second reading rather than the flattering one. Those are the totals of that day, when the
name set was 218; the row in the table is a later sweep against a larger set, so read the
*one* it moved by, not the absolute figures.

**Every row above is one run, and the notes below say why that is a point estimate rather than
a bound.** Two of these fixtures have a check that lands on either side of a race, so their
split moves between runs while the name total does not. The table is not re-ranged here
because a range needs several runs per fixture and this sweep was one each --- read a
disagreement of one as the documented variation, and a disagreement in the *name total* as the
bug this arrangement exists to catch.

The `text-heavy.pdf` row was `142 / 21` and marked derived until 2026-08-03, when running it
on the only machine that has the document made it `143 / 20`. The derivation was one check
out, which is the cost of carrying arithmetic in a column of measurements: it looks exactly
like the rows either side of it.

**Then the measurement replaced it with a point, and the true value is a range** --- two runs
the same evening against the release bundle both reported `142 / 21`. Nothing regressed: the
one check that moves is the withdrawal race described below, and this row and
`outline-simple`'s are the two the note there says land on both sides. So the correction
above fixed the arithmetic and reintroduced the shape of the original error, which is worth
saying plainly: **a number measured once is still a point estimate of a quantity that varies**,
and for these two fixtures the invariant is the name total, not the split.

**`outline-simple`'s range was widened to `147--149 / 14--16` on 2026-08-08**, on six Windows
runs: three at `149 / 14`, then two at `148 / 15` and one at `149 / 14`. The total is 163 in
all six, which is the invariant the paragraph above says it is --- and the widening is the
same lesson a third time, since `147--148` was itself two platforms' point estimates read as
a bound. Do not narrow it back on the strength of one green run. The three runs that came
first also carried the stale-focus-mirror failure described in `docs/TRAPS.md`, so treat any
row of this table as a statement about the split, never about whether the run passed.

**The two A0 corpora straddle the watchdog's old 300 s default, and their spread is far wider
than the bound.** Four runs on macOS on 2026-08-03: `vector-multi.pdf` at **275 s**, **387 s**
and once still going past **600 s**, and `vector-heavy.pdf` at **249 s** having been killed at
300 s on the run before. So the bound was a coin flip rather than a consistent failure, which
is why it survived --- a corpus that fails every time gets fixed, and one that fails half the
time gets re-run. Do not read any single figure here as the cost; the spread is the
measurement, and it is roughly 2.2x on one document. Both are the fit-page setup on an A0
page, which is the operation
that defeats spatial culling: the whole page becomes visible, and PDFium charges its large
fixed cost per render call. The two bounds are one number now --- `viewer_check.py` derives
`TPDF_VIEWERCHECK_TIMEOUT` from its own `--timeout` --- so raising the one people edit raises
the one that decides. Before that they disagreed, and the app's was the tighter.

**Every measured row is its previous split with exactly three more skips**, which is a
stronger statement than a matching total: the three layout checks added on 2026-08-02 skip
on every fixture but `mixed.pdf`, for the stated reason that the fixture has no geometry
sidecar, and nothing else moved. `mixed.pdf` is the eleventh row and the only one where they
run.

The earlier note that eight of nine rows were the macOS split plus arithmetic still holds
underneath this one, and so does the exception: `multilingual.pdf`'s generator picks a font
per page from what the machine has, so the Windows fixture is a different document and one
check that skipped on macOS for want of text has text to work on here. A ran/skip split is a
property of the document, and that corpus is the only one whose document is not the same on
both platforms.

**Every measured row above is green as of 2026-08-02, and getting there took two rules rather
than one.** The check involved is `a page reads in the order its generator laid it out`, which
compares each page against what the generator *wrote*. It is downstream of `text.rs`,
`reading.ts` and the fixture's machine-local fonts and of nothing in the layout ---
`readingChecks` builds its own `TextCache` and never touches the viewer or the scroller --- so
it is the check that sees a font substitution, and both failures below were one.

**`multilingual.pdf` was red for a missing space, and that is fixed.** The folding page came
back `cafélatte` where the manifest says `café latte`. PDFium's extraction *does* contain the
space --- `text-probe --mode order` shows `café`, a space run, then `latte` --- so it was
dropped between extraction and the line's *ranges*. Measured through `FPDFText_GetCharBox`
against the vendored library: the space at index 4 comes back **placed**, 0.02 pt tall at
y 752.00--752.02, while every letter on the line sits at 752.14--766.08, the two bands missing
each other by 0.12 pt. `reading.ts` refuses a box that thin and re-attaches it by preceding
index. The page reads `café latte` here as of the run above.

**Fixing it broke `encodings.pdf`, and only running the whole corpus found that.** The rule as
it first landed was absolute --- under `SLIVER_PT`, a tenth of a point --- which is a claim
about glyphs and turns out to be a claim about *metrics*. Page 2 of `encodings.pdf` is set in a
predefined CMap with no embedded font, so PDFium has no metrics for it and reports **every**
character 0.018 pt tall. All of them were refused, nothing was placed, and the page came back
as a single fragment: its two lines, 632 pt apart, read as one. Established by reverting
`reading.ts` alone and rebuilding, which gives exactly complementary results:

| | `multilingual` | `encodings` |
|---|---|---|
| absolute rule absent | **129/130** `cafélatte` | 130/130 |
| absolute rule present | 130/130 `café latte` | **129/130** `日本語の符号\r\n日本語の符号` |

**The rule is a conjunction now**: `height < SLIVER_PT && height < SLIVER_OF_LINE * typical`,
where `typical` is the median height of the page's placed characters and `SLIVER_OF_LINE` is a
twentieth. The two measured samples are three orders of magnitude apart on the relative
quantity and adjacent on the absolute one --- 0.02 pt against 13.94 pt letters is 0.0014 of
them, 0.018 pt against a page median of 0.018 is 1.0 of it --- and `tagged.pdf`'s comma, at a
third of its letters, is well clear of both and stays `SHORT_MARK`'s business. Each half was
proved by a mutation turning exactly one test red; the median was proved against a maximum,
which survived the whole suite until a control was written for it. See the traps for the full
account.

**What this cost, and the discipline that paid for it:** the fix was verified on the corpus it
was written for and on macOS, where all eleven were green because that machine's substitute
font has real metrics. Nothing in either run could see the regression. It surfaced only from
re-running every corpus and diffing the name sets, which is what the standing instruction above
asks for and the reason it is worth its wall-clock.

**The two-page one is worth having for a reason unrelated to tags.** Adding it turned three
checks red that had been green on every corpus for a week --- two nav probes guarded on "more
than one page" where the guard has to be "a page that can be reached", and a search check that
cannot tell "the scan restarted" from "there was nothing ahead to find". None was a defect in
the subject; all three were preconditions written as assertions, and the smallest multi-page
fixture until then had three pages. See the traps.

**The multilingual corpus paid for itself before it was green.** It found four things, and only
the first is in the viewer: a search picker written as `/[A-Za-z]{5,}/` matched nothing on a
Japanese page, so **seventeen** search checks skipped while printing *"page 1 has no extractable
text"* about a page with forty-nine characters on it --- the checks did not run and the reason
printed was false. Twelve of them run now, on the same binary. The drag check had no precondition
for *"there is text where I dragged"* and reported a sparse page as a defect. In the backend,
`FPDFText_GetUnicode` turned out to be a UTF-16 API, so a code point above the BMP arrived as two
lone surrogates and was unfindable; and a combining accent on a word with no ascender opened a
line of its own. See the traps for each.

Its own harness is **`examples/search-probe`**, which is where the search claims live: 60/60 with
9 not applicable, against a manifest a different program wrote. Run it directly, since it needs no
webview:

```sh
cargo run --release --example search-probe -- --file ../testdata/multilingual.pdf
```

Twenty-one queries, and the manifest labels each count as **stated** (from what the generator
wrote), **measured** (a property of PDFium this corpus established --- that the Alphabetic and
Arabic Presentation Forms come back normalised) or **decided** (a product decision). Conflating
the three is how a measurement comes to read as a specification, so a change to a `decided` count
has to be argued for rather than absorbed --- and one of them has since been argued for and
changed: the fold case-folds rather than lowercasing since 2026-08-01, so `strasse` finds `Straße`
and its count went from 1 to 2. The `decided` prose records both the old answer and the new one.

**`encodings.pdf` is the other half of the multilingual work**, and a separate corpus because
the subject is different: those pages are correct documents in other scripts, and these are
documents whose own statement of what their bytes mean is missing or wrong. Three pages, and
it found a product defect on the first run.

| page | what it is | what it established |
|---|---|---|
| `no-mapping` | Identity-H, **no `/ToUnicode`** | PDFium does not fail --- it returns eighteen characters of plausible garbage for eighteen drawn. The page is *not* textless, so nothing tells a reader that a search of it means nothing |
| `broken-map` | a `/ToUnicode` with lone surrogates | the only fixture reaching `text.rs`'s replacement path. Two of its broken entries also **pair into one astral character**, which nobody predicted |
| `predefined` | `/UniJIS-UCS2-H`, **non-embedded** KozMinPro | extracts correctly, so the `chromium/7881` build has the bundled Adobe-Japan1 CMaps. A fact about the pin, to re-establish if it moves |

```sh
cargo run --release --example search-probe -- --file ../testdata/encodings.pdf
```

23/23 with 7 not applicable. Six of its seven queries are `measured` rather than `stated`: what
a broken document extracts as is a property of PDFium, and writing it as a fact about the file
is how a measurement comes to read as a specification.

**The defect it found is in the regex path.** A pattern was compiled case-sensitively against a
haystack the fold had already lowercased, so with match-case off **any uppercase letter in a
pattern matched nothing at all**. It survived because `compile`'s own doc comment asserted the
invariant it was breaking, and because `viewer_check.py` builds its pattern from a word taken
from the page --- so on every corpus with ordinary prose the pattern was lowercase and the two
sides agreed by accident. This corpus's garbage happens to be uppercase.

**Compare the name *sets*, not the counts, and slice the name by column.** Every label is
exactly six characters --- `[OK]  `, `[FAIL]`, `[SKIP]` --- so the name begins at column 7
whatever the outcome, and consuming the label with a regex `\s` eats one space for `[OK]`
and none for `[SKIP]`. An ad-hoc comparison written that way reported five corpora
disagreeing when the only difference was which checks had skipped. The name column is padded
to 40 and a longer name is followed by a single space, so there is no reliable split at all:
key on the first 41 characters after the label, which are byte-identical for the same check
on any document. Two corpora legitimately differ in the *order* they record two checks;
compare sets.

**Diff the names mechanically, and not with a naive split.** `record` pads each name to 40
characters and then prints the detail, so a name *longer* than that is followed by a single
space and any pattern keyed on "two or more spaces" swallows the whole line --- the padded-column
trap, walked into again on 2026-07-30 while checking this very invariant. The label is seven
characters wide and the padded name forty, so a fixed slice is exactly right:

```
grep -E "^\[(OK|FAIL|SKIP)\]" run.log | cut -c8-47 | sort > names.txt
```

**That `8` is a fact about this harness, not about the repository.** `backend-probe` and
`worker-probe` built their label by interpolating `OK`/`FAIL` into `[{}]`, so their passing
rows began at column 6 and their skipped rows at column 8 --- and the recipe above, applied
there, sliced the `[OK]` rows two characters short and reported *"the name sets diverge"*
across three corpora that were in fact identical. Both now pad the label to seven like
everything else, so one recipe reads every harness; before copying it to a new one, check the
widths (`grep -hoE "^\[[A-Z]+\] *" run.log | awk '{print length($0)}' | sort -u` must print a
single value). See the trap of that name.

Six of those, diffed pairwise, is the invariant in one command. It also reports the count,
which must equal the number of unique lines --- two checks whose first forty characters
coincide would otherwise merge silently.

**86 until 2026-07-30**, when word and line selection added three, the palette's argument
mode added five, the two find options added three, the results sidebar added four, the fit
modes added six, and reading order added two. The
results four skip together on a document with no extractable text, which is why the two
vector fixtures gained four skips and no runs. The selection three run on every
corpus with extractable text, rotated included --- line grouping follows the page's own
reading axis, so there is nothing in them that assumes lines advance downwards --- and skip
together on the two vector fixtures, which is why those gained three skips and no runs. The
find-option three are the same shape: they need a word taken from page 1, so they run
wherever search does. One of them skips on a fixture whose needle is already upper case,
there being no spelling of it that matching case would reject --- and it says so rather than
passing on nothing.

The reading-order two run only where a manifest exists, which today is `columns.pdf` alone;
everywhere else they skip together, which is why every other corpus gained two skips and no
runs. `columns.pdf` in turn skips two that no other corpus does --- the drag-ordering check,
whose premise is false on any multi-column layout, and the rotated-lines check, whose samples
are shorter than it can compare. Both say so.

The fit six run on every corpus but one, and the exception is the informative part:
`rotated-90` skips *"fitting the page shows less of it than fitting the width"*, because its
pages are landscape and already fit the window vertically at fit-width. That check is the
control on the one beside it --- without it, "fit page shows the whole page" would be
satisfied there by doing nothing --- so it prints the measurement that made it inapplicable
(`495px in 700px`) rather than passing.

**The single values in that table are one sample each**, not a claim that nothing moves. One
check races (see below) and can swing a run by one in either direction; a `78--79` style
range in an earlier revision of this table was that check being honest.

**`vector-multi` takes about 4m40s**, and everything else a fraction of that --- twelve A0
pages is what it is for. The default timeout was 300 s, which sat close enough to that to
fail intermittently, and the timeout path *discarded the transcript* --- so a slow machine
produced one line, `[FAIL] run timed out`, which is exactly what a page that never ran a
line of JavaScript produces. It now prints how far it got and the bound is 900 s, well
clear of the slowest corpus rather than beside it.

The two vector fixtures skip three of the six inversion checks, and that is the design
working rather than a gap: "the page went dark" cannot be shown on a document with no bright
paper, so it says so instead of passing on nothing.

**The ranges are all one check: "the strip withdraws its work when the viewer needs the
renderer".** A thumbnail on a cheap page takes about a millisecond, so whether one is still
in flight when the viewer asks for a tile is a race, and the check skips when it is not ---
correctly, since nothing outstanding reads exactly like a successful withdrawal. Repeated
runs of `text-heavy` and `outline-simple` have each landed on both sides of it. It is
deterministic only on `vector-multi`, which exists for it.

Absolute counts are deliberately not quoted in this paragraph: they move whenever a check is
added, and a stale number here would send someone looking for a regression that is a
changelog entry. The table above is the one place they are written down.

**So the ran/skipped columns are not the invariant** --- the **names** are, and how many there
are of them is in the table above rather than in this sentence, which said `109` for two days
after the number stopped being right.

**Measured 2026-08-20 --- 279 names, on every corpus, byte-identical as sets:**

| fixture | ran | skipped | failed |
|---|---|---|---|
| `rotated-90` | 227 | 52 | 0 |
| `comments` | 244 | 35 | 0 |

Re-measured the same day at **281 names** after multi-stroke drawing added the
two preview checks: `comments` 246 ran / 35 skipped / 0 failed, 281 names, all
distinct. And again at **284** when the eraser landed: `comments` **249 ran / 35
skipped / 0 failed**, all distinct. The three it added are `edit.erase` in the
command sweep and the two that read the eraser's preview --- *"a stroke the
eraser has taken stops being drawn at once"* at 38% of the band before the nib
and 0% after it, and its control *"and one the nib missed is still there"* at
44%. The control is not a formality: an overlay that stopped painting the whole
drawing satisfies the first check perfectly.

**The first of those runs went red, on the check written for exactly it.**
`edit.erase` was registered and unclassified, so *"every registered command is
classified, and every classification is registered"* failed with
`unclassified [edit.erase]` --- which is the trap about a command deliberately
left out of the harness still having to be classified, firing on a command that
was not meant to be left out at all. That last clause is now checked by the harness itself --- `Report.finish`
fails a run in which two checks share a name, because the roll above is compared
as a **set** and a set cannot see a repeat. It caught a real one within the hour
of being written; see `docs/TRAPS.md`.

Two names were added that day --- `edit.draw` in the command sweep and *"a drawing follows
its strokes and does not fill its rectangle"* in the overlay phase --- and *"the five kinds do
not all look the same"* was reworded to `six`.

**Measured 2026-08-20 at 310 names**, after the marks panel. Seven are new ---
`view.showMarks` in the command sweep, the five that drive the panel, and *"every sidebar
tab fits inside the panel"*. Two of the panel's five are worth naming because they are the
ones no unit test can reach: *"activating a row opens that mark's note and goes to it"*,
which needs a real press on a real element and a document with pages to travel through,
and *"pressing a mark on the page selects its row"*, which is the `onMark` wiring end to
end --- the popup reports, the viewer forwards, the panel marks the row, and nothing in
the phase told the panel which mark that was.

**The seventh went red on its first run, on a defect it was written to look for.** Five
labels want 293 px of content in a 260 px sidebar, so **Marks** was clipped by the host's
`overflow:hidden` --- present in the DOM, `role="tab"`, and unreachable by a pointer. The
tab *count* check beside it passed throughout, because a clipped button is still a button.
The row wraps now, and the detail line prints every label's `scrollWidth/clientWidth` so a
failure says which one and by how much.

**And the sweep is what found two defects in the panel phase itself**, neither visible on
`comments.pdf`: on `links-cropped`, a one-page document, the phase's two synthetic marks
were at the same height on the same page, so the press meant for the first opened the
second; and on `rotated-90` the check asserted the viewer's page number after activating a
row, which the last page cannot satisfy --- a scroll to the end clamps and leaves the page
before it at the top of the viewport. The trap is recorded under that name. It asserts the
mark is *visible* now, which is what "goes to it" means and is what a viewer that opened
the note without scrolling fails.

**One check was red on four corpora and is now fixed.** *"a text box draws its words and
not its rectangle"* failed on `vector-heavy`, `vector-multi`, `rotated-90` and
`links-cropped`; a `git worktree` control at the text-box commit reproduced it, so it
shipped there and was invisible because that increment was verified against `comments.pdf`
alone. **The painter was right on all four** --- the predicate's every reading was a
fraction of a rectangle that scaled with the page, while a text box's type is a fixed
11 points, so it failed in both directions at once: on A0 the readings rounded to zero, and
on a 20-pixel-tall box the `edges` sample, which reads the middle tenth of the height,
landed on the second line. It is now two type-sized bands and three border strips measured
in points off the box's own corner, on a fixture rectangle of a fixed 260 x 90 points.
`docs/PLAN.md` has the full account, including why the 90 came from the sampler's
two-pixel floor rather than from the type. Three mutations prove it can fail.

**Measured 2026-08-20, all fourteen corpora, `--raise` off** (superseded by the 2026-08-21
table below, and kept because it is the run the text-box repair was proved against): the same
**310 check names**
on every one, diffed as sets. `text-heavy` 265/45, `outline-simple` 273/37,
`outline-hostile` 273/37, `vector-heavy` 172/138, `vector-multi` 212/98, `rotated-90`
258/52, `columns` 262/48, `tagged` 237/73, `multilingual` 254/56, `encodings` 255/55,
`mixed` 262/48, `comments` 275/35, `links` 282/28, `links-cropped` 217/93. 740 s in total,
of which `vector-multi` is 398 s and `vector-heavy` 164 s. The only failing check anywhere
was the one above. **Re-swept after the repair: all fourteen green** --- every ran/skipped
split byte-identical to the run above, the same 310 names, and `no failing checks on any of
14 corpora`. The splits being unchanged is the useful half: repairing a check by making it
skip somewhere would have moved one.

**Measured 2026-08-21, all fourteen corpora, `--raise` off: the same 313 check names on
every one, no failing check anywhere, 686 s in total.** Three names were added that day ---
*"a row's remove control asks for that mark and does not open it"* with the marks panel's
remove control, then *"and the words they cover are the words that are selected"* and *"a
mark nothing was typed on is listed by the words it covers"* with the covered-words row.

| corpus | ran | skipped | 2026-08-20 | s |
|---|---|---|---|---|
| `text-heavy` | 268 | 45 | 265/45 | 25 |
| `outline-simple` | 276 | 37 | 273/37 | 10 |
| `outline-hostile` | 276 | 37 | 273/37 | 10 |
| `vector-heavy` | 174 | 139 | 172/138 | 152 |
| `vector-multi` | 214 | 99 | 212/98 | 341 |
| `rotated-90` | 261 | 52 | 258/52 | 9 |
| `columns` | 265 | 48 | 262/48 | 9 |
| `tagged` | 240 | 73 | 237/73 | 38 |
| `multilingual` | 257 | 56 | 254/56 | 39 |
| `encodings` | 258 | 55 | 255/55 | 9 |
| `mixed` | 265 | 48 | 262/48 | 9 |
| `comments` | 278 | 35 | 275/35 | 15 |
| `links` | 285 | 28 | 282/28 | 11 |
| `links-cropped` | 220 | 93 | 217/93 | 8 |

**Re-measured the same day after the comments panel: the same 317 check names on every one,
no failing check anywhere, 680 s.** Four names were added --- *"a mark nobody wrote on is
listed by the words it covers"*, *"and they are the words the fixture's generator says are
there"*, *"and those words are really on the page it is on"*, and the control *"a comment
with a body is still listed by what its author wrote"*.

| corpus | ran | skipped | earlier that day | s |
|---|---|---|---|---|
| `text-heavy` | 268 | 49 | 268/45 | 25 |
| `outline-simple` | 276 | 41 | 276/37 | 10 |
| `outline-hostile` | 276 | 41 | 276/37 | 10 |
| `vector-heavy` | 174 | 143 | 174/139 | 149 |
| `vector-multi` | 214 | 103 | 214/99 | 339 |
| `rotated-90` | 261 | 56 | 261/52 | 9 |
| `columns` | 265 | 52 | 265/48 | 9 |
| `tagged` | 240 | 77 | 240/73 | 38 |
| `multilingual` | 257 | 60 | 257/56 | 39 |
| `encodings` | 258 | 59 | 258/55 | 9 |
| `mixed` | 265 | 52 | 265/48 | 9 |
| `comments` | 282 | 35 | 278/35 | 15 |
| `links` | 285 | 32 | 285/28 | 11 |
| `links-cropped` | 220 | 97 | 220/93 | 8 |

**Twelve corpora are `+0` ran and `+4` skipped, and the two carrying annotations are `+4`
ran and `+0` skipped.** That is the whole check on the run: the covered-words checks need a
markup annotation nobody wrote on, so on every fixture without one they must stand down by
name rather than vanish, and `comments` and `links` are exactly the two that have one.

**The first attempt at this run was red, and it is the reason the split above is worth
reading.** `commentChecks` returns early on four paths --- the comments could not be read,
the document has none, no comment has a rectangle on the page, the last one has no row ---
and the new checks were called after all of them. So twelve corpora neither ran nor skipped
them: `comments` and `links` reported 317 names and everything else reported 313, and each
of those runs passed on its own. A single-corpus run said `282/282 checks passed` and looked
perfect. Only the cross-corpus name-set diff can see a check that is **absent** rather than
failing, which is what `viewer_sweep.py` is for. The names are a module constant with a
`skipCoveredWords(why)` helper now, called at each of the four returns.

**The previous column is there because the way the splits moved is the check on the run.**
Twelve corpora are `+3` ran and `+0` skipped; `vector-heavy` and `vector-multi` are `+2` and
`+1`, and those two are the documents with no text to select --- so the check that compares
the selection's words is skipped there, exactly as its sibling *"a mark's rectangles come
from the page's own text"* already was. Three names arriving and every corpus accounting for
all three, in the two patterns its own contents predict, is a stronger statement than
fourteen green lines.

`vector-multi` is 50% of the wall clock and `vector-heavy` 22%, as before. The total fell
from 740 s to 686 s, which is machine noise rather than anything about the harness --- these
are single samples, not a benchmark.

Two of the three are unreachable from a unit test, and for different reasons worth keeping
apart. The remove control's is that the fake DOM does not bubble, so `marklist.test.ts`
cannot tell `stopPropagation` from its absence. The covered-words row's is that the fake DOM
**resolves no styles at all**: the words a mark covers and the note a reader typed sit in the
same column, and the only thing separating them is that one is dimmed and italic, so a panel
drawing them alike passes every unit test there is. That check paints a noted row beside a
bare one and reads `getComputedStyle` on both --- the noted row being the control, since a
panel calling every line the document's would satisfy half of it.

One check failed once and did not recur: *"a drag selects text from where it was dragged"*
on `outline-hostile`, in one of three sweeps that day. Two runs failing different checks is
variance; the same check twice is a defect --- the trap is recorded under that name, and
this was the first shape.

⚠ **The first run of that measurement was against `text-base14`, which is not a window
corpus.** `viewer_sweep.py --list` classifies it as *"a backend-probe fixture: font coverage,
measured through the worker"*, and the sweep was pointed at it anyway --- the trap recorded as
*"a probe fixture swept as a corpus, against the file that already said not to"*, walked into
by the person adding checks to the harness. It passed 177/279 with 102 skipped and **the same
279 names**, which is why nothing looked wrong: the name set belongs to the harness, so it is
identical whatever you open, and only the ran/skipped split is a fact about the document. A
split from a non-corpus is meaningless as a table row, and it was written into this table as
one. Take the fixture from `viewer_sweep.py --list`, not from `ls testdata`.

⚠ **The `109` above is of 2026-07-31 and is not the current count.** Between then and now the
harness gained marks, crops, print and the comment panel, and nothing moved that number. It
was left, and it then did exactly what a stale count does: an increment predicted the new
total as `109 + 2 = 111` and was wrong by 168. **Take the count from a run, never from this
file** --- the sentence below about the ran/skipped columns not being the invariant is the
same warning, and it did not stop the arithmetic being done anyway. Read the names, and read
`CHECK-NAMES-JSON`, which the harness prints for exactly this purpose:

```sh
python3 -c 'import json,sys;print(len(json.loads([l for l in open(sys.argv[1]) if l.startswith("CHECK-NAMES-JSON")][0][16:])))' run.log
``` A count chased
back to a documented value is a defect introduced to satisfy a document, and the repair here
would be to delete the outstanding-request condition that makes the withdrawal observable at
all. Read a differing count by checking that the name is present and `[SKIP]`; a name that
has *vanished* is the bug this arrangement exists to catch.

This was written as a fixed `65 | 10` first, and a perfectly ordinary run then read as a
regression. **A table that records one sample of a race as an invariant makes the next honest
run look like a defect** --- state the range and what varies, or the check that flips gets
"fixed" by someone chasing a number.

**Do not run all six while iterating.** Each run needs an `.app` bundle rebuilt and takes
the better part of a minute, and six transcripts of green is not evidence of anything --- the
value of a regression check is in the run that goes red, and nothing about running the same
one repeatedly makes that more likely. Use **one** corpus while a change is in progress,
picked for what it can exercise, and the full sweep **once before a commit**, where "did I
break something elsewhere" is the actual question. What the sweep is for is the corpora's
*differences*: `vector-heavy` skips 31 of the 75, and those are the ones a single corpus
cannot tell you about.

`vector-heavy` skipping most of them is the expected output there, not a problem. The one
search check it does run is the useful one for that document: that the viewer says there is
no text to search rather than reporting no matches.

`rotated-90` is the only document where the text layer's coordinate turn is exercised at all,
and the defect it found was total rather than subtle --- see `docs/PLAN.md`. Its selection
ordering check skips, with the reason: on a page whose lines advance sideways a horizontal
drag crosses all of them, so the comparison is meaningless. What checks that mapping properly
is the probe, per rotation:

```
for page in 0 1 2 3; do
    for view in 0 1 2 3; do
        src-tauri/target/release/examples/text-probe testdata/rotated.pdf \
            --page $page --mode align --view-turns $view
    done
done
src-tauri/target/release/examples/outline-probe testdata/rotated-90.pdf --mode check \
    --manifest testdata/rotated-manifest.json
```

`--mode order` is the third mode and asserts nothing --- there is no right answer for it to
check, because the order a page's characters arrive in is a property of whoever produced the
file. It prints them, which is the only way to see from outside the viewer that the file's
order is not the page's:

```
src-tauri/target/release/examples/text-probe testdata/columns.pdf --page 1 --mode order
```

On page 1 of that fixture it prints `alpha one beta one`, `alpha two beta two`, and so on ---
two columns merged line by line, which is what `src/lib/reading.ts` exists to undo, and what
the clipboard used to get.

`--view-turns` rotates the *view* on top of the page's own `/Rotate`, which is what Cmd-R
does. All sixteen combinations should report 100% of character boxes on ink with every wrong
turn under the control ceiling; anything else means the render and the boxes have stopped
agreeing, and the pattern of which combinations go red says which half. Dropping the
placement's dimension swap fails only the odd view turns; ignoring the rotation in the render
fails all twelve rotated ones.

`vector-multi` earns its place with three checks and nothing else: a thumbnail costs about a
millisecond on a text page and a second and a half on an A0 sheet, so it is the only corpus
where the page strip can still be rendering when the viewer asks for a tile. On every other
document those three report `[SKIP] the thumbnail finished before the viewer asked for
anything` --- which is the honest answer, and is why they are not written as a pass.

What it does **not** cover: the command list `App.svelte` registers, and the Cmd-K that
opens the palette. The check builds its own registry, so it proves the palette works and
not that the application's commands are wired to it.

### Checking that a mark a reader makes reaches the document

The chain a mark travels --- command, gesture on the viewer, callback, edit model,
overlay --- had **nothing running over it end to end**, and a reader found the hole on
2026-08-22: a shape drawn on the last page of a document was dropped with no command sent
and no message shown, while all sixteen gates stayed green. Each half asserted its own side
and was right; the join is an object literal in `App.svelte`, which no unit test imports
and which `viewer_check.py` does not reach, because that harness builds its own `Viewer`
with no model behind it.

```
scripts/mark_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/links.pdf
```

It takes the **binary**, not the bundle --- there is no Launch Services route here and the
document is handed over in `argv`. Inside a bundle it must still be the one under
`Contents/MacOS/`, because WKWebView needs the bundle identity or the page never runs.
`src/lib/markcheck.ts` holds the checks and the argument for each; the one it exists for is
*"and it is recorded on the page it was pressed on"*, which derives the expected page **id**
from the model's own page list at the viewer's own slot and compares it with the id the
model filed the mark under. Under the shipped defect those differed by one on every
document.

Every assertion reads the **model** --- marks that came back over the IPC boundary from
Rust --- and never the viewer that produced the gesture, which is what keeps it from being a
writer agreeing with its own reader. The single exception is the ink reading, whose job is
the last hop a model assertion cannot see.

⚠ **The launch half has never run.** It was written on a machine whose screen was locked,
and `webview_guard` refuses rather than hanging --- correctly, since a suspended WebKit page
does not run the check slowly, it does not run it at all. So **this harness is in the state
`docs/TRAPS.md` warns about: one that has never executed produces no failures, and neither
does one that passes.** What *is* proved is the transcript reader, which needs no screen:

```
scripts/mark_check.py --self-test
```

Seven cases, six of them refusals --- no summary line, a summary disagreeing with the exit
code, a failing summary, a run that never opened a document, a skipped keystone check, and
a name found by prefix rather than by column. Run the real thing on an unlocked screen
before trusting a green line from it, and prove it can go red the way every other harness
here was proved: reintroduce the slot lookup in `Edits.mark`, rebuild, and confirm the
page-identity check fails.

### Checking session restore

Reopening where the reader left off is a property of a *launch*, so it takes more than one:

```
scripts/session_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

**It runs unmodified on Windows** (2026-07-30), and did on the first attempt --- another
harness this file listed as macOS-shaped that never was. `webview_guard` already returns early
off darwin, and the script takes a binary rather than a bundle, so nothing needed porting:

```
cargo build --release --features tauri/custom-protocol --bin tpdf
python scripts/session_check.py src-tauri/target/release/tpdf.exe testdata/outline-simple.pdf
```

All four phases green, both controls included --- the default state differed in all five fields
from the remembered one, and nothing opened when nothing was remembered. Expect
`Failed to unregister class Chrome_WidgetWin_0. Error = 1412` on each shutdown: that is
WebView2 teardown noise on a *passing* run, not a failure.

**Open, and intermittent: the `default` control can hang instead of running** (Windows,
2026-08-08). It passed twice that day and then timed out three runs in a row, always the same
phase and never any other. What the hung launch looks like from outside is the useful part: the
process is alive, and `MainWindowHandle` is **0** --- it never created a window, so it stopped
before any JavaScript could run and no frontend change can explain it. The shape fits a
single-instance secondary that forwarded its argv and failed to exit; the phase launches
immediately after the previous one's process goes away, and `tauri-plugin-single-instance` is
Windows-only, so this race does not exist on macOS.

It is **not** caused by the print or session threading changes in the same release: the run was
repeated with those stashed and the transcript is identical line for line, through the record
phase, the file inspection and the hang. Three of four phases pass either way, `verify` --- the
one that actually tests restore --- at 8/8. Two things to do before chasing it: kill any stray
`tpdf.exe` first, since one alive process changes what every later launch does, and check
`MainWindowHandle` rather than assuming the app got as far as its checks.

**The fixture must have at least eight pages.** The target page is 7 and `Viewer.goToPage` clamps
to the last page, so a shorter document reports a wrong page rather than a wrong fixture ---
`text-base14.pdf` gave *"page 0, wanted 7"*, stably, on a restore that was working. There is a
named check for it now (*"the document is long enough to test page restore"*), so the run says
which it is. `outline-simple.pdf` above has 12 pages; `incr-scan-20p.pdf` has 20 and renders
faster than the A0 fixtures.

**And the run now stops there rather than colouring the rest of the transcript** (2026-07-30).
The named check alone did not settle it: it fails inside the `record` phase, and the driver
launched the other three regardless, so a short fixture still produced eleven failures of which
ten were `it opens on the remembered page: page 0, wanted 7` --- the signature of a broken
restore, below the line that said otherwise, and these harnesses are read from the tail. The
driver reads that check's verdict out of the transcript, skips the remaining phases by name and
ends with `[FAIL] session restore was not tested: <fixture> has too few pages ...`. Measured on
`text-base14.pdf`: eleven failures to one, three launches not made, exit code still 1.

The check's name is duplicated into `session_check.py` to do that, which is a coupling rather
than an assertion --- so a transcript that does not contain it is reported as a failure of the
script, not read as "the fixture is fine". Proved by renaming it: a green run turns red with
*"this script cannot find a check named ... it has been renamed in sessioncheck.ts"*.

**Start from a clean process table, and this is not advice.** A leftover `tpdf.exe` hangs the
next run outright: reproduced twice on 2026-07-30, where the launched app sat at **0.00 CPU**
for minutes and no phase produced a summary, and both times it passed immediately after
`Get-Process tpdf,python | Stop-Process -Force`. Same shape as the occlusion warning below for
`open_check.py`, and worse here because `webview_guard` returns early off darwin, so **nothing
guards it on Windows** --- Chromium suspends an occluded page exactly as WebKit does. The tell is
the CPU figure, not the clock: a child holding 0.00 CPU is hung, and a run that is genuinely
working through four launches is not. Check that before extending a timeout.

Four launches, and the two labelled `control:` are what make the other two mean anything:

| phase | session | argument | asserts |
|---|---|---|---|
| `record` | fresh | a document | drives to page 7, one quarter turn, a fixed zoom, sidebar open --- then writes it |
| `control: opening without a session` | empty | a document | that state is **not** where the app opens by itself |
| `verify` | recorded | none | the app came up in that state, told only by the file |
| `control: launching with nothing remembered` | empty | none | no document opens when nothing is remembered |

Without the first control, "restored to page 7" is satisfied by an app that happens to open
there --- the same shape as a check whose precondition is already satisfied, which this
repository has paid for four times. It fails if *any* of the four fields already matches,
not only if all of them do: a restore that got only the rotation right would otherwise hide
behind a default that shared the page. Without the second, an app that reopened the last
file it could find by some other route would pass `verify` perfectly.

Between the phases the script reads the written `session.json` itself. Writing a place and
reading one back are different halves, and a run that only did the second would find nothing
to restore and report that somewhere else entirely.

Unlike every other harness here, **this one does not replace the application** --- it boots
normally and observes itself, because restoring is part of the boot and a check that drove
`session.ts` directly would be a second implementation agreeing with the first. Same bundle
and unlocked-screen requirements as the viewer check.

Every launch gets its own `TPDF_SESSION_FILE` in a temporary directory, and **the two
controls get one each rather than sharing**. Shared first, and the second control failed:
the first control opens a document, which is what it is for, so by the time the second
launched there was something to restore and a document duly opened. A control is the thing
you assume is inert, which is why the standing rule about what one phase leaves behind for
the next did not fire.

Unlike the viewer check, **the exit code here is meaningful** --- see the note below.

### Checking file associations

A PDF reaches tpdf three ways and they share almost no code, so this drives all of them:

```
scripts/open_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app testdata/text-heavy.pdf \
    --other testdata/outline-simple.pdf
```

Note it takes the **`.app` bundle** on macOS, not the executable inside it: two phases go
through Launch Services and there is nothing else to hand `open`. **On Windows it takes the
executable**, built with `--features tauri/custom-protocol`:

```
python scripts/open_check.py src-tauri/target/release/tpdf.exe \
    testdata/outline-simple.pdf --other testdata/rotated-90.pdf
```

Four of the six phases run there and pass --- `argv`, `beats`, `control`, and all four launches
of `race`. The two that cannot print `[SKIP]` **with the reason**, so the phase-name list is the
same on both platforms and a reader can diff it:

- `double-click` has no second mechanism to test. An Explorer double-click hands the path over
  in argv, which `argv` already covers; there is no Launch Services layer to go through.
- `running` has no route at all. `RunEvent::Opened` is `#[cfg(target_os = "macos")]` and no
  single-instance plugin is linked, so **a second launch is a second process** --- measured, not
  inferred: two launches leave two `tpdf.exe` processes with two windows and two worker pools,
  where macOS produces one app that swaps documents. Whether that is the behaviour to want is a
  product decision; what is certain is that the *emit* branch this phase exists to exercise is
  unreachable there, and that was previously unstated in either direction.

`HANDS_OVER_TO_RUNNING` is the single place that distinction lives, and each of the two
branching phase names is a constant rather than a literal at both call sites --- a name written
twice eventually differs, and the diff then shows a check that vanished on one platform when
nothing had.

| phase | delivery | asserts |
|---|---|---|
| `argv` | the binary, with a path | the terminal and Windows double-click route |
| `double-click` | `open -a` on a cold app | the Apple Event, which is how macOS actually does it |
| `beats` | argv, with a different document remembered | a handed-over document wins |
| `control` | nothing handed over | the remembered one opens --- without this, `beats` passes on an app that ignores the session |
| `running` | `open -a` on an app already up | the *emit* branch rather than the queue |

`running` is the only phase that would notice the frontend and the backend disagreeing about
the event's name, and it carries its own control: nothing may be open before the document
arrives, or "a document arrived" is satisfied by one that was already there.

**The environment does reach an app that Launch Services started** ---
`TPDF_OPENCHECK=… open -a tpdf.app file.pdf` propagates --- which is what makes the
double-click phase testable rather than merely argued. Both `open` phases capture the app's
stdout with `open --stdout`.

Same bundle and unlocked-screen requirements as the viewer check, and one extra: **leftover
tpdf windows occlude new ones**, and an occluded page never runs, so a phase produces no
output at all. `pkill -f "tpdf.app/Contents/MacOS/tpdf"` before a run, or `TPDF_RAISE=1`.
This cost real time once already --- it looked exactly like the failure it was sitting next
to, which was genuine.

### Checking the recent documents the shell is told about

Two lists, unrelated despite the name: `src/lib/recents.ts` is tpdf's own, shown in
the command palette; the shell's is the Windows Jump List and macOS's *Open Recent* and
Dock menu, filled by `recentdocs.rs` and by nothing else the application does.

**Windows: look at the file the shell writes.** `SHAddToRecentDocs` drops a shortcut per
document, so open one and look.

```powershell
ls "$env:APPDATA\Microsoft\Windows\Recent\*.pdf.lnk"
$s = New-Object -ComObject WScript.Shell
$s.CreateShortcut("$env:APPDATA\Microsoft\Windows\Recent\x.pdf.lnk").TargetPath
```

Resolve one --- an entry existing is not an entry that opens. Note a Jump List needs an
*installed* build (a Start Menu shortcut is what gives the app an AppUserModelID), so a
binary from `target\release` will look as though this does nothing.

**macOS: there is no file, and every place you would look says the feature is broken.**
Measured 2026-08-20: `defaults read com.timostein.tpdf NSRecentDocumentRecords` does not
exist and never will (pre-Sierra location); `sfltool list-info` hangs; and
`~/Library/Application Support/com.apple.sharedfilelist/` answers `Operation not
permitted`, so what is in it is unknown --- do **not** run that `ls` with `2>/dev/null`,
which turns the refusal into a convincing `total 0`.

So the check is **two launches**, which is the feature rather than a proxy for it. It
needs a bundle --- `npm run tauri build -- --bundles app` --- because a bare binary has no
identifier to key a list to.

```bash
APP=src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
pkill -f "tpdf.app/Contents/MacOS/tpdf"
TPDF_RECENTDOCS_PROBE=1 "$APP" "$PWD/testdata/text-heavy.pdf" 2>&1 | grep recentdocs
# quit it, then:
TPDF_RECENTDOCS_PROBE=1 "$APP" "$PWD/testdata/rotated.pdf" 2>&1 | grep recentdocs
```

The first launch must print `before filing, AppKit holds 0` and the second
`before filing, AppKit holds 1` naming `text-heavy.pdf` --- a document the second process
never filed. That carry-over is the whole assertion; a second launch holding 0 means the
call is being dropped. The probe is off unless the variable is set, so a shipped run does
not narrate its own menu bookkeeping into the one log a reader sends back.

### Checking that a Save reaches the disk

macOS only, and it needs a built bundle and an **unlocked** screen:

```bash
scripts/save_check.py                                  # the release bundle
scripts/save_check.py path/to/tpdf.app testdata/outline-simple.pdf
```

It copies the fixture to a temporary directory, opens it, and drives the real
menu --- `Page > Rotate page clockwise`, then `File > Save`, then a highlight over
the page's own text and a second Save --- reading the *file* back each time by
digest and through `qpdf --check`, which shares no code with anything here.

**It exists because nothing else in the repository writes a file.**
`viewer_check.py` lists `file.save` as undriven with the reason (it would write
over the corpus fixture the rest of that run is reading), `save.rs`'s tests build
their plans directly, and `edits.test.ts` asserts the shape of the `invoke` call.
So when the 26.8.6 release commit recorded saving as "reported broken from the
running application", nothing here could test that claim --- and put to its
author on 2026-08-21, it turned out he had never said it. The provenance is
recorded rather than quietly deleted, because the lesson is not about saving: an
unattributed sentence in a commit message became an open item, a paragraph in
this file and two harness docstrings, and none of it could be checked until there
was a check.

The control runs first and is the reason the rest means anything: Save must be
**withheld** on a document with no edits. It also asserts Save greys again after
the save --- the reopen has to produce a clean document --- and that nothing is
left in the directory, since staging writes a sibling and renames it, so a stray
is a commit that failed and said nothing.

**A locked screen is refused, not skipped and not survived.** The web view is
suspended while the session is locked, so the document never opens and every menu
item stays greyed, which reads exactly like an application ignoring its own menu.
The check reads `CGSSessionScreenIsLocked` first and exits 2 saying so.

First full run 2026-08-21, on 26.8.6 plus that day's commits: **10 checks, all green,
25 s** --- Save withheld at rest, a rotation offering it, the file changing (10731 -> 10400
bytes), `qpdf` reading it back, Save withheld again after the reopen, a highlight saving too
(-> 14542 bytes), and nothing left beside the document. Its failure path was proved
separately by pointing a phase at a menu item that does not exist: `[FAIL] this check drives
a menu item that is not there`, exit 2, and no claim about saving either way.

So there is **no defect to reproduce here**: saving over the open document works from the
menu, twice in a row, for two different kinds of edit, in a scratch directory and in a
TCC-protected one (`~/Downloads`), on this machine. That is a statement about this route and
this fixture. Every refusal `save.rs` states needs a condition a clean local file does not
have --- an encrypted document, a file changed under the open one, a missing baseline --- and
each of those has tests of its own; what none of them can tell you is whether the refusal
fires when it should not, which is what a real report of a spurious message would be for.

### Checking the menu bar

macOS only, and it needs a built bundle and an unlocked screen --- but no document, no
fixture and no window of its own beyond the app's:

```bash
scripts/menu_check.py                                  # the release bundle
scripts/menu_check.py path/to/tpdf.app
scripts/menu_check.py --self-test                      # the rule, without a build
```

**Run it after touching `menu.rs`, `menubar.ts`, or any command's `title`.** It reads the
live menu bar through System Events and asserts three things: that the read returned menus at
all, that no two items in one menu carry the same name, and that the bar is exactly the
menus `menubar.ts` declares in that order, plus the predefined `Window` that `menu.rs`
appends.

It exists because on 2026-08-21 the application menu carried **two items named "About tpdf"**
and nothing in either language could have said so: the platform's items are never named in
our source, and ours arrive over IPC as data, so the only place both lists exist at once is
the bar. `docs/TRAPS.md` has the entry, including why our About was the one kept.

**It launches with `open`, deliberately** --- not as a subprocess with pipes. A harness that
captures output supplies a stdout and a stderr that a double-clicked application does not
have, which is the trap that hid the Windows open defect for a month; this check has no
reason to differ from the reader's launch, so it does not.

Proved in both directions against real binaries rather than against a fixture: rebuilt from
`git checkout -- src-tauri/src/menu.rs` it reports the duplicate and exits 2, and with the fix
it exits 0. `--self-test` carries both measured menus so the rule can be shown to fire in a
second, and it is not a substitute for the run: it tests the predicate, not the menu.

It is **not** a gate, for the same reason `viewer_check.py` is not --- an accessibility read
needs a real session, and on a headless runner it would not fail, it would hang.

### The exit code of a spike run

`AppHandle::exit(code)` does **not** set the process's exit code. It ends the event loop,
`App::run` returns normally, `main` returns unit, and the process exits 0 whatever was asked
for. Every automated run here therefore reported success through `$?` for its whole
existence, `viewer_check.py` included. Fixed 2026-07-27 in `spike_exit`, which now flushes
and calls `std::process::exit`.

If you add a harness, do not let the exit code be its only verdict --- parse the transcript
too, and make the two agree. That is what caught this: a run printing `[OK] session restore
verified` directly beneath a phase whose own last line said `0/1 checks passed`.

### The four Phase 0 spikes nothing above invokes

`AGENTS.md` says this file has the invocations, and for four `[[example]]` targets it did
not --- measured 2026-08-28 by diffing the `[[example]]` names in `src-tauri/Cargo.toml`
against this document: 40 targets, 4 unnamed. They are the oldest spikes, they answered
their question once, and their answers are load-bearing in `docs/PLAN.md` --- which is
exactly why they should still be runnable rather than quietly rotting into files nobody
knows how to start.

```bash
# Spike 0.3, the gating one: can one text object be edited and the rest of the page
# reproduced faithfully? Two routes, PDFium and lopdf, measured against each other.
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-roundtrip

# Spike 0.6: does an appended update section satisfy a reader that is not ours?
cargo run --release --manifest-path src-tauri/Cargo.toml --example incremental-save

# Does `pdfium-render`'s `thread_safe` actually serialize PDFium? The whole
# worker-process architecture rests on the answer, so it is measured rather than cited.
cargo run --release --manifest-path src-tauri/Cargo.toml --example thread-probe

# Can a worker be handed its document *after* it is sandboxed, over a socket? This is
# what a pre-spawned worker would need, and it is why the ~6.6 ms floor sits where it does.
cargo run --release --manifest-path src-tauri/Cargo.toml --example fdpass-probe
```

Each prints its own verdict and exits non-zero on failure. `text-roundtrip` and
`incremental-save` need `testdata/`; `fdpass-probe` is macOS-only.

---

## Cutting a release

Version scheme is **CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO
starts at 0 and increments within the month.

1. `git fetch` and confirm the local branch is not behind --- this repo is pushed from more
   than one machine, and a version bump on a stale clone has already cost a re-cut release
   elsewhere in the portfolio.
2. Bump **all four** version files so they agree:
   - `package.json`
   - `package-lock.json` (top-level *and* the root package entry --- `npm version <v> --no-git-tag-version` does both)
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
3. `cargo check --manifest-path src-tauri/Cargo.toml` to refresh `Cargo.lock`.
4. In `CHANGELOG.md`, replace `Unreleased` with the release date.
5. `scripts/gates.py` --- all gates pass.

   **On a Mac, also `scripts/check_windows.py`, and it is not optional before a tag.** A
   green gate list on this platform says nothing about any `#[cfg(windows)]` line, because
   the compiler never parses one: `print_win.rs`, `examples/print_probe.rs`,
   `examples/win_sandbox_probe.rs` and the Windows halves of `worker*.rs` are all outside
   what the gate list covers --- the two figures below are 15/15 because that is what the run
   was at the time, and it is 17/17 since 2026-08-22; the gap is the same one whatever the
   count, which is why the figures are left as the runs reported them. Cutting `26.8.3` proved the gap rather than predicted it --- the
   page-move work changed `print::Pages::Only` from `Vec<u32>` to `Vec<PagePlan>` and missed
   the one Windows-only caller, and sixteen commits went by at 15/15 before a rehearsal tag
   turned both runner legs red. That leg reported *four* failures, since clippy, test and
   bins all stop at the same `error[E0308]`.

   ⚠ **If it does not return in about a minute, it is wedged rather than slow --- kill it and
   run it again.** On 2026-08-27 it sat for 15 min 45 s with its log frozen at the banner,
   and the same command on the same tree finished in **21.83 s** a minute later. The
   instrument is CPU time, not elapsed: `ps -eo pid,etime,time,args | grep
   "[x]86_64-pc-windows-msvc"` showed 2 s of CPU across sixteen minutes. Add `--verbose`
   while diagnosing --- output is captured and shown only on failure otherwise, which is
   exactly wrong for a run that never ends. See the trap of that name.

   ⚠ **That minute is the warm figure, and a change to a widely-included module makes the
   run minutes long rather than seconds --- so the rule above will tell you to kill a healthy
   run.** Measured 2026-08-28 after editing `ocr_gate.rs`: **~4 minutes** cold against **1 s**
   warm on the very next invocation, both green. Two things follow. Judge by CPU, never by
   elapsed, exactly as the paragraph above says --- but **use the `grep` form it prints and
   not `ps -p <pid>` on the process you happen to have**, because cargo and `cargo-clippy` are
   both near zero on a perfectly healthy run and all the work is in the `clippy-driver`
   children. Reading the parent is what nearly cost a good run here. And expect the cold cost
   whenever the edit was to a module the whole crate includes: the Windows target has its own
   `target/x86_64-pc-windows-msvc` tree, so it recompiles independently of everything the
   gates just did.

   It is `cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings` with
   the environment that command needs, and it does not link, so no MSVC linker is involved.
   **It was `cargo check` until 2026-08-20**, and the difference is a whole class of
   failure: dead code is not a type error, so a constant read only from a
   `#[cfg(target_os = "macos")]` function passed here and failed `windows-2025` as
   `constant TEXT_SIZE is never used`. 16/16 on the Mac, 15/16 on the runner, clippy the
   only red one --- found by the `v26.8.6-rc1` rehearsal tag at a cost of a 25-minute round
   trip. `-D warnings` is exactly what the `clippy` gate denies, so the two legs now agree
   about what counts as a failure. See the trap of that name.

   **It has three costs, not one, and this file recorded only the middle one.** Measured
   2026-08-21:

   | tree state | cost |
   |------------|------|
   | nothing changed since the last run | **0.47 s** |
   | local Rust changed, dependencies did not | the **~8 s** this file used to quote |
   | after a dependency change | **2 min 58 s**, measured adding three crates |
   | first run for the triple, or after `cargo clean` | **minutes**, and not cleanly measured |

   The bottom two rows are where the surprise lives: clippy *compiles* the dependency tree
   rather than checking it, and a `--target x86_64-pc-windows-msvc` build shares nothing with
   the host one, so a fresh checkout, a `cargo clean` or a `Cargo.lock` bump pays for the whole
   tree again. The 2 min 58 s is real and was taken with nothing else running --- it is what
   adding `cms`, `x509-cert` and `der` cost on 2026-08-21. **The last row is a ceiling rather
   than a measurement**: the run that reached fourteen minutes had a second copy of itself
   contending for cargo's build lock, which is the caveat that matters more than the number.
   Only ever run one at a time; the script captures cargo's output and prints at the end, so
   `Blocking waiting for file lock` is swallowed and two runs look exactly like one slow one.

   Schedule from the third row, not the first: start it when the Windows-relevant work is
   done and let it run while you do something else. Against a CI round trip of six minutes.
   One-time setup, which the script names in
   full if anything is missing rather than failing four times in a row:

   ```
   brew install xwin llvm
   xwin --accept-license --arch x86_64 --variant desktop splat --output ~/.xwin
   scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
   cp /tmp/pdfium-win/bin/pdfium.dll vendor/pdfium/bin/pdfium.dll
   ```

   The DLL is the one that reads as something else: Tauri resolves `bundle.resources` for
   the *target* platform, so without it the build script dies on `resource path ... doesn't
   exist`, which looks like a broken checkout. `vendor/` is gitignored and nothing on macOS
   loads it. The splat is 629 MB, which is why this is not a gate.

   **What it does not say**: only that the Windows tree type-checks and lints. A wrong
   *value* passes --- proved, by changing a `PagePlan`'s turns and watching it stay green.
   Linking, loading and behaviour are still the runner's to find. **The general form is
   worth carrying to any stand-in for another platform: it is only as strong as the command
   it runs there, never as strong as the target it names**, and anything the real gate list
   does that the stand-in does not is a class of failure it cannot report while reading as
   coverage.
6. **Re-check `docs/THREAT-MODEL.md` against the code**, and correct the document before
   trusting anything else in this list --- §3's boundary table, §5's sandbox policy and
   §6's macOS column especially. Every present-tense sentence there claims something is
   *wired*, and a mitigation stated in prose and enforced nowhere reads exactly like one
   that holds: three consecutive review rounds each found at least one claim that had
   quietly become a description of an earlier phase, and the third of them was the CPU and
   memory bounds in §T3. §8 lists the probes that answer the mechanical half
   (`worker-probe`, `backend-probe`, and `worker-bench --mode engine|authority` after any
   PDFium bump). The half no probe covers is reading each claim and naming the line that
   keeps it. Anything that turns out not to be wired gets wired or gets marked, never left.

   **And re-read `README.md`**, which is the same job on the document a stranger reads.
   Since 2026-08-24 `src/lib/readme.test.ts` does the mechanical half in both directions:
   nothing under *Not built yet* may be registered, and every registered command is either
   claimed by a `<!-- built: -->` marker in the prose or excluded there with a reason. So a
   new command can no longer arrive unmentioned, and this step no longer has to diff the
   registry by eye.

   **What it still cannot touch is the status paragraph, which is where the worst of it
   was**: on 2026-08-22 that paragraph said editing had just begun and that *the open file
   is never modified in place*, six weeks and one shipped Save-in-place after either was
   true. Nor does a `built:` marker say the prose beside it is accurate --- only that the
   command is claimed somewhere a reader will look, so a bullet describing a command wrongly
   passes exactly like one describing it well. Read the first three paragraphs, then the two
   feature lists, against what you know shipped this cycle. Do not put a count in the prose:
   every one that was there had drifted, and the files they describe carry their own.
7. Re-run the mutation harnesses. They are not gates --- they rebuild per mutation and two of
   them need a window --- and they are the only thing that says the tests can fail.

   **How much of them to run is a decision, and this step used to duck it.** It read *"if any
   of the code they cover changed"*, which is true of nearly every release and therefore meant
   the full 735 in practice: about 1 hour 40 minutes, of which the viewer table alone is 55.
   Measured over the `26.8.7` cut, that whole spend produced **one** finding. Steps are only
   ever added to a checklist --- this is the first one this file has ever narrowed --- so the
   narrowing states its own trigger rather than leaving it to whoever is tired:

   **Run the FULL tables when any one of these is true.** The first two are the ones that have
   actually paid:

   - the diff touches `src/lib/viewercheck.ts`, or a harness-covered module by more than
     ~100 lines --- `git diff --stat <last tag>..HEAD -- src-tauri/src src/lib`
   - a module joined `FILTERS` or the mutation table since the last release, or a runner did
   - the release adds a capability rather than fixing one
   - nobody has run the full tables in a month

   **Otherwise run the narrow pass**, which is minutes rather than hours:

   ```
   scripts/mutate_rust.py --since <last tag>        # only the mutations whose FILE moved
   scripts/mutate_frontend.py --since <last tag>    # same flag, same meaning
   scripts/mutate_viewer.py --since <last tag>      # and here, where it saves most
   scripts/mutate_viewer.py --runner structure      # the three that need no window,
   scripts/mutate_viewer.py --runner search         # about 24 s a mutation
   scripts/mutate_viewer.py --runner encodings
   ```

   **All three take `--since` as of 2026-08-25**, from one implementation in
   `scripts/mutation_since.py`. This paragraph used to say the flag existed on `mutate_rust.py`
   only and called giving it to the other two *the obvious next piece of work*; that is done.
   Measured against `HEAD~1` of the commit that closed it: **48 of 363** Rust mutations,
   **63 of 432** front-end, **1 of 92** window.

   ⚠ **It had never once worked on Windows, and the reason is one character.** The Rust
   harness built its key with `str(Path("src-tauri") / m.path)`, which is `src-tauri\src\...`
   there, while `git diff --name-only` reports forward slashes on every platform --- so the set
   membership test matched nothing and `--since HEAD~1` over a commit that changed
   `docinfo.rs`, which **48** mutations aim at, selected **0**. What made that cost nothing is
   the guard beside it: an empty selection is refused with *"this run proved nothing, which is
   not the same as a green table"* and exit 1, so the flag was unusable here rather than
   quietly certifying an empty run.

   ⚠ **The narrow pass is still genuinely partial, and `--since`'s reach is shorter than its
   scope.** A mutation is selected by the file it edits; a change in one module can stop a
   mutation in another from being caught without that other file appearing in any diff. Each
   run prints what it left out --- the count against the table's total, and the changed files
   no mutation aims at --- and ends by saying it is not the full table. That is still the thing
   to run before a release that qualifies above.

   ⚠ **A `--runner` run validates only that runner's mutations.** So the narrow pass cannot
   report a mutation registered against the wrong runner --- which is exactly what `26.8.7`
   shipped and what the full table refused to start over. If the narrow pass is what you ran,
   the table's own consistency is unverified, and `scripts/gates.py`'s `anchors` gate covers
   the anchors but not the runner assignment.

   **The full tables:**

   ```
   scripts/mutate_rust.py          # the modules in FILTERS, `cargo test --lib`
   scripts/mutate_frontend.py      # the modules under src/lib, `vitest`
   scripts/mutate_viewer.py        # every runner below, in one pass

   # What each costs, measured 2026-08-21 when 26.8.7 was cut. Scale by the
   # PER-MUTATION figure, never by the total -- the totals move whenever
   # somebody writes a mutation, and `--list` is the only authority on them:
   #
   #   mutate_rust.py       292 mutations, ~2.9 s each  -> about 14 minutes
   #   mutate_frontend.py   368 mutations, ~4.9 s each  -> about 30 minutes
   #   mutate_viewer.py      75 mutations               -> 50 min (3010 s measured)
   #                        8 of those need no screen at 23-24 s each; the
   #                        other 67 rebuild the bundle at 37 s each, plus
   #                        78 s per runner for its baseline and clean rebuild
   #
   # A whole-table viewer run validates ALL TEN baselines first, in
   # alphabetical order, before it mutates anything -- so the first seven to
   # ten minutes print only "Baseline: building and running the <name>
   # harness" and no [CAUGHT] line at all. That is the run working, not the
   # run stuck, and it is worth knowing before somebody kills it: a harness
   # that has not reached its first mutation looks exactly like one that
   # cannot.
   #
   # The Rust figure is wall clock over the whole run INCLUDING its one cold
   # build, and it disagrees with the 405 s / 229 measured below -- 2.9 s
   # against 1.8 s, unexplained, on the same machine two hours apart. The
   # front-end one is that section's per-mutation figure and was NOT
   # re-measured here. Both are said plainly rather than averaged into one
   # confident number: a timing whose provenance is a mixture is worth less
   # than either of the two it came from.
   #
   # See "What the Rust table costs" below, and read it before believing any
   # older figure in this file. A backgrounded run is still worth
   # waiting on by the signal the job emits rather than by asking the process
   # table whether it is alive:
   #
   #   scripts/mutate_rust.py > run.log 2>&1; echo "exit=$?" >> run.log
   #   until grep -q '^exit=' run.log; do sleep 60; done
   #
   # `until ! pgrep -f mutate_rust.py` is the wrong instrument twice over, and
   # `docs/TRAPS.md` has both halves.

   # All three take `--only <substring>`, matched against the mutation's name,
   # for the loop while a change is being made: `--only pagetree`, `--only
   # "page delete"`. The whole table is what runs before a push --- the flag
   # exists because re-proving a hundred mutations that could not have moved is
   # somebody waiting, not because a subset is ever the gate.

   # All three also take `--since <ref>`, which needs no knowledge of the
   # mutation names: it runs the ones whose FILE the diff touched, working tree
   # included, prints how many it left out and which changed files no mutation
   # aims at, and exits 1 rather than looking green when it selected nothing.
   # Its reach is shorter than its scope --- a change in docmodel.rs can stop a
   # mutation in save.rs from being caught --- so it is the loop, and the whole
   # table is still the thing before a push.
   scripts/mutate_rust.py --since HEAD~3
   scripts/mutate_frontend.py --since HEAD~3
   scripts/mutate_viewer.py --since HEAD~3

   # And all three take `--resume`, which is about a run that DID NOT FINISH
   # rather than about narrowing one. Two backgrounded frontend runs were killed
   # at about twenty-five minutes on 2026-08-30 by something neither the operator
   # nor the harness can account for, and each cost four hundred proved verdicts
   # AND left its mutation in the tree. `scripts/mutation_resume.py` answers both.
   scripts/mutate_frontend.py --resume
   scripts/mutate_viewer.py --runner viewer --resume

   # Or one runner at a time. The three probe runners need no webview, no bundle
   # and no unlocked screen; the three viewer ones need all three.
   scripts/mutate_viewer.py --runner structure          # structure.rs, structure-probe
   scripts/mutate_viewer.py --runner search             # search.rs + text.rs, search-probe on multilingual.pdf
   scripts/mutate_viewer.py --runner encodings          # text.rs, search-probe on encodings.pdf
   scripts/mutate_viewer.py --runner viewer             # appcommands/search/results/viewercheck.ts + search.rs
   scripts/mutate_viewer.py --runner viewer-tagged      # a11y/reading/viewercheck.ts, viewer_check on tagged.pdf
   scripts/mutate_viewer.py --runner viewer-encodings   # a11y/search.ts, viewer_check on encodings.pdf
   ```

   **`--resume` is two halves, and only the second one is behind the flag.**

   *Recovery runs on every invocation.* Before the control run, before any baseline and
   before the fingerprint, each harness reads `.mutations/<harness>.json` and asks what the
   last run left behind. The record is written **before** the mutated bytes reach the file, so
   a kill in that window leaves a record and a clean file rather than a mutation nothing
   names. The answer is by digest and has three branches: the file is what the run started
   from (nothing to do, and it says so --- silence there is indistinguishable from the check
   not having run), or it is the mutation that run wrote (restored from the backup beside the
   record, verified), or it is neither, in which case somebody has edited it since and the run
   **refuses and exits 1**. A refusal is right: clobbering a repair made by hand is worse than
   the mutation it would undo. Delete `.mutations/<harness>.json` to clear it.

   *Reuse needs the flag.* Verdicts already proved are reused only if the tracked tree
   fingerprints identically --- `HEAD`, the full `git diff HEAD --binary`, and every
   untracked-but-unignored file's digest. Any edit at all discards all of them, and the run
   says which of the two happened. That is blunt on purpose: a mutation's verdict is a claim
   about the whole suite, so an edit anywhere can move it --- and the practical consequence is
   worth knowing before it surprises you: editing a document, or one of the harnesses, throws
   the verdicts away exactly as editing `search.rs` does. Finish editing, then resume. A
   reused line is printed with `[reused]` on the end and the summary states how many came from
   an earlier process.

   `mutate_viewer.py` also skips the baseline for any runner whose every mutation is reused
   --- that baseline was taken against this same tree when the verdicts were, so building and
   running it again costs about 78 s and establishes nothing. A fully reused
   `--runner structure --only ...` run measured **0.14 s against 75 s**.

   **Two things it does not cover, both said rather than papered over.** The fingerprint
   cannot see gitignored inputs --- the generated corpus under `testdata/`, `vendor/pdfium/`,
   `node_modules/`. `mutate_viewer.py` closes the largest part of that by handing the
   fixtures its chosen runners open to the fingerprint explicitly; the other two do not, so
   regenerating the corpus between a kill and a resume is a reason to drop the state. And the
   verdicts are kept whatever the flag says, so a narrow `--only` run in the middle of a
   killed table adds one verdict rather than destroying the rest --- the first draft wiped the
   file on every plain run, which made the feature useless in exactly the workflow it is for.

   ```
   python3 scripts/mutation_resume.py --self-test   # 24 checks, about 1.5 s
   ```

   Its 13 mutations were run on 2026-08-31 and all 13 were caught by the check named for
   them. Three of the findings are in `docs/TRAPS.md`: a check that read the state file
   directly **raised** under the mutation aimed at it and printed no named failure; the
   failures were collected and printed at the end, so that crash took ten already-found
   failures with it; and a check for "an edited anchor is a different mutation" passed under a
   mutation that broke the key, because a case two above it had emptied the store on purpose
   and both lookups therefore missed.

   **How many mutations each carries is `--list`, not this page.** It said 23, 85 and
   15 on 2026-08-03 against an actual 36, 98 and 31 --- a tally in prose, in the one
   document whose job is to schedule the run, and nothing could go red about it. The
   module names above are the invariant; the counts are a property of the table and are
   printed by `--list` in the shape `<name> -> expects: <test>`.

   **Which modules each covers is `FILTERS` and the mutation table, not this page either.**
   The line above said `search/text/structure/encoding.rs` while the harness covered ten
   modules, which is the same defect as the counts below it and in the half the page calls
   the invariant. Read them out of the scripts:

   ```
   python3 -c "import re,pathlib; print(re.search(r'FILTERS = \[(.*?)\]', pathlib.Path('scripts/mutate_rust.py').read_text(), re.S).group(1))"
   ```

   `mutate_rust.py` filters on those module prefixes, and libtest takes several and ORs them
   --- but only after `--`. `cargo test --lib a:: b::` is cargo's own argument error, which
   reads like the feature being unsupported.

   `mutate_viewer.py` drives **ten** runners, chosen per mutation and filterable with
   `--runner`. The `structure`, `search` and `encodings` ones need no webview and no bundle, so
   they neither wait for one nor require an unlocked screen; each rebuilds one example and runs
   it, at a measured **23--24 s** a mutation. Every runner prints the same `[FAIL] <name>` lines
   and the same summary, so the cross-check, the byte restore and the name validation are
   shared rather than copied. `RUNNERS` in the script is the list; the three probe runners
   share `search-probe` and differ only in the fixture they open.

   **That said seven and "all six" in one paragraph, and both were wrong** --- corrected
   2026-08-21 by asking the script rather than by reading the page:

   ```
   python3 -c "import re,pathlib; print(re.findall(r'\"([a-z-]+)\"\s*:', re.search(r'RUNNERS\s*[:=].*?\n\}', pathlib.Path('scripts/mutate_viewer.py').read_text(), re.S).group(0))[::3])"
   scripts/mutate_viewer.py --runner <name> --list | grep -c 'expects:'
   ```

   Two numbers in one sentence disagreeing with each other is the cheapest possible tell that
   neither was measured, and it sat here through several increments that added runners. The
   split as of 2026-08-21: `viewer` 47, `viewer-tagged` 12, `viewer-mixed` 3, `viewer-encodings`
   2, `viewer-comments` 1, `crop-rotated` 1, `crop-content` 1 --- 67 needing a window --- against
   `structure` 4, `search` 2, `encodings` 2, which do not. Read it from `--list`, not from here.

   `viewer-tagged` is the viewer harness against `tagged.pdf`, and it exists because the two
   tagged-reading-order checks `[SKIP]` on every other corpus. `viewer-mixed` was added on
   2026-08-17 for the same reason on a different property: a page carrying its *measured*
   size to wherever it moved is only observable where the pages are different sizes, and
   `mixed.pdf` is the one corpus that qualifies --- everywhere else the layout's estimate and
   the truth are the same number. A skipped check is in the name
   set and cannot go red, so a mutation aimed at one reported **SURVIVED** --- the most
   misleading verdict this harness produces, since it reads as a gap in the checks rather than
   a fixture that does not exercise them. The baseline validation now refuses that case
   explicitly, alongside the zero-match and ambiguous-prefix ones.

   The viewer runner is different in kind and slower for it: it rebuilds the bundle and runs
   `viewer_check.py` per mutation --- a measured **37 s**, with a further **78 s** per runner for
   its baseline build and the clean rebuild afterwards --- a whole-table run measured **3010 s**,
   50 minutes for all 75 --- because what it covers --- the application's
   own command list, the window shortcuts, and the search behaviour that only shows up
   against a real document --- is reachable from neither `cargo test` nor `vitest`. It needs
   an unlocked, unoccluded screen for the same reason `viewer_check.py` does. It reads check
   results from **stdout only**: `viewer_check.py` writes its own verdict on the run to
   stderr in the same `[FAIL] ` shape, and counting those as checks is what made its first
   run report all ten mutations as broken.

   Each mutation names the test expected to notice, and a mutation nothing caught is
   reported as a defect in the **suite**. Three properties keep that verdict honest: both
   cross-check the failure count two ways, both treat a run with no summary line as broken
   rather than as a survivor, and both **refuse to start** if a mutation names a test the
   suite does not define --- derived from the runner's own listing, since a name that cannot
   go red reports SURVIVED and reads as a gap in the tests. `--list` prints the pairs without
   running anything.

   **A module absent from `FILTERS` is only half the failure, and the loud half.** The
   guard refuses a run whose mutation names a test it cannot see, which is what caught that
   list being forgotten five times. It cannot catch the sixth shape: a module in neither
   `FILTERS` **nor** the mutation table. `fingerprint.rs` was that on 2026-08-19 --- nothing
   refused to start, because nothing was aimed at it, and its central comparison turned out
   to be provable by nothing. When a module lands, add it to `FILTERS` *and* write a
   mutation, and do not wait for the guard to ask.

   **All three run on Windows as of 2026-08-19. Two did as of 2026-07-30 --- 22/22 and
   75/75 --- and neither did before that.** Read the first sentence as dated too: it is the
   second one that expired without anything going red, because `mutate_rust.py`'s table grew
   two macOS-only mutations on 2026-08-17 and the guard that validates test names then
   refused the whole run here. The three defects behind 2026-08-19 are each in
   `docs/TRAPS.md`, and the shape they share is worth more than any of them: **a harness that
   has never run on a platform produces no failures there, and neither does one that passes.**

   `mutate_viewer.py` had never completed a run on Windows at all, for two independent
   reasons that had to be fixed in order. Its five probe runners named their binaries as
   relative forward-slash paths, which `CreateProcess` refuses --- so the run died on the
   first baseline it reached, before any mutation, with a `FileNotFoundError` naming nothing
   in this repository. Underneath that, it read bytes without normalising newlines, so on a
   CRLF checkout every multi-line anchor in its table matched **zero** times; the `anchors`
   gate could not warn, because it reads with `read_text()` and that translation makes the
   same anchors match. Both fixed, and the whole table then ran here for the first time:
   **59/59 caught, 0 survived, 0 unreadable**, about an hour including the nine baselines.

   `mutate_rust.py` had never started here at all: it read each target with `read_text()`,
   whose locale codec on Windows is cp1252, and `search.rs` holds characters whose UTF-8
   encoding contains the byte `0x81`, which cp1252 leaves undefined, so it raised
   `UnicodeDecodeError` on the first mutation. `mutate_frontend.py` ran and reported three
   anchors it could not find, because the same mis-decoding hid the glyphs in them. Both now
   read bytes and decode UTF-8, normalise newlines **for matching only** against a CRLF
   checkout, and restore from the backup as bytes. Fixing the encoding alone took the
   front-end harness from three failures to twelve, because the discarded `read_text` had
   been quietly translating line endings for the anchors that span lines --- the trap of that
   name has it, and it is also a correction to what an earlier entry prescribed.

   **`mutate_rust.py` then stopped running here again, and the guard that stopped it was
   right.** `menu.rs` and `keylayout.rs` are macOS-only, so `cargo test` never compiles them
   and the two mutations aimed at them name tests that do not exist on Windows --- which the
   name validation reports exactly, and then refuses the whole table over. Correct and total
   are different properties: two mutations could not run, and 178 did not. `Mutation.only_on`
   declares the scope, those two print `[SKIP] ... macos only, and this is windows`, and the
   count rides on the final verdict so a partial run cannot read as a whole one. **A mutation
   with no `only_on` still refuses**, which is the property that had to survive; both
   directions were proved by control before the fix was trusted. Measured here on 2026-08-19:
   **all 176 caught by the test named for them, 2 skipped**, about ninety minutes.

   So a Windows run of that table reports 176 and a macOS run reports 178, and the two rows
   the difference names are printed rather than absent. Do not "fix" the 176 into a 178.
   (Those are counts of that date's table, which was 231 on 2026-08-21 and **292** later the
   same day, after the signature work. A parenthetical carrying a count is the shape this file
   keeps getting wrong; `--list` is the authority, and the three tables measured **292 Rust,
   368 front-end, 75 viewer** when `26.8.7` was cut.)

   **What the Rust table costs, and what it used to cost.** On 2026-08-21 a full run was
   measured at **69 s per mutation**, which for 231 of them is 4.4 hours --- a figure nobody
   can pay per feature, and it was almost entirely two things that have nothing to do with
   the mutations:

   - **An editor holding the build lock.** Every mutation writes a file under
     `src-tauri/src`, and rust-analyzer answers each write with
     `cargo check --workspace --all-targets`, which takes the build directory's lock. Cargo
     says so --- `Blocking waiting for file lock on build directory` --- and a no-op
     `cargo test --lib --no-run` measured **28.2 s** against **0.2 s** with the editor idle.
     The harness now sets `CARGO_TARGET_DIR` to `src-tauri/target/mutations`, so it shares
     no lock with anything. One cold build (**42 s**, 2.4 GB, inside the already-ignored
     `target/`) and it is warm for every run after.
   - **607 tests to check one assertion.** Each mutation names the one test it expects to
     redden, and the harness ran the whole filtered suite anyway. Timing the modules
     separately says where that goes: `save::` 32.4 s, `print::` 32.3 s, `keylayout::`
     17.0 s, and **the other fifteen modules 0.1 s between them** --- twelve tests that
     reach PDFKit or HIToolbox, one of which a `sample` shows sitting in
     `TISCopyCurrentKeyboardLayoutInputSource` for its whole run. It now runs the named test
     alone, and the full suite **only** when that test does not go red, which is the case
     where "nothing noticed" and "something else noticed" have to be told apart.

   Measured after both, on the same machine and the same table: **405 s for all 229 runnable
   mutations**, 0 survivors, 2 skipped, and **zero** fallbacks to the full suite. That is the
   number to plan against; every older figure in this file is from before the two changes and
   is left as a statement about its own date.

   **And this one is now such a statement too.** The table was 229 runnable when that was
   measured and is **292** as of `26.8.7`, which reported *all 290 caught, 2 skipped as not
   runnable on macos*. Per-mutation cost is what to carry forward from a timing, never the
   total --- the total moves every time somebody writes a mutation, and nothing goes red when
   it does.

   **It expired without anybody changing the harness, which is the part worth carrying.** By
   `26.8.11` the same table cost **40.0 s** per mutation, measured twice over four minutes ---
   5.6 hours for 508 mutations, against the 1.77 s above. Nothing had regressed in the
   harness: the *crate* had grown, and every mutation touches one file, so cargo re-codegens
   the crate and relinks a test binary that full debug info had taken to 33 MB. A cost that
   goes stale because the subject grew is invisible to every check here, exactly like the
   counts this file keeps getting wrong.

   Fixed the same day by building the mutation target with `CARGO_PROFILE_DEV_DEBUG=0`,
   measured interleaved against the same command in a second target directory --- 22.6/30.1/27.0 s
   against 3.9/3.7/3.3 s, and the directory 1.7 GB rather than 14 GB. The whole table then ran
   in **1316 s including its 178 s cold build**: all 504 caught, 4 skipped as not runnable on
   macos. It is safe because `debug` is debug *information* only --- `debug_assertions` and
   overflow checks are separate knobs and are untouched, so every test runs the program it ran
   before, and what is given up is line numbers in a panic backtrace that nothing here reads.
   The reasoning is in `scripts/mutate_rust.py` beside the constant.

   **Decompose before believing a per-mutation figure.** The no-op freshness check is 0.8 s
   warm; touching one source file costs 14--15 s of it; the named test itself runs in 0.03 s.
   Three measurements said the rebuild was the whole cost, which is what made the lever
   obvious --- and the first theory, that a 33 MB binary was slow to *load*, was wrong and took
   one direct run of the test binary to refute.

   **What the front-end table costs, and why it did not fall as far.** `mutate_frontend.py`
   got the same narrowing --- it runs the test *file* holding the mutation's own test, chosen
   from the file vitest prints beside every test in the control run's listing, with the same
   fallback to all twenty files whenever the narrow run finds nothing red. That took it from
   5.8 s to about 4.9 s per mutation, and no further, because the cost is vitest's own
   startup: **4.6 s for a single small file**, and measured the same through `npx` or `node`
   directly, forks pool or threads. The full table was **1570 s for 322 mutations**; it is
   **368** as of `26.8.7`, all caught, so scale that by the per-mutation figure rather than
   reading the total.

   Getting materially below that means keeping one vitest process warm in watch mode and
   attributing each re-run to the mutation that triggered it. That is worth roughly 26 minutes
   down to ten, against a harness that can mis-attribute a run --- deliberately not built yet.

   One thing the narrowing did break, and it is recorded as a trap: with one file in the run,
   a summary line can read `Tests  2 failed (2)` with no `passed` segment, which the count
   regex required. One mutation of 322 reported `no summary line -- the run did not finish`
   for a mutation its test had caught. Fixed, and proved on five summary shapes.

8. `npm run tauri build` and smoke-test the bundle, then `scripts/viewer_check.py` against
   it on both `testdata/text-heavy.pdf` and `testdata/vector-heavy.pdf`. On Windows also run
   `print-probe` (§8), which is the only check that reaches a real spooler.

   On macOS also `scripts/menu_check.py` and `scripts/save_check.py` against the bundle.
   **They do not take the same arguments, and naming them in one breath is what made this
   worth writing down.** `save_check.py` takes the **binary** and a document, like
   `viewer_check.py`; `menu_check.py` takes the **`.app`** and no document at all --- the menu
   is built before anything is opened, and it launches the app with `open` deliberately, so
   that it reads a bar built by a process with no stdout rather than one a harness supplied.
   Handed a binary and a PDF it fails with *"tpdf is not running, so there is no menu to
   read"*, which reads as an application that would not start:

   ```
   python3 scripts/menu_check.py --self-test    # the duplicate rule, both directions
   python3 scripts/menu_check.py                # the release bundle at the default path
   python3 scripts/save_check.py src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf \
       "$PWD/testdata/text-heavy.pdf"
   ```

   Run the `--self-test` first. It replays the application menu as measured before and after
   the duplicate `About tpdf` was removed, so it shows the rule calling one a defect and the
   other clean --- a check that has only ever passed is not known to be able to fail, and this
   one costs a second.
   Between them they read the two surfaces no test in either language can: the menu bar as
   the reader sees it --- a duplicate label shipped in 26.8.6 and reached a reader before
   anything here noticed --- and the file on disk after a Save, which nothing else in the
   repository writes. Both need an unlocked screen; `save_check.py` refuses a locked one
   rather than reporting an application that ignores its menu.

   **Windows produces an MSI and an NSIS installer**, since 2026-07-30. It did not until
   then, and the rule that came out of it is worth knowing before adding a probe:
   **`src/bin/` must contain only declared bin sources.** The bundler enumerates that
   directory and registers the first entry no `[[bin]]` `path =` claims --- a `.rs` file is
   always claimed, a *subdirectory* never is --- so `src/bin/backend_probe/`, which held only
   `imp.rs`, became a phantom binary and failed WiX. Those bodies now live in `src/probes/`.
   See the trap of that name for the four theories that were wrong first.

   **It no longer ships the probes.** Until 2026-07-31 the installer carried all 17 spike
   and benchmark executables, including a sandbox prober and a hostile-document harness,
   because they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets
   now: cargo still builds and links them, `scripts/gates.py`'s `bins` gate still covers
   them via `--examples`, and the bundler does not see them. The MSI payload was three files
   --- `tpdf.exe`, `tpdf_lib.dll`, `pdfium.dll` --- verified by extracting it, and the MSI went
   16.7 -> 8.0 MB with the NSIS setup 8.8 -> 5.8 MB.

   **It is four files as of 2026-08-02**, and the fourth is the point of the notices work:
   `THIRD-PARTY-NOTICES.md`, 469 KB, which a binary distribution owes and which nothing but
   an extraction can confirm actually shipped. Re-extract and list the payload after any
   change to the resource map --- that is the only step here that reads the artifact rather
   than the configuration that was meant to produce it.

   **Measured against the shipped `26.8.0` MSI it is three, and `tpdf_lib.dll` is not one of
   them** (2026-08-04). Read from the released artifact rather than from a build tree, so
   this is what people download:

   | bytes | what it is |
   |---|---|
   | 14,072,832 | the executable |
   | 478,495 | `THIRD-PARTY-NOTICES.md` |
   | 7,211,520 | `pdfium.dll` |

   The middle row is certain rather than inferred: the committed notices file is 469,298
   bytes over 9,197 lines, and 469,298 + 9,197 = 478,495 exactly --- the Windows runner
   checked out with CRLF, so the shipped copy carried one extra byte per line. The other two
   are identified by size and elimination.

   ⚠ **That arithmetic dates the measurement, and the next Windows build will not reproduce
   it.** `.gitattributes` pins `* text=auto eol=lf` as of 2026-08-26, so the runner now checks
   out LF and the shipped notices file is **469,298 bytes**, the same bytes the macOS bundle
   has always carried. The row is left as measured rather than edited to the new number,
   because it was measured and the new one is predicted; re-measure it at the next release.
   The change is an improvement and is worth stating as one: an artifact that differed between
   the two platforms for no reason now does not.

   **Settled 2026-08-19, and the answer is that a local MSI is not the MSI people get.** The
   released `26.8.4` payload is **three** files, `tpdf_lib.dll` not among them; a local `npm
   run tauri build` of `26.8.5` on this desktop produced **four**, the extra one being
   `tpdf_lib.dll` at 456,704 bytes, and the generated `target/release/wix/x64/main.wxs` names
   it as a `Source=` outright. The 2026-08-02 local `26.7.0` MSI has it too, at 137,728. So
   the earlier list was read off a build tree, and both readings were honest.

   **The runner and this desktop disagree about the same commit, which is the sharpest form
   of it.** `26.8.5-rc1` was built by CI from `5e0bb20`; its MSI holds **three** files, and
   the MSI this machine built from that same commit holds four. So the difference is the
   build environment and not the version, the configuration or the tag. The Windows leg's log
   never mentions `tpdf_lib` at all, so the runner's `target/release/` did not have the
   `cdylib` at bundle time; whether it was never built there or never harvested is still
   **open**, and it does not affect the shipped application, which links the `rlib` and never
   loads the DLL.

   What follows operationally: **read a payload count off a released artifact, never off a
   local build.** `gh release download <tag> --pattern '*.msi'` and extract that --- it needs
   no Windows, takes a minute, and works on a draft, so a rehearsal tag can answer it before
   the real one is cut.

   Its absence is not a defect on its face, since the binary links the `rlib` and does not
   load it --- but it is one more reason the installed app has to be *run* on Windows and not
   only unpacked.

   **This does not need Windows**, which is why it happened at all. An MSI is an OLE
   compound file with a cabinet inside it, and both are readable anywhere:

   ```
   uv run --with olefile python -c "
   import olefile, struct
   ole = olefile.OleFileIO('tpdf_26.8.0_x64_en-US.msi')
   cab = next(ole.openstream(s).read() for s in ole.listdir()
              if ole.openstream(s).read(4) == b'MSCF')
   off, = struct.unpack_from('<I', cab, 16)
   n, = struct.unpack_from('<H', cab, 28)
   for _ in range(n):
       size, = struct.unpack_from('<I', cab, off)
       end = cab.index(b'\x00', off + 16)
       print(f'{size:>12,}  {cab[off+16:end].decode()}')
       off = end + 1
   "
   ```

   The names it prints are WiX **File table keys** (`Path`, `PathFile_I<guid>`), not
   destination filenames, so identify the rows by size --- the count and the sizes are the
   facts here, and the count is what disagreed with this document.

   **Build before hiding the development library, not after.** The bundler copies
   `../vendor/pdfium/bin/pdfium.dll` as a resource, so a build with it already moved aside
   fails at `resource path ... doesn't exist` --- which reads like a broken checkout rather
   than like the sequence being wrong. Build, extract, *then* hide.

   **Run the bundle check with the development library moved aside.** This is not optional
   and it is not paranoia: until 2026-07-31 no bundle contained PDFium at all, and every
   check passed anyway, because `pdfium_library_dir` tries the dev tree first and a check run
   from the repo never reaches the bundled branch. A check on a distributable that can see the
   development tree is a check on the development tree.

   ```
   # Windows. macOS is the same shape with lib/libpdfium.dylib and the .app.
   msiexec /a <the msi> /qn TARGETDIR=<somewhere>
   mv vendor/pdfium/bin/pdfium.dll vendor/pdfium/bin/pdfium.dll.hidden
   python scripts/viewer_check.py <somewhere>/PFiles/tpdf/tpdf.exe <an absolute path>/testdata/outline-simple.pdf
   mv vendor/pdfium/bin/pdfium.dll.hidden vendor/pdfium/bin/pdfium.dll
   ```

   Two things that cost a run each, both worth knowing before starting. Pass the PDF as an
   **absolute** path --- the app resolves a relative one against its own working directory, and
   the failure is a plain "could not find the file" that reads like a broken bundle. And make
   sure the fixture has been **generated**: `testdata/*.pdf` is gitignored, an absent one
   produces the same red, and the first two attempts here died on `text-heavy.pdf`, which this
   machine had never built.

   Move the *bundled* library aside as well, once, and confirm the run fails. A pass on its own
   cannot say which of the candidate paths resolved; the failure names it.

   **Run on macOS 2026-07-31, and it failed --- the Windows fix did not carry over.** The
   `.app` built cleanly and `find` reported the dylib present, which is exactly how this
   stays hidden: `Contents/Resources/pdfium` existed, and it was a **file**, not a directory.
   The bundler read `"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/"` as a target *path* and
   renamed the dylib to `pdfium`, so both bundled candidates missed and the app died on
   `Contents/Resources/libpdfium.dylib` --- `0/1 checks passed`, three `could not load Pdfium`
   lines naming the path. The trailing slash is not a directory marker on this bundler.

   Fixed by naming the file in `tauri.macos.conf.json`
   (`"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/libpdfium.dylib"`), which lands it where
   the second candidate already looked. `tauri.windows.conf.json` is deliberately **not**
   changed: WiX ignores the target directory either way and the resource-root candidate
   catches it there, and that platform cannot be re-verified from a Mac. After the fix, with
   the dev library hidden: **102/102 checks passed, 7 not applicable, 109 names**.

   The failing run before the fix is the negative control, and it is what makes the pass mean
   anything --- the same `.app`, the same command, the only difference being where the library
   sits. Keep both halves when repeating this.
   **On Windows, verify the UPGRADE and not only the install --- with the released
   installer as the failing leg.** A first install is the case every local build exercises
   by accident; an upgrade is the one nobody sees until a reader has it. 26.8.9 could not
   install over 26.8.8 at all (`docs/TRAPS.md`, *A silent installer skips the file it cannot
   write, and exits 0*), and no check here could have said so, because every check started
   from an empty directory.

   The shape, and each leg takes about ten seconds:

   ```
   # The control is the artifact that is actually out there, not a rebuild.
   gh release download v<previous> --repo tstone-1/tpdf --pattern '*_x64-setup.exe' --dir <scratch>

   # Reproduce whatever the previous release leaves behind, in a scratch directory,
   # then run BOTH installers over it with /S /D=<dir> -- last argument, unquoted, no spaces.
   ```

   Read the answer off the *filesystem* (is the payload where `pdfium_library_dir` looks?),
   never off the exit code: the failing leg exits **0**, writes every other file, registers
   itself and creates the shortcut. Silent mode turns the Abort/Retry/Ignore box into Ignore,
   and Ignore reports success.

   Measured 2026-08-24, planting 26.8.8's stray `pdfium` file in both legs:

   ```
   shipped 26.8.9 setup, /S      exit 0   pdfium\pdfium.dll  ABSENT
   26.8.10 setup, /S             exit 0   pdfium\pdfium.dll  present, digest matches vendor/
   26.8.10 setup, /S, clean dir  exit 0   pdfium\pdfium.dll  present
   26.8.10 setup, /S, pdfium/    exit 0   pdfium\pdfium.dll  present, replaced
   ```

   The last two are the hook's other branches --- a first install and an ordinary upgrade ---
   and they are what says the fix costs nothing on a machine that never ran the broken build.

   **Installing writes to the machine you are testing on.** Three keys: `Uninstall\tpdf`,
   `Software\Timo Stein\tpdf`, and `Classes\.pdf`, whose `..._backup` value holds whatever
   handled PDFs before tpdf did. `reg export` all three first; afterwards put the machine
   back by re-running the **shipped** installer into the real location and diffing the
   exports, since re-running the new build would leave an unreleased version installed.

   **If the release adds or changes an NSIS hook, prove it was wired.** A mistyped key and a
   path naming a missing file are both refused --- by the build script's schema and by the
   bundler --- but a file that exists and defines the macro under another name is skipped in
   silence by the generated script's `!ifmacrodef` guard, and the bundle builds green:

   ```
   grep -n 'installer-hooks' src-tauri/target/release/nsis/x64/installer.nsi
   ```

   That is a source-level assertion and does not replace the A/B above; it is what tells you
   *why* the A/B failed when it does.

9. Commit as `Release vYY.M.MICRO: <summary>` and push it.

10. **Rehearse on a throwaway tag, then tag for real.** This list ended at step 9 until
    2026-08-03, which left the single riskiest action in the process written down nowhere
    but a comment in `release.yml` --- and it is the action that runs unreviewed code paths
    beside the signing key.

    ```
    git tag v26.8.0-rc1 && git push origin v26.8.0-rc1     # rehearsal
    # ... watch it, fix what it finds, delete the tag and the draft, repeat ...
    git tag v26.8.0     && git push origin v26.8.0         # the real one
    ```

    **The rehearsal is not optional the first time a workflow changes**, and the tag glob
    `v[0-9][0-9].[0-9]*.[0-9]*` matches an `-rcN` suffix on purpose so it can be done at all.
    Cutting `26.8.0` took **three** rehearsal tags, and each found a real defect that no
    amount of reading had:

    - `rc1` --- both gate legs red. `release.yml`'s `gates` job had been written from
      `ci.yml` and the copy lost the fixture-generation step, so a unit test needing
      `rotated.pdf` failed on both runners while passing in CI and locally.
    - `rc2` --- gates green, Windows published, macOS died on `***: no identity found`.
      Nothing had imported the certificate into a keychain yet; the step that signs the
      vendored dylib has to run *before* the bundler copies it, and the Tauri CLI's own
      import happens two steps later.
    - `rc3` --- the notarization path itself.

    Clean up between rehearsals, and note the two are separate: `git push --delete origin
    <tag>` **does not remove the draft release**, which persists without its tag and will
    sit in the release list looking like a real one.

    **Delete a draft by id, not by tag.** `gh release delete <tag> --yes` answers *"release
    not found"* for a draft that plainly exists --- it resolves the tag through the same REST
    endpoint that does not return drafts, which is the behaviour the `draft` job in
    `release.yml` was written to route around. Measured on `26.8.3-rc5`: the command reported
    not-found and the draft was still there afterwards.

    ```
    gh api graphql -f query='{ repository(owner: "tstone-1", name: "tpdf") {
      releases(first: 5, orderBy: {field: CREATED_AT, direction: DESC}) { nodes {
        databaseId tagName isDraft } } } }'
    gh api -X DELETE repos/tstone-1/tpdf/releases/<databaseId>
    git push --delete origin <tag>
    ```

    A failed run publishes nothing --- `release` needs `gates`, and both legs create the
    release as a **draft**. That draft is the last chance to edit the release body, and
    publishing it is step 11 rather than a clause here: see that step for what describing
    it in this sentence cost.

    **One `draft` job creates the release, and the build legs upload into it by id.** That
    is new on 2026-08-17, and it replaced a failure worth knowing about: `26.8.3-rc2` and
    `-rc3` each produced **two drafts under one tag** with the artifacts split, one holding no
    macOS updater bundle and the other no Windows installers. `tauri-action` used to resolve
    the release itself, which for a draft means paging `listReleases` for the tag --- its own
    source says *"you can't get an existing draft by tag"* --- and that lookup silently came
    back empty. `v26.8.2` logged `Found draft release ...` and was whole; neither leg of rc2 or
    rc3 logged it. **Why the lookup failed is still open**; `releaseId` means nothing looks
    anything up, so it no longer decides whether a release is whole.

    Step 11's asset count stays anyway, and not as ceremony: it is the only check that can
    tell a whole release from half of one, and it would have caught this before publishing.

11. **Publish the draft, and check it from outside the account.** A green `Release` run
    produces four artifacts and shows them to nobody --- GitHub hides a draft from everyone
    but repository owners, and its assets sit under `releases/download/untagged-<hash>/`
    rather than under the tag. Meanwhile the *tag* is public, so from outside the repository
    the state reads as a tag pushed by mistake.

    **Count the assets before publishing, and count them with GraphQL.** A complete release
    is **8** files: the `.dmg`, `tpdf_aarch64.app.tar.gz` and its `.sig`, the `.msi` and
    `-setup.exe` with their two `.sig`s, and `latest.json`. Fewer than that is half a
    release, and the two instruments that look right for this both fail: `gh release view
    <tag>` returns *a* release for the tag with no way to say which, so with two drafts it
    reports one of them as though it were the release; and `gh api
    repos/tstone-1/tpdf/releases` answers **HTTP 200 with `[]`** under the keychain token,
    because the REST endpoint wants a scope it lacks and reports that by returning nothing.

    ```
    gh api graphql -f query='{ repository(owner: "tstone-1", name: "tpdf") {
      releases(first: 5, orderBy: {field: CREATED_AT, direction: DESC}) { nodes {
        databaseId tagName isDraft releaseAssets(first: 20) { nodes { name } } } } } }'
    ```

    ```
    gh release list --repo tstone-1/tpdf                      # second column: Draft
    gh release edit vYY.M.MICRO --repo tstone-1/tpdf --draft=false
    gh release list --repo tstone-1/tpdf                      # second column: Latest
    curl -sIL -o /dev/null -w '%{http_code}\n' \
      https://github.com/tstone-1/tpdf/releases/download/vYY.M.MICRO/<asset>   # expect 200
    ```

    **Read the body before publishing, and if you correct it, send `tag_name` with the
    correction.** It is a literal in `release.yml`, so nothing can make it go stale except
    nobody reading it, and on 2026-08-23 it listed *stamps* under "what it is not, yet" one
    release after they shipped.

    **Half of that is mechanical since 2026-08-28, and the half that is not is named.** The
    *"What it is not, yet"* sentence carries a `<!-- not-built: -->` marker, and
    `src/lib/readme.test.ts` --- which already imports the registry for the README's own
    list --- asserts that nothing it names is registered, and that every id it names is also
    called unbuilt in the README. So the two copies of that list are provably the same claim
    rather than merely both plausible. It was built after the block was wrong in **three of
    four** releases: stamps in `26.8.8`, *Merge documents* in `26.8.10`, and **true
    redaction** in `26.8.11`, which was published as the release that ships it.

    What it does **not** own is the `26.8.10` direction --- a capability that shipped and is
    simply not mentioned. A release body is prose, and requiring it to name all 84 registered
    commands would make it the palette transcribed, so that stays with this step and a person
    reading it. Nor is every phrase covered: *"Signature verification"* names no command,
    because verifying a signature is a behaviour rather than something in the palette. Read
    the feature paragraphs against what you know shipped this cycle; the marker only stops
    the notes calling a shipped command unbuilt. A `PATCH` carrying only `body` resets the draft's `tag_name`
    to `untagged-<hash>`, and publishing in that state attaches the release to no tag ---
    while `gh release list` still shows it by name and `gh release view <tag>` cannot see a
    draft at all, so neither of the two obvious instruments reports it. The GraphQL query
    above prints `tagName` beside the asset count, which is why it is the one to use.

    **Put `tag_name` inside the JSON, because `--input` discards every `-f` beside it.**
    This block carried `-f tag_name=vYY.M.MICRO --input body.json` until 2026-08-24, and
    that command does exactly what the paragraph above warns against: `--input` supplies
    the *whole* request body, so the `-f` never reaches GitHub and the PATCH is a body-only
    one. Measured on `26.8.9` --- the reply came back `"tag_name":
    "untagged-bb1e54625d56b97bbd57"` from a command written to prevent that. The repair is
    one line of `json` and a second PATCH, and it is only cheap because the GraphQL query
    above is run either side of the edit; the two commands `gh release view` and `gh release
    list` both report the release by name in that state.

    ```
    python -c "import json,sys; d=json.load(open('body.json')); d['tag_name']='vYY.M.MICRO'; \
      json.dump(d, open('body.json','w'))"
    gh api -X PATCH repos/tstone-1/tpdf/releases/<id> --input body.json    # tag_name is IN the file
    gh api -X PATCH repos/tstone-1/tpdf/releases/<id> -f tag_name=vYY.M.MICRO -F draft=false
    ```

    The `latest.json` asset carries its own copy of that prose --- `tauri-action` fills its
    `notes` from the body at build time --- and correcting the release page does not correct
    it. Left alone deliberately: nothing in tpdf reads that field, so replacing an asset on a
    published release to fix text no reader sees is the worse trade. See the trap.

    **The `curl` is the point, not ceremony.** `gh release list` reporting `Latest` is our
    own authenticated view; an unauthenticated fetch of a download URL is the reader's, and
    it is the only one of the two that can tell a published release from a draft we happen
    to be able to see.

    This list ended at step 10 until 2026-08-12, with publishing named only in that step's
    closing sentence --- and `26.8.0` sat as a draft for **nine days** after a green run that
    had signed, notarized and uploaded everything. A step described inside the prose of
    another step is not a step anybody executes; nothing can go red for it, since no runner
    runs it and no gate covers it. The trap is *A draft release is invisible, and the tag
    beside it says the work shipped*.

12. **Apply the update from the previous release, by hand.** This is the only end-to-end
    proof the updater works, and no gate, harness or unit test can stand in for it:
    `update.test.ts` fakes the plugin, so what it covers is the state machine and not
    signature verification, TLS, or the shape of the real `latest.json`. Nothing in this
    repository has ever fetched that file.

    Needs two published releases, so it starts from the second one ever cut with the
    updater — first opportunity is applying `26.8.2` from an installed `26.8.2`+1.

    **Carried out for the first time on 2026-08-31, and it passes.** 26.8.11 installed from
    its own `.dmg` over the 26.8.12 that was there, launched normally: the toolbar showed
    `Update to 26.8.12`, pressing it reached `Update ready — restart to finish` in **two
    seconds**, and after quit-and-reopen the toolbar read `tpdf 26.8.12` with no update
    offered — which is the negative direction in the same observation. The bundle on disk
    afterwards is 26.8.12, `spctl -a -vv` says `source=Notarized Developer ID`, and the team
    identifier is unchanged, so the payload the updater installed carries the same signature
    the `.dmg` does. The sentence above about this step never having been carried out is
    therefore spent; keep it, because it is the reason to run this rather than trust it.

    **Two instrument failures on the way, both worth knowing before repeating this.** The
    accessibility tree is *not* a usable observer here: an `entire contents` walk of the
    window reported the toolbar without the update button, and a 70-second polling loop over
    it printed six clean absences while the button was on screen the whole time. The walk
    then began returning nothing at all, silently. **A screenshot of the window is the
    instrument** — `screencapture -x -o -R<x>,<y>,<w>,<h>` with the window's own AX bounds.
    And **a synthetic `click at` from System Events does not reach the WKWebView**; `cliclick`
    posts a real event and does. Verify the pointer landed before believing a click did
    nothing: `cliclick m:<x>,<y>` then `screencapture -C`, which draws the cursor.

    ```
    # With the PREVIOUS release installed in /Applications, launched normally:
    #   1. the toolbar offers "Update to <new version>" within a second or two
    #   2. clicking it shows progress, then "Update ready — restart to finish"
    #   3. quit, reopen, and "About tpdf" -- palette or the tpdf menu -- reports
    #      the new version
    #
    # That third line named a Help/About that did not exist until 2026-08-19, so
    # this step could never have been carried out. See the trap; the short of it
    # is that a checklist has no failing case, and one nobody has executed reads
    # exactly like one that keeps passing.
    curl -s https://github.com/tstone-1/tpdf/releases/latest/download/latest.json | head -20
    ```

    **Check the negative direction too, and it is the cheaper half:** launch the *newest*
    release and confirm the toolbar stays empty. An updater that offers an update to the
    version already running looks identical to a working one right up to the moment somebody
    installs the same build twice.

    If `latest.json` 404s, the release is still a draft — step 11 was skipped, and the
    endpoint resolves only to published releases. That is by design: publishing is what
    offers an update to anybody.

Verify the bump landed everywhere:

```
grep -n '"version"' package.json src-tauri/tauri.conf.json
grep -n '^version' src-tauri/Cargo.toml
```

**The updater needs two secrets and neither can be read back.**
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are set on the
repository, and their only other copy is in KeePass under *"tpdf updater signing key
(minisign)"*. Lose both and no installed copy of tpdf can ever be updated again — the public
half is compiled into every binary, so the only route back is a new key, a new build, and
every user installing it by hand. The private key is deliberately **not** on any development
machine: see the trap *Turning on updater artifacts makes every build demand the signing
key* for why `createUpdaterArtifacts` lives in a CI-only overlay rather than in
`tauri.conf.json`.
