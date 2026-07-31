# Handover to Windows — 2026-07-31

**Delete this file once its list is worked through.** That instruction is the one thing
the previous handover lacked: `HANDOVER-MACOS.md` was consumed the same day it was written
and then sat on `main` with its present tense going stale, until it said things that were
no longer true beside documents that said the opposite. A handover is a work item, not a
record. The permanent home for anything here is `AGENTS.md`, `BUILD.md`, `CHANGELOG.md` or
`docs/TRAPS.md`.

Three commits landed on `main` from the MacBook Pro, gated there (8/8, 182 Rust tests, 311
Vitest). **Everything below is a macOS result unless it says otherwise.**

```
3b47186  Name the library in the resource map, and find a bundle that could not read a PDF
ce60579  Pad a verdict label, so the rows that pass line up with the rows that do not
143fbf3  Settle on a Mac what Windows could only claim, and retire a consumed handover
```

---

## Read this before your first build, or you will lose twenty minutes

`git pull` then `cargo clippy` **will fail**, and the error names neither the cause nor the
file:

```
error: failed to run custom build command for `tpdf`
  ...
  File exists (os error 17)
```

`tauri-build` stages `bundle.resources` for ordinary cargo builds too, so
`target\debug\pdfium` already exists on your machine as a **file** — the old resource map
renamed the DLL to it. The new map wants a *directory* there. Delete it once:

```
del src-tauri\target\debug\pdfium
del src-tauri\target\release\pdfium
```

It fails during **clippy**, which reads as a lint failure in the gate summary, and clippy
does not bundle anything — so the config change and the failure look unrelated. A clean
clone never sees it.

---

## What actually needs doing here

### 1. Confirm the label fix on Windows — this is the real ask

`ce60579` changed three files that compile on both platforms: `backend_probe.rs`,
`worker_probe.rs`, `prespawn_bench.rs`. All three only touch how a verdict label is
printed. **The widths were measured on macOS only**, and `docs/TRAPS.md` now asserts the
same holds here — which is reasoning, not a result.

```
cargo build --release --example backend-probe
.\src-tauri\target\release\examples\backend-probe.exe testdata\text-base14.pdf > run.log
```

Then the check that matters, which is one line:

```
grep -hoE "^\[[A-Z]+\] *" run.log | awk '{print length($0)}' | sort -u
```

It must print **a single value, 7**. Two values means the fix did not carry.

### 2. Expect 41 names here, not 42 — and say which one is missing

macOS reports **42** check names from `backend-probe`, identical sets across
`vector-heavy`, `text-base14` and `outline-hostile`. Your last recorded runs were `37/41`,
`38/41`, `40/41` — i.e. **41**. That is a total from the summary line and predates the
label fix, so it is not an artifact of the bad slice.

So one check is macOS-only, and nobody has said which. The likely candidate is the parent's
memory poll — macOS has no rlimit and polls as a substitute, while the job object caps
commit in the kernel here — which is exactly the one `worker-probe` already reports as not
applicable on Windows. Confirm it, and if that is what it is, `BUILD.md`'s "all 42 names
appear" needs to become a per-platform statement rather than a flat one.

The recipe now works on this harness, so the sets can be diffed directly:

```
grep -E "^\[(OK|FAIL|SKIP)\]" run.log | cut -c8-47 | sort > names-win.txt
```

### 3. Re-run the gates and the viewer, unchanged expectations

`scripts/gates.py` should be 8/8. `viewer_check.py` should report **109 names** — the
invariant is now confirmed on all seven corpora on macOS with every ran/skipped split
matching `BUILD.md`'s table, so a Windows run has something exact to disagree with.

### 4. Nothing to do about the bundle fix

`tauri.windows.conf.json` is **deliberately unchanged**. WiX ignores a resource map's
target directory and drops the DLL beside the executable, where `pdfium_library_dir`'s
resource-root candidate catches it. That is measured and still true. The macOS bundler
honours the target, which is why only that config moved.

If you rebuild the MSI, the existing check still applies: extract it with `msiexec /a`,
hide `vendor\pdfium\bin\pdfium.dll`, and run `viewer_check.py` against the extracted
`tpdf.exe`. A check on a distributable that can see the development tree is a check on the
development tree.

---

## What macOS settled, so you can stop qualifying it

- **`QuietChildStderr`'s `#[cfg(unix)]` arm compiles and runs**, and the claim it was
  written on is false: that noise does not occur on macOS at all. The child is killed while
  still in dyld, before libtest parses argv; Windows creates the process suspended and
  resumes it, and loses that race. The guard is kept — the impossibility lives in another
  type's drop timing — and the doc comment now records the measurement.
- **The `viewer_check.py` invariant is 109**, on all seven corpora, sets byte-identical.
  `AGENTS.md`'s two `86`s were the only stale counts; `BUILD.md`'s table was already right.
- **`BUILD.md` contradicted itself** on `backend-probe`: a comment said 41 names where the
  prose two paragraphs down said 42. Now consistent, pending item 2 above.
- **The V8/XFA symbol scan verifies on macOS.** `docs/THREAT-MODEL.md`'s promotion of
  "JavaScript is disabled" to "there is no engine to disable" rests on that, and on Windows
  still rests on the asset name and pinned digest, because the shipped DLL is stripped of
  local symbols. Unchanged, restated so it is not mistaken for settled everywhere.
- **Both mutation harnesses pass here** — 22/22 and 75/75, controls green — matching the
  Windows figures. The suites can fail on both platforms.

---

## Release readiness

**All six `APPLE_*` secrets are now set on `tstone-1/tpdf`**, at parity with `screenpick`
and `dblitz`. That was the blocker for a first release and it is gone.

It means the workflow can *start*. It does not mean notarization succeeds: the Apple half
of `.github/workflows/release.yml` has still never executed, and the part with no precedent
in either sibling is signing the bundled `libpdfium.dylib`, since neither ships a native
library. The first tag settles it.

Two of the six are verified beyond presence — the certificate pair was proved by importing
the exact `.p12` that was uploaded into a throwaway keychain and getting a usable
codesigning identity out of it. The other four are identifiers checked by shape.

`26.7.0` is still `Unreleased` with no tags. Cut on or after 2026-08-01 and CalVer makes it
`26.8.0`.

**Certificate renewal now touches three repos, not two.** `screenpick`'s `BUILD.md` says to
update `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` in
"this repo and in `tstone-1/dblitz`". That sentence is wrong by one and nothing enforces it.
The certificate is good until 2031-07-26, so this is a note rather than a task.

---

## Still open in Phase 1 — unchanged, and none of it is verification

- Whether a cancelled partial tile is worth showing. Unmeasured, and needs a realistic
  drawing rather than the A0 stress fixture.
- Per-tile object-enumeration bounds. Nothing has measured what enumerating objects per
  tile costs.
- **OCR interfaces.** `docs/PLAN.md` mentions OCR seven times and says Phase 1 *defines*
  the interfaces; the source has **zero** mentions. That is a plan-versus-reality gap, and
  correcting the plan is the cheaper honest fix.
- `worker-bench`'s seven POSIX modes still have no Windows counterpart. That is the genuine
  remaining platform gap and it is this platform's. The refusal already names what such a
  spike would measure that nothing else does: the per-tile overhead decomposition of
  `latency` mode.
