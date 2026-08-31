# RATIONALE.md --- how each of these was established

The worked-out accounts behind `AGENTS.md`'s rules: what was measured, what the
measurement cost, and which earlier sentence it corrected. `AGENTS.md` keeps the rule and
points here; this file keeps the evidence.

Not auto-loaded, on purpose --- the same reasoning as `docs/TRAPS.md`. Read the section
for the area you are working in.

Everything here was moved verbatim out of `AGENTS.md` on 2026-08-28, when that file passed
the 150k-character budget an agent loads it under. Nothing was rewritten in the move; the
`>` lines are the only text added, and each one restores an antecedent that stayed behind.
Later corrections are appended in place, as they were before.

## The process boundary, rung by rung

> *The rule, the six-rung table and its verdict are in `AGENTS.md`. What follows is the evidence. `scripts/win_modules.py` is the external module check named there; this is the run of it either side of the flip.*

It was run **before** the flip and reported the parser mapped: 47 modules at peak, `[FAIL]`.
That control is the reason the pass afterwards means anything. After: all four corpora green
with the same ran/skipped splits as before (81/5, 81/5, 75/11, 52/34), the `[WARN]` gone, and
44--45 modules at peak with no `pdfium` among them --- including `outline-hostile`, which is
the corpus that most wants a boundary.

> *On the `job` row of that table.*

**The `job` row's denials are measured as of 2026-07-30, and were not before.** That row
promised "runaway memory, extra processes, orphans" from the day it was written and the probe
tested none of the three: its three authority probes are all *integrity level* properties, so
every rung reported on `lowil` and above while the job's own two limits went unexercised. Two
more probes close it, and the control earns its keep --- `bare` commits 1 GB and starts a
process; every rung with a job is refused with **1455** (commit charge) and **1816** (process
quota). The third, an orphan outliving the parent, is `KILL_ON_JOB_CLOSE` and is still only
claimed: testing it means killing the probe itself.

⚠ **And it has one counterexample, found by accident on 2026-08-25**: a live
`--render-worker --prespawn` with a dead parent, idle twenty-nine minutes after the app
that started it had gone, holding the pipes it inherited and stalling a mutation run.
That establishes the outcome and not the mechanism, and the row above should be read as
memory and process creation measured, orphan cleanup intended. The probe does *not* need
to kill itself — spawn the pre-spawn from a child and kill the child. Both halves are in
the trap index, and `docs/THREAT-MODEL.md` carries the same correction.

> *On the committed-versus-resident asymmetry `AGENTS.md` states.*

**That paragraph was used to justify not looking, and on 2026-08-22 it cost three weeks.** The
half about the bound is right. What does not follow is that nothing needed reading here: a poll
substitutes for a bound, so a platform that *has* one is the platform where "how close did the
worker come to being refused" is both answerable and decisive --- and for three weeks the probe
answered it on macOS and printed `[SKIP]` here, while an append shipped that reaches 95.7% of
the cap. `Worker::peak_commit` closes it, reading `PeakPagefileUsage` through the handle the
parent already holds, so the probe now reports 17/17 with nothing not applicable on either
platform. `docs/TRAPS.md` carries the entry.

**And the render deadline is real on Windows as of the same day, having silently not been.**
`kill_pid` was a `#[cfg(not(unix))]` no-op whose own comment predicted exactly this, and the
three tests that would have caught it were `#[cfg(unix)]`, so the platform where the guard had
stopped working was also the platform where nothing tested it. It did not merely fail to
enforce: `kill_overdue` set the killed flag and logged a kill, so the caller got a deadline
error while the process went on holding a hung render. The trap index entry *"a guard that
degrades to a no-op off its platform stops being a guard"* carries the detail and the mutation
that proves the fix.

**A Windows worker now exists and works** (2026-07-29). `Worker::spawn` builds one on Windows:
the child is created suspended, dropped to low integrity, assigned to the job object before it
executes an instruction, and given two pipes and the document and tile sections as inherited
handles named in argv. `worker-probe` is the evidence --- 11/11 checks as measured that day, on `text-base14`,
`text-cid`, `vector-heavy` and `rotated`, including **pixel-identical** tiles against the
in-process render, text extraction, outlines and search across the boundary. The font
substitution that the macOS sandbox caused, and that `win_sandbox_probe` predicted would not
recur here, did not.

`Worker` carries the two platforms as per-platform type aliases rather than an enum, so the
macOS *types* are what they were: `WorkerProcess`, `WorkerStdin` and `WorkerStdout` resolve
there to `Child`, `ChildStdin` and `ChildStdout` exactly as before. The reasoning was that none
of this can be re-verified on macOS from a Windows machine, so a diff touching only Windows code
is the strongest statement available about what cannot have regressed.

**It is not literally a Windows-only diff, and this said it was** (corrected 2026-07-30, from
the macOS side). The struct fields, the `use`, `WorkerSender`'s inner type and three accessors
were all renamed onto the aliases, and two `#[cfg(not(target_os = "macos"))]` refusal arms were
deleted --- macOS lines, changed. The behaviour is identical because the aliases resolve to the
same types, but that is a claim about what a compiler does rather than the "nothing on that
platform was touched" the sentence promised, and the two are only the same thing until one of
them is wrong. What actually stands behind macOS is that every harness was re-run there. The
counts those runs produced belong in `BUILD.md`'s table, which is the single place they are
written down; a count in prose goes stale the next time a check is added.

## The Windows port: harnesses, printing, packaging

