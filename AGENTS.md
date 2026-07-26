# AGENTS.md — tpdf

Canonical, portable project knowledge for any coding agent working in this repository.
Claude loads it via the thin `CLAUDE.md` (`@AGENTS.md`); Codex auto-loads it.

Personal cross-repo policy (git workflow, account enforcement, quality gates, per-OS
notes) lives in `tstone-1/agent-memory` and is **not** repeated here. This file records
only what is true of tpdf specifically.

---

## What tpdf is

A desktop PDF viewer and editor for macOS and Windows. Built because nothing on the
market fits: Adobe Acrobat is slow, buggy, and hides its tools behind endless menus;
Foxit is the same shape with a different skin; SumatraPDF is fast and lightweight but
cannot edit.

**The thesis, in one line:** SumatraPDF's speed with Acrobat's capability, and a UI where
you never hunt for a tool.

Three non-negotiable properties, in priority order:

1. **Fast.** Cold start to first page painted under 300 ms. Scrolling never stutters.
2. **Discoverable.** Every command reachable in two keystrokes via the command palette.
3. **Capable.** Annotations, page operations, forms, signatures, true redaction, and
   eventually in-place text editing.

Sibling projects built on the same reasoning: `screenpick` (screenshot tools were
bloated), `dblitz` (DB Browser for SQLite was missing things).

---

## Hard constraints

### Licensing: permissive dependencies only

**No AGPL or GPL dependencies. Ever.** This is a deliberate, load-bearing decision, not
an accident of what was convenient.

MuPDF (what SumatraPDF uses) is the obvious engine and was rejected. It is dual-licensed
AGPL / commercial, and the AGPL path costs three things that matter here:

- **It is viral across all of tpdf.** Every line of Rust and Svelte becomes AGPL.
- **It would forbid reusing tpdf code in private or work repositories.** Lifting tpdf's
  text extraction or page-splitting into a Nexperia tool that processes customer
  declarations or IMDS documents would require AGPL-ing that tool, which is impossible.
  This is the cost that actually bites, given the surrounding portfolio.
- **It would make relicensing later impossible** without an Artifex commercial licence
  (quoted case by case, $1,500 to $50,000+).

It would also rule out the Mac App Store, whose terms conflict with the GPL family.
Direct notarized distribution (what `screenpick` does) is unaffected.

The repository is currently **private**. Because every dependency is permissive, it can
be flipped to public at any time with no licensing work. Do not introduce a dependency
that removes that option. If a copyleft library ever looks necessary, raise it as a
decision rather than adding it.

### Redaction must be genuine

Redaction removes content. It does not draw a black rectangle over it. Any implementation
that leaves the underlying bytes recoverable is a defect, not a limitation --- see
`docs/PLAN.md` for the full subsystem design and the mandatory verification pass.

---

## Stack

| Layer | Choice |
|-------|--------|
| Shell | Tauri 2 |
| Frontend | Svelte 5 (runes), TypeScript `strict: true`, Vite |
| Backend | Rust |
| PDF rendering + page objects | PDFium via [`pdfium-render`](https://docs.rs/pdfium-render) (BSD-3-Clause) |
| PDF object-model surgery | [`lopdf`](https://docs.rs/lopdf) (MIT) |
| Platforms | macOS + Windows |

Same shell as `screenpick`, chosen because the muscle memory transfers and Rust does the
heavy work while the webview does the UI. See `docs/PLAN.md` for the render-pipeline
design that makes a webview viable, and for the fallback if it is not.

### Why two PDF libraries

They do different jobs and both are needed:

- **PDFium** renders, and exposes page objects (text, path, image) for editing. It is
  what Chrome ships, so it is correct on the long tail of malformed real-world PDFs in a
  way no younger library is.
- **lopdf** manipulates the PDF object graph directly --- metadata, embedded files,
  optional content groups, the structure tree, cross-reference tables. PDFium has no API
  for most of this, and redaction requires it.

Pure-Rust renderers were considered and are not yet ready to be the primary engine.

---

## Versioning

**CalVer `YY.M.MICRO`** (`26.8.0` = first August 2026 release). MICRO starts at 0 and
increments per release within the month. Same scheme as `screenpick`, `atr-viewer`,
`snowscreen`, `sitm-explorer`, `ticket-creator2`, `ddf`.

Following `screenpick`, **four files must agree** on every version bump:

1. `package.json`
2. `package-lock.json` (top-level *and* the root package entry --- `npm version <v> --no-git-tag-version` does both)
3. `src-tauri/Cargo.toml`
4. `src-tauri/tauri.conf.json`

Then run `cargo check` to refresh `Cargo.lock`.

Each release is a `Release vYY.M.MICRO: ...` commit. Unreleased work sits under
`## [YY.M.MICRO] - Unreleased` in `CHANGELOG.md`; the date replaces `Unreleased` only at
release time.

---

## Quality gates

`BUILD.md` will carry the release checklist. It must state every CI gating command
**verbatim, with flags** --- a checklist weaker than the gate it exists to satisfy buys
false confidence and goes red after the release is cut.

Planned gates (to be mirrored exactly in `.github/workflows/ci.yml`):

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
npm run check          # svelte-check + tsc
npm run lint
npm run test
```

Note `--all-targets` (covers test code), `-D warnings`, and `--locked` (catches an
uncommitted `Cargo.lock` after a `cargo update`). Dropping any of those silently tests
something weaker than CI does.

---

## Known traps

Things already paid for once, or verified before writing code. Add to this list rather
than rediscovering.

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

### PDFium: content streams are rewritten wholesale, not spliced

There is no way to surgically cut one object out of a content stream --- object
boundaries are not cleanly delimited and some stream content belongs to no object. The
correct approach is to regenerate the whole stream. Consequence: **any page-object edit
reflows the entire content stream**, so byte-level diffs of a page are meaningless, and
round-tripping a page through PDFium is not lossless. Do not use "the file changed" as an
edit-detection signal.

### PDFium: not thread-safe per document

A single `FPDF_Document` handle cannot be used from multiple threads concurrently. Tiled
parallel rendering therefore needs either a dedicated render thread per document with a
work queue, or several document handles opened over the same shared memory buffer. Decide
this in the Phase 0 spike and record the result here.

### Redaction conflicts with incremental save

Incremental save appends an update section and leaves the original bytes intact --- which
is exactly what redaction must not do. Applying a redaction is a **full-rewrite barrier**:
it forces a complete save with no incremental section and no retained original objects.
See `docs/PLAN.md`.

### Embedded fonts are subsetted

An embedded font contains only the glyphs already used. Typing a character that is not in
the subset has no glyph to draw. This is the root cause of every mangled Acrobat text
edit, and it constrains the entire text-editing design.

---

## Repository facts

- GitHub: `tstone-1/tpdf`, **private**.
- Commit identity resolves automatically from the path via the `includeIf "gitdir:"` rule
  in `~/.gitconfig` --- anything under `~/Developer/github.com/tstone-1/` gets
  `48162401+tstone-1@users.noreply.github.com`. Verify rather than assume if the clone
  ever lives elsewhere.
- `gh auth switch --user tstone-1` before pushing.
- Default branch: `main`.
