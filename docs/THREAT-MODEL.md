# tpdf — Threat model

Phase 0's last item (`docs/PLAN.md` §9). The architecture it describes was already
committed to and largely measured; what was missing was the document that says what the
architecture is *for*, which claims rest on evidence, and which rest on nothing yet.

**The rule this document follows:** every mitigation below is either measured — with the
spike that measured it named — or marked untested. A control that has never been shown to
fire is indistinguishable from one that keeps passing, and this repository has been bitten
by that twice already (a crash test the optimizer deleted, a stray-file check that was
inert on macOS for months). An unmarked assertion here would be a third.

Written 2026-07-26.

---

## 1. What is being defended, in priority order

1. **Everything on the machine that is not the document.** Files, keychains, other
   applications' data. A PDF viewer is a program that runs attacker-controlled input
   through a C++ parser on demand; this is the asset that matters.
2. **The user's identity and network position.** Credentials, tokens, and the ability to
   reach an internal network the attacker cannot.
3. **The confidentiality tpdf claims to have delivered.** Unique to an editor with
   redaction in it: a document reported sanitized that is not is worse than no redaction
   feature at all, because the user acts on the claim. This asset has no analogue in a
   pure viewer and it is the one tpdf is most likely to lose.
4. **Availability.** A document that hangs the application. Lowest priority — annoying,
   not dangerous — but it is where resource limits earn their place.

## 2. Who the adversary is

**The document.** Every PDF tpdf opens is assumed hostile, whatever its provenance: opened
by hand, arrived by mail, dropped on the dock, opened by file association, pulled from a
network share, extracted from another document's attachments, or pasted in as a page from
a second file. There is no trusted-source path and there should never be one — the whole
value of the isolation is that it does not depend on knowing where a file came from.

Secondary adversaries, defended against but not the design driver:

- **The distribution channel** — a tampered download or update.
- **The dependency graph** — a compromised crate, npm package, or PDFium build.

Explicitly **not** defended against, and out of scope:

- A local attacker already executing code as the user.
- A modified build of tpdf itself.
- Physical access to an unlocked machine.
- Traffic analysis, timing side channels, and anything requiring the attacker to observe
  the machine's hardware.

## 3. Trust boundaries

Four principals, each trusting only what is below it in the table.

| Principal | Authority it holds | Authority it does not |
|---|---|---|
| **Webview** (Svelte) | Draws, receives tiles, issues commands | No filesystem, no network, no PDF parsing |
| **Coordinator** (Rust, the Tauri process) | Opens files the user chose, owns the window, spawns and kills workers, owns every shared mapping | Never parses PDF syntax itself |
| **Worker** (Rust + PDFium) | Parses and renders whatever bytes it is handed | No filesystem, no network, no path to the document, cannot create a file |
| **Disk** | Holds the document and tpdf's output | — |

Two consequences of that table are load-bearing and worth stating separately.

**The document reaches the worker as memory, never as a path.** The coordinator opens the
file and maps it; the worker receives the *descriptor*, `dup2`'d to a fixed number before
`exec`. A descriptor has no name to guess and survives a policy that forbids opening files
at all — which is what makes a `(deny file-read*)` worker possible in the first place.
Measured in spike 0.5: a worker under that policy opens a 775-page document and renders it
pixel-identically to an unsandboxed one.

**Every buffer the worker writes into is the coordinator's allocation.** Tiles are rendered
straight into a shared mapping the parent created and sized. The worker cannot enlarge it,
so tile memory is bounded by construction rather than by supervision — which matters,
because supervision turns out to be the weaker of the two (§4, T3).

## 4. Threats

### T1 — Memory corruption in the parser

**The threat.** PDFium is native C++ parsing an attacker-controlled file format with
recursive object graphs, a dozen stream filters, and thirty years of accumulated
compatibility. Chrome sandboxes it in a separate process; the reasoning transfers exactly.

**What stops it.** Parsing and rendering happen only in worker processes with no
filesystem or network authority. A worker that is compromised holds nothing worth having:
it cannot open a path, cannot write a file, cannot bind a socket, and can reach the
document it was already given and one tile buffer.