**`backend-probe` runs on Windows too, and passes** --- on `text-base14`, `text-cid`,
`outline-hostile` and `vector-heavy`, the last of which is where a render is slow enough for the
withdrawal checks to run rather than skip. Name sets diffed pairwise rather than counted, no
failures on any; `BUILD.md` carries the per-corpus table. The boundary, the pixel comparisons,
capacity, crash restart, replacement, retirement, close, descriptor return **and the spare's
lifetime** all pass, and that run is also the end-to-end evidence for the Windows spare: a warmed
child exists and is correctly *excluded* from the pool, with the laziness claim beside it intact.
Its Windows primitives are Toolhelp for the module list and the process table,
`GetProcessHandleCount` for descriptors, and `TerminateProcess` for a hostile kill from outside
the pool.

**The two failures it first reported were the probe's, not the pool's**, and the correction is
worth more than the result. Two independent observations agreed that workers were created and
destroyed rather than pooled, and neither could say *when* the sample was taken: it sat behind a
five-second wait for a pre-spawned spare, which Windows did not have, so it spent its whole bound
on every call --- longer than the phase's own four-second idle timeout. The instrument retired the
pool and then measured it. Nothing in `workers.rs` was touched. See the trap, which is now about
the wait rather than about the pool.

`pool-bench` and `prespawn-bench` act as their own worker on Windows now --- their `#[cfg(unix)]`
gate on the re-exec dated from before `worker_child` compiled there, and left each binary unable
to be the thing it measures.

**`tile-bench` was never blocked at all**, and this file said it was for two days: running it
showed no refusal, only a hardcoded `vendor/pdfium/lib` and a `NaN` where a peak should be, both
now fixed. A blocker list is written by reading, and reading over-reports. `worker-bench` is the
one genuine refusal left, and its reason is real --- it carries its own POSIX worker
implementation, fd passing and SBPL profiles included, and shares no mechanism with the Windows
model. Seven of its eight modes; the eighth needs no worker and now runs.

**The two viewer harnesses run there too** (2026-07-30). `session_check.py` needed no porting at
all and passes its four phases with both controls --- note it needs a document of **at least eight
pages**, since its target page is 7; on a shorter one it now says so rather than reporting a wrong
page, which is what it used to do.

`open_check.py` runs **five of six**. It ran four until the last gap was closed: a second launch on
Windows was a second process, two windows and two worker pools, where macOS hands the document to
the running app. `tauri-plugin-single-instance` closes it --- the second process forwards its argv
to the first and exits --- and the callback feeds the same `Launch` queue and emits the same
`OPEN_EVENT` as every other route in, so there is one path for "open this document" rather than
two that can drift. Proved by mutation: disabling the plugin turns the phase red with *"nothing
ever arrived"* while its control still passes.

The one phase that stays macOS-only is the cold double-click, and that is not a gap: an Explorer
double-click arrives in `argv`, which the `argv` phase already covers, so there is no second
mechanism there to test.

So the tally on documented blockers is **four lists wrong in one week, always by
over-reporting**: of six benchmarks and harnesses called macOS-only, two were genuinely gated,
one was trapped behind a `cfg` it never needed, one had only a hardcoded path, one needed
nothing, and one was two-thirds portable. Run it before writing it down as blocked.

**The error has a second direction, and it is the quieter one.** The two mutation harnesses were
on nobody's blocked list --- and `scripts/mutate_rust.py` had never executed a single mutation on
Windows, dying on `read_text()` before its first one, while `scripts/mutate_frontend.py` silently
could not find three of its anchors. Over-reporting a blocker costs a capability nobody uses;
**under-reporting one costs a check everybody believes ran**, and a harness that has never run on
a platform produces no failures there, exactly like one that passes.

**Pre-spawning works on Windows too** (2026-07-30), so both platforms now start a worker before
a file is chosen. The handover is the only part that differs and it had to: a macOS parent
*sends* a descriptor over a socket, and a Windows parent **writes into the child's handle table**
with `DuplicateHandle` and then names the number it wrote. That direction is the one integrity
levels permit --- medium may reach into low, never the reverse --- so the handover survives the
containment structurally rather than by luck. The message is a `Handover` of its own rather than
a `Request` variant, which is what makes "adopt a second document" unsayable instead of something
the child must refuse.

Measured, not assumed, by `prespawn-bench`: **8.4--9.6 ms saved per open**, on a spawn-to-first-reply
of 8.9--10.4 ms for small documents. The saving is nearly constant, and that is the difference
from macOS worth knowing --- there the system-font walk is ~7.4 ms of it, here it is **~1.4 ms**,
so on Windows what pre-spawning buys is almost entirely the fixed floor (`CreateProcess`, the
loader, mapping `pdfium.dll`, the token and the job) rather than font enumeration.

**Printing works on Windows** (2026-07-30), which was the last user-facing capability the
platform did not have --- `present_job` returned `Err("printing is implemented on macOS only")`,
and its comment still justified that with "everything in this repository is macOS-only until a
Windows build has actually run", which had stopped being true two days earlier.

The half that corresponds exactly is the **readback**. macOS refuses to open a panel for a job
PDFKit cannot read; Windows now refuses for one `Windows.Data.Pdf` cannot read. Both are the
platform's own PDF stack, so both are independent of the `lopdf` that wrote the job and the
PDFium that drew what the reader saw --- which is the property the whole print subsystem is built
on, and the same standard `docs/PLAN.md` §6 sets for a redaction.

