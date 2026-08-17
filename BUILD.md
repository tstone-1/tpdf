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
# page count on every document PDFium will open, and the guard is defensive
# rather than demonstrated. `incr-encrypted-pw.pdf` is the file usually cited as
# the counter-example and it is not one: PDFium refuses to open it, so nothing
# downstream ever sees it. To re-run the sweep:
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
uv run --with pyhanko --with cryptography testdata/make_incremental_pdf.py testdata
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

`make_incremental_pdf.py` writes about **550 MB** on purpose, so that "appending to a
300 MB file is near-instant" can be tested at 300 MB.

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
  third population is the one nothing else can see --- the fourteen C++ libraries inside
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
| `outline-hostile.pdf` | 81 | 5 | 0 |
| `rotated-90.pdf` | 75 | 11 | 0 |
| `vector-heavy.pdf` | 52 | 34 | 0 |

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

**11/11 checks, 1 not applicable**, on `text-base14`, `text-cid`, `vector-heavy` and `rotated`
--- tiles **pixel-identical** to the in-process render, plus text extraction, outlines and
search across the boundary. The not-applicable one is the parent's memory poll: macOS has no
rlimit and polls as a substitute, while here the job object caps commit in the kernel, so there
is nothing to poll. It prints `[SKIP]` with that reason rather than vanishing.

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
OCR has interfaces and one engine, and no worker yet.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| fixture | result |
|---|---|
| `text-base14`, `text-cid`, `rotated` | 6/6 |
| `outline-simple`, `form` | 5/5, 1 skipped |
| `columns` | 2/2, 4 skipped --- two columns leave no vertically isolated span to use as a control |
| `vector-heavy` | 1/1 against the *inverted* claim: the page has no text, so reading none is correct |

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
reach. Everything before it can be, because **"Microsoft Print to PDF" is a real driver with a
real spooler**, and naming an output file in `DOCINFOW.lpszOutput` stops it raising a save
dialog:

```
cargo run --release --example print-probe
cargo run --release --example print-probe -- testdata/rotated.pdf
cargo run --release --example print-probe -- testdata/vector-multi.pdf "Microsoft Print to PDF"
```

Eight checks, and three of them are the ones worth understanding:

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

