# Probes, benches and window checks

One section per instrument: what it answers, how to run it, and what running it found. Moved here verbatim from `BUILD.md` on 2026-10-23, which indexes this file.

### Which backend parses the document

Documents are parsed in a sandboxed worker process, one per document. `TPDF_BACKEND`
overrides that:

```
TPDF_BACKEND=worker      # the default on macOS
TPDF_BACKEND=in-process  # the control, and the only thing that runs off macOS
TPDF_POOL=6              # workers one document may have
TPDF_IDLE_MS=30000       # how long one may idle before it is killed
```

`TPDF_IDLE_MS` is a quantity and **zero means zero** — retire at the first sweep. There is
deliberately no spelling for "off": a "no value" marker taken from the value's own range is
how a sentinel collides with a real value the moment the timing is right, which this
repository has already paid for once. A caller that wants no retirement asks for a long
timeout. Unlike `TPDF_BACKEND`, an unreadable value here falls back to the default rather
than refusing, because it cannot make two measurements silently incomparable — every
harness that depends on the timeout is handed one explicitly.

Anything else is **refused before the window is created** — one line on stderr, exit 2. The
variable exists to say which of two implementations ran, so a value that quietly selected
the other one would make any comparison between them meaningless, and `in_process` for
`in-process` is one underscore away.

The refusal is read in `run()` rather than where the backend is used, and that placement is
the whole of its value. `RenderService::start` runs in the Tauri setup hook, which `App::run`
invokes from AppKit's frames — a panic there is non-unwinding, aborts through a backtrace
with no symbols, and races the watchdog's 30-second report about a page that never ran. A
misspelt variable would be diagnosed as an occluded window.

Two things read differently under the worker: the startup timeline has `worker spawned`
where the in-process one has `pdfium bound`, and a render can now fail because the worker
died rather than only because the document did. `backend-probe` is what says the two agree
about everything else.

A worker that dies is replaced and the request retried once, so a crash usually reaches the
reader as nothing at all — but it is never silent in the terminal: the parent prints
`[render] document N: worker killed by signal 11; starting a replacement` on stderr, and the
worker's own stderr is inherited. Seeing that line repeatedly on one document means the
document is faulting PDFium on a page the reader keeps asking for, which is the one case a
single retry cannot make cheap.

### `ocr-probe`: does the recogniser work, and is the flip right

macOS only — it is the Vision binding it exercises. Nothing in it is wired into the viewer;
OCR has interfaces, one engine and a control chooser, and no worker yet.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| fixture | result |
|---|---|
| `text-base14`, `text-marked`, `text-truetype`, `text-cid`, `rotated` | 9/9 |
| `outline-simple` | 8/8, 1 skipped |
| `form` | 7/7, 2 skipped |
| `columns` | 2/2, 4 skipped — two columns leave no vertically isolated span to use as a control |
| `vector-heavy` | 1/1 against the *inverted* claim: the page has no text, so reading none is correct |
| `links` | **7/8**, 1 skipped — one expected red, below |
| `encodings` | **7/8**, 1 skipped — one expected red, below |
| `text-wide` | 9/9 — the wide-sheet fixture, below |

⚠ **Those counts were two behind on 2026-08-28 and are re-measured here.** The shape sweep's
control was added in the same commit that last touched this table and the row was not moved with
it, so every fixture the sweep runs on read one low before today and two after. `columns` and
`vector-heavy` are unchanged, which is the tell that the drift is the sweep: it is skipped on
both. A count in prose has no gate behind it — derive it from a run, and re-derive it whenever
a check is added.

**A shape sweep prints above the checks**, added 2026-08-28 and not a check — it passes and
fails nothing. It exists because the corpus probe's largest remaining bucket is the engine
*answering* and returning no spans at all, and a corpus measurement cannot look at the image it
was handed while a fixture can.

**It sweeps the region strip's height, which is the corpus's own variable**, and every row is a
real `ocr_gate::stack` output rather than a resized one — so the sweep and any padding change
are the same code path. The control strip is byte-identical in every row. Two earlier drafts got
the construction wrong and both were corrected the same day: the first used the page's tallest
blank band and measured a shape the gate never builds, and the second grew the image with
`Vec::resize`, which appends white *below the bottom margin* rather than widening the image the
way `stack` would.

⚠ **The fixed rows cap the aspect, so the band the corpus goes silent in cannot be built here at
all.** `stack` always writes two margins, the gap and the control strip, and on these fixtures
that is 104 to 117 px of a 1190-wide image — a ceiling of **10.1:1 to 11.3:1**. Every target at
12:1 and wider prints how many rows short it is instead of a reading, because a shape that was
never built must not read like a shape that was tried and said nothing. Reaching past 16:1 needs a
*shorter control*: the aspect is `width_pt / (tallest + control_pt + padding)` and the scale
cancels, so with `padding` fixed at 24 pt a 595 pt page needs the region and the control together
under about 13 pt.

**Two checks are the controls over the sweep.** The strip has to read at *some* shape, or every
row is a statement about that strip rather than about the proportions; and the token has to read
back at the gate's own shape, or a "no" further out is not evidence about the shape either. The
second was written from `height == real_h` inside the loop, which no swept aspect ever produces
— it failed on all four fixtures and was right to. It reads the gate's own image explicitly now.

**The `trailing` column separates the aspect from where the padding rows sit**, and it overturned
the reason padding was called a candidate rather than a win. At equal height and equal aspect,
`outline-simple` at 4.0:1 reads the token back when the white is in the region strip and **not**
when it is appended below the bottom margin. The earlier "padding loses the token at 1.9:1" was
about trailing whitespace after the control, not about proportions. Through `stack`'s own
construction the token reads back at every buildable shape on `text-base14`, `outline-simple` and
`encodings`.

**`testdata/text-wide.pdf` is the only fixture that reaches the band the corpus goes silent in**,
and it says the shape is innocent. A 1684 pt sheet with ordinary 14 pt text builds an **18.1:1**
probe image and sweeps to **28.1:1**, where A4 with the same text caps at 10.8:1 — the lever is
the page's width, so the control strip stays a comfortable 34.5 pt against A4's 30.5. Vision
returns a span and reads the token back at 28.1:1, 24.1:1, 20.0:1, 18.1:1, 16.0:1, 8.0:1 and
4.0:1, and loses only the token (not the span) at 2.0:1. `docs/PLAN.md` §6 has why that kills the
padding repair: on a page of ordinary width a wide probe image *requires* a small control, so the
corpus could never separate the two.

⚠ **The two existing fixtures that come closest cannot answer it, and the sweep's own control says
so.** `text-heavy` and `incr-xrefstream` reach 12.2:1, and on both *the token reads back at the
gate's own shape* fails — their controls are too small to be read reliably, so no column of
theirs is evidence about shape. Do not read their `no`s as the wide band starting early.

**The control-chooser check**, added 2026-08-27 — the ninth on a fixture where every check runs, and named here rather than numbered because appending a check renames a number, and it is the only place
`ocr::control_from_page`'s claim meets a real engine. The three gate checks above it take their
control strip out of **Vision's own output**, which is the engine agreeing with itself; this one
chooses from what the *document* says and then asks Vision to read it back. It runs in both
directions: where the engine's reading and the document's text agree the chosen control must
certify, and where they do not it must refuse.

**Two fixtures report one red each, both of them expected, and they are expected for opposite
reasons.** An expected red beside a green run is a bad thing to leave lying around — this
file has an entry about exactly that — so here is what each is and what it would take to
remove it.

`encodings.pdf` fails *what it read matches the embedded text*, 0 of 2 words. That is the
fixture doing its job: it has no usable `/ToUnicode`, PDFium returns plausible garbage, and the
check compares the engine's reading against that garbage. Making it green needs a way to tell a
broken engine from a broken text layer, and there is not one — the check *is* that
comparison. The chooser check reads the same disagreement and reports the refusal as a pass,
which is the honest verdict about the gate rather than about the fixture.

`links.pdf` fails *a blank strip adjudicates Illegible*: the strip control picked the token
`"Donn"` and Vision, handed the same rows inside a composite, read `"Dann 1"`. Nothing is wrong
with the page. It is the weakness the chooser exists for, showing up in the check that predates
it — a control taken from the engine's own earlier reading is not stable across a second call
on a different image. The same fixture passes the chooser check with `"lantern"`, chosen from
the document. **The fix is to give those three checks the same control source**, which is a
change to three verified checks and is deliberately not in the increment that added the fourth.

**The check that earns its keep is the ordering one.** `normalised_to_points` has unit tests
and they cannot catch the thing that matters, because they assert arithmetic against numbers
the same file wrote — Vision's `boundingBox` is normalized with the origin bottom-left, and
whether the conversion understands that is a question about a black box. So the probe asserts
content at a position: the word the *document* places highest must come back highest. Removing
the flip reports `read gap -119 pt against 123 pt in the document` and takes both gate checks
with it.

Two limits worth knowing before reading a run. The control band is a strip of the page's own
text rather than a drawn token, so a fixture whose lines are too close together produces no
usable strip and the gate checks `[SKIP]` rather than failing — `columns` is that case. And
`[SKIP]` here means the harness could not construct the input, never that the gate passed.

### `win-ocr-probe`: can `Windows.Media.Ocr` be the Windows engine at all

Windows only, and it runs **in CI** — the Windows leg of both `ci.yml` and `release.yml`, as a
step after the gates. That is the point of it: the question is what a machine nobody configured
carries, and the developer machines are all configured. Read the two `[verdict]` lines in the
job log.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example win-ocr-probe
```

**Not a gate.** It measures, and its exit code says whether it could *measure* — 0 for any
answer including "no language packs", 2 for a call that failed. A probe that reddened CI for
reporting an inconvenient truth is one somebody switches off, and the answer would go with it.
The cost of that choice is the one `AGENTS.md` records about `18/19 gates passed`: a step that
always exits 0 is a step nobody reads, so the verdict lines are written to be grepped.

Four readings, and the last two are why this is not an enumeration:

| reading | what it decides |
|---|---|
| `AvailableRecognizerLanguages` | whether the in-box engine ships at all, which is `docs/PLAN.md` §9.10's ranking |
| `TryCreateFromUserProfileLanguages` | whether the call an implementation would make comes back |
| `MaxImageDimension` | a real bound on `ocr::Pixels`, since the gate hands over a composited image whole |
| a word and a **non-word**, read back | whether `Options::language_correction` can be honoured here |

The last row is the one that reaches the interface rather than the ranking. That option is
documented as off for verification *always*, because a corrector turns marks it cannot read into
plausible words; Vision honours it (`ocr_vision.rs`'s `setUsesLanguageCorrection`) and
`Windows.Media.Ocr` exposes no such switch. A non-word coming back as something else means a
verdict from that engine means something different from a verdict from Vision.

**First reading, `windows-2025`, 2026-08-29** — the runner image as GitHub ships it, which is
the whole point of taking it there:

| reading | value |
|---|---|
| recogniser languages | **1**, `en-US` (English (United States)) |
| `TryCreateFromUserProfileLanguages` | an engine, `en-US` |
| `MaxImageDimension` | **10000** px |
| 44 px, `"REDACTED"` / `"qwrtzp"` | both **VERBATIM** |
| 16 px, `"REDACTED"` / `"qwrtzp"` | both **VERBATIM** |

So the gating question is answered and answered well: a stock Windows carries a pack, and the
in-box engine is a feature that ships rather than one that needs the machine set up first.

The 16 px row was added the same day because the 44 px one alone is a control easier than the
check: 44 is about 3x `ocr_gate::MIN_CONTROL_PX`, a corrector's effect is largest on marginal
input, and marginal is exactly what this gate hands an engine — a control sized from the
smallest box a redaction covered. 10000 px is a real ceiling on `ocr::Pixels`, worth knowing
before a page is composited at render scale.

**No correction was observed anywhere the probe looked**, which is the better of the two
answers `Options::language_correction` could have had. It is support rather than proof, and the
gap is specific: at 16 px this engine read clean synthetic text *exactly*, so it was never
operating near its limit, and a corrector only shows where a recogniser is struggling. What the
gate actually hands an engine is harder than this in a way size does not capture — a control
composited beside real page ink, at whatever contrast the document has. **The remaining risk
therefore moved rather than closed**: it is no longer "the API exposes no switch, so the
contract may be silently broken" but "we have not yet seen this engine read anything it found
difficult". The instrument for that is the corpus sweep the macOS side already has
(`redact-reach-probe`), not another synthetic string.

⚠ **A blank reading for *both* strings is a suspect probe before it is a suspect engine.** GDI
writes RGB into a 32-bit DIB and leaves the alpha byte alone, so the buffer forces alpha to 255
after drawing; if that were wrong every glyph would be transparent and the engine would honestly
report no text. The comment in `draw` says so. Vary the fixture — a larger entry in `SIZES_PX`, a
different face — before concluding anything about `Windows.Media.Ocr`.

**The containment rung, added 2026-08-29.** Everything above runs at whatever integrity the
shell gave the probe, and a real engine would run where the parser worker runs. So the probe
re-execs itself with `--contained-child` through **`sandbox_win::spawn_contained` with
`Containment::default()`** — the containment that ships, job object plus low integrity — and
takes the same four readings there. macOS answered the mirror of this with *no*: Vision is
killed by SIGTRAP under `SANDBOX_PROFILE` and needs general `file-read`, which is why OCR is a
separate process under `OCR_SANDBOX_PROFILE`. If the same holds here, an in-box Windows engine
needs a second containment story rather than a line in the worker.

Three things make that rung worth trusting:

- **The child proves it is contained before it measures.** `sandbox_win::assert_contained()`
  first, exiting 3 if not. A child that quietly ran uncontained would report that the engine
  survives containment, which is the direction that costs something.
- **The verdict is a comparison, not a survival check.** The uncontained readings are the
  control and the two lists are compared as data. The outcome to fear is not a child that
  died but one that read something *different* — a substituted font or a denied resource
  looks exactly like that, and `docs/TRAPS.md` records a sandboxed PDFium returning `ok`
  while silently swapping a typeface.
- **Dying is a result, not an error.** The child's exit code is read before its answer is
  parsed and passed through `sandbox_win::describe_exit`, because macOS's lesson is that this
  class of engine aborts its host rather than refusing.

It reuses `sandbox_win` rather than building a ladder of its own. `win-sandbox-probe` built six
rungs to find which one PDFium survives; that question is answered and the answer is what
`Containment::default()` implements, so a second ladder here would be a second copy of
security-critical code — and the copy that drifts is the one nobody ships.

**Since 2026-08-29 this drives the shipping engine**, `ocr_windows::WindowsOcr`, rather than
calling WinRT itself. So every CI run exercises `WindowsOcr::recognise` end to end — bitmap
construction, the word walk, the coordinate conversion — contained and uncontained, instead of
a parallel copy of the same calls agreeing with itself. One thing it still cannot see: the probe
draws black on white, and exchanging two channels leaves black and white unchanged, so a missing
RGBA-to-BGRA swap is invisible to any reading here. `ocr_windows`'s own unit test is the
instrument for that, and has to be.

**Measured `windows-2025`, 2026-08-29: `reads IDENTICALLY to uncontained`.** All four readings
come back the same under job object plus low integrity as outside it, so `Windows.Media.Ocr`
does **not** repeat what Vision does on macOS — the engine needs no separate containment story
and can run where the parser worker runs. That is the last thing standing between the interface
and a Windows implementation of `ocr::Recogniser`.

⚠ **That run also revealed a regression this probe had shipped one commit earlier**, and it was
found by diffing the CI output against the previous run rather than by any check: extracting
`languages()` and `make_engine()` replaced a region of `main` that two `say` calls sat in, so
`engine language` and `max image dimension` stopped being printed while `BUILD.md` went on
recording 10000 px as measured. Restored the same day. See the trap about a readings table
outliving the code that produced it, and note what it costs to write a measurement down: the
thing to protect is the call, not the number.

### `ocr-sandbox-probe`: what is left of a process under each profile

macOS only. Three rungs, each a re-exec'd child that renders a page **before** the profile
comes down — the parser worker maps PDFium first too, and sandboxing earlier would measure a
different program.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-sandbox-probe -- \
    testdata/text-base14.pdf --lib vendor/pdfium/lib
```

| rung | writes a file | reaches the listener | runs Vision |
|---|---|---|---|
| `bare` — the control | ok | ok | 4 spans |
| `ocr` — `OCR_SANDBOX_PROFILE` | PermissionDenied | PermissionDenied | 4 spans |
| `parser` — `worker::SANDBOX_PROFILE` | — | — | killed by signal 5 |

7/7 on OS build 25G83, 2026-08-27. This makes executable the table `ocr.rs` has carried by
hand since 2026-07-31, and it measures something that one did not: the rung that worked there
allowed reads and said nothing about **writes**, while the constant that shipped denies
`file-write*` and `network*`.

**7/7 again on macOS 27.0 (26A428), 2026-09-27, in 47 s --- and it was 7/7 there before the
fix too, while every redaction on that machine was *not verified*.** Two things made it blind,
and both are changed:

- **Every rung shared one Vision cache.** Vision on 26A428 compiles its models on first use and
  caches them under `~/Library/Caches/<executable>/`; the unsandboxed `bare` rung ran first and
  filled `ocr-sandbox-probe`'s, so the `ocr` rung never needed the write its profile refuses.
  Each rung now runs with `CFFIXED_USER_HOME` set to a fresh, empty directory the parent
  creates and removes, which is where Core Foundation --- and so Vision's cache --- takes the
  home from (`HOME` and `TMPDIR` do not move it; measured). Each rung therefore pays the cold
  compile, about 23 s, which is where the 47 s goes.
- **The `ocr` rung applied the profile itself.** It now crosses the boundary through
  `ocr_worker::enter_boundary`, the function the worker's `serve` calls, so a change to the
  worker's order is a change to what this measures.

The control for both: with the worker's warm-up line removed, this probe is **6/7**, `ocr:
vision still reads the page` failing with `__objc2.missingError` --- the gate's own failure,
which the old probe could not produce.

**The parent holds a real listener open and passes its port**, and that is not a nicety:
`ConnectionRefused` and a sandbox denial are the same shape from a client's side, so without
something to connect to every rung reports a refusal and the row measures nothing. The `bare`
rung is the control for all three columns — a machine where nothing works reports a
perfectly contained ladder.

### `ocr-worker-probe`: does the engine work from a process of its own

**Linux since 2026-10-14**, with Tesseract: 12/12 with one skip on `text-base14`, `rotated` and
`links` on Ubuntu 24.04 under WSL, in a debug build (a release build finds no language data
unless `tessdata/` is beside it). Linux adds one row: `Seccomp: 2` in the worker's
`/proc/<pid>/status` against `0` in the probe's own, which went red with the boundary removed.

**Both platforms since 2026-08-29**, and the binary is **its own worker**: `OcrWorker::spawn`
re-execs `current_exe`, so what is under test is the shipped child rather than a copy of it.
Same arrangement `pool-bench` uses.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example ocr-worker-probe -- \
    testdata/text-base14.pdf
```

**No `--lib`, and not for brevity**: the default joins `PDFIUM_SUBDIR` — `bin` on Windows,
where `lib` exists, holds the *import* library and binds to nothing. It hardcoded `lib` until
this became portable, and `only_the_macos_spikes_hardcode_the_library_directory` is the rule
that caught it, which is what that test is for.

**It was macOS-only for one line.** The in-process baseline named `ocr_vision::Vision`
directly; `WindowsOcr` is behind the same `ocr::Recogniser`, so only the engine's
*construction* is per-platform now and everything after it is the trait. That mattered more
than it sounds: this probe measures the **worker**, the Windows worker is the newest thing in
the subsystem, and Windows was the one platform that could not measure it. A spike is
macOS-only when its subject is, never when one line of its scaffolding is.

It runs on both CI legs, at **12.7 s** in the debug build the `bins` gate leaves behind.

| fixture | result |
|---|---|
| `text-base14`, `text-marked`, `rotated`, `links`, `columns`, `encodings` | 12/12 |
| `vector-heavy` | 0/0, 1 skipped — A0 at scale 2 is 128 MB against a 16 MB buffer |

**It measures a warm engine and says nothing about the cold path.** The in-process baseline runs
Vision unsandboxed before the worker is asked anything, so on macOS it fills the executable's
model cache first; it stayed 12/12 on macOS 27.0 while every cold worker failed. The cold path
belongs to `ocr-sandbox-probe` above, which is built to see it; this one measures the handover.

The check **set** is the invariant, not the total: on Windows the *engine is mapped from launch*
row is absent, because it is a statement about static linkage — `objc2-vision` links Vision,
while `Windows.Media.Ocr` is WinRT activated through `combase` at the first call. What its
images are and when they arrive is a different question and **unmeasured**; a row asserting a
name nobody has measured would be a guess wearing a check's clothes.

**The baseline is the same program reading the same bytes in-process**, because a worker that
reads nothing and an engine that reads nothing produce identical output. Everything else is a
difference from that row, and the differential is the one that matters on every page: same
engine, same pixels, one process apart, so the text has to be *identical* and a mismatch is
the handover rather than the engine.

Two rows are there because a caller cannot recover from them. An image larger than the shared
mapping must be refused **and leave the worker usable**, or one oversized region costs a whole
document its verification. And a worker killed from outside must report inside its own
deadline rather than block on a pipe nobody will write to — the engine ignores the
`deadline_ms` it is handed, so the parent is the only place that bound can live.

**What this probe does *not* prove, and the first draft claimed it did:** that the process
which asks never maps the engine. `objc2-vision` links Vision, so every binary linking
`ocr_vision` maps it at launch — 2 images of 619, before a single call. `backend-probe` can
make that claim about `libpdfium` because `pdfium-render` `dlopen`s it. The check states the
measured fact instead, with an emptiness control beside it. See `docs/TRAPS.md`.

### `textlayer-probe`: does a text layer read back as the words it was written from

Every platform; it needs no OCR engine. The words come from a document that already has
text: it reads each page's words and boxes through PDFium, empties every page's content with
`lopdf`, writes the words back with `textlayer::write`, and reads the result through PDFium.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example textlayer-probe -- \
    testdata/rotated.pdf
```

| fixture | result, 2026-10-03, macOS arm64 |
|---|---|
| `text-base14`, `text-cid`, `links-cropped` | 7/7 |
| `inherited` (`/Rotate 90`, inherited box) | 14/14, 3 skipped |
| `rotated` (0, 90, 180, 270) | 19/19, 3 skipped |
| `rotated-90` | 18/18, 4 skipped |
| `links-rotated` | 11/11, 1 skipped |
| `columns` | 17/17 |
| `text-heavy` | 3877/3877 |

The skips are the order check on turned pages: those fixtures' type is upright in the
page's own space, so their word order is not a reading order of the displayed page. Do not
run it on `encodings`: that fixture's words hold control characters, which the writer drops
on purpose.

**Run a turned fixture as well as an upright one.** The turn is the part most likely to be
wrong and an upright page cannot show it. And the crop: `links-cropped` is the one fixture
whose displayed corner is not the sheet's.

What each check is for, since three of them exist because the layer was wrong without them:

| check | goes red when |
|---|---|
| the emptied page reads no words | the control: everything after it would pass on the text that was already there |
| every word reads back at its box | the crop origin is dropped, the scaling or the baseline is wrong, the glyph has no height |
| every character is upright as displayed | the type runs the page's way on a turned page. **Nothing else sees this**: the glyph is a box, and a box set sideways in a rectangle fills the same rectangle |
| the page renders as the emptied page did | the layer paints |
| three words whose boxes touch read back as three | no space is written after a word. The fixture's own words cannot show this: their boxes are PDFium's, with the page's real gaps between them |

Seven mutations of `textlayer.rs` were run against it by hand that day, four for the second
row and one for each row after it, and each went red. They are in `scripts/mutate_rust.py` under
`text layer:`, aimed at the unit tests; the probe is the half those cannot be, because a unit
test reads the operators the writer wrote and this reads what PDFium makes of them.

**The allowance is relative**, `0.05 pt + longer side / 64`, because PDFium reports the
layer's glyph 1/64 taller than its ascent and 1/64 wider than its advance. A flat half point
was the first allowance and passed a layer whose every word ran 2.4% long.

**Rebuild after restoring a mutation.** The probe is a binary; restoring the source does not
restore it. One run here reported every fixture red against correct source, because the
binary on disk was the last mutant's.

### `compress-probe`: how much smaller a copy gets, over a corpus

Not a check. It is the instrument behind `docs/PLAN.md` *A smaller copy*: each document in
a directory is made smaller in the four ways `tpdf compress` offers and serialised as that
command serialises it, and the totals are printed. Sizes and counts only.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example compress-probe -- \
    ~/Downloads --emit /tmp/smaller
```

