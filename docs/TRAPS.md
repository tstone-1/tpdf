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