**It requires a bundle, not merely a release build.** A raw `cargo build` binary opens a
window and never executes a line of JavaScript --- WKWebView needs the bundle identity, and
the failure is silent: no error, no crash report, a blank window. Build one with
`npm run tauri build -- --bundles app` and run the executable inside it, which keeps stdout
and the environment that `open -a` does not. The *profile* genuinely does not matter --- the
check asserts behaviour rather than timing it --- so a debug bundle is only slower.

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
scripts/viewer_sweep.py --list          # the 14 corpora, and every fixture excluded, with reasons
scripts/viewer_sweep.py src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
```

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

**Every row below was measured on macOS on 2026-08-17**, and printed by the script rather than
transcribed --- the table is the sweep's own output, pasted. Zero failures
anywhere, and **all fourteen corpora report the same 231 check names** --- diffed as sets by
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
> any check rather than carried over from before it. That is not free --- a sweep is 721 s, of
> which `vector-multi` alone is 338 --- and it is worth it here for a reason this increment
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

| fixture | ran | skipped | what it is there for |
|---|---|---|---|
| `text-heavy.pdf` | 186 | 45 | the dense case, and search across 775 pages |
| `outline-simple.pdf` | 194 | 37 | the only fixture with an ordinary outline |
| `outline-hostile.pdf` | 194 | 37 | the only one with a `/Launch` entry to refuse |
| `vector-heavy.pdf` | 102 | 129 | one page, no extractable text, and no white paper to invert |
| `vector-multi.pdf` | 141 | 90 | twelve A0 pages: the only one where a thumbnail is slow enough to collide with the viewer |
| `rotated-90.pdf` | 181 | 50 | every page at `/Rotate 90`, which nothing else in the corpus has |
| `columns.pdf` | 183 | 48 | the only one whose content-stream order is not its reading order |
| `tagged.pdf` | 158 | 73 | the only one carrying a `/StructTreeRoot`, and the only two-page one |
| `multilingual.pdf` | 175 | 56 | the only one whose text is not Latin: CJK with no word separators, Arabic right-to-left, a decomposed accent, and a code point above the BMP |
| `encodings.pdf` | 176 | 55 | the only one whose character mappings are absent, broken or predefined --- and the only fixture that reaches the replacement-character path at all |
| `mixed.pdf` | 185 | 46 | the only one whose pages are not all the same size, and the only one that exercises the three layout checks at all |
| `comments.pdf` | 196 | 35 | the only one carrying annotations: notes, a reply, a highlight, three text-string encodings, an indirect `/Annots` array and 1,200 marks on one page --- the only corpus where all eight comment checks run |
| `links.pdf` | 203 | 28 | the only one with link annotations, and the only one whose outline is deliberately not in page order --- which is what let it catch a destination landing on the page before the one it named |
| `links-cropped.pdf` | 139 | 92 | the only one whose `/CropBox` is not its `/MediaBox`, so a rectangle placed in media space lands visibly wrong |

**`tagged.pdf` runs three of these thirteen and skips ten**, which is the split worth knowing:
the ten that drive the viewer need a middle page to delete and it has two, while the three that
ask the backend need only a page to spare. They are the only checks of this phase that run
there, and skipping them along with everything else would have been a skip for a reason that is
not theirs.

**`vector-multi` is the one with no margin, and the sweep's timeout was raised for it.** It
takes **338 s** of the default 900 (420 until 2026-08-17, which the page-deletion phase went
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
after the number stopped being right. A count chased
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

### The exit code of a spike run

`AppHandle::exit(code)` does **not** set the process's exit code. It ends the event loop,
`App::run` returns normally, `main` returns unit, and the process exits 0 whatever was asked
for. Every automated run here therefore reported success through `$?` for its whole
existence, `viewer_check.py` included. Fixed 2026-07-27 in `spike_exit`, which now flushes
and calls `std::process::exit`.

If you add a harness, do not let the exit code be its only verdict --- parse the transcript
too, and make the two agree. That is what caught this: a run printing `[OK] session restore
verified` directly beneath a phase whose own last line said `0/1 checks passed`.

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
   what 15/15 covers. Cutting `26.8.3` proved the gap rather than predicted it --- the
   page-move work changed `print::Pages::Only` from `Vec<u32>` to `Vec<PagePlan>` and missed
   the one Windows-only caller, and sixteen commits went by at 15/15 before a rehearsal tag
   turned both runner legs red. That leg reported *four* failures, since clippy, test and
   bins all stop at the same `error[E0308]`.

   It is `cargo check --target x86_64-pc-windows-msvc --all-targets` with the environment
   that command needs, and it does not link, so no MSVC linker is involved. About **8 s**
   warm against a CI round trip of six minutes. One-time setup, which the script names in
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

   **What it does not say**: only that the Windows tree type-checks. A wrong *value* passes
   --- proved, by changing a `PagePlan`'s turns and watching it stay green. Linking, loading
   and behaviour are still the runner's to find.
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
7. Re-run the three mutation harnesses if any of the code they cover changed. They are not
   gates --- each takes minutes and rebuilds per mutation --- and they are the only thing that
   says the tests can fail:

   ```
   scripts/mutate_rust.py          # the modules in FILTERS, `cargo test --lib`
   scripts/mutate_frontend.py      # the modules under src/lib, `vitest`
   scripts/mutate_viewer.py        # every runner below, in one pass

   # None of the three prints anything to a *redirected* log until it exits, so
   # a backgrounded run is silent from the first second to the last --- twenty
   # minutes for the front end, and over two hours for the Rust table on a
   # 114-mutation run that rebuilds per mutation. Wait for a signal the job
   # emits rather than asking the process table whether it is alive:
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
   # an hour of somebody waiting, not because a subset is ever the gate.

   # Or one runner at a time. The three probe runners need no webview, no bundle
   # and no unlocked screen; the three viewer ones need all three.
   scripts/mutate_viewer.py --runner structure          # structure.rs, structure-probe
   scripts/mutate_viewer.py --runner search             # search.rs + text.rs, search-probe on multilingual.pdf
   scripts/mutate_viewer.py --runner encodings          # text.rs, search-probe on encodings.pdf
   scripts/mutate_viewer.py --runner viewer             # appcommands/search/results/viewercheck.ts + search.rs
   scripts/mutate_viewer.py --runner viewer-tagged      # a11y/reading/viewercheck.ts, viewer_check on tagged.pdf
   scripts/mutate_viewer.py --runner viewer-encodings   # a11y/search.ts, viewer_check on encodings.pdf
   ```

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

   `mutate_viewer.py` drives seven runners, chosen per mutation and filterable with
   `--runner`. The `structure`, `search` and `encodings` ones need no webview and no bundle, so
   they neither wait for one nor require an unlocked screen; each rebuilds one example and runs
   it, at about 15 s a mutation. All six print the same `[FAIL] <name>` lines and the same
   summary, so the cross-check, the byte restore and the name validation are shared rather than
   copied. `RUNNERS` in the script is the list; the three probe runners share `search-probe`
   and differ only in the fixture they open.

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
   `viewer_check.py` per mutation (~20 s each), because what it covers --- the application's
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

   **Both run on Windows as of 2026-07-30 --- 22/22 and 75/75 --- and neither did before.**
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

8. `npm run tauri build` and smoke-test the bundle, then `scripts/viewer_check.py` against
   it on both `testdata/text-heavy.pdf` and `testdata/vector-heavy.pdf`. On Windows also run
   `print-probe` (§8), which is the only check that reaches a real spooler.

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
   checks out with CRLF, so the shipped copy carries one extra byte per line. The other two
   are identified by size and elimination. **Whether `tpdf_lib.dll` ever shipped is not
   settled here**: this is the first extraction taken from a released artifact rather than a
   local build, so it may have been dropped, or the earlier list may have been read off the
   build directory, where the `cdylib` does exist. Its absence is not a defect on its face,
   since the binary links the `rlib` and does not load it --- but it is one more reason the
   installed app has to be *run* on Windows and not only unpacked.

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
    sit in the release list looking like a real one. `gh release list` then
    `gh release delete <tag> --yes`.

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

    ```
    # With the PREVIOUS release installed in /Applications, launched normally:
    #   1. the toolbar offers "Update to <new version>" within a second or two
    #   2. clicking it shows progress, then "Update ready — restart to finish"
    #   3. quit, reopen, and Help/About or the palette reports the new version
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
