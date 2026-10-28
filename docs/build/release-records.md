# Release records

What was run for each release, what passed, and what was not run. The record of a new release goes at the top. Moved here verbatim from `BUILD.md` on 2026-10-23, which indexes this file.

**26.10.27 verification and publication, 2026-10-28:** prepared on the Windows development
machine, with the macOS half on the second Mac over ssh. Step 1: `cargo update --dry-run`
locked nothing in either lockfile, `npm outdated` was empty and `check_pdfium_age.py` found the
pin at the head of 8103. The window checks passed 16 of 16 on macOS (`run_on_mac.py`) on the
tree before the version bump. Step 6 found the backtracking matcher in `said.ts` before it
shipped (`docs/VERIFICATION.md`, *Backend messages in German*). Step 7 in the mutation
worktree on the release tree: `mutate_python.py` 38/38, `mutate_frontend.py --near v26.10.26`
747/747, `mutate_rust.py --near v26.10.26` 1,768 of 1,768 with 22 skipped as not runnable on
Windows. That run was killed once and resumed; one row failed to compile in the first part and
was caught when run alone with `--only` (`docs/TRAPS.md`, *A mutant that failed to compile
once*). The Rust table refused to start at first: a macOS-only row had no `only_on`, and
`check_mutation_anchors.py` now reads a `#[cfg]` on a parent's `mod` line, so it reports that
row. All 34 gates passed on the release tree (3,287 Rust tests with 12 ignored, 630
command-line checks with 3 skipped, 3,250 frontend tests), and `check_linux.py` passed. Step 8
on Windows 11: the installer built with the Explorer overlay passed `installed_check.py` 26/26;
from its extracted payload, with the development PDFium hidden, `tpdf-cli` answered 26.10.27
and verified `testdata/incr-signed.pdf`; `print-probe` 15/15; `redact-reach-probe` over 181
documents in the desktop's Downloads: 15,314 regions, 92.5% taken whole; the gate read back
13,947 on 395 pages, none still read as text, 8,279 not shown unreadable and 5,668 shown
unreadable, in 91.4 s; a `--no-bundle` checks build passed `viewer_check.py` 378/378 on
`text-heavy.pdf` and 277/277 on `vector-heavy.pdf`. No rehearsal tag: `release.yml` changed
only in its notes. CI passed all three jobs on `4e8d6ce4` (run 40772931058) and `Audit`
passed (run 40772931086); release run 40777564103 skipped its gates and left a draft of eight
files whose `latest.json` offered 26.10.27 for the four macOS and Windows entries. On the
draft's files: the installer read as `Valid` and timestamped by Certum and passed
`installed_check.py` 26/26; the `.dmg`'s app was accepted as Notarized Developer ID and
stapled, its `tpdf-cli` (Developer ID, `runtime`) answered 26.10.27 and verified
`incr-signed.pdf` with the development engine hidden, and against that app `menu_check.py`
(after its self-test) found a clean bar of ten menus and `save_check.py` saved over the
document twice, read back by `qpdf` both times, and refused a print after a second writer.
`menu_check.py` had read only `['tpdf']` out of `menubar.ts` since the menu titles became
catalog keys in 26.10.27's translation, and was corrected the same night; `save_check.py`
needs Python 3.10, which the Mac's system `python3` is not. `publish_release.py v26.10.27
--publish` published it; without authentication `releases/latest` is v26.10.27, the `.dmg`,
the `-setup.exe`, the `.deb`, the AppImage and `latest.json` answer 200, and `latest.json`
offers 26.10.27. An installed 26.10.26 updated itself: `update_check.py` 18/18 on Windows
(the first run, five minutes after publishing, saw no offer; `docs/VERIFICATION.md`) and
`update_check_mac.py` 12/12 on the second Mac, and each `--control no-press` failed its seven
checks and nothing else. The cask went to 26.10.27 with the `.dmg`'s SHA-256 equal to
GitHub's digest; `brew audit --cask --online` was clean once the tap was trusted, and `brew
livecheck` reads 26.10.27. The winget pull request moved to 26.10.27 and passed `winget
validate`. **Not run:** step 7's pass after the release (`--since v26.10.26 --resume`), the
Windows window phases other than the viewer, and the README pictures.

**26.10.6 verification, macOS arm64, 2026-10-05:** all 30 gates passed on the release tree
(2,823 Rust tests with ten documented ignored, 2,454 frontend tests). Every Rust and
frontend mutation selected `--near v26.10.5` ran in the mutation worktree and was caught:
171 Rust and 223 frontend. That run was on the tree before the dependency refresh, which
moved Tauri to 2.12.1 with its plugins, Vite to 8.3.2, Vitest to 5.0.3 and seventeen smaller
crates; no major version is held back. `worker-probe` 48/48; `backend-probe` 41/44 on
`text-heavy.pdf` with three skipped and 42/44 on `vector-heavy.pdf` with two skipped.
`sign-probe` was not run here; CI runs it on the macOS leg. The unit tests and the
command-line suite passed on Windows 11 through `scripts/run_on_windows.py` (2,847 unit
tests, 515 command-line checks), on the release tree. The normal bundle, with the
development engine hidden: the bundled `tpdf-cli` answered `--version` with 26.10.6,
`verify` and a text extract, and the application opened and drew a fixture with its workers
mapping the bundle's PDFium and the application process mapping none. In a development
window, with real pointer and key events (`cliclick`): the Form button's menu lists its
thirteen commands, *Add a text field* from it arms a drag, a field's name shows on the page
while it is typed, and on the blank page the remove button, the Delete key and *Clear recent
documents* each changed the session file. The menu bar was read through System Events and
has *Form* between *Arrange* and *Page*. `menu_check.py`, `save_check.py` and the checks
build's window phases were not run. Whether the tool row, one button longer, stays on one
line in a Windows window of the default width was not looked at. The five pictures and
`demo.gif` were regenerated with `scripts/screenshots.py` (7/7) and each was looked at; the
`--more` set at 1,000 pixels wide and the dark set were looked at too, and the first of them
showed Form at the head of the *More* menu with Pages and Redact below the fold, so Form
now comes last there.
`docs/THREAT-MODEL.md` already stated the three new commands (T6.35, T6.36). The release
notes in `release.yml` were rewritten.

**26.10.15 verification, 2026-10-10:** all 31 gates passed on the release tree on macOS
arm64 (3,083 Rust tests with eleven documented ignored, 2,809 frontend tests) and
`check_windows.py` type-checked the Windows tree. Every mutation selected
`--near v26.10.14` ran in the mutation worktree and was caught, 109 Rust and 33 frontend,
and so was every row of `mutate_python.py` once four stale ones were corrected (step 7).
`window_checks.py` passed 11 of 11 on Windows 11 and 11 of 11 on macOS on a second Mac,
both on the release tree, the viewer check with its four refusals of
`add_validation_data` among them. `scripts/run_on_windows.py` passed the command-line
suite on Windows 11 with `qpdf` installed there, on the tree before the version bump.
npm and cargo dependencies were updated within their ranges; no major was looked for.
`docs/THREAT-MODEL.md` and `README.md` were checked for what this cycle changed and not
read through. **Not run:** the written copy of `tpdf long-term` against a public
authority (`docs/VERIFICATION.md` has why), the new entry of `mutate_viewer.py` by the
harness itself, `print-probe` and `redact-reach-probe`.

**26.10.25 verification and publication, 2026-10-23:** a dependency release that took a new
engine on the way. Step 1 took `tokio` 1.54.1 for the yanked 1.54.0 (RUSTSEC-2026-0384), Tauri
2.12.5, `rfd` 0.16.1 and patch crates, and removed five `unic-*` ignores from
`.cargo/audit.toml` that matched no advisory any more; `tauri-plugin-updater` held at 2.13.3.
All 33 gates passed on that tree, `check_linux.py` passed, and `window_checks.py` passed 16 of 16
on macOS and on Windows. CI passed on `b392df90`, and `Audit` failed only on the PDFium pin:
Chrome stable had moved to 8103, so step 1's engine question was answered by the `Audit` run
rather than before it, and `check_pdfium_age.py` is in step 1 now (`1f2cd6dd`). The engine
record is *PDFium 8103-tpdf.1* above. On the release commit `1885db12`, after the repin: all 33
gates in 8 min 37 s (3,284 Rust tests with 12 ignored, 622 command-line checks with 3 skipped,
3,199 frontend tests), `mutate_python.py` 38/38, `window_checks.py` 16 of 16 on macOS and on
Windows, and `run_on_windows.py --suite all` passed. CI passed all three jobs (run
39942302195) and `Audit` passed (run 39942302237). No rehearsal tag: `release.yml` changed only
in its notes. Release run 39945723611 left a draft of eight files whose `latest.json` offered
26.10.25 for the four macOS and Windows entries. On the draft's files: the installer read as
`Valid` and timestamped by `Get-AuthenticodeSignature` and passed `installed_check.py` 25/25; the
`.dmg` and its app were accepted as Notarized Developer ID and stapled, with a `tpdf-cli` that
answered 26.10.25 and verified `testdata/incr-signed.pdf`. `publish_release.py v26.10.25
--publish` published it; without authentication `releases/latest` is v26.10.25, all eight files
answer under the tag, and `latest.json` offers 26.10.25. An installed 26.10.24 updated itself:
`update_check_mac.py` 12/12 and `update_check.py` 17/17, and each `--control no-press` failed its
seven checks and nothing else. The cask went to 26.10.25 with the `.dmg`'s SHA-256 equal to
GitHub's digest; `brew audit --cask --online` was clean and `brew livecheck` reads 26.10.25. The
winget pull request moved to 26.10.25 and passed `winget validate`. **Not run:** `print-probe`,
`menu_check.py` and `save_check.py`.

**26.10.24 verification and publication, 2026-10-21:** prepared on the Windows development machine, with the
macOS half on the second Mac and the Windows half on the Windows build machine over ssh. Step 1 took `lopdf` 0.46,
a `0.x` minor of a direct dependency: its writers still leave out `ObjStm`, `XRef` and
`Linearized` objects, `processor.rs` is byte-identical, and it still records no free
cross-reference entry, so the guards in `save.rs`, `redact.rs` and `docinfo/freed.rs` hold; it
reads Type 3 `d1` as one operator now, which `type3.rs`'s substitution handles either way. Also
`vite` 8.3.5 and eight patch crates; `tauri-plugin-updater` held at 2.13.3 again. Two more held
crates are new to the list above and are held by others: `windows-core` 0.100 by Tauri's `tao`,
`wry` and `webview2-com`, and `parking_lot` 0.13, which would be a second copy beside `tao`'s
0.12. All 33 gates passed on the release tree, and `check_linux.py` passed. `window_checks.py` 16
of 16 on the second Mac through `run_on_mac.py` and 16 of 16 on the Windows build machine on a `--no-bundle`
checks build. Step 7 in the mutation worktree: `mutate_python.py` 38/38, `mutate_frontend.py
--near v26.10.23` 12/12, `mutate_rust.py --near v26.10.23` 14/14 with 2 skipped as not runnable
on Windows. `redact-reach-probe` over 177 documents in the desktop's Downloads: 15,034 regions,
92.4% taken whole; the gate read back 13,669 on 388 pages, none still read as text, 8,119 not
shown unreadable and 5,550 shown unreadable, in 96.0 s. **`print-probe` failed all three
fixtures on the Windows build machine at first**, the print worker refused `vendor\pdfium\bin\pdfium.dll` with
`LoadLibraryExW` error 5: `fetch_pdfium.py` left an owner-only access list on development trees
(`docs/TRAPS.md`, *PDFium unpacked through a Python temporary directory could not be loaded by a
low-integrity worker*). Fixed in `0be47c6f`, the tagged commit; reinstalled with `--force`, the
same binary passed 14/14 on `vector-multi.pdf`, 15/15 on `rotated.pdf` and 14/14 on `links.pdf`,
each reading 78 modules with none named pdfium. No rehearsal tag: `release.yml` changed only in
its notes. CI passed all three jobs on `0be47c6f` and `Audit` passed (run 39591223349); release
run 39595647629 skipped its gates and left a draft of eight files whose `latest.json` offered
26.10.24 for the four macOS and Windows entries. On the draft's files: the installer read as
`Valid` by `Get-AuthenticodeSignature` and passed `installed_check.py` 25/25 on the Windows build machine; the
`.dmg` and its app were accepted as Notarized Developer ID and stapled, with a `tpdf-cli` that
answered 26.10.24 and verified `testdata/incr-signed.pdf`. `publish_release.py v26.10.24
--publish` published it; without authentication `releases/latest` is v26.10.24 and the `.dmg`,
the `-setup.exe`, the `.deb`, the AppImage and `latest.json` answer 200. An installed 26.10.23
updated itself: `update_check_mac.py` 12/12 on the second Mac and `update_check.py` 17/17 on
the Windows build machine. The cask went to 26.10.24 with the `.dmg`'s SHA-256 equal to GitHub's digest; `brew
audit --cask --online` was clean and `brew livecheck` reads 26.10.24. The winget pull request
moved to 26.10.24 and passed `winget validate`. After publication, `run_on_windows.py --rev
0be47c6f --pictures` on the Windows build machine: `screenshots.py --more` 51/51 and 20 pictures, the print
preview among them, looked at. Step 7's pass after the release, `--since v26.10.23 --resume` on
the Windows development machine on `0be47c6f`: frontend 74 of 74; Rust 127 of 128 with 5 skipped as not
runnable on Windows. The survivor was a row that changed nothing on Windows (`docs/TRAPS.md`, *A
mutation that replaced `cfg!(windows)` with `true` changed nothing on Windows*); rewritten, it is
caught there. **Not run:** the `no-press` controls of the update checks, `menu_check.py` and
`save_check.py`.

**26.10.23 verification, 2026-10-18:** prepared on the development Mac, with the macOS window checks
on the second Mac and the Windows half on the Windows build machine and the Windows development machine over ssh. Step 1
moved nothing: the lockfiles already held what `cargo update` and `npm update` reach, and
`tauri-plugin-updater` stays at 2.13.3 in both Cargo lockfiles and npm. The engine is
`pdfium-8078-tpdf.2`, whose code is that of `.1` (near the top of this file). All 33 gates
passed on the release tree and `check_windows.py` type-checked the Windows tree, once the
Windows `pdfium.dll` was placed in `vendor/pdfium/bin` as its message says. `window_checks.py`
16 of 16 on the second Mac through `run_on_mac.py`, `compare` and `add text` among them, and 16
of 16 on the Windows build machine on a `--no-bundle` checks build. Step 7, in the mutation worktree:
`mutate_rust.py --near v26.10.22` 164 of 164, `mutate_frontend.py --near v26.10.22` 61 of 61,
`mutate_python.py` 38 of 38; the new rows for compare and added text had found twelve test
gaps the day before (`75ec7e23`). `run_on_windows.py --suite all` on the Windows build machine: 3,252 unit
tests and 618 command-line checks, 4 skipped. The normal macOS bundle with the development
engine hidden: `tpdf-cli --version` 26.10.23, `verify`, `text`, `page-numbers`, a Greek and
Chinese `watermark`, and `compare` exiting 1 and 0; the app opened a document with all seven
workers mapping the bundle's PDFium and the app process none. On the Windows build machine the NSIS bundle
passed `installed_check.py` 25/25, once built with `tauri.explorer.conf.json` as `release.yml`
builds it (without the overlay, `tpdf_shell.dll is installed` fails, 24/25). `redact-reach-probe`
over 175 documents in the desktop's Downloads: 14,867 regions, 92.3% taken whole; the gate read
back 13,507 on 383 pages, none still read as text, 7,997 not shown unreadable and 5,510 shown
unreadable, in 90.2 s. **`print-probe` on `vector-multi.pdf` hung at the range print** on
the Windows build machine, its job left *Spooling* with 0 pages, twice; `rotated.pdf` passed 10/10. The
26.10.22 probe, whose `print_win.rs` and probe are the same, hangs the same way there today,
and a spooler restart did not clear it, so the machine changed and the code did not; the
range print on that fixture was unverified on Windows at the tag. After a reboot of the Windows build machine
on 2026-10-19 the same probe binary passed 9/9 on `vector-multi.pdf`, the range print
included, so the hang was the machine's state. That was the reading at the time; on 2026-10-19
the desktop showed the same *Spooling*, 0 pages, while the probe spent 2.5 minutes rasterising
each sheet and then passed 9/9, so the "hang" was more likely tpdf's own render, very slow in
some machine state (`docs/TRAPS.md`, *A print job *Spooling* with 0 pages was tpdf rasterising,
not a stuck spooler*). No rehearsal tag: `release.yml` changed only in its notes.
Publication, 2026-10-19: on `cc8611c1` CI passed all three jobs and `Audit` passed (run
39165274780); release run 39167261266 left a draft of eight files whose `latest.json` offered
26.10.23 for the four macOS and Windows entries, with a `.dmg` Gatekeeper accepts as Notarized
Developer ID. `publish_release.py v26.10.23 --publish` published it; without authentication
`releases/latest` is v26.10.23 and all eight files answer 200. An installed 26.10.22 updated
itself: `update_check_mac.py` 12/12 on the second Mac and `update_check.py` 17/17 on
the Windows build machine, and each `no-press` control failed its seven checks and nothing else. The cask
went to 26.10.23 with the `.dmg`'s SHA-256 equal to GitHub's digest; `brew audit --cask
--online` was clean and `brew livecheck` reads 26.10.23. The winget pull request moved to
26.10.23 and passed `winget validate`; the published installer had the manifest's SHA-256,
installed with `/S`, showed in `winget list` as `tpdf 26.10.23` and uninstalled with `/S`.
`winget install --manifest` itself stopped at SmartScreen (step 12's winget paragraph).
Step 7's pass after the release, `--since v26.10.22 --resume`, on the second Mac on `cc8611c1`:
Python 38 of 38, Rust 1,006 of 1,006, frontend 593 of 593. The frontend harness first stopped
at its control with *no summary line*: vitest coloured its output inside tmux (`docs/TRAPS.md`,
*vitest coloured its output inside tmux*); the harness now strips colour codes, and the run
was repeated with it.

**26.10.22 verification and publication, 2026-10-17:** prepared on the Windows development machine. Step 1
moved `libc` and `unicode-normalization` a patch each; `tauri-plugin-updater` 2.14.0 was held
again with its npm half. All 33 gates passed on the release tree. `window_checks.py` passed 14 of
14 on the second Mac through `run_on_mac.py` and 14 of 14 on the Windows build machine on a `--no-bundle` checks
build. Step 7: `mutate_python.py` 38/38, `mutate_frontend.py --near v26.10.21` 10/10;
`mutate_rust.py --near v26.10.21` selected none, which it refuses as a pass, and `--since
v26.10.21` caught 5 of 5. On the Windows build machine the NSIS bundle passed `installed_check.py` 25/25, and
`print-probe` through Microsoft Print to PDF passed 9/9 on `vector-multi.pdf` and 10/10 on
`rotated.pdf`. Run from the desktop's own build, `cargo run --example print-probe` failed in the
interactive scheduled task at `resource path ..\vendor\pdfium\bin\pdfium.dll doesn't exist` and
built from the ssh session; the probe binary ran in the task. `redact-reach-probe` over 174
documents in the desktop's Downloads: 14,780 regions, 92.3% taken whole; the gate read back
13,425 on 380 pages, none still read as text, 7,915 not shown unreadable and 5,510 shown
unreadable, in 86.8 s. Run from `src-tauri` instead of the repository root, the same probe
refused all 174 documents (*could not load its PDF engine*) and still exited 0. No rehearsal tag:
`release.yml` changed only in its notes. CI passed all three jobs on `d6d1f51d` and `Audit`
passed (run 39061175640); release run 39063139652 skipped its gates and left a draft of eight
files whose `latest.json` offered 26.10.22 for the four macOS and Windows entries.
`publish_release.py v26.10.22 --publish` published it; without authentication `releases/latest`
is v26.10.22 and all eight files answer 200. An installed 26.10.21 updated itself:
`update_check_mac.py` 12/12 on the second Mac, its `no-press` control failing its seven checks and
nothing else; `update_check.py` 17/17 on the Windows build machine, and `winarm.yml` 25/25 and 17/17 on both
`windows-11-arm` (ARM64) and `windows-2025` (AMD64). The Windows build machine's `no-press` control failed its
seven and one more, *the .pdf class reads as it did before the install*: that control never
updates, so 26.10.21's uninstaller ran and left the empty key 26.10.22 fixes; from 26.10.22 on
the control uninstalls a version that does not. The cask went to 26.10.22 with the `.dmg`'s
SHA-256 equal to GitHub's digest; `brew audit --cask --online` was clean and `brew livecheck`
reads 26.10.22. The winget pull request moved to 26.10.22 with `ProductCode` and
`AppsAndFeaturesEntries`; on the Windows build machine `winget install --manifest` installed it after
SmartScreen's prompt was answered, and `winget uninstall --manifest --silent` removed it in two
seconds with no window. **Not run:** `menu_check.py` and `save_check.py` on a macOS bundle, and
the two Linux-only mutation rows.