**Evidence** (spike 0.5, `worker-bench --mode crash`): a worker killed by SIGABRT, by
SIGSEGV, or exiting non-zero is noticed by the coordinator within **0.1–0.6 ms**, as an
EOF on the line-delimited control channel. Respawning, reopening the 775-page document and
rendering the first tile costs **8.5–12.9 ms**. The coordinator is unaffected in every
case. The boundary itself is not a reason to hesitate: a control round trip is **6 µs**
and moving a 4 MB tile through shared memory is **0.11 ms**, against **3.0 ms** to hand
the same tile to the webview. Isolation costs about 1/27th of the UI.

**Residual.** A worker compromise can still lie about what it rendered or extracted. Any
security-relevant answer — above all a redaction verification — must therefore not be
taken on a worker's word alone; see T5.

### T2 — Execution through the document's own features

**The threat.** PDF carries document-level JavaScript, launch actions, URI actions,
embedded executables, and XFA. These are format features, not bugs, and Acrobat runs
several of them.

**What stops it.** Nothing in tpdf ever invokes them — and, more usefully, the vendored
PDFium build cannot.

**Evidence** (`worker-bench --mode engine`, and a read of `pdfium-render` 0.9.3):

- The build contains **zero `v8::` symbols and no real `CJS_Runtime`** — only
  `CJS_RuntimeStub`, whose `ExecuteScript` disassembles to three instructions that zero
  the output and return. There is no engine to disable.
- It contains **zero `CXFA_` symbols**. XFA is not built in, so §6's XFA refusal is a
  property of the binary rather than a policy that could be forgotten.
- `pdfium-render` never calls any `FORM_Do*` function — not `FORM_DoDocumentOpenAction`,
  not `FORM_DoDocumentJSAction`, not `FORM_DoDocumentAAction`. Those are the only entry
  points through which PDFium executes document script.
- Its `FPDF_FORMFILLINFO` sets `m_pJsPlatform` to null and every callback to `None`, so
  even a fired action has no platform to open a URL, launch a file, mail, upload, or
  download with.

This cannot be tested behaviourally. A document whose JavaScript does nothing looks
exactly like a document whose JavaScript was never run, so the absence of an effect is not
evidence of the absence of an engine. The symbol table is the only thing that
discriminates, which is why the check reads the binary.

**Residual, and it is real.** `FPDFDOC_InitFormFillEnvironment` *is* called on every
document open by `pdfium-render`, so the form-fill machinery is reachable attack surface
even with nothing behind it — this is T1 surface, not T2 execution, but it is surface that
a viewer with no form support did not have to expose. And all of the above is a property
of *this* PDFium build: it must be re-checked after every bump, and a build that ships V8
would silently move this threat from "impossible" back to "policy".

### T3 — Resource exhaustion

**The threat.** A decompression bomb, an A0 CAD page, a 25,000-object graph, or a page
that simply takes forever. None of these requires a vulnerability.

**CPU is bounded, in two layers.** `RLIMIT_CPU` is accepted on macOS and does fire — but
it counts CPU over the *process lifetime*, not per request: under a 3 s limit a 1.72 s
render succeeds and the next dies 1.30 s in, at a cumulative 3.0 s (spike 0.5). So it
bounds how long a worker may live, and the per-request bound has to be the coordinator's
own deadline plus a kill, measured at **1.2 ms to kill and reap, 4.8 ms to respawn**.
PDFium's progressive API (`FPDF_RenderPageBitmap_Start` with an `IFSDK_PAUSE` callback) is
the cooperative alternative and is still unexercised — it is the mechanism for cancelling
without discarding the work, and for not holding the global PDFium mutex through a long
render.