**A third asymmetry closed 2026-08-23, and it had been a missing capability rather than a
defect: a Windows reader could not print a page range at all.** `PRINTDLGW`'s `nMinPage` and
`nMaxPage` both came from `..Default::default()` as zero, and Win32 greys out the Pages radio and
its edit controls whenever those are equal, so the field was dead while a macOS reader typing
"2 to 4" into `NSPrintPanel` got two to four. `print::sheets` now turns a range into sheet indices
and `print_win::spool` prints those rather than `0..count`. The arithmetic is in the **portable**
module deliberately, so the half that decides which page comes out is tested on every platform;
nothing on macOS calls it, because AppKit applies its own range to the document it was handed.
What no check here can reach is the dialog itself --- see the trap of that name. Copies are the
same shape and are deliberately left alone: `nCopies` goes in as 1 and is never read back.

Three things came free with it, and the third is the one worth noticing:

- **`examples/print_probe.rs` verifies the whole path without paper.** "Microsoft Print to PDF"
  is a real driver and a real spooler, and naming an output file in `DOCINFOW.lpszOutput` stops
  it raising a save dialog --- so everything except the panel is driven end to end and the result
  is re-read by the OS parser. It asserts **ink per page** rather than a page count, because a
  broken blit produces the right number of blank sheets (proved: mutating the blit away leaves
  the count green and only the ink red).
- **Three of `print.rs`'s four third-parser checks now run on Windows**, where they were
  `#[cfg(target_os = "macos")]` because PDFKit used to be the only independent parser available.
  They buy real coverage rather than merely existing: breaking `effective_rotation` turns both
  rotation checks red here, including `rotated.pdf`'s *which-pages-survived* case. The fourth
  needs text, which `Windows.Data.Pdf` has none of, so it asserts the page count and prints a
  `[SKIP]` naming what it could not check.
- **Printing maps a PDF parser into the app process, on both platforms.** That is the honest
  complication in "the app process never maps the PDF parser", and it is measured rather than
  glossed: `print-probe` reads its own module table and finds none named pdfium, with
  `Windows.Data.Pdf.dll` beside it as what it mapped instead. The boundary's real guarantee is
  narrower than the sentence sounds --- no *our* PDFium, and the parser that is there is patched
  by Windows Update rather than pinned in `Cargo.lock`.

The `windows` crate this needs adds no crate to the tree: it is already there transitively
through Tauri's WebView2 stack, and it is `MIT OR Apache-2.0`, checked rather than assumed.

> *On the phantom binary under `src/bin/` --- the rule `AGENTS.md` states.*

It had never been caught because Windows packaging had never been attempted, and the trap entry
records the four theories that were wrong first, including an experiment whose control was placed
where it could not fire.

> *`AGENTS.md` carries this decision in four lines; this is the argument.*

**The JavaScript harness does ship, and as of 2026-08-02 that is a decision rather than the
unexamined half of the same hygiene.** `App.svelte` statically imports all six webview entry
points --- `viewercheck`, `scrollbench`, `sessioncheck`, `opencheck`, `autobench`, `startup` ---
so the functional check and its five siblings sit in the bundle that `frontendDist` embeds whole
into the binary, beside `dist/shell.html`, the framework-free page `ShellMode::Blank` loads. Read
out of the shipped file rather than off the import list. The weight is **77.1 kB of a 221.2 kB
bundle, 34.9%**, measured two ways that agree to 0.9%.

**It stays, for two reasons.** The checks are built on observing the artifact that ships --- the
frame loop, the input handlers and the layout they assert against exist nowhere else, which is why
they need a real window at all --- so excluding them at build time would run the 109-name
invariant against a bundle nobody installs, which is the writer-and-its-own-reader failure this
repository has already recorded twice from other directions. And the payload is not what decides
cold start: the `blank` variant deletes the *entire* payload --- no module graph, no Svelte, no
`@tauri-apps/api` --- and moved warm start by -8.4, +9.9 and -0.2 ms across three interleaved runs
(`docs/PLAN.md` §0), because the webview's first custom-protocol request costs ~45 ms and whichever
request is first pays it. 77 kB inside that floor is not a lever.

**The 2026-07-31 removal does not transfer, and the difference is authority rather than size.**
The 17 that left were *executables*: independently launchable, each with its own hostile-input
surface, sitting in the install directory where anything that can run a file can run them. Dead JS
in an embedded bundle is launchable by nothing --- it holds no authority the bundle does not
already have, and every entry point is inert unless its variable is set in the app process's own
environment, which is the binary's surface rather than the bundle's: **no `TPDF_` string occurs in
the shipped JS at all**. Read the *"payload of three files"* above as the statement about
executables that it is; the frontend rides inside `tpdf.exe`.

**The honest cost is `spike_print` and `spike_exit`**, registered in `generate_handler` and
therefore callable by any script the webview runs: one prints to stdout, the other calls
`process::exit` with the code it is handed. Two things bound that, and neither is a promise about
the harness. The CSP is `default-src 'self'` with no `'unsafe-inline'`, so the only script that
runs is the one that shipped --- residual risk 7 in `docs/THREAT-MODEL.md` carries that, the T8
invariant that keeps document text from becoming script or navigation, and the seam it leaves,
since a grep over TypeScript cannot see the Rust half. The marginal authority is nil: a caller
able to reach `spike_exit` can already reach `open_document` and the print path, so what these two
add is a denial of service, not an escalation. **What would reopen the decision**: a spike command
with authority past print-and-exit, or a harness grown to where bundle size moves the shell floor.
The second is 45 ms of protocol toll away --- a decision about the numbers above, to be
re-measured rather than inherited.

## The PDF layer: what each dependency cost to settle

> *On the encrypted-save work `AGENTS.md` describes.*

