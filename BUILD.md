# BUILD.md — tpdf

Native UI probes now require a checks build:
`npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app`
(macOS), or
`npm run tauri build -- --config src-tauri/tauri.checks.conf.json --no-bundle`
(Windows; the executable is in `src-tauri/target/release/`). On Linux the same
`--no-bundle` build copies no resources, so put the library where a bundle would before
running a check:
`mkdir -p src-tauri/target/release/pdfium && cp vendor/pdfium/lib/libpdfium.so src-tauri/target/release/pdfium/`,
and `cp -r vendor/tessdata src-tauri/target/release/` for OCR. A Linux build of any kind needs
`scripts/fetch_tessdata.py` first: the bundle names the language data as resources, and Tauri's
build script refuses to build while one is missing. OCR also needs the system's Tesseract
(`libtesseract5` on Ubuntu), which tpdf opens at run time in the OCR worker only.
The window checks need no monitor there: `xvfb-run -a -s "-screen 0 1280x800x24" scripts/window_checks.py src-tauri/target/release/tpdf`
passed 11 of 11 on Ubuntu 24.04 under WSL on 2026-10-14, with `WAYLAND_DISPLAY` unset and
`GDK_BACKEND=x11` so WebKitGTK draws into Xvfb rather than into WSLg. On 2026-10-18 all 16
passed the same way from a checkout in the WSL home (`~/tpdf-linux`), fed uncommitted work as
`run_on_mac.py` feeds it: `git stash create` here, `git update-ref refs/tpdf-check/linux <sha>`,
and in WSL `git fetch /mnt/c/<the Windows clone> refs/tpdf-check/linux` and a detached checkout,
then `npm ci`, `fetch_pdfium.py` and `fetch_tessdata.py` there. Building in `/mnt/c` instead
would share `node_modules`, `vendor` and `target` with the Windows tree. `open_check.py` runs the
same way and passed every phase there, the running handover included; the cold double-click
skips, as on Windows. Add `--debug` for a
development build in `src-tauri/target/debug/`. It has a separate application identifier;
normal `npm run tauri build` excludes the frontend harness. Existing probe commands
below refer to this checks executable when they drive the UI. Run
`python3 scripts/check_bundle_share.py --checks` on its frontend output; the normal
gate requires zero harness code and deliberately refuses a checks build.
`scripts/mutate_viewer.py` also builds this explicit checks profile and launches
its matching bundle; a normal build cannot supply its mutation observer.
The content-completion mutation removes both strict decoding and the later
complete token walk: removing only strict decoding is masked by the second
validator. Separate stream-patch mutations cover that writer's boundary checks.
The build
gate verifies both profiles and leaves normal assets ready for packaging. Smoke-test
the normal bundle separately before release.

**The record of each engine pin**, from 8044 to the current one, is in
[`docs/build/pdfium-records.md`](docs/build/pdfium-records.md), newest first. The record of a
new pin goes at the top there.

**The pin is now asked about every week.** `scripts/check_pdfium_age.py` is red when
Chrome stable is on a later branch than the pin, and when the pinned branch has gained
commits since the pin, which is how a fix for a shipped milestone arrives. `audit.yml`
runs it, for the reason it runs the advisory audit and `gates.py` does not: the answer
changes when upstream moves. Since `scripts/publish_release.py` needs a green `Audit` run
on the tag's commit, **a release cannot be published on an engine that is behind**; build
the newer one first (below). Whether that should only warn was asked on 2026-10-11 and
decided: it stays a refusal, because a stale pin is how a merged security fix is missed. The check's `--self-test` runs offline, and against the
8066 pin the live check exits 1 naming `chromium/8078`.

**A stale pin opens an issue and starts the build (2026-10-11).** No dependency bot reads
this pin: PDFium is in no package registry, and a new engine is a build, not a changed
line. So when the check finds the pin behind, on the weekly run or on a push, it also
says what to build: the head of Chrome stable's branch as `<branch>-tpdf.1`, or the new
head of the pinned branch as its next build. The job `pdfium-bump` of `audit.yml` then
starts `pdfium.yml` with that version and commit and opens one issue naming both. The
build writes the two values into its own checkout with `check_pdfium_age.py --repin` and
produces candidate archives; the pin of the repository is unchanged, and the steps from a
candidate to a pin are the ones below. One issue per proposal: a run that finds an open
issue with the same title starts nothing. `pdfium.yml` takes the same two values by hand:
`gh workflow run pdfium.yml -f version=<branch>-tpdf.<n> -f pdfium=<commit>`. **The build
writes its result into the issue** (since 2026-10-23): its job `report` comments with the
verdict, each platform's conclusion and a link to the run, so a failed candidate is red
where the work is tracked. Until then `pdfium-bump` was green once it had started the
build, and the 8103 candidate failed on all three platforms while issue #5 said nothing.
A build started by hand with no issue of that title prints the same text in its log.

**To see that path work while the pin is current**, `gh workflow run audit.yml -f
rehearse=true`: the check runs against a copy of the pin set back to 8066, the build of
what it proposes is started as for a real finding, and the issue is titled as a rehearsal.
Close the issue afterwards, and cancel the build unless a build is wanted. That run is red
by design and is then the newest `Audit` run of its commit, so start `audit.yml`
once more without the input before publishing from that commit.

The build is defined by `.github/workflows/pdfium.yml` and `scripts/build_pdfium.py`,
with source/tool revisions in `scripts/pdfium_build.json`. It builds one unpatched
native engine each for mac-arm64 and win-x64, and emits an archive only after the engine
passes the RTL observation `scripts/pdfium_verify.py` pins and upstream's text tests.
The supplier's license collector omits Dragonbox and HarfBuzz; the wrapper adds
their permissive notices from the pinned sources, includes the builder's own
`LICENSE`, and refuses any other unknown library. Windows selects Git Bash explicitly and
checks it before compiling: PATH can otherwise select the WSL launcher.
Run its safeguards with `python3 -m unittest discover -s scripts -p test_pdfium_build.py`.
Building locally needs full Xcode or VS with the pinned Windows SDK; use
`python3 scripts/build_pdfium.py --help` for the disposable-directory invocation.
Source/dependency revisions are pinned. Host SDK/CRT versions are recorded rather
than hermetically supplied; no byte-identical compiler-output claim is made.
A workflow artifact alone neither publishes a dependency release nor changes the pin.

Plain Cargo and Tauri builds share the macOS deployment-target default in
`.cargo/config.toml` and `src-tauri/tauri.conf.json`; the toolchain gate checks
that they agree. Run Cargo from within this checkout so it finds the root
configuration. A shell override still takes precedence and can invalidate the
cache. The value preserves Tauri's existing `10.13` setting; it is not evidence
that the application or bundled PDFium works on that OS version.

Protected signature storage can be exercised with synthetic maximum-size pixels:
`cargo test --manifest-path src-tauri/Cargo.toml --lib native_store_roundtrips_maximum_pixels_and_forgets -- --ignored --nocapture`.
It creates and removes its own OS storage entry; it needs an unlocked desktop.
The regular gates compile it but do not contact Keychain/DPAPI.
Run `uv run scripts/signed_save_check.py <checks-executable> testdata/incr-certified-2.pdf`
to exercise warning text, initial Cancel focus, cancellation and accepted saving, and ---
since 2026-09-26 --- that the packaged app's sandboxed worker reports the fixture's
signature `intact`, the one place the integrity verdict is computed out of process. First
run 2026-09-26 on macOS arm64 against the release checks bundle: 6/6 in both launches, the
worker answering `intact` / `SHA-256` / `RSA`. Each launch also prints `[worker] could not
reply: Broken pipe` a few times as the app exits under a worker still answering; that line
predates the integrity check and is shutdown noise, not a failure.
The driver verifies unchanged file bytes after cancellation and changed bytes
after acceptance; the application drives its own modal without Accessibility.



How to get a clean clone building, what the quality gates are, and how a release is cut.

Durable project knowledge lives in [`AGENTS.md`](AGENTS.md); the architecture and roadmap
are in [`docs/PLAN.md`](docs/PLAN.md). This file is only the mechanics. The records and
long write-ups it held until 2026-10-23 are in [`docs/build/`](docs/build/), each linked
from where it stood; `scripts/check_build_index.py` keeps the lists of their sections whole.

---



## The Python client and PyPI

`api/python` is the distribution `tpdf-client`; its import name is `tpdf_client`. It is on
PyPI since 2026-10-08, <https://pypi.org/project/tpdf-client/>:

```
pip install tpdf-client
```

Version 0.1.0 was uploaded by run 37813188402 from `03f8b62c`. Read afterwards: the page
answers, the wheel and the source archive are there, and the wheel installed from PyPI in
a fresh environment answered `info` on a document through the built tool.

The import name was `tpdf` until 2026-10-08. PyPI has a project of another author named
`tpdf` whose module is also `tpdf`; with both installed, one writes over the other's files
and neither `pip` nor Python says so. The rename was made before the first upload, because
after it a rename breaks every script.

On 2026-10-02 the decision was not to publish, for three reasons. They still hold as facts,
and the decision of 2026-10-08 to publish accepts them:

- The client only starts the `tpdf` command-line tool, which comes with the desktop
  application, so `pip install tpdf-client` alone installs something that cannot run. The
  package's description has to say so. (It also said the client did nothing on Linux, where
  there was no tool; since 2026-10-14 the Linux packages install `tpdf-cli`, which the client
  looks for first.)
- It is a second release to keep in step with the tool's JSON reports. The client refuses
  a report whose `schema` is not 1 with `ProtocolError`, so a tool that has moved on is
  noticed and not misread.
- A version on PyPI cannot be withdrawn and replaced. A wrong upload is answered with the
  next version.

To publish: raise `version` in `api/python/pyproject.toml`, push, wait for CI, then

```
gh workflow run pypi.yml --ref main
gh run list --workflow pypi.yml --limit 1
```

`pypi.yml` refuses a version PyPI already has, requires a green `ci.yml` run on the same
commit (the client's full tests start the built tool, which a Linux runner does not
have), runs the two test files that need no tool, builds, checks the two archives,
imports the built wheel from outside the checkout and uploads. It stores no password: PyPI trusts this workflow in the environment `pypi` (project `tpdf-client`,
owner `tstone-1`, repository `tpdf`, workflow `pypi.yml`), set on pypi.org under
*Publishing*.

The same steps by hand, which is what the install line from the repository runs:

```
uv build api/python --out-dir scratch/dist
uvx twine check scratch/dist/*
uv run --isolated --no-project --with scratch/dist/tpdf_client-*.whl python -c "import tpdf_client; print(tpdf_client.Tpdf)"
```

## README screenshots

The six pictures and `demo.gif` in `docs/img/` are generated, never taken by hand, and
from one run: every state the application holds is a frame of the animation, and six of
those frames are the stills. One of the six, `hidden.png`, is of `site/sample-redacted.pdf`,
the letter the landing page offers: `uv run --with reportlab testdata/make_sample_pdf.py
site/sample-redacted.pdf` writes it, and writes the same bytes each time.

```
npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app
uv run --with reportlab --with pillow scripts/screenshots.py "src-tauri/target/release/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf"
npm run build
```

A tpdf window opens for about half a minute and nothing is typed; the screen must be
unlocked. On macOS the window is photographed by its window number, so it does not have to
be in front, and the terminal needs the Screen Recording permission. The last line puts the
normal frontend back in `dist/`: the `bundleshare` gate refuses a checks build.

The same script runs on Windows, against `src-tauri\target\release\tpdf.exe` from a
`--no-bundle` checks build, and `--out` is required there because `docs/img/` holds the macOS
pictures. The picture is a copy of the screen, so the script keeps the window above the
others without activating it. It has to run in the desktop session: from a console there, or
from another machine through a one-shot `/IT` scheduled task. Run it before a release that
changed the toolbar or a menu. 2026-10-02 on Windows 11 it found what the macOS pictures
could not: a scrolling menu's scrollbar takes room there, an item wrapped, and its second
line was printed over the next item.

The session has to have a screen as well as a logged-in user. A session left by Remote
Desktop is logged in and *disconnected*, and a picture taken in it fails with *The handle is
invalid* before the application is started. `quser` shows the state. `tscon <id>
/dest:console`, run with administrator rights, attaches the session to the machine's own
screen, unlocked; lock it again afterwards. 2026-10-13: 45 of 45 checks and 20 pictures that
way, and the first of them showed the header's menu, which no Mac draws, with every item
centred.

From a Mac all of that is one command, which sends this tree, builds the checks binary there,
attaches and locks the session as needed, and copies the pictures back:

```
scripts/run_on_windows.py --host HOST --clone 'C:\path\to\tpdf' --pictures <folder>
```

It needs a PowerShell login shell and an administrator account, and it stops when nobody is
logged in. `--picture-arg` passes arguments on to `screenshots.py`.

`--dark --out <folder>` takes the same pictures with the window in its dark appearance,
without changing the system setting: the script sets `TPDF_THEME`, which the application
reads at start. Run it after a change to a colour, and look at the pictures; they are not
committed, and `--dark` refuses to write into `docs/img/`.

`--more` adds the chrome that only appears on demand, as stills named `more-...`: the find
bar with results, an armed tool's status row, the Highlight menu, the last menu of the tool
row, the narrow layout's More menu, the text editor, and a failure message. `--size 1000x760`
opens the window at that size (through `TPDF_WINDOW_SIZE`) and keeps the pictures at their
own size. The three combine, and each needs `--out`:

```
uv run --with reportlab --with pillow scripts/screenshots.py "<checks binary>" --more --dark --out scratch/dark
uv run --with reportlab --with pillow scripts/screenshots.py "<checks binary>" --more --size 1000x760 --out scratch/w1000
```

Run these after a change to the toolbar, a menu or a colour. They found what the five README
states could not: a menu opening outside the window at 1,000 px, and a failure colour too
dim on a dark background.

The states are `src/lib/screenshotcheck.ts`, the document is `testdata/make_demo_pdf.py`,
and the signature cards come from `testdata/incr-two-signers.pdf`. Regenerate after a
change a reader would see in one of them, and look at each picture before
committing it: the first two runs of this script were green and wrong, once photographing
the menu bar five times and once showing an unmarked page beside an empty mark list.

## Prerequisites

| Tool | Notes |
|------|-------|
| Rust (stable, via rustup) | `rustup update`. Do not install a second toolchain through Homebrew. |
| Node 20+ and npm | |
| Python 3.9+ | Only for `scripts/`; not a runtime dependency of tpdf. |
| `uv` | Only for the test fixtures that need `fontTools` or `pyhanko`. |
| `qpdf` | Not needed to build or run tpdf, and **required** for the hostile corpus — `testdata/make_hostile_pdf.py` shells out to it, so without it there is no `hostile-manifest.json` and `sanitize-rewrite` cannot start. Also the structural oracle for spike 0.4. On Windows the winget package needs elevation; the release's `msvc64.zip` unpacks anywhere and needs none. |

---

## Clean clone

```
npm install
scripts/fetch_pdfium.py
```

`vendor/pdfium/` is gitignored — a 7.7 MB binary does not belong in the object store — so
**a fresh clone has no PDFium and every binary fails to bind at runtime until the fetch
script has run.** The script downloads the pinned source-built archive, verifies its SHA256
before extracting anything, and refuses a V8 asset.

### A scratch copy or a second worktree

`git archive` and `git worktree add` give the tracked files and nothing else, and two things
`cargo test` needs are gitignored: `vendor/pdfium/` and the generated fixtures in `testdata/`.
Both failures look like something else. Without the library the build stops at `resource
path ../vendor/pdfium/lib/libpdfium.dylib doesn't exist`; without the fixtures the build
succeeds and unrelated tests go red on a missing `testdata/*.pdf` --- nine of them in `save`,
`print` and `docinfo` on 2026-09-26, in a review whose change touched none of the three. Set a
copy up before its first build:

```
cp -R <checkout>/vendor/pdfium vendor/          # or scripts/fetch_pdfium.py
python3 scripts/ci_fixtures.py --signed --hostile
```

