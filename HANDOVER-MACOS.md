# Handover to macOS — 2026-07-31

Four commits landed on `main` from the Windows desktop, all gated there (8/8, 205 Rust
tests, 311 Vitest). **Everything below is a Windows result.** This file exists to separate
what was measured from what is only a claim, and to say which claims a Mac can settle.

```
1bcdb83  Sign and notarize on a tag, and say plainly that half of it has never run
9a54020  Take the spikes out of the installer, and keep the gate that links them
25197b9  Put the PDF engine in the installer, and hide the dev tree so a check can say so
d351337  Draw form widgets in the cancellable renderer, and quiet a control that read as a failure
```

Read `CHANGELOG.md` for what changed and why. This file is only the verification list.

---

## The one thing to know before anything else

**No tpdf bundle has ever contained PDFium.** `tauri.conf.json` declared no
`bundle.resources`, so nothing copied the engine into an `.app` or an installer, and the
resource-directory fallback in `pdfium_library_dir` pointed at a directory the bundler
never created. Every macOS bundle ever built opens a window and cannot parse a document on
any machine without this repository checked out at the same absolute path.

It survived because **every check ran where the dev tree exists**, so the first candidate
always hit and the bundled branch was never exercised. `viewer_check.py` against a bundle
passed either way.

Fixed on Windows and verified there. The macOS half is `src-tauri/tauri.macos.conf.json`,
which has never been used by a build.

---

## Run these, in order

### 1. The gates, and the arm that has never compiled

```
python3 scripts/fetch_pdfium.py
python3 scripts/gates.py
```

`src-tauri/src/worker.rs` gained a `#[cfg(unix)]` guard, `QuietChildStderr`, that was
**written on Windows and has never been compiled**. It `dup`s descriptor 2 to `/dev/null`
around a test spawn and restores it on drop. If it is wrong, `cargo test` fails to build —
loudly, which is why it was included rather than left out.

What it fixes: two checks spawn a worker whose child is the libtest harness, which has no
`--render-worker` dispatch and complains on the stderr every worker inherits by design.
On Windows that put a bare `error:` line above a passing suite. macOS should have printed
the same line by the same route, through `Stdio::inherit()`.

**So the check is:** the gate transcript should contain no `error:` line at all. If it did
not print one *before* this commit either, say so — that would mean the macOS route
differs from what the code comment now claims.

### 2. Form widgets in the cancellable renderer

`src-tauri/src/progressive.rs` is fully shared code and this is the largest change to it.

```
python3 testdata/make_form_pdf.py
cargo run --release --manifest-path src-tauri/Cargo.toml --example progressive-probe -- \
    testdata/form.pdf --mode identity --slices 0
cargo run --release --manifest-path src-tauri/Cargo.toml --example progressive-probe -- \
    testdata/hostile-unused-form.pdf --mode identity --slices 0
```

Both must report `[OK] all checks passed`. On Windows the form fixture gives digest
`0fd46a2bc75f91f8` on all three rows; deleting the `FPDF_FFLDraw` pass moves the
progressive rows to `9d5e43830f4e70db` with 4,587 of 4,194,304 bytes differing. The
unused-form document is the control and must stay byte-identical.

Digests are unlikely to match Windows' exactly — font rasterisation differs — but the
three rows must agree **with each other**, which is what the probe asserts.

### 3. Note the new invocation

All 17 probes and benchmarks are `[[example]]` targets now, not `[[bin]]`, so the
installer stops shipping them. Two consequences on a Mac:

- `--example <name>`, not `--bin <name>`. An old command fails as "no such target".
- Built artifacts moved to `target/release/examples/`. **Delete any probe executables
  still in `target/release/`** — nothing rebuilds them, and a path copied from an older
  document silently runs a binary frozen before the split. 53 were removed on Windows.

`BUILD.md` is fully rewritten for this; follow it rather than memory.

### 4. The bundle check that could not previously fail

