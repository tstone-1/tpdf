# The viewer on Windows

How the viewer came to run contained on Windows, and the measurements made there. Moved here verbatim from `BUILD.md` on 2026-10-23, which indexes this file.

### Windows runs the viewer, and how it came to be contained

**Read this section as a timeline, not as a status.** It opens with the state before
2026-07-29 — uncontained, failing open — because the controls taken then are what make the
later evidence mean anything. The present state is at *Windows no longer fails open* below:
workers are selected there, proved from outside the process. `AGENTS.md` carried the
pre-flip wording in its own gates section for a day after the flip, in flat contradiction of
its own constraints section, which is the hazard this note exists to prevent here.

`scripts/gates.py` reported **8/8 on `x86_64-pc-windows-msvc`** on 2026-07-29 — a dated
count, and there are twelve gates now, so ask `--list` rather than this line — and the same
day a Windows build **opened documents and passed the full functional check**. A clean clone bootstraps
with no changes — `npm install` and `scripts/fetch_pdfium.py` both do the right thing, the
fetch script selects the `win-x64` asset and verifies its digest.

`viewer_check.py` runs unmodified: `webview_guard` already returns early off darwin, and
WebView2 needs no bundle identity, so a plain `target/release/tpdf.exe` is enough where macOS
needs an `.app`. Two things about the invocation, both of which present as something other
than what they are. The binary must come from `cargo build --release --features
tauri/custom-protocol` or the window shows *"localhost refused to connect"* (see the trap —
the profile is not what embeds the frontend). And **pass it as a backslash path**:
`CreateProcess` does not accept a relative forward-slash path, so
`src-tauri/target/release/tpdf.exe` raises `FileNotFoundError: [WinError 2] The system cannot
find the file specified` for a file that is plainly there, from inside Python's `subprocess`
rather than from anything in this repository.

Four corpora, every one reporting the **86 check names** that were the invariant then, with
splits inside the ranges the table above records. Word and line selection took that to **89**
on 2026-07-30, after this run; the splits below are left as measured rather than adjusted by
arithmetic. A Windows re-run should expect **109** names — 23 added since, and the macOS
table further down says which of them skip on which document:

| fixture | ran | skipped | failed |
|---|---|---|---|
| `outline-simple.pdf` | 81--82 | 4--5 | 0 |
| `outline-hostile.pdf` | 197 | 37 | 0 |
| `rotated-90.pdf` | 184 | 50 | 0 |
| `vector-heavy.pdf` | 104 | 130 | 0 |

Re-run 2026-07-30 with pre-spawning live, since that changes the app's own behaviour — every
open now consumes a warmed process and starts another. All four green, no `[WARN]`, 44 modules
at peak with no `pdfium` among them over 27--978 samples. `outline-simple` reported 82/4 that
time against 81/5 before: the **name set** is what is invariant, not the split, and one of
them stopped skipping. A split that moves is information; a name that disappears would not be.

#### The 109-name re-run, measured

Done 2026-07-30 after the reading-order work landed, on the **six** corpora this machine can
generate — `text-heavy.pdf` is a real document rather than a generated fixture and has never
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
withdrawal checks beside it skipped — which looks like two findings and is one, since nothing
navigated, so no new thumbnail was ever requested to be in flight.

It led to a real defect in two classes. The page strip and the outline tree both activated
`focused` — a **mirror** of the DOM's focus kept by a `focusin` listener — rather than the
row the key event reached, and a mirror that misses an update sends the reader to whatever it
still names, which is page 1 because it starts at 0. `focusin` is not guaranteed: a document
without system focus moves `activeElement` without delivering focus events. Both now take the
row from `event.target` and keep the mirror only as the fallback for a key that arrived on the
container. Each has a unit test that was shown to go red first, plus a control on the
fallback; `sidebar.ts` had no unit tests before this.

**The intermittent itself was never caught a second time** — five further corpus runs,
including a replay of the back-to-back loop it came from and one under deliberate concurrent
CPU load, are all green — so this is an identification by mechanism and symptom, not by
re-observation. Contention was the first guess and was wrong when tested. The check now prints
`activeElement`, whether the strip followed, and `document.hasFocus()`, so a recurrence
settles it. See the trap *A mirror of the DOM's focus goes stale*.

Rendering, scrolling, zoom, pinch, view rotation, text selection, search, the palette, the
accessibility tree, the outline sidebar, thumbnails, inversion and the print command's
refusals all behave as they do on macOS.