**Memory has no kernel bound on macOS, and the substitute is weaker than it looks.**
`setrlimit` refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright with `EINVAL`
(spike 0.5, confirmed independently through Python's `resource` module). The remaining
mechanism is supervision: the coordinator samples the worker's `ri_phys_footprint` through
`proc_pid_rusage` and kills it over budget. Measured 2026-07-26
(`worker-bench --mode footprint`), against a child taking memory as fast as the allocator
will hand it over (~22 GB/s) and a 128 MB budget:

| poll | overshoot, median | worst seen | what the interval bounds | poll costs | bursts missed |
|---|---|---|---|---|---|
| 0 ms | 0.0 MB | 0.0 MB | — | 100% of a core | 0/5 |
| 1 ms | 16.4 MB | 18.3 MB | 22 MB | 0.033% of a core | 0/5 |
| 5 ms | 22.4 MB | 88.7 MB | 113 MB | 0.007% of a core | 0/5 |
| 20 ms | 225.8 MB | 225.8 MB | 280 MB | 0.002% of a core | 4/5 |
| 50 ms | 368.7 MB | 368.7 MB | 483 MB | 0.001% of a core | 4/5 |

A sample costs **0.33 µs**, so polling is essentially free and the interval is a pure
overshoot-versus-nothing trade. Three things this measurement establishes:

- **Overshoot is interval × growth rate**, and the worst case is what a budget must be set
  from, not the median. Neither the median nor the worst *observed* is the bound, because
  both depend on where the crossing happens to fall between two samples; the arithmetic
  bound is the column that matters.
- **Polling bounds a sustained leak, not a burst.** At 20 ms and above, most runs never saw
  the event at all — the child took its full 512 MB and exited between two samples. This is
  the important negative result: supervision cannot be the only memory defence, because a
  bounded burst can complete inside one sampling gap and never be attributed to anything.
  Bounding the *inputs* — decompressed stream size, tile dimensions, page count per
  request — is the layer that catches those, and it is not optional.
- **A zero interval is not free supervision.** It burns a core, and the low overshoot it
  shows is partly bought by starving the child of the CPU it was allocating with.

A pool of N workers must therefore be budgeted at (per-worker budget + bounded overshoot)
× N. The overshoot term is exactly the price of having no kernel limit — and on Windows it
should disappear, since a job object with `JOB_OBJECT_LIMIT_PROCESS_MEMORY` is a real
kernel bound that needs no polling.

**Decompression is bounded at the parser.** `lopdf`'s `LoadOptions::max_decompressed_size`
refuses a 1 GiB-inflating stream in 0.3 ms (spike 0.4). Worth remembering why the bound
belongs on the rewriter and not only on a verifier: `qpdf in out` re-encodes stream data
by default and so fully decodes that same stream, costing **1.92 s of CPU at 8.4 MB
resident** — 600× amplification in time, at no cost in memory. A limit expressed in
megabytes would have caught none of it.

**Residual.** One pathological page still holds PDFium's global mutex and starves every
other render in that process (`pdfium-render`'s `thread_safe` feature serialises every
call). The progressive API is the fix and is unwritten. And a burst below the polling
granularity is unbounded until the input limits exist.

### T4 — Filesystem and network reach from a compromised worker

**The threat.** T1's payoff. A worker that has been taken over wants to read `~/Documents`,
write a launch agent, or open a socket.

**What stops it.** `sandbox_init` with the profile in §5, applied after the mappings are in
place and PDFium is bound, and irrevocable thereafter. Reads, writes and socket binds are
all denied and the render is unaffected, because the document never arrives as a path.

**Evidence** (spike 0.5, `worker-bench --mode authority`): under the profile, `read
/etc/hosts`, `write temp file`, `bind tcp socket` and `bind udp socket` are all denied, and
the rendered tile is **pixel-identical** to an unsandboxed render on base-14, TrueType, CID
and the 775-page corpus.

**The trap, which cost a day.** A stricter-looking profile renders base-14 documents
*differently* while returning success — PDFium silently substitutes a font face with almost
the same amount of ink. Denying `file-read*` and allowing it back on the font directories
does not fix it, because what the font mapper needs is **metadata** reads across the whole
filesystem, not data reads from the font directories. Hence the shape of the profile below.
The general rule, and it is the third time this shape has appeared in this project: **verify
a sandbox by comparing pixels, never by checking that the render returned `ok`.**

**Residual.** A hostile document can still learn which paths exist. It cannot read one,
write one, or open a socket. `sandbox_init` denials also do not appear in the unified log
without an explicit report clause, so "no log entries" is not evidence that nothing was
denied.

### T5 — False assurance

**The threat.** tpdf tells the user a document is redacted, or that an edit was applied
faithfully, and it is not true. This is the threat this project is most exposed to, because
it is the one the competition fails at and the one a user cannot check.

**Every known instance is a case of a check that cannot see what it claims to certify.**
Collected here because they are one failure, arriving from five directions:

- **A byte scan cannot verify a document with a Type0 font.** Under Identity-H the content
  stream carries glyph ids, not text, so a secret drawn on the page is never present in the
  file as its own bytes. Spike 0.3's own leak scanner called a CID fixture clean while text
  extraction proved the needle was still there.
- **A clean pixel diff is not evidence of a faithful edit.** PDFium regenerates page
  content wholesale on any object edit; spike 0.3 measured marked content and its
  `/ActualText` being discarded while every pixel matched.
- **`set_text()` draws `.notdef`, or codes for glyphs that do not exist, and returns
  success.** In one of the two measured cases displayed text and extracted text disagree —
  a search hit on text nobody can see.
- **An object a prior revision overwrote is reachable by no parser.** It is handed to
  nothing: not a graph walk, not `qpdf --check`, not PDFium. A file with more than one
  revision cannot be certified, only rewritten and then certified.
- **`lopdf` silently drops encryption on save**, and **PDFium accepting a file is not
  evidence the file is well formed** — it rendered a document with a wrong `/Size`
  pixel-identically to a correct one.

**What stops it.** One rule, stated in `docs/PLAN.md` §6 and repeated here because it is the
core of this threat: **a verifier must decode each carrier in that carrier's own encoding,
and a carrier it cannot decode makes the result "not verified", never "clean".** "Grep found
nothing" is not evidence. Verification re-parses with an independent parser, and that
requirement paid for itself the first time it ran, on a bug in tpdf's own object sweep.

**Residual, and it is the largest open risk in the project.** The rule as written refuses
almost every scanned document, because `/DCTDecode`, `/CCITTFaxDecode`, `/JBIG2Decode` and
`/JPXDecode` are all carriers the sanitizer does not decode (§10 q9). Where the line
between "cannot decode" and "is an image and belongs to a different check" sits has not been
established on a real corpus. Until it is, tpdf must refuse rather than reassure.

### T6 — What the save path leaves behind

**The threat.** The bytes a redaction was supposed to remove surviving in the file, or next
to it.

**What stops it.** Applying a redaction is a **full-rewrite barrier**: an incremental save
appends and leaves the original bytes intact, which is exactly what redaction must not do.
But a non-incremental save is not sufficient either — a serializer can carry over
unreachable objects, unused resources and embedded originals, and overwriting in place can
leave trailing bytes past the new `%%EOF`. So redaction writes a **fresh file from a
garbage-collected reachable object graph**, then atomically replaces the target.

**Evidence** (spike 0.4): a collected `lopdf` rewrite reaches the same verdict as QPDF on
all eleven hostile fixtures. Two conditions attach, both measured: `lopdf`'s own
`prune_objects`/`renumber_objects` are quadratic (1.41 s on a 25,583-object graph, against a
mark-and-sweep whose cost is indistinguishable from not collecting at all), and after
sweeping, `max_id` must be lowered by hand or
`/Size` overstates the file — `qpdf --check` rejects that and PDFium does not notice.

**Residual, and it must be said in the UI.** This sanitizes the PDF. It does not sanitize
previous copies, backups, versioned snapshots, or recoverable filesystem sectors. And on a
signed document there is a further limit: spike 0.6 measured that an appended update leaves
a signature cryptographically intact and rejected by difference analysis at **every** DocMDP
level, including an annotation-only edit to a level-3 certified document that the
specification explicitly permits. "The spec permits this edit" and "a validator will accept
it" are different claims, and only the second is what a user sees.

### T7 — Distribution and update

**The threat.** A tampered download, a tampered update, or a compromised dependency —
including the PDFium binary itself, which is prebuilt from a third party.

**What stops it, and what does not yet.** Direct notarized distribution with a signed
update payload is the intended shape, following `screenpick`. Two things are settled by
policy rather than by code today: the PDFium build's provenance (pinned build, re-checked
after every bump — see T2, and the `remove_probe` standing regression) and permissive-only
licensing, which keeps the dependency set small enough to audit.