`--emit` keeps every copy, for `qpdf --check` and for a pixel comparison against the
source. It loads with `lopdf` in its own process and not in a worker, so point it at
documents you trust. A document that opens with an empty password is decrypted on load
and its copies are written without encryption, which the tool itself does not do. 65
documents and 255 MB take 104 s.

### `redact-reach-probe`: how much of a redaction can be proved, over a corpus

Not a check — it passes nothing and fails nothing. It is the instrument behind
`docs/PLAN.md` §6's *What a removal can take, re-measured*, and it exists because the 39.1%
that section had quoted since the beginning was measured before the form carrier, before the
image carrier and before there was an OCR gate at all. **A figure that decides which increment
comes next is worth exactly as much as the date on it.**

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-reach-probe -- \
    ~/Downloads --pages 3 --regions 40 --no-gate
```

**Counts and shapes only.** Point it at a corpus of real documents: no page text, no
recognised string and no filename beyond the stem leaves it, because a measurement that prints
what it read is one nobody can run twice.

| flag | what it does |
|---|---|
| `--pages N` | pages sampled per document, spread through it rather than off the front |
| `--regions N` | regions sampled per page — one per word of four characters or more |
| `--max-mb N` | files above this are not opened; a rewrite copies the whole document |
| `--no-gate` | skip the write-and-read-back half, which is 40x the cost |
| `--full-width` | widen every region to the page. A **control** over the gate, not the removal |

Under *taken whole* it prints how many regions cut a drawing at their edge and how many of
those were then taken whole, which is the measure of `pathcut.rs` (`docs/PLAN.md` §6 *A rule
cut at the region's edge*): 651 and 565 of 4,616 on the 2026-10-03 sample.

The cheap half is 1.8 s over 40 documents and 2,893 regions; with the gate on it is about 12 s
for a twentieth of that sample, which is why the two halves are separable.

**`--full-width` is a control that failed to isolate what it was aimed at, and is kept for
what it found instead.** `ocr_gate::strip` renders the rows a rectangle covers as a
full-width tile, so widening a region leaves the row band identical and should move no
verdict. It moves them a great deal — 54 *still readable* became 9 on one sample — because
a wider region covers more words, which changes the control the gate may choose, which
changes the render scale. The region feeds two mechanisms, so varying it isolates neither;
what it establishes is that **the verdict turns heavily on the control choice**, which
nothing else here measures.

The gate half reads `ocr_gate::judge_all` rather than `run`, so it has the engine's own
rectangles and reports how many surviving reads were inside the region's own columns. Since
`ocr_gate::mask_columns` that has been all of them, on 104 regions and again on 448.

**Every *not verified* region is attributed to a step, and the buckets have to close.** Each
prints as its own row — twelve of them, including the ones that never fired, because an
absent row and a zero are different readings. `NotVerifiedCause` is a type rather than a
substring of the sentence: the version before 2026-08-28 bucketed by
`why.contains("control token")` and discarded the verdict of every page-wide refusal, so it
could attribute one cause of twelve. The `[WARN]` beneath them is the check — buckets plus
run-refusals must equal the unanswered total, so a region that reached it by a route carrying
no cause is subtracted and named rather than absorbed.

**Two extra axes print under *control not read back*, and only under that one.** It is the sole
cause where the gate got as far as showing the engine something, so it is the only one with a
rendered control to describe. The first bucket is how tall that control landed against
`ocr_gate::MIN_CONTROL_PX`, which is the bound the scale rule exists to clear — a row below the
floor is the rule missing what it aims at, and on 2026-08-28 that was 34 of 38. The second is how
many characters the token drew, because `ocr::adjudicate` matches by containment and one
recognised span has to hold the whole token. The first prints every bucket including the empty
ones, with a `[WARN]` if they do not sum to the cause's own count.

⚠ **The token axis prints `unread / all` and a rate, and the denominator is not decoration —
it is what stopped a wrong increment being built.** Read as a numerator alone the bucket says 29
of 33 unread controls drew eight characters or more, which reads as an indictment of
`control_from_page` picking the *longest* qualifying word. With the denominator it says 29 of
**128**: a rate of 22.7% against 33.3% for five-to-seven characters, so long tokens fail *less*
and the obvious repair moves the chooser toward the worse bucket. A count of failures bucketed
by a property is never evidence about that property until the same bucketing is applied to the
population.

⚠ **That reading is macOS's, and Windows reverses it.** Measured 2026-09-02 over 109 documents
and 372 unread controls, the rate climbs with token length rather than falling: 7.6% at four
characters, 10.9% at five to seven, **23.6%** at eight or more. The two engines agree almost
exactly on the long bucket — 23.6% against Vision's 22.7% — and disagree on the short ones,
where Vision is three times worse. So the paragraph above is arithmetically right and its
conclusion does not travel: on Windows the repair it argues against is the correct one. The
denominator saved the argument from being wrong on macOS; what it could not supply is the
platform label, and a rate is a fact about the engine that produced it. `docs/PLAN.md` §6 under
*The gate on Windows* carries both columns.

**A third axis prints under the same cause: what the engine had actually returned.**
`ocr::Unread` rides on the verdict and carries how many spans came back for the whole probe
image, how many fell in the control band, and how far outside the band the nearest span
*containing the token* sat. Three rows follow — *read nothing at all*, *read spans, none
holding it*, *read it, outside its band* — each split by the rendered-height bucket beneath
it. The split is the point: the height rows and the shape rows are two bucketings of one
population, and two marginals bound their overlap without measuring it. Measured 2026-08-28 over
197 refusals at three densities, the outside-the-band row is **0** at every one, and at
`--regions 40` exactly 40 of the 80 silent refusals had a control at or above `MIN_CONTROL_PX`
— which the marginals alone could only place between 40 and 80.

**A fourth axis, added 2026-08-28: the probe image's own proportions.**
`ocr_gate::geometry_for` reports the shape it planned, and the row prints `unread / all` and a
rate per aspect band, with the silent count beside it. It is a different axis from the control's
rendered height rather than another reading of it, because an aspect is a ratio and the render
scale cancels out of it — a probe image halved to fit the buffer keeps its shape. Measured over
40 documents at `--regions 12`: **12 / 36 up to 8:1, 28 / 294 between 8:1 and 16:1, 26 / 36 beyond
16:1**, and all 36 silent refusals are in the two tails with none in the middle band that holds
four fifths of the population.

⚠ **That row's denominator has to be counted in the per-region loop, not inside the
`ControlUnread` branch.** Written one scope too low it counts the failures, so every band prints
`N / N 100.0%` — which happened on 2026-08-28, one increment after the trap about denominators
was written. The `bad / all` form is what makes it visible; a bare percentage would have read as a
finding.

**A sixth and seventh axis, added 2026-08-28: the control's height in points, and which clamp
left it short.** Points is what the aspect turned out to be standing in for, and it is the sharper
single reading: **every control under 2 pt failed**, 24 of 24 and 40 of 40 at the two densities,
against 16.1% for 2 to 6 pt and 7.4% for 6 to 12 pt. That boundary is `MIN_CONTROL_PX /
MAX_SCALE` written as the division rather than as `2.0`, so raising either constant moves the
bucket with it. Crossed with the shape it gives the comparison neither axis could make alone: at a
control of 2 to 6 pt, 517 regions inside 8:1--16:1 are 0% silent and 104 beyond 16:1 are 50%
silent, so the shape matters at a fixed control size.

⚠ **The aspect axis is a description of the corpus, not a lever — established by building the
lever and measuring it.** Padding every probe image into the 8:1--16:1 band was implemented in
`ocr_gate` (one rule, two callers, six mutations all caught), and at `--regions 40` it moved 120 of
1,469 regions out of the wide band while changing **no verdict at all**: 79 still-readable, 96
unread, 80 silent, 404 provable, identical before and after. At `--regions 12` it left the silent
count at 36 and took *shown unreadable* from 276 to 264. The change was reverted; `docs/PLAN.md`
§6 and the trap entry have why. Read the aspect rows as a property of the population, and do not
rank work off them again.

**An eighth axis, added 2026-08-28: what a higher scale ceiling would do.** For every unread
control the probe computes the scale it would have needed — `ocr_gate::scale_wanted`, unclamped
— and whether the probe image fits at it, through `ocr_gate::bytes_at` against the worker's
capacity. Both went public for this; neither is a second copy of anything. Measured: **0 of 24 and
0 of 40 would fit**, worst case asking **31.1x** against a ceiling of 8. So raising `MAX_SCALE`
moves the refusal from *the ceiling could not reach it* to *probe image will not fit* and changes
nothing, which is why it was not written. Rendering the control alone at a generous scale does fit
and is unsound — a control read in its own kindly rendered image says nothing about the region
strip.

That measurement is what `NotVerifiedCause::ControlTooSmall` came out of: those regions now refuse
with *no scale renders the control legibly* and a message carrying what the page removed, the scale
it would have taken and the ceiling. **No region's outcome changes** — at `--regions 12`, *control
not read back* goes 66 to 42 with 24 stated, and *shown unreadable* stays at 276 — and the
evidence that it costs nothing was already printed: the points axis carries its denominator, and
every region with a control under 2 pt went unread.

The clamp row answers a question `ocr_gate.rs` recorded as open — *"no measurement has separated
them"*. A sub-floor control comes from the `MAX_SCALE` ceiling being unable to reach 16 px, or
from the image being halved to fit the buffer, and the two can hold together, so *both* is its own
row rather than an arm of an ordered chain. Measured: **24 and 40 from the ceiling, 0 from the
halving, 0 short for neither reason.** The `MIN_SCALE` clamp has never fired on real input; re-run
this if `capacity` or the region sampling changes.

**A fifth axis, added 2026-08-28: the two above, crossed.** The height row and the shape row are
marginals of one population, so equal counts on them are not evidence of one set of regions. The
crossing prints `silent / all` per cell, and populated cells only — an unpopulated cell is not a
zero rate, it is no measurement, and printing it as `0.0%` reads as the former. It answered the
question the marginals could not: at `--regions 12` both rows report a **12**, and the cell
carrying both properties has **no population at all**, so the overlap is 0 and the two tails are
separate defects. `docs/PLAN.md` §6 has the table.

Every row above is guarded, and the guards print nothing when they agree — deliberately not counted here, because a total in prose has nothing asserting it and the two loops each fire per bucket. The three shapes plus the no-evidence count must equal the
cause's own total; the cross-tabulated total must equal the height-bucket total, since both count
the regions that had a measurable control; and the crossing must reproduce **each** of the two
rows it was derived from, checked per axis rather than over the total. The per-axis split is what
makes a failure readable — keying the crossing on a constant aspect fires the shape control and
leaves the height control silent, and a constant height does the mirror, so the `[WARN]` names
which axis drifted. A single check over the total goes red for both and names neither. The points
crossing gets the same treatment — one loop per axis against that axis's own row — and the clamp
rows have to come to the same total as the height rows, since they partition the same regions. A
non-zero *carried no evidence* is a defect in `ocr::adjudicate` rather than a finding about the
gate: the type says that arm always records one.

⚠ **`--regions N` is not only the sample size; it changes what the gate can do, so every
percentage from this harness has to be quoted with its density.** The regions set `size_pt`
— the height of the smallest box any of them covers — and they consume the pool of
surviving words a control may come from, so sampling more of them makes `control_from_page`
harder to satisfy.

⚠ **That also moves which pages reach the shape axis at all, so aspect-band populations are
comparable within a run and not across runs.** A page whose control cannot be chosen contributes
no regions to any denominator here. Between `--regions 12` and `--regions 40`, *no surviving word
is long enough* goes from 60 to 594 and the squarest aspect band empties completely — which is
the opposite direction from more regions producing a taller image, and is not the capacity rule,
since *probe image will not fit* is 0 in both. Measured over the same 40 documents and the same
three pages each:

| `--regions` | gate regions | shown unreadable | not verified | control not read back | control-selection causes |
|---|---|---|---|---|---|
| 1 | 43 | 69.8% | 25.6% | 10 | 1 |
| 4 | 156 | 67.3% | 26.3% | 33 | 8 |
| 12 | 448 | 56.7% | 38.0% | 64 | 106 |
| 40 | 1,389 | 27.1% | 67.2% | 84 | 850 |

Measured 2026-08-28, after `geometry_for` began choosing the render scale from the control
word's own height rather than from the smallest box a region covered. The four rows before that
change read 65.1 / 64.1 / 55.8 / 26.4 in the third column.

A reader marks a name or a line. **Use `--regions 4` for a figure about the gate and
`--regions 40` for a stress of the control rule**, and say which in the sentence that quotes
it. The *still reads as text* rate is 4.7--6.4% at every density and is the one figure here
that does travel.

⚠ **The `--regions 12` row used to be quoted as reproducing `docs/PLAN.md` §6 to the digit, and
that is the wrong half of the row to make a control out of.** A change to the *gate* is supposed
to move the verdict columns, and this one did. What reproduces exactly across a gate change is
the **left** of the table — the region counts (43 / 156 / 448 / 1,389) and the
control-selection causes (1 / 8 / 106 / 850), neither of which any scale can touch. Those are
the control over the harness; the verdict columns are the measurement.

### `sanitize-rewrite`: does a collected rewrite sanitize without losing the document

The instrument `docs/PLAN.md` Phase 3 states its exit criterion in terms of. It runs six
routes over `testdata/hostile-*.pdf` — a byte copy and a plain `lopdf` round trip as
controls, then `lopdf`'s own collection, our mark-and-sweep, `qpdf`, and `qpdf` with object
streams — and checks each against `hostile-manifest.json`, which says per needle whether a
rewrite is supposed to remove it, keep it, or be unable to decide.

```
python3 testdata/make_hostile_pdf.py testdata
cargo run --manifest-path src-tauri/Cargo.toml --example sanitize-rewrite -- --strict
```

**`--strict` is not decoration, and the harness had none until 2026-09-02.** Without it every
run exits 0: a leak, a dropped carrier, a fixture that does not contain its own needles, all
printed and none of them fatal. A criterion nothing can refuse is not a criterion. What
`--strict` asserts, and each part was proved able to fire by a control manifest rather than
reasoned about:

| assertion | control that makes it fail |
|---|---|
| no **collecting** route keeps a needle marked `removed` | declare a surviving carrier `removed` — 4 failures |
| no collecting route drops one marked `survives` | unlink the nested chain from the page — 8 failures |
| a fixture carrying `unverifiable` or `needs-ocr` never reports clean | declare a clean fixture `unverifiable` — 4 failures |
| the **non-collecting** routes leak something | a manifest of only-surviving fixtures — 1 failure |
| a run covered at least one fixture | `--only no-such-fixture` — 1 failure |

The fourth is the one worth understanding, because it is what makes the first three mean
anything. Every collecting route reporting nothing is the same output whether the sweep works
or the corpus hides nothing at all, and only `copy` and `lopdf` — which are supposed to leak
— can tell those apart. On the real corpus they leak 14 carriers.

**It is not a `scripts/gates.py` gate and cannot be one**, because that list has to pass on a
fresh checkout where `testdata/` is empty and `hostile-manifest.json` therefore does not exist.
A gate that refuses on a precondition of running is red on every machine that is not running.

**It is a CI step on both legs since 2026-09-02**, which is a different thing and was worth the
one install line it costs. The paragraph here said *"not a gate and cannot be one"* for about
an hour, and the reason it gave — qpdf is on neither runner — was a fact about the runners
rather than about the work: both workflows install it now, `ci_fixtures.py --hostile` builds the
corpus, and the strict run follows. Homebrew on macOS, the project's own pinned release zip on
Windows, which is the asymmetry the prerequisites table at the top of this file already
describes for a person.

Run it by hand as well after anything that touches `sweep.rs`, `verify.rs` or the rewrite —
CI answers on push, and the loop while you are working is the instrument that owns the change.

Last run 2026-09-02 on macOS arm64: **15 fixtures, exit 0**, no collecting route leaking or
dropping, every unreadable file refusing certification, controls leaking 14.

### `encrypted-rewrite-probe`: does a rewrite keep an encrypted document's encryption

`docs/PLAN.md` §5 said for months that letting a reader delete a page from an encrypted
document needed QPDF. It needed `lopdf::Document::encrypt`, which `save.rs`'s `rewrite` now
calls with the state `checked` took off the document after a password load.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example encrypted-rewrite-probe
```

Seven checks over the two encrypted fixtures and the locked case; about a second. **The
verdict comes from `qpdf`, not from `lopdf`** — a reload with the writer's own reader is the
writer agreeing with itself, and here that is worse than usual, because a `lopdf` load
*without* the password parses no objects at all and reports zero pages. The spike this grew
from round-tripped an empty document and printed `[OK]` three times before that was caught, so
every page count here is read back with the password and the encryption comparison is
`qpdf --show-encryption` on the source against the output.

Without `qpdf` installed the three encryption checks `[SKIP]` with that reason and the page
counts still run. The last check is the control and is the one to read first: a rewrite that
dropped the encryption passes both of the others, so the probe scans the written bytes for
`/Encrypt`.

### `redact-gate-probe`: does the redaction gate certify a clean file and refuse a dirty one

`docs/PLAN.md` §6 step 4 is wired into `redact_copy` and `redact_document`, and neither is
reachable from a unit test — they are Tauri commands, and the join between one and
`ocr_gate::run` is the layer `docs/TRAPS.md` records as *a feature can be inert in the
application while three layers of tests pass*. This drives the real function against a real
render service, a real render worker, a real OCR worker and a real engine. The binary is both
workers, the way `ocr-worker-probe` and `pool-bench` are.

```
cargo run --release --manifest-path src-tauri/Cargo.toml --example redact-gate-probe -- \
    testdata/text-base14.pdf
```

**No `--lib`, and not for brevity**: this runs on both platforms, so the default joins
`PDFIUM_SUBDIR` — `bin` on Windows, where `lib` exists, holds the *import* library and binds
to nothing.

**It runs on both CI legs since 2026-08-29, and it had to.** Until then it was a thing a human
ran by hand on a Mac — and it is the only instrument on either platform that drives the OCR
gate end to end through a worker process. The `child_main_if_asked` dispatch it needs was
widened to Windows in `lib.rs` alone, so on Windows this probe scored **5/8**: the child found
no marker, fell through into the *parent's* argument parser and exited, and every region came
back `the engine crashed`. Nothing in that sentence is about the engine. The step uses the
debug artifact the `bins` gate already built — **33.6 s on macOS against 1 s for the release
build**, which is the debug PDFium render and is the price of not building twice.

It is deliberately **not** a `scripts/gates.py` gate: the gate list has to pass on a fresh
checkout, where `testdata/` is empty because the fixtures are generated and gitignored. This
needs a real document, so it belongs after the step that writes one.

| fixture | result |
|---|---|
| `columns`, `text-base14`, `text-marked`, `rotated`, `links`, `text-cid`, `outline-simple` | 8/8 |
| `encodings` | 0/0, 1 skipped — one text object is every word on the page, so no control survives |

**Re-measured on macOS 27.0 (26A428), 2026-09-27: the seven fixtures are 8/8 each and
`encodings` is 0/0 with its one skip, with a cold Vision cache** (`~/Library/Caches/redact-gate-probe`
removed first; the first fixture run pays the ~23 s model compile, later ones take about
0.4 s). Before that day's fix `text-base14` was 5/8 there, the three rows that need the engine
to read, with `Vision refused the image ... __objc2.missingError` for every region; with the
worker's warm-up line removed and the cache cold it is 5/8 again. With a *warm* cache and no
warm-up it is 8/8, which is why a machine that had once run Vision unsandboxed under the same
executable name could not show the defect. `docs/TRAPS.md`, *On macOS 27 the OCR worker's
Vision refuses every image, and the probes that read a whole page still pass*.

**`columns` ran 0/0 until 2026-08-27 and it is the fixture that matters most.** Its longest
word is `alpha`, five characters, and the target filter was six — so the one corpus that
puts a *second* text object on the region's own rows was the one this skipped. Every other
fixture draws a line as a single text object, so redacting a word in it took the whole line
and there was no neighbour left to misread. (Since 2026-10-07 a word is cut out of its line
and the neighbours on that line stay, so every fixture now has them; the numbers in this
section were measured before that and have not been measured again.) Lowering the floor to five moves no other corpus,
because the choice is the longest word on the page.

Removing the `ocr_gate::mask_columns` call turns `columns.pdf` red on two checks — *the
redacted file is certified* and *a word beside the region on its own rows is not reported*
— and no other corpus on any. That is the control for the mask, and it is the only fixture
where the right rule and the wrong rule disagree.

**The control is the same gate run against the file that was not redacted.** A gate that
certifies everything passes *the redacted file has no reasons* perfectly, so that row on its
own is worth nothing; the source file, with the same regions and the same words, has to come
back **legible** and has to quote the word that is still there. One variable between the two
runs — which file — and it is the one under test.

The other three rows: a page the gate knows no words for must be *not verified* rather than
clean, since a page nothing was read on is also a page nothing survived on; the region's own
pixels must differ either side of the write; and on a platform with no engine the gate must
say so **once**, not once per region.

**That pixel row was a byte scan first and it was the wrong instrument.** `verify::scan` for
the removed words goes red on `text-marked.pdf`, where the same line appears four times and one
copy is an annotation the removal is right to keep — see the trap of that name. A gate about
a region is checked with an instrument about a region.

**Costs, measured on this machine at scale 2.** The gate renders strips rather than pages, and
these are why:

| what | cost |
|---|---|
| `OcrWorker::spawn`, once per save | 1.5 ms |
| one 1190 x 128 probe image through Vision | ~9 ms |
| a whole A4 page through Vision, for comparison | 195 ms |
| a whole page render, warm | 13--48 ms |
| the probe's end-to-end gate run, one region, open included | 200--260 ms |

### `latency-bench`: what one tile costs, decomposed

The last thing `worker-bench` measured that nothing else did. It is a **spike, not a port**:
`worker-bench` carries its own POSIX worker, `dup2` handover, socket pair and SBPL bisection and
cannot run off unix, so this drives the **production** `Worker` instead — which means it runs on
both platforms. **Measured on Windows 2026-07-30 and on macOS 2026-07-31**, and the point of it
being portable is that macOS can cross-check it against `worker-bench --mode latency`, an
implementation it shares no worker code with. That cross-check has now run, and it is the most
useful thing this harness has produced — see below.

```
cargo build --release --example latency-bench
./src-tauri/target/release/examples/latency-bench.exe testdata/text-base14.pdf
./src-tauri/target/release/examples/latency-bench.exe testdata/vector-heavy.pdf
```