**What was missing then was containment, not function** (superseded 2026-07-29 — see below).
`sandbox_init` is SBPL and macOS-only, so `Worker::spawn` refused off macOS and
`Backend::default_here()` fell back to `Backend::InProcess`. A Windows build parsed
attacker-controlled PDF **in the app process**, which is exactly what `AGENTS.md` and
`docs/THREAT-MODEL.md` forbid. **It failed open**:
`Worker::spawn`'s refusal is asserted by tests, but only a caller that asks for
`TPDF_BACKEND=worker` ever reaches it — the default selects in-process and renders perfectly
happily, so nothing refuses. A port owes a real containment answer (job objects, a restricted
token, a separate desktop) before Windows can ship. That, and not the viewer, is now the whole
gap.

It is at least **visible**: the uncontained default records `render::UNSANDBOXED_MARK` on the
startup timeline and prints `[WARN] no sandbox on this platform ...` on stderr, and
`viewer_check.py` echoes `[WARN]` lines even on a passing run — it previously showed stderr
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
profile and `OpenProcess` on the parent. It does not deny *reads* — an integrity level
governs writes — so the child is handed its document and its output as inherited handles
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
worker — on macOS, 362.7 MB before the request and 1029.8 MB after, so the append itself adds
667 MB on that document.

**Measured on Windows 2026-08-22, and the margin is 4.3% rather than the 35% that was
reasoned.** The first measurement was taken from outside the process, because `[INFO]` could
not print here at all: it was guarded on `Worker::footprint`, which is `phys_footprint`, which
is `None` off macOS — so a Windows run was told to read a line the build could not emit. That
is fixed, and the fix is the point rather than the convenience. The quantity a job object caps
is **commit**, and `Contained::peak_commit` reads it through the handle the parent already
holds, so `Worker::peak_commit` is to Windows what `footprint` is to macOS and the probe prints
whichever the platform has, named. The footprint check is no longer a `[SKIP]` here: it reads
*"the parent can read what bounds the worker's memory"* and passes on both, so the probe now
reports **17/17 with none not applicable** on either platform.

**Two checks were added on 2026-08-24, so the number is now 19** — measured **19/19 with none
not applicable on Windows**; the macOS figure was 17/17 before they existed and has not been
re-run. They cover a worker that cannot load PDFium at all: it must **answer** the request with
a reason, rather than exiting 1 the way the shipped 26.8.8 did, and the reason must name the
engine rather than being the parent's epitaph for a dead child. The fixture is a directory with
no PDFium in it, so it needs nothing generated.

**Four more on 2026-08-26, so the number is now 23** — measured **23/23 with none not
applicable on macOS**; the Windows figure was 19/19 before they existed and has not been
re-run. They put a worker on the save's *verification* side, which nothing exercised until
then: `save::InWorker` was reachable only from the command bodies (in `lib.rs` at the time,
now `commands/mod.rs`), so every test and every other probe passed `save::Here` and the
shipped verifier was proved by compiling.

What they assert is a differential — the worker and the coordinator asked the identical
question about identical bytes — plus the two things a differential cannot say on its own.
That the worker's refusal is **`lopdf`'s and not PDFium's at document-open**: the fixture is a
real document with a trailer pointing at offset 999999999, which PDFium reconstructs and opens
happily while `lopdf` names the cross-reference table, so the two messages differ and the
assertion pins the wording. And that a **worker was involved at all**, which no comparison of
answers can establish, since an `InWorker` delegating to `Here` would agree everywhere; that
one points the verifier at a directory with no PDFium in it, where `Here` still answers and
`InWorker` cannot start a child.

Both of those exist because the first draft got it wrong in the reassuring direction — it
planted a file that was not a PDF, the worker refused it at open, and the check reported `[OK]`
having never run `lopdf`. `docs/TRAPS.md` has both entries.

**Five more on 2026-08-28, so the number is now 28** — measured **28/28 with none not
applicable on macOS**; the Windows figure was 19/19 before any of the last nine existed and has
not been re-run. They put a worker on the save's *writing* side, which is the half
`docs/THREAT-MODEL.md` residual risk 18 was still disclosing: the same four shapes as the
verification checks above, plus one that only this path can make.

The differential here is **byte for byte** rather than a number, and it is affordable because a
rewrite of one document under one plan is deterministic — every date in the output comes from
the plan's own marks and not from the clock. On `testdata/comments.pdf` under a plan that turns
every page, `save::Here` and `save::InWorker` both write 222,667 identical bytes. A comparison
of lengths or page counts would have passed for a worker that dropped the turns.