**Two defects came out of building it, and both had been shipping.** The rewrite's guard
asked `trailer.has(b"Encrypt")`, which `lopdf` removes the instant it authenticates --- and it
tries the empty password unprompted --- so every permission-restricted document, the
commonest encrypted PDF there is, went straight past the guard and was written out
decrypted. And the properties panel reported *no encryption* for exactly those documents,
for the same reason one module over. Both are in the trap index; the fixture that would have
caught the first is one its own doc comment argued was unnecessary.

> *`AGENTS.md` carries these three in one paragraph; this is the full version.*

**Links take the same route, and it costs a second destination resolver** ---
`outline.rs` asks PDFium because a bookmark is a PDFium object, `links.rs` reads the
destination array itself. That is the drift trap this file's index names, and sharing
`Target` fixes the vocabulary while saying nothing about whether the two reach the same
page. So `links.pdf` gives its outline entries the same destinations as its links and
`links-probe --mode agree` compares them --- both against the manifest rather than against
each other, since two resolvers wrong in the same way agree perfectly. **It found a defect on
its first run**: `FPDFDest_GetLocationInPage` answers only for `/XYZ`, so every `/FitH`
outline entry had been landing at the top of its page since `outline.rs` was written.

**The properties readout takes the same route, and there the PDFium alternative genuinely
existed.** All eight `FPDF*Signature*` symbols are exported by the vendored build --- checked
with `nm`, not assumed --- so `docinfo.rs`'s signature half could have gone through it.
`FPDFSignatureObj_*` has no accessor for the signature *field's* name, none for `/Location`,
and nothing at all for `/Info` or `/Encrypt`, so a PDFium implementation would still have
needed this parse and would then have been a second resolver to disagree with it. What that
API is good for is a **differential**, which is the same instrument `links-probe --mode agree`
is and is not built here.

**Since 2026-08-21 that module also parses the signer's certificate**, which is a second ASN.1
parser on attacker-chosen bytes and is bounded and sandboxed accordingly --- see
`docs/THREAT-MODEL.md` §T6.8 and *Who signed it* in `docs/PLAN.md`. The route stays `lopdf`
plus `cms` rather than PDFium for the same reason and one more: `FPDFSignatureObj_GetCert`
hands back the DER of the signer's certificate and nothing above it, so the chain length and
the `matched_signer` distinction would be unavailable through it.

**The differential is built as of 2026-08-21** --- `examples/signature_probe.rs`, seven
comparisons per signature against PDFium's own reading of the same file, including the
certificate parsed out of *each reader's own* `/Contents` blob. 35 comparisons across the five
signed fixtures, and five mutations of `docinfo.rs` proving each check can go red. It is the
same instrument `links-probe --mode agree` is, and it is what makes `parse_certificate` public.
`BUILD.md` has the invocations and the mutation table.

**One crate reads XMP, added 2026-08-21, and it adds no package.** `quick-xml` (MIT) was
already in the tree through Tauri's `plist` dependency, so declaring it direct changed the
count by nothing --- checked with `cargo metadata` before and after rather than assumed, which
is the standing rule and the one case where it produced a genuinely surprising answer. What it
does change is the trust boundary: an XML parser is newly reachable from attacker-chosen bytes.
`docs/THREAT-MODEL.md` carries the four bounds, and the one worth knowing here is that entity
expansion is **structurally** impossible rather than bounded --- `quick-xml` hands every
`&...;` back as its own event and expands nothing unless you supply a resolver, which this
never does.

**Three crates read certificates, added 2026-08-21**: `cms` for the CMS `SignedData` in a
signature's `/Contents`, `x509-cert` for the certificate inside it, and `der` underneath both.
Nine packages in total (563 to 572), every one `Apache-2.0 OR MIT` except `flagset`, which is
`Apache-2.0` alone. They matter to the threat model as much as to the licence: this is a
**second ASN.1 parser on attacker-chosen bytes**, and `docs/THREAT-MODEL.md` §T6.8 records
what bounds it --- it runs in the worker, the blob is capped at `MAX_SIG_BLOB` before the
parser sees it, and exceeding that is reported rather than passed off as a document with no
certificate.

**Nothing reads those bytes directly, and since 2026-08-21 nothing reads them as they arrive.**
`src-tauri/src/ber.rs` walks a signature's `/Contents` first and hands the parsers a value in
definite-length form, dropping whatever follows it. It is **no dependency at all** --- about 150
lines, because the alternative was a general BER library for one length rule --- and it exists
because the specification and reality disagree. RFC 5652 requires DER; a signer that streams its
output cannot know a value's length before writing the value, so it writes the indefinite form,
and `der` refuses that outright. Measured on a real signed contract: five indefinite values
nineteen levels deep, and every reader here saw nothing. It also decides **where the blob ends**,
which is the same question and was the larger half --- the trailing-zero scan it replaced could
not tell zero padding from a two-byte end-of-contents marker, and ate three of them. What it
deliberately does **not** do is canonicalise: a `SET OF` out of order or a constructed string in
segments comes out as it went in and is refused by the parser after it, which is reported as
unread. Its bounds are in `docs/THREAT-MODEL.md` §T6.8, and the property that lets it sit in
front of *every* signature --- a DER blob comes back byte-identical --- is asserted against the
real fixtures.

## The gates, one at a time

**`anchors` exists because two different failures are invisible in `git status`, and both
happened on 2026-08-16.** It asserts that every mutation's search string occurs exactly once in
the file it names, across all three tables. How many that is is the gate's own output
(513 on 2026-08-19) and deliberately not a number here: this sentence said 289 for two
weeks after the tables grew past it, which is the failure the trap count above already
has a `grep -c` for.