`ci_fixtures.py` builds what CI builds, which is not every fixture a local run can use (its
docstring lists what it leaves out); copying the generated files from a checkout that has
them is the complete alternative. A red test that names a file under `testdata/` in a fresh
copy is this setup, not a finding.

A worktree an agent harness creates may start at the remote's default branch rather than at
the checkout's `HEAD`, so commits not yet pushed are missing from it. Check
`git rev-parse HEAD` in it before the first edit.

Verify an existing install without touching the network:

```
scripts/fetch_pdfium.py --check
```

The pin is `pdfium-8103-tpdf.1` since 2026-10-23; Phase 0 measurements in `AGENTS.md` and
`docs/PLAN.md` used `chromium/7881`. Bumping it means `scripts/check_pdfium_age.py --repin
<version> <commit>` for `scripts/pdfium_build.json`, editing `TAG` and the whole `PINS`
table in `scripts/fetch_pdfium.py` together, then re-running the checks that a digest
cannot stand in for:

Those checks, each with its command, what it should print and why it exists, are in
[`docs/build/pdfium-probes.md`](docs/build/pdfium-probes.md). Run them from the repository
root after generating the fixtures below; each exits non-zero on failure.

### Test fixtures

`testdata/*.pdf` is gitignored and generated. Nothing it produces may be committed or
redistributed — `make_text_pdf.py` embeds a system font.

**Off CI, a missing fixture fails its test.** `scripts/gates.py` sets
`TPDF_REQUIRE_FIXTURES=1` unless `CI` is set, and a unit test whose fixture is absent then
fails through `src-tauri/src/fixtures.rs` instead of printing `[SKIP]` and passing. A runner
still skips the ones `ci_fixtures.py` cannot make. Run `cargo test` by hand with the variable
set to get the same rule, and without it to let a fixture you have not generated skip.
`text-heavy.pdf` is supplied by hand and no script writes it, so a machine without it fails
the gate's `test` until it is copied in.

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
below told a reader to run the viewer check against both — so the instruction that
produces a fixture and the instruction that consumes it disagreed, and the failure is
an absent file reported as a broken bundle. `text-heavy.pdf` is deliberately not here:
it is a real document rather than a generated one, and a machine that does not have it
cannot make it.

**What that was quietly costing, found 2026-08-22.** This limitation had been written
down three times and every one of them discusses *corpora* — a viewer sweep that cannot
run all fourteen, a `prespawn-bench` check that skips, a 109-name re-run taken on six.
All true, and none of them is where it hurt. **Ten `cargo test` tests over the save
path's guards also asked for `text-heavy.pdf`**, and returned at their first line without
it: here, and on both runners, which cannot have it either. A test that returns early is
counted among the `753 passed`, and its `[SKIP]` goes to a stdout libtest discards for a
passing test, so nothing in a green run said so. Every mutation aimed at those guards
SURVIVED, which is the only instrument that could tell.

They use `comments.pdf` now — generated, appendable, and carrying `/Annots` of its own
so the array-bearing branch is exercised too — except the one whose control needs a page
listing *nothing*, which takes `rotated.pdf`. All twelve `append` mutations and all 62
`save` ones are now caught by the test named for them. The guards were correct throughout.

The general form is worth carrying to the next "this machine cannot have X" note: the
question is not whether it is true, it is **what else consumes X**. `docs/TRAPS.md` has
the entry.

`make_incremental_pdf.py` writes about **550 MB** on purpose, so that "appending to a
300 MB file is near-instant" can be tested at 300 MB.

**Its signed fixtures did not exist on a hosted runner until 2026-08-21**, so CI tested none
of the signature reader. Both workflows now install pyhanko and call
`scripts/ci_fixtures.py --signed`, which builds the nine of them — eleven since the two
encrypted fixtures joined the group on 2026-08-23, and seventeen since the six fixtures of
the signature integrity verdict (`signed-p256`, `-p384`, `-pss`, `-sha1`, `-altered`,
`-broken`) joined on 2026-09-26. `SIGNED` in that script is the list; these counts are history. Two things had to change for
that to be possible, and the first is why it had never worked: `make_incremental_pdf.py` called
**qpdf** with `check=True` and nothing else, so a machine without qpdf died there with a
`FileNotFoundError` naming a program rather than a fixture — and died *before* every signed
fixture, none of which needs qpdf at all. It skipped that one fixture from then on. The
second is `--scan-pages` with no values, which is the existing switch for not writing 550 MB.

**It calls qpdf for nothing at all since 2026-08-23**, and the skipped fixture is a runner
fixture now. `encrypt_with` writes both encrypted documents with pyhanko — one behind
`swordfish`, one on an empty user password — so `incr-encrypted-open.pdf` and
`incr-encrypted-pw.pdf` are in the `--signed` group and CI builds them. That is what makes
`password-probe` run on a runner instead of printing twelve `[SKIP]`s, and it is not a
tidiness fix: the save path's encryption guard had been wrong for four weeks with every gate
green, and the fixture that catches it was the one no runner could build.

Proved both ways before the step was written: with the fixtures moved aside and pyhanko absent,
`ci_fixtures.py --signed` exits **1** with `exited 0 but testdata/incr-signed.pdf does not
exist`; with pyhanko present it exits 0 and writes all nine. **Green on both runners since
2026-08-21**, after three pushes — the first two failed on assertions that had pinned a value
out of a locally generated fixture, which is one trap and worth reading before adding a test
that reads one. That hard failure is what makes the
tests' own `[SKIP]`-when-absent safe — a runner that failed to build them goes red at the step
that built them, not green through a suite that skipped.

**The fixtures are not reproducible, and CI generates them fresh every run.** Two consecutive
runs *on one machine* produce nine files of identical size and differing bytes, because pyhanko
mints a new key pair and serial each time. **Across machines the size moves too**: both CI
runners build an `incr-signed.pdf` of **8,097** bytes where this laptop builds **8,128**, on the
same commit. So nothing absolute may be pinned out of one — not a digest, not a serial, not a
date, and **not a size**, which is the one that looked safe after the local pair agreed and went
red on both runners at the first push. What replaced the pinned numbers is a quantity derived
from the file at test time, by a route `docinfo` does not take.

Three tests reading them ended with `assert!(examined > 0)` until the same day, which is red on
exactly the machines that cannot have the files — measured by hiding `testdata/incr-*.pdf`:
three failures, each telling a runner to generate what the repository had written down as
deliberately absent. They now assert that **every** named fixture was examined, behind an early
return when none of them exists. Both directions proved: no signed fixture gives 702 passed, 0
failed; hiding exactly one gives two red.

`make_comments_pdf.py` is the only fixture carrying annotations, and it is also one of the
three `scripts/ci_fixtures.py` builds on a hosted runner — it needs nothing but the standard
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
scans got wrong until 2026-08-16. Run `text-probe` against it as well as `links-probe` — the
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
y-flip — the un-flipped convention reaches 87% and the `/Rotate 180` control 77%, against
0--5% on the fixtures written for this probe. So what a green run proves on these two is
**placement, not orientation**, and the probe now says exactly that in a `[NOTE]` line rather
than reporting an undiscriminating control as a failure. Until 2026-08-16 it failed them and
exited 1, so this documented command was red and this page quoted only its passing line.

**The 96.4% is worth reading against its own control rather than against 100.** Removing the
origin shift in `text.rs` takes it to **74.8%** — a `[FAIL]`, since the threshold is 95% —
while `text-base14.pdf`, which has no crop box, stays at 100%. That is the measurement proving
this probe covers the text half of the crop-box fix at all, and it is closer to the threshold
than it looks: a 50 pt inset moves each box by less than a line's height, so on dense text most
still overlap some ink. The `0% before the fix` figure this page used to carry came from a
different and larger error — the scan then mixed PDFium's *cropped* size with page-space
boxes. A fixture with a bigger inset would give the probe more margin.

96.4% rather than 100% is correct and not a near-miss: the fixture's text runs past the crop
box's bottom edge, so the characters the crop hides have boxes outside the rendered page. A
fixture whose every glyph sat inside the crop would not exercise that. Two things about it carry the same kind of intent as
the comment fixture's. Its **outline points at the same destinations its links do**, which is
what makes `links-probe --mode agree` able to compare tpdf's two destination resolvers at all;
delete the outline and that mode still runs, still prints a count and can no longer fail.
And the rotated page is a **separate file** — `links-rotated.pdf` — because a document
that mixes page sizes reddens two of `viewer_check.py`'s rotation checks, which derive what
they expect from page 1's aspect ratio. That is the same split, for the same reason, that
`comments-rotated.pdf` exists.

`make_tagged_pdf.py` is the other side of that coin: the only fixture that carries a
`/StructTreeRoot`, so it says what its own reading order is. Page 1 puts a margin note beside
the first paragraph — geometry reads it third, the tags read it last — and page 2 is the
control, tagged in the order geometry would have inferred anyway. A tagged fixture whose tag
order matches geometry tests nothing, so the generator **asserts the discrimination itself**
and refuses to write a fixture that has lost it. Its manifest states both orders, which is what
lets a check say "the tagged answer was used" rather than "an answer was produced".

Its manifest also carries the three fields `viewer_check.py`'s reading-order check reads
(`page`, `name`, `lines`), so this is an ordinary corpus for that harness rather than one it has
to know about — and what that check then asserts is the **lines**, in tagged order, against a
file a different program wrote.

Worth knowing as external evidence that the fixture is not merely self-consistent: poppler's
`pdftotext` reads page 1 in **geometric** order — heading, margin note, body — which is the
wrong answer the tags exist to correct.

**It carries two heading levels, and that is not decoration.** A page with one heading cannot
tell a consumer that uses the document's level from one that announces every heading as `h1`:
the mutation doing exactly that survived against the first version, and the check passed. A
property with one value present is the same as none — see the trap, whose list of the usual
suspects (one page, one rotation, one font, one column) is worth reading before building any
fixture.

`make_columns_pdf.py` is the only fixture whose *content-stream order* is the point. Its
three pages are two columns emitted column by column, the same two columns emitted line by
line across the gutter, and a heading spanning both over the second of those. The first two
look identical and must read identically, which is an assertion neither page can satisfy by
agreeing with itself. It writes `columns-manifest.json` beside the PDF, and
`viewer_check.py` passes any `<stem>-manifest.json` it finds through to the check — so
what reading order is compared against is a file a different program wrote.

`make_mixed_pdf.py` is the only fixture whose pages are not all the same size. Every other
document in the corpus is uniform — `make_rotated_pdf.py` builds a second, uniform file for
exactly that reason — so until this existed no check could fail on the frontend's largest
layout assumption. It is A4 with an A3-landscape insert (wider, same height, so a failure is
the crop and not the offset), an A5 page (shorter, so a failure is the offset), and an A4
control before and after both. Each page carries a marker at every one of its own edges, and
page 3 carries one just past A4's width, so a cropped render loses a named string rather than
losing something unnamed.

It writes `mixed-geometry.json` rather than `mixed-manifest.json`, because the
`-manifest.json` suffix enrols a fixture in the reading-order check and this one makes no
claim about reading order. `viewer_check.py` binds that sidecar to `TPDF_GEOMETRY_MANIFEST`,
the `geometry_manifest` command hands its contents to the webview, and the three layout checks
assert against it — against a file a different program wrote, rather than against the backend
the viewer renders through. On every other fixture those three say `[SKIP] no geometry sidecar
for this fixture`.

---

## Quality gates

```
scripts/gates.py
```

**Mark new files before the run: `git add -N $(git ls-files --others --exclude-standard)`.**
Several gates read `git ls-files` --- `dates`, the doc and anchor checks --- so a file that is
not yet tracked is invisible to them, and a green local run says nothing about it. That is
not hypothetical: on 2026-09-27 a full local run passed 26/26 over new command-line test data
carrying certificate expiry dates in 2027, and the pushed commit failed `dates` on CI with 148
findings. `-N` records the path without staging its content, so it changes what the gates
see and nothing about what a commit would contain.

That is the whole checklist. **`scripts/gates.py` is the definition of the gates, not a
description of them** — it holds the commands with their flags, and this file deliberately
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
  most of the backend — the request queue and the `tile://` parser, the worker protocol
  and the pool, rendering, text, search, outlines, printing, session and sweep — with
  `npm run test` doing the same for the front-end logic beside it. What it deliberately
  leaves to the harnesses under `scripts/` is everything that needs a live webview — and a
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
  its own lifetime, but the gaps between runs — and any headless bench running alongside
  it — are unprotected, and a session that locks mid-batch fails the next frame-rate run
  outright. A locked macOS session cannot be unlocked from a script by design, so this is
  preventable and not recoverable.

- **The `toolchain` gate runs first, and it is what makes the Rust pin real.**
  `rust-toolchain.toml` names the compiler; `RUSTUP_TOOLCHAIN` in the environment overrides
  that file completely and silently, so a pin with nothing asserting it is indistinguishable
  from no pin. The gate compares the running rustc against the file, checks clippy and
  rustfmt came from the same toolchain commit, and prints `RUSTUP_TOOLCHAIN` whether or not
  it is set. Neither workflow uses a toolchain-installing action any more — both run
  `rustup show`, which installs exactly what the file names.

  To move to a newer Rust: edit `rust-toolchain.toml`, run `scripts/gates.py`, and commit it
  on its own. Expect `-D warnings` to surface new lints; that is the pin working, and dealing
  with them in a dedicated commit is the whole reason it exists.

- **The `workflows` gate compares `ci.yml` and `release.yml`'s `gates` jobs, and only those.**
  They must be the same job: one says a commit is good, the other stops a tag on a broken
  commit producing artifacts, and if the release copy is weaker then every ordinary push is
  checked harder than the thing that actually ships. They had drifted exactly that way —
  see step 10 of the release checklist for what it cost. The gate compares every `uses:` with
  its pinned SHA and every `run:` body, in order; step *names* are not compared, since two
  identical commands under different labels are still the same job. It deliberately says
  nothing about the `release` job, the triggers or the permissions: those differ on purpose,
  and that difference is the fork threat model rather than drift.

- **The `pdfium` gate is a pin check, not a build step.** It fails if `vendor/pdfium` is
  missing or is not the pinned build — which is the difference between a benchmark that
  means something and one that does not.

- **The `notices` gate runs last because it reads the build's output.** It derives which
  npm packages ship from `dist/assets/*.js.map` — the bundler's own account of what it
  emitted — so it needs the `build` gate above it to have run. Two checks in one command:
  that `THIRD-PARTY-NOTICES.md` still matches the dependency tree, which is the
  binary-distribution obligation; and that no GPL, LGPL or AGPL licence has appeared. Its
  third population is the one nothing else can see — the C++ libraries inside
  libpdfium, read from `vendor/pdfium/licenses/`, which `cargo metadata` is structurally
  blind to. Regenerate with `scripts/third_party_notices.py` and commit the result; never
  hand-edit the file. On a mismatch it prints the **diff**, not the word "stale" — a gate
  that fails on a machine you are not sitting at is only actionable if its message carries
  the evidence.

  **After any PDFium pin bump, cross-check the two archives.** They ship the same fifteen
  licence files and nine of them differ — eight by line endings, and `pdfium.txt` by
  carrying a `//` comment prefix on macOS and none on Windows. A document generated from
  whichever archive is installed is then a function of the platform, which is how this gate
  came to be green on macOS and red on Windows with nothing wrong:

  ```
  scripts/fetch_pdfium.py --platform win-x64 --dest /tmp/pdfium-win
  scripts/third_party_notices.py --cross-check /tmp/pdfium-win
  ```

  Note this is doable **from either machine** — the other platform's archive is a download,
  not a machine you have to be sitting at. Worth reaching for before waiting on a CI round
  trip to diagnose a platform difference.

### CI runs per push and per pull request, and again on a tag

`.github/workflows/ci.yml` runs the gates on `macos-latest` and `windows-2025` for every
push to `main` and every pull request, since 2026-08-02.