The extra check is the one that says the **output channel** is real: an ordinary worker,
spawned with no output file, must refuse the rewrite in words rather than writing a document
into whichever descriptor happens to be open at that number. Every other check in the section
would pass just as well if the descriptor were handed over unconditionally and
`worker::OUT_ARGV` did nothing.

**What the move costs is printed beside them.** On `comments.pdf` (4 pages, 238 KB) the
rewrite is **2.4 ms in this process and 11.4 ms in a worker, +9.0 ms** — best of five
interleaved, minima rather than means, because the question is what the work costs and not
what the machine was doing while it ran. That delta is one process spawn plus PDFium's
initialisation, so it is **fixed rather than proportional**: on a document where the parse and
the serialisation are hundreds of milliseconds it is noise, and this fixture is close to the
worst case for it. Nothing else on the save path got slower — the parse moved, it did not
happen twice.

**Six more on 2026-09-01, then six more the same day, then two, then three, so the number is
45.** The last three are the redaction read-back — the differential, the needs-a-worker
control, and the one that says the compared reports are about a document that was actually
read: the report must name a needle every PDF contains, not name one no document contains, and
have reached objects. Without that last control "the two agree" is satisfied by two reports
that looked at nothing, which is exactly what a worker answering from an unparsed document
produces.

The earlier ones. The second six are the page-range print and the merge — three checks each: the differential,
the needs-a-worker control, and the scratch or page-count reading. The last two are
`xref-bomb.pdf`, whose `/W` widths aborted lopdf 0.44. With 0.45 it is refused normally;
the check accepts that parser refusal or a contained worker death and requires no output.
There is deliberately **no coordinator arm**, preserving containment if this regresses. Three of them put a
worker on **Save a copy** and three on the **print job**, which are the last two writing paths
`docs/THREAT-MODEL.md` residual risk 18 was disclosing.

The copy's three are the shapes above with one difference: they go through `save::write_copy`
rather than through `save::InWorker` directly, so they cover the staging, the length check and
the rename as well as the channel — a copy's destination is a name the reader chose in a
dialog, which the in-place path never has. The differential is byte for byte again.

The print job's three are the same shapes for the one path whose answer comes **back**:
`NSPrintOperation` and `Windows.Data.Pdf` take bytes rather than a pathname, so the worker
writes into a scratch file this process created and this process reads it. The third check is
the one only this path needs — that no `tpdf-print-job.*` file is left in the temporary
directory, on the refusal as well as on the answer, because what it holds for the length of a
print is the reader's document with its encryption off and its pages in the order they asked
for.

**What those two moves cost is printed beside them**, measured the same way as the rewrite's
and reported as minima of five interleaved runs. On `text-base14.pdf`, over three consecutive
runs: the copy is **4.0 ms here and 11.9 ms in a worker, +8.0**, and the print job **0.2 ms
here and 7.2 ms in a worker, +7.0**. The rewrite on the same runs reads **+7.1**, and
`text-wide.pdf` gives +7.2 for the copy, +7.0 for the print job and +6.9 for the rewrite. All
three are one number: a process spawn plus PDFium's initialisation, **fixed rather than
proportional** to the document.

⚠ **The copy's in-process baseline is 4.0 ms where the rewrite's and the print job's are 0.2,
and that is the check rather than the copy being slower.** A copy is written through
`save::stage`, which ends in `sync_data` — the bytes have to be on the platter before the
rename swaps them in. A print job is read straight back out of the handle that wrote it and
never has to reach the platter, and `worker-probe` measures the rewrite through `Rewriter`
directly rather than through `stage_in_place`. Comparing the three *deltas* is comparing the
same mechanism; comparing their baselines is comparing three different amounts of disk.

⚠ **The first readings taken for this paragraph were 1.5 to 2 times these, and the machine
was the variable.** They were measured under a load average of 8 to 12 with another job
saturating the cores, which is exactly what interleaving does *not* correct for: it controls
for drift between the two arms, not for a machine that is slow for both. Take a reading only
when the box is quiet, and say so when it was not.

⚠ **`26.9.0` shipped without a Windows run, and this paragraph is why.** The rewrite's output
channel there is a `DuplicateHandle` into the child rather than a `dup2` before `exec`, so the
five checks above were macOS evidence for a mechanism that has two implementations, and nothing
had exercised the Windows one — last measured **19/19 on 2026-08-24**, before these five, the
four verification-side checks and the six writing-path ones above existed. `AGENTS.md` records what a single sentence about
two platforms costs.