Zero means one of two things and the gate deliberately does not guess which, because they need
different fixes. Either **a killed harness left its edit in the tree**: the harnesses mutate files
a feature branch is usually already modifying, so the leftover shows nothing new in `git status`
and nothing eye-catching in a large diff --- `viewer.ts` sat holding `this.rotateBy(turns)` in
place of a page turn, and the next run's red baseline read as a defect in the feature. Or **the
anchor has drifted**, and the mutation is aimed at code that is gone. The harness does refuse that
when it reaches it, which is correct and far too late: that is one run of a harness that takes
twenty minutes, and an anchor has sat dead for weeks with nothing saying so.

**It asks a second question as of 2026-08-20: can the test it names go red on this platform?**
An anchor is a string in a file; platform gating decides which strings become code, so the first
invariant was structurally unable to see that `recentdocs`'s two Windows mutations named a test
inside `#[cfg(all(test, windows))]` and declared no `only_on`. On a Mac that name does not exist
and the harness's guard --- right to be loud about a name it cannot find --- refuses the **whole**
table, so 198 mutations had been unrunnable there since the day those two were written. The gate
locates the `fn`, finds its enclosing gated module, and requires `only_on` to match; a test
defined on both sides of the cfg needs no declaration. Proved three ways: a missing declaration
fails, a wrong one fails, and a scan finding no gated module anywhere fails rather than passing
everything in silence.

**And a third as of 2026-08-27: does the test it names exist at all?** That gate above looks only
at *gated* tests, and said so in its own docstring --- *"a name the harness cannot find anywhere is
the case its own guard owns, and is deliberately loud about"*. Loud, and after a full control
pass: on 2026-08-27 a mutation named `an_image_in_the_region_makes_the_plan_incomplete`, which the
increment before had renamed, and the refusal arrived minutes into a run. This is the Rust half of
what `mutations` does for the frontend and it takes about a second. It requires an **exact**
`#[test]` function name, which all 439 distinct `expect` values are, measured before it was
written --- `cargo test` is a substring filter, so a deliberate substring would have to become a
name. Three controls, all red: a name that exists nowhere, a name that is an ordinary `fn` rather
than a test, and a scan that collected nothing.

**`mutations` exists because a guard that works can still answer too late.**
`mutate_frontend.py` runs vitest over `TEST_FILES`, a hand-kept list, and a suite absent from
it still resolves as a name on disk --- it simply never runs, so a mutation aimed at it can
only report SURVIVED, which reads as a gap in the tests rather than a mistake in the harness.
The harness refuses to start when a mutation names a test its control run did not see, and
that guard has a perfect record: **twelve** omissions between 2026-08-17 and 2026-08-23,
twelve refusals, no false SURVIVED. What it cannot do is answer before a full control pass, so
each catch costs a run that had already started --- on 2026-08-23 that was seven mutations
refused while `26.8.8` was being cut. This asks the same question, against the same source of
names, in about twelve seconds.

The names come from `vitest list --json`, which collects without executing, rather than from a
regex over the sources. That is not fastidiousness: a name built in a loop
(`... at ${turns} turns`) is a literal nowhere on disk, and a static scan reports three
failures that are not. It also removes the second parser this repository keeps finding in
other forms.

The second half is `UNMUTATED`, beside `TEST_FILES`: every suite vitest collects is either run
or excluded **with a reason**, so a file that is neither is a finding rather than an omission
nobody can see. Eleven are excluded today, ten of them because no mutation aims at the module
at all --- and the coupling is what makes that safe, since writing one immediately reddens the
first check. The exception is `rowline.test.ts`, whose module *is* mutated while the
expectations live in `marklist.test.ts`; that entry says so.

Nine failure modes, all proved by mutation before the gate was trusted, and the last three are
the ones that matter: a collection that came back empty, a non-JSON stdout, and a non-zero exit
each **refuse** rather than passing quietly, because a broken collector agrees with a clean
tree about everything. A `TEST_FILES` entry vitest cannot collect fails --- that is what a
clobbered or renamed suite looks like. An `UNMUTATED` entry naming nothing is a `[WARN]`,
following the exemption tables in `sinks` and `wiring`.

The last `TEST_FILES` comment had argued for deriving the list from a glob and deferred it,
because widening the name set can surface a duplicate test name and refuse a run for an
unrelated reason. That objection still holds and this does not touch it: the gate changes what
is *checked*, never what runs. Its first run found `viewer.test.ts` listed twice.

**The README is checked against the command registry, and since 2026-08-24 that check is
`src/lib/readme.test.ts` rather than a gate of its own.** It exists because the public README
described an older product for weeks and nothing could see it: an outside review compared it
with the registry on 2026-08-22 and found it saying editing had just begun, saying *the open
file is never modified in place* --- false since Save in place shipped in `26.8.5` --- and
listing four registered commands with shortcuts under *Not built yet*. A prospective user was
being told the product was materially less capable than the binary.

**Both directions are checked, and the first one alone was measurably not enough.** A bullet
under *Not built yet* carries `<!-- not-built: id -->` and none of those may be registered, so
claiming a feature is absent means stating the absence in a form the registry can contradict.
That catches a bullet whose command ships *under the name the bullet guessed* and nothing
else --- which is why stamps went on being listed as absent after shipping as
`edit.stamp.approved` and three siblings. The other direction closes it: every registered
command is either named in a `<!-- built: -->` marker in the README's prose or excluded in the
test's `UNLISTED` table **with a reason**, so a capability cannot arrive unmentioned by being
called something nobody predicted. Same shape as `viewer_sweep.py`'s fixtures and
`viewercheck.ts`'s command audit. Every check was proved red by a control before it was
trusted, the three refusals included, and six of them are permanent mutations in
`mutate_frontend.py` aimed at `README.md` itself.