Four variants, interleaved within each round, round 0 discarded as warm-up and printed anyway:
`inproc` (no boundary), `raw` (`Tile { png: false }`), `png` (`Tile { png: true }`), and
`control` (`Outline`, a round trip carrying no tile). Each row decomposes into render, encode,
parent fold and transport; every pixel-bearing variant folds its whole payload in the parent so
none can look cheap by never reading what it received. It ends with a `N/M checks passed`
summary and exits non-zero on a failure, so a scripted run can see one; a `[WARN]` does not fail
the run, because every warning here says a *derived* figure is untrustworthy rather than that the
measurement broke.

**There is no `pipe` row, and that is a finding.** `worker-bench` compares pixels down the pipe
against pixels through shared memory. Production never does the first — `Response` documents
that payloads travel through the mapping and never inline — so a pipe row would measure a route
no tile takes. The same quantity is recovered by differencing `raw` against `png`, two paths that
are both real.

Measured 2026-07-31, Windows, 1024² tile at scale 1:

| fixture | boundary cost | spread over rounds | round trip, no tile | per 100 KB moved |
|---|---|---|---|---|
| `text-base14.pdf` | 0.269 ms | 0.004 ms | 0.040 ms | 0.0055 ms |
| `outline-simple.pdf` | 0.309 ms | 0.016 ms | 0.070 ms | 0.0069 ms |
| `vector-heavy.pdf` | 0.294 ms | 0.150 ms | 0.052 ms | `[SKIP]` — see below |

And on macOS, 2026-07-31, same tile and scale, three interleaved passes per fixture rather than
one (the fixtures were run round-robin, not in blocks, because wall clock on these Macs drifts
several percent over minutes):

| fixture | boundary cost, 3 passes | within-run spread | round trip, no tile | inproc residual |
|---|---|---|---|---|
| `text-base14.pdf` | 0.103 / 0.079 / 0.071 ms | 0.048 / 0.024 / 0.002 ms | 0.012 ms | 0.001 ms |
| `outline-simple.pdf` | 0.100 / 0.085 / 0.079 ms | 0.003 / 0.007 / 0.010 ms | 0.022 ms | 0.001 ms |
| `vector-heavy.pdf` | 0.150 / 0.200 / 0.194 ms | 0.125 / 0.145 / 0.142 ms | 0.019 ms | 0.002 ms |

Expected shape reproduced exactly: 3/3, 3/3, and 3/4 with 1 skipped on `vector-heavy`, exit 0
throughout, and its `[SKIP]` for payload differencing appears for the documented reason — png
4027 KB against raw's 4096 KB, so the two variants move nearly the same bytes. That is a property
of the document and it held on both platforms.

**Read the invariance, not the numbers.** The boundary cost is a property of the boundary, so it
should not depend on the document, and across three fixtures that differ by three orders of
magnitude in render time it lands within 0.02 ms of itself. That agreement is the result; any one
of those figures alone would be a single sample.

**The invariance is looser on macOS, and the looseness is confined to one fixture.** The two
light fixtures agree tightly across six runs — 0.071 to 0.103 ms, overlapping completely — while
`vector-heavy` sits clear of both at 0.150 to 0.200 ms. Absolute spread across fixtures is
0.137 ms here against 0.040 ms on Windows. Before reading that as a defect, note that
`vector-heavy`'s *own* within-run spread is 0.125--0.145 ms, i.e. as large as its offset from the
others: it is the one fixture where the estimator is near the edge of what it can resolve, which
is exactly what the spread column exists to say. The check is `spread < boundary`, and there it
passes at 0.73--0.83 of its limit against Windows' 0.51 — so a macOS run of `vector-heavy` is
the plausible place for this to go red first. Three passes did not. Worth knowing rather than
worth acting on.

The absolutes are **~3.5x lower than Windows**, not the 1.5--1.8x the other render constants
differ by. A latency budget written from the Windows figures is conservative on a Mac by more
than the usual factor.

**The cross-check against `worker-bench --mode latency`.** Both harnesses were run on this
machine in one session, and both figures below use the *same* estimator — the tile variant's
transport column minus `inproc`'s, `inproc` being the variant that renders but crosses nothing:

| | `worker-bench` (private POSIX worker) | `latency-bench` (production `Worker`) |
|---|---|---|
| `text-base14` | 0.006, 0.008 ms | 0.071--0.103 ms |
| `outline-simple` | 0.008 ms | 0.079--0.100 ms |
| `vector-heavy` | **-0.087 ms** | 0.150--0.200 ms |
| in-process residual | 0.013--0.037 ms, and **46.7 ms** on `vector-heavy` | 0.001--0.002 ms |

Two conclusions, and the second is why the cross-check was worth doing:

- **The production worker's per-tile boundary cost is roughly 10x the prototype's** — ~0.08 ms
  against ~0.007 ms, non-overlapping across nine runs. Both are far below anything that matters
  (the same tile costs 3.0 ms to hand to the webview), so nothing architectural moves, but the
  production protocol is not free the way the spike suggested.
- **`worker-bench`'s latency mode cannot resolve its own answer**, and now says so. Its
  `transport` is a residual and it baselines on `ping`, which never renders, so the render-noise
  floor stays in the figure; on `vector-heavy` the residual is 46.7 ms against a printed 46.6 ms
  and the `inproc`-baselined value goes *negative*. It now prints the residual and the
  `inproc`-baselined figure beside the two `ping`-baselined ones and warns when the error is as
  large as the answer — which is on **every fixture measured so far**. Read its two headline
  transport figures as upper bounds. Trap: *"A baseline that skips the expensive step leaves its
  noise in the answer"*.

**No sandbox font substitution on macOS.** The handover flagged this as the second reason a Mac
run was worth more than a re-run, since a sandboxed PDFium has previously substituted fonts
silently while still returning `ok`. It did not happen: `inproc` and the worker agree on render
time to within 0.25% on all three fixtures (0.130 vs 0.133 ms, 0.523 vs 0.507 ms, 1670.5 vs
1666.3 ms).

Its four mutations were re-proved here rather than taken on trust — **4/4 caught**, control
green on all three fixtures first, file restored by bytes and verified by digest against `HEAD`.
Two are caught as `[WARN]` rather than `[FAIL]`, which is by design and worth knowing before
writing a harness around it: a parser that treats `passed = total - skipped` as the failure count
reports those two as broken runs. `passed = checks - failures - skipped - warnings`.

Two things the A0 fixture forced, both of which the small ones hid, and both now traps:

- **The boundary figure is differenced on the transport column, not on end-to-end.** The obvious
  estimator subtracts two ~2.7 s numbers to recover a ~0.3 ms one and reports render noise: it
  read **-265.822 ms** there. The run reports how far the same render varies between variants
  beside the figure, which is the error that estimator would have carried — on the A0 sheet a
  factor of several hundred.
- **Payload differencing is guarded by materiality, not by ordering.** A dense vector page barely
  compresses — png 4027 KB against raw 4096 — so `raw > png` passes on a 68 KB gap and divides
  noise by it. That fixture now reports `[SKIP]` naming both sizes.

The `control` variant subtracts the outline walk the reply reports in `walk_ms`, rather than
warning that the walk is inside the number. Whether that subtraction is sound is cross-checked
two ways — the entry count and the walk time must agree about whether any work happened — and
both disagreement branches were shown to fire under mutation. They exist because the first
version trusted the count alone, misparsed an object as an array, and printed *"the document has
no outline"* for `outline-simple.pdf`.

**The boundary check is on reproducibility, not on sign, and the difference was forced by a
mutation that survived.** The first version simply required the figure to be positive — a
boundary cannot be free — and restoring the wall-based estimator on the A0 fixture *passed* it,
because -265.822 ms had been one sample of a noisy quantity and the next run of the same broken
arithmetic landed positive. A check that fires only when noise falls one way is decoration on
every run where it does not. It now requires the figure to be positive **and** to repeat across
rounds, which the two estimators differ in enormously: 0.004--0.150 ms of spread on the sound
one, 48 ms on the broken one.

That fix exposed a second defect worth more than the first. The check compared a spread against
a figure that was computed by a *different route*, so the mutation moved the figure and left the
spread sound, and the comparison passed on an estimator that had been broken on purpose. Both
now come from one per-round vector. **Two derivations of one quantity have to be tied together
or their agreement means nothing** — which is the same lesson as the outline count above,
arriving from the other direction.

**Its checks are proved, not assumed.** Four mutations, four caught, each with the predicted
verdict, restored by bytes and verified by digest: forcing the outline count to zero, forcing it
non-zero, dropping the fold term from the transport formula, and estimating the boundary on
end-to-end again.

### Printing on Windows, and how to check it without paper

`print_win.rs` reads a job back with `Windows.Data.Pdf` — the OS's own PDF stack, the PDFKit
counterpart — then has PDFium draw each page in a sandboxed worker started over the job, and
copies the pixels onto a printer DC. Windows has no in-box PDF print API, so rasterising is what
every Windows PDF viewer does; the output is **raster at 300 dpi** where macOS is vector. The
worker is this executable started again, so the probe answers the worker marker before anything
else, as the CLI does.

**In the app, tpdf's print preview is the print dialog** (since 2026-10-20). `printers_win.rs`
lists the printers and changes the public fields of the chosen one's `DEVMODE` (paper,
orientation, copies, two-sided, colour) through `DocumentProperties`, which keeps the driver's
private settings and corrects what the driver cannot do; *Printer settings...* hands the same
record to the driver's own dialog. `print_layout.rs` lays the job out on the printable area that
`GetDeviceCaps` reports for those settings, and `print_win::spool_laid_out` draws that layout,
rendering only the part of each page that lands on the paper. `PrintDlgW` is left only behind
`print_document`, which the Windows app no longer reaches. Copies past the driver's own `DC_COPIES` are
made by sending the pages again. What is tested without paper: the layout
(`cargo test --lib print_layout`), the cut of a page to the paper (`print_win`), and every
printer on the machine opened, described and turned (`cargo test --lib printers_win -- --nocapture`
prints one line per printer). What is not: whether a driver acts on `dmDuplex`, `dmColor` and
`dmCopies` on paper, and the driver's dialog itself.

The preview's other choices are all decided before the spooler: pages turned to fit
(`Layout::rotate`, drawn by PDFium a quarter turn anticlockwise, `turns: 3`), posters
(`Scale::Poster`, one page over up to `MOST_POSTER_SHEETS` sheets overlapping by
`POSTER_OVERLAP`), and comments left out, which is PDFium rendering without `FPDF_ANNOT` while
`FPDF_FFLDraw` still draws the form fields (`TileRequest::annotations`). Odd, even, reversed and
current pages are only the page list the window sends. **Stop** (`print_cancel`) sets a flag
`spool_planned` reads before each sheet and each band of one; the open document's guard then
calls `AbortDoc`. The settings kept between prints are in the window's `localStorage`
(`tpdf.print-settings`, `printpreview.ts`).

`present` opens a modal dialog, so that last step is the one thing no automatic check can
reach — and since 2026-08-23 there is something behind it worth stating rather than leaving
implied. The panel's **Pages** field was disabled until then (`nMinPage == nMaxPage`, both zero
by default) and now offers a range that `print::sheets` and `spool` honour. The arithmetic is
tested on every platform and the probe proves the spooler sends the named sheets; **whether the
field is enabled, and whether `PD_PAGENUMS` and `nFromPage`/`nToPage` come back holding what the
reader typed, is untested by anything here and cannot be.** The first person to print a range on
Windows is the instrument for that half. `nCopies` is a known second gap of the same shape: it
goes in as 1 and is never read back. Everything before it can be, because **"Microsoft Print to PDF" is a real driver with a
real spooler**, and naming an output file in `DOCINFOW.lpszOutput` stops it raising a save
dialog:

```
cargo run --release --example print-probe
cargo run --release --example print-probe -- testdata/rotated.pdf
cargo run --release --example print-probe -- testdata/vector-multi.pdf "Microsoft Print to PDF"
```

**Over ssh, build it in the ssh session and run only the binary in the desktop session.** On
2026-10-17 on the Windows build machine, `cargo run --example print-probe` inside an interactive scheduled task
failed in tpdf's build script at `resource path ..\vendor\pdfium\bin\pdfium.dll doesn't exist`,
with the file there; the same build from the ssh session passed, and
`target\release\examples\print-probe.exe` then ran in the task (9/9 and 10/10). Why the task's
build cannot see the file is not known.

Fifteen checks, and these are the ones worth understanding:

- **Ink, not the page count.** A wrong `BITMAPINFO`, a DC in the wrong mapping mode and a bad
  `StretchDIBits` rectangle all produce the right number of perfectly blank sheets. Proved
  rather than assumed: mutating the blit away leaves *"the printed output has the pages that
  were sent"* green and only the ink red, at `[0, 0]` — and the output file drops from 598,694
  bytes to 1,183. The pages that were *sent* are the control, because "both zero" would
  otherwise pass on a completely broken path.
- **Ink extent against a predicted geometry, not an ink ratio.** This is the check that found a
  real defect, and the history is the useful part. It began as printed-ink-over-sent-ink with an
  order-of-magnitude band, which read `0.49` and **passed** while every page was going to paper
  at half physical size (a DIB rendered at 300 dpi placed unit-for-unit onto a 600 dpi printer
  DC). The same formula then failed at `0.01` on an A0 page for no reason but the paper being
  16× smaller in area. What holds for both is predicting where the ink should land — the source
  page's ink extent scaled by the page-to-sheet ratio — which reports 1% error on the reference
  run and **48%** against the reverted bug. See the trap of the same name.
- **Its own module table**, which is where the boundary claim gets its honest caveat: 80 modules
  mapped, none named pdfium, and `Windows.Data.Pdf.dll` printed beside it as what *is* mapped.
  Printing parses in the app process on both platforms; what the boundary buys is that the
  parser doing it is not ours.
- **A page range spools the sheet it names, not the first one.** Added 2026-08-23 with the range
  itself. A count of one sheet is equally satisfied by the *first* page, which is exactly what a
  loop ignoring its range produces, so the second check compares the printed ink's extent
  against the prediction for the sheet that was asked for. It asks for the **last** sheet for
  the same reason. On a fixture whose first and last sheets measure alike it prints a `[SKIP]`
  saying so, rather than passing on a comparison that cannot fail; `rotated.pdf` is a fixture
  where they differ.
- **Four to a sheet is read by quarter, not by sheet count.** The ink in each quarter of the
  printed sheet must match the pages laid there, and the quarters with no page must be white:
  `[1802, 2840, 0, 0]` for two pages. A layout that drew every page over the first cell would
  print one inked sheet too.
- **A turned page against the turned geometry, not against its ink's direction.** The first
  version expected every sheet's ink to run tall on portrait A4 and failed on `rotated.pdf`,
  whose first page is landscape after the job's quarter turn while its text block runs tall: the
  page turned correctly and its ink came out wide. The check now swaps the source page's spans
  and holds the print to `predict`, which an unturned page misses by about 60% across.
- **Marks left out, pixel by pixel, with the form as the control.** Comments are about a
  twentieth of `comments.pdf`'s ink, so a ratio of totals read 0.945 with every comment gone.
  The check counts the pixels only the sheet with marks has (1451) and the ones only the other
  has (0). `form.pdf`'s field value has no appearance stream and only `FPDF_FFLDraw` draws it,
  so it must survive unchanged: 0 pixels gone.
- **Stop on a live job.** Another thread presses Stop every millisecond while the print runs,
  since a print clears a Stop pressed before it began; the print must end as stopped and the
  printer must write no file, which is the guard's `AbortDoc` and not an `EndDoc`.

Reference run, `testdata/rotated.pdf`, pages 1--2 with a quarter turn (the first ten lines; the
five laid-out checks follow them):

```
  pages    : 4 in the source, printing [1, 2] with a quarter turn

[OK]   the job we built is readable by the OS parser        2 pages
[OK]   control: the pages we sent have ink on them          non-white pixels per page: [5012, 5012]
[OK]   the spooler accepted every page                      2 pages spooled
[OK]   the printer produced a file                          598694 bytes
[OK]   the printed output has the pages that were sent      2 pages
[OK]   every printed page has ink ...                       non-white pixels per page: [4777, 6850]
[OK]   printed ink lands where the page geometry says ...   got 0.27x0.28, 0.45x0.27,
                                                            predicted 0.27x0.27, 0.45x0.27
                                                            (worst axis off by 1%, 0%)
       ... printed/sent ink, for information                [0.953, 1.367]
[OK]   the printing process never mapped our PDF parser     80 modules mapped, none named pdfium
       ... the OS PDF component it maps instead             Windows.Data.Pdf.dll
```

`outline-hostile.pdf` is the second shape worth running — A4-sized pages rather than small ones,
so the predicted extent is 0.72x0.47 instead of 0.27x0.27 and a wrong *fit* would show where a
wrong *scale* does not. Also 8/8.

**`vector-multi.pdf` is not a quick check.** One A0 page of 200,000 vector operations takes
PDFium about 25 s to draw for an A4 sheet, so the probe's run, with its own checking renders and
the range job, took 117 s on 2026-10-20. Before that day `Windows.Data.Pdf` drew the pages and
one sheet took **2m51s** — see the trap *"the OS's PDF rasteriser is not fast"* and
`docs/VERIFICATION.md`, *How long Windows takes to rasterise a page for printing*.

Beyond the probe, `cargo test --lib print::` runs **18** checks on Windows where it ran 14, because
three of the four third-parser checks are no longer macOS-only. The fourth asserts the page count
and prints a `[SKIP]`: it needs per-page *text* to say which pages survived, and
`Windows.Data.Pdf` has no text API at all. `a_third_parser_checks_a_job_built_from_a_document_we_-`
`did_not_write` covers that property on both platforms instead, using per-page rotation —
`rotated.pdf` carries 0/90/180/270, so keeping the wrong two pages is a different rotation pair.

### Linux packages: the deb and the AppImage

```
npx tauri build --bundles deb,appimage
```

`APPIMAGE_EXTRACT_AND_RUN=1` in the environment where FUSE is missing, as under WSL. Both
packages put the two binaries in `usr/bin` (`tpdf`, the window, and `tpdf-cli`, the tool) and
the resources in `usr/lib/tpdf`: `pdfium/libpdfium.so`, `tessdata/` and the notices. That is
Tauri's Linux resource folder, `../lib/tpdf` from the executable, and `cli::linux_resources`
applies the same rule for the tool; before 2026-10-14 the installed `tpdf-cli` looked in
`/usr/bin` and could read no document. The desktop entry is `src-tauri/linux/tpdf.desktop`.

The deb depends on `libtesseract5` and `p11-kit-modules` as well as WebKitGTK and GTK, because
tpdf opens both at run time, where `dpkg-shlibdeps` cannot see them. **The AppImage carries
no Tesseract**, for the same reason: on a system without `libtesseract5` its OCR refuses with
a sentence. Everything else in it is self-contained.

Checked on 2026-10-14 under WSL (Ubuntu 24.04), with `vendor/` out of the way of both:
the installed `tpdf-cli` answered `info` and `text`, and `render`, `images --paper a4` and
`ocr` turned the text page into a scan and back into 31 recognised words, through the deb and
through the AppImage's own `tpdf-cli`. Each window, started on a document under Xvfb, ran its
render workers with `Seccomp: 2` and `libpdfium` mapped, and the application process with
neither. The first GTK application in a fresh WSL session waited 25 s in
`g_application_register` while D-Bus started `xdg-desktop-portal`; the next started in
0.12 s. That is the session, not tpdf, and a desktop session has the portal running.

`release.yml` builds both on a third leg, `Linux (x64)`, and attaches them to the draft
(decided 2026-10-14), so a draft now holds 8 assets where it held 6. They stay out of
`latest.json`: that leg takes no updater overlay and has `includeUpdaterJson` off, and an
installed Linux copy does not check for updates (`update.ts`, `inApp`). The leg checks the
deb's Depends and its `Exec=tpdf %F`, and `check_packaged_api.py` runs the Python client's
suite against the tool extracted from each package, with the missing-engine control.
`publish_release.py` refuses a draft unless all three legs passed that check.

### Printing on Linux, and how to check it without paper

`print_linux.rs` shows GTK's print dialog and sends the job's PDF to CUPS through a
`GtkPrintJob`, unchanged; CUPS converts it for the printer. Before the dialog the job is
read back by PDFium in a worker (`save::InWorker::printed_pages`) and its pages counted. The
dialog is not driven by anything here; `print_linux::print_to` reaches the same `submit` as
its Print button, and `print-probe` uses it with cups-pdf, a CUPS queue that writes what it
is sent to `~/PDF`. On the test machine only (cups-pdf is GPL and is not part of tpdf):

```
sudo apt-get install cups printer-driver-cups-pdf
xvfb-run -a cargo run --release --example print-probe
xvfb-run -a cargo run --release --example print-probe -- testdata/rotated.pdf PDF
```

Run it from the repository root, where `testdata/` is. Eight checks: the read-back worker's
page count, a control that the pages sent have ink, GTK sending the job, the job's file
removed once GTK let go of it, the pages that came out of CUPS counted and their ink
measured, and a second job with the dialog's range set to the last page, which must come out
as one page with that page's words and none of page 1's. The last check needs pages whose
words differ and is skipped otherwise, so run it on `testdata/links.pdf` as well:

```
xvfb-run -a cargo run --release --example print-probe -- testdata/links.pdf PDF
```

On 2026-10-14 under WSL (Ubuntu 24.04): 7 of 7 with the last skipped on `vector-multi.pdf`,
8 of 8 on `links.pdf` and `rotated.pdf`, and each of six mutations turned its own check red
(a read-back one page high, the file kept, `gtk_printer_accepts_pdf` inverted, `/dev/null`
sent in place of the job, the range not handed to CUPS, a one-page range sent as page 1).

The dialog itself was driven by hand the same day, under Xvfb with `xdotool`, from the
installed deb: Ctrl+P opens it over the window with cups-pdf's queue listed; *Pages* `2-3`
printed 2 of 12 pages and `2-` printed 11, on A4 as the queue's default; *Print to File* with
a range was refused in a message. That run found the range ignored: GTK keeps a range on the
job, where only `GtkPrintOperation`'s drawing reads it, so before the fix `2-3` printed all 12
(`print_linux.rs`). Not checked: paper.

### Explorer thumbnails and the preview pane, on Windows

*Show PDFs in Explorer with tpdf* registers `tpdf_shell.dll` (`src-tauri/shellext/`) for the
current user (`explorer.rs`, `docs/THREAT-MODEL.md` §T6.47). The DLL is not a default member of
the Cargo workspace, so no gate and no plain `tauri build` builds it; an installer that carries
it is built with the overlay that builds it first and bundles it beside `tpdf.exe`:

```
npm run tauri build -- --config src-tauri/tauri.explorer.conf.json --bundles nsis
cargo build --locked --release --manifest-path src-tauri/Cargo.toml -p tpdf-shell --example explorer_probe
```

`release.yml` passes the same overlay on its Windows leg and checks the DLL's signature with
the other three files. Then, on a Windows machine where tpdf is not installed, in the
logged-on session (it opens an Explorer window and reads the screen; over ssh, through an
`/IT` scheduled task as for `installed_check.py`):

```powershell
python scripts\explorer_check.py <path>\tpdf_<version>_x64-setup.exe `
    src-tauri\target\release\examples\explorer_probe.exe --shots <folder>
```

It installs into a folder of its own, turns the integration on with `tpdf-cli explorer --add
--replace`, has Windows' thumbnail cache extract a thumbnail of each hostile fixture, a document
that renders, random bytes and a locked document (`explorer_probe thumbs`, forced extraction,
so the provider runs in `dllhost.exe` as for a window), hosts the preview handler in
`prevhost.exe` and turns a page (`explorer_probe preview`), reads which of `dllhost.exe`,
`prevhost.exe` and `explorer.exe` have `tpdf_shell.dll` and `pdfium.dll` mapped, opens an
Explorer window on the corpus in large icons and times its answers for 15 s, turns the
integration off and compares the user's classes with what they were, and uninstalls with it
on. `--control no-add` and `--control no-remove` break one step each and exit 0 exactly when
the checks aimed at it failed.