**The first Windows run was 2026-09-01, and it is the CI step below rather than a run somebody
made**: `34/34 checks passed, 0 not applicable to this platform` on `windows-2025`, run
33501693368. Nothing was skipped, so the copy, the split, the print job and the output channel
are each watched working there. **34 was the count at that run**; the six checks added later
the same day for the page-range print and the merge have not been through a Windows leg yet,
and the next push is what takes them there.

**The requirement was real and the placement was the defect.** This said *"Run it on Windows
before the next release"* until 2026-09-01, and a release then went out without it. An
imperative in a reference section is read by whoever is already in that section; a release
gate is read by whoever is cutting a release, and those are different readings of the same
file. That is the checklist-weaker-than-its-gate trap, found by an external review rather
than by anything here.

**So it is neither, now: both workflows run this probe on both legs, on every push**
(`.github/workflows/ci.yml` and the `gates` job of `release.yml`, which the `workflows` gate
holds equal). It costs under a second, needs no screen, and runs against
`testdata/text-wide.pdf`, since `scripts/ci_fixtures.py` cannot produce the `text-base14.pdf`
above — the macOS reading is the same against both fixtures (42/42 as of 2026-09-01, and
28/28 when it was 28), and was taken in **both profiles** (0.36 s release, and the debug one CI
actually builds), so neither the fixture nor the profile is load-bearing. The debt is paid continuously rather than once, and the reading below
stays as the last thing measured by hand.

Reverting `worker_child`'s bind arm to `bind(&library_dir)?` turns both red with
`worker stopped answering (exited with 1 (0x00000001))` — which is the string the reader who
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
is correct rather than noise — it goes quiet when the append stops carrying a discarded copy
of the previous revision, and not before.

**The probe appends a document the application would not.** `save::APPEND_MAX_BYTES` bounds the
production path at 256 MiB, so a 336.6 MB scan is reserialised rather than appended when a
reader saves it. `worker-probe` asks the worker for the append directly and therefore still
measures it, which is deliberate: the bound is a judgement placed under a measured ceiling, and
it can only stay under it if something keeps measuring where the ceiling is. A run whose `[WARN]`
disappears is the signal that the bound can rise.

The sweep that bracketed the ceiling was read from outside the process the first time, through
PSAPI's `PagefileUsage` / `PeakPagefileUsage` over the probe's children. On the Windows development machine
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
really is file-backed and not commit — peak working set runs ~343 MB above peak commit on the
40-page scan, which is the document. But macOS `phys_footprint` excludes clean file-backed pages
too, so the mapping is absent from the 1029.8 as well, and the 362.7 MB baseline taken for it is
PDFium's own allocation, which is private commit here. The two metrics measure the same thing:
**980.3 MiB = 1027.9 MB against the macOS 1029.8 MB, 0.2% apart.** The last two rows are their
own control on the reading: commit stops at 1020 MiB and the allocator then fails, so the number
being read is the number the kernel is enforcing.

So `incr-scan-40p.pdf` — the largest fixture in the repository — sits **4.3% under the cap**,
and the ceiling is bracketed rather than extrapolated: 345.0 MB saves, 361.9 MB does not. Above
roughly **350 MB an append cannot be built on Windows.** The failure is the safe direction and
worth stating exactly: the allocation fails, the worker aborts with `0xC0000409`, and the append
is prepared *before* `save_document` closes the document — so it is a `refused`, nothing is
written, and the reader keeps their edits. What they are told is `worker stopped answering
(exited with 3221226505 (0xC0000409))`, which names neither the size nor the cap.

The asymmetry that makes this odd from a reader's chair: only the **append** runs in the worker.
`save::Mode::Rewrite` goes through `spawn_blocking` in the app process, which is under no job
object — so on a 400 MB scan, highlighting a line cannot be saved while highlighting a line
*and deleting a page* takes the uncapped path. `docs/PLAN.md` §3 carries the ranking this
measurement now speaks to.

**11/11 checks, 1 not applicable**, on `text-base14`, `text-cid`, `vector-heavy` and `rotated`
— tiles **pixel-identical** to the in-process render, plus text extraction, outlines and
search across the boundary. That is what the run measured on 2026-07-29 and is left as it was
read: the probe gained three checks on 2026-08-22 — a save's update section built across the
boundary, re-parsed after being appended, and compared against the length it was built for —
so a current Windows run reports **14 of 14 with one not applicable**, and nobody has taken one.
A count in prose is a dated statement about a dated run; the probe's own output is the
authority, and macOS measured 17/17 that day.