**It moved out of Python because a regex over `appcommands.ts` could not see eleven of the
seventy-seven commands.** Seven colours from `PALETTE` and four stamps from an inline array
are mapped into template-literal ids, so those ids are literals nowhere on disk. Measured on the day it moved:
`check_readme_claims.py` reported `[OK]` on a README claiming `edit.stamp.approved` was not
built --- the exact error it was written for --- and counted 66 registered commands against a
registry holding 77. Importing the registry removes the second parser rather than improving
it, which is the reasoning `check_mutation_test_files.py` already records for taking test
names from `vitest list --json`. The README arrives through Vite's `?raw`, so neither half
needs a filesystem and the project keeps having no Node type declarations.

What it does **not** check is everything else, including the status paragraph, which was the
sentence most wrong. There is no honest mechanical test for "does this paragraph describe
the product", and a keyword list approximating one would be a second inventory to drift.
Nor does a `built:` marker say the prose beside it is accurate --- only that the command is
claimed somewhere a reader will look. `BUILD.md`'s release checklist carries that half and is
a checklist rather than a check on purpose --- naming which half is weak beats implying both
are strong. The volatile counts
went out with the same commit: the README quoted 325 crates, four npm packages, fourteen
PDFium libraries, 531 cargo packages and "over two hundred" traps against a tree holding
382, 4, 16, 572 and 425. `THIRD-PARTY-NOTICES.md` is generated and carries its own.

**`corpora` exists because the list of window corpora had no home.** It lived in whatever
shell loop somebody typed, so on 2026-08-16 `links-rotated.pdf` was swept as a corpus and
produced eight red checks, none of them a defect --- against a `BUILD.md` paragraph that
already said the fixture is a separate file *because* it reddens two rotation checks.
`scripts/viewer_sweep.py` is that list now: every `testdata/*.pdf` is either a window corpus
with a stated purpose or excluded with a stated reason, and a fixture matching neither is an
error rather than an omission. Same shape as `ci_fixtures.py` and `check_trap_index.py`, both
of which exist because the same class of list went wrong the same way. It also asserts, when
run for real, that every corpus reports the **same check names** --- diffed as sets, since a
check that stopped being printed and a check that started skipping are identical in a total.

**It also asked a second question until 26.8.3, and that one made it red on every hosted
runner**: whether every corpus has a fixture. That is a precondition of *running* a sweep, not an
invariant of the repository, and `ci_fixtures.py` states in its own docstring why nine of the
fourteen are deliberately not generatable there --- fonttools with a per-image system font, qpdf,
a 550 MB write. So the gate demanded on a runner exactly what the repository had already written
down as absent, and no local run could notice, because a development checkout has every fixture.
The missing list is an `[INFO]` line from `--list` now, and the refusal moved to the run path,
aimed at the corpora that run will actually open.

**`workflows` exists because the first tag this repository ever pushed went red on both runners,
and the code was fine.** `release.yml`'s `gates` job was written from `ci.yml` and the copy
dropped the fixture-generation step, so a `print.rs` test that needs `rotated.pdf` failed on both
runners while passing in CI and locally. The release gate was therefore weaker than the gate it
exists to satisfy, which is the rule this file already states about hand-copied commands, with a
whole step lost rather than a flag. Two fixes, and the second is the one that lasts: the list of
runner-generatable fixtures moved into `scripts/ci_fixtures.py` so both workflows call one line,
and `scripts/check_workflow_parity.py` compares the two `gates` jobs step for step --- every
`uses:` with its pinned SHA and every `run:` body, in order. Step *names* are deliberately not
compared, and a control proves it: rewording a label stays green while repointing a pin,
weakening a gate command, deleting a step and renaming the job all go red. It refuses a job it
cannot find and a job whose step scan came back empty, since both read exactly like two jobs that
agree. What it does **not** compare is anything outside that job --- the triggers and the
`release` job differ on purpose, and that difference is the fork threat model rather than drift.

**It also asserts what authority a gates job holds, and that half exists because comparing steps
was blind to it.** The composition an outside review found: the release workflow declared
`contents: write` at file level, every job inherited it, and the gates job then checked out with
the default credential-persisting `actions/checkout` and ran `pip install pyhanko
pyhanko-certvalidator` --- unpinned, resolved from PyPI at the moment the job started ---
**before any gate ran**. Three properties close it, in both files because these two jobs are
meant to be one job: `contents: read` declared on the job or the workflow,
`persist-credentials: false` on the checkout, and a Python install that names
`scripts/fixture-tools.txt` rather than package names. All four failure modes were proved by
mutation, the load-bearing one being **deleting the install step**, which without that check
passes exactly like a clean run. The exclusions this gate's docstring lists are where the next
defect lives.

**`pdfium` verifies the library, not the stamp beside it**, as of 2026-08-02. It compared the
pin against a digest the installer itself had written, and its only fact about the tree was
that *something* matching `*pdfium*` sat in `lib/` or `bin/` --- which on Windows the import
library `lib/pdfium.dll.lib` satisfies alone, so deleting `bin/pdfium.dll`, the blob that
parses every hostile document, left the gate green. `SHA256.txt` now carries a second line
recording the extracted library's own digest, and `--check` asks for `library_path(key)` by
name and re-hashes it. An install predating that line is not refused --- it was admitted by
the archive check and the machines holding one are fine --- but the run prints a `[WARN]`
saying which of the two checks it actually ran. The trap *"A directory that exists is not the
library you need"* had arrived inside the script whose docstring names that same mistake.