This is the one that matters, and it is new in `BUILD.md`'s release section.

```
npm run tauri build
mv vendor/pdfium/lib/libpdfium.dylib vendor/pdfium/lib/libpdfium.dylib.hidden
python3 scripts/viewer_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf \
    "$PWD/testdata/outline-simple.pdf"
mv vendor/pdfium/lib/libpdfium.dylib.hidden vendor/pdfium/lib/libpdfium.dylib
```

Hiding the development library is the whole point — with it in place the check cannot fail,
which is how this shipped broken for weeks. On Windows the extracted MSI gave **102/102
checks passed, 7 not applicable** with the dev library hidden, against a negative control
(no PDFium reachable at all) that failed and named the path it looked in.

Two things that each cost a run here: pass the PDF as an **absolute** path, and make sure
the fixture has been generated — `testdata/*.pdf` is gitignored, and an absent fixture
produces the same red as a broken bundle.

**Please report where the dylib actually landed:**

```
find src-tauri/target/release/bundle/macos/tpdf.app -name 'libpdfium.dylib'
```

`pdfium_library_dir` tries `<resources>/pdfium` and then `<resources>`, because Tauri's WiX
template *ignores* a resource map's target directory and puts the DLL beside the executable
— measured by extracting the MSI. macOS is expected to honour the target and place it in
`Contents/Resources/pdfium/`, and that is **explicitly recorded as unverified** in the
function's doc comment. Whichever it is, the answer belongs in that comment.

### 5. The release workflow — the whole macOS half is unrun

`.github/workflows/release.yml` fires on a CalVer tag only. It is ported from
`screenpick`'s working workflow, which you pointed me at, and it invokes `scripts/gates.py`
rather than re-listing commands in YAML.

**Nothing in the Apple half has executed. The YAML parses; that is the extent of it.**

Needs six repository secrets, all of which `screenpick` already has:
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
`APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_P8`.

The part with **no precedent anywhere in the portfolio** is the bundled engine — neither
`screenpick` nor `dblitz` ships a native library. Notarization requires every Mach-O in the
bundle to carry a Developer ID signature and the hardened runtime, `libpdfium.dylib`
included, and whether Tauri signs nested resources is not established here. So the workflow
signs it in `vendor/` *before* the bundler copies it, which should hold either way: with
`--deep` Tauri overwrites using the same identity, without it the copy that lands is already
signed. **That is reasoning, not a result.**

Its verification step is written to fail rather than warn, because a skipped notarization
exits 0 and yields an app Gatekeeper rejects on any machine that has never seen it.

Two decisions to overrule if you disagree, both recorded in the workflow's comments:

- **Apple Silicon only.** `fetch_pdfium.py` installs one architecture. Universal needs
  `mac-arm64` and `mac-x64` fetched separately and `lipo`'d, and an x86_64 slice carrying an
  arm64-only engine fails at bind time on hardware nothing here can test. Both Macs are ARM.
- **No updater**, so no minisign key. `screenpick` has one; tpdf's config does not, and
  adding it is a product decision rather than a porting detail.

---

## Known stale, not fixed

`AGENTS.md` and `BUILD.md` state **86 check names** as the `viewer_check.py` invariant. A
Windows run on `outline-simple.pdf` reported **110**. The count grew legitimately with the
work since 2026-07-29, but I did not write in a new number from one corpus on one platform.
Worth settling with a sweep across all six.

---

## Still open in Phase 1, unrelated to the above

- Whether a cancelled partial tile is worth showing — unmeasured, needs a realistic drawing
  rather than the A0 stress fixture.
- Per-tile object-enumeration bounds — nothing has measured what enumerating objects per
  tile costs.
- OCR interfaces, which `docs/PLAN.md` §Cross-cutting says are defined in Phase 1. Nothing
  exists in the source.
- `worker-bench`'s seven POSIX modes still have no Windows counterpart. That is the genuine
  remaining platform gap, and it is a Windows one.