**There is no not-applicable one any more, as of 2026-08-22.** It was the parent's memory poll,
skipped here on the grounds that the job object caps commit in the kernel so there is nothing to
poll — true, and the wrong conclusion: a kernel bound makes the reading matter *more*, because
what a reader needs is how close the worker came to being refused. What was missing was a way to
look, not a reason. `Contained::peak_commit` is it, and both platforms now report **17/17 with
none not applicable**.

Two things that check does *not* cover, deliberately, because a `cargo test` child is the test
harness and never answers: pipe **direction** and content. Both are the probe's job, measured
by mutating the pipe pair and watching the probe go red — see the trap *A test whose child
never answers cannot see the pipes being crossed*.

**Windows no longer fails open** (2026-07-29). `Backend::default_here()` selects workers there,
and the evidence is external rather than a mark of our own:

```
python scripts/win_modules.py <pid>          # on its own
python scripts/viewer_check.py <exe> <pdf>   # samples it throughout a real run
```

`viewer_check.py` now launches the app rather than blocking on it, reads the loaded module list
from outside the process while a document is open, and takes the **union** of its samples —
the parser is mapped only while a document is open, so a single look could miss it in either
direction. The module count is printed beside the verdict, because an enumeration that read
*nothing* reports "not mapped" exactly as containment does; a peak of zero is reported as a
broken observation, never as a pass.

Run **before** the flip it reported `[FAIL] the app process mapped the PDF parser, 47 modules
at peak`. That control is why the pass afterwards means anything. After: four corpora green
with unchanged ran/skipped splits, no `[WARN]`, 44--45 modules at peak, no `pdfium` among them.

That line is printed *outside* the check names on purpose — those are `viewercheck.ts`'s
and are the cross-platform invariant, and adding a Windows-only name to that set would make the
two platforms look divergent when they are not.

**Outside** means on **stderr**, and the passing direction of it went to stdout until
2026-08-02. `mutate_viewer.py` reads check results from stdout alone for exactly this reason
and its own docstring says so, so on Windows every baseline silently carried a 
"check name" that no mutation could turn red — and a mutation whose expected name happened
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
one does. `Handover` is deliberately not a `Request` variant — a handover is legal exactly once,
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
— `text-base14`, which embeds nothing, costs 10.38 ms against 8.87/8.99 ms for the two that do.
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

**The name total is 43 as of 2026-08-16** — *"comments return the same list on both"* was
added with the comment layer — and the four rows above are the Windows measurement at 42,
left as they were taken. macOS re-measured the same four the day the check landed and reports
each row's passed column one higher against the new total: `39/43`, `39/43`, `40/43`, `41/43`,
with the skip counts unchanged. Windows has not been re-run.

That check compares what the two backends *return*, not whether either is right: a defect in
`annots.rs` breaks both identically and it stays green. `comments-probe` is what says the
answer is correct; this says the worker boundary does not change it. Proved to bite before
being trusted — truncating the worker's reply to three comments turns it red, and restoring
it turns it green, on the same fixture in the same minute.

Re-measured 2026-07-31. **The earlier `41`s were not a missing check**, which is what they
looked like: this table read `37/41 ... 40/41` against macOS's 42, and a handover went out
asking which check was macOS-only and proposing that the flat "all 42 names appear" sentence
above become a per-platform one. Nothing is macOS-only. The 41s were taken at `df1ca61`, and
`9fb728f` — the very next commit to touch this file — added *"a search option crosses the
worker boundary"*. Windows has had all 42 ever since; the name sets are byte-identical across
all four corpora here, diffed rather than counted.

Worth keeping as the shape of the error rather than only its answer: **a count taken at one
commit and compared against a count taken at another is not a platform difference**, however
neatly the two platforms line up on either side of it. The cheap discriminator is the one that
settled it in a single command — grep the *name* out of the source at each commit, rather than
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
the pool — deliberately not `Contained::kill`, since the pool has to notice a death it did not
cause.

This is also where the Windows spare is proved end to end, and the detail says more than the
count: `at open: pool [18840], children [2672, 18840], spares [2672]` — a warmed child exists,
is excluded from the pool rather than miscounted into it, and `opened with 1` beside it keeps the
laziness claim. `a spare does not outlive the service that started it` reports
`its 1 spare process(es) [58096] went with it`.