**Untested.** The signing and notarization path for a bundled dylib (§10 q7) is a known
open question, and it bit `screenpick`'s release path before.

### T8 — The webview

**The threat.** Content injected into the UI layer reaching Tauri's command surface.

**What stops it.** The webview loads no remote content: the frontend is bundled, tiles
arrive over a custom URI protocol as raw pixels rather than as anything parsed as markup,
and no document-derived string is interpolated into HTML. Document text that must be
displayed — outline entries, search results, form field labels — is attacker-controlled and
must be treated as data at every point. Today none of it reaches the UI at all — the
frontend renders tiles and nothing else, and there is no `@html` or `innerHTML` anywhere in
it — so this is a rule to hold to rather than a property that has been tested.

**Untested.** No CSP audit has been done, and the Tauri capability set is still the
scaffold default. This is Phase 1 work and is a gap, not a mitigation.

## 5. The sandbox policy

macOS, applied in the worker after the mappings are in place and PDFium is bound, and
irrevocable thereafter. The authoritative copy is `PROFILE_WORKER` in
`src-tauri/src/bin/worker_bench.rs`; it is reproduced here because a threat model that
describes a policy without showing it cannot be checked.

```
(version 1)
(allow default)
(deny network*)
(deny file-write*)
(deny file-read*)
(allow file-read-metadata)
(allow file-read-data
  (subpath "/System/Library/Fonts")
  (subpath "/Library/Fonts"))
```

