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
| `qpdf` | Optional. A structural oracle for spike 0.4; not needed to build or run. |

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
cargo run --release --manifest-path src-tauri/Cargo.toml --bin remove-probe -- \
    testdata/text-truetype.pdf c

# The V8 and XFA symbol scan. This mode reads the library rather than binding it,
# so --lib is required even though every other mode defaults it.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin worker-bench -- \
    testdata/text-heavy.pdf --mode engine --lib vendor/pdfium/lib

# Progressive rendering still agrees with the safe path, byte for byte. Slow:
# roughly 20 s, because the point is the page that takes seconds to render.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin progressive-probe -- \
    testdata/vector-heavy.pdf --mode identity --slices 0

# Character boxes still land on the ink they describe. Run it on a *small* text
# fixture: on testdata/text-heavy.pdf the wrong convention also scores 70%, so
# that page cannot discriminate and the probe fails rather than reporting a pass.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin text-probe -- \
    testdata/text-marked.pdf --mode align

# The outline walk terminates, resolves and refuses. Run BOTH: the hostile
# fixture proves the bounds fire, and the ordinary one proves they do not fire
# when they should not, which is the half that catches a walk bounding
# everything.
cargo run --release --manifest-path src-tauri/Cargo.toml --bin outline-probe -- \
    testdata/outline-simple.pdf --mode check
cargo run --release --manifest-path src-tauri/Cargo.toml --bin outline-probe -- \
    testdata/outline-hostile.pdf --mode check
```

Two notes on why these are written out in full. The binary names are **hyphenated**, and
`--bin remove_probe` fails as "no such target", which reads like a missing binary rather
than a wrong name. And `remove-probe` with no case argument defaults to case `a`, whose
whole purpose is to segfault --- so the obvious invocation of the regression check crashes
by design and looks like the bump broke something.

The third check is why the progressive path restates `FPDF_ANNOT`,
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
```

`make_incremental_pdf.py` writes about **550 MB** on purpose, so that "appending to a
300 MB file is near-instant" can be tested at 300 MB.

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
  targets, which is where `--all-targets` clippy findings surface. Coverage is currently
  the request queue and the `tile://` URL parser --- the parts a change can silently
  break --- and not rendering, which the spike probes assert instead.
- **Wrap a batch of benchmark runs in `caffeinate -du`.** `scroll_bench.py` holds one for
  its own lifetime, but the gaps between runs --- and any headless bench running alongside
  it --- are unprotected, and a session that locks mid-batch fails the next frame-rate run
  outright. A locked macOS session cannot be unlocked from a script by design, so this is
  preventable and not recoverable.

- **The `pdfium` gate is a pin check, not a build step.** It fails if `vendor/pdfium` is
  missing or is not the pinned build --- which is the difference between a benchmark that
  means something and one that does not.

### There is no remote CI, deliberately

The project is pre-release and developed on one machine. A GitHub Actions workflow would
add macOS-runner minutes and a second place for the gate list to live, in exchange for
catching nothing that `scripts/gates.py` does not catch locally first.

When CI is added --- the natural trigger is the repo going public, or a second contributor
--- **the workflow should invoke `scripts/gates.py`**, not re-list the commands in YAML. That
keeps the checklist and the gate the same object rather than two things that happen to
agree today.

### Windows is not verified

Every gate run and every spike measurement to date is macOS arm64, and the tree will not
build on Windows as it stands. Three things are known to be in the way:

- **`libc::` is used without a `cfg` gate** in `sanitize_rewrite.rs` and `tile_bench.rs`.
  `startup.rs`, `worker_bench.rs` and `incremental_save.rs` do gate their macOS-only code
  behind `cfg(target_os = "macos")`; those two do not, so they are compile errors rather
  than missing functionality.
- **PDFium's loadable library is at `bin/pdfium.dll`** on Windows and
  `lib/libpdfium.dylib` on macOS. `scripts/fetch_pdfium.py` knows both;
  `pdfium_library_dir()` in `src-tauri/src/lib.rs` only knows the macOS shape.
- **The sandbox is `sandbox_init` SBPL**, which has no Windows equivalent. The threat
  model's containment argument is macOS-specific and needs its own answer there.

Do not claim a Windows build works until one has run.

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

### Checking the viewer

The reading surface is asserted rather than eyeballed. This opens a document in a real
webview, dispatches real wheel and key events at it, and checks fit-width, scrolling, End
and Home, the zoom ladder, a pinch, resize, text selection and copy, find-in-document, the
command palette, the screen-reader text layer, the outline sidebar, the page-thumbnail
strip, and that the frame loop idles when there is nothing to do:

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

The watchdog identifies **any** page that never executed, whatever the reason --- an
occluded window and a raw unbundled binary produce exactly the same silence. Every spike
entry point starts by asking Rust for its path, which records a `webview alive` mark; a run
that times out without one is told in full that the page never ran a line of JavaScript.
Confirm independently with `TPDF_STARTUP=<file> <binary>`, which fails the same way in 30 s
and settles "environmental or mine" in one command. Results otherwise print as they are
produced, so a run that stops partway names the last check it completed.

Two of its assertions carry the weight, and both tie a position to specific content rather
than checking that something happened. For **selection**, text dragged near the top of the
page must come from earlier in the page's text than text dragged further down --- a substring
check was tried first and cannot fail, since a selection is a contiguous range of indices
whatever the boxes claim. For **search**, a match's index range must cover the characters
searched for, re-extracted independently; every other search assertion passes just as well
when the indices are off by one.

Run all six corpora. Every run reports the same **75 check names**; what differs is how
many are `[SKIP]` with a reason, and a name that goes missing rather than skipping is the
bug this arrangement exists to catch:

| fixture | ran | skipped | what it is there for |
|---|---|---|---|
| `text-heavy.pdf` | 65 | 10 | the dense case, and search across 775 pages |
| `outline-simple.pdf` | 70 | 5 | the only fixture with an ordinary outline |
| `outline-hostile.pdf` | 70 | 5 | the only one with a `/Launch` entry to refuse |
| `vector-heavy.pdf` | 44 | 31 | one page, no extractable text |
| `vector-multi.pdf` | 51 | 24 | twelve A0 pages: the only one where a thumbnail is slow enough to collide with the viewer |
| `rotated-90.pdf` | 64 | 11 | every page at `/Rotate 90`, which nothing else in the corpus has |

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
        src-tauri/target/release/text-probe testdata/rotated.pdf \
            --page $page --mode align --view-turns $view
    done
done
src-tauri/target/release/outline-probe testdata/rotated-90.pdf --mode check \
    --manifest testdata/rotated-manifest.json
```

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

Note it takes the **`.app` bundle**, not the executable inside it: two phases go through
Launch Services and there is nothing else to hand `open`.

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
6. `npm run tauri build` and smoke-test the bundle, then `scripts/viewer_check.py` against
   it on both `testdata/text-heavy.pdf` and `testdata/vector-heavy.pdf`.
7. Commit as `Release vYY.M.MICRO: <summary>`.

Verify the bump landed everywhere:

```
grep -n '"version"' package.json src-tauri/tauri.conf.json
grep -n '^version' src-tauri/Cargo.toml
```