**It first reported 34/41, and the two failures were the probe's own.** They said a burst grew
the pool to six and 1.2 s into a 4.0 s idle timeout one was left, with **144 handles with one
worker, 144 grown, 144 retired** beside it — and five extra workers cannot cost zero handles.
Two independent observations agreeing, and the diagnosis drawn from them (created, used and
**destroyed rather than pooled**) was recorded here as an open defect for a day. It was wrong.

Both numbers were honest; neither could say *when* it was taken. `settled_descriptors` waits up
to five seconds for a pre-spawned spare to appear, Windows has none, and the verdict of that wait
was discarded — so it spent its whole bound on every call, which is longer than the idle timeout
the phase runs at. The instrument retired the pool and then measured it. One worker of six and a
lean handle count are precisely what a correct pool looks like five seconds after a burst. The
pid clause is now asked for only where a spare can exist, and a wait that expires says so with a
`[WARN]`. Nothing in `workers.rs` changed.

Do not "fix" a failure here by relaxing a check — but do check the clock before believing one.
The pre-fix run remains the red control for both: they were observed failing, are now observed
passing, and `an idle pool is retired down to one worker` is green on both sides, so retirement
was never the thing that broke.

**A second check went red the day pre-spawning landed, and it was the same shape.** `closing
gives back every descriptor opening took` reported *137 quiet, 145 with it open, 142 after
closing it* — five handles, one spare's worth. Nothing leaked: an `open` consumes the warmed
spare and starts a replacement on another thread, so a raw sample includes one spare or not
depending on how far that thread has got. macOS forks and wins that race; Windows creates a
process, a token, a job and a fresh map of `pdfium.dll`, and does not. Its three samples now go
through `settled_descriptors`, which exists for exactly this and predated them. See the trap —
the lesson is that passing on one platform was evidence about that platform's timing.

**Of the "four probe binaries that refuse to act as a worker off unix", one did.** That list was
in this file and in `AGENTS.md` for two days and was wrong about two of its four entries, in the
direction a list written by reading always errs — see the trap. What was actually true:

- `pool-bench`, `prespawn-bench` — a real `#[cfg(unix)]` gate on the `--render-worker` re-exec,
  dating from before `worker_child` compiled on Windows. Worth understanding before copying it:
  each binary re-execs *itself* as a worker, so gating that made the benchmark **unrunnable**
  rather than degraded. Ported 2026-07-30, along with the hardcoded library path.
- `tile-bench` — **never refused anything.** It ran on the first try and failed at
  `LoadLibraryExW` on the hardcoded path. Ported the same day; numbers below.