On 2026-10-16 on the Windows build machine (Windows 11, build 26200): 20 of 20. All 15 hostile fixtures and
the document got drawn thumbnails, random bytes and the password-protected document got
Explorer's generic icon, each within 0.25 s; the provider ran in `dllhost.exe` and not in
`explorer.exe`, and no `dllhost.exe`, `prevhost.exe` or `explorer.exe` had `pdfium.dll` mapped;
the preview pane showed page 1 (63% of the pane white) and page 2 after Page Down; Explorer
answered all 75 messages at once; the user's classes and preview list read as before after
`--remove`, and the uninstaller, run with the integration on, left no key naming the handlers.
`no-add` failed its 7 checks and `no-remove` its 2, and nothing else. The comparison leaves out
`Software\Classes\Local Settings`, where the Explorer window the check opens records its view.
Over ssh, send a file longer than about 24 KB as base64 on standard input: on the command line
it passes Windows' 32,767-character limit, and the copy fails with only "exec request failed".
The receiving end matters as much (2026-10-16): in the Windows build machine's PowerShell 5.1 a 140 KB file
read through `$input` and joined arrived with its line breaks turned into spaces, and
`[Console]::In.ReadToEnd()` never returned. What kept the bytes, with no base64 at all, is a
one-line Python receiver, `open(sys.argv[1], 'wb').write(sys.stdin.buffer.read())`, saved on
the machine and run as `python recv.py <path>`, then the SHA-256 compared on both sides.

### The "reopen its windows" dialog

A development build is killed constantly — harness timeouts, the deliberate crash probes,
an aborted panic — and macOS answers each abnormal exit by offering, on the *next* launch,
*"the last time you opened tpdf, it unexpectedly quit while reopening windows. Do you want
to try to reopen its windows again?"* That dialog **blocks the launch** until someone clicks
it, in front of a run that has nothing to do with whatever produced it.

`src-tauri/Info.plist` sets `NSQuitAlwaysKeepsWindows` to false, which is merged into the
bundle by tauri-bundler — check it with
`plutil -p .../tpdf.app/Contents/Info.plist | grep Quit` after a build rather than assuming
the merge happened. An app that saves no window state cannot be asked to restore it, and
the observable is the mechanism rather than the symptom: hard-kill a running bundle and
`~/Library/Saved Application State/com.timostein.tpdf.savedState` must not appear.

This is also the right *product* behaviour, not only a developer convenience. tpdf reopens
the document you were reading, on the page you were on, through its own session file
(`session.rs`); Cocoa's restoration would be a second mechanism doing the same job, and two
mechanisms agree until they do not.

An existing machine that has already been prompted also wants the user-domain switch, since
the plist only governs bundles built after it:

```
defaults write com.timostein.tpdf ApplePersistenceIgnoreState -bool true
defaults write com.timostein.tpdf NSQuitAlwaysKeepsWindows -bool false
rm -rf ~/Library/"Saved Application State"/com.timostein.tpdf.savedState
```

### Checking the viewer

The reading surface is asserted rather than eyeballed. This opens a document in a real
webview, dispatches real wheel and key events at it, and checks fit-width, fit-page, actual
size, scrolling, End and Home, the zoom ladder, a pinch, resize, text selection and copy,
find-in-document, the
command palette, the screen-reader text layer, the outline sidebar, the page-thumbnail
strip, page inversion, and that the frame loop idles when there is nothing to do:

```
scripts/viewer_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

It is **not** a `gates.py` gate: it needs a built bundle and a generated fixture, neither of
which a gate run has. Run it before a release, and after any change to `viewer.ts`,
`scroller.ts` or the tile protocol.

**One corpus while you are working; the sweep before a push.** The rule above names *files*,
and a file is the wrong unit: `viewer.ts` is 4,400 lines and most changes to it cannot vary
by document. What `viewer_sweep.py` buys over a single run is the name-set invariant across
fourteen corpora, so the question to ask is whether the change could make a check appear,
vanish or skip on *some* documents — layout, rotation, text extraction, the tile protocol,
anything reading a page's size. A change whose checks drive the DOM and the callbacks
directly cannot, and the sweep is then fourteen runs of the same answer. This is the
portfolio rule about running the owning gate while iterating and the whole suite once, at the
push, applied to the slowest instrument here.

**It requires a bundle, not merely a release build.** A raw `cargo build` binary opens a
window and never executes a line of JavaScript — WKWebView needs the bundle identity, and
the failure is silent: no error, no crash report, a blank window. Build one with
`npm run tauri build -- --bundles app` and run the executable inside it, which keeps stdout
and the environment that `open -a` does not. The *profile* genuinely does not matter — the
check asserts behaviour rather than timing it — so a debug bundle is only slower.

**On a zero exit it now asks whether the run happened**, which it could not until 2026-08-26.
Every guard in it was aimed at a run that failed, and a run that did *nothing* exits 0: on
Windows, single instance makes a second launch forward its argv to a window already open and
exit immediately, so an empty transcript came back as a pass, with the wrapper's own
containment `[OK]` as the only check-shaped line in it. A blank-window bundle failure produces
the same silence, so the paragraph above is no longer describing a failure with no report. The
observable is the `CHECK-NAMES-JSON` roll `checkreport.ts` prints before its summary, and the
refusal names no single cause — it prints the exit code, the byte count and the number of
`[FAIL]` lines, and lists the four things that look identical from here. Both callers already
guarded themselves (`viewer_sweep.py` on the same roll, `mutate_viewer.py` on the summary),
which is why the layer they share had nothing; neither is made dead by this, since the sweep
looks for the roll before it looks at the exit code.

That guard is a pure function of the transcript, so it can be proved without a screen, a
bundle or a document — six cases, one of them the acceptance, since a reader that refuses
everything passes all five refusals:

```
scripts/viewer_check.py --self-test
```

The same command runs the cases of the look beside the scratch path that the validation-data
phase depends on (*Adding validation data, as far as a refusal*, below).

It also requires an unlocked screen, for the reason `scroll_bench.py` does: WebKit suspends
a page whose window is not visible, so behind a lock screen the check does not fail, it
stops. Both scripts share that guard (`scripts/webview_guard.py`).

**On a timeout it says which silence it hit**, which it could not until 2026-08-01. A page
WebKit has suspended and a page stuck in a loop both present as no output and a live process,
and they want opposite responses — one is an occluded window or a screen that locked
*mid-run*, the other a defect in whatever was last changed. `webview_guard.diagnose_silence`
samples the app's CPU **time** twice, two seconds apart, before the kill: suspended uses none,
waiting uses a little, spinning uses a core. The delta is load-bearing — a single
`ps -o %cpu` is a lifetime average on macOS, so a page that worked hard and then got suspended
reads as busy. `mutate_viewer.py` gets the line for free, since it already forwards
`[FAIL]` stderr lines into its own broken-run verdict.

`session_check.py` and `open_check.py` do **not** have it: they use `subprocess.run`, so the
process is already dead when the timeout is raised, and giving them the diagnosis means
converting four call sites to `Popen`. Worth knowing rather than assuming it is everywhere.

**It does not take focus.** The window appears and has to stay visible, but it will not raise
itself over what you are doing, so the run can sit in the background while you work.
`scroll_bench.py` is the exception and calls `set_focus()` on purpose — an unfocused window
is throttled, and a frame-rate benchmark would then be measuring the throttle.

**"Visible" is stricter than "unlocked", and the guard does not check it.** A window fully
covered by another — a full-screen terminal, a different Space — is *occluded*, and
WebKit suspends the page exactly as it does behind a lock screen. The run then produces no
output, uses no CPU, and stays alive, which reads as a hang in whatever was last changed.
`viewer_sweep.py` and `mutate_viewer.py` take **`--raise`**, and it is **off by default**
as of 2026-08-20. They used to force `TPDF_RAISE=1` on every launch, which on a fourteen-
corpus sweep takes the keyboard away fourteen times in a row — reported from the machine
as *"these tests are locking up this mac, as every window opens in foreground"*, and it
was never what the checks need. `lib.rs` says so at the call site and keeps a polite
default for exactly this reason: the checks drive behaviour rather than time it, so an
unfocused window costs them nothing. What costs them everything is an **occluded** window,
which is a different property — and one `webview_guard.py` already detects and names the
remedy for. Use `--raise` when a run produces nothing, not before.

Set `TPDF_RAISE=1` to raise the window when there is nowhere visible to put one:

```
TPDF_RAISE=1 scripts/viewer_check.py <binary> testdata/text-heavy.pdf
```

**An empty transcript file mid-run means nothing, and reading it as a stall is a mistake this
page invited.** `viewer_check.py` collects the app's output with `communicate()` and prints the
whole transcript when the process exits, so a redirected run shows **zero bytes** from the
first second to the last — on `vector-multi` that is minutes. The "results print as they are
produced" sentence below is about `viewercheck.ts` writing into the pipe, which is what makes a
*timeout's* partial transcript useful; it is not a promise that a live run's log grows. The
liveness signal is CPU time (`ps -o time= -p <pid>`), which is what `diagnose_silence` samples:
a page that never ran accumulates none, and a slow render accumulates seconds.

The watchdog identifies **any** page that never executed, whatever the reason — an
occluded window and a raw unbundled binary produce exactly the same silence. Every spike
entry point starts by asking Rust for its path, which records a `webview alive` mark; a run
that times out without one is told in full that the page never asked for its first setting, and the two causes of
that: a covered window, and a page that threw while it was being built (2026-10-13; the
message named only the first until then).
Confirm independently with `TPDF_STARTUP=<file> <binary>`, which fails the same way in 30 s
and settles "environmental or mine" in one command. Results otherwise print as they are
produced, so a run that stops partway names the last check it completed.

**That was false under a redirect until 2026-07-30, which is how these are always run.** Python
block-buffers stdout the moment it is not a tty, so `open_check.py > out.txt` held **zero bytes**
for a twelve-minute run — indistinguishable from a script that died at import, and exactly the
ambiguity printing-as-you-go exists to remove. `scripts/live_output.py` makes the three harnesses
line-buffered explicitly; A/B'd at the same four-second mark, 0 bytes against 38. Prefer that over
`python -u`, which is a property of the invocation that every future caller has to remember.

Two of its assertions carry the weight, and both tie a position to specific content rather
than checking that something happened. For **selection**, text dragged near the top of the
page must come from earlier in the page's text than text dragged further down — a substring
check was tried first and cannot fail, since a selection is a contiguous range of indices
whatever the boxes claim. For **search**, a match's index range must cover the characters
searched for, re-extracted independently; every other search assertion passes just as well
when the indices are off by one.

Run them with **`scripts/viewer_sweep.py <app-exe>`**, which is the list of corpora as well as
the way to run them:

```bash
scripts/viewer_sweep.py --list          # every window corpus, and every fixture excluded, with reasons
scripts/viewer_sweep.py src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
```

> **Not yet re-measured, and the shortfall has grown.** The properties dialog added five
> names to every corpus on 2026-08-21, and the comments panel's covered-words face added a
> sixth later the same day — so the totals in the table are **six** short of what the next
> run will print, and the invariant the sweep asserts is the *agreement* between corpora
> rather than any particular number. Written down rather than left to be noticed, because the
> table looks measured either way and a stale total reads exactly like a current one.
>
> The current figure, measured 2026-08-23: **342 names**, all distinct — 324 cutting
> `26.8.7`, plus five for the overlay-against-the-file phase, five for the stamp (its own
> overlay reading and its four commands), six for cropping by dragging (three backend
> checks, its one palette command, and two reading the scrim off the overlay), and two for
> the eraser taking a mark whole (the wash the nib crossed, and the wash beside it that is
> the control). Take the names from the harness's own
> `CHECK-NAMES-JSON` line, never by splitting the printed columns — this page records that a
> `\s{2,}` split matched 175 of 189 lines, and reaching for it again is what produced a diff
> full of per-corpus *detail* differences that looked like missing names.

#### The overlay against the file, and the one thing it needs from outside

Five of those names are a phase comparing what the overlay draws with what the *saved file*
renders — the one comparison nothing made, since `viewer_check.py` measured the overlay
against the model's numbers and `annot-probe` measured the file against the same numbers. It
makes nine marks, reads the overlay, saves a copy, opens it, renders the same page and reads
that; the file's ink is isolated by diffing that render against one taken before any mark was
made, so page content cancels and the classifier knows nothing about the colour it is about to
compare. `docs/PLAN.md` has the design and what it measured.

**It needs a writable path, which the webview has none of.** `viewer_check.py` makes one under
the system temp directory, binds it to `TPDF_VIEWERCHECK_SCRATCH`, and removes it at exit; the
`viewercheck_scratch` command hands it to the page. A run that gets nothing there skips all
five with that reason rather than passing. Running the harness by hand without the script is
therefore a run with those five skipped — which is correct, and worth knowing before reading
a hand-run transcript as a full one.

**The comment is excluded from the colour comparison**, with a measured reason: PDFium draws
its own `/Text` icon and ignores the `/C` we write, so blue reads 224 degrees on screen and 60
in the file, and red reads 0 and 60. See the trap of that name — the file is right and the
renderer is not ours.

#### Adding validation data, as far as a refusal

`add_validation_data` is asked for by the running application and never for its result: four
names, each a refusal from a different branch, because one refusal could come from a backend
that refuses everything. A document with no signature (a worker's reading), the output named
as the original and a timestamp authority over `ftp:` (both judged in the command, before a
worker is asked), and a signer this computer does not trust. Each is matched by a fragment of
the backend's sentence; nothing reads the sentences out of Rust, as nothing does for the print
refusals beside them.

**The fourth needs a signed document, and `--signed` supplies it.** `window_checks.py` passes
`testdata/incr-signed.pdf`, whose signer issued its own certificate. `viewer_check.py` puts a
copy beside the scratch path, and then requires that name to be recorded `OK`: without the
flag it skips, which is what the sweep's corpora print, and with it a skip is a failure. The
timestamp authority in every call is `http://127.0.0.1:9/`, where nobody listens. No call
names an outside host.

**Whether a refusal wrote anything is looked at from outside**, since a page cannot see a
file. After a green run `viewer_check.py` lists what is beside the scratch path by its stem,
not by the names the phase uses, fails on anything but the signed copy, and holds that copy
against the fixture byte for byte. `scripts/viewer_check.py --self-test` proves that look on a
directory of its own, the accepting cases first.

**What it does not reach is the copy written.** That needs a signer the operating system
trusts, and no check adds a root to a key store; what a worker builds and reads back is
`commands::validation`'s tests. Nor does it reach `App.svelte`'s own call: the arguments here
are typed against the same `ipc.ts` entry and sent by this harness.

Measured 2026-10-10 on macOS arm64, a checks build on a second Mac through
`scripts/run_on_mac.py --only viewer`, `text-heavy.pdf`: 337/337, 51 not applicable, the four
names `OK`. Three hand mutations, each built and run the same way from a commit the working
tree never held:

| Mutation | What went red |
|---|---|
| `add_validation_data` left out of `generate_handler!` (the entry in `mutate_viewer.py`) | all four names, *Command add_validation_data not found*; 333/337 |
| the output written before the worker is asked (`std::fs::write(&path, …)` after `refuse_same_file`) | no name: 337/337, and the wrapper's *refused, and left a file behind*, naming both outputs |
| `vouched_chain` answering for any standing | the fourth name alone, 336/337 |

The third is worth reading for what it printed: *the signer's certificate (tpdf spike 0.6
test signer) does not say where its revocation data is published*. The fixture's certificate
names no address, so a build that skipped the trust rule asks nobody either, and the closed
port is never reached. This check therefore shows the refusal is the trust rule's and not
that a request was withheld; the second is `longterm::existing`'s tests', whose authorities
record what they were asked.

Every run reports the same check names; what differs is how many are `[SKIP]` with a reason,
and a name that goes missing rather than skipping is the bug this arrangement exists to catch.
**The script asserts that**, as a set difference across the corpora, rather than leaving it to
whoever compares two totals — a check that stopped being printed and a check that started
skipping are the same number. It also prints the table below, so those numbers are measured
rather than transcribed.

> **The list is a gate (`corpora`) because it went wrong the moment it had no home.** On
> 2026-08-16 it lived in a hand-typed shell loop and `links-rotated.pdf` went into a sweep,
> producing eight red checks and three chased diagnoses, none of them a defect — against the
> paragraph on this page that already says that fixture is separate *because* it reddens two of
> these rotation checks. Every `testdata/*.pdf` is now either a window corpus with a stated
> purpose or excluded with a stated reason, and a fixture matching neither fails the gate.

**The sweep runs on Windows as of 2026-08-19, and did not before.** Two things had to be
fixed, and both are recorded in `docs/TRAPS.md`. It shelled out to `pkill` unconditionally,
which is not a program here: `check=False` swallows a non-zero exit and not a
`FileNotFoundError`, so it died on its first corpus with a traceback and **exit 0**. And
`subprocess.run(text=True)` decodes with the locale codec, which is cp1252 on this machine
— six corpora in, `multilingual.pdf` produced a byte it refuses, the decode raised inside
subprocess's own reader thread, and the failure arrived as a `TypeError` between a `None`
and a string with no mention of an encoding.

Two further things are worth knowing before running it here. A stray `tauri dev` makes
`tauri-plugin-single-instance` forward the launch and exit, so the check reports one line
about the module scan and nothing else — kill leftovers first, which the sweep now does.
And `text-heavy.pdf` is a real document rather than a generated fixture, so a machine that
does not have it cannot run the full sweep at all; `--only` over the other thirteen is the
honest substitute, and the sweep says which corpora it is missing rather than skipping them
quietly.

**`vector-multi` is timing-variable on this machine, and a single red run there is not a
finding.** Measured 2026-08-19 across four runs of the same corpus: **351 s, 496 s, 386 s**
on one build and **384 s** on another — a 41% spread on identical code. Two of those runs
failed, and they failed **different checks** (`the page already rendered is not rendered
twice`, reporting 11 borrows against 4 draws; and `covers the first screen`, timing out at
sharp=0.0%). Both are checks whose observable is a race between the thumbnail strip and the
viewer, on the only fixture where a thumbnail is slow enough for that race to be real —
which is precisely what the corpus is *for*.

So: **re-run it before treating a red vector-multi as a regression, and check whether the
same check fails twice.** Two different checks failing is variance; one check failing
repeatedly is a defect. The control that settled it was a `git worktree` at `HEAD` built and
run against the same fixture — cheap, and it leaves the working tree untouched, which
`git stash` does not.

Measured here on 2026-08-19: thirteen corpora, **276 check names each** (277 once
`app.about` became a driven probe), diffed as sets by the sweep rather than inferred from
the totals agreeing, and **no failing check on any of them**. 629 s in total, of which `vector-multi` is 341 s and `vector-heavy` 145 s; every
other corpus is under 40 s. The ran/skipped splits are the sweep's own output and differ
from the macOS table above only by the checks added since it was taken.

**Every row below was measured on macOS on 2026-08-17**, and printed by the script rather than
transcribed — the table is the sweep's own output, pasted. Zero failures
anywhere, and **all fourteen corpora report the same 234 check names** — diffed as sets by
the sweep, not inferred from the totals agreeing.

The link work took the total from 171 to 189, turning a page in the document took it to 204,
and deleting one took it to 218: ten in the viewer, three against the backend, and one command
probe. Of the ten, the one that carries the weight is about **identity** — the slot below the
gap must now hold the page that was under it, compared by its text — because a page count one
lower is equally true of a viewer that dropped the wrong page. On a corpus whose pages read
alike that check says so and skips rather than passing on a comparison that cannot fail.

**Moving one took it to 229**: nine checks and two command probes. The nine are where the
deletion ten do not transfer, and the reason is worth stating because it is what made the
frontend defect this phase found invisible. **Every deletion check is built on the page count,
and a move does not change it.** So the length is asserted to be *exactly what it was*, and
every statement that can fail is about identity: the moved page's text is in the slot it was
moved to, the page displaced by it is one slot lower rather than gone, the reader is still
looking at the page they were reading, and both pages keep the sizes they were measured at —
that last one on `mixed.pdf`, the only corpus where two pages have different sizes, so on every
other one it says so and skips.

**That size check asserted one slot until the mutation aimed at it survived**, which is what
the harness is for. The defect it was written against is a scroller re-indexing its learned
sizes by position, and one comparison catches that. The other way to lose a size is to lose
them all, and then every page falls back to one estimate — free to land within tolerance of
whichever single shape is being compared, which on this corpus it did. Asserting both slots
makes it arithmetic instead: a shared estimate would have to be within 0.02 of two shapes the
check's own precondition has just established are further apart than that. The same mutation
reddened a *deletion* check the whole time, which reads absolute boxes rather than shapes —
so the coverage existed and the check named for it was the one that could not fail.

> **The table above is one sweep of the tree as committed**, re-run after the last change to
> any check rather than carried over from before it. That is not free — this one was **728 s**,
> of which `vector-multi` alone is 349 and `vector-heavy` 205, so two of the fourteen are
> three quarters of the run; `scripts/viewer_sweep.py` prints that breakdown at the end so the
> question does not have to be answered by hand again — and it is worth it here for a reason this increment
> demonstrated: the run before it went red on four of the fourteen corpora, against ten that
> passed for no better reason than having strips too short to scroll. A table pasted from a
> sweep of a different tree would have been a claim about neither.

**Dragging one took it to 231**, and the two names are the narrowest pair that were worth
paying a window for. The slot arithmetic is two pure functions with unit tests, the gesture's
state machine has nine mutations against a fake DOM, and the edit a drop runs is already
covered by the three backend names below. What none of those can answer is whether a real
WKWebView captures the pointer, keeps delivering moves after it has left the row, and lays out
geometry the gap arithmetic can read — so the strip's handler here *records* rather than
edits, and the document is never touched. The control is the half that found something: its
first version could not fail either way it was read, and the mutation aimed at it survived
until both clauses were replaced. See the trap of that name.

**Extract took it to 232**, and one name is all it is worth: `file.extractPages runs from the
palette`, driven with a real argument. The arithmetic is 30 unit tests over `parsePageRange`
and `namePages`, the subset plan is ten Rust tests, and the command is four more — none of
which can say whether a typed value survives the palette's own input and arrives as the slots
the action is handed, which is the only thing the window is asked. The argument is `1-2`
rather than `1` on purpose: a single slot reads the same whether the parser produced one page
or dropped one, and every window corpus has at least two pages.

**The right-click menu took it to 234**, and both names are about the same gesture because the
report that asked for them was: right-clicking a page offered the web view's own menu, whose
one entry reloads the frontend. So one name asserts that menu is *suppressed* — read off
`defaultPrevented` on a real `contextmenu` event rather than inferred from a screenshot — and
the other that the slot under the pointer is the one reported. The menu's own behaviour is 18
unit tests over the real class in a fake DOM; what only a window can say is whether a
right-click on a row arrives at all. Three ways of posting a secondary click from *outside* the
process were tried first and every one of them failed silently; `docs/TRAPS.md` has them.

Putting the order back must restore the document exactly, which
is the check a viewer that reordered its own view but not its model passes and a viewer that
lost a page fails. Two of the eleven drive `page_move` for real, including the refusal of a
page moved behind itself, and one is undo.

The three that ask the backend are the `page_delete` round trip: the command is registered,
names a page by identity, refuses a second deletion of that id as *deleted* rather than as
unknown, and undo puts the page back. They leave the model as they found it, asserted rather
than assumed, because every phase after them reads the document.