It is an allow-by-default profile that removes the three authorities that matter, which is
weaker than a `(deny default)` profile and is what actually works — a deny-default worker
renders base-14 documents with substituted fonts and reports success. The policy was
arrived at by **bisection, not by reasoning**: `worker-bench --profiles` accepts raw SBPL so
a candidate can be narrowed from the shell without a rebuild, and every candidate is judged
by comparing pixels against an unsandboxed render.

Two rules attach to changing it. Verify by pixels, never by a return code. And re-verify on
a base-14 fixture specifically — an embedded-font document is pixel-identical under a
profile that is badly wrong.

## 6. Windows — a gap, not a policy

Untested, and it shares no mechanism with any of the above. The intended shape:

| macOS | Windows |
|---|---|
| `sandbox_init` SBPL profile | Restricted token, low integrity level, job object |
| No memory rlimit; parent polls `proc_pid_rusage` | `JOB_OBJECT_LIMIT_PROCESS_MEMORY` — a real kernel bound, no polling |
| `RLIMIT_CPU` (lifetime), parent deadline | `JOB_OBJECT_LIMIT_JOB_TIME`, parent deadline |
| Unlinked temp file passed by descriptor | Named section object, or an inherited handle |
| `dup2` to fixed fds before `exec` | Handle inheritance with an explicit attribute list |

The memory row is the one worth looking forward to: Windows has the kernel bound macOS
lacks, so the overshoot term in T3 should not exist there. The rest needs its own spike
before the architecture can be called cross-platform.

## 7. Residual risk, in one place

1. **Redaction verification refuses too much** — the "cannot decode" rule has not been
   calibrated against a real corpus, and applied literally it fails almost every scan
   (§10 q9). Largest open risk in the project.
2. **A memory burst below the polling interval is unbounded on macOS.** Input limits are
   the mitigation and are unwritten.
3. **One pathological page starves every other render in its worker** — PDFium's global
   mutex, and the progressive API that would fix it is unexercised.
4. **Windows is entirely untested** (§6).
5. **A hostile document can enumerate paths** under the sandbox profile.
6. **The form-fill environment is initialised on every document open**, so that surface is
   exposed before any form feature exists.
7. **Webview CSP and Tauri capabilities are scaffold defaults.**
8. **A compromised worker can lie about what it saw** — no verification result may rest on
   a single worker's word.
9. **Nothing here protects previous copies, backups, or free sectors.**

## 8. How to re-verify any of this

```
worker-bench <file.pdf> --mode engine     --lib vendor/pdfium/lib
worker-bench <file.pdf> --mode authority  --lib vendor/pdfium/lib   # use a base-14 fixture
worker-bench <file.pdf> --mode footprint  --lib vendor/pdfium/lib --budget-mb 128 \
                                          --poll-ms 0,1,5,20,50
worker-bench <file.pdf> --mode crash      --lib vendor/pdfium/lib
worker-bench <file.pdf> --mode limits     --lib vendor/pdfium/lib
```

`--mode engine` and `--mode authority` are the two that must be re-run after every PDFium
bump: the first because the absence of a JavaScript engine is a property of the build, the
second because font handling under the sandbox is a property of the mapper.