Windows cache investigation, 2026-09-29: [CI run 36559413820](https://github.com/tstone-1/tpdf/actions/runs/36559413820)
took 31m53s. Rust cache restore took 2m27s and its post-step 9m18s; within
the latter, archive preparation took 8m29s and upload about 20s. The saved
archive was 5,088,915,448 bytes. The pinned cache action already removes
incremental output and examples. An exact cache-key hit skips saving; this run
restored an older dependency key and wrote the new one. Do not treat this save
cost as an overhead paid on every source-only push.

A native Windows compression comparison used zstd 1.5.7 and Git's tar on
1,584 dependency artifacts (3,627,418,892 bytes), with four interleaved pairs
and reversed pair order. Level 1 versus the default level 3 made the archive
13.0% larger (890 MB versus 787 MB); median compression-plus-extraction time
was 10.58s versus 10.31s. All eight extractions reproduced every path and
SHA-256 digest. Lower compression was therefore not adopted. These are desktop
measurements of dependency artifacts, not timings for the full hosted cache.
The desktop's accumulated app/fuzz `debug/deps` directories also contained
1,731 identically named, byte-identical artifacts totalling 2,876,360,610 bytes.
That includes historical builds and is not a size estimate for CI's duplicate
content.

The shared-target experiment then ran on native Windows x64 at `9d05867`, with
`CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0`. Both configurations passed all
29 gates. Four interleaved pairs, reversing pair order, measured restoration,
the five affected gates (`clippy`, `test`, `fuzz`, `bins`, `api`), the pinned
cache action's target-cleanup code, and archiving. Each round started from its
configuration's fixed, cleaned cache snapshot; both snapshots were verified
after restoration by path and SHA-256 digest. All eight timed cycles passed.

| Target layout | Compressed target cache | Median measured cycle |
|---|---:|---:|
| Separate app/fuzz directories | 1.154 GB | 4m29s |
| Shared directory | 0.801 GB | 5m24s |

Sharing cut the archive by 30.6% but was slower in every pair, by 30-77 seconds
(11-28%). The action cleans each workspace in turn: mapping both to one target
removes fuzz-only `jobserver` and `libfuzzer-sys` artifacts. Cargo also rebuilt
the application/examples after the fuzz build in every shared trial; with
separate directories that step took less than a second. Keep the separate
targets. These are desktop target-cache cycles, excluding the common Cargo
registry, network transfer, initial setup and unaffected gates; they are not
hosted CI timings.

This section said "CI runs on a tag, and on nothing else" until then, and the reason it
gave was half wrong in a way worth keeping. The objection was never runner minutes — it was
that a workflow would be **a second place for the gate list to live**. `ci.yml` does not
restate the commands, it invokes `scripts/gates.py`, so that objection never applied to the
workflow that was eventually written. What changed materially is that the repository went
public and macOS minutes stopped costing 10x against a private allowance. "One machine" was
a description of the circumstances, not an argument.

**What CI cannot cover, and why the harnesses below stay manual.** `viewer_check.py` and
`mutate_viewer.py` drive a real window and need an unlocked, unoccluded screen. On a
headless macOS or Windows runner they do not fail, **they hang** — which is the failure shape
this project reads worst, since a hang and a pass both produce no red. Do not add them to a
workflow on those two. On Linux Xvfb is a screen and nothing hangs, so the Ubuntu job of both
workflows runs `viewer_check.py` under `xvfb-run` since 2026-10-15: the blank window of
`ca1203e4`, put back, failed it in WSL with *The page threw while it was being built*, and the
tree passed it, 355 of 355 in 42 s on `links.pdf`. It is a launch check: the full window checks
and the mutation tables stay here.

`.github/workflows/release.yml` fires on a CalVer tag. It **invokes `scripts/gates.py`** on
both platforms rather than re-listing commands in YAML, so the checklist and the gate stay
one object instead of two that happen to agree today. Then it builds, signs, notarizes and
publishes a draft release.

It arrived for a reason this document did not predict — it expected the trigger to be the
repo going public or a second contributor. What actually forced it is notarization: it needs
a Mac, a Developer ID and Apple API credentials, and a signed macOS release should not
depend on which machine is free.

**Its macOS half ran green on 2026-08-03**, and this paragraph said it had never run until
that day. Ported from `screenpick`, whose version is proven; the part with no precedent is
signing the bundled `libpdfium.dylib`, since neither sibling ships a native library.
Notarization requires every Mach-O in the bundle to be Developer ID signed with the hardened
runtime, so the dylib is signed in `vendor/` before the bundler copies it — correct whether
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
  again on any release whose verification step has been touched — rc3 is the case where
  the artifact was perfect and the checker was broken.
- **The macOS bundle layout for a resource map is settled**, which `pdfium_library_dir`
  records as unverified and the code cannot answer: the engine lands at
  `Contents/Resources/pdfium/libpdfium.dylib`, and the verification step prints the path it
  found.

The Windows measurements are in [`docs/build/windows-viewer.md`](docs/build/windows-viewer.md),
one section each:

<!-- build-index: docs/build/windows-viewer.md -->
- Windows runs the viewer, and how it came to be contained
- `backend-probe` on Windows, and the defect it found in itself
- `tile-bench` on Windows, and what the render constants cost here
- `pool-bench` on Windows: what a pool buys a screenful

---

## Running it

```
npm run tauri dev -- --release
```

**Never benchmark through `tauri dev` without `--release`.** It shells out to `cargo run`
in the dev profile, and because PDFium arrives as a prebuilt optimized dylib the result is
not uniformly slow but *selectively* slow — PNG encoding of a tile measured 67 ms in debug
against 1.41 ms in release while the PDFium render beside it moved 1.39 -> 1.36 ms. Ratios
invert rather than merely inflate.

**Startup timing needs a bundle, not just `--release`.** Under `tauri dev` the frontend is
served by Vite over HTTP, so a startup measurement describes Vite's module graph:

```
npm run tauri build -- --bundles app
scripts/startup_bench.py target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf <file.pdf>
```

Run the executable inside the `.app` directly — that keeps stdout and the environment,
which `open -a` does not. `--purge` gives a genuinely cold page cache and needs a sudoers
entry for `/usr/sbin/purge`.

Each probe, bench and window check has a section in
[`docs/build/running.md`](docs/build/running.md), in this order:

<!-- build-index: docs/build/running.md -->
- Which backend parses the document
- `ocr-probe`: does the recogniser work, and is the flip right
- `win-ocr-probe`: can `Windows.Media.Ocr` be the Windows engine at all
- `ocr-sandbox-probe`: what is left of a process under each profile
- `ocr-worker-probe`: does the engine work from a process of its own
- `textlayer-probe`: does a text layer read back as the words it was written from
- `compress-probe`: how much smaller a copy gets, over a corpus
- `redact-reach-probe`: how much of a redaction can be proved, over a corpus
- `sanitize-rewrite`: does a collected rewrite sanitize without losing the document
- `encrypted-rewrite-probe`: does a rewrite keep an encrypted document's encryption
- `redact-gate-probe`: does the redaction gate certify a clean file and refuse a dirty one
- `latency-bench`: what one tile costs, decomposed
- Printing on Windows, and how to check it without paper
- Linux packages: the deb and the AppImage
- Printing on Linux, and how to check it without paper
- Explorer thumbnails and the preview pane, on Windows
- The "reopen its windows" dialog
- Checking the viewer
- Checking that a mark a reader makes reaches the document
- Checking session restore
- Checking file associations
- Checking the recent documents the shell is told about
- Checking that a Save reaches the disk
- Checking the menu bar
- The command-line tool
- The exit code of a spike run
- The four Phase 0 spikes nothing above invokes
- Fuzzing, and how to read what it leaves behind

---

## Windows signing

The maintainer applied to the SignPath Foundation on 2026-09-12. The Foundation declined:
the project did not yet have sufficient public adoption and independent recognition. The
decision was about public visibility, not a technical assessment of the project. A paid
SignPath subscription was deferred on 2026-09-16, and Windows releases stayed unsigned up
to 26.10.10. The workflow that built samples for that application and the proposed
artifact configuration were removed on 2026-10-08, after 26.10.11 was signed with the
Certum certificate; both are in the history before that date. One sentence of that plan
still holds: updater signatures and `latest.json` are made from the final installer bytes,
after Authenticode signing, because replacing an installer afterwards would invalidate its
updater signature.

### Signing with the Certum certificate

Since 2026-10-07 there is a certificate: Certum *Open Source Code Signing in the Cloud*,
subject `CN=Open Source Developer Timo Stein`, issued by *Certum Code Signing 2021
CA*, valid until 2027-10-07. The key is in Certum's SimplySign service and cannot be
exported. Certum supports one way to use it: log in to its desktop program with the
account's e-mail address and a six-digit code, after which the certificate appears in the
user's certificate store and `signtool` can use it. That works, and it
is the fallback when everything below does not:

```
signtool sign /sha1 <thumbprint> /fd sha256 /tr http://time.certum.pl /td sha256 <file>
scripts/verify_signature.ps1 -Signer 'Open Source Developer Timo Stein' -Path <file>
```

The six-digit code comes from the `otpauth://` address Certum shows once, at activation.
Keep the whole address and not only its `secret`: `ssign` reads `algorithm`, `digits` and
`period` from it and assumes SHA-256, 6 and 30 where it names none, and an authenticator
that assumes SHA-1 shows six digits that are refused.

**A release is signed by the workflow, without the desktop program.** `release.yml` builds
`ssign`, an unofficial client for the SimplySign service, from a pinned commit and names it
as Tauri's `signCommand` through `src-tauri/tauri.signing.conf.json`, on the Windows leg
only. The login is two secrets of the GitHub environment `signing`: `CERTUM_EMAIL` and
`CERTUM_OTP_URI`, the whole `otpauth://` address. The environment accepts the `main` branch
and `v*` tags. After the build the leg reads the signatures back, of the installer and of
`tpdf.exe`, `tpdf-cli.exe` and `pdfium.dll` unpacked from it, and fails unless each is
valid, timestamped and by that signer. `docs/THREAT-MODEL.md` §T9, residual 6, has what the
login is worth to somebody who steals it.

**The built client is a file of a release of this repository.** Building `ssign` took 3.5
minutes of the Windows leg of 26.10.12, which took 19, and each build fetches the client's
dependencies anew. So one build is kept: `sign-rehearsal.yml` builds the client from
`SSIGN_REV`, signs with it, and keeps the file as an artifact of the run; that file is
published as `ssign.exe` in a prerelease named by `SSIGN_BUILD`, and its SHA-256 is
`SSIGN_SHA256`. Both workflows carry the three pins and fetch
`releases/download/<SSIGN_BUILD>/ssign.exe` without a token. The file is used only when its
digest is the recorded one. **A file with another digest fails the job**; a file that does
not arrive is a warning, and the client is then built from the pinned commit. The release
name says what a file claims to be: whoever can write a release here can replace the file,
and the digest in the workflow is what says which file is meant. The workflow check holds
the three pins equal in the two files and refuses a job that holds a secret and fetches a
release file, or restores a cache, without such a comparison.

**To record a build**, after changing `SSIGN_REV` or when the pins are empty:

1. Set `SSIGN_BUILD` and `SSIGN_SHA256` to `''` in both workflows, on `main`.
2. `gh workflow run sign-rehearsal.yml --ref main`. It builds, logs in, signs, and keeps
   the client as the artifact `ssign-<commit>`. This is also the proof that the new client
   logs in.
3. `gh run download <run> --name ssign-<commit>`, and compare the file's SHA-256 with the
   run's line `ssign.exe built from <commit>, sha256 <digest>`.
4. Publish it as a prerelease that is not the latest, like an engine release, beside the
   two texts a published program has to carry: `gh release create
   ssign-<first 7 of the commit>-tpdf.<n> ssign.exe LICENSE-ssign.txt
   THIRD-PARTY-NOTICES-ssign.txt --prerelease --latest=false --target <full commit id of
   main>`. `LICENSE-ssign.txt` is the client's `LICENSE` at that commit. The notices are
   the licence files of every package that `cargo metadata --locked --filter-platform
   x86_64-pc-windows-msvc` resolves in the client's source at that commit; read the
   licences it declares first, because a copyleft one there means the file is not
   published.
5. Record the release name and the digest in both workflows, and run the rehearsal on
   `main` again: its line `fetched from <release>, digest as recorded` is the proof that a
   release will find the file.

**The digest is that of the published file and of no other build**: the client does not
build reproducibly, and on 2026-10-10 three builds of the same commit had three digests.
So a digest printed by a release run that had to build cannot be recorded.

The recorded build is `ssign-5fd4daf-tpdf.1`, digest `1ff20aab...d47d485`, built by rehearsal
run 38120091581 on 2026-10-11, which signed a plain executable and the installer of
26.10.9 with it and installed from that installer.

Until 2026-10-11 the built client was kept in an Actions cache with the key
`ssign-<os>-<arch>-<commit>`, checked against the same digest pin. It did not last there. A
repository's caches are limited to 10 GiB together, the Rust cache of the Windows gates is
near 7, and the entry was removed before each release from 26.10.14 on, so each built the
client and the pin was compared with nothing. A run on a tag also saved its own build under
that key, which no other tag and no run on `main` can read.

`sign-rehearsal.yml` proves the build of `ssign` and the login without building tpdf: it
signs a plain executable and the installer of a published release, reads both back and
installs from the signed installer. Run it before a release whenever `SSIGN_REV` changed,
and when a release leg fails in the build step with a login error:

```
gh workflow run sign-rehearsal.yml --ref main
gh run list --workflow sign-rehearsal.yml --limit 1
```

Measured on 2026-10-07, `windows-2025`:

| Run | Result |
|---|---|
| 37631357684 | Login and a plain executable passed. The `.msi` was refused: `not a PE (no MZ signature)`. `ssign` 0.1.7 signs executables only |
| 37633778247 | A plain executable and the 26.10.9 installer signed; both read back valid, timestamped and by the signer; the signed installer installed with exit code 0 |

**Three rules the login brings.** One code is good for one login, so two jobs that log in
within the same 30 seconds make the second fail, and repeated failed logins can lock the
account: everything that signs is in the concurrency group `certum-signing`. A group holds
inside one repository only, so it does not stop a signing job of another project that
uses the same certificate: start one release or rehearsal at a time across all of them. Do
not log in to the desktop program while a signing job runs, for the same reason. And `ssign` keeps its
session in `%TEMP%\ssign\session.json` for twenty minutes; both workflows remove it.

**There is no `.msi` from the release after 26.10.10 on**, because `ssign` cannot sign one
and a release with a signed installer beside an unsigned package is worse than one without
the package. The `.msi` had one download in each of 26.10.6, 26.10.8 and 26.10.9. What that
costs somebody who installed from it was measured on Windows on 2026-10-07: with the 26.10.10
`.msi` installed, the 26.10.10 `-setup.exe` run as the updater runs it (`/S /UPDATE`) ended
with exit code 0 and left the `.msi` registered beside its own entry. The updater of such
a copy finds no `windows-x86_64-msi` in `latest.json`, falls back to `windows-x86_64` and
runs that installer, so it ends with two installed copies. The release notes say to
uninstall the `.msi` once.

**A release built this way was rehearsed four times on 2026-10-07**, and each tag failed
one step later than the last:

| Tag | Windows leg |
|---|---|
| `v26.10.11-rc1` | Stopped before building: the overlay's path was put together in the job matrix, where `github.workspace` is empty, so Tauri was given `/src-tauri/tauri.signing.conf.json` |
| `v26.10.11-rc2` | `tpdf-cli.exe` signed, then `failed to run ssign` on `tpdf.exe` and no reason: Tauri shows nothing of a sign command that failed. Hence `scripts/sign-windows.cmd` and its log |
| `v26.10.11-rc3` | The log gave the reason: `atomically replacing ...tpdf.exe: Access is denied. (os error 5)`, a fifth of a second after Tauri had written to the file. `ssign` replaces a file by renaming a signed copy over it. Hence `scripts/sign-windows.ps1`, which lets it sign into a folder of its own and copies the bytes back, again for up to thirty seconds |
| `v26.10.11-rc4` | Passed. One login signed nine files: `tpdf-cli.exe`, `tpdf.exe`, `pdfium.dll`, five NSIS plugin DLLs and the installer. The four the leg reads back were valid, timestamped and by the signer. The draft held 6 assets, `latest.json` named the installer for `windows-x86_64` and `windows-x86_64-nsis`, and `minisign` verified the downloaded installer against its `.sig` and the key in `tauri.conf.json`, and refused a copy with one byte added |

What holds a file open against the rename was not found. `sign-rehearsal.yml` holds one of
its files open against writing for twenty seconds while it signs, and fails unless the copy
had to wait and then succeeded: 5 attempts waited in run 37664244204.

**26.10.11 shipped an unsigned `uninstall.exe`**, read on 2026-10-08 on a Windows 11
computer after installing the published release: `tpdf.exe`, `tpdf-cli.exe` and
`pdfium.dll` were `Valid` and the uninstaller `NotSigned`. Tauri puts the sign command into
the installer script as `!uninstfinalize '"sign-windows.cmd" "%1"'`, and makensis runs it
from `target\release\nsis\x64` through the shell. Started by its quoted name through
`PATH`, `sign-windows.cmd` got that folder for `%~dp0` and asked `pwsh` for a script that
is not there; makensis printed `UninstFinalize command returned 64` and went on. Found by
building 26.10.11 on that computer with a stand-in for `ssign` and `--verbose`, which is
the one way to see what makensis says: without it Tauri shows none of it. The wrapper now
takes the script's path from `TPDF_SIGN_SCRIPT`; the same build then logged the
uninstaller's temporary file, `nst2743.tmp`, between the last plugin DLL and the installer.
The Windows leg now installs what it built and reads `uninstall.exe` where it lands, and
`sign-rehearsal.yml` starts the wrapper the way makensis does. **Not yet seen:** a real
signature on an uninstaller; the next release run is that.

## Cutting a release

**The steps, one line each.** Each line links to its step in the list further down,
which carries the reasons and the commands.

1. [Fetch, confirm the branch is not behind, and take the dependency updates](#release-step-1)
2. [Bump the four version files](#release-step-2)
3. [Refresh both lockfiles with `cargo check`](#release-step-3)
4. [Put the release date in `CHANGELOG.md` and the release notes in `release.yml`](#release-step-4)
5. [Run `scripts/gates.py`, then `scripts/window_checks.py` on a screen](#release-step-5)
6. [Re-check `docs/THREAT-MODEL.md` against the code](#release-step-6)
7. [Run the mutations for the changed behaviour](#release-step-7)
8. [Build the bundle, smoke-test it, and run `scripts/installed_check.py` on Windows](#release-step-8)
9. [Commit, push, and confirm the `Audit` run is green](#release-step-9)
10. [Rehearse changed release mechanics, then tag](#release-step-10)
11. [Publish the draft, and check it from outside the account](#release-step-11)
12. [Apply the update from the previous release: `scripts/update_check.py` on Windows, `scripts/update_check_mac.py` on a Mac](#release-step-12)

**The records of past releases** are in
[`docs/build/release-records.md`](docs/build/release-records.md). The record of a new release
goes at the top there.

Version scheme is **CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO
starts at 0 and increments within the month.

1. <a id="release-step-1"></a>`git fetch` and confirm the local branch is not behind — this repo is pushed from more
   than one machine, and a version bump on a stale clone has already cost a re-cut release
   elsewhere in the portfolio.

   **Then take the dependency updates, before anything is verified** (2026-10-13). It is
   part of this step and has no number of its own, because the steps are named by number
   in five files and in `release.yml`. Every check after it then runs on the tree that
   ships; done late, an update invalidates the gate run before it, and the notices file
   is stale until it is regenerated:

   ```
   npm outdated; npm update
   (cd src-tauri && cargo update --verbose)      # the `Unchanged` lines are what is held back
   (cd src-tauri/fuzz && cargo update)           # its own lockfile, audited on its own
   git diff --stat -- package-lock.json src-tauri/Cargo.lock src-tauri/fuzz/Cargo.lock    # cargo's own lines are no proof
   scripts/third_party_notices.py                # when a crate or a package moved
   scripts/check_pdfium_age.py                   # the engine pin against Chrome stable
   ```

   The last line asks the question `Audit` asks on the release commit, and a pin that is
   behind refuses the release there (*The pin is now asked about every week*, near the top
   of this file). Asked here, a stale pin is found before the verification rather than after
   the push, as on 2026-10-23, when Chrome stable had moved to `chromium/8103` while the
   26.10.25 tree was being checked. It needs the network, which is why it is not a gate.

   Updates within the ranges are taken. A `0.x` minor of a direct dependency is taken too,
   by raising its range, and so is a major, unless something named here holds it. For each
   crate still held back, ask `cargo tree -i <crate>@<version>` what holds it, and take it
   when the answer has changed. Held on 2026-10-13, 26.10.18:

   - **`der`, `x509-cert`, `sha1`, and the second `sha2`** remain on the generation
     required by `cms` 0.2.3 and `rsa` 0.9,
     and the versions of both that take the new one are release candidates (`cms`
     0.3.0-rc.0, `rsa` 0.10.0-rc.19). Take that set when both are released. ECDSA is
     independent: it exchanges prehashed bytes and SEC1 points, not their ASN.1 types.
     `ecdsa` 0.17 and `p256`/`p384` 0.14 move to fixed `crypto-bigint` 0.7.6 for
     RUSTSEC-2026-0387. The earlier requirement to hold them together was wrong.
     The Linux certificate-file reader's `pkcs5`, `des`, `rc2`, `cbc` and `hmac` belong to the same
     generation (`Cargo.toml` says so beside `pkcs12`) and move with it (26.10.20).
   - **`gtk` 0.19** is held by Tauri: `muda` and `tao` name 0.18. Take it when they do.
   The fuzz package has a lockfile of its own, and `Audit` reads both. 26.10.19 updated only
   the application's: rustls moved to 0.23.46 there and stayed at 0.23.45 in the fuzz one,
   and four advisories published that week failed `Audit` on the release commit.
   - `toml`, `toml_datetime`, `toml_edit`, `brotli-decompressor`, `generic-array` and
     `rand_chacha` are not named in `Cargo.toml`; what names them pins them.
   - **`windows-core` 0.100** is held by Tauri: `tao`, `wry` and `webview2-com` name 0.62.
     **`parking_lot` 0.13** would be a second copy beside `tao`'s 0.12, the reason `base64` is
     held. Take each when Tauri does (26.10.24).
   - **`tauri-plugin-updater` is pinned exactly in both ecosystems** (`Cargo.toml` and
     `package.json`): a hold recorded only here was once undone by the routine
     dependency-update commands above. A new updater is taken only in a release that runs
     step 12 on it before the tag. 2.14.1 (2026-10-28) replaced 2.13.3, which had been held
     since 26.10.19; it brought `zip` 9 and `base64` 0.23 with it.
   - **While every version a requirement allows is yanked, `cargo update` with no `-p`
     cannot resolve** (`zip` 4, 2026-10-16): update by name instead, every package in
     `Cargo.lock` except the yanked one and `tpdf` as `-p` arguments (`name@version` where
     a name is locked twice), then `--precise` for anything held.
     `scripts/check_cargo_audit.py` is where `Audit` allows such a yank.
2. <a id="release-step-2"></a>Bump **all four** version files so they agree:
   - `package.json`
   - `package-lock.json` (top-level *and* the root package entry — `npm version <v> --no-git-tag-version` does both)
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
3. <a id="release-step-3"></a>`cargo check --manifest-path src-tauri/Cargo.toml` to refresh `Cargo.lock`.
   Also run `cargo check --manifest-path src-tauri/fuzz/Cargo.toml --bins`:
   the separate fuzz lockfile records the application version too, and the
   locked fuzz gate otherwise rejects a version bump.
4. <a id="release-step-4"></a>In `CHANGELOG.md`, replace `Unreleased` with the release date.
   **And add the release's *New in* paragraph to the notes in `release.yml`** (the heredoc
   that writes `release-notes.md`), above the previous release's, in the commit that is
   tagged. The body is a literal there and the draft is made from it, so a paragraph added
   after the tag reaches the release only through a `PATCH` of the draft, which is the
   path step 11 warns about: on 26.10.20 the paragraph was missing until step 11's reading
   found it, and the draft was corrected by a `PATCH` that had to carry `tag_name`.
5. <a id="release-step-5"></a>`scripts/gates.py` — all gates pass.

   **Once, on the final tree.** If only version files, lockfiles and prose change after that
   run, step 3's `cargo check` plus `scripts/check_trap_index.py` and `scripts/check_dates.py`
   cover what changed, and CI then runs the full list on both platforms on the release commit.
   The 26.9.18 cycle ran the suite twice here, once on the Windows development machine and twice more in the cloud on
   one commit. **Do not run `gates.py` on the Windows development machine as part of the window checks**:
   CI's `windows-2025` leg runs the same gates on the same commit. The desktop is for what CI
   cannot do: the window phases, `print-probe`, `redact-reach-probe` and the installers.

   On a Windows host with many cores, cap Cargo concurrency if linking exhausts
   memory: `$env:CARGO_BUILD_JOBS='2'`. The gate suite defaults to two jobs on
   Windows. A 2026-09-10 run launched more than 30 linkers; a later allocation
   failure exhausted system commit memory and terminated the calling session.
   Keep this cap for release builds as well as the gates.

   **On a Mac, also `scripts/check_windows.py`, and it is not optional before a tag.** A
   green gate list on this platform says nothing about any `#[cfg(windows)]` line, because
   the compiler never parses one: `print_win.rs`, `examples/print_probe.rs`,
   `examples/win_sandbox_probe.rs` and the Windows halves of `worker*.rs` are all outside
   what the gate list covers — the two figures below are 15/15 because that is what the run
   was at the time, and it is 17/17 since 2026-08-22; the gap is the same one whatever the
   count, which is why the figures are left as the runs reported them. Cutting `26.8.3` proved the gap rather than predicted it — the
   page-move work changed `print::Pages::Only` from `Vec<u32>` to `Vec<PagePlan>` and missed
   the one Windows-only caller, and sixteen commits went by at 15/15 before a rehearsal tag
   turned both runner legs red. That leg reported *four* failures, since clippy, test and
   bins all stop at the same `error[E0308]`.

   **On Windows, the mirror is `scripts/check_linux.py`**, before a push that adds, moves or
   removes a platform `cfg`, and before a tag. It sends the tree as it is to `~/tpdf-linux` in
   WSL (Ubuntu 24.04, set up as *Linux* near the top of this file says) and runs the `clippy`
   gate there. On 2026-10-20 the gates passed 33 of 33 on the Windows development machine while both other
   runners failed on `function chosen_pages is never used`: its callers had all moved behind
   `#[cfg(windows)]`. The script, with that defect put back, fails on the same line; warm it
   takes under a minute. It compiles Linux, not macOS, so a `target_os = "macos"`-only line
   is still the macOS runner's to find.

   **And `scripts/run_on_windows.py` before a push that touches redaction, OCR, printing or
   any `#[cfg(windows)]` behaviour** (2026-10-03). `check_windows.py` compiles and runs
   nothing, so what only Windows does is otherwise first run by CI, after the push:

   ```
   scripts/run_on_windows.py --host HOST --clone 'C:\Users\me\tpdf'
   ```

   **A push that touches Windows printing also needs `print-probe`** (§8, the only check that
   reaches a real spooler), not only at step 8. Since 2026-10-20 a worker draws every printed
   page, and neither the gates nor `run_on_windows.py` starts one: 26.10.24's release commit was
   pushed before `print-probe` found that no Windows development tree could start it
   (`docs/TRAPS.md`, *PDFium unpacked through a Python temporary directory could not be loaded
   by a low-integrity worker*).

   It sends the tree as it is here, uncommitted work included, to a Windows machine over
   SSH and runs `cargo test --test cli` there (`--suite lib` or `all` for more), in a
   worktree of its own beside the clone, so the clone's checkout is never moved. It
   generates the fixtures CI generates and borrows the ones CI cannot from the clone's
   `testdata`. Warm, a run takes about two minutes; the first is a cold build. `--rev`
   runs a commit, and the control that the script can fail is `--rev` on a head known to
   be red: on `224be2a` it reported the two redaction checks that had failed on the
   runner and exited 1, and on the tree with the fix it exited 0 with 464 checks passed.
   A host that does not answer, or a run that never prints its exit marker, exits 2.

   The host's SSH login shell may be WSL's bash or Windows PowerShell; the script asks
   which before it sends anything, and a host that answers neither exits 2 with nothing
   sent (2026-10-10). Until then it assumed bash, and a PowerShell host read the bundle's
   base64 as commands, one line each. The suite's independent read-backs need `qpdf` on
   that host's `PATH`, and without it they print `[SKIP]` and the run still exits 0, so
   read the skip lines of a first run on a new machine. The first run against a PowerShell host,
   on a Windows 11 machine with `qpdf` 12.4.2, passed 567 checks with 4 skipped, none of
   them for `qpdf`; the run before it, without the fixture packages, exited 2 at the
   fixture step with nothing tested.

   The day it was written it found two things CI had not. `Windows.Media.Ocr` does not
   read a 16 px control word reliably (`docs/TRAPS.md` has the entry), which CI showed
   only as two failures blamed on the wrong feature. And the OCR checks of the CLI suite
   had never run on Windows at all: they need `text-base14.pdf`, which a runner does not
   generate, so they skip there, and on a machine that has the fixture the engine reads
   24 of its 30 words. The pass mark on Windows is now that, with the measurement beside
   it in `tests/cli/ocr.rs`.

   ⚠ **If it does not return in about a minute, it is wedged rather than slow — kill it and
   run it again.** On 2026-08-27 it sat for 15 min 45 s with its log frozen at the banner,
   and the same command on the same tree finished in **21.83 s** a minute later. The
   instrument is CPU time, not elapsed: `ps -eo pid,etime,time,args | grep
   "[x]86_64-pc-windows-msvc"` showed 2 s of CPU across sixteen minutes. Add `--verbose`
   while diagnosing — output is captured and shown only on failure otherwise, which is
   exactly wrong for a run that never ends. See the trap of that name.

   ⚠ **That minute is the warm figure, and a change to a widely-included module makes the
   run minutes long rather than seconds — so the rule above will tell you to kill a healthy
   run.** Measured 2026-08-28 after editing `ocr_gate.rs`: **~4 minutes** cold against **1 s**
   warm on the very next invocation, both green. Two things follow. Judge by CPU, never by
   elapsed, exactly as the paragraph above says — but **use the `grep` form it prints and
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
   only red one — found by the `v26.8.6-rc1` rehearsal tag at a cost of a 25-minute round
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
   tree again. The 2 min 58 s is real and was taken with nothing else running — it is what
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
   *value* passes — proved, by changing a `PagePlan`'s turns and watching it stay green.
   Linking, loading and behaviour are still the runner's to find. **The general form is
   worth carrying to any stand-in for another platform: it is only as strong as the command
   it runs there, never as strong as the target it names**, and anything the real gate list
   does that the stand-in does not is a class of failure it cannot report while reading as
   coverage.

   **And `scripts/window_checks.py` on a checks build, before a tag** (2026-10-09). No gate
   opens a window, so what is wrong only at the join in `App.svelte` is found by these
   checks or by a reader:

   ```
   npm run tauri build -- --config src-tauri/tauri.checks.conf.json --bundles app
   scripts/window_checks.py "src-tauri/target/release/bundle/macos/tpdf Checks.app/Contents/MacOS/tpdf"
   npm run build
   ```

   It runs the viewer check, the mark check on three documents, the tab phases `tabs`,
   `tabs-position`, `sides`, `views`, `answers`, `form-beside`, `import`, `fit`, `compare`,
   `add-text` and `recognise`, the session check and the open check, one after the other, and
   exits 1 when any of them fails or a fixture is missing; `--list` prints the commands.
   `recognise` clicks **Recognize this page** in the find bar; 26.10.26 shipped that button
   disabled while every check reached the command through the palette.
   The last line puts the normal frontend back in `dist/`, which the checks build replaced.
   It needs an unlocked screen and takes the keyboard while it runs, so it is not a gate.
   The Tab check of `form-beside` needs a form of two controls and `testdata/form.pdf` has
   one, so that run prints that it was not made; run it once by hand with the fixture the
   `forms` phase uses (`tabs_check.py <binary> <fixture> --phase form-beside`).
   The reason it is a step: two of these checks had been red for days when the two-sides
   work began, one from command lists that had fallen behind the registry and one from a
   drag aimed in the wrong coordinates, and nothing had run either. `--only` is for the
   loop while one check is being worked on, and its run says that it is not the release's.
   It runs the viewer check on one document. The sweep over every window corpus is
   `scripts/viewer_sweep.py` (*Checking the viewer*), a longer run with its own table.

   **Also before the push of any change that adds, removes or renames a command, a menu
   entry or a dialog's controls** (2026-10-16), not only before a tag. The checks read
   menus and dialogs by their words and count their entries, so such a change turns one of
   them red while every gate stays green, and nothing says so until the next tag. Run it
   where nobody is working, on the second Mac (below) or on the Windows build machine,
   which is about ten minutes unattended. The cause: step 7.4 added two entries to the tab
   menu on 2026-10-15, the `tabs` phase still counted eight, and it was found a day later
   by the first Windows run since. On 2026-10-09 two checks were found red the same way.

   **On a second Mac, as one command** (2026-10-10), so that the machine somebody is
   working at keeps its screen and its keyboard:

   ```
   scripts/run_on_mac.py --host HOST --clone '~/path/to/tpdf' [--wrap 'COMMAND --']
   ```

   It sends the tree as it is here, uncommitted work included, builds the checks bundle in
   a worktree beside the other Mac's clone, and starts `window_checks.py` in that Mac's
   logged-in session as a one-off `launchd` job. `--rev` runs a commit and `--only` is
   passed on. `--wrap` is a command put before the build and the signing, for a host
   whose SSH login has neither the pinned Node nor an unlocked keychain. The other Mac
   needs no display attached. It needs a session that is not locked, and with no setting
   of its own the screen saver locks it after twenty minutes; the script's header has the
   one command that turns that off, and the script refuses a locked session before it
   builds. When the build is signed with a Developer ID the script signs the bundled
   PDFium with the same identity, without which every check ends in *could not load its
   PDF engine*. First run, on `923d1860`: 11 of 11. An unreachable host, a clone that is
   not there and an `--only` that matches nothing each ended in exit 2. A run with a
   failing check, which must end in 1, has not been seen.

   **It runs unmodified on Windows** against the `--no-bundle` checks build, from a session
   that is logged on (2026-10-09, Windows 11): every check passed. In that run the `sides`
   phase passed its checks and `tabs_check.py` then exited 1 with `could not verify worker
   exit: [WinError 5] Access is denied`. That was the harness: `win_worker_exit.py` asked
   about a worker that was exiting and did not wait for it (`docs/TRAPS.md`, *A process
   that is exiting refuses a query before its handle says it has exited*). It waits now.
6. <a id="release-step-6"></a>**Re-check `docs/THREAT-MODEL.md` against the code**, and correct the document before
   trusting anything else in this list — §3's boundary table, §5's sandbox policy and
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
   true. Nor does a `built:` marker say the prose beside it is accurate — only that the
   command is claimed somewhere a reader will look, so a bullet describing a command wrongly
   passes exactly like one describing it well. Read the first three paragraphs, then the two
   feature lists, against what you know shipped this cycle. Do not put a count in the prose:
   every one that was there had drifted, and the files they describe carry their own.
7. <a id="release-step-7"></a>Run mutations for the changed behaviour, including new failure cases for a new
   capability. Start with `--near <last tag>` and add affected callers or runners
   when a shared contract changes.

   **And `scripts/mutate_python.py`, whole, from 2026-10-10.** Its rows break the gate
   scripts and it has no `--near`; it takes a few minutes. This step named only the Rust and
   frontend tables, so nobody ran it, and when 26.10.15 did, four of its rows were not
   caught: two expected a count of fixture paths the tree had outgrown, one tested a rule
   removed by design on 2026-10-04, and one was right. `check_workflow_parity.py` marked a
   drifted step and cut both lines at 100 characters, so a change further into a long step
   printed as two identical lines; it now prints where the two part.

   **`--near` and not `--since`, from 2026-10-04.** `--since` reruns every mutation in a
   file one line of which changed. Three files hold hundreds each, so the 26.10.4 release
   reran 981 Rust mutations in 85 minutes and 495 frontend ones in ten, half of the whole
   release, and found one blunted test, a few lines from the change that blunted it.
   `--near` selects a mutation whose target is within 25 lines of a changed line, or whose
   test changed: 381 Rust and 260 frontend on that same tree, the blunted one among them.
   `scripts/mutation_since.py --self-test` is its control.

   **Since 2026-10-05 a test counts as changed only where a test can be.** The rule looked
   for each test's name in every changed file, and the tables and `docs/VERIFICATION.md`
   name tests in quotes, so an edit to either selected them: on the 26.10.5 tree 853 Rust
   mutations where the corrected rule selects 743. That run took 75 minutes and found one
   mutation not caught: `tpdf text -o` checked for an existing output twice, so removing
   the older check changed nothing.

   **Run the tables in a checkout of their own, so this one stays free.** A harness edits
   source files while it runs, and for that hour nothing else may build, test or run the
   window here. A worktree has its own `target`, and the generated inputs are copied in
   because git does not carry them:

   ```
   W=~/.cache/tpdf/mutations
   git worktree add --detach "$W" HEAD            # the first time
   git -C "$W" checkout --detach "$(git rev-parse HEAD)"   # afterwards
   rsync -a vendor/pdfium/ "$W/vendor/pdfium/" && rsync -a testdata/ "$W/testdata/"
   (cd "$W" && npm ci --ignore-scripts)
   (cd "$W" && python3 scripts/mutate_rust.py --near <last tag>)
   ```

   It runs a commit. For work not yet committed, `git stash create` prints a commit of the
   tracked changes without touching the tree; check that out there. The first build is cold,
   about a minute on an M5. A push does not wait for the tables; the tag does.

   **A Rust selection of nothing is a pass only when no Rust source changed** (since
   2026-10-23). With no `.rs` file and no row of `scripts/mutation_rows/rust/` changed since
   the ref, `mutate_rust.py` prints `[OK] no Rust source changed since <ref>; nothing to
   mutate` and exits 0, as a dependency release does; a lockfile does not count. A Rust file
   that changed with no mutation near it is still refused with exit 1. The frontend and
   viewer harnesses still refuse every empty selection.

   **What that gives up is run afterwards, not dropped.** A change far from a mutation, in
   the same file, can still blunt the test for it, and only the file rule finds that. So
   once the release is out, run `--since <the tag before>` with `--resume` on all three
   harnesses, with nobody waiting on it; a survivor there is a defect fixed in the next
   commit, and is written beside this release's record. Always run the full quality gates once, and
   the relevant native checks. Record the selected mutation count and any gaps.

   **No full GUI mutation table is required for an ordinary release.** A dependency
   pin, version bump or checks-profile/bundle-path correction does not by itself
   justify every historical mutation. Check the affected behavior and prove the
   affected observer with representative controls; expand only when a shared
   behavior cannot be covered by a bounded selection.

   **Release scope, corrected 2026-09-10:** a new capability, a large diff, or a
   month elapsed does not by itself justify every historical mutation. Full
   tables are for a harness-wide change or a shared contract whose reach cannot
   be bounded. Run the affected table when its test runner changes (for example,
   the frontend table when upgrading Vitest). The historical timings below explain
   the cost; they are not an additional requirement to run those tables.

   **How much of them to run is a decision, and this step used to duck it.** It read *"if any
   of the code they cover changed"*, which is true of nearly every release and therefore meant
   the full 735 in practice: about 1 hour 40 minutes, of which the viewer table alone is 55.
   Measured over the `26.8.7` cut, that whole spend produced **one** finding.

   ⚠ **Both of those figures are stale, and the tables have roughly doubled since.** Measured
   on the `26.9.1` cut, **Windows x64, 2026-09-02**, running all three end to end:

   | table | mutations | wall | each |
   |---|---|---|---|
   | `mutate_rust.py` | 646 (9 skipped as macOS-only) | **61.1 min** | 5.7 s |
   | `mutate_frontend.py` | 593 | **15.9 min** | 1.6 s |
   | `mutate_viewer.py` | 92 | **89.9 min** | 58.6 s |
   | total | 1,331 run of 1,340 | **2 h 47 min** | — |

   So the full pass is **1,340** rather than 735, and about **2 h 47 min** rather than 1 h 40.
   The shape of the spend is also not what the sentence above implies: the viewer table is 7%
   of the mutations and **54%** of the time, because each one rebuilds and drives a window,
   while the front-end table is 45% of the mutations and 10% of the time. If only one table
   can be afforded, the two cheap ones are 77 minutes for 1,239 mutations.

   The older figures are left above rather than overwritten: they were taken on a different
   cut and their platform is not recorded, and a number whose platform is unstated is not one
   to replace with a Windows reading. What the two together support is the *trend* — the
   tables grow with the code, and this step's cost estimate goes stale silently, because
   nothing measures it but a person running it. Steps are only
   ever added to a checklist — this is the first one this file has ever narrowed — so the
   narrowing states its own trigger rather than leaving it to whoever is tired:

   **The default is the narrow pass**, which is minutes rather than hours:

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
   there, while `git diff --name-only` reports forward slashes on every platform — so the set
   membership test matched nothing and `--since HEAD~1` over a commit that changed
   `docinfo.rs`, which **48** mutations aim at, selected **0**. What made that cost nothing is
   the guard beside it: an empty selection is refused with *"this run proved nothing, which is
   not the same as a green table"* and exit 1, so the flag was unusable here rather than
   quietly certifying an empty run.

   ⚠ **The narrow pass is still genuinely partial, and `--since`'s reach is shorter than its
   scope.** A mutation is selected by the file it edits; a change in one module can stop a
   mutation in another from being caught without that other file appearing in any diff. Each
   run prints what it left out — the count against the table's total, and the changed files
   no mutation aims at — and ends by saying it is not the full table. That is still the thing
   to run before a release that qualifies above.

   ⚠ **A `--runner` run validates only that runner's mutations.** So the narrow pass cannot
   report a mutation registered against the wrong runner — which is exactly what `26.8.7`
   shipped and what the full table refused to start over. If the narrow pass is what you ran,
   the table's own consistency is unverified, and `scripts/gates.py`'s `anchors` gate covers
   the anchors but not the runner assignment.

   **Nor does `anchors` cover whether a mutation still compiles, which is what the `types`
   gate was added for on 2026-09-21.** An anchor that still matches is not a mutation that
   still works: the `before` string can sit untouched while the code around it grows a return
   value, an enum arm or an argument the `after` does not account for, and the harness then
   gets a compile error where it wanted a red test. Three of those were found in one week,
   each by a `--since <tag>` run of a table measured in tens of minutes, and each invisible
   until then — `git status` clean, `anchors` green. `scripts/check_mutation_types.py` applies
   every mutation, type-checks, and puts the bytes back; it is in the gate list, so a stale
   replacement now goes red in seconds on the run that made it stale. Its own cost, measured
   on this Mac with a warm build tree: **0.4 s** when nothing under `src-tauri/src` or `src`
   has moved since the last run, **1.8 s** after a TypeScript edit, **~16 s** after a Rust
   one, and **22 s** for the whole 2,389-mutation table from an empty cache — thirteen
   compiles, because mutations with non-overlapping anchors are applied together and the
   compiler's own file:line is what names the culprit in a failing batch.

   Two things to know before relying on it. **It is the one gate that writes to the working
   tree**, so do not run it and a mutation harness against the same checkout at once;
   a killed run is recovered from `.mutations/types-inflight.json` on the next one, which
   says so loudly. And **the two groups are judged by different criteria**: Rust must
   compile, because cargo refuses to run a mutation that does not, while TypeScript is
   checked only for names that *resolve*, because vitest never type-checks and a good third
   of the front-end table breaks the types on purpose — passing a slot number where a branded
   `FilePage` is wanted is how you mutate a page-addressing bug into existence. Requiring the
   front end to type-check reported 40 mutations of which 36 were deliberate; requiring it to
   resolve reported 5, and all 5 were vacuous.

   ```
   scripts/check_mutation_types.py              # both groups, using the cache
   scripts/check_mutation_types.py --all        # ignore the cache
   scripts/check_mutation_types.py --group rust # one group
   scripts/check_mutation_types.py --list       # what would be checked, and what cannot be
   scripts/check_mutation_types.py --self-test  # the control
   ```

   `--self-test` is the control and takes about 10 s. It plants a replacement that cannot run
   beside real mutations and requires the sweep to name it, requires the real ones beside it
   to come back clean, and — the arm that is easy to leave out — requires a planted
   *type-only* TypeScript break to pass while being **counted**, so a deliberately narrow
   exemption stays distinguishable from a checker that saw nothing.

   **Where the rows are.** Since 2026-10-15 the Rust and frontend rows live in
   `scripts/mutation_rows/rust/` and `scripts/mutation_rows/frontend/`, one part per stretch
   of the table, joined in order by each folder's `__init__.py` into the harness's `MUTATIONS`.
   A new row goes at the end of the part for its area, or of the last part. No harness file,
   row part or window-check phase may pass 3,000 lines: the `mutations` gate
   (`check_mutation_test_files.py`, `HARNESS_CEILING`) fails it, and a part past it is cut in
   two. The window check's phases are likewise in `src/lib/viewercheck/`, run in order by
   `viewercheck.ts`.

   **The full tables:**

   ```
   scripts/mutate_rust.py          # the modules in FILTERS, `cargo test --lib`
   scripts/mutate_frontend.py      # the modules under src/lib, `vitest`
   scripts/mutate_viewer.py        # every runner below, in one pass
   scripts/mutate_python.py        # the gates themselves, ~30 s for the table

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
   # "page delete"`. Select release scope by the rule above --- the flag
   # exists because re-proving a hundred mutations that could not have moved is
   # somebody waiting, not because a subset is ever the gate.

   # All three also take `--since <ref>`, which needs no knowledge of the
   # mutation names: it runs the ones whose FILE the diff touched, working tree
   # included, prints how many it left out and which changed files no mutation
   # aims at, and exits 1 rather than looking green when it selected nothing.
   # Its reach is shorter than its scope --- a change in docmodel.rs can stop a
   # mutation in save.rs from being caught --- so include affected callers when
   # selecting release scope under the rule above.
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
   from (nothing to do, and it says so — silence there is indistinguishable from the check
   not having run), or it is the mutation that run wrote (restored from the backup beside the
   record, verified), or it is neither, in which case somebody has edited it since and the run
   **refuses and exits 1**. A refusal is right: clobbering a repair made by hand is worse than
   the mutation it would undo. Delete `.mutations/<harness>.json` to clear it.

   *Reuse needs the flag.* Verdicts already proved are reused only if the tracked tree
   fingerprints identically — `HEAD`, the full `git diff HEAD --binary`, and every
   untracked-but-unignored file's digest. Any edit at all discards all of them, and the run
   says which of the two happened. That is blunt on purpose: a mutation's verdict is a claim
   about the whole suite, so an edit anywhere can move it — and the practical consequence is
   worth knowing before it surprises you: editing a document, or one of the harnesses, throws
   the verdicts away exactly as editing `search.rs` does. Finish editing, then resume. A
   reused line is printed with `[reused]` on the end and the summary states how many came from
   an earlier process.

   `mutate_viewer.py` also skips the baseline for any runner whose every mutation is reused
   — that baseline was taken against this same tree when the verdicts were, so building and
   running it again costs about 78 s and establishes nothing. A fully reused
   `--runner structure --only ...` run measured **0.14 s against 75 s**.

   **Two things it does not cover, both said rather than papered over.** The fingerprint
   cannot see gitignored inputs — the generated corpus under `testdata/`, `vendor/pdfium/`,
   `node_modules/`. `mutate_viewer.py` closes the largest part of that by handing the
   fixtures its chosen runners open to the fingerprint explicitly; the other two do not, so
   regenerating the corpus between a kill and a resume is a reason to drop the state. And the
   verdicts are kept whatever the flag says, so a narrow `--only` run in the middle of a
   killed table adds one verdict rather than destroying the rest — the first draft wiped the
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
   15 on 2026-08-03 against an actual 36, 98 and 31 — a tally in prose, in the one
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
   — but only after `--`. `cargo test --lib a:: b::` is cargo's own argument error, which
   reads like the feature being unsupported.

   `mutate_viewer.py` drives **ten** runners, chosen per mutation and filterable with
   `--runner`. The `structure`, `search` and `encodings` ones need no webview and no bundle, so
   they neither wait for one nor require an unlocked screen; each rebuilds one example and runs
   it, at a measured **23--24 s** a mutation. Every runner prints the same `[FAIL] <name>` lines
   and the same summary, so the cross-check, the byte restore and the name validation are
   shared rather than copied. `RUNNERS` in the script is the list; the three probe runners
   share `search-probe` and differ only in the fixture they open.

   **That said seven and "all six" in one paragraph, and both were wrong** — corrected
   2026-08-21 by asking the script rather than by reading the page:

   ```
   python3 -c "import re,pathlib; print(re.findall(r'\"([a-z-]+)\"\s*:', re.search(r'RUNNERS\s*[:=].*?\n\}', pathlib.Path('scripts/mutate_viewer.py').read_text(), re.S).group(0))[::3])"
   scripts/mutate_viewer.py --runner <name> --list | grep -c 'expects:'
   ```

   Two numbers in one sentence disagreeing with each other is the cheapest possible tell that
   neither was measured, and it sat here through several increments that added runners. The
   split as of 2026-08-21: `viewer` 47, `viewer-tagged` 12, `viewer-mixed` 3, `viewer-encodings`
   2, `viewer-comments` 1, `crop-rotated` 1, `crop-content` 1 — 67 needing a window — against
   `structure` 4, `search` 2, `encodings` 2, which do not. Read it from `--list`, not from here.

   `viewer-tagged` is the viewer harness against `tagged.pdf`, and it exists because the two
   tagged-reading-order checks `[SKIP]` on every other corpus. `viewer-mixed` was added on
   2026-08-17 for the same reason on a different property: a page carrying its *measured*
   size to wherever it moved is only observable where the pages are different sizes, and
   `mixed.pdf` is the one corpus that qualifies — everywhere else the layout's estimate and
   the truth are the same number. A skipped check is in the name
   set and cannot go red, so a mutation aimed at one reported **SURVIVED** — the most
   misleading verdict this harness produces, since it reads as a gap in the checks rather than
   a fixture that does not exercise them. The baseline validation now refuses that case
   explicitly, alongside the zero-match and ambiguous-prefix ones.

   The viewer runner is different in kind and slower for it: it rebuilds the bundle and runs
   `viewer_check.py` per mutation — a measured **37 s**, with a further **78 s** per runner for
   its baseline build and the clean rebuild afterwards — a whole-table run measured **3010 s**,
   50 minutes for all 75 — because what it covers — the application's
   own command list, the window shortcuts, and the search behaviour that only shows up
   against a real document — is reachable from neither `cargo test` nor `vitest`. It needs
   an unlocked, unoccluded screen for the same reason `viewer_check.py` does. It reads check
   results from **stdout only**: `viewer_check.py` writes its own verdict on the run to
   stderr in the same `[FAIL] ` shape, and counting those as checks is what made its first
   run report all ten mutations as broken.

   Each mutation names the test expected to notice, and a mutation nothing caught is
   reported as a defect in the **suite**. Three properties keep that verdict honest: both
   cross-check the failure count two ways, both treat a run with no summary line as broken
   rather than as a survivor, and both **refuse to start** if a mutation names a test the
   suite does not define — derived from the runner's own listing, since a name that cannot
   go red reports SURVIVED and reads as a gap in the tests. `--list` prints the pairs without
   running anything.

   **A module absent from `FILTERS` is only half the failure, and the loud half.** The
   guard refuses a run whose mutation names a test it cannot see, which is what caught that
   list being forgotten five times. It cannot catch the sixth shape: a module in neither
   `FILTERS` **nor** the mutation table. `fingerprint.rs` was that on 2026-08-19 — nothing
   refused to start, because nothing was aimed at it, and its central comparison turned out
   to be provable by nothing. When a module lands, add it to `FILTERS` *and* write a
   mutation, and do not wait for the guard to ask.

   **All three run on Windows as of 2026-08-19. Two did as of 2026-07-30 — 22/22 and
   75/75 — and neither did before that.** Read the first sentence as dated too: it is the
   second one that expired without anything going red, because `mutate_rust.py`'s table grew
   two macOS-only mutations on 2026-08-17 and the guard that validates test names then
   refused the whole run here. The three defects behind 2026-08-19 are each in
   `docs/TRAPS.md`, and the shape they share is worth more than any of them: **a harness that
   has never run on a platform produces no failures there, and neither does one that passes.**

   `mutate_viewer.py` had never completed a run on Windows at all, for two independent
   reasons that had to be fixed in order. Its five probe runners named their binaries as
   relative forward-slash paths, which `CreateProcess` refuses — so the run died on the
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
   been quietly translating line endings for the anchors that span lines — the trap of that
   name has it, and it is also a correction to what an earlier entry prescribed.

   **`mutate_rust.py` then stopped running here again, and the guard that stopped it was
   right.** `menu.rs` and `keylayout.rs` are macOS-only, so `cargo test` never compiles them
   and the two mutations aimed at them name tests that do not exist on Windows — which the
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
   measured at **69 s per mutation**, which for 231 of them is 4.4 hours — a figure nobody
   can pay per feature, and it was almost entirely two things that have nothing to do with
   the mutations:

   - **An editor holding the build lock.** Every mutation writes a file under
     `src-tauri/src`, and rust-analyzer answers each write with
     `cargo check --workspace --all-targets`, which takes the build directory's lock. Cargo
     says so — `Blocking waiting for file lock on build directory` — and a no-op
     `cargo test --lib --no-run` measured **28.2 s** against **0.2 s** with the editor idle.
     The harness now sets `CARGO_TARGET_DIR` to `src-tauri/target/mutations`, so it shares
     no lock with anything. One cold build (**42 s**, 2.4 GB, inside the already-ignored
     `target/`) and it is warm for every run after.
   - **607 tests to check one assertion.** Each mutation names the one test it expects to
     redden, and the harness ran the whole filtered suite anyway. Timing the modules
     separately says where that goes: `save::` 32.4 s, `print::` 32.3 s, `keylayout::`
     17.0 s, and **the other fifteen modules 0.1 s between them** — twelve tests that
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
   total — the total moves every time somebody writes a mutation, and nothing goes red when
   it does.

   **It expired without anybody changing the harness, which is the part worth carrying.** By
   `26.8.11` the same table cost **40.0 s** per mutation, measured twice over four minutes —
   5.6 hours for 508 mutations, against the 1.77 s above. Nothing had regressed in the
   harness: the *crate* had grown, and every mutation touches one file, so cargo re-codegens
   the crate and relinks a test binary that full debug info had taken to 33 MB. A cost that
   goes stale because the subject grew is invisible to every check here, exactly like the
   counts this file keeps getting wrong.

   Fixed the same day by building the mutation target with `CARGO_PROFILE_DEV_DEBUG=0`,
   measured interleaved against the same command in a second target directory — 22.6/30.1/27.0 s
   against 3.9/3.7/3.3 s, and the directory 1.7 GB rather than 14 GB. The whole table then ran
   in **1316 s including its 178 s cold build**: all 504 caught, 4 skipped as not runnable on
   macos. It is safe because `debug` is debug *information* only — `debug_assertions` and
   overflow checks are separate knobs and are untouched, so every test runs the program it ran
   before, and what is given up is line numbers in a panic backtrace that nothing here reads.
   The reasoning is in `scripts/mutate_rust.py` beside the constant.

   **Decompose before believing a per-mutation figure.** The no-op freshness check is 0.8 s
   warm; touching one source file costs 14--15 s of it; the named test itself runs in 0.03 s.
   Three measurements said the rebuild was the whole cost, which is what made the lever
   obvious — and the first theory, that a 33 MB binary was slow to *load*, was wrong and took
   one direct run of the test binary to refute.

   **What the front-end table costs, and why it did not fall as far.** `mutate_frontend.py`
   got the same narrowing — it runs the test *file* holding the mutation's own test, chosen
   from the file vitest prints beside every test in the control run's listing, with the same
   fallback to all twenty files whenever the narrow run finds nothing red. That took it from
   5.8 s to about 4.9 s per mutation, and no further, because the cost is vitest's own
   startup: **4.6 s for a single small file**, and measured the same through `npx` or `node`
   directly, forks pool or threads. The full table was **1570 s for 322 mutations**; it is
   **368** as of `26.8.7`, all caught, so scale that by the per-mutation figure rather than
   reading the total.

   Getting materially below that means keeping one vitest process warm in watch mode and
   attributing each re-run to the mutation that triggered it. That is worth roughly 26 minutes
   down to ten, against a harness that can mis-attribute a run — deliberately not built yet.

   One thing the narrowing did break, and it is recorded as a trap: with one file in the run,
   a summary line can read `Tests  2 failed (2)` with no `passed` segment, which the count
   regex required. One mutation of 322 reported `no summary line -- the run did not finish`
   for a mutation its test had caught. Fixed, and proved on five summary shapes.

8. <a id="release-step-8"></a>`npm run tauri build` (on Windows,
   `npm run tauri build -- --config src-tauri/tauri.explorer.conf.json --bundles nsis`)
   and smoke-test the normal bundle externally, including opening
   and rendering a PDF with the development engine hidden. Normal bundles exclude the
   in-app harness, so `scripts/viewer_check.py` must instead use a separate checks build
   (commands at the top of this file), against both `testdata/text-heavy.pdf` and
   `testdata/vector-heavy.pdf`. Keep the normal-bundle and checks-build results separate.
   On Windows also run `print-probe` (§8), which is the only check that reaches a real spooler.
   The Explorer overlay is required for the installer under test, as it is in
   `release.yml`: a plain local build omits `tpdf_shell.dll`. The 26.10.26
   installation check caught that omission while its other 24 checks passed.

   **The command-line tool, from the bundle and not from `target/`** (from 26.9.21): run
   `tpdf.app/Contents/MacOS/tpdf-cli --version` and `... verify testdata/incr-signed.pdf`, and on
   macOS `codesign -dv --verbose=4` on it (Developer ID, `runtime`), with the development engine
   hidden --- the tool finds PDFium beside itself in the bundle, and a run that finds the
   development tree's proves nothing about that. Then *Install command-line tool…* from the
   bundle and `tpdf --version` through the link. On Windows, `tpdf-cli.exe verify` in a console
   from the installed folder. *The command-line tool* (§ above) has the rest.

   Capture only the test process's own window: use its CGWindowID on macOS and
   `PrintWindow` on Windows. For the Windows capture, verify that an overlapping
   control window does not change the captured application content. A desktop-rectangle screenshot can
   contain unrelated windows and is not valid application evidence.

   **One more on Windows, and it blocks the tag rather than decorating it.** This step is
   where a Windows-only mechanism gets exercised, and it lives here rather than in a step of
   its own on purpose: fifteen sentences across five files and `release.yml` name the steps of
   this list by number, so inserting one renames every reference — the trap of that name,
   arriving in the list that made it worth writing.

   ```powershell
   cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- `
       "$env:USERPROFILE\Downloads" --pages 3 --regions 40
   ```

   **That was written in `cmd.exe` syntax until 2026-09-02**, on machines this portfolio
   documents as PowerShell 7 — `^` is not a line continuation there and `%USERPROFILE%` does
   not expand, so the first line would have run with a literal `^` argument and the second as
   a command of its own. It was the only cmd-syntax block in this file; every other Windows
   example here is a ```powershell fence using `$env:`. Nothing caught it because nothing ran
   it, which the note below says in as many words.

   **`redact-reach-probe` against real scanned documents.** Windows OCR rests on
   `win-ocr-probe`'s synthetic validation, which reads clean text at two sizes and therefore
   never puts the engine near its limit; the corpus sweep is what would, and
   `docs/THREAT-MODEL.md` §20 says so about itself. Note the flags differ from the macOS
   invocation in §`redact-reach-probe`: **drop `--no-gate`**, because the gate is the half
   under test here. Counts and shapes only leave the process — no page text, no recognised
   string, no filename beyond the stem. Record the result where this file's macOS numbers
   live, **labelled Windows**.

   **It ran for the first time on 2026-09-02**, and the reading is in `docs/PLAN.md` §6
   under *The gate on Windows*. 109 documents, 8,940 regions, 5,254 read back: **68.1%**
   taken whole, against 67.7% and 66.9% on macOS, so the ratio travels. The gate cost
   **4.4x** the cheap half here (23.5 s against 5.3 s) rather than the 40x quoted for macOS
   above — that figure is Vision's, and Windows OCR is much faster. Every bucket closed
   and no `[WARN]` printed. Three axes read differently from the Mac's and one of them
   reverses the token-length argument in the ⚠ paragraph above; that paragraph now carries
   the platform label it lacked.

   **Windows x64, 26.9.3 release check, 2026-09-07:** 117 documents opened, none
   refused; 9,808 regions sampled, 6,399 taken whole (65.2%). The gate read back
   5,775 regions on 211 pages and showed 2,256 unreadable (39.06%), in 48.5 seconds.
   No arithmetic warnings were reported. This is the current local corpus, not a
   controlled comparison with the earlier run.

   **Windows x64, 26.9.6 release check, 2026-09-12:** 133 documents opened, none
   refused; 11,728 regions sampled, 8,062 taken whole (68.7%). The gate read back
   7,556 regions on 260 pages: zero still read as text, 3,337 were shown unreadable,
   and 4,219 remained unverified. The sweep took 29.2 seconds; no arithmetic
   warnings were reported. The same build passed all 10 real-spooler print checks.

   **Windows x64, 26.9.8 release check, 2026-09-16:** 141 documents opened, none
   refused; 12,008 regions sampled, 8,326 taken whole and 3,682 not wholly
   removable. The gate read back 7,820 regions on 267 pages: zero still read as
   text, 3,492 were shown unreadable, and 4,328 remained unverified. The sweep
   took 30.9 seconds; no arithmetic warnings were reported. Unverified is not a
   clean verdict. This is the current local corpus, not a controlled comparison
   with the earlier runs.

   **The state line stays, because the debt is now a different one.** The run separates
   nothing: the two corpora share no document, so a more permissive engine and a corpus of
   cleaner type explain the Windows figures equally well, and §6 records that as open.
   **This is the one that cannot be moved to CI**, and the reason is what the probe is for:
   it needs a corpus of *real* documents, which a hosted runner has none of and must not be
   given.

   **`worker-probe` used to be listed here and is a CI step instead, since 2026-09-01.** The
   rewrite's output channel is `dup2` before `exec` on macOS and `DuplicateHandle` into a
   suspended child on Windows, so one platform's result says nothing about the other; macOS
   was at 42/42 and the last Windows run was **19/19 on 2026-08-24**, before the four
   verification-side checks, the five writing-side ones and the six copy-and-print ones
   existed — and `26.9.0` shipped without a Windows run. The probe needs no screen and takes under a second, so both
   workflows' `gates` job now runs it on both legs against `testdata/text-wide.pdf`, and every
   push proves it rather than every release. `text-wide.pdf` rather than the `text-base14.pdf`
   this file's own invocation uses: `scripts/ci_fixtures.py` generates nothing from
   `make_text_pdf.py`, and the macOS reading is the same against both fixtures, so the probe
   is not fixture-specific. The general form is worth carrying — **a requirement that cannot go
   red belongs in a runner, not in a checklist**, and a checklist step is what you write when
   no runner can hold it, as with the sweep above.

   On macOS also `scripts/menu_check.py` and `scripts/save_check.py` against the bundle.
   **Both take a `.app` bundle.** `save_check.py` also takes a document;
   `menu_check.py` takes no document. Both launch through `open`, so the application
   starts through LaunchServices without a stdout supplied by the harness. Pass the
   bundle directory, not its executable:

   ```
   python3 scripts/menu_check.py --self-test    # the duplicate rule, both directions
   python3 scripts/menu_check.py                # the release bundle at the default path
   python3 scripts/save_check.py src-tauri/target/release/bundle/macos/tpdf.app \
       "$PWD/testdata/text-heavy.pdf"
   ```

   Both probes refuse an already running bundle identifier and target only the PID they
   launched. `save_check.py` needs Python 3.10 or later, and over ssh `python3` is the
   system's 3.9 unless `/opt/homebrew/bin` comes first on `PATH`; on 3.9 it stops at a
   `TypeError` about `ignore_cleanup_errors` before it starts anything. Use a separately identified test bundle when the installed app is open.

   Run the `--self-test` first. It replays the application menu as measured before and after
   the duplicate `About tpdf` was removed, so it shows the rule calling one a defect and the
   other clean — a check that has only ever passed is not known to be able to fail, and this
   one costs a second.
   Between them they read the two surfaces no test in either language can: the menu bar as
   the reader sees it — a duplicate label shipped in 26.8.6 and reached a reader before
   anything here noticed — and the file on disk after a Save, which nothing else in the
   repository writes. Both need an unlocked screen; `save_check.py` refuses a locked one
   rather than reporting an application that ignores its menu.

   **On Windows, run the installer itself and use what it installed.** One command, on a
   Windows machine where tpdf is not installed, in a session with a desktop:

   ```powershell
   python scripts\installed_check.py <path>\tpdf_<version>_x64-setup.exe testdata\text-wide.pdf --word Ledger
   ```

   It installs silently into a folder of its own, reads the installed files and the tool's
   version, removes a word with the installed `tpdf-cli.exe` and reads the copy back, starts
   the installed application on the document, waits for its window, for a worker that maps
   the installed `pdfium.dll` and for the session file to name the document, asks the window
   to close, runs the installer a second time over the installed copy, as an update does,
   uninstalls, and reads the registry and the folder afterwards. It refuses to start
   where tpdf is installed or running. A step that does not end is a failed check and the
   uninstall still runs. `--control NAME` breaks one thing on purpose (`no-install`,
   `one-install`, `no-uninstall`, `engine`, `document`, `time-limit`) and exits 0 exactly when the checks
   aimed at it failed and no other did; run them when the script or the installer changed. Over ssh the window needs an interactive scheduled task
   (`schtasks /Create ... /IT`, then `/Run`): started from the ssh session itself the
   application has no desktop to open a window on.

   **First run, 2026-10-09, Windows 11, against the published 26.10.13 installer:** 20 of 20
   checks passed. Two controls failed as they should: with `--version 26.10.12` the tool's
   version and the window's title failed, and with a word the document does not have the
   run stopped at the tool's first check. The document and the word are not
   interchangeable: in `text-base14.pdf` the Windows recogniser does not read the 10 pt
   control word, so every removal there is written and reported as not verified.
   **Second run, the same day, after an independent read-only review:** the installer had
   been started outside the protection that runs the uninstall, so one that hung would have
   left tpdf installed; eleven checks had never been seen to fail; and the application's
   process was sampled only until its window appeared. All 22 checks passed, and each of the
   five controls ended with exit code 0. `no-uninstall` showed that the installer does write
   the `.pdf` class. Two checks are still not known to be able to fail: that the
   application's own process maps no PDFium, and that the application's own registry key
   reads as before, which it did before, during and after the install on that machine.
   What it does not do: read the page the window drew, or edit and save in the window. A
   normal build has no check harness, so those stay with `window_checks.py` on a checks
   build.

   **Windows produces an MSI and an NSIS installer**, since 2026-07-30. It did not until
   then, and the rule that came out of it is worth knowing before adding a probe:
   **`src/bin/` must contain only declared bin sources.** The bundler enumerates that
   directory and registers the first entry no `[[bin]]` `path =` claims — a `.rs` file is
   always claimed, a *subdirectory* never is — so `src/bin/backend_probe/`, which held only
   `imp.rs`, became a phantom binary and failed WiX. Those bodies now live in `src/probes/`.
   See the trap of that name for the four theories that were wrong first.

   **It no longer ships the probes.** Until 2026-07-31 the installer carried all 17 spike
   and benchmark executables, including a sandbox prober and a hostile-document harness,
   because they were `[[bin]]` targets of the bundled crate. They are `[[example]]` targets
   now: cargo still builds and links them, `scripts/gates.py`'s `bins` gate still covers
   them via `--examples`, and the bundler does not see them. The MSI payload was three files
   — `tpdf.exe`, `tpdf_lib.dll`, `pdfium.dll` — verified by extracting it, and the MSI went
   16.7 -> 8.0 MB with the NSIS setup 8.8 -> 5.8 MB.

   **It is four files as of 2026-08-02**, and the fourth is the point of the notices work:
   `THIRD-PARTY-NOTICES.md`, 469 KB, which a binary distribution owes and which nothing but
   an extraction can confirm actually shipped. Re-extract and list the payload after any
   change to the resource map — that is the only step here that reads the artifact rather
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
   bytes over 9,197 lines, and 469,298 + 9,197 = 478,495 exactly — the Windows runner
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

   **From 26.9.21 there is one more on purpose: `tpdf-cli.exe`**, the command-line tool, a
   second `[[bin]]` the bundler ships beside `tpdf.exe` (`src/bin/tpdf-cli.rs`). So a released
   MSI should hold four files, and three means the tool did not ship. Since 26.9.21 the
   release workflow checks this itself. The *Test packaged CLI and installed Python API*
   step now extracts both the MSI and NSIS setup, requires `tpdf-cli.exe` beside `tpdf.exe`,
   and exercises both through the installed Python client. The original presence check was
   proved failing on 26.9.20's own installers first, on
   the Windows development machine: the MSI extracted to `THIRD-PARTY-NOTICES.md`, `tpdf.exe` and `pdfium.dll` only,
   and the step stopped at *the MSI does not carry tpdf-cli.exe*. Still extract one as below
   after a release that changes it, and run
   `tpdf-cli.exe --version` and `tpdf-cli.exe verify` on a signed PDF from the installed
   folder, in a console --- the one Windows check a harness cannot stand in for, since the
   tool exists for a console the application never has.

   What follows operationally: **read a payload count off a released artifact, never off a
   local build.** `gh release download <tag> --pattern '*.msi'` and extract that — it needs
   no Windows, takes a minute, and works on a draft, so a rehearsal tag can answer it before
   the real one is cut.

   Its absence is not a defect on its face, since the binary links the `rlib` and does not
   load it — but it is one more reason the installed app has to be *run* on Windows and not
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
   destination filenames, so identify the rows by size — the count and the sizes are the
   facts here, and the count is what disagreed with this document.

   **Build before hiding the development library, not after.** The bundler copies
   `../vendor/pdfium/bin/pdfium.dll` as a resource, so a build with it already moved aside
   fails at `resource path ... doesn't exist` — which reads like a broken checkout rather
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
   **absolute** path — the app resolves a relative one against its own working directory, and
   the failure is a plain "could not find the file" that reads like a broken bundle. And make
   sure the fixture has been **generated**: `testdata/*.pdf` is gitignored, an absent one
   produces the same red, and the first two attempts here died on `text-heavy.pdf`, which this
   machine had never built.

   Move the *bundled* library aside as well, once, and confirm the run fails. A pass on its own
   cannot say which of the candidate paths resolved; the failure names it.

   **Run on macOS 2026-07-31, and it failed — the Windows fix did not carry over.** The
   `.app` built cleanly and `find` reported the dylib present, which is exactly how this
   stays hidden: `Contents/Resources/pdfium` existed, and it was a **file**, not a directory.
   The bundler read `"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/"` as a target *path* and
   renamed the dylib to `pdfium`, so both bundled candidates missed and the app died on
   `Contents/Resources/libpdfium.dylib` — `0/1 checks passed`, three `could not load Pdfium`
   lines naming the path. The trailing slash is not a directory marker on this bundler.

   Fixed by naming the file in `tauri.macos.conf.json`
   (`"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/libpdfium.dylib"`), which lands it where
   the second candidate already looked. `tauri.windows.conf.json` is deliberately **not**
   changed: WiX ignores the target directory either way and the resource-root candidate
   catches it there, and that platform cannot be re-verified from a Mac. After the fix, with
   the dev library hidden: **102/102 checks passed, 7 not applicable, 109 names**.

   The failing run before the fix is the negative control, and it is what makes the pass mean
   anything — the same `.app`, the same command, the only difference being where the library
   sits. Keep both halves when repeating this.
   **On Windows, verify the UPGRADE and not only the install — with the released
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

   The packaged executable differs from the loose `target/release/tpdf.exe` by
   Tauri's installer-kind marker: `__TAURI_BUNDLE_TYPE_VAR_UNK` becomes
   `__TAURI_BUNDLE_TYPE_VAR_NSS` for NSIS. Verify exactly one marker replacement
   and byte identity everywhere else, or compare against an extracted matching
   installer. A raw digest comparison against the loose binary falsely rejected
   the correct 26.9.5 upgrade; the difference was exactly those three bytes.

   Measured 2026-08-24, planting 26.8.8's stray `pdfium` file in both legs:

   ```
   shipped 26.8.9 setup, /S      exit 0   pdfium\pdfium.dll  ABSENT
   26.8.10 setup, /S             exit 0   pdfium\pdfium.dll  present, digest matches vendor/
   26.8.10 setup, /S, clean dir  exit 0   pdfium\pdfium.dll  present
   26.8.10 setup, /S, pdfium/    exit 0   pdfium\pdfium.dll  present, replaced
   ```

   The last two are the hook's other branches — a first install and an ordinary upgrade —
   and they are what says the fix costs nothing on a machine that never ran the broken build.

   **Installing writes to the machine you are testing on.** Three keys: `Uninstall\tpdf`,
   `Software\Timo Stein\tpdf`, and `Classes\.pdf`, whose `..._backup` value holds whatever
   handled PDFs before tpdf did. `reg export` all three first; afterwards put the machine
   back by re-running the **shipped** installer into the real location and diffing the
   exports, since re-running the new build would leave an unreleased version installed.

   **If the release adds or changes an NSIS hook, prove it was wired.** A mistyped key and a
   path naming a missing file are both refused — by the build script's schema and by the
   bundler — but a file that exists and defines the macro under another name is skipped in
   silence by the generated script's `!ifmacrodef` guard, and the bundle builds green:

   ```
   grep -n 'installer-hooks' src-tauri/target/release/nsis/x64/installer.nsi
   ```

   That is a source-level assertion and does not replace the A/B above; it is what tells you
   *why* the A/B failed when it does.

9. <a id="release-step-9"></a>Commit as `Release vYY.M.MICRO: <summary>` and push it. Then confirm the `Audit`
   workflow is green **at that commit** before tagging it:

   ```
   gh run list --workflow=audit.yml --commit "$(git rev-parse HEAD)" --json status,conclusion
   ```

   Exactly one run, `completed` and `success`. An empty list is not a pass: it means the
   push has not triggered the run yet, or the filter matched nothing. A red run means an
   advisory not listed in `.cargo/audit.toml` — read it and either update the dependency or
   add it to that file with its reason; do not tag over it. The audit is not a gate because
   its answer changes when an advisory is published, not when the code does
   (`.github/workflows/audit.yml` says so at the top).

   `scripts/publish_release.py` asks the same question again in step 11: it refuses to
   publish unless the newest `Audit` run on the tag's commit has completed successfully.
   Check here all the same, because a red audit found after the tag costs a second tag.

10. <a id="release-step-10"></a>**Rehearse changed release mechanics, then tag for real.** This list ended at step 9 until
    2026-08-03, which left the single riskiest action in the process written down nowhere
    but a comment in `release.yml` — and it is the action that runs unreviewed code paths
    beside the signing key.

    ```
    git tag v26.8.0-rc1 && git push origin v26.8.0-rc1     # rehearsal
    # ... watch it, fix what it finds, delete the tag and the draft, repeat ...
    git tag v26.8.0     && git push origin v26.8.0         # the real one
    ```

    **The `Release` run skips its own gates when CI already passed them on the tagged SHA**, so
    tag only after `ci.yml` is green on both legs for that commit. A tag on a commit CI never saw runs the full gates, about 25 minutes longer on Windows.

    **Rehearse changes to build, signing, notarization or publication mechanics.**
    Release-note wording and dependency pins alone do not require a second build
    under a throwaway tag. When those mechanics are unchanged, verify the real
    release draft before publishing it. The tag glob
    `v[0-9][0-9].[0-9]*.[0-9]*` matches an `-rcN` suffix on purpose so it can be done at all.
    Cutting `26.8.0` took **three** rehearsal tags, and each found a real defect that no
    amount of reading had:

    - `rc1` — both gate legs red. `release.yml`'s `gates` job had been written from
      `ci.yml` and the copy lost the fixture-generation step, so a unit test needing
      `rotated.pdf` failed on both runners while passing in CI and locally.
    - `rc2` — gates green, Windows published, macOS died on `***: no identity found`.
      Nothing had imported the certificate into a keychain yet; the step that signs the
      vendored dylib has to run *before* the bundler copies it, and the Tauri CLI's own
      import happens two steps later.
    - `rc3` — the notarization path itself.

    Clean up between rehearsals, and note the two are separate: `git push --delete origin
    <tag>` **does not remove the draft release**, which persists without its tag and will
    sit in the release list looking like a real one.

    **Delete a draft by id, not by tag.** `gh release delete <tag> --yes` answers *"release
    not found"* for a draft that plainly exists — it resolves the tag through the same REST
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

    A failed run publishes nothing — `release` needs `gates`, and both legs create the
    release as a **draft**. That draft is the last chance to edit the release body, and
    publishing it is step 11 rather than a clause here: see that step for what describing
    it in this sentence cost.

    **One `draft` job creates the release, and the build legs upload into it by id.** That
    is new on 2026-08-17, and it replaced a failure worth knowing about: `26.8.3-rc2` and
    `-rc3` each produced **two drafts under one tag** with the artifacts split, one holding no
    macOS updater bundle and the other no Windows installers. `tauri-action` used to resolve
    the release itself, which for a draft means paging `listReleases` for the tag — its own
    source says *"you can't get an existing draft by tag"* — and that lookup silently came
    back empty. `v26.8.2` logged `Found draft release ...` and was whole; neither leg of rc2 or
    rc3 logged it. **Why the lookup failed is still open**; `releaseId` means nothing looks
    anything up, so it no longer decides whether a release is whole.

    Step 11's asset count stays anyway, and not as ceremony: it is the only check that can
    tell a whole release from half of one, and it would have caught this before publishing.

11. <a id="release-step-11"></a>**Publish the draft, and check it from outside the account.** A green `Release` run
    produces the release assets and shows them to nobody — GitHub hides a draft from everyone
    but repository owners, and its assets sit under `releases/download/untagged-<hash>/`
    rather than under the tag. Meanwhile the *tag* is public, so from outside the repository
    the state reads as a tag pushed by mistake.

    Use `scripts/publish_release.py` for publication. It requires the latest
    `release.yml` run for the exact tag and commit to have completed successfully,
    with a successful packaged CLI/API step in both platform jobs. An older
    green run, a skipped step or an empty job list cannot authorize publication.
    It also requires the newest `audit.yml` run on that commit, pull-request runs
    not counted, to have completed successfully: no run, an unfinished run or a
    failed run refuses publication.
    Without `--publish` the command only checks. GitHub repository owners can
    bypass this helper through the website or direct API calls; do not publish
    a failed draft that way.

    **Count the assets before publishing, and count them with GraphQL.** A complete release
    is **8** files: the `.dmg`, `tpdf_aarch64.app.tar.gz` and its `.sig`, the
    `-setup.exe` and its `.sig`, `latest.json`, and since 26.10.19 the `.deb` and the
    AppImage. It was 8 up to 26.10.10, the last release with an `.msi`, and 6 from 26.10.11
    to 26.10.18. Fewer than that is half a
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
    python scripts/publish_release.py vYY.M.MICRO --publish
    gh release list --repo tstone-1/tpdf                      # second column: Latest
    curl -sIL -o /dev/null -w '%{http_code}\n' \
      https://github.com/tstone-1/tpdf/releases/download/vYY.M.MICRO/<asset>   # expect 200
    ```

    **Read the body before publishing, and if you correct it, send `tag_name` with the
    correction.** It is a literal in `release.yml`, so nothing can make it go stale except
    nobody reading it, and on 2026-08-23 it listed *stamps* under "what it is not, yet" one
    release after they shipped.

    **Read `SECURITY.md` in the same pass**, for the same reason and with no mechanical half
    at all: no gate reads it, and it is the file a stranger is sent to from `README.md` when
    they have something to report. It said *"Nothing has shipped yet — there are no tags and
    no released binaries"* for thirteen releases, corrected on 2026-08-31. The supported-version
    statement is written so that it does not name a version and therefore cannot go stale by
    shipping — what can go stale is the **scope** list, so read that against what this release
    added: a new parser, a new format read, or a new place a document's bytes reach is a line
    in *In scope* or a disclosure that it is not.

    **Half of that is mechanical since 2026-08-28, and the half that is not is named.** The
    *"What it is not, yet"* sentence carries a `<!-- not-built: -->` marker, and
    `src/lib/readme.test.ts` — which already imports the registry for the README's own
    list — asserts that nothing it names is registered, and that every id it names is also
    called unbuilt in the README. So the two copies of that list are provably the same claim
    rather than merely both plausible. It was built after the block was wrong in **three of
    four** releases: stamps in `26.8.8`, *Merge documents* in `26.8.10`, and **true
    redaction** in `26.8.11`, which was published as the release that ships it.

    What it does **not** own is the `26.8.10` direction — a capability that shipped and is
    simply not mentioned. A release body is prose, and requiring it to name all 84 registered
    commands would make it the palette transcribed, so that stays with this step and a person
    reading it. Nor is every phrase covered: *"Signature verification"* names no command,
    because verifying a signature is a behaviour rather than something in the palette. Read
    the feature paragraphs against what you know shipped this cycle; the marker only stops
    the notes calling a shipped command unbuilt. A `PATCH` carrying only `body` resets the draft's `tag_name`
    to `untagged-<hash>`, and publishing in that state attaches the release to no tag —
    while `gh release list` still shows it by name and `gh release view <tag>` cannot see a
    draft at all, so neither of the two obvious instruments reports it. The GraphQL query
    above prints `tagName` beside the asset count, which is why it is the one to use.

    **Put `tag_name` inside the JSON, because `--input` discards every `-f` beside it.**
    This block carried `-f tag_name=vYY.M.MICRO --input body.json` until 2026-08-24, and
    that command does exactly what the paragraph above warns against: `--input` supplies
    the *whole* request body, so the `-f` never reaches GitHub and the PATCH is a body-only
    one. Measured on `26.8.9` — the reply came back `"tag_name":
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

    The `latest.json` asset carries its own copy of that prose — `tauri-action` fills its
    `notes` from the body at build time — and correcting the release page does not correct
    it. Left alone deliberately: nothing in tpdf reads that field, so replacing an asset on a
    published release to fix text no reader sees is the worse trade. See the trap.

    **The `curl` is the point, not ceremony.** `gh release list` reporting `Latest` is our
    own authenticated view; an unauthenticated fetch of a download URL is the reader's, and
    it is the only one of the two that can tell a published release from a draft we happen
    to be able to see.

    This list ended at step 10 until 2026-08-12, with publishing named only in that step's
    closing sentence — and `26.8.0` sat as a draft for **nine days** after a green run that
    had signed, notarized and uploaded everything. A step described inside the prose of
    another step is not a step anybody executes; nothing can go red for it, since no runner
    runs it and no gate covers it. The trap is *A draft release is invisible, and the tag
    beside it says the work shipped*.

    **Then the Homebrew cask** (from 26.10.2). In the tap `tstone-1/homebrew-tpdf`, set
    `version` and `sha256` in `Casks/tpdf.rb` to the published `.dmg`
    (`shasum -a 256 tpdf_<version>_aarch64.dmg`), commit and push. Check it with
    `brew audit --cask --online tstone-1/tpdf/tpdf` and `brew livecheck --cask
    tstone-1/tpdf/tpdf`. The cask's `livecheck` block reads the latest full release: without
    it Homebrew takes the engine prerelease tag `pdfium-...` for the version. To install for
    a test without touching `/Applications`, pass `--appdir=<folder>`. Since 2026-10, Homebrew
    refuses to load a cask from a tap nobody has trusted (*Refusing to load cask ... from
    untrusted tap*); `brew trust tstone-1/tpdf` once per machine.

    **And winget** (first submitted for 26.10.2, as `tstone-1.tpdf`, pull request 445804 to
    `microsoft/winget-pkgs`; not merged when this was written, so the README does not name
    winget yet). A new version is three files under
    `manifests/t/tstone-1/tpdf/<version>/`, copied from the previous version with
    `PackageVersion`, `InstallerUrl`, `InstallerSha256` (of the `-setup.exe`) and
    `ReleaseNotesUrl` changed, on a branch of the fork `tstone-1/winget-pkgs`, titled
    `Update: tstone-1.tpdf to <version>`. Run `winget validate --manifest <folder>` first.
    The fork is a copy of a very large repository, and on 2026-10-28 every git fetch from it
    (a sparse, blobless clone included) was refused with HTTP 429 (*gitmon refuses to schedule
    us*). The REST API still answered, with retries: read the three files through `contents`
    and write the move as one commit through `git/trees`, `git/commits` and `git/refs`.
    `winget install --manifest` needs local manifests enabled by an administrator; without
    that, run the downloaded `setup.exe /S` and read `winget list tpdf`. The installer is
    `nullsoft`, user scope. Over ssh, `winget install --manifest` downloads the installer,
    marks it as from the internet, starts it and then waits for good: SmartScreen holds the
    start, and no window of that session can be answered (winget's log under
    `DiagOutputDir` ends at `Starting: ... '/S'`). A `curl.exe` download has no such mark
    and `/S` runs at once. The installer goes into the folder the last install on that
    machine used, so after `update_check.py` it is `%LOCALAPPDATA%\tpdf-update-check`;
    read `InstallLocation` from the uninstall key instead of assuming `%LOCALAPPDATA%\tpdf`.
    The Windows build machine had `LocalManifestFiles` off again on 2026-10-19, two days after it was on;
    `winget settings --enable LocalManifestFiles` from the ssh session turned it back on.
    `git mv` of the version folder stages only the rename: edit the files after it, then
    `git add` them, and read `git show HEAD:<file>` before pushing. On 2026-10-19 the commit
    carried a `26.10.23` folder holding `PackageVersion: 26.10.22`, and the pull request's
    validation caught it, not anything here.

12. <a id="release-step-12"></a>**Apply the update from the previous release: `scripts/update_check.py` on Windows, `scripts/update_check_mac.py` on a Mac.** This is the only end-to-end
    proof the updater works, and no gate, harness or unit test can stand in for it:
    `update.test.ts` fakes the plugin, so what it covers is the state machine and not
    signature verification, TLS, or the shape of the real `latest.json`. Nothing in this
    repository has ever fetched that file.

    **When a release moves the updater's own crates, test the new updater before the tag.**
    This step proves the previous release can update; a release whose own updater is broken
    passes it, and the damage shows one release later, in copies that can no longer update.
    Build the release tree under a lower version and run the same script on it
    (26.10.16's verification record has the run and the two checks that fail by construction):

    ```
    npm run tauri build -- --config '{"version":"26.10.1","bundle":{"createUpdaterArtifacts":false}}' --bundles dmg
    python3 scripts/update_check_mac.py src-tauri/target/release/bundle/dmg/tpdf_26.10.1_aarch64.dmg testdata/text-wide.pdf
    ```

    **On Windows the crate's version has to go down too**, because `update_check.py` waits
    for a window titled `tpdf v<version>` and asks the installed tool for its version, and
    both come from `Cargo.toml`: with only `--config` lowered, the run ends at the first
    window check without pressing anything. Save `src-tauri/Cargo.toml` and `Cargo.lock`,
    set the package `version` to `26.10.1`, build, and put both files back byte for byte.
    A background `tauri build` ends its `beforeBuildCommand` with exit code -1, so build the
    frontend first and pass `"build":{"beforeBuildCommand":""}` in the same `--config` file:

    ```powershell
    npm run build
    npx tauri build --config <file with version, beforeBuildCommand and createUpdaterArtifacts> --bundles nsis
    python scripts\update_check.py src-tauri\target\release\bundle\nsis\tpdf_26.10.1_x64-setup.exe testdata\text-wide.pdf
    ```

    The run on 2026-10-28, updater 2.14.1 offered the published 26.10.26: 17 of 17.

    Needs two published releases, so it starts from the second one ever cut with the
    updater — first opportunity is applying `26.8.2` from an installed `26.8.2`+1.

    **On Windows, start the previous release by opening a document that lies under a
    folder with a space in its name, and press the update button with that document
    open.** The installer starts tpdf again with the document, and that hand-over lost the
    quotes around the path until 26.10.12: the update was applied from an empty window on
    2026-10-08 and looked right, and the same update with such a document open ended in
    *could not open*. The document has to be on screen again after the restart.
    `docs/TRAPS.md` has the three programs on that route.

    **On Windows it is one command since 2026-10-10**, on a machine where tpdf is not
    installed, in a session with a desktop, with the installer of the release before the
    published one:

    ```powershell
    python scripts\update_check.py <path>\tpdf_<previous>_x64-setup.exe testdata\text-wide.pdf
    ```

    It reads `latest.json` for the version on offer, installs the previous release into a
    folder of its own, starts it on a copy of the document under a folder with a space in
    its name, finds the button `Update to <version>` through UI Automation and presses it,
    and waits for the installer to start the application again: a window of the new
    version from the same folder, the document open again in a worker that maps the
    installed PDFium, the installed tool at the new version, and no second offer in the new
    window. Then it uninstalls and reads the registry back. `--control no-press` finds the
    button and does not press it, and exits 0 exactly when the checks of the restart failed.
    The real session file is moved away for the run and put back. It needs the network.

    **First run, 2026-10-10, Windows 11, from 26.10.13 to the published 26.10.14:** 16 of 17
    checks passed and the control failed its seven. The one failure is a defect in every
    release up to 26.10.14 and not in the check: after an update and an uninstall, `.pdf`
    of the user read `PDF document`, a class the uninstaller had just deleted
    (`docs/TRAPS.md`, *An installer that backs up what it replaces backs up itself the
    second time*). `installer-hooks.nsh` keeps the first backup from the release after
    26.10.14 on: its installer, run twice and uninstalled, left the class as it was (23 of
    23 in `installed_check.py`), and the published 26.10.14 installer failed that one check
    in the same run. So this check stays red on that line until the release after 26.10.14
    is the one on offer. The first version of the check counted the old copy's workers as
    the application started again; the control showed it, passing two checks it must fail.

    **On a Mac it is one command too, since the same day**, on a Mac that is logged in and
    not locked, with the disk image of the release before the published one:

    ```
    python3 scripts/update_check_mac.py <path>/tpdf_<previous>_aarch64.dmg testdata/text-wide.pdf
    ```

    It copies the application out of the image into a scratch folder, starts it on a copy
    of the document under a folder with a space in its name, waits for *Install update* in
    the application's menu to become available and chooses it, then *Restart to finish
    update*, and waits for the application to come back from the same bundle with the tool
    inside at the new version, the document open again, no update on offer, and a bundle
    that Gatekeeper reads as Notarized Developer ID. The menu is chosen through System
    Events, so whatever starts the script needs the Accessibility permission; the button
    in the window is not used, for the reason given below. `--control no-press` chooses
    nothing. **First run, 2026-10-10, over SSH on a Mac with no display attached, from
    26.10.13 to the published 26.10.14:** 12 of 12, and the control failed its seven. Its
    first attempt reported that the application did not start: the scratch folder was
    under `/var`, and `ps` prints the same path under `/private/var`.

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
    #   2. clicking it shows progress, then "Restart to finish update"
    #   3. clicking THAT restarts tpdf -- no quit and reopen by hand -- and
    #      "About tpdf" (palette or the tpdf menu) reports the new version
    #   4. the document and the page you were on come back, as after any restart
    #
    # Step 2's label read "Update ready — restart to finish" over a DISABLED
    # button until 26.9.17, and step 3 was "quit, reopen" for the same reason:
    # nothing in the application could relaunch it. Both are the action now.
    #
    # And run step 3 twice, the second time with an edit outstanding: with a
    # mark drawn and not saved, pressing Restart must ask before discarding it,
    # and answering "Keep open" must leave the update still offered and the
    # document still open. A relaunch does not go through the window's close
    # handler, so that question is asked by `finishUpdate` and by nothing else
    # -- see the trap of that name.
    #
    # That third line named a Help/About that did not exist until 2026-08-19, so
    # this step could never have been carried out. See the trap; the short of it
    # is that a checklist has no failing case, and one nobody has executed reads
    # exactly like one that keeps passing.
    curl -s https://github.com/tstone-1/tpdf/releases/latest/download/latest.json | head -20
    ```

    **On Windows steps 2 and 3 are one step, and that is the plugin rather than
    a difference in tpdf.** `Update::install_inner` there hands the installer to
    `ShellExecuteW` and calls `exit(0)`, so the application closes while
    installing and the installer starts it again; "Restart to finish update"
    never appears, because no process is left to show it. What to check instead
    is the question in front of it: with an unsaved mark, pressing **Install
    update** must ask before closing, and "Keep open" must leave the update
    still offered. Not verified on Windows as of 2026-09-21.

    **Windows live update verified 2026-09-12, from 26.9.5 to 26.9.6.** The old
    installation offered the published update; applying it installed the executable
    extracted from the verified CI MSI, after normalizing Tauri's installer-kind
    marker. The new application explicitly reported itself as current. A synthetic
    PDF rendered with the packaged engine and no workers survived exit.

    **An updater relaunch can drop `TPDF_SESSION_FILE`.** The first Windows run
    reopened the normal last document. Automated updater checks must also back up,
    temporarily clear and finally restore
    `%APPDATA%\com.timostein.tpdf\session.json`, with the application closed.
    Repeating the check this way kept the relaunched window empty and restored the
    normal session byte for byte, alongside the original executable and registry
    exports. Require the explicit latest-version message: an absent update button
    can also mean that the network check failed.

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

### Per-feature verification records

The verification record of each feature, from *Existing-text workflow* to *The command-line
tool on the Windows `PATH`*, is in [`docs/VERIFICATION.md`](docs/VERIFICATION.md). The records
stood here until 2026-10-05 and were moved unchanged. A reference elsewhere to a `BUILD.md`
section that this file does not hold names a section there.
