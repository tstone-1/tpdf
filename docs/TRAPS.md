# TRAPS.md --- tpdf

Things already paid for once, or verified before writing code, in full.

[`AGENTS.md`](../AGENTS.md) carries the index of these --- titles only, grouped by area ---
and is what an agent loads automatically. This file is what it points at. Read the entry
for any area you are about to work in; add new ones here **and** to the index in the same
commit, or the index stops being a reliable answer to "is there anything known about this?"

The order is the order they were written in, which is roughly chronological and is left
alone deliberately: several entries correct an earlier one, and a few say so explicitly.

**A reference elsewhere to "`AGENTS.md` records ..." means an entry in this file.** These
lived in `AGENTS.md` until 2026-07-28, when they had grown to 93% of it --- an instruction
budget spent, on every task, on the ninety-eight traps that were not the one in front of
you. The roughly one hundred references in code comments and the other documents were
deliberately not rewritten: a mechanical diff over that much prose is a worse risk than one
hop through the index.

---
### PDFium: removed objects come back unless you regenerate the content stream

After `FPDFPage_RemoveObject` (or any page-object mutation), you **must** call
`FPDFPage_GenerateContent()` before saving. Otherwise the original content stream is
written out unchanged and the removed object is still in the saved file --- it looks like
the edit worked, because the in-memory page reports the object gone. This is
[pdfium issue 1051](https://groups.google.com/g/pdfium-bugs/c/RBwhmdbejRk).

For redaction this is not a cosmetic bug, it is a data leak. It is the single strongest
argument for the mandatory post-save verification pass described in `docs/PLAN.md`.

Note also that `FPDFPage_RemoveObject` is marked Experimental API upstream. Pin the
PDFium build and re-test object removal after any bump.

Confirmed still true on chromium/7881 by spike 0.3: saving after `set_text()` without
regeneration changes exactly zero pixels and leaves the original string in the stream.

### Destroying an object removed from a page segfaults

`fpdf_edit.h` says `FPDFPage_RemoveObject` transfers ownership to the caller and that
`FPDFPageObj_Destroy()` frees it. `pdfium-render`'s `Drop` does exactly that, and it
**segfaults inside `FPDFPageObj_Destroy`** --- for text and path objects alike, and whether
the destroy happens immediately, after `regenerate_content()`, or after the save. The fault
is a bad vtable dereference, i.e. the handle is already dead.

`std::mem::forget` on the returned object is the only safe route through this binding; the
memory is reclaimed when the document closes. Removal is redaction's primitive, so this is
not an edge case --- it is on the main path.

`src-tauri/examples/remove_probe.rs` is the minimal repro, kept as a standing regression:
case `c` (leak) must pass, and if case `a` (destroy) ever starts passing, the upstream bug
is fixed and the `forget` can go. Re-run it after any `pdfium-render` or PDFium bump.

Beware the shape of this bug when diagnosing it: piping the run through `tail` reports
`tail`'s exit status, so a segfaulting binary looks like a clean exit. Check `$?` on the
program itself.

### PDFium mutations regenerate page content wholesale

PDFium's page-object edit path does not splice content streams; it regenerates them.
Consequence: **any page-object edit reflows the entire content stream**, so byte-level
diffs of a page are meaningless, round-tripping a page through PDFium is not lossless,
and "the file changed" is not a usable edit-detection signal.

Measured in spike 0.3, editing one text object on a four-line page: `Td` became `Tm`, `Tj`
became `TJ`, every run was wrapped in `q`/`Q` with explicit `rg`, `RG` and `0 Tr`, an
ExtGState `/FXE1 gs` was introduced, `/F1` was renamed `/FXF1` --- and the marked-content
span around the target was **discarded, `/ActualText` with it**. Every other text object on
the page was rewritten too, though none had been touched.

The page rendered **pixel-identical** through all of that. So the rule worth carrying:
**a clean visual diff is not evidence of a faithful edit.** Tagged structure, accessibility
text, optional-content membership and marked-content property lists can all be gone while
every pixel matches. Anything that must survive an edit needs its own assertion.

This is a property of PDFium, **not of PDF**. Surgical operator-level rewriting is
entirely possible --- tokenize the stream, remove or replace selected operators, re-encode
--- and `lopdf` exposes decoded page content as a sequence of operations for exactly this.
Spike 0.3 did it, and it preserved everything PDFium's regeneration destroyed.
What is genuinely hard is mapping a PDFium page object back to the exact operator range
that produced it while preserving graphics and text state. Where surgical precision is
required (redaction), go through a content-stream interpreter, not through PDFium.

### `set_text()` silently draws `.notdef` when a glyph is outside the subset

`PdfPageTextObject::set_text()` takes a Unicode string and re-encodes it into the object's
font. When the font is embedded and subsetted --- i.e. almost always --- characters absent
from the subset have no glyph, and PDFium **returns success anyway**. Measured in spike
0.3, replacing text with `QUARZ ÜBERPRÜFT` where seven of its characters are absent:

- **Type0 / Identity-H:** every missing character encoded as **glyph 0**, `.notdef`.
  Renders as boxes; text extraction returns neither the old string nor the new one.
- **TrueType simple font:** the correct WinAnsi *codes* were written for glyphs that do not
  exist. Renders as jammed-together fragments (the missing codes carry zero advance) while
  **text extraction returns the requested string in full**. Displayed text and extracted
  text disagree --- a search hit on text nobody can see.

Both are silent. Neither is detectable from the return value, and the second is not
detectable from a pixel diff either.

Working in **code space** rather than Unicode makes the condition visible: build the
character-to-code table from the object's existing operand and its extracted text, and a
character with no entry is exactly a character with no glyph. Refuse and report it (see
`docs/PLAN.md` §7 point 6) rather than emitting something. Silent substitution is what
makes the competition untrustworthy; this is where tpdf would acquire the same habit.

### A byte scan cannot verify a document with a Type0 font

Under Identity-H the content stream carries **glyph ids, not text**, so a secret drawn on
the page is never present in the file as its own bytes. A `strings`- or `grep`-shaped leak
check therefore reports *clean* on exactly the documents most modern producers emit.

This is not hypothetical: spike 0.3's own leak scanner did it, calling the CID fixture
clean while text extraction proved the needle was still there. It now reports **not
verified** whenever the document contains a Type0 font.

The general rule, and it is the same one `docs/PLAN.md` §6 states for redaction: a verifier
must decode each carrier **in that carrier's own encoding**, and a carrier it cannot decode
makes the result "not verified", never "clean". "Grep found nothing" is not evidence.

### `thread_safe` does not serialize PDFium --- there is no mutex, and threads crash

Upstream PDFium makes **no thread-safety guarantee at all**, and its authors recommend
parallel *processing*, not multi-threading. That much has always been right here. The
mechanism this file gave for it was not.

**`pdfium-render`'s `thread_safe` feature does not lock anything.** Measured on 0.9.3,
2026-07-27, by `examples/thread_probe.rs`. What the feature actually does is store the
bindings in a global `OnceCell` that is *awaited* rather than unwrapped, plus
`unsafe impl Send for Pdfium` and `unsafe impl Sync for Pdfium`. The only `Mutex` in the
crate guards a page-index cache; the only `RwLock` is in the WASM bindings. A native call
dispatches straight through a function pointer:

```rust
unsafe fn FPDF_LoadPage(&self, document: FPDF_DOCUMENT, page_index: c_int) -> FPDF_PAGE {
    (self.extern_FPDF_LoadPage)(document, page_index)   // no lock, anywhere
}
```

The crate's README still says the feature "wraps access to Pdfium behind a mutex". The
implementation and its documentation disagree, and this file believed the documentation.

What actually happens when two threads render at once, each owning its own
`FPDF_DOCUMENT` --- the exact scenario the old text said was serialized:

| fixture | threads | outcome |
|---|---|---|
| `vector-heavy` (A0) | 2 | **SIGSEGV** |
| `vector-heavy` (A0) | 4 | **SIGSEGV** |
| `text-heavy` | 4 | survives, 3.85x speedup, pixel-correct --- 6 runs out of 6 |
| `text-heavy` | 8 | **SIGABRT** |
| `text-heavy` | 4, five rounds | survives round 0 at 3.85x, then **crashes on round 1** |

So both halves of the old claim were wrong, in opposite directions. They do **not** render
sequentially --- 3.85x on four threads is near-linear, which no global mutex permits. And
extra handles buy **no crash-safety whatsoever**; they buy a segfault.

The architectural conclusion is unchanged and now rests on something true: in-process
parallel tile rendering is not achievable, and parallelism requires **separate worker
processes**. But the reason is that threads are *undefined behaviour*, not that they are
pointless. Those are different arguments, and only one of them is a safety argument.

Two corrections that follow, both previously stated as consequences of the mutex:

- **Nothing "holds the mutex and starves" anything.** tpdf's renders are serialized today
  because `src/render.rs` deliberately uses one render thread, which is our design choice
  and reversible, not a property of the library.
- **The progressive API's value is cancellation, not lock release.** There is no lock to
  release between `FPDF_RenderPage_Continue` calls.

The trap worth carrying is the middle row of that table. **Concurrent PDFium often works.**
On a simple document at four threads it returned pixel-perfect tiles six times out of six,
and a developer who tested exactly that would have concluded threads were fine. The same
configuration crashed on the second round of a longer run. A race that usually wins is
indistinguishable from correct code until it is in front of a user with a CAD drawing ---
the same shape as the sandbox that rendered `ok` with a substituted font, and the crash
test that compiled away.

This is the **second** time this entry has been wrong. An earlier version claimed PDFium
was unsafe only "per document handle" and that multiple handles would render in parallel;
that was caught in the 2026-07-26 audit. Both errors came from trusting a dependency's
prose over its behaviour. The rule that would have caught either: **a claim about a
library's concurrency is a measurement, not a citation.**

What worker processes buy depends on the *shape* of the work, and there is no single
scaling factor to quote. One tile from each of many pages of the text corpus: **3.89x on
four** workers, then about 0.4x per further worker (spike 0.5). Six tiles of one A0 page,
which is what a viewport actually asks for: **2.56x on four, 3.22x on six, and nothing at
eight** (2026-07-27, `worker-bench --mode parallel --grid 3`). Same machine, same tile size.

So "size the pool from performance cores, not `hw.ncpu`" was drawn from the first workload
and does not survive the second --- the plateau there is at six on a 4P+6E machine. Measure
the workload you have; a speedup measured across documents does not predict one measured
across tiles, and the difference is large enough to change an architectural decision.

### A worker process is nearly free; the webview boundary is not

Measured 2026-07-26 (`worker-bench --mode latency`). A control round trip to a worker ---
write a JSON line, wake it, read a JSON line --- is **6 µs**. Moving a 4 MB tile out of it
costs **0.11 ms through shared memory** and 0.61 ms down a pipe. The shared-memory figure
is indistinguishable from the in-process residual, and one worker matches the in-process
baseline for throughput exactly.

Two things make it that cheap, and both are worth keeping:

- **PDFium renders straight into the shared mapping** via `PdfBitmap::from_bytes`, so
  there is no copy on the worker side at all. `as_rgba_bytes()` would allocate and copy a
  second 4 MB; the BGRA-to-RGBA swizzle is 0.27 ms and is better done in place.
- **The mapping is an unlinked temp file passed by descriptor**, `dup2`'d to a fixed fd
  before `exec`, not a `shm_open` name. A descriptor has no name to guess and survives a
  policy that denies opening files --- which is what lets the worker be sandboxed at all.
  Note `dup` both sources to fresh descriptors before `dup2`ing them down: the parent's own
  mapping files typically land on exactly the fd numbers being targeted.

Put next to §3's other measurement --- 3.0 ms to hand the same 4 MB to the webview --- the
process boundary costs about 1/27th of the UI boundary. Isolation is not where the time
goes.

### macOS has no memory rlimit, and `RLIMIT_CPU` is a lifetime budget

Measured 2026-07-26, and confirmed independently through Python's `resource` module:
`setrlimit` on macOS refuses `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` outright with
`EINVAL`. There is no address-space or heap bound available this way at all. `RLIMIT_CPU`,
`RLIMIT_NOFILE` and `RLIMIT_FSIZE` are accepted and do fire.

The subtler half: `RLIMIT_CPU` counts CPU consumed over the **process lifetime**, not per
request. Under a 3 s limit a 1.72 s render succeeds and the next one dies 1.30 s in, at a
cumulative 3.0 s (SIGXCPU, signal 24). So it bounds how long a worker may live, and a
per-render bound has to come from the parent's own deadline and kill --- measured at 1.2 ms
to kill and reap, 4.8 ms to respawn.

Always read a limit back after setting it. A limit the kernel accepts and never enforces is
worse than none, because it reads in the source as a bound that exists.

### Polling a child's footprint bounds a leak, not a burst

The substitute for the missing memory rlimit is supervision: the parent samples the worker's
`ri_phys_footprint` through `proc_pid_rusage` and kills it over budget. A sample costs
0.33 µs, so the poll interval trades overshoot against essentially nothing, and the obvious
reading is that a tight interval solves it. Measured 2026-07-26
(`worker-bench --mode footprint`), against a child allocating at ~22 GB/s and a 128 MB
budget: 1 ms polling overshoots 16 MB, 5 ms overshoots 22 MB.

The result that matters is the one that is not an overshoot at all. **At 20 ms and above,
most runs never saw the event** --- the child took its whole 512 MB burst and exited between
two samples, so supervision never engaged and reported nothing. A burst smaller than
interval × growth rate is invisible to any polling scheme, whatever the interval. Bounding
the *inputs* --- decompressed stream size, tile dimensions, pages per request --- is the
layer that catches those, and it is not optional.

Two smaller traps in measuring this. Neither the median nor the worst *observed* overshoot
is the bound; both depend on where the crossing falls between two samples, so a budget must
be set from interval × rate. And a zero interval is not free supervision --- it burns a core,
and the low overshoot it appears to buy is partly the child being starved of the CPU it was
allocating with.

Use footprint, not RSS: a footprint excludes clean file-backed pages, so a worker with a
337 MB document mapped is not charged for it, and an RSS bound would kill a worker for
reading its own input.

### `proc_pid_rusage` takes the struct's address, not a pointer to it

`rusage_info_t` is itself `void *`, so the declared `int proc_pid_rusage(int, int,
rusage_info_t *)` reads as taking a pointer to a pointer and does not. Every caller in the
SDK writes `(rusage_info_t *)&info`, passing the struct's own address.

Passing the address of a pointer instead **type-checks cleanly in Rust, returns 0, and has
the kernel write the whole struct over whatever follows that pointer on the stack.** The
only symptom was a footprint that read as zero for every child, which looks exactly like a
permissions problem and sends you off to check entitlements. Note also that `RUSAGE_INFO_V0`
is the oldest flavour carrying `ri_phys_footprint`, so it is the one least likely to shift
under a macOS update.

### The vendored PDFium has no JavaScript engine and no XFA --- verify it, do not assume it

`bblanchon/pdfium-binaries` ships a build with **zero `v8::` symbols**, no real
`CJS_Runtime` (only `CJS_RuntimeStub`, whose `ExecuteScript` disassembles to three
instructions that zero the output and return), and **zero `CXFA_` symbols**. So "document
JavaScript is disabled by default" understates the position: there is no engine to disable,
and the XFA refusal is a property of the binary rather than a policy that can be forgotten.

Both are properties of *that build*, not of PDFium, so they must be re-checked after every
bump --- `worker-bench --mode engine` does it. Note it cannot be tested behaviourally: a
document whose JavaScript does nothing looks exactly like one whose JavaScript never ran, so
the absence of an effect is not evidence of the absence of an engine. The symbol table is
the only thing that discriminates, which is why the check needs its own controls --- one
that the file really is PDFium, one that its local symbols survived stripping. Without the
second, every absence is "not verified", not "not present".

What this does *not* buy: `pdfium-render` calls `FPDFDOC_InitFormFillEnvironment` on every
document open, so the form-fill machinery is reachable parsing surface even with nothing
behind it.

### The no-V8 property is one word in a URL, so the fetch asserts it

`bblanchon/pdfium-binaries` publishes **both** variants of every release:
`pdfium-mac-arm64.tgz` (3.5 MB) and `pdfium-v8-mac-arm64.tgz` (13.4 MB), and the same pair
for every platform. The entry above --- and `docs/THREAT-MODEL.md`'s promotion of "document
JavaScript is disabled" to "there is no engine to disable" --- is a property of the *asset
that was fetched*, not of PDFium. Downloading the other one silently reinstates a
JavaScript engine, and nothing downstream would notice: the symbol scan is the only check
that discriminates, and it is not run on every build.

So `scripts/fetch_pdfium.py` refuses a V8 asset by name and verifies a pinned SHA256
before extracting anything. A digest alone would not have caught it --- a V8 build pinned
to its own digest passes a digest check perfectly.

The pin is `chromium/7881`. The fetched mac-arm64 archive is byte-identical to the install
every Phase 0 measurement was taken on (dylib sha256 `1bc45b15…`), which is what makes the
script a reproduction of the tested binary rather than merely a download of a similar one.

### PDFium ships its loadable library in a different directory on Windows

macOS gets `lib/libpdfium.dylib`. Windows gets the runtime DLL at **`bin/pdfium.dll`**,
and `lib/` holds only the import library `pdfium.dll.lib`. Found 2026-07-27 by a
cross-platform install test that was only meant to check a digest.

The first app path joined `vendor/pdfium/lib` unconditionally, so on Windows it resolved
to a directory containing nothing loadable. That app path is fixed and the distinction
lives once as `PDFIUM_SUBDIR` in `src-tauri/src/lib.rs`, shared by every binary as it is
run on Windows. `progressive-probe` was the third copy found afterwards (2026-07-31):
its documented command failed at `LoadLibraryExW` and told the reader to reinstall a
perfectly valid PDFium. A platform path duplicated beside a shared constant is a latent
copy of the original defect, even when the application itself is already correct.

### A sandboxed PDFium substitutes fonts silently --- and the obvious fix does not work

A worker under `sandbox_init` with files and network denied still opens the document (it
arrives as a mapped descriptor, never a path) and still renders. On a document with
**embedded** fonts the output is pixel-identical to an unsandboxed render. On a **base-14**
document it is not: PDFium returns success and draws a different face, with almost exactly
the same amount of ink --- so it is a substitution, not a blank page, and nothing in the
return value says so.

The repair that looks right is wrong. Denying `file-read*` and allowing `file-read*` back
on `/System/Library/Fonts` still renders differently, because what the font lookup needs is
**metadata** reads across the filesystem, not data reads from the font directories. What
works, verified pixel-identical on base-14, TrueType, CID and a 775-page corpus:

```
(version 1)
(allow default)
(deny network*)
(deny file-write*)
(deny file-read*)
(allow file-read-metadata)
(allow file-read-data (subpath "/System/Library/Fonts") (subpath "/Library/Fonts"))
```

The residual is that a hostile document can learn which paths exist; it cannot read one,
write one, or open a socket.

The general rule, and it is the third time this shape has appeared: **verify a sandbox by
comparing pixels, never by checking that the render returned `ok`.** Same silent
substitution as `set_text()` drawing `.notdef`, arriving from a completely different
direction.

Bisect an SBPL profile rather than reasoning about it --- `worker-bench --profiles` accepts
raw SBPL so a policy can be narrowed from the shell. Note also that `sandbox_init` denials
do not appear in the unified log without an explicit report clause, so "no log entries" is
not evidence that nothing was denied.

### Break the code on purpose, or the test suite is decoration

The first tests in this repo (the request queue and the `tile://` parser) were written, run,
and passed. That proves nothing on its own --- a test that cannot fail passes exactly like
one that can. Each was then checked by deliberately mutating the code it covers and
confirming the *expected* test went red. Two of the twenty-six were wrong, and neither would
have been found any other way:

- **A guard no mutation could fail.** `Queue::withdraw` opened with `if rid == 0 { return }`.
  Deleting it broke nothing, because `enqueue` and `claim` already keep zero out of both
  tables, so the early return was unreachable defence. It was deleted rather than kept:
  a check nothing pins is a check that can silently become wrong. The two guards that *are*
  load-bearing were each confirmed to fail a test on their own.
- **A test that probed the wrong direction.** "A query key is matched whole" asserted that
  `fmt` is not found in `xfmt`. Mutating `==` to `starts_with` **passed** --- the hazard for
  a prefix match is a key that *extends* the sought one (`fmtx`), not one that precedes it.
  The test asserted the mistake it happened to think of, not the mistake the code could
  make.

The procedure is cheap --- copy the file, apply one edit, run, restore --- and it is the
only thing that distinguishes a suite from a comfort blanket. **Write down which mutation
each test is supposed to catch, then make that mutation.** If nothing goes red, either the
test or the code is wrong, and both are worth knowing about.

Two smaller notes from doing it. Mutate one thing at a time: a double mutation showed a
test failing that a single one did not, which is how the redundant guard was found rather
than misread as covered. And expect a mutation to trip *more* tests than the one aimed at
--- that is a coverage overlap, not a problem, but the aimed-at test must be among them.

**A mutation that changes nothing looks exactly like a test that cannot fail.** Doing this
again on the viewer (2026-07-27, six mutations), one of them replaced
`this.scroller.setZoom(next)` with `setZoom(this.zoom)` --- and the line above had already
assigned `this.zoom = next`, so it was an identity. Nothing went red, which reads precisely
like a missing assertion, and the first instinct was to go and strengthen a check that was
already fine. Before concluding a test is decoration, confirm the mutation was one: print
the diff, or pick a mutation whose effect is impossible to miss.

**Aim at the control, not only at the assertion.** A second mutation there deleted the zoom
invalidation entirely. The check it was aimed at --- "recovers coverage after a zoom" ---
stayed green, correctly: coverage never dropped, so there was nothing to recover. What went
red was the *control* beside it, "a zoom step discards what it invalidates". Both outcomes
are the suite working; predicting the wrong one is a fact about the prediction. Write down
which check should notice, and treat a different check noticing as a result worth reading
rather than a miss to be tidied away.

### A property that holds by construction cannot test the thing it resembles

The viewer check asserted that text dragged out of the page appears in the page's own text:
`whole.includes(dragged)`. That reads like a check on the *geometry* --- if the character
boxes and the character indices disagreed, surely the drag would return the wrong words.

It cannot fail. A selection is a contiguous range of character **indices**, and the string it
produces is built from those indices, so it is a substring of the page's text whatever the
boxes claim. Inverting the y-flip in `text.rs` --- the exact defect the check was written to
catch --- passed all twenty checks, and the drag returned real words. They were simply the
wrong words, from the wrong part of the page.

What discriminates is an assertion that ties a **screen position** to **specific characters**:
text dragged near the top of the page must come from earlier in the page's text than text
dragged further down. Under the flip that inverts, and the check goes red.

The general form, and it is subtler than the usual "the test never ran": **before trusting an
assertion, ask what would have to be true for it to fail.** If the answer is "nothing the
code under test controls", it is decoration however relevant it sounds. Related to the query-key
test above that probed the wrong direction, but worse --- that one could fail on some input,
this one on none.

### A fixture the library itself wrote cannot tell a passthrough from a rewrite

`print.rs` hands the file over untouched when every page is wanted and nothing is rotated,
because rewriting a document in order to change nothing about it is pure risk. The test is the
obvious one: build, and assert the bytes equal the file's.

It could not fail. The fixture was written by `lopdf`, so loading and saving it reproduces it
byte for byte --- and "handed over untouched" is then equally true of a full rewrite. Both
passthrough mutations survived. The repair is in the *fixture*, not the assertion: append a
tail past `%%EOF` that readers tolerate and no serialiser emits, and the two paths separate
immediately.

The general form is one this file keeps meeting from new directions: **a round trip through
the tool under test is not a control, because the tool is idempotent on its own output.**
Anything asserting "unchanged" needs an input the code could not have produced.

### An oracle more forgiving than the thing it stands in for cannot fail

Same session, same module. `/Resources` is inheritable, so a page kept from a subset must
still reach its parent's --- lose that and the page still opens, still counts, and prints
blank. The test asked `lopdf`'s `get_page_fonts` whether the font was reachable.

`get_page_resources` collects the page's own resources **and every ancestor's, and merges
them**. A renderer does not: PDF 32000-1 §7.7.3.4 makes an inherited attribute one the page's
own dictionary *replaces*. So a page carrying an empty `/Resources` --- the exact defect the
test exists to catch, and the one AGENTS.md already records as "removes every inherited one"
--- still reports the font through lopdf, and the mutation modelling it survived.

**Before trusting a library call as an oracle, check that it is at least as strict as the
consumer you are standing in for.** A lenient parser is the right choice for *reading* real
documents and the wrong one for *judging* them, and the same crate is usually both.

### A writer and its own reader agree about a document that is wrong

The sharpest form of the two entries above, and the one that says what to do instead. Every
check on the print job read the result back with `lopdf` --- which wrote it. That cannot
distinguish *"the document says this"* from *"our writer and our loader resolve this the same
way"*, and only the first of those is a fact about the file.

The mutation that shows it: leave `/Pages /Count` at its pre-subset value after deleting pages,
so the page table contradicts its own `Kids` array. `lopdf`'s `get_pages()` walks `Kids`, so
every check reading it back saw two pages and passed. **PDFKit reported five** --- the two real
pages, followed by three blank ones it manufactured to satisfy the count:

```
Reading { pages: [ {text: "page 2"}, {text: "page 4"}, {text: ""}, {text: ""}, {text: ""} ] }
```

Out of a printer that is two correct sheets and then three blank ones. Nothing in the file is
malformed enough for anyone to complain about, and no amount of reading it back with the writer
can see it.

So `print.rs` re-reads every built job with `print_macos::read` before the panel opens, and the
three `a_third_parser_*` checks assert against that rather than against `lopdf`. **Where output
leaves the process --- a printer, another application, a file someone else opens --- at least
one check has to go through a parser that did not write it.** PDFKit is the right one here for
a second reason beyond independence: it is what the macOS print system itself uses, so it is
not a neutral third party but *the* consumer.

Note the mutation was written to test the tests, not the code: `delete_pages` updates `/Count`
correctly. The finding is what the checks are worth, not a bug.

### A canvas round trip cannot read back what a renderer produced

The check for page inversion computes the expected tile itself --- lightness inversion has a
closed form --- and compares it against what the renderer returned. It failed on 17% of the
bytes, reporting `wanted 255, got 0`, which reads unambiguously as a renderer that did not
invert something.

The renderer was right. An `ImageBitmap` drawn onto a 2D canvas and read back with
`getImageData` is **premultiplied by alpha**, so every pixel with alpha 0 comes back as
`[0,0,0,0]` whatever colour it carried --- and a square tile of a portrait page is about a
sixth transparent margin. The oracle inverted the *read-back* black-and-transparent to white,
the renderer had genuinely produced white-and-transparent, and the comparison failed on a
difference that existed only inside the decode.

The fix is to read the bytes off the wire: `fetch(tileUrl(...))` and `arrayBuffer()`, no
canvas anywhere. The claim under test is what the renderer returns, so the wire is where to
test it --- and a decode in the middle of a comparison means comparing through a transform
nobody asked about.

Two things worth carrying. **A comparison against an oracle needs the oracle and the subject
to be read the same way**, and "the same way" has to include every lossy step, not just the
obvious ones. And the diagnosis was cheap only because the failure was made to print the
actual pixel: `wanted 255, got 0` sends you to the renderer, `pixel 116 was [0,0,0,0]` settles
it in one run. Same lesson as dumping the cancelled tiles and looking at them --- **when a
numeric comparison fails, print the values before theorising about the producer.**

### A documented count that is one sample of a race makes an honest run look like a defect

`BUILD.md` recorded what each corpus reports from the viewer check as a fixed table --- for
`text-heavy`, `65 | 10`. A routine sweep then returned 64 ran and 11 skipped, every check
green, and that read as a regression: one check had started skipping, which is precisely the
failure the table exists to surface.

Nothing had changed. One check --- "the strip withdraws its work when the viewer needs the
renderer" --- depends on whether a thumbnail is still in flight when the viewer asks for a
tile, and on that corpus a thumbnail takes about a millisecond. Three runs measured 65/10,
65/10, 64/11. The code says so in its own comment; the table did not.

The cost is not the twenty minutes. It is that the obvious repair is to make the check stop
skipping, which would mean deleting the condition that keeps it honest --- an outstanding
request is what makes a withdrawal observable, and without one "nothing is outstanding" is
true for reasons that have nothing to do with the mechanism. **A number chased back to a
documented value is a defect introduced to satisfy a document.**

So: when recording expected output, state a range and name what varies, or do not record it.
The invariant worth asserting here was never the split --- it is that all **75 check names
appear**, skipping with a reason rather than vanishing, and that one holds on every run.

### A dependency that refuses your test input makes your own guard look redundant

`path_from_url` checks `url.scheme() != "file"` before calling `Url::to_file_path`, and the
test for it used `https://example.com/a.pdf`. Delete the guard and that test still passes ---
so by the standing rule, a guard no mutation can break is a guard to delete.

Deleting it would have been wrong. `Url::to_file_path` refuses that URL for a reason that has
nothing to do with the scheme: its host is a domain. Read its source and the match arm is
`None | Some(Host::Domain("localhost")) => None` --- a **`localhost` host is treated as no
host at all, whatever the scheme**, and the path is then built from the segments. So
`https://localhost/a.pdf` becomes `/a.pdf`, and the scheme check is the whole of what stops a
URL handed over by another application from naming a local file. The test probed the one
direction the guard does not defend, and the dependency's own refusal covered for it.

Same shape as the `fmt` / `xfmt` query-key test, with an extra layer: there the wrong
direction was ours to see, here it was a library's behaviour on an input we chose. **When a
mutation leaves a guard standing, read what is refusing the input before concluding the guard
is redundant** --- "something rejects this" and "this guard rejects this" are different facts,
and only the second one is about the code being deleted.

The repair is a second case (`https://localhost/a.pdf`) rather than a different one: both are
kept, because the first still documents the ordinary rejection and the pair brackets where the
responsibility actually lies.

### A defect that switches off a check's precondition is not caught by that check

The sharpest form yet of "a test whose precondition is already satisfied never runs", and
worse, because here it is the **defect itself** that arranges the precondition.

The page strip builds only the rows its panel can show. The check for it read: if the strip
built every row, skip --- *all the rows fit in the panel* --- otherwise assert it built some.
Perfectly sensible, and then the mutation that deletes windowing entirely, so that `layout`
builds every row in the document, makes the check report `[SKIP]`. It does not go red.
Nothing goes red.

The fault is that the skip condition was derived from the same quantity being asserted. The
repair is to derive it from something the defect does not control --- here the panel height
and the row height, which give the most rows that *could* be on screen --- and then assert
against that bound rather than against the strip's own output.

**Ask what a check does when the thing it checks is broken, not only when it is absent.** A
skip is a third outcome, and a defect that reaches it is as invisible as one that passes.

Worth recording how it surfaced, because it is the cheap half of the procedure: it was found
while **writing down which check each mutation should turn red**, before running any of them.
The check had already been written, reviewed and run green on five corpora. Stating the
prediction is what exposed that there was no prediction to make.

Two earlier versions of the same check were wrong in the other direction, which is worth the
sentence because the pair brackets the mistake. The first estimated the row height at 60 px
and skipped on the twelve-page document whose rows are 186 px --- so the only fixture that
could exercise windowing declared itself inapplicable. The second asked the strip how many
rows it had built, which is exactly the quantity the defect controls.

### An "already have it" cache needs an in-flight set, not just the cache

The strip borrows a page's bitmap from the viewer's tier-1 cache when it has one, and copies
it with `createImageBitmap` --- which completes in a microtask, not immediately. The guard
was "is this page in `bitmaps`", and between starting the copy and finishing it the page is
in neither the cache nor the outstanding-request slot. Every scroll, resize and position
change in that window starts the *same* borrow again.

It read as twelve borrows on a twelve-page document with seven rows on screen. Every one of
them succeeded and the pictures were correct, so the only symptom was a number in a detail
column that looked slightly too round. What pins it is `borrowCount <= renderedCount` --- the
*upper* bound. The check said `borrowCount > 0` first, which is the obvious way round, and
under this defect that passes **harder**: more duplicates make it more true.

**A "do we already have it" test is not a "is one already on the way" test**, and any cache
filled asynchronously needs both. Same shape as the request queue's `queued`/`inflight`
split, arriving from a direction that did not look like a queue.

### A mutation harness needs the same control as the thing it is testing

The script driving those mutations rebuilt the app, ran the check, and looked for `[FAIL]`
lines. A run that never produced a report --- a crash, a timeout, a lock screen --- has no
`[FAIL]` lines either, so it printed *"nothing went red at all"*, which is exactly what a
mutation nothing noticed looks like. Two of five results were unreadable for that reason.

It now requires the summary line (`N/M checks passed`) to be present and reports a missing
one as a broken run, distinct from a surviving mutation. **A harness that reads absence as
evidence needs to distinguish absence from silence** --- the same failure as the leak scanner
that could not decode a Type0 font, one level up.

### A timeout that discards the transcript recreates the failure it was added to diagnose

`viewercheck.ts` prints each result as it is recorded, specifically so that a run stopping
midway can say *where* --- that entry is above. `viewer_check.py` then captured the child's
output and, on timeout, **printed the verdict and threw the transcript away**. So the one
failure mode the streaming was added for produced a single line, `[FAIL] run timed out`,
which is character-for-character the shape of a page that never executed any JavaScript.

Both were seen on the same corpus within an hour, which is what makes it worth an entry: the
fix upstream is undone by any consumer that buffers. `subprocess.TimeoutExpired` carries
`.stdout`; print it, and say how many checks got in.

The timeout itself was the other half. 300 s against a corpus that takes **276 s** is not a
bound, it is a coin toss --- the same run passed and timed out on consecutive attempts. A
timeout on a check that cannot wedge quietly exists only to stop an unattended run hanging
forever, so it belongs far clear of the slowest case, not next to it.

### A text comparison cannot see a property that is not about text

Two defects in the *checks* for the screen-reader layer, found within minutes of each other,
same shape both times.

**`textContent` concatenates block elements with nothing between them.** Comparing a page's
accessible text against an independent extraction failed at 2,562 characters against 2,618
--- exactly one missing separator per line on a 56-line page. The content was identical; the
comparison had flattened away the block structure that carried the line breaks, and then
reported the absence of that structure as a content mismatch. Join the blocks (or read
`innerText`, which respects layout) when the thing being compared is what a reader hears.

**`display:none` and `visibility:hidden` remove an element from the accessibility tree, and
every text assertion still passes.** A screen-reader layer hidden either way is completely
inert while reading perfectly correct through `textContent`. The visually-hidden idiom has
to be the clipping form (`width:1px;height:1px;clip-path:inset(50%)`) precisely because
those two are not equivalent to it, and that is worth its own assertion: the container must
be 1x1 *and* neither `display:none` nor `visibility:hidden`. Without it the whole layer is
one CSS edit away from doing nothing, with no check able to notice.

### Restoring a mutated file by *moving* a backup over it tests the mutated binary

The harness driving the `search.rs` mutations copied the file, mutated it, ran the tests,
and put the backup back with `shutil.move`. A move carries the backup's metadata --- so the
restored file's mtime was the moment the *copy* was taken, older than the artifact cargo had
just built from the mutated source. Cargo compared timestamps, concluded nothing had
changed, and ran the suite against the mutation. The confirmation run afterwards reported a
failing test that was not in the tree, on a file whose contents were provably correct.

The mutations themselves were unaffected --- each was written with `open(path, "w")`, which
stamps the current time, so every one of those runs did rebuild. It is only the restore that
went backwards in time, which is the half nobody thinks to check.

Two rules, and the second matters more: **restore by writing the bytes back, never by moving
a file over them**, and **verify the restore by re-running the suite and requiring green**.
A harness that can leave the tree in a state its own results do not describe is worse than
no harness, because the next real failure reads as another restore artifact.

The same harness then failed a second way, which is worth the sentence because the mechanism
is so ordinary. It found failure lines with a regex expecting **two** spaces between the
check's name and its detail — and the names are printed with `padEnd(40)`, so a name of
exactly 41 characters is followed by one space and matched nothing. That mutation was
reported as a **survivor** while the summary line in the same output said 28 of 29 checks
passed. Two numbers in one buffer disagreeing, and nothing comparing them. **When a harness
can derive the same fact two ways — a parsed detail and a printed total — make it check that
they agree**, because the parse is the half that breaks silently.

**It recurred on 2026-07-28, in a harness written by someone who had just read this
paragraph.** The retirement mutation harness used `^\[FAIL\] (.+?)\s{2,}` against a probe that
pads names to **56**; the check "a worker idle for less than its timeout survives a sweep" is
55 characters, so one space followed it and the regex matched nothing. The mutation it was
aimed at was reported `SURVIVED` — the single most misleading verdict a mutation pass can
produce, since it reads as a gap in the tests — while the summary line four lines further down
in the same buffer said `37/41 checks passed, 3 skipped`, which is one failure.

Two things are worth taking from the repetition rather than from the incident. The lesson that
failed to transfer was not "beware regexes"; it was the **repair**, which is mechanical and
was simply not implemented: derive the count both ways and refuse to report either when they
disagree. A rule stated as a caution gets nodded at, and a rule stated as a line of code gets
written. And the padding width is not a constant anyone remembers — so the parse must not
depend on it at all. Split on the marker (`[FAIL] `) and take the rest of the line.

### A test whose precondition is already satisfied never runs

The sharpest instance of the shape above, and the fourth in this project. `viewercheck.ts`
pressed **End** on a 775-page document and then waited for full sharp coverage before
declaring "covers the last page". It passed instantly --- because the *first* screen was
already fully covered, so the predicate was true before the jump had rendered a single tile.
Its own detail line said so: `sharp=100.0% on page 1/775`.

The fix is a control that asserts the **drop** before the recovery: one frame after the jump,
coverage must be *below* threshold. With that in place the same check reads
`sharp=100.0% on page 775/775`.

The general rule: **an assertion that something recovered is worth nothing unless something
was first shown to have broken.** Any test of the form "do X, then wait for the good state"
needs to establish that the good state was not already there --- and the seam is invisible,
because a check that returns immediately and a check that waited both print `[OK]`.

Note the control is not always applicable, and that has to be visible too. On a one-page A0
sheet **End** moves 488 px and the tiles on screen stay valid, which is correct behaviour;
the check reports `[SKIP]` with the reason rather than silently vanishing, because a control
that disappears on some inputs is indistinguishable from one that ran.

### A crash test that compiles away proves containment of a crash that never happened

`worker-bench --mode crash` originally faulted with `null_mut::<u8>().write(1)`. That is UB
the optimizer is entitled to delete, and in release it did: the process exited normally
through the fallthrough arm, the parent reported clean containment, and the run looked like
a pass. The tell was the epitaph --- "exited with code 9" where a segfault should have said
"killed by signal 11".

Route the address through `std::hint::black_box` and use `write_volatile`. More generally:
a test whose failure mode is *not failing* needs its own assertion on how it failed, not
just on the outcome.

### `AppHandle::exit` does not set the process's exit code

`app.exit(1)` ends the event loop; `App::run` then returns normally, `run()` returns, `main`
returns unit, and **the process exits 0**. So every automated run in this repository
reported success for its entire existence, whatever it printed --- including
`scripts/viewer_check.py`, whose closing `return completed.returncode` could not fail.

Nothing caught it because nothing looked. The mutation harnesses parse `[FAIL]` lines out
of the transcript rather than reading `$?`, which is why the mutation results they produced
were nonetheless correct; and a human reading a transcript sees the failures directly. The
exit code was the one consumer with no second opinion, and it was wrong for months.

What surfaced it was a run printing `[FAIL]` and `0/1 checks passed` immediately above its
own harness verdict of `[OK] session restore verified` --- two numbers in one buffer
disagreeing, with nothing comparing them. That is the same defect as the `search.rs`
harness whose regex silently matched nothing, one level up, and the same repair applies:
**when a harness can derive the same fact two ways, make it check that they agree**, and
treat a disagreement as a broken run rather than as either answer.

`spike_exit` now flushes and calls `std::process::exit(code)` directly. Verify a fix like
this in **both** directions --- a failing run must exit non-zero *and* a passing one must
exit zero. Only one of those was ever in doubt, and checking just it would have been happy
with `exit(1)` unconditionally.

### `RunEvent::Opened` fires before the setup hook, so managed state is not there yet

A macOS double-click does not put anything in `argv`. Launch Services sends an Apple Event,
Tauri surfaces it as `RunEvent::Opened`, and **it arrives before the setup hook runs**. So a
handler that reaches for state registered in `setup` --- the obvious place --- calls
`state::<T>()` on unmanaged state, which **panics**, on precisely the path it was written to
serve.

The symptom is not a crash dialog at the time. The window appears, nothing is in it, no
error reaches stdout or stderr, and the last startup mark is `app built`. macOS records it
and offers to reopen windows on the *next* launch, which is how it surfaced --- a day later,
in a dialog. `~/Library/Logs/DiagnosticReports/<app>-*.ips` has the truth: `EXC_CRASH
SIGABRT`, `Abort trap: 6`, main thread.

Register anything the run callback touches with `Builder::manage`, before the event loop
exists, and read it with `try_state` --- a panic here is invisible, where a `None` costs one
document not opening.

Note what this does to the ordering advice already in this file. `Builder::build()` does not
run the setup hook and `App::run` does; `Opened` is dispatched by that same `run` *before*
setup. So "before the setup hook" is a strictly larger set of moments than it looks.

**Two causes were producing this one symptom, and the second nearly buried the first.**
Windows left over from testing were occluding new ones, so unrelated runs *also* produced no
output --- and `TPDF_RAISE=1` "fixed" those, which read as the whole problem being
environmental. A missing `tauri setup` mark was then read as "the setup hook never ran",
which was true and not the cause. Two mechanisms, one silence: fix the one you can prove and
re-measure, rather than accepting the first explanation that covers the evidence.

**The harness was the last thing to doubt, and should have been.** Four phases passed and one
produced nothing, so `open --stdout` looked like the culprit --- plausible, since `open`
detaches. What settled it was abandoning stdout and asking a different question: does the app
write its session file after a double-click? It did not. That turned "the capture is broken"
into "the feature is broken" in one command. **When a check reports a failure it alone can
see, find a second channel that does not share its machinery.**

### A test for an atomic write must plant the intermediate it is meant to prove

`Session::save` writes a scratch file and renames it over the target, so that a crash
mid-write leaves the previous session rather than a truncated one. The obvious test --- save,
then assert no scratch file is left behind --- **passes identically for a save that writes
the target directly**, because a direct write produces the right bytes and leaves no scratch
file of its own to find. Every other test in the module passes too: they all read the
result, and the result is the same.

What discriminates is planting a stale scratch file *first*, as a write that died would have
left, and asserting it is gone afterwards. Only a save that renames over it removes it.

The general form, and it is the one this repository keeps rediscovering: **when the property
is "how the result was produced", no assertion about the result can test it.** Find the
intermediate state the mechanism must pass through and assert on that. Same shape as the
crash test whose failure mode was not failing, and as the leak scanner that could not decode
its own carrier.

It was found by writing the mutation down before running it --- "the save writes the target
directly, with no rename" had no check to name, which is the cheap half of the procedure
doing its job for the second time in this project.

### A control can be contaminated by the phase that ran before it

The session check launches the app four times, two of them controls: "the app does not open
in the remembered state by itself", and "nothing opens when nothing is remembered". Both
were pointed at one scratch session file, on the reasoning that both wanted it empty.

It is not empty by the time the second one runs. The *first* control opens a document ---
that is what it is for --- and the app dutifully remembers it. So the second control
launched with a document to restore, restored it, and failed.

The standing rule about interleaving covers this ("before trusting an A/B, ask what each
variant leaves behind that the next one can find"), and it did not fire, because this did
not look like an A/B at all --- it is a control, and a control is exactly the thing assumed
to be inert. **A phase that writes state needs its own copy of that state, not a shared one
that happens to start empty.** The two controls now get a file each.

Worth noting the failure was legible only because the control existed: a check that had
merely asserted "the remembered state comes back" would have passed the whole run.

### A locked macOS session cannot be unlocked from a script, so it must be prevented

WebKit suspends a page whose window is not visible, and a locked screen occludes every
window --- so a frame-rate benchmark behind one does not run slowly, it does not run at all.
`scripts/scroll_bench.py` already refuses to start in that state rather than hanging.

There is no supported way to unlock the session programmatically; the only mechanisms are a
typed password, Touch ID or a paired watch, all of which need a person. The workaround that
exists in the wild --- storing the login password and having `osascript` type it at the lock
screen --- puts that password in a file readable by anything running as the user, and is not
worth the convenience.

So the only lever is prevention, and the trap is that per-run prevention is not enough.
`scroll_bench.py` holds `caffeinate -du` for **its own lifetime**: the gaps between runs are
unprotected, and a long headless bench running alongside it holds nothing at all. That is
how a session locked mid-batch here. Wrap the whole batch:

```sh
caffeinate -du bash -c '<run> ; <run> ; <run>'
```

`-u` as well as `-d`, because `-d` only stops a display going idle and will not turn one
back on that is already off.

### A raw `cargo build` binary runs no webview content at all

**`src-tauri/target/release/tpdf` opens a window and never executes a line of
JavaScript.** No error, no crash report, no console output --- a blank window and a page
that never loads. The same code inside the bundle
(`target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`) works perfectly. WKWebView
needs the bundle identity; a bare Mach-O has none.

This cost most of an afternoon on 2026-07-27, and the reason it was not obvious is that
every other harness in this repository runs a raw binary happily --- `outline-probe`,
`text-probe`, `worker-bench` and the rest are all plain executables with no webview in
them. Only the four `TPDF_*` spike entry points need a page to run, and `BUILD.md` gives
the bundle path in its example while the prose beside it says the check "does not require
a release bundle". Both statements are true and together they are misleading: it does not
require a *release* build, it requires a *bundle*.

So: **any run that needs the webview needs the `.app`.** `npm run tauri build -- --bundles
app`, then the executable inside it --- which keeps stdout and the environment, unlike
`open -a`.

The diagnosis that led there is worth as much as the fact. Chasing it produced two wrong
answers first, and the thing that settled it was building **HEAD itself** and watching it
fail identically. An earlier control that reverted only the frontend was not enough: the
Rust changes were still in the binary, so "it is not my frontend" had been established and
silently generalised to "it is not my code". **A control has to cover everything that
changed, not the part currently under suspicion.**

### A page that never ran looks exactly like one that ran slowly

The symptom shared by the entry above and the lock-screen one: **no output, 0% CPU, the
process alive.** It reads as a hang in whatever was most recently changed, and on
2026-07-27 that reading was confidently wrong three times in a row --- first blaming the
new code, then a broken frontend bundle, then window occlusion. The actual cause was the
raw binary, above.

Note especially that the occlusion theory was *stated as a conclusion* while it was still a
guess, and it survived longer than it should have because it is a real WebKit behaviour
that produces the identical symptom. **A plausible mechanism that explains the evidence is
not the same as the mechanism, and neither is one that has an entry in this file.**

Two repairs came out of it, both worth keeping whatever the cause turns out to be next
time:

- **The watchdog names the condition.** Every spike entry point begins by asking Rust for
  its path or config, so the first of those calls proves the page ran; `spike_env` records
  it as a `webview alive` mark. When the watchdog fires without that mark it says the page
  never ran a line of JavaScript, instead of printing a mark list that has to be
  interpreted. Both halves are now verified: it fires on a raw binary and stays quiet on a
  bundled one.
- **`viewercheck.ts` prints each result as it is recorded.** Buffering the report until the
  end meant a run that stopped midway printed nothing at all --- indistinguishable from one
  that never started, which is precisely the state being diagnosed.

`TPDF_RAISE=1` also exists now, and raises the window for a run that has nowhere visible to
put one. It did **not** fix this, and is kept because occlusion is nonetheless real. Opt-in
rather than the default, because raising a window over whatever someone is doing,
every time a check runs, is its own bug --- the scroll benchmark raises unconditionally
only because an unfocused window would falsify its numbers.

### A harness that prints only at the end cannot say where it stopped

Found by the entry above, and the reason it took an afternoon. `viewercheck.ts` collected
every result and printed them in one block at the very end, so a run that did not reach the
end printed **nothing at all** --- identical to a run that never started, and identical to
a run whose first line of code never executed. The only fact available was that the process
was alive.

It now prints each result as it is recorded. The lines are chained through one promise
rather than awaited at the call site, so `check()` stays synchronous and the transcript
cannot arrive shuffled --- `invoke` resolves out of order under load, and out-of-order
results are worse than late ones.

The general form, and it is the same shape as the crash test that compiled away one level
up: **buffering a report until the end makes every partial failure look like the same
failure.** If a harness can stop midway, it has to be able to say where.

### A mean cannot test a claim about a minimum

`docs/PLAN.md` §9 requires that the visible page area is **never** below its tier-1
placeholder. The scroll benchmark reported the mean coverage across frames, it printed
`100%`, and that was very nearly written up as the criterion being met.

It is not evidence. A mean that rounds to 100% over 300 frames is entirely consistent with
one frame that showed nothing --- which is precisely and only the thing the criterion
forbids. The statistic could not express the failure, so it could not test for it, and a
pass read off it would have been an assertion wearing a number.

The fix is a `floor` column: the worst single frame, of the worst round, not the mean of the
per-round minima --- averaging minima hides the bad round behind the good ones exactly as
averaging frames hid the bad frame. It reads 100% on both corpora, so the conclusion did not
change; what changed is that there is now something behind it.

**Match the statistic to the quantifier.** "Never" and "always" need a min or a max; "95% of
the area" needs a mean; a tail requirement needs a percentile. A number that cannot go bad
when the property does is decoration, and it is the same failure as the crash test that
compiled away and the leak scanner that could not decode its carrier --- three different
subjects, one shape.

### A frame-rate pass means nothing without a coverage number beside it

Spike 0.8 scrolled the A0 vector page at a flawless **60 fps, zero dropped frames, in every
variant** — over a screen that was **0–4% sharp**, and at 400% zoom showed nothing at all
for about a fifth of its frames. Both statements are true of the same run. The frame loop
is decoupled from the renderer on purpose, so the thing a frame-rate test measures is the
compositor, and a scroller that has given up entirely posts the best numbers in the table.

So any smoothness measurement needs a second metric asserting something was *on screen* —
here, the fraction of visible page area backed by a sharp tile, plus a second fraction
counting the tier-1 placeholder, since "blurry" and "blank" are different failures. Same
lesson as the crash test that compiled away and the sandbox that rendered `ok` with the
wrong font: **assert on what was produced, never on the absence of a complaint.**

The harness itself failed this way first, and more sharply. Clearing tier 2 *after* the
warm-up — to guarantee the timed section scrolled over unrendered content — left the
warm-up's four requests outstanding but invalidated, so against a one-second-per-tile page
the in-flight limit was never released and the timed section could not issue a single
request. It reported a perfect 60 fps over a document it had not asked for. The tell was
`requested: 0`, which is why the counter is in the output at all.

### Interleaving controls for drift, not for what the last variant left behind

The standing rule for a performance comparison is to interleave A,B,A,B and compare
pairwise, because wall clock drifts over minutes. That defends against the machine changing
underneath the measurement. It does **not** defend against variant A leaving work in a
shared, stateful component that variant B then inherits — and when the component is a
single FIFO render thread whose backlog outlives a round, that is the larger effect by far.

Measured 2026-07-27 comparing withdrawal of stale tiles against leaving them to render. On
the A0 sheet, whichever variant ran **first** reached 100% tier-1 coverage and the better
sharp figure; whichever ran second reached 75% and the worse one. Swapping the order swapped
the result exactly, so both orderings "showed" that the first variant was better. Reading
either run alone gives a confident, reproducible, wrong answer — and the pairwise comparison
the interleaving rule prescribes is what produces it, because the pairs are always
first-then-second.

The fix is to drain between variants, not to average across orderings: `scrollbench.ts`
waits for a variant's outstanding requests to settle before the next one starts, bounded,
and shouts if the bound is hit. With that in place the two variants measured identically at
a slow scroll and reproduced within a tile at a fast one, in both orderings.

The general form: **before trusting an A/B, ask what each variant leaves behind that the
next one can find.** Caches, queues, background threads, page-cache state, a warmed JIT. If
the answer is "something", run the orderings both ways as a control — if they disagree, the
harness is measuring the order.

### WKWebView presents at 59 Hz on a 120 Hz display

Measured 2026-07-26 on the M5 MacBook Pro, whose panel reports a `120.00Hz` mode: an idle
`requestAnimationFrame` loop returns a **17.0 ms median interval**. Ruled out, each with
its own control in the benchmark's header — mains power (re-measured on AC), a
non-visible page (`document.visibilityState`), and an unfocused window
(`document.hasFocus()`, with the app raising itself via `set_focus()` before the run).

Two consequences. tpdf's scroll budget is **16.7 ms, not 8.3 ms**, so frame numbers here are
against the easier of the two cadences this hardware can present at — and tpdf cannot scroll
as smoothly as a native app on the same machine, whatever it does. It is the shell floor
again, in the time domain.

Never assume 60 Hz when deciding what a dropped frame is. Time an idle loop first and state
every threshold as a multiple of what it returned; a threshold derived from an assumed
cadence reports drops that are not drops, or misses the ones that are.

### `performance.now()` is clamped to 1 ms — average, do not take a median

The webview clamps the clock, which is visible in the integer-valued series spike 0.1 saw.
For anything smaller than a millisecond this is fatal to the obvious statistic: our
per-frame scroller work costs ~0.1–0.6 ms, so *every individual sample* reads as 0 or 1, and
the median is only ever one of those two values. The **mean over hundreds of samples**
recovers a usable figure from a clock that cannot resolve one, and a rate taken across the
whole run (frames ÷ elapsed) is immune to the clamp entirely.

Probe the resolution rather than assuming it — spin until the value changes — and print it,
so no claim finer than the clock's step is made by accident.

### Never benchmark through `tauri dev` without `--release`

`tauri dev` shells out to `cargo run` in the **dev profile**. PDFium arrives as a prebuilt
optimized dylib, so it is barely affected --- but every line of our own Rust is
unoptimized. The result is not uniformly slow, it is *selectively* slow, which is worse:
ratios between our code and PDFium's are inverted rather than merely inflated.

Concretely, 2026-07-26: PNG encoding of a 1024² tile measured **67 ms** under `tauri dev`
and **1.41 ms** under `tauri dev -- --release`, a 48x difference, while the PDFium render
alongside it moved only 1.39 -> 1.36 ms. Read naively, the debug run said encoding cost
4813% of rendering; the truth was 91%. A conclusion had already been drafted from the
first table.

Also note `tauri dev` runs a bare `cargo run`, which fails with "could not determine which
binary to run" as soon as the crate has more than one bin. `default-run = "tpdf"` in
`[package]` fixes it.

**Startup timing is worse still: `--release` is not enough, it needs a bundle.** Under
`tauri dev` the frontend is served by a Vite dev server over HTTP, so a startup measurement
describes Vite's module graph, not the app. Build with
`npm run tauri build -- --bundles app` and run the executable inside the `.app` directly
(`target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf`) --- that keeps stdout and the
environment, which `open -a` does not.

### PDFium pays a large fixed cost *per render call*, not per page open

Measured 2026-07-26 (`src-tauri/examples/tile_bench.rs --mode single`). On a complex page
--- an A0 sheet with ~200k path segments --- every render call pays roughly **1 second**
before any area-proportional work, and this is charged per *call* even when the same
`PdfPage` object is held across all of them. Rendering that page to a 150 px-wide
thumbnail (1/270th of its 1x pixels) still costs **1.52 s**; a single 256x256 tile costs
0.98 s; the full page costs 22.8 s at 1x and 48.4 s at 2x.

PDFium does cull spatially --- a 256² tile costs 4.3% of the full page while covering 0.8%
of its area --- so tiling is not futile. But the cost does not approach zero as the
request shrinks, which has three consequences worth stating plainly:

- **Small tiles are a trap.** Tile count multiplies the constant. Covering a 1920x1080
  viewport of that page takes 39 s in 256² tiles and 18 s in one 2048² tile. Prefer the
  fewest, largest tiles the memory budget allows; 1024²--2048², not 512².
- **A cheap low-resolution placeholder is not cheap** on the pages that most need one.
  Render it once at document open, off the critical path.
- **Any "just re-render it smaller" fallback does not help.** Scale is the wrong lever;
  spatial extent is the only one that moves the number.

Measure with interleaved variants (A,B,A,B across rounds, compared pairwise within a
round). Round 0 is a consistent warm-up outlier on cheap pages --- 0.895 against a 0.207
steady state in one text-page series --- and vanishes once renders take seconds.

### Startup has three regimes, and two of them are the OS, not us

Measured 2026-07-26 on the M5 MacBook Pro, release bundle, 775-page document, first page
presented (`scripts/startup_bench.py`):

| regime | to first page | pre-`main` |
|---|---|---|
| warm | 374 ms | 4.2 ms |
| cold page cache (`--purge`) | 1562 ms | 217 ms |
| cold + never-seen binary | ~1.8 s | 444 ms |

Two separate OS costs are hiding in there, and they are additive:

**Paging the frameworks.** `main` to the Tauri setup callback is 142 ms warm and **951 ms
cold**. That single interval is the whole cold penalty. Nothing of ours runs in it; it is
Tauri and WebKit being read off disk. Cold start is therefore mostly a function of how much
framework code is *linked*, not of what the app does.

**Validating an unseen code signature, ~300 ms, charged per binary identity.** First launch
of a fresh build spends 444 ms before `main`; relaunching the same binary spends 4.2 ms.
Copying the bundle to a new path --- same bytes, same warm page cache, only the file
identity is new --- reproduces it at 299 ms, and relaunching *that* copy costs 4.8 ms.

Consequences worth keeping:

- **A cold-start budget cannot cover the signature cost,** because it is spent before any of
  our code runs. State startup targets as warm and report the other two regimes separately.
- **It recurs on every update,** not just on install --- a new build is a new identity.
- **It makes "first run after build" a useless benchmark sample.** Every rebuild pays it, so
  the first run of a series is systematically ~300 ms slow for reasons unrelated to the
  change being measured. Discard it or report it separately.
- **Do not read a cold number as an I/O problem to cache away.** Page geometry enumeration
  costs 86.5 ms cold against 85.9 ms warm --- identical, i.e. pure CPU. Compare the two
  columns before concluding anything about where time goes.

Note `--purge` needs root, so an unattended cold run needs sudo credentials arranged for
`purge` on the machine doing the measuring.

### The shell floor is ~250 ms, and no lever on our side moves it

Measured 2026-07-26 with interleaved variants (`scripts/startup_bench.py --variant`), three
runs of 8 rounds. Of a 368 ms warm start, the first line of application code runs at
~247 ms, and that interval is seven Tauri/WebKit intervals of which **none is ours**: 4 ms
dyld, 30 ms `Builder::build()`, 25 ms `App::run` prologue, 78 ms
`WebviewWindowBuilder::build()`, 58 ms HTML load, 48 ms to the first completed IPC.

The two obvious levers were tested and neither is one:

- **Deleting the entire frontend framework is worth nothing.** A variant doing identical
  work in one inline script --- no module graph, no Svelte, no `@tauri-apps/api` --- measured
  −8.4, +9.9 and −0.2 ms across the three runs. See the next entry for why.
- **Creating the window in the setup hook rather than from `tauri.conf.json` is ±1 ms.**
  The cost is the WKWebView, not when it is asked for.

Two things *were* reducible, and both were defaults rather than requirements:

- **The 86 ms page-geometry walk**, which is ours. Two routes exist --- collect lazily, or
  collect during the shell's boot before the webview can ask --- and they are
  **alternatives, not complements**: both delete the same 86 ms, so doing both measures the
  same as doing either. Lazy is the better one; it needs no path known at launch.
- **Tauri's default macOS menu, ~16 ms.** Split across `Builder::build()` (39.7 → 32.9) and
  the `App::run` prologue (94.0 → 87.8), so looking in one place would have missed most of
  it. An empty menu is not shippable --- Cmd-Q, Cmd-W and clipboard shortcuts live there ---
  so the lesson is to build the menu the app needs instead of accepting the default.

Together they take 368 ms to **276 ms**. Note the menu effect is clean against the
low-variance `lazy` variant (negative in 7/7 rounds) and reads as −9 ms with a range
spanning zero against the noisier baseline: when two pairings disagree, believe the one
with the tighter distribution rather than averaging them.

The consequence to carry: tpdf's own startup budget is about **50 ms**, of which ~45 is
already spent. There is no headroom elsewhere to absorb a regression, and anything below
the floor requires presenting the first page without the webview at all.

### A webview's first custom-protocol request costs ~45 ms, whichever request it is

The reason the payload experiment above came out flat, and the more useful half of it. The
framework build spends 48 ms between its first script and its mount --- and its first IPC
then costs **0.0 ms**. The no-framework build has nothing to fetch, is fully loaded 39 ms
earlier, and its first IPC costs **43.9 ms**.

Same toll, different door. It is charged once, to whichever request over a Tauri custom
scheme happens to be first --- an asset fetch or an `invoke`, it does not matter. So a
smaller bundle does not avoid it, it only moves which line of the timeline shows it, and
"module load and framework mount: 47 ms" is a misreading of an interval that is mostly not
either of those things.

Corollary for reading any startup table: an interval named after what the application is
doing may be dominated by a fixed cost the platform charges inside it. Attribute by
substituting the work, not by naming the interval.

### Tauri creates config windows *before* the setup hook, hiding the webview's cost

`tauri::Builder::build()` does not run the setup hook; `App::run` does, and the internal
`setup` creates every window listed in `tauri.conf.json` *before* calling ours. A mark at
the top of the setup hook is therefore already after WKWebView creation, and no mark can be
placed between the two --- which is why 142 ms sat unattributed for a full spike.

To time it: take `tauri::generate_context!()` as `mut`, `context.config_mut().app.windows.clear()`,
and build the window inside the hook with `WebviewWindowBuilder`. Note `build()` returns
once the webview exists and has been told what to load, not once it has loaded it.

### A page whose window is not visible is suspended --- so a JS watchdog cannot fire either

WebKit suspends a page when its window is not visible, and behind a **lock screen** or on a
display that has gone dark, every window qualifies. The suspension stops
`requestAnimationFrame` **and `setTimeout`**. So a startup run that ends on a presentation
callback does not time out, does not error, and produces no output at all --- it stops dead
after the last milestone before presentation and looks exactly like a slow machine. A
JavaScript watchdog is useless here: it is suspended alongside the thing it was watching.

An hour went into this, on a machine that had simply idled and locked mid-benchmark. Three
guards now, and the third is the one that diagnosed it:

- `startup_bench.py` checks `CGSSessionScreenIsLocked` (via `ioreg -n Root -d1 -a`) and
  refuses to start.
- It holds `caffeinate -du` for its own lifetime. **`-d` alone is not enough** --- it
  prevents a display going idle and will not turn one back on that is already off.
- The app carries a **Rust-side** watchdog that prints the marks it did reach and exits 2.

The general rule is the same one the crash-test entry states from the other direction: a
harness must be able to fail. If the only mechanism that could report a failure lives on
the same side as the failure, it will not report it.

### PDFium parses a document lazily --- but enumerating pages is not lazy

Opening the 775-page text corpus with `FPDF_LoadDocument` takes **0.6 ms**. Collecting every
page's size afterwards takes **86 ms**. On a *one-page* document it still takes 52 ms, so
this is not per-page geometry arithmetic --- it is a fixed cost plus a real page load each
time.

The trap is that "open the document and read the page table" looks like one cheap operation
and is two very different ones. Anything that wants full document geometry up front ---
a virtual scroller sizing its scrollbar is the obvious one --- is putting 50-90 ms on the
critical path to buy exactness it could have estimated. Load geometry lazily and correct.

### PDFium rendering *is* interruptible --- via the progressive API

`FPDF_RenderPageBitmap()` cannot be cancelled once entered. But
`FPDF_RenderPageBitmap_Start()` takes an `IFSDK_PAUSE` callback whose `NeedToPauseNow()`
is polled during rendering; returning non-zero suspends the render, and
`FPDF_RenderPage_Continue()` resumes it. This is the mechanism for both cancellation and
for abandoning a tile the viewport has already left. (An earlier version of this entry also
credited it with releasing the global mutex between `Continue` calls. There is no global
mutex --- see the `thread_safe` entry above.) Use the progressive API for anything that is
not a small, bounded tile.

Reaching it is not free, though: `PdfDocument::handle`, `PdfPage::page_handle` and
`PdfBitmap::handle` are all `pub(crate)` in `pdfium-render`, so the progressive functions
--- which are public on `PdfiumLibraryBindings` and take raw handles --- **cannot be called
on anything the safe API produced.** Cancellable rendering therefore means owning
`FPDF_DOCUMENT`, `FPDF_PAGE` and `FPDF_BITMAP` ourselves and driving the render through the
bindings trait. The safe wrapper is all-or-nothing: use it and you cannot cancel. This
points the same way as the worker design, whose processes want raw handles regardless.

`src/progressive.rs` is that ownership, and `examples/progressive_probe.rs` measured it. An
uncancelled progressive render is **byte-identical** to `render_into_bitmap_with_config`
on every fixture tried, sliced or not --- which is also what asserts the flags, clear
colour and placement, since `pdfium-render` re-exports the handle *types* out of its
private `bindgen` module but not the `FPDF_ANNOT` / `FPDF_REVERSE_BYTE_ORDER` /
`FPDFBitmap_BGRA` constants, so those had to be restated by value. The former form-widget
gap closed 2026-07-31: the raw document now owns PDFium's pinned form environment and the
progressive path follows a completed render with `FPDF_FFLDraw`. The discriminating fixture
has a value but no `/AP` appearance stream; deleting that pass makes 4,587 bytes differ,
while an unused-form control stays byte-identical.

### PDFium decides how often it can be interrupted, and the slice does not change it

Measured 2026-07-27 on the A0 sheet, one 1024² tile at 1x, a 6.4 s render
(`progressive-probe --mode poll`). PDFium polled `NeedToPauseNow` **268 times, and exactly
268 times in every variant** --- with no slice, with a 16 ms slice, and with a 0 ms slice
that asks it to stop at the first opportunity. The polls are wherever PDFium's own work
divides; a slice only chooses *which* of them we pause at.

So the granularity is not a tuning parameter. Mean gap 24 ms, worst observed **66 ms**, and
asking for a 1 ms slice cannot beat that --- the number says how long PDFium goes without
asking, not how long we are willing to wait. Any latency budget has to be written against
the poll spacing, and any claim of finer control is a claim about a knob that does nothing.

What it buys is nonetheless decisive: cancellation from another thread returned in
**0.25--24 ms against a 6.3 s render**, and slicing costs about 1--2%, which is inside the
round-to-round noise. Note the cancelling thread sets an `AtomicBool` and touches no PDFium
state --- that is the only reason a second thread is involved at all.

Two traps in testing this. A **cheap page offers no interruption points**: the text corpus
renders a tile in 1.5 ms with `polls: 1`, so a cancellation test there passes by never
being exercised, which is why the probe fails a run whose sliced variant never actually
paused. And a cancelled render leaves a **genuine partial composite** in the caller's
bitmap rather than an untouched buffer --- PDFium composites as it goes --- so the buffer
is not safe to reuse on the assumption that a cancelled render wrote nothing.

### `FPDF_LoadPage` re-parses every time, and on a complex page that is 44 ms

PDFium does not cache a loaded page. Measured 2026-07-27 (`progressive-probe --mode
pageload`), loading page 0 of an already-opened document, repeatedly:

| corpus | per load |
|---|---|
| `text-heavy` | 0.18 ms |
| `vector-heavy` (A0) | **44.3 ms** |

The cost tracks page complexity, because what is being repeated is the content-stream
parse. So "load the page, render a tile, drop the page" --- which is what `render.rs` does
today, once per tile request --- charges a screenful of six tiles **266 ms of pure
re-parsing** on the one document where latency is already the problem, and nothing at all
on the corpus most likely to be used for testing. That asymmetry is why it went unnoticed:
the cheap page hides it perfectly.

`RawDocument` now keeps a small bounded cache of page *handles*. Handles rather than
`RawPage` values because a `RawPage` borrows its document, so storing one inside the
document would be self-referential --- whereas a handle is a plain pointer, copied out
under a short borrow, with the document closing every one it holds on drop. Note the cache
bound (4) is untuned; it was picked to be obviously safe.

Two things to keep: **evict outside the `RefCell` borrow**, or a PDFium call can re-enter
it and panic. And keep `evict_page` even though nothing in the viewer needs it yet --- it
is what lets the probe measure the uncached path, and a cache whose value cannot be
re-measured after a bump is a cache taken on faith.

### `Instant` on Apple Silicon ticks at 41.67 ns, so "elapsed == 0" is reachable

A latent bug, and worth the entry because the shape generalises. A pause deadline was
stored as nanoseconds-since-origin in an `AtomicU64`, with **0 meaning "no deadline"**.
Arming a *zero-length* slice a few nanoseconds after taking the origin should give a
deadline of a few nanoseconds --- but `Instant` here is backed by the 24 MHz timebase, so
its resolution is about **41.67 ns** and two reads that close together return the same
value. `origin.elapsed()` was genuinely 0, collided with the sentinel, and turned "pause at
the first opportunity" into "run to completion".

It was intermittent, because whether the two reads land in the same tick depends on what
the caller did between them: adding the page cache changed the timing enough to expose it,
having been silent until then. The fix is `u64::MAX` as the sentinel --- a nanosecond count
that cannot occur, rather than one that merely seems unlikely.

**A "no value" sentinel drawn from the value's own range is a bug waiting for the right
timing.** Zero is the worst possible choice for an elapsed-time field, because zero elapsed
time is exactly what a fast path produces.

What caught it was not a test of the sentinel. It was the probe's rule that a sliced render
reporting `resumes: 0` **fails** --- the control that exists so a "pausing is lossless"
result cannot be produced by never pausing. Without it, slicing would have silently stopped
working and every identity check would still have passed, because a render that never
pauses is byte-identical to one that never had to.

### Three similarity metrics in a row, each unable to see its own failure

Characterising a cancelled tile went through three metrics before one behaved, and all
three failures are the same shape as the crash test that compiled away:

- **"Fraction of pixels with ink"** --- defined as *not opaque white* --- reports an
  untouched, all-zero buffer as **100% ink**. It cannot distinguish "PDFium drew the whole
  tile" from "nothing ever wrote here", which is precisely the distinction it was added to
  make. Fixed by reporting the all-white and all-zero fractions separately.
- **Exact pixel match** reports **0.00%** for a partial that is visibly most of the way
  there, because the A0 fixture is antialiased random linework covering every pixel and no
  pixel is ever exactly right until the last stroke lands.
- **Mean absolute channel error** reads **45.1, 44.4, 44.2, 45.3** for cancellations at
  0.5, 1.5, 3.0 and 5.0 s of a 6.3 s render. It does not converge, because the distance
  between two dense random-colour images is roughly constant whatever fraction is drawn.

The fixture saturates all three, which is a property of a fixture built to be a
pathological *renderer* stress test and never intended as a *perceptual* one. What can
still be proven on it: the partial is not the untouched buffer, not the cleared buffer, and
not the finished tile, and an early cancellation differs from a late one. Whether a partial
tile is worth putting on screen is **not measured** and needs a realistic drawing.

The rule that survives all three: **a similarity number is not evidence until you have
shown what it reads on the failure you are trying to exclude.** Dumping the tiles and
looking at them took two minutes and settled what an hour of metrics could not.

### `FPDFText_GetText` drops characters, so it cannot be indexed alongside boxes

It extracts UCS-2 and, in its own documentation, "ignores characters without UCS-2
representations". Every other text API --- `FPDFText_GetCharBox`, `GetUnicode`,
`GetCharIndexAtPos`, the search functions --- is keyed by *character index*. So the string it
returns and the indices everything else speaks are two different sequences, and they diverge
exactly on the documents nobody tests with: CJK, symbol fonts, anything astral.

The symptom is not a crash or an error. It is a selection that highlights the right
rectangles and copies text from a few characters further along, on one document out of
twenty. `src/text.rs` therefore sends **one Unicode scalar per index** and no string at all,
and the frontend builds a string from the range it selected. Same rule as `set_text()`
drawing `.notdef`: work in the code space the document uses, never in a re-encoding of it.

**This is also why search does not call `FPDFText_FindStart`.** PDFium's search API is what
Chrome's Ctrl-F uses and would have been far shorter --- but it matches against that same
extracted string and answers in positions into it: a second index space carrying the same
divergence, which then has to be mapped back onto the boxes. Matching over the codes instead
(`src/search.rs`) makes a hit a range of the indices the boxes are already keyed by, so there
is no mapping left to get wrong. The shorter route is not cheaper; it is the same work with
the failure moved somewhere no test would look.

### A page carries `/Rotate`, and PDFium answers in two coordinate systems at once

Nothing in the corpus had one until 2026-07-27, and `/Rotate 90` is what a scanner emits.
For such a page PDFium reports:

* `FPDF_GetPageWidthF` / `GetPageHeightF` --- and `FPDF_GetPageSizeByIndexF`, measured, which
  is not obvious since it reads the page dictionary rather than the loaded page --- give the
  size **after** rotation, and a render comes out rotated to match. Layout and tiles were
  already right.
* `FPDFText_GetCharBox` and `FPDFDest_GetLocationInPage` give coordinates in the page's own
  **unrotated** space.

So the obvious flip, `height_pt - y` against the reported height, is correct at `/Rotate 0`
and wrong at every other value. Measured with `text-probe --mode align` on
`testdata/rotated.pdf`, per page: **100% of character boxes landed on ink at 0 and 0.0% at
90, 180 and 270.** Not approximately wrong --- every selection, every search highlight and
the whole screen-reader reading order was somewhere else entirely, in tidy rectangles, on
exactly the documents a scanner produces.

Three things worth carrying:

- **One mapping, two callers.** `text::to_device` turns a box; `outline.rs` places a
  destination by handing it a degenerate one. A second implementation of the turn is a
  second place to get it wrong, and the destination case is the one nobody would test.
- **A quarter turn makes the display's vertical axis the page's horizontal one**, so a
  destination that names no `x` cannot be placed at all. That path returns "no coordinate",
  which is what `/Fit` means anyway --- and it needed a fixture entry (`/XYZ null 600 0`) built
  for it, because without one the guard could be deleted and nothing would notice.
- **Reading the rotation costs a page load.** `FPDFPage_GetRotation` needs an `FPDF_PAGE`,
  while everything else in the outline walk reads the page dictionary. Measured on
  `outline-simple`: the walk went **0.17 ms -> 7.5 ms** (45.7 ms on a cold first run), about
  1 ms per distinct page named with coordinates. A three-hundred-entry table of contents is
  therefore a third of a second of the render thread, which is why `App.svelte` now waits for
  the first screen before asking for the outline at all.

### A line-grouping rule assumes an axis, and the axis is not always vertical

The consequence of the entry above, found the same day and worth its own note because the
first fix did not imply the second. Characters are grouped into lines by **vertical**
overlap --- which is right for a page read left to right, and on a `/Rotate 90` page puts
every character on a line of its own. The screen-reader layer then reads the page **letter
by letter**, and the selection highlight becomes one rectangle per glyph.

Every text assertion still passed. `textContent` is identical either way; only the block
structure differs, and the check that noticed compared a page's accessible text against an
independent extraction and got 877 characters against 534 --- the difference being the spaces
introduced by joining one block per character.

So the grouping takes the axis from the page's rotation. The honest limit, stated because it
is easy to believe otherwise: this fixes the *whole-page* case only. A rotated **run** inside
an upright page --- a sideways table header --- is still split character by character, as it
was before.

### PDFium's render rotation composes with `/Rotate`, and wants the turned size

`FPDF_RenderPageBitmap_Start` takes a rotation argument, and it is the right mechanism for
rotating the *view*: PDFium's display matrix applies the page's own `/Rotate` first and this
on top, so "turn it a quarter clockwise" means the same thing on a scanned page as on an
upright one. Two things about it are easy to get wrong in the same direction.

The `size_x`/`size_y` arguments are the **displayed** dimensions, so a quarter turn swaps
them. PDFium fits the page into the rect it is given and rotates inside it; passing the
upright size squeezes a landscape page into a portrait box rather than turning it. The
symptom is a page that is recognisably the right content at the wrong proportions, which
reads as a tiling bug.

And the text side does *not* go through the same call. `FPDFText_GetCharBox` knows nothing
about a view rotation --- it is not a property of the document --- so the boxes have to be
turned in our own code. That gives two implementations of one idea, which is what the
composition rule is for: turning a device-space box by `v` after `to_device(p, …)` must equal
`to_device((p + v) % 4, …)` of the raw box. `text.rs` asserts it for all sixteen
combinations, and it is the only thing tying the frontend's turn to the mapping that was
verified against pixels.

Note the two are separately capable of being wrong, in ways that hide each other. A view
that turns its boxes and not its render, or its render and not its boxes, still selects text
in tidy rectangles --- the wrong ones. `text-probe --mode align --view-turns N` renders with
the rotation and maps with it, so it catches either half; mutating each was worth doing,
because dropping the dimension swap goes red only on odd turns and reading the rotation as
zero goes red on all three.

### Two rotation tables, disagreeing at every turn but zero

A rotated page has two reading directions and they are not the same table. **Across** the
lines, the first line is at the low end of the axis at no rotation and at three quarter
turns --- because one quarter turn sends the page's y to the display's x *decreasing*, and
three send it to x increasing. **Along** a line, reading runs in the increasing direction at
no rotation and at *one* quarter turn.

Written as one table --- which is the obvious mistake, since both are "is it rotated" --- a
check that drags two lines out of a rotated page went red on two corpora and passed on
neither. Derive each from where the page's top-left corner ends up; do not derive the second
from the first.

### PDFium's character order is not the page's line order

A check asserted that text dragged from nearer the start of a page has a lower character
index. On `text-heavy` that is true. On `rotated-90.pdf` it is not: dragging out `Line 03`
gives index 405 and `Line 10` gives 90, so the extraction runs the other way --- and the
check went red against a rotation that `text-probe --mode align` had already confirmed
correct at 100% of character boxes.

That is a claim about PDFium and the document, not about the code under test. What is ours,
and what the check now says, is that **the same two lines come back**: sample two lines
before the rotation and the same two after, from wherever the rotation should have put them.
It assumes nothing about extraction order, and a rotation applied backwards still returns the
other line.

Compare exactly, though, and it fails for a reason that is not a defect: rotating refits the
page to the window, so the zoom changes and the drag endpoints land a character further in or
out. `"ine 03 charlie delta ech"` against `"Line 03 charlie delta ec"` is the same line.
Compare the core of the shorter string --- that tolerates an edge and not a line.

### A check that derives its inputs from the thing it is testing cannot fail

The fourth instance of this shape in this project, and the most disguised. "The same lines
come back out of a rotated page" computed its drag positions from the viewer's own turned
character boxes, then asked the viewer's caret which characters were there. Delete the line
that tells the text layer about the rotation and it is wrong **consistently**: the sample and
the caret agree, the same lines come back, and the check passes over a selection ninety
degrees out from the page on screen. It survived that mutation.

What catches it is a direct assertion on the wiring rather than on the geometry --- the text
layer must *report* the turned rotation and the swapped page width, checked against a second
fetch from the backend. Cheap, and unsatisfiable by self-consistency.

Two smaller ones found the same way, both gaps rather than wrong answers:

- **Nothing in a viewer check looks at a pixel.** Drop the rotation on its way into the tile
  URL and the boxes still turn, the layout still turns, and the page underneath is upright.
  The check for it fetches one tile at two rotations and asserts they differ --- with the
  control beside it that the *same* rotation twice is byte-identical, or "they differ" is
  satisfied by a renderer that is merely non-deterministic.
- **The viewer and the scroller each keep a rotation, and either can be wrong alone.**
  Checking the zoom covers the viewer's; a scroller laying every page out upright survived
  it completely, producing a narrow page inside a correctly refitted window. The scroller's
  own laid-out page box has to be asserted separately, and its aspect is the thing to assert:
  the mutation's detail line read `aspect 0.773 then 0.773`.

### A dense page of uniform lines cannot detect a y-flip

Character boxes come in page space (y up, origin bottom-left) and everything downstream wants
device space (y down, origin top-left). Getting that flip wrong is the classic failure here,
and it does not look like a bug --- the highlight is still made of tidy rectangles, just on
the wrong lines.

`bin/text-probe --mode align` checks it against pixels: render the page, and for each drawable
character ask whether its mapped box covers ink. On the four small fixtures the correct
convention scores **100%** and the flipped one **4.1--4.8%**. On `text-heavy.pdf` the correct
one still scores 100% --- and **the flipped one scores 69.9%**, because a page of evenly
spaced identical lines has ink almost everywhere a mirrored box could land.

So the corpus most likely to be reached for is the one where the check is blind, and the
probe fails the run and says so rather than printing the 100%. The general form: **a control
that discriminates on one input may not on another, and "the check passed" is only meaningful
alongside "the check could have failed here".**

Two smaller notes from building it. A whole-page ink bounding box is not an oracle --- the
text fixtures draw a frame, so the ink box is far bigger than the characters and *neither*
convention matched it; per-character is both stricter and indifferent to other content. And
whitespace has a box and no ink, so it has to be excluded or it puts a floor on the failure
rate that has nothing to do with the mapping.

### `evict_page` can dangle a live `RawPage`, and the borrow checker allows it

`RawDocument::evict_page` takes `&self` --- it has to, since the cache is behind a `RefCell`
--- and closes the `FPDF_PAGE`. `RawPage` also borrows `&self`. Two shared borrows coexist
happily, so this compiles:

```rust
let page = document.page(0)?;
document.evict_page(0);       // closes the handle `page` holds
let _ = page.width_pt();      // use after close, and rustc is fine with it
```

`RawPage` has no `Drop`, so `drop(page)` does not end anything either --- and clippy rejects
it as a no-op, which is how this surfaced. Scope the borrow instead. It bit while writing
`text-probe --mode extract`, whose whole method is load, extract, evict, load again.

`evict_page` is not the only route, and reading the entry as though it were is the mistake
to avoid. `RawDocument::page` evicts too --- loading a fifth page closes the oldest cached
handle --- so the same shape is reachable by holding a `RawPage` across a *load*, with no
eviction call in sight. It cannot happen today only because every shipped caller holds one
page at a time, which is a property of the callers rather than of the type; the safety
comment on that close says as much. A caller that ever holds two must scope the first
borrow before asking for the second.

### A timer that starts after the setup measures the wrong thing, and reports it

`text-probe --mode extract` compares extracting from a cached page against an uncached one.
Written with `document.page()` before `Instant::now()`, both columns measured extraction
alone: they came out identical on the text corpus (1.43 vs 1.43 ms, which reads as "the page
cache does nothing") and reported the A0 sheet's *uncached* case at **0.116 ms**, against a
page load independently measured at 44 ms.

Moving the load inside the timer gives 1.42 / 1.64 ms and 0.12 / **43.2 ms**. The tell was
available before the fix: a column named for a cost that is 44 ms cannot read 0.1 ms. **When
an A/B shows no difference, check that the variable is inside the measurement before
concluding it does not matter.**

### `FPDFBookmark_GetDest` follows the bookmark's action without checking its type

The narrow-sounding accessor is not narrow. When a bookmark has no `/Dest`, PDFium's own
implementation falls back to `FPDFBookmark_GetAction` and returns **that action's `/D`
array**, with no check on the action's `/S`. So an entry meaning *"open other.pdf at page
1"* --- a `/GoToR` --- comes back as an ordinary destination, and
`FPDFDest_GetDestPageIndex` then resolves it against **this** document.

Measured 2026-07-27 on `outline-hostile.pdf`: the entry titled "Remote goto" reported
`page 1`. Not an error, not a refusal --- a plausible page of the file the reader already
has open. The same fallback reaches `/Launch` and `/URI` actions, which happen to carry no
`/D` and so come back null; nothing about the API guarantees that, and a `/Launch` with a
`/D` would be followed just as silently.

The fix is an ordering, not a filter: **read the action first**, obey its type, and consult
`FPDFBookmark_GetDest` only when there is no action at all --- which is precisely the case
where its fallback has nothing to reach. It also happens to be what PDF 32000-1 §12.3.3
says, `/Dest` being forbidden alongside `/A`.

What makes this worth an entry is that the wrong version *works*. Every ordinary outline
resolves identically, because ordinary entries have a `/Dest` or a `/GoTo`. Only a document
carrying a remote destination behaves differently, and it behaves plausibly. It was caught
by a fixture built to contain one, not by reading the header.

### An outline can be infinite, and PDFium says so in its own documentation

`FPDFBookmark_GetNextSibling`: *"the caller is responsible for handling circular bookmark
references, as may arise from malformed documents."* That is not a caveat to note, it is
the library declining to bound a walk over attacker-controlled data --- and the obvious
loop hangs the render thread forever, with no output and no error.

`src-tauri/src/outline.rs` carries three bounds, and the point is that each catches
something the others cannot:

- **A visited set** stops a cycle at its first repeat. Note the alternative that looks
  equivalent --- abandoning the sibling list on a repeat --- loses every entry *after* the
  loop, which on the fixture is nine of the ten top-level items.
- **A depth bound** stops a chain that is deep without ever repeating. 200 distinct nested
  nodes put nothing in the visited set twice.
- **An item budget** stops everything else, including a visited set defeated by PDFium
  handing back a fresh pointer for a node already seen. The set is the mechanism expected
  to work; the budget is what makes termination not depend on that expectation.

Deleting the visited set does **not** hang --- the budget catches it --- which is exactly
why `outline-probe` asserts that the budget was *not* what stopped the walk. Without that
control the run still says "18 checks", the cycle is unnoticed, and 14 of them go red for
reasons nobody would connect to a loop. Measured: removing it drops the hostile fixture
from 18/18 to 4/18.

Whatever any bound cuts is counted and reported rather than dropped. An outline shown as if
it were complete when it is not is the same failure as a leak scanner reporting clean on a
carrier it could not decode.

### `FPDFDest_GetLocationInPage` answers only for `/XYZ`, so every other fit lands at the page top

PDFium implements it over `CPDF_Dest::GetXYZ`, which returns false for any fit that is not
`/XYZ`. So `/FitH top` --- which names a vertical coordinate as plainly as `/XYZ` does ---
came back from it with `has_y == 0`, and `outline.rs` correctly concluded "no coordinate"
and scrolled to the top of the page. The reader lands on the right page, near enough to the
right place that it reads as a slightly loose viewer rather than as a bug.

Nothing caught it for two reasons that are worth separating. The **API** looked total: its
name says location-in-page and its out-parameters are `has_x`/`has_y`/`has_zoom`, which
reads as "whichever of these the destination named". And the **corpus** had no `/FitH`
entry --- `outline-simple.pdf` carries `/XYZ` and `/Fit`, so the gap in the code matched a
gap in the fixtures exactly.

The fix is `FPDFDest_GetView`, which reports the fit type and its parameters. Note its
indices are **not** the destination array's: it returns the elements *after* the fit name,
so `/FitR left bottom right top` has its top at parameter 3 and array element 5. Two
tables saying the same thing in different coordinates is its own hazard, and both now say
which they are in.

**What found it was a differential check, and nothing else could have.** See the entry
below.

### Two resolvers agreeing with themselves is not two resolvers agreeing

tpdf resolves PDF destinations twice: `outline.rs` asks PDFium, because a bookmark is a
PDFium object, and `links.rs` reads the object graph with `lopdf`, because asking PDFium
would cost `FPDF_LoadPage` per page for a whole-document answer. Two implementations of one
rule is the drift trap this file already records --- and the usual mitigations do nothing
about it. Sharing the `Target` type keeps the *vocabulary* identical. Sharing the refusal
wording keeps the *words* identical. Neither says the two arrive at the same page.

Each module's own tests are structurally unable to notice: they assert their module against
its own expectations, and a module that is consistently wrong passes perfectly. The corpus
cannot notice either, because each fixture is read by one resolver.

`links.pdf` gives its outline entries the same destinations as its links, and
`links-probe --mode agree` puts the two answers side by side. **It found a defect on its
first run** --- the `/FitH` one above, which had been in `outline.rs` since it was written.

Two details make it an instrument rather than a ceremony. Both answers are compared against
the **manifest**, not against each other: two resolvers wrong in the same way agree
perfectly, and only the fixture's generator knows what it wrote. And it asserts the outline
came back with at least as many entries as it means to compare, because a comparison that
cannot find its entries reports agreement by having nothing to disagree with.

### A destination's offset belongs to the page it lands on, not the page it left

`/XYZ x y z` measures `y` upwards from the bottom of the **destination** page, and a viewer
scrolls in points down from its top --- so the flip is `height - y`, with `height` being
that page's. Reaching for the height already in hand, which is the page the link is *on*,
is the natural mistake and is invisible on every document of uniform page size. That is
every fixture in this repository except the one written to catch it, and very nearly every
real document too.

The general shape: when a conversion needs a property of a *remote* object, having a
perfectly good local copy of that property is what makes the wrong version compile, run and
look right. `links.rs` builds the geometry of every page once and indexes it by page
number, so the destination page's height is as easy to reach as the current one's; the
mutation `links: flip the offset against the wrong page` is what says the difference is
still observable.

### `/F` is a bit field, and the flag every real link sets is not the one you are testing

Annotation flags: bit 1 Invisible, **bit 2 Hidden**, bit 3 Print, bit 4 NoZoom. A hidden
annotation is not shown, so a link carrying it has nothing under the pointer to click ---
and testing `/F != 0` instead of `/F & 2` drops every link in any document, because
essentially every real link sets `/F 4` so that it survives printing.

It fails loudly, which is the good case, but only if the test has a control. A fixture
whose links carry no `/F` at all passes both readings; the assertion that discriminates is
a link with `/F 4` beside one with `/F 2`, asserting the first survives and the second does
not.

Worth noting where this differs from a comment. `annots.rs` *keeps* a hidden comment and
carries the flag, because the panel still lists it --- a reader asked for that list
explicitly. A link has no panel, so a hidden one dropped at scan time is the whole of it;
keeping it would leave an unclickable rectangle every hit test walks past.

### A hit-test slack that rescues a small target hands the click to its neighbour

`annots.rs`'s comments get three points of slack around their rectangle, because a sticky
note is a 24-point icon a reader aims at and an exact test makes small marks feel broken.
Copying that number to links is wrong, and the reason is not that links are bigger.

A link is a run of text, and the thing next to a link is usually **another link** --- a
wrapped sentence produces two rectangles a point or two apart. Slack there does not make a
small target reachable; it makes the gap between two targets belong to both, and the tie is
broken by whichever the loop saw last. A cross-reference that jumps to the wrong section is
worse than one that needs a second click, so links get one point.

The check that says so asserts a press in the gap between two adjacent rectangles resolves
to the nearer one --- not that a press inside a rectangle hits it, which passes at any
slack at all.

### Recording a jump at the call sites is a rule; recording it inside the primitive is a mechanism

Back has to undo a link, an outline row, a search result and a comment that scrolled. Four
call sites, so the first draft called `markJump()` at each --- which is a rule someone has
to keep following, and this file already records what those are worth. The fifth caller is
the one that forgets, and the symptom is a Back button that works until it does not, which
is worse than no Back at all because the reader has already trusted it.

Recording inside `goToDestination` --- the one primitive all four go through --- makes it a
property of the operation. It needs exactly one escape: the history's own replay, or
stepping back would push the place being left onto the stack it was just popped from,
making Back a toggle between two positions and Forward unreachable.

The flag that does it is set and cleared around one call in a `try`/`finally`, and the test
that pins it is the *forward* one: a history that recorded the wrong end still changes the
page and still looks right from a single assertion about Back.

### A fixture where the right rule and the wrong rule agree cannot tell them apart

The link ordering bands two rectangles onto one line when they overlap vertically by more than
half the shorter one's height, and the alternative worth ruling out is an absolute overlap ---
a constant tuned for body text, which separates a footnote marker from the sentence it sits in.

The first fixture for it was a 8-point marker at `[300, 102, 306, 110]` beside a 20-point
sentence at `[100, 100, 280, 120]`, and the mutation `band lines by absolute overlap` survived
it. Both rules give `[sentence, marker]`: the proportional one bands them and orders them
across the page, the absolute one splits them and orders them by top --- and the marker's top
is *below* the sentence's, so the split happens to produce the same sequence.

The fix is one number, and it is the number that makes the fixture realistic: a superscript
sits **above** the baseline, so its top is above the sentence's. At `[300, 96, 306, 106]` the
two rules disagree, because now the split orders the marker first.

The general form, and it is not "use a bigger fixture": **an input can exercise the code under
test and still be outside the region where the rules differ.** Every ingredient was present ---
two links, different heights, a real overlap --- and the discrimination was not. The only thing
that finds it is a mutation aimed at the rule, which is why a surviving mutation is a statement
about the *fixture* at least as often as about the assertion.

**Two more of exactly this shape, the same day, on the character-to-link intersection.** A guard
rejecting a character box read past the end of the array was mutated to coerce the missing edges
to zero --- which puts the phantom character at the origin, where the fixture's link was not, so
both versions answered "not a link" and the mutation survived. It bites only against a rectangle
whose corner *is* the origin. And a guard skipping a link whose rectangle has no height survived
because a degenerate rectangle contains exactly the points on its own line, and no character in
the fixture was centred on it; the discriminating input is a character whose centre lands
*exactly* there.

Both are the same lesson from the other end: **when a guard rejects a degenerate case, the input
that tests it is the degenerate one**, and a fixture built from plausible values will not contain
it. Neither was a missing assertion --- the assertions were right and could not fail.

### A check that cannot run is not a check, and a locked screen is enough to stop one

`viewercheck.ts` carries an audit asserting that the set of registered commands and the set of
*classified* commands are equal, so a command added without deciding how it is covered turns it
red. It is a good check and it did nothing: `nav.back` and `nav.forward` were added on
2026-08-16 and left unclassified, and the run that would have said so never happened, because
the screen was locked and `viewer_check.py` refuses rather than hanging.

Nothing was broken by it --- the omission was found later the same day and the four commands
were classified as driven probes --- but the shape is worth stating, because the check's whole
purpose is to catch an omission at the moment it is made. **A gate that runs on a schedule
somebody controls is a gate with a queue**, and anything that lands while the queue is stopped
is unprotected for exactly as long as the stoppage lasts.

Two things follow. When a harness is blocked, the commit that lands anyway should say **which
checks did not run**, in the artifact rather than only in a report --- `BUILD.md` carries that
here. And a check whose subject is *the completeness of a list* is the one to be most suspicious
of while it is unrun, because its failure mode is silence: the list is simply short, and nothing
about the shorter list looks wrong.

**And a locked screen does not always announce itself as a refusal --- on 2026-08-21 it wore
the shape of an application defect.** Driving the real menu bar to reproduce a save report, the
run reported that a page rotation nine seconds after launch changed nothing: `Undo` stayed
greyed, the file was untouched, and three consecutive launches agreed. That is a plausible and
specific finding --- an edit silently dropped just after open --- and it was one keystroke from
being written down. The screen had locked at 07:11:03, between the measurements that worked and
the ones that did not: a locked session suspends the web view, so the document never opens,
every command guarded on a document stays greyed, and a click on a greyed item does nothing at
all. **The application looked broken because the check could not run**, and the only tell was a
timestamp nobody had reason to read.

Two habits close it, and the first is one line. Read `CGSSessionScreenIsLocked` out of
`ioreg -n Root -d1` **before** any window-driving measurement and refuse with that as the
reason, which `scripts/save_check.py` now does. And when a result changes partway through a
session, ask what changed in the *machine* before believing the change is in the code: the
successful runs and the failing ones were the same binary and the same commands, minutes apart.

### Reading a decision back out of the DOM makes the test double part of the logic

`a11y.ts` builds a page element out of text nodes and `role="link"` spans, and returns whether it
put anything in it --- so a caller can drop an element that would be empty, which a screen reader
passes over in silence. The obvious way to answer that is
`(element.textContent ?? "").trim().length > 0`.

In the browser it is correct: `textContent` on an element aggregates its descendants'. In
`testdom.ts` it deliberately does not --- the double stores assignments verbatim and computes no
aggregate, which is recorded here as its own trap. So the emptiness test answers "empty" for
every element under test while working perfectly in the application, and every one of those
elements is dropped. **A check that disagrees with its subject only under test is worse than one
that is simply wrong**, because the code is right and the tests say otherwise, which sends you
looking in the wrong file.

The fix is not to make the double smarter. It is to **track what was written rather than read it
back**: the function already knows every piece of text it appended, so accumulating them costs a
string and removes the dependency on DOM semantics entirely.

The general rule: a function that *builds* DOM should decide from what it built, not from what
the DOM now says. Reading back is a second implementation of your own bookkeeping, evaluated by
somebody else's engine --- and under test, by a stand-in for it.

Related, and it failed loudly rather than silently, which is the good case: the same change
introduced `document.createTextNode`, which the double did not have at all. Eight tests died with
`is not a function` and named the line. **An absent method is a better failure than a present one
that behaves differently**, and it is worth noticing which kind a double gives you before relying
on it.

### An empty answer from a whole-document scan cannot say whether it looked

`annots.rs` and `links.rs` both walk `document.get_pages()` from `lopdf` and bound themselves
against a `page_count` that came from **PDFium**. Every loop is `for page in pages.take(count)`,
so when `lopdf` reports no pages the loop runs zero times, the list comes back empty, and *not
one bound has tripped*. The reader is told the document has no comments and no links, which is
exactly what a document with none looks like.

`encoding.rs` had already drawn this distinction --- "a page `lopdf` cannot account for is
unknown, not clean" --- and the two younger modules did not, because the shape was copied from
each other rather than from it. Both now count `page_count - pages.len()` into a `pages_missed`
limit, before the walk, since the walk's own emptiness is the thing it cannot distinguish.

**No fixture on disk makes it fire, and that is the interesting half.** Swept across every
`testdata/*.pdf` on 2026-08-16: the two parsers agree about page count on every document PDFium
will open. So the guard is **defensive rather than demonstrated**, its tests are synthetic, and
saying so is the difference between a bound with a known instance and one without.

### A cited instance can be half right, and the wrong half is the one doing the work

`encoding.rs` justified taking its page count from PDFium with a specific example, repeated in
`docs/PLAN.md`: *"`lopdf` reports zero pages for `testdata/incr-encrypted-pw.pdf`, which PDFium
opens and paginates normally."*

Measured 2026-08-16, both halves separately:

- `lopdf` loads that file and reports **0 pages**. True.
- PDFium **refuses to open it** --- `RawDocument::open` fails and `links-probe` exits 2 --- because
  it is AES-256 behind a real user password. False.

So the two parsers never both see that document, and it demonstrates no disagreement at all. The
sentence was persuasive precisely because its first half is checkable and true; the half nobody
checked is the one the argument rests on, since a disagreement needs two answers.

The design it justified is unchanged and still right: two independent parsers with no guarantee
of agreeing is reason enough to take the count from the one whose pagination the reader is
looking at. What changed is what the codebase claims to *know* --- and the general lesson is that
**a compound claim is not verified by verifying the memorable part of it.** The way to catch it
is to state each half as its own proposition and measure each: here that was two commands.

### An error message that names no cause is not vague, it is a wrong diagnosis

Both of `RawDocument`'s open paths reported the same thing whatever had happened: *"could not
open <path>"* and *"could not parse N bytes as a PDF"*. The second is the interesting one,
because it is not a refusal to guess --- it is a **claim**, and for a password-protected document
it is false. The file parses perfectly; it is locked. A reader told their document is not a PDF
goes looking for another copy of a file that was fine.

Measured before the fix: 3 of the 39 PDFs in a real Downloads folder carry `/Encrypt`, so this is
not a corner. PDFium keeps the reason and it costs one call --- `FPDF_GetLastError`, which
distinguishes file, format, **password** and unsupported security.

Two details worth carrying:

- **It is one error per thread and the next call overwrites it**, so it is only meaningful
  immediately after the failure it describes. The code is read at the call site and passed into
  the mapping, rather than the mapping fetching it, so no future caller can ask it late.
- **`FPDF_ERR_SUCCESS` is reachable here.** PDFium can hand back a null handle with no error set,
  and the tempting arm --- `0 => "no error"` --- produces a message that reads as though the open
  worked. Unknown codes and zero both collapse into "PDFium did not say why", and the test that
  pins it asserts the message does *not* contain "no error".

The general shape: an error path that cannot fail is as bad as an assertion that cannot fail, and
it hides in the same way. Every message being identical means no test comparing one to another
can go red, which is why the check here enumerates every documented code and asserts that the
four PDFium distinguishes produce four *different* sentences.

### `FPDFBookmark_GetDest` cannot tell a heading from a damaged link

It returns null for an entry that carries no `/Dest` **and** for one whose `/Dest` names a
destination that does not resolve. `outline.rs` therefore answers `Target::None` --- "a heading
that is only a heading" --- for a document that plainly does name a destination, and the reader
is told "no destination" about something the file states.

Measured rather than suspected, and that took an instrument: `links-probe --mode agree` resolves
the same outline through `lopdf` and compares entry for entry. Across **44 real documents and
421 outline entries**, exactly one disagreed --- an entry in a BS EN standard whose `/Dest` is a
name string that resolves nowhere, which `lopdf` correctly calls `Broken`.

Not fixed, and the trade is written down rather than left implicit: distinguishing them means
consulting the object graph, which is the second parse `outline.rs` exists to avoid, for one word
of explanation on a row that is non-navigable either way. What *is* fixed is the check: the
probe allows that one pair by name and fails on any other difference, so a resolver that started
answering `None` for a page destination still goes red.

### A differential that needs a manifest is a differential over one document

`links-probe --mode agree` compares tpdf's two destination resolvers, and it found a real defect
--- but only on `links.pdf`, because it asserted both sides against a manifest stating what the
destinations should be, and only that fixture has one.

The stronger version needs no manifest at all: resolve the **same outline** both ways and compare
the two lists. Nothing has to be stated, so any document with an outline becomes a test. That
took 6 assertions on one fixture to 421 entries across 44 real files, and it is what turned the
`FPDFBookmark_GetDest` limitation above from a suspicion into a measurement.

**The mode's own doc comment claimed it needed no manifest while the code still demanded one.**
`section()` returned `Err` for a document the manifest does not describe, so every real file came
back *"manifest is not JSON"* and the differential ran on exactly one document --- the state it
had just been rewritten to escape. Absent is not an error there; it means "no stated
expectations, run the half that needs none".

Two details make the comparison an instrument rather than a ceremony. **Compare the counts
first**: two lists compared pairwise up to the shorter one agree perfectly when one walk stopped
early, which is the failure a differential is least able to see. And **the two walks must cut at
the same bound**, or the difference between two limits is reported as a disagreement about
destinations.

### PDFium lays a page out from its `/CropBox`, and everything else here read `/MediaBox`

A page has two boxes that matter: `/MediaBox` is the sheet, `/CropBox` is the part displayed.
PDFium renders and measures the **crop** box --- `FPDF_GetPageWidthF` returns its width --- so the
viewer's coordinate space starts at the crop box's lower-left corner.

Three places disagreed with that, all silently:

- **`links.rs` and `annots.rs`** computed the page from `/MediaBox` and mapped every rectangle
  into it, so a link or a comment landed offset by the difference between the two corners.
- **`text.rs`** was worse, because it mixed the two: the *size* came from PDFium (cropped) while
  `FPDFText_GetCharBox` answers in the page's own space (media-origin). Every character box was
  out by the crop origin.

Measured, with the control that makes it a measurement: a fixture with `/CropBox [50 50 545 742]`
on `/MediaBox [0 0 595 842]` renders 495x692 and landed its character boxes on ink **0%** of the
time; the same page with no crop box landed **100%**.

**The discriminating property is the crop box's origin, not its size.** A `/CropBox
[0 0 545 742]` --- smaller, same corner --- passes both before and after the fix, because the
flip against a smaller height is still the right flip. Only an origin away from (0, 0) breaks
it. A fixture that merely shrinks the page tests nothing.

**It is live, and the real instance is too small to catch.** One of the 43 PDFs on this machine
carries `/CropBox [0 7.83 595.5 850.08]` on all ten pages. That 7.8-point offset misplaces every
selection by about two thirds of a line --- and the "boxes land on ink" check still passes on it
at 100%, because a 7.8-point shift on a glyph that size still overlaps ink. The committed fixture
insets by 50 points for exactly that reason: **a fixture has to be able to fail.**

Two details worth keeping. §14.11.2 says the crop box is **intersected** with the media box, and
that is done rather than trusted --- a producer can write one larger than the sheet, and a page
displayed bigger than its own paper is not a space to map coordinates into. And the shift is
applied *before* the `/Rotate` turn, because `to_device` works in the displayed page's
coordinates and the displayed page starts at the crop corner.

### PDFium cannot create digital signatures

`fpdf_signature.h` is an **inspection** API --- it reads existing signatures. Applying a
cryptographic signature requires a separate crypto stack, trust store, certificate
selection, timestamping and revocation handling. Placing a signature *image* and
*cryptographically signing* are unrelated problems; do not let the roadmap conflate them.

### Redaction conflicts with incremental save --- and a full rewrite is not sufficient either

Incremental save appends an update section and leaves the original bytes intact, which is
exactly what redaction must not do. So applying a redaction is a **full-rewrite barrier**.

But a non-incremental save is still not proof of sanitation. A serializer can happily
rewrite a file while carrying over unreachable objects, unused resources, embedded
originals and copied streams; and overwriting a file in place can leave trailing bytes
past the new `%%EOF`. Redaction must write a **fresh file from a garbage-collected
reachable object graph**, then atomically replace the target. See `docs/PLAN.md` §6.

Scope note to state honestly in the UI: this sanitizes the PDF, not previous copies,
backups, or recoverable filesystem sectors.

### Digital signatures constrain what may be edited at all

Incremental save preserves a prior revision's cryptographic integrity, but that is not the
same as the signature remaining *valid and trusted* --- the document is still reported as
modified after signing. Worse, a certification signature with a DocMDP permission entry
can **forbid** page, annotation or form changes outright. Every edit command must be
classified as incrementally representable, full-rewrite-required, or forbidden on a signed
document. Do not claim "signatures survive".

Spike 0.6 measured how far that goes, and it is further than "some edits are forbidden".
Against pyhanko, on an approval signature and on DocMDP levels 1, 2 and 3, an appended
update leaves the signature `intact=yes valid=yes` every time --- and the difference
analysis rejects the change every time, at every level, including an **annotation-only**
edit to a level-3 certified document, which the specification explicitly permits. Cutting
the edit down to its irreducible minimum does not clear it: extending an `/Annots` array
that is its own object, so the page dictionary is never touched, narrows the complaint to
two objects and still fails. pyhanko says why in its own log --- *"StandardDiffPolicy was
not designed to support DocMDP level 3 (MDPPerm.ANNOTATE)"*.

So DocMDP is not the discriminator it looks like. **"The spec permits this edit" and "a
validator will accept it" are different claims**, and only the second one is what a user
sees. Treat any signed document as edit-hostile regardless of its permission level, offer
to save a copy, and never let the UI imply the signature will be fine.

### Whether `/Annots` is an indirect array decides how large an annotation edit is

A page can carry `/Annots [5 0 R]` written inline in its dictionary, or `/Annots 12 0 R`
pointing at an array object. Both are common. They are not interchangeable for incremental
editing: extending the array *object* touches one object and leaves the page dictionary
untouched, while extending an inline array means rewriting the page dictionary itself.

On a signed document that difference is the whole game --- the page dictionary is a signed
structural object, so rewriting it is a change no difference analysis can justify, and
`.Root.Pages.Kids[0]` shows up by name in the rejection. Prefer the array object; when the
producer inlined it, know the edit is bigger than it looks.

The same shape applies to `/Contents`, and there it has a second trap: `/Resources` is an
*inheritable* attribute. A page that does not carry one takes its parent's, so "create an
empty `/Resources` on the page" does not add a resource, it removes every inherited one.
The page then renders blank while every check that counts pages still passes.

### Embedded fonts are subsetted

An embedded font contains only the glyphs already used. Typing a character that is not in
the subset has no glyph to draw. This is the root cause of every mangled Acrobat text
edit, and it constrains the entire text-editing design. What that looks like in practice,
and how PDFium handles it, is measured above under `set_text()`.

### `lopdf`'s object collection is quadratic, but the algorithm is not

`prune_objects()` and `renumber_objects()` both walk the graph through
`traverse_objects()`, which accumulates seen ids in a `Vec` and calls `contains` before
every push. Measured 2026-07-26, cost of collection over a plain save: 3.7 ms at 2,445
objects, 83 ms at 7,758, **1,414 ms at 25,583** — a 3.3x larger graph costs 17x more.
25,583 objects is a medium document.

A mark-and-sweep over a `HashSet` (`route_lopdf_mark` in `sanitize_rewrite.rs`, about
thirty lines) produces byte-identical output at a cost indistinguishable from not
collecting at all. Renumbering is dropped with it — contiguous object numbers are cosmetic
and cost a second quadratic pass — but then **`max_id` must be lowered by hand**, because
`/Size` is written from it and sweeping does not touch it. Skip that and the file claims
more objects than it has; `qpdf --check` rejects it and PDFium does not notice.

So: use `lopdf` for the rewrite, write the sweep yourself. Do not reach for QPDF to solve
a `Vec::contains`.

**The same walk is reached from a third door, and it is worse there.** `delete_pages` calls
`delete_object` once per page, and `delete_object` calls `traverse_objects` — so deleting *n*
pages runs that quadratic walk *n* times. Printing one page of the 775-page corpus measured
**620 ms** for the deletion alone, release profile; a single pass over the graph, dropping
`/Kids` entries and dictionary keys that name a doomed page and decrementing `/Count` up each
`/Parent` chain, does it in **1.2 ms** and produces **byte-identical output** on every fixture
and corpus. `print.rs`'s `drop_pages` is that pass, and the byte comparison against
`delete_pages` is kept as a test rather than having been run once.

The generalisation worth carrying: **`lopdf`'s convenience methods are built on
`traverse_objects`, so any of them is a graph walk in disguise.** Before using one in a loop,
check what it calls.

### `cargo test` is a debug build, and a debug number in a doc comment is a lie

Corollary of the `tauri dev` entry below, and it was one keystroke from being published. The
`delete_pages` figure above was first measured at **15,912 ms**, and that number was written
into a doc comment as a measured fact. It is a debug-profile measurement: the release figure is
620 ms, **26x apart**. The conclusion happened to survive — 620 ms is still terrible — but the
number would have been wrong in a file whose whole value is that its numbers are real.

`cargo test`, `cargo run` and `cargo bench`-adjacent harnesses all default to the dev profile.
Anything measured through a test needs `--release`, and the same asymmetry applies: PDFium and
other prebuilt native code are barely affected while our own Rust is 20-50x slower, so debug
numbers do not merely inflate — they **reorder** what looks expensive.

### `lopdf` silently drops encryption on save

Given an AES-256 file with an empty user password — one that opens in any reader with no
prompt — `lopdf` decrypts on load and writes **plaintext** on save. No error, no warning,
no flag. QPDF re-encrypts with the original parameters.

Quietly removing a document's protection is its own security failure, and it is invisible
in every check that looks at content rather than structure. Any save path must preserve
encryption or refuse; `qpdf --is-encrypted` (exit 0 when encrypted, 2 when not) is the
cheap assertion.

### An incremental save is cheap on disk, not in memory --- and its cost is the parse

Measured 2026-07-26 across a 0.9 KB to 337 MB sweep (`incremental-save --mode speed`). In
memory, a full `lopdf` rewrite of a 337 MB scan costs **12.4 ms** against an incremental
append's **12.3 ms** --- no advantage at all, because the rewrite is essentially a large
`memcpy` and this machine has the bandwidth. Reading only that table would kill incremental
save as pointless.

Landing the save on disk reverses it: **29 ms against 239 ms, 8.2×**, because the append
writes 723 bytes to a file that already exists and the rewrite writes 336,623,496 and
renames. Below a few megabytes the ratio is 1.0× --- both are dominated by a single
`F_FULLFSYNC`, about 3 ms. The rewrite also needs room for two copies of the document,
which no timing shows.

Two things worth carrying:

- **Benchmark a save by writing it, not by serialising it.** The interesting cost of a
  save is I/O, and an in-memory harness measures precisely the part that does not matter.
- **`sync_all` is `F_FULLFSYNC` on macOS, a device-wide barrier.** Staging a 337 MB fixture
  with `fs::write` and then timing an append's flush charges the append for the staging: it
  measured *slower* than the full rewrite until the setup was made durable outside the
  timer. Any fsync benchmark on macOS must account for what else is dirty.

What remains of the append's cost is almost entirely **parsing** --- 5.7 ms of the 29 ms at
337 MB --- which every edit must pay to know what it is editing. Note also that
`IncrementalDocument::create_from` takes the previous bytes by value and `save_to` writes
them through before appending, so the obvious use of the API copies the whole document
twice for no reason. Sink the prefix, or append to the file in place.

### An object a prior revision overwrote is reachable by no parser

Incremental update replaces object 5; the old object 5 stays at its old offset and the new
cross-reference table points past it. Every parser resolves through the newest table, so
the old bytes are handed to nothing — not to a graph walk, not to `qpdf --check`, not to
PDFium. A byte scan finds them only if they were uncompressed, which for real content they
will not be.

The consequence for any verifier: **a file with more than one revision cannot be certified,
only rewritten and then certified.** `%%EOF` count is the cheap test. Note this is a
different failure from an object the update merely *dropped from the page* — that one is
still in the cross-reference table and a graph walk does see it, as an orphan.

### A decompression bomb costs QPDF CPU, not memory — and `lopdf` neither

`qpdf in out` re-encodes stream data by default, so it fully decodes a stream that inflates
to 1 GiB: **1.92 s for a 2,879-byte input**, at only 8.4 MB resident, because it streams
rather than buffers. `lopdf` with `LoadOptions::max_decompressed_size` refuses in 0.3 ms.
`qpdf --stream-data=preserve` finishes in under 10 ms — by copying bytes it never looked
at, which is precisely what a sanitizing rewrite must not do.

Two things follow. The bound belongs on the *rewriter*, not only on the verifier. And a
resource limit expressed in memory would have caught none of this: the attack here is
600x amplification in time.

### A rotated page whose box it inherited comes back `width x width`

PDFium reports the wrong displayed size for a page that inherits its `/MediaBox`
from an ancestor **and** carries a quarter turn. Measured 2026-08-24 on a page
that is 400x600 in its own space, with the box on the `/Pages` node above it:

| `/Rotate` | correct | `FPDF_GetPageWidthF` x `GetPageHeightF` |
|-----------|---------|------------------------------------------|
| 0         | 400x600 | 400x600 |
| 90        | 600x400 | **400x400** |
| 180       | 400x600 | 400x600 |
| 270       | 600x400 | **400x400** |

The width is right and the height is the width again. It is the **box's**
inheritance that does it, not the rotation's --- crossed both ways, with the same
page and the same content:

| `/MediaBox` | `/Rotate` | reported |
|-------------|-----------|----------|
| page        | page      | 600x400 |
| page        | node      | 600x400 |
| node        | page      | **400x400** |
| node        | node      | **400x400** |

So PDFium inherits `/Rotate` correctly, inherits `/MediaBox` correctly on an
upright page, and gets the combination wrong. `testdata/inherited.pdf` is that
document, and `testdata/make_inherited_pdf.py` explains why the corpus needed
one.

**What it does downstream is not a failure.** The page lays out square, at an
aspect nothing on it matches, and every coordinate derived from the page size is
off: the render is clipped to a box smaller than the content, so a page of text
comes out nearly blank --- 0, 1 and 3 inked pixels on the three pages of that
fixture, against 1013, 1062 and 1317 for the same pages once their attributes are
written onto them. Nothing errors.

**tpdf already owns a correct implementation**, and it is not the one the render
path uses. `pagetree::displayed_page` walks `/Parent` with `lopdf` and answers
600x400 for every row above. It is what `save.rs` writes from, which is why a
*merge* of such a document comes out right --- and it is what `merge-probe` uses
as its oracle, because comparing a merged page against PDFium's reading of the
source would demand that a merge reproduce this defect.

Two things follow, and the second is the open one:

- **A check that baselines on PDFium's render of a source page is only a check
  where PDFium reads that page correctly.** `merge-probe` skips its ink
  comparison with the reason printed, rather than failing a correct merge or
  passing quietly.
- **Fixed 2026-08-24, and the repair is not the one this entry predicted.** It said to
  prefer `pagetree::displayed_page` over `RawPage::width_pt` on the render path. That
  would correct the *number* and leave the *render*, because PDFium draws from its own
  idea of the sheet: the page would report 600x400 and still come out clipped. What
  works is to give PDFium the box instead --- `RawDocument::page` writes the page tree's
  rectangle onto the loaded page with the setter the crop tool already uses, and the
  reported size, the origin, the render and the character boxes all follow. Measured on
  the fixture: 400x400 and 1 inked pixel before, 600x400 and 1013 after, through
  `RawDocument::page` with nothing else touched.

  Two things worth carrying from doing it. **The discriminator is
  `FPDFPage_GetMediaBox` answering `None`** --- that API does not walk `/Parent` either,
  so "PDFium has no sheet for this page" *is* "this page inherits one", which is why the
  repair costs nothing on a document that states its own boxes: it never parses
  anything. And **`FPDFPage_GetCropBox` does answer on such a page, in a different
  convention** --- `[0 0 600 400]` here, the displayed shape rather than the page's own,
  where every ordinary page gives the unrotated box. So it is not usable as the second
  opinion, and a repair built on it would be right on this fixture by accident.

  `examples/geometry_probe.rs` is the check, `docs/PLAN.md` has the increment, and
  `merge-probe` went from 27/27 with 3 skipped to **30/30 with none**: the skip existed
  because PDFium mis-read the source page, and it does not any more.

Worth knowing that this is not an exotic document. A producer that writes one
`/MediaBox` on the page tree root rather than on every page is ordinary --- it is
what a tool emitting uniform pages does --- and `/Rotate 90` is what a scanner
writes.

### PDFium accepting a file is not evidence the file is well formed

PDFium is deliberately lenient — that is why it is the right renderer for real-world PDFs —
so it will happily render a file whose `/Size` is wrong, and render it pixel-identically to
a correct one. `qpdf --check` named the same defect immediately.

Corollary for verification: a render comparison proves the *content* survived and nothing
about the *structure*. `docs/PLAN.md` §6 requires re-parsing with an independent parser for
this reason, and the requirement paid for itself the first time it ran, on a bug in tpdf's
own sweep.

### A refusal in the setup hook cannot speak, so it must happen before the event loop

`RenderService::start` panics on an unreadable `TPDF_BACKEND` — the right decision, in the
wrong place. The setup hook is invoked by `App::run` from inside AppKit's own frames, so a
panic there is **non-unwinding**: it aborts through a backtrace with no symbols, and it
races the watchdog, which has 30 seconds to report that the page never ran a line of
JavaScript. A misspelt environment variable is then diagnosed as an occluded window.

The refusal moved to `run()`, beside the worker-argv check and before the watchdog: one
line on stderr, `exit(2)`, no window in existence to be occluded and no event loop to lose
the message in. Same family as the `RunEvent::Opened` entry above — **anything that must be
*heard* has to happen before Tauri's frames are on the stack.**

Two notes on how this surfaced, and the second is the more useful one. It was found by
asking whether the loud refusal was actually loud, rather than by anything failing. And the
first run "proving" it silent was **contaminated**: the control — a perfectly valid
configuration — failed identically, because the display had gone dark between batches and
WebKit suspends every window then. Two mechanisms, one silence, and the wrong one nearly
went into a commit message. Run the control before believing a failure you have just
provoked.

### A delivery counter cannot say WHICH delivery, and the guard was satisfied by the event it excluded

The wait in front of the broken-pattern search check read
`!viewer.searching && seen.updates > beforeBroken`, and flaked --- twice in about seven runs
on the day it was fixed, against a comment recording once in three. It is worth reading as
two separate mistakes, because the second one is the interesting half.

**The instrument mixed two clocks.** `viewer.searching` is a live read of the searcher;
`seen` is a mirror the viewer fills a frame later, through `wake()`. So the live half goes
false the instant the `invoke` resolves, a whole frame before the status carrying `problem`
is delivered.

**The counter half, added to fix exactly that, was satisfiable by the event it existed to
exclude.** `Search.run` emits a status at the *start* of a scan, deliberately, because a
search over 775 pages has to look like it is working. When a frame happens to land between
that start and the scan's completion, the first status delivered after `beforeBroken` is the
start one — `running: true`, `problem: ""`. Counter satisfied, live flag false, mirror
holding the empty value. **A guard whose condition the excluded event also satisfies is not
a guard**, and it reads as one in every review.

The fix is to read `running` **out of the mirror**: only a status taken after the scan
stopped satisfies it, whichever of the two was delivered. The counter stays and still does
work — it excludes the idle status from *before* the search, which also reads
`running: false`. Waiting on `problem` itself would be the obvious fix and the wrong one: it
is the value being asserted, so the check could then only pass or hang.

**The control is the reason any of the above is right, and it went red.** The first attempt
at this fix came with a check asserting the mechanism — *"a scan reports itself started
before it reports a result"* — and it **failed on its first run**, reporting
`running=false, problem="regex parse error:"`. The start status is normally *coalesced away*:
both `onChange` calls land before the next frame, so the mirror usually sees one status
carrying the final state. That is why the check passed for weeks, and the first written
account of this trap — confident, detailed, and asserting the start status always arrives —
was wrong. It was corrected by a control that could fail, not by re-reading the code.

That control was then **deleted rather than kept**, which is the last lesson here: it
asserted the race instead of the behaviour, so it could only pass on exactly the runs where
the bug would have fired. A check whose truth depends on the timing it is meant to remove is
worse than no check. There is no deterministic control for this from inside the harness;
what stands behind the fix is the construction argument plus repetition — 4/4 clean runs,
against 2 failures in the preceding 7.

### Turning on updater artifacts makes every build demand the signing key

`createUpdaterArtifacts: true` in `tauri.conf.json`, beside a `plugins.updater.pubkey`, does
not mean "sign when a key is available". It means **every** `npm run tauri build` fails
without one:

```text
Error A public key has been found, but no private key.
      Make sure to set `TAURI_SIGNING_PRIVATE_KEY` environment variable.
```

Which is a security defect wearing a build error's clothes. The obvious fix is to put the
key on the development machine — and that takes the one secret capable of forging an update
that every installed copy will accept, and copies it onto every laptop that builds. The
release checklist's own bundle smoke-test would require it, so it would not even be
optional.

The flag therefore lives in an overlay config passed only by CI, and the main config keeps
only the **public** key, which the app genuinely needs at runtime to verify what it
downloads. Signing stays where the key is.

**Two ways to pass that overlay are wrong, and both fail only on a tag push** — the most
expensive place in this repository to discover anything, since it runs unreviewed paths
beside the signing key:

- **A relative `--config` path** resolves against the invocation directory, and
  `tauri-action`'s is not the one a local `npm run tauri build` uses.
- **Inline JSON** does not survive the trip. The action's `args` string is split shell-style,
  and a shell eats the JSON's double quotes, leaving `{bundle:{createUpdaterArtifacts:true}}`
  — which is not the config that was written. Checked with `shlex.split`, not reasoned about:

  ```text
  '--config {"bundle":{"createUpdaterArtifacts":true}}'
    -> ['--config', '{bundle:{createUpdaterArtifacts:true}}']
  ```

What works is an absolute path built from `${{ github.workspace }}`: no quotes to eat, no
spaces to split on, and no assumption about the working directory.

**The control that makes any of this a finding rather than a theory** is the pair, and it is
worth keeping: building *with* the overlay and no key must fail with exactly that message
(so the overlay is provably doing something), and building *without* it must succeed (so a
development machine is provably unencumbered). Either half alone is satisfied by a config
that does nothing.

### A status element that comes and goes rearranges the toolbar it sits beside

Reported by the user as *"scrolling fast, the toolbar with the find field is briefly
overlaid/replaced"*. Nothing overlaid anything. The header is a single flex row, and the
degraded-state label was the second-to-last item in it, so every time coverage dipped below
the threshold the label entered the row and **displaced everything to its left** --- the
whole find toolbar stepped sideways, and because flex items shrink by default and `.find`
had a `width` but no `flex`, the search field was squeezed narrower at the same time. At
scroll cadence that reads as the toolbar being replaced by a progress bar.

Two independent defects, and fixing either alone leaves a visible fault:

- **Position.** An element that appears and vanishes must not be able to move anything a
  reader is aiming at. Moved next to the document title, where it grows into the slack the
  spacer was already holding, so its arrival moves nothing at all.
- **Rate.** It was truthful at display cadence, which is the failure. `degraded.ts` holds a
  transient state back until it has lasted 300 ms, so a scroll that resolves in a few frames
  says nothing. **The delay is deliberately not applied to a failure** --- `failed > 0` is
  the one state waiting does not fix, and it can arrive with the frame loop already
  quiescent, so delaying it would suppress it entirely rather than postpone it.

The rate half was already half-known and written down one level lower: the thresholds are
`0.999` rather than `1` because a tile boundary landing a rounding step inside the viewport
leaves a fraction of a percent uncovered, and that comment says in its own words that "a
status line that flickers on that is worse than none". **The same judgement had been made
about the same indicator and not carried to the case that actually reaches a reader** ---
the threshold answers "is this dip real", and nothing answered "is this dip worth saying".

Worth stating because the symptom named the wrong component. The complaint was about the
find toolbar, which is correct in that the toolbar is what visibly moved, and the toolbar's
own markup and CSS are innocent --- the cause is an entirely different element two lines
away, plus a default (`flex-shrink: 1`) that nobody wrote down anywhere.

### An outcome two mechanisms can produce cannot test either one

Withdrawing a tile has two halves once the renderer is in another process: the parent's
own queue decides what the caller sees, and a `Withdraw` on the pipe decides whether the
worker keeps burning CPU. The obvious check asserts the outcome --- the request comes back
`Abandoned` --- and it **passes with the pipe half deleted**, because the parent's token
produces that answer on its own. Confirmed by mutation: removing the broadcast changed
nothing the check could see.

What discriminates is *when*: with the withdrawal crossing, the reply arrives in 2.2 ms
against a 1,125 ms render; without it, the caller waits out the whole render and then
reports `Abandoned` anyway. So the assertion has to be the outcome **and** the latency,
and the threshold has to come from the render's own measured time rather than a constant.

The general form, and it is the sharpest instance of "assert on what was produced" this
project has hit: **when two independent mechanisms can produce the same answer, that
answer tests neither of them.** Ask what else could have produced the result before
believing the check covers the mechanism you had in mind --- and if the only difference is
timing, the timing is the assertion.

### A length bound cannot be tested by the verdict it produces

`read_reply_line` refuses a reply over 32 MB, and the test fed it 4,096 bytes with no
newline and a 64-byte limit, asserting `TooLong`. Delete the bound entirely and it still
passes: an unbounded read consumes the lot, hits EOF without a newline, and is refused for
*that* reason instead. The verdict is identical and the allocation the limit exists to
prevent has already happened.

Two repairs, and both are needed. Make the input a **complete** line that is merely too
long, so an unbounded read succeeds rather than failing for the other reason. And assert
the reader's **position** afterwards --- 64 bytes consumed, the rest still waiting --- because
the property is "it stopped reading", and no statement about the return value can express
that. Same family as the atomic-write test that had to plant the intermediate file: when
the property is *how* the result was produced, the result cannot test it.

### The linker's image table is an observable; a milestone of ours is a claim

The strongest claim about the worker backend is that the app process never parses a PDF,
and the first version of the check for it asserted the absence of our own `pdfium bound`
startup mark. That tests our bookkeeping: the mark is written by the code being trusted, so
a path that bound PDFium without marking it reads as clean.

`_dyld_image_count` / `_dyld_get_image_name` answer the actual question --- **is libpdfium
mapped into this process** --- and the answer comes from the dynamic linker rather than from
us. The probe scans before the in-process backend starts (615 images, no pdfium) and again
after (616, pdfium present), so the control that the scan can see one is in the same run.

Same rule as reading a written PDF back with a parser that did not write it, arriving from
a completely different direction: **prefer an observable the code under test does not
produce.** Note `libc` deprecates both symbols in favour of the `mach2` crate; they are
declared directly instead, because adding a dependency for two dyld entry points is a
licensing decision (see the constraint above) taken for nothing.

### A Rust process absorbs the first SIGSEGV you send it

Killing a child with `SIGSEGV` is the obvious way to simulate a crash, and against a Rust
child it does nothing at all. `std` installs a handler on SIGSEGV and SIGBUS so a stack
overflow can be reported; on a fault address outside the guard page it restores the default
disposition and **returns** --- which, for a signal that arrived by `kill(2)` rather than
from a faulting instruction, simply resumes the process. Measured 2026-07-28, three
processes, same harness: a tpdf worker and a trivial `rustc`-built sleeper both survived the
first SIGSEGV and died on the second; `/bin/sleep` died on the first.

A *genuine* fault still terminates, because the faulting instruction re-executes against the
restored default --- which is why nothing in production depends on this and why it is easy to
believe the simulation works. What it broke was the check: `backend-probe` killed its worker,
the worker carried on, and the two checks that followed passed against a worker that had
never died. They were caught only because a control asserted the death separately, which is
the entry above arriving from a new direction --- **assert that the thing you broke is
broken, never that the recovery looks fine.**

Use `SIGKILL` to make a process go away. Use a real fault (`black_box` plus
`write_volatile`, as `worker-bench --mode crash` does) to test what a crash does.

### A check nested inside a lookup for the thing under test disappears with it

Found 2026-07-28 by mutation M1 on the worker restart, and it is the *silent* sibling of
"a defect that switches off a check's precondition". The crash checks were written as
`if let Some(victim) = worker_pids().first() { ... }` --- perfectly reasonable, since there is
nothing to kill otherwise. Then the mutation that stops workers being replaced leaves **no
worker to find**, so the block does not run: not `[FAIL]`, not `[SKIP]`, nothing. The check
name never appears.

The only trace was the total: `19/22 checks passed` where every other run says 23. Nothing
compared those two numbers, and a reader scanning for `[FAIL]` lines sees three where four
were predicted --- which reads as a prediction that was slightly wrong, not as a check that
evaporated. Writing the prediction down beforehand is what turned it into a finding.

So: **when the subject of a check is also the thing that decides whether it runs, do not let
it decide.** Hoist the lookup, pass the absence in as a value, and let "there was nothing to
kill" be a failing detail line rather than a branch not taken. The standing invariant is the
one this file already states for the viewer: the set of check *names* is fixed, and a count
that moves is itself a defect.

A smaller one from the same run, worth a sentence because it costs nothing to avoid: the
failing check printed *"1048576 bytes, identical to the tile before it died"* next to
`[FAIL]`, because the detail string was written on the success branch of a `match` on
`Result` and said nothing about the comparison that actually decided the verdict. **A detail
line has to be computed from the same quantity as the verdict**, or it will eventually
contradict it --- at exactly the moment someone is relying on it.

**That invariant then caught one, which is the first time it has earned its keep.** Later the
same day, a routine sweep across the corpora reported 86 check names everywhere except
`text-cid`, which reported 85 --- no failure, no skip, nothing red anywhere. The missing name
was `"finds something from the end of the document"`, and the cause is a shape worth naming
separately from the one above: **`searchesFromHere` records two names and its early returns
skipped only one.** On a one-page document it took the `pageCount < 2` branch, skipped
`"searches forward from the page being read"` by name, and returned --- and the second check,
recorded further down by an `eventually`, simply never happened.

Note what did *not* find it. Every corpus was green; the suite had been run and mutated
repeatedly; and a static scan for names that are recorded but never skipped returns 48
candidates, most of them false, because a skip can be reached through a `const` or a
multi-line call the scan cannot see. The only thing that discriminated was **comparing the
name sets across corpora**, which costs one `diff` and needs no knowledge of the suite at all.

So the rule generalises past nesting: **a function that records more than one check name must
skip every one of them on every path out.** Count the names a function can print, then count
the ones each early return prints; if those differ, a document exists that makes one vanish.
And when a suite runs over several inputs, diff the name sets rather than reading the totals
--- a total tells you a number moved, the diff tells you which check stopped existing.

### A released id must leave a hole, because removing it renumbers the rest

Documents live in a `Vec` whose index *is* the id the caller was given. Releasing one by
removing the entry is the obvious implementation and is the worst available outcome: every
later document shifts down one, so a request naming the *closed* id is answered, in full,
from a document the caller has never asked about. Measured 2026-07-28 by mutation --- with
`Vec::remove` in place of a hole, a tile request for the closed document returned
`rendered 1048576 bytes` of the wrong file, and the check that noticed was the one asserting
the request is **refused**, not any check on the pixels.

So a closed slot is `None`, ids are never reused, and the two failures are named apart: an id
past the end is a caller that invented one, a hole is a caller still using a document it
closed itself. The same applies to the parallel `Vec` of withdrawal senders, which is
positional for the same reason.

Note what protects the *in-flight* case, because it is not a lock: the render thread is FIFO,
so a close lands behind everything already queued for that document. There is no window in
which a request outlives its document, and nothing needed a reference count to say so.

### Two copies of a distinction drift, and a mutation of one survives

`open_slot` and `open_slot_mut` each spelled out the same two error messages. Mutating one of
them to collapse the distinction changed **nothing any check could see** --- because the
worker path reaches documents through the `_mut` variant and the in-process tile path does
not, so each check went through whichever copy was still correct.

That reads exactly like a distinction nothing depends on, which is the wrong conclusion:
both copies are load-bearing, on different paths. The repair is to share the message, after
which the same mutation goes red immediately. **When a mutation of duplicated logic survives,
check whether the callers are split across the copies before deciding the logic is
unnecessary.**

### Dropping the owner does not close a pipe something else has cloned

`Worker` owns its child's stdin --- and the withdrawal broadcast holds a **clone** of the
`Arc<Mutex<ChildStdin>>`, because a withdrawal has to be sendable while the owning thread is
blocked reading a reply. So killing the worker and dropping it leaves the write end of the
pipe open, held by an entry in a `Vec` nobody thinks of as owning anything. One descriptor
per document the reader ever opened, invisible to every functional check: writing to it fails
harmlessly, so nothing misbehaves.

`/dev/fd` counts what the process actually holds, and the assertion that discriminates is
that closing a document gives back exactly what opening it took --- 9 before, 9 after, and 10
with the clearing removed. **A resource whose leak has no functional symptom needs a check
that counts the resource**, and the kernel's own listing is the place to count it.

### A descriptor without `FD_CLOEXEC` leaks into every later child, and keeps it alive

The same family, one level nastier, found 2026-07-28 building the pre-spawned worker. A spare
waits for its document by blocking in `recvmsg` on a `socketpair`, so --- unlike a
document-serving worker --- it is **not reading stdin** and cannot notice the parent going
away that way. What should end it is the socket reaching EOF when the parent's end closes.

It never does. `socketpair` descriptors are not close-on-exec, so **every child spawned
afterwards inherits a copy** and holds the write end open. The spare then waits forever, on a
socket a sibling is keeping alive, reparented to init. The process table showed eighteen
orphaned `--prespawn` processes, some of them seconds old.

Three things about it are worth carrying:

- **`Drop` is not the fix, and believing it was cost an hour.** A `Drop` that kills was added
  first, and it is correct and does nothing here: `std::process::exit` runs no destructors,
  and every probe and the app itself exit that way. Anything that must not outlive the process
  has to be arranged so the *kernel* ends it --- a pipe that reaches EOF, a signal --- not so a
  destructor would have.
- **The neighbouring mechanism hid it.** Document workers do not leak, because their stdin
  closes and they exit on EOF. So "workers clean up after themselves" was true, observed, and
  did not generalise to the one worker that reads a different descriptor.
- **`dup2` clears the flag**, so setting `FD_CLOEXEC` on both ends costs the child nothing ---
  it still receives a usable socket on the agreed number.

The general rule: **any descriptor a process keeps in order to notice something ending must be
close-on-exec**, or a later `fork`/`exec` silently appoints a third party to keep it open. And
the symptom is not a stray process anybody notices --- it is that whatever captures the
parent's output waits on a pipe an orphan still holds, so a run that finished cleanly looks
like it hung. `backend-probe` appeared to wedge on its first corpus exactly this way, which is
how the whole thing surfaced.

### Two mechanisms with the same limit make one of them untestable

The render service grew a pool of worker processes, capped per document, and the number of
threads serving the job queue was set equal to that cap --- one thread to drive each worker,
which reads as obviously right. A mutation removing the cap **entirely** then survived every
check: `idle` can only be empty when every worker is checked out, which takes one thread
each, so a thread arriving to find none free cannot exist. The thread count was silently
doing the cap's job, and both the cap and the wait beside it were unreachable.

By this file's own rule that would make them guards to delete. The better reading is that the
coincidence was the defect: the two limits are about different things --- how many *processes*
a document may have, and how many *jobs* may be in flight --- and tying them together also
means six tiles of a slow document occupy every thread, so a request for a second document
waits behind a render while its own workers sit idle. Decoupling them (threads = pool + 2)
made both bounds reachable *and* fixed the starvation.

**When a mutation of a bound survives, check whether some other quantity is enforcing the
same limit** --- and if it is, ask whether the two were ever meant to be equal.

A second, sharper reason the first attempt could not see it: the burst used to provoke
contention was `capacity + 1` tiles, and the extra one was the tile being *withdrawn*. A
withdrawal is refused at the claim, before a worker is checked out --- deliberately, so a
tile nobody wants does not occupy a process --- so the burst could never demand more than
`capacity` workers however the cap behaved. **A surplus that gets cancelled is not a
surplus.**

### A check whose failure mode is a wait cannot fail

Several properties of a worker pool break by *not answering*: a pool that believes in a
worker it retired never finishes a close, and a checkout waits for a process that will never
exist. Every check for those was written with a blocking `recv`, so the defect produced no
verdict at all --- the run stopped, and the harness had to interpret a timeout.

Two repairs, and both were needed. The probe's own `wait` takes a bound far above any
legitimate wait (60 s against a 1.2 s render) and returns *"the service is wedged, not slow"*
as a failure. And the mutation harness keeps the partial transcript on timeout: one mutation
turned a check red **and then** wedged the run, and a harness that reads a timeout as "no
result" throws away a correct red and reports it as nothing. It now distinguishes
`CAUGHT, then hung` from `BROKEN: hung with nothing red`.

Same family as the timeout that discarded `viewercheck.ts`'s transcript, one level out: there
the fix upstream was undone by a consumer that buffered, here by a consumer that could not
time out at all.

### A mutation harness that dies leaves the mutation in the tree

`AGENTS.md` already says to restore by writing the bytes back rather than moving a file. That
is necessary and not sufficient: the restore was the last statement of a loop body, and an
unhandled `TimeoutExpired` skipped it. The tree sat mutated, and the next thing run against it
was a build.

Put the restore in a `finally` around the whole run, not at the end of the happy path. A
harness that can leave the tree in a state its own output does not describe is worse than no
harness, and this is the second way this project has found to do it.

**Third way, 2026-08-16: the operator kills it.** A `finally` does not survive `pkill`, and
killing a harness that is visibly stuck is the obvious thing to do. Two runs were killed that
way and each left an edit behind --- one of them `viewer.ts` holding `this.rotateBy(turns)` in
place of the two lines a page turn needs.

**On a feature branch that leftover is invisible, which is the part worth knowing.** The
harnesses mutate exactly the files a branch is already modifying, so `git status` shows what it
showed before, and two swapped lines do not draw the eye in a large diff. It surfaced as the
*next* run's baseline going red, which reads as a defect in the feature rather than in the
tree --- and the failing checks were the ones the feature had just added, so the reading was
plausible.

The gate is `anchors` (`scripts/check_mutation_anchors.py`, in `scripts/gates.py`): every
mutation's search string must occur exactly once in the file it names. It caught this, and in
the same pass caught two mutations aimed at code that no longer exists --- see the entry
below.

### An unreachable guard is worth keeping if the type can carry it instead

`PreWorker::adopt` consumed the worker's readiness line before sending it a document, so that
the announcement could not become the answer to the caller's first real request. Deleting that
call changed **nothing**: not a check, not a benchmark, on any corpus. `Workers::prewarm` waits
for readiness on its own thread and publishes a spare only if that succeeded, and the wait was
idempotent --- so no `PreWorker` that could reach `adopt` was ever unwarmed.

By this file's own rule that is a guard to delete. Deleting it would have been wrong for the
reason the `path_from_url` entry gives from the other direction: **what made the guard
unreachable lived in a different module.** `worker.rs` would have become silently dependent on
a publishing policy in `render.rs`, and nothing would have failed when that policy changed.

The third option is the one that was taken: make the ordering structural. `wait_warm` now
*consumes* a `PreWorker` and returns a `WarmWorker`, which is the only type `adopt` accepts. The
runtime check is gone, the guarantee is stronger than it was, and the mutation that motivated
all this no longer compiles --- which is the strongest verdict a mutation can get.

**When a mutation of a guard survives, the choices are not only "delete" and "keep".** Ask
whether the impossibility can be moved into the type, and prefer that: it deletes the code
*and* the assumption.

### Three mechanisms, no checks: measure what a commit's tests can actually see

A mutation pass over the pre-spawned worker put one deliberate defect into each of the three
mechanisms that commit added. **All three survived every check in `backend-probe`** --- 32 check
names, zero failures, on every corpus, in all three runs. What each was caught by instead:

| mutation | backend-probe | what noticed |
|---|---|---|
| delete the font warm | green | `prespawn-bench`: 0.37 ms -> **7.68 ms** on a base-14 document |
| skip the readiness wait | green | **nothing** --- unreachable defence, see above |
| drop `FD_CLOEXEC` | green | the harness hanging, and a `pgrep` count |

The two that were real defects are now pinned. `prespawn-bench` asserts and exits non-zero
rather than printing a table nobody reads --- and the discriminating comparison is **between two
fixtures**, base-14 against embedded-font, because the gap between them *is* the system-font
walk that warming exists to pay early. Measured 0.35 vs 0.80 ms warm, 9.96 vs 0.84 ms with the
warm deleted, against a 3.7 ms bound: an order of magnitude clear on both sides, not next to it.

The descriptor leak needed a **second process**, because it is invisible while the parent lives:
the spare waits on a socket whose other end the parent legitimately holds, and the failure is
only that the socket does not reach EOF afterwards. So `backend-probe` runs itself as a
short-lived service (`--spare-lifetime`), lets it exit, and asserts the spare went with it. Two
details are load-bearing. The child must **open a document and grow the pool**, or no sibling is
ever spawned to inherit the descriptor and the leak does not reproduce. And the parent must read
**one line, never to EOF** --- that pipe is precisely what a leaked spare holds open, so waiting
on it converts a red check into a hang.

The general shape, and it is the reason to do this at all: **a commit's test suite can be
entirely orthogonal to what the commit added.** Every check was passing, none of them was wrong,
and none of them was about the new code. Only mutating it says so.

### A verdict that reads a timeout as "no result" throws away the finding

The mutation harness classified any run that timed out as `BROKEN: hung with nothing red`. For
the `FD_CLOEXEC` mutation that was exactly backwards: the probe printed a complete
`28/32 checks passed` **and then never exited**, because the leaked spare held its stdout. Every
check had run, and the hang *was* the defect.

A timeout is only unreadable if the report is also missing. Split the two: a run with no summary
line is broken, and a run with a full summary that failed to exit is a result --- and, for
anything holding a descriptor, usually the result. Same family as the `viewercheck.ts` timeout
that discarded its transcript, one level further out.

The same harness then reported the correct, restored tree as `[FAIL] restore is not green`,
because its orphan count was absolute rather than a delta and the leaked processes from the
mutated runs were --- by definition --- still there. **A counter for a leak has to be reset
before the run it is attributed to**, or every later run is charged for every earlier one.

### FIFO dequeue is not FIFO completion

The moment tiles render in a pool, replies come back in *completion* order. A check that
issued two tiles, withdrew the second, and then read two replies positionally reported the two
outcomes **swapped** --- the withdrawn one answers in milliseconds and arrives first.

It announced itself, which was luck. The same change quietly converted the check's meaning:
"withdrawn before it starts" needs the request to be *waiting*, and with a pool the second
tile starts immediately in another process. Had the reply ordering not also changed, the check
would have passed while testing "withdrawn after it started" --- which the parent's own token
satisfies regardless, and which this file already records as an outcome two mechanisms can
produce.

So: match replies by their own identity, never by arrival; and when adding concurrency, re-read
every check whose meaning depended on there being one of something.

### `caffeinate <utility>` becomes a child of the utility, so a child count counts it

Every observation of a *process* in this repository comes from `pgrep -P <us>` --- deliberately,
because the kernel's table is the observable and our own `Vec<Held>` is what the code under
test believes. The unstated premise is that every child of this process is a worker, and it is
false in one way nobody goes looking for.

`caffeinate -d -u <utility>` does **not** run the utility as its child. It forks a helper to
hold the power assertion and then `exec`s the utility in the *parent* --- so the helper ends up
a child of the very process it was wrapping. `pgrep -P` finds it, and it is indistinguishable
from a worker by parentage alone.

The consequence is worse than a wrong number, because this file and the personal notes both say
to wrap long unattended batches in exactly that, to stop the screen locking mid-run:

```
[FAIL] concurrent tiles grew the pool, and no further than its capacity
       7 workers, capacity 6, opened with 2
```

A capacity overrun *and* a broken laziness claim, both entirely fictitious, both perfectly
reproducible, and both gone the moment the same binary is run bare. Two harnesses had it
independently --- `backend-probe`, where it read as the defect above, and `pool-bench --mode
retire`, where it read as a pool that never retires. The second was caught only because that
wait is bounded and says which; the first survived several runs being read as a real
regression at HEAD.

The fix is to match on **argv** rather than on parentage: `pgrep -P <us> -f -- --render-worker`.
That is the marker the worker is spawned with, so it identifies our processes rather than
merely our descendants --- and it excludes `backend-probe`'s own `--spare-lifetime` child, which
it needed anyway.

Two things worth carrying past this instance. **A wrapper is part of the process tree**, so
anything reasoning about children has to identify what it is looking for rather than assume
everything found is it --- `nohup`, `timeout`, `stdbuf` and a shell's job-control fork all sit in
the same place. And the diagnosis went the wrong way for an hour because the failure was
*stable*: three runs, same failure, same shape, so it read as a defect at HEAD rather than as an
artefact of how it was being run. **A reproducible failure is evidence about the whole setup,
not about the code**, and the cheapest control is to change the way you invoke it before
changing anything else.

### The test fixtures are generated, not committed

`testdata/*.pdf` is gitignored. Regenerate with:

```
uv run --with fonttools testdata/make_text_pdf.py testdata   # text-*.pdf, spike 0.3
python3 testdata/make_hostile_pdf.py testdata                 # hostile-*.pdf, spike 0.4
python3 testdata/make_vector_pdf.py testdata/vector-heavy.pdf # spike 0.1
python3 testdata/make_vector_pdf.py testdata/vector-multi.pdf 200000 12  # Phase 1
uv run --with pyhanko --with cryptography \
    testdata/make_incremental_pdf.py testdata                 # incr-*.pdf, spike 0.6
python3 testdata/make_outline_pdf.py testdata                 # outline-*.pdf, Phase 1
python3 testdata/make_rotated_pdf.py testdata                 # rotated*.pdf, Phase 1
```

`make_incremental_pdf.py` writes about **550 MB** — the scan fixtures exist so that
"appending to a 300 MB file is near-instant" can be tested at 300 MB, and they are
uncompressed on purpose so nothing downstream can make them cheap. An existing scan of the
right name is reused rather than rewritten. The signed fixtures need `pyhanko`, which is a
*test oracle* and not a dependency of tpdf: it both writes the signatures and is the only
implementation here that can validate them (`testdata/check_signature.py`). Without it
those five fixtures are skipped and the rest still build.

`make_hostile_pdf.py` also writes `hostile-manifest.json`, which records where each
fixture's needle is and whether a collected rewrite is expected to remove it; the Rust
harness reads that rather than hardcoding expectations. It shells out to `qpdf` for the
encrypted fixture, and needs `--preserve-unreferenced` there — without it qpdf collects the
orphan on the way in and the fixture arrives already sanitized.

A fixture whose needle cannot be found in the fixture *itself* proves nothing about any
rewrite, so `sanitize-rewrite` checks that first and says so. Two fixtures were vacuous on
their first run and had to be redesigned.

`make_text_pdf.py` embeds a **system** font, which is fine only because the output stays on
the machine. Nothing it generates may be committed or redistributed.

Its default font is deliberately a **serif** face, and changing that breaks the fixture in
a way that is invisible. PDFium's font mapper aliases Helvetica to Arial, so embedding
Arial made the substituted `base14` render **bit-identical** to the embedded ones --- three
fixtures that cannot distinguish "used the embedded subset" from "silently substituted",
which is the one thing they exist to test. Caught only because three different fixtures
hashed the same. If a fixture's baselines ever match across fonts, suspect that first.

`make_outline_pdf.py` writes `outline-manifest.json` the same way, and one of its
expectations is deliberately **weaker** than the rest. The unpaired-surrogate title is
marked `observed`, not `required`: PDFium may repair or drop it while parsing the document
string, in which case the fixture proves nothing about our decoder and asserting on it
would be a check that cannot fail. What pins the decoder is a unit test in `outline.rs`
that hands it the bytes directly, where the input is ours. **When a fixture cannot
guarantee it delivers the input a check needs, move the check to where the input is
controlled and say so in the manifest** --- rather than writing an assertion whose meaning
depends on a library's parsing.

Its hostile fixture is also verified by an independent oracle: `qpdf --check` reports
*"loop detected in /Outlines tree"* against it. A fixture built to be cyclic that no other
implementation agrees is cyclic is a fixture that might simply be wrong.

`make_rotated_pdf.py` writes two files and they are not interchangeable. `rotated.pdf` has
one page per `/Rotate` value, for `text-probe --mode align`, which takes a `--page` and needs
all four. `rotated-90.pdf` has every page at 90 because the viewer cannot use the first one:
its scroller lays every page out at page 1's size, so a document alternating portrait and
landscape is drawn wrong for reasons that have nothing to do with rotation, and a check run
on it would be measuring that instead. The second also carries an outline, which is what pins
the *destination* half of the same conversion --- including one entry with `/XYZ null 600 0`,
which exists so that the "this destination names no horizontal coordinate" path has an input
that reaches it.

`vector-multi.pdf` is twelve A0 pages sharing **one** content stream object, which is the
point rather than a shortcut: PDFium re-parses a page on every `FPDF_LoadPage` and rebuilds
per-page state on every render call, so twelve pages pointing at one stream cost twelve
times as much to draw while the file stays under 3 MB. It exists because the page strip's
yield needs a document whose *background* work outlives a moment, and the single-page
`vector-heavy` cannot provide one.

Its page count is load-bearing and was raised from three. The check suite visits page 1 and
the last page, so on a short document the viewer has already made a tier-1 placeholder for
every page and the strip borrows all of them --- rendering nothing, and giving the yield
check nothing to catch. **A fixture built to exercise a slow path has to be long enough that
the rest of the run does not warm it.**

---

### A worker killed a moment ago still says it is running

Found wiring the per-request deadline (2026-07-29). The supervisor SIGKILLs a worker whose
call has outrun its deadline, and the thread blocked on that call wakes up --- because the
pipe closes --- *before* the child becomes waitable. `is_running()` is `try_wait`, and for
those microseconds it answers "still running", so a deadline kill cannot be recognised by
interrogating the process: the corpse read as a healthy worker, was checked back in, and
failed someone else's request.

The fix is to make the kill legible from shared state rather than from the process: the
supervisor marks the in-flight entry *before* signalling, and the waiting thread reads the
mark (`CallWatch::end`). The general shape: **when one side kills and another side waits,
the fact of the kill has to travel by a channel neither of them can race** --- the process
table is not that channel, because death and waitability are two events with a gap between
them.

### The cleanup after an fd shuffle can close what it just installed

`dup` returns the lowest free descriptor, so the scratch copy made *before* a `dup2` shuffle
can land on one of the target numbers: with the document on 3, the tile on 5 and a hole at
4, `dup(doc_fd)` returns 4 --- which is `TILE_FD`. The later `dup2(t, TILE_FD)` overwrites
it, and the cleanup's `d != DOC_FD` test then closes the descriptor the shuffle just
installed. The worker dies on a closed fd, the retry respawns into the same layout, and the
document intermittently will not open --- as a function of the parent's fd-table holes,
which nothing logs.

The dup-first advice this file already carries is right; the trap is the cleanup undoing
it. A scratch is anything that is *not a target*, and the only safe way to say so is to
derive the test from the same table that drives the installs (`is_scratch(fd, shuffle)`
takes the `(source, target)` array itself), so the two can never drift apart.

### Repeating a race inside one process re-runs the first round, not the race

Found writing the `race` phase of `open_check.py` (2026-07-29), which issues two document
opens without awaiting the first and asserts the second one wins. Mutated so the queue is
gone, one round reported the defect in about two runs out of three --- which of two `invoke`
round trips lands last is genuinely a race, and the run where the right one happened to win
is indistinguishable from a correct build.

The obvious fix is to repeat the round and demand every one of them pass. It made detection
**worse**: five rounds per launch caught the same mutation in one run out of four. Only the
first round is cold. By round two the workers are warm, a document is already open, and the
two opens land in the same order every time --- so the extra rounds are not four more draws,
they are four copies of a draw whose outcome was decided by the state the first round left
behind. Pairing a slow document with a fast one did not help either, nor did a 336 MB one
against a 4-page one: the ordering is decided by IPC scheduling, not by what either open
costs.

Two things to take from it. **Independent draws need independent processes** --- the
repetition that buys something is separate launches, and it belongs in the driver, not in
the phase. And **a probabilistic check has to say so where it is defined**, with the
measured rate: this one is a smoke test that the application still routes opens through its
queue, while the property itself is pinned deterministically by a unit test of the queue
(`serial.test.ts`). Same family as the contaminated control already recorded here, arriving
through repetition rather than through phase order.

### A post-destroy guard that returns early leaks what it declined to take

The frontend's dominant defect class was a `.then` callback running after its object's
`destroy()`, and the first fixes were a `destroyed` boolean checked at the choke point --- 
correct for the continuations that carry nothing, which is how it was found (a viewer
restarting its own frame loop, a select-all retry outliving its document).

Applying the same shape to the *other* continuations would have been wrong, and it would
have looked right. `ImageBitmap` holds GPU-backed memory that is released by `close()` and
by nothing else, so a tile arrival that sees a dead scroller and merely returns leaks
exactly as much as the version that pushed it onto a queue nothing drains --- the leak is
the failure to release, not the queueing. Three such paths existed when this was written:
both of the scroller's arrivals, and both of the strip's, one of which is a *copy* the
strip makes of a bitmap the scroller owns.

Hence `Lifetime.claim(live, dispose)` with the disposal a **required** argument: the guard
cannot be written without saying what happens to the value it throws away. That is the
same "move the impossibility into the type" move this file records elsewhere, applied to
an omission rather than to a state.

Two things the mutation pass added. A test that a late arrival is *disposed* needs its
control --- an object that closed every arrival would pass it perfectly while drawing
nothing, so each disposal test is paired with one asserting a live arrival is kept. And a
guard whose *call site* is unreachable from the fixture is untested however thorough the
suite looks: the strip's borrow path never ran, because the test fixture's `placeholderFor`
returned null, and the mutation removing that disposal survived everything until a fixture
that actually borrows was written.

### A crate-root `#![cfg]` empties a `[[bin]]`, and cargo reports a missing `main`

`fdpass_probe.rs` opened with `#![cfg(target_os = "macos")]`, which reads as "this file is
macOS-only" and is exactly right for a module. For a binary target it is a trap: an inner
attribute applies to the crate root, so on any other platform *every* item goes away
including `main`, and cargo then says ``error[E0601]: `main` function not found in crate
`fdpass_probe` ``. That names the one thing the file unambiguously does have, so the first
reading is a parse failure or a misconfigured target rather than a gate that did its job
too well.

There is no attribute that fixes it in place: whatever removes the body removes the entry
point beside it. What works is a gated `mod` holding the body, and two `main`s, one on each
side of the gate. That means indenting the whole file --- let `cargo fmt` do it, rather than
hand-editing three hundred lines and reviewing the result.

### An uninhabited type carries its impossibility into every caller

Making `Shm` an uninhabited `enum` off unix is the "move the impossibility into the type"
move this file recommends elsewhere, and on its own terms it works perfectly: no mapping can
be constructed, therefore no worker can be, and the compiler enforces it rather than a
runtime check somebody could later forget to write.

It still had to be reverted, and the reason is the general lesson. `Worker` holds a `Shm`,
so an uninhabited mapping makes the *worker* uninhabited --- and uninhabitedness propagates
through every type that embeds it. rustc then correctly reported the pool's `retire_idle`
loop in `workers.rs` --- ordinary code, on a platform that never runs it --- as an
unreachable definition, with `mut` bindings that no longer needed to be mutable beside it.
Under `-D warnings` those are build failures, and the only repairs are `#[allow]`s scattered
through production paths that have nothing to do with the platform.

So the rule needs its second half: **move the impossibility into the type when that type is a
leaf.** A type other types embed carries its uninhabitedness into all of them, and the
diagnostics land where it is *used* rather than where the decision was made --- which is the
opposite of what the technique is for. The replacement is dull and local: a struct with a
private field, constructors that refuse, and accessors that panic rather than return a
plausible zero.

### A directory that exists is not the library you need

`pdfium_library_dir()` joined `vendor/pdfium/lib` and returned it if it existed. On Windows
that directory *does* exist --- the upstream archive puts the import library
`pdfium.dll.lib` there and the loadable runtime in `bin/pdfium.dll` --- so the check passed,
and the bind failed much later against a path that was sitting right there on disk. The
failure mode is worse than a missing directory, which would at least have fallen through to
the bundle branch.

The weak check survived because on macOS the two questions are the same question: the
directory that should hold the library does hold it. Ask for the **library**, not for the
directory that ought to contain it.

Note also which half of this a gate can see: none of it. `scripts/gates.py` compiles and
runs unit tests, and this is a path chosen at runtime --- so a fully green Windows gate run
says nothing whatever about it. `scripts/fetch_pdfium.py` had known the split all along and
its docstring even named this function as the one getting it wrong, which is its own small
lesson about where a fact gets written down versus where it is needed.

### A list of documented blockers can be wrong in the direction that looks thorough

`BUILD.md` carried three specific, checkable reasons the tree would not build on Windows,
one of them naming two files and the exact `cfg` they were missing. Running the gates found
something else: the **library** failed first with 18 errors in `worker.rs` --- `std::os::fd`,
`mmap`, `File::from_raw_fd`, `ExitStatus::signal` --- clippy never reached either named
binary, and the two `libc::` helpers were real but were the last thing to fix rather than
the first.

The list was not careless. It was written from reading the code and every item in it was
true. What reading cannot establish is what fails *first*, because that is a property of the
build graph rather than of the source, and a blocker list assembled by inspection reads
exactly like one assembled by running the build. Only the second can order the work or size
it.

The same section also stated that `TPDF_BACKEND=in-process` was "the only thing that runs
off macOS". That was false in a way inspection would not catch either: `pub mod worker;` was
unconditional, so the crate carrying that very control did not compile, and *nothing* ran off
macOS. A document describing a platform nobody has built on decays without anyone editing
it --- the code moves underneath it, and there is no run to contradict it.

**It has now happened four times, and the ratio is what makes it a rule rather than an
anecdote.** Later lists in `AGENTS.md` and `BUILD.md` named the harnesses and benchmarks that
were macOS-only. Running each one, on 2026-07-30:

| listed as blocked | actually |
|---|---|
| `pool-bench`, `prespawn-bench` | genuinely gated, on `#[cfg(unix)]` --- and *unrunnable* rather than degraded, since each re-execs itself as its own worker |
| `tile-bench` | never refused anything; failed only at `LoadLibraryExW` on a hardcoded `vendor/pdfium/lib` |
| `worker-bench` | seven modes genuinely blocked; the eighth (`--mode engine`) needs no worker at all and was trapped behind the module's `cfg` |
| `session_check.py` | needed **nothing** --- passed all four phases on the first attempt |
| `open_check.py` | four of six phases ran unmodified; two have no route and now skip with reasons |

So a documented blocker is roughly as likely to be absent as present, and the error is always
in the same direction --- over-reporting, because a list written by reading counts everything
that *looks* platform-shaped. **Run the thing before writing it down as blocked, and run it
again before believing an entry someone else wrote.** The cost of being wrong is asymmetric:
a false blocker hides work that is already done, and nobody goes looking for it.

### A gate list that never links a binary cannot see a link error

`scripts/gates.py` reported **7/7 on Windows** on 2026-07-29 while `npm run tauri build`
failed outright, and the two ran on the same tree minutes apart. The failure was
`backend_probe.rs` calling `_dyld_image_count` and `_dyld_get_image_name` --- dyld, so macOS
only --- with no `cfg` around them.

Neither of the two gates that look like they compile everything actually links one:

- **clippy stops at metadata.** `cargo clippy --all-targets` type-checks every target and
  never invokes the linker, so an unresolved external symbol is not a thing it can report.
- **`cargo test` links each `[[bin]]`, but with a different `main`.** Under `--test` the
  harness supplies its own entry point and the crate's `main` becomes unreachable, so
  anything reachable *only* from `main` is dead code the linker drops. `mapped_images` is
  called from `main`'s call graph and nowhere else, so the test-profile binary linked clean
  and the real one did not.

The gate list now carries `cargo build --locked --bins`, and the gate was proved to fail by
restoring the un-gated file and running it: red in **5.7 s** in the debug profile, which was
worth checking separately because the original observation was a release build and this whole
entry is about linking depending on how the target was built.

Two things generalise past this repository. **"It compiles" and "it links" are different
claims**, and a gate list assembled from `check`/`clippy`/`test` makes only the first while
appearing to make both. And a target that exists solely to be *run* by a human --- every
`bin/*_probe.rs` here --- has no test that would notice it stopped building, so it is exactly
where this hides.

### A custom URI scheme is not spelled the same way on every platform

`src/lib/tiles.ts` fetched `tile://localhost/...`. That is correct on macOS, where WKWebView
registers a real URI scheme, and resolves to nothing on Windows, where WebView2 cannot and
Tauri serves every custom protocol at `http://tile.localhost/...` instead.

**The symptom is not an error.** On 2026-07-29 the Windows viewer bound PDFium at 262 ms,
parsed the document, laid out twelve pages, fitted the page to the window, ran its frame
loop, scrolled, and reported `sharp=0.0%` on every check that asked whether anything had been
painted. Everything that does not need a tile worked. A blank page is what a viewer looks
like when its only failing subsystem is the one that draws.

The fix asks Tauri rather than keeping a second copy of the rule: `convertFileSrc("", "tile")`
is the same call the framework makes for this translation and yields the bare origin. It is
used for the origin *only* --- handed a whole path it percent-encodes the separators, and the
server splits on them, so an encoded URL is refused rather than mis-parsed, but refused on
every tile.

Worth noting how close the project already was to knowing this: the CSP in
`tauri.conf.json` **already carried `http://ipc.localhost`** beside `ipc:`. The convention was
understood and applied to one scheme and not the other, which is what a
platform-conditional spelling does when it is written out by hand in two places instead of
derived once. `img-src` and `connect-src` now name `http://tile.localhost` too --- a fetch
blocked by CSP fails exactly as invisibly as a scheme that does not resolve.

### A release build is not a production build; a cargo *feature* decides that

`cargo build --release` produced an optimised Windows binary that opened its window and
displayed the webview's own *"can't reach this page --- localhost refused to connect"*,
because it was still pointed at `devUrl` (`http://localhost:1420`).

The profile has nothing to do with it. `tauri`'s `build.rs` computes

```rust
let dev = !has_feature("custom-protocol");
```

and `tauri-build` turns that into the `dev` cfg that decides whether `generate_context!`
embeds `frontendDist` or points at the dev server. The Tauri CLI passes the feature; a bare
`cargo build` does not, at any optimisation level. Verified both ways rather than read off the
source: `cargo build --release --features tauri/custom-protocol` produced a binary that
passed the full viewer check, **84/84**, from the same tree that had just produced a
connection-refused window.

Note this repository has no `[features]` section, so the usual template alias
`custom-protocol = ["tauri/custom-protocol"]` does not exist here and the dependency feature
has to be named in full.

This is the Windows-shaped sibling of *A raw `cargo build` binary runs no webview content at
all*, and the cause is **different** --- that entry is WKWebView refusing to run a page for a
Mach-O with no bundle identity, this is the frontend never having been embedded. They are
worth telling apart because the macOS symptom is a silent blank window and the Windows one
names its own reason on screen, so the same mistake looks like two unrelated bugs.

### A guard that degrades to a no-op off its platform stops being a guard

`scripts/webview_guard.py` refuses to run a frame-driven check behind a lock screen, because
WebKit suspends an occluded page and the run then cannot even time itself out. Both of its
halves begin `if sys.platform != "darwin": return`, so on Windows `require_visible_session()`
returns `True` having checked nothing.

That is not obviously wrong --- the macOS mechanisms genuinely do not exist elsewhere --- but
the *hazard* does: Chromium throttles `requestAnimationFrame` for occluded windows too, and
WebView2 is Chromium. So the Windows runs in this session were protected by nothing, and a
future one that stops mid-check will present as a viewer defect rather than as an occluded
window. It has not bitten yet and is recorded before it does.

The shape to recognise: a cross-platform guard whose implementation is entirely inside one
platform's branch reads, at every call site, exactly like a guard that ran and passed. If the
condition cannot be tested on a platform, saying so --- `[SKIP]` with a reason, or a printed
warning --- is the difference between an unprotected run and an unprotected run nobody knows
about.

**It has bitten now, on 2026-07-30, and the real instance is worse than the one above.**
`workers.rs`'s `kill_pid` --- the *only* thing that enforces the render deadline --- was
`#[cfg(not(unix))] fn kill_pid(_pid: u32) {}`. Its own doc comment had named the trigger in
advance: *"a silent no-op is safe only because `Worker::spawn` refuses on those platforms ...
if a worker ever starts on Windows this has to become `TerminateProcess`, or the deadline
silently stops being one."* Workers started on Windows the day before, and nothing connected
the two.

Three things make it the sharpest version of this trap:

- **It lied rather than merely doing nothing.** `kill_overdue` counted the pid, set the
  `killed` flag, and printed *"worker killed for exceeding its deadline"*. So the caller got a
  deadline error, the log recorded a kill, and the process went on holding a hung PDFium render
  forever --- one leaked worker per hung document, with a line in the log saying otherwise. A
  no-op that stayed silent would have been easier to find.
- **The three tests that would have caught it were `#[cfg(unix)]` too.** So the platform where
  the guard had stopped working was also the platform where nothing tested it, and the suite
  was green. That correlation is not a coincidence: the same reflex writes both gates.
- **Nothing was inspecting it, and the prediction was in the source.** The comment did not need
  to be cleverer; it needed something mechanical to notice that its precondition had changed.

The fix was `OpenProcess` + `TerminateProcess(KILLED_EXIT)`, and un-gating the tests --- with a
`ping`-based sleeper, because Windows has no `sleep` and `timeout.exe` exits immediately under a
redirected stdin, which would have made every assertion pass for the wrong reason. Proved by
mutation: reverting `kill_pid` to the no-op turns
`the_supervisor_kills_the_process_holding_an_overdue_call` red with `ExitStatus(0)` --- the
sleeper ran to completion --- and takes the suite from 0.06 s to 5.08 s, which is the sleeper's
own lifetime and exactly what the test was designed to do instead of hanging.

One further finding from that mutation, worth keeping because it was a wrong prediction:
`a_deadline_kill_is_reported_to_the_thread_that_was_waiting` stayed **green** under it. That is
correct --- it checks the flag, which is set whether or not the signal lands --- but it means
that test is not a check on the kill, and the decoupling it reveals *is* the defect's shape.

### A harness that prints stderr only on failure hides what a passing run said

`viewer_check.py` echoed the child's stderr inside `if returncode != 0`, so a run that
passed discarded it entirely. That is the ordinary shape --- stderr is noise until something
goes wrong --- and it is wrong as soon as the program has anything to say about a run that
*succeeded*.

Found on 2026-07-29 immediately after adding a `[WARN]` for the uncontained backend: the
warning fired correctly, and a full-marks Windows run showed no trace of it. The first
reading was that the warning had not been emitted at all, which would have sent the next
hour into `default_here`. Running the binary directly, with stderr redirected rather than
captured by the harness, showed it on the first line.

The general form is worth holding on to, because the incentive runs the wrong way: a warning
is added precisely so that a *working* run announces something about itself, and the harness
convention of surfacing stderr only on failure is exactly calibrated to suppress that. Any
diagnostic whose purpose is "this run was fine, but you should know X" is invisible to a
gate that treats stderr as a failure artifact.

`viewer_check.py` now echoes `[WARN]` lines on a passing run and stays quiet about
everything else, so the webview's ordinary teardown noise does not come back with it.

### `CreateProcessAsUser` waives a privilege only for a token it still recognises

`CreateProcessAsUser` normally needs `SE_ASSIGNPRIMARYTOKEN_NAME`, which an ordinary
non-elevated account does not hold, and waives it when the token handed to it is a
restricted version of the caller's own. So a restricted-token spawn is supposed to work
without elevation --- and the first attempt failed with `ERROR_PRIVILEGE_NOT_HELD` (1314).

The cause was the *order* of two calls that look interchangeable. Duplicating the process
token and then calling `CreateRestrictedToken` on the duplicate produces a token Windows no
longer recognises as derived from the caller, so the waiver does not apply. Restricting the
process token first and duplicating the *result* --- which is Chromium's order, and looks
like an arbitrary stylistic difference until it isn't --- spawns fine.

Worth generalising past this API: when a documented waiver depends on a relationship
between two objects ("a restricted version of the caller's token"), every intermediate step
is a chance to break the relationship while preserving every visible property. Both tokens
here are primary, both are restricted, both have the same SIDs and privileges; only one is
accepted. `ERROR_PRIVILEGE_NOT_HELD` names the symptom and points at the wrong problem,
which is why the first reading was "this machine cannot do it" rather than "this derivation
cannot".

### A restricting SID stops the loader, and the code never runs

The rung above `lowil` does not fail *in* PDFium. It fails before `main`: a token carrying
`S-1-5-12` (`RESTRICTED`) as a restricting SID passes every access check twice, once against
the normal token and once against the restricted SID list, and system DLLs grant nothing to
`RESTRICTED`. The child dies at `0xC0000135`, `STATUS_DLL_NOT_FOUND`.

This is the exact inverse of the macOS ordering rule (`worker_child.rs`: bind PDFium
*before* `sandbox_init`, open the document after). There the boundary is applied by the
process to itself, so there is a "before" in which to load a library. On Windows the token
is chosen at `CreateProcess` time and is in force from the first instruction, so there is no
"before" at all --- which is why Chromium runs an *initial* impersonation token for startup
and drops to the lockdown token only once the DLLs are in. Any Windows worker that wants a
restricting SID owes that two-token dance; nothing simpler reaches it.

`lowil` has no such problem, and that is the practical finding: an integrity level governs
what a process may *write*, so the loader still reads. Measured on 2026-07-29, low integrity
renders **pixel-identical** to an uncontained render, denies writing to the user profile and
denies opening the parent process --- and still allows reading any path, which is stated in
the probe's own output rather than left to be discovered, because a report showing all three
denied would claim more than an integrity level buys.

### One failing rung cannot say which ingredient failed

The restricted-token rung combined two things: `DISABLE_MAX_PRIVILEGE` and a restricting SID.
When it failed, either was a plausible cause, and a plausible cause written into a document
becomes a fact nobody re-tests. Adding two diagnostic rungs that each differ from it by
exactly one ingredient settled it in one run: privileges-dropped-only renders perfectly,
SID-only fails identically to the pair. The restricting SID is the whole cause.

The cost was about twenty lines. The alternative --- publishing "restricted tokens do not
work" --- would have been wrong in the direction that looks thorough, which this file already
records as its own trap from the other side.

### A verdict that takes the last row that worked recommends the weakest one

The probe printed a ladder and then named "the highest rung that renders identically" by
taking the last successful row. With the two diagnostics inserted before `restricted`, that
was `noprivs` --- which renders perfectly and denies **nothing**: it allows writing to the
user profile, allows opening the parent process, allows reading any path. A verdict line
recommending it would have read exactly like the real answer.

Position in a list is not strength, and a summary that assumes it will confidently propose
the one configuration that buys nothing. Rows that exist to attribute a failure are marked
and excluded from the verdict; the ordering that makes them readable is not the ordering
that ranks them.

A relative of the same shape: `3221225781` and `0xC0000135 STATUS_DLL_NOT_FOUND` are the same
number, and only the second says what happened. The probe printed the first, so the single
most important result in the run looked like a value nobody would bother to look up. Decode
what a platform hands back, in the units the platform meant --- otherwise the finding is
present and unreadable, which is the same outcome as absent.

### A refusal that exists because nobody wrote the code is not a guarantee

`Shm`'s off-unix constructors all returned "render workers are implemented on macOS
only", and a test asserted it. That reads like a containment decision and was nothing of
the kind --- it was the absence of an implementation wearing the language of a policy. When
the Windows mapping landed, the test asserting the refusal had to be **deleted**, and that
deletion is the point rather than a casualty: what it pinned was the gap.

The tell is worth naming, because the two look identical from the call site. A real refusal
survives someone implementing the thing (`Worker::spawn` still refuses off macOS, because
the *sandbox* is what is missing and no amount of Windows code changes that). A placeholder
refusal disappears the moment the code exists. Ask which one a check is pinning before
trusting that it is about security.

The companion mistake was already latent in the suite. `spawning_a_worker_refuses_off_macos`
passed `"nonexistent.pdf"`, which was fine while *every* constructor refused with the same
sentence --- and became wrong the instant `map_file` worked, since a missing file then fails
one step earlier with "could not open". Its own doc comment had predicted this ("would still
have to hold if `Shm` ever grew a Windows implementation") without noticing that its fixture
would not. A check whose input is invalid for the code under test can pass for years on the
strength of an unrelated error.

### The kernel refuses a writable mapping of a read-only file, on both platforms

The POSIX side records that `mmap` rejects `PROT_WRITE` over a read-only descriptor with
`EACCES`, and that the kernel refused it before the threat model did. Windows does the same
thing through a different door: `CreateFileMapping` with `PAGE_READWRITE` over a handle
opened by `File::open` fails with `ERROR_ACCESS_DENIED`.

Worth writing down because it makes an otherwise untestable property testable. "A document
mapping is read-only" sounds like it needs a write to prove, and a write to a read-only view
is an access violation that takes the whole test process with it. It does not need one: the
*constructor* fails, so a mutation swapping `PAGE_READONLY` for `PAGE_READWRITE` turns an
ordinary round-trip check red with a legible message. Look for the layer that refuses early
before concluding a safety property can only be demonstrated by violating it.

### A mutation caught by an access violation produces no test results at all

Stripping `FILE_MAP_WRITE` from a writable view is caught --- the check that writes to the
mapping faults, `cargo test` exits `0xC0000005`, and the gate goes red. But it produces
**zero** `test result:` lines, because the process dies before the harness can summarise.

Grepping for `FAILED` therefore reports nothing, which is indistinguishable from a mutation
that survived --- and "survived" is the most misleading verdict a mutation pass can give,
since it reads as a gap in the tests rather than a crash in the run. This file already
records that a harness needs positive evidence a run happened; this is the instance that
proves the rule is not theoretical. Count the result lines and treat zero as a broken run,
distinct from a clean one.

### A comment claimed an ordering mattered, and the mutation that should have hurt did not

`Shm::drop` unmaps the view before closing the section, and said the reverse order "leaks the
view rather than failing, and nothing reports it". Reversing the two left all fifteen checks
green --- and the comment, not the test suite, turned out to be what was wrong: Windows keeps
a mapped view valid after its section handle is closed and holds the backing open until the
last view is unmapped.

The rule this file already carries is that a guard no mutation can break is usually a guard
to delete, after asking where its impossibility is enforced. There is a fourth case, and it
is this one: the guard is fine and the *justification* is false. Deleting the code would have
been wrong, and leaving the claim would have been worse --- the next person reads a stated
invariant, believes ordering is load-bearing, and preserves it somewhere it genuinely is not.
The order is kept because it mirrors the POSIX side and reads acquisition-order, the comment
now says exactly that, and it says no test pins it.

### "Inherit nothing" cannot be spelled as an empty handle list

`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` narrows inheritance to an explicit set, so the obvious
way to say "this child gets no handles" is an empty set. `UpdateProcThreadAttribute` refuses
it with `ERROR_BAD_LENGTH` (24).

That is not a quirk to route around, because the fallback is the dangerous direction:
`bInheritHandles: TRUE` with no attribute list inherits **every** inheritable handle the
parent holds, which for a viewer that has other documents open is the exact opposite of what
was asked for. An empty request has to reach `CreateProcess` as `bInheritHandles: FALSE` and
no extended startup info at all. It is modelled as `Option<AttributeList>` rather than an
empty `Vec` so a caller cannot construct a request Win32 has no way to represent.

Written here because the check asserting an empty list *would* build was written first, and
was wrong. So was its neighbour: `SetInformationJobObject` rejects a zero
`ProcessMemoryLimit` with `ERROR_INVALID_PARAMETER` (87), where the guess had been that the
kernel would accept it and instantly kill every worker. Two assumptions about what Windows
tolerates, both wrong in the same session, both cheap to settle by running the call.

### A safe function taking a raw `HANDLE` has an unstated contract, and clippy says so

`Job::assign(&self, process: HANDLE)` and `make_inheritable(handle: HANDLE)` were safe
public functions over `*mut c_void`. `clippy::not_unsafe_ptr_arg_deref` denies that by
default, and it is right in a way worth internalising rather than silencing: nothing in the
type distinguishes a live handle from a closed or forged one, so the obligation is real and
belongs in the signature where a caller has to acknowledge it.

The tempting repair is `#[allow]`, on the reasoning that a `HANDLE` is "just an integer" and
Win32 validates it. Win32 validating it is exactly the problem --- a closed handle value gets
recycled, so the failure is not a clean error but an operation applied to *someone else's
object*. Marking the function `unsafe` costs one `unsafe {}` at each of three call sites and
turns an invisible assumption into a written one.

### A test whose failure is a hang reports a pass and a timeout in one breath

Windows containment, 2026-07-29. `Contained::kill` terminates the job; a lifecycle test
killed a `cmd /c pause` child and then called `wait()` to read its exit code. Mutating `kill`
into a no-op did not turn that test red. It made the run take **177 seconds** against a
180-second harness timeout, and the harness then printed `test result: ok` *and* `[HUNG]` in
the same output --- one line from the tests that did finish, one from the timeout that killed
the process reading them.

Two distinct defects, and only the second is about mutation testing.

**The assertion could not fail.** `wait()` is unbounded, so "the child exited" has two
outcomes: it passes, or it blocks forever. A blocked test is not a failing test --- it is a
suite that never finishes, diagnosed eventually by whatever timeout notices, on a harness
that by then cannot say which check was to blame. Any assertion of the form *do X, then wait
for the consequence* needs a **bounded** wait, so the consequence not happening is a red and
not a wall. `Contained::wait_timeout` exists for that and `wait()` is now the wrong tool for
a test to reach for. Rerun with the bound: red in 10.02 seconds, naming the test.

**The harness's own verdict was self-contradictory and it did not notice.** It grepped for
failure lines and separately checked its exit code, then printed both findings without
comparing them. `test result: ok` beside `[HUNG]` is not two facts, it is one broken run, and
a harness that can derive the same fact two ways has to say so when they disagree --- the
same cross-check `AGENTS.md` already records from the padded-column case, arriving from the
timeout side instead of the parser side.

Related: *A check whose failure mode is a wait cannot fail*, which is the same shape found in
the viewer; *A timeout that discards the transcript recreates the failure it was added to
diagnose*; and *A mutation harness needs the same control as the thing it is testing*.

### `GetExitCodeProcess` reports 259 for a live process, and 259 is a legal exit code

`STILL_ACTIVE` is `259`. So the obvious way to ask whether a child has finished --- call
`GetExitCodeProcess` and compare against `STILL_ACTIVE` --- is wrong for exactly one input,
and that input is an exit code a process is entitled to choose. A worker that really did exit
259 reads as running forever, and a pool waits on something that is gone.

The fix is not a better sentinel, because there isn't one: liveness has to come from a
*different* mechanism. `WaitForSingleObject` with a zero timeout answers it exactly
(`WAIT_OBJECT_0` versus `WAIT_TIMEOUT`), and the code is only read once that has said the
process is finished. `Contained::try_wait` is that, and `wait_timeout(0)` is that, and the
second is written in terms of the first so the two cannot drift.

Testable, cheaply, and worth doing: spawn `cmd.exe /c exit 259` and assert `try_wait` returns
`Some(259)`. The wrong implementation passes every other test in the file.

### A pipe reaches EOF before the process it belonged to is signalled

`read_reply` gets end of file, concludes the worker is gone, and asks for its epitaph --- which
answers **"still running"**, about a process that has already exited. Both halves are correct
and they disagree because they observe different objects: the pipe reaches EOF when the child's
last write handle closes, which happens while the process is tearing itself down, and the
process object becomes signalled only when that finishes. The gap is microseconds, and the
epitaph is asked inside it every single time, because EOF is what prompts the question.

"Still running" is the one answer that sends a reader in the wrong direction --- to look for a
worker that is wedged rather than one that died. So the fix is a **bounded** wait rather than a
zero-timeout poll: `Contained::epitaph` waits `EPITAPH_GRACE` (100 ms), which cannot hang, only
runs on a path where something has already gone wrong, and still says "still running" about a
worker that genuinely is one. Liveness polling (`is_running`) keeps the zero timeout; only the
diagnostic gets the grace.

Found by a test written for something else entirely --- it was checking that a dead child is
reported rather than waited on forever, and it went red on the wording rather than on the wait.
Related: *`GetExitCodeProcess` reports 259 for a live process*, which is the other half of "do
not ask one mechanism a question the other should answer".

### A test whose child never answers cannot see the pipes being crossed

`spawn_mapped` hands the child the read end of one pipe and the write end of the other, and
getting that backwards is a plausible edit --- four handles, two of them named for the side that
does *not* own them. A mutation swapping the pair **survived the whole unit suite**.

It survives for a reason worth generalising: under `cargo test`, `current_exe` is the test
harness, which has no worker dispatch, so the child exits immediately. Every check that spawns
one is therefore a check about the *lifecycle* --- does the parent notice, does it name the
cause --- and a lifecycle is identical whichever way the pipes point. Nothing in a suite whose
child never speaks can distinguish a channel from a crossed channel.

What does catch it is `worker-probe`, where a real worker answers: the first request fails with
a broken pipe and the run stops on check one. That was **measured, not assumed** --- the
mutation was applied, the probe rebuilt, and the `[FAIL]` line read --- because "the integration
probe covers it" is exactly the sort of claim that turns out to be false. So the division is
real and should be stated where someone might otherwise add a redundant unit test: `cargo test`
owns EOF propagation and epitaphs, the probe owns direction and content.

### A wait for a condition that cannot hold spends its whole bound, and retires the pool it was about to measure

`backend-probe`, ported to Windows, reported **6 workers from 1** for a burst and then, 1.2 s
into a 4.0 s idle timeout, **1 of 6** still alive. Beside it the descriptor check reported
**144 handles with one worker, 144 grown, 144 retired** --- and five extra worker processes,
each costing the parent two pipe ends, a process, a thread, a job and two section handles,
cannot cost zero. Two independent observations, agreeing, and the conclusion drawn from them
was that the workers were created, used and **destroyed rather than pooled**.

That conclusion was wrong, and it was written into three documents as an open defect for a day.

Both readings were honest. What neither could say is **when** it was taken. The sample sat
behind `settled_descriptors`, whose wait is `!spare_pids().is_empty() && spares_settled()` under
a five-second bound. Windows never pre-spawns --- `Worker::prespawn` refuses, because a child
there is handed its document at `CreateProcess` and one started before a file is chosen has
nothing to receive --- so `spare_pids` is empty for the life of the process and that wait could
never succeed. It spent its full bound on every call. Five seconds is longer than the phase's
own four-second idle timeout, so **the instrument retired the pool it was measuring, and then
measured it.** One worker of six, and a handle count back at its lean value, are exactly what a
correctly working pool looks like five seconds after a burst. Guarding the pid clause behind the
platform turned 34/41 into 36/41 with nothing in `workers.rs` touched.

Four things worth keeping, and the first is the one that would have ended it in a minute:

- **A helper that polls until a deadline returns a verdict, and discarding it converts "never
  happened" into "took a while".** `settle_for` returned `false` three times per run and nobody
  asked. A timing print was all it took --- `false after 5.00 s, spares []` --- and it was the
  first thing tried after the conclusion was doubted rather than the last. Any wait whose
  expiry is not an error should say so out loud; `settled_descriptors` now prints a `[WARN]`
  naming the bound it waited out.
- **Two agreeing observations still share every assumption the sampling makes.** The pid list
  and the handle count are genuinely independent of each other and were taken through the same
  five-second delay, so their agreement measured the delay, not the pool. Independence is a
  property of the *observation*, not of the two quantities.
- **Cross-check the elapsed time, not only the values.** A phase that reasons about a 4.0 s
  timeout and a 1.2 s control is making a claim about a clock, and nothing in it read a clock.
- **A `cfg!` for a platform fact wants a name, because a second reader will appear.** The same
  distinction --- this platform does not pre-spawn --- was already spelled out inline where the
  spare-lifetime check is skipped, correctly, and was simply missing here. It is
  `PRESPAWNS` now, in one place. See the trap about two copies of a distinction drifting; this
  is that trap arriving as a copy that was never made.

Resolved 2026-07-29. The pre-fix run is the red control for both checks: they were observed
failing and are now observed passing, with `RETIRE_DOWN` (retirement still happens: 1 of 6 left)
green on both sides, so the fix did not simply make them unable to fail.

### A check that wins a race on one platform has not been shown to pass on it

`backend-probe`'s descriptor check opens one document, closes it, and asserts the count comes
back to where it started. It sampled with a raw `open_descriptors()` and had passed on macOS
since it was written. Pre-spawning reached Windows and it went red the same hour: **137 quiet,
145 with it open, 142 after closing it** --- five handles, which is one spare's worth.

Nothing leaked. An `open` **consumes** the pool's warmed spare and starts a replacement on
another thread, so whether a sample includes one spare's handles depends on how far that thread
has got. On macOS the replacement is a `fork` and is up before the next sample; on Windows it is
a `CreateProcess`, a token, a job and a fresh map of `pdfium.dll`, and it is not. The check had
been reading a race it happened to win.

The fix was the wait that already existed for exactly this --- `settled_descriptors`, whose own
doc comment describes the miss as "a leak of exactly that size" --- applied to three call sites
that predated it. What is worth carrying past the instance:

- **"It passes on macOS" is evidence about macOS's timing, not about the check.** A concurrency
  bug in a test is invisible until something changes the timing, and a second platform is the
  cheapest thing that ever will. Treat a check that goes red *only* on the new platform as a
  question about the check first and the platform second --- here the platform was innocent
  twice in two days, and the first time cost a phantom defect in the pool.
- **A helper written to bracket a race has to be used at every sample, not at the alarming
  one.** The three raw calls were older than the helper and nothing pointed from one to the
  other. If a sampling helper exists because a naive sample is wrong, the naive call is the
  thing to grep for when it is written.
- **The delta that shows up is the size of the thing you forgot**, which is the fastest way to
  identify it: five handles is one worker, and one worker that nobody asked for is a spare.

### `eprintln!` is not one write, and every worker shares the parent's stderr

A `pool-bench` run of about 120 worker processes ended with a line reading exactly `[worker] `
--- the prefix of the diagnostic a dying worker prints, with no message behind it. That reads as
a worker that failed with an empty reason, which is precisely the silent death the line exists to
rule out.

It is not. Every error path reaching that `eprintln!` was checked and all of them produce
non-empty text, and it did not recur across two runs with stderr on a handle of its own,
including the same corpus and the same pool sizes. The mechanism is the macro: Rust's stderr is
**unbuffered**, and `write_fmt` issues a separate write per format piece --- the literal, then the
argument, then the newline. Every worker of every pool inherits the same handle, so those writes
interleave between processes and a reader can be left holding one piece.

Three things follow, and the third is the one that generalises furthest:

- **A diagnostic that must survive is one `write_all` of one `String`.** `format!` first, write
  once. It costs an allocation on a path that is about to call `exit`.
- **Two capture channels are not equivalent, and the convenient one is the worse one.** The
  fragment appeared under PowerShell `> file 2>&1`; `Start-Process -RedirectStandardError` on a
  handle of its own did not show it. When a diagnostic looks malformed, re-capture before
  believing it --- `AGENTS.md` already records piping through `tail` for the same reason, and this
  is that hazard arriving through a shell redirect instead.
- **The failure mode is the worst-shaped one available**: an interleaved fragment does not look
  like corruption, it looks like a *finding* --- a worker with an empty error. A channel that can
  drop half a message will eventually drop the half that carried the meaning, and what arrives is
  a plausible bug report about something else.

### A symbol scan needs symbols, and the Windows PDFium has none

`worker-bench --mode engine` is the check behind the threat model's strongest sentence: the
vendored PDFium has zero `v8::` symbols and zero `CXFA_` symbols, so "document JavaScript is
disabled" is not a policy but a property of the binary. It lived inside a `#[cfg(unix)]` module
for no reason --- it spawns nothing, it reads a file --- so it had **never been run on Windows**,
and the claim was untested there rather than merely unmeasured.

Moved to file scope and run, it reports `[NOT VERIFIED]`. The shipped `pdfium.dll` carries no
local C++ symbols: `CPDF_Document` is absent, so `v8::` and `CXFA_` being absent from it says
nothing at all. The harness's own second control catches this exactly as designed --- the entry
above it says "without the second, every absence is *not verified*, not *not present*" --- which
is the check working, and also the finding. On Windows the no-engine property rests on the asset
name and pinned digest, which is a claim about *which file was fetched* rather than about what is
in it.

Four things worth keeping:

- **A check that cannot run on a platform is not neutral there; it is a claim nobody has
  tested.** The `cfg` gate was on the *module*, so the one mode that needed no worker went with
  the seven that did. Look for portable pieces trapped behind a platform gate they never needed.
- **Print the dimension that survives, and print it before the early return.** The export table
  is always named, because a loader must find it. It was originally printed *after* the
  stripped-binary exit, so the one run that most needed it showed nothing.
- **Windows exports four XFA-named functions**, and they are surface rather than a
  contradiction: `FPDF_GetXFAPacket{Count,Name,Content}` read `/XFA` streams out of an AcroForm
  dictionary and need no XFA engine. Whether `FPDF_LoadXFA` is a stub is open --- and it is
  behaviourally decidable, unlike JavaScript, because a fixture with an `/XFA` packet gives
  `FPDF_GetXFAPacketCount > 0` as a positive control. The entry above says the property "cannot
  be tested behaviourally"; that is true of JS and over-generalised to XFA.
- **Two independent parsers agreeing is the cheapest confirmation available.** The Rust export
  reader and a throwaway Python PE parse both said 460 exports and the same four XFA names. That
  took a minute and is worth more than re-reading the Rust.

### `cargo fmt` was blamed for mangling a string, and it was innocent

A refusal message came out with ten-space runs inside it --- `socket          pair` --- and the
obvious suspect was the formatter, since the string had been written with `\`-continuations and
`cargo fmt` had just run. Testing that took one command: a file with the same shape, `rustfmt`
run on it, `diff` clean. **rustfmt does not touch string contents.**

The mangling was self-inflicted, from generating Rust *through* a Python heredoc. A `\` before a
newline is a line-continuation in a Rust string literal **and** in a Python one, so a fragment
crossing three escaping layers --- shell heredoc, Python literal, Rust literal --- can have its
continuation consumed by the wrong one. Python ate the newline and left the indentation, so the
Rust source contained one long line with the indent baked in as spaces.

- **`concat!` with one literal per line is immune**, has no continuations to lose, and is stable
  under the formatter. Use it for any long message, and especially for one being written by a
  script.
- **The failure is silent and plausible.** The string still compiles, still reads correctly at a
  glance, and only prints wrong. Nothing fails.
- And the meta-lesson, which cost the smaller half of the time: **verify the tool before blaming
  it.** The accusation was one command from being disproved, and "the formatter corrupts string
  literals" would have been a memorable and completely false trap in this file.

### A harness that prints as it goes writes nothing until it exits, under a redirect

`BUILD.md` says of the check scripts that "results otherwise print as they are produced, so a
run that stops partway names the last check it completed". That property is real in a terminal
and **false the way these are actually run.** Python switches stdout from line-buffered to 4 KB
block-buffered the moment it is not a tty, which a redirect guarantees, so
`open_check.py > out.txt` held **zero bytes** for its entire twelve-minute run --- indistinguishable
from a script that died at import.

This is the sibling of *A harness that prints only at the end cannot say where it stopped*, and
the distinction matters: there the harness was at fault, here the harness is right and the
**producer's buffering** undoes it. Both end at the same place --- a partial run that cannot say
where it stopped --- so a reader chasing one will not think to check the other.

- **The fix is four characters of intent**: `sys.stdout.reconfigure(line_buffering=True)`, in
  `scripts/live_output.py`, called explicitly by each harness rather than as an import side
  effect. `python -u` and `PYTHONUNBUFFERED=1` also work and are worse, because they are
  properties of the invocation and every future caller has to remember them.
- **A/B it rather than reasoning about it.** Same script, same 4 s mark, one env flag apart:
  **0 bytes against 38**. That took one command and is the difference between a fix and a
  plausible change.
- And the meta-point, because this hazard was *already written down* in the cross-repo memory
  and was walked into anyway: **a caution that has to be recalled at the right moment loses to
  a line of code that cannot be forgotten.** The note had been read in this same session.

**And the very next run paid for it, in the direction that matters.** With the fix in place an
`open_check.py` run streamed its phases and finished in **45 s**. The attempt immediately before
it --- same script, same arguments, no buffering --- sat at zero bytes for **17 minutes** before it
was inspected, and the app it had launched was using **0.00 CPU**: hung at the first phase, almost
certainly the occluded-window suspension this file warns about for that script. Without streaming,
"hung at phase one" and "still working through four race launches" produce the identical empty
file, so the natural move is to keep waiting. The process table is what settled it --- **a child
holding 0.00 CPU is the tell**, and it is worth checking before extending a timeout.

**Three scripts still had the hazard a year later, and one of them cost a verdict.**
The fix reached `viewer_check.py`, `open_check.py` and `session_check.py` and stopped
there; `mutate_frontend.py`, `mutate_rust.py` and `mutate_viewer.py` never got the line.
On 2026-08-19 a full frontend run --- 276 mutations, about fifty minutes --- sat at **three
lines** for its whole duration under `> file 2>&1`, so there was no way to tell a run in
progress from one that had died at its third mutation. An earlier attempt the same day,
piped through a filter and wrapped in `timeout`, produced **nothing at all**: the timeout
killed it, the buffer went with the process, and the only evidence that fifty minutes had
been spent was the wall clock.

That is the same failure from the other end. The first time it was a run that looked dead
and was fine; this time it was a run that looked fine and was dead. All three harnesses
call `stream_results()` now.

The general lesson is not about buffering. **A fix applied to the scripts that had the
symptom leaves the ones that merely have the defect**, and nothing distinguishes them until
somebody redirects the next one.

### A `DataWriter` closes the stream it was created over, so a helper that returns the stream returns a closed one

WinRT has no "load a PDF from a byte slice" entry point: everything goes through
`IRandomAccessStream`. So `print_win.rs` has a `to_stream(bytes) -> Option<InMemoryRandomAccessStream>`
that creates a stream, wraps it in a `DataWriter`, writes, stores, flushes and returns the
stream. Every document was then refused by `PdfDocument::LoadFromStreamAsync`.

A `DataWriter` **owns** the output stream it was constructed over and closes it when released
--- which, in Rust, is the end of that function. The stream handed back was closed, and the
only symptom is the loader saying it cannot read the document. `DetachStream()` before
returning is the fix, and it is one line that looks like tidying up.

Two things about *how* it was found are the transferable part:

- **The identical code worked when written inline in a probe**, because there the writer was
  still alive when the stream was used. Moving working code into a helper broke it. Any WinRT
  type that wraps a stream (`DataReader` too) has this lifetime, and Rust's `Drop` hides the
  moment it fires.
- **The failure was first observed on our own hand-rolled test PDF, and believing that would
  have sent the fix into the fixture generator.** What settled it in one command was retrying
  with a known-good fixture and then with the raw calls inline: `LoadFromStreamAsync` returned
  `Ok(Ok(1))` on the same bytes the module refused. "Our test input is malformed" and "our
  helper is broken" produce the same error message, and only an input you did not write can
  separate them.

### WinRT reports a PDF page's size in DIPs, not points

`PdfPage::Size()` answers in device-independent pixels at **96** to the inch. A PDF page is
defined in points at **72**. So A4 --- 595x842 by definition --- comes back as 793.33x1122.67,
and computing a render scale as `dpi / 72.0` asks for a page 96/72 too large in each dimension.
It obliges: a 200x100 page rendered 267x133.

What makes this worth an entry rather than being an arithmetic slip is that **it is invisible
downstream.** The error is a uniform 1.33x, so the page still renders, still has the right
aspect ratio, and is still scaled down to fit the sheet by the print path --- so the printed
output would have been very slightly soft and nothing else, on a path whose only honest
verification is paper.

Caught by asserting pixel dimensions at **two** resolutions, which is a check on the *units*
rather than on the picture: with the constant wrong both rows are 1.33x out, with it right both
are exact. A single row cannot tell a wrong scale from a wrong unit.

### A BMP's DIB header is never 4-byte aligned, so reading it in place is undefined behaviour

The Windows print path asks WinRT for a **BMP** rather than the default PNG, because a BMP is a
DIB with a 14-byte `BITMAPFILEHEADER` in front of it --- so the bytes after that header go
straight to `StretchDIBits` and the module needs no image decoder. The obvious way to get the
`BITMAPINFO*` that GDI wants is to cast a pointer into the buffer at offset 14.

Offset 14 is never a multiple of 4, and `BITMAPINFO` is a struct of `u32` fields. Rust's debug
assertions caught it immediately --- *"misaligned pointer dereference: address must be a
multiple of 0x4"* --- and the way it reports is the part to remember: it is a
**non-unwinding** panic, so it aborted the entire test binary with `STATUS_STACK_BUFFER_OVERRUN`
rather than failing one test. A whole suite disappearing is a much worse signal than one red
line, and on x86 the read would have "worked" in release.

The fix is to copy the header into storage that is aligned by construction --- a `Vec<u32>` ---
and copy up to the header's own **declared** `biSize` plus whatever sits between it and the
pixels, not a fixed 40 bytes: `BITMAPV4HEADER` and `BITMAPV5HEADER` are longer, and the colour
masks of a 32-bit image live in that gap and are read through the same pointer.

### A DIB pixel is not a device unit, and every page printed at half size while a check passed

`StretchDIBits` takes a destination rectangle in the DC's own units. A printer DC's units are its
device dots --- 600 per inch on a typical laser --- and the DIB handed to it was rendered at
`PRINT_DPI = 300`. The first version of `draw_bmp` computed the destination as
`min(sheet / dib_size, 1.0) × dib_size`, i.e. one DIB pixel per device unit, so **every page came
out at exactly half its physical size**, centred, with a wide even margin. It looks deliberate.
For a page small enough that the fit-scale never engages there is nothing to correct it either.

The reason this is the most useful entry from that session is not the arithmetic, it is that a
check was watching and said nothing:

- `print-probe` compared **printed ink against sent ink** and read `0.49`. That is an entirely
  plausible number for a path that rasterises twice and scales down, so it passed. An oracle
  whose expected value is "roughly less" cannot distinguish correct from half.
- The same oracle then read `0.01` on one A0 page and *failed* --- for no reason but the paper
  being 16× smaller in area than the page raster (1192x1685 at 99.9% saturation against 298x421).
  So it produced a false pass on a real defect and a false failure on correct behaviour, in the
  same session, from the same formula.
- Replacing it with "ink spans more than 0.7 of the sheet" then failed `rotated.pdf` **for being
  right**: this path prints a page at its true size and only shrinks it to fit, so a small page
  occupying a third of an A4 sheet is the correct answer, not a clipped one.

What works is a **prediction**: the printed ink extent should equal the source ink extent scaled
by the page-to-sheet ratio, with the same down-only uniform fit. Margins cancel, because both
sides measure ink rather than page edges; it is scale-invariant; and it is derived from the source
page's inches and the driver's own sheet size rather than from the drawing code, so it is not a
restatement of the thing under test. It reports 1% and 0% error on the two `rotated.pdf` pages,
and **48%** against the reverted bug --- one half, named as such.

The general shape, worth carrying past printing: when a transformation should preserve a
*geometric* relationship, assert the relationship against a computed expectation. A tolerance band
on a *quantity* ("within an order of magnitude", "roughly conserved") admits every scale error,
which is usually the family of bug being made.

### A print check that counts pages cannot see a blank page

`examples/print_probe.rs` drives the whole Windows print path to a real spooler --- "Microsoft Print
to PDF", with `DOCINFOW.lpszOutput` naming a file so the driver writes instead of raising a save
dialog --- which makes everything except the panel itself verifiable without paper.

The temptation is to check the page count of what came out. It is exactly the wrong check: a
wrong `BITMAPINFO`, a DC in the wrong mapping mode and a bad `StretchDIBits` rectangle all
produce **the right number of perfectly empty sheets**. Proved rather than argued --- mutating
`draw_bmp` to skip the blit leaves *"the printed output has the pages that were sent"* green,
and only the ink checks go red, at `[0, 0]`. The output file shrank from 721,222 bytes to 1,183,
which is the same tell from the other side.

So each printed page is rendered back and its ink counted, with **the pages that were sent as
the control**: source pages with no ink would make the printed pages' ink unfalsifiable, and
"both zero" is precisely how this check would otherwise pass on a completely broken path. The
comparison is an order-of-magnitude band and not an equality, because the page is legitimately
scaled onto the sheet and rasterised twice (measured at 0.49 of the source's ink).

### Printing maps a PDF parser into the app process, on both platforms

`docs/THREAT-MODEL.md` and `AGENTS.md` say the app process never maps the PDF parser, proved by
reading the loader's image table from outside. That claim is about **PDFium** and it stays true
--- but printing parses the job *in the app process* on both platforms, with PDFKit on macOS and
`Windows.Data.Pdf` here, so a PDF parser is mapped in whenever someone prints.

That is deliberate and it is the same trade on both sides: the readback wants a parser
independent of the `lopdf` that wrote the job and the PDFium that drew what the reader saw, and
the platform's own stack is the only such parser available. What the boundary actually buys is
narrower than the sentence suggests, and worth stating in exactly these terms: the process
holding the print job never maps **our** PDFium, so a PDFium bug reachable from a crafted
document is not reachable through printing, and the parser that *is* mapped is patched by
Windows Update rather than pinned in our `Cargo.lock`.

Measured rather than asserted, since a containment claim that nobody checked is the thing this
file exists to prevent: `print-probe` reads its own module table after parsing, rendering and
printing, and reports **80 modules mapped, none named pdfium** with `Windows.Data.Pdf.dll`
printed beside it as what it mapped instead. The count is there for the reason recorded
elsewhere here --- an enumeration that returned nothing looks exactly like an absence.

### A print DPI relative to the page is the wrong quantity, and A4 is the example that hides it

`print_win.rs` rasterises each page at `PRINT_DPI = 300`, and the constant's doc comment
justified the number against A4: 2480x3508 pixels, about 35 MB as 32-bit BGRA, which is a
reasonable buffer for a page.

A0 is 33x47 inches. At 300 dpi that is 9933x14043 --- **532 MB per page** --- for a sheet that
can display 9 MB of it, because the page is scaled down to fit the paper and every pixel beyond
what the sheet holds is rendered, paid for, and thrown away by the scaler. `print-probe` on
`vector-multi.pdf`, twelve A0 pages, did not finish in two minutes.

The fix is not a cap but the right quantity: **render at the resolution that yields `PRINT_DPI`
after the fit.** A page twice the sheet's size renders at half, and lands on paper at exactly
the same density as one that fits. With a floor, because a pathological page must still print
something legible rather than a thumbnail stretched over a sheet.

The transferable part is how the mistake survived review: the doc comment did the arithmetic,
and did it for the page size that makes the constant look sensible. Any per-item buffer sized
from *input* dimensions wants its worst realistic input in the comment, not its typical one ---
and this repository has an A0 fixture precisely because A0 is the case that breaks things.

### The OS's PDF rasteriser is not fast, and a raster print path inherits that

Measured with `print-probe` after the DPI fix above, so this is not the buffer problem: one A0
page of `vector-heavy.pdf` --- 200,000 vector operations --- takes **minutes** for
`Windows.Data.Pdf` to rasterise, at ~75 effective dpi, with a working set of about 110 MB. It is
compute-bound on the operation count, essentially independent of resolution.

For scale, `tile-bench` measures PDFium at **35.1 s** for a full A0 page at 1x on this machine,
so the numbers are the same order and the content is the problem rather than the API. But the
architectural consequence is one-sided: macOS hands PDF bytes to `NSPrintOperation` and the print
system consumes them as **vectors**, so it never rasterises at all and pays none of this.
Windows has no in-box PDF print API, so the raster path is not a choice and neither is its cost.

What follows, stated so it is not rediscovered as a bug: **printing a large-format CAD drawing on
Windows takes minutes and macOS does not.** The route out is not a faster rasteriser but avoiding
rasterisation --- `IPrintDocumentPackageTarget` and the XPS pipeline can hand PDF to a printer
that understands it directly, which GDI cannot express. That is a real piece of work and is not
started.

### A page count read too early is 0, and 0 is not a count

`session_check.py` drives a document to `TARGET.page = 7` and then asserts the restore lands
there. `Viewer.goToPage` **clamps** to the last page, so on a document with fewer than eight pages
the check reported *"it opens on the remembered page: page 0, wanted 7"* on a one-page fixture and
*"page 2, wanted 7"* on a four-page one. Stably, reproducibly, and on a session restore that was
working perfectly — verified afterwards at 7/7 on a twenty-page document.

So the check needed to state its precondition. The first attempt read the count straight after
waiting for the viewer to exist, and reported **"0 pages"** for a document with 1 — because the
status the count comes from is published a frame or two later. A guard that reports 0 for every
document refuses exactly the long fixtures it was written to admit, and it would have been very
easy to ship: the check *fired*, and its message was plausible.

Two rules out of it:

- **Wait for the value, and keep "not yet known" as its own outcome.** `0 pages` and `1 page` are
  different facts and neither is `the document did not finish opening`; collapsing them sends a
  reader to swap a fixture when the viewer is what failed.
- **A clamping accessor turns a precondition failure into a wrong answer.** Anywhere a setter
  silently clamps, a caller asserting the value it asked for is really asserting the input was in
  range. The clamp is right — `goToPage(9999)` should not throw — which is why the check has to
  carry the range itself.

### Single-instance turns a stray process into a launch that succeeds and does nothing

Giving Windows the document handover macOS gets from `RunEvent::Opened` means
`tauri-plugin-single-instance`: a second launch forwards its argv to the first process and then
**exits**. That is what a reader wants and it is poison for a harness, because a stray instance
left by an earlier run — a killed check, a timeout, an aborted build — silently absorbs every
later launch. The new process writes nothing and exits at once.

The failure surfaces one phase later and in the wrong shape. `session_check.py`'s
*control: opening without a session* reported `run timed out` / `no summary line, so the run did
not finish`, while `verify` on the same document passed **7/7 in the same run** — which reads as
the app hanging on one particular phase. Four stray processes were on the machine. Cleared table,
same code, and the phase passes; nothing was wrong with the app.

Two things make this worth an entry rather than a note about tidiness:

- **The mechanism inverts the usual reasoning.** A stray process normally causes a *conflict* —
  a port in use, a lock held — which announces itself. Here it causes a **success**: the launch
  returns 0 and the harness waits for output that was already delivered somewhere else.
- **It is invisible on the platform that has no plugin.** macOS gets its handover from Launch
  Services and has no single-instance plugin linked, so a stray instance there is merely untidy.
  The same harness is reliable on one platform and intermittently mysterious on the other.

`scripts/stray.py` clears instances of the binary under test before the first launch, and prints
a `[WARN]` when it found any — a run that needed it is a run whose earlier phases are suspect, and
a helper that tidied up silently would turn that into someone else's mystery. Matched on the
**executable path**, never the process name: a harness that killed every `tpdf` would kill the copy
the person at the keyboard is reading, which is a harness that cannot be run on a working machine.

### One constant standing for two platform distinctions breaks the moment they diverge

`open_check.py` had `HANDS_OVER_TO_RUNNING = sys.platform == "darwin"`, and branched on it for
four things: whether to demand an `.app` bundle, what hint to print when the binary is missing,
whether to run the cold-double-click phase, and whether to run the handover-to-a-running-app
phase. Its own docstring even said it was named once *because* two phases branch on it, citing
the entry here about two copies of a distinction drifting.

That was the opposite mistake, and it was invisible while macOS was the only platform with a
handover: the constant was standing for **"this is macOS"** and **"a second launch reaches the
first process"** at the same time. Giving Windows a handover via
`tauri-plugin-single-instance` and flipping the constant to `True` made the harness demand a
`.app` on a platform that has none — one line into the run, before any phase executed.

The two facts are independent and now have separate names. `USES_LAUNCH_SERVICES` governs *how a
launch is spelled* — an `.app`, `open -a`, an Apple Event. `HANDS_OVER_TO_RUNNING` governs
*whether a second launch reaches the first process*, which Windows does by forwarding argv with no
Launch Services anywhere near it.

The rule that would have caught it: **a boolean named after a capability must not be defined as a
platform test.** `sys.platform == "darwin"` on the right-hand side of something called
`HANDS_OVER_TO_RUNNING` is the tell — it says the author knew only one platform did it, not what
the property was. Where a capability genuinely coincides with a platform today, define it as the
capability (`True`, or a feature probe) and let the platform test live in a separate constant that
is *about* the platform.

### A directory under `src/bin/` becomes a phantom binary in the Windows installer

`npm run tauri build` produces a working `tpdf.exe` and then fails bundling the MSI: WiX
`light.exe` reports `LGHT0091 Duplicate symbol 'Component:backend_probe'`. The generated
`main.wxs` carries 20 file entries where 19 are expected, the extra one being
`backend_probe.exe` with an underscore --- a path that does not exist on disk, alongside the
real `backend-probe.exe`.

**The cause: a *directory* under `src/bin/` becomes a phantom binary.** `tauri build` enumerates
`src/bin/` and registers the first entry that no `[[bin]]` `path =` claims. A `.rs` file there is
always claimed; a **subdirectory** never is. So `src/bin/backend_probe/`, which existed only to
hold `imp.rs`, was registered as a binary named `backend_probe` — pointing at a
`backend_probe.exe` that does not exist, and colliding with the component id WiX derives from the
real `backend-probe.exe` (`-` sanitised to `_`).

The fix is that **`src/bin/` must contain only declared bin sources.** The two `imp.rs` bodies
moved to `src/probes/`, reached by `#[path = "../probes/<name>.rs"]`, which changes nothing about
module parentage so every `super::` in them still resolves. `main.wxs` then carries 19 entries for
19 targets and both an MSI and an NSIS installer build.

Getting there took four wrong theories, and how each died is the transferable part:

- **A stale artifact.** Deleting `target/release/backend_probe.pdb` and rebuilding reproduces it
  — but cargo *relinks* when you delete a build output, so that test is inconclusive rather than
  negative. It dies properly on `print_probe.pdb` and `win_sandbox_probe.pdb` existing without
  duplicates of their own.
- **Cargo.** `cargo metadata` lists exactly 18 bin targets, all hyphenated, no `backend_probe`;
  17 `[[bin]]` blocks with no duplicate names and no name/path-stem mismatches.
- **Configuration.** The only `backend_probe` string outside `src/` is that bin's `path =`, and
  `tauri.conf.json` declares no `externalBin` or resources.
- **The main-binary slot.** The phantom is appended *after* all 17 declared bins, so it looked
  like the app's own entry being misidentified — `tpdf` comes from `src/main.rs` and no `[[bin]]`
  claimed it. Declaring `tpdf` explicitly changed nothing, which is what killed it.

**And one experiment was worthless because its control could not have fired.** A marker directory
`src/bin/ztest_marker/` produced no phantom, which read as "directories are not the cause" and
sent the investigation elsewhere for several rounds. `ztest_marker` sorts **last**; under a
"first unclaimed entry" rule it could never have appeared. Re-running it as `aaa_marker/` — sorting
first — produced `aaa_marker.exe` immediately and `backend_probe.exe` vanished. A control has to be
placed where the mechanism being tested can reach it, and "nothing happened" from a control that
was out of reach is not evidence of anything. That is the same family as the entries here about
checks whose preconditions are already satisfied, arriving through a diagnostic rather than a test.

Two smaller facts worth keeping:

- The phantom breaks the build **either way**, and which error you get is a red herring. When its
  name collides with a real bin you get `LGHT0091 Duplicate symbol`; when it does not, you get
  `LGHT0103 The system cannot find the file`. Chasing the duplicate specifically was time spent on
  the wrong half.
- `light.exe` run by hand needs `-loc`, or it reports five `LGHT0102 unknown localization variable`
  errors that have nothing to do with the problem. Read tauri's own invocation via
  `npm run tauri build -- --verbose` instead of reconstructing it.

### A green gate list can sit beside a distributable that cannot be built

The general point the gate list cannot make on its own still stands. `gates.py` has a `bins` gate
because *nothing else linked a binary*; this was the next ring out — nothing **packaged** one, so a
Windows release would have discovered it at the point where reacting is most expensive. It was
found only because a Windows package was attempted for the first time; `BUILD.md` had mentioned
neither MSI nor WiX.

One thing the fix does **not** address, recorded so it is a decision rather than an oversight: the
installer ships all **17 probe and benchmark executables**, about 35 MB of development spikes
including a sandbox prober and a hostile-document harness. That is a property of declaring them as
`[[bin]]` in the bundled crate, it is identical on macOS, and moving them to a separate workspace
crate or to `[[example]]` targets is the real fix.

### A precondition that names the cause still lets the symptom print

Found 2026-07-30, on the macOS side, one day after the guard it is about was written --- which
is the useful part: the guard was a correct and deliberate fix, and it removed the *diagnosis*
while leaving the *evidence that caused the misdiagnosis* exactly where it was.

`session_check.py` drives four launches of the real app. `TARGET.page` is 7, and
`Viewer.goToPage` clamps to the last page, so on a document shorter than eight pages the
`record` phase drove to page 0 and every phase after it reported `it opens on the remembered
page: page 0, wanted 7`. That is the signature of a broken session restore, it is stable and
reproducible, and it cost a real diagnosis on a restore that was working perfectly. The repair
was a named check --- *"the document is long enough to test page restore"* --- which fails
first, in the record phase, with the fixture and the required page count in its detail column.

It is a good check and it did not fix the problem. `if (!longEnough) return;` returns from the
phase running inside the webview; the Python driver does not know that happened, accumulates
with `ok &= report(...)` and launches the other three phases regardless. So the run still
produced **eleven** failures, of which ten described a restore that was never attempted, and
still ended:

```
[FAIL] it opens on the remembered page     page 0, wanted 7
[FAIL] verify: 5 of 7 checks failed
[FAIL] session restore is not verified
```

The check tells the truth to a reader who starts at the top. These harnesses are run redirected
to a file, where the **tail and the summary line** are what get read --- which is why
`live_output.py` exists at all. A guard whose correction is only visible above the noise it was
meant to correct has not removed the trap, it has added a line to it.

So the shape to watch for: **a precondition that stops one phase of a multi-phase run, where
the later phases are launched by something that cannot see the precondition.** The fix is not in
the check, it is in whatever owns the sequencing --- the driver reads the named check's verdict
out of the transcript, skips the remaining phases *by name*, and ends with a verdict that says
the fixture was the problem rather than the restore. Ten misleading failures to zero, and three
launches not made.

Two details worth copying, both of which this repository has paid for elsewhere:

- **Read the verdict by splitting on the label, never by a fixed column.** `Report` pads names
  to a width nobody remembers, and a pattern encoding that padding stops matching the day a name
  grows past it --- silently, in the direction that reads as good news.
- **A constant duplicated across a language boundary is a coupling, not an assertion**, and the
  distinction matters within one file: `EXPECTED_PAGE` is duplicated *so that* the two sides can
  disagree, while the check's *name* must match or the skip path becomes unreachable and the
  eleven-failure transcript quietly returns. So its absence from the transcript is reported as a
  failure of the driver. Proved by mutation: renaming it turns a green run into
  `[FAIL] this script cannot find a check named ... it has been renamed in sessioncheck.ts`.

### A harness that synthesises input must reset the input's own state machine

Found 2026-07-30, adding double- and triple-click selection. Three functional checks failed
on their first run, including the *control*, and nothing was wrong with the code under test.

Double-click detection counts presses that land close together in time and place. The checks
dispatched a single click, then a double-click, then a triple-click at the same point, back to
back --- which is not three gestures, it is **six consecutive clicks at one point**, and the
counter read them exactly as it should: one run, cycling 1,2,3,1,2,3. Every reading was off by
however many presses the previous check had made. The control, a single click that must select
nothing, reported seven characters selected, because the *drag* in the check before it had
pressed at the same coordinates a few milliseconds earlier and this was that run's second
click.

The general shape, and it will recur for anything driven by synthesised events: **an input
state machine spans the checks, and neither check mentions it.** A key-repeat counter, a
gesture recogniser, a drag threshold, an IME composition, a chord being held --- each is state
that survives a check boundary while looking like nothing at all, because the checks
communicate through the DOM and this does not travel through the DOM.

Reset it explicitly at the start of each gesture. Prefer breaking the run by *distance* over
waiting out its timer: a sleep long enough to be safe is a sleep in every run, and one just
short of the window is a flake that appears only on a loaded machine. Here one press a few
pixels away, before each gesture, is deterministic and costs nothing.

Two further things fell out of the same work, both about the check rather than the code:

- **A fixed drag distance let a mutation survive.** The word-drag check asserted that a drag
  begun with a double-click ends on a word boundary --- and it passed with the granular branch
  mutated to `false`, because 240 px happens to land on a boundary in that fixture, so the
  character drag and the word drag return the identical string. The repair is not a better
  constant: the check now *searches* candidate distances for one whose character-granular end
  falls inside a word, and skips with the reason if none does. A precondition that has to hold
  for a check to discriminate must be established by the check, not inherited from a fixture.
- **The first search found nothing, for a reason unrelated to words.** Every candidate near
  240 px ended on a boundary because from x=300 a drag that long runs off the end of the line,
  so the selection ends at its last character however far past it the pointer travels. A
  search that comes up empty is evidence about the search, not about the property.

### The last page cannot reach the top of the viewport

Found 2026-07-30 checking a typed page jump. The check asserted that after going to page *n*
the viewer's `position.page` --- the page at the **top edge** --- is *n*. True on five corpora
and false on `rotated-90`, which has four landscape pages: jumping to page 4 scrolls as far as
the document goes and leaves page 3 still visible above it, so the viewer honestly reported
page 3 for a jump that was entirely correct.

The general form is not about PDFs. **A "scroll X to position P" operation is bounded by the
content, and the last item can never reach the top of a viewport taller than what follows it.**
Anything that asserts a scroll target by reading back a top-edge position has this, and it
appears only on inputs short enough for the clamp to bite --- which is why five fixtures agreed
and the sixth did not.

Two ways to get it wrong while fixing it, both worse than the bug:

- **Weakening the assertion everywhere.** "The position is *n*, or we are at maximum scroll"
  passes on any corpus for a jump that simply scrolled to the end. The excuse has to be scoped
  to the case that earns it: the target is the *final* page **and** the document is pinned at
  maximum scroll. On every other input the strict equality still holds.
- **Choosing a target that avoids it.** Picking page 2 instead makes the check pass and stops
  it testing a jump anyone would make. The clamp is real behaviour; the check should state it,
  not dodge it.

Worth pairing with the entry about a control contaminated by the phase before it: both are
cases where the *fixture* decides whether an assertion means what it says, and neither is
visible until a corpus with the awkward shape runs.

### A restored file with its original timestamp leaves the build serving the mutation

**This repository had already paid for this once**, and the entry it wrote down named the
wrong thing. *"Restoring a mutated file by moving a backup over it tests the mutated
binary"* blames `mv`, and `mv` was never the mechanism --- the mechanism is that the
restored file carries the backup's **older mtime**, which `cp -p`, `shutil.copy2`, `rsync
-a` and `tar -x` all do just as faithfully. A new harness written months later, by someone
who had read that entry and had written *"copied aside and copied back, never moved"* into
its own comment as the lesson, reproduced the defect exactly.

`scripts/mutate_rust.py` copied each target aside with `shutil.copy2`, mutated it, ran
`cargo test`, and copied the backup back in a `finally`. `copy2` preserves mtime by design,
so the restored file ended up *older* than the artifact cargo had just built from the
mutated one. Cargo compares timestamps, found nothing newer than its output, and rebuilt
nothing --- so every `cargo test` after the run, including the harness's own control on the
next run, executed the **last mutation**. It surfaced as `a_page_with_no_text_reports_it`
claiming a blank page had 7 characters, which is `"catalog".len()`: the query, from a
mutation that had been reverted twenty minutes earlier.

What makes it expensive is that every ordinary check agrees with the source. `git diff` is
clean, the file reads correctly, and reading the function proves nothing. The tell is in
cargo's own output --- `Finished in 0.14s` with no `Compiling` line, for a crate whose
source has supposedly just changed.

- **Restore by writing the bytes, not by copying the file**: `target.write_text(backup.read_text())`.
  A write stamps the current time, which is what every build system is watching for. Keep the
  backup as a real file so a harness that dies mid-run still leaves something to recover from.
- **Within a run it does not bite**, which is why the first run's results were sound and the
  second one's control was not: mutating uses `write_text`, so each mutation *is* newer than
  the artifact. Only the restore is stale. A harness that ran one mutation and stopped would
  look perfect and leave the tree poisoned.
- The general form: **any tool that decides staleness by timestamp can be fooled by a restore
  that preserves one** --- cargo, `make`, `tsc --incremental`, `ninja`. `rsync -a`,
  `cp -p`, `tar -x` and `git checkout` of an unchanged blob all preserve or restore mtimes.
- `mutate_frontend.py` has had the same `copy2` restore since it was written and has never
  misbehaved, because vitest reads sources per run rather than consulting a timestamp. It was
  safe by accident, not by design, and is fixed the same way --- a latent defect that only
  the *build system* decides whether you notice.

The lesson about the lesson, which is the reason this entry is worth its length: **name the
mechanism in the title, not the operation that happened to expose it.** "Restore by move"
reads as a rule about `mv`, so a harness using `copy2` looks compliant, and its author wrote
a comment saying so. The rule that transfers is *"a restore must stamp a new mtime"*.

### A label rendered only from real ids cannot be tested on a combination none of them uses

`keys.ts` renders a shortcut's label from the modifiers its binding declares, which is the
whole point of the module: a label cannot disagree with its handler because it is derived
from it. It took `label(id: BoundCommand)` --- an id that exists --- and the modifier order
inside was therefore only ever exercised by the combinations the binding table happens to
contain.

No binding held Shift *and* Option, so the order between those two was decided by a line of
code no test could reach. It was wrong: the comment above it said "Control, Option, Shift,
Command", which is the platform's order, and the code emitted Shift first. Nothing was red,
nothing could be, and the comment sitting two lines above the contradiction is what makes
this different from an ordinary untested branch --- the intent was written down and the code
disagreed with it in the same commit.

The fix is not a test for a fake command. It is **taking the data instead of the key to the
data**: `render(binding: Binding)` accepts any combination, `label(id)` is one line calling
it, and the ordering can be asserted for a chord no command uses. The mutation that swaps the
two now goes red.

Generalises to anything shaped "look it up, then compute": a formatter keyed by enum, a price
rule keyed by SKU, a permission string keyed by role. **The lookup restricts the test suite to
the inputs that happen to exist today**, and the combination nobody uses is exactly where a
disagreement can sit undisturbed. Split the computation from the lookup and the coverage
follows.

### A leak no behaviour can see needs an accounting observable, not a cleverer assertion

`TextCache` holds two maps: the pages as extracted, and the rotated views derived from them.
Evicting a page has to drop both, or the leak moves rather than closing --- and on a rotated
document the derived map is the larger of the two.

The test written for it asserted the obvious behavioural consequence: after eviction,
`peek(evicted)` is null. It passed. It also passed with the `turned.delete` line removed,
and it could never have done anything else, because `view` consults `pages` first and never
reaches `turned` for a page that has gone. The stale entry sits there for the life of the
document, unreachable by any caller, invisible to every assertion about what the cache
*returns*. That is what a leak is: memory nothing can observe through the front door.

**The tell is that the property is about retention, and every assertion available was about
results.** No amount of rewriting a behavioural check reaches it. What does is exposing the
count --- `retainedViews` --- and asserting it stays equal to the number of pages held. One
getter, and the mutation goes red.

Two things worth taking from it beyond this class:

- **A cache with a derived second tier has two things to evict**, and only the primary one is
  visible in the API. The same shape appears in a memo keyed by the same id, a rendered-string
  cache beside an object cache, a prepared-statement map beside a connection map. Grep for
  every `Map` keyed by the thing being evicted, not just the one being read.
- **Exposing an internal count for a test is not always a leaky seam.** It is when the count
  is an implementation detail standing in for behaviour. It is not when the *claim itself* is
  about resources --- there is no other language for "this does not grow", and refusing the
  getter means shipping the claim untested.

Its sibling in the same review is the other direction: a guard that no mutation could break
because the path reaching it had already excluded its condition. That one is deleted; this
one is kept and made observable. Which of the two applies is decided by asking whether the
code does anything --- not by how untestable it looks.

### A mutation naming a test the harness cannot run reports SURVIVED

`mutate_frontend.py` runs vitest. One of its mutations --- page numbers drawn from zero
instead of one --- named *"a row shows the words around its hit"* as the test that should
notice, and that is a check in `viewer_check.py`, which runs a real webview and is not part
of this harness at all. The mutation was applied, vitest went green, and the run printed
**SURVIVED**.

That is the most misleading verdict a mutation pass can produce, and this file already says
so about a different cause: it reads as *"your tests have a hole"* and sends the next hour
into writing a test that already exists somewhere else. Here the code was fine, the coverage
was fine, and the harness was pointed at the wrong suite.

The fix is a cross-check the harness can make against itself, and it costs four lines: derive
the set of test names from the control run --- vitest's `--reporter=verbose`, libtest's
`--list` --- and refuse to start if any `expect` does not appear in it. Both harnesses now do
this, and both print the number of tests they matched against, so a listing that silently
returned nothing cannot read as "all names valid".

Three things worth carrying:

- **A hand-written name that refers to something in another system is a foreign key with no
  constraint on it.** Test names, fixture ids, feature flags, translation keys, metric names.
  Whatever enumerates them at runtime is the constraint; wire it up rather than being careful.
- **Validate before running, not while running.** The check is in the control phase, so the
  answer arrives in a second rather than after a twelve-minute pass whose result is void.
- **Prove the guard fires.** Pointing one `expect` at a name that does not exist and watching
  the harness refuse takes ten seconds, and this repository has already recorded the cost of
  a safety net whose only evidence was that it kept passing.

### A verification chained after a failed edit reports success for work that is not there

Nine mutations were added to `mutate_frontend.py` by a script whose second assertion tripped
--- an anchor written with twelve spaces of indent for a list that had been moved to module
level and now had four. Python raised, `p.write_text` never ran, and **neither** change was
saved. The command chained on with `;`, so the harness ran anyway and printed
`[OK] all 42 mutations caught by the test named for them`.

Every word of that is true and it is the wrong answer. The nine mutations did not exist, and
the line that says so is a number nobody was comparing against an expectation: 42 where 51
was wanted. It read as a clean pass of work just completed.

- **Chain with `&&`, not `;`, whenever a later step verifies an earlier one.** A failed edit
  must not be followed by a green run of the code as it was.
- **Grep for the new thing, not for the verdict.** `grep -E "recents:|registry:"` over the
  transcript returned nothing, which is what actually exposed it; the summary line was
  perfect. Confirm the work is *present* before believing a report that it passes.
- **Write to a file, then assert, then write once.** A script that mutates a document in
  several steps should do all its `assert`s before its first `write_text`, so a failure
  leaves the file untouched rather than half-edited --- which is what saved this one from
  being worse than confusing.

The same shape as the harness traps above and arriving from outside them: the check was
sound, the instrument was sound, and the thing being measured was not the thing that had been
built. A total is only evidence when something knows what it should be.

### A page fitted to the element's own width is measured under the scrollbar

The check for fit-page was written first as *"the laid-out page box is no larger than
`root.clientWidth` by `root.clientHeight`"*, which is what "the whole page is visible" means
in words. The mutation that deletes the refit on rotation --- the exact defect it was added
to catch --- **passed** it.

The reason is a dozen pixels. The scrollbar sits in a gutter over the right-hand edge, so the
width a page is actually fitted into is `clientWidth - SCROLLBAR_WIDTH`, and on the text
corpus that is 688 against an element of 700. An upright A4 fitted by its *height* is 541 x
700; turned a quarter and left at that zoom it becomes 700 x 541 --- which overflows the
readable width by 12 px and is exactly `clientWidth`. The check was reading a page whose last
column was underneath the scrollbar as one that fitted.

What makes it worth an entry rather than a fix is that the run *did* go red: the existing
rotation check, which derives the expected zoom from the page's aspect ratio, caught the
mutation immediately. So the transcript said `1 failed` and the suite was working --- and the
new check, the one written for this feature and reported as passing beside it, was
decoration. Nothing but running the mutation could have told those two apart.

- **Assert against the bound the code fits into, not the one the eye sees.** The element's
  width, the viewport's width and the width available to content are three numbers, and a
  check that picks the wrong one is loose in the direction that passes.
- **A mutation caught by an older check is not evidence for the new one.** Read *which* names
  went red, not the count --- an aimed-at check that stayed green while its neighbour fired is
  the same result as no check at all.
- The constant is now exported from `viewer.ts` and imported by the check rather than written
  out again, because a second copy of 12 is a number that drifts silently and in this same
  direction.

### A synthetic heading that does not reach the second column tests nothing

The fixture for multi-column reading order has a page with a heading spanning both
columns, which exists to defeat the obvious implementation --- cluster the lines by x
position and read the clusters left to right. A heading belongs to neither cluster.

Written first as a short heading sitting above column one, it left the region between the
columns empty for the *whole* height of the text. So a vertical cut separated the columns
perfectly well, the heading was filed as the top of column one, and the page came out in
the right order by a route that had nothing to do with the case being tested. It was
caught only by dumping the fragment boxes: the heading ran from x=100 to x=170 and column
two started at 400.

Then it happened **again**, in the same session, in the unit test written for the same
case --- `word("HEADING", ...)` is seven ten-point characters, which reaches 170 on a page
whose second column starts at 400. The fixture generator had a comment about exactly this
by then and the unit test still repeated it.

- **A fixture for a spanning element has to be measured, not described.** "The heading
  spans both columns" is a claim about coordinates; assert it, or print the boxes once and
  look.
- **The mutation is what exposed it**, not the passing test: with the heading not
  spanning, a whole branch of the algorithm was never reached, so a mutation to that
  branch survived. A branch that no test reaches and no mutation kills is invisible from
  the transcript, which shows only green.
- Generalises past headings to any fixture built to defeat a heuristic: a table that
  straddles a gutter, a footnote rule, a full-width figure. If it does not actually
  straddle, the heuristic it was built to defeat is never invoked.

### A mutation that survives may be a variant, not a gap --- check before strengthening

Fourteen mutations were written against the reading-order module and three survived. The
instinct, and this repository's own standing rule, is that a surviving mutation means a
test that cannot fail. For one of the three that was exactly right. For the other two it
was wrong, and acting on it would have added tests asserting an implementation detail.

Both survivors were **behaviour-preserving**:

- Banding characters in arrival order rather than sorted by position. `readingLines` merges
  fragments that share a band within a block, so a mis-banded line is put back together
  before anything can observe it.
- Splitting a band at *any* gap rather than at a gutter-sized one. `blocksOf` re-applies
  the threshold when it decides where the columns are, so over-splitting is repaired and
  only under-splitting loses information --- and the under-splitting mutation is caught, by
  six tests.

The design turns out to be self-repairing in one direction, which is worth knowing and is
not what anyone would guess from reading the functions in isolation.

- **Establish what a surviving mutation changed before deciding what it means.** Apply it
  by hand, print the intermediate structure, and look. Ten minutes here; a fabricated test
  pinning an internal ordering would have outlived the code.
- **Record the ones deliberately left out, and why.** They are absent from
  `scripts/mutate_frontend.py` with a note, so the next person to spot the gap learns it
  was measured rather than overlooked.
- The third survivor was the real thing and is the entry above.

### A text-mode restore is not a byte restore, and the locale codec cannot even read the file

The entry *Restoring a mutated file by moving a backup over it tests the mutated binary*
ends by prescribing `target.write_text(backup.read_text())`. Its title says **bytes**; its
code is a text round trip through the **locale** codec. On macOS those are the same thing,
because the locale codec is UTF-8. On Windows they are not, and both mutation harnesses
were wrong in a different way for it.

`scripts/mutate_rust.py` had never run on Windows **at all**. `search.rs` carries `Turkish
dotted I` (U+0130) and the `fi` ligature (U+FB01) for the case-folding tests; their UTF-8
encodings contain the byte `0x81`, and cp1252 leaves `0x81` **undefined**. So `read_text()`
did not mangle the file, it raised `UnicodeDecodeError` on the first mutation and the
harness died before doing any work.

`scripts/mutate_frontend.py` did run, and reported *3 of 75 mutations were not caught as
described* --- "its anchor appears 0 times". Two of those anchors hold the Option sign
(U+2325, one of them the Shift sign beside it) and the third an ellipsis (U+2026), and
cp1252 had mis-decoded the file, so none of the three could be found in it. That verdict is the honest one and is the only reason any of this was visible: a
harness that reports "the mutation is not the one described" rather than SURVIVED is saying
*I could not do the thing*, which is a different claim from *the tests did not notice*.

**And fixing the encoding alone made it worse --- three failures became twelve.** The
discarded `read_text` was also translating CRLF to LF, and every anchor in both harnesses is
written with a bare newline while a Windows checkout is CRLF; eight of the Rust anchors and
several front-end ones span lines. The universal-newline translation had been doing load-bearing
work that nothing named, on the same call that was breaking everything else.

So the shape that is actually right, and it is three separate decisions rather than one:

- **Read bytes and decode UTF-8 explicitly.** The file's encoding is a property of the file,
  never of the machine reading it.
- **Normalise newlines for matching only**, then put the file's own convention back, so the
  mutation is the sole difference on disk.
- **Restore from the backup as bytes** --- which is what the earlier entry meant, and now
  says in code as well as in its title. It must still be a *write*, because that entry's
  whole point is that `copy2` preserves an mtime and cargo then serves the last mutation.

Verified rather than assumed, in both directions: `Path('src-tauri/src/search.rs').read_text()`
raises here, and after the change both harnesses report **22/22** and **75/75** caught, with
every touched file digest-identical to `HEAD`.

The lesson worth carrying past this file: **a harness that has never run on a platform looks
exactly like one that passes there**, because neither produces a failure. Four documented
Windows blockers had already been found wrong by over-reporting; this is the same error with
the sign flipped --- two harnesses nobody had listed as blocked, one of which could not start.

### A mirror of the DOM's focus goes stale, and Enter activates the row nobody is on

`activating a thumbnail goes to its page` failed once in three Windows runs of
`vector-multi`, with `from page 1 to 1, wanted 7`. The check-side half is that the symptom
is ambiguous: the strip activates its **own** idea of the focused row, so a `focus()` that
did not take and an Enter that never reached the handler print character for character
alike. The code-side half is the actual defect, and it was in two classes at once.

Both `thumbnails.ts` and `sidebar.ts` kept a `focused` field --- a **mirror** of the DOM's
focus, maintained by a `focusin` listener --- and activated *that* on Enter rather than the
row the key event reached. A mirror is only as good as its updates, and `focusin` is not
guaranteed: a document without system focus moves `activeElement` without delivering focus
events at all. Whenever the mirror is stale the reader is sent to whatever it still names,
and since it starts at 0 that is **page 1** --- which is exactly what the transcript said.

The fix is to stop consulting the mirror for activation. `event.target` is authoritative,
because it *is* the focused element; the mirror survives only as the fallback for a key that
arrived on the container rather than on a row.

**It was already half-known, which is the part worth stealing.** `sidebar.ts`'s `focusin`
listener carries a comment saying that a roving tabindex which does not follow focus "aims
every key at whichever row happened to be tracked", and it was added to fix exactly that for
the arrow keys. Adding a synchroniser makes a mirror *usually* right, and usually-right is
the version that passes review and then fails once a month. The arrows were fixed and Enter
went on reading the mirror.

(That last sentence was wrong about the arrows, and the paragraph above it says why. A
synchroniser is not a fix, and the arrows kept reading the mirror until 2026-08-08, when they
failed on Windows exactly as this entry predicts --- once in three runs. See the entry
immediately below.)

Reproduced deterministically rather than waited for: dispatching Enter with a target that
differs from the mirror is precisely the state a missed `focusin` leaves behind, and under
the old code it activated page 0 while the key sat on page 3. Each class got that test plus
a control asserting the fallback still works --- without the control, "use the event's row"
is satisfied by a class that activates nothing. Both were shown to go red before the fix and
green after; `sidebar.ts` had no unit tests at all before this.

**What is not established is that this is what happened in that run.** It has not recurred:
five further corpus runs, including a deliberate replay of the back-to-back loop the failure
came from and one under concurrent CPU load, are all green. So the identification rests on a
defect that produces exactly that symptom, not on catching it twice. Contention was the
first guess and was wrong when tested, which is the reason this paragraph exists. The check
now prints `activeElement`, whether the strip followed, and `document.hasFocus()`, so a
recurrence will confirm or refute it instead of restarting the argument.

Fixed in both classes in one change, deliberately. Two copies of one mistake drift, and the
outline tree's version had never failed a check --- which is what a latent defect looks like
right up until it is the one on the screen.

### A synchroniser is not a fix, and the entry above called the arrows fixed anyway

`collapsing a row hides its children` failed one Windows run in three on `outline-simple`,
with `7 rows -> 7 after collapsing "1"`. Same class, same mirror, same mechanism as the entry
above --- and that entry had already written down the reason, one sentence before asserting
the opposite about the very keys that failed. It is worth reading the two claims side by
side, because both were made by someone who understood the defect:

- *"Adding a synchroniser makes a mirror usually right, and usually-right is the version that
  passes review and then fails once a month."* Correct, and it named the interval almost
  exactly: six days.
- *"The arrows were fixed and Enter went on reading the mirror."* The arrows were never
  fixed. What they got was the `focusin` listener --- the synchroniser the previous sentence
  had just finished disqualifying.

`focusin` is not delivered when the document lacks system focus, so `element.focus()` moves
`activeElement` and the mirror keeps naming whatever it named before. `onKeyDown` then looked
the row up by the mirror, and ArrowLeft on a row the mirror did not name fell through to
`toParent` or to `return` --- collapsing nothing, visibly, while every other check passed.
The fix is the one already applied to Enter: resolve the row from `event.target`, which *is*
the element the key landed on, and keep the mirror only as the fallback for a key that
arrived on the tree rather than on a row. Resolved once at the top of the handler now, so
`move`, `toParent` and `activate` cannot disagree about which row that is.

Three things to carry, and the third is the one that cost the six days.

- **A synchroniser converts a wrong answer into a rare wrong answer**, which is strictly
  harder to find. The failure rate is set by how often the event is missed, and nothing about
  the code says what that rate is.
- **The first deterministic test passed against the unfixed code**, and only the mutation
  said so. `setOutline` leaves the mirror on the first row, so a test that dispatches to that
  row has the mirror and the target naming the same thing and cannot tell which one is read
  --- the "property that holds by construction" trap, arriving inside the test written to
  prove a fix. The mirror has to be moved off the target first; here by sending one ArrowDown.
  The fake DOM's `focus()` sets a flag and dispatches no event, so it cannot move it.
- **"Fixed" is a claim about a call site, not about a class.** The previous change fixed
  Enter in two classes and recorded it as fixing the defect. Enumerate the call sites that
  read the shared piece of state --- there were six here, one per key --- rather than the
  classes that contain them.

### An expected error line beside a passing suite makes a green run unreadable

The Windows check `a_worker_whose_child_dies_says_so_rather_than_blocking` spawns a worker
whose child, under `cargo test`, is the libtest harness --- which has no `--render-worker`
dispatch and refuses. That refusal is the check's **control**: a child that dies at once is
what exercises the pipe plumbing hardest, and a real worker answering would test less.

A worker inherits its parent's stderr on purpose, on both platforms, so the refusal printed
`error: Unrecognized option: 'render-worker'` into the gate transcript --- one bare `error:`
line, immediately above the `ok` that followed it, in a run of 205 passing tests. Nothing was
wrong and nothing could be told that from reading the output. A reader who trusts the line
looks for a failure that does not exist; a reader who learns to ignore it has learned to
ignore `error:` in a gate transcript, which is worse.

The fix quiets the **console**, never the child: a `#[cfg(test)]` guard points the process's
own stderr at the null device for the length of the spawn, restoring it on `Drop`. That is
the input the production spawn path already reads --- `Stdio::with_inherited_stderr` on
Windows, `Stdio::inherit()` on macOS --- so no `cfg(test)` branch appears in the code under
test, which is the only reason the check is worth running at all. The child copies the
handle at `CreateProcess`/`exec`, so the window need only cover the spawn; the spawn, the
death and the epitaph are unchanged. Rust's own `eprintln!` and panic messages go through
libtest's per-test capture and never through this handle, so nothing a failure would have
said is lost.

**Mutate a process-wide guard single-threaded, or the verdict is a lie.** Deleting the
`install` call and running the module's nineteen checks in parallel printed nothing at all:
the *other* test's guard was open at the time and quieted this spawn too. That reads exactly
like a guard nothing depends on, and the obvious next move --- delete it as dead --- would
have put the line straight back. Alone, the same deletion restores it immediately. This is
"a control can be contaminated by the phase that ran before it" with the phases running
*beside* each other instead of before, and concurrency makes it intermittent rather than
reproducible.

The POSIX arm was written on Windows and has never been compiled, let alone run. It is
there because macOS prints the same line by the same route, and because a wrong arm fails
loudly on the next macOS run rather than quietly claiming a clean console --- but until
someone runs it there, it is a claim about macOS made from Windows.

### A bundled app that finds its library in the dev tree proves nothing about the bundle

tpdf's `tauri.conf.json` declared no `bundle.resources` until 2026-07-31, so no bundle ever
contained PDFium. `pdfium_library_dir` tries the dev tree first and the resource directory
second; the second branch pointed at a directory nothing created. Every Windows installer
built before that date, and every macOS bundle, shipped an app that opens a window and cannot
parse a document anywhere this repository is not checked out at the same absolute path.

**`viewer_check.py` against the bundle passed the whole time, and could not have failed.** It
runs from the repo, where the first candidate hits, so the bundled branch was never once
exercised. Same family as "a test whose precondition is already satisfied never runs", with
the precondition being an entire directory nobody thought of as an input. The rule that falls
out is worth applying past this case: **a check on a distributable has to run somewhere the
development tree cannot be reached**, or it is a check on the development tree.

Two cheap controls make it honest, and both are one `Move-Item` apart:

- Hide the dev library and re-run. That is the whole test.
- Hide the *bundled* one too, and confirm the run fails. Without it, a pass could still be a
  third path nobody enumerated --- and here that negative control also printed the resolved
  path, which is what identified which candidate was doing the work.

**The bundlers disagree about a resource map's target directory, so one bundled candidate is
not enough.** The map asks for `pdfium/`. Tauri's WiX template ignores it: `msiexec /a` puts
`pdfium.dll` directly beside `tpdf.exe`, and the generated `main.wxs` shows the component under
`INSTALLDIR` with no intermediate `<Directory>`. The lookup therefore tries `resources/pdfium`
then `resources`, and applies this function's own older lesson --- look for the **file**, not the
directory that should contain it --- which is what lets one lookup serve two layouts without
either being asserted.

**"macOS is expected to honour it" stood here until it was checked, on 2026-07-31, and it was
wrong** --- see the next entry. macOS honoured neither candidate: it renamed the dylib to
`Resources/pdfium`. The prediction was reasonable, it was labelled as unverified, and it was
still false, which is the argument for running the control rather than for writing a better
guess.

**The size of an installer is a real signal and is not evidence.** The NSIS setup was 5.59 MB
while `pdfium.dll` alone is 7.21 MB, which settles it in that direction; the reverse does not
follow, and after the fix the MSI grew 13.0 -> 16.7 MB, which is consistent with a compressed
copy and equally consistent with several other things. `msiexec /a` extracts the file list
without installing, and an extractor that did not build the package is the reader whose opinion
counts.

A harness note that cost two runs: the first attempts failed with *could not open
"...text-heavy.pdf"* --- a fixture never generated on this machine, since `testdata/*.pdf` is
gitignored. An absent fixture and a broken bundle produce the same red, and the message names
the file, so read it before concluding anything about the thing under test.

### Moving a binary out of the installer moves it out of the gate that links it

The seventeen spike and benchmark harnesses were `[[bin]]` targets of the crate Tauri bundles,
so the MSI and NSIS installers shipped every one of them --- a sandbox prober and a
hostile-document harness included. They are `[[example]]` targets since 2026-07-31, which the
bundler does not enumerate. The payload went from twenty executables to three (`tpdf.exe`,
`tpdf_lib.dll`, `pdfium.dll`), the MSI 16.7 -> 8.0 MB and the NSIS setup 8.8 -> 5.8 MB.

**The trap is what the move does to `scripts/gates.py`.** Its `bins` gate exists because
nothing else in the list links a binary --- clippy stops at metadata, and `cargo test` links
each target with `main` replaced by the harness's own, so a symbol reachable only from `main`
is dropped as dead code. The file that motivated it was `backend_probe.rs`, and that file is
now an **example**. `cargo build --bins` after the move therefore links exactly one target, the
app, and reports success in under a second --- looking identical to the gate that used to cover
seventeen. The gate is `--bins --examples` now.

Proved rather than assumed, and it is worth doing because the pass is so fast it reads as a
no-op: an undefined `extern "C"` called from one example's `main` turns the gate red with
`LNK2019: unresolved external symbol ... referenced in function ..._12fdpass_probe4main`. That
also settles the speed --- 0.6 s to catch it, so the quick green was incremental caching and not
an empty gate.

**Delete the old executables.** `target/release/backend-probe.exe` and its sixteen siblings
survive the move; nothing rebuilds them, and every path in a document written before the split
still resolves --- to a binary frozen on the day of the change. Fifty-three stale artifacts were
removed here. Same shape as the stale `.pdb` recorded elsewhere in this file, arriving through a
manifest edit rather than a failed build, and the reason `BUILD.md` now says to clear them.

The two `#[path]` includes need re-pointing, and they fail loudly: `src/bin/backend_probe.rs`
reached its body at `../probes/`, which from `examples/` means `src-tauri/probes/` rather than
`src-tauri/src/probes/`. A wrong `#[path]` is a compile error, so this one cannot be shipped by
accident --- unlike everything above it.

### A trailing slash in a Tauri resource map is a rename on macOS, not a directory

`tauri.macos.conf.json` asked for `"../vendor/pdfium/lib/libpdfium.dylib": "pdfium/"`, the same
shape the Windows config uses, and the entry above predicted the macOS bundler would place the
file in `Contents/Resources/pdfium/`. Measured on 2026-07-31, it does not. It reads the value as
the target **path** and writes the dylib as a *file*:

```
Contents/Resources/pdfium   7732336 bytes   Mach-O 64-bit dynamically linked shared library arm64
```

So both bundled candidates missed --- `Resources/pdfium/libpdfium.dylib` and
`Resources/libpdfium.dylib` are each absent --- and the app fell through to the resource-directory
fallback and died with three `could not load Pdfium` lines, `0/1 checks passed`. The fix is to
name the file: `"pdfium/libpdfium.dylib"`. After it, 102/102 with the dev library hidden.

**The reason this is worth an entry rather than a config diff is how thoroughly it looks
correct.** Every cheap observation agrees with the working case:

- `npm run tauri build` exits 0 and reports the bundle.
- `find tpdf.app -name '*pdfium*'` prints a path with `pdfium` in it, which is what someone
  checking "did the library get bundled?" is looking for and what they will accept.
- The bundle is the right size, because the file really is in it.
- `viewer_check.py` passes --- from the repo, where the dev candidate hits first.

What discriminates is `find -type f` versus `-type d`, or `file`, on that one path. A path
existing is not the same fact as a path being the *kind of thing* that was asked for, and the
trailing slash reads as a directory marker to a human and as nothing at all to this bundler.

**The two platforms diverge here, and only one of them can be tested from a given machine.**
Windows survives the same map because WiX ignores the target directory and the resource-root
candidate catches it; macOS does not survive it because it honours the target and the target was
under-specified. `tauri.windows.conf.json` was therefore deliberately left alone --- changing a
config for a platform you cannot re-run is trading a measured pass for a predicted one, which is
the mistake this entry exists to record in the first place.

**It also breaks the next build in an existing tree, and the error names nothing useful.**
`tauri-build` stages resources for ordinary `cargo` builds too, so `target/debug/pdfium`
already existed as a *file* from the old config. The new map wants a directory at that path,
and the build script fails with `File exists (os error 17)` under
`error: failed to run custom build command` --- during **clippy**, which reads as a lint failure
in the gate summary and mentions neither the resource nor the path. `rm target/debug/pdfium`
is the whole fix, and a clean tree never sees it. Worth knowing because the config change and
the failure look unrelated: one is a bundling concern, the other kills a gate that does not
bundle anything.

**One check was already strict enough to catch it, and would have --- at the worst moment.**
`release.yml`'s notarization verification does
`mapfile -t DYLIBS < <(find "$APP" -name 'libpdfium.dylib')` and refuses anything but exactly
one. Against the broken layout that finds **0** and fails with *"expected exactly one
libpdfium.dylib, found 0"*; against the fixed one it finds exactly the one, at
`Contents/Resources/pdfium/libpdfium.dylib`. Both measured on 2026-07-31. So the macOS half of
that workflow, none of which has ever run, contains at least one assertion now known to be
correct and load-bearing --- and the bug would have surfaced on the first tagged release, after
the version bump and the tag, rather than during a build anyone could repeat. That is the
argument for the dev-library-hidden check being a *step* in `BUILD.md` rather than something CI
eventually notices.

### The same trailing slash on the other platform, left there by a prediction that it was survivable

The entry above fixed `tauri.macos.conf.json` and deliberately left
`tauri.windows.conf.json` at `"../vendor/pdfium/bin/pdfium.dll": "pdfium/"`, on the reasoning
that *"Windows survives the same map because WiX ignores the target directory and the
resource-root candidate catches it"*, and that changing a config for a platform you cannot
re-run trades a measured pass for a predicted one. The caution was right. The prediction was
wrong.

Measured on 2026-08-24 against the released 26.8.8, installed from CI:

```
C:\Users\mail\AppData\Local\tpdf\pdfium       7211520 bytes
```

One file, no extension, byte-identical to `vendor/pdfium/bin/pdfium.dll`. Windows honours the
target exactly as macOS does. So both bundled candidates in `pdfium_library_dir` miss ---
`<resources>/pdfium/pdfium.dll` cannot exist, because `pdfium` is a *file*, and
`<resources>/pdfium.dll` is absent --- the worker's `bind` fails, and **the shipped Windows build
could not open a single document**.

**Why no local run ever saw it, and this is the half worth carrying.** The first candidate
`pdfium_library_dir` tries is the dev tree, `CARGO_MANIFEST_DIR/../vendor/pdfium/<subdir>`, baked
in at compile time. An installer built on the development machine therefore finds the
repository's own library and works perfectly --- while the release binary carries the *runner's*
path:

```
D:\a\tpdf\tpdf\src-tauri
```

which exists on no machine that installs it. A locally built install and a CI-built install of
the same commit behave differently, and only the second is what anybody downloads.

**What the reader saw was `worker stopped answering (exited with 1 (0x00000001))`** --- for every
document, by every route. The worker's own message names the missing library, and it goes to
stderr, which a GUI-subsystem process does not have. The parent's open path did not write to the
diagnostics file either, so a session in which nothing could be opened left an empty log:
byte-identical to a session with nothing wrong. Both are fixed --- the worker answers a failed
bind with a sentence a reader can act on instead of exiting 1, and the open path records the
failure with the path it was opening.

Three things to take from it, none of which is "check the config":

- **A prediction about the platform you cannot run is not a reason to leave it alone; it is the
  reason to write the check that runs everywhere.** The test that now pins both maps,
  `the_bundle_puts_pdfium_where_the_app_looks_for_it`, reads both files through `include_str!`
  from whichever host runs, and would have gone red on the Mac the day the macOS half was fixed.
- **A fallback reaching outside the artifact makes the artifact untestable.** The dev-tree
  candidate exists for `cargo run`, and it silently rescued every bundle ever opened on this
  machine. `AGENTS.md` already recorded that a bundled app finding its library in the dev tree
  proves nothing about the bundle --- and then the same tree proved it for three weeks.
- **Fixing one twin is half a fix.** The entry above is a complete, correct, measured account of
  the mechanism, and it sat directly above the config still carrying it.

**The fix leaves a landmine in every build directory that predates it, and it reads as a broken
checkout.** The old map wrote a 7 MB *file* called `pdfium` into `target/<profile>/`; the new one
wants a *directory* there. Cargo does not clean it up, so the next build of that profile dies in
the Tauri build script with

```
Cannot create a file when that file already exists. (os error 183)
```

which names no path and mentions neither PDFium nor the resource map. It hit three profiles on one
machine on 2026-08-24 --- `debug`, `release` and `mutations`, the last of which is the mutation
harness's own target directory, so the harness reported *"the control run produced no summary
line"* rather than a build failure. Remove the stale file (`rm target/*/pdfium`) and the build is
immediately fine. Worth knowing because the reflex on `os error 183` is to suspect the checkout or
a stale lock, and it is neither: it is the previous release's artifact sitting where the current
one needs a directory.

### A silent installer skips the file it cannot write, and exits 0

The sequel to the entry above, and the half it did not predict: the fix for a packaging
mistake does not reach a machine that still has the mistake on it.

26.8.8 wrote the engine to a **file** named `pdfium`. 26.8.9 corrected the map and needs a
**directory** of that name. The generated `installer.nsi` copies resources with

```
CreateDirectory "$INSTDIR\pdfium"
File /a "/oname=pdfium\pdfium.dll" "...\vendor\pdfium\bin\pdfium.dll"
```

`CreateDirectory` against an existing file fails and says nothing --- it sets the error flag,
which nothing reads --- so the `File` that follows reports
`Error opening file for writing: ...\pdfium\pdfium.dll` and offers Abort, Retry, Ignore.

**Retry cannot work, and that is not obvious from the box.** It re-attempts the `File`
instruction, not the `CreateDirectory` that already failed, so the parent directory is still
absent on every press. Deleting the stray file from outside does not help either, for the same
reason: something has to create the directory, and nothing will. On 2026-08-24 the only way
through was to create `$INSTDIR\pdfium` by hand from another process, mid-dialog.

**Ignore is worse than Abort, and the silent install is Ignore.** Measured, A/B, both legs
starting from a byte-identical planted stray:

| leg | `pdfium` before | exit | `pdfium\pdfium.dll` after |
|-----|-----------------|------|---------------------------|
| shipped 26.8.9 setup, `/S` | 7,211,520-byte file | **0** | **absent** |
| 26.8.10 setup with the hook, `/S` | 7,211,520-byte file | 0 | present, digest matches `vendor/` |
| 26.8.10 setup, `/S`, empty directory | --- | 0 | present |
| 26.8.10 setup, `/S`, `pdfium/` already a directory | older copy | 0 | present, replaced |

The control leg wrote `tpdf.exe`, `uninstall.exe` and the notices, registered itself in
`HKCU\...\Uninstall`, created the Start Menu shortcut and the file association, **and returned
0** --- an install that looks complete from every angle a caller can see, with no PDF engine in
it. `pdfium_library_dir` then misses both bundled candidates and the application opens no
document at all, which is exactly the defect 26.8.9 was released to fix.

That matters because of who runs the installer silently: `tauri-plugin-updater` does. A reader
on 26.8.8 who accepted an in-app update got a success and a broken application, with no dialog
anywhere.

**The fix is `NSIS_HOOK_PREINSTALL`** (`src-tauri/installer-hooks.nsh`, wired through
`bundle.windows.nsis.installerHooks`). Tauri inserts it immediately after `SetOutPath $INSTDIR`
and before the resource copies, which is the one place a stray can be removed before
`CreateDirectory` meets it.

Three things worth carrying:

- **Two of the three ways to mis-wire a hook are loud, and the third is not.** This bullet
  first said all three were silent, which was reasoning from `!ifmacrodef` rather than
  measuring; each was then tried. A **mistyped key** is refused by the build script's schema
  (*"unknown field `installerHooksTypo`, expected one of ... `installerHooks`"*), so it never
  reaches a test. A **path naming a file that is not there** is refused by the bundler
  (*"failed to resolve `bundle > windows > nsis > installerHooks`"*) --- but only when a bundle
  is built, which is a CI leg rather than a gate, and note `npm run tauri build | tail` still
  exits 0 there, because a pipeline's status is the last command's. A **file that exists and
  defines nothing**, or defines the macro under another name, is the one nothing catches: the
  `!ifmacrodef` guard skips it, the bundle builds, the installer runs, and the step does not
  happen. `the_windows_installer_clears_the_way_for_the_pdfium_directory` asserts the macro
  and the `Delete` are in the file for exactly that case --- a source-level assertion, which
  is why the A/B above exists as well.
- **The control has to be the artifact that is actually out there.** The failing leg is the
  released `tpdf_26.8.9_x64-setup.exe`, downloaded with `gh release download`, not a rebuild
  with the hook removed. A rebuild would have tested the hook; only the released binary tests
  the upgrade.
- **A test that installs writes registry keys and shortcuts on the machine running it.** Three
  keys here --- `Uninstall\tpdf`, `Software\Timo Stein\tpdf` and `Classes\.pdf`, the last of
  which holds the *backup* of whatever handled PDFs before. Export all three with `reg export`
  first, run the legs into scratch directories with `/S /D=<path>` (last argument, unquoted, no
  spaces), then put the machine back by re-running the **shipped** installer into the real
  location and diffing the exports. Restoring by re-running the new build would leave an
  unreleased version installed.

### An interpolated status label is two columns narrower when it passes

`backend-probe` and `worker-probe` printed their verdicts as `"[{}] {name:56} {}"` with `OK`
or `FAIL` interpolated. `[OK]` is four characters and `[FAIL]`/`[SKIP]` six, so **the rows that
pass start two columns to the left of the rows that do not** --- in the harness's own output, and
in anything reading it.

What that costs is not cosmetic. `BUILD.md`'s documented recipe for extracting a check-name set
is a fixed slice, `cut -c8-47`, correct for `viewer_check.py` where every label is seven wide.
Applied to `backend-probe` it takes the `[OK]` rows two characters short --- `e page asked for
is one a wrong page num` where the `[SKIP]` row gives `the page asked for is one a wrong page
n` --- so the same check appears under two different "names" depending on whether it passed.
Diffing three corpora on 2026-07-31 therefore reported **the name sets diverge**, on three runs
whose name sets were identical. That is the single conclusion this whole arrangement exists to
make trustworthy, and the instrument produced the opposite of it.

Three things worth carrying:

- **The failure is silent and self-consistent.** Every corpus reports the same *count* (42), so
  a check on totals passes; only the set diff moves, and it moves in the direction that looks
  like a regression rather than like a broken parser. A count agreeing while a set disagrees is
  the signature.
- **Uniformity is the fix, not a cleverer parser.** Both now use `{label:7}` with the brackets
  in the literal, matching every other harness here. `fdpass_probe.rs` had independently dodged
  it by padding the *word* (`"OK  "`), which is why the inconsistency survived being read
  several times --- two of four probes were right, in two different ways.
- **Check the widths before reusing a slice recipe.**
  `grep -hoE "^\[[A-Z]+\] *" run.log | awk '{print length($0)}' | sort -u` must print one value.
  It is one line, and it is the difference between a recipe and a recipe that happens to work
  on the harness it was written against.

Same family as the padded-column trap already in this file, and the mirror of it: there the
*name* overran its field, here the *label* underran it. Both end with a fixed offset reading
the wrong bytes and nothing announcing that it did.

### Two counts from two commits are not a platform difference

`backend-probe` reported **42** check names on macOS and **41** on Windows. Both numbers were
honest, both were recorded in the files that are supposed to be believed, and the conclusion
drawn from the pair --- *one check is macOS-only, find out which* --- went out as the headline
item of a handover, together with a plausible candidate: the parent's memory poll, which macOS
uses as a substitute for an rlimit and which `worker-probe` genuinely does report as not
applicable on Windows.

There is no macOS-only check. The 41 was measured at `df1ca61`; `9fb728f`, the next commit to
touch the file, added *"a search option crosses the worker boundary"*. The two counts were taken
one commit apart and compared as though the only variable between them were the operating
system.

What makes this worth an entry rather than an erratum is that **every property that usually
exposes a bad comparison was present and pointed the wrong way.** The two numbers were adjacent,
so the difference looked like exactly one check rather than like drift. A specific,
mechanism-level explanation was available and fit the evidence. And the platform that reported
fewer is the platform that really does lack the thing the explanation named --- so confirming
the hypothesis on `worker-probe` would have *strengthened* a conclusion about a different
harness. A wrong answer that survives its own corroboration is the expensive kind.

Two habits:

- **A count is a measurement of a commit, not of a machine.** Before reading a cross-platform
  difference out of two numbers, check they came from the same tree. Same rule the repository
  already applies to debug-versus-release timings, in the axis nobody labels.
- **Grep for the name, not for a reason.** One command settles it ---
  `git show <commit>:<path> | grep -cF '<check name>'` across the candidate commits --- and it
  is available *before* any theory about which platform lacks what. The theory is the expensive
  route and it is the one that feels like progress.

The near-miss: the proposed repair was to soften `BUILD.md`'s flat *"all 42 names appear"* into
a per-platform statement. That sentence was correct. Weakening a true invariant to accommodate a
mismeasurement is the documentation form of chasing a number back to a documented value, which
this file already records from the code side.

### A reply parsed as the wrong shape reads as absence, and absence is the reassuring branch

`latency-bench` uses an `Outline` round trip as its no-tile control, and the outline walk is
inside that measurement --- so how much work the walk did decides whether the number is a
measurement or a bound. The harness read the entry count out of the reply with
`json.as_array().map(Vec::len).unwrap_or(0)`.

`Outline` is an **object** --- `{items, total, limits, walk_ms}` --- not an array. `as_array()`
returned `None`, `unwrap_or(0)` turned that into zero, and zero means *"the document has no
outline"*, which is the branch that prints `[OK] ... the bound above is tight`. It printed
exactly that for `outline-simple.pdf`: the one fixture in the corpus whose entire reason to
exist is having an ordinary outline.

Nothing failed. There was no error, no missing field diagnostic, no `[WARN]`. A parse that
could not find what it was looking for produced the most reassuring sentence the harness can
emit, on the input designed to make it say the opposite.

**Its own output contradicted it four lines up.** The control read 0.460 ms on that document
against 0.041 ms on one with genuinely no outline --- an order of magnitude, sitting in the
table directly above a claim that no outline work happened. Two derivations of the same fact
were both printed and neither was compared against the other.

Three things, and the second is the general one:

- **Never default a parse whose zero value is the quiet answer.** `unwrap_or(0)` is a
  reasonable habit exactly where zero is unremarkable; here zero *is* the verdict. It refuses
  now, naming the shape it got, because a count this harness cannot read is not a count of
  zero. Same family as the padded-column parser already in this file: the failure is silent
  and lands on the side that looks like good news.
- **When a run derives the same fact two ways, make it compare them.** The check is now a
  match on `(entries > 0, walk_ms > threshold)` with both disagreeing combinations printing
  `[WARN]`, and both were shown to fire under mutation before being trusted. This file already
  says to write that cross-check *before* the first run rather than after the first surprise;
  this is the third time it has been learned by not doing so.
- **The fix was better than the check.** `walk_ms` is in the reply, so the walk is now
  *subtracted* rather than warned about, which turns the round trip from a bound that is tight
  on some fixtures into a measurement on all of them. Evidence it is right rather than merely
  different: across three fixtures with 0, 0 and 10 outline entries it reads 0.039--0.068 ms,
  where the un-subtracted figures spanned an order of magnitude.

### A difference is only a measurement when the operands make it one

Two defects in one benchmark, found by the same run, and they are the same mistake pointing in
opposite directions. Both survived the small fixtures and both were exposed the first time
`latency-bench` was pointed at the A0 sheet.

**Too far apart.** The cost of crossing the worker boundary was `raw end-to-end` minus `inproc
end-to-end`. On a text fixture that is 3.0 ms minus 2.5 ms and looks fine. On a dense vector
page it is 2674.7 ms minus 2940.5 ms, because rendering dominates and rendering *varies* ---
the same tile measured 2669, 3095 and 3050 ms in-process across three rounds. The run reported
the boundary cost as **-265.822 ms**. A negative one, on a run whose transport columns were a
perfectly sensible 0.152 against 0.445 ms. Recovering a 0.3 ms quantity by subtracting two
2.7-second ones asks for four digits of cancellation from a number that is not repeatable to
two. The estimator is now a difference of the two *transport* columns, which are small and
exclude the render.

**Too close together.** The payload-differencing figure was guarded by `raw_bytes >
png_bytes`, which is an ordering test where materiality was wanted. A dense vector page barely
compresses: PNG came back at 4027 KB against raw's 4096, the guard passed on a 68 KB gap out
of 4 MB, and sub-millisecond noise was divided by it to print a **negative cost per 100 KB**.
The condition is now a ratio, and a document that fails it gets a `[SKIP]` naming both sizes
and why.

The evidence the first fix is a fix, and it is the part worth copying: the boundary cost is a
property of the *boundary*, so it should not depend on the document. Before, three fixtures
gave -265.822, 0.357 and 0.242 ms. After, the same three give **0.279, 0.263 and 0.283 ms**.
A quantity that ought to be invariant becoming invariant is a stronger result than any single
number, and it costs nothing but running the harness on a corpus instead of on a favourite.

So, before differencing: **ask what fraction of each operand the answer is.** If it is a
rounding error on either side, the difference is measuring the noise in the larger one; if the
operands are nearly equal, it is measuring the noise in both. Neither condition is visible in
the output --- both produce a plausible small number, and the only reason these two were caught
is that the noise happened to be large enough to push them negative.

### A check on the sign of a noisy quantity fires only when the noise falls one way

`latency-bench` estimated the cost of crossing the worker boundary by subtracting two
end-to-end figures, which on the A0 fixture meant subtracting two ~2.7 s numbers to recover a
~0.3 ms one. It printed **-265.822 ms**. The estimator was replaced, and a check was added to
stop the old one coming back: the boundary must be *positive*, since a process boundary cannot
cost less than nothing.

That check looked airtight. A mutation restoring the wall-based estimator, on the same fixture
that had produced the negative number, **survived**.

Nothing was wrong with the mutation. -265.822 ms was one sample of a quantity whose noise is
hundreds of times its own magnitude, and on the next run the same broken arithmetic landed
positive. The check was not testing the estimator; it was testing which way the noise fell.

**The property that discriminates is reproducibility, not sign.** That is where the two
estimators genuinely differ: a difference of two transport columns lands within 0.004--0.150 ms
of itself across rounds, and a difference of two end-to-end figures swings by 48 ms. Requiring
the figure to repeat catches the broken estimator on every run rather than on the lucky ones.

Worth pausing on how ordinary the mistake looks. "A boundary cannot be free" is a true statement
about the system, it is exactly the kind of invariant this file keeps recommending, and it is
still a bad check --- because the *failure* it is meant to catch does not reliably produce the
symptom it tests for. Before writing a check, ask not only "is this property true?" but **"does
the defect I fear always break it?"**

**And the fix contained a worse bug than the thing it fixed.** The reproducibility check
compared a spread against a headline figure that was computed by a *separate* expression, so a
mutation of the estimator moved the figure while the spread went on being derived the sound way.
The comparison then passed on an estimator broken on purpose --- the check and the thing it
checked had come apart, silently, and the only reason it was noticed is that the mutation was
re-run rather than assumed to work. Both now come from one per-round vector.

So, the general form, which this file has now met three times in one session from three
directions: **two derivations of one quantity have to be tied together, or their agreement
means nothing.** An outline count read one way and a walk time read another. A parsed detail and
a printed total. A figure and its own spread. In each case the agreement was doing no work,
because nothing forced the two to move together.

### A baseline that skips the expensive step leaves its noise in the answer

Found on macOS 2026-07-31, running `latency-bench` for the first time and cross-checking it
against `worker-bench --mode latency` as the handover asked. The two harnesses share no worker
code, which is what makes the comparison worth anything --- and they disagreed by an order of
magnitude on the same nominal quantity, the cost of moving one 4 MB tile out of a worker.

`worker-bench` has four variants: `ping` (a round trip carrying nothing, no render at all),
`inproc` (renders in-process, crosses no boundary), `pipe` and `shm`. Its `transport` column is
a **residual** --- `wall - render - swizzle - fold` --- so it absorbs every microsecond the other
three columns fail to account for. And its derived figures subtract `ping`:

```rust
mean("shm", Row::transport) - mean("ping", |r| r.wall)
```

`ping` never renders, so the render-noise floor that the residual is full of is never
subtracted out. The right baseline is `inproc`, which does everything the measured variant does
**except the one thing being measured**.

The size of the mistake is not academic. On `text-base14` the in-process residual is 0.014 ms
against a reported shared-memory cost of 0.015 ms --- the answer is its own error. On
`vector-heavy`, where the render is ~830 ms and varies by ~12 ms between rounds, the residual is
**46.7 ms** and the reported figure **46.6 ms**: entirely noise, printed to three decimal places.
Baselined on `inproc` instead, that run yields **-0.087 ms**, a boundary costing less than
nothing, which is the tell that was invisible while `ping` was the baseline.

Three things worth taking from it:

- **A baseline is not "the cheapest thing you can measure".** It is the thing that differs from
  the measurement in exactly one respect. `ping` differs in two --- it neither renders nor
  crosses --- so the difference cannot isolate either.
- **A residual column is a debt, not a measurement.** It reports whatever the accounting missed,
  and it is the only column that grows when an unrelated cost gets noisier. Every harness
  printing one owes the reader the in-process value beside it, because that is the floor under
  all the others.
- **The number was already correctly hedged, and the hedge did not travel.** The trap *"A worker
  process is nearly free; the webview boundary is not"* says the shared-memory figure "is
  indistinguishable from the in-process residual" --- which is exactly this, written down on
  2026-07-26 by whoever measured it. But the **0.11 ms** it hedges is quoted flat in
  `docs/PLAN.md` §3 and its Phase 0 verdict table, in `docs/THREAT-MODEL.md`, and in
  `CHANGELOG.md`. A qualification that lives only next to the first statement of a number does
  not survive the number being cited; put it in the harness's own output, where it cannot be
  left behind.

`worker-bench` now prints the in-process residual and the `inproc`-baselined figure beside the
two `ping`-baselined ones, and warns when the error is as large as the answer. It warns on every
fixture measured so far, which is the finding rather than a flaw in the warning --- and it was
proved to be able to stay silent (scale the residual down 100x and it does), because a warning
that cannot not-fire is a constant.

The conclusion drawn from the original number survives all of this and is worth saying plainly:
the boundary is cheap. `latency-bench`, with a control on precisely this and an in-process
residual of 0.001 ms, puts the production worker's per-tile cost at **0.071--0.103 ms** on macOS.
That is ~10x the prototype's, and still ~30x below the 3.0 ms it costs to hand the same tile to
the webview. Nothing architectural moves; only the digits do.

### A control that is easier than the check certifies nothing

Designing the OCR verification gate, 2026-07-31. `docs/PLAN.md` §6 step 4 renders the redacted
regions and OCRs them "confirming no legible text survives", and it is the *only* check that can
say anything about an image carrier --- step 3's byte scan cannot see into a `/DCTDecode` stream,
and refusing every such stream would refuse every scanned page in existence.

So the whole guarantee rests on an empty OCR result. And an empty OCR result is also what a
missing language pack, a crashed engine, a wrong page, a blank region and a downscaled image all
produce. That much is the familiar shape --- `AGENTS.md` already carries *"an empty filter is not
a pass"* and *"a check whose failure mode is a wait cannot fail"*. The fix is equally familiar:
prove the engine can read *something* in this image before believing it read nothing.

The part that is not obvious is that **the control has to be at least as hard as the thing being
checked for**, and it is very easy to build one that is not. Composite a token into the probe
image, get it back, and the check feels earned. But if the token is drawn at 48 pt and the text
that was redacted was a 6 pt footnote, what has been proved is that the engine reads 48 pt. It
says nothing about 6 pt, and the page is then certified with its small print intact.

So `Control::no_easier_than` takes the boxes the redaction covered and sizes the token from the
**smallest** of them, and refuses to build a control at all when no box has a usable height ---
because with nothing to size against there is no honest control, and no control means no
certification. `fold(f32::INFINITY, f32::min)` mutated to `NEG_INFINITY, max` is a one-token
change that turns the gate into decoration, and there is a test that goes red for it.

Three details that each cost something:

- **Put the control in a band appended to the image, not drawn over the region.** Drawn over, it
  can obscure exactly the survivor it was meant to make findable.
- **Match the control by position, not by string.** If the token counts wherever it appears, an
  engine that returns one box for the whole image satisfies its own control --- and so does a
  document that happens to contain the token.
- **Turn language correction off.** A corrector turns marks it cannot read into plausible words,
  which is the wrong bias when the question is whether anything is readable; it will also
  "repair" a high-entropy control token and fail the check for the wrong reason.

Generalises past OCR to any gate whose pass condition is an absence: a virus scan that finds
nothing, a linter with no findings, a diff with no output, a log with no errors. Ask what the
detector would have to see to speak, then make sure it saw something.

### macOS Vision cannot run in the parser worker's sandbox, and it aborts rather than refusing

Measured 2026-07-31 on macOS 26.5.2 with `scripts/vision_sandbox_probe.swift`, which applies a
profile to itself *post-launch* exactly as `worker_child.rs` does. Running it under
`sandbox-exec` instead would apply the profile before `exec` and the process would die in dyld
--- a different failure that reads as "Vision cannot be sandboxed" when it only means the loader
was denied its own reads. Same shape as the Windows restricting-SID rung already in this file.

| profile | result |
|---|---|
| `SANDBOX_PROFILE`, i.e. font directories only | **killed, SIGTRAP** |
| `+ file-read-data` on all of `/System/Library` | ran, then failed with `nilError` |
| `+ file-read` allowed entirely | read the control string back |

Two things follow.

**Vision needs general read authority**, which is the single most valuable thing that profile
withholds: a worker parsing a hostile document must not be able to read the user's files. So OCR
cannot be another `Request` on the parser worker, and the tempting one-line relaxation would
quietly trade away the containment the worker exists for. It does not need to share that
boundary --- an engine consumes a fixed-size RGBA buffer *we* rendered, with no format to parse,
no lengths to trust and no recursion --- so `ocr.rs` defines a separate `OCR_SANDBOX_PROFILE`
keeping the two properties that still apply, no network and no writes.

**The failure mode is a crash, not an error.** That is worth more than the licence question it
settles: a subsystem that aborts its host cannot live in the app process either, whatever its
authority. It is also a warning about how this would have presented if it had been discovered in
production rather than in a probe --- a worker dying at a fixed point, restarted by the pool, dying
again.

And the reason to probe at all rather than reason: the answer sounded obvious in both directions
beforehand. "Vision is a system framework, it will be fine under `allow default`" and "OCR is
just pixels, of course it needs nothing" are both wrong, and a design was about to be written on
whichever one got asserted first.

### An OCR engine's bounding box is a detection, not a measurement

Found 2026-07-31 the first time `ocr-probe` ran the real Vision binding rather than its unit
tests. `Control::contains` was strict containment --- a recognised span counted as the control
only if its whole rectangle lay inside the control band --- which is the obvious reading of
"in the band" and passes every test written against synthetic input.

The probe builds its control band by cropping a strip of the rendered page to one recognised
span's own rectangle and stacking it under a blank strip. Vision then read that text back
perfectly and reported it **1.5 pt above the strip it had been cropped from**. `top >= band_top`
was false, the control counted as a survivor rather than as the control, and the gate returned
`NotVerified` for a redaction that was fine.

The fix is a centre-inside test, which keeps the property that matters --- position decides what
the control is, so a token occurring elsewhere cannot stand in for it --- while tolerating an
engine whose boxes are looser than the pixels it was handed. Every engine's are: they are
detections fitted to what a model thinks it saw, not measurements of ink.

Two more of the same family surfaced in the same hour, and both are about the *harness* rather
than the gate:

- **A strip cropped flush to a span's box clips its ascenders, and the engine misreads its own
  text.** On `outline-simple` a line beginning "Donn" came back from the isolated strip as
  `"L UNVG"`. Padding fixed it --- and a *fixed* pad then pulled a neighbouring line into the
  band on `rotated` and cost that fixture its gate checks entirely, so the padding is now a
  search for the largest pad that still isolates the span. A recogniser needs the whitespace
  around a line nearly as much as the line.
- **Matching a recognised span by substring finds the first occurrence, not the one measured.**
  The ordering check that exists to catch a y-flip "passed" on `columns.pdf` by **1 pt on an
  842 pt page**, because the two words it compared each occur more than once and it had found
  unrelated instances of them. It now requires a word to occur exactly once in the document and
  in exactly one recognised span, and asserts the read gap is at least half the document's ---
  and on that fixture it now correctly reports `[SKIP]`. Passing by 1 pt out of 842 is what a
  broken check looks like when the sign happens to come out right.

The general point, and the reason this is worth an entry rather than a comment: **a unit test
over synthetic geometry cannot tell you what a black box means by the numbers it returns.** The
conversion in `normalised_to_points` had five tests, all green, all correct --- and every one of
them asserted arithmetic against numbers the same file had written. What the engine's
`boundingBox` *means* is only answerable by putting known content at a known place and reading
it back, which is the same lesson `docs/TRAPS.md` already records for the selection code from
the other end.

### A wrapper's own verdicts are on the other stream, in the same shape as a check's

`scripts/mutate_viewer.py`'s first run reported all ten mutations as **broken runs**, each off
by exactly one: "summary says 2 failed, 3 `[FAIL]` lines". Nothing was wrong with the
mutations. `viewer_check.py` prints the webview's results on **stdout** and its own verdict on
the run --- `[FAIL] exit 1`, a timeout, the loaded-module audit --- on **stderr**, in the same
`[FAIL] ` shape. The harness read both streams and counted the wrapper's line as an eleventh
failing check.

Two things worth taking from it, and the second is the reason this is an entry rather than a
one-line fix:

- **Split the streams, do not filter by content.** A pattern excluding "exit 1" would break the
  day the wrapper learns a new message, and it would break silently and in the direction that
  reads as good news.
- **The cross-check is what made this a five-minute problem.** Both counts come from the same
  buffer and answer the same question through different code, and their disagreement was
  reported as *unreadable* rather than resolved in either direction. Without it the run would
  have reported ten mutations caught --- which is what it would also report if the checks were
  perfect, and the two are indistinguishable from the outside. `AGENTS.md` says to write the
  cross-check before the first run rather than after the first surprise; this is what that buys.

Related, and found the same day in an ad-hoc analysis script rather than in a committed one:
**every outcome label is exactly six characters** --- `[OK]  `, `[FAIL]`, `[SKIP]` --- and the
name starts at column 7 whatever the outcome. Consuming the label with a regex `\s` eats one
space for `[OK]` and none for `[SKIP]`, so the *same* check reads as two different names
depending on whether it ran or skipped --- and a comparison of check names across documents
then reports differences that are entirely the parser's. Slice by column; do not tokenise.

### The page break is whitespace, and concatenating two pages loses it

Cross-page search joins the tail of one page to the head of the next and matches over the
join. The obvious implementation concatenates the two pages' characters, and it finds nothing:
a page's extracted text does not end with whitespace, so "raster" at the foot of one page and
"appearance" at the head of the next read as `rasterappearance`, and the phrase a reader typed
matches nothing at all.

The break **is** whitespace --- it is a line break with a sheet of paper in it --- so a
separator has to be inserted before folding, and then the existing fold collapses it against
any whitespace either side exactly as it does a line break inside a page. Two tests were
written before the code and both went red on the first run, which is the only reason this took
minutes.

The separator needs a source index that belongs to no page (`u32::MAX` here), and the reason is
not tidiness: a hit that *starts* or *ends* on the separator lies wholly inside one page, so it
is that page's own reply to report, and emitting it from the join as well would double every
count in a document that repeats a phrase across a break.

One consequence follows and is worth stating rather than discovering: **a word the break splits
is not rejoined.** `appear` / `ance` across a break is two words, because the break is
whitespace. That is the same answer the module already gives for a word a *line* break splits,
and the alternative would manufacture a hit out of two unrelated words whenever a page happens
to end mid-syllable.

### A pattern over folded text has no lines, so `^` means the page

Regular-expression search matches against the same **folded** sequence a literal query gets ---
runs of whitespace already collapsed to one space, soft hyphens already gone, case already
decided by the match-case option rather than by an inline `i` flag. Keeping one haystack is
what makes a pattern and a literal mean the same thing by the same switches, and what keeps a
hit expressible in the character indices the selection and the highlight already use.

The cost is two things a reader would assume and which are false:

- **`\n` never occurs**, so a pattern written with one matches nothing.
- **`^` and `$` anchor to the page**, not to a printed line. There are no lines left by the
  time the pattern runs.

Both are the same bargain that makes a phrase match across a line break at all, so neither is
fixable without giving that up. They are pinned by tests and stated in the module docs; the
trap is assuming the pattern sees what the page looks like.

Two smaller ones from the same work. **A zero-length match is not a match** --- `a*` matches the
empty string at every position, which would report a hit per character, each highlighting
nothing. And **a pattern that does not compile must be reported, not answered**: a reader typing
one *expects* to get it wrong, and "no matches" for `foo(` is a statement about the document
rather than about the query. That is why `PageMatches` carries a `problem` and the find bar
shows it in place of the counter.

### A closure and a direct read of the same variable disagreed, and it is unexplained

Recorded because it cost an hour and because the next person to write this check will reach for
the same shape. It is **not** a resolved trap; what follows is what was ruled out.

The palette's "only Open document is offered with no document" check originally held the viewer
in a `let` that the actions object closed over, set it to `null` for the duration, and put it
back. On `vector-heavy` it failed, and the diagnostic read:

    direct=null through=[object Object]

--- inside one template literal, in one synchronous expression: `attached === null` was true
while `actions.viewer() === null` was false, where `actions.viewer` is `() => attached` and
prints as exactly that. Which way round the two disagreed **changed between runs of the same
binary**, and `outline-simple` was correct throughout.

Ruled out, each by reading rather than by assumption: one declaration of the variable, one
closure over it, one call site of the enclosing function, no reassignment of the actions object,
and the *compiled* bundle reading correctly (`let ... r=e ...; const a={viewer:()=>r,...}` and
later `r=null;const Fn=\`direct=${r===null} through=${a.viewer()===null}\``). Nothing static
accounts for it. What remains is a JavaScriptCore artifact in a very large async function with
many closures and awaits --- run-to-run variation with the same binary is the shape of a
tier-dependent miscompile --- and that is a suspicion, not a finding.

The response was not to explain it but to remove the need for it: the check now builds a
**second registry** whose viewer is null *by construction*, so there is no shared mutable
binding for anything to observe at the wrong moment. Both readings have been stable since.

**The rule to carry forward is the one that applies whether or not the cause is what it looks
like: do not ship a check whose own mechanism you cannot account for.** A check that fails for
a reason nobody understands is worth less than no check, because the next failure will be read
as the same mystery.

### A guard that also guarantees termination fails as a hang, not as a red test

`find_in` refused a query of only whitespace, and that guard quietly did a second job: `all`
is true of an empty sequence, so an empty query returned early too --- which mattered, because
the literal walk advances by the needle's length and an empty needle advances it by **zero**.

Deleting the guard as a mutation therefore did not produce a failing test. It produced a run
with no result at all: the harness reported *"no summary line --- the run did not finish"*,
which is its verdict for a crash, a timeout and a build error alike. The mutation was
unreadable rather than caught or survived, and an unreadable mutation says nothing about the
tests either way.

Two things follow:

- **Do not let a termination argument lean on another guard's implementation.** The empty
  needle is now refused first and on its own, with the reason next to it. The whitespace guard
  still refuses whitespace, and deleting *it* now turns the test red the way a mutation should.
- **A mutation that hangs is a finding about the code, not noise from the harness.** It located
  a real latent hazard --- one edit to an unrelated guard away from an infinite loop in the
  search path --- that no test could have found, because no test can reach it while the guard
  stands. The harness's insistence on positive evidence that a run happened is what surfaced
  it; a harness that read "no failures" as "caught" would have reported a clean pass.

### A paragraph is one mark and several text objects, and the gap between them belongs to neither

`structure.rs` maps a tagged element's marked-content ids to character ranges, and the mapping
route is worth knowing before reaching for the obvious one: `FPDFText_GetTextObject` gives the
page object a character was drawn by and `FPDFPageObj_GetMarkedContentID` gives that object's
mark, so a character index resolves to an MCID **directly**. Parsing the content stream and
correlating it with the extractor's output --- the obvious route --- would have been the third
independent extraction in this codebase, which is the failure `text.rs` opens by warning about.

The trap is what the literal mapping then reports. A paragraph is *one* marked-content id and,
in the content stream, *one text object per line*. PDFium inserts a **generated** character
between two text objects so the extracted text reads as prose, and that character belongs to no
page object, so it carries no mark at all. Taken at face value a four-line paragraph is
therefore four runs with three unclaimed characters between them: the first run of
`structure_probe` reported **ten runs for four blocks**, and every run's text was a single line.

Bridging the gaps is right, and both halves of the condition are load-bearing:

- **Unmarked**, so a run cannot absorb another element's characters.
- **Whitespace**, so a run cannot absorb *visible* text the producer failed to tag. That is the
  one thing a reading order must never do quietly --- the word would appear in the wrong place
  and nothing would say so. Bridging on "unmarked" alone looks equivalent and is not.

Two consequences for anything built on this. **"Every character is claimed" is not the
invariant** and asserting it fails on a correct implementation: a separator falling *between*
two elements belongs to neither, and the honest assertion is that nothing **visible** is
unclaimed. And a page with no structure tree must report **no runs at all** rather than an order
it inferred, because that emptiness is how a caller tells "fall back to geometry" from "the
document says its reading order is this".

### A tolerated gap in the input becomes a hole in the output

The entry above ends on the right rule --- an unclaimed *whitespace* character is not a hole in
a tagged reading order, so a page carrying some must still be usable. That is a statement about
the **decision** to trust the tags. It says nothing about what to *emit*, and the obvious
reading of it is wrong in a way that no test written for the decision can see.

The first consumer built the page from the runs: for each run, the characters in its range, in
the order the tags give. Every one of those characters is on the page and in the right place.
What is not there is the six characters no run claimed --- one `\r\n` per paragraph boundary ---
so the page came back **six characters shorter than the page**, and both halves of the check
that noticed were about counting rather than about order: select-all reported 272 code points
against an extraction of 278, and the accessibility text compared as a multiset and was short by
five after whitespace folding.

The rule that fixes it is the one `fragmentsOf` already used for a character PDFium placed
nowhere: **every character gets an owner** --- its own run where it has one, the run of the
nearest character before it otherwise, and the first run that follows for anything before the
first claimed character. The output is then a permutation of every index, exactly as the
geometric order is.

Worth stating as an invariant rather than a fix, because it is the thing to assert: a reading
order is a **permutation of the page**, and one that quietly holds less than the page is worse
than one that holds all of it in a poorer order. `readingOrder(text).length === codes.length` is
the whole test and it is one line.

The generalisation past tags: wherever a validation step *tolerates* something the emitter does
not *handle*, the tolerance is a silent deletion. Two different questions --- "may I use this
input?" and "what do I do with every part of it?" --- and answering the first does not answer
the second.

### A comma opens a line of its own, and every space on the line joins it

Found by the tagged fixture, present in the geometric path since long before it, and it survived
because every other generated corpus is built from words with no punctuation in them.

Lines are recovered by banding character boxes: two boxes share a line when they overlap by more
than half of the shorter one's extent. That is right for two boxes of comparable height and
wrong for a **comma**, which PDFium reports as a box starting inside the line and dropping below
the baseline --- about a third of the line's height, overlapping it by 46% of *itself*. Measured
on `testdata/tagged.pdf`: letters banded at 227.41--236.13, the comma at 234.80--237.69.

Below the threshold, so the comma opened a band of its own. The characters are scanned in order
of their top edge, so what came next were the **spaces**, which PDFium reports 0.01 pt tall
sitting on the baseline --- and a space overlaps anything it touches by 100% of itself, so every
one of them matched the comma's new band rather than the letters'. One line of text came back as
two:

```
in the main column, and closes the section.
->  "inthemaincolumnandclosesthesection"  and  ", .      "
```

Read aloud, copied, and searched exactly like that.

The fix is a statement about type rather than a tuned constant: **a box too short to be a line of
text joins the line it touches.** A mark a third the height of the letters beside it is a mark on
their line, and nothing set in the same type is half the height of the line above it.

Two things worth carrying. The failure is invisible in every aggregate --- the characters are all
present, in one order or another, so a multiset comparison passes and only an assertion about
*lines* can see it. And the control for the new rule has to use lines that **overlap**: written
first with 12 pt boxes 16 pt apart, it could not fail, because boxes that do not touch are
refused by the guard above the rule whatever the rule says. Real text lines overlap by their
ascenders and descenders, which is the case that had to be held.

### A font can float a space's box clear of its own line, and overlap banding drops it

The entry above ends with a rule that reads as complete --- *a box too short to be a line of
text joins the line it touches* --- and the load-bearing word turns out to be **touches**.
`sameBand` requires overlap before it will consider anything, and it is right to: the
short-mark clause exists for a comma that dips *into* the line, and loosening it to bridge a
gap would start joining a mark to the line above it in tightly leaded text.

So a space that touches nothing is banded with nothing. Measured through `FPDFText_GetCharBox`
on `multilingual.pdf` laid out in `msgothic.ttc`, the space at index 4 of `café latte` comes
back **placed**, with a real box 0.02 pt tall at y 752.00--752.02, while every letter on the
line sits at 752.14--766.08. The two bands miss each other by 0.12 pt. The space matched no
band, became a fragment of its own, and fell out of the line's ranges entirely:

```
café latte  ->  "cafélatte"
```

Three things make this worth an entry of its own rather than a sentence on the comma's.

**It is a platform difference that is not a code difference.** The generator picks a font per
page from what the machine has, so the same fixture is `msgothic.ttc` on Windows and Arial
Unicode on macOS --- and Arial Unicode puts its space *inside* the letters' band, where the
comma rule handles it. The macOS corpus was green throughout and could not have found this.
It cannot fix it either: a corpus whose document differs per machine is not a control for a
rule about geometry, so what discriminates is a unit test carrying the measured numbers,
which fails on any platform.

**"Placed" is the wrong question.** The existing route for a character that reads on a line
but is not on it --- PDFium's synthesised separators --- keys on a box of four zeroes, and
this box is not that. What it has in common with them is not the absence of a box but the
absence of *information*: 0.02 pt is the font's bookkeeping about where it parked a space,
not a claim about the page. The rule is therefore absolute rather than relative --- nothing a
reader can see is a tenth of a point tall at any legible size, so `SLIVER_PT` is a statement
about type in the same way `SHORT_MARK` is.

**A sliver must not be absorbed the way a mark is.** Both re-attach by preceding index, and
there the resemblance stops: a combining mark is drawn *over* its base and belongs in the
line's box, while a sliver floating 0.12 pt below the line would drag the line's box down to
meet it. The control that holds this is the comma --- 2.89 pt, twenty-nine times the
threshold --- and it has to be asserted **on the box**, because a character routed by index
keeps its place in the ranges and reads identically either way.

### An absolute epsilon refuses a page whose every glyph is that thin

The entry above is correct and was not sufficient, and the gap cost a day. `SLIVER_PT` refuses
a box under a tenth of a point across the line, on the reasoning that nothing a reader can see
is that short at any legible size --- which is a claim about *glyphs* and turns out to be a
claim about *metrics*. A page set in a predefined CMap with no embedded font gives PDFium no
glyph metrics at all, and it reports every character on the page **0.018 pt tall**. Measured on
`encodings.pdf` page 2 through `FPDFText_GetCharBox`:

```
idx  code       left      top    right   bottom   height
  0  日 U+65E5  60.000   89.982  78.000   90.000  0.0180
  8  日 U+65E5  60.000  721.982  78.000  722.000  0.0180
```

Two lines, 632 pt apart, and every character of both refused. `fragmentsOf` then has nothing
placed, takes its `items.length === 0` branch, and returns the page as **one** fragment
covering every index --- so the page read, aloud and on the clipboard, as
`日本語の符号\r\n日本語の符号`. One line.

**The two failures are exactly complementary, which is what makes this worth an entry.** The
A/B, one file reverted and rebuilt:

| | `multilingual` | `encodings` |
|---|---|---|
| absolute rule absent | **129/130** `cafélatte` | 130/130 |
| absolute rule present | 130/130 `café latte` | **129/130** `日本語の符号\r\n日本語の符号` |

A fix that *moves* a failure passes every check aimed at it. The corpus it was written for goes
green, its unit tests go green, and the run that would contradict it is the one nobody re-runs
because nothing changed there. Only the standing rule --- run the whole corpus and diff the name
sets --- reaches it, and the handover that asked for exactly that is why this was caught on the
same day rather than shipped.

**The question is not "is this box thin" but "is it thin compared with what it sits among".**
The two measured samples are three orders of magnitude apart on that quantity and adjacent on
the absolute one: the floated space is 0.02 pt against letters of 13.94 pt (0.0014 of them),
while the predefined page is 0.018 pt against a page median of 0.018 (1.0 of it). So the rule
is a conjunction --- `height < SLIVER_PT && height < SLIVER_OF_LINE * typical` --- and both
halves are load-bearing, each proved by a mutation that turns exactly one test red. Absolute
alone refuses the degenerate page; relative alone would refuse ordinary 5 pt footnote type on a
page set in 200 pt display type, and append it to the line before.

**The reference is a median, and the maximum passed everything until a test was written for
it.** A maximum needs only one substituted glyph on an otherwise metric-less page --- and the
`broken-map` page of the same fixture shows that a document of this kind contains exactly that
--- to read 13 pt as typical, call every real character a twentieth of it, and collapse the page
again. The mutation survived the whole suite first time; the control for it is a page of
degenerate boxes with one tall glyph among them.

**And the platform asymmetry is the same one, a second time.** macOS was green on all eleven
corpora after the absolute rule landed, because its PDFium substitutes a font with real metrics
where Windows has none --- so the machine that could see the defect was again the only one with
the document that has it. Two rules now carry measured Windows geometry into unit tests for
this reason. When a geometric threshold is chosen against one sample, the second sample is not
a confirmation to look for later; it is the thing that decides the *shape* of the rule.

### A test cannot see the direction of an attachment it puts in index order

The check for "an unclaimed character stays with the text it follows" was written over two
tagged blocks in index order, with the separator between them. Attaching it to the block before
gives `body\n` + `note`; attaching it to the block after gives `body` + `\nnote`. The same
string. The test passed, and it passed identically with the rule reversed.

Tagging the **second** block first is what makes the placement observable: `note` + `body\n`
against `\nnote` + `body`. One character moved across a boundary is invisible whenever the two
sides are adjacent in the output, so a check on where something is attached needs the sides
*separated* --- which on this subject is the same fixture property that makes the tags worth
reading at all.

### A guard for "more than one page" is not a guard for "a page that can be reached"

`nav.goToPage` already carried the right guard --- `page_count > 2`, because the last page cannot
reach the top of the viewport --- and `nav.nextPage` and `nav.previousPage` beside it carried
`page_count > 1`. Both had been green on every corpus for a week, because the smallest
multi-page fixture in the corpus had three pages.

A two-page fixture arrived and both failed, `0 -> 0`, which reads as a broken command. Nothing
was broken: stepping forward from page 1 targets page 2, page 2 is the last page, and on a
window taller than one page it can never become the page being read.

The lesson is not about page counts. **A guard is a claim about what the fixture can exercise,
and it goes stale silently when the corpus grows in the direction it did not anticipate** --- a
guard that is too weak produces a red check that looks like a defect in the subject. When one
check's guard is strengthened, look for its siblings: these three were written together, one was
fixed once, and the fix was not carried across.

### A wrap is correct when there is nothing ahead, so the check cannot fire

"The scan starts at the page being read, not at page 1" is asserted by pressing End, searching,
and requiring the first hit to be at or after the page reached. On a document where the needle
appears only *before* that page, wrapping to the start is the correct behaviour and the check
reports it as `the scan restarted at the beginning`.

The needle is picked from page 1 and no corpus repeated it on a later page until a two-page
fixture arrived. So the check could not distinguish its **subject** --- where the scan starts ---
from its **precondition** --- that there is anything ahead to find.

The fix is to establish the precondition from the result and skip with it printed: *"no match at
or after page 2, so wrapping to the start is correct"*. Its sibling in the same file had the
same shape from the other side: a wait for "matches on two pages" that spends its whole bound
and then fails on a document whose needle is on one page, when what it has established is that
this fixture cannot exercise the check below it. Wait for the *search* to finish, then decide
between a check and a skip. Both are the same mistake --- **a precondition written as an
assertion** --- and both only appear when a corpus arrives that cannot satisfy it.

### A mutation aimed at a check that skips reports SURVIVED

`mutate_viewer.py` refuses to start on an expectation matching zero or two check names, which
catches a renamed check and an ambiguous prefix. It did not catch the third case: a name that is
present in the baseline and is `[SKIP]`.

A skipped check is in the name set --- deliberately, since a name that vanishes is the bug that
arrangement exists to catch --- so the validation passed, the mutation ran, nothing went red, and
the harness printed **SURVIVED**. That is the most misleading verdict it can produce: it reads
as a gap in the checks rather than as a fixture that does not exercise them.

Two lines fix it, and the reason to write them rather than remember is that the two new checks
skip on six of the seven corpora, so the natural fixture is the one that cannot judge them.
`mutate_viewer.py` grew a per-mutation runner for this: same harness, different corpus, and the
baseline validation now refuses a mutation whose expected check skipped.

### A leaner data structure turned a wrong edit into a no-op

The tagged reading order gives every character an owner, and there are two shapes for that: a
list of indices per run, or one owner per character with the run named by its **position** in the
run list. The second is leaner --- an `Int32Array` and an array read instead of a `Set` per run
--- so it was written that way second, as a simplification.

It was reverted by a mutation. "Order the tagged blocks geometrically after all" is one of the
mutations the suite exists to catch, and against the leaner shape it read:

```ts
[...tagged].sort((a, b) => a.start - b.start).map((_, at) => within(text, fragments, owner, at))
```

which changes **nothing**. The callback uses the positional index, never the run, so sorting the
list it maps over cannot affect the result --- and the owner array was built from the unsorted
list anyway. The harness reported SURVIVED, which reads as a missing test.

Two things worth carrying, and the second is the general one:

- **A shape that couples two values by index has an invariant nothing enforces.** Here it was
  "the owner array's numbers are positions in *this* run list", and the natural wrong edit
  violates it silently rather than failing. The list-per-run shape carries the coupling in the
  value: there is no order to keep in step, so the edit is not expressible.
- This repository already records the better version of the same lesson from the other end ---
  *"an unreachable guard is worth keeping if the type can carry it instead"*. **Move the
  impossibility into the value or the type**, and pay a `Set` per run for it. A page has a few
  thousand characters; the allocation is not the thing that matters.

And the reason it was caught at all is that the mutation had to be **re-anchored** after the
refactor. A mutation harness re-run after a shape change is not bookkeeping: it is the only
thing that reads the new shape adversarially.

### A selector naming one element stops reading the page when the layer gains another

`spokenText` in `viewercheck.ts` gathered a page's text with
`article.querySelectorAll("p")`, which was exactly right for a layer that emitted one `<p>` per
line, and became wrong the moment a heading became an `<h1>`.

What makes it worth an entry is **where the failure would have surfaced**. The check that uses
it is *"the text read out is the page's own text"*, compared against an independent extraction.
A page whose heading was no longer selected reads as the accessibility layer having **lost the
heading** --- a defect in extraction, or in the tree, or in the reading order. Nothing points at
the selector, which is in a helper thirty lines away and has been right for a week.

Two habits, and the second generalises past selectors:

- **Gather by structure, not by tag name.** `[...article.children]` says "every block this layer
  emitted" and stays true whatever the layer emits. A tag list is a copy of the layer's
  vocabulary that has to be updated in step with it, and nothing enforces the step.
- **When a layer's output vocabulary grows, grep for everything that names the old one.** The
  change that adds `<h1>` is the change that has to widen the selector; a later session sees
  only a check failing about text.

### A `null` that means "inferred" is not a `null` that means "unknown"

`ReadingBlock.tag` is the element type a block came from, and it is `null` when the block came
out of the geometry rather than out of the document's tags. The temptation --- and the first
draft --- is to fill that in with `"P"`, since an inferred block is, after all, a paragraph.

It is not the same claim. A tagged boundary is the producer's **statement**; an inferred one is
this file's **guess** from where the whitespace fell. `a11y.ts` uses exactly that difference:
a tagged block is handed to a screen reader as one element, because its boundaries are real, and
an inferred one is handed over line by line, because merging on a wrong guess joins two columns
into a paragraph while an over-eager split costs a reader nothing.

Write `"P"` there and the distinction is gone, silently, and every consumer downstream is now
asserting something nobody claimed. The mutation that does it is one word and it is caught by one
test, which is the cheap version of this entry.

The general shape: **an optional field's absent case often carries information, and filling it
in with a plausible default destroys the information rather than the absence.** Ask what the
absence *means* before defaulting it --- "not stated" and "stated to be the ordinary thing" are
different, and only one of them can be acted on.

### Whatever a fixture is meant to discriminate, it needs two of

The tagged fixture was built to discriminate a reading order --- a margin note that geometry
reads third and the tags read last --- and it does that well. It carried **one** heading, an
`/H1`, and the check written against it says *"a heading is announced as a heading, at the
document's own level"*.

That check passed. It could not fail: the mutation that maps every heading level to `h1`
produces `h1` for the only heading on the page, so the tree and the tags agree and the run is
green. The claim in the name --- *at the document's own level* --- was untested, on a fixture
built specifically for testing claims.

The general rule, and it applies to a fixture rather than to a test: **a property with N values
needs at least two of them present, and one value is the same as none.** The list is longer than
it looks once stated that way --- one heading level, one page, one rotation, one font, one
language, one column, one revision. This repository already has entries for the two-page case
(three checks whose guards had never met one) and for the rotation case (a dense page of uniform
lines cannot detect a y-flip); this is the same failure arriving through an element type.

The fix was one line of fixture --- an `/H2` subheading --- and it turned the mutation red
immediately. Cheaper than the check it made honest, which is the usual ratio here.

Corollary worth keeping: the *unit* test caught this mutation the whole time, because a mapping
table can be enumerated in a test where a fixture has to contain each case physically. When a
viewer check and a unit test cover the same rule, the unit test is the one that can be
exhaustive and the viewer check is the one that proves the rule is wired --- so a viewer check
surviving a mutation its unit test catches means the *fixture* is thin, not the suite.

### `FPDFText_GetUnicode` is a UTF-16 API, so an astral character is two characters

`text.rs` opens by explaining why `PageText` carries codes rather than a string: three
features reading three different extractions would disagree in ways no test catches. Its own
docs go further and name the documents where a re-encoding goes wrong --- *"CJK, symbol fonts,
anything astral"*. And then the code did exactly that, through a different call.

`FPDFText_GetUnicode` returns a UTF-16 **code unit**. A code point above the BMP therefore
arrives as two characters: measured on `testdata/multilingual.pdf`, U+20000 came back as
U+D840 and U+DC00, each with the same box. So `codes` was documented as "one Unicode scalar per
character index" and was not, and every consumer that turns a code into a `char` gets `None`
for both halves --- `char::from_u32` refuses a surrogate. The fold in `search.rs` dropped them,
which means a **CJK Extension B ideograph was unfindable while being plainly visible on the
page**, and `PageMatches::chars` reported 30 for a page with 27 characters.

The part that let it live is the direction the two consumers fail in. Rust drops the halves;
**JavaScript reassembles them by accident**, because `String.fromCodePoint(0xD840)` is a legal
lone surrogate and concatenating two of them produces the right character. So the frontend read
the broken array correctly and the backend did not, and no check written on either side alone
could see it. The one that found it compares a *hit's own text* against the code points its
indices address --- which is a Rust-side assertion by necessity, and is the reason
`examples/search_probe.rs` exists rather than a frontend check.

The fix is one place: `extract` joins the pair into one entry and unions the boxes, so the field
means what it says. An **unpaired** surrogate --- a broken `/ToUnicode` CMap rather than an astral
character --- becomes U+FFFD, because dropping it would silently shorten the page and shift every
box after it, and keeping it raw leaves a number no consumer can decode.

The general shape: **an FFI call's element type is part of its contract, and a name like
`GetUnicode` does not tell you which encoding.** Ask what one call returns for one character
outside the BMP before assuming the array is scalars.

### A content stream has no bidi, so logical order draws right-to-left text backwards

Writing the Arabic page of `multilingual.pdf`, the obvious thing is to put the logical string
into a `Tj`. `Tj` advances left to right in the order the glyphs are given, so the first
character read lands leftmost and the line is drawn reversed. A real producer emits **visual**
order --- for one unbroken run, the logical order reversed --- and shapes at layout time.

What makes this worth an entry is not the mistake, it is how it presents. PDFium recognises a
right-to-left run and reverses it to recover logical order, so the page laid out logically
extracted *exactly reversed*, and the fixture's own expectations then looked like an extraction
defect in tpdf. Two hours were one wrong turn away: the first reading was "our extraction
reverses Arabic", which is a plausible and serious-sounding claim about the code. The check that
settled it was one line --- comparing the extracted characters against `reversed(written)` ---
and it should be the first thing tried whenever an ordering comes back inverted, because a
*systematic* reversal is nearly always a convention mismatch rather than a bug.

A second trap sits inside the first. Reversing the **whole line** is wrong once it contains a
Latin word: `PDF` becomes `FDP`. Direction is per-run, and a neutral (a space) between two runs
of *different* direction has to be resolved to the paragraph direction, not attached to whichever
run precedes it. Getting that wrong put two spaces in one place and none in the other, and what
surfaced was a space **missing** from the extracted text --- which reads as a extraction defect
too.

Worth knowing for the viewer side: reading order survives this. A fragment's ranges follow index
order rather than x order, so a right-to-left line comes back logical even though the code sorts
band members left to right. That is luck rather than design, and it is now measured.

### PDFium maps Arabic presentation forms to base letters, which was assumed to be false

A producer that shapes at layout time writes Arabic in the presentation-forms blocks (U+FB50 to
U+FEFF): U+FEDF is a lam, and it is not U+0644. The obvious consequence is that a reader typing
base letters cannot find shaped text, and `multilingual.pdf` was built with the two spellings on
two lines to demonstrate it. The manifest said so, in a field explaining why the count was 1.

It is 2. **PDFium normalises presentation forms to their base letters when it extracts**, so both
lines come back character for character identical and a base-letter query finds both. The
corollary is the one that would have been guessed wrong in the other direction too: a query
written *in* presentation forms finds **nothing**, including the line that was written in them.

Two things follow, and the second is the reusable one.

The narrow one: do not add compatibility folding to `search.rs` for Arabic. The layer below
already did it, and folding again would only widen what a highlight covers.

The general one: **a fixture's expectations come in three kinds and conflating them is how a
measurement comes to look like a specification.** `multilingual-manifest.json` now marks each
query `why` (stated from what the generator wrote --- it inserted the substring, so nothing about
the code can change the count), `measured` (a property of PDFium this corpus established), or
`decided` (a product decision, so changing it has to be argued for). The count that was wrong
here was written as a `why` --- as a fact about the file --- when it was a claim about a
dependency nobody had asked.

### `ß` does not lowercase to `ss`, and the doc comment saying so stood for days

`search.rs` documented its fold with an example: *"Because folding can change a character's
length --- `ß` lowercases to `ss` ---"*. It does not. `ß` **uppercases** to `SS` and lowercases to
itself, because it is already lowercase. So `strasse` finds `STRASSE` and not `Straße`, which is
a gap a German reader meets on their first search.

Nothing was wrong with the code; the example was. It survived because both halves of the
sentence around it are true --- the fold *can* change a character's length, and it *does* carry a
source index per folded character for that reason --- so the paragraph reads as coherent and only
the instance is false. The mechanism was real and had a different cause: `İ` (U+0130) lowercases
to `i` followed by U+0307, which is the length change that actually happens here.

Three consequences, all one cause, and all now stated as `decided` counts in
`multilingual-manifest.json` so that changing any of them is a decision:

- `strasse` does not find `Straße`.
- `istanbul` does not find `İstanbul` --- the fold leaves the combining dot between the `i` and
  the `s`.
- Greek `οδος` (final sigma, which is what a reader types) finds neither `ΟΔΟΣ` nor `οδός`,
  because `Σ` lowercases to `σ` and never to `ς`.

**Lowercasing is not case folding.** Case folding fixes all three in one move and Rust's standard
library does not offer it, so it means a dependency --- and it also folds `ﬁ` to `fi`, which the
same module says outright it does not do. That makes it a decision about what a highlight may
cover, not a bug fix, which is why the limitation is recorded rather than quietly patched.

The habit: **a factual claim inside an otherwise-correct paragraph is the hardest kind to
notice.** The thing that caught it was a fixture line asserting a count, not a reader.

### A combining mark does not touch its own line, and a word with an ascender hides it

An acute accent sits above the x-height. Measured on `multilingual.pdf`: U+0301 at 718.64--721.30
against an `e` at 707.80--717.68 --- a 0.96 pt gap, and **no overlap at all**. `sameBand` requires
overlap before it considers anything, and is right to: its short-mark clause exists for a comma
that *dips into* the line, and loosening it to bridge a gap would start joining a mark to the line
above in tightly leaded text.

So `resumé` written decomposed came back as three lines --- `resume`, the accent alone, then the
rest --- and the accessibility tree announced them that way.

`café` does **not** show it. The `f` reaches to 721.30 and drags the accumulated band up into
contact with the accent, so a word with an ascender passes. That is the discriminating property,
and it took a second fixture line to find: the first decomposed line in the corpus had an `f` in
it and was green.

The fix is not geometric. **Unicode already answers the question the geometry cannot, and it
answers it about the character rather than about where the producer drew it**: `\p{Mn}` and
`\p{Me}` have no advance width and belong to the grapheme before them, so a mark is attached to
the preceding character the same way a box-less character already was, and its box is folded into
its base's.

Two things the fix needed that the first attempt did not have. A mark with **nothing before it**
has no base, and keeps its own band --- inventing an attachment to the character *after* it would
be wrong in the one direction that reorders text. And the control against over-reach cannot be an
*order* assertion: within one fragment the order is index order, so a raised digit beside its
neighbours reads identically whether it is treated as a character or as a mark, and two
arrangements were tried before that was clear. What discriminates is the **line count** --- a digit
widened into the mark class swallows the line below it.

### A harness that cannot read a script skips, and blames the fixture

`viewer_check.py` picks a word out of page 1 and searches for it. The picker was
`/[A-Za-z]{5,}/`, which finds nothing on a page of Japanese --- so it returned null and
**seventeen** search checks skipped, every one of them printing *"page 1 has no extractable
text"* about a page with forty-nine characters on it.

Two failures, and the second is worse than the first. The checks did not run, which a count of
`[OK]` lines would show. But the **reason printed was false**, and a skip is read as *"this
fixture cannot exercise this"* rather than *"this harness cannot read this input"* --- the first
is a property of the corpus and needs no action, the second is a hole in the harness. Nine of
those skip sites shared one hardcoded string, so the lie was a copy-paste rather than a
misjudgement.

Both halves are worth fixing separately: the picker now falls back to `[\p{L}\p{N}]{2,}` and
takes a slice from the middle of the longest run --- two characters is a word in Japanese, and a
needle that *is* the whole run makes the whole-word check vacuous --- and the reason now
distinguishes "the page has no text" from "no word could be read out of the page's N characters".
Twelve of the seventeen then ran, on the same binary, with no other change.

The general rule: **a skip's reason is an assertion and can be wrong.** It is not commentary. Any
branch that produces a skip has to name the condition it actually tested, and a reason shared by
several sites is where that stops being true first.

### A check with no precondition reports a sparse fixture as a defect

`multilingual.pdf`'s pages carry three to six lines spread down an A4 sheet. The drag check drags
horizontally at `y=140` and again at `y=620` and asserts the higher one comes from earlier in the
text --- and `y=620` fell in a gap between two lines, so it selected nothing and the check
reported *"selected 20 and 0 characters"* on a viewer that was working perfectly.

The check already had two carefully written preconditions, for a rotated page and for a
side-by-side layout, both with the reason printed. It had none for the simplest thing: **that
there is text where it dragged.** With nothing selected at one of the two heights there is no
ordering to compare and any verdict is invented, so the answer is a precondition rather than a
widened assertion --- the assertion is the valuable part.

Two habits from it. A fixture whose pages are **mostly blank** silently narrows any check that
reads a position, which is why the corpus spreads its lines down the page rather than stacking
them at a fixed leading in the top fifth; that alone moved the failure but did not remove it,
because no even spread of three lines can guarantee one at 68% of the page. And a check that
reads a hardcoded coordinate should say so in its skip, because the next fixture will have its
text somewhere else again.

### A check name that is a prefix of another cannot be aimed at

`scripts/mutate_viewer.py` matches a mutation's expectation as a **substring** of the check names
and refuses an expectation that names more than one --- a good rule, and the entry
*"a mutation naming a test the harness cannot run reports SURVIVED"* is why it exists.

`search_probe.rs` named its three checks per query `query astral-alone`, `query astral-alone:
indices address the hit` and `query astral-alone: hit is paintable`. The first is a prefix of the
other two, so no mutation could be aimed at it: the harness correctly refused, saying the
expectation matched three checks.

The fix is one word --- the count check is `query astral-alone: hit count` --- and the rule is
worth stating because it constrains naming rather than code: **in any family of check names
matched by substring, no name may be a prefix of another.** Adding a suffix to the general one is
the cheap fix; renaming the specific ones is not, because their names are the useful part.

Related to the padded-column entry, and the same family: a parser that works on the names you
have now, and stops working when one grows.

### A mutation aimed at code no fixture reaches survives, and the fix is not a new corpus

`extract` translates the tagged runs from PDFium's character indices into ours, which only
matters on a page that is **both** tagged and carrying a character above the BMP. `tagged.pdf`
has no astral character and `multilingual.pdf` has no tags, so a mutation that switched the whole
translation off passed `search-probe` and `structure-probe` alike. It is not defence against
something impossible --- a tagged Japanese document with an Extension B ideograph in a name is
ordinary --- so the surviving mutation was a real gap.

Three options, and the ranking is the point. A **new corpus** that is tagged and astral is the
thorough answer and the expensive one: a structure tree in a second generator, expectations for
it, and a ninth fixture whose purpose overlaps two existing ones. **Deleting the translation** is
what the "unreachable guard" entry would suggest and is wrong here, because the code is reachable
by input rather than unreachable by construction. What was done instead: the arithmetic was split
into a function of its own and given **five unit tests**, one per case including a run that ends
between the halves of a pair, and the mutation was moved to the Rust harness where a unit test
can judge it.

The general rule: **when no fixture reaches a branch, ask whether the branch is arithmetic before
building a fixture.** Arithmetic can be tested directly and exhaustively; only behaviour needs a
document. Two of the five cases here --- an end index inside a pair, an index past the end --- are
ones no realistic fixture would have contained anyway.

### A stand-in glyph with a degenerate box measures the wrong rule

The astral page of `multilingual.pdf` draws a stand-in glyph, because no font on either platform
has a CJK Extension B ideograph, and re-labels its CID in the `/ToUnicode` CMap. The first choice
was U+2F00 KANGXI RADICAL ONE, on the reasoning that a rare character cannot disturb another
line.

U+2F00 is a single horizontal stroke. PDFium reported it **1.6 pt tall inside an 18 pt line**, so
it tripped the short-mark clause in the line grouper and the page split into three lines --- which
is a finding about the *banding* rule, on a page built to test *surrogate pairs*. The check that
failed named neither.

Swapping it for U+3007 IDEOGRAPHIC NUMBER ZERO, which has a box of ordinary proportions, made the
page measure what it is for.

The habit: **a stand-in has to be typical in every dimension the harness reads, not only in the
one it was chosen for.** It was chosen for being unused and it was also, invisibly, chosen for
being the thinnest glyph in the font. Same family as the fixture entries above --- a property with
one value present cannot be distinguished --- arriving through a substitution rather than through
a missing case.

### With no `/ToUnicode`, PDFium returns plausible garbage rather than nothing

A CID font with no `/ToUnicode` CMap is ordinary in the wild --- some LaTeX setups and some
scanner output emit it --- and the reasonable expectation is that its text is unextractable.
It is not. PDFium falls back to reading the glyph ids as character codes, so
`testdata/encodings.pdf` page 0 draws `Encoding probe ABC` and extracts
`(QFRGLQJ\x03SUREH\x03$%&`: **eighteen characters for eighteen drawn**, in the right shape,
with the right word lengths and the right spacing. This subset's glyph ids happen to sit
0x1D below ASCII, which is why it looks like text rather than like noise.

Everything downstream then behaves impeccably and is wrong. The page is **not textless**, so
`PageMatches::textless` is false and the find bar's one honest signal --- *"this document has
no extractable text"*, which exists precisely so that "no matches" is never a lie of omission
--- does not fire. A reader searching for a word they can see gets *no matches*. Copy yields
nonsense. The accessibility tree reads the nonsense out.

So there is a **third state** between "text" and "no text", and nothing in tpdf currently
represents it: text that is present, positioned correctly, and means nothing. Worth stating
what a detector would look like, because it does not need a heuristic on the characters: the
font dictionary either declares a `/ToUnicode` or it does not, and a `/CIDSystemInfo` ordering
of `Identity` with no CMap is PDFium guessing by construction. That is a `lopdf` question with
a yes-or-no answer, not a guess about whether text looks like language.

Not built here. Surfacing it is a product decision --- what a reader is told, and where --- and
the corpus exists so that the decision can be made against a measurement instead of a
suspicion. `docs/PLAN.md` Phase 1 records it.

### A pattern was compiled case-sensitively against a haystack the fold had lowercased

`search.rs` folds the page --- lowercasing it when `match_case` is off --- and matches against
the folded sequence. A **literal** query is folded the same way by `Folded::of_query`. A
**pattern** is not: a regex source is not text and cannot be lowercased safely, since `\S`
would become `\s`, `\D` become `\d`, `\B` become `\b` and `[A-Z]` become `[a-z]`, each
silently meaning the opposite of what was typed. So it was handed to the engine raw.

The consequence is total rather than partial: with the option off, **any uppercase letter in
a pattern matched nothing at all.** A reader with regex on and match-case off, typing
`Encoding`, got no results on a page that plainly contains it.

Two things kept it alive for as long as the feature has existed, and both are worth copying
down.

The first is that `compile`'s doc comment asserted the invariant it was breaking: *"Case is
handled by the fold rather than by the `i` flag ... with the fold already lowercasing both
sides."* Both sides is exactly what it does not do. **A comment that states an invariant is a
claim, and it is the one place nobody re-derives** --- the next reader takes it as given, which
is what it is for.

The second is that the harness could not produce an uppercase pattern. `viewer_check.py`
builds its pattern out of a word taken from the page under test, so on every corpus with
ordinary prose the pattern was lowercase and the two sides agreed *by accident*. The corpus
that found it did so because its text is garbage that happens to be uppercase --- which is
luck, and the reason to keep the fixture rather than to be pleased with the process.

The unit test beside it could not fail either: it used a lowercase pattern against mixed-case
text, which agrees whether or not the pattern is case-insensitive. **Both directions of a
switch need a test, and for case that means an uppercase query as well as a lowercase one.**

The fix is the `i` flag, not folding the pattern: it composes with a haystack that is already
lowercase, leaves every class and escape alone, and is what a reader means by "ignore case" on
a pattern.

### A harness sliced a code-point index with `String.prototype.slice`

The cross-page phrase check resolves each half of a hit against a fresh extraction of the page
it claims to be on, and the comment above it says so: *"Re-extracted, so the two index spaces
are checked against the pages rather than against the reply that reported them."*

It did it by building a JavaScript string from the page's codes and slicing it with the match's
`start` and `end` --- which are **code point** indices, while `slice` counts **UTF-16 code
units**. On a page holding a character above the BMP the two differ by one per such character,
so the right-hand half came back one letter short: `Encoding�probe𠀀�` where
`...�C` was wanted.

The comment is the whole entry. It names the exact hazard, in a check written to guard against
it, in the two lines that fall to it. Nothing about the code was careless; the mistake is that
`String.fromCodePoint(...codes)` produces a string whose indices are no longer the indices you
started with, and that conversion reads as lossless because the *characters* are all there.

**Slice the codes, not the string.** A helper that takes two code point indices and returns
`String.fromCodePoint(...codes.slice(from, to))` cannot be got wrong, and it is the same shape
as the fix in `text.rs` for the same underlying fact.

### A fixture's self-check forbade its own finding

`make_encodings_pdf.py` asserts its own properties before writing, which is the discipline this
repository applies to every generator: a fixture that has stopped discriminating should fail
loudly rather than pass quietly.

The assertion was *"every page must extract as something other than what it was written as"*,
which is true of the two pages with absent and broken character maps and is the entire subject
of the corpus. It is exactly backwards for the third page, where a **predefined CMap over a
non-embedded font extracting correctly is the finding** --- the fact being established is that
PDFium's bundled Adobe-Japan1 tables are in the vendored build.

So the check refused to write the fixture on the strength of its own result, and the failure
line read like a defect in the generator.

The rule: **a blanket invariant over a set of deliberately different cases is usually wrong for
one of them**, and the one it is wrong for is the control. Assert per page, by name. It costs
three lines and it makes the exception visible in the source rather than discovered when the
check fires.

### A measured string transcribed off a terminal loses what the terminal does not draw

The expected extraction for the unmapped page was written by reading `text-probe --mode order`
output and copying the string: `(QFRGLQJSUREH$%&`. Sixteen characters. The real answer is
eighteen --- the two spaces map to glyph id 3, so the fallback yields **U+0003**, which a
terminal prints as nothing at all.

The probe said so. Its own first line reads `18 characters`, two lines above the string that
was copied, and the count was read past on the way to the interesting part.

Two habits, and the second is the cheap one:

- **Never transcribe a measured value from rendered output.** Print it as escapes, or read it
  from a file, or write the expectation as a program that computes it. Control characters,
  zero-width joiners, bidi marks and combining marks are all invisible or misleading in a
  terminal, and every one of them is exactly the sort of thing an encoding fixture contains.
- **When a harness prints a count beside a value, compare them.** The disagreement was
  available at zero cost and in the same buffer --- the same shape as the padded-column entry,
  and the same fix: make the check do the arithmetic rather than the reader.

### A mutation aimed at one branch when the fixture only reaches the other

`scalar_of` replaces a lone surrogate with U+FFFD along two paths: a **high** surrogate whose
follower is not a low one, and a **low** surrogate reached with nothing in front of it. The
mutation written to prove the replacement was covered end to end changed the second, and
SURVIVED.

Correctly. The fixture mapped a space to a high surrogate and `A` to a low one, and in
`Encoding probe ABC` every `A` is preceded by a space --- so **every low surrogate in the corpus
paired**, and the branch the mutation broke was never taken. The check that was supposed to
notice was passing for a reason unrelated to the mutation.

Two readings of a survivor, and telling them apart is the work: *the test is weak* or *the
input never reaches the code*. Here it was the second, and the fix was one more entry in the
CMap --- mapping `B` to a low surrogate as well, so that by the time it is reached the pair
before it has been consumed and it is genuinely alone.

Generalises past surrogates: **a function with two paths to the same result needs an input for
each**, and a fixture built to exercise "the replacement path" naturally produces only whichever
one the author had in mind. Enumerate the `return`s, not the outcomes.

### Two broken `/ToUnicode` entries can decode to one valid astral character

The broken-map page maps a space to a lone high surrogate and `A` to a lone low one. In
`Encoding probe ABC` the first space is followed by `p` and becomes U+FFFD, and the second is
followed by the `A` --- so **two unrelated characters, at different positions on the page,
decode to one astral character** with a box spanning both, and the page comes back seventeen
characters long for eighteen drawn.

Nothing is wrong. That is what decoding a UTF-16 stream means, the document is broken, and no
interpretation of it is more correct than another. It is written down because the *length* is
the sort of number that looks like an off-by-one later, and because the pairing rule joining
two characters the producer never meant to join reads as over-reach until the alternative is
stated: refusing to pair across a "suspicious" boundary would need a rule about which
boundaries are suspicious, and there is none.

Worth having as a fixture for a second reason. It is the only input in the corpus that reaches
the pairing code from *broken* data rather than from a correct CMap --- same function, two
provenances --- and a mutation aimed at each catches the same defect from both sides.

### A change predicted to fix three things fixed two, and the third was never the same problem

Lowercasing was replaced with Unicode case folding on 2026-08-01, on the strength of a claim
made in three documents and a doc comment: that it fixed all three of the reader-visible
consequences the multilingual corpus had measured. It fixed two.

| | before | after |
|---|---|---|
| `strasse` finding `Straße` | missed | **found** --- `ß` folds to `ss` |
| `οδος` finding `ΟΔΟΣ` | missed | **found** --- `Σ` and `ς` both fold to `σ` |
| `istanbul` finding `İstanbul` | missed | **still missed** |

The third was never a case problem. `İ` folds to `i` followed by U+0307 --- *exactly* what
lowercasing gave --- because the difference between it and a plain `i` is a **combining mark**,
and no case operation removes a mark. Reaching it needs the dot stripped, which is accent
stripping: a different decision, and one the module refuses for a stated reason.

The three were grouped because they shared a *symptom* --- "a reader types the obvious spelling
and finds nothing" --- and the grouping was then read as sharing a *cause*. It is an easy slip
to make in a list, and the list was written here, in `search.rs`, in `docs/PLAN.md` and in
`BUILD.md` before anything was measured.

What caught it was checking before writing code: one throwaway test printing
`default_case_fold` and `to_lowercase` side by side for six characters, which took a minute and
showed the two agreeing exactly on U+0130. Had it been written after the change instead, the
fixture would have been adjusted to match the new behaviour and the wrong claim would have
survived as documentation.

**A remedy that addresses N symptoms is a claim about N causes.** Before committing to one, put
the inputs through it and print the outputs --- not the outcomes you expect, the outputs. Where a
prediction turns out wrong, say which one and why it was a different kind of problem, because
"fixed two of three" with no explanation reads as an incomplete job rather than as a
misclassification.

Unicode has a Turkic mapping (`T` in `CaseFolding.txt`) that *does* fold `İ` to a bare `i`, and
it is deliberately not used: it also folds `I` to `ı`, which is right for Turkish and wrong for
every other language, and nothing here knows a document's language.

### PDFium normalises ligatures too, so the cost of case folding was smaller than stated

Case folding was taken as a trade: it fixes `ß` and Greek sigma, and it also folds `ﬁ` to `fi`,
which `search.rs` had explicitly refused to do because the resulting highlight covers one code
point for a two-letter query. That refusal was the reason the change needed a decision rather
than being applied as a bug fix.

Measured after the change: **a ligature never arrives as one.** U+FB01 sits in the Alphabetic
Presentation Forms block, the same range as the Arabic forms this corpus had already shown
PDFium normalising --- and PDFium normalises the whole range on extraction. A page typeset
`ﬁnal` comes back as `f`, `i`, `n`, `a`, `l`, and the fold's ligature rule is never reached from
the page side at all.

So the cost is near-theoretical for page text and real for exactly one case: a reader who
**pastes** a ligature into the find bar. Before the change that reader got nothing; now they get
the word. The trade turned out to be one-sided.

Two things worth carrying:

- **The existing measurement generalised further than the entry that recorded it.** That
  presentation forms come back as base letters was written up as an Arabic fact; it is a fact
  about a *range*, and ligatures, Roman numerals and the width-variant forms are all in
  neighbouring blocks. When a normalisation is discovered, ask what else it covers before
  reasoning about anything in the same neighbourhood.
- **A fixture line for the ligature was still worth adding**, and its expectation had to be
  relabelled from `decided` to `measured` once this was known: it passes because PDFium
  normalised the page, not because of anything in `search.rs`. A check that passes for a
  different reason than its name gives is the failure mode the three-way labelling exists to
  prevent, and it caught itself here.

### The gates had never run on the platform where they fail

`examples/ocr_probe.rs` did `use tpdf_lib::ocr_vision::Vision;` at the top level, and
`ocr_vision` is `#[cfg(target_os = "macos")]` in `lib.rs`. On Windows that is an
unresolved import, so `clippy --all-targets`, `cargo test` and `cargo build --examples`
all failed --- three of the nine gates, red for **two days**, on a commit whose author had
run the full sweep and seen 9/9.

Both facts are true at once because gates run where you are standing. The OCR interfaces
landed 2026-07-31 on a Mac; the next Windows work was on a different day and a different
branch of the checklist. Nothing in the repository knew the difference, and nothing could:
a green sweep is a statement about one machine, and it reads exactly like a statement about
the product.

The repository's **first ever CI run** found it in six minutes. That is the entry: not the
`cfg` mistake, which is ordinary, but that a two-platform project verified by one machine
has an entire platform's worth of compile errors it cannot see, and the size of that blind
spot is unknowable from inside it. It was three gates. It could have been thirty.

Two things follow.

- **Fix it with a module gate, never a crate-root `#![cfg]`.** That is a separate trap with
  its own entry: `#![cfg(...)]` at the top of a target removes every item including `main`,
  and cargo then reports "`main` function not found", which reads like a missing entry point
  rather than a deliberately empty target. The shape that works is a refusing `main` under
  `#[cfg(not(...))]`, a dispatching `main` under `#[cfg(...)]`, and the body in
  `src/probes/` reached by `#[path]` --- and the body's `main` must be `pub`, or the
  dispatcher gets `E0603: function main is private`, which is how this fix failed its first
  compile.
- **A cross-compile check is available and is not free.** `cargo check --target
  x86_64-pc-windows-msvc --all-targets` catches exactly this class locally, but it needs the
  *other* platform's PDFium staged in `vendor/` (the Tauri build script validates that the
  resource exists) and then still stops at `tauri-winres`, which wants `llvm-rc`. Both were
  attempted here; the second was not worth a 1.5 GB LLVM install when CI answers the same
  question for nothing.

### A test that refuses an empty fixture set is what makes CI's absence visible

The same first CI run failed on macOS too, and for the opposite reason: nothing was wrong.

`print.rs`'s third-parser check iterates six fixtures, `[SKIP]`s each absent one, and then
asserts `examined > 0`. `testdata/*.pdf` is gitignored and generated, and the workflow did
not generate any --- so the check printed six SKIP lines and refused, exactly as designed.
The assertion was added because *"a run where every fixture was absent prints six SKIP lines
and otherwise looks exactly like a run where every one passed"*, and this is that sentence
collecting.

Worth recording for the shape rather than the fix. A guard written against a hypothetical
--- nobody had a fixture-less environment when it was written, because everyone's checkout
had run the generators --- sat inert for weeks and then fired the first time a genuinely new
environment appeared. **The environments a check will meet are not enumerable in advance,
which is the argument for asserting the precondition rather than trusting it.**

The fix is a fixture step in `ci.yml` generating the two **dependency-free** ones, and the
part that needs saying out loud is what it does *not* generate: anything from
`make_text_pdf.py` needs fonttools and embeds a *system* font that differs per runner, and
`make_incremental_pdf.py` writes ~550 MB on purpose. Those tests still skip in CI. A
workflow that silently covers two thirds of a fixture set is the same failure one level up,
so the omission is written in the workflow beside the step.

### A document meant to cover both platforms was generated from platform-specific inputs

`THIRD-PARTY-NOTICES.md` is one document describing what both installers contain. It is
generated from `vendor/pdfium/licenses/`, and **`vendor/pdfium` is whichever platform's
archive this machine installed.** So the "platform-independent" document was a function of
the platform, and the `notices` gate was green on macOS and red on Windows with nothing
wrong on either.

`bblanchon/pdfium-binaries` ships the same fifteen licence files in both archives. Nine
differ. Eight of those are CRLF, which `read_text` already normalised, so they were
invisible --- and that is the interesting half: **the failure that survives is the one whose
sibling you already handle by habit.** Normalising line endings is reflex; it made eight of
the nine differences vanish and left one, which then looked like a mystery rather than an
instance of a pattern.

The ninth is `licenses/pdfium.txt`, shipped with every line prefixed `// ` on macOS and with
none on Windows. Same licence, different packaging.

Three things worth carrying.

- **The threshold I first wrote was a guess dressed as a safeguard.** `normalise_licence_text`
  originally stripped the prefix only when 80% of a file's lines carried it, reasoning that a
  whole-file comment prefix is safe to remove and a stray `//` is not. But `pdfium.txt` is
  PDFium's own licence followed by a dozen other projects' --- only the first block is
  prefixed, 27 lines of 196 --- so the guard declined, the output was unchanged, and the
  verification still failed. The number was chosen from an imagined input and was off by a
  factor of five. Per line and unconditional is both simpler and correct, because a leading
  `//` in a licence text is never content.
- **The property is now testable rather than claimed**: `--cross-check <other-pdfium-dir>`
  renders against a second archive and requires byte equality. Run it after any pin bump.
  Its refusal to accept a directory with no `licenses/` is not defensive noise --- without it,
  pointing at the wrong path renders a document with the whole PDFium section missing, which
  "differs" and would read as the very failure being tested for.
- **This was found from a Mac, without a Windows machine**, by staging the other platform's
  archive with `fetch_pdfium.py --platform win-x64 --dest`. Worth remembering before waiting
  on a CI round trip to diagnose something: the input that differs can usually be fetched.

And the reason it was diagnosable at all is that `--check` was changed, in the same session,
to print the diff rather than the word "stale". A gate that fails on a machine you are not
sitting at is only actionable if its message carries the evidence.

### A pin that nothing verifies is indistinguishable from no pin

`rust-toolchain.toml` states a channel and rustup honours it --- unless
`RUSTUP_TOOLCHAIN` is set in the environment, which overrides the file **completely and
silently**. No warning, no note in `rustup show`, nothing in the build output.

That is not an exotic corner. It is what a CI action whose job is installing a toolchain may
reasonably do, and all three workflow jobs here used `dtolnay/rust-toolchain@stable`. Adding
the pin file alone would have produced the worst available outcome: the pin visible in the
repository, absent from every CI build, and green either way. The thing it was added to
prevent --- a new stable's lint turning `main` red under `-D warnings`, with nobody having
changed anything --- would have gone on happening, now with a file in the tree that looked
like it had been dealt with.

Two halves to the fix, and the second is the one that generalises:

- `rustup show` in place of the action. rustup ships on both runner images and that command
  installs exactly what the pin file names, so the file is the single source of truth.
  `components = ["clippy", "rustfmt"]` is not optional there: an on-demand install takes
  only what is listed, and omitting them gives a runner a toolchain where two gates fail on
  a missing binary rather than on anything real.
- **A gate that asserts the result.** `scripts/check_toolchain.py` compares the running
  rustc against the file and prints `RUSTUP_TOOLCHAIN` whether or not it is set, so the
  override is visible in every log rather than only when it bites. It runs *first*: if the
  compiler is not the one we think, every result after it is about a different toolchain.

Two things went wrong writing that check, both worth keeping.

- **The version numbers of a toolchain's own components do not agree, and cannot be compared
  arithmetically.** rustc is `1.97.1`, clippy is `0.1.97`, rustfmt is `1.9.0-stable`. The
  first draft asserted "the minors match" and failed on a perfectly correct toolchain,
  because clippy's `97` is its *patch* and rustfmt's version tracks nothing here at all. What
  all three do carry is the **commit hash of the toolchain build**, which is the actual
  oracle for "same toolchain" --- rustc prints nine characters of it and the others ten, so
  compare on the shorter.
- **The mutation that proved the gate was tested through `| tail`, and the exit code read was
  `tail`'s.** It printed `exit=0` for a run that had correctly failed. This repository has an
  entry saying not to do that; knowing it did not prevent doing it, in the one command whose
  entire purpose was reading an exit code. Re-run unpiped: `cmd > out.txt 2>&1; echo $?`.

The mutation also settled the premise rather than leaving it to the action's documentation:
`RUSTUP_TOOLCHAIN=beta` produced rustc 1.98.0 against a 1.97.1 pin. The override is real,
and now so is the check.

### A gate's static reason turned a crash into a wrong diagnosis, twice over

Windows CI reported `[FAIL] notices -- THIRD-PARTY-NOTICES.md is stale, or a forbidden
licence appeared`. It was neither. `third_party_notices.py` had **crashed before producing
any verdict at all**, and `gates.py` printed the reason string attached to that gate, which
is a hint written when the gate was added and not a statement about what happened.

That cost two rounds. The message named a content difference, so the investigation went
looking for one --- and found a real one, the `//` prefix on `pdfium.txt`, fixed it, and the
gate stayed red, because the genuine bug was somewhere else entirely.

The real failure is a trap **this repository already had an entry for**, reintroduced in a
new script on the same byte. `subprocess.run(..., text=True)` decodes with the *locale*
codec --- cp1252 on Windows --- and `cargo metadata` emits UTF-8 containing `0x81`. It gets
there from a crate author's name: `Emilio Cobos Álvarez`, whose `Á` is `C3 81`. cp1252
leaves `0x81` undefined, so the reader thread raises `UnicodeDecodeError`, `.stdout` comes
back `None`, and `json.loads(None)` fails with *"the JSON object must be str, bytes or
bytearray, not NoneType"* --- a message about JSON types, mentioning no encoding, from a
line that has nothing to do with the cause.

Four things worth keeping.

- **Always `encoding="utf-8"` on `subprocess.run(..., text=True)`.** Not when you expect
  non-ASCII --- always. The offending byte here is in a *dependency's author metadata*, which
  no amount of thinking about your own inputs would predict. `mutate_frontend.py` already
  carried this fix with a comment; `mutate_rust.py` did not, despite being documented as
  passing 22/22 on Windows, and reads `cargo test` output from a crate whose sources contain
  the same byte. Both are fixed now.
- **Knowing a trap does not prevent it.** The entry existed, was read this same session, and
  was written into a new file anyway. What actually stops it is the keyword argument, not the
  paragraph --- which this repository has already said once, about a cross-check that was
  described and never implemented.
- **A gate's reason string is a guess about the future.** Print the **exit code** beside it
  and word it as *"usually means"*: 1 is a checker saying no, anything else is usually a
  traceback, and the two want different first moves. A checker that dies and a checker that
  reports a failure are different events wearing the same label.
- **The diff the gate was taught to print never appeared**, and its absence was itself the
  evidence --- a `--check` that reaches its comparison always prints one. A missing diagnostic
  is information about *where* execution stopped, if you notice you did not get it.

### A decoder told to replace what it cannot read does, and the result ships

`read_text` in `third_party_notices.py` was `decode("utf-8", errors="replace")`, and the
docstring justified it as "tolerating the odd stray byte in a licence text". The tolerance
was the defect. `vendor/pdfium/licenses/freetype.txt` is Latin-1, and its `0xA9` is the `©`
in the credit line FreeType's licence **requires** be reproduced:

    Portions of this software are copyright <?> <year> The FreeType Project

So the document whose entire purpose is faithful reproduction shipped a required attribution
with the copyright sign replaced by a question mark in a diamond. **Exactly one U+FFFD in
469 KB** --- which is why it survived generation, review, a gate that compares the file
against a fresh render, and a cross-platform byte-equality check. Every one of those passed,
correctly: the corruption is deterministic, so a regeneration reproduces it exactly and the
comparison agrees.

This is the quiet direction of the `text=True` entry two above. There, an undefined cp1252
byte raised, `.stdout` came back `None`, and the failure was loud and immediate. Here the
decoder was *asked* to carry on past what it could not read, so nothing raised, nothing
logged, and the damage went into the artifact. **The loud one cost two rounds of debugging;
the quiet one cost a wrong legal notice in every installer built for a week.**

The fix is a codec chain, and the order carries the reasoning: UTF-8 first, because a UTF-8
file read as cp1252 is the mojibake direction and *never raises*; cp1252 next, being what
Windows tools emit; then latin-1, which maps every byte and cannot fail --- which is what
removes any path back to `errors="replace"`. A fallback is reported with a `[note]` naming
the file and codec, because a licence text that is not UTF-8 is worth seeing once rather
than absorbing silently.

Generalises past licences: **`errors="replace"` is only correct where the output is for a
human to glance at.** Anywhere the decoded text is copied into something that ships --- a
notice, a manifest, a signature payload, an invoice --- substitution is data loss with a
plausible-looking placeholder, and it is invisible to any check that goes through the same
decoder. Grep for it before trusting a generated artifact.

### A toolchain pin can match on version and still be the wrong ABI

`rust-toolchain.toml` pins `channel = "1.97.1"`, and `check_toolchain.py` asserts rustc
reports that version with clippy and rustfmt built from the same commit hash. On this
project's Windows desktop all of that passed and the build then died three gates later:

    error: error calling dlltool 'dlltool.exe': program not found

A bare `channel` carries **no target triple**, so rustup resolves it against its *default
host triple* --- which is a **different setting** from the default *toolchain*, with nothing
keeping the two in step. That machine's default toolchain was
`stable-x86_64-pc-windows-msvc`, which is what every Windows measurement in `AGENTS.md` was
taken on and what built the MSI; its default host triple was `x86_64-pc-windows-gnu`. Adding
the pin therefore moved the machine from MSVC to GNU, and the GNU ABI wants MinGW binutils
that were never installed.

Two things make it worth an entry rather than a footnote:

- **The gate that exists to catch "the compiler is not the one we think" said `[OK]`.** It
  was not wrong about anything it checked --- version matched, hashes matched. It simply did
  not check the axis that had moved. A pin verified on one axis reads exactly like a pin
  verified on all of them.
- **CI cannot see it.** GitHub's `windows-2025` runners default to MSVC, so the pin resolves
  correctly there and stays green forever. It is per-machine, invisible from the other
  platform *and* from CI --- which is precisely the shape that has to live in a gate, because
  nothing else in the system is standing where it happens.

Fix the machine, not the pin: `rustup set default-host x86_64-pc-windows-msvc`. Writing a
full triple into `rust-toolchain.toml` would pin one platform's ABI into a file both
platforms read.

The check now compares `rustc -vV`'s `host:` line against the ABI the platform actually
ships. Proved with a **real** wrong toolchain rather than a synthetic one --- the GNU install
was still on the box, so `RUSTUP_TOOLCHAIN=1.97.1-x86_64-pc-windows-gnu` gives the same
version, the same commit hashes, every pre-existing check passing, and only the new one
firing. That is the exact state the machine was in.

---

### A mitigation present and disclaimed is quieter than one claimed and absent

`docs/THREAT-MODEL.md` opens with a rule: every mitigation is either measured, with the
spike named, or marked untested. That rule is aimed squarely at the **over-claim** --- a
sentence in the present tense describing a control nobody wired --- and this repository has
been bitten by it three times, most sharply when §T3 gave the timing of a kill that could
not happen.

The fourth review, on 2026-08-02, found seven drifted claims and **six of them drifted the
other way**. §6 was still titled *"Windows --- a gap, not a policy"* and said *"none of it
is wired"*, four days after `Backend::default_here` started returning `Backend::Worker`
there and `sandbox_win` started building a job object and a low-integrity token around
every worker. Residual risk 4 carried the matching *"nothing uses it, so the risk is
undiminished"*. §T8 rested on *"none of it reaches the UI at all"*, which the sidebar and
search had falsified. §7.7 called a real `default-src 'self'` policy a scaffold default,
where Tauri's scaffold ships `"csp": null`. Only one --- `JOB_OBJECT_LIMIT_JOB_TIME`,
claimed by §6's table and set nowhere --- was the over-claim the rule anticipates.

**The asymmetry is the lesson, and it runs opposite to intuition.** An over-claim is
self-correcting on contact: it is a specific, checkable assertion, so the first person to
look for the line finds nothing and the document gets fixed. An under-claim is checked by
nobody, because nobody audits a document for being *too modest* --- it reads as diligence,
and diligence is not a defect anyone goes looking for. It survives exactly as long as it
takes for someone to independently rediscover the work.

And it costs more than a stale sentence. A threat model is what a reader plans against: a
reader budgeting from §6 on 2026-08-01 would have scheduled a Windows sandbox that already
existed, and one reasoning about the residual would have carried a risk that had been
closed. The over-claim wastes a check; the under-claim wastes the work.

The mechanism is mundane and is the part worth defending against: these sections were
written *before* the thing they describe was built, and the commit that built it touched
code, tests and `AGENTS.md` --- never the document whose job was to say whether it existed.
Nothing points from `sandbox_win.rs` back to the paragraph that disclaims it.

So the rule needs a second half, and it is now in the document: **a mitigation marked
untested must be re-read when the thing it describes is built, and the commit that wires a
control is the commit that owes the threat model a line.** The same review is what caught
§5 naming `worker_bench.rs`'s copy of the sandbox profile as authoritative when the shipped
one is `worker::SANDBOX_PROFILE` --- so `--mode authority`, which §8 names as *the*
re-verification after a PDFium bump, was certifying the spike's copy rather than the
profile that ships. That copy is now the production constant; the entry *"two copies of a
distinction drift"* is the general form.

---

### A snapshot taken after the first mutation restores the mutation, and verifies itself clean

Proving `scripts/check_webview_sinks.py` could fail meant four mutations, and the fourth
needed a repo-wide edit --- rename every `.setAttribute(` so the rule's pattern occurs
nowhere. The harness did this:

```python
t.write_bytes(renamed)                                 # mutate results.ts
saves = {p: p.read_bytes() for p in paths}             # <-- snapshot, one line too late
for p in paths: p.write_bytes(...)                     # mutate the rest
...
for p, b in saves.items(): p.write_bytes(b)            # restore
print(all(p.read_bytes() == b for p, b in saves.items()))
```

`saves` captured `results.ts` **after** it had already been mutated, so the "restore" wrote
the mutation back. Every other file restored correctly, which is what makes it hard to see.

**The verification could not detect it, and this is the part that generalises.** It compares
the file on disk against `saves` --- the same polluted snapshot the restore came from --- so
the two agree *because* they share the error. It is the *"a writer and its own reader agree
about output that is wrong"* family, arriving in a mutation harness rather than a PDF: a
restore checked against its own backup can only tell you the copy succeeded, never that the
backup was right.

The printed `byte-identical=False` was real but arrived for an **unrelated** reason: a
later line restored `results.ts` from the true backup, so the surviving disagreement was
between the good file and the bad snapshot. A verdict that is correct by accident is worse
than one that is wrong, because it retires the suspicion that would have found the cause.
What actually established the tree was clean was `git status --porcelain src/` and
`git diff HEAD -- src/` --- an oracle outside the harness.

Three habits, and the third is the cheap one:

- **Snapshot before the first mutation, always** --- including the file a previous step in
  the same harness already touched. The bug is an ordering bug, and ordering bugs in
  throwaway scripts are the norm rather than the exception.
- **Verify a restore against something the harness did not produce.** In a git repository
  that is free and total: `git diff HEAD -- <paths>` being empty is the whole check, and it
  is indifferent to what the harness believes about its own bookkeeping.
- **Prefer `git checkout -- <paths>` to a hand-rolled restore** when the mutations are of
  tracked files. There is no snapshot to get wrong.

Related from the other direction: *"a text-mode restore is not a byte restore"* --- that one
is about `read_text`/`write_text` corrupting content that compares equal; this one is about
a byte-perfect restore of the wrong bytes.

### A feature made a standing check false, and the only corpus that could tell had never been opened

`encoding.rs` and its frontend landed on 2026-08-02 with a deliberate change to what a
screen reader is handed: a page whose fonts declare no character mapping has its
characters **withheld** and a sentence given instead, because reading PDFium's guess aloud
is the symptom whose reader can least easily tell something is wrong.

`viewercheck.ts` has asserted since long before that *"the text read out is the page's own
text"*. On such a page that is now false **by design**, and the check went red the first
time `encodings.pdf` was ever opened in a window --- which was the day after the feature
shipped, on the other platform, during the handover. Eleven gates, 352 frontend tests, 92
Rust tests, 95 frontend mutations and CI on two platforms all stayed green, correctly: none
of them opens a document.

The failure was in the *check*, not the code, and that is what makes it worth an entry. The
new behaviour is right; the check encoded an assumption --- "spoken text is extracted text"
--- that had been true of every fixture, and a fixture that breaks an assumption is exactly
what a new corpus is for. It now branches three ways (no text / guessed text / stated text)
with the branch taken from the **fixture's manifest**, so a layer that quietly stopped
withholding fails rather than skips.

Two things generalise past this repository:

- **Adding a corpus is not the same as running it.** `encodings.pdf` had been in the fixture
  table, in the search probes and in three mutation harnesses for days. None of those opens
  a window, so the one assertion it contradicted never executed.
- **When a feature changes what a subsystem produces, grep the check suite for the old
  claim in the same commit.** The contradiction here was one sentence long and sitting in a
  file the feature's author had edited that afternoon.

### A rewritten line leaves a mutation aimed at nothing, and only the harness says so

`statusFor` gained the unreadable-page branch, and in doing so its one-line running clause
was restructured. `mutate_frontend.py` carried a mutation whose anchor was that exact line;
after the rewrite the anchor matched nothing, and the run reported

```
[FAIL] results: do not say a scan is still running: its anchor appears 0 times
```

which is the harness working exactly as designed --- it counts occurrences *before* the run
rather than inferring a stale anchor from a survivor afterwards. The point is what it cost
to find out: the harness is not in CI, takes minutes, and had not been run since the change,
so for a day the frontend suite had 94 live mutations and one that could not fire while the
summary still said 95.

**A mutation harness is a second consumer of every line it anchors on**, and nothing in a
compiler or a test run knows that. Two habits: after editing a file the harness covers,
`--list` is free and `grep` for the anchor is nearly so; and prefer the harness's own
anchor-count refusal to any scheme that silently skips a mutation it cannot place. A skipped
mutation reads as a passing one.

### A negative assertion needs an observable saying the question was asked

The check that a reader is told about an unreadable page runs on every corpus, and on nine
of ten what it asserts is that the sentence is **absent**. That assertion is satisfied
before anything happens: the count starts at zero, the panel starts empty, and a frontend
that never asked the backend anything passes it perfectly.

The obvious wait --- settle until `unsearchablePages` is what is expected --- cannot fix it,
because on those nine the value waited for is the value it starts at. What fixes it is a
separate observable for *asked and answered*, which is why `Search.mappingKnown` exists and
is public: the wait is on that, and the assertion about the count is made only afterwards.

The general shape, and it is common enough to be worth naming: **a check whose expected
outcome is "nothing happened" needs a second signal that the machinery ran at all.** A
timeout, a fetch, a subscription, an event that was not delivered --- in every case the
passing state and the not-yet-started state are the same state, and only an explicit
"finished" observable tells them apart. The cost is one boolean on a production type, which
is a fair price for a control that would otherwise be decoration.
### A stream split done for the failing direction leaves the passing one where it was

The entry above --- *"A wrapper's own verdicts are on the other stream, in the same shape as a
check's"* --- was written, the fix was applied, and the fix covered half the cases. On
2026-08-02 `scripts/viewer_check.py` still printed

```
[OK]   the app process never mapped the PDF parser 43 modules at peak over 115 samples
```

on **stdout**, beside its two `[FAIL]` forms on stderr, and `scripts/mutate_viewer.py`'s
`run_check` docstring asserted the opposite in so many words: *"Only stdout carries check
results."* So on Windows --- the only platform where that audit runs at all --- every baseline
silently carried one extra "check name" that no mutation could ever turn red. The number the
harness prints for a reader to sanity-check against `BUILD.md` was **164** where the transcript
had 163 checks.

Two things it cost, and the second is the one that would have been hard to diagnose:

- **`BUILD.md`'s cross-platform name count could not be reconciled by anyone reading both.**
  `viewercheck.ts` produces the names, the table records them, and one of the two machines
  quietly added one from outside.
- **`hits = [line for line in baseline if after_marker(line).startswith(m.expect)]`** matches an
  expectation against the wrapper's line as readily as against a check's. A mutation whose
  expected name happened to share a prefix with *"the app process never mapped the PDF parser"*
  would have been validated against, and then judged by, a line the mutation cannot affect ---
  reported as `SURVIVED`, which this file already records as the most misleading verdict a
  mutation pass can print.

The existing cross-check did **not** catch it, and that is not a fault in the cross-check: it
compares the `[FAIL]` count against the summary's arithmetic, and this line is an `[OK]`. A
consistency check proves agreement, never completeness, and what it cannot see here is an extra
*passing* line --- the direction nobody reads.

So the lesson is narrower and more useful than "split the streams": **when a stream split is
made because one direction was misclassified, move every direction the same day.** A pass is
where the drift hides, because a green run is the run nobody re-reads. The fix is three lines
(`file=sys.stderr`), and what proves it is not the code but the harness's own baseline print
going from 164 to 163 --- so if a wrapper prints a verdict at all, check the count it produces
against the transcript it wraps, on the passing path.

### A per-page invalidation counter is not the same as a generation

`scroller.ts` had two epoch counters before per-page geometry landed, and they are the right
model for what they do: `generation` is bumped when tier 2 is cleared, `placeholderGeneration`
when tier 1 is dropped, and a reply naming a stale one is closed on arrival rather than drawn.
Both are **document-wide**, because the events that bump them --- a zoom, a rotation, an
inversion --- change every page at once.

Learning a page's real size is not that event. It changes one page's box and leaves every other
page's pixels perfectly valid, and the temptation is to reuse the counter that is already there,
because it is one line and it is *correct*: bumping `generation` does invalidate the stale tile.
It also invalidates every other tile on screen. On a document being read straight through, each
page's size is learned exactly once as it comes into the band, so the whole screen would be
thrown away and re-rendered **once per page** --- an invalidation storm with no wrong pixel
anywhere to say it is happening, on the one code path whose entire justification is that
correcting geometry is cheap enough to do on the frame loop.

What replaces it is an `epochs: number[]`, one entry per page, captured into the request closure
and compared in `drain`. The shape is worth naming because the choice recurs: **the granularity
of the counter has to match the granularity of the event, and a coarser counter is not a
conservative approximation --- it is a performance defect that no correctness test can see.**

Three details that are not obvious from the outside:

- **A withdrawal is not enough on its own.** `withdraw` is gated on the `cancel` variant flag and
  in any case cannot reach a render that has already finished, so the epoch is what actually
  drops the reply. Both are needed and they do different jobs; a mutation replacing the epoch
  comparison with `false` turns the test red while every withdrawal still happens.
- **The entry stays in `inFlight` until its reply lands**, so `request` will not issue a
  duplicate for a tile still on its way. The consequence is that the corrected page is asked for
  again only after the stale requests settle --- a test that takes a frame immediately after the
  correction sees the one genuinely new column and reports that as the whole answer. It did.
- **The stale reply must be counted as neither delivered nor discarded.** Nothing about the
  scroll invalidated it, so charging it to `discarded` would report a supersedable-queue failure
  that did not happen, in a counter that a benchmark reads.

And the mutation that proves the whole thing is not the epoch check but the layout: reverting
`computeGeometry` to lay every page out at page 1's size turns five unit tests red *and* three
checks in the real app, one of them reporting `0% page` where the A3 page's own ink is.

### Two budgets for one run, and the one that was raised is not the one that decides

`viewer_check.py` bounds a run at 900 s, and its comment says exactly why the number moved
there from 300: *"`vector-multi` renders twelve A0 pages and measured 276 s, i.e. it passed or
timed out depending on the machine's mood."* The raise was worth nothing. The app runs a
watchdog of its own (`start_watchdog`), whose viewercheck default stayed at **300 s**, and that
one calls `process::exit(2)` itself --- so the harness's `communicate` never got to arbitrate.
The tighter of the two budgets is the one that decides, and it was not the one anybody was
editing.

Measured on macOS 2026-08-03, and the spread is the finding rather than any one figure:
`vector-multi` ran **275 s**, **387 s**, and once past **600 s**, while `vector-heavy` was
killed at 300 s and then finished in **249 s** on the same binary. Roughly 2.2x on one
document, with the bound sitting inside it --- so this was a coin flip, not a consistent
failure, which is exactly why it lasted. A check that fails every time gets fixed; one that
fails half the time gets re-run, and the re-run passes.

Three things worth carrying:

- **A second enforcement point turns a fixed bug back into a live one, silently.** The comment
  above is correct, prominent, and describes a defect that was still shipping. Nothing in
  either file mentions the other, so reading either one leaves you sure the bound is what it
  says. When a limit exists twice, the fix is one number with the other derived from it ---
  `viewer_check.py` now sets `TPDF_VIEWERCHECK_TIMEOUT` from its own `--timeout`, keeping the
  intended ordering (the watchdog fires first, because its timeline says *where* the run
  stopped, which `communicate` cannot) as a consequence rather than a coincidence.
- **The failure presented as a hang in whatever was last changed**, which is what it was
  hunted as: it landed the day a shared `reading.ts` rule changed, on the two corpora that
  came back red, and the merge was the obvious suspect. What it actually pointed at was the
  fit-page setup on an A0 page --- the operation that defeats spatial culling, since the whole
  page becomes visible and PDFium charges its large fixed cost per render call. The tell was
  that two documents differing 12x in page count stopped at the *identical* check: a clock
  running out stops wherever it stops, and a shared cost stops in the same place.
- **The cheap decisive experiment is the bound, not the code.** Re-running with the watchdog
  raised took one command and separated "slow" from "wedged" before anything was instrumented
  or bisected. Same family as running a suspect binary bare before instrumenting it.

### A workflow copied from CI can lose a whole step, and then the release gate is the weaker one

Found 2026-08-03 by the first tag this repository ever pushed, which is a late and expensive
place to find anything. `release.yml`'s `gates` job was written from `ci.yml`, and the copy
dropped one step: the one that generates the fixtures a hosted runner can build. So
`print.rs`'s `a_third_parser_checks_a_job_built_from_a_document_we_did_not_write` --- which
needs `rotated.pdf` --- failed on **both** runners, while passing in CI and passing locally.

Three things make this worth an entry rather than a one-line fix.

- **The failure was maximally confusing in the direction that wastes time.** A Rust unit test
  went red on a commit that changed no Rust source, on both platforms at once, minutes after
  the same test passed in CI on the parent commit. Every instinct says flaky runner or a
  toolchain roll. The reflex that settled it in one command was running the named test
  locally, and then diffing the two workflows' step lists --- *not* reading the test.

- **`AGENTS.md` already carried the rule and it did not help.** "A release checklist must
  state the CI gates verbatim" is about `BUILD.md` losing a `--locked`; this is the same
  failure in YAML, with a whole step missing instead of a flag. Both workflows even carried
  comments explaining why the fixture step mattered. A comment cannot go red, and the copy
  that dropped the step was made by someone who had read it. **The rule was written as prose
  and was followed for the file it named**, which is exactly the "a rule written as a caution
  gets nodded at, the same rule written as a line of code gets followed" entry above, arriving
  in the one place where a weaker gate is worst: the thing that decides whether a tag ships.

- **The assertion that caught it was already there, and had caught the same gap once before.**
  `print.rs` guards with `assert!(examined > 0)`, so a run where every fixture is missing goes
  red instead of reporting a suite of skips. `ci.yml`'s own comment records that this is why
  the *first* CI run went red. One guard, two independent workflow gaps, and it is the only
  reason either was found at all --- keep that shape whenever a test corpus is generated
  rather than committed.

The fix is two-part and only the second part is durable: the list of runner-generatable
fixtures moved into `scripts/ci_fixtures.py`, so both workflows call one line and there is
nothing left to copy; and `scripts/check_workflow_parity.py` is a gate that compares the two
`gates` jobs step for step --- every `uses:` with its pin and every `run:` body, in order.
Names are not compared, and a control proves it: rewording a step label stays green while
repointing a pin, weakening a gate command, deleting a step and renaming the job all go red.

**The generalisation, for any repository with two workflows that are supposed to agree:**
duplicated YAML is not reviewed like duplicated code, because nothing compiles it and nothing
imports it. If two jobs must be the same job, either they call one script or something
compares them --- and the second is needed even after the first, since the setup steps around
the shared call are exactly where the next copy will drift.

### A step that signs before anything imports the certificate fails with the masked secret as its error

The second half of the same tag push (2026-08-03). With the gates fixed, `release.yml` reached
its macOS leg for the first time and died on *"Sign the vendored PDFium"* with:

```
***: no identity found
```

`***` is GitHub masking `APPLE_SIGNING_IDENTITY`, so the error names nothing at all --- the one
piece of information a reader wants is exactly the piece the masking removes, and the message
reads like a broken secret rather than a missing keychain.

**The cause was an ordering error licensed by a true sentence.** The preparation step's comment
said the Tauri CLI *"imports the certificate itself from `APPLE_CERTIFICATE` and
`APPLE_CERTIFICATE_PASSWORD` and sets the key partition list ... No manual keychain step."*
Every clause of that is correct about the CLI. It is false about the *job*, because the CLI's
import happens inside `tauri-action`, which runs **after** the step that signs the vendored
dylib --- and that step exists precisely because the dylib has to be signed before the bundler
copies it. So the workflow contained a step whose prerequisite was created two steps later.

Worth generalising: **a true statement about a tool is not a statement about when it runs.** The
comment would have been correct in a job that only signed through the CLI, which is the job both
sibling repositories have and this one was copied from. Whenever a workflow adds a step *before*
the tool that was doing all the work, re-read the setup comments as claims about ordering rather
than about capability.

**And the local rehearsal was stricter than the runner, which nearly wasted the fix.** The block
was rehearsed against a synthetic `openssl`-generated `.p12` before pushing, and its own
verification failed: `security find-identity -v -p codesigning` reported **0**. That reads as the
fix not working. It was the rehearsal: `-v` means *valid identities only*, and a self-signed
certificate is `CSSMERR_TP_NOT_TRUSTED` by construction, so it can never be valid whatever the
import did. Dropping `-v` to make the rehearsal pass would have removed the check that catches a
`.p12` shipped without its intermediate --- a real failure mode, and one that otherwise surfaces
forty minutes later at `notarytool`.

The discriminating command is the same one without `-v`, which lists identities *and* why each
is unusable:

```
1) BCBE...5F85 "Developer ID Application: ..." (CSSMERR_TP_NOT_TRUSTED)
   1 identities found
```

One identity present-but-untrusted and zero identities are different failures wanting different
fixes, and the gate now prints that listing on failure for exactly that reason. This is the
inverse of the *"An oracle more forgiving than the thing it stands in for cannot fail"* entry
above: an oracle **stricter** than the real thing fails when nothing is wrong, and the damage is
that the honest response --- loosen the check --- is the wrong one.

### The verification step failed after everything it verifies had succeeded, because `mapfile` is bash 4

Third and last defect of the same tag sequence (2026-08-03, `rc3`). The macOS leg got all the
way through: the vendored dylib signed, `tauri-action` built and published, the `.app`
notarized `Accepted`, the DMG notarized, stapled, and `The staple and validate action worked!`
printed. Then *"Verify the macOS build is signed, notarized and stapled"* exited **127** and
took the job red.

127 is "command not found", and the command was `mapfile`. **macOS ships bash 3.2 as
`/bin/bash`** --- Apple froze it at the last GPLv2 release --- and GitHub's `shell: bash` on a
macOS runner is that 3.2, so a bash 4 builtin is simply absent. Under `set -euo pipefail` the
step then aborts with no message of its own, which is the worst possible shape here: the
release was *fine*, and the thing that said otherwise was the checker.

Three things worth carrying.

- **A broken verifier and a real failure are indistinguishable from the outside**, and the
  instinct on seeing a red "verify signed and notarized" step is to go looking at the
  signature. The tell was the exit code: no `::error::` line from any of the step's own
  guards, each of which prints one, and 127 rather than 1. **When a step with explicit error
  messages fails without printing one, suspect the script before the subject.**
- **This is locally reproducible and nobody thinks to try**, because the ambient shell is
  modern. `zsh` is the login shell on these Macs and `/opt/homebrew/bin/bash` is 5.x, so every
  local rehearsal runs under something newer than the runner. `/bin/bash` **is** 3.2 on the
  same machine: prefixing a rehearsal with `/bin/bash -c` reproduces the runner's shell
  exactly, and it reproduced this in one command. Do that for any workflow shell body before
  pushing a tag to find out.
- **The portable form is three lines and has no downside**, so there is no reason to reach for
  `mapfile` in CI at all:

  ```bash
  ARR=()
  while IFS= read -r item; do ARR+=("$item"); done < <(some-command)
  ```

  The same family: `readarray`, `declare -A`, `${var,,}`/`${var^^}`, and `globstar`. All are
  bash 4, all are absent on a macOS runner, and all fail as 127 or as a silently wrong value.

The wider lesson is the one this file keeps arriving at from new directions: **the last step of
a pipeline is the least-tested code in it**, because everything before it has to succeed before
it runs even once. Three tags were needed here and each one failed one step later than the
last --- the gates, then the signing, then the verification. That is not bad luck, it is the
shape of testing a sequence end to end for the first time, and it is an argument for rehearsal
tags rather than against them.

### A mirrored value read after "idle" is the previous operation's, and it flaked on a release artifact

`viewer_check.py`'s search phase keeps a `seen` object that the viewer fills through
`onStatus`. One check searched for a deliberately broken pattern, waited for
`!viewer.searching`, and read `seen.status.search.problem`. A broken pattern is rejected
almost immediately, so the viewer can be idle again *before* the status carrying the problem
has been delivered --- and the read then returns the previous search's status, with an empty
`problem`, and the check fails.

It failed **once in three runs against a byte-identical binary**, and the run it chose was
the first check of a freshly notarized `26.8.0` release artifact, downloaded from its own
draft. So the first evidence about the thing that was about to be published was a red check,
in a document phase, on a build that had passed everything else. Two re-runs cleared it and a
mutation confirmed the app was fine. **A flake costs most when it lands on the artifact you
are deciding about**, which is exactly when a rare failure is most likely to be read as a
discovery.

Three things generalise.

- **Waiting for "the operation finished" is not waiting for "its result arrived"** wherever
  the result reaches you through a mirror --- a callback, a store, a subscription, an event.
  The two are the same only if delivery is synchronous with completion, which for anything
  crossing a frame or a channel it is not. Same family as the stale focus-mirror entry above.
- **The obvious fix is the wrong one, and it is worth naming.** Waiting for `problem` to be
  non-empty removes the flake completely --- and makes the check unable to fail, since
  `problem` is the value being asserted. It could then only pass or hang, which this file has
  a separate entry about. Wait on something that says *a delivery happened* and nothing about
  its content: here a monotone `updates` counter incremented in `onStatus`, captured before
  the search and compared after.
- **The control needs the same wait for the opposite reason.** The literal-search control
  asserts `problem` is empty, so a stale mirror there still holds the problem set a moment
  earlier and the control fails rather than passing. One race, both directions, and only one
  of them was visible.

Proved rather than assumed: with the wait fixed, the mutation aimed at this check
(`problem: Some(problem)` -> `problem: None` in `search.rs`) still turns it red, so the flake
was removed without removing the failure the check exists for.

### A MAP_SHARED document does not pin the file, so a truncation is a SIGBUS

The viewer maps the document with `MAP_SHARED` and hands the worker a descriptor, which is
what lets a sandboxed process read a file it has no authority to open. A mapping does **not**
hold the file's length: another process can shorten it while the document is open, and every
page beyond the new end is then unbacked. Reading there is not an error return --- it is
`SIGBUS` at the faulting instruction, which kills the worker.

Measured 2026-08-04, deterministically, by `examples/rewrite-probe`: eight runs, signal 10 on
macOS every time. Two things beside it are worth as much as the finding.

**The other two ways a file gets rewritten are both benign, and only one of them is
obvious.** A temporary written and renamed over the path leaves the old inode alive under the
mapping, so the reader keeps the whole document indefinitely --- stale but coherent, and the
probe proves it by renaming a *deliberately invalid* file into place and then rendering a page
the worker had never parsed. An in-place overwrite that keeps the length is visible to the
worker, but fails closed: `FPDF_LoadPage` returns null rather than drawing from bytes it
cannot parse. Only shrinking is fatal.

**So the check is on the descriptor, never on the path**, and that is the whole design rather
than a detail. `metadata()` on the path after a rename reports the *replacement* file's
length, concludes the document was truncated, and condemns a document that is perfectly
healthy --- every time the reader's own editor saves. `Shm::backing_len` asks the file the
mapping was made from, which nothing but a real truncation can shorten.

It cannot be prevented, and trying is the wrong instinct: the file can be shortened between
any check and the read that faults on it. What it can be is **fail-stop and legible** --- the
page never renders, the pool diagnoses it, and the reader is told.

### A pool that replaces a dead worker with the same bytes faults again, forever

The crash path's whole design is that a replacement worker is handed the **same** document
mapping, so it serves the same bytes as the one that died --- deliberately, since re-reading
the path would silently swap the reader's document for whatever is on disk now. Against the
truncation above that guarantee inverts: the replacement maps bytes that are gone and faults
exactly where its predecessor did.

Nothing loops, so it never looks like a runaway. It is bounded by the requests the reader
makes --- and a reader scrolling through the missing tail makes one per tile, each paying two
process spawns and two `SIGBUS` deaths for a region that can never render, with one line in
the log and a blank area on screen.

The fix is a latch on the document rather than a smarter retry: once the file is known to have
lost bytes, every checkout refuses without spawning anything. Measured after the change ---
0.01 ms worst of twenty requests against 1.3 ms for the one that diagnosed it, which is the
observable that says no process is being created, since a spawn alone is ~12 ms.

### A diagnosis placed after a liveness check inherits that check's race

The sharpest form of *"a worker killed a moment ago still says it is running"*, and it bit in
a new place. That entry is about the deadline path; the same `try_wait` sampling decides the
**crash** path, where `with_worker` asks `worker.is_running()` to tell "a live worker that
answered with an error" from "a worker that died". A worker that faulted microseconds ago
answers *still running*, so the corpse is checked back into the pool and the caller gets the
raw pipe error.

A new diagnosis placed *after* that check therefore never ran on the first failure. It ran on
some later request, once `try_wait` had caught up --- so the log carried a correct explanation
while the reader was handed `worker stopped answering (still running)`, and the mechanism
looked like it worked.

Two things follow. **Ask about the world before asking about the process**: the file check is
an `fstat` on a path that has already failed, it cannot produce a false diagnosis, and it does
not care whether the worker is reaped yet. And **an epitaph taken at that instant is worse
than none** --- it renders as *"worker still running; this file changed on disk"*, a sentence
that contradicts itself.

Found by the probe, not by a unit test, and that is the honest limit: the ordering is only
reachable with a real fault. Re-swapping the two reproduces the original defect exactly, which
is the mutation that makes the fix a claim rather than a hope.

### A class used with `instanceof` must not live in a module the tests mock wholesale

Four test files replace the tile client with `vi.mock("./tiles", () => ({ ... }))`, and a
factory supplies only what its author remembered to name. A failure class exported from that
module is therefore `undefined` inside all of them, and `reason instanceof undefined` throws a
`TypeError`.

What makes it expensive is where it throws: inside the scroller's *failure* handler. The tile
is never settled, the frame loop never quiesces, and six tests fail in two files that have
nothing to do with the change, all saying **"the frame loop never settled"** --- which reads
as a bug in the frame loop. Nothing points at the mock.

Moving the class to a module nobody mocks fixes it for every existing test and, more to the
point, for the next mock somebody writes without knowing this. It also *improves* the test
that motivated the class: the scroller's suite can now throw the real one instead of a
stand-in, so `instanceof` means the same thing there as in production.

The general shape: **a wholesale module mock silently narrows that module's exports to
whatever the factory lists**, so anything whose *identity* matters --- a class, a symbol, a
sentinel object --- is fragile there in a way a function is not. A function comes back as
`undefined` and fails loudly at the call site; an identity check fails as a type error deep
inside a handler.

### A valid in-place rewrite is served silently, and a length check cannot see it

The truncation entry above records the fatal case and the guard built for it, which compares
the mapping's length against the file's. This is the case that guard is structurally blind to,
and it is quieter than a fault.

Measured 2026-08-04 by `rewrite-probe`, deterministically over three runs. Two documents are
generated with identical structure whose content streams differ by one character --- so both
are valid PDFs, every object sits at the same offset, and the files are the same length to the
byte. Writing the second over the first, in place, under an open document produces:

- page 2, already rendered, still shows **revision A**, because PDFium has it in its object
  cache;
- page 190, never touched, renders **revision B**, because the cross-reference offsets the
  worker parsed at open still point at real objects --- belonging to a document it was never
  given;
- **no error anywhere**, and the length is unchanged, so nothing in `workers.rs` has anything
  to compare.

So the open document is one revision in its cache and another on disk, and a reader scrolling
through it sees a document that has never existed. Everything downstream --- search hits, text
extraction, a print job, a copied selection --- is drawn from whichever half answered.

Three things worth carrying:

- **Equal length is not "no change", and a length check was never a change detector.** It
  answers exactly one question, "are the bytes I mapped still there", which is the question a
  `SIGBUS` asks. Reading it as "has this file changed" is the mistake to avoid.
- **What would detect it is `mtime` on the mapped descriptor**, not on the path --- the same
  distinction the truncation entry makes, and for the same reason: a rename-over leaves the
  mapped inode untouched and healthy, while an in-place write keeps the inode and moves its
  timestamp. Those are opposite findings and a path-level file event cannot tell them apart.
- **There is no crash to hang the check on.** The truncation guard runs on a path that has
  already lost a worker, which is why it costs nothing. Nothing fails here, so noticing at all
  means asking on some schedule --- which is a watcher, and a decision about polling rather
  than a bug fix.

The probe's own controls are what make this readable rather than a guess: the two revisions
are asserted to *render differently* first, since if they looked alike "it served the new
bytes" and "it served the old ones" would be the same picture; and the pair is asserted to be
the same length, so a generator that drifts turns the scenario into a `[SKIP]` naming the two
sizes instead of a finding about the wrong mechanism.

### Reaching for a constant *because* it is portable, and picking the one that is absent

`rewrite-probe` names the signal behind a worker's death, and the doc comment beside it says
why the number is not written as a literal: `SIGBUS` is 10 on macOS and 7 on Linux, so a
literal would be right on one platform and quietly wrong on the other. `libc::SIGBUS` is the
correct instinct and it broke the Windows build, because Windows has no POSIX signals and the
constant does not exist there at all.

The reasoning was right and the mechanism was not, which is the part worth keeping: portability
thinking that stops at "which value" never asks "does this symbol exist". A literal `10` would
have compiled everywhere and been wrong somewhere; the constant compiled nowhere on one
platform. Both are portability bugs and only one of them is loud.

Three things around it, and the second is the expensive one.

- **It landed on `main` and went red in CI**, on a change whose macOS half was measured eight
  ways. Every gate passed locally --- `bins` builds examples, so it *would* have caught this,
  on a machine that could compile for Windows. This one cannot, which is the standing asymmetry
  in this repository: the machine that writes the work is the machine that cannot compile half
  of it.
- **A local Windows compile check is closer than it looks, and still out of reach.**
  `rustup target add x86_64-pc-windows-msvc` plus `fetch_pdfium.py --platform win-x64` gets as
  far as the build script, which then panics in `tauri-winres` with `NotAttempted("llvm-rc")`.
  Reaching a real `cargo check` needs LLVM installed for one binary. Worth knowing before
  spending the download; not worth the download for a two-branch `cfg`.
- **What *is* cheap is compiling the changed fragment alone.** `rustc --target
  x86_64-pc-windows-msvc --emit=metadata` on a file holding just the two `cfg` arms needs no
  build script, no crate graph and no linker, and it answers the only question at issue. With a
  control: the same file with the split removed fails on the same line. Note the control fails
  with *"unresolved crate `libc`"* rather than CI's *"cannot find value `SIGBUS`"*, because a
  standalone compile links no `libc` --- same line, same conclusion, different message, and
  saying so is the difference between a check and a demonstration.

The related lesson for the probe itself: the truncation it provokes is **unreachable** on
Windows, since a file with a section object mapped over it cannot be shortened
(`ERROR_USER_MAPPED_FILE`). That refusal is a result rather than an obstacle --- it is the first
test of a belief `Shm::backing_len` records and nothing had ever exercised --- so the scenario
reports it as a `[SKIP]` naming the OS error rather than failing three checks that are not
about the document.

### The same platform refusal, a result in one scenario and a failure in the next

The entry above is the reasoning, and it was applied to exactly one of the two places that
needed it. `rewrite-probe` truncates twice --- once against a bare `Worker`, once through the
render service the viewer actually uses --- and Windows refuses both identically, with
`ERROR_USER_MAPPED_FILE` (os error 1224). The first scenario was built knowing that: it carries
a `mutation_error` field whose entire purpose is to keep a refusal distinct from a failure, and
it reports `[SKIP]`s naming the OS error. The second discarded the error and recorded `false`
with the detail `"failed"`.

So the first Windows run of a probe written to turn that belief into a measurement produced the
measurement in one scenario and a red line in the other. Two defects, and the second outlives
the first:

- **The detail said `"failed"` and threw the error away**, so the one row holding the answer
  printed the least informative string in the file. `os error 1224` *is* the finding.
- **Three check names vanished rather than skipping.** The failure path `return`ed, so
  `the first request past the truncation is diagnosed`, `every later request is refused with the
  same diagnosis` and `a refused request costs nothing` were simply absent on Windows. A name
  that goes missing is the failure this arrangement exists to *catch*, not one it should
  produce --- and nothing said so, because the summary read `19/20` and looks exactly like an
  ordinary single failure. The name set was 20 here against 22 on macOS.

The fix records the name on the way through when the truncation does land, so the set is 23 on
both platforms and only the verdicts differ. Proved by mutation in both directions on one
platform: forcing the success branch turns two of the three red and leaves the total at 23.

Two things to carry. **When a platform fact is handled somewhere, grep for the other places
that meet it** --- the thinking had already been done and written into a field, a doc comment
and a scenario, and the sibling twenty lines below never received it. And **an early `return`
on a failure path is how a name set silently changes size**; here it also skipped the
service's own `close`, which the comment immediately below it exists to prevent.

### A command deliberately left out of the window harness still has to be classified

`file.reload` was added with an explicit, reasoned decision *not* to drive it from
`viewercheck.ts`: the commit argued that a thirtieth entry there proves little the others do
not, while moving a per-corpus invariant that "cannot honestly be incremented by hand" and
would need a full corpus run of a harness that requires an unlocked screen. Every part of that
is correct, and the unit tests chosen instead genuinely cover the wiring.

What it missed is that `viewercheck.ts` does not only *drive* commands. It also asserts that
every registered command is **classified** --- either driven, or listed in `undriven` with the
reason it cannot be. A command that is neither is unclassified, and the check goes red:

```text
[FAIL] every registered command is classified, and every classification is registered
       30 registered, 26 driven, 3 not driven; unclassified [file.reload], stale []
```

The count reasoning was right: the invariant stayed at **163** names, because an `undriven`
entry classifies without adding a check. The run was red anyway. **"This will not move the
invariant" and "this leaves the harness green" are different claims**, and only the second is
the one worth making. The completeness check exists precisely so that "not covered" is a
decision recorded in a table rather than an absence --- which makes opting out an edit, not an
omission.

Two things made it expensive to find, and both generalise. It is **corpus-independent**, so it
fails on every fixture identically and no amount of choosing a better fixture would have
surfaced it earlier. And it is **invisible to CI**, because `viewer_check.py` needs a real
window and a headless runner cannot run it at all --- so it sat under a green 13/13 gate run
and two green CI jobs, and appeared on the first machine to open a window after the commit.

### A PATCH that sets only the body clears the draft's tag, and publishing then attaches it to nothing

Step 11 says to read the release body before publishing, because it is a literal in
`release.yml` and can only ever be as current as the last person who read it. On 2026-08-23 it
was stale --- it listed **stamps** under *What it is not, yet* one release after they shipped ---
so the draft was corrected in place:

    gh api -X PATCH repos/tstone-1/tpdf/releases/<id> --input patch.json   # {"body": "..."}

The body changed, and so did something nobody asked to change. The response came back with
`"tag_name": "untagged-53698856d15ecfb119ff"`, and GraphQL agreed: the release that had read
`tagName: v26.8.8` a minute earlier was now attached to no tag at all. Publishing it in that
state would have produced a release whose assets sit under `releases/download/untagged-<hash>/`
--- exactly the URL shape the entry below describes for a *draft*, except public, permanent and
next to a `v26.8.8` tag pointing at a commit with no release on it.

**A draft has no tag until it is published**, so `tag_name` on one is a *field* rather than a
fact, and GitHub treats a PATCH that omits it as a request to reset it. That is the half worth
carrying: a partial update to a REST resource is only partial for the fields the server chooses,
and the draft's tag is not one of them. Restoring it is one call --- `-f tag_name=v26.8.8` --- and
publishing should carry it too, since the same reset applies to the call that flips `draft`:

    gh api -X PATCH repos/tstone-1/tpdf/releases/<id> -f tag_name=v26.8.8 -F draft=false

**Read it back with GraphQL, not with the response.** The response is authoritative here and it
was, but `gh release list` still showed the draft under its name, and `gh release view v26.8.8`
resolves through the REST endpoint that does not return drafts --- so the two commands most
likely to be reached for both report the situation as normal. The GraphQL query step 11 already
prescribes for counting assets prints `tagName` beside them, which makes it the one instrument
that answers both questions at once.

The `latest.json` asset is a separate copy of the same prose and was left alone deliberately:
`tauri-action` fills its `notes` from the body at *build* time, so it still carries the stale
sentence, and nothing in tpdf reads that field --- the updater shows only the version. Replacing
a signed release's asset to correct text no reader sees is the worse trade.

### A draft release is invisible, and the tag beside it says the work shipped

`v26.8.0` was tagged and pushed on 2026-08-03. The `Release` run went green on all four jobs
in 20m19s --- gates on both runners, then the Windows and macOS builds --- and uploaded four
artifacts: a notarized DMG, an MSI, an NSIS setup and the bare `.app.tar.gz`. The release body
was written. Nine days later the question was *"why is there no release yet tagged in github /
available for download?"*, and the answer was that the release had been sitting as a **draft**
the whole time, which GitHub shows to repository owners and to nobody else.

That is `release.yml`'s `releaseDraft: true`, and it is deliberate --- a failed run must publish
nothing, and the draft is the last chance to edit the body before it is public. The defect is
not the flag. It is that **publishing was described and never listed**: `BUILD.md`'s checklist
ended at step 10, whose own closing sentence read *"the last checkpoint is a human publishing
it"*. A step named inside the prose of the step before it is not a step anybody executes.

**Every instrument that was consulted was true and structurally unable to answer.** The tag is
public and `git ls-remote --tags origin` shows it; `gh run list` shows the `Release` run
`completed success` with its four jobs. Both are facts about the *build*, and neither says a
word about publication --- so from outside, a public tag with no release beside it reads as a
tag pushed by mistake. The one instrument that answers is `gh release list`, which had been
printing the word `Draft` in its second column for nine days, and the asset URLs, which lived
under `releases/download/untagged-dab938da94342ec9bfb5/` rather than under the tag. Neither was
hidden. Neither was asked.

Three things worth carrying.

- **This is the last-step-of-a-pipeline lesson one step further out than the entries above it.**
  The three rehearsal tags each failed one step later than the last --- gates, then signing,
  then verification --- and this failed at the step *after* the last one any machine executes.
  Nothing could go red: no runner runs it, no gate covers it, and a checklist cannot fail. The
  earlier entries argue that the tail of an automated sequence is its least-tested code; the
  tail of a *manual* sequence has no tests at all, so it has to be written down as an
  imperative line rather than as a clause.
- **"Green" is a claim about the run, not about the artifact reaching anyone.** A build that
  succeeds, signs, notarizes and uploads has done everything except the thing the user wanted.
  Distinguish *produced* from *published* whenever a document says a version shipped.
- **Verify publication from outside the account.** `gh release list` reporting `Latest` is our
  own view; an unauthenticated fetch of the download URL is the reader's. After publishing,
  `curl -sIL -o /dev/null -w '%{http_code}'` against
  `releases/download/<tag>/<asset>` returned **200** and followed through to GitHub's asset
  CDN, which is the check that says the file is reachable by someone who is not signed in.

The documentation failed in the same direction and is part of the trap. `CHANGELOG.md`'s
preamble said *"The first release is `26.8.0`, tagged 2026-08-03"* --- true of the tag, false
about anything downloadable --- in a file whose preamble already carried a parenthetical about
having contradicted its own top entry once before. **A release date in prose is a claim that
something is fetchable, and the word "tagged" does not weaken it enough for a reader to
notice.**

### A refusal that carries a `NaN` is not equal to itself, and both sides print the same

`docmodel.rs` refuses a crop box enclosing no area, `NaN` corners included, and the refusal
carries the offending rectangle so a caller can say which one. The test was written as the
obvious equality:

```rust
assert_eq!(
    doc.apply(Command::Crop { page: a, to: Some(bad) }),
    Err(Refusal::DegenerateCrop(bad)),
);
```

It failed on the first `NaN` case, and the failure is worth reading before the explanation:

```
assertion `left == right` failed
  left: Err(DegenerateCrop(Rect { llx: NaN, lly: 20.0, urx: 30.0, ury: 40.0 }))
 right: Err(DegenerateCrop(Rect { llx: NaN, lly: 20.0, urx: 30.0, ury: 40.0 }))
```

**Two identical lines and a failed comparison between them.** The code was right --- the
rectangle was refused, with the right variant and the right payload. `PartialEq` on the
rectangle compares the floats, and no comparison against `NaN` is true, so
`Refusal::DegenerateCrop(nan) == Refusal::DegenerateCrop(nan)` is `false` and the derived
equality on the enum inherits it. The fix is to match the variant rather than compare the
value.

Three things generalise past this one type.

- **A derived `PartialEq` is only as reflexive as its fields.** Any type holding an `f32` or
  `f64` --- a rectangle, a point, a matrix, a duration in seconds, a DPI --- makes every enum
  and struct that contains it non-reflexive for the values that matter most, which are
  exactly the pathological ones a test is written to pin down. Reach for `matches!` when the
  payload can be `NaN`, and say so at the type rather than at each call site.
- **This one fails loudly, which is the only reason it cost minutes rather than weeks.** The
  usual shape in this file is an assertion that cannot *fail*; this is an assertion that
  cannot *pass*, and the whole file is about preferring the second. It still has to be
  recognised, because printed output showing two identical values either side of a failed
  `==` reads as a broken harness, and the instinct is to distrust the test framework.
- **The refusal itself is correct and stays correct.** Nothing here argues for changing the
  comparison semantics of the rectangle to make the test easier: a crop box with a `NaN` in
  it is not equal to another one, and a model that said otherwise would be lying about
  geometry to suit an assertion.

### A test that walks every prefix of a journal still could not see the snapshot rule

`docmodel.rs` rebuilds its working document from the nearest snapshot at or below the target
cursor. The mutation for that rule takes the newest snapshot instead, wherever it sits, and
it was aimed at `a_journal_replays_to_the_state_it_was_applied_to` --- the general property
test, which drives a mixed journal of rotations, moves, deletes and crops, then checks
**every prefix** against a replay from the baseline, then every undo down and every redo back
up. If any test covers "a rebuild lands on the right state", it is that one.

It cannot see this rule at all. It applies **eight** commands and `SNAPSHOT_EVERY` is **32**,
so no snapshot is ever taken, `nearest` returns 0 on both sides of the mutation, and the
mutated build passes it. The mutation was still caught --- by a different test, which crossed
a boundary and panicked on a reversed slice range --- and that is the whole finding:

**A test's thoroughness is bounded by the constants it happens to exceed.** "Walks every
prefix" sounds exhaustive and is exhaustive over the eight-command space it was written in.
The mechanism under test switches on at 32. Nothing about the test's shape says so, and
nothing about reading it would have.

Two habits follow.

- **When a rule is gated by a constant, the test for it must be written in terms of that
  constant**, not in terms of a number that looked big enough. The replacement test loops
  `SNAPSHOT_EVERY * 2 + 5` times and asserts `snapshots() >= 2` as its control, so it goes
  red rather than vacuous if the constant is ever raised past it.
- **A mutation harness that only counts red tests would have reported this as a clean
  catch.** `scripts/mutate_rust.py` records which test each mutation is expected to turn red
  and compares the names, so it reported `1 red, but NOT the expected one` and named the
  substitute. That cross-check is what converted a passing result into a finding; without it
  the general property test would still be carrying a reputation it had not earned.

### A body's newlines live below the table that decodes it

A PDF text string with no byte-order mark is PDFDocEncoded, and `lopdf` ships that table, which
is worth using rather than transcribing: a table typed out from the specification is a second
authority that agrees with itself. Its `PDF_DOC_ENCODING` is built from **glyph names**, and no
glyph is named for a control character --- so every entry below 0x18 is `None`, and
`bytes_to_string` skips what it cannot map.

For a bookmark title that is invisible. For a **comment body** it is the content: a
two-paragraph note decodes to one paragraph, with the words intact and the shape gone. Nothing
in the round trip looks wrong, because nothing was replaced --- the newline is simply not in the
output.

`annots.rs` therefore decodes in runs: bytes at 0x18 and above go through `lopdf`, and tab and
the two newline characters are emitted here. That is the specification's own reading --- Table
D.2 gives PDFDocEncoding the same HT, LF and CR as ASCII --- so this is restoring the table's
intent rather than departing from it.

**The sentinel in `pdf_doc_encoded` is load-bearing and looks like a hack.** The table is not
exported; the way in is `lopdf::decode_text_string`, which sniffs a byte-order mark at the start
of whatever it is handed. A run begins wherever the previous control character ended, so a body
reading `a\nþÿ…` hands it a run starting `FE FF` and gets the rest read as UTF-16. An ASCII `A`
in front makes that impossible, and one character is dropped afterwards.

Two more things about that function are worth knowing before reusing it. Its UTF-16 branch is
`String::from_utf16`, which fails the **whole string** on one bad code unit --- the trap
`outline.rs` records --- and its UTF-8 branch does not strip the byte-order mark it just matched,
so the result begins with U+FEFF. Both are why `annots.rs` keeps its own branches for those two
encodings and delegates only the third.

### Testing a rule is not testing that the rule is used

`sanitize_body` keeps a comment's paragraphs where `sanitize_title` collapses them, and there is
a unit test asserting exactly that, comparing the two functions on the same input. It passes
whether or not any comment is ever routed through it.

The mutation that proves the point sets the routing condition to `false`, so every body is
flattened as a title. The suite stayed green: the test calls `sanitize_body` **directly**, and
the code that decides which flattener a body reaches has no test at all. A reader would have got
one-line comments out of a module whose tests all pass and whose doc comment explains why they
must not be one line.

The fix is a test that reads a body **out of a document** --- and a second one asserting the
mirror, that an author *is* flattened, since routing everything through `sanitize_body` would
otherwise look correct. Two tests for one `if`, which is what a conditional with two arms costs
when both matter.

**The same mutation run has a second instance of this, from the other direction.** A mutation
aimed at `decode_text_string`'s call to `pdf_doc_encoded` inside its loop also survived: the loop
flushes only when it meets a control character, the fixture body has none, and the *other* call
site --- the flush after the loop --- did all the work. Aiming inside the function caught it
immediately.

So: **when a mutation survives, ask whether it was aimed at the rule or at one route into the
rule.** A pure function with several callers is exactly where those two come apart, and the
answer is not always a new fixture --- here it was one test at a different level, and one
mutation moved four lines.

### A shortcut can produce the right answer and lose the report

`resolve_replies` breaks `/IRT` cycles so the panel can walk a thread with no visited set. A
comment replying to itself is the one-element cycle, and the first draft handled it in the
proposal step: `(*target != index).then_some(*target)`, with a comment saying the walk below
would catch it too but that naming it here kept the walk's job to chains.

Both halves of that were true and the result was wrong. The link was dropped, `reply_to` came
back `None`, every assertion about the resulting tree passed --- and `limits.cycles` stayed at
zero, so the panel told the reader the list was complete. The module's stated rule is that every
cut is counted, and the shortcut satisfied the first clause while silently failing the second.

The test that found it asserts **both**: the link is gone *and* the cut is reported. The fix was
to delete the special case and let the walk see it, which is one line shorter and reports
correctly.

Worth generalising, because the shape recurs wherever a bound and its report are separate
statements: **an early return that produces the correct value is not equivalent to the general
path, if the general path also records something.** Check what the shortcut skips, not only what
it returns.

### A square fixture cannot tell a rotation from an identity

The rotated page in `comments.pdf` first carried a note at `/Rect [20 20 44 44]` --- a
24-point square 20 points in from the corner. Under `/Rotate 90` the display mapping sends
`[left, bottom, right, top]` to `[bottom, left, top, right]`, and for that rectangle the answer
is **the input**. The probe's check passed, the unit test's check passed, and a mapping that had
been deleted entirely would have passed both.

It is the fixed point of the transform, and it is easy to write by accident: a square annotation
at a symmetric offset is the most natural thing to put in a fixture. The half-plane assertion
that ran beside it (`y is near the top of the page`) did discriminate, which is what makes this
worth writing down rather than merely fixing --- the check was sound and the *fixture* could not
support a stronger one.

The rectangle is `[20 30 44 90]` now, and the manifest states the full expected result rather
than a half-plane. A transposition --- x and y swapped --- is what that catches and the previous
pair did not.

General rule, alongside "whatever a fixture is meant to discriminate, it needs two of": **for a
geometric transform, choose a fixture that is not a fixed point of it.** Asymmetric in both axes,
and off-centre.

### A bound in the code hides everything after it in the fixture

`comments.pdf`'s hostile page carries three deliberately malformed `/Annots` entries: a
reference to an object that does not exist, an integer, and a string. It also carries 1,200
notes, to trip a per-page bound of 1,000.

The first draft appended the malformed entries **after** the crowd, because the generator's
`finish()` took them as a parameter and put them at the end. The scan reached the bound at entry
1,000 and returned, so none of the three was ever read: `unreadable` came back as 1 --- the
no-subtype annotation, which happened to be written earlier --- rather than 4. The fixture
looked as though it covered three cases it never delivered, and the count that revealed it was
in the `read` mode of the probe rather than in any assertion.

**When a fixture's consumer is bounded, the fixture's order is part of the fixture.** Anything
past the bound is decoration. It is worth asserting the count a bounded-input fixture expects
--- the manifest now names `unreadable: 4` --- because a silent 1 reads exactly like a scan
that is working.

### A panel that lists a hidden comment must not let the page open it

`/F` bit 2 marks an annotation hidden, and PDFium does not draw it. Two consumers want opposite
things from that fact, and writing one rule for both gets one of them wrong.

The **panel** lists it. Somebody wrote it, it is in the file, and a reader who opens the comments
tab to find out what was said about this document is asking for exactly that. It is drawn with a
`hidden` flag beside it, because a reader who then goes looking for the mark on the page will not
find one.

The **page** must not open it. `hitTest` skips it: there is no mark under the pointer, so a hit
would be a note attributed to a rectangle showing nothing --- and the rectangle is still in the
file, so without the rule an invisible clickable region sits over the page.

The two rules live in different functions and are tested separately, which is the point worth
recording: a single `hidden` predicate consulted in one place would have made the panel's row
disappear or the page's dead zone appear, and either would have looked like the same decision
being applied consistently.

### A `-manifest.json` sidecar enrols a fixture in a check it never claimed

`viewer_check.py` binds any `<fixture>-manifest.json` to `TPDF_READING_MANIFEST`, and
`readingChecks` then asserts that fixture's pages read in the order the manifest states. The
suffix *is* the enrolment: nothing else opts in.

`comments-corpus.json` was called `comments-manifest.json` for one commit. It is keyed by page
number, not a list of pages, so the loop over `manifest.pages` threw `{} is not iterable` --- and
an exception in a check function ends the whole run. **Sixteen checks in, and the other 155 never
ran.** The transcript's last line was a `[FAIL] run completed` with a `TypeError` in it, which
reads as a broken harness rather than as a fixture that claimed a name.

Three things worth keeping from it:

- **`src-tauri/src/lib.rs` predicted this in a doc comment** --- `geometry_manifest` exists as a
  separate variable *because* the `-manifest.json` suffix enrols a fixture, and it names
  `mixed.pdf` as the fixture that would have failed a check it was not built for. The prediction
  was right, was written down, and did not stop the second fixture walking into it. A convention
  documented at its definition is invisible from the place where a new file gets named.
- **The fix is two-sided.** The sidecar is `comments-corpus.json` now, and `readingChecks`
  refuses a manifest with no `pages` array instead of throwing --- as a red check naming the
  remedy, not a skip, because nothing here is inapplicable: a file has claimed a name meaning
  something it does not mean.
- **An exception in one check costs every check after it.** That is the argument for the
  guard even though the rename alone fixes today's instance: the next wrong-shaped file
  should cost one row, not the run.

### A rotated page makes a document mixed-size, and two checks assume it is not

`comments.pdf` carried a `/Rotate 90` page so the scan's display-space mapping had something to
be wrong about. Its four other pages are A4 upright. That is a **mixed-size document** --- a
rotated page is displayed 842 wide where its neighbours are 595 --- and `viewer_check.py`'s
rotation checks derive their expected zoom from *page 1's* aspect ratio.

Two of them went red: `the page is laid out sideways` wanted 0.5371 and measured 0.7601
unchanged, and `four quarter turns come back to where they started` reported the document's
length changing from 2377 to 3626. The viewer was behaving as designed. The zoom entering the
check was fit-width **of the rotated page**, because `applyFit` runs on a resize or on newly
learned geometry and not when the reader moves between pages of different shapes --- the known
mixed-size gap `thumbnails.ts` and `docs/PLAN.md` §4 both record.

**The bisect is the part to copy.** Disabling the eight new comment checks and re-running
produced the same two failures, which is what separated "my checks left bad state" from "this
fixture meets a documented gap" in one build. Guessing between those two would have been a
coin flip: both stories fit every symptom.

The rotated page lives in `comments-rotated.pdf` now, which is what `make_rotated_pdf.py`
already does for the same reason. The mapping it tests is a property of the *scan*, which
`comments-probe` reads directly; the window harness never looks at it.

### A new corpus has to satisfy the sample points every existing check hardcodes

Three of `viewer_check.py`'s checks drag or click at fixed screen positions --- `MID_X`,
`HIGH_Y`, `LOW_Y` --- because tying a *screen position* to *specific content* is the only way
they can see a mapping applied backwards. A new fixture is therefore not free to lay its text
out however it likes, and `comments.pdf` broke two of them by being ordinary:

- Its lines read `Ordinary line 0`, so the double-click at `(MID_X, HIGH_Y)` landed on a
  **single digit**. `a double-click selects a word rather than a character` reported one
  character, which is what a viewer with no notion of granularity would produce.
- It had ten lines with 24-point leading, so `LOW_Y` fell past the last of them and the drag
  caught a stray `g`. `whole.indexOf("g")` then found the first `g` on the page --- in `golf`,
  four lines from the top --- and the check reported *the page reads bottom to top*. **The
  verdict was invented from a position that meant nothing.**

The fixture now uses six-letter words and 36 lines at 18-point leading, which is a statement
about the harness rather than about the corpus and belongs in the generator's own comments ---
where it now is.

The harness gained a precondition for the second one: a drag selecting fewer than three
characters cannot be located and is skipped with that reason. It already had one for *no* text
at a height, added when `multilingual.pdf` hit the same edge --- so this is the same lesson
arriving at the same check from one step further in. **A selection short enough to be
ambiguous is as unusable as an empty one, and `indexOf` will not say so.**

### An empty transcript is what a *running* viewer check looks like

`viewer_check.py` launches the app with its stdout on a pipe and calls `communicate()`, then
prints the transcript when the process exits. So a redirected run --- which is how these are
always run --- writes **zero bytes** for its whole duration. On `vector-multi` that is several
minutes of an empty file.

`BUILD.md` said "results print as they are produced, so a run that stops partway names the last
check it completed", which is true of `viewercheck.ts` writing into the pipe and is what makes a
*timeout's* partial transcript useful. Read as a promise about the log file it is false, and it
cost two wrong diagnoses in one session: a live A0 render was called a hang, and a genuinely
occluded run was diagnosed correctly for the wrong reason.

**CPU time is the liveness signal.** `ps -o time= -p <pid>` on the app: a page that never
executed accumulates none at all, and a slow render accumulates seconds while sitting at low
percentages. That is exactly what the harness's own `diagnose_silence` samples, and it is
available to a human at any moment without waiting for the run to end.

The related failure it hides is real and separate: **a leftover tpdf window occludes the next
one**, WebKit suspends an occluded page, and the run then produces nothing and uses no CPU. Two
runs died that way here before `pkill -f "tpdf.app/Contents/MacOS/tpdf"` between runs went into
the sweep script. `BUILD.md` already prescribes that for `open_check.py`; it applies to every
harness that opens a window, and the sweep is where it matters most because each run leaves one
behind for the next.

### A margin above a destination lands on the previous page, and the tolerance that compensates for it can only reach within a page

Jumping to an outline entry leaves `DESTINATION_MARGIN_PT` --- 6 pt --- of air above it, so the
heading does not read as cut off against the top edge. `REACHED_TOLERANCE_PT` = 8 exists to
compensate in the *highlight*, and its docstring says exactly what it is for: "without this,
clicking an outline entry highlights the entry **before** it". The pair is asserted to stay
ordered, in `outline.test.ts` rather than in a comment.

All of that was correct and none of it reached the case where the destination is the **top of
the page**. Subtracting 6 pt from an offset of zero scrolls into the *previous page*;
`position` then reports that page, and `currentId` drops every row whose page is past the
reader --- `if (row.target.page > page) continue` --- **before** the tolerance is consulted. So
the tolerance could only ever rescue an entry on the same page as the reader, which is not the
case it was written for.

It is not an edge case. `/Fit` and `/FitB` name no coordinate at all, which `goToDestination`
reads as the page's top; `/XYZ x 0 z` is an explicit zero; and on a **rotated view every**
destination has offset zero by construction, because the destination's axis is not the one
being scrolled. Any heading within 6 pt of the page top is in the same position.

Caught by `viewer_check.py` on `links.pdf`, and by that fixture alone --- `outline-simple` and
`outline-hostile`, the two corpora whose whole purpose is outlines, both passed. The
discriminator is checked rather than inferred: `grep` over `testdata/*.py` finds `/Fit` in
`make_links_pdf.py` and **nowhere else**, so no other fixture has an outline entry that names
no coordinate, and every other entry's `y` is far enough down its page that the margin stays
inside it. The corpora built to exercise outlines could not reach the case, and the corpus
built to exercise *links* did, because its outline exists to be compared against them.

Its outline is also deliberately **not in page order**, which is what made the wrong answer
legible: the entry chosen was a visibly different one, `"" -> "3", wanted "1"`, rather than the
neighbour. That is a fixture being easy to read from, not the thing that caught it --- a
monotonic outline with a `/Fit` entry would have failed the same check with a less obvious
number.

The fix is one clamp --- `Math.max(0, offset - DESTINATION_MARGIN_PT)` --- and the reasoning is
that the margin reveals what is above the heading, so when there is nothing above it the margin
has nothing to do. Three unit tests, each red under the unclamped form, and a control at offset
200 that stays green so the tests cannot be satisfied by deleting the margin outright.

### A guard asking how long the document is cannot answer how far the jump went

"A jump discards what it leaves behind" presses End and requires the tiles from the previous
screen to be gone one frame later. It only means something when the jump left the screen
behind, so it is guarded --- and the guard read `viewer.maxOffset > HEIGHT`, the document being
longer than the window.

Those are the same quantity only from a standing start, and the check does not start from one:
a wheel notch 400 px down runs immediately before it. On `links-rotated.pdf`, `maxOffset` is
750 against a 700 px window, so the guard passes, End travels **350 px**, the tiles on screen
stay valid, and the check reports `sharp=100.0%` --- a failure printed against a viewer that
discarded exactly what it should have.

Every other corpus is long enough that the two quantities agree, which is why a guard testing
the wrong one had been green for as long as it had existed. **Measure the quantity the
assertion depends on**, which here is the travel: `viewer.offset - leftFrom`. The skip line now
prints it, so a fixture that lands near the boundary says so rather than looking like a
document that was simply short.

### A probe fixture swept as a corpus, against the file that already said not to

`links-rotated.pdf` went into a `viewer_check.py` sweep and produced eight red checks. Three
were chased before the cause was found, and none of the eight was a defect in the viewer: two
are the documented mixed-size rotation gap, one is a two-page document retaining every page it
has, and the rest are a last page that cannot reach the top of the viewport.

`BUILD.md` already said so, in the fixture's own paragraph: the rotated page is *"a **separate
file** --- `links-rotated.pdf` --- because a document that mixes page sizes reddens two of
`viewer_check.py`'s rotation checks"*, and names `comments-rotated.pdf` as the same split for
the same reason. The note was written when the fixture was, and read by nobody at the moment it
would have helped.

**The list of corpora had no home.** It lived in whatever shell loop somebody typed that day, so
there was no artifact to be wrong, no diff to review, and nothing that could refuse. That is the
same defect the repository had already fixed twice --- once for CI fixtures
(`scripts/ci_fixtures.py`, after a release workflow lost a whole step) and once for the trap
index (`scripts/check_trap_index.py`, after three entries went missing from a list nobody
counted).

`scripts/viewer_sweep.py` is the list, and it is a gate. Every `testdata/*.pdf` must be either a
window corpus with a stated purpose or excluded with a stated reason; a fixture matching neither
is an error, a corpus named here and absent from disk is an error, and an exclusion pattern
matching nothing is a warning. All three proved by mutation before the gate was trusted --- and
the third mutation had to be redone, because renaming an exclusion pattern *orphans* the fixture
it covered, so the unclassified error fires first and the warning path never runs. A control
that trips an earlier check has not tested the one it was aimed at.

It also asserts the invariant `BUILD.md` could previously only state in prose: **every corpus
reports the same check names**, diffed as sets and printed as a difference. A check that stops
being printed and a check that starts skipping are indistinguishable in a total, and the totals
are what a person compares.

### The tool written to catch a missing check reported agreement about the wrong set

`scripts/viewer_sweep.py` exists to assert that every corpus prints the **same check names**,
because a check that quietly stops existing and a check that starts skipping are the same
number in a total. Its first version recovered those names by parsing the transcript, splitting
each result line on two-or-more spaces --- the padded column the names are printed in.

`Report` writes `LABEL + name.padEnd(46) + " " + detail`, and `padEnd` does not truncate. A name
longer than 46 characters is therefore followed by **one** space, indistinguishable from the
single spaces inside the name itself, and there are plenty: *"the keyboard reaches a link, and
draws a ring on it"* is 50. The split matched 175 lines of 189, truncated some of what it did
match, and the truncations collided --- so the run reported

    [OK]   all 2 corpora report the same 137 check names

beside `162 ran, 27 skipped`, which is 189. Two numbers on adjacent lines of the same output,
disagreeing by 52, and the verdict was `[OK]`.

**The tell was free and nearly missed.** The summary counts and the parsed count are both
printed; nothing compared them. That comparison is now an assertion --- `len(names) == ran +
skipped` --- and it is the cheapest check in the file.

The fix is not a better regular expression. **The run knows its own names**, so `Report.finish`
prints them as `CHECK-NAMES-JSON [...]` and the sweep reads that, refusing outright when the
line is absent rather than falling back on the guess. A bundle predating the marker is an old
bundle, and that refusal was proved against a real one before it was trusted: the build on disk
at the time had not been rebuilt, and the sweep said so and stopped.

Three tests pin the marker in `checkreport.test.ts`, each proved by mutating `finish` --- not
emitting the roll at all, emitting only the checks that ran, and emitting it after the summary
--- and the file that holds them already existed for exactly this reason, with a docstring
warning that the padded column must not be parsed. **The warning was written, the harnesses had
already been rewritten to obey it, and the new tool did it anyway.**

### A control that cannot discriminate is not a failure, and calling it one made a documented command red

`text-probe` asserts that character boxes land on ink, and guards it with four controls: the
un-flipped convention and the three `/Rotate` turns the page is not displayed at. Each must stay
under 50%, because a control that also lands on ink has not caught anything.

On `links.pdf` and `links-cropped.pdf` two of the four reach **68--87%**, against 0--5% on the
four fixtures written for this probe. Nothing is wrong with the mapping: `make_links_pdf.py`
writes 36 rows of even text, and **a dense page of uniform lines cannot detect a y-flip** ---
which is a trap this repository had already recorded, arriving from the other side. The probe
even detected it and printed the explanation in full: *"this page cannot tell them apart ... or
this check is proving nothing."*

And then reported it as `[FAIL]` and returned it in the verdict, so the run exited 1. `BUILD.md`
prescribed exactly that command against exactly that fixture and quoted only its passing line,
so the documented way to verify the crop-box fix was a red run whose redness was about the
choice of document.

**The distinction the verdict was missing is between a check that failed and a check that could
not fire.** They are reported as `[SKIP]` now, excluded from the exit code, and followed by a
`[NOTE]` saying how many controls could not discriminate and what therefore remains proved ---
placement, not orientation. The skip is driven by the measured rate, not by the fixture's name:
`text-base14.pdf` still reports `[OK]` on all four, which is the control over the change itself.

**What made this worth doing rather than documenting around**: the probe *is* the coverage for
`text.rs`'s half of the crop-box fix, which needs a live PDFium page and so has no unit test.
Proved by mutation rather than assumed --- removing the origin shift takes `links-cropped.pdf`
from 96.4% to 74.8%, red against the 95% threshold, while `text-base14.pdf` stays at 100%. Note
how narrow that is: a 50 pt inset moves each box by less than a line's height, so most still
overlap some ink and it is the threshold rather than a collapse to zero that catches it.

### Three crop-box mutations in one module and one in its twin, for code written twice

`links.rs` and `annots.rs` compute page geometry independently --- deliberately, since a shared
helper would make one mutation blind both suites. The crop-box work gave `links.rs` three tests
and three mutations, and `annots.rs` **one of each**. The asymmetry was in
`scripts/mutate_rust.py`'s own `--list` output the whole time, sorted together, two lines apart.

What that cost: `annots.rs`'s intersection clamp --- the `max`/`min` against the media box that
stops an oversized `/CropBox` scaling every rectangle against a page the renderer never uses ---
was reachable by **no test at all**, in the module that places a comment's rectangle. Its origin
half was covered by a test but by no mutation, so nothing had shown that test could fail.

The lesson is not "write more tests". It is that **twin modules need their coverage compared as
a pair**, because each suite is individually plausible: one test for a crop box looks like
coverage until you notice the other module has three for the same rule. `--list` grouped by
module is the cheapest place to see it, and it had been printing the evidence for as long as the
asymmetry existed.

### A guard written inline with an FFI call is reachable by nothing

`RawPage::origin_pt` calls `FPDFPage_GetCropBox` and then applies two rules to what comes back:
normalise the box, because a producer may write either corner first, and refuse a non-finite
value, because one `NaN` here becomes a whole page of `NaN` boxes --- and a `NaN` comparison
fails silently rather than loudly, so it reads as an empty text layer rather than as a bug.

Both rules are ordinary arithmetic and neither could be tested, because reaching them needed a
live page, which needs a document and a loaded PDFium. The guards sat in the one function every
character, link and comment position is measured from, and nothing in the repository could make
either of them go red.

**The fix is a seam, not a harness.** The decision moved into `corner_of(ok, box_pt)`, a free
function over four floats, and `origin_pt` became the FFI call plus one line. Three tests, three
mutations, each proved to fire --- including the control that matters most, an *ordinary* box in
the non-finite test, since a guard written as an unconditional `return (0.0, 0.0)` satisfies
every assertion about refusal.

Worth reaching for whenever a rule is entangled with a call that cannot be made under test:
`docs/TRAPS.md` already carries *"an unreachable guard is worth keeping if the type can carry it
instead"*, and this is the same move made with a function instead of a type.

### A rule about names, enforced by the one harness that discovers it last

`mutate_viewer.py` decides a mutation was caught with `line.startswith(expect)` over the checks
that went red, and refuses an expectation matching more than one. So **a check name that is a
prefix of another cannot be aimed at** --- a rule this file already records, from the day
`search_probe.rs` broke it with `query astral-alone` sitting beside `query astral-alone: indices
address the hit`.

The refusal is correct and arrives at the worst moment: when somebody writes a mutation for that
check, which may be months after the name was added, and which reads as a problem with the
mutation. Nothing checked the *names*. The rule was written down and enforced by nothing, which
is the failure mode this repository has now recorded from four directions.

Three families are matched this way. All three were measured and all three are clean --- 189
viewer check names, 75 and 30 from `search-probe`, 11 from `structure-probe`, no name a prefix
of any other --- and the check now runs on every sweep and every probe run rather than on
somebody's initiative.

**The first measurement of it was wrong in the way this file keeps recording.** The probes print
`LABEL name:<52 detail` and `:<52` pads without truncating, so a longer name runs into its detail
with one space between. Splitting on runs of spaces silently dropped **15 of 75** names from one
probe and **15 of 30** from another, and reported a clean result over the remainder. The verdict
happened to be right; the population was 80% and 50% of the real one. Both probes emit a
`CHECK-NAMES-JSON` roll now, for the same reason `checkreport.ts` does.

**And the first control aimed at the new check was aimed at nothing.** Shortening a name to
`page 1: the order` makes it a prefix of no other name, so the mutation survived and proved
only that the plant was wrong --- *"a mutation that survives may be a variant, not a gap"*.
Re-aimed so one name became a genuine prefix of another, it goes red on both pages, names the
name each one shadows, and exits 1.

**One duplication left deliberately.** Seven probes carry their own `Report` struct, by
convention rather than accident, and the roll now lives in `src/probes/checkroll.rs` reached by
`#[path]` from the two that need it. Extending it to the other five is worth doing when one of
them next needs its names read; doing it now would be five edits for no current question.

### A mechanical insert before a declaration can land between an attribute and its item

`#[cfg(windows)]` and the `pub mod sandbox_win;` under it are one thing. A `sed` that
inserted a new `pub mod save;` *before* the module declaration put it between the two, so the
attribute now gated `save` --- which exists on every platform --- and `sandbox_win`, which is
Win32 only, became unconditional. On macOS the result compiles until something reaches into
either one, and the diagnosis then points at the wrong module entirely.

Nothing about this is specific to `cfg`. A doc comment, a `#[derive]`, a `#[test]`, an
`#[allow]` and a decorator all bind to the next item, and an insert "before the declaration"
is an insert *into* whatever is attached to it. The general form: **a mechanical edit anchored
on a line is anchored on a line, and a declaration in Rust is rarely one line.**

Caught by reading the result rather than by the compiler, which is the part worth noticing ---
the tree still built. Read the four lines around a mechanical insert, or anchor the edit on
something that cannot have an attribute in front of it.

### A size-driven invalidation cannot see a half turn

`Scroller.applySizes` invalidates a page whose box dimensions changed and merely re-places one
that only slid. That is exactly right for a size correction: a page whose box did not move
still holds the right pixels.

It is exactly wrong for a rotation. A half turn leaves the box identical --- same width, same
height, same position --- and the picture inside it upside down. So `setPageTurns` invalidates
the page **before** the geometry is consulted at all, and the quarter turns, where the box does
change, are the cases that would have hidden this: three of the four turns work under the
size-driven rule and the fourth is silently wrong.

The test needs its tiles to have **landed** first, which is its whole precondition. A request
still in flight is not re-issued while it is outstanding, so a version that turned the page
mid-flight sees no new request and reads as a defect in the invalidation rather than in the
fixture --- see the entry below.

### A request still in flight is not re-issued, so a mid-flight invalidation looks broken

`Scroller.request` returns early for a tile id already in `inFlight`, and invalidating a page
does not remove those entries: the reply arrives, is discarded for a stale epoch, and *then*
the next frame asks again. Correct, and it means a test that invalidates while requests are
outstanding observes no new request at all.

The first draft of the half-turn test above did exactly that --- `fetchTile` returned a promise
that never resolved, which is the right fixture for testing withdrawal and the wrong one for
testing invalidation --- and its failure (`expected 0 to be greater than 0`) reads as a page
that was not invalidated. Resolve the tiles, run a frame, and assert some landed before
touching the thing under test.

### Every statement about a turned page is also true of a rotated view

Rotating one page of the document and rotating the whole view produce the same evidence about
the page in front of the reader: it is the turned shape, its tiles were discarded, its text
runs sideways, its fit was recomputed. A `setPageTurns` implemented as `rotateBy` passes every
one of those assertions.

What separates them is what did **not** happen. A page nobody touched keeps its shape and its
text stays upright, and `viewer.rotation` does not move. Those three are the checks with a
failing case; the rest are decoration on their own.

The corollary for fixtures: this needs a document with **at least two pages**, and a first page
that is not square. On a one-page document every assertion here holds whatever the code does,
and on a square page a quarter turn is invisible in the shape --- both are skips with a stated
reason rather than checks that cannot fail.

### An exclusion keyed on a prefix grows on its own

`appcommands.test.ts` sweeps every registered command and asserts each reaches an action,
excluding the ones that reach the viewer instead --- by prefix: `view.`, `nav.`, `edit.`.

That was right on the day it was written, when every `edit.` command was about the selection.
The page operations landed under the same prefix a fortnight later, reaching the *shell*, and
four commands left the sweep without anyone deciding they should. The list did not change; what
it covered did.

An exclusion list should name what it excludes. The three selection commands are now written
out in full, so the next `edit.` command is swept by default and leaving it out is an edit
somebody has to make. Same shape as the allowlist rules elsewhere in this file: an entry that
can match something nobody has written yet is a blanket permission wearing an allowlist's
clothes.

### `instanceof` against a constructor the runner does not have throws, it does not answer no

The window-key handler has to know whether a keystroke went to a text field, and the
conventional spelling is `target instanceof HTMLElement`. The guess about what that does under
vitest was wrong in the reassuring direction, so it was measured:

    typeof globalThis.HTMLElement          -> undefined
    ({tagName: "INPUT"}) instanceof HTMLElement
      -> TypeError: Right-hand side of 'instanceof' is not an object

It does not quietly report "not a text field" --- it takes the handler down. The practical
consequence is not a wrong answer, it is that the guard could only be tested by standing up a
DOM, which no other test in this file needs. Duck-typing the target (`tagName`,
`isContentEditable`) costs three field reads, answers identically in the webview, and is
exercised by the same plain-object events every other window-key test uses.

Worth recording mostly for the method: the first version of this paragraph asserted the silent
answer, in a code comment, without running anything. Two lines in a scratch test settled it.

### Writing a page's rotation "for completeness" flattens what a bounded walk could not read

`/Rotate` is inheritable: a page with no `/Rotate` of its own takes its parent's. So a save
that writes every page's rotation, turned or not, does not leave the untouched pages alone ---
it replaces inheritance with a stated value, and the stated value is whatever the walk
returned. `effective_rotation` is bounded at 64 `/Parent` hops and answers **0** when it gives
up or meets a cycle, so on a document with a deeper chain that write silently *flattens* the
rotation of pages nobody asked to change. `save.rs` therefore skips a page whose turn is zero.

**The first version of this entry got the mechanism wrong, and the wrong version is the one
that sounds right.** It said writing `/Rotate 0` overrides an inherited rotation --- true as a
sentence about PDF, and not what this code would do: for an untouched page the composed value
is `effective_rotation + 0`, which is the inherited value, so writing it changes nothing at
all in the ordinary case. The guard is worth having for the bounded-walk case above and for
keeping an unedited page byte-identical, not for the reason first written down.

**And the test could not fail, for a reason the entry itself explains.** It asserted
`effective_rotation` on the untouched page, which answers 90 whether the page states it or
inherits it --- so the mutation that writes to every page left every number unchanged and
survived. The assertion that works is the **absence of the `/Rotate` key**, with the turned
page asserted to *have* one as the control, or "no key" is also satisfied by a save that
writes nothing. Every fixture in the corpus states its rotation on the page, so none of them
can see any of this; the fixture is a hand-built two-page document whose `/Pages` node carries
`/Rotate 90`.

### Two page numbers can be one page object, and the second turn composes on the first

`lopdf`'s page walk keeps **no visited set**. `PageTreeIter` (`document.rs`) descends `/Kids`
and yields every `/Type /Page` reference it meets; it bounds depth at 256 and total steps at
the object count, and it never asks whether it has returned this object before. `get_pages()`
is `page_iter().enumerate()`, so a `/Kids` array that names one page twice produces **two page
numbers mapping to one `ObjectId`**.

Any code that says *"for each page, do X to its object"* is then wrong on such a document,
because the second visit sees what the first visit did. Nothing crashes and nothing is
refused --- a file is produced, and it is wrong.

**Three sites, and two of them predate the feature that exposed the shape.**

- `print.rs`'s rotation loop composes onto `effective_rotation`, so the second visit reads the
  value the first wrote: one quarter-turn asked for on each of two pages comes out as 180 on
  both. Wrong since printing landed.
- `print.rs`'s `drop_pages` is the damaging one. `doomed` is built from the *dropped* page
  numbers with no regard for whether a kept number names the same object, so printing "page 1
  only" of such a document deletes the object page 1 is, and prints nothing. Its `/Count`
  arithmetic has the same cause from the other side: it decrements once per doomed *object*
  where the tree counts *page numbers*.
- `save.rs` inherited the rotation shape from the first of those.

**The fixes are not the same, and making them uniform is the mistake to avoid.** Print's
rotation applies one turn to *every* page, so two numbers reaching one object cannot disagree
--- deduplicating is exactly right, and the object turns once. `drop_pages` must instead
*subtract* the kept pages' objects from `doomed`, because the question there is not "how many
times" but "may this be deleted at all". Save takes one turn *per page*, so they can disagree,
and then no output satisfies the request: page 3 cannot be at 90 and page 7 at 180 when they
are the same object. It groups by object, refuses only a genuine conflict, and otherwise
applies the agreed turn once. A blanket refusal was the obvious move and is wrong for the case
that dominates --- a document nobody edited, where every turn is zero and nothing conflicts.

**The test asserts the precondition, not only the outcome.** A guard against something a
future `lopdf` might deduplicate is a guard reachable by nothing, and the outcome assertion
would keep passing while it became decoration. So the fixture test asserts that `get_pages()
` really does return the same id under two page numbers, and says in its own message that this
is where a change in that library shows up. Found by reading the loop and then reading
`lopdf`'s iterator, not by a failing test --- no fixture in the corpus is malformed this way,
which is exactly why it survived review twice.

### Fit-width rescales every page when one of them becomes the widest

A check written to prove that turning page 1 does not turn page 2 compared page 2's rendered
box before and after, with a one-pixel tolerance. On `text-heavy` it went 640x828 to 495x640
and the check called it a defect. It is not one: fit-width sizes the layout to the widest page
in the document, so turning page 1 to landscape makes *it* the widest and every other page is
legitimately rescaled --- here by 22%, with the aspect ratio identical to three decimals.

**The observable was wrong, not the code.** What separates "page 2 was turned" from "everything
was rescaled" is the **ratio**: a turned page reports the reciprocal, and no rescale can produce
that. Comparing proportions still catches the defect the check exists for --- a `setPageTurns`
that called `rotateBy` --- while being blind to a size change that is correct behaviour.

**It was written and watched pass on one corpus.** The first sweep across all fourteen found
the one where a legitimate fit change moves the quantity being asserted. A check whose
observable is disturbed by something other than its subject is a false positive waiting for the
right document, and the corpus that has it is not usually the one you develop against.

### A sweep that names one cause for a symptom several produce sends you to rebuild what is current

`viewer_sweep.py` refuses a run that printed no `CHECK-NAMES-JSON` roll, which is right ---
recovering the names by guesswork is how its first version reported agreement about a set that
was wrong. What it also did was state the reason: *"The bundle predates it --- rebuild with
`npm run tauri build`."* No hedge, no evidence.

The first time it fired, the bundle was five minutes old. The run had died before reaching the
roll for an unrelated reason, and a freshly built app was rebuilt again on the strength of a
sentence. A stale bundle, a crash, an expired `--timeout`, and a window that never became
visible all produce exactly this silence, and the tool has the evidence to tell them apart: the
exit code, how many bytes came back, whether a summary line appeared, and the last lines of the
run. It reports those now and lists the candidates rather than asserting one.

Same family as the gate whose static reason turned a crash into a wrong diagnosis. The rule
that generalises: a refusal may say what it *observed* with certainty and what it *concludes*
only with a hedge --- and when one symptom has several causes, naming a single one is not
helpfulness, it is a wrong answer delivered confidently.

### A mutation aimed at deleted code is refused far too late to matter

`mutate_viewer.py` refuses a mutation whose search string is not in the file, prints which one,
and stops. That is right, and it is not a safeguard: the refusal arrives inside a run of the
harness itself, and if nobody completes one the table can hold a dead mutation indefinitely
while looking complete. `--list` prints it exactly as it prints a live one.

Two were found on 2026-08-16 and the pair is the argument. One had been dead for **weeks**:
commit `9e9be98` removed the `a11y.ts` line it named when links began to be announced as links,
and nothing said so, because the harness that would have said so had not finished a run in that
time --- it was hanging, so one defect was concealing the other. The second took **an hour**: an
ordinary `*id` -> `id` cleanup in `save.rs`, made while fixing something else, silently unaimed a
mutation that had passed earlier the same session. Neither is exotic, and normal refactoring
produces the second kind continuously.

So the check belongs where it costs nothing and runs every time, not inside the hour-long thing
it is about. `anchors` is that gate. It reports the count and refuses to guess *why* an anchor
is missing: a drifted anchor and a leftover mutation look identical to it and need opposite
fixes, and a check that picked one would be confidently wrong half the time.

**Proving it needed the trap it is named for.** The first control aimed at the drifted-anchor
direction perturbed a string that is not in the table, so `str.replace` changed nothing, the
gate correctly reported a clean tree, and that read as the gate failing to fire. Assert the
plant landed --- compare the text, not just call `replace` --- before reading any mutation's
result.

### A `pgrep -f` wait loop is defeated by the command that checks on it

A script that waits for another job with

    while pgrep -f 'mutate_rust.py' >/dev/null; do sleep 10; done

matches **any** process whose command line contains that string --- including the shell running
the command you typed to see whether the job had finished. So on 2026-08-16 the harness it was
waiting for exited at 22:00, and the waiter was still waiting an hour and twenty minutes later,
held open by the diagnostics. *Observing it is what kept it blocked*, which is why the state
looked consistent every single time it was checked.

`pgrep -f` searches the full argv of everything on the machine, so a pattern this specific
feels safe and is the opposite: the more distinctive the string, the more certain it is to
appear in the very command written to look for it. Two escapes, and the second is better ---
match on something the observer cannot contain (a PID recorded when the job started, or `pgrep
-f "[m]utate_rust"`), or **do not wait on a process at all**: have the job write a sentinel and
wait for that.

**The same run had a second reason to look dead, and either alone was enough.** Its steps were
piped through `tail` inside a script whose stdout was a file, so nothing reached the log until
the pipeline closed --- and a script that has printed nothing is indistinguishable from one
that has done nothing. Both halves are already recorded here separately, in the entries about
running a long command through `tail` and about a harness that prints only at the end. They
were written into one script anyway.

The general shape, which is what makes it worth a fourth entry: **an instrument that shares a
namespace with its subject can hold the subject in the state it is measuring.** Prefer a
positive signal the job emits (`WIN2-DONE`) over an inference from the process table.

### A page number is a position, and deleting a page renumbers every one after it

`lopdf`'s `get_pages` numbers the pages it finds from 1 as it walks the tree, so a page number
is an *index into the current document* rather than a name. Delete page 2 of four and the old
page 4 is now page 3; look a plan's entry up in a table read after the deletion and the number
either names a different page or names none at all.

Both happened in the same function on 2026-08-17, when the print path learned to carry a
per-page turn. The code read `doc.get_pages()` *after* `drop_pages`, and a job that kept pages
1 and 4 of `rotated.pdf` came back with page 1 turned and page 4 untouched --- the lookup for
number 4 found nothing, `filter_map` dropped it silently, and the result was a plausible
document with one page at the wrong angle.

**Object ids do not move.** The fix is one line of ordering: resolve every plan entry to an
`ObjectId` *before* anything is dropped, and hand those to the code that writes. `save.rs` does
the same thing for the same reason, and its comment says so at the point where the two could
drift apart again.

What caught it was an existing test, `a_third_parser_checks_a_job_built_from_a_document_we_did_not_write`,
which keeps the **first and last** pages of a four-page fixture whose pages carry 0/90/180/270.
A range of adjacent pages would not have shown it, and neither would a fixture whose pages all
carry the same rotation: it needs a kept page whose number *changes*, and a way to tell that
page apart from its neighbours afterwards.

### Removing one of two page numbers that name one page cannot be done by removing objects

A `/Kids` array may name the same page object twice, and then two page numbers are one page.
`pagetree::drop_pages` works in objects and correctly keeps any object a *surviving* number
names --- which is the guard that stops "print page 1 only" deleting the page it was asked
for. The consequence on the save path is the opposite failure and it is not obvious: "delete
page 2" of such a document computes a doomed set of one object, finds page 1 still names it,
keeps it, and writes a copy **with the page the reader deleted still in it**.

Found by writing the test that expected the deletion to work. Nothing was wrong with
`drop_pages`; the request is one no output of that mechanism satisfies, because removing the
page means removing one *entry* from a `/Kids` array rather than one object from the file.

`save.rs` refuses it by name --- "pages 1 and 2 are the same page in this file, so page 2
cannot be removed on its own. Remove both, or keep both." --- with a control asserting that
removing **both** numbers is still accepted, since a blanket refusal of every shared page
would have passed the first test while denying the case that works.

### Dropping a reference out of a destination array leaves a destination with no page

`drop_pages` removes every reference to a doomed object in one pass over the graph: an array
entry naming it is dropped, and a dictionary key whose value names it is removed. That is
right for a `/Kids` array and for an `/Annots` array, and it is subtly wrong for a
**destination**, which is an array whose *first element is the page*: `[5 0 R /XYZ 0 792 0]`
becomes `[/XYZ 0 792 0]`, which is not a broken destination but a malformed one.

So a document that loses pages loses its outline whole, in the print path and now in the save
path, rather than keeping entries whose destinations have been quietly hollowed out. It is a
real loss --- delete one page of a 500-page manual and the bookmarks go --- and it is the only
option that cannot write a structure no reader can parse.

Repairing it instead is a piece of work rather than a flag: a destination is reachable as a
direct `/Dest`, as a `/Dest` inside an `/A` action, as a name into `/Dests` or into the
`/Names` tree, and each has to be resolved, tested against the pages that are going, and then
either rewritten or removed together with the entry that held it. That is `links.rs`'s
resolver, on the write side.

### State keyed by a slot belongs to whatever moves into that slot

Deleting a page is the first edit that makes the viewer's slots and the file's pages different
numbers, and the interesting part is not the translation --- it is everything that was
*already* keyed by a slot and quietly changes meaning: the scroller's learned page sizes, its
tile epochs, the tiles themselves, the tier-1 placeholders, the page strip's thumbnails, the
accessibility tree's built pages, a search's matches, the selection, the focused link, the open
note.

Each one gets one of three answers, and which is right depends on what the state is *about*.
Two of them are cheap to get right:

- **Carry it with the page**, by identity. A learned size and a page's own turn belong to the
  page and must travel to wherever it went --- `Scroller.setPages` re-indexes both through a
  map from page id to old slot. Carried by slot instead, every page below the gap is laid out
  at the size of the page that used to be there, which is invisible on a document whose pages
  are all the same size. That is most of them, which is why the test that pins it uses three
  pages of three different heights rather than a corpus.

  A tile epoch is carried the same way and deliberately **not** bumped, which took a mutation
  to establish: `clearTiles` bumps the generation in the same call, one mechanism drops every
  outstanding reply, and a per-page bump beside it changed nothing any test could see.
- **Throw it away.** Tiles and thumbnails are placed by the slot they were rendered for, so
  after a deletion the surviving pixels are in the wrong places rather than merely stale. A
  search's matches name slots that now hold other pages. Keeping either is the plausible wrong
  answer; dropping them costs a re-render the reader is already expecting.

The third --- leave it alone and translate on the way out --- is the one to be careful of. It
is right for exactly one thing on the list, the text cache, because a page's text belongs to
the page of the file and the cache is keyed by that; applied to anything else it produces
state that is quietly about a different page.

### A wait built on `pgrep -f` outlives the job, and every later check agrees with it

`docs/TRAPS.md` already records that `pgrep -f` matches the command written to look for a job,
so `until ! pgrep -f mutate_rust.py; do sleep 60; done` holds itself open forever. The second
half, found on 2026-08-17: **once one such waiter exists, every later `pgrep -f` check reports
the job as running whether it is or not** --- the waiters match each other as well as
themselves.

Three of them were started while a mutation run was going, each a fresh "is it done yet". Every
`pgrep -f mutate_rust >/dev/null && echo RUNNING` after the first one printed `RUNNING`, and
would have printed it just as steadily if the harness had died in its first minute.

**What that costs is not the wait, it is that a wrong belief cannot be corrected.** The run was
believed to be two hours old and was six minutes old --- the log was silent (a harness that
prints per mutation writes nothing to a redirected file until it exits, recorded here
separately), so the only two instruments were an elapsed-time guess and a liveness check that
always said yes. `ps -o lstart=` settled it in one call: the process had started at 08:53 and
the clock read 08:59.

Two habits, and the second is the one to keep: `pgrep -f "Python scripts/mutate_rust.py"` names
the *process* rather than the file it runs, and better, **wait on a signal the job emits** ---
append `exit=$?` to the log and `until grep -q "^exit=" log; do sleep 60; done`, which nothing
but the job finishing can satisfy.

### A cross-check that counts names against a count of tests is wrong wherever two tests share a name

`mutate_frontend.py` reads vitest's output twice and requires the two readings to agree: the
per-test failing lines, and the `Tests  N failed` summary. That is the right shape --- if one
of the two stops describing the run, the verdict beside it is worthless --- and it was
comparing the wrong quantity. The failing lines went into a **set** of names, and the summary
counts test *instances*, so the check held only while every failing name was unique.

Thirteen names in `src/lib/*.test.ts` are defined in more than one file --- `closes on Escape`,
`is one tab stop`, `normalises a negative turn`, `says nothing when nothing was cut` and nine
others. So any mutation reddening two of them was condemned:

    [FAIL] viewer: hit-test the page before letting the box tool have the press:
           36 failing test lines but the summary says 37 -- this harness cannot read its own output

Three of 412 on 2026-08-23, each off by exactly one, and every one of them a mutation the
suite had caught perfectly. The verdict is the harness accusing itself, which is the reading
least likely to be doubted --- and the least likely to be chased, because the natural next
step is to go looking for a broken reporter rather than at the suite.

**The same run already knew.** `all_test_names` returns `dict[str, list[str]]` and its
docstring says why in as many words: `says nothing when nothing was cut` is in two files, so a
`dict[str, str]` aimed the narrow run at the wrong one. That was learned on 2026-08-21 at the
*listing* end of the run and never carried to the *failure* end, twenty lines away. A fact
established about a data source is a fact about every reader of it.

Fixed by returning the line count beside the set --- `len(lines)` for the cross-check, `set(lines)`
for the name match --- so the check now compares instances with instances. Proved in both
directions: the three mutations go from `[FAIL]` to `37 red`, `8 red` and `2 red` in
`viewerdraw.test.ts`, and planting `[1:]` on the line list brings the failure straight back,
which is what says the cross-check is still a check rather than a passed statement.

The general form is worth more than the fix: **a check comparing two counts has to compare the
same quantity, and a set is a different quantity from a list.** Deduplication anywhere between
the measurement and the comparison turns a check into one that fires on the data rather than
on the defect.

### A mutation harness knows only the tests it was told to run

Both unit harnesses select their suite from a list they carry: `mutate_rust.py`'s `FILTERS`
names module prefixes for `cargo test --lib`, and `mutate_frontend.py`'s `TEST_FILES` names
`.test.ts` files for vitest. A module or a file missing from that list is invisible to the
harness --- so a mutation whose expected test lives there **cannot go red**, and the run would
report it SURVIVED: a gap in the suite, reported as a gap in the suite, for a test that exists
and passes.

Three of them in one increment, on 2026-08-17. `pagetree.rs` was a new module and `pages.ts`
was a new file, so their own tests were outside both lists; and `select` was written in
`lib.rs`, whose crate-root `tests::` module no prefix in `FILTERS` reaches. Nine mutations in
total, every one of them aimed at code that is tested.

**None of them reported SURVIVED, and that is the point.** Both harnesses cross-check every
mutation's `expect` against the *engine's own listing* --- `cargo test -- --list` and vitest's
verbose reporter --- before running anything, and refuse to start while one names a test they
cannot see. The failure is loud, names the mutations, and costs one run of the control.

Two habits follow. When a module or a suite is added, add it to the harness's list in the same
commit --- `scripts/check_mutation_anchors.py` is a gate and checks that anchors point at code
that exists, not that expectations point at tests that run. And when a function's tests would
land somewhere the harness cannot see, that is a reason to move the *function*: `select` went
to `print.rs`, which owns the type it returns, and its tests came with it.

### A check written because a mutation survived has to inherit that mutation's expectation

The loop this repository runs on --- write a mutation, watch it survive, add the check that
catches it --- has a step that is easy to leave out: the mutation still names the check it
survived. The next run then reports **SURVIVED** for a defect the suite does catch, and the
verdict is wrong in the direction that costs work rather than confidence.

`page turn: invalidate a turned page only when its box moves` deletes the explicit
`invalidatePage` from `Scroller.setPageTurns`. It named *"a page turn discards that page's
pixels"*, which is a quarter turn --- and a quarter turn changes the page box, so `applySizes`
invalidates the page whether the call is there or not. That is precisely why *"a half turn
discards the pixels its box did not move"* was written, in the commit that added it and left
the expectation pointing at the old check. One day later the run said SURVIVED.

**What made it a two-minute fix rather than an investigation is that the harness prints the
check that *did* go red**: `expected "a page turn discards that page's pixels" to fail; 1 did:
['a half turn discards the pixels its box did not move ...']`. A verdict that had only said
SURVIVED would have sent somebody to write a check that already exists.

So: when a new check is added because a mutation survived, move that mutation's `expect` in the
same edit --- and keep printing what went red, because a mutation credited to the wrong check
is indistinguishable from a gap in the suite without it.

### Flattening a page tree loses what a page inherited from the node it hung under

`/Resources`, `/MediaBox`, `/CropBox` and `/Rotate` are **inheritable** (PDF 32000-1 table
29): a page that states none of them takes the first value found walking up `/Parent`. That
is why `print.rs` deletes pages *in place* rather than re-parenting them, and its module note
has said so since printing landed.

Reordering cannot be done in place. A permutation moves pages between the nodes they hang
under, and **what a page inherits belongs to the slot, not to the page** --- so a page moved
from a node stating `/MediaBox [0 0 595 842]` to one that does not silently changes size, and
a page moved out from under `/Rotate 90` comes back upright. The document opens, has every
page, in the right order, and one of them is wrong.

`pagetree::reorder_pages` therefore writes the four attributes onto each page *before*
rebuilding the tree, and only where the value would otherwise change: pushing all four onto
every page costs an untouched page its byte-for-byte identity, and for `/Rotate` it is the
flattening `apply_turns` already refuses to do, since `effective_rotation` answers 0 whenever
its 64-hop walk gives up.

**The corpus cannot test this and looks as though it can.** `text-heavy.pdf` is the only
nested fixture --- 113 `/Pages` nodes, three levels --- and every inheritable attribute it has
sits on the **root**, so flattening onto the root preserves all of them. A check written
against it passes whether the push-down exists or not. The fixture is hand-built, four pages
under two nodes, one of which states what the root does not; the same shape twice, in
`pagetree.rs` for the mechanism and in `save.rs` for the file, where PDFKit reads the rotation
back because `/Rotate` is the one inheritable attribute an OS parser reports.

### A permutation and a subset are the same document to every reader, and not the same file

The reorder above must run **only** when the order really changed, and the reason is not
performance. A document nobody rearranged, put through `reorder_pages`, comes out with the
same pages in the same order at the same angles --- so every check that reads the document
agrees, including the third-parser ones that read it through PDFKit. What differs is that
every page has been reparented and every intermediate tree node abandoned.

So the control for "does it reorder when it should not" cannot be written against the pages.
It is written against the **shape of the tree**: the `/Type` of the first thing the catalog's
`/Pages` node points at, which is `Pages` for a document whose tree survived and `Page` for
one that was flattened. Two checks, one in `save.rs` and one in `print.rs`, and each carries
its own opposite --- the same document rearranged *does* come out flat --- because a check
asserting "still nested" passes trivially against code that never reorders at all.

The general shape: when two implementations produce the same output for the input you have,
the difference is real and lives somewhere your checks are not reading. Find where, or the
"optimisation" is an untested branch.

### A quirk documented as harmless becomes a defect the day its precondition is wired

`print::Pages::Only` carried this, in the type's own doc comment:

> Exactly these, one-based --- and printed in **document order**, not in the order they are
> listed here. [...] `[3, 1]` prints page 1 then page 3.

Accurate, deliberate, and reasoned: `build` produced a subset by *deleting* the pages nobody
wanted, so the survivors kept the positions the file gave them. It was harmless for as long as
nothing in the application could produce an order the file did not already have.

`Command::Move` being wired ended that, and nothing in the print path would have said so.
`expect_pages` compares how many pages came out, never which, so a reader who rearranged a
document and pressed print would have got the old order on paper with every check green ---
the print path's own documentation being the only record that this was expected.

`save.rs` had the same gap and had *closed* it, with a refusal whose test said in as many
words that it was unreachable from the application and existed for the day `Move` landed. Two
subsystems, one shared assumption, and only one of them left a tripwire.

**So when a constraint is written down as "cannot happen yet", write down what happens when it
does, and put it where the code is rather than only where the plan is.** A refusal that fails
loudly is one way; the weaker one that print took --- a sentence in a doc comment --- depends
entirely on somebody reading the right file at the right moment.

### The order a model inserts into is not the order its caller is looking at

`Command::Move` names a **neighbour**: put this page behind that one. A reader names a
**destination**: put this page here. Something has to invert one into the other, and the
inversion is not `pages[to - 1]`.

The model removes the page first and *then* reads the anchor's position, so the anchor has to
be read out of the order **without the moved page in it**. Reading it out of the order that
still holds it is correct for every move towards the front and wrong for every move towards
the back --- and it is wrong in two different ways, which is why both are checked:

- **One slot short.** Moving slot 0 to slot 3 of four names the page at slot 2, and the page
  lands at slot 2. A drag that stops just before where it was dropped.
- **A refusal.** Moving slot 0 to slot 1 names the page at slot 0 --- the page being moved ---
  and the model answers `AnchorIsTarget`. The same arithmetic error, with a completely
  different symptom, on the shortest move there is.

The inversion lives in `edits.ts` and is the one piece of arithmetic in a file whose header
says it holds no rules. That is the honest place for it: the model refuses an index for a
reason, and inverting it in Rust would need the order the frontend already holds.

### A page count cannot see a move, and every deletion check is built on the page count

`pageDeletionChecks` reads a deletion through observables a deletion produces: one page
shorter, an empty last slot, coverage dropping, the page below moving up. **Every one of them
reads identically for a move that worked and a move that did nothing at all**, because a move
changes no length --- which is why the move phase is its own function rather than three more
names in that one, and why its first check asserts the count *stayed*.

What is left is identity, and identity needs a fixture that has two of whatever it
discriminates. The phase compares the text on each page by fingerprint and **skips with a
stated reason** where two pages read alike, rather than passing on a comparison that cannot
fail. The one property the text cannot see --- that a page carries its *measured size* to
wherever it moved --- runs only on `mixed.pdf`, the single corpus whose pages are different
sizes, and the mutation aimed at it has its own runner for that reason: on a uniform document
the estimate and the truth are the same number, so the check skips and a mutation aimed at a
skipped check reports SURVIVED.

### A duplicate key in an object literal is legal JavaScript, so the suite stayed green

A mechanical edit that inserts a line after a matched one has to match the **whole** line.
Adding a property after `deletePage: ...` in two files, once for each of two indent levels,
inserted it twice wherever the four-space pattern occurred inside a six-space line --- the
four-space form is a substring of the six-space form, so the second pass matched the line the
first had already handled.

The result was an object literal with the same key twice. `vitest` ran all 639 tests and
passed: a duplicate key is legal, the last one wins, and both were identical here. The gate
that caught it was `npm run check` --- TypeScript's *"An object literal cannot have multiple
properties with the same name"* --- which is a diagnostic, not a test, and would not have
existed to catch a duplicate whose two values differed.

Two habits. Anchor a line-wise mechanical edit on the line *including* its indentation and
assert the count you expect, rather than replacing whatever matches. And when an edit script
reports more replacements than there are sites, that is the finding, not a rounding error ---
the script said 3 for two known call sites and the number was read past.

### A tolerance around one value is satisfied by an estimate that replaced every value

`page move: forget every page's measured size when the order changes` SURVIVED, and the check
it was aimed at was neither skipped nor absent --- it ran, and reported `[OK]`.

The check compared the shape of the page that moved against the shape it had been measured at,
within 0.02. That is the right comparison for the defect it was written against: a scroller
re-indexing its learned sizes by position gives the moved page the size of whatever used to be
at the front, and on `mixed.pdf` those shapes are 1.41 and 0.71, so it fires.

The mutation is a different mechanism. It does not put the wrong size on the page, it drops
**every** page's carried size, so the whole document falls back to one estimate --- and an
estimate is free to land within 0.02 of whichever single shape is being compared. Here it did.
One tolerance around one value cannot tell "this page kept its size" from "no page has a size
and the placeholder resembles this one".

The fix is not a tighter tolerance, which would trade this for flakiness. It is to compare
**both** slots against the two shapes the check's own precondition has already established are
further apart than the tolerance: a single shared estimate would have to be within 0.02 of both
at once, which is arithmetically impossible. The assertion then fails for that mutation
whatever value the estimate takes, rather than because the value was unlucky.

Two things worth carrying. **A scale-invariant comparison is blind to a whole class of
substitute**, and shape was chosen here for a good reason --- fit-width rescales every page when
the widest one changes slot, and the moved page really does land at twice the width it was
measured at. The property that survives normalising is smaller than the property you wanted.
And **the coverage existed the whole time**: the same mutation reddened a deletion check that
reads absolute boxes. A mutation that reports SURVIVED while some *other* check goes red is
naming the check that cannot fail, not a gap in the suite --- read which check went red before
concluding anything is missing.

### The natural place to press is the one place the defect has no effect

`page drag: treat a press that never moved as a drag` SURVIVED, against a check written to be
exactly its control --- *"a press that does not travel asks for nothing"*. Both of that check's
clauses were incapable of failing, for two unrelated reasons, and each is worth having.

The first is a state read after the state was cleared. `strip.dragging` was read after the
`pointerup`, and the release is what ends the drag: it is false at that moment whether or not a
drag ever started. The question the name asks is *did this become a drag*, and the code was
asking *is this still a drag*, which has one answer. Read it before the release and the
mutation fails immediately.

The second is the interesting one. The press was at the row's **centre**, because that is where
one presses a thumbnail. The gap nearest a row's centre is the gap on one side of that row, and
`landingSlot` deliberately calls both gaps either side of a page a no-op --- otherwise releasing
the pointer a pixel from where it was pressed would move the page. So a press that wrongly
became a drag asks for **no reorder at all**, and the "no reorder was asked for" clause is
satisfied by the defect it was written to catch. No press position fixes this: any movement
small enough to be a click keeps the pointer inside one of those two gaps, which is precisely
what makes the threshold's absence invisible in that observable.

So the general shape: **when a guard exists to stop a small input having an effect, the
small-input case is where the effect is absent by other means too.** The observable that
discriminates is not the outcome, it is whether the machinery ran --- which is why the check now
asserts on `dragging` at the moment the drag would exist, and keeps the outcome clause only as
the separate statement that a drop did not fire.

### A feature reached only through an optional callback is invisible to a harness that omits it

The page strip does not take a pointer capture, and does not drag, unless `onReorder` is
supplied --- deliberately, because a strip driven by a harness or showing a document nobody can
edit should not swallow pointer events for a gesture that can never do anything.

The consequence is that the window harness, which builds its own `Sidebar` mirroring
`App.svelte`, had no way to observe the gesture *at all* until it supplied a handler. Not a
failing check: a check that would have found `strip.dragging` false forever and reported the
drag as broken, against code that was fine.

What the harness supplies is a **recorder**, not the application's handler, and that is the part
worth being explicit about rather than apologetic. Running the real edit there would be a second
implementation of `App.svelte`'s one-line handler, and the two seams it would exercise --- the
slot arithmetic and the `page_move` round trip --- already have checks that need no pointer.
What a window can answer and nothing else can is whether WKWebView captures the pointer, keeps
delivering moves after it has left the row, and lays out geometry the gap arithmetic can read.
Scoping the window check to exactly that leaves it fast, leaves it stable, and leaves the
recorder's dishonesty confined to a seam that is one line long and covered by reading it.

### Pressing a row navigates, and navigating scrolls the list out from under the drag

Two separate things, and the entry is worth reading for the second one because the first is
what it looked like from the outside for two rebuilds.

**The hazard.** Pressing a page thumbnail navigates to that page, and the strip follows the
page being read --- so a press can make the strip scroll itself at the instant a drag begins,
with the pointer stationary. The content moves and the pointer does not, and the drop lands on
a gap the reader never pointed at. The guard has to start at the **press**, not at the drag,
and that is the part with no natural place to put it: the navigation happens on `pointerdown`,
before the pointer has travelled far enough for the press to be a drag, so refusing "while
dragging" is too late by one event. The press is recorded first and the navigation second, and
the scroll refuses while a press exists at all. Writing those two lines the other way round is
the whole bug, and neither order looks wrong on its own.

**What the corpus sweep actually found was not that.** Four of fourteen documents reported a
drop that asked for nothing, and the guard above --- reasoned out, written, unit tested and
mutation proved --- changed the result by exactly zero. The defect was in the check: it
measured the row's position, the drop target's position and the drop target *element* once at
the top of the phase, then ran a control press, and reused all three for the real drag. The
control press navigates. On `outline-simple` the strip scrolled 400 px between them, so the
real drag pressed 500 px below where its row had moved to and released against a row element
the windowing had since replaced --- a detached element, whose `getBoundingClientRect()` is all
zeros and therefore looks like a perfectly ordinary coordinate.

The arithmetic reconciles to the digit once the numbers are in front of you:
`contentY = 1 - 34 + 400 = 367`, and `round(367 / 200) = 2`, which is the gap the failure
reported. **Getting those numbers took one enriched detail line.** The check had said only
*"asked nothing"*, and two rebuilds went into theories that the panel's scroll offset and the
two rectangles would each have refuted in seconds. A failing check should print its inputs, not
only its verdict --- most of all when its inputs are geometry nothing else can see.

Three things to carry:

- **A coordinate measured before an action that can scroll is not a coordinate.** Re-read it at
  the moment you act. The staleness is unbounded, and re-reading costs nothing.
- **An element handle held across a virtualised list's re-layout is a handle to a dead node**,
  and a dead node measures as the origin rather than as an error.
- **A fix that is correct and a fix that is the cause are different claims.** The guard stays,
  because the hazard is real and now has a test and a mutation. But it was written down as the
  explanation for a failing sweep before the sweep had been asked whether it agreed, and the
  next run said no.


### A break recorded as a position in a list the callee does not own

A tagged paragraph's lines were joined with a space by recording *where* the separators fell
as a count of entries in the input list, then walking the output of `linkRunsIn` and inserting
a space at the matching count. The two lists are not the same list. `linkRunsIn` walks
character by character and **coalesces** adjacent indices into one range, so a paragraph whose
two lines are contiguous in the character stream — the ordinary case — comes back as one run
holding one range. The loop never reached position 1, the space was never written, and a
screen reader read the last word of one line joined to the first of the next.

The fix is one word of vocabulary: record the break as a **character index**, which names a
place in the page's own stream, rather than as a position in a list the function receiving it
is free to restructure. An index into somebody else's intermediate representation is a
coupling that nothing declares and no type checks.

**It shipped for sixteen days and no fixture reached it.** The separator exists only for a
*tagged* block; `tagged.pdf` is the only corpus carrying a `/StructTreeRoot`; and none of its
blocks wraps to a second line. So the branch was unreachable across the whole fourteen-corpus
sweep, the window check that would have noticed compares text as a sorted multiset and never
ran on a page that could differ, and the release passed every gate.

**What surfaced it was a mutation reporting SURVIVED**, which is the verdict this project
treats as least trustworthy and most valuable: it reads as a gap in the checks, and it was one,
but writing the test that closed the gap is what found the code beneath it had been wrong since
it was written. Per the trap about a branch no fixture reaches, the mutation moved to the
harness where a unit test can judge it rather than earning a fifteenth corpus.

**The control cost two fixtures and is still weaker than its first name claimed.** Written as
"does not join the lines of a block nobody tagged", it was measured against a mutation that
sends untagged blocks down the tagged path — and survived, because at every line spacing tried
the geometry had already split the two lines into two *blocks*, so both paths emit one
paragraph each and the fixture cannot tell them apart. The block cut is a multiple of the type
size and there is no spacing that is two lines and one block for this generator. The test was
renamed to what it establishes, and the tag distinction it was reaching for is proved a layer
up in `reading.test.ts`, where a mutation on the same property is caught. **A control you
cannot make fail is a control you must rename, not keep.**

### A Windows-only file is invisible to every gate on a Mac, and cargo can cross-check it

`scripts/gates.py` was 15/15 for sixteen commits while `examples/print_probe.rs` did not
compile. The page-move work changed `print::Pages::Only` from `Vec<u32>` to `Vec<PagePlan>`,
because printing a reordered document has to carry the order and each page's own turns; every
caller was updated except the one behind `#[cfg(windows)]`, and a Mac never parses that line.
The first tag of `26.8.3` turned **both** runner legs red on it, and the Windows leg reported
four failures rather than one --- clippy, test and bins all stop at the same `error[E0308]`,
so one type error reads as four broken gates.

**The reason it survived so long is that these commits reached CI for the first time that
day.** Sixteen commits of Phase 2 work sat local, so the platform that could see the break was
never asked. That is the trap `AGENTS.md` names as *the gates had never run on the platform
where they fail*, arriving through a different door: not a gate that fails on Windows, but a
*file* that only Windows compiles.

**`cargo check --target x86_64-pc-windows-msvc --all-targets` answers it from a Mac in about
eight seconds warm**, and `scripts/check_windows.py` is that command with the environment it
needs. `check` does not link, so no MSVC linker is required --- but four things are, and each
fails with an error naming a different missing tool, so finding them one at a time is five
rebuilds:

- the splatted SDK headers (`xwin`), or `ring` stops at `'assert.h' file not found`;
- `llvm-lib` as the archiver, or `cc-rs` wants MSVC's `lib.exe`;
- `llvm-rc` **on `PATH`**, or `tauri-winres` panics with `NotAttempted("llvm-rc")`;
- `vendor/pdfium/bin/pdfium.dll`, because Tauri resolves `bundle.resources` for the *target*
  platform and otherwise dies on `resource path ... doesn't exist`, which reads like a broken
  checkout rather than a missing cross-compilation input.

**Three controls, and the first one is the honest limit.** Changing a `PagePlan`'s `turns`
from 0 to a wrong value stays **green** --- this is a type-check, not a test, and it says
nothing about whether the Windows code is correct, only that it compiles. Restoring the
original type error goes red with CI's exact diagnostic, and hiding the DLL makes it refuse
with exit 2 rather than fail confusingly. It is not a gate on purpose: it needs a 629 MB SDK
splat that a fresh checkout does not have, and CI runs a real `windows-2025` runner, which is
strictly better evidence. What it buys is that the runner confirms rather than discovers.

### A gate that refuses on a precondition of running is red on every machine that is not running

The same tag's macOS leg failed on the `corpora` gate, and the gate was right about everything
except which question it was being asked. `classify()` in `scripts/viewer_sweep.py` did two
jobs: *is every fixture on disk accounted for* --- an invariant of the repository, true
anywhere --- and *is every corpus present*, which is a precondition of running a sweep. It
raised on the second inside the first, so `--list`, which sweeps nothing, refused.

`scripts/ci_fixtures.py` generates seven fixtures and **states in its own docstring** why the
other nine are not generatable on a hosted runner: fonttools with a per-image system font,
qpdf, a 550 MB write. So the gate demanded, on every runner, something the repository had
already written down as deliberately absent --- and no local run could notice, because a
development checkout has all forty-three.

The fix returns the missing list instead of raising on it. `--list` prints it as `[INFO]`, and
the refusal moves to the run path, aimed at the corpora that run will actually open rather than
at all fourteen: a full sweep is unchanged, and `--only links` on a machine holding `links`
now works instead of being refused over twelve fixtures it was never going to touch.

**One consequence is worth stating because it is the same mistake one level down.** The
exclusion-rot warning --- an exclusion pattern matching no fixture --- is now asked only where
the whole set is present. On a runner holding seven fixtures most patterns match nothing for a
reason that is not rot, and a warning that fires on every CI run is one nobody reads on the run
where it means something.

**It happened again on 2026-08-21, in unit tests rather than in a gate, and it had not been
pushed yet.** Nine commits added the signature reader and its fixtures, and three of its tests
end with `assert!(examined > 0)` beneath a loop that `[SKIP]`s a fixture it cannot read. The
guard is the right instinct --- five SKIP lines and a pass look identical --- but no signed
fixture exists on a hosted runner: they need pyhanko, and `scripts/ci_fixtures.py` says in its
own docstring which families a runner deliberately does not get. Measured by hiding
`testdata/incr-*.pdf`: **three red**, every one of them telling a machine that structurally
cannot to "generate testdata/".

The fix is not to weaken the guard but to aim it at the state it was for. *Some present and one
missing* is a broken checkout and looks exactly like a pass; *none present* is a runner and is a
fact about the machine. So the count became `assert_eq!(examined, cases.len())` --- every named
fixture, not merely one --- behind an early return when none of them exists. Both directions
were proved: no signed fixture at all gives **702 passed, 0 failed**, and hiding exactly one
gives two red, which the count it replaced would have let through.

**The general question to ask of any "did this actually run" guard: what does it say on the
machine with the fewest inputs?** If the answer is "it fails", it is not a coverage check, it is
a requirement --- and the place to state a requirement is the thing that installs the inputs.

### Two drafts under one tag, with the artifacts split, and the first cause I recorded was wrong

`v26.8.3-rc2`'s macOS build died in `Set up job` on `429 Too Many Requests` fetching an
action from codeload --- an infrastructure failure, before a line of our code ran. Re-running
that one job with `gh run rerun --failed` turned every job green, and left **two draft
releases under the one tag**:

| draft | assets | what is missing |
|---|---|---|
| `371785472` | 6 | `tpdf_aarch64.app.tar.gz` and its `.sig` --- the macOS updater bundle |
| `371787661` | 4 | every Windows installer |

A complete release is 8 assets, as `v26.8.2` is. So publishing either one ships something
broken: a release macOS cannot update from, or one with no Windows build at all. `26.8.0`,
`26.8.1` and `26.8.2` each produced exactly one complete draft, and none of them involved a
re-run.

**That paragraph attributed it to the re-run, and the next tag falsified it.** `rc3` was a
clean run with no re-run of anything, and it split identically: 5 assets and 4. So the re-run
was a coincidence, and this entry is left with its first answer visible rather than quietly
corrected, because the shape of the error is the lesson --- a plausible cause, adjacent in
time, recorded in the same breath as a sentence promising not to guess at one.

**The second answer was wrong too.** A race between the two build jobs is ruled out by the
timestamps: `max-parallel: 1` is already set and has been, `rc3`'s macOS leg uploaded at
14:27:49 and its Windows leg at 14:38:20 --- eleven minutes apart, strictly serial, and the
second still did not find the first's draft.

**The logs then named the failing step, which is as far as reading gets you.** `tauri-action`
resolves the release itself, and its own source says why that is fragile: *"you can't get an
existing draft by tag, so we must find one in the list of all releases"* --- so it pages
`listReleases` for a matching `tag_name` and creates one when it finds none. `26.8.2`'s
Windows leg logged **`Found draft release with tag v26.8.2 on the release list`**; neither leg
of `rc2` or `rc3` logged `Found`. So the lookup ran and came back empty while a matching draft
existed.

**Why the lookup came back empty is still open**, and the workflow diff against `v26.8.2` is
the release body's wording and nothing else. Worth suspecting rather than believing: the same
REST endpoint answers **HTTP 200 with `[]`** under the token in this machine's login keychain
while `gh release list` shows five releases, so it is an endpoint that reports "nothing" where
another instrument reports five --- but nothing establishes that the runner's token behaves
that way.

**So the fix removes the lookup instead of repairing it.** A `draft` job creates the release
once and hands both build legs its id; `releaseId` makes the action take the branch
`if (tagName && !releaseId)` and never resolve anything. The manifest merge that made
`26.8.2` whole is unaffected and that was read out of the action's source rather than assumed:
before writing `latest.json` it lists the release's assets and seeds `platforms` from one
already there, so the second leg extends the first's file. It only ever held because both legs
uploaded to **one** release, which is now guaranteed rather than hoped for.

**`max-parallel: 1` was already set, and its comment had expired.** It read *"tpdf ships no
updater today, so that exact failure cannot occur here"* --- the updater landed in `26.8.2`,
five days before anyone noticed --- with the condition for its own expiry stated in the very
next line: *"If an updater is ever added, this is not optional any more."* A comment that
carries its own expiry date is only as good as the person who re-reads it.

**The reason this is dangerous rather than merely untidy is that the checklist said "publish
the draft".** With one draft that sentence is unambiguous and for three releases it was true.
With two it names neither, and both look right in `gh release list`: same tag, same name, same
`createdAt` --- which for a release is the tagged commit's timestamp, not when the draft was
made, so even the timestamps cannot separate them. Step 11 now counts assets before
publishing, because a count is the only thing here that can tell a whole release from half of
one.

**Three instruments are useless for this, and the third one bites during cleanup.**
`gh release view <tag>` returns *a* release for the tag with no way to say which, so it
reports one draft's assets as though they were the release's. `gh release delete <tag>`
answers **"release not found"** for a draft that plainly exists --- measured on `rc5`, where
the draft was still there afterwards --- because it resolves the tag through the same REST
endpoint that does not return drafts, which is the behaviour the whole `draft` job routes
around. Delete by `databaseId` with `gh api -X DELETE`. And `gh api repos/<owner>/<repo>/releases` answers **HTTP 200
with `[]`** under the token in the login keychain, while `gh release list` shows five releases
--- the REST endpoint wants an OAuth scope this token lacks and says so by returning nothing
rather than by failing. An empty list from an authenticated call is not evidence of an empty
repository. `gh api graphql` reaches them, ids and assets included, and is what the table
above was read from.

### `$?` read in the same word as a command substitution is the substitution's status

Three refusal branches of the new `draft` job were measured and all three reported **exit 0**,
which would have meant a job that prints `::error::` and stays green, handing an empty
`releaseId` to the build legs --- the exact failure the job exists to prevent, reintroduced by
its own guard. The guard was fine. The measurement was:

```sh
echo "case=[$(echo "$CASE" | tr '\n' ',')] exit=$? output=[$(cat out.txt)]"
```

Expansions inside one word are evaluated left to right, so the `$( … | tr … )` **runs before
`$?` is expanded** and `$?` reports `tr`. Capturing the status into a variable on the line
immediately after the command gives 0, 1, 1 --- the branches were always right.

**The tell was that all three agreed.** A control set where the passing case and both failing
cases return the same value is measuring something other than what it names, and that is
cheaper to notice than to debug: two of those three were *designed* to differ from the first.
Same family as every other entry here where the instrument, not the subject, was broken ---
and it is worth stating in shell terms because `set -euo pipefail` was on, which makes the
script look like the last place a status could be lost.

### A caller that validates first cannot reach the guard beneath it

`file.extractPages` parses its argument twice: the palette's `problem` callback rejects text
that is not a page range, and the command's own `run` checks again before handing slots to
the action. The second check is the one that decides whether a defect writes a file, so it
got a test --- driven, reasonably enough, the way every other command in that suite is:

```ts
registry.run("file.extractPages", "nonsense");
expect(fired).toEqual([]);
```

It passed, and the mutation that **deletes the guard entirely** passed with it. `CommandRegistry.run`
consults `argument.problem` before calling `argument.run`, so the guard under test was never
executed in either direction. The test asserted the registry's refusal and read it as the
command's.

**Reach the guard from where its bad input would actually come.** The value here is
`command.argument.run("nonsense")` --- a caller that skipped validation, which is precisely
the case a second check exists for. With that, the mutation goes red.

The general shape is worth more than the instance: **when two layers check the same thing,
a test entering at the outer layer cannot see the inner one**, and it does not fail to see
it — it passes, which is indistinguishable from the inner check working. It is the
belt-and-braces arrangement that makes this invisible: the braces are what you are testing,
and the belt is what you put on first. Related to *a test whose precondition is already
satisfied never runs*, and different in where the satisfaction comes from — there the fixture
supplies it, here the caller does.

**The complement, hit the next day, and the fix is a different one.** `runMenuCommand` checks
`enabled` before dispatching, and `CommandRegistry.run` checks it again. The test asked for a
withheld plain command and expected `false`; deleting the *outer* guard left it green, because
the inner one answers identically. Same two layers, the other direction — and here entering at
the outer layer is right, so the fix is not to move the entry point but to **aim at the branch
the inner check does not reach**. A command that takes an argument never goes through
`registry.run` at all: it opens the palette. So the discriminating case is a withheld
*argument* command, where the outer guard is the only thing standing between a closed
command and an input on screen asking for its value.

Read the two together as one rule with two remedies: when a guard is duplicated, find the
input path on which it is **not** duplicated, and test there. If there is no such path, the
guard is genuinely dead and the honest move is to delete it or let the type carry it — which
is what *an unreachable guard is worth keeping if the type can carry it instead* is about.

### A menu item is a global key claim, not a label

The native menu bar was built by generating it from the command registry, so that a menu
entry and a palette entry could not describe different things. The accelerator beside each
item was the obvious next derivation: `keys.ts` already holds every binding as data and
renders the palette's `⇧⌘R` from it, so rendering `Shift+CmdOrCtrl+R` from the same record
is a third reader of one table and cannot drift.

That reasoning is right and it very nearly shipped a serious regression, because it treats
the accelerator as **display**. It is not. On macOS the menu bar receives a key event before
the web view does, so registering an accelerator *moves the shortcut out of whatever has
focus*. Two families of binding cannot survive that:

- **Anything with no ⌘ at all.** `nav.nextPage` is bare `n` and `nav.firstPage` is `Home`.
  As menu accelerators they would take those keys out of the find field — and out of every
  text input the application ever grows — while the menu itself looked perfectly correct.
- **Chords a text field claims anyway.** ⌘Z, ⇧⌘Z, ⌘C, ⌘A. The application's own handler
  already carries an explicit `inTextField` guard on undo, written precisely so that a
  reader correcting a typo in the find field does not silently undo a page rotation. A menu
  accelerator fires before the page, cannot see focus, and therefore **undoes that guard
  from outside**, which no test of the guard could notice.

So the rule is: the menu may claim a chord only where the application already claims it
unconditionally. Both refusals are enforced in code — `keys.ts` returns null for a binding
without the accelerator key, `menubar.ts` names the four exceptions with a reason each — and
both are proved by mutation, because "the menu shows no shortcut" and "the menu stole a
shortcut" look identical from inside the page. There is no third option, either: a menu item
cannot display a shortcut it does not claim.

The related half is a **collision**. Two items on one accelerator is not a build error;
AppKit takes the first and the second is simply dead, which reads as one command not working
rather than as a menu that is wrong. That is one assertion over the built spec, and it is
cheap enough that there is no reason to find out the other way.

The general shape, for the next platform affordance that looks passive: **anything the OS
registers on your behalf is authority you have taken, not information you have displayed.**
Ask what it now intercepts before asking whether it reads correctly.

### A framework can abort your whole test binary, and 470 passing tests report nothing

Reading what the keyboard prints on a key means calling `TISCopyCurrentKeyboardLayoutInputSource`.
Three tests were written over it, each passed when run alone, and the three together produced:

```
process didn't exit successfully: ... (signal: 6, SIGABRT: process abort signal)
```

No test names, no failure, no assertion — and 470 unrelated tests in the same binary died with
it having reported nothing at all. The crash report says exactly what happened, which is the
only reason this took minutes rather than an afternoon:

> ABORT -> Text Input Sources or Text Services Manager API is being called in two threads
> concurrently. If you are a UI application, you must call TIS/TSM API on the main thread. If
> you are a non-UI application ... you must not call TIS/TSM API from multiple threads
> concurrently.

**This is a deliberate refusal, not a race that happened to corrupt something.** HIToolbox
checks, and kills the process. So the usual instinct — narrow it down, add logging, look for
the data race — is wasted: there is nothing subtle to find, and the message is in
`~/Library/Logs/DiagnosticReports/`, not on stderr, which is why the terminal shows only the
signal.

Two things follow, and the second is the one that generalises.

**`cargo test` is a multi-threaded caller by default.** One test over such an API is fine
forever; the second one is what kills it. That is a hazard shaped like a landmine — the cost
arrives when someone adds coverage, which is exactly when they will read the failure as being
about their new test. Both halves of the rule are enforced here: a module-level `Mutex`
serialises every call (the non-UI half), and the Tauri command hops to the main thread (the UI
half). Neither alone is enough, and they are enforced in different places, so a comment saying
so lives at both.

**A SIGABRT is not a red test.** Every check this repository has for "prove the test can fail"
assumes failure looks like a failed assertion. It does not here: the harness gets a dead
process and no results, which is indistinguishable from a build that never ran — and is the
same shape as *a mutation caught by an access violation produces no test results at all*, met
from the other direction. When a module wraps a framework that can abort, the first question
is not "does this test fail when the code is wrong" but "does this test *report* when the code
is wrong".

Worth checking before wrapping any HIToolbox, Carbon or Text Services API, none of which is
thread-safe and several of which say so only by aborting.

### A mutation that survived, a comment that claimed a behaviour, and no test to add

`kUCKeyTranslateNoDeadKeysBit` was set with a confident reason beside it: `Equal` is the
acute-accent dead key on a German layout, so translating with dead keys enabled would swallow
the call and return nothing for that position. A mutation flipping the bit to 0 **survived**,
and the instinct — strengthen the test until it dies — would have been wrong twice over.

Measured instead, by flipping it and printing the map: **byte-identical, all eleven positions
present.** The reasoning was true of `kUCKeyActionDown` and false of `kUCKeyActionDisplay`,
which is the action this asks for. Obvious in hindsight — a key *cap* has no dead-key state,
which is the whole difference between what a key shows and what it types.

So the constant is inert for this call. It stays, because it is the documented-correct argument
for a lookup that wants a legend rather than an insertion and costs nothing, and the mutation is
deleted rather than the test strengthened. The general rule this repository already states —
*a mutation that survives may be a variant, not a gap* — has a sharper form here: **when the
survivor is a constant, measure what the constant does before concluding anything about the
tests.** A test written to kill this one would have been a test asserting a behaviour that does
not exist.

The comment is the durable part. It now records the wrong reason, the measurement, and the fact
that there is no test to add — because the next person to run mutation coverage will find the
same survivor and, without it, will re-derive the same wrong explanation.

### A synthetic right-click posted to the window server never reaches the web view

Right-clicking a page thumbnail offered WKWebView's own menu, whose one entry reloads the
frontend. Fixing that is one `preventDefault`; **proving it** turned out to be the interesting
part, because the obvious instrument does not work.

Three ways to post a secondary click from outside the process were tried and none of them
reached the page:

- `osascript`'s `key down control` + `click at {x, y}`. Control-click is the secondary click on
  this platform, and the two events do not combine into one at the HID layer.
- The same through System Events on the process rather than the screen.
- A `CGEventCreateMouseEvent` with `kCGEventRightMouseDown` — which needs PyObjC, absent here.

Each of them **appears to work**: the click lands, nothing errors, and the screenshot afterwards
shows no menu — which is exactly what a broken handler looks like. Two rounds were spent on the
handler before the instrument was suspected.

**The right instrument was already in the repository.** `viewer_check.py` runs inside the page,
so it can dispatch a real `contextmenu` `MouseEvent` at a row and read `defaultPrevented` off
it — which asserts the *suppression* directly rather than inferring it from a screenshot. Two
checks, both proved by mutation, in less time than one more round of fighting the event layer.

Two details that are easy to get wrong and cannot fail loudly:

- **`cancelable: true` is required.** Without it `preventDefault()` is a no-op and
  `defaultPrevented` stays `false` no matter how right the handler is — an assertion that can
  only fail, which is the loud direction, but it reads as the fix not working.
- **A slot assertion needs a row that is not the current page.** Every command such a menu
  offers acts on the page the viewer is on, so reporting the current page instead of the one
  under the pointer looks correct whenever the two agree — which on a freshly opened document's
  first row is always. The mutation that adds one to the slot is what says the check can tell
  them apart.

The general shape: **when a gesture crosses a process boundary, the check belongs on the far
side of it.** Driving the operating system to drive the application is the wrong layer whenever
the application can be asked directly — and the failure mode of the wrong layer is silence, not
an error.

### `PdfiumLibraryBindingsAlreadyInitialized` — a helper that binds its own library works alone and fails in company

A probe that renders two files wants a render helper, and the obvious one takes a path and does
the whole job: bind the library, open the document, render the page. It works in isolation and
fails the moment the caller has already opened a document of its own, which in a probe is
always:

```
[FAIL] could not load Pdfium from vendor/pdfium/lib/libpdfium.dylib: PdfiumLibraryBindingsAlreadyInitialized
```

`Pdfium::bind_to_library` may be called once per process. So a helper takes
`progressive::Bindings` and never loads anything — the same shape `RawDocument::open` already
has, and the reason it has it.

Worth knowing because the error names the *library path*, which sends you to check the vendor
directory and the pin. Neither is wrong. The second call is.

### A wash that reads as zero everywhere: PDFium's buffer is RGBA, not BGRA

`progressive::RENDER_FLAGS` is `FPDF_ANNOT | FPDF_REVERSE_BYTE_ORDER`, and the second one is
what decides the channel order of every pixel a probe reads. Written the other way round —
`let (b, g, r) = (px[0], px[1], px[2])` — a check for a yellow highlight looked for
`r > 180 && g > 150 && b < 170` against a pixel of (255, 230, 51) read as b=255, g=230, r=51,
and counted **nothing**, on every fixture.

The reason this is worth an entry rather than a shrug: **the failure is indistinguishable from
the feature not working.** Three fixtures reported "0 wash after the mark", which reads as an
annotation that was written and never drawn — and the next move from there is to go looking at
the writer, the appearance stream and the renderer's annotation flag, none of which is the
problem. What settled it in one step was dumping the *bounding box* of the matching pixels over
the whole page rather than counting them inside an expected band: a count of zero says nothing
about where to look, and a bounding box would have been empty for a real failure and present
for this one.

### A coverage figure over the union of several quads measures the line spacing

A multi-line highlight is several rectangles. Asking "what fraction of the mark's area is
washed" over their **bounding box** answers a question about the gaps between lines: measured on
`links-cropped`, a correct two-line highlight covers **32%** of its own bounding box, and a
correct forty-quad one on a rotated page covers 25%. Both look like a mark drawn somewhere else.

Measure per quad. Two further corrections came out of doing that, and each is the same shape —
a statistic that is right for one input and meaningless for another:

- **Count wash *or* ink, not wash alone.** The wash multiplies, so the glyphs under it come out
  dark rather than yellow: a tight box around a dense glyph is legitimately more ink than wash,
  and a wash-only figure reads 2% for a mark that is exactly where it should be.
- **Skip quads too small to hold a percentage, and say how many.** The worst quad on
  `links-rotated` is a 7.9 × 0.97 pt punctuation glyph — 30 pixels at 2x, most of them the
  antialiased edge of the glyph itself. A threshold that has to accommodate it is a threshold
  that cannot fail. Counting and reporting them matters as much as skipping them: a page whose
  every quad were skipped would otherwise pass by having nothing to check, which is why the
  control asserts that at least one was measurable.

### PDFKit reports an annotation's bounds rotated and renders the page unrotated

Diagnosing a highlight on a `/Rotate 90` page with `PDFDocument`: `page.draw(with: .mediaBox)`
draws the page in its **own** unrotated space, while `PDFAnnotation.bounds` answers in the
page's **rotated** space. So the wash appears at one place in the rendered image and the
annotation reports another, transposed — and the arithmetic to reconcile them is a rotation
about a corner nobody has written down.

Two rounds were spent concluding the writer was wrong. It was not: rendering the same file
through PDFium, whose render *is* rotated, put the wash within 1.5 pt of where the mark was
made, on both rotated fixtures.

**The lesson is not about PDFKit.** It is that a cross-check run in a different coordinate
convention than the thing under test cannot be read directly, and reading it directly produces
a confident wrong conclusion. Cross-check in the space the feature is defined in, or convert
explicitly and say so.

> ⚠ **Re-measured 2026-08-20 on macOS 26, and the two halves are the other way round.**
> `PDFAnnotation.bounds` returned `(60.949, 501.426) 246.7 x 11.9` for an annotation whose
> `/Rect` is `[60.949 501.426 307.611 513.347]` — the raw rectangle, **unrotated**, byte for
> byte. And `page.draw(with: .mediaBox)` drew the content **rotated**: the mark's pixels landed
> at the turned position, and six of `rotated-90`'s twelve text lines were clipped off the side,
> because `page.bounds(for: .mediaBox)` answers 612x792 for a page poppler renders at 792x612.
> So PDFKit rotates when it draws and does not when it reports, which is the mirror of what is
> written above.
>
> Whether the original session measured a different call, a different display box or a different
> macOS is not recoverable, so this is left standing rather than rewritten. **The conclusion is
> unaffected either way** — the two layers are in different frames and a figure read across them
> is meaningless — and that is the half worth carrying: an assignment of which layer moved is a
> detail to re-measure on the machine in front of you, not to inherit. `pdftoppm` honours the
> turn on both layers and is the better oracle for a rotated page.

### A mutation that survives every check because nothing reads the field

`/QuadPoints` says where a highlight's rectangles are. `save.rs` also writes an `/AP` appearance
stream that draws the wash from the same numbers — so with the appearance present, **no
renderer reads `/QuadPoints` at all**. Reordering every quad's corners changed no pixel, passed
the geometry round trip, and passed the ink check.

That is not a redundant field. A reader that regenerates the appearance — one that ignores
`/AP`, or any editor that re-renders after a change — uses those numbers. The check is to
**strip the `/AP` from the saved file and render it again**, which is `annot-probe --mode noap`:
what appears is then the renderer's own wash, from `/QuadPoints`.

> ⚠ **This entry named PDFKit as such a reader for a fortnight and that is false**, measured
> 2026-08-20 on macOS 26. Blanking the `/AP` key of a saved highlight with spaces — same file
> length, so every xref offset still holds, and PDFKit still finds and paints the annotation —
> changed what it draws: **43634 px over a 13.2 pt band with the appearance present, 33680 over
> 10.8 pt without it**. PDFKit reads ours when it is there. The mode is not weakened by this,
> because its real justification never needed a named reader: it is the only thing in this
> repository that reads `/QuadPoints` at all, and a mutation reordering every quad's corners
> survives without it. What the correction removes is the *reassurance* — nobody has yet shown a
> reader in the wild that regenerates our appearance, so `/QuadPoints` is written on the
> specification's authority rather than on a measurement of somebody rendering it.

Two details:

- The strip must **count what it removed and refuse if that is nothing**, or the mode silently
  becomes a second, slower copy of the mode it was added to complement.
- The pixel evidence for the corner order is real but thin: PDFium's generated wash covers
  28–36% of each quad for the conventional order (upper-left, upper-right, lower-left,
  lower-right) and 21–24% for the corners rotated by one. A check standing on a seven-point
  margin will one day pass for the wrong reason, so the order is asserted **on the bytes** —
  read `/QuadPoints` back and check the relationships — and the pixel measurement is what
  justifies which order is expected.

### A control refused by a different guard than the one it was written for

The mark writer refuses a page object that two page numbers share, because an annotation hangs
off the object and would appear on both. The control written beside it — "a mark on a shared
page *is* written when only one of the two numbers is kept" — never exercised that guard:
keeping one of two shared numbers is a deletion, and `unshared` refuses it first, with a
different message.

The test failed, which is the lucky case. Had the fixture been one where both refusals produced
the same outcome, it would have passed while proving nothing about the guard it named.

The working control is a document with a shared page **and a spare**: keep every page, mark the
spare, and the guard has to be scoped to *this mark's page* rather than to "this file contains
a shared page anywhere". Written the loose way, a reader could not highlight a perfectly
ordinary page because some other page in the file was malformed.

**And `--input` discards every `-f` beside it, so the command written down here to prevent that
did not.** `BUILD.md`'s step 11 carried `gh api -X PATCH .../releases/<id> -f
tag_name=vYY.M.MICRO --input body.json` from the day this entry was written, in the paragraph that
warns about exactly this failure. `--input` supplies the *whole* request body, so the `-f` never
reaches GitHub and the PATCH is a body-only one. Measured on `26.8.9`: the reply came straight back
as `"tag_name": "untagged-bb1e54625d56b97bbd57"`, from a command whose purpose was to stop that
happening. Put `tag_name` **inside the JSON file**; the repair is a second PATCH and it is only
cheap because the GraphQL query is run either side of the edit.

### The mutation that proves a guard is the one that performs the write it prevents

`a_merge_will_not_be_written_over_any_document_going_into_it` aims `write_merged` at
`testdata/links.pdf` as its own destination and expects a refusal. The mutation written to
prove that guard --- `merge: guard the destination against the source alone` --- deletes the
check for the incoming files. Under it the test does exactly what it exists to forbid: it
**merges four pages into `testdata/links.pdf` and writes the result over the fixture**.

It ran twice on 2026-08-24. `links.pdf` went from 8 pages to 12, then to 16, and its bytes
from 17,347 to 18,536. Every check in this repository stayed green throughout:

- The **mutation harness reported the mutation as correctly caught** --- which it was. It
  restores the *source file* it edited, knows nothing about `testdata/`, and has no reason to.
- The **test suite passed** on every later run, because the tests that read `links.pdf` derive
  what they expect from the file (`let theirs = page_count(&other)`), so a longer fixture is
  simply a longer document.
- The **`corpora` gate passed**, because the fixture still exists and is still classified.
- Nothing appeared in `git status`, because `testdata/` is generated and gitignored.

It was found by accident, in a printed line: the same test's `[OK]` said *"merged 4 pages with
8"*, then *"with 12"*, then *"with 16"*, across a session. A number that moves is the only
witness there was, and it was only visible because the check prints its operands.

**Three things to take from it.**

- **A test that proves a destructive operation is refused must aim at a copy.** The fix is
  four lines: copy both fixtures into the scratch directory and point everything at the
  copies, so a deleted guard destroys a scratch file. The general rule is stronger than the
  instance --- **any** test whose subject is "this must not be written" performs that write the
  moment the guard under test is mutated away, and a mutation harness is a machine for doing
  exactly that.
- **The same test should assert that nothing moved**, not only that a refusal came back. A
  guard that refuses *after* writing satisfies an `expect_err` perfectly, and that is the
  shape of what actually happened here.
- **A shared generated fixture is shared state between tests.** `testdata/` is regenerated by
  hand and gitignored, so a test that damages one has damaged every later run on that machine
  and left no trace a diff can show. `scripts/ci_fixtures.py` regenerates the runner-buildable
  set in seconds and is the repair; running it after a mutation session over any write path is
  cheap insurance.

The reason CI never saw any of this is that CI generates `testdata/` fresh on every run and
does not run the mutation harnesses at all --- so the machine that can do the damage is the
only machine that keeps it.

### `--only "text: "` runs every `context:` mutation too

**A repeated `--only` is not a union --- the last one wins, and the run says so quietly.**
`argparse` stores a single value, so `--only "a" --only "b" --only "c" --only "d"` runs whatever
matches `d`. The harness's own summary is honest about it (`all 1 mutations caught by the test
named for them`), and that line is easy to read as a report about the four you asked for: three
mutations were never run and nothing failed. Run one filter at a time, or widen it to a prefix
that covers them all, and check the count in the summary against how many you expected.

`mutate_rust.py --only <s>` matches `s.lower() in name.lower()`. `"text: "` is a substring of
`"context: "`, so a filter meant to select two mutations in `text.rs` selected six in
`search.rs` as well.

Harmless on its own — they all passed — but it cost two false diagnoses, because the run was
concurrent with editing:

- **Two mutation harnesses must not overlap.** The second one's control run compiles whatever
  the first has in the tree at that instant, and reports the first's in-flight mutation as
  *"the control run is not green"* with three unrelated tests named. That is indistinguishable
  from a genuinely broken suite.
- **Nor may a `cargo test` overlap one.** The same edit made a full-suite run go red in
  `search.rs`, in a module the session had not touched, minutes after it had been green.

The tell in both cases is that the failing tests are somewhere you have not been. Before
believing it, check whether a harness is running: `git status` will not show it — the harness
restores the file — and the failure is gone by the time you look.

### A menu item's greying is a snapshot, so a guard that moves without an edit is stale for ever

The macOS menu bar is AppKit's, and enablement crosses the boundary as a pushed map:
`menuEnablement(commands)` evaluates every guard once and `set_menu_enabled` sends the
answers. `App.svelte` pushed it from three places --- after an edit, after an open, and when
the updater's state moved.

`edit.highlightSelection`'s guard reads the *selection*, which moves through none of those. So
from the day it shipped, **the menu bar's Highlight selection was greyed at exactly the moment
there was something to highlight**, and became live only if the reader happened to make an
edit while the selection stood. Nothing failed: the command worked from the palette, worked
from the right-click menu, and the item was there in the menu with the right title and the
right shortcut beside it.

Three things made it invisible:

- **A greyed item cannot be pressed**, so there is no refusal to notice and no error to read.
  `runMenuCommand` refuses a disabled command, which is a second gate that never got the
  chance to fire.
- **The palette evaluates `enabled()` live**, so every check that drives a command through the
  palette --- which is all of them, including the window harness's sweep over every
  registered id --- sees the guard working perfectly.
- The `refreshMenu` docblock already said a missed call "leaves a menu item live that the
  palette would withhold", i.e. it had reasoned about the *stale-live* direction and named the
  cost as a stale grey. The direction that actually bit is the other one, and it is not
  cosmetic: a stale grey is a route the reader cannot take.

Found while adding `edit.removeMark`, whose guard reads whether a mark's note is open --- the
same shape, and it would have shipped dead in the menu for the same reason.

**Found by reading the three call sites, not by a check**, and worth saying because this
repository's own rule is that a claim about runtime behaviour belongs in an experiment. What
makes reading sufficient here is that the question is a closed one: `set_menu_enabled` is a
one-shot push, the three callers are all there are, and none of them fires when a selection
appears --- there is no fourth mechanism that could be re-querying. Nothing in the harness
covers `App.svelte`'s wiring, which is the honest gap: the fix is asserted by neither a gate
nor a window check, only by the diff.

The fix is to push from the frame loop and compare before sending: `refreshMenu` now
JSON-encodes the map and returns early when it matches what was last pushed, so twenty
closures run per frame and a message crosses the boundary only when an answer changed. The
failed-push path clears the memo, because remembering a push that did not land would withhold
the identical retry that would have corrected it.

**The general shape: an enablement that is *pushed* is a cache, and every guard reading state
that changes outside the push sites is wrong between them.** Enumerate what each guard reads,
not what each command does.

### An unguarded `invoke` for a command that is not registered ends the run, and the harness calls it SURVIVED

Written while adding a window check whose whole purpose is to notice a `generate_handler!`
list that forgot a name --- every layer under the command is tested somewhere, and all of it
passes with the command unreachable, so the reader is the one who finds out.

The check worked. The mutation that removes `annot_note` from the handler list was then
reported by `mutate_viewer.py` as:

```
0/1 caught, 1 survived
  SURVIVED: lib: leave the note command out of the handler list
expected 'the model takes a note through the command' to fail;
1 did: ['run completed    Command annot_note not found']
```

**The defect was detected and the verdict was wrong**, because a rejected `invoke` is a
rejected promise: an unguarded `await` on it walks out of the phase, past every check below
it, and out of the run. The named check never printed at all, so the harness --- which asks
whether *that name* went red --- could only answer no. What went red was the wrapper's own
line about the run, with no check name on it.

Two things follow, and the second is the one worth carrying:

- **Guard every backend call in a check with `try`/`catch` and turn the rejection into the
  named check's failure message.** Three lines, and the mutation then reports `[CAUGHT] -> 2
  red` --- the check that names the command, and the refusal control beneath it, which calls
  the same command.
- **A harness that keys on check names cannot see a failure that prevented the name from
  being printed.** That is the same shape as this file's entries on a crash producing no test
  results and a timeout read as "no result": absence and refutation are different answers, and
  a name-keyed verdict collapses them. The tell is a SURVIVED verdict whose evidence line
  quotes a failure that is obviously the mutation --- read the evidence, not the verdict.

### A page's own turn is not the view's, and a rectangle drawn by one was found by the other

A comment, a link and one of the reader's own marks all arrive as a rectangle in the page's
**display** space: points from the displayed page's top-left, after the file's `/Rotate` and
before any turn the reader or an edit added. Placing one on screen therefore needs both of
the turns still outstanding, and `Scroller.effectiveTurns` exists to add them --- its own
doc comment says it is *"the one place the two are added"*.

Eleven places did not ask it. Six turned rectangles by `this.turns`, the reader's rotation
alone: `commentUnder`, `anchorFor`, `topPtOf`, `linksOn`, `linkTopPt` and `linkAnchor`.
Four decided whether a vertical offset within a page was meaningful by `this.turns === 0`,
which is a per-document test of a per-page quantity: `goToDestination`, `position`, and the
two restores. One removed `-this.turns` from a size that had every turn in it. And a
twelfth, `displayedPage`, wrote the sum out by hand rather than calling the method.

So on a page turned with Rotate Right, a comment's icon was painted where the tile put it
and was clickable where it used to be. **The mark subsystem was right**, because it was
written after the page turn existed and asked `effectiveTurns` from the start --- which is
what made the measurement decisive:

```
turn=1 eff=1 viewTurns=0  painted=(730,120) -> comment -1  | lookedUp=(120,70) -> comment 7
                                            -> mark     3  |                    -> mark    -1
```

One rectangle, one page, two subsystems, disjoint answers. Nothing about PDFium is assumed
there: the mark path has window checks behind it, so the comment is the one that moved.

**The reason it survived 772 frontend tests and fourteen window corpora is that a page turn
and a view rotation are the same picture.** Every check that rotates rotates the *view*,
where the two numbers are equal, and every check with a comment in it leaves the page
upright. The defect needs both at once, and no fixture had both.

The fix is one primitive, `turnsOn(page)`, returning the effective turns and the document's
size; `viewQuadsOf` was changed to use it too, so the mark path stops being a second copy of
the same distinction. Four `this.turns` uses are left and all four are right: the status
report, the getter, and `rotateBy`'s own arithmetic --- the view's rotation, where the view's
rotation is what is meant.

### A size is learned once, so a page turned before it was seen keeps a transposed one

The quietest of the eleven, and the only one that does not correct itself. `learnGeometry`
takes a page's size from its extracted text and records it, and `TextCache.peek` answers in
**view** space --- which its own doc comment says includes the page's edit turn. Removing
only the view's rotation therefore leaves the page turn in:

```
LEARN turn=0 knows=true size={"width_pt":600,"height_pt":800}   <- control
LEARN turn=1 knows=true size={"width_pt":800,"height_pt":600}   <- a 600x800 page
```

A size is learned once, so the transposed one is the page's geometry for the life of the
document: its layout box, the fit, `displayedSize` applying the turn to an already-turned
size, and every rectangle divided by a width that is really a height. Reaching it needs a
page turned *before it has ever been on screen* --- Rotate All, or a rotate followed by a
scroll --- which is why nothing had.

### A single-entry cache is evicted by the grid scan that was about to test it

The links memo holds one page's rectangles, keyed by page and turn count. Both ends of that
key were mutated and both mutations **survived** a test that pressed a grid of points across
the page, rotated it, and pressed again.

The scan was the reason. It walks `y` from 0 to 800, and past the bottom of page 0 those
presses land on pages 1 and 2 --- each one a lookup that replaces the single cached entry. So
the poisoned page-0 entry was evicted by the tail of the very scan that produced it, and the
next scan started cold and recomputed correctly. The test could not have failed.

Two things it took to make both halves reachable, and they are different:

- **One press per turn, never off the page under test.** The reference points come from a
  separate throwaway viewer holding a mark, so the test still recomputes no geometry.
- **Turn, and turn back.** Reading at turn 1 after warming at 0 catches a *lookup* by the
  view's number, which hits when it should miss. Only returning to 0 catches a *store* by it,
  which leaves the turned rectangles under a key the untuned lookup matches. A one-way test
  catches exactly one of the two, and it is not obvious in advance which.

The general shape: **a cache with one slot makes any test of it order-dependent, and a
sweep is the worst possible order.** Sweeping is the instinct for "cover the page", and here
covering the page is what destroyed the evidence.

### Reading the code predicted four call sites, and there were eleven

`docs/PLAN.md` recorded this defect a day before it was fixed, and recorded it honestly ---
as read from the code, not measured, with the alternative explanation checked and the
experiment named as the next step. It was right that the defect was real. It was wrong about
its extent, and in the direction that matters: *"the fix is `effectiveTurns` at four call
sites"*.

Four is what a grep for `viewRect(..., this.turns, ...)` finds while looking at comments. It
missed the two link twins of the same call, both one screen further down; the four offset
guards, which are the same mistake written as `=== 0` instead of as an argument; and
`learnGeometry`, which is the same mistake with a minus sign. Eleven, plus one place that
had written the sum out by hand.

The estimate came from reading, and reading is what produced the undercount --- the same
mechanism as this file's entry on a claim about runtime behaviour belonging in an
experiment, applied to *scope* rather than to behaviour. The cheap corrective is mechanical
and takes one command: grep the whole file for the symbol, not for the shape of the call you
have in mind. `grep -n 'this\.turns' src/lib/viewer.ts` lists sixteen lines, and deciding
each one is right or wrong is a minute's work that no amount of careful reading around the
part you already suspect will substitute for.

### Removing the second copy is what made the differential unable to fail

The eight placement checks in `viewerturns.test.ts` compare a comment's found region and a
link's against a *mark's*, at each of the four turns. That was the right instrument for the
defect: the mark path was correct, the other two were not, and comparing them settled it
without the test recomputing any geometry the code computes.

Then the fix collapsed all three onto one primitive --- `viewQuadsOf` had held its own
correct copy of the same two lines, and leaving it there would have been the *two copies of a
distinction drift* trap kept deliberately. Correct, and it silently changed what the eight
checks can say. Measured rather than assumed, because the mutation was re-run afterwards on
the finished tree:

```
before the collapse:  place a page's rectangles under the view's turn alone -> 8 red
after  the collapse:  ... -> 2 red, and NOT the expected one
```

**A comparison between subsystems that share an implementation is true by construction.** A
fault in the primitive moves all three regions together, so all eight comparisons still
agree, and the only thing left red was the control beside them --- which happened to survive
because it asserts the region *moves* when the page turns, and a placement using the view's
number alone does not move at all.

So a differential needs an absolute half beside it, and the absolute half has to come from
somewhere other than the geometry under test. Here it is the page: **a rectangle on a page
must be found within that page**, bounded by the laid-out pitch read off the scroller. A
turn one quarter too far still moves the region, so the control cannot see it; it puts the
rectangle 130 pt off the bottom of a page the turn made 600 pt tall, which the bound can.

Two mutations now, and they are opposite failures of the same line: no turn at all, caught by
the control, and one turn too many, caught by the bound. Neither is caught by the eight
comparisons, which are worth keeping for what they *do* test --- that nobody re-inlines the
geometry into one of the three, which is exactly how this defect was born. The mutation that
proves that is the one that breaks only the link path: 4 red.

The general form is worth carrying past this file. **Deduplication changes what your tests
prove, in the direction of proving less**, and nothing goes red at the moment it happens ---
the suite that was passing goes on passing. If a check compares two implementations, merging
them is a reason to re-run the mutation that check was written for, not a tidy-up to do
afterwards.

### A probe that writes one colour cannot measure a mark drawn in another

`annot-probe --mode rule` renders a page before and after an underline is written and asks
where the new pixels are. Its first run:

```
Underline: 0 px in the top third, 0 in the middle, 0 in the bottom
[FAIL] the renderer drew a rule at all
```

Which reads, unambiguously, as PDFium ignoring our `/AP` and generating its own appearance
for `/Underline` — a real risk, a plausible diagnosis, and wrong. The appearance stream was
in the file and correct:

```
/GS0 gs 1 0.9 0.2 rg
60.32 717.07 253.33 0.918 re f
```

**`1 0.9 0.2` is yellow.** The probe writes one colour for every mark it makes, because for
its first year every mark it made was a highlight; the pixel classifier written beside the new
mode looked for red, because red is what the application sends for a line. Two constants, in
two places, that had never had to agree before.

The fix is not to correct the constant. It is to **derive the classifier from the value the
probe actually sent** — `rule_pixels` takes the target colour and matches near it, so the
instrument and the input cannot disagree by construction. Correcting the constant leaves the
next person free to change one of them.

Two things worth carrying:

- **A measurement that comes back zero has two explanations and only one of them is about the
  code under test.** The other is the instrument, and it is the cheaper one to rule out: the
  written file was one `zlib.decompress` away and settled it in a minute, against a diagnosis
  that would have sent somebody into PDFium's appearance-generation code.
- **A probe grows a second subject long after its constants were written.** Every constant in
  it that was "the colour" or "the size" is now an assumption about one subject, and nothing
  says which ones. Worth a sweep when a probe gains a mode rather than a fix per surprise.

### A `|` in the data split my own mutation in half, and the run reported a pass

Four mutations proved by hand in one shell loop, each written as `old|new` and split with
`${m%%|*}` and `${m##*|}`. Three landed. The fourth was:

```
MarkKind::Underline | MarkKind::StrikeOut => false,
```

which contains the delimiter, so `old` became `MarkKind::Underline ` — three matches in the
file — and the edit was refused. The loop printed the refusal and then **ran the tests
anyway**, on an unmutated tree, and reported `514 passed`. Read quickly, that is a mutation
that survived; read carefully, it is a mutation that never happened.

This repository already has three entries on mutations that never landed, and the mechanism is
new each time: `perl -pi` eating `$a`, `\Q…\E` not stopping interpolation, `grep -cF` counting
lines. This one is the delimiter appearing in the payload, which no amount of quoting fixes
because the split happens before quoting is relevant.

The habits that catch it, in order of cost:

- **Assert the edit landed before reading the result.** The loop did print `AssertionError: 3`
  and it scrolled past under three `[OK]` lines; a `continue` on failure would have said so
  once, loudly, instead of producing a fourth result that looks like the other three.
- **Prefer the harness to the loop.** `mutate_rust.py` takes the pair as two Python strings and
  has no delimiter at all, which is why the same mutation registered there works. The one-off
  loop exists to prove a mutation *before* registering it, and it is the least careful
  instrument in the repository.

### Three near-copies of a command made an existing mutation's anchor ambiguous

Adding Underline and Strike out beside Highlight in `appcommands.ts` produced three entries
differing in an id, a title and one string argument. The `anchors` gate went red:

```
appcommands: offer the highlight with nothing selected
anchor occurs 3x in src/lib/appcommands.ts, expected 1.
```

The gate exists for a mutation aimed at code that is *gone*; here it fired for code that had
been **duplicated**, which is the more interesting signal and was not what it was written for.
An ambiguous anchor is a reliable tell that a near-copy has just been made, and a near-copy is
exactly where a guard gets dropped without anything noticing.

Widening the anchor to include the id line is half the fix and the less important half. The
other half is that the *test* the mutation names walked one command, so the copies it
provoked were covered by nothing — this repository's *"a check bound to one caller covers only
that caller"* arriving through a gate rather than through a defect. The test now loops over all
three, and a second mutation aimed at the third entry is what says so.

There is a third mutation beside those two, and it is the one the ambiguity did *not* point
at: the entries differ by a string argument, so a copy that kept the guard and forgot to change
`"highlight"` to `"underline"` gives a reader a Strike out that highlights, with every check
that asks whether the command ran passing.

### JSON refuses `NaN`, which is what made an unchecked `f32` look safe

`Mark::color` is documented as *"Red, green and blue in 0..=1, as `/C` takes them"*, and
nothing made it so. The value arrives from the webview as three JSON numbers, `save.rs` writes
each with `format!`, and the appearance stream is a content stream — so a channel that is not
a number in the PDF sense is a **syntax error in a file tpdf wrote and signed its name to**.

The reason it reads as safe is that the obvious hostile inputs are already refused. JSON has no
`NaN` and no `Infinity` literal, and serde says so:

```
serde_json::from_str::<f32>("NaN")  -> Err
```

But `1e40` is perfectly good JSON, and the cast to `f32` is where it goes wrong:

```
1e40 as f32 = inf -> formatted "inf"
```

Three letters in the middle of a content stream. Measured in a throwaway crate in under a
minute, which is the whole argument for measuring: *"JSON cannot express infinity"* is true,
and the conclusion drawn from it — that an `f32` off the wire is finite — does not follow.

The fix is a clamp at the boundary where a wire value becomes a model value, and the shape of
it is worth copying:

- **Clamped, not refused.** A colour a fraction outside the range is what a slider produces,
  and every PDF reader clamps `/C` anyway; refusing would be stricter than the format.
- **A non-finite value has no clamped meaning**, so it is not clamped: `f32::NAN.clamp(0.0,
  1.0)` is `NaN`, and `f32::INFINITY.clamp(0.0, 1.0)` is 1.0 only by luck of the ordering. It
  becomes zero, which is the only total answer.
- **The test asserts finiteness separately from the range**, because that is the property
  `format!` actually needs and a range assertion on its own does not state it.

Generalises past colours: **any numeric field crossing a JSON boundary into a fixed-width
float can be non-finite even where the encoding forbids non-finite literals**, and any field
whose documented range is enforced by a doc comment is enforced by nothing. This repository
already records *"a rule you wrote down is not a rule you enforce"*; this is that rule meeting
a type conversion.

---
### A key handler is only as safe as the newest element inside it

`viewer.ts` has handled the reader's keys since the viewer existed: `n` and `p` turn the
page, Home and End reach the ends, Space and the arrows scroll, ⌘R turns the view, ⌘C
copies the selection. On 2026-08-18 every one of them was still firing while the reader
typed a note, because the note box added four days earlier is a `<textarea>` **inside the
viewer's own root**, and a key delivered to it bubbles to the root handler exactly as a key
pressed on the page does.

So typing the word "annotation" into a note turned the page twice and jumped to the start of
the document; the space bar scrolled the note out from under the box; and ⌘C wrote the page's
selected text over what the reader had just copied out of the field.

**The half that makes this worth an entry is that the codebase already held the correct
reasoning, in a comment, two files away, and it did not transfer.** `appcommands.ts` guards
⌘Z and ⌘⇧Z and nothing else, and says why:

> The `inTextField` guard is on these two and on nothing else here, and the asymmetry is
> deliberate rather than an oversight. Every other binding above is a chord no text field
> claims, so taking it from the find bar is what a reader wants.

That is right, and it is right *about the window handler*, whose bindings are all chords.
The viewer's handler holds the opposite half — the bare letters and the navigation keys, which
a text field claims all of. A rule stated with its reasoning still has to be re-derived for
the next surface, because the reasoning is what varies.

Two things generalise:

- **The question is not "does this handler have a guard" but "what is the newest focusable
  element under it".** A handler is written once and its subtree grows for years. The find
  bar was outside this root, so for months there was genuinely nothing to guard against, and
  the day that stopped being true nothing in the diff said so.
- **It is invisible in every instrument short of typing.** No test failed, no lint fired, and
  the two effects (the character appears *and* the page moves) read as one confusing bug
  rather than as a handler running twice.

The fix is one line at the top of the handler and a shared `inTextField` in `keys.ts` — one
definition of what a text field is, because the two handlers need it for opposite reasons and
two copies would be two chances to disagree.

---
### An event without the modifier fields a matcher tests reads as no match at all

The probe written to measure the leak above dispatched `{ key: "n", target: {...} }` at the
viewer's root and reported that `n`, `p`, Home and End were all safely ignored, while Space
and the arrows leaked. That split is not a fact about the code; it is the probe's own bug, and
it took the correct-looking result at face value for two rounds.

`keys.ts`'s `matches` tests every modifier **in both directions**, which is deliberate and
recorded in its own doc comment — a binding that does not ask for Shift must not match an event
holding it, or ⇧⌘G would also fire find-next:

```ts
if (event.shiftKey !== (binding.shift ?? false)) return false;
if (event.altKey !== (binding.alt ?? false)) return false;
```

An event object that simply omits those fields has `undefined !== false`, so **every**
`matches` call returns false. The keys that appeared guarded were the ones reached through
`matches`; the ones that leaked were the literal arms (`event.key === "ArrowDown"`) further
down the chain, which read `key` and nothing else. Adding
`shiftKey: false, altKey: false, metaKey: false, ctrlKey: false` turned four "guarded" keys
into four leaks, ⌘R included.

The general form: **a synthetic event is a partial object, and a matcher that tests a field
in both directions reads an absent field as a mismatch.** A real `KeyboardEvent` always
carries all four as booleans, so the production path never sees this and the harness always
does. The tell was the *pattern* of the result rather than any single reading — a guard that
happened to cover exactly the chorded keys and none of the literal ones is a suspiciously
tidy answer, and tidy is what to distrust when the code contains no such distinction.

---
### A probe copied from its neighbour inherits a starting point that may not apply

`nav.nextLink` and `nav.nextMark` are the same shape in `viewercheck.ts`'s command table:
put the viewer somewhere the command can be seen to move it from, read a focus, run the
command from the palette, read it again. The mark pair was written from the link pair and
went red on its first run, reporting `-1 -> -1`: a working command, measured as dead.

The two walks do not start from the same thing. A link walk starts from the **focused link**,
so `viewer.clearLinkFocus()` is a complete reset and the reader's scroll position is
irrelevant. A mark walk with no note open starts from **where the reader is looking** — that is
the property `stepAlong` exists for, so that "next" on page 400 means the next one there. The
previous probe in the table had left the reader below both marks the new probe plants near the
top of page 1, so "next mark" correctly found nothing.

Its sibling failed in the more misleading direction: `nav.previousMark` reported `-1 -> 4247`
and would have looked like a pass under a weaker predicate. Its `from` walks forward twice to
give Previous something to reach; both walks found nothing for the same reason, so the reading
it took as "before" was the empty state, and the command then stepped *backwards* from the
scroll position onto a mark that was genuinely there.

The fix is one line — `viewer.goToStart()` in both `from`s — and the lesson is in what the two
probes share on the page and not in the code: **when a probe's reset is copied, check what the
original was resetting.** A reset that names the right subject for one command can be silent
about the state the next one actually depends on, and the failure arrives as a defect report
against working code.

---
### A fallback is in the coordinate system of whoever wrote it

`RawPage::crop_pt` reads the page's visible box, and its fallback --- for a page whose crop
box PDFium will not report --- was:

```rust
normalised(ok != 0, [left, bottom, right, top])
    .unwrap_or([0.0, 0.0, self.width_pt(), self.height_pt()])
```

Which is wrong, and was wrong for months without a single symptom. `width_pt` and
`height_pt` are the page's **displayed** size, after `/Rotate`; a crop box is in the page's
**own** space, before it. On an unrotated page those are the same four numbers, so thirteen
of the fourteen corpora cannot tell them apart.

It surfaced when cropping gave the box a second consumer. `testdata/rotated-90.pdf` is
`/MediaBox [0 0 612 792]` with `/Rotate 90`, so the sheet is 612 by 792 and the displayed
page is 792 by 612. Reading the fallback and writing it back through `FPDFPage_SetCropBox`,
which reads page space, made PDFium intersect the two:

```text
FPDFPage_GetMediaBox -> [0, 0, 612, 792]   ok = true    the sheet
FPDFPage_GetCropBox  -> [0, 0, 0, 0]       ok = FALSE   there is no /CropBox
the fallback         -> [0, 0, 792, 612]                the DISPLAYED page
after SetCropBox     -> the page reports 612 x 612
```

A size that page never had, on a document nobody had cropped, from the code path whose
whole job was to leave it alone.

**The half worth keeping is why it was invisible.** The fallback had exactly one consumer,
`origin_pt`, which takes the *corner* --- and the corner of a page-space box and of a
display-space one are both `(0, 0)`. A fallback is written in the coordinate system its
first caller happens to need, and the second caller is where that stops being harmless.
So: **a default value is a value, and it inherits every frame, unit and convention of the
function it was written inside.** When a second consumer arrives, re-derive the default
rather than the call.

Two further notes, because the first diagnosis of this was wrong and recorded as a trap
before being checked:

- **PDFium behaved correctly throughout.** `FPDFPage_GetCropBox` returns *false* for a page
  with no `/CropBox` --- there is none to report --- and answers in page space when there is
  one (`links-cropped.pdf`'s `[50 50 545 742]`). The first write-up of this entry blamed the
  library for "answering with the displayed rectangle", which is what the *fallback* did.
  The evidence that settled it was reading `ok` separately from the box, on a page nothing
  had yet written to; every earlier reading had been taken through a code path that had
  already set a crop.
- The fix is `/CropBox` **intersected** with `/MediaBox` --- §14.11.2's own rule, and the one
  `pagetree::displayed_page` already applied to the same two boxes through `lopdf`. Two
  libraries, one rule, stated in both places, because a rectangle measured against one page
  and drawn on another is the failure this whole family of entries is about.

### Two handles to one cached page are aliases, and a reading taken after a change describes the change

`RawDocument` caches four loaded pages, so `page(3)` twice hands back two `RawPage` values
wrapping **one** `FPDF_PAGE`. They are not copies of a page; they are two views of it.

That has a design consequence and a measurement consequence, and both were paid for.

The design one: a crop set on a handle stays set, so a request that simply took the cached
page would see whichever crop the *previous* request left — a tile of page 3 rendered
cropped because a text extraction two seconds earlier asked for it that way. The fix is not
to write that rule down for callers to follow. `RawDocument::page` **restores the file's own
box** and `page_cropped(index, Some(box))` is the explicit opt-in, so the dangerous state is
unreachable rather than merely discouraged.

The measurement one is sharper, because it produced a number that cannot exist. A probe
comparing ink density before and after a crop read the page's size *after* cropping through
a second handle:

```rust
let page = doc.page(index)?;               // the uncropped page
let before = inked(&tile(&page));
let cropped = doc.page_cropped(index, Some(box_pt))?;   // same handle
let after = inked(&tile(&cropped));
let page_px = page.width_pt() * page.height_pt();       // <- the CROPPED size
```

`page` and `cropped` alias, so the denominator for the uncropped count was the cropped
page's. The probe reported a density of **1.23** — a fraction of a page's pixels that are
ink, larger than one. That impossibility is the only reason it was caught rather than being
read as a marginal improvement. **Take every reading of the old state before creating the
new one**, and treat a normalised quantity outside its range as an instrument fault rather
than a surprising result.

---
### A denominator that is constant in one dimension cannot compare areas

The same probe's next guard was to skip pages with nothing to crop, so that a page whose
ink already reaches its edges could not report a failure for doing the right thing. It
compared the two renders' **pixel counts**:

```rust
if crop_px >= page_px * 0.98 { skip("the content box is the page") }
```

Both renders are 200 pixels wide whatever shape the page is — the height follows from the
aspect ratio — so `crop_px / page_px` is the ratio of two *aspect ratios* and not of two
areas. A crop keeping a fifth of the sheet came out as "247% of it", every corpus skipped,
and the two rows that had something to say were silent. The output looked orderly: twelve
`[SKIP]` lines with a stated reason, which is exactly what a careful harness prints.

The rule: **when a measurement is normalised by a value held constant on one axis, it can
compare shapes and not sizes.** Compare in the units the question is asked in — here
points, from the boxes themselves — and keep the pixel counts for what they are, which is
ink per rendered pixel.

The tell was available and was not read for a round: a "percentage of the page" above 100.
Two guards in one probe, on the same afternoon, both produced an out-of-range fraction, and
both were arithmetic in the harness rather than anything about the subject.

---
### Two tests sharing a name make a mutation harness's two counts disagree

`crop.test.ts` had `moves nothing for a page nobody cropped` twice — once under
`describe("intoCrop")` and once under `describe("outOfCrop")`. Both are good tests and
vitest runs both; the full name differs by its describe block and nothing complains.

`scripts/mutate_frontend.py` reads failing test **names into a set** and cross-checks that
count against the summary line's own arithmetic. A mutation that reddened five tests
therefore reported:

```text
[FAIL] crop: give an uncropped page a corner that is not the origin:
       4 failing test lines but the summary says 5 -- this harness cannot read its own output
```

Which is the guard working exactly as designed — it refused the run rather than reporting a
survivor — and it took a manual re-run of the mutation to see that the fault was two
identical `it(...)` strings rather than anything about the code.

Worth knowing in both directions. **Naming two tests the same thing is not free**, even
where the runner tolerates it: any tool that keys on the name alone silently merges them.
And a harness whose two counts disagree should be believed about the disagreement and not
about its cause — the message names the symptom correctly and says nothing about duplicate
names, because it cannot know.
### A rename over a mapped file succeeds, and the mapping goes on serving the file that is gone

Saving over the document a reader has open means replacing a file that a worker process has
memory-mapped, and the question of whether the document has to be closed first has an
answer that reads as reassuring and is the dangerous one.

Measured on this machine, with a `MAP_SHARED` mapping held across the rename:

```text
[before] mapping head: b'OLDOLD'
[rename] ok
[after ] mapping head: b'OLDOLD'   <- the mapping, after the rename
[after ] path reads  : b'NEWNEW'   <- the same path, read fresh
[after ] fd reads    : b'OLDOLD'   <- and the descriptor with it
```

Nothing fails. `rename` replaces a directory entry, not an inode, so the old inode stays
alive for as long as anything holds it, and the mapping keeps serving it. There is no
SIGBUS --- that is the *truncation* case, which has its own entry above --- and no error
anywhere for a caller to notice.

**So the failure is a reader scrolling a document that disagrees with their own file, with
everything looking right.** The save reports success, the file on disk holds the edits, and
every tile, every text extraction and every search answer still comes from the document as
it was before the save, until it happens to be reopened. A crash is loud and a refusal is
loud; this is neither.

**Windows fails the other way, and that asymmetry is the argument for one order rather than
two.** A file with an open section mapping cannot be replaced there, so the rename is
refused outright --- visibly, immediately, and on the platform where nobody would have gone
looking. Closing the document before the rename is correct on both, and it is not a Windows
workaround: it is what macOS needs too, and macOS is the platform that will not tell you.

What the code does with that is close the *shape* rather than remember the rule.
`save.rs`'s single atomic write is two functions --- `stage_in_place` writes the sibling
temporary file, `commit_in_place` renames it --- so there is a place for the close to go and
no way to spell the write without one. `lib.rs`'s `save_document` is the only caller and
holds the order. A mutation that puts the save in place during the staging is in
`scripts/mutate_rust.py`, and it is caught by the test that asserts the source is untouched
until the commit.
### A GUI process has no stderr, and every Windows check launched the app from a shell

Reported 2026-08-19, on the first install of tpdf on Windows: dragging a PDF into the window
put *"this process has no stderr to share"* in the corner and opened nothing. Not a document
that failed --- **no document could be opened at all**, by any route: drag, double-click,
⌘O, the recent list.

The message is ours, from `sandbox_win.rs`. A contained worker is spawned with
`STARTF_USESTDHANDLES`, which makes the child take **all three** standard handles from the
parent's `STARTUPINFO`, so the parent has to supply a stderr; it supplied its own, via
`GetStdHandle(STD_ERROR_HANDLE)`, and treated a null answer as an error.

**A null answer is the normal case for the shipped application.** `main.rs` carries
`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` --- which is correct and
must stay, since without it every launch flashes a console window --- and a GUI-subsystem
process gets no console, so it has no standard handles. `GetStdHandle` returns null, the
spawn refused, and since the viewer's own render path goes through a worker, the application
could not open a file.

**Why nothing caught it, and this is the durable half --- and the first answer written here
was wrong.** The obvious explanation is that the harnesses launch the exe from a shell and a
shell has a console. That was recorded, offered to the reporter as a control, and
**falsified in about a minute**: `./tpdf.exe` from a PowerShell prompt in the install
directory failed identically. A GUI-subsystem process is not attached to the console of
whatever started it, so a terminal buys nothing.

The real reason is one keyword argument. `scripts/viewer_check.py` starts the application
with `subprocess.Popen(..., stdout=subprocess.PIPE, stderr=subprocess.PIPE)`, because the
check's whole transcript is what the app prints. Python implements that on Windows by
setting `STARTF_USESTDHANDLES` and handing the child pipe handles --- so **the harness
creates the stderr whose absence is the bug**, for no reason connected to what it is
checking. `open_check.py` and `session_check.py` do the same, for the same reason.

So the harnesses were not unlucky, and they were not merely running in a different
environment: **the instrument's own reading mechanism supplied the missing precondition.**
That is a sharper thing than "checks run differently from users", and it generalises badly ---
any harness that captures a program's output has, by that act, given it a valid stdout and
stderr, and can never observe what the program does without them. Nothing in this repository
can reach the case; a person double-clicking the installer is the only instrument that has.

Read alongside the entry about a bundled app that finds its library in the dev tree. Same
family, one layer up: what a check runs *inside* is part of what it is checking, and the
difference is invisible in every artifact the check produces.

**The fix is a fallback, not a refusal.** A parent with no stderr hands the child a write
handle to `NUL`, opened once and never closed, so the child always has three valid handles.
The cost is real and is the right trade: a worker's dying message is discarded when there is
no console to receive it, where before the application refused to start. And the decision
moved out of the FFI call into `stderr_for(inherited)`, a seam that takes the handle as an
argument --- inline it was reachable by no test, since exercising it would have meant changing
the real `STD_ERROR_HANDLE` out from under every other test in the binary.

**The three tests are Windows-only and their first real run is CI.** They cannot be mutation
proved from a Mac, which is the same gap `scripts/check_windows.py` exists for and does not
close: a type-check is not a test, and a wrong *value* passes it.
### A mutation that inserts rather than moves runs the code twice, and the second run overwrites the first

`drag.ts` tears a drag down --- clears its state, removes its two listeners,
releases the capture --- and only then tells the target it is over. That ordering
is deliberate: a target whose `end` starts another drag must not find a
half-registered one in its way. The mutation written to prove it was an
insertion, anchored on the first line of the teardown and prepending the call.

That does not move the call. It **adds** one, and the original stays at the
bottom of the function. So `end` ran twice: once with the drag still live, once
after the teardown. The check written for this recorded what it saw from inside
`end` --- and the second call overwrote the first's recording with the correct
values, so it passed. Three unrelated checks went red instead, because a target
told twice records two endings.

The harness reported it exactly right: *"3 red, but NOT the expected one"*. A
mutation that lands as something other than what its name says is the family
`AGENTS.md` records under *three ways a mutation lies to you*, and this is a
fourth mechanism for it --- not an edit that failed to apply, but one that applied
and meant something else.

**The fix is to anchor the whole tail**, so the replacement is the same lines in
a different order and nothing is duplicated. The general rule: when a mutation is
a **reordering**, its `before` has to span every line being reordered. An anchor
on the first line alone can only ever insert.

### An escape sequence written into a mutation table through a shell never arrives as an escape

Every multi-line anchor in `scripts/mutate_*.py` is a Python source literal
holding a backslash followed by `n`, two characters. Writing one from a shell
heredoc failed four times in one session, in three different ways, and each
failure produced a *different* wrong outcome:

  * a doubled backslash inside a quoted heredoc arrived as a real newline, so the
    mutation table became a Python file with an unterminated string literal ---
    caught, at least, by `python -c "import ast; ast.parse(...)"`.
  * a backtick inside a quoted heredoc was treated as command substitution, so
    `bash` reported *"unexpected EOF while looking for matching"* and refused the
    whole command. Nothing was written and nothing said which line was at fault.
    Quoting the heredoc delimiter did not prevent it, which is the surprising
    half: something between here and `bash` is expanding the string first.
  * the same doubled backslash in an argument to a small edit helper produced a
    pattern that matched zero times, which the helper refused --- and the refusal
    scrolled past while the next command in the chain built and ran happily
    against the unmutated tree, printing a header that said `MUTATED`.

The third is the dangerous one and is the trap this repository already records
about a verification chained after a failed edit. The other two are loud.

**Build the characters in the script, never in the command:**

```python
NL = chr(92) + "n"     # the two source characters a Python literal needs
anchor = "    const x = 1;" + NL + "    const y = 2;"
```

`chr(92)` cannot be eaten by any layer between here and the interpreter. The same
applies to a backtick, `chr(96)`. And for a whole file rather than a fragment,
prefer the editor over a heredoc: the test file that finally landed in this
increment was written with an editor tool after three shell attempts, and the
shell was never going to work.

### A difference assertion is satisfied by any difference, including the one the defect produces

The box's coordinate inverse was first checked like this: draw the same screen
rectangle on an unturned page and on a quarter-turned one, and assert the two
answers differ. The reasoning was that a viewer reporting *laid-out* coordinates
--- the defect --- would report the same four numbers for both.

It would not. A 600x800 page turned a quarter lays out 800 wide, so it is fitted
at a different zoom, so the same client drag is a different number of points on
it. The defect produces two different answers too, and the check passed under it.
The mutation said so: caught by two other tests in the same file, not by the one
written for it.

**A difference is not a measurement unless the operands make it one** --- which
this repository already records about benchmarks, arriving here in a coordinate
test. The repair is to assert something only the correct answer can satisfy. Here
that is the **corner**: the same screen drag near the top-left is a different
corner of the *sheet* at each turn, walking round it as top-left, bottom-left,
bottom-right, top-right. A viewer that skips the inverse reports the top-left
four times, whatever the zoom does.

The same asymmetry bit the size checks a few lines above. `MIN_BOX` is in
**points**, a drag is in **client pixels**, and the zoom sits between them --- so a
test that drags `MIN_BOX - 1` pixels is really a test of whatever zoom the
fixture happened to pick. Those moved to a press with no movement at all, and the
bound itself is tested in one unit, on both sides, in `markband.test.ts`.

### A probe reading one edge of a box cannot see a mutation that clips the other three

`save.rs` insets a box's stroke path by half the stroke width, because a stroke
straddles its path and the appearance stream's `/BBox` clips whatever falls
outside. `annot-probe --mode outline` was given a thickness reading to prove it:
one pixel column at the box's horizontal centre, over the top quarter of it.

The mutation written for it --- `let inset = 0.0;` --- changed nothing the probe
could see. 5 px before, 5 px after. The reason is that `outline_path` computes
the size from the constant and the origin from the variable:

```rust
[quad[0] + inset, quad[1] + inset,
 (quad[2] - quad[0]) - OUTLINE_WIDTH, (quad[3] - quad[1]) - OUTLINE_WIDTH]
```

so zeroing `inset` moves the origin back to the corner while leaving the
rectangle a stroke-width smaller. Two edges are then clipped in half and the
other two sit a stroke-width *inside* where they belong. The probe was reading
one of the two that had moved inward.

Two things came out of it. The probe now reads **both** the top and the bottom
edge and reports the thinner --- a defect that clips one edge is not less of a
defect than one that clips four. And the honest mutation, which removes the inset
from the path entirely, halves the reading from 6 px to 3 and takes a fifth of
the frame's ink with it.

Also worth keeping: the first write-up of this said pixels could *not* see the
inset at all, on the argument that a `/BBox` clip leaves no ink outside the quad
and therefore nothing to count. True, and beside the point --- it removes ink from
inside. One run settled it, which is the standing rule about a claim regarding
runtime behaviour belonging in an experiment rather than in a document.

### One predicate answering three questions is right until a second kind makes them disagree

`save.rs` had `is_note`, and it decided three things: whether to write `/Name` and
`/Open`, whether to skip `/QuadPoints`, and whether to write an appearance stream
at all. That was correct, and it was correct only because the comment bubble was
the one kind for which all three answers happened to coincide.

A box splits them. It also skips `/QuadPoints` --- `/Square` is not a text-markup
subtype --- and it very much needs an `/AP`, because nothing synthesises a
rectangle and a `/Square` with no appearance is an annotation Acrobat draws as
nothing. So the three questions are now `is_note`, `is_text_markup` and
`ink(kind) == Ink::None`, and each has one caller.

The test moved with them, and that is the part worth reading. It used to assert
"the comment has no quads **and** no `/AP`" as one block; a box makes those two
lines disagree about the same kind, which is what turns one assertion into two.
Before the box existed no test could have told which of the three properties it
was checking, because one predicate answered for all of them.

The same increment replaced `is_wash` and `is_note` with a single exhaustive
`ink(kind) -> Ink` returning `Wash`, `Line`, `Outline` or `None`. Three booleans
for five kinds is where copies of a distinction begin to drift, and the value the
writer actually needs is one: it decides the geometry, the blend mode and both
opacities together, and those four have never been independent.

### An Escape ordering that no reachable input can distinguish

The viewer's Escape ladder dismisses the innermost thing first: an armed drawing
tool, then an open comment, then a focused link, then the selection. The comment
beside it said the tool had to go first or a reader with a note open would need
two presses to leave the mode --- and a mutation that swapped the first two
survived, because that state cannot arise. `armDraw` closes both note boxes, and
a press with a tool armed is intercepted before anything can open one.

So the ordering is defensive rather than load-bearing, and the comment claiming
otherwise was the defect. It has been corrected in place and the mutation
removed, since a mutation nothing can kill is a permanent red line that trains
the reader to skip the report.

The ordering itself stays. It costs one comparison, it is the order that remains
correct if a later change does make the two co-exist, and the failure it guards
against --- a reader stuck in a mode they cannot see --- is the one thing the
one-shot design exists to rule out. Read alongside *an unreachable guard is worth
keeping if the type can carry it instead*: same conclusion, different reason. The
type cannot carry this one, so the comment has to carry it honestly instead.
### The sweep shelled out to `pkill`, which is not a program on Windows

`viewer_sweep.py` kills any leftover tpdf before each corpus, because a leftover
window occludes the next one and WebKit suspends an occluded page. That line was
`subprocess.run(["pkill", ...], check=False)`, and `check=False` swallows a
non-zero exit --- not a `FileNotFoundError`. So on Windows the sweep died on its
first corpus with a traceback, having printed no table and no verdict, and the
shell reported **exit 0** because the traceback goes to stderr.

The Windows leftover is also worse than an occlusion, and that is what sent the
first run looking in the wrong place. `tauri-plugin-single-instance` makes a
second process forward its argv to the first and exit, so a stray `tauri dev`
from earlier in the session swallowed the launch entirely: `viewer_check.py`
printed one line --- *"the app process could not be read at all (0 samples, 0
modules seen)"* --- which reads as a broken module probe rather than as an app
that never started. Three stray debug processes were holding it.

`taskkill /F /IM tpdf.exe` on Windows, `pkill -f` elsewhere, and the whole thing
wrapped in `except OSError` so a machine with neither tool degrades to a slow run
rather than no run.

**And it was in two files.** `mutate_viewer.py` carried the identical line for
the identical reason --- its own comment says a leftover window is what made a
mutation run hang for fourteen minutes at 0% CPU --- and died the same way, before
its first mutation, with the same exit 0. Both are fixed; the second was found
only by running it, an hour after the first, which is the argument for running
every harness on the platform rather than reading them. The locale-codec entry
below is the same story: one `text=True` fixed, a second found in the other file
by looking for it once the first was known.

Read alongside *single-instance turns a stray process into a launch that succeeds
and does nothing*, which is the same mechanism seen from the application's side.

### `subprocess.run(text=True)` decodes with the locale codec, and the multilingual corpus is the one that breaks it

Six corpora into a thirteen-corpus sweep, on `multilingual.pdf` --- CJK, Arabic,
a decomposed accent and a code point above the BMP --- the run died with:

```text
UnicodeDecodeError: 'charmap' codec can't decode byte 0x81 ... cp1252.py
TypeError: unsupported operand type(s) for +: 'NoneType' and 'str'
```

The first line is the cause and the second is what the reader sees, because the
decode raised inside `subprocess`'s own reader thread. That left `result.stdout`
as `None`, so the failure surfaced on the next line as an arithmetic error
between a None and a string, mentioning no encoding at all.

`text=True` means "decode", not "decode as UTF-8": the codec is
`locale.getpreferredencoding()`, which is **cp1252** on this machine. Pass
`encoding="utf-8", errors="replace"` explicitly. `mutate_frontend.py` already
carried the same fix for vitest's output and its own comment says why; this is
the second script in the repository to need it, and the first one's lesson had
not been generalised.

Worth noting which corpus found it. Twelve of the thirteen are Latin text and
would never have produced a byte cp1252 refuses, so this is a defect that only
the fixture written to hold non-Latin text can reach --- and it sat in a script
that had presumably run cleanly on macOS for months, where the preferred encoding
is UTF-8.

### There was no check on the overlay at all, and that is why a reader found the underline defect

Every mark tpdf draws is drawn twice: once by `paintMarks` onto the overlay
canvas while the document is open, and once by PDFium from the appearance stream
`save.rs` writes, after the file has been saved and reopened. `annot-probe`
measures the second in pixels --- `--mode ink` for a wash, `--mode rule` for a
line, `--mode outline` for a frame --- and until 2026-08-19 **nothing measured
the first**.

So when `paintMarks` filled every mark across its whole quad in one colour, the
saved file stayed correct and only the screen was wrong. A reader chose "Underline
selection" from the right-click menu and got a yellow wash. The mark then changed
under them when they saved and reopened, which is a worse failure than either
half alone: neither renderer could be trusted from what the other one showed.

`overlayInkChecks` closes it. Two readings per kind --- the fraction of the mark's
own rectangle carrying ink, and the fraction of its inner half --- which
discriminate all five without knowing anything about where a band sits:

| kind | rectangle | middle |
|---|---|---|
| highlight | high | high |
| underline | low | **empty** |
| strikeout | low | **full** |
| square | low | **empty**, with ink on the edges |
| note | moderate | drawn |

Underline against strikeout is the pair the shipped defect got wrong, and the two
centres are what tell them apart. Square against underline is the pair the
rectangle fraction cannot separate --- 8% against 11%, far too close to assert ---
and the left edge is what does: both leave the centre clear, and only the box has
a side.

**Two of the five checks failed on a correct painter before the observable was
right, and in both cases the bound was not the problem.** It sampled the inner *half* of the rectangle and
wanted most of it inked; a rule is 7% of the quad's height, so inside a box half
that height it covers about 14% of the area, and the reading came back 17%. The
repair was to change the observable rather than the threshold --- at the dead
centre a rule reads full and an underline reads empty, which is the whole
discrimination in one number. A bound loosened to accommodate 17% would have
admitted a painter drawing almost anything through the middle.

The box failed next, on the reading meant to separate it from an underline: the
fraction of its left edge that carries ink. A stroke is 1.5 points wide inside a
sample five per cent of the mark's width, so a correct box reads **7%** --- and
the only repair available on that observable is a threshold two per cent above an
underline's zero, which is a check held together by the zoom the fixture happened
to pick. The robust form of the same question is a **count**: an underline has
ink on one of its four sides, a rule through the centre on two, a frame on four.
No bound, no dependence on the stroke's width in device pixels, and the same
discrimination.

Both repairs are the same move --- change what is measured rather than how much of
it is demanded --- and both were available only because the check had a failing
case in front of it. A first draft written to pass would have chosen neither.

**Every reading is relative to the anchor the viewer supplies**, not to a
rectangle the check computes. That is what makes the phase work on `rotated-90`,
where an underline is drawn down the side of the screen: anything keyed on "the
bottom band" would sample the wrong strip there, and skipping rotated corpora
would skip the one that most needs checking. It is also the drift rule this
repository states about a second implementation of the same turn.

The control comes first and is not decoration: "the middle is empty" means
nothing unless an untouched overlay reads as empty too, and a canvas the harness
never cleared would read as inked everywhere and pass three of the five by
accident.
### A feature can be inert in the application while three layers of tests pass

The box tool was finished, mutation-proved and green everywhere, and it did
nothing. `onDrawn` was added to `ViewerOptions`, `Viewer` fired it at the end of
every committed drag, and the object literal in `App.svelte` that constructs the
viewer never gained the key. Arming worked, the dashed preview followed the
pointer, and letting go reached no model.

**Nothing could see it, and the three things that might have are worth naming:**

  * `viewerdraw.test.ts` constructs its own viewer and supplies its own
    `onDrawn`, so it covers the viewer's half completely --- including that the
    callback fires with the right page id and the right quad.
  * `viewer_check.py` drives `edit.drawBox` from the palette in a real window and
    reads a recorder, so it covers the command's half.
  * `appcommands.test.ts` sweeps every registered command and asserts it reaches
    an action, which `drawBox` did.

The seam between them is one object literal in a `.svelte` file that no unit test
imports and no harness constructs. **Every callback on `ViewerOptions` is
optional by design** --- the check harness builds a viewer with none of them ---
so a missing key is not a type error either. There was no layer at which this
could fail.

The immediate cause was a batched edit whose first pattern did not match: the
helper refused the whole batch, and the re-application afterwards covered only
the part that had been diagnosed. That is the trap about a verification chained
after a failed edit, and it is the reason the *class* is worth a gate rather than
a resolution to be careful.

`scripts/check_viewer_wiring.py` diffs the declared callbacks against the wired
ones, both directions, refusing an empty scan on either side. Proved by mutation
in four directions before being trusted --- dropping the wiring, renaming a wired
key, an exemption naming nothing (a `[WARN]`, because an allowlist that stops
applying rots into a blanket permission), and the control.

**It found a second one on its first run.** `onNavigate` exists so a Back and
Forward affordance can be re-enabled after a jump, and nothing consumes it: both
commands are guarded on `withDocument` alone, so neither greys when there is
nowhere to go. It is the one entry in the exemption table, with that written
down, and wiring it is the same piece of work as making them grey. A spot fix for
`onDrawn` would have left it exactly as it was.

The general shape, worth carrying to any two modules joined by an optional
interface: **the layer that composes is the layer nothing tests**, because each
side is testable alone and the composition is a literal rather than a function.
Where the composition can be enumerated from the type, enumerate it.
### A relative forward-slash path is not an executable, and `cwd` makes every other argument in the list work

Every probe runner in `mutate_viewer.py` named its binary as
`src-tauri/target/release/examples/crop-probe`, and on Windows that raises
`FileNotFoundError: [WinError 2] The system cannot find the file specified` for a
file that is plainly there. `CreateProcess` will not resolve a relative path
written with forward slashes, and it wants the extension. All five probe runners
had been dead on this platform from the day they were written.

**What makes it invisible is that `cwd=ROOT` is set on the same call.** The fixture
paths, the `--lib vendor/pdfium/bin`, every other member of the argv list resolves
correctly against it --- only the executable is resolved by a different rule, so
reading the list tells you nothing is wrong with it. `BUILD.md` records exactly this
against `viewer_check.py`'s app path, and it arrived here anyway, in a harness
written afterwards.

The failure shape is the reassuring one: the run dies on the **first baseline it
reaches**, before any mutation, so nothing is ever reported as SURVIVED and the
summary a reader is looking for simply never appears.

**And I hid it from myself for one round.** The run was backgrounded as `nohup
python ... > "$TMPDIR/log" 2>&1; echo "exit=$?" >> "$TMPDIR/log"`, so the traceback
went into that file while the harness's captured output held three lines and **exit
0** --- the exit code of the `echo`. A wrapper that appends its own status to the log
it is diverting reports on itself, not on the job. Let the job's exit code be the
command's, or read the log before believing the code.

`probe_exe()` builds the path from `ROOT` and appends `.exe` on Windows.

### The gate guarding the anchors reads the file differently from the harness that uses them

`scripts/gates.py`'s `anchors` gate asserts that every mutation's search string
occurs exactly once in the file it names --- every anchor in every table, green.
`mutate_viewer.py`
then refused its own first mutation with *"its anchor appears 0 times"*, in the same
file, on the same tree, in the same minute.

Both are right about what they read. The gate reads with `Path.read_text()`, whose
universal-newline translation turns a CRLF checkout into "\n" before counting, so a
multi-line anchor written with "\n" matches. The harness reads **bytes** and decodes
UTF-8, which does not translate, so on Windows every multi-line anchor in its table
matches zero times. `mutate_frontend.py` and `mutate_rust.py` were given the
normalisation on 2026-07-30 and this harness was not.

**The gate is the more forgiving reader, and that is the direction that hurts.** It
is green precisely where the harness is dead, and it was written to be the early
warning for this class --- an anchor that has drifted, a killed harness's leftover
edit. It cannot warn about a difference it does not share.

Two habits fall out. **A guard and the thing it guards must read their subject the
same way**, or the guard is describing a different file. And when two readers of one
file disagree about whether a string is present, the answer is a property of **how
each one reads**, not of the file --- neither reading is the mistake, the pair is.

Found only because the run stopped before it, on the trap above; both were fixed the
same day, and only then could this table run here at all. The fix normalises "\r\n" to
"\n" for matching and puts the file's own convention back, so the mutation stays the
only difference on disk.
### A guard that answers by refusing the whole run turns two blocked mutations into 178

`mutate_rust.py` validates every mutation's `expect` against libtest's own list
before it starts, because a mutation naming a test that does not exist cannot go
red and reports SURVIVED --- which reads as a gap in the suite rather than as a
broken mutation. The guard is good and it has fired usefully four times.

On Windows it refused the entire table. `menu.rs` and `keylayout.rs` are macOS-only
(a menu bar and Carbon), so `cargo test` never compiles them, the two tests they
name genuinely do not exist here, and the guard said so precisely --- naming both
mutations, quoting both test names, giving the right reason --- and returned 1
before running any of the other 176.

**The refusal was correct and total, and those are different properties.** Two
mutations could not run; 178 did not. A guard whose only vocabulary is "refuse the
run" cannot express "this row is not applicable here", so every local fault it
finds becomes a global one --- and the more accurate its diagnosis, the more
convincing the wrong outcome looks.

**Nothing said so for two days**, because the table had not been run on Windows
since `menu::` was added on 2026-08-17. `BUILD.md` still carried *"Both run on
Windows as of 2026-07-30 --- 22/22 and 75/75"*, which was true when measured and
had quietly expired: a dated measurement is evidence about a date, and a table that
grows can leave the platform it was measured on behind.

The fix is a declared scope, not an inferred one. `Mutation.only_on` is set on
those two, they print `[SKIP] ... macos only, and this is windows`, and the count
rides on the final verdict so a partial run cannot read as a whole one. **A
mutation with no `only_on` still refuses**, which is the property that had to
survive: skipping on a name the harness merely failed to see is exactly how a
mutation stops being able to fail. Proved by control before the fix was trusted ---
pointing a non-scoped mutation at a test that does not exist still refuses and
exits 1, and a selection consisting only of out-of-platform mutations refuses too
rather than reporting a vacuous pass.
### A checklist step nothing can perform, and a comment promising a mechanism that does not exist

Two shapes of the same defect, found together on 2026-08-19 because a reader hit
the Windows no-console bug and could not tell which version they were running.

**`BUILD.md`'s release checklist, step 12, has said since the updater landed:
"quit, reopen, and Help/About or the palette reports the new version".** There was
no Help menu, no About, and no command that reports a version. The step could
never have been carried out, by anybody, from the day it was written. Nothing can
go red for a checklist: no runner runs it, no gate covers it, and a step that has
never been executed reads exactly like one that keeps passing --- which is this
repository's oldest lesson arriving in the document that schedules the checks
rather than in the checks.

**`update.ts` carried the matching half in a code comment**: `updateLabel` returns
null for `current`, and the comment beside it explained that "a reader who asked
explicitly gets the answer from the palette instead". The palette command existed.
Pressing it when you were up to date did visibly nothing, because the state it
lands on is exactly the one the header stays silent about. The comment described a
mechanism, the mechanism was absent, and prose describing something that does not
exist is indistinguishable from prose describing something that does.

What the pair have in common is worth more than either: **a claim written in a
document or a comment has no failing case.** The repository already enforces this
for gates, for mutations and for the trap index --- each of those has something
that goes red. A sentence does not, so the useful habit is to ask of any
present-tense claim in prose *what would fail if this stopped being true*, and if
the answer is nothing, either build the thing or write the claim as a wish.

The fix here is the version display: an `app_version` command reading
`CARGO_PKG_VERSION`, an "About tpdf" command that needs no network, the version in
the empty state, and `updateNotice`, which answers in **every** state so that a
command can never appear to do nothing. And the check with a failing case is
`the_version_files_agree_with_the_crate`, which compares `Cargo.toml`,
`tauri.conf.json` and `package.json` at build time --- `BUILD.md` step 2 has listed
those four files since the first release and nothing had ever compared them, which
mattered much less while no reader was shown the number.
### A guard that reads the whole file does not belong on the path a reader waits on

The external-modification check needs a digest of the file as it was at open, so
the obvious place to take one is the open. Measured on this desktop before that
was written down as the design: **452 ms cold and 156 ms warm for the 337 MB scan
fixture**, 3.8 ms for a 3 MB drawing, 0.1 ms for a small text page.

Priority 1 in this project is a cold start under 300 ms. The synchronous version
spent more than the whole budget on precisely the documents a reader most wants
opened promptly --- and spent it **invisibly**, because a slow open on a huge scan
reads as a huge scan. There is no error, no warning, and nothing in the timeline
that says a new check was added; the regression would have been indistinguishable
from the file being big.

The fix is that `Edits::open` takes the path rather than the fingerprint, starts a
thread, and returns. The cell is a `OnceLock` and `Edits::plan` waits on it ---
reached only by a save or a print, both of which are about to read the whole file
anyway, so the wait is paid where it is already being paid.

Two things had to be right rather than assumed, and both are the kind that fail
quietly:

  * **The wait is outside the `docs` mutex.** Waiting inside it would hold the
    lock for as long as the hash takes and block every other edit command on the
    file being read --- which presents as the application hanging, not as a slow
    save.
  * **A document opened with no path settles the cell immediately.** A cell that
    nobody ever sets leaves every later `plan` waiting for ever. That control is a
    test, and it is worth noticing that its failure mode is a **hang** --- the
    shape this repository reads worst, since a hang and a pass both produce no red
    line. `docs/TRAPS.md` already carries two entries on that and this is a third
    place it would have applied.

The general form: **before putting a check on a path somebody waits on, measure it
against the largest input the project claims to serve.** A check whose cost scales
with the document is a different object from one that does not, and the difference
does not show up on the fixtures that are 888 bytes.

### A check that defers to a cheaper one it supersedes cannot be tested, and refuses what it should forgive

`fingerprint.rs` compares three things against what a file was when the reader
opened it: length, modification time, and a SHA-256 of every byte. The deep
check was written the obvious way --- do the cheap comparison first, then the
expensive one:

```rust
pub fn agrees_with(&self, path: &Path) -> Result<(), String> {
    self.agrees_shallowly(path)?;   // length and mtime
    let now = Fingerprint::of(path)?;
    if now.digest != self.digest { return Err(changed(path, "...")); }
    Ok(())
}
```

That reads as thoroughness --- fail fast on the cheap evidence, and only pay for
the digest when you have to. It is two defects, and neither is visible in the
code.

**The digest comparison was proved by nothing.** Deleting the three lines that
compare digests left **all seven** of the module's tests green, including the
two named for it: `a_rewrite_of_the_same_length_is_caught_by_the_digest_and_not_by_the_length`
and `a_file_larger_than_one_chunk_hashes_every_chunk`. Both rewrite a file and
assert the refusal contains `"changed on disk"` --- and a rewrite moves the
mtime, so `agrees_shallowly` refused first with *"it was modified"*, which
contains those same words. This is *An outcome two mechanisms can produce cannot
test either one* arriving through an ordering rather than through a shared
message: the two branches were distinguishable in principle and the assertions
did not distinguish them, so the cheap check stood in for the expensive one and
the test could not tell.

**And it refused saves it should have allowed.** An mtime is wrong in both
directions: `cp -p` and `rsync --times` preserve it across a rewrite, and a
backup tool, a sync client or a bare `touch` moves it without changing a byte.
Deferring to it when the bytes are already in hand means refusing a save whose
file is byte-for-byte what the reader opened --- a false refusal at the one
moment a reader least wants an argument, and one that sends them to *Save a
copy* over a backup having run.

**The fix is not a cleverer assertion.** It is that the deep check stops
consulting the timestamp at all: length (cheap, conclusive when it differs) and
then the digest, which is the answer. The shallow check keeps the mtime, because
it has nothing better --- it exists for the moment between staging and the
rename, where a third full read is the wrong cost. The two now answer different
questions rather than one wrapping the other, which is what makes each testable.

The mutation is the evidence: *stop comparing the digest* was **0 red** before
the change and **2 red** after, with no new test written for it.

The general form, and it is not about timestamps: **when a strong check is
implemented as a weak check plus a strong one, the weak one masks the strong
one --- in the tests and in production both.** Ask what the cheap comparison is
*evidence of*. If the expensive one supersedes it rather than extending it, the
cheap one belongs on the path that cannot afford the expensive one, and nowhere
else.

### A guard's last look should compare against the moment of the first look, not the moment of the open

The corollary of the entry above, in the caller rather than the callee, and it
is the half that survives fixing the other one.

`save_document` splits a save in two: stage the bytes into a sibling file, close
the document (a `rename` over a mapped file succeeds on macOS and leaves the
worker serving the old inode), then commit. The window between staging and the
rename is real, so there is a second, cheap check immediately before the rename
--- and it was comparing against the fingerprint taken at **open**.

That undoes the fix above completely. A `touch` between opening and saving is
forgiven by the deep check, which reads the bytes and finds them identical --- and
then refused by the shallow check twenty milliseconds later, because it is still
comparing a timestamp against a value from an hour ago. Worse than a plain false
refusal: it arrives **after the document has been closed**, so the reader is told
their file changed at the one point where the answer is "reopen and start again".

The fix is that `agrees_with` returns the fingerprint it took, `stage_in_place`
carries it out in a `Staged { path, verified }`, and the last look compares
against **that**. The window the shallow check covers is then the window it was
written for.

Two smaller things fell out, both worth having:

  * **`Staged::verified` is not an `Option`.** It could have been --- the plan's
    fingerprint is optional --- and then the caller's last look would have had a
    `None` arm, which can only be written as *skip the check*. Making it
    unsayable is what stops a refusal several lines earlier being undone by a
    later `if let`.
  * **The mutation table's anchors moved.** Three of them, silently, because the
    refactor rewrote the lines they name. One became **ambiguous** rather than
    absent: `let planned = planned_bytes(source, plan)?;` now appeared in both
    save paths. Distinct bindings are the fix; a longer anchor is the workaround.

### One refusal message, two moments, and it told the reader to do something they no longer could

`fingerprint.rs` worded its refusal once, which is normally the right instinct:

```rust
fn changed(path: &Path, how: &str) -> String {
    format!("{} changed on disk since you opened it --- {how}. \
             Your edits are still here: save them under another name, \
             or open the file again to start from what is on disk now.", path.display())
}
```

The advice is good, and it is true at the check it was written for --- the deep
comparison runs before the parse, with the document open and the journal intact.

It is false at the other one. `save_document` stages, **closes the document**,
and then takes a last cheap look before the rename. That look calls the same
comparison and appends its own tail, so the reader gets:

> report.pdf changed on disk since you opened it --- its length went from 4096
> to 5120. **Your edits are still here: save them under another name**, or open
> the file again to start from what is on disk now. --- nothing was written, and
> **the document has been closed**

Two clauses of one sentence contradicting each other, at the moment a reader is
least able to work out which half to believe. And the frontend makes it worse
rather than better: on a `reopen` failure `App.svelte` reopens the file
immediately, so by the time the message is read the model it refers to is gone.

**The fix is to split the fact from the advice**, not to reword either. The
comparison returns the bare fact; the deep check appends the way-out that is
true where *it* runs; the pre-rename check appends its own, which says the
document is closed and the edits are gone. Each call site supplies the sentence
whose truth it is in a position to know.

Both directions are held by a mutation, because a split like this collapses
back the moment somebody tidies two near-identical strings into one: putting the
advice into the fact reddens the pre-rename test, and removing it from the deep
check reddens the deep one.

The general form: **wording a message once is right only while every caller is
at a moment where it is true.** A shared message is a shared claim about program
state, and the second caller is where that claim quietly stops holding. The tell
is a caller that has to *append* to the message it was given --- if the ending
needs adjusting, the beginning probably does too.

### A wait built on a program the machine does not have returns instantly, and every check after it reads as a pass

Waiting for the viewer sweep to finish, on Windows, with:

```bash
until ! pgrep -f "viewer_sweep" >/dev/null 2>&1; do sleep 25; done
tail -30 sweep.log
```

`pgrep` **is not a program on this machine.** Git Bash ships no procps, so the
shell reports "command not found", the exit status is non-zero, `!` turns that
into true, and the loop's condition is satisfied on its first evaluation. The
wait returned in well under a second, and the `tail` after it printed two of
thirteen corpora as though that were the whole run.

**The failure is silent in the direction that looks like success.** There is no
error --- stderr went to `/dev/null`, which is exactly what a working `pgrep`
would produce for a process that is gone. The output afterwards is a real, green,
partial result, and a partial run of a sweep whose corpora are independent looks
identical to a complete one until you count the rows.

This repository already carries *The sweep shelled out to `pkill`, which is not a
program on Windows*, and the same absence bit the **waiter** rather than the
sweep this time. That is the part worth keeping: the lesson was recorded about
the code under test, and it applies just as well to the instrument watching it.

Two habits close it:

  * **Wait on the job's own output, not on a process table.** The sweep prints
    a summary line when it is done; `until grep -qE '^\[(OK|FAIL)\] +(no failing
    checks|[0-9]+ corpus)' sweep.log` cannot be satisfied by a missing program.
  * **A wait whose condition is "a command failed" needs the command to exist.**
    `command -v pgrep` answers in one call. On this box: it does not.

The general form is the one this file keeps arriving at from new directions:
**a check that cannot fail and a check that passes are the same output.** Here
it was a *wait* that could not wait, and what it certified was two corpora out
of thirteen.

### Two runs failing different checks is variance; the same check twice is a defect

`viewer_sweep.py` went red on `vector-multi` twice in a row, on a tree carrying a
day's worth of changes. The reflex reading --- a red run after a change is a
regression from that change --- is wrong here, and the discriminator is cheap.

The two runs failed **different** checks:

  * *the page already rendered is not rendered twice*, at 11 borrows against 4 draws
  * *covers the first screen*, timed out at sharp=0.0%

Four runs of that one corpus took **351 s, 496 s, 386 s** on one build and
**384 s** on another. A 41% spread on identical code, and the same binary that
failed at 496 s passed at 386 s.

`vector-multi.pdf` is twelve A0 pages and exists *because* it is the only fixture
where a thumbnail render is slow enough to collide with the viewer. Both failing
checks observe that collision. A corpus built to sit on a race will sometimes
land on the other side of it.

**The discriminator, and it costs nothing: does the *same* check fail twice?**
One check failing repeatedly is a defect with a name. Two different checks
failing is the schedule moving. Reading only the verdict --- red, red --- loses
exactly the information that separates them.

**The control is a `git worktree`, not a `git stash`.** `git worktree add <dir>
HEAD` gives a clean checkout of the unmodified tree to build and run against,
while the working tree with a day's uncommitted work in it is never touched.
`git stash` would have put all of it on a stack for the duration, and this
repository already carries an entry about what a mass `git checkout` does to
uncommitted work in files nobody was thinking about.

One caveat on the control itself: a fresh worktree has no `vendor/pdfium` and no
`testdata/`, both gitignored. Copy in the library and the one fixture the run
needs, or the control fails to start and that reads as a broken checkout rather
than a missing prerequisite.

### A test that changes the working directory silences every other test that reads a relative path

`recentdocs.rs` needed to prove that a relative path is made absolute before the
shell sees it. The obvious way to get a relative path is to stand in its
directory:

```rust
let restore = std::env::current_dir().expect("cwd");
std::env::set_current_dir(&directory).expect("enter the scratch directory");
let wide = shell_path(Path::new(&name));
std::env::set_current_dir(restore).expect("restore cwd");
```

Careful, symmetric, restores what it changed --- and wrong, because the working
directory is **process-global** and `cargo test` runs tests on several threads.
For the width of that window every other test in the crate has a different cwd.

`save.rs` resolves its fixtures with `Path::new("../testdata").join(name)`, a
relative path, and dozens of tests use it. So during the window they take one of
two branches:

  * `Document::load` on a file that is suddenly not there --- a panic, exit 101.
  * `path.exists().then_some(path)` returns `None`, the test prints
    **`[SKIP] rotated.pdf: fixture not generated`** and **passes**.

The second is the one that matters. A test that stops running looks exactly like
a test that ran, and the run is still green.

**It cost one unexplained exit 101 in four runs, and three clean hand-runs
afterwards said nothing was wrong** --- which is what a race of a few
milliseconds looks like from the outside. What settled it was not more runs: it
was `grep set_current_dir`, then *widening the window on purpose*. Planting a
throwaway test that moves the cwd and sleeps 400 ms produced **exit 101, five
failures and four `[SKIP]` lines** on the first try. Removing it restored 581
passed, 0 skipped.

**The fix is to never change the cwd**, not to restore it more carefully: build
the relative path against the directory the tests already run in. `target/` is
gitignored, so a run killed mid-test leaves nothing in `git status` to mistake
for real work.

The general form: **process-global state in one test is a mutation of every
other test**, and the ones it breaks are the ones that never mention it. Env
vars, the current directory, the locale and a process-wide logger are all this
shape. And the tell to look for is not a failure --- it is a **skip count that
moved**.

### A refusal that names a fallback has to keep the fallback open, and this one closed it

`stage_in_place` refuses a save when tpdf cannot tell whether the file is still
the file, and its comment states the rule that makes the refusal safe:

> So the fallback the message names has to keep working, or the refusal strands
> the reader.

It was written for a *missing fingerprint*, and it holds there: `write_copy`
tolerates that case on purpose. Then the changed-file check landed and refused
inside `planned_bytes`, which is the function **both** save paths call. So:

  * Save over the file → refused, message says *"save them under another name"*.
  * Save a copy → refused, by the same guard, one function down.

A reader whose file changed underneath them could put their edits nowhere at
all. The message pointed at a door and the same commit locked it.

**Nothing went red, and nothing could have.** There was a test named
`a_copy_is_refused_when_the_source_changed_under_it`, it passed, and its doc
comment argued for the behaviour: *"the copy would be as wrong as the in-place
save -- it just would not destroy anything on its way."* That reasoning is
correct about the copy's *contents* and silent about the reader having nowhere
to go, which is the question the rule was about. A test can encode a dead end
perfectly.

The fix is the asymmetry the file already believed in, applied to the second
case: an `OnChange` parameter, `Refuse` for the in-place path and `Proceed` for
the copy, because a copy writes a new file and leaves the original exactly as it
is. The copy reports `changed` so the reader is told which document it was built
from --- silence there would be the worst of the three outcomes. What still
refuses is a file whose *shape* changed, caught by the page-count guard
whichever path asks.

Two general forms, and the second is the one that generalises furthest:

  * **When you add a guard to a shared function, check every caller against the
    advice the other callers give.** The guard was correct in isolation and
    wrong in company.
  * **A rule written in a comment is enforced by nobody.** This one was stated
    in the file it governs, three lines above the code that violated it, and
    survived a full review because the reader of that comment was the person who
    had just written the violation. `docs/TRAPS.md` already carries *A rule you
    wrote down is not a rule you enforce*; this is that, with the rule and the
    breach in the same function.

### A message set before the operation that clears the message area is a message nobody sees

`save_document`'s `after_close` refusals are the ones a reader can do least
about: the document is closed, the journal is spent, and nothing was written.
`App.svelte` set the message and then reopened the file so the reader had
something to look at:

```js
say(prompt.message, prompt.offers);
if (!failure.reopen) return;
openDoc = -1;
await openPath(path, false, place);
```

`openPath` begins with `say(null)`. So on exactly the path where the message
matters most, it was displayed for **zero frames**. Every other refusal in that
function returns before the reopen and shows correctly, which is why the code
reads as fine: the working cases and the broken one are the same two lines.

It had been that way since `save_document` shipped. What made it visible was
adding a *second* consumer of the same message --- buttons --- and asking where
they would appear.

**The fix is ordering, not a flag**: say it *after* the reopen, and only if the
reopen had nothing of its own to report, since a file that also failed to reopen
is the more urgent fact and is already on screen.

The general form: **an operation that resets the surface a message lives on is a
delete of that message**, and it does not look like one at the call site. Before
writing `notify(...)` above an `await`, ask what the awaited thing does to the
place the notification went. The tell is a shared surface --- one `error` string,
one status bar, one toast slot --- being written by both the reporter and the
thing it is reporting about.

A second lesson sits underneath it and is the one worth carrying: **the producer
of a message should state the fact and let the caller own the advice.** The
backend's text ends *"open the file again to see what is there now"*, and the
window opens it again automatically --- so the instruction is addressed to
somebody who has already had it carried out for them. Same shape as the refusal
that told a reader to save edits that were already gone, one layer up.

### Three ways to look for a macOS recent-documents list, and all three say nothing is there

`recentdocs.rs`'s Windows half is checkable by looking at a file:
`SHAddToRecentDocs` writes `%APPDATA%\Microsoft\Windows\Recent\<name>.lnk`, and
the sweep produced one per corpus. Writing the macOS half on 2026-08-20, the
module doc predicted the counterpart before measuring it:

```text
defaults read com.timostein.tpdf NSRecentDocumentRecords
```

It answers **"The domain/default pair does not exist"** --- on a run where the
call had just succeeded. So did the next two places:

* Still absent after 75 s of running, and after a clean `osascript` quit. The
  application's `.plist` mtime never moved.
* Probed from *inside* the process, `NSUserDefaults.standardUserDefaults` did not
  hold the key either, immediately after the call. So this is not a `cfprefsd`
  flush delay --- there is nothing to flush.
* `sfltool list-info com.apple.LSSharedFileList.ApplicationRecentDocuments`
  **hangs**. It is not an instrument.
* `ls ~/Library/Application Support/com.apple.sharedfilelist/` --- and this one
  is the trap inside the trap. Run with `2>/dev/null`, as it first was, it prints
  `total 0` and reads as **an empty directory**. Run without, it says
  **`Operation not permitted`**: the path is TCC-protected, and suppressing
  stderr had converted a permission error into an emptiness claim that was then
  written into three documents. The same shape as `gh api` printing its 404 body
  to stdout --- *never suppress the stream whose silence you intend to treat as
  evidence.*

  What is inside it is therefore **not established**. `stat` on a named path
  answers `No such file or directory` rather than `Operation not permitted`, so
  search permission works and the answer looks real --- but it says the same for
  `com.apple.Preview`, `com.apple.TextEdit` and `com.apple.Terminal`, so there is
  **no positive control** proving `stat` would see a file that is there. An
  instrument that answers "absent" for every input has not been shown to be able
  to answer "present".

Four negative answers in a row, and the conclusion they support --- "the call is
accepted and then dropped, so ship it disclaimed" --- was drafted into
`docs/PLAN.md` before the fifth measurement was taken. It was wrong.
`NSRecentDocumentRecords` is simply the pre-Sierra location, and of the four,
**one was a real absence, one was a hung tool, and one was a permission error
wearing an absence's clothes.**

**What settles it is asking a second process.** Launch on one document, quit,
launch on another: the second process starts with `recentDocumentURLs` already
holding the first document, which it never filed. Measured, `text-heavy.pdf` then
`rotated.pdf`: `BEFORE filing, AppKit holds 0` on the first launch and `1` on the
second, carrying the first document over, then ordered most-recent-first. The
feature works, and every obvious observable had reported it absent.

Two things to carry. **An absence is a fact about where you looked**, and four
independent-looking places can share one wrong assumption --- here, that a list
macOS persists must be persisted *as a file*. And the direction of the near-miss
is the uncomfortable one: the wrong conclusion was the modest one. A disclaimer
saying "filed, but it does not survive a launch" would have been quieter than the
overclaim, would have read as commendable caution, and would have been false ---
which is the shape the entry on a mitigation present and disclaimed already
warns about, arriving here in a measurement rather than in a document.

### `NSURL` hands a path back decomposed, and the fixture that shows it is not the ASCII one

The macOS `recentdocs` seam converts a resolved path into an `NSURL`, and the
test read it back with `url.path()` and compared:

```rust
assert_eq!(Path::new(&url_path), absolute, "and it is still the same file");
```

Green on a scratch file called `tpdf-recentdocs-plain-16036.pdf`. Red on
`tpdf-recentdocs Prüfbericht 16036.pdf`, with a failure message that prints the
path it was given and looks **identical** to the literal it was compared against.

Measured rather than guessed, because the reflex answer here is wrong. Byte for
byte:

| where | `ü` |
|-------|-----|
| the Rust source literal | `c3 bc` (U+00FC, NFC) |
| `std::fs::canonicalize` | `c3 bc` --- APFS preserved what it was given |
| `NSURL::fileURLWithPath` → `path()` | `75 cc 88` (u + U+0308, **NFD**) |
| `absoluteString()` | `Pru%CC%88fbericht` |

So the standing note that *"APFS hands filenames back in NFC, so the macOS-
normalises-to-NFD reflex is wrong here"* is exactly right about the filesystem
and says nothing about AppKit, which is the layer that decomposes. Both halves
are needed: the file on disk is composed, the URL naming it is decomposed, and
the two are one `canonicalize` apart.

**It is not a mangled name.** APFS looks a filename up normalisation-
insensitively, so the decomposed path opens the composed file ---
`Path::new(&url_path).exists()` is true, and `canonicalize` of it returns the
composed form. Which is why the assertion to write is not an equality on the
string but a resolution: *does this URL name the reader's file?*

```rust
assert_eq!(std::fs::canonicalize(&url_path).ok().as_deref(), Some(absolute.as_path()));
```

The general form is the one this file keeps recording from other directions: **a
fixture where the right rule and the wrong rule agree cannot tell them apart.**
An ASCII scratch name makes byte equality and same-file equivalent, so the test
that was written first could not fail, and it was the *second* fixture --- a name
no reader would call unusual --- that exposed the rule. Both tests use the
resolving form now, including the ASCII one, so neither encodes a rule that holds
only for its own fixture.

### A mutation written on one platform names a test the other platform does not compile

`recentdocs`'s two Windows mutations name
`a_path_the_shell_is_given_is_absolute_nul_terminated_and_not_verbatim`, which
lives inside `#[cfg(all(test, windows))] mod tests`. Neither carried `only_on`.

On a Mac that test does not exist, and `mutate_rust.py`'s name guard --- which is
right to be loud about a name it cannot find, and says so in its own comment ---
refuses **the whole table** over it. So a table of 198 mutations was unrunnable
on macOS from the day those two were written, for the same reason and in the
exact mirror of the `menu::` incident that comment describes: written on one
platform, silently blocking the other, invisible until somebody runs it there.

Nothing caught it. `check_mutation_anchors.py` asks whether the anchor exists in
the file, and it did --- an anchor is a *string in a file*, while platform gating
decides which strings become *code*, so the existing gate was structurally unable
to see this. It was found by reading, which is luck.

The gate asks the second question now: **where is the test that is supposed to go
red, and can it go red here?** It locates the `fn` by name, finds the enclosing
`#[cfg(all(test, ...))]` region, and requires `only_on` to match. A test defined
inside *several* platform-gated modules --- one rule with a test on each side of
the cfg, which is what `resolved` has --- needs no declaration and gets none. A
test found outside any gate is not this check's business.

Proved three ways before it was trusted: dropping a required `only_on` fails
naming the platform to set; declaring the *wrong* platform fails naming the right
one; and a scan that finds **no** gated module anywhere fails rather than passing
every mutation in silence --- which is the shape the emptiness control beside it
already guards for the table itself. Exit 1 in all three, exit 0 clean.

Note the control run printed `exit=0` at first: `$?` after a pipe through `tail`
is `tail`'s status, which this file already records from another direction.

### Padding a rectangle to make one refusal legal disables the check that refusal was doing

An ink mark's `/Rect` is the bounds of its strokes, and the first version took
them tight. A reader ruling a straight line down a margin then produced a
rectangle of **no width**, which `Quad::covers_area` rejects --- so the most
ordinary drawing there is came back as *"that mark covers nothing"*.

The fix is not a special case: a stroke straddles its path, so the rectangle the
ink actually occupies is the tight bounds grown by half the line width, and
`Stroke::bounds` takes that pad. The vertical line then covers area, because it
does.

**And that quietly removed the emptiness check.** `Doc::annotate` refused a mark
whose every quad was degenerate, which is what catches a click that never became
a drag. With the pad, *every* ink mark covers area --- including one whose stroke
is the same point repeated, which is exactly what a click produces. So the
padding did not merely admit the vertical line; it admitted an invisible mark the
reader cannot find again to remove, and no assertion moved.

The general shape: **`covers_area` was answering two questions --- "is this
rectangle drawable" and "did the reader make a gesture" --- and they coincided
only while the rectangle *was* the gesture.** Ink is the first kind where the
rectangle is derived from the gesture rather than being it, and the two come
apart. The model asks `Stroke::is_drawable` for ink and `covers_area` for
everything else, and the test that pins it asserts the padded rectangle covers
area *first*, so the case cannot be mistaken for the one above it.

Same family as the entry about one predicate answering three questions until a
second kind made them disagree --- and worth noticing that the trigger here was a
*fix*, not a feature: the pad was added to make a legitimate mark legal, and it
took a check away on the way past.

### A band check can pass by two hundredths of a point, and a passing run does not say so

`annot-probe --mode strokes` proves ink is drawn where it was drawn by reading
three bands of the mark's rectangle: the outer two must hold ink and the middle
one must be empty. Its fixture draws two horizontal strokes at 15% and 85% of the
text box.

It passed. `0 px in the gap`, twice, on two corpora.

The margin is one arithmetic step and it is not what the run suggests. A stroke
centred at fraction `f` of a text box of height `h` occupies `[f·h, f·h + w]`
measured from the derived rectangle's top, where `w` is the line width; the band
boundary is `(h + w)/3`. On `text-base14`, `h = 9.2 pt` and `w = 2.5 pt`:

```
stroke reaches 0.15 * 9.2 + 2.5 = 3.88 pt
band ends at  (9.2 + 2.5) / 3   = 3.90 pt
```

**Two hundredths of a point.** A corpus with slightly shorter lines puts the
mark's own ink in the band the check asserts is empty, and the run reports a
defect that is not there --- a control that cannot pass, which is the failure
this file records from several directions and which reads as a real finding
rather than as a broken instrument.

Moving the strokes to 5% and 95% takes the margin to about a point, and the
condition for the bands to separate at all becomes `h > 2w/0.85` --- which the
mode now checks and refuses with the number rather than reading a rectangle it
cannot discriminate in.

The lesson is not about ink. **When a check is built from fractions of a derived
quantity, the run tells you the sign of the margin and never its size.** Compute
it once, in the units the check works in, and put the arithmetic in the comment:
a green run is the same output at 0.02 pt of headroom as at 1 pt, and the first
one is a defect waiting for a different fixture.

### The harness prints the count so nobody has to derive it, and it was derived anyway

`viewer_check.py` ends every run with `CHECK-NAMES-JSON` --- the full list of
check names, as JSON, on one line. It exists so that the number is *read from a
run* rather than remembered.

On 2026-08-20 an increment added two names and needed to say what the new total
would be. The screen was locked, so no run was possible; `BUILD.md` said **109**;
the prediction written down was **111**.

The measured answer is **279**.

`109` was correct on 2026-07-31 and had not been touched since, while the harness
gained marks, crops, print and the comment panel. So the error was not two-plus-a
-hundred-and-nine going wrong --- it was treating a documented count as current
because it was the only number to hand.

**What makes this worth an entry is that the warning was already there and one
paragraph away.** `BUILD.md` says, in its own words, that the ran/skipped columns
are not the invariant, that the names are, and that *"a count chased back to a
documented value is a defect introduced to satisfy a document"*. That is the same
failure in the other direction, it was read during the same session, and the
arithmetic was done anyway --- which is the shape recorded under *"a rule you
wrote down is not a rule you enforce"*, arriving in a document about not trusting
documents.

Two things follow. **A count in prose needs a date beside it or it will be read
as current** --- `109` carried none, and the entry that has one (`docs/TRAPS.md`'s
own total, whose authority is a `grep -c`) has never been wrong in this way. And
when the instrument emits the number, quote the command rather than the value:

```sh
python3 -c 'import json,sys;print(len(json.loads([l for l in open(sys.argv[1]) if l.startswith("CHECK-NAMES-JSON")][0][16:])))' run.log
```

### A check named by its position in a list is renamed by whatever is appended to that list

`overlayInkChecks` declares its names in one array so that a run which cannot
start still prints them, and then indexes that array at the call sites. Adding
ink pushed the last entry along, and the distinctness check --- written
`OVERLAY_INK_CHECKS[6]` --- began reporting under the drawing's name while the
drawing's own verdict went unprinted.

That was noticed and "fixed" as `OVERLAY_INK_CHECKS[OVERLAY_INK_CHECKS.length - 1]`,
with a comment explaining the trap. **Which encodes *"distinctness is last"*, and
two checks were appended after it within the hour.** Identical failure, identical
symptom, one hour and one comment later.

The symptom is quiet in a way worth knowing. Every count still adds up: the run
reports the same number of checks, all of them pass, and the roll of names is the
right length. What actually happens is that one check's verdict is printed under
another's label, the label appears twice, and a third name vanishes from the roll
--- and `viewer_sweep.py` compares those rolls as **sets**, which cannot see a
repeat.

Two fixes, and the second is the one that generalises:

- The names are a **keyed object** now, `INK_CHECK.distinct` rather than an
  index. A key cannot shift. The array the harness prints is derived from it, so
  the declared list and the names in use are one thing.
- **`Report.finish` fails the run when two results share a name.** That is the
  mechanism, and it belongs there rather than in the phase: any phase can do
  this, and the set comparison the roll exists for is blind to exactly this. It
  is recorded as a failed check rather than thrown, so a run still prints
  everything it measured --- the names are the diagnosis.

It found the real damage on its first live run, which was not the bug it was
written for. See the entry below.

### A global text replace with a "one or more" assertion rewrote four unrelated checks

The keyed-name repair above was applied with a script that replaced
`check(\n    names[0] ?? "",` with `check(\n    INK_CHECK.preview,`. Python's
`str.replace` replaces **every** occurrence, and the assertion guarding it was
`assert n >= 1` rather than `== 1`.

Four other phases --- page turns, page deletion, page moves --- have a local
`names` array of their own and call `check(names[0] ?? "", ...)` with the same
formatting. All four were rewritten to report under the ink preview's name. The
type-checker was happy, all 927 unit tests passed, `npm run check` was clean, and
the window run reported **246/246 checks passed**.

What caught it was the duplicate-name guard added in the same session, on its
first real run, printing the four detail strings under one label: *"page 1 turned
90"*, *"4 pages, now 3"*, *"4 pages, now 4"*. A check named *"a drawing in
progress is previewed as a line"* reporting *"4 pages, now 3"* is not subtle ---
and nothing except that guard was looking.

**`assert n == 1` is not pedantry when the edit is textual.** A count is the only
thing standing between a mechanical replace and the rest of the file, and
loosening it to `>= 1` removes the guard entirely while still looking like one.
This repository already records three ways a mutation can fail to land; this is
the mirror --- an edit that landed, four more times than intended.

The general form, and it is not about Python: **a textual edit is scoped by its
pattern and by nothing else.** `check(\n    names[0] ?? "",` reads like a
location and is a shape that four other functions happen to share. Where a
mechanical edit is worth doing at all, count the sites first, assert the exact
count, and read the diff for files you did not mean to touch.

### The window reads the status and the tests read the viewer, so the copy between them is untested

`Viewer` reported a drawing in progress twice: `drawnStrokes`, an accessor the
check harness and the unit tests call, and `ViewerStatus.drawing`, which is what
`App.svelte` renders into the status line. They were one line apart in the same
file and computed the same thing from the same state.

A mutation that emptied the status field **survived**. Every test asked the
accessor; nothing in the frontend suite reads the status, because a status is
published by the frame loop and driving that in a unit test needs the tile IPC
those files deliberately do not stub. So the reading a reader actually sees was
the one with no coverage, and the reading with coverage was the one nobody sees.

The repair is not a cleverer test. **`ViewerStatus.drawing` is now the accessor**
--- one expression, used by both --- so there is nothing between them to drift and
the mutation has somewhere to bite. That is the same argument this file makes
about two copies of a distinction, arriving where the copies are a *public* and
a *private* reading of one fact.

The general form: **when a value is exposed twice, ask which of the two the
product uses and which one the tests use.** If the answer is different, the
tested one is decoration. It applies to any accessor added "for the harness"
beside a field the application renders.

### A bound stops discriminating when the behaviour around it changes, and its test keeps passing

`viewer.ts` refuses an ink stroke of fewer than two points --- a press that never
moved is a reader who has not started, not a mark of no length. The test asserted
that after such a press nothing was committed and the tool was still armed.

Sound, while the tool was one-shot: a kept stroke would have spent the tool, so
`drawArmed` being non-null said the press was refused.

**Making the tool stay armed for several strokes removed that link, silently.**
The tool is now armed either way, and nothing is committed either way --- so both
assertions hold whether the bound is enforced or not, and a mutation deleting it
survived. The test did not break; it stopped being able to.

The reading that still discriminates is the stroke *count*: a refused press
leaves the drawing at zero strokes and a kept one at one. Which is a second
lesson in the same paragraph --- the count had to be *added* to the accessor as a
distinct value (`0` for armed-with-nothing, `null` for not armed) before it could
be asserted, because "no drawing" and "a drawing with nothing in it" had been the
same answer until the tool could stay armed.

**What to do with it: when a mode changes, re-read the tests that assert about
the old mode's side effects.** They will not fail. `drawArmed` was never a
statement about the bound; it was a statement that happened to imply it, and
implications are what a behaviour change takes away.

### Four checks that say where the ink is, and none that says how long it is

`annot-probe --mode strokes` asserts that a freehand drawing put ink in the outer thirds of its
rectangle and none in the middle. On `rotated-90` — an invocation `BUILD.md` has recommended
since the mode landed — it reported **5 of 5 green while each stroke was 11.9 pt long instead of
246.7**, a nineteenth of its length. Two stubs at the ends of a rectangle satisfy "ink in the
outer thirds, nothing between" exactly as well as two full-length strokes do.

**The rectangle cannot be the standard, because the rectangle is derived from the strokes.**
`Stroke::bounds` builds `/Rect` from the points, so the two agree by construction, in the wrong
case as in the right one. Any assertion relating a stroke to the mark's own rectangle is
therefore true whatever was drawn. What separates them is the *shape of the ink inside* the
rectangle: the extent along its longer side, which is 99% of it for a stroke that runs the
length of the mark and 1% for one drawn across it.

The cause was the probe's input rather than the writer — `save::user_strokes` mapped what it was
handed. `mark_and_save` synthesised its two strokes at 5% and 95% of the box's *height*, spanning
`left` to `right`, and on a page displayed sideways the lines advance across the screen while the
characters run down it, so that is across the run instead of along it. **The lesson had already
been written down forty lines below, in `quads_for`'s own doc comment** — *"The axis is not always
the vertical one, and the first version of this assumed it was"* — and did not transfer to the
two functions added later that needed it.

Found by pointing an unrelated renderer at the saved file. Nothing inside this repository could
have said so, because every check here reads the mark through the same derivation that produced
it.

### A check that measures along the axis it is policing shrinks its expectation with its measurement

The repair for the entry above was a span check: each stroke must reach at least 80% of the
rectangle's long side. The first version asked *along the axis `sideways` had chosen*, `sideways`
being the same determination the band split uses — and with both halves of the fix reverted it
reported **"14.2 pt of 14.4, needs 11.5"** and passed.

The arithmetic is the whole of it. A wrong `sideways` calls the short side the long one, so the
expectation shrinks from 224.5 pt to 14.4 at exactly the moment the measurement shrinks from
224.5 to 14.2. Ratio preserved, check satisfied, defect intact. **A check built on the decision it
exists to police cannot fail when that decision is wrong** — which is the only case anyone cares
about.

The fix is one line and it is the shape to reach for: take the maximum span over **both** axes and
compare it against `width.max(height)`, neither of which reads `sideways` at all. The two
mechanisms can then disagree, and a disagreement is the finding.

Proved by reverting both halves — the code exactly as it shipped — and watching the four original
checks stay green while the two new ones report `14.2 pt of 224.5, needs 179.6`.

### The same assumption, quiet in one mode and loud in its neighbour

`--mode rule` splits a quad into thirds *down the page* and asks that an underline be in the
bottom one. On `rotated-90` it reports **330 / 330 / 332** and fails two of its four checks — an
underline drawn perfectly correctly, along a run that goes down the screen, crosses all three
horizontal bands.

So the same axis assumption sat in two modes of one file, silently certifying a wrong drawing in
`--mode strokes` and loudly condemning a right one in `--mode rule`. **The loud one had never been
seen**, because `BUILD.md` lists `--mode rule` only against `text-base14`: a check that would fail
on an input nobody gives it is indistinguishable from a check that works.

Which edge is "under" has four answers, read off `text::from_device`'s four arms — for turns 0, 90,
180 and 270 the page-space bottom is the device box's bottom, left, top and right. `rotated.pdf`
carries all four rotations on pages 0 to 3, so the table is testable in one sweep, and two
mutations show it is load-bearing cell by cell: collapsing it to one answer reddens 90° and 180°
only, and splitting down the page regardless reddens 90° and 270° only. Both leave the strikeout
green under the first mutation, correctly — a strikeout's band is the middle, which a swap of the
outer two does not move, so the underline checks alone carry the table.

### Borrowing the writer's own table to avoid drift made the check unable to fail

`annot-probe --mode preview` asks whether PDFKit reads back the `/Subtype` we wrote. The first
version compared against `save::subtype`, the writer's own table, which was made `pub` for the
purpose — deliberately, and citing this file's entry about two copies of a distinction drifting
apart.

Mutating `save::subtype` to write `/Underline` for a strikeout left the check **green**. Of
course it did: the check read the same table the writer did, so the expectation moved with the
code. The rule against a second copy is real, and applying it here produced the worse defect —
a check that cannot fail is worth less than one that occasionally goes red for a stale reason.

**The way out was neither table.** `annots::Kind::of` turns a `/Subtype` string into a kind
through the *reader's* parser, so PDFKit's string and `annots.rs`'s reading are compared as
kinds, by a third thing that is neither of them. No table in the probe, and no borrowing from
the writer.

That still cannot catch a writer that writes a legal wrong subtype, because both readers read
the wrong value and agree about it. Two things already do, both measured red under the same
mutation: `save::tests::each_kind_writes_its_own_subtype`, and `--mode roundtrip`'s kind
assertion, which compares against what the run asked for. **Knowing which check owns which
failure is the point of measuring; the mode's doc comment carries the table.**

### Two readers of one file cannot catch the writer that moved it

The general form of the entry above, and it is worth stating separately because it is a
property of the whole design rather than of one check. `--mode preview` compares PDFKit against
`annots.rs` over the same bytes. Shift the `/Rect` three points sideways in `save.rs` and both
readers report a rectangle three points sideways; they agree perfectly and every check passes.

So a differential between two readers is evidence about **parsing**, never about **geometry**.
What it catches is a dictionary one of them rejects, a string one of them cannot decode, a
subtype they map differently, an appearance one of them will not draw. What it cannot catch is
anything the writer did consistently. Geometry needs a standard outside both — `--mode
roundtrip` has one, the character boxes the mark was made from, which is why it fails on the
same mutation.

The practical consequence: when adding a check to a differential, ask which of the two
populations could move without the other. If the answer is "neither, ever", the check is
decoration.

### PDFKit synthesises an appearance for an annotation that has none

Deleting the `/AP` from every mark `save.rs` writes — proved landed, zero occurrences of `/AP`
in the saved file — changed what PDFKit draws for a `/Square` from 1306 px to 1056, and drew it
anyway. It generates its own frame. The same is true of a highlight: blanking `/AP` moved the
wash from a 13.2 pt band to a 10.8 pt one rather than removing it.

Two things follow. **A "does a foreign reader draw it" check cannot test whether the appearance
stream was written**, which is what makes `save.rs`'s own assertion that the key is present the
coverage for that. And **`docmodel.rs`'s note that "a `/Square` with no `/AP` is an annotation
Acrobat draws as nothing at all" is a claim about Acrobat and has not been checked here** —
PDFKit is not Acrobat, and this measurement says nothing either way about the reader that
sentence names.

### A `/Text` annotation's rectangle is advisory, and PDFKit replaces it

A comment written with `/Rect [60.322 717.074 313.652 730.192]` — 253.3 by 13.1 — comes back
from `PDFAnnotation.bounds` as `(60.322, 706.192) 24 x 24`. Not a corruption and not a
rounding: the standard icon size, hung off the rectangle's **top-left** corner, since
`730.192 - 24 = 706.192` exactly. The specification allows it; a reader draws the note icon at a
size of its own choosing and the rectangle only says where.

It reads as a 229 pt error the first time you see it, and the reflex is to go looking at the
writer. Two consequences for anything comparing rectangles across readers: **exempt `/Text` and
assert the anchor and the size instead** — which still proves the foreign reader found your
rectangle, because it cannot place the icon on your corner otherwise — and note that the icon
hangs **below** the rectangle's bottom edge, so a containment check against your own `/Rect`
reports a defect in a mark that is correct.

### A guard whose only reachable input is one the model forbids

`viewer.ts`'s eraser skips marks that are not drawings: `if (!isPath(mark.kind)) continue;`.
Deleting that line **changed nothing** — a mutation aimed at a test that swept the nib across a
highlight survived, because a well-formed highlight has an empty stroke list and the loop under
the guard finds nothing whether the kind is checked or not. The guard is unreachable for every
input the backend can send, which is the model's `strokes.is_empty() != (kind != Ink)`
biconditional doing its job one layer down.

**The fixture that makes it reachable is a malformed one**, and building it deliberately is the
answer rather than deleting the guard: a highlight carrying three strokes, which the model
refuses and which the viewer must therefore never see. The viewer is still the place that must
not act on it if it arrives, and that input is the only thing that can tell a working guard from
a deleted one.

Related to the existing entry about keeping an unreachable guard when the type can carry it
instead, and the difference is worth naming: there the type could express the impossibility, so
the guard could be deleted. Here `MarkView` allows any kind to carry strokes — the wire format
is a JSON object and the biconditional lives in Rust — so the guard is real defence against a
real (if remote) shape, and the test has to construct that shape by hand.

### The nib was tested where it was, not where it had been

The eraser's first version asked, at every pointer report, which strokes were within its radius
of *that point*. A pointer reports at the display's rate and a hand crosses several strokes
between two reports, so a quick sweep down a column of three strokes took the outer two and left
the middle one — it lay between the samples.

**It is the same failure the hit test already avoided one level down.** `strokeTouches` measures
to the nearest *segment* of the stroke rather than to its nearest recorded point, precisely
because a fast hand leaves points far apart; the sweep then made the identical mistake about its
own path. Two polylines, and only one of them was being treated as a polyline.

The fix is segment-to-polyline: the travel from the last report to this one, against each
segment of each stroke. Which needs a crossing test as well as the four endpoint distances — an
X of two long strokes is at distance zero with all four ends a hundred points apart, and a
mutation deleting that test reddens five checks.

Two notes for anyone writing the next one. A press is a segment of no length and needs no
special case, so `strokeTouches(points, at, r)` is now `strokeSwept(points, at, at, r)` and the
existing tests still hold. And **the first mutation written for this survived**: replacing two of
the four endpoint distances with degenerate ones left three terms still reading the previous
point, so the travel was still tested. Aim a mutation at the line that *holds* the state — here
`const from = swept.last;` — not at one of several places that consume it.

### A bound no correct input can reach makes a check that cannot pass, and a manual-only harness is where that survives

`viewer_check.py`'s overlay phase asserts that a strikeout crosses the middle of its quad:

```ts
(r) => r.whole < 0.3 && r.core > 0.8 && r.edges === 2
```

`core` samples the middle tenth of the mark's height. `markBand` centres a rule
`LINE_FRACTION` — 7% — of that height. So the **ceiling** for a correct painter is
0.07 / 0.10 = 0.70, and every run read 0.71 with antialiasing. The check has never once
passed. It went in red on 2026-08-19 with the phase it belongs to and was still red a day
later, on a `main` that CI called green on both platforms.

**Two things had to be true at once, and each is ordinary on its own.** The bound was
written to say "most of it" without anybody doing the division; and the harness that runs it
is not a `gates.py` gate — it needs a built bundle and an unlocked screen, so nothing in CI
or in a gate run can execute it. A check in a manual harness is only as green as the last
time a person ran it, which means **a check can be born red and stay that way indefinitely**.
Every other kind of rot at least starts from a passing run.

The tell was the printed evidence, not the verdict: `71% of its centre` against a bound of
80% is a *near* miss, which reads like drift or a rendering difference. It is not drift. It
is a number that cannot go higher, and the way to know is arithmetic on the two constants
rather than another run.

**Do not repair this by deriving the bound from the constant it polices.** `r.core >
LINE_FRACTION / 0.10` would pass for every thickness including zero — the trap about a check
that measures along the axis it is policing, arriving one file over. The bound is a fixed
number chosen for what it has to tell apart: an underline, a frame and a drawing all read
0.00 at the centre, so anything strictly between 0 and 0.70 discriminates, and 0.5 sits in
the middle of that gap.

**A bound lowered to turn a red check green needs its own control**, or the repair is an
assertion nothing can fail. `mutate_viewer.py` carries it: putting a strikeout's rule where
an underline's goes reddens the check at 0.5 exactly as it should.

Related, and different in the half that matters: the `NaN` entry is about an assertion that
cannot pass *loudly* — it fails on the first run and reads as a broken harness. This one
fails quietly, in a phase of 250 checks, in a harness nobody runs on a schedule.

### An accounting observable nobody reads is the same as not having one

`Doc::ink_bodies` was added with the eraser for the stated reason that a version kept after
its command was discarded and one correctly dropped produce **identical documents** — no
assertion over the working document can tell them apart. It was then never read by a test for
that case, and the arm that drops an `Ink` from a discarded redo tail was never written. A
reader erasing and undoing in a loop grew the table forever.

Its twin `note_bodies` had the test (`a_note_in_the_discarded_redo_tail_goes_with_it`) and the
`Command::Renote` arm beside it, so the omission was not a missing idea. The GC's `match` ends
in `_ => {}`, and a new command variant joins the silent arm by default — which is the
mechanism worth remembering: **a catch-all arm makes forgetting the quiet outcome.**

Found while writing the third table of the same shape, by asking what the second one's test
looked like and discovering there wasn't one. The check is one line: for each accounting
observable, grep for a test that *reads* it. An observable with no reader is documentation.

Both arms are now covered by mutations, and the drawing's went in as a **regression** rather
than as a symmetry — the test was written first and went red, which is why this entry can say
the leak was real rather than possible.

### A new test can make an existing mutation's anchor ambiguous, and the anchor never moved

The `anchors` gate exists for two failures it names in its own output: an anchor that has
**drifted** onto code that is gone, and a killed harness that left its **edit** in the tree.
It refused a third on 2026-08-20, and the message's three explanations are all about the
mutation rather than about what actually happened.

A new unit test for the ellipse's appearance stream needed the inset the writer applies, and
wrote the obvious line:

```rust
let inset = OUTLINE_WIDTH / 2.0;
```

That is byte-for-byte the body of `outline_path`, which an existing mutation — *"save: stroke
a box on its edge rather than inset by half the stroke"* — is aimed at. The gate went red with
`anchor occurs 2x in src/save.rs, expected 1`.

**Nothing drifted and nothing was left behind. A second copy of the anchor appeared, in a
file the mutation has no interest in.** The direction is the point: the existing entry above
about near-copies is about *production* code being duplicated, and the reflex when a count
goes to 2 is to look for a stale harness or a moved function. Here the new occurrence was in
`#[cfg(test)]`, added deliberately, thirty seconds earlier, by the person reading the error.

Two things follow. **A test is a place anchors can collide**, so writing an assertion that
restates a one-line production expression is enough to break an unrelated mutation. And **the
fix belongs in the test, not in the anchor**: re-aiming the mutation at a longer string would
work and would move a load-bearing invariant to accommodate a test that could just as easily
say `let rightmost = 300.0 - OUTLINE_WIDTH / 2.0;`.

Worth running the gate immediately after adding any test that echoes a line of production
code. It costs 0.1 s, and the alternative is a mutation nobody can reason about the next time
the harness runs — which is twenty minutes away and reports `SURVIVED` or `MUTATED` for a
reason that has nothing to do with the code under test.

### A new command turns the mutation harness's control red, one layer from where it reads

Registering `edit.drawEllipse` and not placing it in a menu is exactly the state
`menubar.test.ts`'s *"gives every registered command a menu or a written reason"* was written
to catch, and it caught it. What is worth recording is **where the failure surfaced**.

It did not arrive as a red test in a `vitest` run. It arrived as:

```
[FAIL] the control run is not green: 1 failed, ['gives every registered command a menu or a written reason']
```

— from `mutate_frontend.py`, which runs the suite once *before* mutating anything, precisely
so that a red baseline is never mistaken for a mutation's effect. That control did its job
perfectly, and the reading is still momentarily wrong: the eye goes to the mutation being
tested (an ellipse drawn as a filled rectangle) and asks what about *that* broke the suite.
The answer is nothing. The harness had not mutated anything yet.

The general shape, and it applies to every harness with a baseline: **a control's failure is
a statement about the tree, not about the thing the harness was aimed at.** Read the test name
in the control line before reading anything else — it names a defect that exists on `main`
with no harness running, and it would have gone red in the plain `vitest` gate a minute later.

The same pairing happened with the eraser, from the other side: its window check went red on
*"every registered command is classified"* with `[edit.erase]` unclassified. Two harnesses,
two different lists a new command has to join, and neither is the test suite — which is why
adding a command reddens something surprising every time.

### A new kind that is a near-twin inherits a predicate written when it had no twin

`viewer_check.py` samples each mark kind as `{whole, core, edges}` and gives each one a
bound. The box's is `whole < 0.3 && core < 0.05 && edges === 4` — a frame: little ink, an
empty centre, all four sides carrying some.

Every one of those is **also true of an ellipse**, and not approximately. An ellipse touches
its bounding rectangle at the middle of each side, which is exactly where `edges` samples; its
centre is as empty as a box's; and its ink covers a similar fraction of the rectangle. So the
obvious move when adding the kind — give it the box's predicate, it is the box's sibling — is
a check that **cannot fail**, and the mutation it would need to catch (`strokeRect` where the
ellipse should be traced) passes it cleanly while the saved file stays correct, so nothing
else in the repository sees it either.

The same hole existed independently in the writer, against a different wrong implementation:
`--mode outline`'s three readings are satisfied by a `re` operator emitted for a `/Circle`.
Two languages, two mistakes that cannot share evidence, one missing observable.

The observable is the **corners**. A rectangle inks all four; an ellipse inks none. Added to
both harnesses, and in both it is asserted in *both directions* — the box's `corners === 4`
sits on the line above the ellipse's `corners === 0`, because an emptiness assertion whose
control never runs cannot tell "the corner is clear" from "nothing was drawn at all".

**The question to ask when adding a kind to a family**: not "does the existing predicate hold
for the new one" — it usually does, that is what makes them a family — but *"what reading is
different, and is any check taking it?"* If the answer is none, the new kind's check is
decoration, and it will report green for the whole life of the defect.

A related trap one level up says a fixture where the right rule and the wrong rule agree
cannot tell them apart. This is the same failure arriving through the *predicate* rather than
the fixture, which is worth separating because the fixture here was fine — `comments.pdf`
renders both shapes perfectly, and the numbers were sitting there unread.

### A test named for the population it covers is renamed by every kind you add

One test in `save.rs` loops over the highlight, the underline and the strikeout and asserts
each fills its rectangle rather than stroking it. The body has never changed. Its name has,
twice, in two days, and both names were true when written:

- `only_a_box_is_stroked` — accurate until the **ellipse** was added, which is also stroked.
- `the_text_markup_kinds_fill_and_are_not_stroked` — accurate for about six hours, until the
  **squiggly** was added, which is a text-markup kind and is stroked.

The second rename is the instructive one, because it was made *deliberately, to fix exactly
this problem*, and it reproduced the problem immediately. Both names described the set of
kinds that happened to satisfy the assertion. A set is what the next kind changes.

It is now `the_wash_and_the_rules_fill_rather_than_stroke`, which names the property: these
three kinds fill. That survives a fourth stroked kind, because it never claimed to be about
all of anything.

**The damage is real even though nothing fails.** The body stays correct, every gate stays
green, and a reader grepping `the_text_markup_kinds_fill_and_are_not_stroked` concludes that
a squiggly fills its rectangle — which is the opposite of true, and is exactly the sort of
thing someone checks a test name for rather than reading the loop.

The rule: **name a test for the property it asserts, not for the population that currently
satisfies it.** "Only X does this", "all the Y kinds", "every Z" — any name with a
quantifier in it is a claim about a set, and it is a claim nothing enforces. A name is read
far more often than a body, and no gate compares the two.

The mirror case is worth keeping in view: where a test *does* enumerate a closed set, say so
and make it closed. The quad-carrying loop next door lists all four markup subtypes because
PDF 32000-1 defines exactly four, and its comment says that — there, "the whole list" is a
fact about the specification rather than about what has been implemented so far.

### A mutation that ANDs with true has changed nothing, and SURVIVED is then correct

Adding a field to a guard wants a mutation that weakens it. This one was written to make
`textbox::encodable` accept everything:

```rust
// anchor
    text.chars().all(|ch| {
// replacement
    text.chars().all(|_ch| true) && text.chars().all(|ch| {
```

It landed cleanly, the file compiled, and the harness reported **SURVIVED — no test
noticed**. The harness was right and the code was fine: `true && original` *is*
`original`. The mutation changed the source and changed no behaviour.

This is a fourth member of the family this repository already records — the edit that
never landed, the perturbation below display precision, the malformed mutation credited
to the wrong gate — and it is the one that leaves the least evidence, because every
mechanical check passes. The edit is present, the digest moved, the occurrence count is
one, the build succeeded. Only the semantics are a no-op.

**Two habits.** When a mutation SURVIVES, read the mutated source before concluding
anything about the tests — the diff is the first suspect, not the last. And prefer a
mutation that *replaces* a predicate over one that composes with it: `let _ = ch; true`
cannot accidentally preserve the original, where anything joined by `&&` or `||` might.

The same run produced the honest kind of survivor beside it, which is what made the
contrast readable: *"put a font in every mark's appearance resources"* really did weaken
the writer, and survived because nothing tested the claim — a comment saying only one
style gets a font, with no assertion behind it. Two survivors, one a defect in the
mutation and one a defect in the suite, and they look identical in the output.

### A mechanical edit keyed on a field name hits every occurrence of that name

Adding `lines` to `MarkView` meant adding it to every fixture that builds one — about
fifteen literals across nine files. Done with a regex on the line above it:

```python
m = re.fullmatch(r'(\s*)note: (.*),', ln)
```

Every `MarkView` literal has a `note:`. So does a **function parameter list**
(`swatch(note: MarkPopup, name: string)`), a **`NewMark` request payload** — the wire
struct going the *other* way, which has no `lines` — an **`invoke` assertion** checking
the arguments of a call, and the **`INK_CHECK` table**, whose `note` key is the name of a
check about comment bubbles. Six wrong insertions in all.

Nothing was lost, because the type-checker caught three and `vitest` caught two more, and
each failure named the file and line. The cost was five rounds of correction on an edit
that looked like one command.

**The property name is not the type.** `note:` identifies a `MarkView` only inside a
`MarkView`, and a regex has no idea what it is inside. Where the edit is "add a field to
every literal of type T", the tools that know about T are the type-checker and the
compiler — so the cheapest correct method is often to **add the field as required, run
the type-checker, and fix exactly what it names.** That is a list of real construction
sites, generated by something that can tell a literal from a parameter.

Worth doing the other way round when the field is optional, since then nothing goes red
and the mechanical edit is the only method available — which is an argument for making
such a field required while the edit is being made.


### A getter that answers from the rows it was handed cannot see a panel that drew one

`MarkList.rowCount` was `this.rows.length` — the rows the panel had been *given*. The
window check written for it compares that number against the marks the harness handed
over, so it was comparing an input with itself, and a mutation that drew the first row
and stopped survived it:

```
[SURVIVED] marklist: draw a row for the first mark and stop
    expected 'the marks panel lists every mark the reader has made' to fail; 0 did: []
```

The panel really did draw one row out of two. Nothing about the check was subtle — it
reads `sidebar.marks.rowCount === marks.length` — and it could not fail, because the
getter's number arrives from the same place the expectation does. Counting the elements
carrying a `data-id` out of the list element fixes it, and the mutation is then caught.

**The same file already had the correct version of this, one method below.** `rowText`
carries a comment saying it reads the DOM back rather than reporting the source, *"for
the reason `results.ts` gives: a getter returning the source would agree with itself
whatever the row actually contains"* — written by the same hand, in the same increment,
about the getter three lines further down. Knowing the rule is not applying it: the
question to ask of every accessor a check reads is **which side of the operation this
number comes from**, and `rowCount` sounded like a fact about the panel while being a
fact about its argument.

**The defect was inherited, and so was the unfalsifiable check.** `MarkList` was written
from `CommentList`, whose `rowCount` was the same expression — so *"the sidebar lists
every comment"* was equally unable to see a comments panel that drew one row, and had
been since that panel was built. It was fixed in both, and proving the second one needed
a new `viewer-comments` runner in `mutate_viewer.py`: the comments checks `[SKIP]` on a
document with no comments, so a mutation aimed at one on any other corpus is aimed at a
check that cannot go red. That is the fourth constant of its kind in that file.

The general form: **when a mutation survives, ask what the check's two operands are
before strengthening anything.** Here both were the same value under two names, and no
amount of tightening the comparison would have helped.

### A count of the tabs cannot see that one of them is clipped out of the panel

The sidebar had four tabs and 260 pixels. The fifth made five, and the labels then wanted
293 px of content — Outline 58, Pages 50, Results 57, Comments 78, Marks 50 — which with
the row's own padding and gaps is 318 against 247 available. The row did not wrap, and the
host carries `overflow:hidden`, so **Marks** was clipped: present in the DOM, carrying
`role="tab"`, `aria-selected` and its click handler, and unreachable by a pointer.

`SIDEBAR_TABS` had gone from 4 to 5 in the same commit, and *"the sidebar has a tab for
pages"* — which counts `[role="tab"]` elements and checks that exactly one is selected —
passed throughout. It could not do otherwise: **a clipped button is still a button**, and
every property that check reads survives being invisible.

The reading that sees it is geometric, and it takes two, because they fail differently:

```ts
const clipped = tabs.filter((t) => t.scrollWidth > t.clientWidth + 1);
const outside = tabs.filter((t) => t.getBoundingClientRect().right > bar.right + 1);
```

A button whose *label* does not fit is unreadable; one whose *box* runs past the panel is
unpressable. It went red on its first run, on the defect it had just been written for.

**The fix is to wrap the row, not to trim the padding.** Removing the buttons' horizontal
padding entirely gets the content to 254 px against 247 — a fit by six pixels, dependent on
the system font and on nobody ever adding a longer label. Wrapping is correct at every
width and self-adjusts if the labels change; the cost is a second row of chrome exactly
when the labels genuinely do not fit on one.

Two general points. **Arithmetic predicted this before it was measured and measuring is
still what settled it** — the estimate was 316 px against a measured 318, close enough to
have been believed and not evidence. And when a container clips, *every* check that reads
its children by identity rather than by geometry is blind to the clipping; the same is true
of `overflow:hidden` on a list, a toolbar or a status row.

### Two synthetic marks addressed by page land on top of each other on a one-page corpus

The marks-panel phase builds two: one on the first page to press, one on the last page to
navigate to. On `comments.pdf` that is pages 1 and 8 and everything works. On
`links-cropped.pdf`, which has **one** page, "the first page" and "the last page" are the
same page — and both marks were placed at the same 6% band, so they occupied the identical
rectangle. The press aimed at the first opened the second, and the check reported
`selected=4247 for #4246`.

Nothing was wrong with the panel. The phase had two subjects that must be distinguishable
and separated them on **one** axis, the page, which a corpus is free to collapse. Two
marks meant to be told apart have to differ on every axis the phase addresses them by:
different page *and* different height, so that a document with one page still has two
distinct subjects.

Related but not the same as *"Whatever a fixture is meant to discriminate, it needs two
of"*: there were two of them. They were two of the same thing.

Found by the corpus sweep and by nothing else. The phase was developed against
`comments.pdf`, which has eight pages, and passed there every time.

### A reading in fractions of a rectangle cannot test something that is a fixed size

*"a text box draws its words and not its rectangle"* was red on four of the fourteen
corpora — `vector-heavy`, `vector-multi`, `rotated-90`, `links-cropped` — against a painter
that was drawing correctly on all four. Its predicate was

```ts
r.whole > 0.02 && r.whole < 0.6 && r.edges === 0 && r.second > 0.005
```

and every one of those readings is a **fraction of the mark's rectangle**, which was
`height_pt * 0.04` tall — so it scaled with the page. A text box's content does not scale:
it is 11-point type on A4 and 11-point type on A0. The mismatch failed in both directions
at once, which is why no single bound could have been adjusted to fix it:

* On the A0 corpora the box was about 1,073 x 135 points, so two lines of type rounded to
  **0%** of it against a bound of 2%, and the check read *nothing was drawn*.
* On a 20-pixel-tall box the *sample* moved into the type. `edges` reads the middle tenth
  of the height, which is where a rule through the centre would be — and where a two-line
  box's **second line** sits. The check read *the rectangle was drawn*. On A4 it passed by
  half a point, which is the margin that made it look fine for a week.

The repair has two halves and the second is the interesting one. The readings became
absolute point offsets from the box's own corner — two type-sized bands where the lines
must be, and three border strips that must be clear — written as **literals**, because a
band derived from `TEXT_SIZE` and `TEXT_LEADING` moves with them and stops being able to
fail. The literals are then a claim about the type, so the check refuses to run at all if
those three constants are not what it was written against. And the fixture rectangle became
a fixed 260 x 90 points instead of a fraction of the page, which is what lets the literals
be literals and what removes the precondition that a page-relative box would need — on
`rotated-90` a 24-point-tall box cannot hold a second line, so the property a mutation had
defeated could not have been asserted there at all.

**The height came from the sampler, not from the type.** Two lines end 28.4 points down, so
40 seemed right, and it turned the red check into a *skipped* one on the A0 corpora: `inked`
refuses a region under two pixels, `core` reads the middle tenth of the height, and A0 fits
a 900-pixel window at 0.37 pixels per point — 1.5 pixels. The whole reading came back
`null`. A fixture's size can be constrained from a direction that has nothing to do with
what it is a fixture of, and a skip is the failure shape that looks like success.

Measured after: the two type bands read 38%/40%, 27%/27%, 25%/23% and 23%/23% across boxes
from 70x24 to 320x116 pixels, and the border strips are clear on all of them. Three
mutations, all caught — draw only the first line, fall through to `fillRect`, start the type
one line lower — and the third exists because the first two both leave the top band inked.

### A cross-check that type-checks the other platform does not lint it

`scripts/check_windows.py` exists because a Mac compiler never parses a `#[cfg(windows)]`
line, so fifteen green gates here say nothing about that half of the tree. It ran `cargo
check --target x86_64-pc-windows-msvc --all-targets`, which was enough for what it was
written for — a type error in a Windows-only file — and not enough for the thing that
actually shipped next.

`examples/annot_probe.rs` defines `const TEXT_SIZE: f64 = 11.0;` and reads it from exactly
one place, `preview_pdfkit`, which is `#[cfg(target_os = "macos")]` because PDFKit is. On
Windows the constant therefore has no reader, and the `clippy` gate denies warnings:

```
error: constant `TEXT_SIZE` is never used
error: could not compile `tpdf` (example "annot-probe") due to 1 previous error
```

16/16 on this Mac, 15/16 on `windows-2025`, clippy the only red one. Caught by a rehearsal
tag — `v26.8.6-rc1` — at a cost of a 25-minute round trip, which is exactly what that step
of the release checklist is for.

**The gap is one layer inside the gap the script was written to close.** Dead code is not a
type error, so a cross-*check* cannot see it; only a cross-*lint* can. And the direction is
peculiar to conditional compilation: the same constant is perfectly alive here, so nothing
local can go red, and no amount of reading the file tells you which arms the other platform
keeps. The script runs `cargo clippy ... -- -D warnings` now, matching what the `clippy`
gate denies, so the two legs agree about what a failure is. It costs about what `check` did.

Proved by control in both directions rather than assumed: with the `#[cfg]` removed the
script reports `[FAIL] the Windows tree does not type-check (exit 101)`, and with it back it
reports `[OK]`.

**The general form: a stand-in for another platform is only as strong as the *command* it
runs there, not as strong as the target it names.** Anything the real gate list does that
the stand-in does not — a lint, a test, a link — is a class of failure the stand-in is
structurally unable to report, and it will read as coverage.

### A label the platform writes is compared against a label we write by nothing

A reader sent a screenshot on 2026-08-21 with **two items named "About tpdf"** in the
application menu, one above the other, separated by a rule. Both were real and both worked:
the first opened the macOS panel, the second wrote `tpdf 26.8.6` into the header.

The two halves were written six weeks apart and neither could see the other. `menu.rs` put
`PredefinedMenuItem::about` at the top of the application section, and **never names it** ---
the label is the OS's, derived from the bundle name, so no string in that file says "About".
`app.about` was added on 2026-08-19 with the version display, declared in `appcommands.ts`
with `title: "About tpdf"` and placed first in `menubar.ts`'s application section, where it
belongs: it is the only answer to "which version is this" on Windows, which has no such
panel. Each side is correct alone.

**Every test in both languages checks ids, and a reader reads labels.** `menu.rs`'s tests
cover the wire shape of the spec; `menubar.test.ts` covers placement, accelerators and the
four bindings the menu may not claim; `appcommands.test.ts` sweeps every registered command
for an action and a rank. A duplicate *title* is invisible to all of them, and would be even
if both lists were in one language --- nothing compared a label against a label.

The only place both lists exist at once is the menu bar itself. `scripts/menu_check.py`
reads it with System Events and asserts no two items in one menu share a name, which is an
invariant neither source can state. Same argument as `examples/backend_probe.rs` against the
dynamic linker's image table: when two subsystems each hold half the answer, measure the
artifact they both write into.

Proved against the real binary in both directions rather than against a fixture: the
pre-fix bundle rebuilt from `git checkout -- src-tauri/src/menu.rs` reports
`[FAIL] the tpdf menu carries "About tpdf" more than once` and exits 2; the fixed one exits
0. The script also carries both measured menus as a `--self-test`, so the rule can be shown
to fire without a build.

**Ours was the one kept, and that is the general shape.** The reflex is to drop your own item
and defer to the platform, and it is wrong here: the palette offers "About tpdf" on both
platforms, so deferring would have left one name meaning two things depending on which
surface the reader used --- the same defect moved rather than fixed.

### An AppleScript loop over a property list iterates a reference, and every menu reads as empty

The first two runs of `menu_check.py` reported `[FAIL] the tpdf menu is empty` for all eight
menus of an application whose menus were demonstrably full --- the same script had printed
their contents minutes earlier from a one-line `osascript`.

`repeat with nm in (name of every menu item of menu 1 of mbi)` does not iterate a list. It
iterates a **reference** into the property, and reading an element raises
`Can't make item 1 of name of every menu item ... into type specifier. (-1700)`. The loop sat
inside a `try` --- there because a menu bar item without a menu is legal --- so the error
aborted the whole menu and left it looking like a menu with nothing in it. Materialising the
list first (`set nms to name of every menu item of menu 1 of mbi`, then index it) is the fix.

Two things worth carrying:

- **An instrument failure wore the shape of a finding.** Eight empty menus is a plausible
  defect --- a menu that failed to build is exactly what a broken `set_menu` looks like ---
  and the run was one keystroke from being reported as one. What settled it in ten seconds
  was that a *direct* read of the same menu worked; when a harness and a one-liner disagree
  about the same observable, the harness is the suspect.
- **A separator's name is `missing value`, and `missing value as text` raises -1700 too**, so
  the same `try` swallowed a second, independent bug. Two causes with one symptom, in six
  lines of AppleScript.

The reason the empty case is a `[FAIL]` and not a `[SKIP]` in that script is this run: an
empty read is the reassuring branch, and it was wrong both times it appeared.

### A harness that edits source files pays for the editor watching them

`mutate_rust.py` was measured on 2026-08-21 at **69 s per mutation**, which over a table of
231 is 4.4 hours. The reader's objection was the right one --- nobody can pay that per feature
--- and almost none of it was the mutations.

**The larger half was the editor.** Every mutation writes a file under `src-tauri/src`, and
VS Code with rust-analyzer open answers each write with `cargo check --workspace
--all-targets`. That check takes the build directory's lock, so the mutation's own
`cargo test` waits for a whole-workspace re-check before it can start. Cargo says so in as
many words and the line is easy to miss in a harness that captures output:

```
Blocking waiting for file lock on build directory
```

Measured either side of it: a no-op `cargo test --lib --no-run` took **28.2 s** while the
editor's check was running and **0.2 s** with it idle. The fix is one environment variable ---
`CARGO_TARGET_DIR` pointed at a directory of the harness's own --- and it is better than
asking a human to disable their tooling, because it holds whether or not they remember to.
One cold build, 42 s, and it is warm from then on.

**The smaller half was running 607 tests to check one assertion.** Each mutation names the
single test it expects to redden; the harness ran the whole filtered suite regardless. Timing
each module says where that went: `save::` 32.4 s, `print::` 32.3 s, `keylayout::` 17.0 s, and
**the remaining fifteen modules 0.1 s between them**. Twelve tests reach macOS frameworks, and
a `sample` of one shows it parked in `TISCopyCurrentKeyboardLayoutInputSource` for its entire
run. So ~35 s of every mutation bought the same twelve framework waits over and over.

Running the named test alone is safe **only with the fallback**, and the fallback is the
interesting part: when the named test does *not* go red, the full suite still runs, because
"nothing noticed" and "something else noticed" are different findings and the second one has
to name what went red instead. It also covers a case the narrow run cannot see on its own ---
a test whose outcome depends on its neighbours running.

Both together: **405 s for all 229 runnable mutations**, 0 survivors, and zero fallbacks. The
general shape, for the next harness that edits a tree in place: **measure what else is
watching that tree.** A file-watcher, a language server, a sync client and a backup daemon all
answer a write, and a harness that writes thousands of times pays each of them thousands of
times --- while every number you take reads as the cost of your own work.

### Narrowing a run made a shape the output parser had assumed away

`mutate_frontend.py` was changed on 2026-08-21 to run only the test file holding the test
each mutation names, instead of all twenty. The first full run afterwards reported one
mutation of 322 as

```
[FAIL] search: let the plain search match case: no summary line -- the run did not finish
```

which is this harness's most alarming verdict: it means the run produced nothing readable,
and the usual cause is a transform error. The run had in fact taken **16 s** and the mutation
had been caught perfectly --- two tests red, exactly the one it named among them.

The parser was reading vitest's count with
`^\s*Tests\s+(?:(\d+) failed)?.*?(\d+) passed`, and that had been correct for every run it had
ever seen. Narrowing broke it in a way narrowing was bound to: with twenty files in the run
something always passed, and with one file where *every* test fails the line reads

```
      Tests  2 failed (2)
```

with no `passed` segment at all. The regex matched nothing, and "no summary" is reported as a
broken run rather than as a parse failure --- which is the right default and is exactly what
makes this hard to read.

**The general shape: a parser encodes properties of the runs it has seen, not of the format.**
Any change that makes the input *smaller* --- one file instead of twenty, one test instead of
607, one page instead of a document --- can produce a shape the wider run never took: an empty
list, a missing section, a zero where there was always a number. Ask what the narrowed input
can now print that the wide one never did, before reading its first result as a finding.

The fix keeps the strict half deliberately. The count is now read out of the line's body, but
the line still has to contain `failed` or `passed` to count as a summary at all, so a
transform error printing `Tests  no tests` stays unreadable rather than parsing as zero
failures --- which would report SURVIVED for a run that never executed a test. Proved on five
shapes including both of those, and on `Test Files  1 failed (1)`, which must not match.

And the diagnosis order is the point: the run was the first with the change in it, so *"is
this failure mine?"* came before *"is this a defect?"*. It was mine, and the answer took one
reproduction and one hand-applied mutation --- against a full re-run of the table, which would
have reproduced the symptom and explained nothing.

### The plan said the words had to be extracted, and the model had never let them be lost

`docs/PLAN.md` §8 carried this as the marks panel's next piece, in its own words: *"the text
a text-markup mark **covers**, which is the row content a reader would actually recognise and
**needs extraction per mark** rather than the note"*. The first half is the feature and was
right. The second half is a mechanism, and building it would have been the wrong program.

Extraction means asking, for a rectangle on a page, which characters lie under it. That is a
real problem with a real cost --- a page-text request per marked page, queued in front of the
tiles a reader is waiting on, for pages `TextCache` may have evicted; rows that fill in at
different times; and, worst, **a second answer to a question something else already answers**,
since the rectangle was made by mapping a character range the other way. Two mappings that
disagree is the drift this repository records under *"Two copies of a distinction drift, and a
mutation of one survives"*.

None of it was necessary, and one signature says so:

```rust
pub fn open(pages: u32) -> Doc
```

`Doc::open` takes a page count and nothing else. **The model never loads a file's
annotations**, so every mark in this panel was made in this session, and a saved document
reopened puts its annotations in the *comments* panel --- which `annots.rs` fills by scanning
the file, and which is where the "read it back off the page" reflex comes from. There is no
mark in this list whose creation tpdf did not watch, and at that moment `markSelection` is
holding the selection that produced the rectangles. The words come out beside them, from the
same range, in one line of `selectionQuadsByPage`.

**The general shape, and it is the second time in two increments.** A *Not done* line is
written when the surrounding work is fresh and the thing itself does not exist; it is a good
statement of the goal and a **guess** about the mechanism, made before anyone looked at what
the mechanism would have to touch. The previous increment's line called panel removal *"a
second removal path beside the note box's own"*, and building it showed that for a mark with
no page it is the **only** path --- the opposite of a duplicate. Both were wrong in the same
direction: the plan reasoned from the feature, and the answer was in the model.

So read a *Not done* as naming the outcome, not the method, and spend the ten minutes on the
data structure before designing to it. The cost of not doing that is not a wasted hour; it is
a shipped second implementation of something that already had one.

The trade this one does make is worth stating rather than discovering: because the words are
held beside the mark rather than derived from the page, a mark that came back from a saved
file has none, and its row says *"No note"* exactly as before. That is not a gap the
extraction build would have closed either --- a PDF has no entry for the text a highlight sits
on, so such a mark's words would have to be re-derived from geometry every time, which is the
program this entry is about.

### A *Not done* note outlives the work that closes it, and it is the recommendation nobody re-checks

Written the day after the entry above, on the mirror of the same mistake, and this one cost a
wrong recommendation to the user rather than a wrong build.

`docs/PLAN.md`'s save increment ended with a *Not done* saying that **"nothing warns the
reader that the file changed on disk before they try to save ... §5's identity-plus-mtime
watch is not here"**. Read on 2026-08-21 while ranking what to build next, it named the only
outstanding item whose failure mode was somebody's work disappearing, so it went to the user
as the recommendation with that reasoning attached.

It had been false since **2026-08-19**. §5's own *External modification* section, four
thousand lines up the same file, records that watch as built: `fingerprint.rs` takes the
file's length, mtime and a streamed SHA-256 at open, it rides on `Plan` beside `baseline`,
and three separate checks refuse a save or a copy planned against a file that has moved ---
with a Reload the refusal offers and `recovery.ts` makes reachable. `save.rs` imports it at
line 70 and its comments point back at §5 by name. Nothing about it is subtle or hidden.

**The mechanism is that a *Not done* is written in the section that could not do the thing,
and the work lands in a different section.** The commit that closes it has every reason to
write up where it landed and no reason at all to go looking for a sentence elsewhere that
happens to claim its absence. So the note ages in place, and it ages *silently*: a claim of
absence has no test, no gate and no reader who would notice, because the way you find out it
is wrong is by knowing the answer already --- which is precisely what somebody consulting it
does not.

**A claim of absence is the most expensive kind of stale sentence.** A stale claim that
something *works* gets caught the first time somebody uses it. A stale claim that something
is *missing* gets caught only if somebody builds it twice, and the sentence is read exactly
when the reader has least context. This one was read while planning, which is the moment its
being wrong does the most damage.

Two habits, and the second is the one that would have worked here:

- When closing a gap, grep the plan for the words the gap was described in and strike the
  note where it stands, in the same commit. `docs/PLAN.md` is one file.
- **Before recommending work off a *Not done*, spend one grep confirming the thing is still
  absent.** `grep -n fingerprint src-tauri/src/save.rs` answers this in a second, and it is
  the same discipline the repository already applies to a documented blocker --- *"a list of
  documented blockers can be wrong in the direction that looks thorough"*, where the error
  ran the other way and four such lists were wrong in one week. Absence is a claim about the
  tree; ask the tree.

The note is corrected rather than deleted, with what it said and why it was wrong, because
the sentence that remains true --- nothing *watches* the file while it is open, so the reader
learns at the moment they press Save --- is a much smaller thing than the one it was read as,
and the difference between the two is the whole lesson.

### A refusal the reader needs, reported on a channel that does not exist

`tpdf` shipped four sentences a reader can act on --- *"This document needs a password, and
tpdf cannot ask for one yet"*, *"This document uses a security scheme tpdf cannot read"*,
*"This file is not a PDF, or it is damaged beyond reading"*, *"This file could not be read
from disk"*. `open_failure` in `progressive.rs` picks one from PDFium's error code, and it
had been right about which one since it was written.

None of them ever reached anybody. Reported 2026-08-21 against 26.8.6 on Windows, dropping a
supplier's RoHS certificate onto the window:

```
worker stopped answering (exited with 1 (0x00000001))
```

**The mechanism is one `?`.** `worker_child::serve` opened the document with
`RawDocument::open_bytes(bindings, bytes)?` above the request loop, so a refusal returned
`Err` from `serve`; `main` wrote `[worker] <the good message>` to stderr and exited 1. The
parent, waiting on its `Request::Open`, read a closed pipe and could say only what it knew:
the epitaph. And **a GUI process has no stderr** --- already a trap in this file from
2026-08-19, where it made the installed application unable to open any document at all --- so
the one sentence that named the cause was written to a handle that is not connected to
anything.

Two things made it survive. `Workers::open` has carried
`if !response.ok { return Err(response.error) }` from the beginning, and that branch is
correct, complete and **unreachable for the failure a reader actually meets**: the worker
cannot answer a request it dies before reading. And every harness that opens a document opens
a *good* one, so nothing exercised the path at all.

The fix is that a worker with no document answers rather than dies --- `refuse(&reason)`
replies `Response::err(reason)` to each request until stdin closes, and exits 0, which lights
up the branch that was already there.

**It was never a Windows defect.** The code is `cfg`-free, and it reproduced on macOS in one
command with two different causes:

```
$ qpdf --encrypt secret secret 256 -- testdata/comments.pdf locked.pdf
$ worker-probe locked.pdf
[worker] This document needs a password, and tpdf cannot ask for one yet.
[FAIL] a sandboxed worker opens a document it has no path to  worker stopped answering (exited with code 1)
```

Windows is only where it is *visible*, because there the message goes nowhere. That is the
general shape worth keeping: **a diagnostic written to a channel some deployments do not have
is not a weaker diagnostic, it is an absent one**, and the deployment that has the channel is
the one where you will do all your testing. The same reasoning is why the deploy-time
diagnostic in the sibling repository goes in a response header rather than in the body.

**The regression check is in `worker-probe` rather than in a `#[test]`, and that is forced
rather than chosen.** What is being asserted is that a *process* answers instead of dying,
and `Worker::spawn` re-execs `current_exe` --- which under `cargo test` is a test binary that
does not serve `--render-worker`. The probe is its own worker, so it is the only harness that
can spawn one. It writes nine bytes of `not a pdf` to a temp file, which needs no fixture and
takes the same door a password does.

Two checks, because the first alone is weak: *"a document PDFium refuses is answered rather
than died on"* is satisfied by a worker that answers every open with an empty error, so
*"and the reason names the document, not the worker"* asserts the message is non-empty and
contains neither `stopped answering` nor `exited with`. Reverting the one line turns both red
and prints, verbatim, the string the report quoted.

### A loop that re-attaches to the previous item drops a leading orphan

`fragmentsOf` in `reading.ts` keeps a character PDFium placed nowhere by attaching it to the
character before it --- `trailing.get(last)`, where `last` is the index of the last *placed*
character. Nothing is dropped, and the ranges stay usable as ranges. Its own doc comment says
what happens at the start of a page:

> On a page whose first character has no box there is nothing before it, and it starts a
> fragment of its own with a degenerate box.

It does not. `last` is `-1` there, so the character is filed under key `-1`, and `fragmentOf`
only ever reads `trailing.get(item.index)` for indices that exist. **The character is dropped
from `readingOrder` entirely** --- measured, three characters in, one unplaced and leading:

```
leading  : [1, 2]      <- index 0 is gone
trailing : [0, 1, 2]
```

So it is missing from what a copy produces and from what a screen reader is handed, on any
page whose producer emits a separator before the first glyph. `ownership`, the tagged path in
the same file, does have the backward pass this needs and says why: *"A leading unclaimed
character has nothing before it and takes the first owner that follows."* The two halves of
one file disagree, and only one of them is right.

**How it was found is the part worth keeping.** A new test asserted that `coveredText` does
not take a character with no box, and built the fixture the obvious way --- the unplaced
character first, because that is where the interesting case is. The mutation that deletes the
guard was then reported red in `links.test.ts` and **green in the file the test was written
in**: `coveredText` intersects the covered indices with `readingOrder`, and a character
`readingOrder` never emits cannot be taken however wrong the rule is. The assertion passed for
a reason unrelated to what it was testing.

Two things follow. **A fixture must be placed where the code under test can actually reach
it** --- moving the unplaced character to the end made the same mutation red in the right
file, and nothing about the assertion changed. And **the mutation harness naming which file
went red is what made this visible**: "caught" would have been a true and useless verdict, and
the trap *"a check written because a mutation survived has to inherit that mutation's
expectation"* is the same observation from the other side.

The general shape, which is not about PDFs: any loop that fixes up an item by reference to the
previous one has an orphan at the front, and the fix is a second pass in the other direction.
Reaching for `last` and never asking what it holds on the first iteration is how it gets
written.

### A byte grep cannot see inside an object stream, and it returns enough hits to look like it worked

Asked whether a reader's compliance certificate carried a digital signature, the first
instrument reached for was a grep over the file's bytes for `/ByteRange`, `/Sig`,
`/SubFilter`, `/AcroForm` and `/Perms`. All five returned **zero**, and the answer given was
"this document is not signed".

It is signed. It carries an `adbe.pkcs7.detached` signature by a certification authority,
at DocMDP level 1, whose byte range covers the whole file.

The mechanism is not encryption, which is what the same file's `/Encrypt` invited everyone
to blame. **Dictionary keys stay plaintext under `/Encrypt`** --- a reader has to find
`/Root` before it can decrypt anything --- so that reasoning was sound and the conclusion
was still wrong. The blinder is **compression**: the document's second revision uses a
cross-reference stream with compressed object streams, and every one of those five keys
lives inside a `FlateDecode`d `/ObjStm`. A grep over the file cannot see any of them.

**What made it convincing is the part worth remembering.** The same grep *did* return hits
--- two each for `/Creator`, `/Producer`, `/CreationDate` and `/ModDate` --- because the
first revision was written uncompressed. So the instrument produced a plausible, partial,
correctly-formatted answer, which is far more persuasive than returning nothing. A tool that
finds *some* of what you asked for reads as a working tool.

Two habits close it, and the second is the general one:

- **Ask a parser, not the bytes.** `qpdf --json --decrypt` answered the whole question in
  one call, including the byte range and the DocMDP level. `qpdf --show-encryption` decoded
  the permissions in the same second.
- **A negative result from a grep over a binary format is not evidence of absence.** PDF,
  DOCX, XLSX and every zip-shaped format hide their structure behind compression by default,
  and the modern default for PDF *is* object streams. Reach for grep to find something, never
  to establish that it is not there.

### `lopdf::decrypt` removes the entry that says the document is encrypted

`Document::decrypt` ends with `self.trailer.remove(b"Encrypt")` and deletes the object it
pointed at. That is correct --- the strings and streams are now plaintext, so a trailer
still advertising a security handler would describe a document that no longer exists --- and
it means **`is_encrypted()` answers `false` immediately afterwards**.

So a properties readout that decrypts first and then asks what the encryption was reports
*none*, for a document that is plainly locked, with all eight of its permissions gone with
it. The order is the whole of the fix and it is one line apart either way, which is what
makes it easy to get wrong while reading well.

`docinfo.rs` reads the encryption dictionary **before** calling `decrypt`, and the mutation
`docinfo: ask about the encryption after decrypting rather than before` is what keeps that
true. It is worth noticing that this is safe to do: every field a reader wants from that
dictionary --- `/V`, `/R`, `/P`, `/Length`, `/CF` --- is plaintext by necessity, because a
reader needs them before it can derive a key. The security summary is therefore available
even for a document nothing can open, which is exactly when somebody wants it.

The same shape as the crop-box trap two hundred entries above: **PDFium has no "put it
back"**, and neither does this. A value you can only read once has to be read at the one
moment it is there.

### A documented cost measured warm is the wrong number for the run you are about to make

`BUILD.md` recorded `scripts/check_windows.py` as taking **about 8 s**. On 2026-08-21 it ran
for fourteen minutes and was still inside its clippy pass, which held up a commit that was
otherwise finished. The obvious correction --- *8 s warm, a quarter of an hour cold* --- was
written into `BUILD.md` and was itself wrong in both halves, which is the part worth reading.

**Measured afterwards, on the same tree, with nothing else running: 0.47 s.** So the documented
8 s was never the warm figure either. It is the middle of four regimes, all measured on this
machine on the same day: **0.47 s** when nothing has changed, **~8 s** when local Rust
recompiles, **2 min 58 s** after three crates were added, and **minutes more** when the whole
dependency tree has to be built for `x86_64-pc-windows-msvc` --- clippy *compiles* rather than
checks, and a cross-target build shares nothing with the host one. A document that records one number for a command with a
three-order-of-magnitude spread is not slightly stale, it is describing a machine state and
calling it a cost.

**And the fourteen minutes is a ceiling, not a measurement, because I had started the command
twice.** The second copy blocked on cargo's build lock for its whole life. Cargo says so ---
`Blocking waiting for file lock` --- but the script captures its output and prints at the end,
so both runs showed one header line and nothing else, and two contending runs look exactly like
one slow one. `docs/TRAPS.md` already carries *"A harness that prints only at the end cannot say
where it stopped"*; this is that entry arriving as a wrong number in a document rather than as a
lost diagnosis.

Three habits, and the third is the general one:

- **Record the regimes, not a number.** Say what tree state each figure was taken on. Either
  half of a warm/cold pair alone points somewhere wrong, and the cheap one points at the more
  expensive mistake: an 8 s step reads as one you can put in a tight loop and would never start
  early; a fifteen-minute step reads as one to fire off before you need the answer.
- **One at a time, and prove it before quoting an elapsed time.** `ps` costs nothing. A duration
  read off a contended run is evidence about your own scheduling, and it is the kind of wrong
  number that gets written down as a property of the tool.
- **Any documented duration for a build-shaped command is a warm figure unless it says
  otherwise**, because that is what whoever timed it had. This applies to every timing in this
  repository's documents, and to `cargo`, `npm`, `vitest` and the mutation harnesses alike ---
  `docs/TRAPS.md` already carries an entry about a harness whose 4.4 hours turned out to be 97%
  an editor holding the build lock, which is the same lesson from the other side: the number
  you were given describes a machine state, not the command.

### A mutation block below the `__main__` guard is counted by the gate and run by nothing

Ten new mutations were appended to `scripts/mutate_rust.py` and landed **after** its
`if __name__ == "__main__":` line. `scripts/check_mutation_anchors.py` reported
`251/251 anchors present exactly once` and went green. A real run registered **241**, and
every one of the ten was silently absent.

The two disagree because they load the file differently, and each is right for itself. The
gate **imports** the module, and an import executes every top-level statement including the
ones under the guard --- the guard suppresses `main()`, not the module body --- so
`MUTATIONS` is fully populated by the time the gate reads it. A real run executes the file as
`__main__`: `main()` is reached at the guard, reads the table as it stands *at that moment*,
runs, and returns. The trailing block appends to a list nobody looks at again.

**Nothing about this is visible.** The anchors are real, they point at real code, and they
appear exactly once; a `--list` would have shown them, because `--list` is inside `main()`
only when the block is above it. There is no error, no warning, and the gate's green line
says the exact number you expected to see.

Same family as *"The gate guarding the anchors reads the file differently from the harness
that uses them"*, which is about newline translation, and it is worth stating that this is
the **second** time that shape has produced a green gate over a blind harness. A guard whose
reading of its subject differs from the real reader's is not a weaker guard, it is a guard
aimed at a different file.

`check_mutation_anchors.py` now refuses any `MUTATIONS` statement below the guard line, in
every table. Proved by planting one: `[FAIL] ... 1 MUTATIONS block(s) below the __main__
guard`, and green with it removed.

Two things to carry:

- **Insert a table block above `def main()`, never by appending to the file.** Appending is
  the obvious mechanical edit and it is the one that fails.
- **A count from the gate is not a count from the harness.** `python3 scripts/mutate_rust.py
  --list | wc -l` is one command and it is the number that decides what runs.

> ⚠ **And the repair to this cost the ten mutations a second time.** Proving the new guard
> fires meant appending `MUTATIONS += []` as a control; undoing that with
> `git checkout scripts/mutate_rust.py` restored the file from `HEAD` and discarded every
> uncommitted change in it, the ten new mutations included. To undo an appended line, delete
> the line --- `git checkout <path>` is not an undo, it is a revert of the whole file, and
> the cross-repo notes already carry that rule for the whole worktree.

### A test pinned a random value out of a generated fixture, and both places it runs hid that

`what_an_independent_reader_says_about_the_same_certificate` asserted
`certificate.serial == "085398B6930734A2C5F6F74C89AACE579C0EE11B"` and
`certificate.from == "2026-07-25 16:52:27 UTC"`, transcribed from `openssl` reading
`testdata/incr-signed.pdf`. It shipped green and it could not have stayed green: the fixture is
written by `make_incremental_pdf.py`, which calls `x509.random_serial_number()` and
`datetime.now(timezone.utc)`. Every regeneration changes both.

**It went red the first time anyone followed `BUILD.md`** --- in this case, an hour later, when a
new fixture was added and the generator rewrote all of them.

The reason it looked safe is the part worth carrying, because **it was green in both places it
runs, for two different reasons**:

- **Locally**, the fixtures on disk were weeks old. `testdata/*.pdf` is gitignored --- *"The test
  fixtures are generated, not committed"* is already an entry here --- so a development checkout
  accumulates whatever it generated last, and nothing ever compares it against what the generator
  would produce today.
- **On a runner**, `scripts/ci_fixtures.py` deliberately does not build the signed fixtures (they
  need pyhanko), so the test hit its `[SKIP]` path and passed without executing an assertion.

A test that skips on CI and reads a stale artifact locally has no place left where it can tell
you it is wrong.

**The fix is not a stabler fixture, it is asserting a different kind of thing.** Two populations,
and they want opposite treatment:

- **What the generator hardcodes is stable and may be pinned**: the common name
  `tpdf spike 0.6 test signer`, that it self-signs, that there is no chain above it. Plus the
  *shape* of the random parts --- a 40-character uppercase-hex serial --- and the discriminating
  property that all five fixtures report five **different** serials, which a parser returning a
  constant could not manage.
- **What must be pinned by value belongs on input the test wrote itself.** The synthetic
  `cms_blob` builder takes the serial bytes and fixes the validity dates, so
  `every_field_of_a_certificate_whose_bytes_the_test_chose` can assert `"010203"` and
  `2026-01-01 00:00:00 UTC` honestly. That is also the *only* arrangement in which a reversed
  serial or a validity read from one end is visible at all --- three bytes, all different, so a
  reversal is not a palindrome.

Two mutations had been aimed at the fixture test and were re-aimed at the synthetic one, which is
the tell that the coverage had been resting on the stale bytes rather than on the assertion.

**The general form: before pinning a value, ask what wrote it.** If the answer is a generator you
also maintain, read that generator rather than the artifact --- `random`, `now()`, a UUID, a
temporary path and a hostname are all values that look like constants in a passing test.

### PDFium's signature enumeration does not walk the field tree, and ours does

`/AcroForm /Fields` is a **tree**. An entry is either a field or a node whose `/Kids` hold
fields, and a fully qualified field name is the `/T` values joined down the chain --- producers
that group fields, Acrobat included, write it that way. `docinfo.rs` recurses, with a depth
bound. `FPDF_GetSignatureCount` reads the array's entries and stops.

So on a document whose signature field sits under a `/Kids` node, **PDFium reports zero
signatures and tpdf reports one**, and the differential that exists to corroborate us instead
reports a count mismatch that reads like a defect in us.

**Established by control, not by inference, and the control is the whole entry.** Two files
differing in exactly one thing --- whether the leaf sits directly in `/Fields` or two `/Kids`
nodes down --- with the same page, the same field dictionary and the same signature dictionary
byte for byte:

```
flat     PDFium found 1 signature(s)
nested   PDFium found 0 signature(s)
```

`qpdf --check` passes the nested file with `No syntax or stream encoding errors found`. Without
that pair the obvious reading is that the fixture is malformed, which is where an hour goes.

Two things follow, and the second is the general one:

- **Write a known limitation down as an assertion, not as a comment.**
  `signature-probe --mode nested` asserts the *disagreement*: one signature here, none there,
  and a certificate still read. It says so in its own output --- *"if this is 1, PDFium now
  recurses and this mode is obsolete"* --- so the day the limitation ends, the check goes red
  instead of a comment quietly becoming false. This repository already carries
  *"A quirk documented as harmless becomes a defect the day its precondition is wired"*; this
  is the same lesson from the side where the quirk is somebody else's.
- **A differential's silence is bounded by the weaker reader.** Every check
  `signature-probe --mode agree` makes is a check PDFium is *able* to make, so the shapes
  PDFium cannot see are exactly the shapes where we are on our own --- and a nested field is
  one of them. `docinfo: do not walk into a field node's kids` is caught by a unit test and by
  nothing else, because PDFium's own answer under that mutation is the mutated one.

⚠ **`qpdf --check` in a pipeline exits with the pipeline's status, not qpdf's.** The first run
here reported exit 1 and it was the `python3` stage failing to parse `qpdf --json` output;
qpdf's own exit was 0. Same family as the `$?`-after-a-command-substitution entry: run the tool
alone before reading its status as evidence about the file.

### A `Decode<'static>` bound is satisfiable by leaking, and nothing goes red

Reading a certificate's extensions means decoding three DER values out of borrowed bytes. The
signature that suggests itself, and compiles, is:

```rust
fn decode_extension<T: der::Decode<'static>>(extensions: &[Extension], oid: &str) -> Option<T>
```

`'static` is not satisfiable from a borrow, so the body has exactly one way out, and it is the
one that was written:

```rust
let owned = bytes.to_vec();
T::from_der(Box::leak(owned.into_boxed_slice()))
```

That is an allocation an attacker sizes, at a count an attacker chooses --- one per extension
per signature per document, inside the process the sandbox exists to contain --- and **every
gate was green with it in the tree**. It is not a crash, not a lint, not a test failure, and
not visible in a diff that reads as "decode three extensions". `cargo clippy -D warnings` has
no lint for a deliberate `Box::leak`; that is what the function is for.

The correct bound is the higher-ranked one:

```rust
fn decode_extension<T>(...) -> Option<T> where T: for<'a> der::Decode<'a>
```

`KeyUsage`, `ExtendedKeyUsage` and `BasicConstraints` own everything they keep, so they decode
from any lifetime and the borrow ends with the call. The leak disappears rather than being
bounded.

**The general form, and it is a reading habit rather than a tool.** When a trait bound demands
`'static` and your data is borrowed, the compiler is not asking you to leak --- it is asking
whether the bound is the one you meant. `for<'a>` is the bound for a type that *can* decode
from any lifetime, and `'static` is the bound for one that must hold its input forever. Reaching
for `Box::leak`, `mem::forget` or a `lazy_static` to satisfy a signature you wrote yourself is
the tell: the signature is the thing to change.

Worth knowing which instruments would have caught it, because the answer is none of the ones
that run here. A leak checker (`valgrind --leak-check`, macOS `leaks`) sees it; `heaptrack` on a
long run sees it; a fuzz corpus of many-extension certificates makes it a memory-growth curve.
The repository has none of those, so the guard is the reviewer noticing `Box::leak` in a parser
--- which is why it is written down rather than fixed and forgotten.

### `trim_text` trims each event, and a value with an entity in it arrives as several

Reading a value out of XML looks like one event and is not. `quick-xml` delivers every `&...;`
as a **separate** `GeneralRef` event, so `PDF/X-4 &amp; later` reaches the reader as five:

```
TEXT      "PDF/X-4 "
GENREF    "amp" -> unescape Ok("&")
TEXT      " later"
```

Two independent bugs follow from that, and each is correct for every value with no entity in
it --- which is nearly all of them, so neither shows up until it matters.

**Taking the first text event reports `PDF/X-4` for a document stating `PDF/X-4 & later`.** A
plausible wrong answer, which is the dangerous kind: the value is a real prefix of the real
value, so nothing about it looks truncated.

**`trim_text(true)` trims each fragment, so the spaces beside the entity disappear.** The same
document then reads `PDF/X-4&later`. The setting is the obvious one to reach for --- it exists
to stop indentation whitespace becoming text --- and it silently corrupts every value it is
combined with accumulation over. The fix is to leave it off and trim the assembled value once,
where it is stored.

**And the repair that looks right and breaks ordinary documents: refusing every `GeneralRef`.**
The security property being protected is that a custom entity must never expand --- but
`&#233;` is a `GeneralRef` too, so a blanket refusal drops the `é` out of any value that spells
one that way, and marks a perfectly ordinary packet unreadable. The correct rule is narrower:
reconstruct `&{name};` and hand it to `unescape`, which resolves the five predefined names and
character references and returns `UnrecognizedEntity` for anything else. That is one call, and
it is the whole difference between *bounded* expansion and *no* expansion.

Measured rather than reasoned about: printing the event stream for
`<a>AT&amp;T caf&#233; and &lol9; end</a>` answers all three questions in one run, and is what
turned a test asserting the wrong outcome into three tests asserting the right ones.

### A stale binary answered for a source file that was never written

A measuring probe was rewritten and re-run, and printed the *previous* version's output. The
command was:

```
cd src-tauri && cat > examples/probe.rs <<'RUST'
...new source...
RUST
cargo run --release --example probe -- ~/Downloads/*.pdf
```

The shell's working directory was **already** `src-tauri` from an earlier call, so `cd
src-tauri` failed, `&&` short-circuited, and the heredoc never ran. `cargo run` then rebuilt
nothing and executed the old binary, whose output was plausible data of the right shape.

**The failure is silent in both halves.** A failed `cd` prints one line that scrolls past among
compiler output, and a stale binary produces no error at all --- it produces *results*. The tell
was `cargo build` reporting `Finished in 0.15s`, which is not a build.

Two habits, and the first is the cheap one:

- **Absolute paths for every file write.** The working directory persists across calls in this
  harness and is not visible in the command you are writing.
- **When a rebuild is part of the measurement, read the build line.** `Finished in 0.15s` after
  editing a source file means the edit did not land, and it says so before the numbers do.

The same session produced the other half of the same confusion, and it leaves a mess rather
than a wrong reading: `mkdir -p src-tauri/examples` run from inside `src-tauri` **succeeds**,
creating `src-tauri/src-tauri/examples/`. `cd` at least fails; `mkdir -p` is defined to make
whatever is missing, so it builds the nested tree in silence and the file lands somewhere
nothing will look for it. It surfaced as an untracked directory at commit time, which is late
but is at least a place it surfaces.

Same family as the `$?`-after-a-command-substitution entry and the `time`-as-a-pipeline-stage
one: a step that did no work reporting success, with the next step's output standing in for
evidence that it ran.

### A signature blob is trimmed by trailing zero, and BER ends in zeros

A signature's `/Contents` is written by reserving a fixed span and filling it, so the blob is
right-padded with zeros. `docinfo.rs` trims them the obvious way:

```rust
let last = raw.iter().rposition(|b| *b != 0)?;
let trimmed = &raw[..=last];
```

That is wrong twice, and the second one is how it was found.

**A DER blob whose last byte is legitimately `0x00` loses it.** The final byte is the tail of an
RSA signature, so it is uniform: roughly **1 in 256** signatures is corrupted this way, the parse
then fails, and the certificate is reported unread. Rare enough never to be noticed, common
enough to be happening.

**A BER indefinite-length blob loses its end-of-contents markers.** Those markers *are* `00 00`,
so the trim eats exactly the bytes that terminate the structure. Demonstrated on a real CAdES
signature: `openssl pkcs7` parses the padded blob and fails on the trimmed one with *"not enough
data"* — which reads as a truncated file rather than as the reader having truncated it.

The fix for both is to read the outer TLV's length rather than scan for zeros, and the second
half would buy nothing on its own, because:

**`der` refuses indefinite length outright.** `30 80` is a SEQUENCE of indefinite length, which
is legal BER and not legal DER, and the crate says so: `indefinite length disallowed`. One of ten
real signed documents to hand is encoded that way, so tpdf reads no certificate and no timestamp
from it at all. It reports that honestly — `certificates_unread` is 1 — but a whole class of real
signatures is unreadable, and the class is *the* class that carries timestamps, since CAdES is
where timestamping is routine.

Two things worth carrying:

- **A "not enough data" error can be the reader's fault**, and it points at the file. The control
  is one line: hand the untrimmed bytes to a second parser and see whether it agrees.
- **Ask what a format's padding is made of before stripping it.** Zero padding and zero
  terminators are indistinguishable byte-for-byte and distinguishable structurally.

**Closed 2026-08-21 by `src-tauri/src/ber.rs`, and both halves went together because they are
one question.** Where a blob ends and what length form it uses are both answered by walking it:
`to_definite_length` returns exactly the first value, re-encoded with definite lengths, and drops
whatever follows. A blob that is already DER comes back byte-identical, which is what makes it
safe in front of every signature rather than only the ones that need it — and that property is
asserted against the real fixtures rather than reasoned about.

The measurement, on the same contract: the trailing-zero scan gave 46,281 bytes and the structure
gives **46,287** — six bytes, three nested end-of-contents markers, and 8,298 bytes of padding
after them that no byte-level rule can tell from the markers. Before: `cert="(no certificate)"`.
After: `cert="Dropbox Sign"`, the key usage it states, and a timestamp from *Timestamp Authority
2 — Notarius* one second after the signing time. One change, three readers that had never seen a
real CAdES document between them.

### A guard whose neighbour refuses the same input cannot be tested by it

Three mutations of the timestamp reader survived on the first run, and all three were the same
shape: a real guard with no input that reaches it, because something adjacent refuses the same
case first.

- **`eContentType == id-ct-TSTInfo`.** The fixture used to test it was an
  `adbe.pkcs7.detached` signature — which is *detached*, so it carries no encapsulated content
  and the very next line refuses it. Deleting the type check changed nothing. What
  discriminates is a CMS that **has** content the check must reject: the real token with its
  content type relabelled and its `TSTInfo` left exactly where it was.
- **The SEQUENCE tag check on `TSTInfo`.** Fed a bare INTEGER, both versions refuse — the
  four-value walk fails on one byte with or without the check. What discriminates is an OCTET
  STRING wrapping a *well-formed* body: same contents, different tag, and only the check can
  tell them apart.
- **"refuse an attribute carrying several values".** The fixture added an INTEGER beside the
  token, and a `SET OF` is ordered by encoded bytes — `02...` sorts **ahead** of `30...`, so a
  mutation taking the first value got the rubbish and failed anyway. An empty SET (`31 00`)
  sorts *after* the token, so "take the first" and "refuse several" finally give different
  answers.

The general form: **a guard is only covered by an input that gets past everything else that
would refuse it.** Two guards producing one outcome means the weaker one is doing all the
observable work, and no assertion over that input can see the difference. The question to ask of
a survivor is not "is my assertion strong enough" but "does my input reach this line at all" —
and in each of these three the answer was no for a different reason.

A related one, from the same run and cheaper: an ordering assertion built on `indexOf` passes
when the row is **missing**, because `-1` is less than every real index. Assert both operands
exist before comparing them.

### A differential's most important check was hard-coded to pass when both readers failed

`signature-probe --mode agree` reads a signed document twice — once through `lopdf` and once
through PDFium — and compares. Its own module note explains why `--mode clean` has to exist:

> Two readers that both find nothing agree perfectly.

One function away, the certificate comparison ends like this:

```rust
(None, None) => report.check(
    &at("neither reader found a certificate"),
    true,
    "both empty, and the blobs agree about that",
),
```

A `check` whose condition is the literal `true`. On a real CAdES contract, whose signature is BER
and which no parser here could read, the run printed:

```
[OK]   signature 1: neither reader found a certificate: both empty, and the blobs agree about that

7 passed, 0 failed
```

Seven passed on a document where the check that matters most — the one the code comment above it
calls "the one that matters most", because a wrong pick shows a reader the wrong signer — had
nothing to compare. Every signature reaching that loop is one **both readers found** and
`ours.signed` is true for all of them, so two empty answers were never agreement; they were both
parsers defeated by the same blob.

Three things to carry:

- **A reasoning written down in one place does not apply itself in the next.** The clean-mode note
  is the correct argument, in almost these words, about the same failure mode. It was written
  three months before the arm below it.
- **Grep a differential for arms that cannot fail.** A literal `true`, an `assert!(x == x)`, a
  branch whose only statement is a pass — the shape is findable, and in a comparison harness the
  empty-versus-empty arm is where it hides, because that is the arm nobody has a fixture for.
- **A check name that changes with the arm hides it in the transcript.** This one printed
  `neither reader found a certificate` rather than `the same certificate...`, so the run did say
  what happened, in a line a reader scanning for `0 failed` never reads. The repaired arm reuses
  the ordinary name.

The repair is one word, and it was proved by control rather than by reading: with
`parse_certificate` stubbed to refuse everything, `incr-signed.pdf` goes `6 passed, 1 failed`.

### Putting a guard in front of a parser disarms the parser's own guard, and the test still passes

`ber::to_definite_length` was added ahead of every signature parse, and it refuses a blob whose
structure will not walk — counting it, correctly, as unread. One mutation of the *parser's*
counter then survived:

```
[FAIL] docinfo: report an unreadable certificate as an absent one: SURVIVED -- no test noticed
```

Nothing about that counter had changed. Its test fed `30 82 FF FF 01` — a SEQUENCE claiming
65,535 bytes with one present — and asserted `certificates_unread == 1`. That blob no longer
reaches the parser at all: the new walk refuses it first and increments the same counter, so the
assertion held by the wrong mechanism and deleting the one it was written for changed nothing.

This is the neighbour-refuses-the-same-input trap arriving from the other direction, and the
direction is the news. There, the guard was born untestable and a first mutation run found it.
Here it was **testable, covered, and disarmed by an unrelated change** — a change that adds
validation, which is exactly the kind nobody re-runs a mutation table after, because it only ever
makes things stricter.

Two habits:

- **After adding a guard upstream of anything, re-run the mutation table for what is downstream
  of it.** Not the tests, which stay green by construction — the mutations.
- **One input per mechanism.** The fix is a well-formed value that is not a CMS `ContentInfo`
  (`30 03 02 01 41`) for the parser's counter, and the malformed one for the walk's, in a test of
  its own. Two counters that can produce one number need two inputs, or one of them is decoration.

### A test helper that builds its fixture with the encoder under test

`ber.rs`'s unit tests build values with a `definite(tag, body)` helper, and its first draft wrote
the length by calling `length_field` — the function that writes lengths in the module under test.
Mutating that function to always use the long form produced this:

```
[FAIL] ber: write a length in the long form whatever its size: 16 red of 701 tests,
       but NOT the expected one ('a_definite_encoding_comes_back_unchanged')
```

The named test is the identity check — a value that is already DER must come back byte-identical
— and it is precisely a check *on the encoder*. Building its input with the encoder made both
sides move together, so it agreed with whatever the encoder did. Sixteen other tests caught the
mutation, none of them by design.

**That output is the diagnostic, and it is worth recognising on sight:** a mutation reddening many
tests but not the one named for it says the named test's fixture is built by the code under test.
A mutation reddening *nothing* says something else entirely — the input never reaches the line.

The fix is four lines of hand-written length encoding in the helper, covering only what the
fixtures need and panicking on anything else. A test helper may share a *type* with production
code; it must not share the transformation it exists to check.

### One unguarded call to an external program made eleven fixtures that need nothing unbuildable

`make_incremental_pdf.py` writes twelve fixtures. Eleven need only pyhanko; one needs **qpdf**,
to encrypt a two-page document. That one was written like this:

```python
subprocess.run(["qpdf", "--encrypt", ...], check=True)
```

On a machine without qpdf that raises `FileNotFoundError` — and it sits above every signed
fixture in the file, so the script died there having produced *none* of them. A hosted runner is
such a machine, which is why CI tested no part of the signature reader: not the certificate, not
its extensions, not the timestamp, not the BER walk. Five increments of work, checked on one
laptop.

The reading that hides it is that the missing thing is the *fixture*. It is not: the missing
thing is everything sequenced after the fixture. **An optional step that can throw is a
mandatory step for everything below it in the same script**, and the cost is not proportional to
what depends on it — here one dependant made eleven independents unreachable.

Two more things came out of building the fix, and both are about the instrument.

**A "did it generate?" check is satisfied by files that were already there.** `ci_fixtures.py`
runs a generator and then asserts the artifact exists, which is the right assertion and says
nothing on a machine that already has it. The first run of the new path took **0.65 s** and
reported nine fixtures `[OK]` — with pyhanko not installed, on files from an earlier run. It
proved nothing. The measurement that means something is taken with the artifacts moved aside:
pyhanko absent gives exit **1** and `exited 0 but testdata/incr-signed.pdf does not exist`;
pyhanko present gives exit 0 and nine files.

**The fixtures are not reproducible — and the half I got wrong is the instructive half.** Two
consecutive runs *on one machine* produce nine files of identical size and differing bytes,
because pyhanko mints a fresh key pair and serial each time. I measured that, wrote *"a test may
pin a size and must never pin a serial"*, and pushed. Both CI legs went red on a pinned size:
`incr-signed.pdf` is **8,097** bytes on both runners and **8,128** here, on the same commit.

**A two-run comparison on one machine cannot see a per-machine constant.** It is the control you
reach for, and it holds every part of the environment fixed — which is precisely what the
question turned out to be about. Answering it needs a second machine, and CI was the second
machine, so the right reading of that first push is that it worked.

What replaced the pinned numbers is a quantity **derived from the file at test time, by a route
the code under test does not take**: `docinfo` reads `covered_bytes` from `/ByteRange`, and the
test sums where the `/Contents` hex actually sits. A whole-file signature covers everything but
that span, so the two must agree, and neither is a constant anyone transcribed. Proved able to
fail by a mutation that counts only the first range pair.

Its first draft was wrong in a way worth keeping: **`/Contents` is also a page key**, naming the
content stream, so taking the next `<` after every occurrence read an unrelated dictionary and
over-reported by 181 bytes. A scan for a PDF key is a scan for a name that several kinds of
object share.

**Then it happened a third time, and the third one is a flake rather than a constant.** The next
push went green on macOS and red on Windows: `assert_eq!(certificate.serial.len(), 40)`, against
a serial of **38** hex characters. pyhanko mints a random serial of at most 20 bytes and DER
drops leading zeros, so roughly **one run in 256** produces a 19-byte one — per fixture, five
fixtures, on every machine. It is not a per-machine constant like the size; it is a coin that had
not come up yet, and the two legs disagreeing on the same commit is the tell.

So the class has three members and they fail differently: a value that differs **per run** (the
serial's bytes), one that differs **per machine** (the file size), and one that differs
**rarely** (the serial's length). Only the third can sit green for weeks. The rule that covers
all three is the same: **assert the invariant the generator guarantees, not the value it
happened to produce** — here, hex, non-empty, whole bytes, at most twenty of them, and distinct
between fixtures.

**And the sweep after the third is what should have followed the first.** Enumerating every
assertion in the module that reads a generated fixture takes one script and finds the remaining
ones by inspection rather than by a fourth red run: two pinned strings, both safe — one over a
certificate the test builds itself, one over a timestamp the generator pins on purpose. Three
reds in a row is the signal to stop fixing instances and enumerate the population.

---
### An id and a slot are both `number`, so a mark drawn on the last page vanished

Reported from use: *"I can draw an ellipse, but it will vanish after letting lose
the LMB --- nothing stay in the PDF visually."*

`Viewer.onDrawn` hands over the page's **id**, deliberately, and its doc comment
says so and gives the reason: a drawing is committed at the *end* of a gesture, so
the page has to be pinned by name rather than by position in case the order moved
while the hand was down. `Edits.mark` took a **slot** and did
`this.current.pages[page]?.id` with it.

An unedited document numbers slot 0 as id 1, so the two are always one apart:

- A shape drawn on any page but the last was written to the **next** page.
- One drawn on the **last** page resolved to `pages[n]`, which is `undefined`, and
  the method returned the state it already held --- no command, no refusal, no
  message. The reader watched it vanish.

**Nothing was red, at four layers.** `edits.test.ts` asserts that `mark` sends an
id and it did; `viewerdraw.test.ts` asserts that `onDrawn` reports an id and it
did; every window check passes because the check harness builds a `Viewer` with no
model behind it, so a drag draws its preview and commits nothing. The defect lived
only in the join, which is inside `App.svelte` --- a file no unit test imports and
no harness constructs. That is the same blind spot the `wiring` gate was written
for after `onDrawn` shipped unwired; the gate compares which callbacks are wired,
and this was a callback wired to a function with the wrong *units*.

Two fixes, and the second is the one that lasts:

- `Edits.mark` takes a page id and sends it verbatim. There is no translation left
  in that path to get wrong.
- **`PageId` is a distinct type** --- `number & { readonly __pageId: unique symbol }`
  in `pages.ts`, erased at runtime, minted only by `pageId()`. Feeding a slot to
  anything that wants an id is `error TS2345` now, and the three call sites in
  `App.svelte` were flagged the moment the parameter changed. Two of them were
  right and one was the defect, which is exactly what a type is for.

The fixture that can see it needs slots and ids to **disagree**: `viewerdraw.test.ts`
re-orders a three-page document so slot 0 holds id 3, a number no slot in it has.
It also has to `goToPage(0)` afterwards --- re-ordering keeps the reader on the page
they were looking at *by identity*, so the view follows id 1 down to slot 1, and a
press near the top of the window would land on the one page whose id equals its old
slot. The first draft of that test did not, and reported 1 where it expected 3.

Generalises past this repository: any two integer keys over the same domain ---
a database id and a row number, a file descriptor and an index into a table, a
1-based display position and a 0-based array position --- are one transposition away
from a defect that no test in either module can see, because each module is right.

---
### A one-shot tool armed from the palette says nothing, and the reader is not stuck but lost

Reported in the same message: *"edit -> add comment adds a speech bubble - but I
can't drag it to move it. I would expect the cursor to become a speech bubble to
place it, instead of adding it always to the top left."*

`edit.addComment` had no gesture. It ran, `commentAt(null)` answered with the
top-left of whatever of the page was on screen, and the mark was made --- a
defensible spot, chosen deliberately and documented, and it reads as a command that
ignored where the reader was pointing.

Three things came out of fixing it, and only the first is about comments.

**A comment is placed by a press, and every other armed tool refuses one.**
`boxQuad` will not build a rectangle from two identical corners, which is right for
a shape and wrong for a pin. The drag-end handler now reads `live.from` for a
`note` and the two corners for everything else.

**A cursor image was the obvious answer and is the worse one.** It cannot show the
size the icon will be, so it would not move with the zoom; it is drawn by the
window server, so it cannot sit under the page's own colours; and a
`url(data:...)` in a `cursor` rule is an image load, which `default-src 'self'` has
an opinion about. What the reader asked for is *feedback about where it will land*,
and a dashed ghost of the actual bubble on the overlay canvas is that --- placed by
`iconQuad`, the same function that places the mark, so the two cannot disagree.

**The status line's own gate could not see a mode change.** `ViewerStatus` is
reported every frame and `onStatus` fires only when a summary string moves ---
and `drawing`, `erasing` and the new `armed` were all **absent from that string**.
Arming a tool moves nothing else in it: no tile becomes pending, the selection
stays empty, the page and the zoom are where they were. So the line that exists to
make a mode visible was told about it only when something unrelated happened to
change, or never. Found while adding a third field that would have inherited it.

The general shape: **a computed digest that decides whether to notify is a second
list of the fields that matter, and it goes stale silently.** Nothing goes red when
a field is added and not listed --- the value is still correct in every reading, it
just arrives late or not at all. If a struct is compared by a hand-written summary,
the summary needs a test per field, or it needs to be derived from the struct.

---
### Moving a mark is a re-inking of it, and reusing the command beat adding one

Dragging a mark to a new place needed a model command and there was none. The
obvious shape is a new `Command` variant carrying new geometry --- and it would have
been the wrong one.

`Doc::quads_of` and `Doc::strokes_of` already answer *where a mark is now* by
looking in `Working::ink_of` first and falling back to the mark's body. That
indirection exists for the eraser: rubbing out a stroke changes both the strokes
and the rectangle round them, so `Ink { strokes, quads }` carries the pair together
and `Command::Reink` swaps versions of it. A move changes exactly the same pair.

So `Doc::displace` translates both and issues a `Reink`. What that buys:

- **One precedence rule.** A second variant would mean `quads_of` choosing between
  three sources, and undo replaying whichever came last --- a rule that is right
  today and drifts the first time somebody adds the fourth.
- Undo, redo, snapshots and the save path all work unchanged, because nothing new
  was added for them to not know about.

Two decisions inside it are worth stating, because both could reasonably have gone
the other way:

- **It takes a delta, not a geometry.** The caller has the gesture and could send
  rectangles --- it is what `annotate` takes. One offset applied here to everything
  the mark owns cannot resize a box or reshape a drawing; a geometry computed on
  the far side of the boundary can do both through one sign error, and the mutation
  that moves a rectangle's top-left corner and not its bottom-right is in
  `mutate_rust.py` because that is precisely what it looks like.
- **Every kind, no shape check** --- `recolor`'s posture rather than `reink`'s, and
  its neighbour refuses a highlight where this accepts one. Geometry is geometry;
  *which* kinds a reader is offered the drag on is a product rule, and it lives in
  `markband.ts`'s `isMovable` where the gesture is. Putting it in the model would
  make that rule unchangeable without a commit in another language.

The cost is a name that has widened: `Ink`, `InkId`, `inks` and `Reink` now carry
every kind's geometry rather than a drawing's. Renaming was measured and refused ---
261 hits across 18 files on a grep that also matches `MarkKind::Ink` and the PDF's
own `/Ink`, which is the mechanical-edit-keyed-on-a-name trap this file already
records. The paragraph on `Ink` says what it means now and why the name stands.

**And the clamp is not in the model.** Keeping a mark on its page needs the page's
size in points, and `docmodel` holds ids, turns and crops --- the size is the
renderer's answer. So the viewer clamps the offset before sending it, in the same
place and for the same reason `iconQuad` and `boxQuad` clamp the geometry they
build. Which means the *offset* is in the page's display space and the *gesture* is
in the laid-out space, and converting between them is two points through
`fileRectOn` rather than one rectangle: a quarter turn swaps a vector's components
and negates one, and the crop does nothing to it at all. The fixture that can see a
viewer skipping that conversion is a turned page --- upright, the two spaces are the
same two numbers and the assertion passes either way.


---
### A harness written on a locked screen is a harness that has never run

`scripts/mark_check.py` and `src/lib/markcheck.ts` were written on 2026-08-22 to
close the gap that let a mark land on the wrong page: the chain from a command,
through a gesture on the real viewer, to the edit model and back to the overlay,
which nothing had ever executed with a document open.

It has not run. `webview_guard` refused, correctly --- the screen was locked, and a
locked macOS session cannot be unlocked from a script by design. So the harness
sits in the state this file already records from the other direction: **a check
that has never executed produces no failures, and neither does one that passes.**
Nothing about it having been written carefully changes that.

Two things were done about it rather than one, and the second is the useful one.

**The half that needs no screen was proved.** The transcript reader has four
independent grounds for refusal --- no summary line, a summary disagreeing with the
exit code, a run that never opened a document, and the keystone check skipping ---
and every one of them exists because the reassuring branch is the wrong answer.
`--self-test` runs six refusals and one acceptance against hand-written
transcripts, plus a control that the name lookup is not keyed on a column width.
That is a real instrument on a real question, and it is available on any machine.

**The gap went into `BUILD.md`, not only into a message.** Beside the invocation,
where the next person reads it, with the mutation that would prove the harness can
go red: reintroduce the slot lookup in `Edits.mark`, rebuild, and confirm the
page-identity check fails. A note in a chat reply is read once; a note in the file
that tells you how to run the thing is read every time somebody runs it.

The general rule, and it is one this repository keeps re-learning from new angles:
**an untested instrument is worth less than no instrument, because it invites the
conclusion it cannot support.** A green line from a harness whose failing path has
never been observed says only that the harness printed something.

---
### Two writers for one document, and the printer got the older one

Printing a document a reader had marked up produced paper with no marks on it,
and printing a page they had cropped produced the page at its full size. Both
had been true since marks existed. Found by reading the print path and then
**measured before anything was changed**: a job built from a plan carrying one
mark and one crop came back with **no page carrying `/Annots` and none carrying
`/CropBox`**, and a plan whose only change was a crop reported
`is_identity() == true`, so `print::select` answered `Pages::All` and the file
went to the printer untouched.

Two independent causes, and it is worth separating them because only one is the
trap in the title.

**`Plan::is_identity` was a list of the ways a document can differ from its
file, and the crop was not on it.** Marks, page count, order, turns --- four
clauses, all correct, written before a page could be cropped. A list like that is
wrong the moment somebody adds a fifth way, silently, and in the direction that
reads as *nothing was edited*. The clause is there now, and so is the mechanism
that makes the next one impossible to forget: the closure destructures
`PageView`, so a tenth field is `error[E0027]` until whoever added it says which
half it is in. **A predicate over a struct should destructure it**, and the cost
of not doing so is a defect nobody can see in a diff.

**`print::build` and `save::planned_bytes` were two implementations of "produce
the working document".** Printing came first and needed a subset --- pages,
order, turns --- so it grew its own page walk. `save.rs` later learned to write
marks and crops. Nothing compared the two, and nothing could: they take different
arguments and return different types, and every test either module has passes
with the other one broken. This is the *two copies of a distinction drift* entry
arriving between modules rather than within one, and the tell was there to be
read --- `print::build` never mentions `plan.marks` at all.

The fix is one writer. `save::print_bytes` is `planned_bytes` with the one input
a print job has and a save does not: the reader's own rotation. `print::Route`
names the three producers, so which one a job uses is a pure function that can be
exercised without a document open --- a check inside a Tauri command has no
failing case a test can reach.

**Routing through the other writer silently dropped a guarantee, and a mutation
is what said so.** Deleting the view rotation from `print_bytes` left every test
green: the route test asserts *which* producer is chosen and says nothing about
what it produces, and every rotation test beside it drives `build`, which is now
the other producer. So moving work between two implementations moves its coverage
too, and the coverage does not follow by itself. Ask, for each property the old
path guaranteed, which test would go red if the new path stopped guaranteeing it.

**And the test written for that survived its own second mutation.** Letting the
view rotation *replace* a page's own turn rather than composing with it passed,
because the fixture had a mark and no edit turn --- so `page.turns + view` and
`view` were the same number. Every ingredient was present except the one that
discriminates. `rotated.pdf` is the only fixture whose pages carry four different
rotations, which is what makes "each page a quarter past where the file had it"
an assertion rather than a tautology; adding an edit turn on one page is what
made the composition observable.

---
### The append was 8.2x in the spike and 1.1x in the application, and the difference is a hash

`docs/PLAN.md` §5 measured an incremental save at **29.1 ms against the rewrite's 239 ms** on
a 337 MB scan and carried that 8.2x as the reason to build it. Built, and measured end to end
in the same A/B shape, it comes out at **637 ms against 672 ms --- 1.1x**, and on a 1.4 MB
document it is *slower* than the rewrite.

Nothing was wrong with either measurement. They measure different things, and the gap between
them is the whole lesson: **the spike timed the writer and the reader waits for the save.**

Timed separately, the streamed SHA-256 the open-time fingerprint takes is **582 ms of the
append's 637** on that fixture. Both modes pay it, so the mode chooses about 55 ms of a
640 ms save. The remaining difference is smaller still: 21 ms reading the file, 6 ms parsing
it, and 43 ms parsing it *again* to verify the result --- a check the rewrite does not perform
at all, which is why the append loses on a small document where there is no write to save.

Three things follow, and the third is the one that generalises.

**A spike's number is a lower bound on a subsystem, never a prediction about a feature.** The
spike had the file's bytes in hand before its timer started, because that is what isolating
the writer means. The application has to get them, verify them, and verify what it wrote.

**Measure the thing you are about to justify, before you justify it.** The 8.2x had been in
the plan for a month and would have gone into a changelog as the reason for the work. What
stopped it was running the A/B against the real path on the real fixtures, which took one
`#[ignore]`d test.

**The claim that survived is the one nobody was leading with.** The append writes **839 bytes
where the rewrite writes 337 megabytes**, and that is worth having for reasons that are not
speed at all: a document in a synced folder is not re-uploaded, the disk is not rewritten, and
the previous revision stays byte for byte inside the new file so a validator can still show
what a signature covered. When a measurement kills the headline reason for a piece of work,
the question is whether a different reason survives it --- not whether the work does.

---
### A field documented as the caller's last look, and read by nobody

`Appended::verified` held a full `Fingerprint` of the source as it was when the update
section was built against it. Its doc comment said, in as many words, that *"the caller's
last look before it writes goes through this field, and a `None` arm could only be written
as 'skip the check'"* --- which is why it is not an `Option`. `append_bytes` populated it on
every call. `append_in_place` never read it.

What actually guarded the write was `metadata(source).len() != appended.was`. A length, and
only a length, on a path whose whole hazard is that a document can be replaced by a
different one of the same size --- which this repository already had a trap about under
*Equal length is not no change*, and which `fingerprint.rs` argues at length in its own
header.

**Three things pointed at the guard and none of them was the guard**, and that combination
is what let it stand for as long as it did:

- the field's type, which cannot express "unchecked";
- the field's doc comment, which describes a check;
- and a comment at the call site in `lib.rs` explaining why no second look was needed there
  --- on the grounds that comparing a length is *"a sharper answer"* than comparing a length
  and a modification time. It is the wrong way round. Two files in the same crate said so.

The mechanical tell is cheap and would have found it in one command: **a `pub` field with no
consumer.** `grep -rn '\.verified' src-tauri/src` returns the rewrite path's use and nothing
for the append. A field that only ever gets written is either dead or a guard nobody makes,
and the second is worse, because the type and the prose both go on claiming otherwise.

The general shape, which is not about saving: **a value carried to a call site is not a check
performed at it.** Plumbing evidence to where a decision is made feels like most of the work
and is none of it. If the decision does not read the value, the plumbing is decoration that
reads as diligence --- and reviews, including several of this file's own, will keep pointing
at the plumbing.

---
### A guard that looks a pathname up again is not a guard on the file you are writing

The fix for the entry above was not only *which bytes* but *which file*. `append_in_place`
opened the file, wrote to it, read it back by calling `Document::load(source)`, and rolled
back by reopening `source` and truncating it. Four operations, four separate lookups of one
name, and a rename between any two of them puts a different file under the later ones.

The costly one is the roll-back: a save that fails after a rename lands `set_len` on whatever
now has that name --- a file this process never opened, never checked and was never asked to
touch, truncated to the length of a document it has nothing to do with.

**A pathname is a lookup, not a file.** Everything now goes through one descriptor: the
fingerprint comparison reads `file.metadata()`, the writes append to it, the verification
seeks it to zero and reads it, and the roll-back is `file.set_len` on it. `Fingerprint`
grew `agrees_with_metadata` for the first of those, sharing one body with `agrees_shallowly`
so the two cannot drift about what "the same file" means.

Two things worth knowing beyond the mechanics.

**The window is inside the function, so the seam has to be an argument.** Between `open` and
the first write there is nothing a test can plant into, because both are statements in one
body. `append_through(&mut File, ...)` is the whole fix for that: the test opens the handle,
lets something else rename over the pathname, and then calls the function. Same shape as
*A guard written inline with an FFI call is reachable by nothing*.

**The intruder in that test must not be a valid PDF.** With a valid one, verifying through
the handle and verifying by name both succeed, so the check stops discriminating and a
mutation that swaps one for the other survives. With unparseable bytes at the pathname the
two routes diverge --- and the by-name route additionally rolls *our* file back, which the
test's last assertion catches. Whatever a fixture is meant to discriminate, it needs the two
cases to produce different observables, not merely different intentions.

**One handle is not one set of rights, and the platform that cannot be tested is where that
bites.** The obvious way to write this is `OpenOptions::new().read(true).append(true)`, and
it is wrong on Windows: Rust maps append mode there to
`FILE_GENERIC_WRITE & !FILE_WRITE_DATA` (`std::sys::fs::windows`, `get_access_mode`), while
`File::set_len` is `SetFileInformationByHandle(FileEndOfFileInfo)`, which needs exactly the
right that mode removes. Every write would have succeeded and every roll-back failed with
*access denied* --- on the platform this repository cannot run a test from, which is where
such a thing survives. `read(true).write(true)` plus an explicit `seek(End(0))` is correct on
both, and it is also the better semantics for the job: the trailer has to land immediately
after the body, which a file offset says and `O_APPEND` does not.

Worth noticing where the risk came from: the code being replaced was *safe* here, by
accident of its defect. It opened one handle in append mode to write and a **second** one in
write mode to truncate --- the by-name reopen that this whole entry is about --- so each
operation happened to hold the right it needed. Collapsing four lookups into one handle is
the fix, and it is also what made one handle have to satisfy four different requirements at
once. A consolidation inherits the union of what it consolidates.

The general form: **a mode is a set of rights, not a verb.** "Append" reads as *what I intend
to do* and is really *what the OS will let me do*, and the two come apart the moment a second
operation on the same handle needs a right the first did not. Read the platform's mapping
before assuming one handle covers write, truncate and read.

**And the last question is about the name, deliberately answered last and without a
roll-back.** If the pathname stopped naming our file, the reader's edits are complete and
correct in the file that had the name when the save began --- which is either unreachable or
living under another name, and truncating it in that second case would destroy the only copy
of the work. So it reports and touches nothing. `FileId` is `st_dev`/`st_ino` on Unix and
`GetFileInformationByHandle` on Windows; `std::os::windows::fs::MetadataExt::file_index`
would answer it too and is unstable behind `windows_by_handle`, which a pinned stable
toolchain cannot use.

---
### One temporary name for every save, written with a call that truncates

Every atomic write in `save.rs` staged to `out.with_extension("tpdf-partial")` and wrote it
with `std::fs::write`. One predictable name derived from the destination, and a call that
creates-or-truncates and follows symlinks. Three consequences, all of them writes outside
the file the reader named:

- saving `report.pdf` **destroyed** any existing `report.tpdf-partial` beside it;
- a **symlink** planted at that path sent the bytes wherever it pointed;
- two saves aimed at one destination **shared** a staging file, so the second truncated the
  first's bytes and either could rename or delete the other's work.

And the cleanup made it worse rather than better: on failure it removed the path whether or
not this call had created it.

`create_new(true)` is the whole fix, and it is worth naming what it buys rather than treating
it as a stylistic preference: it is `O_CREAT | O_EXCL`, so it **fails instead of truncating**
and it **refuses a symlink at the path** rather than resolving it. A name that is taken is
skipped and the next counter value tried, because a collision is not an error --- it means
another save got there first, which is what the counter exists for. The name appends rather
than replacing the extension (`report.pdf.tpdf-partial-<pid>-<n>`), so it cannot collide with
a document somebody happened to name `report.tpdf-partial`.

The general rule: **a temporary file is a file, and every rule about not writing over things
the user did not name applies to it.** "It is only a temporary" is the reasoning that puts a
truncating call on a predictable path in somebody else's directory.

The `sync_data` before the rename went in with it. Without it the atomicity claim is about
the directory entry only: a crash after the rename can leave the new name pointing at a file
of zeros, which is worse than either outcome the staging split exists to guarantee.

---
### Four assertions became unfalsifiable without being touched

Four tests asserted `!out.with_extension(PARTIAL).exists()` --- no staging file left beside
the destination. Every one of them was correct, load-bearing, and had been passing for weeks.
The moment the staging name gained a pid and a counter they became assertions about a path
**no code can produce**, so they are satisfied by a directory full of leftovers.

The usual version of this trap is an assertion written wrongly. This is the other direction:
the assertion never changed, the code moved out from under it, and nothing went red at the
moment it stopped testing anything --- because becoming impossible to fail is not a failure.

The tell was not in the test files at all. It was in the diff: **a constant that had been the
whole of a name became one part of it.** Any assertion built from the old shape is worth
re-reading whenever a name gains structure, and the repair is to derive the observable
instead of spelling it out --- `partials_beside(out)` lists what is actually there, whatever
the naming rule becomes next.

The same edit produced its mirror one file away, and it is worth noticing that both arrived
together: the mutation `save: write straight to the destination rather than renaming into it`
was aimed at `let partial = out.with_extension(PARTIAL);`, a line that no longer exists, so
the anchor gate caught it immediately. **The gate covers the mutation table and nothing
covers the assertions**, which is why one of the two was found in a second and the other by
reading.

---
### A parity check that compares steps is blind to the authority they run with

`check_workflow_parity.py` compares the two `gates` jobs step for step: every `uses:` with
its pinned SHA and every `run:` body, in order. It was written after a release workflow lost
a whole step in a copy, and it does that job.

It cannot see `permissions:`, and it cannot see a `with:` block. Its own docstring said so
and framed the difference as deliberate --- CI and release *should* differ on triggers and
authority. That framing is right about the workflow and wrong about the job, and an outside
review found what fell through it: `release.yml` declared `contents: write` at workflow
level, every job inherited it, and the `gates` job then checked out with the default
credential-persisting `actions/checkout` and ran `pip install pyhanko pyhanko-certvalidator`
--- unpinned, resolved from PyPI at the moment the job started --- **before** any gate ran.
So the newest release of a third-party package executed first, with a token that can write to
the repository.

Each ingredient looks reasonable alone. A release workflow needs write. A checkout persists
credentials by default. `pip install <name>` is what every README says. The composition is
the finding, and no check in the repository was looking at compositions.

Three properties close it, all now asserted by the same script: the gates job declares
`contents: read` of its own (or the workflow does), its checkout sets
`persist-credentials: false`, and its Python install names a committed requirements file
rather than package names. All four failure modes were proved by mutation before the script
was trusted, including the one that matters most --- **deleting the install step entirely**,
which without that check passes exactly like a clean run.

The generalisation: **when a check exists to stop two things drifting, ask what it compares
and write down what it therefore cannot see.** This script's docstring listed the exclusions
correctly and treated the list as a design note rather than as a gap register. A stated
exclusion is where the next defect lives.

---
### A test cannot see a change to a profile it does not run under

`docs/THREAT-MODEL.md` gained a disclosure that a parser panic on the save path reaches the
reader as a refusal rather than closing their document. That is true, and it is true only
while the crate unwinds: `spawn_blocking` hands a panic back as a `JoinError`, and
`panic = "abort"` would turn the same panic into process death taking the unsaved journal
with it.

The obvious way to pin it is a test that panics inside a blocking task and asserts the error
comes back. It passes. It would also pass with `panic = "abort"` in `[profile.release]`,
because **`cargo test` builds under the test profile, which does not inherit it.** The test
aimed at the exact property is structurally unable to see the exact change that removes it.

So the assertion with teeth is a source-level one --- read the crate manifest, refuse any
`panic` key --- and the runtime half stays as the control that the mechanism is what the
disclosure says it is. Proved by planting `[profile.release] panic = "abort"` and watching it
go red, which is the only thing that distinguishes this from the version that could not fail.

The general form is worth carrying past Rust: **a check runs in a configuration, and a claim
about a different configuration is not something it can make.** Debug versus release,
test-profile versus ship-profile, the dev server versus the bundle --- this repository has
now been caught by that four times, in four different vocabularies.

---
### The only document nobody re-reads is the one strangers read

`README.md` said editing had *just begun*, listed ink, shapes, text boxes and squiggly
under **Not built yet** while all four were registered commands with keyboard shortcuts,
and stated that *the open file is never modified in place* --- six weeks and one shipped
Save-in-place after that stopped being true. It also carried five exact counts (crates, npm
packages, PDFium libraries, cargo packages, traps) and every one of them had drifted.

Nothing in this repository was in a position to notice. Seventeen gates, four mutation
harnesses, a threat model re-read at every release, a trap index with a set-diff behind it
--- all of them aimed at the documents the people writing the code read. The README is the
one written *for* somebody else, which is exactly why it is never opened while working, and
its errors are the ones with a consequence outside the repository: a stranger deciding
whether to download this was being told it was materially less capable than the binary.

**Prose is not checkable, but one shape of claim is: an assertion of absence.** "This
feature exists and works as described" has no mechanical test. "This feature does not exist"
does, provided the claim names the thing whose existence can be looked up. So every bullet
under *Not built yet* now carries `<!-- not-built: edit.foo -->`, and `check_readme_claims.py`
refuses any of those that is registered. Claiming a feature is missing costs you naming its
absence in a form the registry can contradict.

Two things to copy rather than the specific mechanism.

**Check the half that can be checked and say out loud that the rest is not.** The status
paragraph --- the sentence that was most wrong --- has no gate and cannot have one, and the
tempting move is a keyword list approximating one. That would be a second inventory, drifting
the same way the first did. It went in `BUILD.md`'s release checklist instead, with the gate's
docstring naming the boundary. `docs/TRAPS.md` already records that a checklist is the weaker
instrument; naming which half is weak beats implying both are strong.

**Delete a count rather than correcting it, wherever something else already owns it.** Every
number removed from the README is in a generated file or one `grep -c` away. A count in prose
has no gate by construction, and this repository has now been caught by that three times in
three documents.

---
### A guard the type system already makes unexpressible has no mutation to write

`Plan::opened_as` is `#[serde(skip)]`, so a plan crossing into the worker cannot carry a
fingerprint of the reader's file. The obvious way to prove that guard is covered is to
delete the attribute and watch the test named for it go red. It does not go red. It does
not compile: `error[E0277]`, because `Fingerprint` implements neither `Serialize` nor
`Deserialize`, so the attribute is what makes `Plan` derivable at all.

**That failure is the good outcome and it reads exactly like the bad one.** The harness
reports `no summary line -- the run did not finish`, which is the same line it prints for a
mutation aimed at code that has drifted, for a killed harness's leftover edit, and for an
anchor that matched the wrong place. Read the compiler error before concluding anything:
here it says the property is *unexpressible*, which is stronger than *tested*.

Three things follow, and the second is the one that costs people time.

**Delete the mutation rather than weakening the code to accommodate it.** The tempting fix
is to derive serde on `Fingerprint` so the mutation compiles. That trades a compile-time
guarantee for a runtime test of the same property, which is the wrong direction, and it
would put a digest of the reader's file one careless field away from the wire.

**Keep the test anyway, and say in it what it is for.** It no longer proves the attribute is
present --- the compiler does that. What it catches is the change the compiler *would* wave
through: somebody adding serde to `Fingerprint` for an unrelated reason and dropping the skip
in the same edit. That is a real, reachable future commit, and nothing else would notice it.

**A test with no mutation is not automatically decoration**, which is the reflex this
repository has trained. Ask what would have to change for the assertion to fail, and whether
that change is reachable. If it is reachable but not expressible as one search-and-replace,
the honest record is a note in the test rather than a mutation that cannot land.

---
### Where the parse runs is not observable from a unit test

Moving a save's preparation into the worker changes nothing a unit test can see. The same
function produces the same bytes from the same input; `append_bytes` and `Request::Append`
agree by construction, because the second calls the first's builder. Every test stayed green
before and after, which is correct and says nothing at all about the thing that changed.

The claim is about *which process* did the parsing, and this repository already knows how to
evidence that: `backend-probe` reads the app process's own module table from outside it, and
`worker-probe` provokes the boundary rather than inferring it. So the append got three checks
in `worker-probe` --- an update section built by a real contained worker, appended to the
fixture and **re-parsed** as a document with the right page count, and its stated build length
compared with the file's own.

Two things that made those checks worth having rather than ceremonial.

**Assert on the bytes, not on `ok`.** A worker starved of something by its sandbox still
answers; a reply that went wrong still sets `ok`. What cannot be faked is an update section
that a parser accepts as a revision of that document. Same reasoning as the pixel comparison
next to it, which exists because a sandboxed PDFium once returned `ok` while drawing a
different typeface.

**The first run failed, and the refusal was the probe working.** The fixture plan's quad was
written with `top` above `bottom`, as a `/CropBox` has it. `Quad` is *display* space, y
increasing downwards, and the worker refused: *"a mark on page 1 covers no area in that page's
own space"*. Two rectangle conventions one flip apart, in a codebase that keeps them in
separate types precisely because of this --- and the type could not help, because both are
`Quad`, and only the values were wrong.

---
### The check that could not exist while one function did both halves

`save::append_bytes` read a file and built an update section from what it had read. The
update's byte offsets and `/Prev` are measured from the length of those bytes, and the caller
later appends after a file it measured itself. Two lengths --- and while one function did both,
they were one number under two names, so there was nothing to compare and no test could have
found a discrepancy.

Splitting the parse into the worker made them two numbers for real. A worker on a stale
mapping, or a file that changed between the caller's measurement and the worker's build,
produces an update whose cross-reference points into a document nobody has --- **in a file that
still opens**, which is the failure this whole module is organised around.

So `save::appended` exists to compare them, and it is the only thing that can: neither half
sees the other's number. It is a small function whose entire body is one comparison, and it
would look like ceremony to anyone reading the code without knowing that the property it
asserts used to be structural.

The generalisation, and it applies to any refactor that moves work across a boundary: **when
two things stop being the same object, list what used to be true by construction.** Those
facts do not announce themselves — they were never asserted, because nothing could have made
them false. The moment a call becomes a message, every one of them is a claim.

---
### A clamped delta turned "the baseline moved" into "this cost nothing"

A benchmark for how much memory a rewrite needs printed
`peak.saturating_sub(before)` at three points, and reported **+0.0 MB** for reading and
parsing a 337 MB file. Read literally that says a third of a gigabyte was free. What it
actually says is that the delta was *negative* and the clamp flattened it: `phys_footprint`
is what the process holds now, the allocator does not hand everything back at `drop`, and a
later iteration can begin below where an earlier one ended.

**The clamp chose the reassuring reading.** `saturating_sub` on unsigned integers is written
everywhere as ordinary defensive arithmetic, and it is --- until the quantity is a
*measurement*, where the case it silently absorbs is the one that says your instrument is
wrong. A negative memory delta is not noise to be squashed; it is the baseline telling you it
is not a baseline.

Two habits, and the second is the one that generalises past memory.

**Print absolutes at every point and let the reader see the shape.** `idle 772.3 -> parsed
772.4 -> rewritten 1109.2` is legible: the parse fit inside memory already held, the rewrite
did not. The delta form threw the first half of that away.

**A per-iteration baseline in a long-lived process is not a baseline.** Anything accumulated
by an earlier iteration is in it --- allocator arenas, caches, lazily-initialised statics. The
fix is a fresh process per measurement, or an instrument that does not need one: here the real
answer came from `worker-probe`, which reads the footprint of a *freshly spawned worker*
before and after one request.

---
### The edit that moved a copy and reported it as removing one

`save::append_update` took `&[u8]` and called `original.to_vec()` to satisfy
`IncrementalDocument::create_from`, which wants an owned `Vec<u8>`. On a 337 MB document that
is a 337 MB copy, and the sink beneath it discards every byte. Changing the signature to take
`Vec<u8>` so the caller could *move* a buffer in looked like removing it, and a comment went in
saying so, with the measured number attached.

The re-measurement came back **+667.0 MB before and +667.0 MB after**, identical to four
significant figures. The worker's document is a read-only mapping, so the caller's
`into_owned()` costs precisely what the callee's `to_vec()` cost. The copy moved one stack
frame and nothing else happened.

**An edit that changes nothing is indistinguishable from an edit that works, unless you
measure the same thing twice.** This repository already records that shape as a mutation that
ANDs with true; it arrives just as easily in a hand-written optimisation, where the reasoning
is sound at every step and the conclusion is still wrong because one term was never on the
critical path.

Two things worth taking from it.

**Identical to four significant figures is a result, not a coincidence.** A real change to a
memory measurement moves the low digits. When the digits do not move, suspect the edit before
suspecting the instrument --- and suspect a stale binary before either, which is why the
rebuild is part of the loop.

**Read the library rather than guessing what it needs the buffer for.** `save_internal` uses
the previous revision's bytes three times: it writes them to the target, adds their length to
`bytes_written`, and looks at the last one to decide whether to emit a newline. So the 337 MB
is carried to supply a number and a byte, and the copy is unavoidable only because
`create_from` takes `Vec<u8>` and offers no way to say less. That is a much better thing to
know than "the copy is necessary", and it is three minutes of reading.

### The delta was the wrong term, because the mapping was already absent from both numbers

The append that shipped on 2026-08-22 parses the document a second time, and on the 337 MB scan
`worker-probe` measured a macOS worker going from **362.7 MB** to **1029.8 MB** across the
request. A Windows worker is capped at 1 GiB of commit by its job object, which is closer to the
1029.8 than anyone wants, so the question was which of the two numbers the cap actually bounds.

The answer written down was the **delta, 667 MB**, and the argument for it was correct in its
one checkable step: `ProcessMemoryLimit` counts private commit, a read-only document mapping is
file-backed, so the mapping is not commit and the 362.7 MB baseline it was taken for does not
count against the cap. That leaves a 35% margin and the append was shipped on it.

**Measured on Windows, the worker peaks at 980.3 MiB --- 1027.9 MB, which is the macOS 1029.8 to
within 0.2%.** The margin is 4.3%, not 35%, and the largest fixture in the repository is 15 MB
short of not saving at all.

The error is one step earlier than where anyone was looking. `phys_footprint` excludes *clean*
file-backed pages too, so the mapping was never in the 1029.8 either --- and the 362.7 MB
baseline, taken for the mapping because it is close to the file's 337 MB, is PDFium's own
allocation for a 40-page scan. Both metrics were already reporting the same quantity. The
delta removed a term that was not in either number, and the coincidence of size is what made
that look right.

**When two platforms' metrics are said to differ, check the difference against a number they
should agree on before reasoning from it.** Here that check is free and decisive: the totals
agree to 0.2%, so there is nothing for the delta to correct for. It was available the whole
time and nobody could take it, because taking it needs the Windows machine, which is exactly
the condition under which reasoning gets written down as a substitute.

Two smaller things worth keeping.

**Working set minus commit is the mapping, and it is the observation that confirms the half of
the argument that was right.** Peak working set runs ~343 MB above peak commit on the 337 MB
scan. The mapping really is outside the cap; it just was not inside the number it was being
subtracted from.

**The cap and the reading were their own control.** At 362 MB and 404 MB the measured commit
stops at 1020.5--1020.7 MiB and the allocator then fails, so the quantity being read from
outside the process is demonstrably the quantity the kernel is enforcing. A memory figure that
never approaches a limit cannot tell you it is the figure the limit is about.

### An `[INFO]` line guarded on a macOS-only reading cannot print on Windows, and the instruction was to read it

`worker-probe` prints what the append costs the worker as an `[INFO]` line, and `BUILD.md` said,
for the three weeks the question was open: *"Run it against `incr-scan-40p.pdf` too, and read
the `[INFO]` line."* On Windows there is no such line and there never could have been.

The line is inside `if let (Some(was), Some(now)) = (before_append, after_append)`, those come
from `Worker::footprint`, and `phys_footprint` is `#[cfg(not(target_os = "macos"))] -> None`.
The probe is not hiding the number, it has no number; the neighbouring check says so out loud
with `[SKIP] the parent can read the worker's footprint --- not applicable`. So the instruction
asked for the one output the build cannot emit, on the one platform the question was about.

**A `[SKIP]` is a check declaring itself absent. A silently omitted `println!` is not**, and the
two lived four lines apart in the same function. The check was written to survive its platform
being wrong about it; the `[INFO]` was written as a convenience and inherited the same guard
without the same care.

What made it survive review is that the instruction is unfalsifiable from the platform that
wrote it: on macOS the line prints, so the sentence is true where it was tested and false where
it was aimed. The same shape as a harness that has never run on a platform --- it produces no
failures there, and neither does one that passes.

The fix is not to port `phys_footprint`. The quantity that matters on Windows is *commit*, which
is what the job object caps, and a parent cannot poll it usefully anyway --- the kernel refuses
the allocation rather than letting a poll observe the peak. It is read from outside the process
instead, through PSAPI's `PeakPagefileUsage` over the probe's children, which is what `BUILD.md`
now carries.

**Before telling someone to read a line, check that the build they will run can print it.** A
grep for the format string is enough, and it is cheaper than the round trip to the machine that
finds out.

### A `[SKIP]` whose stated reason is true can be the check you most need

`worker-probe`'s memory check ran on macOS and printed this on Windows, from 2026-07-29 to
2026-08-22:

```
[SKIP] the parent can read the worker's footprint   not applicable --- the job
       object caps memory in the kernel here
```

Every word of that reason is true. macOS refuses every relevant rlimit, so the footprint poll
there is a *substitute* for a bound it cannot have; Windows has the bound, so there is indeed
nothing for a poll to add. The skip was written deliberately, with its reason attached, by
someone who had just proved the cap works.

The inference is inverted, and it took three weeks and a shipped feature to see it. **A poll
stands in for a bound; a bound makes the reading matter more, not less** --- because the
question a reader needs answered is not "how much is it using" but "how close did it come to
being refused", and only the platform with a limit can answer that at all. So the one platform
where the number decides something was the one reporting no number.

What was missing was never a reason to look. It was a *way* to look, and it was four lines:
`GetProcessMemoryInfo` through the process handle the parent already holds. Nobody went looking
for it, because the skip read as settled.

**The reason had a shelf life, and nothing was watching it.** It was written when no worker
allocated anywhere near 1 GiB. Three weeks later an append landed that reaches **95.7%** of the
cap on the largest fixture in the repository, and the sentence saying the reading was not
applicable was not re-read --- there is no mechanism by which it would have been. The
justification for a skip is a claim about the surrounding system, and the surrounding system
moves.

Two things worth taking, and the first is *not* "distrust skips".

**The `[SKIP]` is what made this findable at all.** It stayed visible on every run, named its
reason, and printed under the same check name as the platform that ran it. A check quietly
omitted on one platform would have left nothing to re-read and no discrepancy to notice. This
repository's rule that a control which disappears cannot be told from one that ran is what
turned a three-week-old mistake into a five-minute one.

**Read a skip's reason as explaining why the *mechanism* is absent, then ask separately whether
the *question* is.** Here the mechanism really was inapplicable --- polling is the wrong
instrument next to a kernel limit --- and the question was more applicable than on the platform
that answered it. The two look like one thing in a single line of output, and they are not.

Sibling of *An `[INFO]` line guarded on a macOS-only reading cannot print on Windows*, which is
the same reading missing from the same run for a different reason: that one vanished silently
and this one announced itself, and the silent one was the cheaper mistake to make.

### A fixture no script writes gated ten guards, and the tests that skipped passed

Running the `append` mutation family on 2026-08-22 returned **six SURVIVED and one reddening
two tests other than the one it named**, out of nine. Every one of them was aimed at a guard on
the save path: refuse an update built against a different length, refuse a replacement file
that kept the length, cut the file back when the append cannot be read afterwards, refuse a
source that changed since it was opened. Freshly shipped code that writes to the reader's own
document, and deleting its guards reddened nothing.

The first job was to find out whether the change in the tree had caused it. Copy the three
edited files aside, `git checkout --` them, re-run, restore, compare digests: **the same seven
failed at `HEAD`**, with a byte-identical "red instead" list. Not the change.

The cause is one line in each of ten tests:

```rust
let Some((at, plan)) = appendable(&scratch, "text-heavy.pdf") else {
    println!("[SKIP] text-heavy.pdf: fixture not generated");
    return;
};
```

`text-heavy.pdf` is a **real document supplied by hand**. No script writes it,
`scripts/ci_fixtures.py` says so in its own docstring, and `BUILD.md` has recorded since
2026-07-30 that this machine "has never had it". So the ten tests returned before their first
assertion --- here, and on **both CI runners**, which cannot have it either. The guards were
correct all along; nothing anywhere could tell.

**The limitation was known, and it was filed under the wrong heading.** Every place it is
written down discusses corpora and benchmarks: a viewer sweep that cannot run all fourteen, a
`prespawn-bench` check that skips, a 109-name re-run done on six corpora instead of seven. All
true, all about *harnesses*. Nobody computed what the same absence does to `cargo test`, and
that is the transferable part: **a documented limitation gets filed under the category it was
discovered in, and its blast radius in every other category goes uncomputed.** The question to
ask of any "this machine cannot have X" note is not whether it is true but what else consumes
X.

**`cargo test` cannot report this and does not pretend to.** The run says `753 passed`, and a
test that returned at its first line is one of the 753. The skip prints to stdout, which libtest
captures and discards for a passing test, so the count and the transcript agree that everything
is fine. Only a mutation harness distinguishes them, and only because it asks a question the
test's own result cannot: *if I break the code, does this go red?*

**Read six SURVIVED naming six different tests as one cause, not six.** The instinct is to
strengthen the assertions one at a time. What actually discriminates is cheap and structural:
every one of the named tests contained the same early-return shape, on the same fixture name.
One `grep` for the fixture answered it, where six readings of six assertions would each have
concluded "this test looks fine" --- correctly, and uselessly.

**The fix is not to obtain the fixture.** Copying `text-heavy.pdf` from the machine that has it
would restore coverage on exactly one machine and leave CI where it was. The question is what
the tests actually require, and it is not a real document: they are about lengths, fingerprints
and rollback, so they need something *appendable* and *generated*. `comments.pdf` is both, is
built by a dependency-free script CI already runs, and carries `/Annots` of its own, so the
array-bearing branch is exercised too. After the swap all twelve `append` mutations are caught
by the test named for them, and all 62 in the `save` family.

**One of the ten needed the opposite fixture, and the failure said so immediately.**
`an_appended_mark_is_listed_by_the_page_it_was_made_on` asserts the mark's page lists it *and
the next page lists nothing* --- and a fixture shipping its own comments has no such page. The
weaker repair is worse than it looks: asking whether page 1 gained a `Highlight` does not
rescue it, because `comments.pdf`'s own marks include highlights, so the assertion could not
separate "the mark went to the wrong page" from "the fixture was already like that". It takes
`rotated.pdf`, which carries no annotations at all. **Whatever a fixture is meant to
discriminate, it has to be able to supply the absence as well as the presence.**

Sibling of *A test whose precondition is already satisfied never runs* and of *An empty
transcript is what a running viewer check looks like*: the same silence, reached by a third
route.

### A refusal a reader could answer, reported on a channel with no answer in it

`open_failure` has said *"This document needs a password, and tpdf cannot ask for one
yet"* since the day it was written, and every word of it was true. It was also the whole
implementation: a PDF behind a user password could be chosen from the file dialog, was
diagnosed exactly right, and could not be opened by any route in the application. The
sentence documented the dead end so precisely that it read as a decision.

**The shape to notice is that a correct diagnosis is what made it invisible.** A wrong
message gets reported --- somebody opens a file, is told it is damaged, and says so. This
one told the reader the truth, so nobody filed anything, and the gap survived every
review of the code around it because reading that line always confirmed it was right.
The same increment that had made the state *visible* --- the properties dialog, which
reports `locked` --- did not make it answerable, and the two are easy to conflate.

**The generalisation: a message naming a capability tpdf lacks is a to-do with no
ticket.** Grep for one. `progressive.rs` had "cannot ask for one yet"; `save.rs` still
has "tpdf will not write it" for the same documents, and that one is now the narrower
statement it looks like rather than the flat refusal it reads as.

### PDFium answers the same error for no password and for the wrong one

`FPDF_GetLastError` is `FPDF_ERR_PASSWORD` (4) in both cases --- measured across four
loads of one AES-256 fixture in one process, not inferred from the header. So nothing
downstream of the load can tell a reader who has not been asked yet from a reader who
just mistyped, and the two need different sentences: one says the document is locked, the
other says *that* password did not open it.

The consequence is where the wording has to live. It cannot be in `open_failure`, which
sees only the code; it has to be in the loop that knows it supplied a password, which is
`worker_child::unlock`. A design that put both sentences next to the code would have to
invent a distinction PDFium does not report, and the natural way to invent it --- assume
the first refusal is the un-asked one --- is wrong for the second worker of a document,
which is asked and refused before any reader sees it.

**The check that proves the retry happened is that the two sentences differ**, and it has
to assert both are present as well as unequal: `Option` inequality is also satisfied by
one of them being absent, which is a refusal that never arrived rather than one worded
differently.

**And a failed load poisons nothing**, which is the other half of the same measurement
and the one that decided the architecture. Loading the same bytes with no password, the
right password, a wrong one and the right one again opens on both correct attempts and
refuses on both others, in one process --- so a wrong password costs a reply rather than a
process, and the worker retries in place instead of being respawned.

### A password that unlocks the first worker unlocks nothing else

A pool exists, so "open the document" is not one event. The first worker is built in
`Workers::open`; every worker after it --- the one `checkout` grows under contention, and
every replacement for one that crashed --- is built by `spawn_into` from the *same
mapping*, which means it meets the same encryption and has to be told the same password.

The failure this produces is worth stating exactly, because it is not "the document does
not open". It is: **the document opens, the page the reader is looking at renders, and
the next one refuses.** Measured by mutation, with the password removed from `spawn_into`
alone: `8 served, then: This document is locked, and needs a password.` Everything else
in `password-probe` stayed green, including the check that a tile renders with ink in it.

Two things follow. The password has to live on the *document's* slot rather than in the
open that acquired it, which is what `Held::password` is for. And a probe for this has to
**force** the pool to grow rather than hope it does: a tiny fixture renders in
microseconds, so tiles issued one after another never overlap, `checkout` always finds an
idle worker, and the check passes having exercised exactly one process. `grow` issues
eight tiles per thread across `pool_size()` threads for that reason, and prints the count
so a run that did not grow reads as such rather than as a pass.

Sibling of *A check bound to one caller covers only that caller*, arriving through a
different door: here every caller goes through one function, and the defect is a caller
that was never given what it needed to pass on.

### Wrapping stdin in a `BufReader` eats the first request of the session

`worker_child::refuse` may wrap `std::io::stdin()` in a `BufReader`, and does. `unlock`
may not, and the difference is not style: `refuse` never hands the stream on to anybody,
and `unlock` returns an opened document into the ordinary serve loop, whose reader thread
reads through `std::io::stdin()` and its own shared buffer.

A private `BufReader` reads ahead. Whatever arrived promptly behind the password --- which
on the startup path is the `Open` request, sitting in the pipe microseconds later --- is
pulled into that buffer, and the buffer is dropped when `unlock` returns. The request is
simply gone, and the symptom is a worker that opened the document and then never answers,
which reads as a hang in the pool rather than as a read that consumed too much.

The rule was already written down, in `wait_for_document`'s own doc comment on the
Windows side, for exactly this reason. It transferred here only because that comment
existed to be read --- and the two functions are far enough apart in the file that nothing
would have connected them. **A buffering decision is part of a stream's contract with
whoever reads it next**, so it belongs beside the handover, not beside the read.

### A mock's default return value decides whether a mutation fails or hangs

`openWithPassword` loops until the document opens or the reader declines, and three of its
tests assert that the ask function is **never called**. Written the obvious way, that mock
is `vi.fn()` --- which resolves `undefined`.

`undefined` is neither a password nor a decline. So the moment a mutation made the loop
reach that mock, it asked, got `undefined`, retried with `undefined`, and did that until
the vitest worker died: `Worker exited unexpectedly`, `Tests (7)` with no counts, 23
seconds. The mutation *was* detected, in the sense that the run was not green --- and it
was detected as a broken runner, which is the diagnosis that sends you to look at vitest.

**The fix is one word in the test and it is not a workaround.** `vi.fn().mockResolvedValue(null)`
makes the mock able to *end* the loop even in the test that asserts it is never entered, so
the mutation now fails `expect(ask).not.toHaveBeenCalled()` in 2 ms. The general form:
**a double standing in for something that terminates a loop has to be able to terminate
it**, including in the tests that expect it never to run --- because those are exactly the
tests a mutation reaches first.

The reason this is worth its own entry beside *a test whose failure is a hang reports a
pass and a timeout in one breath* is where the defect lives. There the code under test
hangs; here the code is fine and the **test double** supplies the non-terminating value.
Nothing about the assertion, the loop or the mutation looks wrong, and reading any of them
would not find it --- running the mutation is what found it, which is the whole argument
for running them rather than reasoning about them.

### The guard that could not fire, because the library removes the evidence first

`save.rs` refused to write an encrypted document. The refusal was correct, the reason was
right, it had a test, and the test passed --- and for four weeks it did not fire for the
commonest encrypted PDF there is.

The guard was `doc.trailer.has(b"Encrypt")`. `lopdf` **removes** that entry, and the object
it names, the moment it authenticates the document; and it tries the empty user password by
itself, unprompted, before it looks at anything the caller supplied. So a permission-restricted
AES-256 file --- the kind that opens with no prompt in every reader, and the kind a reader is
therefore most likely to try to annotate --- arrived at the guard with nothing left to see,
sailed past it, and was reserialised in the clear.

Measured rather than argued, in one command: `qpdf --is-encrypted` exits 0 for
`incr-encrypted-open.pdf` and 2 for what `write_copy` wrote from it.

**The fixture's own doc comment predicted this and got it exactly backwards.** It read: *"The
encryption is not real --- nothing here encrypts any stream --- and it does not need to be: the
guard is about the presence of the dictionary, which is what `lopdf` drops. A genuinely
encrypted fixture would test the same branch and would additionally not load."* Every clause
is wrong in the same direction. A genuine fixture takes a **different** branch; the synthetic
one keeps `/Encrypt` precisely *because* its encryption is fake, so authentication fails and
`lopdf` leaves the trailer alone; and it loads perfectly well. It is the family entry *a
fixture where the right rule and the wrong rule agree cannot tell them apart*, with the
argument for the agreeing fixture written down as a justification for not building the other
one.

The predicate that works is `was_encrypted()`, which reads `encryption_state` and survives the
load, with `is_encrypted()` beside it for the document nothing could unlock. Two questions
because there are two states and they want different answers: the first is appendable and the
second is not.

**The general shape: when a guard asks a library about the input, ask what the library did to
the input on the way in.** A parse is not a passthrough. `lopdf` also drops `/Encrypt` for a
document you *did* supply a password for, which is the same trap one step further along --- and
it is why `docinfo.rs` could report an AES-256 document as carrying no encryption at all while
its own comment warned about the ordering of the `decrypt` call two lines below.

### A field with no reachable `true`, guarded by a comment about the wrong call

`docinfo::Encryption::opened_without_password` is documented as *"whether an empty user
password opened it"*. It was never `true`. Not once, for any document.

The only route to an `Encryption` value at all was `read_encryption`, which reads the
trailer's `/Encrypt` --- and reaching that route required `lopdf` to have **failed** to
authenticate, since a successful load strips the entry. A document that failed to
authenticate is by definition one an empty password did not open. So the field was `false`
by construction on every path, while `read_encryption`'s doc comment and a registered
mutation both stood guard over a *different* call, `Document::decrypt`, whose ordering was
genuinely correct and had stopped mattering.

Two things hid it. The trailer route is the obvious one and reads correctly in isolation ---
nothing about it says "this only runs for locked documents". And **nothing renders the
field**: `properties.ts` declares it, the panel shows the method and the permissions, so
there was no screen on which the wrong value could be seen. A dead field cannot be seen to
be wrong, which is the argument for deleting one rather than for leaving it.

The fix is a second route, `encryption_from_state`, reading the version, revision, key
length, permission bits and crypt filters out of `Document::encryption_state`, which
survives the load. It fixes the unreachable value and a user-visible defect at the same
time: before it, every permission-restricted document reported **no encryption at all**, so
the properties panel told a reader an AES-256 file was unprotected.

The `decrypt("")` call underneath went with it, and that half is worth stating separately.
`lopdf` authenticates during the load; a document still reporting `is_encrypted` afterwards
is one no password opened, so `decrypt` on it can only fail, and on the other branch it
returns `NotEncrypted` --- which the code read as "locked". **A call that can only return
one of two errors is not a decision.**

### A test helper that reads through a parser that could not read

Writing the encrypted-append test, two helpers failed before the code under test did, and
both failed the same way: `page_count` and `listed_on_page` load with `Document::load(path)`
and no password.

`lopdf` parses **no objects** for a document it cannot authenticate, so the first reported
0 pages --- and the plan built from it was then refused by the real code with *"the document
on disk has 2 page(s) and the edits were made against 0"*, which is a correct refusal naming
entirely the wrong thing. The second panicked with `index out of bounds: the len is 0 but
the index is 0`, which reads as a save that lost every page.

Neither is a defect in the helper as it was written; each is a helper whose reader is
narrower than the fixtures it is now pointed at. What makes it worth an entry is that this
is **the same defect the increment was fixing**, arriving in the test harness first and
wearing a different symptom each time. A count taken through a reader that could not read is
not a small count, it is not a count at all --- and every downstream message describes the
number rather than the blindness.

### A capability nobody could use is invisible to every check, including the mutation harness

Adding a password to four `lopdf` readers was one line each. Proving it did something
needed six mutations through `password-probe`, and five of them reddened exactly the check
named for them. The sixth --- taking the password away from `annots::scan` --- reddened
**nothing**, because the probe had no comments check.

The reason it had none is the reason the check is hard: the fixture carries no comments, so
a count of them cannot tell *none* from *could not look*, which is the trap this repository
already has under *an empty answer from a whole-document scan cannot say whether it looked*.
The observable that works is the module's own `Limits::pages_missed` --- pages PDFium
paginated and `lopdf` could not account for --- which goes from 0 to 2 the moment the key is
withheld.

**The finding is the shape, not the missing check.** Four sibling readers got the identical
one-line change; three had an observable and one did not, and nothing in the type system,
the test suite or the gate list distinguishes them. Running a mutation per call site is what
separated them, and a mutation that reddens nothing is a statement about the *harness*
rather than a survivor to argue with.

### The same silent decryption, on the path whose output a reader keeps

The save path's encryption guard was found wrong and fixed. Sweeping for the same predicate
found `print::build` with **no guard at all** on the branch that reserialises, and the
comment on `Job::is_passthrough` explains why it looked covered: *"a rewrite that changes
nothing is the risk `lopdf` dropping encryption is about, so the caller says which it means"*.
That reasoning is right about the whole-document case, which is handed over byte for byte,
and silent about every other case. **The risk is also a rewrite that changes something.**

Measured before the fix: a one-page selection of `incr-encrypted-open.pdf` built **1,278
bytes** with the encryption gone, no message. A locked document refused with *"page 1 is not
in this document, which has 0"* --- correct arithmetic on a document nothing could read.

The reason this is worse than it sounds is where the bytes go. A print job is not only sent
to a printer: it is handed to the platform's own PDF reader for the panel, and **Print to PDF
is how most people make a copy of a document**. So the output is a file the reader keeps, and
it is decrypted, and nothing said so.

The fix refuses, rather than threading the password, and that is not laziness: even *with*
the key, `lopdf`'s full serialiser cannot put the encryption back. An append could, and a
page selection is not appendable. So the honest answer is that an encrypted document prints
whole or not at all, and the message says which.

**The general lesson is about sweeping.** One wrong predicate was found by building a test;
the second was found by grepping for what the first one should have said. A defect class with
one instance almost never has one instance, and the cheapest moment to look for the rest is
while the right predicate is still in your head.

### A capability absent through a struct default has no defect to find

Windows readers could not print a page range. Not because the range was ignored --- because the
Pages radio button and its two edit controls were **greyed out**, and had been since printing
landed.

The whole of the cause:

```rust
let mut dialog = PRINTDLGW {
    Flags: PD_RETURNDC | PD_ALLPAGES | PD_NOSELECTION,
    nCopies: 1,
    ..Default::default()      // nMinPage: 0, nMaxPage: 0
};
```

Win32 disables the Pages controls whenever `nMinPage == nMaxPage`. Both were zero, so the
condition held, so the field was dead. Nothing was mis-set: the two fields were never mentioned,
and a field you did not write is a field no reviewer reads.

**The failure has no observable of any kind.** A wrong value produces wrong output; an ignored
value produces a surprise. An absent capability produces *nothing* --- no error, no log line, no
wrong page, no failing check. `print_probe.rs` drives the entire Windows print path to a real
spooler and back through the OS parser, and it is structurally blind here, because a modal
dialog needs a person and every check it makes is about the job rather than the panel. The only
instrument that reports a missing capability is somebody trying to use it.

**The neighbouring flag makes it sharper.** `PD_NOSELECTION` is set two lines above, with a
comment giving the rule: *"offering the radio button and then ignoring it would be worse than
not offering it."* That reasoning was applied deliberately to the Selection control and never
reached the Pages control, which was in the same struct, governed by the same dialog, and
disabled by an omission rather than by the argument.

The general form, and it is not about Win32. **Where a platform API takes a struct you fill in
partially, the fields you leave out are decisions you did not know you made.** They are invisible
in a diff, invisible in a review, and invisible to any test that does not drive the UI those
fields configure. The cheap habit: when initialising an OS struct with `..Default::default()`,
read the documentation for the fields you are *not* setting, specifically for sentences of the
form "if X equals Y, the control is disabled".

**And the fix has to close the rule it just exposed.** Enabling the field without reading
`PD_PAGENUMS` back would have produced the exact defect `PD_NOSELECTION`'s comment forbids ---
a control offered and ignored --- on the noisier of the two controls. Offering and honouring
are one change, not two.

### A *Not done* note can describe a route with no reader in it

`docs/PLAN.md` carried this for a week, in the ranked list, as the print subsystem's open gap:

> **Not done:** an explicit page range still carries no marks and no crops. A reader who types
> "2-4" into the print panel of a marked-up document gets those pages without their marks.

The first sentence is true. The second cannot happen, and never could.

`print_document` takes a `pages` argument, and `grep -rn "print_document" src/` finds two
callers: `App.svelte`, which passes `pages: null` on every print, and `viewercheck.ts`, which is
the check harness. tpdf has **no page-range field of its own** --- `appcommands.ts` says so at
the command, as a decision --- so what a reader types goes into the *system* panel, which
filters the job we already handed over. That job is `save::print_bytes`'s output. It has the
marks on it.

So the note described the parameter accurately and the product not at all, and the two were
easy to conflate because the parameter is named after the thing the reader does.

**What it cost is the ranking.** For a week the print subsystem's gap was a route no reader
reaches, while the real gap sat one platform over --- the Windows panel's Pages field, disabled
by a struct default, which nothing in the document mentioned. A wrong entry in a ranked list is
worse than a missing one: it is read as coverage, and it aims the next session at the wrong
place. This one aimed two.

**A *Not done* is a claim about the product, and it deserves the check any other claim gets.**
Not "is this code still unwritten" --- that is easy and it is the wrong question --- but *can a
reader get here at all*. For a Tauri command, that is one grep over the callers. The trap
already in this file about a note outliving the work that closes it is the same family and the
milder case: that one was true once. This one was written about a route that has had no reader
in it since the day the command was added.

### PDFium draws a comment's icon in its own colour, and the file is not wrong

A reader picks blue for a comment. The overlay draws a blue icon. They save, reopen the file
in tpdf, and the icon is **yellow**.

Nothing in the save path is at fault, which is what makes this hard to find by reading.
`save.rs` writes `/C` with the colour the reader chose, and writes **no appearance stream**,
deliberately: the specification describes `/Name` as choosing an icon and expects readers to
draw it, so a hand-drawn speech bubble of ours would look foreign in Acrobat and in Preview.
The file says blue. PDFium's synthesised `/Text` appearance ignores `/C` and paints its own
house style.

**Measured, not inferred, and the control is the whole of it.** Sending blue read 224 degrees
of hue on screen and **60** in the file; sending red read 0 on screen and **60** again. A
reading that does not move when the input does is the file being ignored, and one run with one
colour could not have told that from a mistake in what we wrote.

It is the *"the mark changed under the reader"* failure the overlay phase was written for,
arriving in the one kind that phase structurally cannot see --- it reads the overlay's alpha,
not its colour, and the file's renderer is in another process.

**The choice it leaves is not between right and wrong.** Writing an appearance stream makes
tpdf agree with itself and disagree with every other reader's icon; leaving it makes tpdf
disagree with itself and agree with readers that honour `/C`. Recorded in `docs/PLAN.md` §10
question 8 as a decision rather than fixed as a defect.

The general form is worth carrying past this kind: **an annotation with no appearance stream
is a request, not a picture**, and what any given reader draws from it is that reader's
business. A check comparing two renderers has to know which properties the file actually
determines, and for a `/Text` annotation the colour is not one of them in PDFium.

### A check reported `[OK]` with the reason it should have failed printed beside it

The first run of a new phase printed this:

```
[OK]   the saved copy renders, so there are two pictures to compare invalid args `mark` for command `annot_m...
```

The verdict tested `after !== null` --- the copy did render. The detail line was built from a
different variable, the error from the marks that had been refused a moment earlier, and it was
true: the mark payload was `MarkView`'s shape rather than the command's, so every one of the
nine was rejected and the "copy" was a copy of an unmarked document.

**The name is what gives it away.** *"there are two pictures to compare"* was false --- the two
pictures were identical --- and the condition tested something narrower than the name claimed.
A detail line assembled from more state than the verdict reads is a check that can print its
own refutation and still pass.

Two habits, and the second is the one that costs nothing. **Put every variable the detail line
mentions into the condition, or stop mentioning it.** And when writing the verdict, read the
check's own name back as a sentence and ask what would have to be true for it: here, two
pictures that differ.

**What saved the run was a control, not the check.** `control: saving the marks changed what
the file renders` went red on the same run, correctly, because nothing had been saved. The
phase reported a green check and a red control about one event, which is the shape that says
the green one is the one to distrust.

### A check read the palette's rendered rows, which are capped at 64

*"With no document only the commands needing none are offered"* went red on a change that
added four commands, and named three that had nothing to do with them:
`view.showThumbnails`, `view.showMarks`, `view.invertPages` reported as **withheld from the
reader**.

They were not withheld. `palette.ts` renders `registry.search(query).slice(0, 64)`, and the
check took its answer from `palette.visible` --- the titles of the rows that were drawn. Before
the change 63 commands were enabled with a document open; after it, 67. Three fell off the
bottom of a list, and a list truncated for display cannot answer the question *"is this command
offered"*.

**The check had been one command away from saying so for some time**, and nothing could have
told anybody: it passed at 63 exactly as it passes at 5. There is no reading that distinguishes
"just under a cap" from "nowhere near one", which is why a bound that a growing population
approaches is a defect on a timer rather than a risk to weigh.

The fix is one line and it is not raising the cap: ask `registry.search("")` rather than the
palette. The cap is a rendering decision that belongs to the palette; the question belongs to
the registry. The palette is still opened and closed, because the phase after this one asserts
the viewer was left as it was found.

**The general form: a check that reads a UI's *rendered* state is asking a different question
from the one it is named for.** Rendered state is truncated, virtualised, scrolled, collapsed
and animated, and every one of those is a way for a correct system to look wrong. Read the
model the UI renders from, and let a separate check --- with its own name --- say the rendering
matches it.

### PDFium synthesises an appearance for `/Text` and not for `/Stamp`

Both are annotations a reader *places* rather than draws, both are positioned by `/Rect` alone,
and both have a `/Name` naming one of a standard list. The obvious inference is that they are
the same case. They are not, and getting it wrong in either direction ships a mark nobody can
see or a mark drawn twice.

Measured before a line of the stamp was written, on one page through one code path:

| annotation, no `/AP` | non-white pixels PDFium drew |
|---|---|
| none (the bare page) | 0 |
| `/Stamp` with `/Name /Approved` | **0** |
| `/Text` with `/Name /Comment` | **336** |

So a stamp is on `/Square`'s side of the line --- we write the appearance or nothing appears ---
and a comment is not, which is why `save.rs` deliberately writes none for it.

**The two zeroes are why the third row exists.** A blank page reading 0 and a stamp reading 0
are the same number, and a probe that rendered nothing at all produces both. The `/Text` row is
the positive control, and without it the measurement establishes nothing whatever. It cost
three lines.

The generalisation worth carrying: **which annotations a renderer synthesises for is a list, not
a rule**, and it differs per renderer. Do not infer it from the specification, from the
annotation being "the kind a reader places", or from what a neighbouring subtype does. Render
one and count the pixels.

### A round trip is a composition, so it is blind to a symmetric error

`place_crop` maps a crop box onto the screen and `crop_from_display` maps a dragged rectangle
back to a crop box. They carry separate rotation tables --- `text::to_device` and
`text::from_device` --- which is exactly why a round trip through both is worth writing: this
repository already records two such tables disagreeing at every turn but zero, and a round trip
catches that.

What it cannot catch is the class of error the two directions make **symmetrically**. Measured,
because the plausible reading is the wrong one:

| mutation | round trip | the corner test |
|---|---|---|
| drop the file-box offset in `crop_from_display` only | **red** | red |
| drop it in `place_crop` only (composes) | red | red |
| drop it in **both** | **green** | **red** |
| the wrong quarter turn | **red** | red |
| half the clamp | green | green (the clamp test) |

So the honest statement is narrow: a round trip pins that the two directions **agree**, and
nothing about whether they agree on the right thing. The first draft of the comment beside
these tests claimed the opposite --- that a one-sided deletion would leave the round trip green,
because "the round trip only sees the composition". That reasoning is right about a *symmetric*
edit and wrong about a one-sided one, and the two are easy to conflate when writing the comment
rather than running the mutation.

Two tests are what close it, and neither is a stronger round trip: one asserting a **known
absolute** (dragging the whole visible page must name the file's own box back, on a `/CropBox`
that deliberately does not start at the origin), and one asserting a **bound** the composition
cannot reach, since a round trip also agrees with itself about a rectangle that never left the
page.

Generalises to every A/B pair in this repository: an encoder and its decoder, a writer and its
reader, `to_device` and `from_device`, `intoCrop` and `outOfCrop`. Ask what the pair would still
agree about if both halves were wrong the same way, and pin that separately.

### A fixture whose origin is zero makes an offset term unfalsifiable

The same increment, one line up. `place_crop` subtracts the page's own `/CropBox` corner before
turning, and `crop_from_display` adds it back, because a crop box is in absolute page
coordinates while the display space starts at the file box's corner.

On a page whose `/CropBox` is `[0, 0, 595, 842]` --- which is most pages, and the first constant
anyone reaches for --- that subtraction is a no-op. Every test written against such a fixture
passes with both terms deleted, and the deletion is invisible in a diff that only touches
arithmetic.

The fixture here is `[12.0, 20.0, 607.0, 862.0]`: A4 offset by twelve and twenty, chosen for no
reason except that it is not the origin. It costs nothing and it is the difference between three
tests that pin an offset and three that are decoration.

The general shape is *"a property that holds by construction cannot test the thing it
resembles"*, arriving in the least conspicuous place there is: a constant at the top of a test
module. When a function has an additive term, look at whether the fixture makes it zero --- and
when it has a multiplicative one, whether the fixture makes it one.

### Adding a third drag made five existing mutations aim at nothing, or at two things

`viewer.ts` had two `PointerDrag`s and gained a crop's. Nothing about the existing two changed;
one press handler grew an `||`, `armErase` grew a line, and the new drag's `move` and `end` are
written the way the box's are, because they do the same thing.

That was enough to break **five** mutation anchors in `scripts/mutate_frontend.py`, in both
directions --- four of them before a line of the status was touched, and the fifth when it was:

| anchor | after | why |
|---|---|---|
| `const id = quad ? this.pages.idOf(live.slot) : undefined;` | **2x** | the crop's `end` needs the identical line |
| `const { x, y } = this.pageAndPoint(at);\nlive.to = { x, y };` | **2x** | the crop's `move` needs the identical pair |
| `if (this.drawDrag.start(event)) {` | **0x** | the press now offers the crop drag first |
| `this.drawKind = null;\nthis.inking = null;\nthis.erasing = true;` | **0x** | `armErase` now puts the crop away too |
| `armed: this.drawnStrokes === null ? this.drawKind : null,` | **0x** | the crop reports through the same field |

None of the five is a drifted intention: every mutation still describes a real defect and every
test named is still the right test. What moved is only whether the harness can *find* the line,
and the two failure modes are not the same --- a `0x` anchor is refused loudly twenty minutes
into a run, and a `2x` anchor would be applied to both occurrences and could then be killed by
the wrong test, or by none.

`scripts/check_mutation_anchors.py` caught all five in 0.1 s. That gate exists because a killed
harness leaves its edit in the tree, and this is its **other** value, which was not the reason it
was written: a feature that duplicates a shape is the ordinary way an anchor becomes ambiguous,
and nothing about the feature looks like it touched the mutations at all.

The fix for a `2x` is a **wider** anchor rather than a different line: the box's `const id` is
preceded by its own `boxQuad(...)` call, the crop's is not, so including the line above
disambiguates without moving what the mutation does. Re-run each re-aimed mutation afterwards ---
widening an anchor changes what gets replaced, and an anchor that compiles is not an anchor that
still kills.

### A gate over claimed absences only catches the name the claim guessed

`scripts/gates.py`'s `readme` gate exists because tpdf's public *Not built yet* list named four
features that had shipped. Each bullet carries an HTML comment naming the command that would
exist if the feature did, and none of those may be registered. Four failure modes were proved
by mutation before it was trusted, and it has been green ever since.

It was green while the list said **"Stamps, the one annotation kind with no way to make it
here"**, one commit after stamps shipped. The bullet named `edit.addStamp`. What shipped is
`edit.stamp.approved`, `edit.stamp.confidential`, `edit.stamp.draft` and `edit.stamp.final` ---
four commands, none of them the guessed name, so there was nothing for the gate to contradict.
The same bullet-and-implementation mismatch was sitting in the crop line at the same moment:
`edit.cropToRectangle` claimed absent, `edit.cropToDrag` built.

The mechanism is the trap, not the oversight. **A bullet naming a command id is a claim about a
string somebody has not chosen yet**, written at the moment the feature is deliberately *not*
built --- which is the moment least likely to predict the name it eventually gets, and a feature
that ships as a *family* of commands escapes a bullet naming one of anything.

Two things follow. The gate is worth keeping: it is the only mechanical contradiction available
for prose, and the four features it was written for would still be caught. And the claim it
supports is narrower than its output reads --- `[OK] every claimed-absent command is absent` is
a statement about a name, never about a capability.

The generalisation is the one this repository keeps meeting from new directions: a check whose
subject is a **string chosen later** cannot be a check on the thing the string was going to
name. The stronger invariant, and the one to build if this happens a third time, runs the other
way --- every *registered* command must appear in the README's built prose or in an allowlist
with a reason, which is the shape `viewer_sweep.py` and `viewercheck`'s command classification
already use, and which a new command cannot escape by being named something unexpected.

### Two constants in different units, and the comment comparing them was false at every zoom a reader uses

`ERASER_RADIUS` in `markband.ts` carried an argument for its own value: the nib
is *"deliberately smaller than the ring a press uses to find a mark, because
taking the wrong stroke is a loss and opening the wrong note is not"*. A clear
reason, naming the other constant, and wrong.

The two are not in the same space. `HIT_SLACK_PT` is **3 points** --- its own
comment says points on purpose, so the slack is the same physical size at every
zoom. `ERASER_RADIUS` is **6 view pixels**, and the sweep converts it with
`ERASER_RADIUS / this.zoom` before comparing. So the relationship the sentence
asserts is not a relationship between two numbers at all; it is a function of
the zoom, and it changes sign inside the range a reader uses:

| zoom | nib, in page points | press ring | which is wider |
|------|--------------------|------------|----------------|
| 50%  | 12.00 | 3 | nib, by 4x |
| 100% | 6.00  | 3 | nib, by 2x |
| 150% | 4.00  | 3 | nib |
| 200% | 3.00  | 3 | equal |
| 400% | 1.50  | 3 | press ring |

Measured by printing both at five zooms rather than by arithmetic on paper,
which is the only reason the 200% crossover is a number here instead of "about
double".

**The general shape: a comment comparing two constants is a claim, and it is
checkable exactly when they share a unit.** These did not, and nothing in the
tree could have gone red --- there is no assertion relating them, and there
cannot usefully be one, because whichever zoom you pinned it at would be an
arbitrary choice dressed as an invariant. The tell is available by reading
alone: **two constants whose names end in different units** (`_PT` against a
value the caller divides by a scale) being compared in prose.

It had been false since the eraser was written and cost nothing while a sweep
took one stroke of a drawing. It was found while extending the same nib to take
**whole marks**, which is when the argument the comment makes became worth
checking --- so the second lesson is that a stale justification surfaces when the
stakes it reasons about change, and that is the moment to re-read it rather than
inherit it.

The comment is corrected and the constant is not: the sentence was wrong, and
what the nib *should* be is a question about how the tool feels. See
`docs/PLAN.md`.

**And the same file was wrong about the units twice more, which is why the false
comparison was easy to write.** `strokeTouches` and `strokeSwept` both said
*"the viewer hands both in view pixels"*. It does not: `viewRectOn` applies the
crop and the two turns and no zoom at all, so every point handed to those
helpers is in the slot's **laid-out points**, and the only thing converted is
the radius. So the constant is view pixels, the comparison is points, and three
comments in one file said otherwise --- which is exactly the state in which
somebody writes a sentence comparing 6 with 3 and it reads as obviously true.
Found by re-reading them while writing `quadSwept`'s own units line, not by
anything going red, because a wrong unit in prose has nothing to go red.

### A comment defending a name can become an argument for the opposite name, with no word of it changing

`appcommands.ts` explained why the eraser was called *Erase drawing...*: a bare
*"Erase" beside "Remove mark" would read as a second, blunter way to delete
anything*. Every clause of that stayed true. The command became exactly the
thing the sentence warned the name would falsely suggest --- the nib now takes
any mark it crosses --- so the observation survived intact and the conclusion it
supported inverted.

That is why it is worth an entry rather than filing under "update the comments".
A justification that has gone stale by being *contradicted* is easy to spot: the
code says one thing and the comment says another, and a reader stops at the
disagreement. This one had no disagreement to find. The comment described the
new design accurately, in a sentence arguing against it, and the only way to
notice is to ask what the argument concludes rather than whether its statements
hold.

**The habit: when a change makes a comment's premise true, re-read its
conclusion.** A premise becoming true is not the reassuring direction --- for a
comment shaped *"X would be misleading, so we do not call it X"*, X becoming the
truth is precisely the event that flips it.

The old argument is kept in the file beside the new name rather than deleted, so
that a reader meeting *Erase marks...* can find out what the previous name was
for. A justification with a date on it is worth more than one with only a
verdict.

### An insertion between a doc comment and its declaration orphans it, and TypeScript says nothing

`armErase`'s doc comment ran to twelve lines and documented nothing. The crop
tool had been added between it and the method, so the file held two `/** */`
blocks in a row --- the eraser's, then the crop's --- and only the second one
binds. TypeScript accepts that silently: two doc comments before one declaration
is legal, tooling takes the last, and the first becomes prose sitting in the
middle of a class.

It is invisible in three ways at once. The **diff** that caused it shows an
insertion of a whole coherent block, which is exactly what an intentional
insertion looks like. The **file** reads correctly top to bottom, because a
detached comment and a section header are the same characters. And nothing
**mechanical** can see it: there is no lint for it, no type error, and no test
can assert on a comment.

What made it expensive here is what the orphan said. It read *"Only drawings are
erasable ... making the eraser remove whole marks of any kind would be a second,
much more destructive command wearing the same cursor"* --- a live design
argument against the feature being built, attached to nothing, in the file where
somebody would go looking for exactly that reasoning.

**A scan finds them, and the tree had 31.** The first version of it found 26 and
missed five, because it looked for a line that is exactly `*/` and a *single-line*
`/** ... */` does not produce one --- which is worth more than the count: a scan
for a defect can have the defect's own shape, and the fix was to track block
starts rather than block ends.

```
*/ followed by /**   ->   the first block documents nothing
```

The objection that looks fatal is the **group header**: a block introducing
several constants is exactly this shape and is right. It is answered by a
*spelling* rather than by an allowlist --- **a group header is a plain `/* */`,
not a doc comment** --- so the rule needs no exceptions and no list to rot. There
was one in the tree, over `commands.ts`'s scoring weights, and it now says so in
its own text. The module header at line 1 is the single structural exception,
recognised by position, and removing it is one of the four controls that prove
the gate fires: it then reports all 22 of them.

All 31 were repaired and `scripts/check_doc_comments.py` is a gate as of
2026-08-23. The repair was made provable rather than eyeballed: a mover that
takes a block by its first prose line and an anchor, and asserts that **the file
with every doc block stripped is byte-identical** before and after. A mass
comment move has no compiler and no test behind it, so without that assertion a
mistake is silent --- which is this entry's own subject arriving in its fix.

Two of the moves were wrong on the first attempt and the byte check did not
catch them, because both landed a block on a declaration that already had one:
the stripped text was identical and a *new* orphan appeared. The re-scan found
them. So the invariant proves no code moved; only re-running the scan proves the
comment landed on the right thing.

The habit is cheaper than the scan: **when inserting a declaration above an
existing one, look at what is directly above the insertion point.** If it is a
`*/`, the comment belongs to the thing you are pushing down.

### A mitigation that moved half a path reads exactly like one that moved the path

`docs/THREAT-MODEL.md`'s residual risk 17 said the coordinator parses attacker
bytes on three writers, then recorded a narrowing: *"a save that only adds marks
is prepared in the worker now (`Request::Append`), so what is left is the
rewriting save and the two copy paths"*. True, precise, dated, and it left the
append parsing in the coordinator.

The **preparation** moved. The **verification** did not. `append_in_place`
re-reads the whole file it has just written and parses it with `lopdf` to check
the cross-reference chained and the page count survived --- and the previous
revision of that file is the document the reader opened, so it is a
coordinator-side parse of untrusted input on the commonest save there is. The
entry's own sentence names the mechanism that moved, and a reader takes it as
naming the *path* that moved, because that is what a risk register is about.

Worse in the detail than in the summary: the three writers the entry does list
run under `spawn_blocking`, which the entry says. This one does not --- the
`match` that calls it is on the async runtime --- so the one parse the entry had
stopped covering is also the one with the weaker containment.

**Why nothing could catch it.** A `Request::Append` variant exists and works; a
grep for it finds the worker doing the preparation and confirms the sentence. The
tests over the save path all assert on *outcomes* --- the bytes written, the roll
back, the page count --- and none can see which process did the parsing. There is
no assertion anywhere that could go red, which is what makes the release
checklist's *re-read the threat model against the code* a step and not a
courtesy: it is the only instrument aimed at this.

**The question to ask of any narrowing.** Not "did the thing named move", which
is what a grep answers, but **"what else on this path does the same thing, and
did that move too?"** A path that parses usually parses more than once ---
prepare and verify, write and read back, request and reply --- and a change lands
on one of them. Enumerate the calls, not the feature.

Found in step 6 of the checklist while cutting `26.8.8`, three days after the
narrowing was written, by the session that had just written a different half of
the same document.

### An option whose value is optional swallows the next argument, and `vitest list --json` overwrote a test file

Building the gate that reads `mutate_frontend.py`'s `TEST_FILES`, the obvious
first step is to ask vitest which tests exist in those files:

```bash
npx vitest list --json src/lib/text.test.ts
```

That does not filter by file. **`--json` takes an optional path**, so the
argument after it is a destination: vitest wrote its listing *over*
`src/lib/text.test.ts`, replacing 598 lines of assertions with a JSON array.
Exit status 0, stdout empty, nothing on stderr.

**Every reading afterwards was consistent with a filter that had matched
nothing.** An empty stdout is exactly what `list` prints for a filter naming no
file, and the next command in the session --- a full `vitest list --json`
redirected properly to a scratch file --- reported **50 files collected against
51 on disk**, which reads as a vitest quirk worth investigating rather than as
damage already done. It took `head` on the file itself to see the JSON in it.
The tell nobody looks at in a probe loop was `git status`, which said `M
src/lib/text.test.ts` the whole time.

Recovery was one command, `git checkout -- src/lib/text.test.ts`, and it was
available only because the worktree had been clean when the session started ---
the file was tracked and unmodified, so the working copy was the only casualty.
Had the same probe been run mid-increment against a file with uncommitted work
in it, the content would have been gone.

**The class, which is larger than this flag.** An option with an optional value
cannot distinguish "the value" from "the next positional argument", so the
argument grammar decides, and for `--json` it decides *destination*. `--reporter`,
`--outputFile` and `git`'s own `--output` are the same shape. Two habits close
it: put a value on the option with an `=` when you mean one (`--json=/tmp/x`) and
nothing at all when you do not, and **never let a path you care about be a bare
positional after a flag that writes**. Collect everything and filter in the
caller, which is what `check_mutation_test_files.py` does.

The gate that came out of this now carries the warning in its own docstring,
beside the call it applies to --- which is the only place someone about to repeat
it will read.

Paid for on 2026-08-24, while automating the check that would have caught the
twelve `TEST_FILES` omissions before their runs.

### A correction that changed the direction of a movement that was never happening

`save: pad every mark's rectangle down the page` exists to prove that
`control: paper no mark was placed on renders identically` can go red. It pads
every annotation's `/Rect` 120 points toward the foot of the page, so that ink
lands in the eleventh band the phase deliberately leaves bare.

It has now SURVIVED twice, and the second time is the interesting one.

**The first survival was diagnosed and corrected, plausibly and wrongly.** That
version replaced the whole rectangle with the page box, and the note written at
the time reads: *"`bounds` works in the page's own space where y grows upward, so
growing the box moved the ink away from the band below."* Every clause of that is
true about coordinate spaces. The correction --- pad *down* instead of up ---
followed from it, and the mutation survived again.

**Neither direction could ever have worked, because the edit is on the wrong
side of a boundary.** `bounds()` feeds `/Rect` and nothing else. The ink comes
from `appearance_stream(doc, mark, &quads, &strokes, rect)`, which draws from the
**quads**. So padding the rectangle grows the box and leaves every drawn pixel
exactly where it was: the bare band renders identically, and the control is right
not to fire. The correction changed the direction of a movement that was not
happening in either direction.

**The evidence was in the harness's own output both times.** It prints the checks
that *did* go red, and one of them read `83.0% on screen against 0.0% in the file
(207.4x)` --- a mark whose ink covers none of its own rectangle, which is a
one-line description of "the rectangle moved and the ink did not". It was read as
a detail of a failing mutation rather than as the diagnosis, because attention
was on the check that stayed green.

**What it cost.** Exactly one mutation named that control, and it could not redden
it. So `control: paper no mark was placed on renders identically` --- the control
that makes every coverage reading in the phase meaningful, by refusing a render
that differs everywhere --- had only ever passed, for as long as it has existed.
A control with nothing able to break it is the exact thing this harness is for.

**The fix, and why it carries no constant.** A second mutation aimed at
`user_quads`, which feeds the appearance stream *and* the dictionary, so the ink
moves with the box: every quad's lower edge dropped to the page origin, which is
the shape of a mapping that lost its flip. Not an offset --- the bare band's
position is a *fraction* of the page, so 120 points lands two bands down on A4 and
somewhere else on anything taller. Reaching the origin is true at every page size.
Measured red, not argued: `-> 2 red`, the named control among them.

**The question to ask of any surviving mutation**, and it is not "is my
assertion strong enough": **which output of the code am I actually on?** A
function that fills a metadata field and a function that produces the drawn
pixels are different sides of a boundary, and a check that reads pixels can only
see the second. The trap index already carries *A mutation that survives may be a
variant, not a gap* and *A mutation that survived, a comment that claimed a
behaviour, and no test to add*; this is the third shape, where the mutation and
the check were never connected at all.

And the corollary that made this expensive: **a correction derived from the same
wrong premise inherits it.** Up-to-down was a real change to a real quantity that
nothing downstream reads, and it looked like progress because the mutation was
different afterwards.

Paid for on 2026-08-24, in the window-mutation pass after `26.8.8`.

### A mark's rectangle survives a quarter turn and everything drawn inside it does not

`save::user_quads` maps a mark out of the reader's frame and into the page's own,
which is right for the rectangle and wrong for every direction inside it. On a
page carrying `/Rotate 90` a box the reader dragged 300 wide and 40 tall arrives
40 wide and 300 tall --- the same set of points, with its sides swapped --- and
every arm of `appearance_stream` that draws something with a *direction* was
reading those sides as the reader's.

Measured 2026-08-24, one mark of each kind on one box, `testdata/inherited.pdf`
against `testdata/text-base14.pdf`, reading where the ink landed inside the box
as displayed:

| kind | upright | turned |
|------|---------|--------|
| underline | a band at y 0.93..0.99 | a rule down the left edge, x 0.00..0.07 |
| strikeout | y 0.46..0.53 | a vertical line, x 0.46..0.53 |
| squiggly | y 0.81..0.99 | x 0.00..0.15 |
| text box | x 0.01..0.34 | a column at x 0.82..0.98 |
| stamp | 25,011 px | 11,024 px, sideways |
| highlight | the whole box | the whole box |
| box | its four edges | its four edges |

**Four kinds wrong, and a scanner's output is where a reader meets it.** `/Rotate
90` is what a scanner writes; a reader underlining a scanned contract got a
vertical line down the left of the words.

The text box is the worst of the four because two things go wrong at once.
`textbox::wrap` was handed 40 points where the reader had dragged 300, so the
model --- which works in the reader's frame throughout --- broke four words into
**one** line and the writer broke them into **eighteen**, two glyphs across; and
each of those eighteen was then drawn along the page's own axis. Its appearance
`/BBox` came out `[80 72 84 528]`: four points wide and 456 tall.

**Why nothing caught it, and this is the reusable half.** The two kinds that came
out right are exactly the two whose shape is symmetric under a quarter turn. The
window sweep's agreement check compares *coverage fractions*, and a band turned
through a right angle covers the same fraction of the same rectangle --- so three
of the four defects were invisible to it by construction. `annot-probe --mode
rule` and `--mode wave` refuse a rotated page outright, in their own words,
because the strip they measure is not horizontal there. **Every instrument aimed
at these marks was either blind to rotation or excused from it.**

The one check that did fire was the text box's, at 27x, on the one rotated
fixture in the corpus --- and the diagnosis written down at the time was that the
fixture's pages were short. That was the fixture's most conspicuous property and
it was not the cause: measured across three unrotated fixtures afterwards, the
overlay and the file agree exactly, both drawing nothing below **13.0 pt** and
one line at and above it. **A red check on a single fixture is evidence about
that fixture; which of its properties is doing the work is a separate
measurement, and the conspicuous one is the tempting answer.**

The repair is `save::Upright`: the box as the reader saw it, plus the map back
into the page. Text is set on a `Tm` rather than a `Td`, because `Td` can only
move an origin and cannot say which way the glyphs face.

**And the first assertion written for the rule could not fail.** It said the band
comes out "long the way the words run and thin across them" --- a proportion,
measured along the axis the defect is on. A mutation taking the *thickness* from
the page's box survived it, because a rule 7.5 times too thick is still thinner
than the box. The assertion that works is the differential: the same box on an
upright page and a turned one, read back through `text::to_device` and compared
as fractions of the box, all four edges.

### A multiplied mark's coverage is a reading about the page, not only about the mark

`examples/turned_probe.rs` compares one mark across the four pages of
`testdata/rotated.pdf`, whose own generator says they *"carry identical content
and differ only in /Rotate, so any difference the probe reports is the rotation
and nothing else"*. A highlight reported 0.933 of its box inked on page 0 and
1.000 at a half turn.

Nothing about the mark had moved: its ink bounding box read 0.00..1.00 by
0.00..0.99 on all four pages. A highlight is drawn with `/BM /Multiply` so the
words stay readable underneath, and multiply leaves a pixel where it is wherever
the paper is already dark --- so the pixels that did *not* move were the glyphs
under the box. The fixture confines its type to the upper part of the **page**,
which is a different part of the **display** at every turn.

So the generator's claim is true and does not say what it is quoted as saying: it
is about the page's own space. A reading taken in display space over a
content-sensitive blend mode is a reading about the content.

The probe compares the ink's extent for a wash and its coverage for everything
else, using `save::is_wash` rather than a copy --- and the exclusion can only
shrink coverage, never grow it silently: a kind that *became* a wash would still
have its coverage compared until that line was changed, so the run would go red
rather than pass in silence.