- `worker-bench` — seven of its eight modes genuinely refuse, and the reason is accurate: it
  carries its own POSIX worker implementation, fd passing and SBPL profile bisection included,
  and shares no mechanism with the job-object model. Those need a spike, not a port, and the
  refusal now says what such a spike would measure that nothing else does (the per-tile overhead
  decomposition of `latency` mode — parallel scaling is `pool-bench`, the authority rungs are
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

  `--mode engine` spawns nothing — it reads the library file — and was unreachable off unix only
  because it sat inside a `#[cfg(unix)]` module. It is at file scope now, and on Windows it
  reports **`[NOT VERIFIED]`**: the shipped `pdfium.dll` carries no local C++ symbols
  (`CPDF_Document` is absent), so `v8::` and `CXFA_` being absent from it means nothing. That is
  the harness's second control working exactly as written — and it means
  `docs/THREAT-MODEL.md`'s promotion of "JavaScript is disabled" to "there is no engine to
  disable" is established on **macOS only**. On Windows it rests on the asset name and pinned
  digest `fetch_pdfium.py` asserts, which is a claim about which file was fetched rather than
  about what is in it. The threat model now says so.

  It also prints the one dimension that survives stripping, because exports are always named:
  **460 exported functions, four of them XFA-named** — `FPDF_LoadXFA` and
  `FPDF_GetXFAPacket{Count,Name,Content}`. Surface, not a contradiction: the three
  `GetXFAPacket*` calls read `/XFA` streams out of an AcroForm dictionary and need no XFA engine.
  Whether `FPDF_LoadXFA` is a stub there is open, and unlike JavaScript it is behaviourally
  decidable — a fixture carrying an `/XFA` packet makes `FPDF_GetXFAPacketCount > 0` a positive
  control, so `FPDF_LoadXFA` returning false on it would mean the implementation is absent rather
  than the document empty. Not written; that fixture does not exist.

  Both numbers were cross-checked against a throwaway Python PE parse before being written down
  — two independent parsers, same 460 and same four names. Every branch was exercised: a
  non-PDFium file `[FAIL]`s, a file that passes both controls but is not a PE reports "not a PE
  image" rather than a zero, a missing `--lib` exits 2, and another mode still refuses.

**Numbers are macOS arm64 unless a Windows one says so.** The pre-spawn table above and the
tile-bench section below are the sets taken on Windows and are labelled as such; everything else
in this file and in `AGENTS.md` still is not, and the platforms are far enough apart — a ~1.4 ms
font walk against ~7.4 ms — that carrying a figure over is a guess, not an estimate.

### `tile-bench` on Windows, and what the render constants cost here

```
cargo build --release --example tile-bench
./src-tauri/target/release/examples/tile-bench.exe testdata/vector-heavy.pdf --mode single --rounds 4
./src-tauri/target/release/examples/tile-bench.exe testdata/text-base14.pdf  --mode single --rounds 4
```

It needed two fixes and neither was a refusal: the hardcoded `vendor/pdfium/lib` (on Windows that
directory exists and holds the *import* library, so it fails at `LoadLibraryExW` rather than at a
missing path — see the trap), and `peak_rss_mb`, which returned `NaN` off unix. That is
`GetProcessMemoryInfo`/`PeakWorkingSetSize` now, keeping the `NaN`-on-failure contract because a
zero would read as "PDFium allocated nothing". A working set is trimmed under memory pressure, so
it can read below the peak *commit* the same run reached; it is still the right counterpart to
`ru_maxrss` for this question, since both are about pages actually held.

**§9's architectural conclusions hold on Windows, and every constant behind them is worse.**
`vector-heavy` is generated by the committed `make_vector_pdf.py` against the same PDFium pin, so
this row is a fair comparison of *constants* — across different machines, which is the useful
framing here rather than a CPU verdict:

| | macOS arm64 (`docs/PLAN.md`) | Windows (2026-07-30) |
|---|---|---|
| 256² tile of the A0 page | 0.98 s | **1.35 s** |
| that tile as a share of a full render | 4.3% | **3.8%** |
| full page, 1× | 22.8 s | **35.1 s** |
| full page, 2× | 48.4 s | **88.3 s** |
| fixed cost per render *call* | ~1 s | **~1.3 s** |

So the shape is the same on both — PDFium culls spatially, a tile is a few percent of a full
render, and there is a hard per-call floor that does not shrink with the request. The magnitudes
are **1.5--1.8× worse**, and the floor about a third worse. The practical consequence: a latency
budget written against the macOS floor is optimistic on Windows by roughly that much, and
`docs/PLAN.md` §4's four consequences now rest on two platforms rather than one.

Independently cross-checked on the same machine before being believed: `backend-probe` measured a
**1536 ms** 512² render of the same document through the worker, against tile-bench's 2203--3073 ms
for that tile size. Same order, differing about as much as a centred tile and a placed one should
— which is what says the numbers are the document's and not the harness's.

The cheap-page half confirms the asymmetry the plan bets on: `text-base14` is **flat**, 0.6--0.9
ms/Mpixel at every tile size and scale, with no per-call floor at all. Read that as a Windows
result on its own and **not** as a comparison — macOS measured `text-heavy.pdf`, which this
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
| 2 | 1.34× | 1.52× | — |
| 4 | 1.99× | 2.29× | 2.56× |
| 6 | **3.59×** | **3.60×** | **3.22×** |
| 8 | 3.60× | 3.54× | nothing further |

**The shape reproduces exactly**: monotone gains to six and nothing at eight, which is the
capacity ceiling doing its job. Six is stable to within 0.01× across runs and is slightly
*better* than macOS.

**Do not read the middle rows as a platform difference.** Pool 2 moved 1.34 → 1.52× and pool 4
1.99 → 2.29× between two identical runs, and the per-round warm figures span ±20% (pool 2: 2625
to 3894 ms). Only the pool-6 result and the flat pool-8 result are outside that spread. The
per-round table is printed for exactly this reason — a single speedup column would have made
the intermediate points look like measurements.

Cold and warm are indistinguishable here (5155 vs 5105 ms at pool 1, 1100 vs 1094 at pool 6),
because a ~9 ms spawn is noise against a 5 s screenful. That is a property of this corpus, not
a finding about spawn cost.

Still macOS-shaped, but less than it was: **`open_check.py` runs five of six phases** since the
single-instance plugin closed the document-handover gap. The one that stays macOS-only is the cold
double-click, and that is not a gap — an Explorer double-click arrives in `argv`, which the
`argv` phase already covers. It skips with its reason rather than disappearing.

`session_check.py` needed no porting at all. It does need a document of **at least eight pages**,
since its target page is 7 and `goToPage` clamps; on a shorter one it now says so as a named check
rather than reporting a wrong page, which is what it used to do. `incr-scan-20p.pdf` is the quick
fixture for it; `text-base14.pdf` (1 page) and `rotated.pdf` (4) are not long enough.

Both now call `clear_strays` before their first launch. That is not tidiness: on Windows and
Linux a leftover instance **silently absorbs** every later launch through the single-instance plugin, so
the next phase reports `run timed out` with no output — which reads as the app hanging. It prints
a `[WARN]` naming the pids when it finds any, because a run that needed it is a run whose earlier
phases are suspect.

`webview_guard` still checks nothing off darwin (see the trap — Chromium throttles occluded
windows too, so those runs are protected by nothing).

#### What the port changed, so it is not rediscovered

- `worker.rs` now compiles everywhere and refuses off macOS — which its own module doc had
  claimed since it was written, and which was not true. 38 error sites, all POSIX:
  `std::os::fd`, `mmap`/`munmap`, `File::from_raw_fd`, `ExitStatus::signal`. `Shm` off unix is
  a type with a private field and constructors that refuse.
- `worker_child.rs` was `#[cfg(unix)]`, with the `--render-worker` argv refusing off unix
  rather than falling through. **Both are gone as of 2026-07-29.** The module compiles
  everywhere; three functions know the platform (the two mapping handovers and
  `establish_boundary`) and the rest is shared. The refusal that replaced the `cfg` is
  `establish_boundary` itself, which fails where there is no boundary to establish and does
  so *before* a document is opened — the deleted one was never the load-bearing guard, and
  keeping it would have suggested otherwise.
- `pdfium_library_dir()` picks `bin/pdfium.dll` on Windows against `lib/libpdfium.dylib` on
  macOS, and now checks for the **library** rather than the directory. See the trap: on
  Windows `vendor/pdfium/lib` genuinely exists and holds the import library, so the old
  existence check passed and the bind failed later.
- `launch.rs`'s percent-decoding test takes a platform-shaped URL. `Url::to_file_path` wants a
  drive letter on Windows and refuses `file:///Users/...`, so the macOS fixture asserted only
  that refusal. Written that way rather than gated off Windows, deliberately — a check that
  silently stops existing on a platform is the thing this file warns about elsewhere.

#### What running it found, which no gate could

Three defects, none of which any amount of compiling would have surfaced.

- **`npm run tauri build` failed on a tree that gated 7/7.** `backend_probe.rs` called two
  dyld symbols unguarded; clippy never links, and `cargo test` links a `[[bin]]` with `main`
  replaced, which drops them as dead code. There is a **`bins` gate** now, and it was proved
  to fail (5.7 s, debug profile) against the un-gated file before being trusted. The probe
  itself is now a thin entry point over `backend_probe/imp.rs`, refusing off macOS the way
  `fdpass_probe.rs` does — every claim it makes is about a worker backend that cannot exist
  there.
- **Not one tile was ever painted.** `tiles.ts` fetched `tile://localhost/...`, which WebView2
  cannot resolve; Tauri serves custom protocols at `http://tile.localhost/...` on Windows. The
  origin now comes from Tauri's own `convertFileSrc`, and the CSP names
  `http://tile.localhost` beside `tile:` — it already named `http://ipc.localhost` beside
  `ipc:`, so the convention was known and applied to one scheme and not the other.
- **`cargo build --release` is not a production build.** It produced a window showing
  *"localhost refused to connect"*: `frontendDist` is embedded by the cargo feature
  `tauri/custom-protocol`, which the Tauri CLI passes and a bare cargo build does not, at any
  optimisation level. Build through `npm run tauri build`, or pass the feature.

**The old version of this section named the wrong blockers**, and the shape of the error is
worth keeping. It listed `sanitize_rewrite.rs` and `tile_bench.rs` as the compile errors;
both were real, but clippy never reached either, because the *library* failed first. A
blocker list assembled by reading code cannot know what fails first — that is a property of
the build graph. It also said `TPDF_BACKEND=in-process` was "the only thing that runs off
macOS", which was false: `pub mod worker;` was unconditional, so the crate carrying that
control did not compile and nothing ran off macOS at all.