Of the page-turn ten, three are negative — a page nobody turned keeps its proportions and its
upright text, and `viewer.rotation` does not move — because every positive statement about a
turned page is equally true of a view that rotated everything. The tenth is the half turn, and
it exists because a mutation deleting the invalidation that runs *before* the geometry survived
all nine others: a quarter turn changes the page box, so `applySizes` invalidates it either
way, and only 180 degrees leaves the box identical.

The Windows column is *not* carried forward: it was measured at 163 names on 2026-08-02 and
nothing has re-run there since, so it is absent rather than adjusted — which is what this
page has twice recorded arithmetic in a measurement column costing.

**Two rows are new**, and both earned their place the day they were added. `links.pdf` caught a
destination landing on the page before the one it named — the only corpus that could, being
the only fixture in the tree with a `/Fit` entry. `links-cropped.pdf` caught two checks whose
control could not be established on a document with a single link, because the check before
them follows it.

**Re-run 2026-08-24 on Windows**, with `inherited.pdf` promoted from an exclusion to a corpus:
**343** names, no failing check anywhere, 731 s. `text-heavy.pdf` is not on this machine —
no script writes it, it is a real document supplied by hand — so the run covered **14 of the
15** and its row below is the earlier macOS reading, marked as such. The sweep refuses a
missing fixture outright rather than skipping it, which is why the run had to name the other
fourteen.

| fixture | ran | skipped | what it is there for |
|---|---|---|---|
| `text-heavy.pdf` | 292 | 50 | *(2026-08-23, macOS — not on the machine that ran the rest)* the dense case, and search across 775 pages |
| `outline-simple.pdf` | 299 | 44 | the only fixture with an ordinary outline |
| `outline-hostile.pdf` | 299 | 44 | the only one with a `/Launch` entry to refuse |
| `vector-heavy.pdf` | 198 | 145 | one page, no extractable text, and no white paper to invert |
| `vector-multi.pdf` | 238 | 105 | twelve A0 pages: the only one where a thumbnail is slow enough to collide with the viewer |
| `rotated-90.pdf` | 278 | 65 | every page at `/Rotate 90`, which nothing else in the corpus has |
| `columns.pdf` | 288 | 55 | the only one whose content-stream order is not its reading order |
| `tagged.pdf` | 263 | 80 | the only one carrying a `/StructTreeRoot`, and the only two-page one |
| `multilingual.pdf` | 280 | 63 | the only one whose text is not Latin: CJK with no word separators, Arabic right-to-left, a decomposed accent, and a code point above the BMP |
| `encodings.pdf` | 281 | 62 | the only one whose character mappings are absent, broken or predefined — and the only fixture that reaches the replacement-character path at all |
| `mixed.pdf` | 282 | 61 | the only one whose pages are not all the same size, and the only one that exercises the three layout checks at all |
| `comments.pdf` | 306 | 37 | the only one carrying annotations: notes, a reply, a highlight, three text-string encodings, an indirect `/Annots` array and 1,200 marks on one page — the only corpus where all eight comment checks run |
| `links.pdf` | 308 | 35 | the only one with link annotations, and the only one whose outline is deliberately not in page order — which is what let it catch a destination landing on the page before the one it named |
| `links-cropped.pdf` | 245 | 98 | the only one whose `/CropBox` is not its `/MediaBox`, so a rectangle placed in media space lands visibly wrong |
| `inherited.pdf` | 272 | 71 | the only one whose pages take their `/MediaBox` from an ancestor while carrying a quarter turn, and the shortest displayed page in the corpus at 400 points |

**The 2026-08-23 sweep read 342 names on macOS, and the difference is not one number.** The
one new name is `file.mergeDocuments runs from the palette`, checked against the run's own
name list rather than inferred from the total. Every row's ran/skipped split also moved by one
or two, and **that is not attributed here**: this run is a different platform *and* four
commits later, so a per-row difference between the two is two variables at once — the trap of
that name. The invariant the sweep asserts is none of these totals, it is that all fourteen
agree on the *names*, and both runs satisfy it.

The 2026-08-23 run for the record: **342** names on fourteen (the same list with `inherited`
excluded), 721 s, no failing check anywhere, and `vector-multi` and `vector-heavy` 73% of the
time between them. Five of those names are the overlay-against-the-file phase, six are
cropping by dragging, and two are the eraser taking a mark whole.

**Re-run the same day** after the eraser and the crop drag: **329 -> 342**, every corpus
gaining thirteen *runs* and losing none. Two sweeps in one day are worth one note — the
totals move whenever a check is added, so a row here is a statement about the run that
produced it, and the invariant the sweep asserts is that all fourteen agree.

**Re-run 2026-08-18** with the crop: **267** names on all fourteen, seven more than the 260
below. Every corpus gained seven *runs* and lost none.
All seven are one backend phase, driving `page_content_box`, `page_geometry` and `page_crop`
against the real backend — which is the only place that can say the three commands are
*registered*, the failure every layer below passes through. Its last check is the control: a
crop whose corners are the wrong way round has to come back as the refusal the model names.

The two palette commands are deliberately **not** driven from the palette, and the reason is
in `viewercheck.ts` beside them: cropping to content is two IPC replies deep — measure the
ink, then ask what size the page becomes — and the probe framework's settle is a frame-loop
wait rather than a reply wait. Their wiring is covered by `appcommands.test.ts`'s sweep over
every registered command.

**Re-run 2026-08-18** with the keyboard route to a mark: **260** names on all fourteen,
six more than the 254 below, and the rows above are that sweep's. Every corpus gained six
*runs* and lost none — the four for the walk and the guard need no selection and no corpus
feature, since the marks are two the harness hands the viewer, and so do the two commands in
the sweep every registered command gets. The two command probes are the only pair in that
table with no `unless`: no fixture carries a mark, because these are the reader's own.

Four of the six are in a real webview for a reason a unit test cannot cover. The guard that
stops a key typed into a note from moving the page is about a key **bubbling** from the field
to the root handler; vitest dispatches at the root with a target of its own choosing, which is
a statement about the handler rather than about the tree it is installed in. Its control is in
the same check — the same key pressed on the page must still scroll — because a guard
tested only on its refusal is satisfied by a viewer that ignores everything.

**Re-measured 2026-08-18** with the note on a mark, on all fourteen corpora: every one
reports the same **254** names, and the rows above are that sweep's, pasted from what
`viewer_sweep.py` prints. Fourteen names on top of the 237 the morning's highlight work
left: **nine** for the note box, **four** for the three mark commands driven against the real
backend, and **one** for `edit.removeMark` in the sweep every registered command gets.

**Re-run 2026-08-18** with underline and strike out: **254** names on all fourteen, three
more than the 251 below. Two are the new commands in the sweep that every registered command
gets — aimed separately, carrying the kind in the expectation, since one action taking a
parameter is where a copy-and-paste gives a reader a Strike out that highlights. The third
drives each kind through `annot_mark` against the real backend and reads the kind back off
the state reply. Every corpus gained three *runs*: they need no selection and no corpus
feature, because the mark is one the harness hands the viewer.

**Re-run earlier the same day** after the page-turn placement fix, and every one of the
twenty-eight numbers then came back **byte-identical**, diffed rather than eyeballed. That is the honest
result and it is worth stating rather than quietly re-pasting the same table: the defect it
fixed — a comment or a link on a page an edit had turned, drawn in one place and found in
another — needs a page turn and an annotation *at the same time*, and no fixture in this
corpus has both. The window harness could not have caught it and still cannot. What it does
cover is the primitive underneath: a mutation that turns every rectangle a quarter too far
reddens three of the mark phase's checks, so the one implementation those three subsystems
now share is reached from here. The measurement that found the defect is a differential in
`viewerturns.test.ts`; see `docs/PLAN.md`.

**Every corpus gained runs and no skips**, which is the difference from the highlight
increment and is deliberate. Those checks needed a selection, so the two fixtures with no
extractable text skipped them; these are driven against a mark the harness hands the viewer
itself. The model is tested in `docmodel.rs`, the file in `annot-probe`, and this phase tests
what neither can reach — a rectangle on screen, a press landing on it, and the box that
opens — for which a synthetic rectangle is the right input and runs everywhere.

That earlier sweep is worth keeping for the shape of its failure: the first version of the
quad checks left them out of the no-text path entirely, so two corpora reported 235 names
against everything else's 237 — a name that had *vanished* rather than one that skipped,
which is exactly what the identical-name-sets rule is for.

**`tagged.pdf` runs three of these thirteen and skips ten**, which is the split worth knowing:
the ten that drive the viewer need a middle page to delete and it has two, while the three that
ask the backend need only a page to spare. They are the only checks of this phase that run
there, and skipping them along with everything else would have been a skip for a reason that is
not theirs.

**`vector-multi` is the one with no margin, and the sweep's timeout was raised for it.** It
takes **325 s** of the default 900 (420 until 2026-08-17, which the page-deletion phase went
past before that phase was cut from two delete-and-restore cycles to one). Every other corpus
is minutes. A tight timeout is worse than a generous one here: it fails as *"the run printed no
CHECK-NAMES-JSON line"*, which reads as a crash rather than as a bound.

**A `pkill -f "tpdf.app/Contents/MacOS/tpdf"` goes between runs**, and
`scripts/viewer_sweep.py` does it. A leftover window occludes the next one, WebKit suspends an
occluded page, and the run then produces nothing and uses no CPU — twice, before that went
into the sweep script. `TPDF_RAISE=1` covers the other half, a window with nowhere visible to
go, and the script sets it.

**`text-heavy.pdf` moved by one between two sweeps an hour apart** — 177/41 and 176/42, the
same 218 names — which is the race the notes below describe, not a regression. It is left as
the second reading rather than the flattering one. Those are the totals of that day, when the
name set was 218; the row in the table is a later sweep against a larger set, so read the
*one* it moved by, not the absolute figures.

**Every row above is one run, and the notes below say why that is a point estimate rather than
a bound.** Two of these fixtures have a check that lands on either side of a race, so their
split moves between runs while the name total does not. The table is not re-ranged here
because a range needs several runs per fixture and this sweep was one each — read a
disagreement of one as the documented variation, and a disagreement in the *name total* as the
bug this arrangement exists to catch.

The `text-heavy.pdf` row was `142 / 21` and marked derived until 2026-08-03, when running it
on the only machine that has the document made it `143 / 20`. The derivation was one check
out, which is the cost of carrying arithmetic in a column of measurements: it looks exactly
like the rows either side of it.

**Then the measurement replaced it with a point, and the true value is a range** — two runs
the same evening against the release bundle both reported `142 / 21`. Nothing regressed: the
one check that moves is the withdrawal race described below, and this row and
`outline-simple`'s are the two the note there says land on both sides. So the correction
above fixed the arithmetic and reintroduced the shape of the original error, which is worth
saying plainly: **a number measured once is still a point estimate of a quantity that varies**,
and for these two fixtures the invariant is the name total, not the split.

**`outline-simple`'s range was widened to `147--149 / 14--16` on 2026-08-08**, on six Windows
runs: three at `149 / 14`, then two at `148 / 15` and one at `149 / 14`. The total is 163 in
all six, which is the invariant the paragraph above says it is — and the widening is the
same lesson a third time, since `147--148` was itself two platforms' point estimates read as
a bound. Do not narrow it back on the strength of one green run. The three runs that came
first also carried the stale-focus-mirror failure described in `docs/TRAPS.md`, so treat any
row of this table as a statement about the split, never about whether the run passed.

**The two A0 corpora straddle the watchdog's old 300 s default, and their spread is far wider
than the bound.** Four runs on macOS on 2026-08-03: `vector-multi.pdf` at **275 s**, **387 s**
and once still going past **600 s**, and `vector-heavy.pdf` at **249 s** having been killed at
300 s on the run before. So the bound was a coin flip rather than a consistent failure, which
is why it survived — a corpus that fails every time gets fixed, and one that fails half the
time gets re-run. Do not read any single figure here as the cost; the spread is the
measurement, and it is roughly 2.2x on one document. Both are the fit-page setup on an A0
page, which is the operation
that defeats spatial culling: the whole page becomes visible, and PDFium charges its large
fixed cost per render call. The two bounds are one number now — `viewer_check.py` derives
`TPDF_VIEWERCHECK_TIMEOUT` from its own `--timeout` — so raising the one people edit raises
the one that decides. Before that they disagreed, and the app's was the tighter.

**Every measured row is its previous split with exactly three more skips**, which is a
stronger statement than a matching total: the three layout checks added on 2026-08-02 skip
on every fixture but `mixed.pdf`, for the stated reason that the fixture has no geometry
sidecar, and nothing else moved. `mixed.pdf` is the eleventh row and the only one where they
run.

The earlier note that eight of nine rows were the macOS split plus arithmetic still holds
underneath this one, and so does the exception: `multilingual.pdf`'s generator picks a font
per page from what the machine has, so the Windows fixture is a different document and one
check that skipped on macOS for want of text has text to work on here. A ran/skip split is a
property of the document, and that corpus is the only one whose document is not the same on
both platforms.

**Every measured row above is green as of 2026-08-02, and getting there took two rules rather
than one.** The check involved is `a page reads in the order its generator laid it out`, which
compares each page against what the generator *wrote*. It is downstream of `text.rs`,
`reading.ts` and the fixture's machine-local fonts and of nothing in the layout —
`readingChecks` builds its own `TextCache` and never touches the viewer or the scroller — so
it is the check that sees a font substitution, and both failures below were one.

**`multilingual.pdf` was red for a missing space, and that is fixed.** The folding page came
back `cafélatte` where the manifest says `café latte`. PDFium's extraction *does* contain the
space — `text-probe --mode order` shows `café`, a space run, then `latte` — so it was
dropped between extraction and the line's *ranges*. Measured through `FPDFText_GetCharBox`
against the vendored library: the space at index 4 comes back **placed**, 0.02 pt tall at
y 752.00--752.02, while every letter on the line sits at 752.14--766.08, the two bands missing
each other by 0.12 pt. `reading.ts` refuses a box that thin and re-attaches it by preceding
index. The page reads `café latte` here as of the run above.

**Fixing it broke `encodings.pdf`, and only running the whole corpus found that.** The rule as
it first landed was absolute — under `SLIVER_PT`, a tenth of a point — which is a claim
about glyphs and turns out to be a claim about *metrics*. Page 2 of `encodings.pdf` is set in a
predefined CMap with no embedded font, so PDFium has no metrics for it and reports **every**
character 0.018 pt tall. All of them were refused, nothing was placed, and the page came back
as a single fragment: its two lines, 632 pt apart, read as one. Established by reverting
`reading.ts` alone and rebuilding, which gives exactly complementary results:

| | `multilingual` | `encodings` |
|---|---|---|
| absolute rule absent | **129/130** `cafélatte` | 130/130 |
| absolute rule present | 130/130 `café latte` | **129/130** `日本語の符号\r\n日本語の符号` |

**The rule is a conjunction now**: `height < SLIVER_PT && height < SLIVER_OF_LINE * typical`,
where `typical` is the median height of the page's placed characters and `SLIVER_OF_LINE` is a
twentieth. The two measured samples are three orders of magnitude apart on the relative
quantity and adjacent on the absolute one — 0.02 pt against 13.94 pt letters is 0.0014 of
them, 0.018 pt against a page median of 0.018 is 1.0 of it — and `tagged.pdf`'s comma, at a
third of its letters, is well clear of both and stays `SHORT_MARK`'s business. Each half was
proved by a mutation turning exactly one test red; the median was proved against a maximum,
which survived the whole suite until a control was written for it. See the traps for the full
account.

**What this cost, and the discipline that paid for it:** the fix was verified on the corpus it
was written for and on macOS, where all eleven were green because that machine's substitute
font has real metrics. Nothing in either run could see the regression. It surfaced only from
re-running every corpus and diffing the name sets, which is what the standing instruction above
asks for and the reason it is worth its wall-clock.

**The two-page one is worth having for a reason unrelated to tags.** Adding it turned three
checks red that had been green on every corpus for a week — two nav probes guarded on "more
than one page" where the guard has to be "a page that can be reached", and a search check that
cannot tell "the scan restarted" from "there was nothing ahead to find". None was a defect in
the subject; all three were preconditions written as assertions, and the smallest multi-page
fixture until then had three pages. See the traps.

**The multilingual corpus paid for itself before it was green.** It found four things, and only
the first is in the viewer: a search picker written as `/[A-Za-z]{5,}/` matched nothing on a
Japanese page, so **seventeen** search checks skipped while printing *"page 1 has no extractable
text"* about a page with forty-nine characters on it — the checks did not run and the reason
printed was false. Twelve of them run now, on the same binary. The drag check had no precondition
for *"there is text where I dragged"* and reported a sparse page as a defect. In the backend,
`FPDFText_GetUnicode` turned out to be a UTF-16 API, so a code point above the BMP arrived as two
lone surrogates and was unfindable; and a combining accent on a word with no ascender opened a
line of its own. See the traps for each.

Its own harness is **`examples/search-probe`**, which is where the search claims live: 60/60 with
9 not applicable, against a manifest a different program wrote. Run it directly, since it needs no
webview:

```sh
cargo run --release --example search-probe -- --file ../testdata/multilingual.pdf
```

Twenty-one queries, and the manifest labels each count as **stated** (from what the generator
wrote), **measured** (a property of PDFium this corpus established — that the Alphabetic and
Arabic Presentation Forms come back normalised) or **decided** (a product decision). Conflating
the three is how a measurement comes to read as a specification, so a change to a `decided` count
has to be argued for rather than absorbed — and one of them has since been argued for and
changed: the fold case-folds rather than lowercasing since 2026-08-01, so `strasse` finds `Straße`
and its count went from 1 to 2. The `decided` prose records both the old answer and the new one.

**`encodings.pdf` is the other half of the multilingual work**, and a separate corpus because
the subject is different: those pages are correct documents in other scripts, and these are
documents whose own statement of what their bytes mean is missing or wrong. Three pages, and
it found a product defect on the first run.

| page | what it is | what it established |
|---|---|---|
| `no-mapping` | Identity-H, **no `/ToUnicode`** | PDFium does not fail — it returns eighteen characters of plausible garbage for eighteen drawn. The page is *not* textless, so nothing tells a reader that a search of it means nothing |
| `broken-map` | a `/ToUnicode` with lone surrogates | the only fixture reaching `text.rs`'s replacement path. Two of its broken entries also **pair into one astral character**, which nobody predicted |
| `predefined` | `/UniJIS-UCS2-H`, **non-embedded** KozMinPro | extracts correctly, so the `chromium/7881` build has the bundled Adobe-Japan1 CMaps. A fact about the pin, to re-establish if it moves |

```sh
cargo run --release --example search-probe -- --file ../testdata/encodings.pdf
```

23/23 with 7 not applicable. Six of its seven queries are `measured` rather than `stated`: what
a broken document extracts as is a property of PDFium, and writing it as a fact about the file
is how a measurement comes to read as a specification.

**The defect it found is in the regex path.** A pattern was compiled case-sensitively against a
haystack the fold had already lowercased, so with match-case off **any uppercase letter in a
pattern matched nothing at all**. It survived because `compile`'s own doc comment asserted the
invariant it was breaking, and because `viewer_check.py` builds its pattern from a word taken
from the page — so on every corpus with ordinary prose the pattern was lowercase and the two
sides agreed by accident. This corpus's garbage happens to be uppercase.

**`--mode scan` is the timing arm, and it is a different question from the manifest checks
above.** It walks a whole document for a query three ways, round by round and interleaved:
`extract` is what a scan did before 26.9.2 (a fresh `text::extract` and a freshly compiled
query per page), `cached` is what it does now (`OpenDocument::page_codes` and one
`search::Prepared` for the walk), and `recompile` is the cache without the compile-once, so
the two effects can be told apart. The three arms' hit counts are compared as well as their
times — a faster answer that is a different answer is not a measurement — and every round
is printed, because interleaving controls for drift between the arms and not for a machine
that is slow for both.

```sh
cargo run --release --example search-probe -- \
    --mode scan --file ../testdata/text-heavy.pdf --query the --rounds 5
```

The first round is reported apart from the rest, because on the `cached` arm it is the round
that fills the cache and therefore the one round that still measures the old cost.

**Compare the name *sets*, not the counts, and slice the name by column.** Every label is
exactly six characters — `[OK]  `, `[FAIL]`, `[SKIP]` — so the name begins at column 7
whatever the outcome, and consuming the label with a regex `\s` eats one space for `[OK]`
and none for `[SKIP]`. An ad-hoc comparison written that way reported five corpora
disagreeing when the only difference was which checks had skipped. The name column is padded
to 40 and a longer name is followed by a single space, so there is no reliable split at all:
key on the first 41 characters after the label, which are byte-identical for the same check
on any document. Two corpora legitimately differ in the *order* they record two checks;
compare sets.

**Diff the names mechanically, and not with a naive split.** `record` pads each name to 40
characters and then prints the detail, so a name *longer* than that is followed by a single
space and any pattern keyed on "two or more spaces" swallows the whole line — the padded-column
trap, walked into again on 2026-07-30 while checking this very invariant. The label is seven
characters wide and the padded name forty, so a fixed slice is exactly right:

```
grep -E "^\[(OK|FAIL|SKIP)\]" run.log | cut -c8-47 | sort > names.txt
```

**That `8` is a fact about this harness, not about the repository.** `backend-probe` and
`worker-probe` built their label by interpolating `OK`/`FAIL` into `[{}]`, so their passing
rows began at column 6 and their skipped rows at column 8 — and the recipe above, applied
there, sliced the `[OK]` rows two characters short and reported *"the name sets diverge"*
across three corpora that were in fact identical. Both now pad the label to seven like
everything else, so one recipe reads every harness; before copying it to a new one, check the
widths (`grep -hoE "^\[[A-Z]+\] *" run.log | awk '{print length($0)}' | sort -u` must print a
single value). See the trap of that name.

Six of those, diffed pairwise, is the invariant in one command. It also reports the count,
which must equal the number of unique lines — two checks whose first forty characters
coincide would otherwise merge silently.

**86 until 2026-07-30**, when word and line selection added three, the palette's argument
mode added five, the two find options added three, the results sidebar added four, the fit
modes added six, and reading order added two. The
results four skip together on a document with no extractable text, which is why the two
vector fixtures gained four skips and no runs. The selection three run on every
corpus with extractable text, rotated included — line grouping follows the page's own
reading axis, so there is nothing in them that assumes lines advance downwards — and skip
together on the two vector fixtures, which is why those gained three skips and no runs. The
find-option three are the same shape: they need a word taken from page 1, so they run
wherever search does. One of them skips on a fixture whose needle is already upper case,
there being no spelling of it that matching case would reject — and it says so rather than
passing on nothing.

The reading-order two run only where a manifest exists, which today is `columns.pdf` alone;
everywhere else they skip together, which is why every other corpus gained two skips and no
runs. `columns.pdf` in turn skips two that no other corpus does — the drag-ordering check,
whose premise is false on any multi-column layout, and the rotated-lines check, whose samples
are shorter than it can compare. Both say so.

The fit six run on every corpus but one, and the exception is the informative part:
`rotated-90` skips *"fitting the page shows less of it than fitting the width"*, because its
pages are landscape and already fit the window vertically at fit-width. That check is the
control on the one beside it — without it, "fit page shows the whole page" would be
satisfied there by doing nothing — so it prints the measurement that made it inapplicable
(`495px in 700px`) rather than passing.

**The single values in that table are one sample each**, not a claim that nothing moves. One
check races (see below) and can swing a run by one in either direction; a `78--79` style
range in an earlier revision of this table was that check being honest.