**26.10.21 publication, 2026-10-17:** one rehearsal tag first, because `release.yml` had
changed since 26.10.20. `v26.10.21-rc1` passed every job; `installed_check.py` passed 24/24
on its signed installer on the Windows build machine, in the desktop session through an interactive
scheduled task, and its `.dmg` was accepted by Gatekeeper as Notarized Developer ID,
stapled, with a `tpdf-cli` inside that answered `--version` with 26.10.21 and verified
`testdata/incr-signed.pdf`. The rc draft and tag were deleted by id. On the release commit
`bfca4137` CI passed all three jobs and `Audit` passed (run 38939032978); the release run
38950710725 passed every job and left a draft of eight files with the 26.10.21 paragraph in
its body and a `latest.json` offering 26.10.21 for `darwin-aarch64`, `darwin-aarch64-app`,
`windows-x86_64` and `windows-x86_64-nsis`. `scripts/publish_release.py v26.10.21 --publish`
published it. Without authentication `releases/latest` is v26.10.21, all eight files answer
under the tag, and `latest.json` offers 26.10.21. The Homebrew cask went from 26.10.20 to
26.10.21 with the `.dmg`'s SHA-256 equal to the digest GitHub records for it; `brew audit
--cask --online` printed no finding and `brew livecheck` reads 26.10.21. An installed
26.10.20 updated itself: `update_check_mac.py` 12/12 over SSH on the second Mac and
`update_check.py` 17/17 on the Windows build machine, and each `--control no-press` failed its seven checks
and nothing else. The winget pull request (445804, still a new package awaiting a moderator)
moved to 26.10.21 and passed `winget validate`; the published installer had the manifest's
SHA-256, a valid timestamped signature, installed with `/S` and showed in `winget list` as
`tpdf 26.10.21`, and uninstalled with `/S`. Step 7's pass after the release, `--since
v26.10.20 --resume`: the four second-launch rows on a Mac caught 4 of 4; Python 38 of 38;
frontend 958 of 959, and the row it missed (Escape leaving the strokes of a drawing behind)
is now red in its own ink test. Rust on the Windows development machine, resumed after a stop for low
memory: 767 of 768, 13 skipped as not runnable there. The one survivor, `docinfo: call a
document timestamp's time attested whatever its verdict`, survived because that machine had never
generated `incr-doc-timestamped.pdf`, so its test skipped; with the fixture it is red, there and
on the second Mac. The second Mac ran the five macOS-only save rows: 5 of 5. The two Linux-only
reveal rows did not run. The `hosted` gate came out of this.

**26.10.21 verification, 2026-10-16:** the first release prepared and checked from the Windows development machine, with the macOS half run on the second Mac over SSH. All 32 gates passed on
Windows 11 on the release tree, once `vendor/pdfium` there was brought to the 8078 pin the
pull had moved (the `pdfium` gate said so first; `test` and `bins` were run again on 8078).
CI passed all three jobs and `Audit` passed on `1f92ca31`. `Audit` had failed all day before
it: every `zip` below 9.0.3 was yanked and `tauri-plugin-updater` requires `zip ^4`, so
`scripts/check_cargo_audit.py` let exactly `zip 4.6.1` through, until 4.6.1 was unyanked
that afternoon and the script, finding its allowance unused, failed the next `Audit` and
the entry went; four patch updates the resolver could then reach were taken with it. `scripts/run_on_windows.py --suite all` passed 3,209 unit
tests and 603 command-line tests with 4 skipped on the Windows build machine. Every mutation selected
`--near v26.10.20` on Windows was caught, 172 Rust (6 skipped as macOS- or Linux-only, the
four second-launch rows among them, which no platform ran this cycle) and 394 frontend, and
all 38 of `mutate_python.py`. `scripts/run_on_mac.py` on the second Mac: 13 of 14 window
checks. **`marks on text` failed there, and the release went out with it red, by decision:**
its *a page cropped by a drag is rendered under its crop* failed in five of five runs. After
the drag-crop of the last page of `text-heavy.pdf`, a 60 by 40 pixel region drag inside the
cropped page made no region and said nothing; the drag reached the crop drag's `end`
committed, and `boxQuad` refused it because the press mapped to y 78,010 pt on a 492 pt page
at zoom 0.936. Seconds later the zoom was 1.435, the cropped page fitted, and the same point
mapped inside the page. Waiting until the page was laid out at the crop's size did not help,
and a unit test of the same sequence in `viewercrop.test.ts` passed, so the cause is not
found; `adoptCrops` notes the new size without the re-anchor and refit `learnGeometry` does,
which is where to look first. The check passed on Windows and on a Mac when the feature
landed. A reader may find the first drag soon after cropping the last page of a long
document does nothing; a second works, and the crop itself is right. In one of the five runs
*and it is recorded on the page it was pressed on* also failed once (slot 10, page 11, the mark
on 12) and not again. `docs/THREAT-MODEL.md` §T8 gained links the reader adds, `README.md`
the new marks, `SECURITY.md` the Explorer processes. **Not run:** the window checks on
Windows on the release tree, `print-probe`, `redact-reach-probe`, `menu_check.py`,
`save_check.py`, and `check_windows.py` (the Windows tree was compiled by the gates here).