**`traps` compares `docs/TRAPS.md`'s titles against this file's index as sets.** The invariant
is the set of titles and a set diff needs no number, which is the doctrine one level up: on
2026-08-02 the tally was right while the index nobody counts was three entries short, added by
the commit that had updated the number. The rule it enforces is the file's own --- a bullet is
the title verbatim. It refuses an empty scan on either side and a duplicate on either side,
since two bullets covering one title can hide a third going missing. Proved four ways, all red:
removing a bullet, adding one naming nothing, duplicating one, and disabling the parenthetical
rule in the checker.

**And it took the whole scheme past the limit it exists to respect, because the checker
tolerated what the rule forbade.** To let one bullet warn that its title names the wrong
mechanism, the matcher strips a trailing ` (...)` before comparing --- so a bullet's tail was
invisible to the set diff by design, and by 2026-08-31 **323** of 588 bullets carried one,
62,440 characters, with `AGENTS.md` at 162,732 against a harness limit of 150,000. Every tail
was audited against the entry it names; one carried a fact its entry did not, which was merged
into the entry, and the other 322 were deleted. The tolerance is an allowlist now
(`ALLOWED_PARENTHETICAL`, one title), and a second rule holds the whole file to 130,000
characters, since the first bounds what a bullet costs and not how many there are --- 116 traps
to 588 in a month is about 1.3 KB a day of index floor with every bullet disciplined. Four more
mutations, all behaving: a bullet regaining a parenthetical goes red, the file passing the
ceiling goes red, an allowlist entry naming a vanished title goes red, and the allowlisted
bullet keeping its parenthetical stays green. The trap entry is *The checker tolerated the thing
the rule forbade, and the index grew until nothing loaded it*.

**`wiring` exists because the box shipped inert and three layers of tests said otherwise.**
`Viewer` reports what it cannot decide through optional callbacks on `ViewerOptions`;
`App.svelte` supplies them in one object literal. `onDrawn` was added to the interface, the
viewer fired it, and that literal never gained the key — so the tool armed, drew its preview,
and reached no model. Every callback is optional by design, because the check harness builds
a viewer with none of them, so a missing key is not a type error either.

The three layers that passed are the point. `viewerdraw.test.ts` constructs its own viewer
and supplies its own `onDrawn`, covering the viewer's half. `viewer_check.py`'s command probe
drives a recorder, covering the command's half. `appcommands.test.ts` sweeps every registered
command for an action, which `drawBox` had. None of them looks at the literal that joins the
two, because it lives in a `.svelte` file no unit test imports and no harness constructs.

The gate diffs the declared callbacks against the wired ones, both ways, and refuses an empty
scan on either side. It found a **second** one on its first run — `onNavigate`, which exists so
a Back and Forward affordance can be re-enabled after a jump and which nothing consumes,
because both commands are guarded on `withDocument` alone and neither greys when there is
nowhere to go. That is now the one entry in its exemption table, with the reason, and wiring
it was the same piece of work as making them grey.

**That work is done as of 2026-08-23 and the table is empty.** `Viewer.canGoBack` and
`canGoForward` are `History`'s own answers, both commands read them, and `App.svelte`
refreshes the pushed menu map on every history change --- which had to include the three
causes that were not announcing at all: a jump from the outline, a search result or a comment
all go through `goToDestination`, and only `followLink` was calling the callback. The table
stays as an empty `dict` rather than being deleted, so the next genuinely-unwired callback is
written against this reasoning rather than from scratch. Proved by mutation in four directions:
dropping the wiring, renaming a wired key, an exemption naming nothing (a `[WARN]`, not a
failure), and the control.

**`docs` exists because a twelve-line comment argued against the feature being built, and
documented nothing.** `armErase`'s doc had been separated from the method by the crop tool's,
and two `/** */` blocks in a row bind only the second --- silently, with no lint, no type
error and nothing a test can assert on. The orphan read *"Only drawings are erasable ...
making the eraser remove whole marks of any kind would be a second, much more destructive
command wearing the same cursor"*: a live design argument, attached to nothing, in the file
where somebody would go looking for exactly that reasoning.

**A scan found 31 across the frontend**, in twelve files, and all were repaired. The rule the
gate pins is total rather than allowlisted --- a doc comment must be followed by code --- and
what makes that possible is a *spelling*: a block introducing a **group** of declarations is
a plain `/* */`, not a doc comment. There is one in the tree, over `commands.ts`'s scoring
weights. The single structural exception is the module header at line 1, recognised by
position, and removing it is one of the four controls that prove the gate fires: it then
reports all 22 of them.

What it cannot see is a doc comment on the **wrong** declaration --- one that binds and
describes something else. Nothing mechanical can, and that is written in the script rather
than left to be discovered.

**`sinks` enforces `docs/THREAT-MODEL.md` T8**, which until 2026-08-02 was the one mitigation
in that document held by convention rather than by a line. Document text --- outline titles,
search results --- is attacker-controlled and reaches the DOM as data; the gate pins the
narrow invariant that makes that checkable at all, **no markup-parsing sink anywhere in the
frontend**, which is sufficient rather than merely necessary because without a sink the only
routes left do not parse markup.

Five further rules close the routes by which a string that cannot become *markup* can still
become a *navigation or a script*: a computed `setAttribute` name, a dangerous literal one
(`href`, `src`, `on*`), an assignment to a navigating property, and --- the blunt ones that
make the others nearly moot --- **creating a URL-bearing element at all**, by a literal name
or a computed one. It also refuses a scan that found no files, no `setAttribute` calls or no
`createElement` calls, since a pattern that stops occurring passes exactly like a clean one.
Every rule proved to fire by mutation, with a control (`this.onChange`, an ordinary field)
proved *not* to.

