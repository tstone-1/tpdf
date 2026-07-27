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
uv run --with pyhanko --with cryptography testdata/make_incremental_pdf.py testdata
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
6. `npm run tauri build` and smoke-test the bundle.
7. Commit as `Release vYY.M.MICRO: <summary>`.

Verify the bump landed everywhere:

```
grep -n '"version"' package.json src-tauri/tauri.conf.json
grep -n '^version' src-tauri/Cargo.toml
```