**26.10.20 publication, 2026-10-15:** one rehearsal tag first, because `release.yml` had
changed (CI's job count, and the glibc check on the packaged binaries). `v26.10.20-rc1`
passed every job, the Linux leg's glibc check read 2.39 for both binaries, and
`installed_check.py` passed 23/23 on the rehearsal's signed installer on the Windows build
computer, run in its desktop session through an interactive scheduled task: the first run of
that check on a release candidate rather than a published installer. The rc draft and tag
were deleted by id. On the release commit `543008f4` CI passed all three jobs and `Audit`
passed; the `Release` run 38737638466 skipped its gates on CI's account and passed every job
at the first attempt. On the draft's files: 8 assets, counted with GraphQL, and `latest.json`
named 26.10.20 for `darwin-aarch64`, `darwin-aarch64-app`, `windows-x86_64` and
`windows-x86_64-nsis`. The release body had no paragraph for 26.10.20, because it was not
added to `release.yml` before the tag; it was written there afterwards and the draft's body
replaced by a PATCH carrying `tag_name`, with GraphQL reading `v26.10.20` after it. The
`.dmg` was accepted by Gatekeeper as Notarized Developer ID, stapled, with a `tpdf-cli`
inside under the hardened runtime that answered `--version` with 26.10.20 and verified
`testdata/incr-signed.pdf`. `scripts/publish_release.py v26.10.20 --publish` published it.
Without authentication `releases/latest` is v26.10.20, the `.dmg`, the `-setup.exe`, the
`.deb`, the `.AppImage` and `latest.json` answer 200 under the tag, and `latest.json` offers
26.10.20. The Homebrew cask went from 26.10.19 to 26.10.20; `brew audit --cask --online`
printed no finding and `brew livecheck` reads 26.10.20. An installed 26.10.19 updated
itself: `update_check_mac.py` 12/12 over SSH on a second Mac and `update_check.py` 17/17 on
the Windows build computer, and each `--control no-press` failed its seven checks and
nothing else. The winget pull request (445804, still a new package awaiting a moderator)
moved to 26.10.20 and passed `winget validate`; the published installer had the manifest's
SHA-256, a valid timestamped signature, installed with `/S` and showed in `winget list` as
`tpdf 26.10.20`, and uninstalled with `/S`. Step 7's pass after the release, `--since v26.10.19
--resume` on `543008f4`, found no survivor: 498 of 498 Rust mutations caught and 30 skipped
as Linux- or Windows-only, 594 of 594 frontend, and 38 of 38 in `mutate_python.py`.

**26.10.20 verification, 2026-10-15:** a release of fixes from the codebase review of that
day, cut the day after 26.10.19. All 32 gates passed on macOS arm64 on the release tree and
on Ubuntu 24.04 under WSL on the code before the version bump, with Cargo bounded to 3 jobs
there by the gate runner's new Linux rule; `check_windows.py` type-checked the Windows tree,
and `scripts/run_on_windows.py --suite all` passed 3,134 unit tests and 586 command-line
tests with 4 skipped on the Windows build computer, which has had a display plug since that
morning. `scripts/run_on_mac.py` passed 11/11 window checks on a second Mac, and
`screenshots.py` on the Windows computer passed 45/45 checks with complete pictures at
1920 x 1080. Every mutation selected `--near v26.10.19` on macOS was caught, 107 Rust (23
skipped as Linux- or Windows-only) and 106 frontend, and all 38 of `mutate_python.py`. The same
Rust selection on Linux under WSL first died in `mutation_since.py`, which used Python 3.13's
`Path.read_text(newline=...)` on Ubuntu's 3.12; with that fixed, 123 of 124 were caught and
the last went red only in two other tests: `a_merge_holds_every_page_of_every_document`
returned before any of its counts on Linux, where there is no OS parser. It now skips only
that parser's count, and the row is caught by its own test on both platforms. Fedora 44 and
Ubuntu 22.04 were checked under WSL: on 22.04 the binaries refuse to start (GLIBC_2.39),
which the README now states and `check_glibc_floor.py` holds; on Fedora `tpdf-cli` read,
redacted with verification and recognised text once Tesseract was opened by its 5.x name.
The normal bundle, with the development engine hidden: the bundled `tpdf-cli` answered
`--version` with 26.10.20, verified `testdata/incr-signed.pdf`, read a fixture's text and
redacted with verification. `tauri-plugin-updater` stayed at 2.13.3 (step 1). **Not run:**
the bundle opened as a window on this Mac with its workers' libraries read, `menu_check.py`,
`save_check.py`, `print-probe` and `redact-reach-probe`, and the signing phase of the window
checks.

**26.10.19 publication, 2026-10-15:** two rehearsal tags came first, because the Linux leg
was new: rc1 failed in that leg's own check of the `.deb` (its members are named `usr/...`
with no `./`, so `tar -xO ./usr/...` found nothing; it extracts with `dpkg-deb -x` now), and
rc2 passed every job; both tags and drafts were deleted. On the release commit `455b2ed1`
`Audit` failed on the fuzz lockfile, which still held rustls 0.23.45, and CI's macOS job
timed out once in `tests/cli/redact.rs` waiting for the render service; `9bf7db13` updated
the fuzz lockfile, both workflows were green on it, and the tag sits on it. The `Release`
run 38643094196 passed every job at the first attempt, the Linux one included. On the
draft's files: 8 assets, counted with GraphQL (the `.dmg`, the `.app.tar.gz` and the
`-setup.exe` with their signatures, the `.deb`, the `.AppImage` and `latest.json`), and
`latest.json` named 26.10.19 for `darwin-aarch64`, `darwin-aarch64-app`, `windows-x86_64`
and `windows-x86_64-nsis` and for no Linux platform. The `.dmg` was accepted by Gatekeeper
as Notarized Developer ID, stapled, with a `tpdf-cli` inside that answered `--version` with
26.10.19 and verified `testdata/incr-signed.pdf`. `scripts/publish_release.py v26.10.19
--publish` published it. Without authentication `releases/latest` is v26.10.19, the six
file names answer a range request through `releases/latest/download/`, and `latest.json`
offers 26.10.19; on a Windows 11 computer `Get-AuthenticodeSignature` reads the downloaded
installer as `Valid`, timestamped, signed by `CN=Open Source Developer Timo Stein`. The
Homebrew cask went from 26.10.18 to 26.10.19; `brew audit --cask --online` printed no
finding and `brew livecheck` reads 26.10.19. An installed 26.10.18 updated itself:
`update_check_mac.py` 12/12 over SSH on a second Mac and `update_check.py` 17/17 on
Windows 11, and each `--control no-press` failed its seven checks and nothing else. In WSL
the published `.deb` reads as `tpdf 26.10.19 amd64` with `Exec=tpdf %F` and
`MimeType=application/pdf;` in its desktop entry, and the `tpdf-cli` extracted from the
`.deb` and from the `.AppImage` each answered `--version` with 26.10.19 and verified
`testdata/incr-signed.pdf`. Not run: `installed_check.py`, and the published Linux packages
installed and driven as a window; the window checks of 2026-10-14 drove a `.deb` built in
WSL from the tree. winget was not updated. Step 7's pass after the release, `--since
v26.10.18 --resume` on `9bf7db13`, found no survivor: 126 of 128 Rust mutations caught and
2 skipped as Windows-only, 457 of 457 frontend, and 32 of 32 in `mutate_python.py`. The
22 Rust verdicts of the release run were discarded by `--resume`, since the tree had moved.

**26.10.19 verification, 2026-10-14:** the first release with Linux packages. All 31
gates passed on the release tree on macOS arm64 (3,104 Rust tests with eleven documented
ignored) and on Ubuntu 24.04 under WSL on the tree before the version bump, and
`check_windows.py` type-checked the Windows tree. `scripts/run_on_windows.py --suite all`
passed 3,127 unit tests and 576 command-line tests with 4 skipped on the Windows build
computer. `scripts/run_on_mac.py` passed 11/11 window checks on a second Mac after one fix:
the viewer check's list of commands a document does not earn lacked *Add a certificate file
for signing...*, which exists only on Linux and is withheld everywhere else. Every mutation
selected `--near v26.10.18` was caught, 22 Rust and 85 frontend, and so were all 32 of
`mutate_python.py`; the Linux-only modules cannot be mutated on a Mac and were mutated by hand
under WSL as each landed (their commits say which). The normal bundle, with the development
engine hidden: the bundled `tpdf-cli` answered `--version` with 26.10.19, verified
`testdata/incr-signed.pdf` and read a fixture's text, and with `text-heavy.pdf` open two
workers mapped the bundle's `libpdfium.dylib` and the application process mapped none. On
Linux the print dialog, a file manager's double-click and the PIN dialog were driven by hand
the day before the tag (*Printing on Linux* above, and the commit that fixed what they
found); `v26.10.19-rc2` built and attached the deb and the AppImage, and `latest.json` named
only macOS and Windows. `tauri-plugin-updater` 2.14.0 was held at 2.13.3 (step 1). **Not
run:** `print-probe` and `redact-reach-probe` on Windows, `menu_check.py` and `save_check.py`,
and the signing phase of the window checks.

**26.10.18 publication, 2026-10-13:** `Audit` and CI were green on the release commit
`8409c945`, both legs, and the tag sits on it. No release mechanics had changed since
26.10.17, so there was no rehearsal tag. The `Release` run 38423439098 skipped its gates on
CI's account. The Windows job passed at the first attempt and logged
`fetched from ssign-5fd4daf-tpdf.1, digest as recorded`. The macOS job built, signed, was
accepted by Apple's notary and stapled, then failed in `bundle_dmg.sh` with no reason
printed; rerun once with `gh run rerun --failed`, it passed. On the draft's files: 6
assets, counted with GraphQL, `latest.json` named 26.10.18 for both platforms, and the
`.dmg` was accepted by Gatekeeper as Notarized Developer ID, stapled, with a `tpdf-cli`
inside that answered `--version` with 26.10.18, verified `testdata/incr-signed.pdf` and
lists `--lock`. `scripts/publish_release.py v26.10.18 --publish` published it. Without
authentication the `.dmg` and the `-setup.exe` answer 200 and `latest.json` offers
26.10.18 for `darwin-aarch64` and `windows-x86_64`; on a Windows 11 computer
`Get-AuthenticodeSignature` reads the downloaded installer as `Valid` and timestamped.
The Homebrew cask went from 26.10.17 to 26.10.18; `brew audit --cask --online` printed no
finding and `brew livecheck` reads 26.10.18. An installed 26.10.17 updated itself:
`update_check_mac.py` 12/12 on a second Mac and `update_check.py` 17/17 on Windows, both
started about twenty minutes after publication and offered the update at once. Not run:
the `no-press` control of either update check, `installed_check.py`, and the `--since`
pass of the mutation tables that step 7 leaves for after the release. winget was not
updated.

**26.10.18 verification, 2026-10-13:** all 31 gates passed on the release tree on macOS
arm64 (3,100 Rust tests with eleven documented ignored, 2,820 frontend tests), the
`notices` gate after `THIRD-PARTY-NOTICES.md` was regenerated for seven crates that moved
within their ranges, and `check_windows.py` type-checked the Windows tree.
`scripts/run_on_windows.py --suite all` passed 3,123 unit tests and 576 command-line
tests with 4 skipped on the Windows build computer, none of them for `qpdf`, and
`scripts/run_on_mac.py` passed 11/11 window checks on a second Mac. Every mutation
selected `--near v26.10.17` ran in the mutation worktree, 81 Rust and 76 frontend, and so
did all 32 of `mutate_python.py`. One Rust mutation was red in another test than the one
named for it: the rule for a piece of one or two characters answers for an empty report
too, so the test of the empty report no longer shows `marked_pages_note` losing its first
line. The row names the test of one finding not placed, and is red there through the
harness. The normal bundle, with the development engine hidden: the bundled `tpdf-cli`
answered `--version` with 26.10.18, verified `testdata/incr-signed.pdf` and read a
fixture's text, and with `text-heavy.pdf` open in the background two workers mapped the
bundle's `libpdfium.dylib` and the application process mapped none. The lock itself was
measured against pyHanko 0.36.2 with `sign-probe --lock` when it was written (the results
are beside the probe's commands near the top of this file). `SECURITY.md` said a timestamp request needs the
reader's explicit signing options; the dialog now opens with an authority selected, and
the sentence says so. `vitest` went to 5.0.4, the one update `npm outdated` listed.
**Not run:** the signing phase of the window checks, which signs with a real key and is
never started unattended, so the new dialog was used in a development window by the
owner and driven by no harness; `menu_check.py` and `save_check.py`; `print-probe` and
`redact-reach-probe`. Not measured: Acrobat on a locked document.

**26.10.17 publication, 2026-10-11:** `Audit` and CI were green on the release commit
`8fcfcce8`. `v26.10.17-rc1` was built first (run 38128819644), because the release workflow
fetches the signing client from a release since this version: its Windows leg logged
`fetched from ssign-5fd4daf-tpdf.1, digest as recorded`, and the tool from its notarized
Mac bundle redacted the four passages of `site/sample-redacted.pdf` with exit 0. The
release run 38130677995 passed at the first attempt and logged the same line, so this is
the first release that did not build the client. `scripts/publish_release.py` published
it; read without a token, `releases/latest` is `v26.10.17` with its six files and
`latest.json` offers 26.10.17 for both platforms. The Homebrew cask is at 26.10.17. An
installed 26.10.16 updated itself: `update_check.py` 17/17 on Windows with its `no-press`
control failing where it is aimed, and `update_check_mac.py` 12/12 on a Mac, both started
about a quarter of an hour after publication and offered the update at once. Not run:
`installed_check.py` on Windows beyond what the update check reads.

**26.10.17 verification, 2026-10-11:** all 31 gates passed on the release tree on macOS
arm64, and `check_windows.py` type-checked the Windows tree. `scripts/run_on_windows.py
--suite all` passed 3,110 unit tests and 567 command-line tests on the Windows build
computer, and `scripts/run_on_mac.py` passed 11/11 window checks on a second Mac. Every
mutation selected `--near v26.10.16` was caught, 8 Rust and 7 frontend, and so were all 32
of `mutate_python.py`. The change is the rule for one run of text, in `cli/regions.rs` and
`text.ts`: `tpdf redact` on the four passages of `site/sample-redacted.pdf` exits 0 and
`tpdf hidden` finds nothing in the result, where 26.10.16 answered *not verified* for the
amount. `docs/THREAT-MODEL.md` names neither function and no claim in it changed. The
release workflow fetches the signing client from a release since this version, so
`v26.10.17-rc1` was built first. Not run: the window's *Redact every search result* by a
window check (unit tests and the shared samples hold it), `print-probe`, and
`redact-reach-probe`. `@tauri-apps/cli` went to 2.12.2, the one update `npm outdated` listed.

**26.10.16 publication, 2026-10-10:** `Audit` and CI were green on the release commit
`f3e8661f`, both legs, `Audit` with its check of the PDFium pin. No release mechanics had
changed since 26.10.15, so there was no rehearsal tag. The `Release` run 38081159571
skipped its gates on CI's account and both platform jobs passed at the first attempt. On
the draft's files: `installed_check.py` passed 23 of 23 with the installer on Windows 11,
`Get-AuthenticodeSignature` read it as `Valid`, and the `.dmg` was accepted by Gatekeeper
as Notarized Developer ID, stapled, with a `tpdf-cli` inside that answered `--version` with
26.10.16, verified `testdata/incr-signed.pdf` and read a fixture's text. The Windows
engine's version resource reads `0.0.8078.1`. `scripts/publish_release.py v26.10.16
--publish` published the draft of 6 assets. Without authentication the `.dmg` and the
`-setup.exe` answer 200 and `latest.json` offers 26.10.16 for `darwin-aarch64` and
`windows-x86_64`. Step 12 passed on both platforms from 26.10.15, `update_check.py` 17 of
17 and `update_check_mac.py` 12 of 12, each with its `no-press` control failing the 7
checks it is aimed at. **Both failed at their first attempt, a few minutes after
publication, at the offer**: the Windows window showed no update button in two runs, and
the Mac's menu item stayed disabled in one, while `latest.json` read from the same
address by `curl` already named 26.10.16. Run again about a quarter of an hour after
publication, both passed. Why an installed copy was not offered what the address already
served is not established; at 26.10.15 the first runs came later after publication and
passed. Until it is, a missing offer in the first minutes is a reason to run the check
again and not yet a finding about the release. The Homebrew cask went from 26.10.15 to
26.10.16; `brew audit --cask --online` passed and `brew livecheck` reads 26.10.16.

**26.10.16 verification, 2026-10-10:** all 31 gates passed on the release tree on macOS
arm64 (3,083 Rust tests with eleven documented ignored, 2,809 frontend tests) and
`check_windows.py` type-checked the Windows tree. The only Rust change since 26.10.15 is a
comment in `save.rs`, so `--near v26.10.15` selected no Rust mutation and the harness
refused to call that a pass; `--since v26.10.15` ran every mutation in that file instead.
The one frontend mutation near a change and all 31 rows of `mutate_python.py` were caught.
The engine is the new pin; its probes and both platforms' runs are in the paragraph on
`8078-tpdf.1` near the top of this file, and `window_checks.py` passed 11 of 11 again on a
second Mac on the release tree. **The updater's own crates moved**, `tauri-plugin-updater`
to 2.13.3 and `minisign-verify` from 0.2.5 to 0.3.0, the crate that checks an update's
signature, and step 12 exercises the previous release's updater and not this one's. So
this tree was built on the second Mac as version 26.10.1 and updated itself with
`update_check_mac.py`: it was offered 26.10.15, installed it, came back as 26.10.15 and
the bundle read as Notarized Developer ID. Two of its twelve checks failed and both were
the test copy's doing: the tool in the bundle takes its version from `Cargo.toml`, and
the copy could not load its engine because that build skipped signing the bundled
library, so no document was open to come back. **Not run:** the Windows window checks on
the release tree (they passed on the pin commit, which differs by the version, the
dependency patch releases and the landing page), the same updater test on Windows,
`print-probe` and `redact-reach-probe`.

**26.10.15 publication, 2026-10-10:** `Audit` and CI were green on the release commit
`2751c2c2`, both legs. The installer's hooks had changed since 26.10.14, so
`v26.10.15-rc1` was built first (run 38070875264): both platform jobs passed at the first
attempt and the draft held 6 assets. Step 8 was done on that draft's files:
`installed_check.py` passed 23 of 23 with the installer on Windows 11, its second install
over the first and the `.pdf` class afterwards among them, and the `.dmg` was accepted by
Gatekeeper as Notarized Developer ID, stapled, with a `tpdf-cli` inside that answered
`--version` with 26.10.15, verified `testdata/incr-signed.pdf`, read a fixture's text and
refused `long-term` for a signer the computer does not trust; its window was not opened.
The tag and its draft were deleted. The `Release` run 38072690219 for the real tag skipped
its gates on CI's account and both platform jobs passed at the first attempt. **The
recorded digest of the signing client matched nothing**, in the rehearsal and in the real
run: the cache entry it names was gone, so both built the client from the pinned commit.
A repository's caches are limited to 10 GiB together, the Windows Rust cache alone is
6.8 GiB, and saving a new one after this cycle's dependency update evicted the 3 MiB
client. The draft held 6 assets and `scripts/publish_release.py v26.10.15 --publish`
published it. Without authentication the `.dmg` and the `-setup.exe` answer 200 and
`latest.json` offers 26.10.15 for `darwin-aarch64` and `windows-x86_64`. The downloaded
`.dmg` staples and Gatekeeper reads it as Notarized Developer ID. Step 12 passed on both
platforms from 26.10.14: `update_check.py` 17 of 17 on Windows 11, where 26.10.14 had
passed 16 with the `.pdf` class after an update as the one failure, and
`update_check_mac.py` 12 of 12 on a second Mac, each with its `no-press` control failing
the 7 checks it is aimed at. On the Mac the script has to be started from the ssh session:
started as a launchd job it waited 20 s for the menu and failed, because the Accessibility
permission belongs to what starts the script. The Homebrew cask went from 26.10.14 to
26.10.15; `brew audit --cask --online` passed and `brew livecheck` reads 26.10.15.

**26.10.14 verification, 2026-10-10:** all 31 gates passed on the release tree on macOS
arm64 (3,040 Rust tests with ten documented ignored, 2,795 frontend tests) and
`check_windows.py` type-checked the Windows tree. Every mutation selected
`--near v26.10.13` ran in the mutation worktree and was caught: 281 Rust and 407 frontend.
`window_checks.py` passed 11 of 11 on Windows 11 and 11 of 11 on macOS, both on the release
tree, and on Windows `tabs_check.py --phase fields` passed 76 of 76 with one not applicable
and `--phase fields-turned` 10 of 10. The macOS run was made on a second Mac over ssh, as a
one-off launchd job in the logged-in session (`launchctl bootstrap gui/<uid>`). That Mac
has no display attached and needs none: it reports one of 1920 by 1080 either way. What it
needs is a session that is not locked, and the screen saver locks it: with no setting of
its own the saver starts after twenty minutes without input, and `loginwindow` then logs
`screenSaverDidFade | about to call lockScreen`, whatever *Require password* says. With
`defaults -currentHost write com.apple.screensaver idleTime -int 0` the session was still
unlocked after 25 minutes without input and with nobody connected, and the eleven checks
passed again in that state. Its first attempt failed in every check with *could not load its PDF engine*: the
checks build was signed with the Developer ID and the hardened runtime and the vendored
PDFium was signed ad hoc, so the library was refused. Signing the bundled
`libpdfium.dylib` and then the app again with the same identity is what made it load.
`docs/THREAT-MODEL.md` and `README.md` were checked for what this cycle changed and not
read through. **Not run:** the Tab check of `form-beside` with a form of two controls,
`print-probe`, `redact-reach-probe`, `run_on_windows.py` (CI's Windows leg ran the suites
on the release commit), and the update from 26.10.13 by hand (step 12).

**26.10.14 publication, 2026-10-10:** `Audit` and CI were green on the release commit
`2756931a`, both legs. The release workflow had changed since 26.10.13 (the signing client
is checked against a recorded digest, and the release job restores no Rust cache), so
`v26.10.14-rc1` was built first (run 38039198093): both platform jobs passed at the first
attempt and the draft held 6 assets. Step 8 was done on that draft's files and not on a
local build: `installed_check.py` passed 22 of 22 with the installer on Windows 11, and the
`.dmg` was accepted by Gatekeeper as Notarized Developer ID, stapled, with a `tpdf-cli`
inside that answered `--version` with 26.10.14, verified `testdata/incr-signed.pdf` and
read a fixture's text; its window was not opened. The tag and its draft were deleted. The
`Release` run 38044233309 for the real tag skipped its gates on CI's account and both
platform jobs passed at the first attempt. `SSIGN_SHA256` is still empty, so the Windows
leg built the signing client from source. The draft held 6 assets and
`scripts/publish_release.py v26.10.14 --publish` published it. Without authentication the
`.dmg` and the `-setup.exe` answer 200 and `latest.json` offers 26.10.14 for
`darwin-aarch64` and `windows-x86_64`. The downloaded `.dmg` staples and Gatekeeper reads
it as Notarized Developer ID. On a Windows 11 computer `Get-AuthenticodeSignature` reads
the downloaded installer as `Valid` and timestamped by
`CN=Open Source Developer Timo Stein`. The Homebrew cask went from 26.10.13 to 26.10.14;
`brew audit --cask --online` passed and `brew livecheck` reads 26.10.14. winget was not
updated.

**26.10.13 verification, macOS arm64, 2026-10-08:** all 30 gates passed on the release tree
(2,957 Rust tests with ten documented ignored, 2,562 frontend tests) and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--near v26.10.12` ran and was
caught, 31 Rust and 63 frontend. The window's check for text the pages do not show was held
against `tpdf hidden --json` on one document by `tests/cli/hidden.rs`, passage for passage;
with the tool's new last line removed by hand that test failed. CI was green on both legs
for `03f8b62c`, the first Windows run of that check and of the launch repair.
**Not run:** any window harness, so the new sidebar tab, the ring around a passage and the
Stop button were never looked at on a screen; the Windows test suites outside CI; the
bundle smoke test (step 8) and the hand-applied update (step 12). The launch repair shows
only in the update *after* this one, because the version being replaced does the restart.
`docs/THREAT-MODEL.md` §T6 gained a paragraph for the new command: it parses nothing in the
application's process, and no probe reads the process's image table during a check.

**26.10.13 publication, 2026-10-08:** `Audit` and CI were green on the release commit
`03061b3a`, both legs. The release workflow had changed since 26.10.12, taking the signing
client from a cache, so `v26.10.13-rc1` was built first (run 37822087488): the client was
restored from the cache, both platform jobs passed at the first attempt, and the tag and
its draft were deleted. The `Release` run 37825754203 for the real tag skipped its gates on
CI's account and both platform jobs passed at the first attempt. The Windows leg signed ten
files with one login, each written at the first attempt, read the installer, `tpdf.exe`,
`tpdf-cli.exe` and `pdfium.dll` back as signed, then installed and read `uninstall.exe`
back as `Valid` and timestamped. The draft held 6 assets and
`scripts/publish_release.py v26.10.13 --publish` published it. Without authentication the
`.dmg`, the `-setup.exe` and its `.sig` answer 200 and `latest.json` offers 26.10.13 for
`darwin-aarch64` and `windows-x86_64`. The downloaded `.dmg` staples and Gatekeeper reads
it as Notarized Developer ID. The downloaded installer verifies with `minisign` against its
`.sig`, and on a Windows 11 computer `Get-AuthenticodeSignature` reads it as `Valid` and
timestamped by `CN=Open Source Developer Timo Stein`, with the SHA-256 of the copy
downloaded on macOS; the 26.10.10 installer read `NotSigned` in the same command, as the
control. The Homebrew cask went from 26.10.12 to 26.10.13; `brew audit --cask --online`
passed and `brew livecheck` reads 26.10.13. winget was not updated. The installer was not
run outside the release run, and the update from an earlier version (step 12) was not
applied by hand before this was written.

**26.10.12 verification, macOS arm64, 2026-10-08:** all 30 gates passed on the release tree
(2,942 Rust tests with ten documented ignored, 2,528 frontend tests) and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--near v26.10.11` ran and was
caught, 15 Rust and 16 frontend. `raster-redact-probe` painted black, red and white and
read each pixel back from the rendered page; with the read-back held to black by hand it
failed on the red page. `sign-rehearsal.yml` passed in run 37733051706, starting the sign
command the way makensis starts it and waiting 5 attempts for a file held open. The
uninstaller's cause and fix are in *Signing with the Certum certificate*; a real signature
on an uninstaller had not been seen when this was written, and the release run's new
check is where it is. **Not run:** the Windows test suites outside CI, any window harness,
the bundle smoke test (step 8) and the hand-applied update (step 12). The image-only copy
in another colour was not looked at in a window, and the README pictures were not retaken.
`docs/THREAT-MODEL.md` needed no change: the cycle added no command and no authority, and
the image-only job carries one more value, a colour of three.

**26.10.12 publication, 2026-10-08:** `Audit` and CI were green on the release commit
`3fbfe708`, both legs, and the tag sits on it. The `Release` run 37736261721 skipped its
gates on CI's account and both platform jobs passed at the first attempt. The Windows leg
signed ten files with one login, each written at the first attempt: the nine of 26.10.11
and the uninstaller, which makensis hands over as a temporary file (`nstBC68.tmp`). It read
the installer, `tpdf.exe`, `tpdf-cli.exe` and `pdfium.dll` back as signed, then installed
and read `uninstall.exe` back as `Valid`, timestamped, signer
`CN=Open Source Developer Timo Stein`. That is the first signed uninstaller. The draft held
6 assets and `scripts/publish_release.py v26.10.12 --publish` published it. Without
authentication the `.dmg`, the `-setup.exe` and its `.sig` answer 200 and `latest.json`
offers 26.10.12 for `darwin-aarch64` and, with the installer, for `windows-x86_64`. The
downloaded `.dmg` staples and Gatekeeper reads it as Notarized Developer ID. The downloaded
installer verifies with `minisign` against its `.sig`, and on a Windows 11 computer that
never held the certificate `Get-AuthenticodeSignature` reads it as `Valid` and timestamped
by the same signer, with the SHA-256 of the copy downloaded on macOS. The Homebrew cask
went from 26.10.11 to 26.10.12; `brew audit --cask --online` passed and `brew livecheck`
reads 26.10.12. winget was not updated. The installer was not run outside the release run,
so the uninstaller's signature was read on the runner only, and the update from an earlier
version (step 12) was not applied by hand before this was written. It was applied later
that day: a Windows 11 computer holding 26.10.10 from the installer was updated with the
button in the application. Afterwards the registry held one tpdf entry, 26.10.12, out of
365 entries read, and `tpdf.exe` and `uninstall.exe` in the installation folder both read
`Valid` with the signer above. The two rehearsal tags `v26.9.22-rc1` and `v26.9.22-rc2`
and their drafts were deleted the same day.

**26.10.11 verification, macOS arm64, 2026-10-07:** all 30 gates passed on the release tree
(2,941 Rust tests with ten documented ignored, 2,528 frontend tests) and `check_windows.py`
type-checked the Windows tree. The program is the one in 26.10.10: since that tag one Rust
file changed, in a comment. What changed is how a release is built, so what was verified is
that: the four rehearsal tags in *Signing with the Certum certificate*, of which
`v26.10.11-rc4` passed on both legs with the installer, `tpdf.exe`, `tpdf-cli.exe` and
`pdfium.dll` read back as signed and the updater signature verified against the signed
installer. Since rc4 the tree changed in version numbers, documents, the release notes and
that comment. **Not run:** the Windows test suites, any mutation, any window harness, the
bundle smoke test (step 8) and the hand-applied update (step 12). Not established: whether
`uninstall.exe` is signed, and what Smart App Control does with the installer. The README,
`SECURITY.md` and `docs/DETAIL.md` no longer say that Windows is unsigned, and the weekly
release limit, which existed because it was, ended.

**26.10.11 publication, 2026-10-08:** `Audit` and CI were green on the release commit
`293df0d3`, both legs, and the tag sits on it. The `Release` run for `v26.10.11` skipped its
gates on CI's account and both platform jobs passed at the first attempt; the Windows leg
signed nine files with one login and read the installer, `tpdf.exe`, `tpdf-cli.exe` and
`pdfium.dll` back as signed. The draft held 6 assets and
`scripts/publish_release.py v26.10.11 --publish` published it. Without authentication the
`.dmg`, the `-setup.exe` and the updater archive answer 200 and `latest.json` offers
26.10.11 for `darwin-aarch64` and, with the installer, for `windows-x86_64`. The downloaded
`.dmg` staples and Gatekeeper reads it as Notarized Developer ID. The downloaded installer
verifies with `minisign` against its `.sig`, and on a Windows 11 computer that never held
the certificate `Get-AuthenticodeSignature` reads it as `Valid`, timestamped, signer
`CN=Open Source Developer Timo Stein`; the 26.10.10 installer fetched beside it reads
`NotSigned`. Smart App Control is off on that computer. The Homebrew cask went from
26.10.10 to 26.10.11; `brew audit --cask --online` passed and `brew livecheck` reads
26.10.11. winget was not updated. The update from 26.10.10 (step 12) was not applied by
hand before this was written, and the installer was not run.

**26.10.10 verification, macOS arm64, 2026-10-07:** all 30 gates passed on the release tree
(2,941 Rust tests with ten documented ignored, 2,528 frontend tests), after four mutation
anchors the cycle's own changes had moved were re-aimed, and `check_windows.py`
type-checked the Windows tree. `--near v26.10.9` selected 190 Rust mutations and 89
frontend ones, run in the mutation worktree on the release tree, and every one was caught
by the test named for it. The unit tests and the command-line suite ran on Windows 11
through `scripts/run_on_windows.py` on the release tree: 2,964 unit tests passed, and the
command-line suite failed one new check and then passed (535, with three skipped, which
need the Mac's recogniser). The failure was the check and not the tool: it asked for
*verified* on a page whose control word is 8.9 points tall, which that platform's
recogniser does not read back, and it now asks that the copy is written and that the exit
code agrees with the verdict. The normal bundle, with the development engine hidden: the
bundled `tpdf-cli` answered `--version` with 26.10.10 and `verify`, took two numbers out
of their lines with `--fill red` and reported the copy verified, and `hidden` found
nothing in that copy. The bundle's window was opened on `text-heavy.pdf` in the
background and a worker of it mapped the bundle's own `libpdfium.dylib`; what the
application process maps was not read, and the window was not looked at. The window
changes of this cycle (the armed redaction tool marking text, the command that leaves it
armed, the colour swatches, the tab strip) were used in a development window by the person
who reported them and were not driven by any harness; `docs/VERIFICATION.md` has what was
measured on the files they were reported on. `worker-probe`, `backend-probe`, the window
harnesses and `redact-reach-probe` were not run, and the README pictures were not
retaken; no picture shows the Redactions panel's new swatches. `docs/THREAT-MODEL.md`
gained T6.39 and T6.40 and lost the sentence that the fill is black. The release notes in
`release.yml` were rewritten and lead with the redaction changes. This release is one day
after 26.10.9, against the rule in `docs/DETAIL.md` *Versioning*: it was asked for, and it
carries a fix for redactions that were refused outright.

**26.10.10 publication, 2026-10-07:** `Audit` and CI were green on the release commit
`f931f6e9`, both legs, and the tag sits on it. Only the text of the release notes had
changed in `release.yml`, so no rehearsal tag was cut; the `Release` run for `v26.10.10`
skipped its gates on CI's account. The Windows job passed at the first attempt. The macOS
job built and signed, then failed in notarisation with `HTTPClientError.connectTimeout`
while uploading to Apple; the same job had timed out twice fetching its cache. Rerun once
with `gh run rerun --failed`, it passed. The draft held 8 assets and
`scripts/publish_release.py v26.10.10 --publish` published it. Without authentication the
`.dmg`, the `.msi`, the `-setup.exe` and the updater archive answer 200 and `latest.json`
offers 26.10.10 for `darwin-aarch64` and `windows-x86_64`. The downloaded `.dmg` staples
and Gatekeeper reads it as Notarized Developer ID. The Homebrew cask went from 26.10.9 to
26.10.10; `brew audit --cask --online` passed and `brew livecheck` reads 26.10.10. winget
was not updated. The update from 26.10.9 (step 12) was not applied by hand before this was
written.

**26.10.9 verification, macOS arm64, 2026-10-06:** all 30 gates passed on the release tree
(2,865 Rust tests with ten documented ignored, 2,491 frontend tests) and `check_windows.py`
type-checked the Windows tree. `--near v26.10.8` selected 247 Rust mutations and no frontend
one, since no frontend file changed. 246 were caught by the test named for them. The one
that was not, `wrap: the measure is the page`, was red in two other tests and not in its
own: a paragraph of several lines now gets the same measure from the page's paragraphs,
its own among them, when its own reading is taken away, so an edit past the page edge
breaks where it did. The entry names the test that pins a last line's own measure, and is
red there through the harness. `worker-probe` 48/48; `backend-probe` 41/44 on
`text-heavy.pdf` with three skipped and 42/44 on `vector-heavy.pdf` with two skipped. The
unit tests and the command-line suite passed on Windows 11 through
`scripts/run_on_windows.py` on the release tree (2,888 unit tests, 517 command-line checks
with three skipped, which need the Mac's recogniser). Two crates moved in the refresh,
`embed-resource` to 3.0.12 and `winreg` to 0.56.0; no npm package was behind and no major
version is held back. The normal bundle, with the development engine hidden: the bundled
`tpdf-cli` answered `--version` with 26.10.9 and `verify`, and `text-runs` offered 32 runs
on the LibreOffice fixture and 83 on the document the work was measured on, which 26.10.8
refuses. The bundle's own window was not opened. The window checks of this cycle were made
in a development window against the dev server, on a scratch copy of that document, and
`docs/VERIFICATION.md` has what each found: two defects no probe had shown, both fixed
before this commit. Not looked at in a window: the last line of an item of several growing
past its measure, and anything on Windows. The window harnesses were not run and the README
pictures were not retaken; no picture shows text being edited in a list. `scripts/text_list_check.py`
passed on the fixture. `docs/THREAT-MODEL.md` needed no change: the cycle added no command,
no request and no authority. The release notes in `release.yml` were rewritten and lead
with the fix.

**26.10.9 publication, 2026-10-06:** `Audit` and CI were green on the release commit
`68c36934`, both legs, and the tag sits on it. Only the text of the release notes had
changed in `release.yml`, so no rehearsal tag was cut; the `Release` run for `v26.10.9`
skipped its gates on CI's account and both platform jobs passed at the first attempt. The
draft held 8 assets and `scripts/publish_release.py v26.10.9 --publish` published it.
Without authentication the `.dmg`, the `.msi`, the `-setup.exe` and the updater archive
answer 200 and `latest.json` offers 26.10.9 for `darwin-aarch64` and `windows-x86_64`. The
downloaded `.dmg` staples and Gatekeeper reads it as Notarized Developer ID. The Homebrew
cask went from 26.10.8 to 26.10.9; `brew audit --cask --online` passed and `brew livecheck`
reads 26.10.9. winget was not updated. The update from 26.10.8 (step 12) was not applied
by hand before this was written.
Afterwards every Rust mutation in a file changed since `v26.10.8` ran, 863, and all were
caught; no frontend file had changed.

**26.10.8 verification, macOS arm64, 2026-10-06:** all 30 gates passed on the release tree
(2,836 Rust tests with ten documented ignored, 2,491 frontend tests) and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--near v26.10.7` ran and was caught,
23 Rust and 12 frontend. The command-line suite's new checks, a two-page scan with a Latin
page and the Arabic page of `multilingual.pdf` through `tpdf ocr` and through the window's
own command, were shown red by six reversions made by hand. Those checks, like the rest of
the suite's OCR part, do not run on a hosted runner, which cannot generate
`text-base14.pdf`; they ran here. `worker-probe` 48/48; `backend-probe` 41/44 on
`text-heavy.pdf` with three skipped and 42/44 on `vector-heavy.pdf` with two skipped. The
unit tests and the command-line suite passed on Windows 11 through
`scripts/run_on_windows.py` on the release tree (2,859 unit tests, 517 command-line checks
with three skipped, the three being checks that need the Mac's recogniser). The normal
bundle, with the development engine hidden: the bundled `tpdf-cli` answered `--version`
with 26.10.8 and `verify`, and on the two-page scan gave page 1 its text, named page 2 as
refused in its report and in `--json`, and exited 0. In the bundle's window, with real key events and the save panel answered by typing a
path: a Latin page with an Arabic page gave the copy, opened it and said *Text was added to
1 of 2 pages (30 words). The recogniser refused page 2, ...* on two lines, and the Arabic
page alone gave *No page was given text. ...* and no file. That check found the commands
left grey after the second case until a tab was switched, which is older than this
release; the task queue now asks for the commands again when it goes idle, and the same
run in a rebuilt bundle left Print and *Recognise text* enabled. Both closing lines are set
in the failure line's red, the first of them for a copy that was written; that was left.
`v26.10.7` was built and tagged and its draft deleted unpublished: letting Vision
work out the script lets it take a very poor scan for a script whose model the worker
cannot load, and in that version one such page cost a document its copy. A rule to refuse a
Greek or Armenian page, which gets wrong text, was measured on 345 pages and not built; the
trap entry has the table. The window harnesses were not run and the README pictures were
not retaken. The release notes in `release.yml` were rewritten.

**26.10.8 publication, 2026-10-06:** `Audit` and CI were green on the release commit
`35f4c0a4`, both legs, and the tag sits on it. The `Release` run for `v26.10.8` skipped its
gates on CI's account and both platform jobs passed at the first attempt. The draft held 8
assets and `scripts/publish_release.py v26.10.8 --publish` published it. Without
authentication the `.dmg`, the `.msi`, the `-setup.exe` and the updater archive answer 200
and `latest.json` offers 26.10.8 for `darwin-aarch64` and `windows-x86_64`. The downloaded
`.dmg` staples and Gatekeeper reads it as Notarized Developer ID. The Homebrew cask went
from 26.10.6 to 26.10.8; `brew audit --cask --online` passed and `brew livecheck` reads
26.10.8. `v26.10.7` has a tag at `0ae3e0e8` and no release: its draft was deleted before
publication, so no installed copy was offered it. The update from 26.10.6 (step 12) was
not applied by hand before this was written.
Afterwards every mutation in a file changed since `v26.10.6` ran, 90 Rust and 166
frontend, and all were caught.

**26.10.7 verification, macOS arm64, 2026-10-05:** all 30 gates passed on the release tree
(2,831 Rust tests with ten documented ignored, 2,489 frontend tests) and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--near v26.10.6` ran and was caught,
28 Rust and 43 frontend. The command-line suite's two new checks, a scan of the Japanese
page of `multilingual.pdf` read with no language named by `tpdf ocr` and by the window's own
command, were each shown red by putting that call back to the plain one. `worker-probe`
48/48; `backend-probe` 41/44 on `text-heavy.pdf` with three skipped and 42/44 on
`vector-heavy.pdf` with two skipped. The unit tests and the command-line suite passed on
Windows 11 through `scripts/run_on_windows.py` (2,854 unit tests, 517 command-line checks
with two skipped) at `737a4bf8`, the commit before this one; this tree differs from it in
version files, `objc2` 0.6.5, which Windows does not build, and prose. The run on the
release tree itself did not happen: the Windows machine was offline. The normal bundle,
with the development engine hidden: the bundled `tpdf-cli` answered `--version` with
26.10.7 and `verify`, and read the Japanese scan with no language named. In a development
window, with real key events: *Recognise text: language...* is in the File menu and enabled
with no document open, an ambiguous beginning names its candidates, an unknown one lists
what the computer offers, and an answer is stored in the session file. That check found the
system's spelling suggestions covering the answer and the confirmation set as a failure;
both are corrected. Reading with a second installed recogniser language on Windows was not
tried, and neither was a relaunch with a remembered language. The window harnesses were not
run and the README pictures were not retaken; no picture shows the changed menu.
`docs/THREAT-MODEL.md` states the two new commands (T6.37). The release notes in
`release.yml` were rewritten and lead with the fix.

**26.10.6 publication, 2026-10-05:** `Audit` and CI were green on the release commit
`a3a39e7a`, both legs, and the tag sits on it. Only the text of the release notes had
changed in `release.yml`, so no rehearsal tag was cut; the `Release` run for `v26.10.6`
skipped its gates on CI's account and both platform jobs passed at the first attempt. The
draft held 8 assets and `scripts/publish_release.py v26.10.6 --publish` published it.
Without authentication the `.dmg`, the `.msi`, the `-setup.exe` and the updater archive
answer 200 and `latest.json` offers 26.10.6 for `darwin-aarch64` and `windows-x86_64`. The
downloaded `.dmg` staples and Gatekeeper reads it as Notarized Developer ID. The Homebrew
cask was set to 26.10.6; `brew audit --cask --online` passed and `brew livecheck` reads
26.10.6. The update from 26.10.5 (step 12) was not applied by hand before this was written.
Afterwards every mutation in a file changed since `v26.10.5` ran, 680 Rust and 549
frontend, and all were caught.

**26.10.5 verification, macOS arm64, 2026-10-05:** all 30 gates passed on the release tree
(2,806 Rust tests with ten documented ignored, 2,321 frontend tests), and `check_windows.py`
type-checked the Windows tree after it found one `mut` that only a Unix build changes. Every
Rust and frontend mutation selected `--near v26.10.4` ran: 853 Rust, of which one was not
caught, and 266 frontend, all caught. The one was `tpdf text -o` checking for an existing
output twice; the older check is gone and the entry is aimed at the one that remains. The
selection itself was corrected in the same step (step 7 has it). `worker-probe` 48/48;
`backend-probe` 41/44 on `text-heavy.pdf`, the three skipped being the withdrawal checks,
which ask for `vector-heavy.pdf` and were not rerun on it. `sign-probe` 18/18 and 20/20 with
the pinned pyHanko. The unit tests and the command-line suite passed on Windows 11 through
`scripts/run_on_windows.py` (2,830 unit tests, 515 command-line checks), on the tree before
the duplicate check was removed and before the version bump. The normal bundle, with the
development engine hidden: the bundled `tpdf-cli` answered `--version` with 26.10.5,
`verify` with the new *Appended* row, and a text extract; with real pointer and key events
(`cliclick`), ⌘C over a selected error message reached the clipboard and *Place signature
image* is set in the system font. `menu_check.py`, `save_check.py` and the checks build's
window phases were not run. The scrollbar under *Place signature image* on Windows was not
looked at before this commit and is checked on the release installer. `docs/THREAT-MODEL.md`
already stated the redaction sweep and the certificate reader's new module. The release
notes in `release.yml` were rewritten and lead with the redaction fix.

**26.10.5 publication, 2026-10-05:** `Audit` and CI were green on the release commit
`681611e3`, both legs, and the tag sits on it; the `sign-probe` step ran on the macOS leg
for the first time, 18/18 and 20/20 on OpenSSL 3.6.4. The unit tests and the command-line
suite passed again on Windows 11 on that tree (2,830 and 515). The upload of the stapled
`.dmg` takes the tag from `env:` since this cycle and no rehearsal tag was cut for it; the
`Release` run for `v26.10.5` skipped its gates on CI's account and both platform jobs passed
at the first attempt. The draft held 8 assets, counted with GraphQL, and
`scripts/publish_release.py v26.10.5 --publish` published it, the first publication its
`Audit` check ran before. Without authentication the `.dmg`, the `.msi`, the `-setup.exe`
and the updater archive answer 200 and `latest.json` offers 26.10.5 for `darwin-aarch64` and
`windows-x86_64`. The downloaded `.dmg` staples, Gatekeeper reads it and the application in
it as Notarized Developer ID, and the bundled `tpdf-cli` reports 26.10.5. The Homebrew cask
was set to 26.10.5; `brew audit --cask --online` passed and `brew livecheck` reads 26.10.5.
winget was not updated. Step 12 and the scrollbar under *Place signature image* on the
Windows installer are the owner's and are not recorded here yet.

**26.10.4 verification, macOS arm64, 2026-10-04:** all 29 gates passed on the release tree
(2,706 Rust tests with ten documented ignored, 2,270 frontend tests), and `check_windows.py`
type-checked the Windows tree. Every Rust and frontend mutation selected `--since v26.10.3`
ran and was caught: 981 Rust and 495 frontend. One frontend mutation had been blunted by the
change that made one press pick a rectangle and a second open it (a press with Shift no
longer opens a note either way, so the test named for it could not see the `return` go);
the test was sharpened and the mutation is caught again. The window mutations that
selection names were not run. Before the push the unit and command-line suites passed on
Windows 11 through `scripts/run_on_windows.py` at `db5ed995` (2,730 unit tests, 483
command-line checks, one skipped by design), and CI passed there on both platforms. On the
checks build, at the commits the features landed in: `tabs_check.py --phase fields` 54/54
with the driver's read and fill of the saved file, `viewer_check.py` 328/328 on
`text-heavy.pdf` and 233/233 on `vector-heavy.pdf`; the forms and tabs phases were last run
earlier the same day, before the properties panel. `scripts/made_form_check.py` passed in
both directions against PDFKit. On the normal 26.10.4 bundle with the development engine
hidden: `tpdf-cli --version`, `verify` and `fields`, `menu_check.py` (9 menus) and
`save_check.py`. Not done: Acrobat, which needs a person, and step 12.

**26.10.4 publication, 2026-10-04:** `Audit` and CI were green on the release commit
`2b3cbcd8`, both legs, and the tag sits on it. The release mechanics were as in 26.10.3, so
no rehearsal tag was cut. The `Release` run for `v26.10.4` skipped its gates on that account
and both platform jobs passed at the first attempt. The draft held 8 assets, counted with
GraphQL, and `scripts/publish_release.py v26.10.4 --publish` published it. Without
authentication the `.dmg`, the `.msi`, the `-setup.exe` and the updater archive answer 200
and `latest.json` offers 26.10.4 for `darwin-aarch64` and `windows-x86_64`. The downloaded
`.dmg` staples, Gatekeeper reads it and the application in it as Notarized Developer ID, and
the bundled `tpdf-cli` reports 26.10.4. The Homebrew cask was set to 26.10.4; `brew audit
--cask --online` passed and `brew livecheck` reads 26.10.4. winget was not updated: pull
request 445804 for 26.10.2 was still open. Step 12 is the owner's and is not recorded here
yet.

**Window checks after the form work, macOS arm64, 2026-10-04, unreleased tree:** on the
checks build, `tabs_check.py --phase fields` 36/36 with the driver's read of the saved file,
`--phase forms` 18/18, `--phase tabs` 21/21, `viewer_check.py` 327/327 on `text-heavy.pdf`
and 232/232 on `vector-heavy.pdf`, `session_check.py` tabs and places. On the normal bundle
with the development engine hidden: `menu_check.py` (9 menus, the new Arrange among them)
and `save_check.py`. Three things in the checks themselves were out of date and are
corrected, none in the application: `viewer_check.py`'s two tables of which commands a
document enables had missed four commands registered since it last ran (it was not run
for 26.10.3) and the fifteen new ones; `save_check.py` addressed menus by position and
clicked into the wrong one once Arrange went in before Page, so it addresses them by name
now; and the fields phase had no line in this file, so its first run failed on its
arguments and was read as a locked screen.

**26.10.3 verification, macOS arm64, 2026-10-03:** all 29 gates passed on the final tree
(2,650 Rust tests with ten documented ignored, 2,144 frontend tests), and `check_windows.py`
type-checked the Windows tree. Every Rust and frontend mutation selected `--since v26.10.2`
ran and was caught: 929 Rust and 460 frontend. The 55 window mutations that selection names
were not run; the field-placement and corner-resize window phases passed when those features
landed, on the checks build. The command-line suite and the unit tests also passed on Windows
11 through `scripts/run_on_windows.py` (2,674 unit tests, 479 command-line checks), on the tree
before the version bump. The normal bundle, with the development engine hidden:
`menu_check.py` (after its self-test) found a clean menu bar, and the bundled `tpdf-cli`
answered `--version` with 26.10.3, `verify`, and a text extract. `save_check.py` was not run
before this commit: the screen was locked, and the check refuses rather than guesses. The
*26.10.3 publication* note says whether it ran before the tag. `docs/THREAT-MODEL.md` was
corrected in this step: it still said a drawing inside a Form XObject is left, and it did not
cover form fields or the reopening of tabs. The release notes in `release.yml` were rewritten
before the release commit. No release mechanics changed.

**26.10.3 publication, 2026-10-04:** `save_check.py` ran before the tag, against the normal
bundle with the development engine hidden: it saved over the document twice from the menu,
`qpdf` read both saves back, and a print after a second writer was refused. `Audit` and CI
were green on the release commit `c47b6bd`, both legs, and the tag sits on it. The `Release`
run for `v26.10.3` skipped its gates on that account and both platform jobs passed at the
first attempt. The draft held 8 assets, counted with GraphQL, and
`scripts/publish_release.py v26.10.3 --publish` published it. Without authentication the
`.dmg`, the `.msi` and the `-setup.exe` answer 200 and `latest.json` offers 26.10.3 for
`darwin-aarch64` and `windows-x86_64`. The downloaded `.dmg` staples, `codesign --deep
--strict` accepts the application, Gatekeeper reads it as Notarized Developer ID, and the
bundled `tpdf-cli` reports 26.10.3. The Homebrew cask was set to 26.10.3; `brew audit --cask
--online` passed and `brew livecheck` reads 26.10.3. winget was not updated: pull request
445804 for 26.10.2 was still open. Step 12 is the owner's and is not recorded here yet.

**26.10.3, `redact-reach-probe` on Windows 11, 2026-10-03, with the gate:** 160 documents,
none refused, 13,524 regions, 60.5 s. Taken whole: 12,389 (91.6%); holding something that
cannot be taken: 1,135 (8.4%) in 47 documents, of which a drawing on the page 712, a block
drawn more than once 299, a drawing in a block 146. Read back: 12,133 regions on 346 pages.
Shown unreadable 4,037 (33.27%); could not be shown unreadable 8,041 (66.27%); still reads as
text 55 (0.45%), every span inside the region's own columns. The 2026-09-02 run read 0.00%,
37.89% and 62.11% on 109 documents; the corpus is not the same one, and the control is 32 px
on Windows since this cycle. All three outcomes other than *shown unreadable* are reported as
*not verified*. The run printed 589 of the 1,000 controls not read back as rendered at
*12 to 16 px*, under the floor for no reason the scale rule gives. **That was the probe and
not the gate, corrected 2026-10-04.** The row's upper bound is the floor, 32 px on Windows,
and its label still said 16; and a control sized to exactly 32 px comes out as 31.999998 for
about one size in eight in single precision, which a strict comparison called short. With
`ocr_gate::reaches_floor` and labels that follow the platform's floor, the same run reports
all 1,000 at 32 px and over and none short. What the engine returned for them is unchanged:
it read nothing for 261 and read spans that did not hold the control for 737.

**26.10.2 verification, macOS arm64, 2026-10-02:** all 29 gates passed on the final tree
(2,451 Rust tests with ten documented ignored, 2,026 frontend tests), and `check_windows.py`
type-checked the Windows tree. CI was green on both legs at `788a607`, two commits before the
version bump. Mutations: `--since v26.10.1` names 586 Rust mutations, of which 342 are in
`textedit.rs` and `save.rs`, where this cycle changed one visibility keyword and added one
call; those were not run. The 129 in the other changed files ran and were caught, with the
one that removes the new call in `save.rs` among them, and all 301 frontend mutations the
selection names ran and were caught. The normal bundle, with the development engine hidden:
`menu_check.py` (after its self-test) found a clean menu bar, `save_check.py` saved over the
document twice and refused a print after a second writer, and the bundled `tpdf-cli` answered
`--version` with 26.10.2, `verify`, and an extract. On Windows 11 the checks build was
photographed in light, dark and at 1,000 pixels wide with `scripts/screenshots.py` (13/13
checks in each run), which is where the wrapped menu item was found, and the default-app
command's Settings call ran in the desktop session. Not run: `print-probe`,
`redact-reach-probe` and the Windows window phases; and the default-app command's change on
macOS through a confirmed system question, because tpdf was already the default on the Mac
that ran it. Its read-back and its declined path were exercised there. No release mechanics
changed.

**26.10.2 publication, 2026-10-02:** `Audit` and CI were green on `a70e6f4` before the tag,
which is the commit after `Release v26.10.2`: it rewrites the release notes inside
`release.yml`, which still said "New in 26.10.1" when the release commit was made. The
`Release` run for `v26.10.2` skipped its gates on that account and both platform jobs passed
at the first attempt. The draft held 8 assets, counted with GraphQL, and
`scripts/publish_release.py v26.10.2 --publish` published it. Without authentication the
`.dmg`, the `.msi` and the `-setup.exe` answer 200 and `latest.json` offers 26.10.2 for
`darwin-aarch64` and `windows-x86_64`. The downloaded `.dmg` staples, `codesign --deep
--strict` accepts the application, Gatekeeper reads it as Notarized Developer ID, and the
bundled `tpdf-cli` reports 26.10.2. Step 12 is the owner's and is not recorded here yet.
Next time, re-read the notes in `release.yml` before the release commit, so the tag can sit
on it.

**26.10.1 verification, macOS arm64, 2026-10-01:** all 29 gates passed on the final tree
(2,425 Rust tests with nine documented ignored, 1,999 frontend tests), and `check_windows.py`
type-checked the Windows tree. CI was green on both legs at the commit before the version
bump. Every mutation selected `--since v26.10.0` ran and was caught: 186 Rust and 11 frontend.
The one window mutation that selection names and the window phases were not rerun: no
frontend source changed since 26.10.0, where they passed. The normal bundle, with the
development engine hidden: `menu_check.py` (after its self-test) found a clean menu bar,
`save_check.py` saved over the document twice and refused a print after a second writer, and
the bundled `tpdf-cli` answered `--version` with 26.10.1 and `verify`. No release mechanics
were rehearsed under a throwaway tag: the one change to them is `installer-hooks.nsh`, and the
installer was built from it and run on Windows x64 the same day (*The command-line tool on the
Windows `PATH`*). On Windows, `print-probe`, `redact-reach-probe` and the window phases were
not run for this release.

**26.10.1 publication, 2026-10-01:** `Audit` and CI were green on the release commit before
the tag. The `Release` run for `v26.10.1` skipped its gates on that account. Its Windows job
failed once in the bundler, on a timeout downloading the WiX toolset (`failed to bundle
project: timeout: global`), after the application had compiled; the job was rerun alone and
passed, and the macOS job was not rebuilt. The draft then held 8 assets, counted with GraphQL,
and `scripts/publish_release.py v26.10.1 --publish` published it. Without authentication the
`.dmg`, the `.msi` and the `-setup.exe` answer 200 and `latest.json` offers 26.10.1 for
`darwin-aarch64` and `windows-x86_64`. The downloaded `.dmg` staples, `codesign --deep
--strict` accepts the application, Gatekeeper reads it as Notarized Developer ID, and the
bundled `tpdf-cli` runs under the hardened runtime and reports 26.10.1. Step 12 is the
owner's and is not recorded here yet; on Windows it is also the first installation through
the published installer's `PATH` hooks.

**26.10.0 verification, macOS arm64, 2026-10-01:** all 29 gates passed on the final tree
(2,415 Rust tests with nine documented ignored, 1,999 frontend tests), and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--since v26.9.24` ran and was caught:
856 Rust, 67 frontend and 14 window, plus the 8 of the three window-free runners. The window
phases passed on macOS against the checks build: viewer 318 text-heavy and 223 vector-heavy,
`tabs` 20, `forms` 18, `signatures` 21, `import` 28, `redact-pages` 34, `textedit` 23 (read
back by PDFKit with 898 changed pixels inside the edit and none outside), and the two phases
new this cycle, `textedit-w9` 24 and `textedit-centred` 24. The normal bundle contains no
harness; with the development engine hidden, `menu_check.py` (after its self-test) found a
clean menu bar, `save_check.py` saved over the document twice and refused a print after a
second writer, and the bundled `tpdf-cli` answered `--version` and `verify`. A local build is
ad-hoc signed, so the Developer ID check belongs to the published artifact. On Windows x64 the
same day, `sign --image` and `--hide` ran natively on the Windows development machine against a certificate-store
key (*`sign --image` and `sign --hide`*). `docs/THREAT-MODEL.md` gained the two commands that
watch the open file, which the cycle had added without it; `README.md` gained centred lines.
`release.yml` changed only in its notes, so no rehearsal tag was cut. **Not run before the
tag:** `print-probe` and `redact-reach-probe` on Windows (printing and OCR redaction are
unchanged this cycle), every Windows window phase, *Install command-line tool...* from the
bundle on either platform (the Windows path it shows changed this cycle and is covered by a
unit test only), and the disk-watch prompt in a window, which has frontend tests and no
window phase. CI's Windows gate leg is the Windows evidence.
Published 2026-10-01 from `5916616` with `scripts/publish_release.py`, after `audit.yml` and
`ci.yml` passed both legs on that commit; the release run (36885034528) skipped its gates
because CI had passed that commit, and its packaged CLI/API checks passed on both platforms.
The draft carried 8 assets. Fetched without an account, the `.dmg`, `.msi` and `-setup.exe`
answered 200 and `latest.json` offers 26.10.0 for `darwin-aarch64` and `windows-x86_64`. The
downloaded `.dmg` staples, its bundle verifies with `codesign --deep --strict`, `tpdf-cli` is
signed `Developer ID Application` with the hardened runtime and answers `--version` with
26.10.0, and Gatekeeper reports *Notarized Developer ID*. The hand-applied update from
26.9.24 (step 12) is the owner's and is not recorded here yet.

**26.9.24 verification, macOS arm64, 2026-09-30:** all 29 gates passed on the final tree
(2,392 Rust tests with nine documented ignored, 1,976 frontend tests), and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--since v26.9.23` ran and was caught:
71 frontend, 808 Rust and 11 window. The window phases passed on macOS against the checks
build: viewer 316 text-heavy and 221 vector-heavy, `tabs` 20, `forms` 18, `signatures` 21,
`import` 28, `redact-pages` 34 and `textedit` 23, the last read back by PDFKit with 2,395
changed pixels inside the edit and none outside. The normal bundle contains no harness; with
the development engine hidden, `menu_check.py` (after its self-test) found a clean menu bar,
`save_check.py` saved over the document twice and refused a print after a second writer, and
the bundled `tpdf-cli` answered `--version` and `verify`. A local build is ad-hoc signed, so the
Developer ID check belongs to the published artifact. On Windows x64 the same day, the new
installed-font lookup (Arial through DirectWrite, and a real CFF font, Miriam Libre), the
factsheet edits and the text-editor tests ran natively (see those sections). A new API check
edits a PowerPoint-shaped page through `tpdf-client` and was proved red with the ActualText
rewrite removed. `docs/THREAT-MODEL.md` already covered this cycle's additions; `README.md`
was corrected for installed fonts and PowerPoint accessible text. `release.yml` changed only in
its notes, so no rehearsal tag was cut. **Not run before the tag:** `print-probe` and
`redact-reach-probe` on Windows (printing and OCR redaction are unchanged this cycle) and the
Windows window phases other than text editing; CI's Windows gate leg is that evidence. The first
`Audit` run on the release commit was red for one denied warning, not an advisory:
`yoke-derive` 0.8.3 had been yanked. `cargo update -p yoke-derive` in both lockfiles moved it to
0.8.4 and changed no other entry, the notices file was regenerated for that one line, and the
tag went on the commit after the release commit.
Published 2026-09-30 from `8881ccf` with `scripts/publish_release.py`, after `audit.yml` and
`ci.yml` passed both legs on that commit; the release run (36751035458) skipped its gates
because CI had passed that commit, and its packaged CLI/API checks passed on both platforms.
The draft carried 8 assets. Fetched without an account, the `.dmg`, `.msi` and `-setup.exe`
answered 200 and `latest.json` offers 26.9.24 for `darwin-aarch64` and `windows-x86_64`. The
downloaded `.dmg` staples, its bundle verifies with `codesign --deep --strict`, `tpdf-cli` is
signed `Developer ID Application` with the hardened runtime, and Gatekeeper reports
*Notarized Developer ID*. The hand-applied update from 26.9.23 (step 12) was done by the owner
the same day.

**Post-release verification, 2026-09-29:** all 27 gates passed on macOS arm64
(2,328 Rust unit tests, nine documented ignored tests; 1,963 frontend tests; 253 CLI
checks). The run took 859.9 s across its gates. The CLI's parity section spent 191.2 s
in repeated CoreFoundation enumeration of the large Cargo cache; after moving only
the macOS test runner into a temporary directory, the full CLI passed 253/253 and
parity passed 13/13. The loader observation now uses per-process logs with deliberate
failure controls. The two-page, shared-field redaction case passes on macOS and Windows;
reintroducing the field-removal defect fails four of its five checks. Independent
pypdf reads reject the original and defective output and accept the corrected output.

On Windows 11 build 26200, the native trust suite passed 23 tests (the contained child
is ignored in ordinary discovery and explicitly run by its parent), and the CLI passed
240 checks with one macOS-only skip. Native all-target Clippy and formatting passed.
Real ROOT-store verdicts agree under low-integrity/job containment and outside it;
`docs/THREAT-MODEL.md` §T6.22 records the controls and account limitations. The installed
Windows signing workflow was measured later the same day, below. This verification
did not build or publish a release.

**Installed Windows signing, 2026-09-29, Windows 11 build 26200:** the official per-user
NSIS installer upgraded 26.9.21 to 26.9.22. External UI Automation drove the installed
application, with no application check harness. A temporary InteractiveToken task at
Limited run level launched it in the unlocked desktop session; the GUI itself measured
medium integrity (8192). A disposable one-day RSA-2048 certificate and non-exportable
CNG key were created under that same ordinary token in CurrentUser/My. No ROOT store
was changed and no existing signing identity was used.

The installed CLI and GUI both signed invisibly; the GUI also placed a visible signature
with a reason and location and signed with a DigiCert timestamp. Independent pyHanko
readback found each signature intact and valid, covering the whole file, and matched
its signer certificate byte for byte to the test certificate. The timestamp was
`INTACT:TRUSTED` with network fetching disabled. qpdf found no structural errors.
Poppler rendered both invisible outputs pixel-identically to the input; all changed
pixels in the visible output stayed within its widget. The installed viewer reopened
that visible copy correctly. App-only PrintWindow capture was proved independent of
an overlapping control window.

Chooser Cancel, native Save-dialog Cancel, and an unavailable timestamp authority left
no new output and preserved the source digest. A covered-byte alteration was rejected
by both the installed verifier and pyHanko. The self-signed signer correctly read as
untrusted at its root; installed `verify --strict` returned 1. Long-term signing correctly
refused the certificate's missing OCSP/CRL information, offered its explicit choices,
and wrote nothing when cancelled. No fallback was counted as a successful long-term
signing: that case still needs a CA-issued test certificate with revocation services.

The GUI mapped no PDFium; both observed renderer children measured low integrity (4096),
job membership, and PDFium loaded solely from the installed package. This run does not
establish smart-card/PIN behavior, a trusted CA-issued signer's workflow, or successful
long-term signing and archive timestamp creation from the installed Windows application.

The installed GUI's Properties readback and installed CLI both reported the DigiCert
timestamp authority trusted by Windows and the self-signed signer untrusted at its root.
Cleanup closed all 11 owned application/worker/WebView processes, restored the original
session and localStorage byte for byte, deleted the exact test certificate and its CNG
key, and removed the temporary task. Other certificates were unchanged. The successful
26.9.22 update remains installed.

The scrolled Properties screenshot exposed a display defect: translucent labels painted
above the sticky title. An unreleased fix gives that title an explicit stacking level
in `src/lib/propertiesdialog.ts`. A Chromium reproduction using the actual dialog showed
row labels covering the header before the change and only the heading at the sampled
header points afterwards; before/after renders confirmed the correction. The installed
26.9.22 binary used for the signing measurements does not contain this display fix.

**26.9.22 verification, macOS arm64, 2026-09-28:** all 27 gates passed on the final tree,
`check_windows.py` type-checked the Windows tree, and CI passed both legs on every pushed
commit of the cycle up to `b94cccf`. No npm update was available. `cargo update` offered only
`tokio-rustls` 0.26.5 to 0.26.6 and was **not taken**, because the attempted updates also
changed Windows dependency edges. **Diagnosis corrected 2026-09-29:** with the pinned Cargo
1.97.1, a targeted `cargo update -p tokio-rustls` reassigns 11 broad `windows-sys` ranges
from 0.61.2 to the already-locked 0.59.0 or 0.60.2. This matches
[Cargo issue #5529](https://github.com/rust-lang/cargo/issues/5529). A full
`cargo update --manifest-path src-tauri/Cargo.toml` restores every edge to its original
version and leaves only the TLS patch's version and checksum changed, reproduced online
and offline. The Rust minimum is not the cause: `windows-sys` 0.61.2 requires Rust 1.71.
For dependency refreshes, use the full update and inspect `git diff -- src-tauri/Cargo.lock`;
Cargo's summary (including `--dry-run`) does not report these edge changes between packages
already present in the lockfile. No extra pin or Rust-version change is needed.
`--since v26.9.21` selected 561 Rust mutations. Each increment ran its own
selection at its commit; before the tag, the groups a later commit touched were run again on
the fixed code: 190 (`longterm`, `lt`, `dss`, `trust`, `revocation`, `cli`) and 125 (`token`,
`integrity`, `chain`, `tsa`, `sign`, `sign visible`, `keystore`, `archive`, `cli json`), all
caught, and `docinfo` 63 of 63 after the archive timestamp reordered it. The other groups,
`save` among them, were not re-run after their own increment's run. The window's signing
phase ran with the owner at the machine, 46/46, and `tpdf-cli sign` signed against the real
keychain with a DigiCert timestamp, read intact by tpdf and pyHanko. A read-only audit of this
cycle's threat-model additions found two undisclosed exposures, both fixed before the tag
(`d446ffc`): long-term signing handed the token's certificates to the OS in the app process
with no count limit, and over `http://` a substituted authority's certificates chose the
addresses tpdf fetched. It also found /DSS limits applied after decoding, two exit-code and
read-back mismatches, and eight stale sentences, all corrected there. `release.yml` changed in
its notes and in the Windows qpdf download path, which CI had already run green, so no
rehearsal tag was cut.

**Not run before the tag:** the other window phases, `mutate_viewer.py`, every Windows window
phase, `print-probe` and `redact-reach-probe`, a timestamp or long-term request from Windows,
a signer certificate from a real CA, the external smoke test of the normal bundle (step 8) and
the hand-applied update (step 12). CI's `windows-2025` gate leg is the Windows evidence.

Published 2026-09-28 from `1978cad`, after `audit.yml` and `ci.yml` passed both legs on that
commit. The release run skipped its gates through `proven` and passed its other jobs, the draft
carried 8 assets under the tag, and its *New in this release* paragraph described 26.9.22. The
published release is Latest. Fetched without an account, the `.dmg`, `.msi`, `-setup.exe` and
`latest.json` answered 200, and `latest.json` offers 26.9.22 for `darwin-aarch64` and
`windows-x86_64`.

**26.9.21 verification, macOS arm64, 2026-09-27:** all 26 gates passed on the final tree and
`check_windows.py` type-checked the Windows tree. Tauri 2.12 and its plugins, wry 0.57 and
tao 0.37 were taken this cycle; the RustCrypto majors stay held because `cms` 0.3 and `rsa`
0.10 have no stable release. Every mutation selected `--since v26.9.20` ran: 372 frontend,
all caught, and 1,248 Rust in a separate worktree, 1,247 caught. The survivor, *save: two
spellings of one path are two files*, named a `same_file` branch the file-identity check had
made unreachable; the branch was removed and the mutation retargeted (`e7dd612`). The window
phases passed on macOS against the Tauri 2.12 checks build: viewer 316 text-heavy and 221
vector-heavy, `forms` 18, `signatures` 21, `import` 28, `redact-pages` 34 (with a new fourth
pass that must come back verified) and `tabs` 20. The first run found two expectations
behind the application --- *Close all tabs* in the tab menu since 26.9.18, and the
command-line tool pair needing no document --- and both were corrected. A read-only audit of
this cycle's threat-model additions found two code defects, fixed before the tag: the window's
invisible signing never applied the certificate rule, and `tpdf-cli` would search the working
directory for its engine when it could not find its own install. `release.yml` changed (the
Windows installer check and the notes), so it was rehearsed as `v26.9.21-rc1`: the new step
printed *both installers carry tpdf-cli.exe, and the MSI's copy runs: tpdf 26.9.21*, the draft
carried 8 assets, and draft and tag were deleted by id. The release commit's own CI run failed
first on Windows: `tauri-build` 2.7 left the fuzz targets linking no C runtime (see the trap
of that name), fixed in `e7dd612`.

**Not run before the tag:** the 57 window mutations `--since` selected, every Windows window
phase, `print-probe` and `redact-reach-probe`, the external smoke test of the normal bundle
(step 8) and the hand-applied update (step 12). CI's `windows-2025` gate leg and the release
build's installer check are the Windows evidence for this release.

Published 2026-09-28 from `7313840`, after `ci.yml` and the audit passed on that commit. The
release run skipped its gates through `proven` and passed its other jobs, and the draft carried
8 assets under the tag. Its *New in this release* paragraph still described 26.9.20 and was
corrected on the draft before publishing, and on `main` (`eaf075b`). The published release is
Latest. Fetched without an account, the `.dmg`, `.msi`, `-setup.exe` and `latest.json` answered
200, and `latest.json` offers 26.9.21 for `darwin-aarch64` and `windows-x86_64`.

**26.9.20 verification, macOS arm64, 2026-09-26:** all 26 gates passed on the code being
released (`392d525`), and CI passed both legs on it; after the version bump, `traps`, `dates`,
`docs`, `workflows` and `fixtures` passed, and `check_windows.py` type-checked the Windows
tree. No npm or Cargo update was available. Every Rust mutation selected `--since v26.9.19`
ran in a separate worktree, 679 of 1,781, and all were caught; no frontend source changed, so
that table selected none. The text-edit changes were measured on the 31-file sample and checked
by hand in *Replacing text set in a font that forbids editing* and *Columns on tagged pages,
the gutter, and the space before a pushed word*. `release.yml` changed only in its notes, so
no rehearsal tag was cut; the README's editing paragraph was corrected for columns on tagged
pages and for Noto in place of a restricted font.

**Not run before the tag:** everything that needs a window or the Windows development machine — the
window phases, `mutate_viewer.py`, Windows `print-probe` and `redact-reach-probe`, the external
smoke test of the normal bundle (step 8) and the hand-applied update (step 12). CI's
`windows-2025` gate leg is the Windows evidence for this release until those are run.

Published 2026-09-26 from `bfa20f1`, after `ci.yml` passed both legs on that commit. The
release run skipped its gates through `proven` and passed its other jobs, and the draft carried
8 assets under the tag. The published release is Latest. Fetched without an account, the
`.dmg`, `.msi`, `-setup.exe` and `latest.json` answered 200, and `latest.json` offers 26.9.20
for `darwin-aarch64` and `windows-x86_64`.

**26.9.19 verification, macOS arm64, 2026-09-26:** all 26 gates passed on the release tree
(1,939 Rust tests with three expected skips, 1,786 frontend) and `check_windows.py`
type-checked the Windows tree, after the Windows PDFium was fetched into `vendor/pdfium/bin/`.
Compatible npm and Cargo updates were applied first, Tauri 2.11.6 and the updater plugin 2.12
among them. Every mutation selected `--since v26.9.18` ran: 9 frontend, all caught, and 370
Rust in 20 minutes in a separate worktree, 369 caught. The one that survived, *cascade: the
edit's own box is no exemption*, had lost its test to this cycle's graze rule, which lets the
fixture's word through before the exemption is reached; a case with the word inside the
edited line's box by more than a graze was added (`7a303fc`) and catches it, on the release
tree. `release.yml` changed this cycle (the `proven` job that skips the release's gates when
CI passed them on the tagged commit), so it was rehearsed as `v26.9.19-rc1`: `proven`
passed, the gates were skipped, and the draft carried all 8 assets; draft and tag were deleted
by id. The release notes still described 26.9.18 and were corrected on the draft before
publishing, and on `main`.

**Not run before the tag:** everything that needs a window or the Windows development machine. The
release was cut unattended overnight: no window phases or `mutate_viewer.py` (it selected
none), no Windows window phases, `print-probe` or `redact-reach-probe`, no external smoke test
of the normal bundle (step 8) and no hand-applied update (step 12). CI's `windows-2025` gate
leg is the Windows evidence for this release until those are run.

Published 2026-09-26 from `7a303fc`, after `ci.yml` passed both legs on that commit. The
release run skipped its gates through `proven` and passed its other jobs, and the draft carried
8 assets under the tag. The published release is Latest. Fetched without an account, the
`.dmg`, `.msi`, `-setup.exe` and `latest.json` answered 200, and `latest.json` offers 26.9.19
for `darwin-aarch64` and `windows-x86_64`.

**26.9.18 verification, macOS arm64 and Windows x64, 2026-09-24:** all 26 gates passed on
macOS (1,928 Rust tests with three expected skips, 1,786 frontend) and on Windows,
and `check_windows.py` type-checked the Windows tree. Every mutation selected `--since
v26.9.17` ran and was caught: 56 frontend, 373 Rust and 13 window mutations. The window
phases passed on both platforms: viewer sweeps 315 text-heavy on both and 220 vector-heavy
on macOS, 217 on Windows; `textedit` 23/23, `textedit-grow` 26/26, `textedit-push` 6/6,
`import` 28/28 and `textedit-wrapped` 28/28. That last phase was the gap 26.9.17 recorded,
and on Windows it passes on the macOS LibreOffice export only (see *Naturally wrapped
paragraphs and end indents*). `textedit-push` first failed 5/6 on both platforms: the
untagged wrap made its 1,000-character draft a wrap refusal, *"this paragraph cannot wrap:
its lines would move onto what is below it"*, where the check expected *"edge of the
page"*. That expectation was stale and was corrected; the draft was still refused with no
preview. On Windows, `print-probe` passed 10/10. `redact-reach-probe` opened 156
documents: of 8,846 regions read back, zero still read as text, 3,835 were shown
unreadable and 5,011 not verified. Both installers built.

**Not run before the tag:** the external smoke test of the normal macOS bundle (step 8),
the Windows installer upgrade and hidden-engine checks, and step 12's hand-applied update
on either platform.

Published 2026-09-24 from `11fc587`, after `ci.yml` passed both legs on that commit. The
release run passed all five jobs, and the draft carried 8 assets under the tag. The published
release is Latest. Fetched without an account, the `.dmg`, `.msi`, `-setup.exe` and
`latest.json` answered 200, and `latest.json` offers 26.9.18 for `darwin-aarch64` and
`windows-x86_64`.

**The development Mac's tags were stale after the 2026-09-24 history rewrite, and any
clone not made after it will be too.** `git fetch` does not overwrite an existing tag, so
`v26.9.17` still named a commit with no merge base with `main`. Every `--since` run then stopped at *"git
could not diff against 'v26.9.17'"*. `git fetch origin --tags --force` once per clone fixes it.

**Historical binary provenance, checked 2026-09-29:** the 31 published tags from
`v26.8.0` through `v26.9.17` were rewritten, while their binaries retain their original
successful release builds. Full-tree comparisons against those build commits found exactly
one changed path per tag: `AGENTS.md`, which the application build does not package or read.
The application and build files are byte-identical. The five later releases through
`v26.9.22` still point at their build commits. [The provenance mapping](../release-provenance.json)
records each original build commit, workflow run, current tag commit and changed blob.
Affected release notes identify the original build. This is source comparison evidence,
not a claim that the binaries were rebuilt or reproduced; tags and binaries were preserved.

**26.9.17 verification, macOS arm64, 2026-09-23:** all 26 gates passed on the release tree
(1,886 Rust tests with three expected skips, 1,782 frontend) and `check_windows.py`
type-checked the Windows tree. Every mutation selected `--since v26.9.16` ran and was caught:
101 frontend in 50 s and 718 Rust in 36 min 54 s. The text-edit changes were checked on the
public sample through the worker, with round trips and independent readback, in *Wrapping onto
a new line of the paragraph* and *Which runs share a line*.

**Not run before the tag, and each is a gap rather than a pass.** The release was cut unattended
with the Mac's screen locked, so nothing needing a window ran: the 14 `mutate_viewer.py` window
mutations `--since` selected, every window phase — including `textedit-wrapped`, whose LibreOffice
fixture this Mac does not have and which is the one tagged phase fixture the wrap was not checked
against — the external smoke test of the normal bundle (step 8), and step 12's hand-applied
update, including the new *Restart to finish update*. Windows had none of its steps run either:
no window phases, `print-probe` or `redact-reach-probe`. The CI release run's gate job on
`windows-2025` is the only Windows evidence for this release until those are run.

Published 2026-09-24 from `703ce12`, after `ci.yml` passed both legs on that commit: the
release run passed all five jobs, the draft carried 8 assets under the tag, and the published
release is Latest. Unauthenticated fetches of the `.dmg`, the `.msi` and `latest.json` answered
200, and the public `latest.json` offers 26.9.17 for `darwin-aarch64` and `windows-x86_64`.

**26.9.16 verification, macOS arm64, 2026-09-20:** all 25 gates passed on macOS
and `check_windows.py` type-checked the Windows tree; the release run's own gate
job passed on `windows-2025`. Every mutation selected `--since v26.9.15` ran: 207
frontend and 570 Rust, the Rust set in 1,642 s. One Rust mutation did not compile
-- `edits: address a mark by its baseline page rather than its position`, whose
replacement rewrote a `match` that gained an arm when inserting pages added
`PageSource::Imported`. It was fixed and caught. That is the third stale mutation
in a week and all three were the same kind: the `anchors` gate asserts that a
search string is present, never that the mutated file compiles, so a mutation can
rot for months and read as covered.

Ten textedit and import window phases passed on macOS, the import phase at 28/28
after this cycle's three increments added ten checks to it. The redaction of an
own page beside an inserted one was checked end to end through the sandboxed
worker (`redact-import-probe`, 17/17) and read back with `qpdf --check` and pypdf.

**The Windows window phases, `print-probe` and `redact-reach-probe` were not run
before the tag: the Windows development machine was unreachable (asleep) for the whole release.** They
were run against the published tag the next morning, 2026-09-21, and all passed:
viewer sweeps 315 text-heavy and 217 vector-heavy checks, `textedit-push` 6/6,
the embedded textedit phase 23/23, the import phase **28/28** including every
inserted-page check this cycle added, `print-probe` 10/10 against the real
spooler, the OCR sweep with zero regions still reading as text, and both
installers built. So the published 26.9.16 has the same Windows evidence as the
releases before it -- after publication rather than before, which is the order
this record exists to state rather than smooth over.

**26.9.15 verification, macOS arm64 and Windows x64, 2026-09-20:** all 25 gates
passed on macOS and `check_windows.py` type-checked the Windows tree. Every
mutation selected `--since v26.9.14` ran: 38 frontend and 287 Rust, in 832 s. One
Rust mutation was caught by six tests and not by the one it named (`clip: omit
rectangle horizontal scale`), a stale expectation rather than lost coverage; it
now names the test whose subject it is. All ten textedit window phases passed on
macOS, including the new `textedit-push` at 6/6 and `textedit-grow` at 26/26. On
Windows the push phase passed 6/6, the embedded textedit phase 23/23, the import
phase 18/18, and the viewer sweeps 315 text-heavy and 217 vector-heavy checks;
`print-probe` passed 10/10 and the OCR sweep found zero regions still reading as
text. Both installers built. The installer upgrade and hidden-engine checks were
not run, and neither was the macOS updater.

**The measurements in this release were corrected twice before it was cut, both
times downwards.** `docs/PLAN.md` and `CHANGELOG.md` claimed 62/51/44 as-typed
acceptance at +10/+25/+50; those came from a tree whose push still refused some
runs their own unchanged text, and the fix for that costs a few points. The
finished code measures **58/48/41**, against 44/33/28 with the box growing alone
and 1/1/0 before either. A number in prose describes whichever tree produced it,
so the run that produces the release's numbers has to be the run on the code being
released.

**26.9.14 verification, macOS arm64 and Windows x64, 2026-09-19:** all 25 gates
passed on the release commit on macOS, and `check_windows.py` type-checked the
Windows tree. Every mutation selected `--since v26.9.13` was run: 4 frontend and
183 Rust, the Rust set in 537 s. 181 Rust mutations were caught at once; the
other two (`browser state: skip external state validation`, `image: skip image
validation`) did not compile, because both calls had gained a return value their
replacement text did not supply. They predate this release, and the `anchors`
gate cannot see it, since it checks that a search string exists rather than that
the mutated file compiles. Both were re-aimed and caught. On macOS, 13 textedit
window phases passed, including `textedit-overhang` at 24/24 after its stale
expectation was corrected (see *Keeping the source's own positioning in the
editor's box*). On Windows, the `textedit` phase passed 22/22 on
`testdata/textedit-embedded.pdf` (generated, not tracked; run
`testdata/make_textedit_embedded.py` first), the import phase 18/18, and the
viewer sweeps 315 text-heavy and 217 vector-heavy cases. The real-spooler probe
passed 10/10, and the OCR sweep opened 146 documents: zero regions still read as
text, 3,640 were shown unreadable and 4,670 could not be. The MSI and NSIS
installers built. The installer upgrade and hidden-engine checks were not run.
The published release carried 8 assets and the public `latest.json` offered
26.9.14 for both platforms.

**26.9.13 verification, macOS arm64 and Windows x64, 2026-09-19:** all 25 gates
passed on the release commit on macOS, and `check_windows.py` type-checked the
Windows tree. All 308 frontend mutations selected `--since v26.9.12` were caught.
The Rust selection was 303 at about 50 s each and was not run in full; the 51
Rust mutations added or re-aimed this cycle were caught when they were written.
The import window phase passed 18/18 on both platforms (dismissing the page
question, a range, every page, search, undo, redo and save). The first macOS run
of the earlier version of that phase found search filing an inserted page's hit
under slot 0, which also mis-filed hits after a page deletion or move in 26.9.12;
fixed before the release. The first Windows sweep found one missing entry in the
viewer harness's withheld-command list, a harness omission rather than a
product fault. Native checks passed 332 links and 315 text-heavy cases on macOS,
and 315 text-heavy and 217 vector-heavy cases on Windows. The Windows real-spooler
probe passed 10/10, and the OCR sweep opened 146 documents and asked about 12,608
regions: zero still read as text, 3,640 were shown unreadable and 4,670 could not
be; unverified is not a clean verdict. The Windows MSI and NSIS installers built
locally; the installer upgrade and hidden-engine checks were not run this time.
The published draft carried 8 assets, and the public `latest.json` offered 26.9.13
for both platforms. After publishing, the installed Windows app applied the update
by hand, and its uninstall entry then read 26.9.13. The macOS updater was not
exercised; the installed Mac app was 26.9.11.

**26.9.12 local verification, Windows x64, 2026-09-18:** all 25 gates passed on
the final code: 1,747 Rust tests with three expected skips and 1,695 frontend
tests. Mutations were selected as those added or re-aimed since the last commit,
96 of them, and all were caught on the final tree; the six README mutations
`--since v26.9.11` selected were caught too. A `--since` Rust run was stopped: it
selected 676 at about 50 s each here. The boxed-edit round trip then failed, a
regression from `0a8f1a8` (after the 26.9.11 checks) where the line breaker and
the ink check measured different strings; fixed with a test and a mutation, and
the round trip and every other check below were rerun on the fixed code.
Native checks passed 314 text-heavy and 216 vector-heavy cases, with 51 and 149
not applicable, plus 22 text-edit checks. The normal MSI rendered with the
development engine hidden and refused with both engines hidden, and its
PrintWindow capture was unchanged under an overlapping control. The released
26.9.11 NSIS installer upgraded to 26.9.12; the installation and registry were
restored. The packaged app applied and saved two edits in the Typst example, one
in its CID-keyed CFF title font and one in its TrueType body font without OS/2;
pypdf read both substitutions exactly and every font resource unchanged at `f32`
precision (a full save writes `353.51562` back as `353.51563`, as lopdf stores
reals as `f32`). The real-spooler probe passed 10/10. The OCR sweep opened 146
documents and read back 8,310 regions: zero still read as text, 3,640 were shown
unreadable and 4,670 were not, in 31.8 seconds without arithmetic warnings;
unverified is not a clean verdict. After publishing, the installed 26.9.11
offered *Update to 26.9.12*, installed it, and the relaunched application
reported *tpdf 26.9.12 is the latest version*; the installed executable matched
the published MSI's after installer-marker normalization, and the normal
session file was restored byte for byte.

**26.9.11 local verification, Windows x64, 2026-09-17:** the final run passed
all 25 gates in 157.4 seconds: 1,642 Rust tests passed with three expected skips,
and 1,695 frontend tests passed. Selected mutations caught 28 distinct Rust,
six frontend and one native keyboard fault; historical tables were not rerun.
The list-frontier regression was corrected after one mutation exposed that it
did not isolate pending siblings. Native checks passed 314 text-heavy and 216
vector-heavy cases, with 51 and 149 not applicable, plus 22 text-edit checks.
The normal MSI rendered with the development engine hidden and refused with
both engines hidden. Its PrintWindow capture was unchanged under an overlapping
control. The released 26.9.10 NSIS installer upgraded to 26.9.11; the original
installation and registry exports were restored. Packaged error copying,
preview, Apply and Save passed on a disposable compatibility input, including
signature confirmation and independent text, resource and structure readback.
The final rebuilt worker round trip matched preview/save pixels and preserved
adjacent content. The real-spooler probe passed 10/10. The OCR sweep opened 144
documents, sampled 12,368 regions and read back 8,155: zero still read as text
and 3,640 were shown unreadable, in 28.2 seconds without arithmetic warnings.
The remaining regions were unverified; that is not a clean verdict.

**26.9.10 local verification, Windows x64, 2026-09-17:** all 25 quality gates
passed in 335.4 seconds after the compatible dependency updates. Rust passed
1,627 tests with three expected skips; the frontend passed 1,692. Selected
mutations caught 29/29 Rust and 9/9 frontend faults covering CJK subsets, Type 3
glyphs, partial-page editing and layouts. The native mutation file selection
contained no changed targets; the complete historical tables were not rerun.
Native viewer checks passed 314 text-heavy and 216 vector-heavy cases, with
51 and 149 not applicable, plus 22 text-edit workflow checks. The normal MSI
rendered with the development engine hidden and refused with both engines
hidden. Its PrintWindow capture stayed unchanged under an overlapping control.
The released 26.9.9 NSIS installer upgraded to 26.9.10; the original installation
and registry exports were restored. Independent PDFKit readback verified CJK
Unicode and unchanged neighbouring geometry and pixels.
The normal app passed automatic CJK preview, apply, undo/redo and save, followed
by independent Unicode readback. Automation must wait for Save to become enabled
after Redo: sending Ctrl+S while it is still disabled performs no save. A control
with the same command sequence and that readiness wait passed. Independent
round trips passed two CJK, six Type 3 and seven partial-page cases; font outlines,
resources, logical text and neighbouring pixels were checked as applicable.
The real-spooler probe passed 10/10. The Windows OCR sweep opened 142 documents
and sampled 12,128 regions. Of 7,926 regions read back, zero still read as text,
3,492 were shown unreadable and 4,434 remained unverified, in 38.1 seconds,
without arithmetic warnings. Unverified is not a clean verdict.

**26.9.9 local verification, Windows x64, 2026-09-16:** all 25 quality gates
passed after refreshing the fuzz workspace lockfile. The full run took 258.9
seconds; its corrected fuzz gate took 41.6 seconds. Rust passed 1,603 tests
with three ignored; the frontend passed 1,691. Selected mutations caught
12/12 Rust, 13/13 frontend and 1/1 native UI faults; historical tables were
not run in full. Native viewer checks passed 314 text-heavy and 216 vector-heavy
cases, with 51 and 149 not applicable, plus 22 text-edit workflow checks.
The normal MSI app passed font fallback, box sizing, wrapping, live preview,
undo/redo and save, with independent parser readback. PDFKit separately checked
new glyphs, wrapping and unchanged neighbouring pixels. The worker round trip
matched preview/save pixels on both pages and preserved adjacent content.
The packaged engine worked with the development engine hidden; hiding both
produced an engine error. PrintWindow captured only the application and its
pixels stayed unchanged under an overlapping control window. The released
26.9.8 NSIS installer upgraded to 26.9.9; the original installation and registry
were restored. The real-spooler probe passed 10/10. The OCR sweep opened 141
documents and sampled 12,008 regions, reading back 7,820 on 267 pages: zero
still read as text, 3,492 were shown unreadable and 4,328 remained unverified.
It took 46.7 seconds with no arithmetic warnings. Unverified is not clean.

**26.9.5 local verification, Windows x64, 2026-09-11:** all 23 gates passed
in 309 seconds (1,319 Rust tests passed, two ignored; 1,631 frontend tests).
The selected mutations caught 14/14 Rust, 251/251 frontend and 2/2 native UI
faults. These cover forms, signatures, movement and changed frontend files;
the historical mutation tables were not run in full. The extracted MSI passed
313 text-heavy and 215 vector-heavy viewer checks, with 51 and 149 not applicable,
plus 34 form and 20 signature checks. The development PDFium was hidden; hiding
the bundled engine too failed as expected. The print probe passed 10/10.
The real-document OCR sweep opened 133 PDFs, sampled 11,728 regions and read back
7,556 on 260 pages: zero still read as text, 3,337 were reported unreadable,
and no arithmetic warnings appeared (27.5 seconds).
The released 26.9.4 NSIS setup upgraded to 26.9.5 in a disposable installation;
the prior installed release and all three registry exports were restored.