**Each rule reads the namespaced spelling too**, added 2026-08-02 with the computed-element
rule. `.setAttribute(` does not match `.setAttributeNS(`, and the control for that is worth
keeping: the gate as it stood reported `[OK]` on a planted
`element.setAttributeNS(null, "href", <document text>)`, which is the sufficiency claim
falsified by two letters. A namespaced call whose arguments the pattern cannot parse is
flagged rather than skipped, on the principle the rest of the file is about.

The **one exemption** in the tree is `a11y.ts`'s `createElement(elementFor(block.tag))` ---
the tag is the document's, the element name is not, because `elementFor` is total and
answers `p` or `h1`..`h6` for every input. The marker (`webview-sink-ok:`) is honoured on
the flagged line or the one immediately above it, since a justification that has to fit on
the end of the line it justifies gets written as "safe", which is not a reason. A marker
that ends up beside no finding is printed as a `[WARN]`: an allowlist entry naming something
that no longer exists is how an allowlist rots into a blanket permission.

**The backend half is enforced by the type**, and the two halves cannot see each other.
`outline.rs` refuses `/URI`, `/Launch` and `/GoToR` into `Target::Refused { action }`, whose
string is one of five literals chosen there rather than anything the document said, and
`no_target_variant_may_carry_a_url` matches `Target` exhaustively --- so adding a URL-bearing
variant is `error[E0004]`, not a red test. Read the two together: a grep over TypeScript
cannot see Rust, so a Rust change cannot turn the gate red, and that seam is residual risk 7.

The gate's own first version, shipped hours earlier the same day, is why this is spelled out:
it enforced only that an attribute *name* be a literal, while the threat model claimed
sufficiency from "every `setAttribute` passes a constant name, so there is no URL-bearing
attribute to poison" --- and `setAttribute("href", row.title)` satisfies both. Correct about
the tree in front of it, wrong about what it guaranteed.

## The release workflow, and what a green sweep does not say

`.github/workflows/release.yml` fires only on a CalVer tag. It **invokes `scripts/gates.py`**
rather than re-listing commands in YAML --- a hand-copied command quietly loses a `--locked` and
then gates something weaker than the real gate. It is ported from `screenpick`'s working workflow,
and the one part with no precedent anywhere in the portfolio is signing the bundled
`libpdfium.dylib`: neither sibling ships a native library, and notarization requires every Mach-O
in the bundle to carry a Developer ID signature and the hardened runtime. The dylib is therefore
signed in `vendor/` *before* the bundler copies it, which is now known to be sufficient --- the
`.app` notarized `Accepted`, and both it and the dylib chain to Apple Root CA with the hardened
runtime. Its verification step is written to fail rather than warn: a skipped notarization exits 0
and produces an app Gatekeeper rejects on any machine that has never seen it.

**It took four rehearsal tags, and the sequence is the lesson.** Each failed one step later than
the last --- the gates job (a step lost when it was copied from `ci.yml`), then the dylib signing
(nothing had imported the certificate yet), then the verification step itself (`mapfile` is bash 4
and macOS runners give a `run:` block bash 3.2, so it exited 127 *after* the app and DMG had both
notarized). That is the shape of running a sequence end to end for the first time rather than bad
luck: **the last step of a pipeline is its least-tested code, because everything before it must
succeed before it runs even once.** All three are in `docs/TRAPS.md`, and `BUILD.md`'s release
checklist has the rehearsal-tag habit as step 10. The tag glob matches an `-rcN` suffix on purpose
so a rehearsal is possible; a failed run publishes nothing, since `release` needs `gates` and the
release is created as a **draft**.

> *On Windows, against the POSIX-only `worker-bench`.*

**Nothing measurable is missing here as of 2026-07-31.** Of `worker-bench`'s seven POSIX modes,
only `latency`'s per-tile overhead decomposition measured anything no other harness covers, and
`latency-bench` covers it on both platforms through the production worker rather than a private
POSIX one (`docs/PLAN.md` §0, `BUILD.md`). `worker-bench` still refuses to run here, which is
correct: a POSIX harness not running on Windows was never the gap, only the measurement it held
exclusively.

**The cross-check that portability was for has now run, and it paid.** `latency-bench` on macOS
was compared against `worker-bench --mode latency`, which shares no worker code with it. They
disagreed by an order of magnitude on the same quantity, and the older harness was the wrong one:
it baselines on a variant that never renders, so its residual --- 46.7 ms on `vector-heavy`,
against a printed 46.6 ms --- stays in the answer. `worker-bench` now prints that residual and
warns when it dominates, which is on every fixture measured. The production worker's per-tile cost
is **0.071--0.103 ms** on macOS, ~30x under the webview hand-off, so no conclusion moves. Two
agreeing harnesses would have proved less than these two disagreeing did.

> *`AGENTS.md` states the conclusion; this is the measurement.*

**And the render constants are now measured on both.** `tile-bench` runs on Windows, and
`docs/PLAN.md` §4's four architectural consequences reproduce there against the same generated A0
fixture: spatial culling intact, a real per-render floor, and a full page in tens of seconds at 1x
and at 2x. The ratios that drove the architecture hold; every absolute number is **1.5--1.8x
worse** than macOS, so a latency budget written against the macOS figures is optimistic here by
about a third. **So does the reason to have a pool** --- `pool-bench` on the same page reaches
**3.6x on six workers** and nothing at eight, against 3.22x and nothing on macOS: the same shape,
with the ceiling doing its job. The intermediate sizes are not stable enough to read. `BUILD.md`
has both tables, the caveats, the independent cross-check that says the numbers are the
document's rather than the harness's, and which figures are conclusions.