**`vector-multi` takes about 4m40s**, and everything else a fraction of that — twelve A0
pages is what it is for. The default timeout was 300 s, which sat close enough to that to
fail intermittently, and the timeout path *discarded the transcript* — so a slow machine
produced one line, `[FAIL] run timed out`, which is exactly what a page that never ran a
line of JavaScript produces. It now prints how far it got and the bound is 900 s, well
clear of the slowest corpus rather than beside it.

The two vector fixtures skip three of the six inversion checks, and that is the design
working rather than a gap: "the page went dark" cannot be shown on a document with no bright
paper, so it says so instead of passing on nothing.

**The ranges are all one check: "the strip withdraws its work when the viewer needs the
renderer".** A thumbnail on a cheap page takes about a millisecond, so whether one is still
in flight when the viewer asks for a tile is a race, and the check skips when it is not —
correctly, since nothing outstanding reads exactly like a successful withdrawal. Repeated
runs of `text-heavy` and `outline-simple` have each landed on both sides of it. It is
deterministic only on `vector-multi`, which exists for it.

Absolute counts are deliberately not quoted in this paragraph: they move whenever a check is
added, and a stale number here would send someone looking for a regression that is a
changelog entry. The table above is the one place they are written down.

**So the ran/skipped columns are not the invariant** — the **names** are, and how many there
are of them is in the table above rather than in this sentence, which said `109` for two days
after the number stopped being right.

**Measured 2026-08-20 — 279 names, on every corpus, byte-identical as sets:**

| fixture | ran | skipped | failed |
|---|---|---|---|
| `rotated-90` | 227 | 52 | 0 |
| `comments` | 244 | 35 | 0 |

Re-measured the same day at **281 names** after multi-stroke drawing added the
two preview checks: `comments` 246 ran / 35 skipped / 0 failed, 281 names, all
distinct. And again at **284** when the eraser landed: `comments` **249 ran / 35
skipped / 0 failed**, all distinct. The three it added are `edit.erase` in the
command sweep and the two that read the eraser's preview — *"a stroke the
eraser has taken stops being drawn at once"* at 38% of the band before the nib
and 0% after it, and its control *"and one the nib missed is still there"* at
44%. The control is not a formality: an overlay that stopped painting the whole
drawing satisfies the first check perfectly.

**The first of those runs went red, on the check written for exactly it.**
`edit.erase` was registered and unclassified, so *"every registered command is
classified, and every classification is registered"* failed with
`unclassified [edit.erase]` — which is the trap about a command deliberately
left out of the harness still having to be classified, firing on a command that
was not meant to be left out at all. That last clause is now checked by the harness itself — `Report.finish`
fails a run in which two checks share a name, because the roll above is compared
as a **set** and a set cannot see a repeat. It caught a real one within the hour
of being written; see `docs/TRAPS.md`.

Two names were added that day — `edit.draw` in the command sweep and *"a drawing follows
its strokes and does not fill its rectangle"* in the overlay phase — and *"the five kinds do
not all look the same"* was reworded to `six`.

**Measured 2026-08-20 at 310 names**, after the marks panel. Seven are new —
`view.showMarks` in the command sweep, the five that drive the panel, and *"every sidebar
tab fits inside the panel"*. Two of the panel's five are worth naming because they are the
ones no unit test can reach: *"activating a row opens that mark's note and goes to it"*,
which needs a real press on a real element and a document with pages to travel through,
and *"pressing a mark on the page selects its row"*, which is the `onMark` wiring end to
end — the popup reports, the viewer forwards, the panel marks the row, and nothing in
the phase told the panel which mark that was.

**The seventh went red on its first run, on a defect it was written to look for.** Five
labels want 293 px of content in a 260 px sidebar, so **Marks** was clipped by the host's
`overflow:hidden` — present in the DOM, `role="tab"`, and unreachable by a pointer. The
tab *count* check beside it passed throughout, because a clipped button is still a button.
The row wraps now, and the detail line prints every label's `scrollWidth/clientWidth` so a
failure says which one and by how much.

**And the sweep is what found two defects in the panel phase itself**, neither visible on
`comments.pdf`: on `links-cropped`, a one-page document, the phase's two synthetic marks
were at the same height on the same page, so the press meant for the first opened the
second; and on `rotated-90` the check asserted the viewer's page number after activating a
row, which the last page cannot satisfy — a scroll to the end clamps and leaves the page
before it at the top of the viewport. The trap is recorded under that name. It asserts the
mark is *visible* now, which is what "goes to it" means and is what a viewer that opened
the note without scrolling fails.

**One check was red on four corpora and is now fixed.** *"a text box draws its words and
not its rectangle"* failed on `vector-heavy`, `vector-multi`, `rotated-90` and
`links-cropped`; a `git worktree` control at the text-box commit reproduced it, so it
shipped there and was invisible because that increment was verified against `comments.pdf`
alone. **The painter was right on all four** — the predicate's every reading was a
fraction of a rectangle that scaled with the page, while a text box's type is a fixed
11 points, so it failed in both directions at once: on A0 the readings rounded to zero, and
on a 20-pixel-tall box the `edges` sample, which reads the middle tenth of the height,
landed on the second line. It is now two type-sized bands and three border strips measured
in points off the box's own corner, on a fixture rectangle of a fixed 260 x 90 points.
`docs/PLAN.md` has the full account, including why the 90 came from the sampler's
two-pixel floor rather than from the type. Three mutations prove it can fail.

**Measured 2026-08-20, all fourteen corpora, `--raise` off** (superseded by the 2026-08-21
table below, and kept because it is the run the text-box repair was proved against): the same
**310 check names**
on every one, diffed as sets. `text-heavy` 265/45, `outline-simple` 273/37,
`outline-hostile` 273/37, `vector-heavy` 172/138, `vector-multi` 212/98, `rotated-90`
258/52, `columns` 262/48, `tagged` 237/73, `multilingual` 254/56, `encodings` 255/55,
`mixed` 262/48, `comments` 275/35, `links` 282/28, `links-cropped` 217/93. 740 s in total,
of which `vector-multi` is 398 s and `vector-heavy` 164 s. The only failing check anywhere
was the one above. **Re-swept after the repair: all fourteen green** — every ran/skipped
split byte-identical to the run above, the same 310 names, and `no failing checks on any of
14 corpora`. The splits being unchanged is the useful half: repairing a check by making it
skip somewhere would have moved one.

**Measured 2026-08-21, all fourteen corpora, `--raise` off: the same 313 check names on
every one, no failing check anywhere, 686 s in total.** Three names were added that day —
*"a row's remove control asks for that mark and does not open it"* with the marks panel's
remove control, then *"and the words they cover are the words that are selected"* and *"a
mark nothing was typed on is listed by the words it covers"* with the covered-words row.

| corpus | ran | skipped | 2026-08-20 | s |
|---|---|---|---|---|
| `text-heavy` | 268 | 45 | 265/45 | 25 |
| `outline-simple` | 276 | 37 | 273/37 | 10 |
| `outline-hostile` | 276 | 37 | 273/37 | 10 |
| `vector-heavy` | 174 | 139 | 172/138 | 152 |
| `vector-multi` | 214 | 99 | 212/98 | 341 |
| `rotated-90` | 261 | 52 | 258/52 | 9 |
| `columns` | 265 | 48 | 262/48 | 9 |
| `tagged` | 240 | 73 | 237/73 | 38 |
| `multilingual` | 257 | 56 | 254/56 | 39 |
| `encodings` | 258 | 55 | 255/55 | 9 |
| `mixed` | 265 | 48 | 262/48 | 9 |
| `comments` | 278 | 35 | 275/35 | 15 |
| `links` | 285 | 28 | 282/28 | 11 |
| `links-cropped` | 220 | 93 | 217/93 | 8 |

**Re-measured the same day after the comments panel: the same 317 check names on every one,
no failing check anywhere, 680 s.** Four names were added — *"a mark nobody wrote on is
listed by the words it covers"*, *"and they are the words the fixture's generator says are
there"*, *"and those words are really on the page it is on"*, and the control *"a comment
with a body is still listed by what its author wrote"*.

| corpus | ran | skipped | earlier that day | s |
|---|---|---|---|---|
| `text-heavy` | 268 | 49 | 268/45 | 25 |
| `outline-simple` | 276 | 41 | 276/37 | 10 |
| `outline-hostile` | 276 | 41 | 276/37 | 10 |
| `vector-heavy` | 174 | 143 | 174/139 | 149 |
| `vector-multi` | 214 | 103 | 214/99 | 339 |
| `rotated-90` | 261 | 56 | 261/52 | 9 |
| `columns` | 265 | 52 | 265/48 | 9 |
| `tagged` | 240 | 77 | 240/73 | 38 |
| `multilingual` | 257 | 60 | 257/56 | 39 |
| `encodings` | 258 | 59 | 258/55 | 9 |
| `mixed` | 265 | 52 | 265/48 | 9 |
| `comments` | 282 | 35 | 278/35 | 15 |
| `links` | 285 | 32 | 285/28 | 11 |
| `links-cropped` | 220 | 97 | 220/93 | 8 |

**Twelve corpora are `+0` ran and `+4` skipped, and the two carrying annotations are `+4`
ran and `+0` skipped.** That is the whole check on the run: the covered-words checks need a
markup annotation nobody wrote on, so on every fixture without one they must stand down by
name rather than vanish, and `comments` and `links` are exactly the two that have one.

**The first attempt at this run was red, and it is the reason the split above is worth
reading.** `commentChecks` returns early on four paths — the comments could not be read,
the document has none, no comment has a rectangle on the page, the last one has no row —
and the new checks were called after all of them. So twelve corpora neither ran nor skipped
them: `comments` and `links` reported 317 names and everything else reported 313, and each
of those runs passed on its own. A single-corpus run said `282/282 checks passed` and looked
perfect. Only the cross-corpus name-set diff can see a check that is **absent** rather than
failing, which is what `viewer_sweep.py` is for. The names are a module constant with a
`skipCoveredWords(why)` helper now, called at each of the four returns.

**The previous column is there because the way the splits moved is the check on the run.**
Twelve corpora are `+3` ran and `+0` skipped; `vector-heavy` and `vector-multi` are `+2` and
`+1`, and those two are the documents with no text to select — so the check that compares
the selection's words is skipped there, exactly as its sibling *"a mark's rectangles come
from the page's own text"* already was. Three names arriving and every corpus accounting for
all three, in the two patterns its own contents predict, is a stronger statement than
fourteen green lines.

`vector-multi` is 50% of the wall clock and `vector-heavy` 22%, as before. The total fell
from 740 s to 686 s, which is machine noise rather than anything about the harness — these
are single samples, not a benchmark.

Two of the three are unreachable from a unit test, and for different reasons worth keeping
apart. The remove control's is that the fake DOM does not bubble, so `marklist.test.ts`
cannot tell `stopPropagation` from its absence. The covered-words row's is that the fake DOM
**resolves no styles at all**: the words a mark covers and the note a reader typed sit in the
same column, and the only thing separating them is that one is dimmed and italic, so a panel
drawing them alike passes every unit test there is. That check paints a noted row beside a
bare one and reads `getComputedStyle` on both — the noted row being the control, since a
panel calling every line the document's would satisfy half of it.

One check failed once and did not recur: *"a drag selects text from where it was dragged"*
on `outline-hostile`, in one of three sweeps that day. Two runs failing different checks is
variance; the same check twice is a defect — the trap is recorded under that name, and
this was the first shape.

⚠ **The first run of that measurement was against `text-base14`, which is not a window
corpus.** `viewer_sweep.py --list` classifies it as *"a backend-probe fixture: font coverage,
measured through the worker"*, and the sweep was pointed at it anyway — the trap recorded as
*"a probe fixture swept as a corpus, against the file that already said not to"*, walked into
by the person adding checks to the harness. It passed 177/279 with 102 skipped and **the same
279 names**, which is why nothing looked wrong: the name set belongs to the harness, so it is
identical whatever you open, and only the ran/skipped split is a fact about the document. A
split from a non-corpus is meaningless as a table row, and it was written into this table as
one. Take the fixture from `viewer_sweep.py --list`, not from `ls testdata`.

⚠ **The `109` above is of 2026-07-31 and is not the current count.** Between then and now the
harness gained marks, crops, print and the comment panel, and nothing moved that number. It
was left, and it then did exactly what a stale count does: an increment predicted the new
total as `109 + 2 = 111` and was wrong by 168. **Take the count from a run, never from this
file** — the sentence below about the ran/skipped columns not being the invariant is the
same warning, and it did not stop the arithmetic being done anyway. Read the names, and read
`CHECK-NAMES-JSON`, which the harness prints for exactly this purpose:

```sh
python3 -c 'import json,sys;print(len(json.loads([l for l in open(sys.argv[1]) if l.startswith("CHECK-NAMES-JSON")][0][16:])))' run.log
```

A count chased
back to a documented value is a defect introduced to satisfy a document, and the repair here
would be to delete the outstanding-request condition that makes the withdrawal observable at
all. Read a differing count by checking that the name is present and `[SKIP]`; a name that
has *vanished* is the bug this arrangement exists to catch.

This was written as a fixed `65 | 10` first, and a perfectly ordinary run then read as a
regression. **A table that records one sample of a race as an invariant makes the next honest
run look like a defect** — state the range and what varies, or the check that flips gets
"fixed" by someone chasing a number.

**Do not run all six while iterating.** Each run needs an `.app` bundle rebuilt and takes
the better part of a minute, and six transcripts of green is not evidence of anything — the
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
and the defect it found was total rather than subtle — see `docs/PLAN.md`. Its selection
ordering check skips, with the reason: on a page whose lines advance sideways a horizontal
drag crosses all of them, so the comparison is meaningless. What checks that mapping properly
is the probe, per rotation:

```
for page in 0 1 2 3; do
    for view in 0 1 2 3; do
        src-tauri/target/release/examples/text-probe testdata/rotated.pdf \
            --page $page --mode align --view-turns $view
    done
done
src-tauri/target/release/examples/outline-probe testdata/rotated-90.pdf --mode check \
    --manifest testdata/rotated-manifest.json
```

`--mode order` is the third mode and asserts nothing — there is no right answer for it to
check, because the order a page's characters arrive in is a property of whoever produced the
file. It prints them, which is the only way to see from outside the viewer that the file's
order is not the page's:

```
src-tauri/target/release/examples/text-probe testdata/columns.pdf --page 1 --mode order
```

On page 1 of that fixture it prints `alpha one beta one`, `alpha two beta two`, and so on —
two columns merged line by line, which is what `src/lib/reading.ts` exists to undo, and what
the clipboard used to get.

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
anything` — which is the honest answer, and is why they are not written as a pass.

What it does **not** cover: the command list `App.svelte` registers, and the Cmd-K that
opens the palette. The check builds its own registry, so it proves the palette works and
not that the application's commands are wired to it.

### Checking that a mark a reader makes reaches the document

The chain a mark travels — command, gesture on the viewer, callback, edit model,
overlay — had **nothing running over it end to end**, and a reader found the hole on
2026-08-22: a shape drawn on the last page of a document was dropped with no command sent
and no message shown, while all sixteen gates stayed green. Each half asserted its own side
and was right; the join is an object literal in `App.svelte`, which no unit test imports
and which `viewer_check.py` does not reach, because that harness builds its own `Viewer`
with no model behind it.

```
scripts/mark_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/links.pdf
```

It takes the **binary**, not the bundle — there is no Launch Services route here and the
document is handed over in `argv`. Inside a bundle it must still be the one under
`Contents/MacOS/`, because WKWebView needs the bundle identity or the page never runs.
`src/lib/markcheck.ts` holds the checks and the argument for each; the one it exists for is
*"and it is recorded on the page it was pressed on"*, which derives the expected page **id**
from the model's own page list at the viewer's own slot and compares it with the id the
model filed the mark under. Under the shipped defect those differed by one on every
document.

Every assertion reads the **model** — marks that came back over the IPC boundary from
Rust — and never the viewer that produced the gesture, which is what keeps it from being a
writer agreeing with its own reader. The single exception is the ink reading, whose job is
the last hop a model assertion cannot see.

⚠ **The launch half had never run when this was written.** It was written on a machine whose screen was locked,
and `webview_guard` refuses rather than hanging — correctly, since a suspended WebKit page
does not run the check slowly, it does not run it at all. So **this harness is in the state
`docs/TRAPS.md` warns about: one that has never executed produces no failures, and neither
does one that passes.** What *is* proved is the transcript reader, which needs no screen:

```
scripts/mark_check.py --self-test
```

Seven cases, six of them refusals — no summary line, a summary disagreeing with the exit
code, a failing summary, a run that never opened a document, a skipped keystone check, and
a name found by prefix rather than by column. Run the real thing on an unlocked screen
before trusting a green line from it, and prove it can go red the way every other harness
here was proved: reintroduce the slot lookup in `Edits.mark`, rebuild, and confirm the
page-identity check fails.

It ran on 2026-10-09 against the checks build: 11/11 on `links.pdf` and on `text-heavy.pdf`.
Before that day's correction it failed *a mark dragged across the page moves in the model*
on both, and on the sources of 26.10.13: the press was aimed with a rectangle measured from
the viewer's corner and landed above the box (`docs/TRAPS.md`, *A press aimed with a
rectangle measured from the viewer's corner missed its mark once the window grew a
toolbar*). The page-identity mutation above was not run that day.

### Checking session restore

Reopening where the reader left off is a property of a *launch*, so it takes more than one:

```
scripts/session_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf testdata/text-heavy.pdf
```

**It runs unmodified on Windows** (2026-07-30), and did on the first attempt — another
harness this file listed as macOS-shaped that never was. `webview_guard` already returns early
off darwin, and the script takes a binary rather than a bundle, so nothing needed porting:

```
cargo build --release --features tauri/custom-protocol --bin tpdf
python scripts/session_check.py src-tauri/target/release/tpdf.exe testdata/outline-simple.pdf
```

All four phases green, both controls included — the default state differed in all five fields
from the remembered one, and nothing opened when nothing was remembered. Expect
`Failed to unregister class Chrome_WidgetWin_0. Error = 1412` on each shutdown: that is
WebView2 teardown noise on a *passing* run, not a failure.

**Reopening every tab is the second half of the same script** (2026-10-03). After the four
phases above it copies the fixture to three files, leaves them open as tabs, and relaunches:
with the preference on the strip must come back in order with the right tab in front, with it
off only the last document may open (the control), and with the front tab's file deleted the
first remaining tab must be on screen. Two more launches leave the front tab on the right
side and relaunch: the two sides must come back with their tabs, the tab in front of each
and the reader on the right, and the launches before them, which recorded no sides, must
show one. `--only tabs` runs that half alone, and `--only places` the first half alone. Any fixture does for it; the eight-page minimum
below is the first half's.

**Open, and intermittent: the `default` control can hang instead of running** (Windows,
2026-08-08). It passed twice that day and then timed out three runs in a row, always the same
phase and never any other. What the hung launch looks like from outside is the useful part: the
process is alive, and `MainWindowHandle` is **0** — it never created a window, so it stopped
before any JavaScript could run and no frontend change can explain it. The shape fits a
single-instance secondary that forwarded its argv and failed to exit; the phase launches
immediately after the previous one's process goes away, and `tauri-plugin-single-instance` is
Windows-only, so this race does not exist on macOS.

It is **not** caused by the print or session threading changes in the same release: the run was
repeated with those stashed and the transcript is identical line for line, through the record
phase, the file inspection and the hang. Three of four phases pass either way, `verify` — the
one that actually tests restore — at 8/8. Two things to do before chasing it: kill any stray
`tpdf.exe` first, since one alive process changes what every later launch does, and check
`MainWindowHandle` rather than assuming the app got as far as its checks.

**The fixture must have at least eight pages.** The target page is 7 and `Viewer.goToPage` clamps
to the last page, so a shorter document reports a wrong page rather than a wrong fixture —
`text-base14.pdf` gave *"page 0, wanted 7"*, stably, on a restore that was working. There is a
named check for it now (*"the document is long enough to test page restore"*), so the run says
which it is. `outline-simple.pdf` above has 12 pages; `incr-scan-20p.pdf` has 20 and renders
faster than the A0 fixtures.

**And the run now stops there rather than colouring the rest of the transcript** (2026-07-30).
The named check alone did not settle it: it fails inside the `record` phase, and the driver
launched the other three regardless, so a short fixture still produced eleven failures of which
ten were `it opens on the remembered page: page 0, wanted 7` — the signature of a broken
restore, below the line that said otherwise, and these harnesses are read from the tail. The
driver reads that check's verdict out of the transcript, skips the remaining phases by name and
ends with `[FAIL] session restore was not tested: <fixture> has too few pages ...`. Measured on
`text-base14.pdf`: eleven failures to one, three launches not made, exit code still 1.

The check's name is duplicated into `session_check.py` to do that, which is a coupling rather
than an assertion — so a transcript that does not contain it is reported as a failure of the
script, not read as "the fixture is fine". Proved by renaming it: a green run turns red with
*"this script cannot find a check named ... it has been renamed in sessioncheck.ts"*.

**Start from a clean process table, and this is not advice.** A leftover `tpdf.exe` hangs the
next run outright: reproduced twice on 2026-07-30, where the launched app sat at **0.00 CPU**
for minutes and no phase produced a summary, and both times it passed immediately after
`Get-Process tpdf,python | Stop-Process -Force`. Same shape as the occlusion warning below for
`open_check.py`, and worse here because `webview_guard` returns early off darwin, so **nothing
guards it on Windows** — Chromium suspends an occluded page exactly as WebKit does. The tell is
the CPU figure, not the clock: a child holding 0.00 CPU is hung, and a run that is genuinely
working through four launches is not. Check that before extending a timeout.

Four launches, and the two labelled `control:` are what make the other two mean anything:

| phase | session | argument | asserts |
|---|---|---|---|
| `record` | fresh | a document | drives to page 7, one quarter turn, a fixed zoom, sidebar open — then writes it |
| `control: opening without a session` | empty | a document | that state is **not** where the app opens by itself |
| `verify` | recorded | none | the app came up in that state, told only by the file |
| `control: launching with nothing remembered` | empty | none | no document opens when nothing is remembered |

Without the first control, "restored to page 7" is satisfied by an app that happens to open
there — the same shape as a check whose precondition is already satisfied, which this
repository has paid for four times. It fails if *any* of the four fields already matches,
not only if all of them do: a restore that got only the rotation right would otherwise hide
behind a default that shared the page. Without the second, an app that reopened the last
file it could find by some other route would pass `verify` perfectly.

Between the phases the script reads the written `session.json` itself. Writing a place and
reading one back are different halves, and a run that only did the second would find nothing
to restore and report that somewhere else entirely.

Unlike every other harness here, **this one does not replace the application** — it boots
normally and observes itself, because restoring is part of the boot and a check that drove
`session.ts` directly would be a second implementation agreeing with the first. Same bundle
and unlocked-screen requirements as the viewer check.

Every launch gets its own `TPDF_SESSION_FILE` in a temporary directory, and **the two
controls get one each rather than sharing**. Shared first, and the second control failed:
the first control opens a document, which is what it is for, so by the time the second
launched there was something to restore and a document duly opened. A control is the thing
you assume is inert, which is why the standing rule about what one phase leaves behind for
the next did not fire.

Unlike the viewer check, **the exit code here is meaningful** — see the note below.

### Checking file associations

A PDF reaches tpdf three ways and they share almost no code, so this drives all of them:

```
scripts/open_check.py \
    src-tauri/target/release/bundle/macos/tpdf.app testdata/text-heavy.pdf \
    --other testdata/outline-simple.pdf
```

Note it takes the **`.app` bundle** on macOS, not the executable inside it: two phases go
through Launch Services and there is nothing else to hand `open`. **On Windows it takes the
executable**, built with `--features tauri/custom-protocol`:

```
python scripts/open_check.py src-tauri/target/release/tpdf.exe \
    testdata/outline-simple.pdf --other testdata/rotated-90.pdf
```

Every phase but one runs there. `double-click` prints `[SKIP]` **with the reason**, so the
phase-name list is the same on both platforms and a reader can diff it: an Explorer
double-click hands the path over in argv, which `argv` already covers, and there is no Launch
Services layer to go through. The two `running` phases go through
`tauri-plugin-single-instance`, which forwards a second launch's argv to the first process.
This paragraph said until 2026-10-16 that `running` had no route on Windows; that was true
before the plugin was linked.

`window_checks.py` runs this check as `open`, with the executable inside `tpdf Checks.app`;
the script finds the bundle three folders up.

`HANDS_OVER_TO_RUNNING` is the single place that distinction lives, and each of the two
branching phase names is a constant rather than a literal at both call sites — a name written
twice eventually differs, and the diff then shows a check that vanished on one platform when
nothing had.

| phase | delivery | asserts |
|---|---|---|
| `argv` | the binary, with a path | the terminal and Windows double-click route |
| `double-click` | `open -a` on a cold app | the Apple Event, which is how macOS actually does it |
| `beats` | argv, with a different document remembered | a handed-over document wins |
| `control` | nothing handed over | the remembered one opens — without this, `beats` passes on an app that ignores the session |
| `running` | `open -a` on an app already up | the *emit* branch rather than the queue |
| `argv` with a place | the binary, with `--page 12 --zoom fit-page` | the document comes up at page 12 following fit page, and is still there after the boot |
| `argv` with a forward search | the binary, with `testdata/synctex/main.pdf --forward-search main.tex:8` | page 2, the line marked there, and the overlay painted over that box |
| `running` with a place | the binary, with the same, beside an app already up | the same place, and the second process ends with 0 within 15 s; on macOS that is `secondlaunch.rs`, which `open -a` does not reach |

`running` is the only phase that would notice the frontend and the backend disagreeing about
the event's name, and it carries its own control: nothing may be open before the document
arrives, or "a document arrived" is satisfied by one that was already there.

**The environment does reach an app that Launch Services started** —
`TPDF_OPENCHECK=… open -a tpdf.app file.pdf` propagates — which is what makes the
double-click phase testable rather than merely argued. Both `open` phases capture the app's
stdout with `open --stdout`.

Same bundle and unlocked-screen requirements as the viewer check, and one extra: **leftover
tpdf windows occlude new ones**, and an occluded page never runs, so a phase produces no
output at all. `pkill -f "tpdf.app/Contents/MacOS/tpdf"` before a run, or `TPDF_RAISE=1`.
This cost real time once already — it looked exactly like the failure it was sitting next
to, which was genuine.

### Checking the recent documents the shell is told about

Two lists, unrelated despite the name: `src/lib/recents.ts` is tpdf's own, shown in
the command palette; the shell's is the Windows Jump List and macOS's *Open Recent* and
Dock menu, filled by `recentdocs.rs` and by nothing else the application does.

**Windows: look at the file the shell writes.** `SHAddToRecentDocs` drops a shortcut per
document, so open one and look.

```powershell
ls "$env:APPDATA\Microsoft\Windows\Recent\*.pdf.lnk"
$s = New-Object -ComObject WScript.Shell
$s.CreateShortcut("$env:APPDATA\Microsoft\Windows\Recent\x.pdf.lnk").TargetPath
```

Resolve one — an entry existing is not an entry that opens. Note a Jump List needs an
*installed* build (a Start Menu shortcut is what gives the app an AppUserModelID), so a
binary from `target\release` will look as though this does nothing.

**macOS: there is no file, and every place you would look says the feature is broken.**
Measured 2026-08-20: `defaults read com.timostein.tpdf NSRecentDocumentRecords` does not
exist and never will (pre-Sierra location); `sfltool list-info` hangs; and
`~/Library/Application Support/com.apple.sharedfilelist/` answers `Operation not
permitted`, so what is in it is unknown — do **not** run that `ls` with `2>/dev/null`,
which turns the refusal into a convincing `total 0`.

So the check is **two launches**, which is the feature rather than a proxy for it. It
needs a bundle — `npm run tauri build -- --bundles app` — because a bare binary has no
identifier to key a list to.

```bash
APP=src-tauri/target/release/bundle/macos/tpdf.app/Contents/MacOS/tpdf
pkill -f "tpdf.app/Contents/MacOS/tpdf"
TPDF_RECENTDOCS_PROBE=1 "$APP" "$PWD/testdata/text-heavy.pdf" 2>&1 | grep recentdocs
# quit it, then:
TPDF_RECENTDOCS_PROBE=1 "$APP" "$PWD/testdata/rotated.pdf" 2>&1 | grep recentdocs
```

The first launch must print `before filing, AppKit holds 0` and the second
`before filing, AppKit holds 1` naming `text-heavy.pdf` — a document the second process
never filed. That carry-over is the whole assertion; a second launch holding 0 means the
call is being dropped. The probe is off unless the variable is set, so a shipped run does
not narrate its own menu bookkeeping into the one log a reader sends back.

### Checking that a Save reaches the disk

macOS only, and it needs a built bundle and an **unlocked** screen:

```bash
scripts/save_check.py                                  # the release bundle
scripts/save_check.py path/to/tpdf.app testdata/outline-simple.pdf
```

It copies the fixture to a temporary directory, opens it, and drives the real
menu — `Page > Rotate page clockwise`, then `File > Save`, then a highlight over
the page's own text and a second Save — reading the *file* back each time by
digest and through `qpdf --check`, which shares no code with anything here.
Since 2026-09-01 it also drives one Print, last, after appending to the file
underneath the open document: the job must be refused before any panel opens.
That phase reads the process's window count, not the prompt — the Save a copy
and Reload buttons the refusal carries are in the web view, which this harness
cannot reach.

**It exists because nothing else in the repository writes a file.**
`viewer_check.py` lists `file.save` as undriven with the reason (it would write
over the corpus fixture the rest of that run is reading), `save.rs`'s tests build
their plans directly, and `edits.test.ts` asserts the shape of the `invoke` call.
So when the 26.8.6 release commit recorded saving as "reported broken from the
running application", nothing here could test that claim — and put to its
author on 2026-08-21, it turned out he had never said it. The provenance is
recorded rather than quietly deleted, because the lesson is not about saving: an
unattributed sentence in a commit message became an open item, a paragraph in
this file and two harness docstrings, and none of it could be checked until there
was a check.

The control runs first and is the reason the rest means anything: Save must be
**withheld** on a document with no edits. It also asserts Save greys again after
the save — the reopen has to produce a clean document — and that nothing is
left in the directory, since staging writes a sibling and renames it, so a stray
is a commit that failed and said nothing.

**A locked screen is refused, not skipped and not survived.** The web view is
suspended while the session is locked, so the document never opens and every menu
item stays greyed, which reads exactly like an application ignoring its own menu.
The check reads `CGSSessionScreenIsLocked` first and exits 2 saying so.

First full run 2026-08-21, on 26.8.6 plus that day's commits, before phase 7 was
added on 2026-09-01: **10 checks, all green, 25 s** — Save withheld at rest, a rotation offering it, the file changing (10731 -> 10400
bytes), `qpdf` reading it back, Save withheld again after the reopen, a highlight saving too
(-> 14542 bytes), and nothing left beside the document. Its failure path was proved
separately by pointing a phase at a menu item that does not exist: `[FAIL] this check drives
a menu item that is not there`, exit 2, and no claim about saving either way.

So there is **no defect to reproduce here**: saving over the open document works from the
menu, twice in a row, for two different kinds of edit, in a scratch directory and in a
TCC-protected one (`~/Downloads`), on this machine. That is a statement about this route and
this fixture. Every refusal `save.rs` states needs a condition a clean local file does not
have — an encrypted document, a file changed under the open one, a missing baseline — and
each of those has tests of its own; since 2026-09-01 the changed-file one is also provoked
from the menu, by phase 7's second writer, though against a Print rather than a Save. What
none of them can tell you is whether the refusal
fires when it should not, which is what a real report of a spurious message would be for.

### Checking the menu bar

macOS only, and it needs a built bundle and an unlocked screen — but no document, no
fixture and no window of its own beyond the app's:

```bash
scripts/menu_check.py                                  # the release bundle
scripts/menu_check.py path/to/tpdf.app
scripts/menu_check.py --self-test                      # the rule, without a build
```

**Run it after touching `menu.rs`, `menubar.ts`, or any command's `title`.** It reads the
live menu bar through System Events and asserts three things: that the read returned menus at
all, that no two items in one menu carry the same name, and that the bar is exactly the
menus `menubar.ts` declares in that order, plus the predefined `Window` that `menu.rs`
appends.

It exists because on 2026-08-21 the application menu carried **two items named "About tpdf"**
and nothing in either language could have said so: the platform's items are never named in
our source, and ours arrive over IPC as data, so the only place both lists exist at once is
the bar. `docs/TRAPS.md` has the entry, including why our About was the one kept.

**It launches with `open`, deliberately** — not as a subprocess with pipes. A harness that
captures output supplies a stdout and a stderr that a double-clicked application does not
have, which is the trap that hid the Windows open defect for a month; this check has no
reason to differ from the reader's launch, so it does not.

Proved in both directions against real binaries rather than against a fixture: rebuilt from
`git checkout -- src-tauri/src/menu.rs` it reports the duplicate and exits 2, and with the fix
it exits 0. `--self-test` carries both measured menus so the rule can be shown to fire in a
second, and it is not a substitute for the run: it tests the predicate, not the menu.

It is **not** a gate, for the same reason `viewer_check.py` is not — an accessibility read
needs a real session, and on a headless runner it would not fail, it would hang.

### The command-line tool

`tpdf-cli` is the second `[[bin]]` (`src/bin/tpdf-cli.rs`, `src/cli.rs`). `cargo build --bins`
and `tauri build` build it; the bundler ships it as `Contents/MacOS/tpdf-cli`, signed with the
hardened runtime before the bundle, and as `tpdf-cli.exe` beside `tpdf.exe` in both Windows
installers. `README.md`'s *Command-line tool* section is its reference and `docs/PLAN.md` Phase 6
the record.

The `api` gate runs `api/python/test_api.py` against the built CLI, including a
complete edit/readback/extract workflow, merge order, split contents, selected-page
rotation and cropping, structured refusals, protocol errors, option-like filenames,
child-only passwords and process-tree timeout cleanup. Redaction tests cover text and
stdin regions, preserved control text, dry runs, refusals, and a retained secret in
an unrelated annotation that must leave the written copy unverified. A composed
fill/redact/readback workflow checks a confidential field with two widgets, selects
only its first occurrence for redaction, and verifies that the public field and
control text survive. A retained secret and a dry run must both stop before the
signing call. The same workflow runs against extracted release packages. The
Rust CLI integration section *JSON edit plans* separately reads written page
content, annotation dictionaries and geometry. The *PNG rendering* section checks
decoded colors across tile seams, saved crops and rotations, and output preservation
on refusal. These are headless tests of the same backend used by the GUI; they do
not replace window interaction checks.

A purchased certificate is not needed for the private-CA integration check:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli -- --filter 'sign --long-term'
# Long-term validation data for a document that is ALREADY signed (`tpdf long-term`,
# `longterm/existing.rs`). Through real workers, against the same private CA, offline:
cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli -- --filter 'long-term adds'
# The unit tests, which hold every refusal and the check before writing:
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -- \
    longterm::existing sign_dss:: commands::validation::
# Documents for other readers (qpdf, pyHanko); docs/VERIFICATION.md has the pyHanko script
# and the readings of 2026-10-10:
TPDF_LONG_TERM_OUT=/tmp/tpdf-long-term-added cargo test --locked \
    --manifest-path src-tauri/Cargo.toml --lib \
    write_documents_with_data_added_for_other_readers -- --ignored
# Proved by hand, since tests/cli.rs is not selectable by mutate_rust.py:
# save_outside.rs `if answered.locked {` made `if false {` turns "long-term refuses a
# document that needs a password" red (exit 4 for 3). Needs testdata/incr-encrypted-pw.pdf.
# Not measured: a real certificate authority or timestamp authority, and Windows.
# pyHanko's ades_lta_validation answers NO_POE for these files and for tpdf's signing-time
# output alike on the private CA, so it is not an oracle here.
```

This section also runs in the normal gates. Its private CA and local timestamp,
OCSP and CRL responders use synthetic certificates. The document signer uses an
in-memory `SecKey` on macOS or an unnamed CNG key on Windows. Only fixture key
bytes are imported, and every native handle is released. The OS evaluates the
timestamp authority against explicit roots for that call; no keychain, certificate
store, trust setting or shipped CLI trust option is changed. Normal worker
verification must still report these roots as untrusted. This exercises CLI
signing, native cryptographic operations, network requests to loopback responders,
contained workers, and saved-file readback. Identity discovery, persistent-key
permissions, hardware tokens and commercial-CA compatibility remain separate checks.

The cases cover successful OCSP and CRL evidence, an intermediate CA, revoked and
unknown answers, responder failures, an untrusted timestamp authority, and an
expired signer refused before native signing or revocation requests. To retain
three successful outputs and public roots for independent offline verification,
use a new evidence directory:

```sh
TPDF_PKI_OUT="$PWD/scratch/private-pki" cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli -- --filter 'sign --long-term'
uv run --with pyhanko python scripts/check_private_pki.py scratch/private-pki
```

On Windows set `$env:TPDF_PKI_OUT` to an absolute directory path before the Cargo
command. The verifier uses these roots only in memory and disables fetching. It
requires both intact signatures, complete-file archive coverage and embedded
revocation evidence. Its controls strip that evidence (intact signature, failed
trust) and alter covered content (both signatures fail integrity). It writes
`verification.json` with verdicts and artifact digests; no private keys are saved.

**Private-CA native signing, 2026-09-29:** macOS and native Windows each passed
all 22 focused checks. Independent offline verification accepted all six outputs
(two OCSP cases, two CRL cases and two intermediate-chain cases), and rejected each
missing-evidence and covered-byte-tampering control. Replacing the native signer
with a software signer made both native-request controls fail. Windows user
certificate/root and persistent CNG key inventories were unchanged. macOS keychain
search list, default keychain, user trust settings and signing-identity inventory
were unchanged after the native tests.

The optional OS-key client check requires an explicitly selected document-signing
identity already in the OS store. It never imports, exports, trusts or removes a key.
Run it separately from unattended gates; macOS may require a key-access prompt:

```sh
python api/python/check_signing.py --executable src-tauri/target/debug/tpdf-cli --identity "<certificate SHA-256>" --output-dir scratch/signing-api --expect-untrusted
```

Use a new output directory on each run. `--expect-untrusted` is for a self-signed
test identity; omit it when strict trust must pass. Optional `--timestamp digicert`
enables the timestamp request. The check writes synthetic inputs, signed and tampered
copies, rendered controls and a digest receipt for independent inspection. It checks
identity selection, invisible and visible signatures, strict trust, tamper rejection,
output preservation and optional timestamp integrity. It also runs the composed
form workflow through signing, then re-reads form values, text, comments, signature
integrity and coverage, and compares rendered output. Its digest receipt includes
the nested workflow artifacts and the redaction report. Ordinary API tests exercise
argument forwarding and refusals without signing with an installed identity. Successful
long-term signing still requires a CA-issued certificate with working revocation services.

For that test, install an identity intended for document signing in the OS store
(or connect its supported hardware token), and confirm its SHA-256 ID with
`tpdf identities --json`. The OS must trust its chain. A token must expose the
private key through Windows CNG or the macOS Keychain; availability in a separate
PDF application's token interface does not establish that. Then run:

```sh
python api/python/check_signing.py --executable src-tauri/target/debug/tpdf-cli --identity "<CA-issued certificate SHA-256>" --output-dir scratch/signing-lt --timestamp digicert --long-term
```

`--long-term` cannot be combined with `--expect-untrusted`. It explicitly enables
revocation requests to certificate authorities and an archive timestamp request.
The check requires two intact signed revisions, an actual document timestamp
covering the final file, trusted timestamps, and complete good revocation evidence
for the signer and signature timestamp authority. `verify --strict` alone also
accepts signatures without revocation evidence, so it is not sufficient for this
check. A refusal ends the run; there is no fallback to ordinary signing. The
readback is saved as `long-term-verify.json`, and a tampered copy must invalidate
both signatures. `test_signing_checks.py` tests the instrument offline, including
missing evidence, an ordinary signature in place of an archive timestamp, and
refusal without fallback. Those controls do not establish live CA compatibility.
No suitable commercially issued identity was available in either OS store on
2026-09-29; the live commercial-CA and hardware-token runs remain outstanding.

**OS-key Python workflow, 2026-09-29:** the existing macOS RSA test identity passed
18 checks, including a DigiCert timestamp. Native Windows x64 passed 18 checks with
a disposable RSA identity and 17 with ECDSA P-256 (no timestamp requested for ECDSA).
Both Windows keys were non-exportable CNG keys; their certificates and key files were
removed after testing, and the pre-existing certificate inventory was unchanged.
No trust root was added. Both platforms passed all 22 unattended API tests and 93 CLI
unit tests. Independent pyHanko readback accepted all eight signed outputs, confirmed
both timestamps and visible reason/location metadata, and rejected all three tampered
copies. These self-signed signers remained untrusted; successful CA-backed long-term
signing was not established by this run.

**Composed form workflow, 2026-09-29:** all 24 unattended API tests passed on
macOS and native Windows. The extended OS-key check passed 34 assertions each for
macOS RSA, Windows RSA and Windows ECDSA P-256, without timestamp requests in this
run. Independent pyHanko verification accepted the three workflow signatures and
matched their certificate digests to the selected identities. Independent form
readback and PNG decoding confirmed that the second confidential widget disappeared
while the public answer and control text retained their pixels. Moving the signing
call ahead of redaction approval made both negative controls fail at the assertion
that signing must not be called. Windows test keys and certificates were removed;
the pre-existing certificate inventory was unchanged.

The release job also runs `scripts/check_packaged_api.py` after bundling and
notarization. It extracts the DMG and macOS updater archive, or both Windows
installers, outside the checkout. It builds the Python client wheel with a pinned
backend, installs it into a fresh virtual environment, asserts the import comes
from that environment, and runs the same API tests against each extracted CLI.
Missing or ambiguous packages, wrong versions, skipped tests and failures stop
the job. A missing-engine control must fail rendering after hiding only the
extracted PDFium library. Nothing is installed into the user's applications or
registered as a PDF handler. The JSON workflow artifacts record package, CLI,
engine and wheel SHA-256 digests and test totals.

Reproduce the package check with Python 3.13+ and `uv` on PATH:

```
python scripts/check_packaged_api.py --bundle-dir src-tauri/target/release/bundle --updater --report scratch/packaged-api.json
```

The directory must contain exactly one current package of each required kind;
use a separate directory when old build outputs are present. Omit `--updater`
for a local macOS build that produced only a DMG. Windows additionally needs
7-Zip on PATH; MSI extraction uses `msiexec /a`, without installing the product.
The ordinary `packages` gate tests the runner's empty-suite, skipped-test,
failure and wrong-import controls, plus release wiring and publication refusal.

Verified on 2026-09-29 against locally built release packages: all 12 API tests
passed for each of the macOS DMG, macOS updater archive, native Windows x64 MSI
and native Windows x64 NSIS setup (48 executions, zero skips). All four
missing-engine controls failed rendering as required. The Windows run left no
CLI process behind and did not change the original checkout. The publication
helper also refused the older green v26.9.22 run because it lacked the packaged
API steps.

The hosted rehearsal also passed on 2026-09-29: `v26.9.22-rc1` at `9d05867`,
[Release run 36562793601](https://github.com/tstone-1/tpdf/actions/runs/36562793601).
Both platform jobs in [CI](https://github.com/tstone-1/tpdf/actions/runs/36559413820)
and the [dependency audit](https://github.com/tstone-1/tpdf/actions/runs/36559413644)
passed before the tag. The release reused that exact-commit CI evidence, verified
macOS Developer ID signing, notarization and stapling, and passed all 48 packaged
API test executions with zero skips and all four missing-engine controls. The
four package SHA-256 digests in the test receipts matched the uploaded release
assets; the draft held all eight expected files. All five updater entries named
uploaded packages and matched their updater signatures. `publish_release.py` accepted
the completed rehearsal in read-only mode. The rehearsal remains unpublished.

**Expanded local package rehearsal, 2026-09-29, `1bc9e74`:** all 24 API tests
passed against each freshly built macOS DMG, macOS app archive, Windows x64 MSI
and Windows x64 NSIS setup: 96 executions, zero skips, and four successful
missing-engine controls. Each run imported the installed wheel outside the
checkout. The macOS archive was created locally from the fresh app; this run
did not exercise hosted signing, notarization or updater signatures.

The same extracted packages and installed wheel also passed 34 OS-key signing
assertions per package/identity: macOS RSA on both packages and Windows RSA and
ECDSA P-256 on both installers, 204 assertions in total. This included filling,
verified redaction and signing in sequence. Independent pyHanko readback accepted
all 18 resulting signatures and rejected all six tampered copies; independent
form and pixel checks confirmed removal of the second confidential widget and
preservation of public content. Package and CLI digests matched between the API
and signing checks. Windows test certificates and CNG keys were removed, with
the original certificate, root and key inventories unchanged. These were
self-signed identities; no timestamp or long-term signing was requested in this
package run. The earlier hosted rehearsal remains evidence for `9d05867`, not
for this later commit.

**Expanded hosted rehearsal, 2026-09-29, `7823da7`:**
[`v26.9.22-rc2`](https://github.com/tstone-1/tpdf/actions/runs/36607432667)
passed both release jobs after both CI platforms and the dependency audit passed
on the exact commit. All four packages passed 24 installed-client API tests
each (96 executions, zero skips) and their missing-engine controls. Their SHA-256
digests matched the uploaded assets. All eight release assets were present;
all five updater entries matched their payloads and signature files, and all
three updater signatures verified against the application's public key.
macOS Developer ID signing, notarization and stapling passed. The publication
helper accepted the draft in read-only mode; the rehearsal remains unpublished.
The preceding CI attempt exposed descriptor aliasing during macOS worker startup.
Regression tests reproduced the old shuffle's failure before the fix; the final
local run passed all 29 gates in 763.4 seconds.

Native Windows x64 execution was verified on 2026-09-29 at `186991b`, using
Rust 1.97.1, Python 3.14.7 and the digest-checked `pdfium-8066-tpdf.1` engine.
All 322 CLI integration checks passed; the only skip was the macOS-only
`sandbox_check` inspection. All 89 CLI unit tests, the raw-tile boundary test
and all 12 Python API tests passed, including Unicode paths, JSON edits, form
filling, PNG rendering and timeout cleanup of the process tree. The complete
CLI run included the generated document-timestamp fixture. Both the isolated
test checkout and the original checkout stayed clean, and no CLI process
remained. This exercised the native debug CLI and Python client, not GUI
interaction or a newly packaged installer.

**What the gates run.** `cargo test` runs three things for it:

- `cli::tests` and `clitool::tests` --- the command line and every refusal, `--identity`
  resolution, the exit codes, the committed JSON and wording samples in
  `src-tauri/testdata/cli/`, and the README holding every JSON key and every exit code.
  Regenerate the samples with
  `TPDF_CLI_SAMPLES=write cargo test --manifest-path src-tauri/Cargo.toml --locked --lib cli::tests::every_json_shape_and_the_wording_match_their_committed_samples`.
  Run this test alone: parallel sample-directory checks can race newly created
  fixtures. Then run the ordinary gates and read the diff: a changed JSON sample
  is a changed schema (a renamed or removed key moves
  `report::SCHEMA`), and a changed `wording.json` must still pass `src/lib/cliwording.test.ts`,
  which holds it to `integrity.ts`, `signing.ts` and, for what was appended after a signature,
  `properties.ts`. `reading.json` is the same arrangement for
  `tpdf text`'s reading order: `reading::tests::cases` with the order `src/reading.rs` gives
  each, which `src/lib/clireading.test.ts` holds to `reading.ts`. A change to either file's
  ordering rules is a change to both, or one of those two tests goes red. `regions.json` is the
  same again for `tpdf redact`: `cli/regions.rs`'s runs and regions over those cases, raw quads
  and hits over a page break, which `src/lib/cliregions.test.ts` holds to `runsFor`,
  `areasFrom` and `matchHalves`; `redact.json` and `redact-dry-run.json` are its report.
- `tests/cli.rs`, **a harness-free test binary** (`harness = false`), because signing spawns
  workers by re-executing the current binary, and libtest's `main` does not answer
  `worker::WORKER_ARGV`. It prints `[PASS]`/`[FAIL]`/`[SKIP]` lines and exits 1 on a failure or
  on no pass at all. Its four parts: the built tool's `verify --json` against the in-process
  reader on the signed fixtures (the fixtures must span intact, altered, broken and weak, or the
  control fails); signing through `cli::run` with a software P-256 key the test holds, read back
  by the built tool, with a wrong-digest key, an encrypted document and a non-PDF as refused
  controls; the tool's process never loading PDFium (`DYLD_PRINT_LIBRARIES`, split by pid,
  with a worker loading it as the control; and this process's own image list before and after
  `cli::run`, then PDFium bound here as the control, with `info` and `text` held to the same
  two rules); `sandbox_check` on a worker, against this process; `info --json` against
  `cli::info::document` built in this process, and a password-protected document locked, then
  described with `--password-env`; `text --json` against the in-process extraction and against
  the lines the fixtures' manifests record, with `tagged.pdf`'s tags removed as the control,
  and `--pages` and its refusals; when Poppler's `pdftotext` is installed, word overlap
  and word order beside it, printed as `[INFO]` and never counted; `fields --json` against
  `cli::fields::report` over `DocumentGraph::form` in this process, on two forms the test
  builds (`tests/cli/forms.rs`: an inherited field on two pages, and one with every control
  and every reason a field is not editable) and on `form.pdf` and `signed-nested-field.pdf`,
  with every kind and every reason required between them as the control; and `fill` through
  the built tool --- every kind of answer read back by a separate `fields` run, every field it
  was not asked to change unchanged, `--values -`, seventeen refusals each on its own and six
  together with nothing written, the output rules, a signed and a certified document refused,
  an encrypted form filled with its password and still encrypted (skipped without `qpdf`), and
  a filled form signed with the test key reading back intact. `fields` and `fill` are held to
  the containment rule too: under `DYLD_PRINT_LIBRARIES` `fill`'s three workers map PDFium and
  the tool's process does not. `redact` (`tests/cli/redact.rs`) on a two-page document the test
  builds --- by `--text`, `--pattern` (an e-mail address and an IBAN), `--regions` and all
  three --- with none of the matched strings left for the built tool's `text`, `pdftotext` and
  the bytes of `qpdf --qdf`, both `CONTROL-KEEP` lines kept, the input as the control that every
  reader finds every string, nothing on stderr, and a dry run of each line writing nothing and
  counting what the write took; parity with `redaction::redact_copy_asked` driven in this
  process on the same regions, for that document, a document whose shared form the removal
  leaves, `text-base14.pdf` and `text-marked.pdf`; and the refusals --- an existing output, the
  input under a hard link, XFA, a signed document (and `--invalidate-signatures`), a locked one
  (and its password). `sign --timestamp` against a fake authority on 127.0.0.1 that mints a
  token over what the request carried: sound, read back intact and attested by the built tool
  and the in-process reader alike; and six failures --- another imprint, another nonce, an SHA-1
  signature, a refusal carrying a token, an answer too long, nobody listening --- each exit 3
  with nothing written, and an `ftp:` address exit 2. Its macOS containment check injects a
  test-only dyld callback library, compiled with `clang`, which logs images per PID. The
  coordinator is identified by the PID returned from spawning it; its log must include
  initialization, known images and completion. Loading PDFium into that PID, or starting
  no engine-loading worker, makes the negative controls fail. Shared dyld stderr is not
  used as evidence because its records can interleave. The parts that bind PDFium in
  this process run after the one asserting it has not. Without generated fixtures (*Test
  fixtures*) the fixture parts say `[SKIP]`.

```bash
cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli
# Focused diagnosis: list sections, then run one without the rest of the suite.
cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli -- --list
cargo test --locked --manifest-path src-tauri/Cargo.toml --test cli -- --filter 'never maps'
cargo test --manifest-path src-tauri/Cargo.toml --lib cli::     # includes clitool::
```

**Proved able to fail.** The `cli:`, `reading:` and `cli tool link:` mutations in
`scripts/mutate_rust.py` (`--only "cli:" --only "reading:" --only "cli tool link:"`, forty-four;
all 41 `cli:` and `reading:` ones caught on 2026-09-27, three only after their fixtures were
strengthened --- see `docs/PLAN.md`) and the `cli wording:` and `cli reading:` ones in
`scripts/mutate_frontend.py`. `tpdf redact` added 24 more (`--only "cli: regions" --only "cli:
redact"`) and four frontend ones (`cli regions:`, and `cli wording:`'s redaction sentence);
two of the 24 survived their first run, each a test whose fixture decided the case before the
rule could --- a turned character that did not overlap the run it must not join, and a third
page that could not have matched a carried tail --- and are caught since. `tests/cli.rs`
cannot be selected by that harness, so its mutations were run by hand, each rebuilt and
restored byte-identical (SHA-256 checked), each turning red exactly the check named:

```
cli::main without the worker dispatch   -> 23 red: every check that goes through the built
                                           tool (its worker runs the usage branch: "`--render-worker`
                                           is not a command"), and the worker-loads-PDFium
                                           control. Signing through cli::run stays green --- its
                                           workers are this test binary, which dispatches.
run_verify binds PDFium in-process      -> 2 red: "the tool's own process never loaded PDFium"
                                           and "no PDFium in this process", alone
worker_child skips sandbox_init         -> 1 red: "the worker is sandboxed", alone
verify reports every verdict as intact  -> 4 red: the altered, broken and SHA-1 fixtures'
                                           agreement, and the verdict-variety control
sign writes the input unchanged         -> 13 red: sign exits 4 with "did not find the new
                                           signature ... intact", and every read-back of it
```

macOS arm64, 2026-09-27, 49 checks green before and after each, the file's SHA-256 equal
before and after. The same day, for `info` and `text`, against 102 checks, the same way:

```
asked() ignores Response::locked        -> 5 red: info's locked report and its wrong-password
  (save_outside.rs)                        check, verify's locked kind, and both of text's
                                           locked refusals
the session never sends the password    -> 2 red: info and text with the password
text ignores --pages                    -> 2 red: --pages 3,1, and the page past the end
info drops the worker's page sizes      -> 10 red: every info agreement, the password one too
text asks for page 1 every time         -> 14 red: every multi-page text agreement, every
                                           manifest page after the first, --pages 3,1 and
                                           the password read
info ignores the worker's form          -> 5 red: the four fixtures with fields, and the
                                           variety control
text binds PDFium in the tool's process -> 2 red: "text: the tool's own process never loaded
                                           PDFium", and "no PDFium in this process"
reading.rs ignores the tags             -> 3 red: tagged.pdf page 1 against its manifest,
                                           the tagged-against-geometric control, and the
                                           orders variety control
```

For `fields` and `fill`, the same day, against 146 checks, the same way:

```
fill without the signed-document refusal -> 2 red: the signed form and the certified document
the writer drops the form answers        -> 11 red: every successful fill exits 4 ("was written
  (save.rs)                                 as ... and reads back as OLD") and its output is
                                           removed, the containment run of fill included
... and fill never compares the read-back -> 6 red: the separate `fields` read of every output,
                                           and the fill-then-sign and encrypted round trips
the read-back is not given the password  -> 2 red: the encrypted fill (4, copy removed) and its
                                           still-encrypted check
fields binds PDFium in the tool's process -> 2 red: "fields: the tool's own process never loaded
                                           PDFium", and "no PDFium in this process"
the worker answers every form with none  -> 41 red: every fields agreement and its two variety
  (worker_child.rs)                         controls, every fill, and info's form agreements
```

The first run of the last one found a defect in the test rather than the code: an index into a
`serde_json::Map` panicked on a missing key and took the summary line with it, so the run
reported nothing countable. It is a `get` now, and the rerun gives the 41.

For `redact`, the same day, against 194 checks, the same way (each file's SHA-256 equal before
and after):

```
redact_copy_asked ignores its also reader -> 1 red: shared-form.pdf's search of the written file
  (commands/redact.rs)                       finds nothing beside the application's reason
a spare reports its parent's exit          -> 4 red: "nothing is printed on stderr", every line
  as an error (worker_child.rs)               ("[worker] the parent closed the handover socket")
cli::main without the OCR worker dispatch  -> 8 red: the four stderr checks ("`--ocr-worker` is
                                             not a command"), and the tool's reasons against
                                             the application's on all four parity documents
redact parses in the tool's process        -> 3 red: the dry run's and the write's containment,
  (Backend::InProcess)                        and "no PDFium in this process"
redact ignores --pages                     -> 1 red: "--pages 1 ... leaves page 2's"
the application calls every copy verified  -> 7 red: three documents' verdicts and exit codes,
  (redact_copy_asked)                         and text-marked.pdf's own reason; shared-form.pdf
                                             stays green, because the tool's search of the copy
                                             finds the word and withholds the verdict itself
```

For `sign --timestamp`, 2026-09-28, against 226 checks, the same way (`cli/sign.rs`'s SHA-256
equal before and after). The fake authority is a `TcpListener` on 127.0.0.1 in the test:

```
sign asks no authority whatever          -> 9 red: the sound case exits 4 --- the read-back's
  --timestamp said                          own rule, a timestamp asked for and not in the
                                           file --- and says nothing attested; and all six
                                           failures exit 4 with the file written, since
                                           nothing was asked that could fail
a failed request exits 4, not 3           -> 6 red: every failure, nothing written in each
```

**Against the real keychain, run explicitly outside unattended gates.** `identities` reads certificates only
and raises no prompt:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin tpdf-cli
src-tauri/target/debug/tpdf-cli identities --json
```

It should list a Developer ID Application certificate, if the machine has one, under
`not_usable` with *it is issued for code signing, not for signing documents*, and the test
identity below (*A TEST SIGNING IDENTITY*) under `usable`. **Signing raises the keychain prompt
for `tpdf-cli`** (a program distinct from `tpdf`, so it is asked about separately), which the
person at the machine answers:

```bash
S=$(mktemp -d)
cp testdata/incr-signed.pdf "$S/in.pdf"
src-tauri/target/debug/tpdf-cli sign "$S/in.pdf" -o "$S/signed.pdf" --json \
    --identity "tpdf TEST SIGNER - not a real identity"
src-tauri/target/debug/tpdf-cli sign "$S/in.pdf" -o "$S/visible.pdf" --no-image \
    --identity "tpdf TEST SIGNER - not a real identity" \
    --visible --rect 40,40,240,80 --reason Approved --location Hamburg
# Three readers that share no code:
src-tauri/target/debug/tpdf-cli verify "$S/signed.pdf" "$S/visible.pdf"      # intact x2 each
uv run --with pyhanko --quiet python3 testdata/check_signature.py --json "$S/signed.pdf"
cargo run --manifest-path src-tauri/Cargo.toml --example signature-probe -- \
    "$S/signed.pdf" --mode integrity
python3 - "$S/signed.pdf" "$S/blob.der" "$S/covered.bin" <<'PY'
import re, sys   # the last signature's CMS and the bytes it covers, as sign-probe cuts them
b = open(sys.argv[1], "rb").read()
a, x, y, n = map(int, re.findall(rb"/ByteRange\s*\[\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s*\]", b)[-1])
raw = bytes.fromhex(b[x + 1:y - 1].decode())
head = 2 if raw[1] < 0x80 else 2 + (raw[1] & 0x7F)
size = raw[1] if raw[1] < 0x80 else int.from_bytes(raw[2:head], "big")
open(sys.argv[2], "wb").write(raw[:head + size])
open(sys.argv[3], "wb").write(b[:x] + b[y:y + n])
PY
openssl cms -verify -binary -noverify -inform DER -in "$S/blob.der" -content "$S/covered.bin" \
    -out /dev/null                                   # OpenSSL 3; "CMS Verification successful"
```

The cut was checked both ways on 2026-09-27: `incr-signed.pdf` verifies, `signed-altered.pdf`
fails with *content verify error*. The OS-key Python workflow described above was
subsequently run against real macOS and Windows stores on 2026-09-29.

**The link.** *Install command-line tool…* is not driven by any check: it writes
`/usr/local/bin` and can raise an administrator prompt. `clitool::tests` holds the decision over
a scratch directory. By hand, on a built bundle: install, `ls -l /usr/local/bin/tpdf`,
`tpdf --version`, uninstall, and check the link is gone; then put an ordinary file at that path
and confirm both commands leave it alone.

### The exit code of a spike run

`AppHandle::exit(code)` does **not** set the process's exit code. It ends the event loop,
`App::run` returns normally, `main` returns unit, and the process exits 0 whatever was asked
for. Every automated run here therefore reported success through `$?` for its whole
existence, `viewer_check.py` included. Fixed 2026-07-27 in `spike_exit`, which now flushes
and calls `std::process::exit`.

If you add a harness, do not let the exit code be its only verdict — parse the transcript
too, and make the two agree. That is what caught this: a run printing `[OK] session restore
verified` directly beneath a phase whose own last line said `0/1 checks passed`.

### The four Phase 0 spikes nothing above invokes

`AGENTS.md` says this file has the invocations, and for four `[[example]]` targets it did
not — measured 2026-08-28 by diffing the `[[example]]` names in `src-tauri/Cargo.toml`
against this document: 40 targets, 4 unnamed. They are the oldest spikes, they answered
their question once, and their answers are load-bearing in `docs/PLAN.md` — which is
exactly why they should still be runnable rather than quietly rotting into files nobody
knows how to start.

```bash
# Spike 0.3, the gating one: can one text object be edited and the rest of the page
# reproduced faithfully? Two routes, PDFium and lopdf, measured against each other.
cargo run --release --manifest-path src-tauri/Cargo.toml --example text-roundtrip -- \
    testdata/text-base14.pdf --strict-surgical

# Spike 0.6: does an appended update section satisfy a reader that is not ours?
cargo run --release --manifest-path src-tauri/Cargo.toml --example incremental-save

# Does `pdfium-render`'s `thread_safe` actually serialize PDFium? The whole
# worker-process architecture rests on the answer, so it is measured rather than cited.
cargo run --release --manifest-path src-tauri/Cargo.toml --example thread-probe

# Can a worker be handed its document *after* it is sandboxed, over a socket? This is
# what a pre-spawned worker would need, and it is why the ~6.6 ms floor sits where it does.
cargo run --release --manifest-path src-tauri/Cargo.toml --example fdpass-probe
```

`text-roundtrip --strict-surgical` exits non-zero if either surgical variant fails
to change the target, changes pixels outside it, cannot be reopened, or yields
the wrong text. Without that flag it is an exploratory report, including expected
PDFium failures; the process exit alone is not a verdict. `text-roundtrip` and
`incremental-save` need fixtures; `fdpass-probe` is macOS-only. The self-contained
Phase 5 font matrix, contained text-edit probe, strict writer controls and native
WebKit/PDFKit commands are
in `docs/PLAN.md` §7; they need no system font or private document.

---

### Fuzzing, and how to read what it leaves behind

`src-tauri/fuzz/run.py` **is** the invocation, for the reason `scripts/gates.py` is the gate
list: three things have to be right on every run — the toolchain, a linker flag without
which the build does not link at all, and a per-target input bound — and a command copied
into prose loses one and then measures something weaker.

```bash
src-tauri/fuzz/run.py --list                                     # targets and their bounds
src-tauri/fuzz/run.py --target save_rewrite_update --seconds 3600
src-tauri/fuzz/run.py --build-only
```

**Serially.** Nine `cargo fuzz run` invocations started together queue on one build lock and
print nothing, which is indistinguishable from nine fuzzers finding nothing — the trap index
has that one.

A short run can spend its whole time budget replaying the saved corpus. The
budget includes initialization: compare the execution counters on `INITED` and
`DONE`, and require the latter to be larger before reporting new-input fuzzing.
If they are equal, extend the budget. `new_units_added=0` alone does not answer
this question; it counts additions to coverage, not generated inputs.

#### A target that reaches a defect we cannot fix has to fork

libFuzzer stops at the **first** finding, which is the right default: a finding is the point,
and stopping puts it in front of you. It is the wrong default for a target that reaches a defect
nobody here can fix, because the run then ends in the same place every time and everything
behind it is unreachable.

`lopdf_load` and `encoding_scan` both reach `docs/THREAT-MODEL.md` residual risk 21 — `lopdf`'s
cross-reference parser multiplying out the `/W` field widths a document declares and asking for
the product, which aborts through `handle_alloc_error` where no guard of ours can sit. Measured
2026-09-02 on `lopdf_load`, same corpus and same binary:

| mode | executions | wall | outcome |
|---|---|---|---|
| default | 80,269 | 21 s | stopped on the abort |
| `-fork=1 -ignore_crashes=1` | 380,803 | 96 s | ran to time |

It had been stopping like that since 2026-09-01 and nothing said so, because no campaign had
been run to completion since. Both targets are in `run.py`'s `MUST_FORK` with that reason
written beside them, so they fork whether or not `--fork` is given; `--fork` forces it on for
everything.

**Fork mode redefines a clean run, and this is the part to carry rather than the flag.** With
`-ignore_crashes=1` libFuzzer answers **0** for a forked run that completed, however many
children it buried — so an exit code stops meaning what it means everywhere else here. A
harness still reading it would report a target aborting every few minutes as permanently green.

`run.py` therefore snapshots `artifacts/<target>/` before the run and diffs it after, in
**both** the foreground and background paths, and a new file is a failure in either mode. The
snapshot is taken before anything starts because the verdict is a difference and not a count:
the directory already holds files from earlier runs. Proved both ways — a 60 s `lopdf_load`
run exits 0 with no new artifacts, and a file appearing mid-run turns it red, names the file
and points here.

The foreground path is worth its own sentence: a bare `--target <name>` lands there, and when
the artifact check was first written it covered only the background branch, so the invocation
people actually use kept the exit code as its whole answer. That is *a check bound to one
caller covers only that caller*, and here the interpreter caught it because `run` had grown an
argument.

#### Reading an artifact: three shapes, and two of them look like the third

A run that stops leaves a file in `src-tauri/fuzz/artifacts/<target>/`. Both the directory and
`corpus/` are gitignored, so **an artifact is scratch and never a regression test** — what
makes a finding permanent is a test in the source tree, and for the one defect that is upstream
rather than ours, a generated fixture (`testdata/make_xref_bomb_pdf.py`).

Every artifact triaged here has fallen into one of three shapes, and the filename tells you
nothing about which:

1. **A real defect.** Re-runs alone, allocates or crashes on its own.
2. **A sampler misattribution.** libFuzzer runs without a sanitizer here, so its memory sampler
   fires on a timer and blames whatever is executing. The clearest instance triaged here was
   `oom-da39a3ee...`, which is the SHA-1 of the **empty string** — verified with
   `printf '' | shasum`, not inferred from the name. The empty input allocates nothing. It was
   deleted on 2026-09-02 along with five siblings, so do not go looking for it; a shape-(2)
   artifact carries no information and costs the next reader a triage.
3. **A real defect below the threshold.** The one that is easy to get wrong, because it reads
   exactly like (2): the input genuinely amplifies, just not past `-rss_limit_mb`, so libFuzzer
   only ever filed the largest instance of the same bug.

The method, and each step exists because skipping it produced a wrong answer at least once:

```bash
B=src-tauri/fuzz/target/aarch64-apple-darwin/release/<target>
"$B" -runs=1 -rss_limit_mb=6144 -print_final_stats=1 <artifact>   # does it reproduce alone?
"$B" -runs=1 -rss_limit_mb=6144 -print_final_stats=1 <corpus file of the same size>
: > /tmp/empty.bin                                                 # NOT /dev/null -- see below
"$B" -runs=1 -rss_limit_mb=6144 -print_final_stats=1 /tmp/empty.bin
```

Read `stat::peak_rss_mb` against the other two, not against a number you remember. The floor is
32--55 MB depending on the target, measured 2026-09-02.

`/dev/null` does not work as the empty input and fails in a way that reads as a broken checkout:
libFuzzer treats any argument that is not a regular file as a **directory**, so it answers
`ERROR: The required directory "/dev/null" does not exist`. Use a real zero-byte file.

**When it comes back clean, that is a hypothesis too.** This is the half that is easy to skip,
and skipping it is how 2026-09-02 opened: an artifact that did not reproduce was read as stale,
and the actual cause was that the fuzz target had been changed to work around the very defect
it found, taking its own subject out of every later run. **The control is to revert the fix and
re-run** — if the artifact goes loud again, the fix is what silenced it; if it stays quiet,
something else did and you do not yet know what.

**Revert *every* guard added since the artifact was filed, not only the one you suspect.**
This is the half that was wrong until 2026-09-02, and getting it wrong nearly retired a real
defect as uninterpretable. `crash-732de3ab` came back clean; reverting the fix it was filed
against — the unreduced `page.turns + view % 4`, which overflows `u8` from 253 up — left it
clean too, which reads as "some third thing silenced this and we do not know what". It was
silent because a guard added a day *later*, the made-page bound of residual risk 22, refuses
that plan before the turns arithmetic runs. With all three reverted together it panics at
`save.rs:3097` with `attempt to add with overflow`, on the first run, every time.

That is *a control refused by a different guard than the one it was written for*, which the trap
index already carries twice — and neither previous instance was about a fuzz artifact. Guards
accumulate on a path, so an artifact's age is the thing to check first: everything merged since
is standing between the input and the defect it was filed for.

That control is what separated shapes (2) and (3) here. Three `save_rewrite_update` OOMs were
about to be written off as sampler noise; with the fix reverted they read **207 MB**, **291 MB**
and **6,201 MB** against a 54 MB floor. All three were the same defect at three magnitudes, and
only the largest had ever been filed as an OOM.

**Know what an ordinary parse of that size costs before calling anything an outlier.** This is
the measurement that separates (2) from (3) when there is no fix to revert, and it is cheap:
run three corpus files of the same size. Measured 2026-09-02, `lopdf_load` has a 32 MB floor,
and 2,879-byte corpus documents cost **32 MB, 100 MB and 165 MB** — so the three artifacts
sitting at 100 MB were squarely ordinary, and one perfectly healthy corpus file is the most
expensive input of the three. `links_scan`'s floor is 53 MB and its corpus reads 53--54 MB. A
single reading against a remembered floor would have made every one of those look like a
finding.

**A `crash-` whose message is `memory allocation of N bytes failed` is an abort, not a panic.**
`catch_unwind` cannot see it, and in this repository it is upstream: `lopdf`'s cross-reference
parser multiplies out the field widths a document declares in `/W` and asks for the product.
`docs/THREAT-MODEL.md` residual risk 21 has the full account, including why no guard in tpdf's
own code can sit in front of it. **Five artifacts across three targets carry it, at five
magnitudes** — 45555555555555555, 3333333333333333332, 1844674400000000000, 40000000000000000
and 6744073709551615 bytes, each reproduced rather than read off a filename. The numbers differ
and the defect does not, so a sixth adds nothing: check the message, then delete it.
