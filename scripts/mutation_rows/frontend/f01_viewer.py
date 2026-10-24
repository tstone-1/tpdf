"""One part of the frontend mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

#: Two mutations that belong here and are deliberately absent, because running
#: them established they are *not* defects --- and a variant that changes no
#: behaviour looks exactly like a test that cannot fail:
#:
#:   * banding `reading.ts`'s characters in arrival order rather than sorted by
#:     position, and
#:   * splitting a band at any gap at all rather than at a gutter-sized one.
#:
#: Both survive because the design repairs them downstream: `blocksOf` re-applies
#: the threshold when it decides where the columns are, and `readingLines` merges
#: fragments that share a band within a block. So over-splitting and mis-banding
#: are both recoverable, and only *under*-splitting loses information --- which is
#: the mutation immediately below the two, and which is caught.
#:
#: A third, added 2026-08-17 and removed the same day: bumping every page's tile
#: *epoch* in `Scroller.setPages` as well as carrying it. It survived the whole
#: suite, and the reason is that `setPages` calls `clearTiles`, which bumps the
#: generation --- one mechanism already drops every outstanding reply, so the
#: per-page bump changed no behaviour any test could see. The bump is gone and
#: the epoch is only carried; what replaced the mutation aims at `clearTiles`
#: itself, which is the mechanism that does the work.
#:
#: Recorded rather than deleted silently: the next person to notice the gap
#: should find out that it was measured, not overlooked.
ROWS += [
    Mutation('locale: ignore English region', 'src/lib/i18n.ts', 'COLOUR_REGIONS.has(locale.region ?? "US")', 'false', 'resolves ["en-GB","en-US"] to en-GB'),
    Mutation('locale: hardcode color command spelling', 'src/lib/appcommands.ts', '${message("color", locale)}: ${entry.name}', 'Colour: ${entry.name}', 'localizes command titles but preserves command ids in en-US'),
    Mutation('toolbar: omit color swatches', 'src/lib/toolbar.ts', '      swatch: entry,', '', 'offers a visible swatch for every color using the annotation palette'),
    Mutation('toolbar: offer the colour and width while erasing', 'src/lib/toolbar.ts', '  if (tool.erasing) return { color: false, width: false };', '', 'offers neither while erasing, even if the pen is still reported'),
    Mutation('toolbar: offer a width to a comment', 'src/lib/toolbar.ts', 'const STROKED: readonly ArmedTool[] = ["square", "ellipse", "ink", "line", "arrow", "polygon", "polyline"];', 'const STROKED: readonly ArmedTool[] = ["square", "ellipse", "ink", "line", "arrow", "polygon", "polyline", "note"];', 'offers each tool what it uses'),
    Mutation('toolbar: recolour a stamp', 'src/lib/toolbar.ts', '  "note", "square", "ellipse", "textbox", "ink", "line", "arrow", "polygon", "polyline",\n];', '  "note", "square", "ellipse", "textbox", "ink", "line", "arrow", "polygon", "polyline", "stamp",\n];', 'offers each tool what it uses'),
    Mutation('toolbar: let a menu run off the left of the window', 'src/lib/toolbar.ts', '    return POPUP_MARGIN - left;', '    return 0;', 'moves a menu that starts left of the window to the left margin'),
    Mutation('toolbar: let a menu run off the right of the window', 'src/lib/toolbar.ts', '  return right > limit ? limit - right : 0;', '  return 0;', 'moves a menu that ends right of the window back by the overhang'),
    Mutation('icons: redraw a vendored icon in place', 'src/lib/icons.ts', '{ d: "m15 18-6-6 6-6" }', '{ d: "m15 18-6-6 6-5" }', 'draws every vendored file exactly as it is vendored'),
    Mutation('icons: name a file that is not vendored', 'src/lib/icons.ts', 'source: "printer",', 'source: "print",', 'draws every vendored file exactly as it is vendored'),
    Mutation('toolbar: leave a form command out of the Form button', 'src/lib/toolbar.ts', '      { id: "edit.tabOrder", label: "Tab through fields in reading order" },\n', '', 'lists under Form what the Form menu lists, in its order'),
    Mutation('menubar: leave a form command in the Form button only', 'src/lib/menubar.ts', '      "edit.fieldProperties",\n      "edit.tabOrder",\n', '      "edit.fieldProperties",\n', 'gives every registered command a menu or a written reason'),
    Mutation('icons: take the picture off a tool', 'src/lib/toolbar.ts', '{ id: "edit.editText", label: "Edit text", icon: "editText" }', '{ id: "edit.editText", label: "Edit text" }', 'gives every button on the tool row a picture of its own'),
    Mutation('error copying: intercept selected interface text', 'src/lib/appcommands.ts', '  if (copyInterfaceText(event, async (text) => navigator.clipboard.writeText(text))) return;', '', 'copies selected error text itself, but still copies PDF text without it'),
    Mutation('error copying: hand selected interface text back to the web view', 'src/lib/keys.ts', '    event.preventDefault();\n    void write(text).catch(() => {});', '', 'writes selected interface text to the clipboard and takes the chord'),
    Mutation('error copying: viewer intercepts native copy', 'src/lib/viewer.ts', '    if (inTextField(event) || nativeCopy(event)) return;', '    if (inTextField(event)) return;', 'does not steal native copying when focus remains on the PDF surface'),
    Mutation('error copying: treat every shortcut as copy', 'src/lib/keys.ts', '  if (!matches("edit.copy", event)) return false;', '', 'copies selected error text itself, but still copies PDF text without it'),
    # The copy chord pressed on a page that has a selection of its own, with
    # interface text still selected elsewhere: the page's, and a press on the
    # page ends the interface selection. `keys.ts` knows a page by the viewer
    # naming its root.
    Mutation("page copying: answer on the selection's type alone", "src/lib/keys.ts", "  if (pageHasSelection?.() && selectionOutside(selection, target)) return false;\n", "", "is the page's when the page has a selection and the selected interface text is elsewhere"),
    Mutation("page copying: take the chord for a page with nothing selected", "src/lib/keys.ts", "  if (pageHasSelection?.() && selectionOutside(selection, target)) return false;", "  if (pageHasSelection && selectionOutside(selection, target)) return false;", "is still the interface text's when the page has nothing selected"),
    Mutation("page copying: take the chord from a selection that reaches into the page", "src/lib/keys.ts", "  if (pageHasSelection?.() && selectionOutside(selection, target)) return false;", "  if (pageHasSelection?.()) return false;", "is the web view's when its selection reaches into the page"),
    Mutation("page copying: read one end of the selection only", "src/lib/keys.ts", "  if (inside(selection.anchorNode) || inside(selection.focusNode)) return false;", "  if (inside(selection.anchorNode)) return false;", "is the web view's when its selection reaches into the page"),
    Mutation("page copying: miss a selection that runs across the page", "src/lib/keys.ts", "  return !selection.containsNode(surface, true);", "  return true;", "is the web view's when its selection reaches into the page"),
    Mutation("page copying: keep answering for a viewer that has gone", "src/lib/keys.ts", "  pageSurfaces.delete(surface);", "", "is about an element a viewer named, and only while it is named"),
    Mutation("page copying: the viewer does not name its root", "src/lib/viewer.ts", "    namePageSurface(root, () => this.hasSelection);\n", "", "copies the page's selection over interface text left selected elsewhere"),
    Mutation("page copying: a destroyed viewer leaves its root named", "src/lib/viewer.ts", "    forgetPageSurface(this.root);\n", "", "copies the page's selection over interface text left selected elsewhere"),
    Mutation("page copying: a press on the page leaves interface text selected", "src/lib/viewer.ts", "    this.root.ownerDocument?.getSelection()?.removeAllRanges();\n", "", "ends a selection of interface text when the page takes a press"),
    Mutation('descender layout: discard validated height', 'src/lib/textlayout.ts', 'run.minimum_height ?? 0', '0', 'keeps the validated descender height when sizing and rounding the default box'),
    # The box the editor opens carries `grow`, which is what lets the worker
    # size it to the typed text up to the room after the line.
    Mutation('grown box: open the box already sized', 'src/lib/textlayout.ts', 'size, wrap: false, font: "auto", grow: true };', 'size, wrap: false, font: "auto", grow: false };', 'opens the box with room to grow and gives it up only when a width is typed'),
    Mutation('grown box: any control counts as sizing it', 'src/lib/textlayout.ts', 'if (input === this.width) this.sized = true;', 'this.sized = true;', 'opens the box with room to grow and gives it up only when a width is typed'),
    Mutation('grown box: a typed width does not size it', 'src/lib/textlayout.ts', 'if (input === this.width) this.sized = true;', '', 'opens the box with room to grow and gives it up only when a width is typed'),
    Mutation('grown box: always report room to grow', 'src/lib/textlayout.ts', 'wrap: this.wrap.checked, grow: !this.sized };', 'wrap: this.wrap.checked, grow: true };', 'opens the box with room to grow and gives it up only when a width is typed'),
    Mutation('grown box: reopen a sized box with room to grow', 'src/lib/textlayout.ts', 'this.wrap.checked = value.wrap; this.sized = !value.grow;', 'this.wrap.checked = value.wrap; this.sized = false;', 'reopens a sized box as sized and an untouched one with room to grow'),
    Mutation('installed font: no explanation on the automatic option', 'src/lib/textlayout.ts', 'if (value === "auto") option.title = AUTOMATIC_ORDER;', '', 'explains the automatic font order, installed copy before Noto, on that option only'),
    Mutation('installed font: the explanation on every option', 'src/lib/textlayout.ts', 'if (value === "auto") option.title = AUTOMATIC_ORDER;', 'option.title = AUTOMATIC_ORDER;', 'explains the automatic font order, installed copy before Noto, on that option only'),
    Mutation('installed font: an order that leaves the installed copy out', 'src/lib/textlayout.ts', 'the same font if it is installed on this computer, otherwise Noto Sans', 'Noto Sans', 'explains the automatic font order, installed copy before Noto, on that option only'),
    Mutation('installed font: Noto named before the installed copy', 'src/lib/textlayout.ts', 'the same font if it is installed on this computer, otherwise Noto Sans', 'Noto Sans, otherwise the same font if it is installed on this computer', 'explains the automatic font order, installed copy before Noto, on that option only'),
    Mutation('text Enter: apply a Cancel button draft', 'src/lib/textedit.ts', ' && event.target === this.input', '', 'Enter on popup buttons leaves their native activation in charge'),
    Mutation('text Enter: ignore input activation', 'src/lib/textedit.ts', 'event.target === this.input', 'event.target === this.apply', 'Enter and Apply commit without a navigable form; composition does not commit'),
    Mutation('text Enter: apply during composition', 'src/lib/textedit.ts', ' && !event.isComposing', '', 'Enter and Apply commit without a navigable form; composition does not commit'),
    Mutation('text Done: lose viewer focus', "src/lib/textedit.ts", 'this.host.focus({ preventScroll: true });', '', 'Done waits for the draft before closing and returning focus to the viewer'),
    Mutation('text Done: scroll on return', "src/lib/textedit.ts", 'this.host.focus({ preventScroll: true });', 'this.host.focus();', 'Done waits for the draft before closing and returning focus to the viewer'),
    Mutation('text outlines: never refreshed', 'src/lib/textedit.ts', '    this.layout();\n    this.refreshOutlines();\n', '    this.layout();\n', 'moves each outline to where the pending edits leave its run'),
    Mutation('text outlines: a stale answer wins', 'src/lib/textedit.ts', 'if (this.disposed || generation !== this.outlineGeneration', 'if (this.disposed', 'moves each outline to where the pending edits leave its run'),
    Mutation("text outlines: another revision's answer", 'src/lib/textedit.ts', '\n        || fresh.revision.join() !== this.source.revision.join()) return;', ') return;', 'moves each outline to where the pending edits leave its run'),
    Mutation('text Done: reclaim a disposed editor', "src/lib/textedit.ts", 'if (this.disposed) return;\n      this.close();', 'this.close();', 'Done waits for the draft before closing and returning focus to the viewer'),
    Mutation('text Done: close before saving', "src/lib/textedit.ts", 'this.commit(); void this.settle().then(() => {', 'this.commit(); this.close(); void this.settle().then(() => {', 'Done waits for the draft before closing and returning focus to the viewer'),
    Mutation('text Done: discard the draft', "src/lib/textedit.ts", 'this.commit(); void this.settle().then(() => {', 'void this.settle().then(() => {', 'Done waits for the draft before closing and returning focus to the viewer'),
    Mutation('text cancel: focus an offscreen target', 'src/lib/textedit.ts', 'selected?.hidden ? this.host : selected', 'selected', 'returns focus to the PDF when the selected text has scrolled out of view'),
    Mutation('text focus: start with the first source run', 'src/lib/textedit.ts', '(this.buttons.find((button) => !button.hidden) ?? host)', '(this.buttons[0] ?? host)', 'starts keyboard navigation at the first visible target or the PDF'),
    Mutation('text focus: lose the PDF fallback', 'src/lib/textedit.ts', '(this.buttons.find((button) => !button.hidden) ?? host).focus', '(this.buttons.find((button) => !button.hidden))?.focus', 'starts keyboard navigation at the first visible target or the PDF'),
    Mutation('text visibility: omit right edge', 'src/lib/textedit.ts', ' || box.right <= 0', '', 'removes fully offscreen targets from keyboard navigation and restores them on return'),
    Mutation('text visibility: omit bottom edge', 'src/lib/textedit.ts', ' || box.bottom <= 0', '', 'removes fully offscreen targets from keyboard navigation and restores them on return'),
    Mutation('text visibility: omit left edge', 'src/lib/textedit.ts', ' || box.left >= width', '', 'removes fully offscreen targets from keyboard navigation and restores them on return'),
    Mutation('text visibility: omit top edge', 'src/lib/textedit.ts', ' || box.top >= height', '', 'removes fully offscreen targets from keyboard navigation and restores them on return'),
    Mutation('text cancel: omit focus restoration', 'src/lib/textedit.ts', 'target?.focus({ preventScroll: true });', '', 'returns focus to the selected text target when cancelling without scrolling'),
    Mutation('text cancel: focus the first target', 'src/lib/textedit.ts', 'this.buttons[this.source.runs.indexOf(this.active)]', 'this.buttons[0]', 'returns focus to the selected text target when cancelling without scrolling'),
    Mutation('text cancel: scroll while restoring focus', 'src/lib/textedit.ts', 'target?.focus({ preventScroll: true });', 'target?.focus();', 'returns focus to the selected text target when cancelling without scrolling'),
    Mutation('rotation fit: infer the sheet after reordering pages', 'src/lib/viewer.ts', 'this.applyFit(false, landing);', 'this.applyFit();', 'fits the retained sheet after its page order changes'),
    Mutation('rotation fit: infer the sheet after rotating the layout', 'src/lib/viewer.ts', 'this.applyFit(false, page);\n\n    this.scrollTop = Math.max(', 'this.applyFit();\n\n    this.scrollTop = Math.max(', 'keeps fitting the visible sheet through every view turn in a mixed-size document'),
    Mutation('rotation fit: infer the sheet after turning a preceding page', 'src/lib/viewer.ts', 'this.applyFit(false, anchor);', 'this.applyFit();', 'keeps fitting the reading page when a preceding page is rotated'),
    Mutation('restoration: discard pending absolute point', 'src/lib/viewer.ts', 'const pending = this.restoredPoint;', 'const pending = null;', 'restores an absolute point after the remembered page size arrives'),
    Mutation('restoration: fit the first page', 'src/lib/viewer.ts', 'this.applyFit();\n    this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);', 'this.applyFit(false, 0);\n    this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);', "restores fit using the remembered page rather than the new viewer's first page"),
    Mutation('restoration: fit the top anchor instead of the visible sheet', 'src/lib/viewer.ts', 'this.applyFit();\n    this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);', 'this.applyFit(false, page);\n    this.scrollTo(this.scroller.pageTopOf(page) + offset * this.zoom);', 'restores a fitted last page when the viewport starts on the previous sheet'),
    Mutation('restoration: replay after navigation', 'src/lib/viewer.ts', 'pending.scroll === this.scrollTop &&', 'true &&', 'does not replay a pending restore after the reader navigates elsewhere'),
    Mutation('restoration: omit offscreen geometry request', 'src/lib/viewer.ts', 'if (this.restoredPoint) this.requestText(page);', '', 'loads a restored page even when the saved offset exceeds its estimated height'),
    Mutation('direction: discard character directions', 'src/lib/text.ts', 'text.char_turns?.[index] ?? 0', '0', 'keeps mixed orthogonal lines whole and copy order invariant under every view turn'),
    Mutation('direction: drop directions when rotating cached text', 'src/lib/text.ts', 'if (text.char_turns) view.char_turns = text.char_turns;', '/* direction omitted */', 'keeps mixed orthogonal lines whole and copy order invariant under every view turn'),
    Mutation('direction: reverse negative caret direction', 'src/lib/text.ts', 'turns >= 2 ? position < middle : position > middle', 'turns >= 2 ? position > middle : position > middle', 'places the caret before and after a glyph in its own displayed direction'),
    Mutation('direction: skip orientation partition', 'src/lib/reading.ts', 'if (!text.char_turns?.length) return alignedFragments(text, axes, gap);', 'return alignedFragments(text, axes, gap);', 'keeps mixed orthogonal lines whole and copy order invariant under every view turn'),
    Mutation('direction: lose original indices after partition', 'src/lib/reading.ts', 'original.push(indices[index]!)', 'original.push(index)', 'keeps mixed orthogonal lines whole and copy order invariant under every view turn'),
    Mutation('direction: band every direction as upright', 'src/lib/reading.ts', 'const ownAxes = axesFor(turns);\n    for (const fragment', 'const ownAxes = axesFor(0);\n    for (const fragment', 'keeps mixed orthogonal lines whole and copy order invariant under every view turn'),
    Mutation('direction: cut reversed paragraphs in page frame', 'src/lib/reading.ts', 'if (text.char_turns?.length) {', 'if (false) {', 'keeps mixed orthogonal lines whole and copy order invariant under every view turn'),
    Mutation('direction: drop direction at tag ownership', 'src/lib/reading.ts', 'if (fragment.turns !== undefined) part.turns = fragment.turns;', '/* direction omitted */', 'retains character direction when tagged ownership splits a fragment'),
    Mutation('text editor: refuse mapped en dash', 'src/lib/textedit.ts', "/[\\x00-\\x1f\\x7f-\\x9f\\ud800-\\udfff\\u2028\\u2029]/u", "/[\\x00-\\x1f\\x7f-\\x9f\\ud800-\\udfff\\u2028\\u2029\\u2013]/u", "sends punctuation unchanged and refuses controls"),
    Mutation('text editor: refuse mapped CFF punctuation', 'src/lib/textedit.ts', "/[\\x00-\\x1f\\x7f-\\x9f\\ud800-\\udfff\\u2028\\u2029]/u", "/[\\x00-\\x1f\\x7f-\\x9f\\ud800-\\udfff\\u2028\\u2029\\u2018\\u2019\\u2212]/u", "sends punctuation unchanged and refuses controls"),
    Mutation('text editor: scroll overlay on input focus', "src/lib/textedit.ts", 'this.input.focus({ preventScroll: true });', 'this.input.focus();', "focuses targets and the input without scrolling their overlay"),
    Mutation('text editor: scroll overlay on first target focus', "src/lib/textedit.ts", '(this.buttons.find((button) => !button.hidden) ?? host).focus({ preventScroll: true });', '(this.buttons.find((button) => !button.hidden) ?? host).focus();', "focuses targets and the input without scrolling their overlay"),
    Mutation("text editor: undo invalidates content", "src/lib/textedit.ts", "...old.keys(), ...next.keys()", "...next.keys()", "invalidates added, changed and undone pages without invalidating reordered changes"),
    Mutation("text editor: failed draft blocks save", "src/lib/textedit.ts", "if (this.failure) throw this.failure;", "if (false) throw this.failure;", "keeps invalid and refused drafts from passing the save drain"),
    Mutation("text editor: original source address", "src/lib/textedit.ts", "original: run.text, replacement", "original: this.accepted, replacement", "undo refreshes a clean input and restoration keeps the original source address"),

    Mutation("assurance: signature dimensions before decoding", "src/lib/signature.ts", "const expected = signatureDimensions(bytes);", "const expected = {width:1,height:1};", "refuses bombs, animation, truncation, duplicate frames and late dimension changes before decode"),
    Mutation("assurance: signature pixel ceiling", "src/lib/signature.ts", "width * height > MAX_IMPORT_PIXELS", "width * height > MAX_IMPORT_PIXELS * 2", "refuses bombs, animation, truncation, duplicate frames and late dimension changes before decode"),
    Mutation("assurance: signature late dimension change", "src/lib/signature.ts", "if (marker === 0xdc) throw invalid();", "if (false) throw invalid();", "refuses bombs, animation, truncation, duplicate frames and late dimension changes before decode"),
    Mutation("assurance: signature decoded dimensions", "src/lib/signature.ts", "image.width !== expected.width || image.height !== expected.height", "false", "closes a decoder result that disagrees with its header"),
    Mutation('signature headers: a longer side than the tool', 'src/lib/signature.ts', 'width > 8192 || height > 8192', 'width > 8193 || height > 8193', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a shorter side than the tool', 'src/lib/signature.ts', 'width > 8192 || height > 8192', 'width > 8191 || height > 8191', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: more pixels than the tool', 'src/lib/signature.ts', 'const MAX_IMPORT_PIXELS = 8 * 1024 * 1024;', 'const MAX_IMPORT_PIXELS = 8 * 1024 * 1024 + 4096;', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: fewer pixels than the tool', 'src/lib/signature.ts', 'const MAX_IMPORT_PIXELS = 8 * 1024 * 1024;', 'const MAX_IMPORT_PIXELS = 8 * 1024 * 1024 - 1;', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a larger file than the tool', 'src/lib/signature.ts', 'const MAX_IMPORT_BYTES = 10 * 1024 * 1024;', 'const MAX_IMPORT_BYTES = 10 * 1024 * 1024 + 1;', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a smaller file than the tool', 'src/lib/signature.ts', 'const MAX_IMPORT_BYTES = 10 * 1024 * 1024;', 'const MAX_IMPORT_BYTES = 10 * 1024 * 1024 - 1;', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a lossless JPEG', 'src/lib/signature.ts', '![0xc0,0xc1,0xc2].includes(marker)', '![0xc0,0xc1,0xc2,0xc3].includes(marker)', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: no progressive JPEG', 'src/lib/signature.ts', '![0xc0,0xc1,0xc2].includes(marker)', '![0xc0,0xc1].includes(marker)', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a twelve-bit JPEG', 'src/lib/signature.ts', ' || bytes[offset+2] !== 8) throw invalid();', ') throw invalid();', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a second JPEG frame', 'src/lib/signature.ts', '.includes(marker) || size || length < 8', '.includes(marker) || length < 8', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a JPEG scan before its frame', 'src/lib/signature.ts', 'if (marker === 0xda) { if (!size) throw invalid(); scan = true; }', 'if (marker === 0xda) { scan = true; }', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a second PNG header', 'src/lib/signature.ts', '\n          || (offset !== 8 && kind === 0x49484452)) throw invalid();', ') throw invalid();', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a PNG end that carries data', 'src/lib/signature.ts', 'end = length === 0 && offset === bytes.length;', 'end = offset === bytes.length;', 'reads the shared headers as the command-line tool reads them'),
    Mutation('signature headers: a PNG without image data', 'src/lib/signature.ts', 'if (!data || !end) throw invalid();', 'if (!end) throw invalid();', 'reads the shared headers as the command-line tool reads them'),
    Mutation("assurance: signature migration readback", "src/lib/signaturestore.ts", "if (!saved || !same(saved, image))", "if (false)", "keeps plaintext on inaccessible, conflicting, corrupt or concurrently changed storage"),
    Mutation("assurance: signature migration conflict", "src/lib/signaturestore.ts", "if (existing && !same(existing, image))", "if (false)", "keeps plaintext on inaccessible, conflicting, corrupt or concurrently changed storage"),
    Mutation("assurance: signature migration race", "src/lib/signaturestore.ts", "if (storage.getItem(SIGNATURE_KEY) !== raw)", "if (false)", "keeps plaintext on inaccessible, conflicting, corrupt or concurrently changed storage"),
    Mutation("assurance: signed save consent", "src/lib/signedsave.ts", "if (message && !await confirm(message))", "if (false)", "continues only on consent and avoids a prompt for known unsigned documents"),
    Mutation("assurance: signed save allowed form filling", "src/lib/signedsave.ts", "signature.signed || signature.certification > 0", "signature.signed && signature.certification < 2", "warns for signed documents and every certification permission, including allowed form filling"),
    Mutation("assurance: signed save incomplete status", "src/lib/signedsave.ts", "!limits || limits.locked || limits.unreadable > 0", "!limits || limits.locked || false", "requires consent when signature enumeration is incomplete or fails"),
    Mutation("assurance: save write guard", "src/lib/edits.ts", "  async save(source: string): Promise<void> {\n    await this.beforeWrite();", "  async save(source: string): Promise<void> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("assurance: saveCopy write guard", "src/lib/edits.ts", "  async saveCopy(source: string, path: string): Promise<Copied> {\n    await this.beforeWrite();", "  async saveCopy(source: string, path: string): Promise<Copied> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("assurance: redactCopy write guard", "src/lib/edits.ts", "  async redactCopy(source: string, path: string, fill: Fill): Promise<Applied> {\n    await this.beforeWrite();", "  async redactCopy(source: string, path: string, fill: Fill): Promise<Applied> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("assurance: redactRasterCopy write guard", "src/lib/edits.ts", "  async redactRasterCopy(source: string, path: string, fill: Fill): Promise<Applied> {\n    await this.beforeWrite();", "  async redactRasterCopy(source: string, path: string, fill: Fill): Promise<Applied> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("redact fill: send black with an image-only copy whatever was chosen", "src/lib/edits.ts", 'return await call("redact_raster_copy", { doc: this.doc, source, path, fill });', 'return await call("redact_raster_copy", { doc: this.doc, source, path, fill: "black" });', "sends the source, the destination and the fill when an image-only redacted copy is made"),
    Mutation("assurance: ocrCopy write guard", "src/lib/edits.ts", "    language: string | null,\n  ): Promise<Recognised> {\n    await this.beforeWrite();", "    language: string | null,\n  ): Promise<Recognised> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("assurance: protectCopy write guard", "src/lib/edits.ts", "  async protectCopy(source: string, path: string, password: string | null): Promise<Copied> {\n    await this.beforeWrite();", "  async protectCopy(source: string, path: string, password: string | null): Promise<Copied> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("protect: send no password", "src/lib/edits.ts", 'return await call("protect_copy", { doc: this.doc, source, path, password });', 'return await call("protect_copy", { doc: this.doc, source, path, password: null });', "sends the new password with a protected copy, and null to remove one"),
    Mutation("protect: accept two passwords that differ", "src/lib/protect.ts", '  if (first !== second) return "The two passwords are not the same.";\n', "", "refuses two that differ, by case as well"),
    Mutation("protect: count characters instead of bytes", "src/lib/protect.ts", "const bytes = new TextEncoder().encode(first).length;", "const bytes = first.length;", "counts the length in bytes, which is what the file format reads"),
    Mutation("protect: accept an empty password", "src/lib/protect.ts", '  if (first === "") return "Type a password.";\n', "", "refuses an empty one before comparing anything"),
    Mutation("protect: accept the C1 controls", "src/lib/protect.ts", "/[\\u0000-\\u001f\\u007f-\\u009f]/.test(first)", "/[\\u0000-\\u001f]/.test(first)", "refuses a character nobody can type back"),
    Mutation("protect: say an unprotected copy needs a password", "src/lib/protect.ts", ": `Saved ${basename(path)}. It opens without a password.`;", ": `Saved ${basename(path)}. It needs the new password to open.`;", "says whether the copy needs a password, by its name"),
    Mutation("protect: drop the changed-source warning", "src/lib/protect.ts", "return changed ? `${said} ${changed}` : said;", "return said;", "adds what a copy from a changed source is told"),
    Mutation("protect: name both copies protected", "src/lib/protect.ts", '${set ? "protected" : "unprotected"}.pdf', '${set ? "protected" : "protected"}.pdf', "names the copy after what was done to it"),
    Mutation("protect: call every character beyond ASCII fine", "src/lib/protect.ts", "return /[^\\u0000-\\u007f]/.test(password);", "return /[^\\u0000-\\uffff]/.test(password);", "is about characters Preview does not accept, not about length"),
    Mutation("new password: save without judging", "src/lib/newpassworddialog.ts", "    if (why) {\n      this.problem.textContent = why;\n      return;\n    }\n", "", "stays open and says why when the two differ, then takes the correction"),
    Mutation("new password: judge the first field against itself", "src/lib/newpassworddialog.ts", "const why = judge(this.first.value, this.second.value);", "const why = judge(this.first.value, this.first.value);", "stays open and says why when the two differ, then takes the correction"),
    Mutation("new password: keep the repeat after closing", "src/lib/newpassworddialog.ts", '    this.second.value = "";\n', "", "clears both fields after a password was given"),
    Mutation("new password: keep the password after closing", "src/lib/newpassworddialog.ts", '    this.first.value = "";\n', "", "clears both fields after a password was given"),
    Mutation("new password: keep the last refusal", "src/lib/newpassworddialog.ts", '    this.note.textContent = ADVICE;\n    this.problem.textContent = "";\n', "    this.note.textContent = ADVICE;\n", "starts each question without the last one's refusal"),
    Mutation("new password: never warn about Preview", "src/lib/newpassworddialog.ts", 'beyondAscii(this.first.value) ? NOT_ASCII : "";', '"";', "warns about Preview while the password has a character it does not accept"),
    Mutation("new password: stack a second question", "src/lib/newpassworddialog.ts", "  ask(name: string): Promise<string | null> {\n    this.settle(null);\n", "  ask(name: string): Promise<string | null> {\n", "settles the first question when a second is asked"),
    Mutation("appcommands: unprotect asks for a new password", "src/lib/appcommands.ts", "run: () => actions.unprotectCopy(),", "run: () => actions.protectCopy(),", "sets and removes a password through two commands, on any open document"),
    Mutation("pictures: strip a picture's ending anywhere in the name", "src/lib/pictures.ts", '.replace(/\\.(png|jpe?g)$/i, "")', '.replace(/\\.(png|jpe?g)/i, "")', "keeps a name whose ending is not a picture's"),
    Mutation("pictures: keep the picture's ending in the name", "src/lib/pictures.ts", '.replace(/\\.(png|jpe?g)$/i, "")', '.replace(/\\.(png)$/i, "")', "takes the first picture's name and makes it a PDF"),
    Mutation("pictures: say 1 pages", "src/lib/pictures.ts", 'made.pages === 1 ? "1 page"', 'made.pages === 0 ? "1 page"', "names the document and counts its pages"),
    Mutation("appcommands: pictures need an open document", "src/lib/appcommands.ts", "      enabled: available,\n      run: () => actions.fromPictures(),", "      enabled: withDocument,\n      run: () => actions.fromPictures(),", "leaves only the commands that genuinely need no document"),
    Mutation("recognise: count pages given a layer only", "src/lib/recognise.ts", "read.pages.length +\n    read.alreadyText.length +\n    read.nothingRead.length +\n    read.refused.length +\n    read.tooLarge.length;", "read.pages.length;", "names the pages that got no layer, each kind in its own sentence"),
    Mutation("recognise: report the last page's words", "src/lib/recognise.ts", "read.pages.reduce((sum, page) => sum + page.words, 0);", "read.pages.reduce((_sum, page) => page.words, 0);", "adds the words of every page given a layer"),
    Mutation("recognise: list every page", "src/lib/recognise.ts", "const SHOWN = 8;", "const SHOWN = 8000;", "does not list three hundred pages"),
    Mutation("recognise: keep the extension in the name", "src/lib/recognise.ts", '.replace(/\\.pdf$/i, "")', '.replace(/\\.pdf$/, "")', "keeps the name and says what the copy is"),
    Mutation("recognise: listen for another event", "src/lib/recognise.ts", 'export const PROGRESS_EVENT = "tpdf://ocr-progress";', 'export const PROGRESS_EVENT = "tpdf://ocr";', "listens for the event the backend emits"),
    Mutation("redact list: say nothing about a drawing that goes", "src/lib/redactlist.ts", "  if (drawings > 0) {", "  if (drawings > 9000) {", "says a drawing inside the region goes, and counts them"),
    Mutation("redact list: say nothing about a line that is cut", "src/lib/redactlist.ts", "  if (said.length === 0) return cuts > 0 ? `Cuts ${cut}` : \"\";", "  if (said.length === 0) return \"\";", "says a line that crosses the region is cut at its edge, and counts them"),
    Mutation("redact list: count the drawings as cuts", "src/lib/redactlist.ts", "  const cuts = (plan?.cuts?.length ?? 0) +", "  const cuts = (plan?.paths?.length ?? 0) +", "says a line that crosses the region is cut at its edge, and counts them"),
    Mutation("redact list: leave a block's drawings out of what a region takes", "src/lib/redactlist.ts", " + (plan?.form_paths?.whole?.length ?? 0);", ";", "counts a drawing inside a reusable block with the page's own, removed or cut"),
    Mutation("redact list: leave a block's cut lines out of what a region cuts", "src/lib/redactlist.ts", " + (plan?.form_paths?.cuts?.length ?? 0);", ";", "counts a drawing inside a reusable block with the page's own, removed or cut"),
    Mutation("arrange: align left to the page's edge", "src/lib/arrange.ts", "      return each((quad) => moved(quad, first.left - quad.left, 0));", "      return each((quad) => moved(quad, -quad.left, 0));", "lines an edge of every mark up with the first mark's, and keeps each size"),
    Mutation("arrange: align right by the left edges", "src/lib/arrange.ts", "      return each((quad) => moved(quad, first.right - quad.right, 0));", "      return each((quad) => moved(quad, first.left - quad.left, 0));", "lines an edge of every mark up with the first mark's, and keeps each size"),
    Mutation("arrange: align top across instead of down", "src/lib/arrange.ts", "      return each((quad) => moved(quad, 0, first.top - quad.top));", "      return each((quad) => moved(quad, first.top - quad.top, 0));", "lines an edge of every mark up with the first mark's, and keeps each size"),
    Mutation("arrange: align bottom by the top edges", "src/lib/arrange.ts", "      return each((quad) => moved(quad, 0, first.bottom - quad.bottom));", "      return each((quad) => moved(quad, 0, first.top - quad.top));", "lines an edge of every mark up with the first mark's, and keeps each size"),
    Mutation("arrange: move a centre the whole way instead of half", "src/lib/arrange.ts", "(first.left + first.right - quad.left - quad.right) / 2, 0));", "(first.left + first.right - quad.left - quad.right), 0));", "lines the centres up with the first mark's"),
    Mutation("arrange: line middles up across instead of down", "src/lib/arrange.ts", "      return each((quad) => moved(quad, 0, (first.top + first.bottom - quad.top - quad.bottom) / 2));", "      return each((quad) => moved(quad, (first.top + first.bottom - quad.top - quad.bottom) / 2, 0));", "lines the centres up with the first mark's"),
    Mutation("arrange: give the same width the first mark's height", "src/lib/arrange.ts", "      return each((quad) => ({ ...quad, right: quad.left + width(first) }));", "      return each((quad) => ({ ...quad, right: quad.left + height(first) }));", "gives every mark the first mark's width, height or both, from its own corner"),
    Mutation("arrange: give the same height from the first mark's top", "src/lib/arrange.ts", "      return each((quad) => ({ ...quad, bottom: quad.top + height(first) }));", "      return each((quad) => ({ ...quad, bottom: first.top + height(first) }));", "gives every mark the first mark's width, height or both, from its own corner"),
    Mutation("arrange: make the same size the same width only", "src/lib/arrange.ts", "      return each((quad) => ({ ...quad, right: quad.left + width(first), bottom: quad.top + height(first) }));", "      return each((quad) => ({ ...quad, right: quad.left + width(first) }));", "gives every mark the first mark's width, height or both, from its own corner"),
    Mutation("arrange: space the marks in the order they were picked", "src/lib/arrange.ts", "    .sort((a, b) => a.mid - b.mid || a.at - b.at)\n", "", "spaces the marks between the outer two evenly, whatever order they were picked in"),
    Mutation("arrange: space the marks with no gap", "src/lib/arrange.ts", "    at += end(quad) - start(quad) + gap;", "    at += end(quad) - start(quad);", "spaces the marks between the outer two evenly, whatever order they were picked in"),
    Mutation("arrange: spread down when asked across", "src/lib/arrange.ts", "      return spread(quads, true).map((quad) => onPage(quad, page));", "      return spread(quads, false).map((quad) => onPage(quad, page));", "spaces the marks between the outer two evenly, whatever order they were picked in"),
    Mutation("arrange: divide the space by the number of marks", "src/lib/arrange.ts", "  const gap = (end(last) - start(first) - taken) / (quads.length - 1);", "  const gap = (end(last) - start(first) - taken) / quads.length;", "spaces the marks between the outer two evenly, whatever order they were picked in"),
    Mutation("arrange: centre each mark on the page by itself", "src/lib/arrange.ts", "      const left = Math.min(...quads.map((quad) => quad.left));", "      const left = Math.max(...quads.map((quad) => quad.left));", "centres the picked marks on the page as one block"),
    Mutation("arrange: centre down when asked across", "src/lib/arrange.ts", "      const dx = how === \"pageCenter\" ? (page.width - left - right) / 2 : 0;", "      const dx = how === \"pageMiddle\" ? (page.width - left - right) / 2 : 0;", "centres the picked marks on the page as one block"),
    Mutation("arrange: centre across when asked down", "src/lib/arrange.ts", "      const dy = how === \"pageMiddle\" ? (page.height - top - bottom) / 2 : 0;", "      const dy = 0;", "centres the picked marks on the page as one block"),
    Mutation("arrange: let a mark leave the page on the left", "src/lib/arrange.ts", "  const dx = quad.left < 0 ? -quad.left : quad.right > page.width ? page.width - quad.right : 0;", "  const dx = quad.right > page.width ? page.width - quad.right : 0;", "keeps every mark on the page"),
    Mutation("arrange: let a mark leave the page at the bottom", "src/lib/arrange.ts", "quad.bottom > page.height ? page.height - quad.bottom : 0;", "0;", "keeps every mark on the page"),
    Mutation("arrange: let a mark leave the page on the right", "src/lib/arrange.ts", "quad.right > page.width ? page.width - quad.right : 0;", "0;", "keeps every mark on the page"),
    Mutation("arrange: arrange rectangles that are not finite", "src/lib/arrange.ts", "  if (!first || !sound) return [...quads];", "  if (!first) return [...quads];", "changes nothing with fewer marks than the arrangement needs, or one that is not finite"),
    Mutation("arrange: ask for two marks to space", "src/lib/arrange.ts", "  if (how === \"distributeAcross\" || how === \"distributeDown\") return 3;", "  if (how === \"distributeAcross\") return 3;", "says how many marks each arrangement needs"),
    Mutation("arrange: ask for two marks to centre on the page", "src/lib/arrange.ts", "  if (how === \"pageCenter\" || how === \"pageMiddle\") return 1;", "  if (how === \"pageCenter\") return 1;", "says how many marks each arrangement needs"),
    Mutation("arrange: call a rectangle moved by rounding moved", "src/lib/arrange.ts", "Math.abs(x - y) > 0.01;", "Math.abs(x - y) > 0;", "tells a moved rectangle from one within rounding of where it was"),
    Mutation("arrange: miss a rectangle whose bottom alone moved", "src/lib/arrange.ts", "far(a.right, b.right) || far(a.bottom, b.bottom);", "far(a.right, b.right);", "tells a moved rectangle from one within rounding of where it was"),
    Mutation("picked: add a mark that is picked already", "src/lib/arrange.ts", "    this.ids = this.has(id) ? this.ids.filter((other) => other !== id) : [...this.ids, id];", "    this.ids = [...this.ids, id];", "keeps the order marks were picked in, and takes one out when it is picked again"),
    Mutation("picked: put the newest pick first", "src/lib/arrange.ts", ": [...this.ids, id];", ": [id, ...this.ids];", "keeps the order marks were picked in, and takes one out when it is picked again"),
    Mutation("picked: keep picking across pages", "src/lib/arrange.ts", "    if (this.page !== page) {\n      this.only(id, page);\n      return;\n    }\n", "", "starts again on another page"),
    Mutation("picked: keep a mark that is gone", "src/lib/arrange.ts", "    this.ids = this.ids.filter(live);", "    this.ids = this.ids.filter(() => true);", "picks one alone, clears, and drops marks that are gone"),
    Mutation("picked viewer: add to the selection without Shift", "src/lib/viewer.ts", "        if (event.shiftKey) {\n          event.preventDefault();\n          this.arranging.toggle(own.id, slot);", "        if (true) {\n          event.preventDefault();\n          this.arranging.toggle(own.id, slot);", "opens no note and moves nothing on a press with Shift"),
    Mutation("picked viewer: open the note on a press with Shift", "src/lib/viewer.ts", "          this.arranging.toggle(own.id, slot);\n          this.pickedChanged();\n          return;", "          this.arranging.toggle(own.id, slot);\n          this.pickedChanged();", "opens no note and moves nothing on a press with Shift"),
    Mutation("picked viewer: pick a mark made of words", "src/lib/viewer.ts", "    if (own && isResizable(own.kind) && !mark) {", "    if (own && !mark) {", "does not pick a mark that is made of the words under it"),
    Mutation("picked viewer: drop the others when one of several is pressed", "src/lib/viewer.ts", "        if (!(this.arranging.count > 1 && this.arranging.has(own.id))) {", "        if (true) {", "keeps several picked when one of them is pressed, and drops them on a press elsewhere"),
    Mutation("picked viewer: keep the selection on a press elsewhere", "src/lib/viewer.ts", "    } else if (this.arranging.count > 0) {\n      this.arranging.clear();\n      this.pickedChanged();\n    }", "    }", "keeps several picked when one of them is pressed, and drops them on a press elsewhere"),
    Mutation("picked viewer: keep a mark the model dropped", "src/lib/viewer.ts", "    this.arranging.keep((id) => live.has(id));", "    this.arranging.keep(() => true);", "forgets a picked mark the model no longer has"),
    Mutation("picked viewer: say nothing when a dropped mark leaves the selection", "src/lib/viewer.ts", "    if (this.arranging.count !== before) this.opts.onPicked?.(this.arranging.count, this.morePickable());", "", "forgets a picked mark the model no longer has"),
    Mutation("picked viewer: change the selection without telling the host", "src/lib/viewer.ts", "  private pickedChanged(): void {\n    this.opts.onPicked?.(this.arranging.count, this.morePickable());", "  private pickedChanged(): void {", "picks one on a plain press and adds others with Shift, in the order pressed"),
    Mutation("picked viewer: leave the picked page at its old slot after a page edit", "src/lib/viewer.ts", "    this.arranging.repage((slot) => after.slotFrom(before, slot));\n", "", "keeps_what_is_picked_on_its_page_when_a_page_above_is_deleted"),
    Mutation("picked viewer: add a mark from the page that took the picked page's slot", "src/lib/viewer.ts", "    this.arranging.repage((slot) => after.slotFrom(before, slot));\n", "", "does_not_add_a_mark_from_the_page_that_took_the_picked_page's_slot"),
    Mutation("picked: keep marks picked on a page that is gone", "src/lib/arrange.ts", "    if (slot === undefined) this.clear();\n    else this.page = slot;", "    if (slot !== undefined) this.page = slot;", "follows its page to another slot, and goes with a page that is gone"),
    Mutation("picked: ask where no page went with nothing picked", "src/lib/arrange.ts", "    if (this.page === null) return;\n    const slot = now(this.page);", "    const slot = now(this.page ?? 0);", "follows its page to another slot, and goes with a page that is gone"),
    Mutation("ink viewer: leave a drawing at its old slot after a page edit", "src/lib/viewer.ts", "      if (slot === undefined) this.inking = null;\n      else drawing.slot = slot;", "      if (slot === undefined) this.inking = null;", "ink_started_before_a_deletion_above_is_not_written_to_the_page_that_moved_in"),
    Mutation("ink viewer: keep a drawing whose page was deleted", "src/lib/viewer.ts", "      if (slot === undefined) this.inking = null;\n      else drawing.slot = slot;", "      if (slot !== undefined) drawing.slot = slot;", "drops ink whose page was deleted before it was finished"),
    Mutation("match marks: hand rectangles read before a page move to the page now in the slot", "src/lib/viewer.ts", "      if (!this.pages.sameOrder(pages)) return null;\n", "", "matches_asked_for_before_a_page_move_are_not_marked_on_the_page_that_took_the_slot"),
    Mutation("field panel: change the field with that id in another document", "src/lib/fieldprops.ts", "  if (deps.model() !== model) return false;\n", "", "changeProperties_made_after_the_document_changed_changes_nothing"),
    Mutation("saved fields: send the model a saved field's id as a mark to remove", "src/lib/savedfields.ts", "  return field ? { field } : null;", "  return field ? { field } : { mark: id };", "removes a mark as a mark and a saved field as a field, and never sends the model a field's id"),
    Mutation("saved fields: remove every mark as a field", "src/lib/savedfields.ts", "  if (!isSaved(id)) return { mark: id };\n", "", "removes a mark as a mark and a saved field as a field, and never sends the model a field's id"),
    Mutation("arrange viewer: report the marks that did not move too", "src/lib/viewer.ts", "      return to && differs(one.quad, to) ? [{ mark: one.id, rect: this.fileRectOn(slot, to) }] : [];", "      return to ? [{ mark: one.id, rect: this.fileRectOn(slot, to) }] : [];", "reports each mark that moved, with its new rectangle, as one named gesture"),
    Mutation("arrange viewer: reuse the last gesture's name", "src/lib/viewer.ts", "    if (moves.length === 0) return false;\n    this.opts.onMarksArranged?.(moves, ++this.sweeps);", "    if (moves.length === 0) return false;\n    this.opts.onMarksArranged?.(moves, this.sweeps);", "reports each mark that moved, with its new rectangle, as one named gesture"),
    Mutation("arrange viewer: journal an arrangement that moved nothing", "src/lib/viewer.ts", "    if (moves.length === 0) return false;\n    this.opts.onMarksArranged", "    this.opts.onMarksArranged", "says nothing was done when too few are picked or they are already in place"),
    Mutation("arrange viewer: report the laid-out rectangle on a turned page", "src/lib/viewer.ts", "? [{ mark: one.id, rect: this.fileRectOn(slot, to) }] : [];", "? [{ mark: one.id, rect: [to.left, to.top, to.right, to.bottom] as [number, number, number, number] }] : [];", "aligns what the reader sees as left on a turned page, and reports it in the file's space"),
    Mutation("arrange viewer: arrange against the unturned page's size", "src/lib/viewer.ts", "    const now = arrange(placed.map((one) => one.quad), how, this.laidSize(slot));", "    const now = arrange(placed.map((one) => one.quad), how, this.pageSize(slot) as unknown as { width: number; height: number });", "aligns what the reader sees as left on a turned page, and reports it in the file's space"),
    Mutation("arrange command: offer every arrangement with one mark picked", "src/lib/appcommands.ts", "    const why = () => lacks(how, actions.pickedMarks());", "    const why = () => lacks(\"pageCenter\", actions.pickedMarks());", "offers each arrangement once enough marks are picked for it, and runs that one"),
    Mutation("arrange command: offer them with no document", "src/lib/appcommands.ts", "      enabled: () => withDocument() && why() === null,", "      enabled: () => why() === null,", "offers each arrangement once enough marks are picked for it, and runs that one"),
    Mutation("arrange command: run align left for every one of them", "src/lib/appcommands.ts", "    run: () => actions.arrange(how),", "    run: () => actions.arrange(\"left\"),", "offers each arrangement once enough marks are picked for it, and runs that one"),
    Mutation("arrange: send the moves without the gesture's name", "src/lib/edits.ts", "    return this.adopt(await call(\"annot_arrange\", { doc: this.doc, moves, sweep }));", "    return this.adopt(await call(\"annot_arrange\", { doc: this.doc, moves, sweep: 0 }));", "sends an arrangement as every move and the gesture's name, in one call"),
    Mutation("saved fields: call the first saved field a mark", "src/lib/savedfields.ts", "  return id >= SAVED_BASE;", "  return id > SAVED_BASE;", "shows each field on the page it is on, under an id of its own, with its kind and name"),
    Mutation("saved fields: show a removed field", "src/lib/savedfields.ts", "    if (page === undefined || edit?.removed) return [];", "    if (page === undefined) return [];", "shows a field where it was moved to and under its new name, and not one that was removed"),
    Mutation("saved fields: show a field whose page is gone", "src/lib/savedfields.ts", "    if (page === undefined || edit?.removed) return [];", "    if (edit?.removed) return [];", "shows nothing for a field whose page is gone"),
    Mutation("saved fields: show a checkbox as a text field", "src/lib/savedfields.ts", "    const kind = widget.control.kind === \"checkbox\" || widget.control.kind === \"radio\"", "    const kind = widget.control.kind === \"radio\"", "shows each field on the page it is on, under an id of its own, with its kind and name"),
    Mutation("saved fields: show a field of several lines as one line", "src/lib/savedfields.ts", "      : widget.multiline ? \"multiline\" : \"text\";", "      : \"text\";", "shows each field on the page it is on, under an id of its own, with its kind and name"),
    Mutation("saved fields: show a renamed field under its old name", "src/lib/savedfields.ts", "      note: edit?.name ?? leafName(widget.name),", "      note: leafName(widget.name),", "shows a field where it was moved to and under its new name, and not one that was removed"),
    Mutation("saved fields: show a grouped field under its full name", "src/lib/savedfields.ts", "      note: edit?.name ?? leafName(widget.name),", "      note: edit?.name ?? widget.name,", "shows each field on the page it is on, under an id of its own, with its kind and name"),
    Mutation("field properties: give a field of choices no text size", "src/lib/savedfields.ts", "    textSize: kind === \"text\" || kind === \"choice\" ? changed(\"text_size\") ?? widget.text_size ?? 0 : null,", "    textSize: kind === \"text\" ? changed(\"text_size\") ?? widget.text_size ?? 0 : null,", "reads what the file has, and only the parts the field's kind has"),
    Mutation("field properties: show the file's text size under a changed one", "src/lib/savedfields.ts", "changed(\"text_size\") ?? widget.text_size ?? 0 : null,", "widget.text_size ?? 0 : null,", "lays what a reader has changed over the file, and the new name too"),
    Mutation("field properties: give a field of choices a default value", "src/lib/savedfields.ts", "    defaultValue: kind === \"text\" ? changed(\"default_value\") ?? widget.default_value ?? \"\" : null,", "    defaultValue: changed(\"default_value\") ?? widget.default_value ?? \"\",", "reads what the file has, and only the parts the field's kind has"),
    Mutation("field properties: show the file's default under a changed one", "src/lib/savedfields.ts", "changed(\"default_value\") ?? widget.default_value ?? \"\" : null,", "widget.default_value ?? \"\" : null,", "lays what a reader has changed over the file, and the new name too"),
    Mutation("field properties: send a text size to a field that has none", "src/lib/savedfields.ts", "  if (now.textSize != null && to.textSize != null && to.textSize !== now.textSize) props.text_size = to.textSize;", "  if (to.textSize != null && to.textSize !== now.textSize) props.text_size = to.textSize;", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send a text size the panel did not name", "src/lib/savedfields.ts", "  if (now.textSize != null && to.textSize != null && to.textSize !== now.textSize) props.text_size = to.textSize;", "  if (now.textSize != null && to.textSize !== now.textSize) props.text_size = to.textSize as number;", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send a default to a field that has none", "src/lib/savedfields.ts", "  if (now.defaultValue != null && to.defaultValue != null && to.defaultValue !== now.defaultValue) {", "  if (to.defaultValue != null && to.defaultValue !== now.defaultValue) {", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: show nought as a text size", "src/lib/fieldprops.ts", "    textSize: now.textSize ? String(now.textSize) : \"\",", "    textSize: String(now.textSize ?? \"\"),", "shows a field's properties as text and ticks"),
    Mutation("field properties: refuse the least text size", "src/lib/fieldprops.ts", "size < MIN_TEXT_SIZE || size > MAX_TEXT_SIZE", "size <= MIN_TEXT_SIZE || size > MAX_TEXT_SIZE", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: refuse the most text size", "src/lib/fieldprops.ts", "size < MIN_TEXT_SIZE || size > MAX_TEXT_SIZE", "size < MIN_TEXT_SIZE || size >= MAX_TEXT_SIZE", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: take a text size that is no number", "src/lib/fieldprops.ts", "(!/^[0-9]{1,3}(\\.[0-9]{1,2})?$/.test(text) || size < MIN_TEXT_SIZE", "(size < MIN_TEXT_SIZE", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: refuse a decimal comma", "src/lib/fieldprops.ts", "    const text = typed.textSize.trim().replace(\",\", \".\");", "    const text = typed.textSize.trim();", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: judge a text size the field does not have", "src/lib/fieldprops.ts", "  if (now.textSize != null) {\n    const text = typed.textSize", "  if (true) {\n    const text = typed.textSize", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: hold a default to no limit beside it", "src/lib/fieldprops.ts", "    const most = maxLength || MAX_LENGTH;", "    const most = MAX_LENGTH;", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: refuse a default as long as the limit", "src/lib/fieldprops.ts", "    if ([...value].length > most) return", "    if ([...value].length >= most) return", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: take a control character in a default", "src/lib/fieldprops.ts", "    if (/\\p{Cc}/u.test(value)) return { problem: \"A default value cannot contain a control character\" };", "", "refuses what cannot be a field's, and says which"),
    Mutation("field properties: show the text size row for a field that says nothing of one", "src/lib/fieldprops.ts", "    this.rows.textSize.style.display = now.textSize == null ? \"none\" : \"block\";", "    this.rows.textSize.style.display = now.textSize === null ? \"none\" : \"block\";", "shows only the rows the field's kind has"),
    Mutation("field properties: show the default row for a field that has none", "src/lib/fieldprops.ts", "    this.rows.defaultValue.style.display = now.defaultValue == null ? \"none\" : \"block\";", "    this.rows.defaultValue.style.display = \"block\";", "shows only the rows the field's kind has"),
    Mutation("field properties: apply without the text size the panel holds", "src/lib/fieldprops.ts", "      textSize: this.textSize.value,\n      defaultValue: this.defaultValue.value,", "      textSize: \"\",\n      defaultValue: this.defaultValue.value,", "shows the field's name and applies what the controls hold"),
    Mutation("field properties: apply without the default the panel holds", "src/lib/fieldprops.ts", "      textSize: this.textSize.value,\n      defaultValue: this.defaultValue.value,", "      textSize: this.textSize.value,\n      defaultValue: \"\",", "shows the field's name and applies what the controls hold"),
    Mutation("pending default: show a default over an answer", "src/lib/forms.ts", "  if (value !== \"\" || widget.control.kind !== \"text\") return value;", "  if (widget.control.kind !== \"text\") return value;", "is the answer, pending or the file's, when the field holds one"),
    Mutation("pending default: show a default in a field that is not a text field", "src/lib/forms.ts", "  if (value !== \"\" || widget.control.kind !== \"text\") return value;", "  if (value !== \"\") return value;", "is never a default for a field that is not a text field"),
    Mutation("pending default: show another field's default", "src/lib/forms.ts", "    .filter((edit) => own.some((other) => fieldKey(other.widget) === fieldKey(edit.object)))", "    .filter(() => true)", "is the default a reader has given a field that holds nothing, as the save will answer it"),
    Mutation("pending default: miss a default held under the field's other widget", "src/lib/forms.ts", "  const own = widgets.filter((other) => fieldKey(other.object) === fieldKey(widget.object));", "  const own = [widget];", "is the default a reader has given a field that holds nothing, as the save will answer it"),
    Mutation("pending choices: let a choice the file lacks name the file's first", "src/lib/forms.ts", "    return { label, index: index < 0 ? null : index };", "    return { label, index: index < 0 ? 0 : index };", "are the ones a reader has just given, each at the first place the file has its label"),
    Mutation("pending choices: take the last place the file has a label at", "src/lib/forms.ts", "    const index = was.indexOf(label);", "    const index = was.lastIndexOf(label);", "are the ones a reader has just given, each at the first place the file has its label"),
    Mutation("pending choices: show the file's alignment over the new one", "src/lib/forms.ts", "  return given(widget, fields, widgets, \"align\") ?? widget.align ?? \"left\";", "  return widget.align ?? \"left\";", "are the ones a reader has just given, each at the first place the file has its label"),
    Mutation("pending choices: leave chosen what the new choices have dropped", "src/lib/forms.ts", "    return Array.isArray(value) && choices ? standingChoice(widget.control.options, choices, value) : value;", "    return value;", "keep chosen what the new choices still have by label, as the save does"),
    Mutation("pending choices: name a kept choice twice, out of order", "src/lib/forms.ts", "  return [...new Set(kept)].sort((a, b) => a - b);", "  return kept;", "keep chosen what the new choices still have by label, as the save does"),
    Mutation("button value: ask with the file's value over the one a reader gave", "src/lib/savedfields.ts", "  if (given !== undefined) return given;", "", "is asked about with the value it gives its group, read from the state the file has"),
    Mutation("button value: send the value as the group's choices", "src/lib/savedfields.ts", "  if (now.single) delete props.options;", "", "takes a new value as a change to that button, and the group's properties under its first"),
    Mutation("button value: take a value another button has", "src/lib/savedfields.ts", "  if (taken) return `The group already has a button with the value \\`${value}\\``;", "", "is refused a value another button of its group has, as the file or the reader gave it"),
    Mutation("button value: hold a removed button's value against the group", "src/lib/savedfields.ts", "    && !editOf(other, state)?.removed && buttonValue(other, state) === value);", "    && buttonValue(other, state) === value);", "is refused a value another button of its group has, as the file or the reader gave it"),
    Mutation("button value: journal the value it has as a change", "src/lib/savedfields.ts", "  if (value === buttonValue(found.widget, state)) return null;", "", "is refused a value another button of its group has, as the file or the reader gave it"),
    Mutation("button value: send two changes where one button takes both", "src/lib/fieldprops.ts", "  const one = target && button && target.object.join() === button.object.join();", "  const one = false;", "takes a new value as a change to that button, and the group's properties under its first"),
    Mutation("button value: say nothing when the value is refused", "src/lib/fieldprops.ts", "  if (typeof button === \"string\") {\n    deps.say(button);\n    return false;\n  }", "  if (typeof button === \"string\") return false;", "is refused a value another button of its group has, as the file or the reader gave it"),
    Mutation("duplicate key: take the chord with nothing picked", "src/lib/appcommands.ts", "  } else if (matches(\"edit.duplicate\", event) && title && actions.pickedMarks() >= 1) {", "  } else if (matches(\"edit.duplicate\", event) && title) {", "duplicates what is picked on Cmd-D, and leaves the chord alone with nothing picked"),
    Mutation("duplicate key: answer the chord with nothing", "src/lib/appcommands.ts", "    event.preventDefault();\n    actions.duplicatePicked();\n", "    event.preventDefault();\n", "duplicates what is picked on Cmd-D, and leaves the chord alone with nothing picked"),
    Mutation("signature field: let one be dragged as small as a text field", "src/lib/fieldnames.ts", "kind === \"signature\" ? 24 : 8;", "8;", "is a signature field under the next name of its kind, which is at least 24 points a side"),
    Mutation("signature field: show a saved one as a text field", "src/lib/savedfields.ts", "      : widget.control.kind === \"signature\" ? \"signature\"\n", "", "shows each field on the page it is on, under an id of its own, with its kind and name"),
    Mutation("signature field: refuse to copy one", "src/lib/duplicate.ts", "    case \"signature\":\n      // A copy is a new place for a signature, whether or not this one is signed.\n      return \"signature\";\n", "", "is an empty signature field for a signature field, signed or not"),
    Mutation("sign a field: offer a field that holds a signature", "src/lib/signfield.ts", "  return widget.control.kind === \"signature\" && !widget.control.signed;", "  return widget.control.kind === \"signature\";", "are the ones that hold no signature, in the form's order, each in one place"),
    Mutation("sign a field: offer a field shown in several places", "src/lib/signfield.ts", ".length === 1);", ".length >= 1);", "are the ones that hold no signature, in the form's order, each in one place"),
    Mutation("sign a field: sign a field of another kind where it is", "src/lib/signfield.ts", "  if (page === undefined || !isEmptySignature(widget)) return null;", "  if (page === undefined) return null;", "are not offered on a page the file does not show, nor when signed or of another kind"),
    Mutation("sign a field: ask for a drag beside the field", "src/lib/signing.ts", "    const placed = field ? { page: field.page, rect: field.rect } : await shell.place();", "    const placed = await shell.place();", "draws a visible one in the field, where the field is, and drags nothing"),
    Mutation("sign a field: sign without naming the field", "src/lib/signing.ts", "    chosen.identity, path, placement, chosen.timestamp, longTerm, field?.name ?? null,", "    chosen.identity, path, placement, chosen.timestamp, longTerm, null,", "draws a visible one in the field, where the field is, and drags nothing"),
    Mutation("sign a field: open the chooser without the field", "src/lib/signing.ts", "  const chosen = await shell.choose(choices.usable, field?.name ?? null);", "  const chosen = await shell.choose(choices.usable, null);", "draws a visible one in the field, where the field is, and drags nothing"),
    Mutation("sign a field: offer an invisible signature first in a field", "src/lib/signing.ts", "  const visible = appearance(\"visible\", \"Visible\", true);", "  const visible = appearance(\"visible\", \"Visible\", false);", "offers a visible signature first when a signature field is being signed"),
    Mutation("sign a field: offer the command with no field to sign", "src/lib/appcommands.ts", "      enabled: () => withDocument() && actions.signableFields() >= 1,", "      enabled: withDocument,", "signs a signature field through its own command, offered only while the document has an empty one"),
    Mutation("placed properties: give a placed dropdown no text size", "src/lib/savedfields.ts", "    textSize: text || field.kind === \"dropdown\" ? field.text_size ?? 0 : null,", "    textSize: text ? field.text_size ?? 0 : null,", "asks with what the field was placed with, by its kind"),
    Mutation("placed properties: show no text size for the one it has", "src/lib/savedfields.ts", "    textSize: text || field.kind === \"dropdown\" ? field.text_size ?? 0 : null,", "    textSize: text || field.kind === \"dropdown\" ? 0 : null,", "asks with what the field was placed with, by its kind"),
    Mutation("placed properties: give a placed dropdown a default", "src/lib/savedfields.ts", "    defaultValue: text ? field.default_value ?? \"\" : null,", "    defaultValue: field.default_value ?? \"\",", "asks with what the field was placed with, by its kind"),
    Mutation("placed properties: show no default for the one it has", "src/lib/savedfields.ts", "    defaultValue: text ? field.default_value ?? \"\" : null,", "    defaultValue: text ? \"\" : null,", "asks with what the field was placed with, by its kind"),
    Mutation("placed properties: read a size the kind lacks as left out", "src/lib/fieldprops.ts", "    ...(now.textSize === null ? { textSize: null } : {}),", "", "are the group's name, its flags, and the one value the button gives"),
    Mutation("placed properties: read a default the kind lacks as left out", "src/lib/fieldprops.ts", "    ...(now.defaultValue === null ? { defaultValue: null } : {}),", "", "are the group's name, its flags, and the one value the button gives"),
    Mutation("duplicate: copy a field of the file without its text size", "src/lib/duplicate.ts", "    ...(now.textSize ? { text_size: now.textSize } : {}),", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: copy a field of the file without its default", "src/lib/duplicate.ts", "    ...(now.defaultValue ? { default_value: now.defaultValue } : {}),", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: carry nought as a text size", "src/lib/duplicate.ts", "    ...(now.textSize ? { text_size: now.textSize } : {}),", "    ...(now.textSize != null ? { text_size: now.textSize } : {}),", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: carry nothing as a default", "src/lib/duplicate.ts", "    ...(now.defaultValue ? { default_value: now.defaultValue } : {}),", "    ...(now.defaultValue != null ? { default_value: now.defaultValue } : {}),", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("field picture: draw again the pages whose fields did not change", "src/lib/savedfields.ts", "    .filter(({ widget }) => of(before, widget) !== of(after, widget))", "    .filter(({ widget }) => of(before, widget) === of(after, widget))", "is the page of each widget whose change is new, different or gone"),
    Mutation("field picture: name a page once for each changed widget", "src/lib/savedfields.ts", "  return [...new Set(pages)];", "  return pages;", "names a page once, and none for a widget the form does not have"),
    Mutation("field picture: forget the changes the pages were drawn with", "src/lib/viewer.ts", "    const pages = redrawn(widgets, this.fieldEdits, fields);\n    this.fieldEdits = fields;", "    const pages = redrawn(widgets, this.fieldEdits, fields);", "repaints every slot drawn from a page whose fields changed, and no other"),
    Mutation("field picture: repaint the slot numbered as the page", "src/lib/viewer.ts", "      if (source !== undefined && pages.includes(source)) this.scroller.invalidatePage(slot);", "      if (pages.includes(slot)) this.scroller.invalidatePage(slot);", "repaints every slot drawn from a page whose fields changed, and no other"),
    Mutation("field picture: report a repaint that did not happen", "src/lib/viewer.ts", "    this.fieldEdits = fields;\n    if (!pages.length) return false;", "    this.fieldEdits = fields;", "repaints every slot drawn from a page whose fields changed, and no other"),
    Mutation("saved fields: show a moved field where the file has it", "src/lib/savedfields.ts", "      quads: [...(edit?.rect ?? widget.display_rect)],", "      quads: [...widget.display_rect],", "shows a field where it was moved to and under its new name, and not one that was removed"),
    Mutation("saved fields: number the fields from one", "src/lib/savedfields.ts", "      id: SAVED_BASE + at,", "      id: SAVED_BASE + at + 1,", "shows each field on the page it is on, under an id of its own, with its kind and name"),
    Mutation("saved fields: fill a removed field", "src/lib/savedfields.ts", "    if (edit?.removed) return [];\n    return [{ ...widget, display_rect: edit?.rect ?? widget.display_rect }];", "    return [{ ...widget, display_rect: edit?.rect ?? widget.display_rect }];", "shows a field where it was moved to and under its new name, and not one that was removed"),
    Mutation("saved fields: fill a moved field where the file has it", "src/lib/savedfields.ts", "    return [{ ...widget, display_rect: edit?.rect ?? widget.display_rect }];", "    return [{ ...widget }];", "shows a field where it was moved to and under its new name, and not one that was removed"),
    Mutation("saved fields: move a field from where the file has it", "src/lib/savedfields.ts", "  return { widget, page, rect: editOf(widget, state)?.rect ?? widget.display_rect };", "  return { widget, page, rect: widget.display_rect };", "moves a field from where it now is, by widget and page id"),
    Mutation("saved fields: change a field whose page is gone", "src/lib/savedfields.ts", "  if (!widget || page === undefined) return null;", "  if (!widget) return null;\n  if (page === undefined) return { widget, page: 0 as never, rect: widget.display_rect };", "answers nothing for an id that names no saved field, or one whose page is gone"),
    Mutation("saved fields: take a name a field beside it has", "src/lib/savedfields.ts", "  if (taken) return `\\`${name}\\`: another field has this name`;", "", "renames a field, and refuses a name another field will have"),
    Mutation("saved fields: hold a removed field's name against a rename", "src/lib/savedfields.ts", "      return !edit?.removed && (edit?.name ?? other.name) === name;", "      return (edit?.name ?? other.name) === name;", "renames a field, and refuses a name another field will have"),
    Mutation("saved fields: hold a field's old name against a rename", "src/lib/savedfields.ts", "      return !edit?.removed && (edit?.name ?? other.name) === name;", "      return !edit?.removed && other.name === name;", "renames a field, and refuses a name another field will have"),
    Mutation("saved fields: take a name a field placed this session has", "src/lib/savedfields.ts", "    || state.marks.some((mark) => mark.kind === \"field\" && mark.note === name)", "", "renames a field, and refuses a name another field will have"),
    Mutation("saved fields: refuse a field its own name", "src/lib/savedfields.ts", "  const others = form.widgets.filter((other) => !same(other.object, found.widget.object) && !other.name.includes(\".\"));", "  const others = form.widgets.filter((other) => !other.name.includes(\".\"));", "renames a field, and refuses a name another field will have"),
    Mutation("saved fields: compare a grouped field with the top of the form", "src/lib/savedfields.ts", "  const taken = top && (", "  const taken = true && (", "renames a field, and refuses a name another field will have"),
    Mutation("saved fields: send a saved field's move as a mark's", "src/lib/savedfields.ts", "    if (!isSaved(move.mark)) {\n      marks.push(move);\n      continue;\n    }", "    if (true) {\n      marks.push(move);\n      continue;\n    }", "splits an arrangement into the placed marks and the saved fields"),
    Mutation("saved fields: put a control where a removed field was", "src/lib/savedfields.ts", "  if (edit?.removed) return null;\n  return edit?.rect ?? widget.display_rect;", "  return edit?.rect ?? widget.display_rect;", "is where the field was moved to, where the file has it, or nowhere once removed"),
    Mutation("saved fields: put a control where the file has a moved field", "src/lib/savedfields.ts", "  if (edit?.removed) return null;\n  return edit?.rect ?? widget.display_rect;", "  if (edit?.removed) return null;\n  return widget.display_rect;", "is where the field was moved to, where the file has it, or nowhere once removed"),
    Mutation("saved fields: ask for an arrangement of no marks", "src/lib/savedfields.ts", "  if (parts.marks.length > 0) await edits.arrange(parts.marks, sweep);", "  await edits.arrange(parts.marks, sweep);", "sends saved fields alone as one change, and asks for no empty arrangement"),
    Mutation("saved fields: send the fields of an arrangement under no gesture", "src/lib/savedfields.ts", "  return edits.refield(parts.fields, sweep);", "  return edits.refield(parts.fields);", "sends both under the one gesture, the marks first"),
    Mutation("saved fields: drop the fields of an arrangement", "src/lib/savedfields.ts", "  if (parts.fields.length === 0) return edits.arrange(parts.marks, sweep);", "  if (true) return edits.arrange(parts.marks, sweep);", "sends both under the one gesture, the marks first"),
    Mutation("placed field panel: give a field of several lines no limit", "src/lib/savedfields.ts", "  const text = field.kind === \"text\" || field.kind === \"multiline\";", "  const text = field.kind === \"text\";", "asks with what the field was placed with, by its kind"),
    Mutation("placed field panel: give a checkbox an alignment", "src/lib/savedfields.ts", "    align: field.kind === \"checkbox\" || field.kind === \"radio\" ? null : field.align ?? \"left\",", "    align: field.align ?? \"left\",", "asks with what the field was placed with, by its kind"),
    Mutation("placed field panel: give a text field choices", "src/lib/savedfields.ts", "    options: field.kind === \"dropdown\" || field.kind === \"radio\" ? [...(field.options ?? [])] : null,", "    options: [...(field.options ?? [])],", "asks with what the field was placed with, by its kind"),
    Mutation("placed field panel: read no tooltip", "src/lib/savedfields.ts", "    tooltip: field.tooltip ?? \"\",", "    tooltip: \"\",", "asks with what the field was placed with, by its kind"),
    Mutation("placed field panel: read a mark that is no field as one", "src/lib/savedfields.ts", "  if (!mark || !field) return null;", "  if (!mark) return null;\n  if (!field) return { name: mark.note, tooltip: \"\", required: false, readOnly: false, maxLength: null, align: null, options: null };", "is none with nothing picked, several picked, a mark that is no field, or a field that is gone"),
    Mutation("placed field panel: change it as a field of the file", "src/lib/fieldprops.ts", "  if (!isSaved(after.id)) {", "  if (false) {", "changes the mark, with the parts that differ, and not a field of the file"),
    Mutation("placed field panel: say nothing when nothing was changed", "src/lib/fieldprops.ts", "    if (Object.keys(props).length === 0) {\n      deps.say(\"Nothing about the field was changed.\");\n      return false;\n    }\n", "", "says so when nothing was changed, and makes nothing for a mark removed meanwhile"),
    Mutation("placed field panel: change whatever is picked when the panel closes", "src/lib/fieldprops.ts", "  const after = subject({ ...deps, picked: () => [before.id] });", "  const after = subject(deps);", "changes the field that was picked when the panel opened, whatever is picked when it closes"),
    Mutation("placed field panel: need the file's fields shown to change a placed one", "src/lib/fieldprops.ts", "  const now = isSaved(id)\n    ? form && properties(form, state, id)\n    : placedProperties(", "  const now = isSaved(id)\n    ? form && properties(form, state, id)\n    : form && placedProperties(", "is a field placed in this session, whether or not the file's fields are being changed"),
    # One press picks a rectangle and two open it, added 2026-10-04.
    Mutation("press: open a rectangle's box on one press", "src/lib/viewer.ts", "      if ((twice || !isResizable(own.kind)) && this.markNote.openId !== own.id) this.showMark(own.id);", "      if (this.markNote.openId !== own.id) this.showMark(own.id);", "picks a rectangle on one press and opens its box on a second soon after"),
    Mutation("press: never open a rectangle's box", "src/lib/viewer.ts", "      if ((twice || !isResizable(own.kind)) && this.markNote.openId", "      if ((!isResizable(own.kind)) && this.markNote.openId", "picks a rectangle on one press and opens its box on a second soon after"),
    Mutation("press: open a highlight only on two presses", "src/lib/viewer.ts", "      if ((twice || !isResizable(own.kind)) && this.markNote.openId", "      if ((twice) && this.markNote.openId", "opens a mark made of the words under it on one press, as before"),
    Mutation("press: count a second press however late", "src/lib/viewer.ts", "      const twice = this.lastPress?.id === own.id && now - this.lastPress.at <= DOUBLE_PRESS_MS;", "      const twice = this.lastPress?.id === own.id;", "picks a rectangle on one press and opens its box on a second soon after"),
    Mutation("press: count a press on another rectangle as the second", "src/lib/viewer.ts", "      const twice = this.lastPress?.id === own.id && now - this.lastPress.at <= DOUBLE_PRESS_MS;", "      const twice = this.lastPress !== null && now - this.lastPress.at <= DOUBLE_PRESS_MS;", "picks a rectangle on one press and opens its box on a second soon after"),
    Mutation("press: shorten the time a second press has", "src/lib/viewer.ts", "const DOUBLE_PRESS_MS = 450;", "const DOUBLE_PRESS_MS = 449;", "picks a rectangle on one press and opens its box on a second soon after"),
    Mutation("remove picked: give each of several a gesture of none", "src/lib/viewer.ts", "    const sweep = picked.length > 1 ? ++this.sweeps : 0;", "    const sweep = 0;", "takes several off under one gesture, and one alone under none"),
    Mutation("remove picked: name a gesture for one alone", "src/lib/viewer.ts", "    const sweep = picked.length > 1 ? ++this.sweeps : 0;", "    const sweep = ++this.sweeps;", "takes several off under one gesture, and one alone under none"),
    Mutation("remove picked: leave the removed ones picked", "src/lib/viewer.ts", "    this.arranging.clear();\n    this.pickedChanged();\n    for (const id of picked)", "    for (const id of picked)", "takes several off under one gesture, and one alone under none"),
    Mutation("remove picked: ignore the mark whose box is open", "src/lib/viewer.ts", "    if (this.markNote.openId !== null) {\n      this.removeOpenMark();\n      return true;\n    }\n", "", "takes the mark whose box is open and leaves the picked ones"),
    Mutation("remove picked: offer removal only with a box open", "src/lib/viewer.ts", "    return this.markNote.openId !== null || this.arranging.count > 0;", "    return this.markNote.openId !== null;", "takes several off under one gesture, and one alone under none"),
    Mutation("remove picked: offer removal only with something picked", "src/lib/viewer.ts", "    return this.markNote.openId !== null || this.arranging.count > 0;", "    return this.arranging.count > 0;", "takes the mark whose box is open and leaves the picked ones"),
    Mutation("remove picked: take Backspace for nothing", "src/lib/viewer.ts", "(event.key === \"Delete\" || event.key === \"Backspace\")", "(event.key === \"Delete\")", "is what Delete and Backspace do with rectangles picked, and not otherwise"),
    Mutation("remove picked: take Delete with nothing picked", "src/lib/viewer.ts", "    } else if (this.arranging.count > 0 && (event.key === \"Delete\"", "    } else if ((event.key === \"Delete\"", "is what Delete and Backspace do with rectangles picked, and not otherwise"),
    # Tab order, added 2026-10-04.
    Mutation("tab order: offer it again once asked for", "src/lib/fieldnames.ts", "  if (!state || state.tab_order) return false;", "  if (!state) return false;", "is not offered again once asked for, nor with no document"),
    Mutation("tab order: offer it for a document with no field", "src/lib/fieldnames.ts", "  return (form?.widgets.length ?? 0) > 0 || state.marks.some((mark) => mark.kind === \"field\");", "  return true;", "needs a field: one of the file's, or one placed since"),
    Mutation("tab order: need a field of the file", "src/lib/fieldnames.ts", "  return (form?.widgets.length ?? 0) > 0 || state.marks.some((mark) => mark.kind === \"field\");", "  return (form?.widgets.length ?? 0) > 0;", "needs a field: one of the file's, or one placed since"),
    Mutation("tab order: count a box as a field", "src/lib/fieldnames.ts", "|| state.marks.some((mark) => mark.kind === \"field\");", "|| state.marks.length > 0;", "needs a field: one of the file's, or one placed since"),
    # Moving several together, nudging and duplicating, added 2026-10-04.
    Mutation("move together: let them leave by the right", "src/lib/arrange.ts", "    dx: Math.min(Math.max(want.dx, -left), page.width - right),", "    dx: Math.max(want.dx, -left),", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: let them leave by the left", "src/lib/arrange.ts", "    dx: Math.min(Math.max(want.dx, -left), page.width - right),", "    dx: Math.min(want.dx, page.width - right),", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: let them leave by the bottom", "src/lib/arrange.ts", "    dy: Math.min(Math.max(want.dy, -top), page.height - bottom),", "    dy: Math.max(want.dy, -top),", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: let them leave by the top", "src/lib/arrange.ts", "    dy: Math.min(Math.max(want.dy, -top), page.height - bottom),", "    dy: Math.min(want.dy, page.height - bottom),", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: stop at the first one's left and not the nearest", "src/lib/arrange.ts", "  const left = Math.min(...quads.map((quad) => quad.left));\n  const top = Math.min(...quads.map((quad) => quad.top));\n  const right = Math.max(...quads.map((quad) => quad.right));", "  const left = quads[0]!.left;\n  const top = Math.min(...quads.map((quad) => quad.top));\n  const right = Math.max(...quads.map((quad) => quad.right));", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: stop at the last one's top and not the nearest", "src/lib/arrange.ts", "  const top = Math.min(...quads.map((quad) => quad.top));\n  const right = Math.max(...quads.map((quad) => quad.right));\n  const bottom = Math.max(...quads.map((quad) => quad.bottom));\n  return {\n    dx: Math.min(Math.max(want.dx", "  const top = quads[quads.length - 1]!.top;\n  const right = Math.max(...quads.map((quad) => quad.right));\n  const bottom = Math.max(...quads.map((quad) => quad.bottom));\n  return {\n    dx: Math.min(Math.max(want.dx", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: stop at the last one's right and not the nearest", "src/lib/arrange.ts", "  const right = Math.max(...quads.map((quad) => quad.right));\n  const bottom = Math.max(...quads.map((quad) => quad.bottom));\n  return {\n    dx: Math.min(Math.max(want.dx", "  const right = quads[quads.length - 1]!.right;\n  const bottom = Math.max(...quads.map((quad) => quad.bottom));\n  return {\n    dx: Math.min(Math.max(want.dx", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: stop at the first one's bottom and not the nearest", "src/lib/arrange.ts", "  const bottom = Math.max(...quads.map((quad) => quad.bottom));\n  return {\n    dx: Math.min(Math.max(want.dx", "  const bottom = quads[0]!.bottom;\n  return {\n    dx: Math.min(Math.max(want.dx", "stops all of them where the one nearest an edge stops, on each of the four sides"),
    Mutation("move together: move by an offset that is not a number", "src/lib/arrange.ts", "  if (quads.length === 0 || !Number.isFinite(want.dx) || !Number.isFinite(want.dy)) return { dx: 0, dy: 0 };", "  if (quads.length === 0) return { dx: 0, dy: 0 };", "moves nothing for no rectangles or an offset that is not a number"),
    Mutation("move together: move no rectangles somewhere", "src/lib/arrange.ts", "  if (quads.length === 0 || !Number.isFinite(want.dx) || !Number.isFinite(want.dy)) return { dx: 0, dy: 0 };", "  if (!Number.isFinite(want.dx) || !Number.isFinite(want.dy)) return { dx: 0, dy: 0 };", "moves nothing for no rectangles or an offset that is not a number"),
    Mutation("drag several: move only the one under the hand", "src/lib/viewer.ts", "            others: several ? this.arranging.list().filter((id) => id !== own.id) : [],", "            others: [],", "moves all of them by what the one under the hand moved, as one gesture"),
    Mutation("drag several: report it as a move of one", "src/lib/viewer.ts", "        if (live.others.length > 0) {", "        if (false) {", "moves all of them by what the one under the hand moved, as one gesture"),
    Mutation("nudge: journal a move of nothing at the page's edge", "src/lib/viewer.ts", "    if (by.dx === 0 && by.dy === 0) return false;\n", "", "stop at the page's edge, and are still taken there"),
    Mutation("nudge: take the arrows with nothing picked", "src/lib/viewer.ts", "    } else if (this.arranging.count > 0 && !event.metaKey", "    } else if (!event.metaKey", "are left to the page with nothing picked, and with a modifier held"),
    Mutation("nudge: take an arrow with the command key held", "src/lib/viewer.ts", "this.arranging.count > 0 && !event.metaKey && !event.ctrlKey", "this.arranging.count > 0 && !event.ctrlKey", "are left to the page with nothing picked, and with a modifier held"),
    Mutation("nudge: take an arrow with the control key held", "src/lib/viewer.ts", "!event.metaKey && !event.ctrlKey && !event.altKey && event.key in NUDGES", "!event.metaKey && !event.altKey && event.key in NUDGES", "are left to the page with nothing picked, and with a modifier held"),
    Mutation("nudge: take an arrow with the option key held", "src/lib/viewer.ts", "!event.ctrlKey && !event.altKey && event.key in NUDGES", "!event.ctrlKey && event.key in NUDGES", "are left to the page with nothing picked, and with a modifier held"),
    Mutation("nudge: move a point with Shift too", "src/lib/viewer.ts", "      const step = event.shiftKey ? NUDGE_FAR : 1;", "      const step = 1;", "move them a point, or ten with Shift, as one gesture each"),
    Mutation("nudge: move left for the right arrow", "src/lib/viewer.ts", "ArrowLeft: [-1, 0], ArrowRight: [1, 0]", "ArrowLeft: [1, 0], ArrowRight: [-1, 0]", "move them a point, or ten with Shift, as one gesture each"),
    Mutation("nudge: move up for the down arrow", "src/lib/viewer.ts", "ArrowUp: [0, -1], ArrowDown: [0, 1]", "ArrowUp: [0, 1], ArrowDown: [0, -1]", "move them a point, or ten with Shift, as one gesture each"),
    Mutation("copies: place a copy beside and not below", "src/lib/viewer.ts", "{ dx: COPY_STEP, dy: COPY_STEP }, this.laidSize(found.slot));", "{ dx: COPY_STEP, dy: 0 }, this.laidSize(found.slot));", "is a step down and to the right of each, under a gesture of its own"),
    Mutation("copies: make every duplication the same gesture", "src/lib/viewer.ts", "    return {\n      sweep: ++this.sweeps,", "    return {\n      sweep: this.sweeps,", "is a step down and to the right of each, under a gesture of its own"),
    Mutation("duplicate: copy a field of several lines as one line", "src/lib/duplicate.ts", "      return multiline ? \"multiline\" : \"text\";", "      return \"text\";", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: copy a list as a dropdown", "src/lib/duplicate.ts", "      return control.combo && !control.editable && !control.multiple", "      return !control.editable && !control.multiple", "is refused for a kind tpdf does not place, a button inside a group of fields, and a field that is gone"),
    Mutation("duplicate: copy a dropdown that takes text as one that does not", "src/lib/duplicate.ts", "      return control.combo && !control.editable && !control.multiple", "      return control.combo && !control.multiple", "is refused for a kind tpdf does not place, a button inside a group of fields, and a field that is gone"),
    Mutation("duplicate: drop the tooltip", "src/lib/duplicate.ts", "    ...(now.tooltip ? { tooltip: now.tooltip } : {}),\n", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: drop required", "src/lib/duplicate.ts", "    ...(now.required ? { required: true } : {}),\n", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: drop read-only", "src/lib/duplicate.ts", "    ...(now.readOnly ? { read_only: true } : {}),\n", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: drop the limit", "src/lib/duplicate.ts", "    ...(now.maxLength ? { max_length: now.maxLength } : {}),\n", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: drop the alignment", "src/lib/duplicate.ts", "    ...(now.align && now.align !== \"left\" ? { align: now.align } : {}),\n", "", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: copy a radio button out of a group of fields", "src/lib/duplicate.ts", "    if (kind === \"radio\" && found.widget.name.includes(\".\")) return", "    if (false) return", "is refused for a kind tpdf does not place, a button inside a group of fields, and a field that is gone"),
    Mutation("duplicate: copy a saved dropdown with no choices", "src/lib/duplicate.ts", "      { kind, options: kind === \"dropdown\" ? now?.options ?? [] : [], group: found.widget.name },", "      { kind, options: [], group: found.widget.name },", "is a placed field of its kind, with its properties and a name the form does not have"),
    Mutation("duplicate: copy a mark in the default colour and line", "src/lib/duplicate.ts", "  const copied = { ...base, page: mark.page, color: mark.color, width: mark.width };", "  const copied = { ...base, page: mark.page };", "is a box or an ellipse with its note, colour and line"),
    Mutation("duplicate: refuse an ellipse", "src/lib/duplicate.ts", "  if (mark.kind === \"square\" || mark.kind === \"ellipse\") return", "  if (mark.kind === \"square\") return", "is a box or an ellipse with its note, colour and line"),
    Mutation("duplicate: drop a placed field's properties", "src/lib/duplicate.ts", "field: { ...made.field, ...rest } };", "field: made.field };", "is a field of the same kind and properties under the next free name, where it was told to go"),
    Mutation("duplicate: give a copied radio button the value it was copied from", "src/lib/duplicate.ts", "  const { kind: _kind, border: _border, options: _options, ...rest } = mark.field;", "  const { kind: _kind, border: _border, ...rest } = mark.field;", "keeps a radio button in its group, with the next value the group does not have"),
    Mutation("duplicate: copy a framed field without its line", "src/lib/duplicate.ts", "    mark.field.border, formNames, state.marks, form,", "    false, formNames, state.marks, form,", "is a field of the same kind and properties under the next free name, where it was told to go"),
    Mutation("duplicate: skip one in silence", "src/lib/duplicate.ts", "      deps.say(copy);\n", "", "says why one is not copied and copies the rest"),
    Mutation("duplicate: pick nothing in place of the pick when none was made", "src/lib/duplicate.ts", "  if (made.length > 0) deps.pick(made);", "  deps.pick(made);", "leaves the pick alone when the model made none of them"),
    Mutation("duplicate: make each copy a gesture of its own", "src/lib/duplicate.ts", "    await deps.make(copy, asked.sweep);", "    await deps.make(copy, 0);", "makes a copy of each under the one gesture and picks the copies"),
    # Radio button groups, added 2026-10-04.
    Mutation("radio group: take an empty name", "src/lib/fieldnames.ts", "  if (group === \"\") return", "  if (false) return", "is held to what a field's name may be"),
    Mutation("radio group: keep the space round the name", "src/lib/fieldnames.ts", "  const group = raw.trim();", "  const group = raw;", "is the name with the space round it dropped"),
    Mutation("radio group: take a name with a period", "src/lib/fieldnames.ts", "  if (group.includes(\".\")) return", "  if (false) return", "is held to what a field's name may be"),
    Mutation("radio group: take a name past the longest", "src/lib/fieldnames.ts", "  if ([...group].length > 255) return", "  if (false) return", "is held to what a field's name may be"),
    Mutation("radio value: count another group's values", "src/lib/fieldnames.ts", "mark.kind === \"field\" && mark.note === group && mark.field?.kind === \"radio\"", "mark.kind === \"field\" && mark.field?.kind === \"radio\"", "is a radio button named for its group, with the first value the group does not have"),
    Mutation("radio value: count a text field's choices", "src/lib/fieldnames.ts", "mark.kind === \"field\" && mark.note === group && mark.field?.kind === \"radio\"", "mark.kind === \"field\" && mark.note === group", "is a radio button named for its group, with the first value the group does not have"),
    Mutation("radio value: ignore the file's buttons", "src/lib/fieldnames.ts", "    for (const state of widget.control.states) values.add(new TextDecoder().decode(new Uint8Array(state)));", "", "counts the values the file's group has as well"),
    Mutation("radio value: count another saved group's buttons", "src/lib/fieldnames.ts", "    if (widget.name !== group || widget.control.kind !== \"radio\") continue;", "    if (widget.control.kind !== \"radio\") continue;", "counts the values the file's group has as well"),
    Mutation("radio value: start past the highest and not at the first free", "src/lib/fieldnames.ts", "    while (taken.has(`Choice ${n}`)) n += 1;", "    n = taken.size + 1;", "is a radio button named for its group, with the first value the group does not have"),
    Mutation("radio: name a button as a field of its own", "src/lib/fieldnames.ts", "    return { field: { kind: \"radio\", border: false, options: [`Choice ${n}`] }, name: armed.group };", "    return { field: { kind: \"radio\", border: false, options: [`Choice ${n}`] }, name: nextFieldName(\"radio\", takenNames(formNames, marks)) };", "is a radio button named for its group, with the first value the group does not have"),
    Mutation("placing: drop a dropdown's choices", "src/lib/fieldnames.ts", "    field: { kind: armed.kind, border, ...(armed.options.length > 0 ? { options: armed.options } : {}) },", "    field: { kind: armed.kind, border },", "is a dropdown holding the choices it was armed with"),
    Mutation("placing: frame every field", "src/lib/fieldnames.ts", "    field: { kind: armed.kind, border, ...(armed.options.length > 0 ? { options: armed.options } : {}) },", "    field: { kind: armed.kind, border: true, ...(armed.options.length > 0 ? { options: armed.options } : {}) },", "is a text field or a checkbox under a name no field has, with the line asked for"),
    Mutation("radio panel: take several values for one button", "src/lib/fieldprops.ts", "    if (now.single && parsed.options.length !== 1) return", "    if (false) return", "take one value, which is not Off and has no semicolon"),
    Mutation("radio panel: take Off as a value", "src/lib/fieldprops.ts", "    if (now.single && parsed.options[0] === \"Off\") return", "    if (false) return", "take one value, which is not Off and has no semicolon"),
    Mutation("radio panel: refuse a dropdown the choice Off", "src/lib/fieldprops.ts", "    if (now.single && parsed.options[0] === \"Off\") return", "    if (parsed.options[0] === \"Off\") return", "take one value, which is not Off and has no semicolon"),
    Mutation("radio panel: ask a button for choices with semicolons", "src/lib/fieldprops.ts", "return now.single ? { problem: \"The value this button gives its group\" } : parsed;", "return parsed;", "take one value, which is not Off and has no semicolon"),
    Mutation("radio panel: caption a button's value as choices", "src/lib/fieldprops.ts", "    this.optionsCaption.textContent = now.single\n      ? \"Value this button gives its group\"\n      : \"Choices, with a semicolon between them\";", "", "captions the choices as a value for a radio button, and as choices again after"),
    Mutation("radio panel: give a button an alignment", "src/lib/savedfields.ts", "    align: field.kind === \"checkbox\" || field.kind === \"radio\" ? null : field.align ?? \"left\",", "    align: field.kind === \"checkbox\" ? null : field.align ?? \"left\",", "are the group's name, its flags, and the one value the button gives"),
    Mutation("radio panel: show a button no value", "src/lib/savedfields.ts", "    options: field.kind === \"dropdown\" || field.kind === \"radio\" ? [...(field.options ?? [])] : null,", "    options: field.kind === \"dropdown\" ? [...(field.options ?? [])] : null,", "are the group's name, its flags, and the one value the button gives"),
    # A field's properties and the panel they are set in, added 2026-10-04.
    Mutation("field properties: read the file's tooltip under a changed one", "src/lib/savedfields.ts", "    tooltip: changed(\"tooltip\") ?? widget.tooltip ?? \"\",", "    tooltip: widget.tooltip ?? \"\",", "lays what a reader has changed over the file, and the new name too"),
    Mutation("field properties: head the panel with the old name", "src/lib/savedfields.ts", "    name: editOf(widget, state)?.name ?? leafName(widget.name),", "    name: leafName(widget.name),", "lays what a reader has changed over the file, and the new name too"),
    Mutation("field properties: give a checkbox a most characters", "src/lib/savedfields.ts", "    maxLength: kind === \"text\" ? ", "    maxLength: true ? ", "reads what the file has, and only the parts the field's kind has"),
    Mutation("field properties: give a field of choices no alignment", "src/lib/savedfields.ts", "    align: kind === \"text\" || kind === \"choice\" ? ", "    align: kind === \"text\" ? ", "reads what the file has, and only the parts the field's kind has"),
    Mutation("field properties: read every field's changes as this one's", "src/lib/savedfields.ts", "    form.widgets.some((other) => same(other.widget, edit.object) && same(other.object, widget.object)));", "    form.widgets.some((other) => same(other.widget, edit.object)));", "reads a field shown twice the same from either widget"),
    Mutation("field properties: read only the picked widget's changes", "src/lib/savedfields.ts", "    form.widgets.some((other) => same(other.widget, edit.object) && same(other.object, widget.object)));", "    same(edit.object, widget.widget));", "reads a field shown twice the same from either widget"),
    Mutation("field properties: send a tooltip that did not change", "src/lib/savedfields.ts", "  if (to.tooltip !== now.tooltip) props.tooltip = to.tooltip;", "  props.tooltip = to.tooltip;", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send a required flag that did not change", "src/lib/savedfields.ts", "  if (to.required !== now.required) props.required = to.required;", "  props.required = to.required;", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send a read-only flag that did not change", "src/lib/savedfields.ts", "  if (to.readOnly !== now.readOnly) props.read_only = to.readOnly;", "  props.read_only = to.readOnly;", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send a limit that did not change", "src/lib/savedfields.ts", " && to.maxLength !== now.maxLength) props.max_length", ") props.max_length", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send a limit for a field that has none", "src/lib/savedfields.ts", "  if (now.maxLength !== null && to.maxLength !== null", "  if (to.maxLength !== null", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: send an alignment for a field that has none", "src/lib/savedfields.ts", "  if (now.align !== null && to.align !== null", "  if (to.align !== null", "names only the parts that differ, and nothing when none does"),
    Mutation("field properties: miss a choice taken away", "src/lib/savedfields.ts", "(to.options.length !== now.options.length || to.options.some(", "(to.options.some(", "sends new choices when one differs, is added or is taken away"),
    Mutation("field properties: miss choices in another order", "src/lib/savedfields.ts", "    && (to.options.length !== now.options.length || to.options.some((option, at) => option !== now.options![at]))) {", "    && (to.options.length !== now.options.length)) {", "sends new choices when one differs, is added or is taken away"),
    Mutation("field properties: change a field under a widget that is removed", "src/lib/savedfields.ts", "    .filter((other) => same(other.object, found.widget.object) && !editOf(other, state)?.removed)", "    .filter((other) => same(other.object, found.widget.object))", "changes a field shown twice under its first widget that is still there, whichever was picked"),
    Mutation("field properties: change a field under the picked widget", "src/lib/savedfields.ts", "  return target(first ?? found, { props });", "  return target(found, { props });", "changes a field shown twice under its first widget that is still there, whichever was picked"),
    Mutation("field panel: show no limit as nought", "src/lib/fieldprops.ts", "    maxLength: now.maxLength ? String(now.maxLength) : \"\",", "    maxLength: String(now.maxLength ?? \"\"),", "shows a field's properties as text and ticks"),
    Mutation("field panel: keep the space round a tooltip", "src/lib/fieldprops.ts", "  const tooltip = typed.tooltip.trim();", "  const tooltip = typed.tooltip;", "reads what a reader typed, for the parts the field's kind has"),
    Mutation("field panel: take a tooltip of any length", "src/lib/fieldprops.ts", "  if ([...tooltip].length > MAX_TOOLTIP) return", "  if (false) return", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: take a tooltip with a control character", "src/lib/fieldprops.ts", "  if (/\\p{Cc}/u.test(tooltip)) return", "  if (false) return", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: read an empty limit as a number", "src/lib/fieldprops.ts", "    if (text === \"\") {", "    if (false) {", "reads back what it showed as the same properties"),
    Mutation("field panel: take a limit of nought", "src/lib/fieldprops.ts", " || Number(text) < 1 || Number(text) > MAX_LENGTH) {", " || Number(text) > MAX_LENGTH) {", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: take a limit past what an answer holds", "src/lib/fieldprops.ts", " || Number(text) < 1 || Number(text) > MAX_LENGTH) {", " || Number(text) < 1) {", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: take a limit that is not a whole number", "src/lib/fieldprops.ts", "    } else if (!/^[0-9]{1,6}$/.test(text) || ", "    } else if (", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: take any alignment", "src/lib/fieldprops.ts", "    if (typed.align !== \"left\" && typed.align !== \"center\" && typed.align !== \"right\") {", "    if (false) {", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: judge the limit of a field that has none", "src/lib/fieldprops.ts", "  if (now.maxLength !== null) {\n    const text", "  if (true) {\n    const text", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: judge the choices of a field that has none", "src/lib/fieldprops.ts", "  if (now.options !== null) {\n    const parsed", "  if (true) {\n    const parsed", "refuses what cannot be a field's, and says which"),
    Mutation("field panel: offer the panel with several picked", "src/lib/fieldprops.ts", "  const id = picked.length === 1 ? picked[0]! : null;", "  const id = picked.length >= 1 ? picked[0]! : null;", "is none with nothing picked, several picked, a mark that is no field, or a field that is gone"),
    Mutation("field panel: say nothing when nothing was changed", "src/lib/fieldprops.ts", "  if (!target && !button) {\n    deps.say(\"Nothing about the field was changed.\");\n", "  if (!target && !button) {\n", "says so when the panel is applied with nothing changed"),
    Mutation("field panel: change a document that has gone", "src/lib/fieldprops.ts", "  if (!after || !state) return false;\n", "", "makes nothing when the document went while the panel was open"),
    Mutation("field panel: show a checkbox a limit", "src/lib/fieldprops.ts", "    this.rows.maxLength.style.display = now.maxLength === null ? \"none\" : \"block\";", "    this.rows.maxLength.style.display = \"block\";", "shows only the rows the field's kind has"),
    Mutation("field panel: show a text field choices", "src/lib/fieldprops.ts", "    this.rows.options.style.display = now.options === null ? \"none\" : \"block\";", "    this.rows.options.style.display = \"block\";", "shows only the rows the field's kind has"),
    Mutation("field panel: show a checkbox an alignment", "src/lib/fieldprops.ts", "    this.rows.align.style.display = now.align === null ? \"none\" : \"block\";", "    this.rows.align.style.display = \"block\";", "shows only the rows the field's kind has"),
    Mutation("field panel: open with the last field's complaint", "src/lib/fieldprops.ts", "    this.heading.textContent = `Properties of ${now.name}`;\n    this.problem.textContent = \"\";", "    this.heading.textContent = `Properties of ${now.name}`;", "stays open and says why when a value cannot be the field's, then takes the correction"),
    Mutation("field panel: close on a click inside it", "src/lib/fieldprops.ts", "      if (event.target === this.backdrop) this.settle(null);", "      this.settle(null);", "does not dismiss on a click inside the panel, and keeps keys from the window"),
    Mutation("field panel: let a typed letter reach the window", "src/lib/fieldprops.ts", "        // A letter typed here is the panel's, not a command's.\n        event.stopPropagation();", "", "does not dismiss on a click inside the panel, and keeps keys from the window"),
    Mutation("field panel: apply what cannot be the field's", "src/lib/fieldprops.ts", "    if (\"problem\" in to) {\n      this.problem.textContent = to.problem;\n      return;\n    }", "", "stays open and says why when a value cannot be the field's, then takes the correction"),
    Mutation("form edit command: offer it on a document with no form", "src/lib/appcommands.ts", "      enabled: () => withDocument() && actions.savedFields() > 0 && !actions.formEditing(),", "      enabled: () => withDocument() && !actions.formEditing(),", "offers changing the document's fields when it has some, and finishing while that is on"),
    Mutation("form edit command: offer it while it is on", "src/lib/appcommands.ts", "      enabled: () => withDocument() && actions.savedFields() > 0 && !actions.formEditing(),", "      enabled: () => withDocument() && actions.savedFields() > 0,", "offers changing the document's fields when it has some, and finishing while that is on"),
    Mutation("form edit command: offer finishing with no document", "src/lib/appcommands.ts", "      enabled: () => withDocument() && actions.formEditing(),", "      enabled: () => actions.formEditing(),", "offers changing the document's fields when it has some, and finishing while that is on"),
    Mutation("form edit command: turn it on to finish", "src/lib/appcommands.ts", "      run: () => actions.setFormEditing(false),", "      run: () => actions.setFormEditing(true),", "offers changing the document's fields when it has some, and finishing while that is on"),
    Mutation("dropdown choices: split at commas", "src/lib/fieldnames.ts", "  const options = typed.split(\";\").map((part) => part.trim());", "  const options = typed.split(\",\").map((part) => part.trim());", "are split at semicolons, with the space round each dropped"),
    Mutation("dropdown choices: keep the space round a choice", "src/lib/fieldnames.ts", "  const options = typed.split(\";\").map((part) => part.trim());", "  const options = typed.split(\";\");", "are split at semicolons, with the space round each dropped"),
    Mutation("dropdown choices: read a semicolon at the end as an empty choice", "src/lib/fieldnames.ts", "  const typed = raw.trim().replace(/;\\s*$/, \"\");", "  const typed = raw.trim();", "are split at semicolons, with the space round each dropped"),
    Mutation("dropdown choices: take a list of none", "src/lib/fieldnames.ts", "  if (typed === \"\") return { problem: \"The choices, with a semicolon between them: Yes; No; Maybe\" };\n", "", "need at least one, none empty and none twice"),
    Mutation("dropdown choices: take an empty choice", "src/lib/fieldnames.ts", "  if (options.some((option) => option === \"\")) return { problem: \"There is an empty choice between two semicolons\" };\n", "", "need at least one, none empty and none twice"),
    Mutation("dropdown choices: take one choice twice", "src/lib/fieldnames.ts", "  if (twice !== undefined) return { problem: `\"${twice}\" is there twice` };\n", "", "need at least one, none empty and none twice"),
    Mutation("dropdown choices: take any number", "src/lib/fieldnames.ts", "  if (options.length > MAX_OPTIONS) return", "  if (false) return", "are held to the most a dropdown offers and the longest a choice may be"),
    Mutation("dropdown choices: refuse the most a dropdown may offer", "src/lib/fieldnames.ts", "  if (options.length > MAX_OPTIONS) return", "  if (options.length >= MAX_OPTIONS) return", "are held to the most a dropdown offers and the longest a choice may be"),
    Mutation("dropdown choices: take a choice of any length", "src/lib/fieldnames.ts", "  if (long !== undefined) return { problem: `A choice is at most ${MAX_OPTION} characters` };\n", "", "are held to the most a dropdown offers and the longest a choice may be"),
    Mutation("dropdown choices: count a choice's length in code units", "src/lib/fieldnames.ts", "  const long = options.find((option) => [...option].length > MAX_OPTION);", "  const long = options.find((option) => option.length > MAX_OPTION);", "are held to the most a dropdown offers and the longest a choice may be"),
    Mutation("dropdown choices: take a control character", "src/lib/fieldnames.ts", "  if (options.some((option) => /\\p{Cc}/u.test(option))) return { problem: \"A choice cannot contain a control character\" };\n", "", "are held to the most a dropdown offers and the longest a choice may be"),
    Mutation("dropdown command: arm the drag with no choices", "src/lib/appcommands.ts", "          if (\"options\" in parsed) actions.drawField(\"dropdown\", parsed.options);", "          if (\"options\" in parsed) actions.drawField(\"dropdown\");", "asks a dropdown for its choices and arms the drag with them"),
    Mutation("dropdown command: arm a text field", "src/lib/appcommands.ts", "          if (\"options\" in parsed) actions.drawField(\"dropdown\", parsed.options);", "          if (\"options\" in parsed) actions.drawField(\"text\", parsed.options);", "asks a dropdown for its choices and arms the drag with them"),
    Mutation("dropdown command: take choices that cannot be a list", "src/lib/appcommands.ts", "          return \"problem\" in parsed ? parsed.problem : null;\n        },\n        preview: (raw: string) => {\n          const parsed = parseChoices(raw);\n          return \"options\" in parsed\n            ? `${parsed.options.length === 1", "          return null;\n        },\n        preview: (raw: string) => {\n          const parsed = parseChoices(raw);\n          return \"options\" in parsed\n            ? `${parsed.options.length === 1", "asks a dropdown for its choices and arms the drag with them"),
    Mutation("dropdown command: say 1 choices", "src/lib/appcommands.ts", "            ? `${parsed.options.length === 1 ? \"1 choice\" : `${parsed.options.length} choices`}.", "            ? `${`${parsed.options.length} choices`}.", "asks a dropdown for its choices and arms the drag with them"),
    Mutation("redact list: say nothing about a picture that stays elsewhere", "src/lib/redactlist.ts", "  const elsewhere = pictures > 0 ? (plan?.shared?.length ?? 0) : 0;", "  const elsewhere = 0;", "says when a picture it takes is drawn elsewhere too, and stays there"),
    Mutation("redact list: leave the cut out when something is also removed", "src/lib/redactlist.ts", "  const sentence = cuts > 0 ? `${removes}, and cuts ${cut}` : removes;", "  const sentence = removes;", "says a line that crosses the region is cut at its edge, and counts them"),
    Mutation("compress: accept a resolution below the lowest", "src/lib/compress.ts", "  if (d === null || d < DPI_MIN || d > DPI_MAX) {", "  if (d === null || d > DPI_MAX) {", "reads the reader's own numbers, and says what is wrong with them"),
    Mutation("compress: accept a resolution above the highest", "src/lib/compress.ts", "  if (d === null || d < DPI_MIN || d > DPI_MAX) {", "  if (d === null || d < DPI_MIN) {", "reads the reader's own numbers, and says what is wrong with them"),
    Mutation("compress: accept a quality of nothing", "src/lib/compress.ts", "  if (q === null || q < 1 || q > 100) {", "  if (q === null || q > 100) {", "reads the reader's own numbers, and says what is wrong with them"),
    Mutation("compress: take anything Number reads for a number", "src/lib/compress.ts", "    /^\\d{1,5}$/.test(text.trim()) ? Number(text.trim()) : null;", "    text.trim() !== \"\" && Number.isFinite(Number(text)) ? Number(text.trim()) : null;", "reads the reader's own numbers, and says what is wrong with them"),
    Mutation("compress: round the share saved to the nearest", "src/lib/compress.ts", "  return Math.floor(((before - after) * 100) / before);", "  return Math.round(((before - after) * 100) / before);", "rounds the share saved down and never below nothing"),
    Mutation("compress: call a copy of the same size smaller", "src/lib/compress.ts", "  return shrinkage.bytesAfter < shrinkage.bytesBefore;", "  return shrinkage.bytesAfter <= shrinkage.bytesBefore;", "puts the size and the share beside a choice, or says it is not smaller"),
    Mutation("compress: say pictures change when none does", "src/lib/compress.ts", "      : shrinkage.picturesChanged === 0\n        ? \" No picture changes.\"", "      : shrinkage.picturesChanged < 0\n        ? \" No picture changes.\"", "says under the choices where the size goes and how many pictures change"),
    Mutation("compress: report a reduction that is none", "src/lib/compress.ts", "  if (sample.dpiBefore > 0 && sample.dpiAfter > 0 && sample.dpiAfter < sample.dpiBefore) {", "  if (sample.dpiBefore > 0 && sample.dpiAfter > 0) {", "says what the two pictures are and how far the picture is reduced"),
    Mutation("compress: keep .pdf in the suggested stem", "src/lib/compress.ts", "  return `${basename(path).replace(/\\.pdf$/i, \"\")} smaller.pdf`;", "  return `${basename(path).replace(/\\.pdf$/, \"\")} smaller.pdf`;", "suggests a name and says the size once the copy is written"),
    Mutation("compress dialog: offer to save a copy that is not smaller", "src/lib/compressdialog.ts", "    this.save.disabled = !(shrinkage && isSmaller(shrinkage));", "    this.save.disabled = !shrinkage;", "does not offer to save a copy that would not be smaller"),
    Mutation("compress dialog: save a copy that is not smaller when asked", "src/lib/compressdialog.ts", "    if (!isSmaller(known.shrinkage)) return null;", "", "does not offer to save a copy that would not be smaller"),
    Mutation("compress dialog: wait behind an estimate from the opening before", "src/lib/compressdialog.ts", "    this.queue = Promise.resolve();\n    this.heading.textContent", "    this.heading.textContent", "drops an answer that arrives after the dialog was closed and opened again"),
    Mutation("compress dialog: keep the pictures after it closes", "src/lib/compressdialog.ts", "      this.known = new Map();\n      this.before.removeAttribute(\"src\");\n      this.after.removeAttribute(\"src\");", "      this.known = new Map();", "is dismissed by Cancel, with no choice and no picture kept"),
    Mutation("compress dialog: ask for numbers that are no numbers", "src/lib/compressdialog.ts", "    if (\"problem\" in read) {\n      this.customFor = null;", "    if (false) {\n      this.customFor = null;", "says what is wrong with numbers that are not numbers, and asks nothing"),
    Mutation("compress dialog: save the middle choice whatever is selected", "src/lib/compressdialog.ts", "    const choice = CHOICES.find((choice) => choice.id === this.selected);", "    const choice = CHOICES[2];", "shows the selected choice's size and the page before and after"),
    Mutation("compress: write a smaller copy without consent", "src/lib/edits.ts", "  async compressCopy(source: string, path: string, pictures: Pictures | null): Promise<Copied> {\n    await this.beforeWrite();", "  async compressCopy(source: string, path: string, pictures: Pictures | null): Promise<Copied> {", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("compress: ask consent to work out an estimate", "src/lib/edits.ts", "  async compressEstimate(source: string, pictures: Pictures | null): Promise<Shrinkage> {", "  async compressEstimate(source: string, pictures: Pictures | null): Promise<Shrinkage> {\n    await this.beforeWrite();", "asks what a smaller copy comes to without asking for consent, and writes one with it"),
    Mutation("compress: write the copy with no settings", "src/lib/edits.ts", "    return await call(\"compress_copy\", { doc: this.doc, source, path, pictures });", "    return await call(\"compress_copy\", { doc: this.doc, source, path, pictures: null });", "asks what a smaller copy comes to without asking for consent, and writes one with it"),
    Mutation("compress dialog: show whatever URL arrives as a picture", "src/lib/compressdialog.ts", "    if (sample && isPicture(sample.before) && isPicture(sample.after)) {", "    if (sample) {", "puts nothing in a picture but a PNG the backend encoded"),
    Mutation("tabs: reopen every tab whatever the preference says", "src/lib/tabrestore.ts", "  const tabs = session.restore_tabs ? recorded(session) : [];", "  const tabs = recorded(session);", "reopens only the last document unless every tab was asked for"),
    Mutation("tabs: show the tab read last, not the one that was showing", "src/lib/tabrestore.ts", "  if (active && tabs.includes(active)) return active;\n", "", "shows the tab that was showing and opens the others behind it, in tab order"),
    Mutation("tabs: show a tab that was not open", "src/lib/tabrestore.ts", "  if (active && tabs.includes(active)) return active;", "  if (active) return active;", "falls back to the tab read most recently, then to the first"),
    Mutation("tabs: open the front tab a second time behind itself", "src/lib/tabrestore.ts", "    behind: tabs.filter((path) => path !== front),", "    behind: [...tabs],", "shows the tab that was showing and opens the others behind it, in tab order"),
    Mutation("tabs: open a handed file a second time behind itself", "src/lib/tabrestore.ts", "      behind: tabs.filter((path) => !handed.includes(path)),", "      behind: [...tabs],", "shows what the launcher handed over, with the tabs behind it"),
    Mutation("tabs: leave a handed file out of the order", "src/lib/tabrestore.ts", "      order: tabs.length ? [...tabs, ...handed.filter((path) => !tabs.includes(path))] : [],", "      order: [...tabs],", "shows what the launcher handed over, with the tabs behind it"),
    Mutation("tabs: offer to reopen a tab that is open", "src/lib/tabrestore.ts", "  return recorded(session).filter((path) => !isOpen(path));", "  return recorded(session);", "lists the ones not open now, whatever the preference says"),
    Mutation("tabs: stop at the first tab that will not open", "src/lib/tabrestore.ts", "      refused.push(path);\n    }", "      refused.push(path);\n      break;\n    }", "carries on past a tab that will not open, and names it"),
    Mutation("tabs: reorder the strip when no order was given", "src/lib/tabrestore.ts", "  if (order.length) host.arrange(order);", "  host.arrange(order);", "carries on past a tab that will not open, and names it"),
    Mutation("tabs: leave the window empty over a strip of tabs", "src/lib/tabrestore.ts", "  if (!host.showing()) await host.showFirst();\n", "", "shows a tab when the one that should be showing did not open"),
    Mutation("tabs: switch to the first tab whatever is showing", "src/lib/tabrestore.ts", "  if (!host.showing()) await host.showFirst();", "  await host.showFirst();", "opens each in turn, then orders the strip"),
    Mutation("tabs: count the named tab among the others", "src/lib/tabrestore.ts", "${refused.length - 1} more. ", "${refused.length} more. ", "says which tabs could not be reopened"),
    Mutation("tab recorder: write the same list twice", "src/lib/tabrestore.ts", "    if (key === this.last) return;\n", "", "writes a change once, and one write at a time"),
    Mutation("tab recorder: write without waiting for the write before", "src/lib/tabrestore.ts", "    this.queue = this.queue.then(write).catch((why: unknown) => {", "    this.queue = write().catch((why: unknown) => {", "writes a change once, and one write at a time"),
    Mutation("tab recorder: stop at a failed write", "src/lib/tabrestore.ts", "    this.queue = this.queue.then(write).catch((why: unknown) => {", "    this.queue = this.queue.then(write); void ((why: unknown) => {", "keeps writing after a write fails"),
    Mutation("tab recorder: remember a record whose write failed as written", "src/lib/tabrestore.ts", "      if (this.last === key) this.last = null;\n", "", "sends a record again when the write of it failed"),
    Mutation("tab recorder: a failed write forgets whatever was noted last", "src/lib/tabrestore.ts", "      if (this.last === key) this.last = null;", "      this.last = null;", "does not forget a later record because an earlier write failed"),
    Mutation("tab recorder: a failed write leaves no line", "src/lib/tabrestore.ts", "      console.warn(`could not record the open tabs: ${String(why)}`);\n", "", "keeps writing after a write fails"),
    Mutation("tab recorder: miss a change of the tab showing", "src/lib/tabrestore.ts", "    const key = JSON.stringify([paths, active, sides]);", "    const key = JSON.stringify([paths, sides]);", "writes when only the tab showing changed, or only the order"),
    Mutation("tab recorder: write the caller's live list", "src/lib/tabrestore.ts", "    const list = [...paths];", "    const list = paths as string[];", "copies the list, so a later change to it is not what gets written"),
    Mutation("tab recorder: write while a launch is opening tabs", "src/lib/tabrestore.ts", "    if (this.held) {\n      this.waiting = [[...paths], active, sides && { ...sides, right: [...sides.right] }];\n      return;\n    }\n", "", "writes nothing while a launch is still opening tabs, then the final list"),
    Mutation("tab recorder: hold the caller's live list", "src/lib/tabrestore.ts", "      this.waiting = [[...paths], active, sides && { ...sides, right: [...sides.right] }];", "      this.waiting = [paths as string[], active, sides];", "writes nothing while a launch is still opening tabs, then the final list"),
    Mutation("tab recorder: drop what was noted while held", "src/lib/tabrestore.ts", "    if (waiting) this.note(waiting[0], waiting[1], waiting[2]);\n", "", "writes nothing while a launch is still opening tabs, then the final list"),
    Mutation("tab recorder: miss a change of the two sides", "src/lib/tabrestore.ts", "    const key = JSON.stringify([paths, active, sides]);", "    const key = JSON.stringify([paths, active]);", "writes the two sides with the tabs, and when only they changed"),
    Mutation("tab recorder: write the caller's live list of the right side", "src/lib/tabrestore.ts", "    const kept = sides && { ...sides, right: [...sides.right] };", "    const kept = sides;", "writes the two sides with the tabs, and when only they changed"),
    Mutation("tab recorder: drop the sides noted while held", "src/lib/tabrestore.ts", "    if (waiting) this.note(waiting[0], waiting[1], waiting[2]);", "    if (waiting) this.note(waiting[0], waiting[1]);", "writes the two sides with the tabs, and when only they changed"),
    Mutation("launch: leave the two sides out of the plan", "src/lib/tabrestore.ts", "    sides: session.sides ?? null,", "    sides: null,", "carries the two sides when the tabs come back, and only then"),
    Mutation("sides to restore: split with nothing left on the left", "src/lib/tabrestore.ts", "  if (right.length === 0 || right.length === open.length) return null;", "  if (right.length === 0) return null;", "are none when a whole side would not open"),
    Mutation("sides to restore: split with nothing on the right", "src/lib/tabrestore.ts", "  if (right.length === 0 || right.length === open.length) return null;", "  if (right.length === open.length) return null;", "are none when a whole side would not open"),
    Mutation("sides to restore: without a document on screen", "src/lib/tabrestore.ts", "  if (!sides || active === null || !open.includes(active)) return null;", "  if (!sides) return null;", "are none without a record, or without a document on screen"),
    Mutation("sides to restore: a tab beside on the reader's own side", "src/lib/tabrestore.ts", "    && right.includes(sides.beside) !== right.includes(active) ? sides.beside : null;", "    ? sides.beside : null;", "name a tab beside only on the side the reader is not in"),
    Mutation("sides to restore: a tab beside that did not open", "src/lib/tabrestore.ts", "  const beside = sides.beside !== null && open.includes(sides.beside)\n", "  const beside = sides.beside !== null\n", "name a tab beside only on the side the reader is not in"),
    Mutation("sides to restore: the right side in the order recorded", "src/lib/tabrestore.ts", "  const right = open.filter((path) => sides.right.includes(path));", "  const right = sides.right.filter((path) => open.includes(path));", "are the recorded ones when every tab opened, the right in tab order"),
    Mutation("panes: rebuild the reader's viewer when their document is on the right", "src/lib/panes.ts", "    if (area >= 0 && this.slotOf(this.sideOf(active)) !== area) this.#flipped = !this.#flipped;\n", "", "keeps the reader's viewer in its area when their document is on the right"),
    Mutation("panes: the tab beside takes the front of the reader's side", "src/lib/panes.ts", "[active, beside].find((id) => mine.includes(id))", "[beside, active].find((id) => mine.includes(id))", "shows the first tab of a side whose front tab is not named or not open"),
    Mutation("panes: leave the reader on the left after arranging", "src/lib/panes.ts", "    if (order.includes(active)) this.#focused = this.sideOf(active);\n", "", "keeps the reader's viewer in its area when their document is on the right"),
    Mutation("tab drag: drag from the first pixel", "src/lib/tabdrag.ts", "    if (!this.#carried && Math.hypot(x - press.x, y - press.y) < SLOP) return null;\n", "", "is a click until the pointer has travelled"),
    Mutation("tab drag: stop dragging back at the start", "src/lib/tabdrag.ts", "    if (!this.#carried && Math.hypot(", "    if (Math.hypot(", "is dragged once it has, and stays dragged back at the start"),
    Mutation("tab drag: drop a tab on its own side", "src/lib/tabdrag.ts", "  return layout.split && side === layout.from ? null : side;", "  return side;", "is the other side with two showing, divided where the divider is"),
    Mutation("tab drag: divide two sides in the middle", "src/lib/tabdrag.ts", "(layout.split ? layout.share : 0.5)", "0.5", "is the other side with two showing, divided where the divider is"),
    Mutation("tab drag: drop above the pages", "src/lib/tabdrag.ts", "  if (y < area.top || y >= area.top + area.height) return null;\n", "", "is nowhere outside the page area"),
    Mutation("tab drag: drop beside the pages", "src/lib/tabdrag.ts", "  if (x < area.left || x >= area.left + area.width) return null;\n", "", "is nowhere outside the page area"),
    Mutation("tab drag: drop the only tab", "src/lib/tabdrag.ts", "  if (layout.tabs < 2 || area.width <= 0 || area.height <= 0) return null;", "  if (area.width <= 0 || area.height <= 0) return null;", "is nowhere with one tab open"),
    Mutation("tab drag: drop after a cancel", "src/lib/tabdrag.ts", "    this.#press = null;\n    this.#carried = null;\n  }\n}", "    this.#press = null;\n  }\n}", "drops nothing when the drag is cancelled, and moves nothing unpressed"),
    Mutation("tab drag: carry the last tab into a second press", "src/lib/tabdrag.ts", "    this.#press = { id, x, y };\n    this.#carried = null;", "    this.#press = { id, x, y };", "starts again from a second press"),
    Mutation("tab recorder: stay held after release", "src/lib/tabrestore.ts", "    this.held = false;\n", "", "writes nothing while a launch is still opening tabs, then the final list"),
    Mutation("panes: a tab opened behind joins the left whatever side the reader is in", "src/lib/panes.ts", "    if (this.#focused === \"right\") this.#right.add(id);\n", "", "adds a tab opened behind to the focused side without showing it"),
    Mutation("panes: a tab brought to the front leaves the reader where they were", "src/lib/panes.ts", "    this.#front[side] = id;\n    this.#focused = side;\n", "    this.#front[side] = id;\n", "brings a tab to the front of the side it is on, and focuses that side"),
    Mutation("panes: a save's new handle loses its side", "src/lib/panes.ts", "    if (this.#right.delete(before)) this.#right.add(after);\n", "", "keeps side, front and area through a save's new handle"),
    Mutation("panes: a save's new handle loses its place in front", "src/lib/panes.ts", "      if (this.#front[side] === before) this.#front[side] = after;\n", "", "keeps side, front and area through a save's new handle"),
    Mutation("panes: a save's new handle loses its page area", "src/lib/panes.ts", "      if (this.#mounted[slot] === before) this.#mounted[slot] = after;\n", "", "keeps side, front and area through a save's new handle"),
    Mutation("panes: focus a side that shows nothing", "src/lib/panes.ts", "    if (this.#front[side] < 0) return false;\n", "", "cannot focus the side that shows nothing"),
    Mutation("panes: the side a tab left keeps showing it", "src/lib/panes.ts", "      if (this.#front[from] === id) this.#front[from] = this.#neighbour(id, from, order);\n", "", "splits, shows the next tab where it was, and focuses the moved one"),
    Mutation("panes: a moved tab is not in front of its new side", "src/lib/panes.ts", "    this.#front[to] = id;\n    this.#focused = to;", "    this.#focused = to;", "splits, shows the next tab where it was, and focuses the moved one"),
    Mutation("panes: a moved tab leaves the reader behind", "src/lib/panes.ts", "    this.#front[to] = id;\n    this.#focused = to;", "    this.#front[to] = id;", "splits, shows the next tab where it was, and focuses the moved one"),
    Mutation("panes: a tab moved back stays on the right", "src/lib/panes.ts", "      else this.#right.delete(id);\n", "", "ends the split when the right side's only tab moves back"),
    Mutation("panes: a closed tab stays in front of its side", "src/lib/panes.ts", "    if (this.#front[side] === id) this.#front[side] = this.#neighbour(id, side, order);\n", "", "shows a side's neighbour when its front tab is closed"),
    Mutation("panes: every tab gone keeps the right side", "src/lib/panes.ts", "    this.#right.clear();\n    this.#front.left = NONE as Id;", "    this.#front.left = NONE as Id;", "forgets everything when every tab is gone"),
    Mutation("panes: every tab gone leaves the reader on the right", "src/lib/panes.ts", "    this.#front.right = NONE as Id;\n    this.#focused = \"left\";\n  }\n\n  /**\n   * Exchanges", "    this.#front.right = NONE as Id;\n  }\n\n  /**\n   * Exchanges", "forgets everything when every tab is gone"),
    Mutation("panes: swap with one side", "src/lib/panes.ts", "    if (!this.split) return;\n", "", "does not swap"),
    Mutation("panes: a swap leaves the page areas with their sides", "src/lib/panes.ts", "    this.#focused = otherSide(this.#focused);\n    this.#flipped = !this.#flipped;", "    this.#focused = otherSide(this.#focused);", "swaps sides without rebuilding either viewer"),
    Mutation("panes: a swap moves the reader to the other document", "src/lib/panes.ts", "    this.#focused = otherSide(this.#focused);\n    this.#flipped = !this.#flipped;", "    this.#flipped = !this.#flipped;", "swaps sides without rebuilding either viewer"),
    Mutation("panes: a swap leaves each side's front tab", "src/lib/panes.ts", "    [this.#front.left, this.#front.right] = [this.#front.right, this.#front.left];\n", "", "swaps sides without rebuilding either viewer"),
    Mutation("panes: a torn down viewer still holds its area", "src/lib/panes.ts", "      if (this.#mounted[slot] === id) this.#mounted[slot] = NONE as Id;\n", "", "ends the split when the right side's only tab moves back"),
    Mutation("panes: plan to mount what is mounted", "src/lib/panes.ts", "      if (wanted === there) continue;\n", "", "changes focus without mounting anything"),
    Mutation("panes: plan to leave the viewer that is replaced", "src/lib/panes.ts", "      if (there >= 0) plan.unmount.push({ id: there, slot });\n", "", "asks for the tab brought to the front to replace the one mounted"),
    Mutation("panes: plan to mount no document", "src/lib/panes.ts", "      if (wanted >= 0) plan.mount.push({ id: wanted, slot });", "      plan.mount.push({ id: wanted, slot });", "ends the split when the right side's only tab is closed"),
    Mutation("panes: the tab before takes a front tab's place", "src/lib/panes.ts", "rest[Math.min(at, rest.length - 1)]", "rest[Math.max(0, at - 1)]", "shows the tab after it when a tab between two others moves"),
    Mutation("panes: rebuild the viewer when the left side empties", "src/lib/panes.ts", "      this.#front.right = NONE as Id;\n      this.#flipped = !this.#flipped;\n      this.#focused = \"left\";", "      this.#front.right = NONE as Id;\n      this.#focused = \"left\";", "makes the right side the left one when the left empties, keeping its viewer"),
    Mutation("panes: leave the reader in a side that shows nothing", "src/lib/panes.ts", "      this.#focused = otherSide(this.#focused);\n    }\n  }\n}", "    }\n  }\n}", "ends the split when the right side's only tab is closed"),
    Mutation("panes: draw the left side in the first area whatever was swapped", "src/lib/panes.ts", "return (side === \"left\") !== this.#flipped ? 0 : 1;", "return side === \"left\" ? 0 : 1;", "swaps sides without rebuilding either viewer"),
    Mutation("partner: a longer name wins over one typed in full", "src/lib/panes.ts", "  if (exact >= 0 && lower.lastIndexOf(typed) === exact) return { index: exact };\n", "", "takes a name typed in full over a longer one containing it"),
    Mutation("partner: choose the first of two documents with one name", "src/lib/panes.ts", "  if (exact >= 0 && lower.lastIndexOf(typed) === exact) return { index: exact };", "  if (exact >= 0) return { index: exact };", "does not choose between two documents with one name"),
    Mutation("partner: nothing typed chooses the first of several", "src/lib/panes.ts", "    return names.length === 1 ? { index: 0 } : { problem: \"Type part of a document's name\" };", "    return { index: 0 };", "answers nothing typed only when there is one document to choose"),
    Mutation("partner: choose the first of several that match", "src/lib/panes.ts", "  if (matching.length > 1) {", "  if (matching.length > 99) {", "asks for more when several match, and says when none does"),
    Mutation("partner: offer four names", "src/lib/panes.ts", "names.slice(0, 3)", "names.slice(0, 4)", "offers the first three names and counts the rest"),
    Mutation("scroll lock: a place before the first page", "src/lib/syncscroll.ts", "  const held = Math.max(0, line);", "  const held = line;", "holds a place before the first page at the first page's top"),
    Mutation("scroll lock: lock with the offset turned round", "src/lib/syncscroll.ts", "    this.#ahead = lineOf(second.at) - lineOf(first.at);", "    this.#ahead = lineOf(first.at) - lineOf(second.at);", "keeps the second as far ahead as it was when they were locked"),
    Mutation("scroll lock: lead from a frame that moved nothing", "src/lib/syncscroll.ts", "    if (before !== undefined && Math.abs(line - before) < SAME) return null;\n", "", "answers nothing for a frame that moved nothing"),
    Mutation("scroll lock: send a move back where it came from", "src/lib/syncscroll.ts", "    if (how.following) return null;\n", "", "does not send a move back to the document it came from"),
    Mutation("scroll lock: keep the old offset after a side scrolled alone", "src/lib/syncscroll.ts", "      this.#ahead = leadsFirst ? partner - line : line - partner;\n", "", "takes a new offset from a side scrolled alone, and moves nothing for it"),
    Mutation("scroll lock: move the partner of a side scrolled alone", "src/lib/syncscroll.ts", "      return null;\n    }\n    return readingAt(leadsFirst", "    }\n    return readingAt(leadsFirst", "takes a new offset from a side scrolled alone, and moves nothing for it"),
    Mutation("scroll lock: take the second document's offset as the first's", "src/lib/syncscroll.ts", "      this.#ahead = leadsFirst ? partner - line : line - partner;", "      this.#ahead = partner - line;", "takes the offset the same way when the second document is the one scrolled alone"),
    Mutation("scroll lock: put the first document ahead when the second leads", "src/lib/syncscroll.ts", "    return readingAt(leadsFirst ? line + this.#ahead : line - this.#ahead);", "    return readingAt(line + this.#ahead);", "keeps the first as far behind when the second leads"),
    Mutation("scroll lock: stay locked after release", "src/lib/syncscroll.ts", "    this.#pair = null;\n", "", "answers nothing once released"),
    Mutation("scroll lock: a stranger has a partner", "src/lib/syncscroll.ts", "    if (!this.#pair || !this.#pair.includes(id)) return -1 as Id;", "    if (!this.#pair) return -1 as Id;", "names each document's partner, and knows a stranger"),
    Mutation("scroll lock: aim the first document ahead of the second", "src/lib/syncscroll.ts", "    return readingAt(this.#pair[0] === id ? line + this.#ahead : line - this.#ahead);", "    return readingAt(line + this.#ahead);", "is aimed where the offset says, moved or not"),
    Mutation("scroll lock: forget where a partner was put", "src/lib/syncscroll.ts", "    this.#last.set(id, lineOf(at));\n    this.#zoom.set(id, zoom);\n  }", "    this.#zoom.set(id, zoom);\n  }", "does not lead from the place and zoom it was put at"),
    Mutation("scroll lock: forget the zoom a partner was put at", "src/lib/syncscroll.ts", "    this.#last.set(id, lineOf(at));\n    this.#zoom.set(id, zoom);\n  }", "    this.#last.set(id, lineOf(at));\n  }", "does not lead from the place and zoom it was put at"),
    Mutation("scroll lock: pass a zoom back", "src/lib/syncscroll.ts", "    if (how.following || how.fitted || before", "    if (how.fitted || before", "does not pass a zoom back, or one the window made"),
    Mutation("scroll lock: pass a zoom the window made", "src/lib/syncscroll.ts", "    if (how.following || how.fitted || before", "    if (how.following || before", "does not pass a zoom back, or one the window made"),
    Mutation("scroll lock: pass a zoom that did not change", "src/lib/syncscroll.ts", "    return Math.abs(factor - 1) < SAME ? null : factor;", "    return factor;", "passes nothing when the zoom did not change"),
    Mutation("scroll lock: pass the factor upside down", "src/lib/syncscroll.ts", "    const factor = zoom / before;", "    const factor = before / zoom;", "passes the factor of a zoom the reader set"),
    Mutation("stage: park and leave the variables as they were", "src/lib/livedocument.ts", "    this.#parked.set(id, capture(this.#slots));\n    install(this.#slots, this.#blank());\n  }", "    this.#parked.set(id, capture(this.#slots));\n  }", "parks the focused document and leaves the variables blank"),
    Mutation("stage: park no document", "src/lib/livedocument.ts", "    if (id < 0) return;\n    this.#parked.set(id, capture(this.#slots));", "    this.#parked.set(id, capture(this.#slots));", "holds nothing before a document is opened"),
    Mutation("stage: drop the document the reader leaves", "src/lib/livedocument.ts", "    if (leaving >= 0) this.#parked.set(leaving, capture(this.#slots));\n", "", "swaps two documents and keeps what each one held"),
    Mutation("stage: keep a second record of the document focused", "src/lib/livedocument.ts", "    this.#parked.delete(id);\n    const leaving", "    const leaving", "swaps two documents and keeps what each one held"),
    Mutation("stage: hand a parked record over twice", "src/lib/livedocument.ts", "    this.#parked.delete(id);\n    return record;", "    return record;", "hands a parked record over once"),
    Mutation("stage: refuse work for the focused document", "src/lib/livedocument.ts", "    if (id === this.focused) return { ran: true, value: work() };\n", "", "runs directly for the focused document"),
    Mutation("stage: run work for no document", "src/lib/livedocument.ts", "    if (id < 0) return { ran: false };\n", "", "holds nothing before a document is opened"),
    Mutation("stage: lose what work wrote to a parked document", "src/lib/livedocument.ts", "      else if (kept) this.#parked.set(id, capture(this.#slots));\n      else this.#parked.delete(id);", "      else if (!kept) this.#parked.delete(id);", "lends the variables to a parked document and keeps what the work wrote"),
    Mutation("stage: keep the record of a document the work ended", "src/lib/livedocument.ts", "      else if (kept) this.#parked.set(id, capture(this.#slots));\n      else this.#parked.delete(id);", "      else this.#parked.set(id, capture(this.#slots));", "drops the record of a document the work ended"),
    Mutation("stage: leave the lent document in the variables", "src/lib/livedocument.ts", "      install(this.#slots, outside.record);\n", "", "puts the focused document back when the work throws"),
    Mutation("stage: never say the variables are lent", "src/lib/livedocument.ts", "    this.#lenders.push(outside);\n", "", "says the variables are lent only while a parked document has them"),
    Mutation("stage: stay lent after the work", "src/lib/livedocument.ts", "      this.#lenders.pop();\n", "", "says the variables are lent only while a parked document has them"),
    Mutation("stage: hold no document", "src/lib/livedocument.ts", "    if (id < 0) return false;\n    return id === this.focused", "    return id === this.focused", "holds nothing before a document is opened"),
    Mutation("stage: drop work for the focused document asked for inside a lend", "src/lib/livedocument.ts", "    for (const entry of this.#lenders) if (entry.id === id) lender = entry;\n", "", "runs work for the focused document asked for inside a lend to the other one, and both records come back"),
    Mutation("stage: lose what the focused document's work wrote inside a lend", "src/lib/livedocument.ts", "      if (lender) lender.record = kept ? capture(this.#slots) : this.#blank();", "      if (lender) lender.record = kept ? lender.record : this.#blank();", "runs work for the focused document asked for inside a lend to the other one, and both records come back"),
    Mutation("stage: put back a focused document that work inside a lend ended", "src/lib/livedocument.ts", "      if (lender) lender.record = kept ? capture(this.#slots) : this.#blank();", "      if (lender) lender.record = kept ? capture(this.#slots) : lender.record;", "leaves the variables blank when work inside a lend ends the focused document"),
    Mutation("stage: say the variables are lent while the focused document has them back", "src/lib/livedocument.ts", "    this.#home = lender !== undefined && lender === this.#lenders[0];", "    this.#home = false;", "runs work for the focused document asked for inside a lend to the other one, and both records come back"),
    Mutation("stage: a document that is lending is not held", "src/lib/livedocument.ts", " || this.#lenders.some((lender) => lender.id === id)", "", "runs work for the focused document asked for inside a lend to the other one, and both records come back"),
    Mutation("live document: a blank one has a search waiting", "src/lib/livedocument.ts", "    findTimer: 0,", "    findTimer: 1,", "names no document"),
    Mutation("scoped: leave a nested group of callbacks unwrapped", "src/lib/livedocument.ts", "copy[key] = scoped(value, run);", "copy[key] = value;", "run every function through the runner, nested groups included"),
    Mutation("scoped: copy a class instance", "src/lib/livedocument.ts", "  if (Object.getPrototypeOf(options) !== Object.prototype) return options;\n", "", "hand arrays, class instances and null through untouched"),
    Mutation("scoped: call a function without the runner", "src/lib/livedocument.ts", "    return ((...args: unknown[]) => run(() => call(...args))) as unknown as T;", "    return options;", "do not run for a document the runner refuses"),
    # What an open decides is `documentopen.ts`; `openDocument` in `App.svelte` is
    # the order and the wiring, held as text because nothing imports that file.
    # What the viewer and the sidebar decide while a document mounts is
    # `documentmount.ts`; `buildViewer` and `buildSidebar` hand it what it acts on.
    Mutation("mount: edit a comment on a slot the model no longer has", "src/lib/documentmount.ts", "  if (!object || page === undefined) return null;", "  if (!object) return null;", "addresses nothing on a slot the model no longer has"),
    Mutation("mount: read page 0 as a missing page", "src/lib/documentmount.ts", "  if (!object || page === undefined) return null;", "  if (!object || !page) return null;", "keeps page 0, which is a page and not a missing one"),
    Mutation("mount: edit a comment the file wrote as a direct dictionary", "src/lib/documentmount.ts", "  if (!object || page === undefined) return null;", "  if (page === undefined) return null;", "addresses nothing for a comment written as a direct dictionary"),
    Mutation("mount: take the ring away while the hidden-text list is shown", "src/lib/documentmount.ts", '  if (tab !== "hidden") work.clearRegion();', "  work.clearRegion();", "leaves the ring over a passage while the hidden-text list is shown"),
    Mutation("mount: find the regions' words for every tab", "src/lib/documentmount.ts", '  if (tab === "redactions") work.redactionWords();', "  work.redactionWords();", "takes the ring away, and walks nothing, for any other tab"),
    Mutation("mount: find the comments' words for every tab", "src/lib/documentmount.ts", '  if (tab === "comments") work.commentWords();', "  work.commentWords();", "takes the ring away, and walks nothing, for any other tab"),
    Mutation("mount: call the viewer busy only past one piece of work", "src/lib/documentmount.ts", "  strip?.setViewerBusy(next.pending > 0);", "  strip?.setViewerBusy(next.pending > 1);", "tells the strip the viewer is busy while any work is outstanding"),
    Mutation("mount: leave the inversion off the strip", "src/lib/documentmount.ts", "  strip?.setInvert(next.invert);\n", "", "passes the rotation and the inversion as the status has them"),
    Mutation("component text: count this file among the suites", "src/lib/apptext.test.ts", "    .filter(([name, text]) => name !== SELF && typeof text", "    .filter(([name, text]) => typeof text", "counts a suite that reads the component, and not one that does not"),
    Mutation("open: hand the viewer the first page's size alone", "src/lib/documentopen.ts", "  return [page, ...doc.pages.slice(1)];", "  return [page];", "are every size the open carried, in order"),
    Mutation("open: mount a document that reports no pages", "src/lib/documentopen.ts", "  if (!page) throw new Error(\"document reports no pages\");\n", "", "refuse a document that reports no pages"),
    Mutation("open: the caller's place wins over the tab's own", "src/lib/documentopen.ts", "  const remembered = kept ?? override ?? places.find", "  const remembered = override ?? kept ?? places.find", "is the tab's own place before the caller's, and the caller's before the snapshot"),
    Mutation("open: resume at the snapshot's first place whatever the path", "src/lib/documentopen.ts", "places.find((place) => place.path === path);", "places[0];", "is the snapshot's place for this path and no other's"),
    Mutation("open: resume past the end of a document that got shorter", "src/lib/documentopen.ts", "  return remembered ? clampPlace(remembered, pageCount) : null;", "  return remembered ?? null;", "is fitted to the pages the document has now"),
    Mutation("open: leave the other views on the old model", "src/lib/documentopen.ts", "    twin.edits = edits;\n", "", "are pointed at the new handle and the new model, every one of them"),
    Mutation("open: take the first twin's answer though it has none", "src/lib/documentopen.ts", "  return twinsOf(tabs, tab).map(read).find((held) => held !== undefined);", "  return twinsOf(tabs, tab).map(read)[0];", "is what the first twin that answers holds, and never the view's own"),
    Mutation("open: share with every tab and not only the document's views", "src/lib/documentopen.ts", "  return twinsOf(tabs, tab).map(read).find((held) => held !== undefined);", "  return tabs.map(read).find((held) => held !== undefined);", "is nothing when no twin is mounted, when there is no twin and when there is no tab"),
    Mutation("open: move a tab returned to into the reader's side", "src/lib/documentopen.ts", "  if (returning || replacing !== undefined) panes.fronted(view);\n  else panes.opened(view);", "  panes.opened(view);", "is the side it is already on for a tab returned to, in front and with the reader"),
    Mutation("open: a save's new handle loses the old one's side", "src/lib/documentopen.ts", "  if (replacing !== undefined) panes.replaced(replacing, view);\n", "", "is the side of the handle a save replaced"),
    Mutation("open: a new document does not join the reader's side", "src/lib/documentopen.ts", "  else panes.opened(view);", "  else panes.fronted(view);", "is the side the reader is in for a document opened for the first time"),
    Mutation("open: tell the sides after the tab has left the row", "src/lib/documentopen.ts", "  panes.closed(view, tabs.all.map((entry) => entry.view));\n  tabs.remove(view);", "  tabs.remove(view);\n  panes.closed(view, tabs.all.map((entry) => entry.view));", "leaves the tabs, and its side shows the neighbour it had in the row"),
    Mutation("open: keep the tab of a view that could not be opened", "src/lib/documentopen.ts", "  panes.closed(view, tabs.all.map((entry) => entry.view));\n  tabs.remove(view);", "  panes.closed(view, tabs.all.map((entry) => entry.view));", "leaves the tabs, and its side shows the neighbour it had in the row"),
    Mutation("open: report a document the launch could not put back", "src/lib/documentopen.ts", "  if (resuming) return null;\n", "", "is nothing for a document the launch tried to put back"),
    Mutation("open: show a refusal as an object and not its sentence", "src/lib/documentopen.ts", "  return isOpenRefusal(why) ? why.reason : String(why);", "  return String(why);", "is the refusal's own sentence, and anything else as it is"),
    Mutation("open wiring: build the page table round the module", "src/App.svelte", "      const pages = pageTable(doc);", "      const pages = doc.pages as [PageSize, ...PageSize[]];", "asks this module for each decision, in the function that needs it"),
    Mutation("open wiring: word a failure round the module", "src/App.svelte", "      const said = openFailure(e, resuming);", "      const said = String(e);", "asks this module for each decision, in the function that needs it"),
    # Page ids start again with a model, so a search confined to a selection
    # is not carried onto a model a save or a reload built.
    Mutation("open: the other views keep a search's scope on the new model", "src/lib/documentopen.ts", "    twin.searchScope = null;\n", "", "is gone from the document's other views too, and their words stay"),
    Mutation("open: the new tab takes what the tab it replaces kept", "src/lib/documentopen.ts", "  return from.retained ?? { view, doc, path, edits, place: from.resume, ...freshState() };", "  return from.retained ?? { view, doc, path, edits, place: from.resume, ...freshState(), searchScope: from.replacing?.searchScope ?? null };", "is gone from the tab a save or a reload opens the file again in"),
    Mutation("open: a tab returned to is kept as a new one", "src/lib/documentopen.ts", "  return from.retained ?? { view, doc, path, edits, place: from.resume, ...freshState() };", "  return { view, doc, path, edits, place: from.resume, ...freshState() };", "stays with a tab the reader switched away from and came back to, on the model it had"),
    Mutation("open wiring: build the tab round the module", "src/App.svelte", "    tabs.keep(tabToKeep(view, doc, path, model, from), replacing?.view);", "    tabs.keep(retained ?? { view, doc, path, edits: model, place: from.resume, ...freshState() }, replacing?.view);", "asks this module for each decision, in the function that needs it"),
    Mutation("open wiring: the other views keep the handle that went", "src/App.svelte", "    repoint(twins, doc, model);\n", "", "asks this module for each decision, in the function that needs it"),
    Mutation("open wiring: a second view keeps a list of its own", "src/App.svelte", "    if (shared) covered = shared;\n", "", "asks this module for each decision, in the function that needs it"),
    Mutation("open wiring: a document that would not open keeps its tab", "src/App.svelte", "      { own: acquired >= 0 ? viewOf(acquired) : undefined, replaced, twins }, refreshTabs);", "      { own: undefined, replaced, twins }, refreshTabs);", "asks this module for each decision, in the function that needs it"),
    Mutation("open wiring: the row is not drawn again after a failed open", "src/App.svelte", "      { own: acquired >= 0 ? viewOf(acquired) : undefined, replaced, twins }, refreshTabs);", "      { own: acquired >= 0 ? viewOf(acquired) : undefined, replaced, twins }, () => {});", "asks this module for each decision, in the function that needs it"),
    # Which views a failed open takes with it, and that the row is drawn after
    # the last of them: `dropAbandoned`. Drawn between the two removals, the row
    # kept the other views of a document shown twice.
    Mutation("open: draw the row before the other views have left it", "src/lib/documentopen.ts", "  if (left.replaced) for (const twin of left.twins) dropView(panes, tabs, twin.view);\n  refresh();", "  refresh();\n  if (left.replaced) for (const twin of left.twins) dropView(panes, tabs, twin.view);", "are gone from the row when it is drawn, which is once and after the last of them"),
    Mutation("open: draw the row only when the open had a handle of its own", "src/lib/documentopen.ts", "dropView(panes, tabs, twin.view);\n  refresh();", "dropView(panes, tabs, twin.view);\n  if (left.own !== undefined) refresh();", "are the other views alone when the handle was a tab's own, and the row is drawn then too"),
    Mutation("open: drop the other views of a document that is still on screen", "src/lib/documentopen.ts", "  if (left.replaced) for (const twin of left.twins)", "  for (const twin of left.twins)", "do not include the other views of a document that was never torn down"),
    Mutation("open: keep the views that went down with the document", "src/lib/documentopen.ts", "  if (left.replaced) for (const twin of left.twins) dropView(panes, tabs, twin.view);\n", "", "are the other views alone when the handle was a tab's own, and the row is drawn then too"),
    Mutation("open: keep the tab of the handle a failed open was given", "src/lib/documentopen.ts", "  if (left.own !== undefined) dropView(panes, tabs, left.own);\n", "", "are gone from the row when it is drawn, which is once and after the last of them"),
    # Where a form control is put is `controlRect`, asked as the form's own
    # document: a control lays itself out from its own key handler too.
    Mutation("form control: shown while its document's fields are being changed", "src/lib/savedfields.ts", "  if (editing || !state) return null;\n  return shownAt(widget, state);", "  if (!state) return null;\n  return shownAt(widget, state);", "is nowhere while that document's fields are being changed"),
    Mutation("form control: put where the file has the field whatever was moved", "src/lib/savedfields.ts", "  if (editing || !state) return null;\n  return shownAt(widget, state);", "  if (editing || !state) return null;\n  return widget.display_rect;", "is where its field is shown, by the document's own changes"),
    Mutation("form wiring: a control is placed by whichever document is focused", "src/App.svelte", "      const rect = asDocument(view, () => controlRect(widget, formEditing, edits?.state ?? null)) ?? null;", "      const rect = controlRect(widget, formEditing, edits?.state ?? null);", "places a form control, runs what it says and a press on a place for a signature, as the form's own document"),
    # The buttons a message is shown with are markup, read as text: an offer a
    # rule returns has an arm and a button in the block that draws them.
    Mutation("offers wiring: an offer a rule returns has no button of its own", "src/App.svelte", "              <button data-testid=\"offer-reload\" disabled={copyTaskBusy}", "              <button data-testid=\"reload-offer\" disabled={copyTaskBusy}", "is the set App.svelte draws a button for"),
    Mutation("form wiring: a control's message goes to whichever document is focused", "src/App.svelte", "      (message) => { asDocument(view, () => say(message)); },", "      say,", "places a form control, runs what it says and a press on a place for a signature, as the form's own document"),
    Mutation("form wiring: a signature is started whichever document the reader is in", "src/App.svelte", "        if (!readerIsIn(view, openView, () => focusSide(panes.sideOf(view)))) {", "        if (false) {", "places a form control, runs what it says and a press on a place for a signature, as the form's own document"),
    Mutation("reader: start for a document the reader cannot be taken to", "src/lib/documentopen.ts", "  return view === focused || take();", "  return true;", "starts for another document only when the reader can be taken there"),
    Mutation("reader: take the reader somewhere they already are", "src/lib/documentopen.ts", "  return view === focused || take();", "  return take() || view === focused;", "starts for the document the reader is in without taking them anywhere"),
    # A reload that fails after its document was torn down mounts the tab
    # again, which puts back the message that tab kept. The reason is said
    # again afterwards: `toldAfterFallback`, and `showPanes` asking it.
    Mutation("fallback: lose why the open failed", "src/lib/documentopen.ts", "  if (failed === null || refused === null) return failed ?? refused;", "  if (failed === null || refused === null) return refused;", "is why the open failed, which the tab's own message would otherwise replace"),
    Mutation("fallback: lose why a tab would not mount again", "src/lib/documentopen.ts", "  if (failed === null || refused === null) return failed ?? refused;", "  if (failed === null || refused === null) return failed;", "is why a tab would not mount again, when that is all there is"),
    Mutation("fallback: say the second failure alone when there are two", "src/lib/documentopen.ts", "  return failed === refused ? failed : `${failed}\\n${refused}`;", "  return refused;", "is both when there are both, the open's first and neither twice"),
    Mutation("fallback: say one reason twice", "src/lib/documentopen.ts", "  return failed === refused ? failed : `${failed}\\n${refused}`;", "  return `${failed}\\n${refused}`;", "is both when there are both, the open's first and neither twice"),
    Mutation("fallback: say the second failure first", "src/lib/documentopen.ts", "  return failed === refused ? failed : `${failed}\\n${refused}`;", "  return failed === refused ? failed : `${refused}\\n${failed}`;", "is both when there are both, the open's first and neither twice"),
    Mutation("fallback wiring: the open does not answer what it said", "src/App.svelte", "      failed = said;\n", "", "is said by the function that mounts the tabs again, with what the open answered"),
    Mutation("fallback wiring: mount the tabs again without the reason", "src/App.svelte", "        try { await showPanes(failed); }", "        try { void failed; await showPanes(); }", "is said by the function that mounts the tabs again, with what the open answered"),
    Mutation("fallback wiring: say why only when a tab would not mount again", "src/App.svelte", "    if (told !== null) say(told);", "    if (told !== null && refused !== null) say(told);", "is said by the function that mounts the tabs again, with what the open answered"),
    Mutation("fallback wiring: hand the two reasons over the wrong way round", "src/App.svelte", "    const told = toldAfterFallback(failed, refused);", "    const told = toldAfterFallback(refused, failed);", "is said by the function that mounts the tabs again, with what the open answered"),
    Mutation("open wiring: the callbacks run as whichever document is focused", "src/App.svelte", "    return (work) => asDocument(view, work);", "    return (work) => work();", "builds the viewer's and the sidebar's callbacks to run as the document they were built for"),
    Mutation("stage: a window's setting reaches the focused document alone", "src/lib/livedocument.ts", "    for (const id of this.parked) this.within(id, work);\n", "", "runs once for the focused document and once for each parked one, and keeps what it wrote"),
    Mutation("stage: a window's setting is applied to no document", "src/lib/livedocument.ts", "    if (this.focused >= 0) work();\n    for", "    work();\n    for", "runs for nothing when no document is in the variables"),
    Mutation("settings wiring: invert the focused viewer alone", "src/App.svelte", "    stage.each(() => viewer?.setInverted(invertPages));", "    viewer.setInverted(invertPages);", "sets the page colours and the nib on every mounted viewer"),
    Mutation("settings wiring: give the nib to the focused viewer alone", "src/App.svelte", "    stage.each(() => viewer?.setNib(chosen.pt));", "    viewer?.setNib(chosen.pt);", "sets the page colours and the nib on every mounted viewer"),
    Mutation("follow: draw a frame for the place the viewer is in", "src/lib/viewer.ts", "    if (this.scrollTop === before) return;\n    cancelAnimationFrame(this.frameHandle);", "    cancelAnimationFrame(this.frameHandle);", "draws nothing for the place it is already in"),
    Mutation("follow: leave the move for the next animation frame", "src/lib/viewer.ts", "    cancelAnimationFrame(this.frameHandle);\n    this.running = true;\n    this.tick();\n  }\n\n  /** Scrolls to the very top", "    this.wake();\n  }\n\n  /** Scrolls to the very top", "draws the move before returning, with no animation frame run"),
    Mutation("follow: go to the start for a page past the last one", "src/lib/viewer.ts", "      : this.scroller.maxScroll;\n    const before = this.scrollTop;", "      : 0;\n    const before = this.scrollTop;", "stops at the end for a page past the last one"),
    Mutation("follow: take a share outside the page as it comes", "src/lib/viewer.ts", "this.scroller.pageTopOf(at) + Math.min(1, Math.max(0, fraction)) * pitch", "this.scroller.pageTopOf(at) + fraction * pitch", "holds a share outside the page to the page"),
    Mutation("follow: move a destroyed viewer", "src/lib/viewer.ts", "    if (this.life.ended) return;\n    const at = Math.max(0, Math.floor(page));", "    const at = Math.max(0, Math.floor(page));", "does nothing once destroyed"),
    Mutation("reading: say the top of the page whatever the place", "src/lib/viewer.ts", "    const down = (this.scrollTop - this.scroller.pageTopOf(page)) / pitch;", "    const down = 0;", "arrives at the page and the share of it that was asked for"),
    Mutation("views: start further views among the handles", "src/lib/views.ts", "  #next = FURTHER_VIEWS;", "  #next = 1;", "is a number no handle has for a further view, and a new one each time"),
    Mutation("views: hand the same further view out twice", "src/lib/views.ts", "    return this.#next++ as ViewId;", "    return this.#next as ViewId;", "is a number no handle has for a further view, and a new one each time"),
    Mutation("views: call a first view a further one", "src/lib/views.ts", "  return view >= FURTHER_VIEWS;", "  return view >= 0;", "is the document's handle for its first view"),
    Mutation("tabs: answer the first tab of a file whichever view was asked for", "src/lib/documenttabs.ts", "    return showing.find((tab) => tab.view === prefer) ?? showing[0];", "    return showing[0];", "names a document shown twice by the view asked for, else by the first"),
    Mutation("tabs: answer nothing for a file when the view asked for shows another", "src/lib/documenttabs.ts", "    return showing.find((tab) => tab.view === prefer) ?? showing[0];", "    return showing.find((tab) => tab.view === prefer);", "names a document shown twice by the view asked for, else by the first"),
    Mutation("tabs: put a second view at the end of the row", "src/lib/documenttabs.ts", "    this.entries.splice(index < 0 ? this.entries.length : index + 1, 0, tab);", "    this.entries.push(tab);", "names a document shown twice by the view asked for, else by the first"),
    Mutation("tabs: put a second view before the first", "src/lib/documenttabs.ts", "    this.entries.splice(index < 0 ? this.entries.length : index + 1, 0, tab);", "    this.entries.splice(index < 0 ? this.entries.length : index, 0, tab);", "names a document shown twice by the view asked for, else by the first"),
    Mutation("tabs: add a second view twice", "src/lib/documenttabs.ts", "    if (this.find(tab.view)) return;\n    const index = this.entries.findIndex((entry) => entry.view === after);", "    const index = this.entries.findIndex((entry) => entry.view === after);", "names a document shown twice by the view asked for, else by the first"),
    Mutation("tabs: find a tab by its handle and not its view", "src/lib/documenttabs.ts", "    return this.entries.find((tab) => tab.view === id);", "    return this.entries.find((tab) => (tab as unknown as { doc: { id: number } }).doc.id === id);", "names a document shown twice by the view asked for, else by the first"),
    Mutation("tabs: remove every view of a document with one", "src/lib/documenttabs.ts", "    const index = this.entries.findIndex((tab) => tab.view === id);\n    if (index < 0) return undefined;", "    const index = this.entries.findIndex((tab) => (tab as unknown as { doc: { id: number } }).doc.id === id);\n    if (index < 0) return undefined;", "names a document shown twice by the view asked for, else by the first"),
    Mutation("twins: count a tab as its own twin", "src/lib/documenttabs.ts", "  return tabs.filter((other) => other !== tab && other.edits === tab.edits);", "  return tabs.filter((other) => other.edits === tab.edits);", "finds the other tabs of a document by its edit model, and counts it once"),
    Mutation("twins: call every other tab a twin", "src/lib/documenttabs.ts", "  return tabs.filter((other) => other !== tab && other.edits === tab.edits);", "  return tabs.filter((other) => other !== tab);", "finds the other tabs of a document by its edit model, and counts it once"),
    Mutation("twins: count a document shown twice as two", "src/lib/documenttabs.ts", "  return tabs.filter((tab, index) => tabs.findIndex((other) => other.edits === tab.edits) === index);", "  return [...tabs];", "finds the other tabs of a document by its edit model, and counts it once"),
    Mutation("tabs: move a tab to the end whichever end was asked for", "src/lib/documenttabs.ts", "    if (where === \"start\") this.entries.unshift(tab);\n    else this.entries.push(tab);", "    this.entries.push(tab);", "moves a tab to either end and leaves the one in front in front"),
    Mutation("tabs: move the last tab for a handle that is not open", "src/lib/documenttabs.ts", "    if (index < 0) return;\n    const [tab] = this.entries.splice(index, 1);", "    const [tab] = this.entries.splice(index, 1);", "moves a tab to either end and leaves the one in front in front"),
    Mutation("tabs: add a tab that is already open", "src/lib/documenttabs.ts", "    if (!this.find(tab.view)) this.entries.push(tab);", "    this.entries.push(tab);", "adds a tab behind the one showing, and only once"),
    Mutation("tabs: bring a tab opened behind to the front", "src/lib/documenttabs.ts", "    if (!this.find(tab.view)) this.entries.push(tab);", "    if (!this.find(tab.view)) this.keep(tab);", "adds a tab behind the one showing, and only once"),
    Mutation("tabs: put tabs that were not open last time first", "src/lib/documenttabs.ts", "      return at < 0 ? paths.length : at;", "      return at;", "puts the tabs back in the order they were closed in"),
    Mutation("tab restore: apply every field at every point of the open", "src/lib/documenttabs.ts", "    if (RESTORED_AT[key] === point) one(key);", "    one(key);", "applies every restored field exactly once across the three points of an open"),
    Mutation("tab restore: put the redacted copy back before the message that clears it", "src/lib/documenttabs.ts", '  error: "unmounted",\n  messageSuccess: "unmounted",\n  offers: "unmounted",\n  notice: "unmounted",\n  redactedCopyPath: "unmounted",\n', '  redactedCopyPath: "unmounted",\n  error: "unmounted",\n  messageSuccess: "unmounted",\n  offers: "unmounted",\n  notice: "unmounted",\n', "applies each field at its point, in the order the open depends on"),
    Mutation("tab restore: restore the search before there is a viewer", "src/lib/documenttabs.ts", '  searchOptions: "mounted",', '  searchOptions: "unmounted",', "applies each field at its point, in the order the open depends on"),
    Mutation("tab restore: apply a field that another field already took along", "src/lib/documenttabs.ts", "export function restoredWith(_field: keyof FreshState): () => void {\n  return () => {};", "export function restoredWith(_field: keyof FreshState): () => void {\n  return () => { throw new Error(_field); };", "names the field another is applied with, and does nothing for it"),
    # A search confined to a selection names slots. The viewer and the scan
    # drop it when the pages move, and a tab keeps it with the pages it was
    # taken on, so one changed through the document's other view is not put back.
    Mutation("search scope: keep the scan's scope when the pages move", "src/lib/search.ts", "    this.scope = null;\n    this.clear();", "    this.clear();", "drops the scope with the matches, since both name slots"),
    Mutation("search scope: keep the viewer's scope when the pages move", "src/lib/viewer.ts", "    this.searchScope = null;\n\n    // Two things a reader has made", "\n    // Two things a reader has made", "lets go of a search confined to a selection when the pages move"),
    Mutation("search scope: keep a scope for a search of the whole document", "src/lib/documenttabs.ts", "  return ranges ? { ranges, order: pages.map((page) => page.id) } : null;", "  return { ranges: ranges ?? [], order: pages.map((page) => page.id) };", "keeps nothing for a search of the whole document"),
    Mutation("search scope: put a kept scope back whatever the pages are", "src/lib/documenttabs.ts", "  return kept.order.every((id, slot) => pages[slot]?.id === id) ? kept.ranges : null;", "  return kept.ranges;", "drops the scope when a page went, came or moved while the tab was away"),
    Mutation("search scope: put a kept scope back after a page was added or went at the end", "src/lib/documenttabs.ts", "  if (kept.order.length !== pages.length) return null;\n", "", "drops the scope when a page went, came or moved while the tab was away"),
    Mutation("search scope: never put a kept scope back", "src/lib/documenttabs.ts", "  return kept.order.every((id, slot) => pages[slot]?.id === id) ? kept.ranges : null;", "  return kept.order.every((id, slot) => pages[slot]?.id === id) ? null : kept.ranges;", "puts the scope back while the pages are the ones it was kept with"),
    Mutation("search scope wiring: keep the scope with no pages beside it", "src/App.svelte", "      searchScope: keepScope(viewer.searchScopeRanges, edits.state.pages), sidebarTab", "      searchScope: keepScope(viewer.searchScopeRanges, []), sidebarTab", "is kept with the document's pages and put back through them, in the window"),
    Mutation("search scope wiring: put the scope back against the pages it was kept with", "src/App.svelte", "      scopeToRestore(kept.searchScope, edits?.state.pages ?? [])),", "      scopeToRestore(kept.searchScope, kept.searchScope?.order.map((id) => ({ id })) ?? [])),", "is kept with the document's pages and put back through them, in the window"),
    Mutation("tab commands: offer both launch choices at once", "src/lib/appcommands.ts", "      enabled: () => !actions.restoreTabs(),", "      enabled: () => true,", "offers only the launch choice that is not the current one"),
    Mutation("tab commands: offer the current launch choice", "src/lib/appcommands.ts", "      enabled: () => actions.restoreTabs(),\n      run: () => actions.setRestoreTabs(false),", "      enabled: () => !actions.restoreTabs(),\n      run: () => actions.setRestoreTabs(false),", "offers only the launch choice that is not the current one"),
    Mutation("tab commands: reopen when nothing is left to reopen", "src/lib/appcommands.ts", "      enabled: () => actions.tabsToReopen() > 0 && available(),", "      enabled: () => available(),", "reopens last time's tabs when some are not open, with or without a document"),
    Mutation("tab commands: reopen tabs under a busy document", "src/lib/appcommands.ts", "      enabled: () => actions.tabsToReopen() > 0 && available(),", "      enabled: () => actions.tabsToReopen() > 0,", "withholds the reopening while a document is busy, but not the preference"),
    Mutation("field names: count from nought", "src/lib/fieldnames.ts", "  for (let n = 1; ; n++) {", "  for (let n = 0; ; n++) {", "counts from one, by the kind's word"),
    Mutation("field names: give a several-line field a word of its own", "src/lib/fieldnames.ts", "  multiline: \"Text\",", "  multiline: \"Lines\",", "counts from one, by the kind's word"),
    Mutation("field names: take one past the highest instead of the first free", "src/lib/fieldnames.ts", "    if (!taken.has(name)) return name;", "    if (!taken.has(name) && ![...taken].some((t) => t.startsWith(word) && Number(t.split(\" \")[1]) > n)) return name;", "takes the first number that is free, not one past the highest"),
    Mutation("field names: count a grouped field by its whole name", "src/lib/fieldnames.ts", "  for (const name of formNames) taken.add(name.split(\".\")[0] ?? name);", "  for (const name of formNames) taken.add(name);", "counts the file's fields by their first part and the placed ones by their name"),
    Mutation("field names: count every mark's note as a name", "src/lib/fieldnames.ts", "  for (const mark of marks) if (mark.kind === \"field\") taken.add(mark.note);", "  for (const mark of marks) taken.add(mark.note);", "counts the file's fields by their first part and the placed ones by their name"),
    Mutation("field names: forget the fields placed and not saved", "src/lib/fieldnames.ts", "  for (const mark of marks) if (mark.kind === \"field\") taken.add(mark.note);\n", "", "counts the file's fields by their first part and the placed ones by their name"),
    Mutation("field commands: place one line where several were asked for", "src/lib/appcommands.ts", "      run: () => actions.drawField(\"multiline\"),", "      run: () => actions.drawField(\"text\"),", "arm one drag with the kind each names"),
    Mutation("field commands: place a text field where a checkbox was asked for", "src/lib/appcommands.ts", "      run: () => actions.drawField(\"checkbox\"),", "      run: () => actions.drawField(\"text\"),", "arm one drag with the kind each names"),
    Mutation("field commands: offer a checkbox with no document", "src/lib/appcommands.ts", "      title: \"Add a form field: checkbox...\",\n      enabled: withDocument,", "      title: \"Add a form field: checkbox...\",\n      enabled: () => true,", "need a document to place a field on"),
    Mutation("field popup: ask for a note where a name is meant", "src/lib/markpopup.ts", "    const label = mark.kind === \"field\"\n      ? \"Field name\"", "    const label = false\n      ? \"Field name\"", "asks for a name, on one line, when the mark is a form field"),
    Mutation("field popup: keep the name box three lines high", "src/lib/markpopup.ts", "    this.input.rows = mark.kind === \"field\" || mark.kind === \"link\" ? 1 : 3;", "    this.input.rows = 3;", "asks for a name, on one line, when the mark is a form field"),
    Mutation("field popup: keep asking for a name on the next mark", "src/lib/markpopup.ts", "      : mark.kind === \"link\" ? \"Goes to: a page number or a web address\" : \"Note\";", "      : mark.kind === \"link\" ? \"Goes to: a page number or a web address\" : this.input.placeholder || \"Note\";", "asks for a name, on one line, when the mark is a form field"),
    Mutation("field: draw a placed field as a box", "src/lib/markband.ts", "    case \"field\":\n      return \"field\";", "    case \"field\":\n      return \"outline\";", "draws a placed form field as itself: a frame on its whole rectangle, which can be dragged"),
    Mutation("field: pin a placed field where it was dropped", "src/lib/markband.ts", "    // A form field is placed by a drag and anchored to nothing, like the box.\n    case \"field\":\n", "", "draws a placed form field as itself: a frame on its whole rectangle, which can be dragged"),
    Mutation("field: send a placed field without its kind", "src/lib/edits.ts", "          ...(field ? { field } : {}),\n", "", "sends a form field with its kind and its name, and a kind with nothing else"),
    Mutation("field: outline a placed field in the box's red", "src/lib/markcolors.ts", "  field: [0.15, 0.35, 0.9],", "  field: [0.85, 0.15, 0.15],", "sends a form field with its kind and its name, and a kind with nothing else"),
    Mutation("resize: offer no corner on a form field", "src/lib/markband.ts", "  return kind === \"square\" || kind === \"ellipse\" || kind === \"textbox\" || kind === \"field\" || kind === \"link\";", "  return kind === \"square\" || kind === \"ellipse\" || kind === \"textbox\" || kind === \"link\";", "resizes the four kinds that are one dragged rectangle, and no other"),
    Mutation("resize: offer a corner on every mark", "src/lib/markband.ts", "  return kind === \"square\" || kind === \"ellipse\" || kind === \"textbox\" || kind === \"field\" || kind === \"link\";", "  return true;", "resizes the four kinds that are one dragged rectangle, and no other"),
    Mutation("resize: take the lower left for the corner", "src/lib/markband.ts", "  return Math.abs(point.x - quad.right) <= reach && Math.abs(point.y - quad.bottom) <= reach;", "  return Math.abs(point.x - quad.left) <= reach && Math.abs(point.y - quad.bottom) <= reach;", "takes the corner within reach of the lower right, inside or out"),
    Mutation("resize: take the whole lower edge for the corner", "src/lib/markband.ts", "  return Math.abs(point.x - quad.right) <= reach && Math.abs(point.y - quad.bottom) <= reach;", "  return Math.abs(point.y - quad.bottom) <= reach;", "takes the corner within reach of the lower right, inside or out"),
    Mutation("resize: take the upper right for the corner", "src/lib/markband.ts", "  return Math.abs(point.x - quad.right) <= reach && Math.abs(point.y - quad.bottom) <= reach;", "  return Math.abs(point.x - quad.right) <= reach && Math.abs(point.y - quad.top) <= reach;", "takes the corner within reach of the lower right, inside or out"),
    Mutation("resize: let the corner leave the page", "src/lib/markband.ts", "    right: within(quad.right + dx, quad.left + least, page.width),", "    right: Math.max(quad.right + dx, quad.left + least),", "moves the lower right corner and holds it to the page and to a least size"),
    Mutation("resize: let a rectangle turn inside out", "src/lib/markband.ts", "    bottom: within(quad.bottom + dy, quad.top + least, page.height),", "    bottom: Math.min(quad.bottom + dy, page.height),", "moves the lower right corner and holds it to the page and to a least size"),
    Mutation("resize: push a rectangle off the page to keep its least size", "src/lib/markband.ts", "    Math.min(Math.max(wanted, Math.min(low, high)), high);", "    Math.max(Math.min(wanted, high), low);", "moves the lower right corner and holds it to the page and to a least size"),
    Mutation("resize: move a mark pressed on its corner", "src/lib/viewer.ts", "            grow: corner && whole ? { ...whole } : null,", "            grow: null,", "resizes a box, an ellipse, a text box and a form field, and moves none of them"),
    Mutation("resize: resize a mark pressed anywhere", "src/lib/viewer.ts", "              && onResizeCorner(whole, from, RESIZE_REACH_PX / this.zoom);", "              && true;", "still moves those kinds when they are pressed anywhere else"),
    Mutation("resize: resize a comment or a drawing by its corner", "src/lib/viewer.ts", "            isResizable(own.kind) && placed.quads.length === 1 && whole !== undefined", "            placed.quads.length === 1 && whole !== undefined", "moves a kind that has no corner to drag, pressed on that same corner"),
    Mutation("resize: report a corner drag as a move", "src/lib/viewer.ts", "          this.opts.onMarkResized?.(live.id, this.fileRectOn(live.slot, rect));\n          return;", "          this.opts.onMarkResized?.(live.id, this.fileRectOn(live.slot, rect));", "resizes a box, an ellipse, a text box and a form field, and moves none of them"),
    Mutation("resize: grow from where the mark was pressed", "src/lib/viewer.ts", "          const rect = { ...live.grow, right: live.grow.right + live.dx, bottom: live.grow.bottom + live.dy };", "          const rect = { ...live.grow, right: live.from.x + live.dx, bottom: live.from.y + live.dy };", "resizes a box, an ellipse, a text box and a form field, and moves none of them"),
    Mutation("resize: clamp a corner drag as a move", "src/lib/viewer.ts", "        const bound = live.grow\n          ? this.clampGrow(live.id, live.slot, live.grow, want)\n          : live.others.reduce(", "        const bound = false\n          ? this.clampGrow(live.id, live.slot, live.grow!, want)\n          : live.others.reduce(", "keeps a form field at least as large as a field of its kind may be"),
    Mutation("resize: hold a checkbox to a text field's least size", "src/lib/fieldnames.ts", "  return kind === \"checkbox\" || kind === \"radio\" ? 6 : kind === \"signature\" ? 24 : 8;", "  return kind === \"signature\" ? 24 : 8;", "keeps a form field at least as large as a field of its kind may be"),
    Mutation("resize: send the rectangle without the mark", "src/lib/edits.ts", "    return this.adopt(await call(\"annot_resize\", { doc: this.doc, mark, rect }));", "    return this.adopt(await call(\"annot_resize\", { doc: this.doc, mark: 0, rect }));", "sends a resize as the mark and its new rectangle"),
    Mutation("field border: leave new fields unframed until asked", "src/lib/fieldnames.ts", "    return storage().getItem(BORDER_KEY) !== \"false\";", "    return storage().getItem(BORDER_KEY) === \"true\";", "is on until it is turned off, and remembers either"),
    Mutation("field border: forget the choice", "src/lib/fieldnames.ts", "    storage().setItem(BORDER_KEY, String(border));\n", "", "is on until it is turned off, and remembers either"),
    Mutation("field border: turn off when storage cannot be read", "src/lib/fieldnames.ts", "  } catch {\n    return true;\n  }", "  } catch {\n    return false;\n  }", "reads anything tpdf did not write as on, and survives storage that throws"),
    Mutation("field border: offer both choices at once", "src/lib/appcommands.ts", "      enabled: () => !actions.fieldBorder(),", "      enabled: () => true,", "offers only the choice that is not the current one, with or without a document"),
    Mutation("field border: offer the current choice", "src/lib/appcommands.ts", "      enabled: () => actions.fieldBorder(),\n      run: () => actions.setFieldBorder(false),", "      enabled: () => !actions.fieldBorder(),\n      run: () => actions.setFieldBorder(false),", "offers only the choice that is not the current one, with or without a document"),
    Mutation("redact list: leave a block's pictures out of what a region takes", "src/lib/redactlist.ts", "  const pictures = (plan?.images?.length ?? 0) + (plan?.form_images?.length ?? 0);", "  const pictures = plan?.images?.length ?? 0;", "counts a picture inside a reusable block with the page's own"),
    Mutation("compress dialog: take any data URL for a PNG", "src/lib/compressdialog.ts", "  return /^data:image\\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(url);", "  return /^data:/.test(url);", "puts nothing in a picture but a PNG the backend encoded"),
    Mutation("redact list: count the pictures as drawings", "src/lib/redactlist.ts", "const drawings = (plan?.paths?.length ?? 0) +", "const drawings = (plan?.images?.length ?? 0) +", "says a drawing inside the region goes, and counts them"),
    Mutation("recovery: drop the note from a clean verdict", "src/lib/recovery.ts", '(applied.notes?.length ? ` Note: ${applied.notes.join("; ")}.` : "")', '""', "reports a redaction as the window does"),
    Mutation("assurance: redactDocument write guard", "src/lib/edits.ts", "  async redactDocument(source: string, fill: Fill): Promise<Applied> {\n    await this.beforeWrite();", "  async redactDocument(source: string, fill: Fill): Promise<Applied> {\n    // consent omitted", "awaits save consent before every writing command and sends nothing when declined"),
    Mutation("choices: compare selection arrays by identity", "src/lib/forms.ts", "a.length === b.length && a.every((v, i) => v === b[i])", "a === b", "keeps duplicate exports distinct and compares selections across IPC replies"),
    Mutation("choices: accept too many selected options", "src/lib/forms.ts", "&& value.length > 1) return", "&& value.length > 100) return", "validates option indices and selection cardinality"),
    Mutation("forms: lose an unchecked value", "src/lib/forms.ts", "?.value ?? widget.value", "?.value || widget.value", "does not confuse an unchecked checkbox with an absent answer"),
    Mutation("forms: ignore a read-only field", "src/lib/forms.ts", "  if (widget.reason) return widget.reason;", "  if (false) return widget.reason;", "accepts supported text and refuses loss, wrong types and field restrictions"),
    # Tab in a form goes to the next control that can take the keyboard. One
    # that is put away or withheld takes none, and the key went nowhere.
    Mutation("form tab: go to a control that is put away", "src/lib/forms.ts", "    if (!placed(control)) continue;\n", "", "passes over a control that is put away, without scrolling to it"),
    Mutation("form tab: go to a control that could not be reached", "src/lib/forms.ts", "    if (reach(control)) return { to: control, further, moved };", "    reach(control); return { to: control, further, moved };", "passes over a control that could not be brought to the keyboard, and says the page moved"),
    Mutation("form tab: forget that looking for a control scrolled the page", "src/lib/forms.ts", "    moved = true;\n  }", "  }", "goes back to the control the keyboard is in after looking for one it could not reach"),
    Mutation("form tab: say there is no control further on when there is", "src/lib/forms.ts", "    further = true;\n", "", "is nowhere when no control further on can take it, and says there were some"),
    Mutation("form tab: look for a next control from one that is not in the form", "src/lib/forms.ts", "  if (at < 0) return { to: null, further, moved };\n", "", "is the web view's from the last control and from the first with Shift"),
    Mutation("form tab: the layer takes a hidden control for one that can be typed in", "src/lib/forms.ts", "return one.input.style.display !== \"none\" && !one.input.disabled; });", "return true; });", "goes back to the control the keyboard is in after looking for one it could not reach"),
    Mutation("form tab: the layer sends the keyboard to a control that is withheld", "src/lib/forms.ts", "return one.input.style.display !== \"none\" && !one.input.disabled; });", "return one.input.style.display !== \"none\"; });", "passes over a control that is withheld while the file is written"),
    Mutation("form tab: the layer scrolls to a control that has no place", "src/lib/forms.ts", "      (one) => this.anchor(one.widget) !== null,", "      () => true,", "goes to the next control that is shown, past one that is put away"),
    Mutation("form tab: the layer leaves the key to the web view past hidden controls", "src/lib/forms.ts", "          if (stop.further) event.preventDefault();", "          if (stop.to) event.preventDefault();", "leaves the keyboard where it is when no control further on is shown"),
    Mutation("form tab: the layer leaves the page where looking for a control put it", "src/lib/forms.ts", "          else if (stop.moved) { this.reveal(widget); this.layout(); }", "", "goes back to the control the keyboard is in after looking for one it could not reach"),
    # A size is shown by the figure as printed: 9.96 is 10 and not 10.0, and
    # 1023.6 KB is 1.0 MB.
    Mutation("coverage: say everything up to the signature of a range that skips the head", "src/lib/properties.ts", "  if (signature.appended_bytes > 0 && signature.starts_at_zero) {", "  if (signature.appended_bytes > 0) {", "does not say everything up to the signature when the range skips the head and was appended to"),
    # Re-aimed 2026-10-16: `formatBytes` moved to `bytes.ts`, which properties.ts re-exports.
    Mutation("sizes: decide the unit before rounding", "src/lib/bytes.ts", "  while (Math.round(value) >= 1024 && unit < units.length - 1) {", "  while (value >= 1024 && unit < units.length - 1) {", "moves to the next unit when the figure rounds up to 1024 of this one"),
    Mutation("sizes: decide the decimal before rounding", "src/lib/bytes.ts", "  const rounded = Number(tenths) < 10 ? tenths : Math.round(value).toString();", "  const rounded = value < 10 ? tenths : Math.round(value).toString();", "never prints a decimal on a figure that rounded up to ten"),
    Mutation("sizes: walk past the last unit", "src/lib/bytes.ts", "  while (Math.round(value) >= 1024 && unit < units.length - 1) {", "  while (Math.round(value) >= 1024) {", "keeps the last unit for a size past the end of them"),
    # Text a removal leaves because it cannot be measured is said in a
    # sentence of its own; the rule for the other kinds printed the backend's
    # name for it.
    Mutation("redactlist: name unmeasured text by the backend's word", "src/lib/redactlist.ts", "    if (object.kind === UNMEASURED_TEXT) {\n      unmeasured = true;\n      continue;\n    }", "", "says in plain words why text it cannot measure stays, and not the backend's name for it"),
    Mutation("redactlist: list unmeasured text among the kinds as well", "src/lib/redactlist.ts", "      unmeasured = true;\n      continue;", "      unmeasured = true;", "says in plain words why text it cannot measure stays, and not the backend's name for it"),
    Mutation("redactlist: say the sentence about kinds with no kind in it", "src/lib/redactlist.ts", "    ...(kinds.size > 0 ? [`Also covers ${said}, which a removal cannot take`] : []),", "    `Also covers ${said}, which a removal cannot take`,", "says in plain words why text it cannot measure stays, and not the backend's name for it"),
    Mutation("redactlist: say nothing of unmeasured text", "src/lib/redactlist.ts", "    ...(unmeasured ? [UNMEASURED_TEXT_WARNING] : []),", "", "says it after the kinds a reader can name, each in its own sentence"),
    Mutation("redactlist: say the unmeasured text before the kinds", "src/lib/redactlist.ts", "  ].join(\". \");", "  ].reverse().join(\". \");", "says it after the kinds a reader can name, each in its own sentence"),
    Mutation("redactlist: spell the kind another way than the backend", "src/lib/redactlist.ts", "export const UNMEASURED_TEXT = \"unmeasured-text\";", "export const UNMEASURED_TEXT = \"unmeasured\";", "knows the kind by the backend's spelling of it"),
    # Every kind a removal leaves has words in `UNHANDLED_KINDS`, held equal
    # to the kinds the two Rust files define; one the table does not hold is
    # said without its name.
    Mutation("redactlist: have no words for a drawing that sets the clip", "src/lib/redactlist.ts", "  [\n    \"clip-path\",\n    \"a drawing that also limits where the things after it are drawn\",\n    \"drawings that also limit where the things after them are drawn\",\n  ],\n", "", "has words for every kind the backend can report, and for no other"),
    Mutation("redactlist: have words for a kind the backend does not report", "src/lib/redactlist.ts", "  [\"shading\", \"a colour gradient\", \"colour gradients\"],", "  [\"shading\", \"a colour gradient\", \"colour gradients\"],\n  [\"text\", \"some text\", \"texts\"],", "has words for every kind the backend can report, and for no other"),
    Mutation("redactlist: spell a kind another way than the backend", "src/lib/redactlist.ts", "  [\"unsupported\", \"something of a kind", "  [\"unknown\", \"something of a kind", "has words for every kind the backend can report, and for no other"),
    Mutation("redactlist: say one drawing that sets the clip for several", "src/lib/redactlist.ts", "      const what = many === 1 ? one : `${many} ${several}`;", "      const what = one;", "says each kind in words a reader has, one and several"),
    Mutation("redactlist: call a colour gradient by the backend's word", "src/lib/redactlist.ts", "  [\"shading\", \"a colour gradient\", \"colour gradients\"],", "  [\"shading\", \"a shading\", \"shadings\"],", "never prints a kind as the backend spells it"),
    Mutation("redactlist: print the name of a kind it has no words for", "src/lib/redactlist.ts", "export const UNKNOWN_KIND: readonly [kind: string, one: string, several: string] = [\n  \"\",\n  \"something else\",", "export const UNKNOWN_KIND: readonly [kind: string, one: string, several: string] = [\n  \"\",\n  \"a soft-mask\",", "says something is there when the kind is one it has never heard of"),
    Mutation("redactlist: say nothing of a kind it has no words for", "src/lib/redactlist.ts", "    const place = known < 0 ? UNHANDLED_KINDS.length : known;", "    if (known < 0) continue;\n    const place = known;", "says something is there when the kind is one it has never heard of"),
    Mutation("redactlist: say a kind it has no words for before the ones it has", "src/lib/redactlist.ts", "    const place = known < 0 ? UNHANDLED_KINDS.length : known;", "    const place = known;", "says something is there when the kind is one it has never heard of"),
    Mutation("redactlist: leave out how often a reusable block is drawn", "src/lib/redactlist.ts", "      return drawn > 0 ? `${what} drawn ${drawn} times` : what;", "      return what;", "says a reusable block stays because the document repeats it"),
    Mutation("redactlist: say one kind's rows in the order the page lists them", "src/lib/redactlist.ts", "    .sort((a, b) => a.place - b.place || a.drawn - b.drawn)", "    .sort((a, b) => a.place - b.place)", "says a reusable block stays because the document repeats it"),
    Mutation("redactlist: cut a warning at the row's edge with no way to read the rest", "src/lib/redactlist.ts", "      said.title = warning;\n", "", "draws the warning under the words, and nothing when there is none"),
    Mutation(
        "toolbar: ignore command enablement",
        "src/lib/toolbar.ts",
        "enabled: command !== undefined && (command.enabled?.() ?? true)",
        "enabled: command !== undefined",
        "updates selection, save, undo, redo and search guards in both directions",
    ),
    Mutation(
        # Pan on any button. The primary one then fights the text selection and
        # every drawing tool for the same press, which is the direction that
        # looks like the viewer ignoring a gesture rather than adding one.
        "viewer: pan on any button, not the middle one",
        "src/lib/viewer.ts",
        "    if (event.button !== MIDDLE_BUTTON) return;",
        "    if (false) return;",
        "is the middle button and no other",
    ),
    Mutation(
        # Send the view the way the pointer went instead of the page. The pan
        # still works, in the wrong direction --- which is the hand-tool
        # convention inverted, and reads as the page fighting the hand holding
        # it.
        "viewer: send a pan the way the pointer went, not the page",
        "src/lib/viewer.ts",
        "    this.scrollTo(from.top - (event.clientY - from.y));",
        "    this.scrollTo(from.top + (event.clientY - from.y));",
        "moves the view with the pointer, on both axes",
    ),
    Mutation(
        # Vertical only. The gesture works everywhere it is not needed and does
        # nothing at the zoom a reader reaches for it, because `maxPan` is zero
        # until the page overflows the window --- so a fixture at a fit zoom
        # cannot tell this from the real thing.
        "viewer: pan vertically and not sideways",
        "src/lib/viewer.ts",
        "    if (this.scroller.setPan(from.pan - (event.clientX - from.x))) this.wake();",
        "    if (false) this.wake();",
        "moves the view with the pointer, on both axes",
    ),
    Mutation(
        # Say something else while panning. The only thing that tells a reader
        # the button they are holding is doing anything.
        "viewer: show the wrong cursor through a pan",
        "src/lib/viewer.ts",
        '        "grabbing"',
        '        "progress"',
        "says a pan is happening, and stops saying so",
    ),
    Mutation(
        # Let an armed tool outrank the pan. The crosshair then describes what
        # the next primary press would do, through a gesture the reader is
        # making now with a different button.
        "viewer: rank an armed tool's cursor above a pan in progress",
        "src/lib/viewer.ts",
        "    this.surfaceHost.style.cursor = this.panFrom",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        '    this.surfaceHost.style.cursor = this.panFrom && this.tool.kind !== "draw"',
        "outranks an armed tool's cursor while the button is down",
    ),
    Mutation(
        # Leave the three listeners on the root. **No drag can see this**:
        # `onPanEnd` clears `panFrom` as well, and that alone stops a stray move
        # panning, so the leak is invisible to every behavioural assertion --- it
        # costs one set of listeners per drag on an element that lives as long as
        # the document. The test named here counts them, which is the only
        # instrument that can.
        "viewer: leave a pan's listeners on the root",
        "src/lib/viewer.ts",
        """    this.root.removeEventListener("pointermove", this.onPanMove);
    this.root.removeEventListener("pointerup", this.onPanEnd);
    this.root.removeEventListener("pointercancel", this.onPanEnd);
    this.showCursor();
  };""",
        """    this.showCursor();
  };""",
        "takes its listeners back off, which no drag can notice",
    ),
    Mutation(
        # Pan from a press on the scrollbar. The track has its own drag and does
        # not test the button, so the two then run at once and the view jumps to
        # wherever the thumb was dragged *and* follows the pointer.
        "viewer: pan from a press on the scrollbar track",
        "src/lib/viewer.ts",
        """    if (this.track.contains(event.target as Node)) return;
    // A reader who pans""",
        "    // A reader who pans",
        "leaves a press on the scrollbar to the scrollbar",
    ),
    Mutation(
        # Never apply the pan. Every page stays centred, the gesture reports
        # movement, and the right-hand side of a zoomed page stays unreachable.
        "scroller: lay the pages out as though nothing had been panned",
        "src/lib/scroller.ts",
        "    return Math.max(0, (this.opts.viewport.width - row) / 2) - this.pan + (box?.x ?? 0);",
        "    return Math.max(0, (this.opts.viewport.width - row) / 2) + (box?.x ?? 0);",
        "moves every page by the same amount, whatever its width",
    ),
    Mutation(
        # Read the pan unclamped. A drag past the right-hand edge then carries
        # the pages off the window entirely, and the reader has to drag exactly
        # as far back to find them.
        "scroller: read the pan without clamping it to the bound",
        "src/lib/scroller.ts",
        "    return Math.max(0, Math.min(this.panX, this.maxPan));",
        "    return this.panX;",
        "clamps a pan to the bound at each end",
    ),
    Mutation(
        # Accept a non-finite pan. `NaN` survives both clamps unchanged and
        # reaches the DOM as `left:NaNpx`, which lays out nothing at all and
        # says nothing about why.
        "scroller: accept a pan that is not a number",
        "src/lib/scroller.ts",
        "    this.panX = Number.isFinite(px) ? px : 0;",
        "    this.panX = px;",
        "parks at the left edge for a pan that is not a number",
    ),
    Mutation(
        # Always report that the pan moved. The viewer then wakes the frame loop
        # on every move of a drag that has run out of page.
        "scroller: report a pan as moved when it did not",
        "src/lib/scroller.ts",
        "    if (this.pan === before) return false;",
        "    if (false) return false;",
        "reports whether the applied pan actually moved",
    ),
    Mutation(
        # Leave the widest page at zero, so `maxPan` is always zero and the pan
        # is inert on every document. It is inert at every fit zoom anyway,
        # which is why the fixture behind this one has to overflow the window.
        "scroller: never grow the widest page",
        "src/lib/scroller.ts",
        "      widest = Math.max(widest, rowWidth);",
        "      widest = Math.max(widest, 0);",
        "has nothing to pan while every page fits, and the widest page sets the bound",
    ),
    Mutation(
        # Drop the counts and keep only the warning. A merge whose report says
        # nothing about how much went in is a copy's report on an operation the
        # reader cannot check by looking at what they asked for.
        "recovery: report a merge without saying what it merged",
        "src/lib/recovery.ts",
        "  const said = `Merged this document with ${others} — ${pages} in all.`;",
        '  const said = "The merge was written.";',
        "says how many documents went in and how many pages came out",
    ),
    Mutation(
        # Always plural. "1 other documents" reads as a defect in everything
        # else on screen, which is why the singular is asserted rather than
        # left to taste.
        "recovery: pluralise a merge of one document",
        "src/lib/recovery.ts",
        '    merged.files === 1 ? "1 other document" : `${merged.files} other documents`;',
        "    `${merged.files} other documents`;",
        "says it in the singular for one document and one page",
    ),
    Mutation(
        # Return before the warning. The counts survive and the fact that the
        # file underneath moved does not --- the direction that looks like a
        # working report.
        "recovery: drop the changed-source warning from a merge",
        "src/lib/recovery.ts",
        "  if (!merged.changed) return said;",
        "  if (merged.changed || !merged.changed) return said;",
        "adds the changed-source warning without dropping the counts",
    ),
    Mutation(
        # A registered command that reaches no action. This is the shape that
        # shipped inert once before, and no type error and no registry sweep
        # can see it: the command has a `run`, and the `run` does nothing.
        "appcommands: register Merge documents without reaching its action",
        "src/lib/appcommands.ts",
        "      run: () => actions.mergeDocuments(),",
        "      run: () => {},",
        "merge documents through the command, with no value to carry",
    ),
    Mutation(
        # Prompt for every refusal, not only the answerable one. A password
        # dialog in front of a corrupt file asks a reader for something that
        # cannot help, and no answer ends it.
        #
        # This one is why the three `ask` mocks that assert they are NOT called
        # still resolve `null`: with a bare `vi.fn()` resolving `undefined` this
        # mutation SPUN FOREVER and killed the vitest worker, which is a hang and
        # not a red test -- docs/TRAPS.md has the entry.
        "unlock: prompt for a refusal a password cannot fix",
        "src/lib/unlock.ts",
        "      if (!isOpenRefusal(e) || !e.locked || !ask) throw e;",
        "      if (!isOpenRefusal(e) || !ask) throw e;",
        "does not ask about a refusal that is not the answerable one",
    ),
    Mutation(
        # Read a dismissal as "try again with nothing". The reader pressed
        # Cancel and the open runs once more, refuses identically, and they are
        # shown the refusal they had already dismissed.
        "unlock: retry with no password when the reader declines",
        "src/lib/unlock.ts",
        "      if (typed === null) throw e;",
        "      if (typed === null) return await open(undefined);",
        "rethrows the refusal when the reader declines",
    ),
    Mutation(
        # One retry rather than a loop. Mistyping twice is ordinary, and the
        # second attempt would return the raw refusal instead of asking again.
        "unlock: allow one retry instead of as many as are offered",
        "src/lib/unlock.ts",
        "  let password: string | undefined;\n  for (;;) {",
        "  let password: string | undefined;\n  for (let once = 0; once < 2; once++) {",
        "asks again after a wrong password, showing the backend's second wording",
    ),
    Mutation(
        # Send an empty field as an empty password. PDFium distinguishes them --
        # an empty *user* password is what most permission-restricted documents
        # carry -- so this retries the attempt that already failed, and the
        # reader watches Unlock do nothing.
        "passworddialog: read an empty field as an empty password",
        "src/lib/passworddialog.ts",
        "    this.settle(this.field.value || null);",
        "    this.settle(this.field.value);",
        "reads an empty field as no answer rather than as an empty password",
    ),
    Mutation(
        # Leave the password in the input. It is the one place the secret would
        # outlive its use, in a live element attached to the document.
        "passworddialog: keep the password in the field after closing",
        "src/lib/passworddialog.ts",
        '    this.field.value = "";\n    if (this.shown) {',
        "    if (this.shown) {",
        "clears the field on every close, however it closed",
    ),
    Mutation(
        # Stack a second question on the first. Whoever awaited the first is
        # holding an open that never finishes, and nothing later can settle it --
        # the test that catches this one does so by TIMING OUT, which is the
        # failure shape docs/TRAPS.md warns reads as a broken harness.
        "passworddialog: leave the previous question unsettled",
        "src/lib/passworddialog.ts",
        "    // A second question dismisses the first rather than stacking on it. Nothing\n"
        "    // issues two today; what this rules out is a promise nobody settles.\n"
        "    this.settle(null);",
        "",
        "settles an outstanding question when a second one is asked",
    ),
    Mutation(
        # Dismiss on any click, including one that bubbled out of the panel. The
        # reader clicks their own password field and the dialog closes.
        "passworddialog: dismiss on a click anywhere, not only the backdrop",
        "src/lib/passworddialog.ts",
        "      if (event.target === this.backdrop) this.settle(null);",
        "      this.settle(null);",
        "treats a click on the backdrop as a dismissal and one on the panel as nothing",
    ),
    Mutation(
        # Offer a reload for every refusal. "A document must keep at least one
        # page" then arrives with a button that discards the reader's work in
        # exchange for nothing, which is the shape this whole module exists to
        # avoid: an offer is only safe where it is the answer.
        "recovery: offer a reload whatever the refusal was",
        "src/lib/recovery.ts",
        "  if (!failure.changed || failure.reopen) {",
        "  if (failure.reopen) {",
        "offers nothing for a refusal that is not about the file changing",
    ),
    Mutation(
        # Offer buttons on a refusal the window has already acted on. `App.svelte`
        # reopens the file itself when `reopen` is set, so Reload reloads what is
        # on screen and Save a copy copies a freshly-opened, unedited document --
        # and a reader presses them, because they are the ones that sound safe.
        "recovery: offer buttons after the window has already reopened the file",
        "src/lib/recovery.ts",
        "  if (!failure.changed || failure.reopen) {",
        "  if (!failure.changed) {",
        "offers nothing once the document is closed, because the window reopened it",
    ),
    Mutation(
        # Put the destructive one first. Both buttons are present either way, so
        # a check on the set cannot see this; the reader reaching for the nearest
        # button is what can.
        "recovery: lead with the offer that spends the journal",
        "src/lib/recovery.ts",
        '    offers: ["saveCopy", "reload"],\n  };\n}\n\n/**\n * What the Reload command does',
        '    offers: ["reload", "saveCopy"],\n  };\n}\n\n/**\n * What the Reload command does',
        "warns before discarding unsaved edits, and offers the copy first",
    ),
    Mutation(
        # Leave out what the settle committed: a form answer still being typed
        # becomes an edit only when the drafts are settled, so a reload that
        # does not look afterwards discards it without a word.
        "recovery: do not look at dirty once the drafts are settled on a reload",
        "src/lib/recovery.ts",
        "  return beforeReload(host.dirty() || unsettled);",
        "  return beforeReload(unsettled);",
        "settles drafts before it reads dirty",
    ),
    Mutation(
        # Reload without a word, which is what it did until this landed. The
        # command was written before there was anything to lose.
        "recovery: reload an edited document without warning",
        "src/lib/recovery.ts",
        "  if (!dirty) return null;",
        "  return null;\n  // eslint-disable-next-line",
        "warns before discarding unsaved edits, and offers the copy first",
    ),
    Mutation(
        # Write a copy from a changed source and say nothing, so a file built
        # from a document the reader is not looking at reads as one that is.
        "recovery: write a copy from a changed source silently",
        "src/lib/recovery.ts",
        "  if (!copied.changed) return null;",
        "  return null;",
        "says the copy came from a newer file, and does not call it an error",
    ),
    Mutation(
        # Stringify the whole rejection, which is what every catch did before
        # there was a seam and what printing's did until the refusal grew
        # fields. `[object Object]` where a sentence belongs, and the buttons
        # still appear beside it, so the window looks like it is working.
        "recovery: stringify a refusal instead of reading its message",
        "src/lib/recovery.ts",
        '    message: typeof fields.message === "string" ? fields.message : String(thrown),',
        "    message: String(thrown),",
        "reads the message off a refusal object",
    ),
    Mutation(
        # Drop the flag on the way through. The sentence is right, the file has
        # changed underneath, and the reader is told so with nothing to press --
        # which is the refusal-flattened-to-a-string failure arriving one layer
        # further in, where no backend test can see it.
        "recovery: read a refusal's message and lose the flag beneath it",
        "src/lib/recovery.ts",
        "    changed: flagOf(fields.changed),",
        "    changed: undefined,",
        "carries the flag the rules decide from",
    ),
    Mutation(
        # Take a flag from whatever arrives in the field. A `reopen` of
        # `"false"` is then true, and the one refusal a reader most needs the
        # buttons for is the one that withholds them.
        "recovery: believe a flag that is not a boolean",
        "src/lib/recovery.ts",
        '  return typeof field === "boolean" ? field : undefined;',
        "  return field as boolean | undefined;",
        "takes a flag only where it is a boolean",
    ),
    Mutation(
        # Answer "am I up to date" without saying which version that is. The
        # reader presses the command *because* they want the number, and an
        # answer that omits it is the silence this replaced wearing a sentence.
        "update: answer the current check without naming the running version",
        "src/lib/update.ts",
        "      return `tpdf ${version} is the latest version`;",
        '      return "Up to date";',
        "says the running version is the latest, rather than saying nothing",
    ),
    Mutation(
        # Report the box in the space the reader is looking at rather than the
        # file's. Identical on an unrotated, uncropped page -- which is thirteen
        # of the fourteen corpora -- and a rectangle somewhere else entirely on
        # the fourteenth.
        "viewer: report a drawn box in the laid-out space, not the file's",
        "src/lib/viewer.ts",
        "    return [moved[0] ?? 0, moved[1] ?? 0, moved[2] ?? 0, moved[3] ?? 0];",
        "    return [quad.left, quad.top, quad.right, quad.bottom];",
        "puts a box drawn at the screen's top-left in the corner the turn implies",
    ),
    Mutation(
        # Undo the turn with the page's un-turned dimensions, which is the
        # mistake `unturnQuad`'s doc comment warns about: a plausible rectangle
        # in the wrong place, with the right proportions.
        "viewer: unturn a drawn box against the wrong pair of dimensions",
        "src/lib/viewer.ts",
        "    const back = unturnQuad(quad, turns, laid.width, laid.height);",
        "    const back = unturnQuad(quad, turns, laid.height, laid.width);",
        "stays inside the page at every turn",
    ),
    Mutation(
        # Clamp against the page as the file describes it rather than as it is
        # laid out. Passes at half turns, where the sides do not swap.
        "viewer: clamp a drawn box against the un-turned page",
        "src/lib/viewer.ts",
        "      this.scroller.effectiveTurns(slot),\n    );",
        "      0,\n    );",
        "stays inside the page at every turn",
    ),
    Mutation(
        # Spend the tool even when the box was refused. A slipped click then
        # costs the reader the command as well, with nothing on screen saying
        # why -- the shape this was written as first, and the test found it.
        # **Re-aimed 2026-08-22**, when the comment tool made the quad a
        # conditional: a comment is placed by a press and every other kind still
        # needs two corners, so the one line this named is now three. The edit is
        # the same one --- spend the tool before the refusal is decided.
        "viewer: spend the box tool on a click that drew nothing",
        "src/lib/viewer.ts",
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));",
        # Re-spelled 2026-09-06: `drawKind` is gone, and a write to a field the
        # class no longer has is a no-op at runtime that vitest cannot see, so
        # the row survived while saying nothing. `tool` is the one field now.
        "            : boxQuad(live.from, live.to, this.laidSize(live.slot));\n        this.tool = NO_TOOL;",
        "refuses a click, and keeps the tool armed",
    ),
    Mutation(
        # Leave the tool armed after a box is drawn. A reader who wanted one box
        # gets a second on their next press, which is a mode they cannot see and
        # did not ask to stay in.
        "viewer: keep the box tool armed after a box is drawn",
        "src/lib/viewer.ts",
        # Re-aimed 2026-09-06 when the five tool flags became one `tool` field.
        '        if (this.tool.kind === "draw") this.tool = NO_TOOL;\n'
        "        this.showCursor();",
        "        this.showCursor();",
        "is spent by one box",
    ),
    Mutation(
        # Back to one-shot: commit on release, as the box does. A drawing is then
        # one stroke and can never be more, which is the state the window was in
        # until 2026-08-20 -- while `/InkList`, the writer and the probe were all
        # built for several, so the harness could make a document no reader could.
        "viewer: commit a drawing when the pointer comes up",
        "src/lib/viewer.ts",
        "          this.inking ??= { slot: live.slot, strokes: [] };\n          this.inking.strokes.push(live.points);",
        "          this.inking ??= { slot: live.slot, strokes: [] };\n          this.inking.strokes.push(live.points);\n          this.finishDrawing();",
        "stays armed so a drawing can be several strokes",
    ),
    Mutation(
        # Keep what was drawn when Escape ends it. Escape has meant abandon since
        # the box, so a drawing left behind by it is a mode the reader thinks
        # they left -- and the next stroke anywhere joins a drawing they believe
        # they threw away.
        "viewer: leave the strokes behind when Escape ends a drawing",
        "src/lib/viewer.ts",
        "    // by it exactly as a half-dragged rectangle is --- which is why the finish\n    // gesture had to be a *different* key rather than a second Escape.\n    this.inking = null;",
        "    // by it exactly as a half-dragged rectangle is --- which is why the finish\n    // gesture had to be a *different* key rather than a second Escape.",
        "throws the whole drawing away on Escape",
    ),
    Mutation(
        # Let a stroke on another page join the drawing. An annotation belongs to
        # one page, so the second stroke is then drawn on a page it was not made
        # on -- ink where nobody put it, which is worse than a refused press.
        "viewer: let a stroke started on another page join the drawing",
        "src/lib/viewer.ts",
        "        if (this.inking && this.inking.slot !== page) return false;",
        "        if (false) return false;",
        "refuses a stroke that starts on another page",
    ),
    Mutation(
        # Stop reporting the drawing. The one mode in this application then has
        # nothing on screen to say it is live or how to leave it, which is what
        # the box's one-shot design existed to avoid.
        #
        # **Aimed at the accessor, because `ViewerStatus.drawing` IS the
        # accessor.** It was its own expression in the status object, and this
        # mutation emptied that and SURVIVED: every test asked the viewer and
        # only the window reads the status, so the copy nobody tested was the one
        # a reader sees. Removing the copy is what makes this catchable.
        "viewer: keep the drawing out of the status, so the mode is invisible",
        "src/lib/viewer.ts",
        '    if (this.drawArmed !== "ink") return null;\n    return this.inking?.strokes.length ?? 0;',
        '    if (this.drawArmed !== "ink") return null;\n    return 0;',
        "reports the drawing in the status, so the mode is visible",
    ),
    Mutation(
        # Let two checks share a name. The roll is compared as a set, so a repeat
        # is invisible there: one verdict prints under another's label and a third
        # name leaves the roll, with every count still adding up.
        "checkreport: let two checks share a name",
        "src/lib/checkreport.ts",
        "    if (repeated.length > 0) {",
        "    if (false) {",
        "fails the run when two checks share a name",
    ),
    Mutation(
        # Keep every pointer event. A hand resting on a trackpad produces dozens
        # of points inside a tenth of a point, none of which changes the line and
        # all of which go into the file and over the IPC boundary.
        "viewer: keep every pointer event rather than sampling the stroke",
        "src/lib/viewer.ts",
        "        const last = live.points[live.points.length - 1];\n        if (\n          !last ||",
        "        const last = live.points[live.points.length - 1];\n        if (\n          true ||",
        "samples the pointer rather than keeping every event",
    ),
    Mutation(
        # Drop the point the pointer was released on. The sample may have
        # discarded it, so every stroke ends up to one sample short --- invisible
        # on a long sweep and the whole of a tick or the crossing of a t.
        "viewer: end a stroke at the last sample rather than at the release",
        "src/lib/viewer.ts",
        "          if (!last || last.x !== live.to.x || last.y !== live.to.y) {\n            live.points.push({ ...live.to });\n          }",
        "          if (false) {\n            live.points.push({ ...live.to });\n          }",
        "keeps the point the pointer was released on",
    ),
    Mutation(
        # Commit a drawing of one point. A press that never moved is a reader who
        # has not started, and spending the tool on it costs them the command
        # with nothing on screen to say why.
        "viewer: commit a drawing the pointer never moved for",
        "src/lib/viewer.ts",
        "          if (live.points.length < 2) return;",
        "          if (false) return;",
        "keeps the tool armed when the pointer never moved",
    ),
    Mutation(
        # Send a drawing's rectangle as well as its strokes. The model refuses
        # the pair outright, so the mark never lands -- which reads as the tool
        # doing nothing rather than as a wire defect.
        "viewer: send a rectangle alongside a drawing's strokes",
        "src/lib/viewer.ts",
        '      kind,\n      id,\n      {\n        quads: [],',
        '      kind,\n      id,\n      {\n        quads: [0, 0, 1, 1],',
        "commits strokes and no rectangle",
    ),
    Mutation(
        # Leave a drawing's points in the view's space. Correct on an unturned
        # page and wrong on every other, which is the shape of defect a single
        # untried corpus hides.
        "viewer: leave a drawing's points unmapped into the file's space",
        "src/lib/viewer.ts",
        "          const mapped = this.fileRectOn(made.slot, {",
        "          const mapped = [point.x, point.y] as const;\n          void this.fileRectOn(made.slot, {",
        "puts the points back in the file's space on a turned page",
    ),
    Mutation(
        # Send the slot rather than the page's id. Works on every document until
        # a page is deleted or moved, and then marks land on the wrong page.
        "viewer: send a drawn box to the slot rather than to the page's id",
        "src/lib/viewer.ts",
        # The `quad` line above it is what disambiguates: the crop's drag has an
        # identical `const id = quad ? ...`, and it builds its quad with
        # `boxQuad(live.from, live.to, ...)` unconditionally rather than choosing
        # between an icon and a box.
        '        if (kind === "picture" && picture && quad) quad = fitSignature(quad, picture);\n'
        "        const id = quad ? this.pages.idOf(live.slot) : undefined;",
        '        if (kind === "picture" && picture && quad) quad = fitSignature(quad, picture);\n'
        "        const id = quad ? live.slot : undefined;",
        "carries the armed kind and the page's id",
    ),
    Mutation(
        # Take the press after the hit tests rather than before them. A box
        # drawn across a highlight is then swallowed by the highlight's note,
        # and one across a link jumps to another page.
        "viewer: hit-test the page before letting the box tool have the press",
        "src/lib/viewer.ts",
        "    if (this.cropDrag.start(event) || this.drawDrag.start(event)) {\n"
        "      event.preventDefault();\n      return;\n    }",
        "",
        "goes to the drag when a tool is armed and not when one is not",
    ),
    Mutation(
        # Commit a cancelled drag. Escape then draws the box the reader was
        # trying not to draw -- the same defect as `drag.ts`'s own cancel
        # mutation, one layer up, where the box actually reaches the model.
        "viewer: draw the box even when the drag was cancelled",
        "src/lib/viewer.ts",
        "        if (!committed || !live || !kind) {",
        "        if (!live || !kind) {",
        "is dropped by Escape mid-drag, without committing the box",
    ),
    Mutation(
        # Re-read the page under the pointer on every move. A drag that wanders
        # onto the next page then silently moves the box to it, and the corner
        # it started from is measured on a page it is no longer on.
        # Named for the two-page test rather than the scroll one: the scroll
        # test asserts the *starting* corner, which this mutation leaves alone.
        # It was aimed at the wrong check first and the harness said so.
        "viewer: follow the pointer onto another page mid-drag",
        "src/lib/viewer.ts",
        # Disambiguated by the line after it: the crop's drag has the identical
        # two lines and then wakes, where the box's goes on to sample for ink.
        "        const { x, y } = this.pageAndPoint(at);\n        live.to = { x, y };\n"
        "        // **Sampled, not every event.**",
        # **`x` and `y` are destructured here and read below**, by the ink
        # sampler that arrived after this mutation was written. An earlier
        # replacement bound `at2` instead and left those reads dangling, so the
        # mutation threw a `ReferenceError` before reaching the behaviour it
        # claims to change --- the test reddened, the harness filed it as
        # caught, and it proved nothing. `check_mutation_types.py` found it.
        "        const { page, x, y } = this.pageAndPoint(at);\n"
        "        live.slot = page;\n        live.to = { x, y };\n"
        "        // **Sampled, not every event.**",
        "keeps the box on the page it started from",
    ),
    Mutation(
        # Build the box by subtracting in arrival order. Correct for a drag down
        # and to the right, inside out for the other three -- and an inside-out
        # rectangle does not draw, which reads as a tool that only works one way.
        # Named for `markband.test.ts`'s check, which is deliberately *not*
        # called the same thing as `viewerdraw.test.ts`'s: it was, and two tests
        # sharing a name made this harness's two failure counts disagree by one
        # and report that it could not read its own output. That guard is right
        # and cannot know the cause.
        "markband: build a box from the corners in the order they arrived",
        "src/lib/markband.ts",
        "    left: clampX(Math.min(from.x, to.x)),",
        "    left: clampX(from.x),",
        "normalises the corners whichever way the drag went",
    ),
    Mutation(
        # Classify a comment as a plain fill. The overlay then draws a filled
        # rectangle where the bubble goes --- which is what the painter's old
        # bare `else` did for *any* kind it had no branch for, and the reason
        # this classifier is exhaustive: a wrong picture rather than a missing
        # one, on every page, with the saved file still correct.
        "markband: draw a comment as a plain fill rather than as its own style",
        "src/lib/markband.ts",
        '    case "note":\n      return "icon";',
        '    case "note":\n      return "fill";',
        "agrees with save.rs arm for arm, which is where the pair can drift",
    ),
    Mutation(
        # Drop the clamp that keeps a drawn box on the page, exactly as the
        # comment's was dropped above. A drag off the edge writes a /Rect past
        # the page box, which `save.rs` maps without complaint.
        "markband: let a drawn box run off the edge of the page",
        "src/lib/markband.ts",
        "  const clampX = (v: number): number => Math.max(0, Math.min(v, page.width));",
        "  const clampX = (v: number): number => v;",
        "stays inside the page at every turn",
    ),
    Mutation(
        # Accept a rectangle of any size. A click then writes an annotation
        # nothing draws and nobody can find again to remove.
        # Aimed at the tall-and-thin check, not at the click: a click is zero
        # in *both* dimensions, so the height guard below catches it whatever
        # this one does. Only a box that is wide enough and too narrow can say
        # which of the two fired.
        "markband: accept a box with no size",
        "src/lib/markband.ts",
        "  if (quad.right - quad.left < MIN_BOX) return null;",
        "",
        "refuses a box that is tall and thin",
    ),
    Mutation(
        # The other dimension. Together they say the bound is on *both* sides,
        # which one of them alone does not.
        "markband: accept a box with no height",
        "src/lib/markband.ts",
        "  if (quad.bottom - quad.top < MIN_BOX) return null;",
        "",
        "refuses a box that is wide and flat",
    ),
    Mutation(
        # Fill a box rather than outlining it, on the overlay. The saved file is
        # still right, so a reader sees a solid block until they save and reopen
        # -- which is exactly the shape of the underline defect this repository
        # already paid for once.
        "markband: draw a box as a filled rectangle, as every mark used to be",
        "src/lib/markband.ts",
        '  return kind === "square";',
        "  return false;",
        "says a box is drawn as an outline and the others are not",
    ),
    Mutation(
        # Fill the text box's rectangle rather than drawing words in it, which
        # is what the final `else` does and what a missing `isText` gives you: a
        # solid red block where the reader typed, with the saved file carrying
        # the words the whole time. The underline defect's shape, a third time.
        "markband: draw a text box as a filled rectangle",
        "src/lib/markband.ts",
        '  return kind === "textbox";',
        "  return false;",
        "says a text box is drawn as words and the others are not",
    ),
    Mutation(
        # Fill the squiggle's band on the overlay rather than waving through it.
        # A solid red bar two and a half times an underline's height under the
        # words, with the saved file correct throughout -- which is the underline
        # defect's shape, one kind later.
        "markband: draw a squiggle as a filled band",
        "src/lib/markband.ts",
        '  return kind === "squiggly";',
        "  return false;",
        "says a squiggle is drawn as a wave and the others are not",
    ),
    Mutation(
        # Give the squiggle the underline's band. Nothing about the shape
        # changes -- it is still a wave -- but it is drawn inside a rule's height,
        # which closes the strip every check that tells the kinds apart reads.
        "markband: fit a squiggle into an underline's band",
        "src/lib/markband.ts",
        "      return { ...quad, top: quad.bottom - height * SQUIGGLE_HEIGHT };",
        "      return { ...quad, top: quad.bottom - height * LINE_FRACTION };",
        "gives a squiggle a band taller than an underline's rule",
    ),
    Mutation(
        # Draw an ellipse as a filled rectangle on the overlay, which is what
        # the final `else` does. The box's mutation above is the same defect one
        # branch along, and the two are separate because the overlay asks
        # `isOutline` first: a reader watching a ring turn into a solid block
        # would see the saved file come back correct, which is the underline
        # defect's shape exactly.
        "markband: draw an ellipse as a filled rectangle",
        "src/lib/markband.ts",
        '  return kind === "ellipse";',
        "  return false;",
        "says an ellipse is drawn as one and the others are not",
    ),
    Mutation(
        # Leave the move listener registered after the drag ends. The pointer
        # goes on being tracked with the button up, which reads as a viewer that
        # has become sticky rather than as a missing line -- and is exactly the
        # failure that made a sixth hand-rolled listener pair worth refusing.
        "drag: keep following the pointer after the button comes up",
        "src/lib/drag.ts",
        '    this.host.removeEventListener("pointermove", this.onMove);',
        "",
        "takes its listeners off and releases the capture",
    ),
    Mutation(
        # Never release the capture. Every later pointer event on the page goes
        # to this element whatever it is over, and nothing about the drag itself
        # looks wrong.
        "drag: hold the pointer capture after the drag is over",
        "src/lib/drag.ts",
        "      this.host.releasePointerCapture(live.pointerId);",
        "      void live;",
        "takes its listeners off and releases the capture",
    ),
    Mutation(
        # Report a cancel as a commit. An Escape, or a touch that turned into a
        # scroll, then draws the box the reader was trying not to draw.
        "drag: treat a cancelled drag as a released one",
        "src/lib/drag.ts",
        "  private readonly onCancel = (event: PointerEvent): void => {\n    this.finish(event, false);",
        "  private readonly onCancel = (event: PointerEvent): void => {\n    this.finish(event, true);",
        "reports a browser cancel as not committed",
    ),
    Mutation(
        # Let a second pointer replace the live drag instead of being refused.
        # A second finger then ends the first drag at its own starting point,
        # committing a rectangle nobody drew.
        "drag: let a second press take over the live drag",
        "src/lib/drag.ts",
        "    if (this.live) return false;\n    const at = { clientX: event.clientX, clientY: event.clientY };",
        "    const at = { clientX: event.clientX, clientY: event.clientY };",
        "refuses a second press rather than replacing the live drag",
    ),
    Mutation(
        # Accept a refused begin anyway. The listeners and the capture are taken
        # for a press the caller said was not theirs -- a press on no page, or a
        # tool that is not armed -- so the surface stops receiving what it wanted.
        "drag: register the drag even when the target refused it",
        "src/lib/drag.ts",
        "    if (!this.target.begin(at)) return false;",
        "    this.target.begin(at);",
        "registers nothing when the target refuses",
    ),
    Mutation(
        # Tell the target before tearing down rather than after. A target that
        # starts another drag from its own `end` -- a one-shot tool that re-arms
        # -- then finds a half-registered one in the way.
        # Written first as an insertion, which *duplicated* the call rather than
        # moving it -- so `end` ran twice, the second run overwrote what the
        # first had recorded, and the check written for this stayed green while
        # three unrelated ones went red. A mutation that lands as something
        # other than what it says is the trap index's "three ways a mutation
        # lies to you"; the anchor below is the whole tail so the move is a move.
        "drag: tell the target before the drag is torn down",
        "src/lib/drag.ts",
        "    this.live = null;\n    this.host.removeEventListener(\"pointermove\", this.onMove);\n    this.host.removeEventListener(\"pointerup\", this.onUp);\n    this.host.removeEventListener(\"pointercancel\", this.onCancel);\n    try {\n      this.host.releasePointerCapture(live.pointerId);\n    } catch {\n      // Never captured.\n    }\n    this.target.end(at, committed);",
        "    this.target.end(at, committed);\n    this.live = null;\n    this.host.removeEventListener(\"pointermove\", this.onMove);\n    this.host.removeEventListener(\"pointerup\", this.onUp);\n    this.host.removeEventListener(\"pointercancel\", this.onCancel);\n    try {\n      this.host.releasePointerCapture(live.pointerId);\n    } catch {\n      // Never captured.\n    }",
        "is already torn down by the time the target is told",
    ),
    Mutation(
        # Take any pointer's release, not the one that started the drag. A
        # second finger lifting ends a drag the first one is still doing.
        "drag: end the drag on any pointer's release",
        "src/lib/drag.ts",
        "    if (event && event.pointerId !== live.pointerId) return;",
        "    if (event && false) return;",
        "ignores its release, so the drag stays live",
    ),
    Mutation(
        # Cancel from the point the drag started rather than the last one seen.
        # Invisible until a caller uses the point a cancel reports, which the
        # box preview does.
        "drag: cancel at the starting point rather than the last point seen",
        "src/lib/drag.ts",
        "    live.at = { clientX: event.clientX, clientY: event.clientY };",
        "",
        "cancels at the last point seen, since a cancel has no point of its own",
    ),
    Mutation(
        # Drop the clamp that keeps a comment's icon on the page. `save.rs` maps
        # quads and does not police them, so an unclamped drop near the edge
        # writes a /Rect running past the page box -- which other readers clip,
        # half-draw, or place wherever they like.
        "markband: let a comment's icon hang off the edge of the page",
        "src/lib/markband.ts",
        "  const left = Math.max(0, Math.min(x, page.width - ICON_SIZE));",
        "  const left = x;",
        "keeps a comment dropped at the far edge inside the page",
    ),
    Mutation(
        # Treat a comment as a wash. It would then be drawn multiplied into the
        # paper rather than as an opaque bubble sitting on it -- and `save.rs`
        # branches on the same question, so the file would get /CA 0.4 too.
        "markband: count a comment as a wash",
        "src/lib/markband.ts",
        '  return kind === "highlight";',
        '  return kind === "highlight" || kind === "note";',
        "is not a wash, so the two questions do not collapse",
    ),
    Mutation(
        # The defect exactly as it shipped, restored. `paintMarks` filled the
        # whole quad in one colour for every kind, so while a document was open
        # an underline and a strikeout both looked like a highlight --- and the
        # saved file was right the whole time, which meant the mark changed
        # under the reader when they saved and reopened it. Reported from use.
        "markband: draw an underline as a wash, as the overlay used to",
        "src/lib/markband.ts",
        "      return { ...quad, top: quad.bottom - thickness };",
        "      return quad;",
        "sits an underline on the bottom edge",
    ),
    Mutation(
        # The subtler half. Both line kinds still draw a line, and both draw it
        # in the same place -- which is the mistake `save.rs` warns about in
        # `line_rect`: a strikeout drawn at the bottom is an underline with the
        # wrong subtype, and nothing about it looks broken.
        "markband: draw a strikeout where an underline goes",
        "src/lib/markband.ts",
        "        top: quad.top + height / 2 - thickness / 2,",
        "        top: quad.bottom - thickness,",
        "centres a strikeout on the text",
    ),
    Mutation(
        # A fixed thickness rather than a fraction of the text. Right for body
        # text and a hairline across a heading, which is `LINE_FRACTION`'s whole
        # reason -- and the fixture that catches it has to be the 36 pt line,
        # since a 12 pt one at 7% is under a point either way.
        "markband: fix the line thickness instead of scaling it with the text",
        "src/lib/markband.ts",
        "  const thickness = height * LINE_FRACTION;",
        "  const thickness = 1;",
        "scales the line with the text rather than fixing it",
    ),
    Mutation(
        # Render the declared character even when the platform has said what the
        # key prints. The palette then advertises Cmd-backslash while the menu
        # bar, which resolves the key itself, shows Cmd-#: one application
        # disagreeing with itself about one shortcut, on the layout where the
        # chord could not be typed at all until this week.
        "keys: label a positional binding by its character rather than the key cap",
        "src/lib/keys.ts",
        "  const printed = binding.code === undefined ? undefined : PRINTED[binding.code];",
        "  const printed = undefined;",
        "names the key this keyboard shows, once the platform has said",
    ),
    Mutation(
        # Merge each answer into the last. A layout change then leaves the
        # previous layout's glyph on any position the new one does not name --
        # invisible until someone switches layouts, and wrong forever after.
        "keys: merge the printed keys instead of replacing them",
        "src/lib/keys.ts",
        "  for (const key of Object.keys(PRINTED)) delete PRINTED[key];",
        "",
        "replaces the whole map rather than merging into it",
    ),
    Mutation(
        # Drop the position path. `\\` is Option-Shift-7 on a German keyboard, so
        # the character path arrives with modifiers the binding refuses -- which
        # is why Cmd-backslash had never once worked there, measured on the
        # running application before this existed.
        "keys: match a binding by its character only",
        "src/lib/keys.ts",
        "  if (binding.code !== undefined && event.code === binding.code) return true;",
        "",
        "matches the physical key when the character is unreachable",
    ),
    Mutation(
        # Hoist the position check above the modifier checks. A position is not
        # a licence to ignore the rest of the chord: Shift-Cmd on the same key
        # would then be this binding too, and the both-directions Shift bug this
        # table already carries an entry for would be back on one binding.
        "keys: let the physical key win before the modifiers are read",
        "src/lib/keys.ts",
        "  const accel = event.metaKey || event.ctrlKey;\n  if (binding.ctrl ? (!event.ctrlKey || event.metaKey) : accel !== (binding.accel ?? false)) return false;",
        "  if (binding.code !== undefined && event.code === binding.code) return true;\n  const accel = event.metaKey || event.ctrlKey;\n  if (binding.ctrl ? (!event.ctrlKey || event.metaKey) : accel !== (binding.accel ?? false)) return false;",
        "keeps the modifier checks on the position path",
    ),
    Mutation(
        # Claim the character rather than the position in the menu. The two
        # disagree on any layout that is not the one the table was written on,
        # and the menu is where that disagreement becomes a wrong shortcut shown
        # to a reader.
        "keys: build the accelerator from the character even when a position is named",
        "src/lib/keys.ts",
        "  const key = binding.code ?? plainKey(binding.keys[0] ?? \"\");",
        "  const key = plainKey(binding.keys[0] ?? \"\");",
        "withholds a punctuation chord, because position is not character",
    ),
    Mutation(
        # The symmetric edit that looks obviously right and is not: Back and
        # Forward are as untypable on a German keyboard as Cmd-backslash was, so
        # naming their positions is the natural next step -- and BracketRight is
        # the `+` key there, which zoom-in already claims.
        "keys: give Forward the physical key a German keyboard prints + on",
        "src/lib/keys.ts",
        '  "nav.forward": { keys: ["]", "\\u2018"], accel: true },',
        '  "nav.forward": { keys: ["]", "\\u2018"], code: "BracketRight", accel: true },',
        "names no physical key that a German keyboard gives to another command",
    ),
    Mutation(
        # The one that matters most on this platform. A menu accelerator is
        # claimed by AppKit before the web view sees the key, so letting an
        # unmodified binding through puts bare `n` -- next page -- in the menu
        # bar, where it is taken out of the find field and every text input the
        # application ever grows. Nothing about the menu would look wrong.
        "keys: let a binding with no accelerator key into the menu",
        "src/lib/keys.ts",
        "  if (!binding.accel) return null;",
        "",
        "refuses a binding that holds no accelerator key",
    ),
    Mutation(
        # The second refusal, and it is a different fact: these five hold the
        # modifier and are still claimed by a text field. `handleWindowKey`
        # guards undo with `inTextField` for exactly this reason, and a menu
        # accelerator would undo that guard from outside the page.
        "menubar: claim the chords a text field needs",
        "src/lib/menubar.ts",
        "  if (id in NO_ACCELERATOR) return null;",
        "",
        "withholds a chord a text field claims",
    ),
    Mutation(
        # Let a punctuation key into the menu. An accelerator names a physical
        # key and `matches` reads the character it produced, so on the German
        # layout this advertises Cmd-# beside a command whose palette entry says
        # Cmd-backslash -- measured on the running application before this rule
        # existed, along with Cmd-O-umlaut for Back.
        "keys: claim a punctuation key whose character depends on the layout",
        "src/lib/keys.ts",
        "/^[A-Z0-9]$/",
        "/^[A-Z0-9\\\\]$/",
        "refuses a punctuation key, whose position is not its character",
    ),
    Mutation(
        # Guess at a key the table does not know. The parser accepts what it can
        # read and silently claims whatever chord it read, so this takes a
        # shortcut nobody chose rather than showing none.
        "keys: guess at a key the accelerator table cannot spell",
        "src/lib/keys.ts",
        "  const upper = key.toUpperCase();\n  return /^[A-Z0-9]$/.test(upper) ? upper : null;",
        "  const upper = key.toUpperCase();\n  return upper;",
        "refuses a key it cannot spell rather than guessing",
    ),
    Mutation(
        # Every item enabled. The menu then offers commands the palette
        # withholds -- Undo on an empty journal, Install update with no update
        # -- and choosing one does nothing, which reads as a broken menu.
        "menubar: build every item enabled",
        "src/lib/menubar.ts",
        "          enabled: command.enabled?.() ?? true,",
        "          enabled: true,",
        "reads enablement from the command rather than assuming it",
    ),
    Mutation(
        # Run a command whose guard is closed. The stale-menu case is real: the
        # enablement push is a round trip, so between an edit and its answer the
        # bar is one step behind, and this is what makes that a grey item rather
        # than a wrong action.
        "menubar: run a menu item whose command is withheld",
        "src/lib/menubar.ts",
        "  if (!(command.enabled?.() ?? true)) return false;",
        "",
        # Named at the *argument* case, not at the obvious one. This mutation
        # SURVIVED a test that ran a withheld plain command and expected false:
        # `registry.run` checks `enabled` as well, so both mechanisms produce
        # the same answer and neither is tested. An argument command never
        # reaches `run` -- it opens the palette -- so the guard is the only
        # thing standing there, and that is the branch a mutation can see.
        "refuses a withheld command that would have opened the palette",
    ),
    Mutation(
        # Run an argument command straight through the registry. `run` refuses
        # it for want of a value, so the item silently does nothing -- the exact
        # failure that looks like a menu bug and is not.
        "menubar: run a command that needs a value without asking for one",
        "src/lib/menubar.ts",
        "  if (command.argument) {\n    if (!palette) return false;\n    palette.askFor(id);\n    return true;\n  }",
        "",
        "opens the palette for a command that takes a value",
    ),
    Mutation(
        # Drop the separators. Purely cosmetic and worth pinning anyway: the
        # groups are what make a fifteen-item View menu readable, and nothing
        # else in the suite looks at the shape of a section.
        "menubar: drop every separator",
        "src/lib/menubar.ts",
        '      if (entry === SEPARATOR) return [{ kind: "separator" }];',
        "      if (entry === SEPARATOR) return [];",
        "keeps separators where the layout puts them",
    ),
    Mutation(
        # An empty enablement push. Every item then keeps whatever state it was
        # built with, forever -- so the menu is correct exactly until the first
        # edit and stale after it.
        "menubar: send an empty enablement update",
        "src/lib/menubar.ts",
        "      if (command) state[command.id] = command.enabled?.() ?? true;",
        "",
        "answers for every command in the layout and nothing else",
    ),
    Mutation(
        # A command missing from the layout, which is the state the whole
        # application was in: reachable from the palette, absent from the menu,
        # and nothing saying so. Deleting the page strip's own delete is the
        # case that started this.
        "menubar: leave a command out of the menu",
        "src/lib/menubar.ts",
        '      "edit.deletePage",\n',
        "",
        "gives every registered command a menu or a written reason",
    ),
    Mutation(
        # Lexicographic sort, which is what `Array.prototype.sort` does without
        # a comparator. Page 10 lands before page 2, `write_copy` writes a valid
        # PDF in an order nobody asked for, and no downstream check could see
        # it: the file opens, it has the right pages, and it is wrong.
        "pageranges: sort the slots the way JavaScript sorts by default",
        "src/lib/pageranges.ts",
        "  return { slots: [...slots].sort((a, b) => a - b) };",
        "  return { slots: [...slots].sort() };",
        "sorts numerically, so page 11 comes after page 3",
    ),
    Mutation(
        # Read a backwards range as the range it resembles. A reader who typed
        # `5-3` made a mistake, and this is the version that silently gives them
        # three pages instead of telling them.
        "pageranges: quietly correct a range that runs backwards",
        "src/lib/pageranges.ts",
        "    if (from > to) {\n      return { problem: `${from}-${to} runs backwards` };\n    }",
        "    if (from > to) {\n      return { slots: [] };\n    }",
        "refuses a range that runs backwards instead of correcting it",
    ),
    Mutation(
        # Off by one at the conversion. Every page a reader names comes out one
        # later, which on a range still produces the right *number* of pages --
        # so a check counting them passes.
        "pageranges: keep the reader's one-based numbers as slots",
        "src/lib/pageranges.ts",
        "      slots.add(one - 1);",
        "      slots.add(one);",
        "reads a single page as one zero-based slot",
    ),
    Mutation(
        # Exclusive at the top, which is what a reader of `2-4` does not mean
        # and what a half-open loop written from habit produces.
        "pageranges: make a range exclusive at its top end",
        "src/lib/pageranges.ts",
        "    for (let page = from; page <= to; page += 1) slots.add(page - 1);",
        "    for (let page = from; page < to; page += 1) slots.add(page - 1);",
        "reads a range inclusively at both ends",
    ),
    Mutation(
        # Accept anything `Number()` accepts. `2.0` becomes page 2, `1e1`
        # becomes page 10, and `+2` becomes page 2 -- none of which is a page
        # number a reader typed on purpose.
        "pageranges: accept every numeric literal rather than digits",
        "src/lib/pageranges.ts",
        '  if (!/^[0-9]+$/.test(text)) return `"${text}" is not a page number`;',
        "  if (Number.isNaN(Number(text))) return `not a page number`;",
        "refuses +2, which Number() would have accepted",
    ),
    Mutation(
        # Run the extract with whatever parsed, including nothing. The palette
        # guards this, so the mutation is aimed at the second line of defence --
        # the one that decides whether a defect writes a file.
        "appcommands: extract even when the range did not parse",
        "src/lib/appcommands.ts",
        "          if (!range.slots) return;\n          actions.extractPages(range.slots);",
        "          actions.extractPages(range.slots ?? []);",
        "refuse to extract what does not parse, and reach no action",
    ),
    Mutation(
        # Send the page's position instead of its identity. Identical on an
        # unedited document and wrong the moment a page moves, which is the whole
        # reason ids cross the boundary at all.
        "edits: name a page by its slot rather than by its id",
        "src/lib/edits.ts",
        "    const id = this.current.pages[page]?.id;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await call(\"page_rotate\"",
        "    const id = page;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await call(\"page_rotate\"",
        "sends the page's identity, not its position",
    ),
    Mutation(
        # The same defect on the command that removes a page, where it is worse:
        # a rotation aimed at the wrong page can be undone by looking at it, and
        # a deletion aimed at the wrong page cannot.
        "edits: delete a page by its slot rather than by its id",
        "src/lib/edits.ts",
        "    const id = this.current.pages[page]?.id;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await call(\"page_delete\"",
        "    const id = page;\n    if (id === undefined) return this.current;\n    return this.adopt(\n      await call(\"page_delete\"",
        "deletes by identity, not by position",
    ),
    Mutation(
        # Read the anchor out of the order that still holds the page being
        # moved. The model removes it *before* reading the anchor's position, so
        # this is one slot short for every move towards the back --- a drag that
        # lands just before where the reader dropped it.
        "edits: pick the move anchor out of the order the page is still in",
        "src/lib/edits.ts",
        "    const rest = this.current.pages.filter((_, slot) => slot !== from);",
        "    const rest = this.current.pages;",
        "turns a destination slot into the neighbour the model accepts",
    ),
    Mutation(
        # Name no anchor at all, which puts every moved page at the front. Half
        # of all one-slot moves then land correctly, and a suite reading only the
        # page count sees nothing wrong with any of them.
        "edits: move every page to the front whatever slot was asked for",
        "src/lib/edits.ts",
        "    const after = landing === 0 ? null : (rest[landing - 1]?.id ?? null);",
        "    const after = null;",
        "names the page after the one being moved for a single step back",
    ),
    Mutation(
        # The same defect in the accessibility tree, where its reader can least
        # easily notice it: a screen reader goes on reading the order the
        # document used to be in, and nothing in the tree says otherwise.
        "a11y: keep the built pages whenever the page count did not change",
        "src/lib/a11y.ts",
        "  setPages(pageCount: number): void {\n    this.pageCount = pageCount;",
        "  setPages(pageCount: number): void {\n    if (pageCount === this.pageCount) return;\n    this.pageCount = pageCount;",
        "rebuilds when the order changes and the page count does not",
    ),
    Mutation(
        # And in the search results, where it is a highlight over a passage that
        # is not there.
        "search: keep the matches whenever the page count did not change",
        "src/lib/search.ts",
        "  setPages(pageCount: number): void {\n    this.pageCount = pageCount;",
        "  setPages(pageCount: number): void {\n    if (pageCount === this.pageCount) return;\n    this.pageCount = pageCount;",
        "drops the matches when the order changes and the page count does not",
    ),
    Mutation(
        # Put the early return back. It was there, it was right for the deletion
        # it was written for, and it is what leaves the page strip showing the
        # old order after a move: same rows, same captions, the wrong pictures
        # under them, and no observable in the strip that says otherwise.
        "thumbnails: keep the strip whenever the page count did not change",
        "src/lib/thumbnails.ts",
        "  setPages(pageCount: number): void {\n    this.opts.pageCount = pageCount;",
        "  setPages(pageCount: number): void {\n    if (pageCount === this.opts.pageCount) return;\n    this.opts.pageCount = pageCount;",
        "throws its thumbnails away when the order changes and the count does not",
    ),
    Mutation(
        # Moved here from `mutate_viewer.py` on 2026-08-17, and the move is the
        # finding. Aimed at the window harness it SURVIVED a full run: the
        # separator exists only for a tagged block, `tagged.pdf` is the only
        # corpus with a structure tree, and none of its blocks wraps --- so no
        # fixture reached the branch and nothing could go red. Writing the test
        # that judges it found the branch had been broken since it was written.
        "a11y: hand a paragraph's lines over with no space between them",
        "src/lib/a11y.ts",
        "        if (wrote || piece) piece += \" \";",
        "        if (false) piece += \" \";",
        "joins the lines of one tagged paragraph with a space",
    ),
    Mutation(
        # The off-by-one the whole helper exists for. Reading the gap against
        # the order the page is still in, and handing that straight to the
        # model, leaves every drag towards the back of the document one slot
        # short --- and a drag one slot down does nothing at all.
        "thumbnails: drop a page where the gap says rather than where it lands",
        "src/lib/thumbnails.ts",
        "  return gap > from ? gap - 1 : gap;",
        "  return gap;",
        "takes one off a gap below the page, because the page has left it",
    ),
    Mutation(
        # The gap boundary moves from the middle of a row to its top edge, so
        # the page lands one slot from where the indicator was drawn --- which
        # is the failure a reader reads as the strip ignoring them.
        "thumbnails: take the gap from the row's top edge rather than its middle",
        "src/lib/thumbnails.ts",
        "  const gap = Math.round(contentY / rowHeight);",
        "  const gap = Math.floor(contentY / rowHeight);",
        "names the gap below the row the pointer is in the bottom half of",
    ),
    Mutation(
        # Every press becomes a drag, so a plain click on a thumbnail reorders
        # the document whenever the pointer moved by a pixel.
        "thumbnails: treat every press as a drag, however still the pointer",
        "src/lib/thumbnails.ts",
        "      if (Math.abs(event.clientY - press.startY) < DRAG_THRESHOLD) return;",
        "      if (false) return;",
        "does not reorder anything when the pointer barely moves",
    ),
    Mutation(
        # A second pointer aims the first one's drag. The guard is one clause
        # and reads like defensiveness; it is what keeps a trackpad's second
        # finger from choosing where the page lands.
        "thumbnails: let any pointer aim a drag another one started",
        "src/lib/thumbnails.ts",
        "    if (!press || event.pointerId !== press.pointerId) return;",
        "    if (!press) return;",
        "ignores a pointer that is not the one being dragged",
    ),
    Mutation(
        # `setPages` is what a drop's own edit comes back through, so completing
        # the drag there applies the reader's move a second time.
        "thumbnails: complete the drag when the document is rebuilt under it",
        "src/lib/thumbnails.ts",
        "    // Every row is about to be rebuilt, so a drag still in flight is aimed at",
        "    this.endDrag(true);\n    // Every row is about to be rebuilt, so a drag still in flight is aimed at",
        "runs no edit when the document is rebuilt under a live drag",
    ),
    Mutation(
        # The capture is never released, so the strip goes on receiving every
        # pointer event on the page for the rest of the session.
        "thumbnails: keep the pointer it captured after the drag ends",
        "src/lib/thumbnails.ts",
        "      if (this.host.hasPointerCapture(press.pointerId)) {",
        "      if (false) {",
        "releases the pointer it captured",
    ),
    Mutation(
        # The loop runs for the life of the drag on a strip that cannot move,
        # which is a frame callback per frame doing nothing.
        "thumbnails: keep the edge loop running when the strip cannot scroll",
        "src/lib/thumbnails.ts",
        "      if (this.host.scrollTop !== was) this.edgeFrame = requestAnimationFrame(step);",
        "      this.edgeFrame = requestAnimationFrame(step);",
        "stops the edge loop when the strip can scroll no further",
    ),
    Mutation(
        # The strip follows the page being read, and pressing a row navigates ---
        # so without this guard the content slides out from under a pointer that
        # has not moved, at the exact instant a drag begins. Four of the fourteen
        # window corpora caught it; the ten that did not had strips short enough
        # for the scroll to clamp.
        "thumbnails: follow the page being read even under a live press",
        "src/lib/thumbnails.ts",
        "    if (this.press) return;\n    const row = this.rows.get(page);",
        "    const row = this.rows.get(page);",
        "does not follow the page being read while a pointer is down on a row",
    ),
    Mutation(
        # The fourth roving list to get this fix, and the last. `focused` is a
        # mirror of the DOM's focus and a window without system focus moves
        # `activeElement` without delivering `focusin`, so a handler that steps
        # from the mirror steps from a row the reader is not on. Enter carried
        # the whole explanation in this file while the arrows read the mirror;
        # deleting the reconciliation restores that, and the arrow test goes red
        # on the row it lands on rather than on the one it left.
        "thumbnails: step the arrows from the tracked row, not the one the key reached",
        "src/lib/thumbnails.ts",
        "    if (from !== this.focused && this.rows.has(from)) this.focus(from);\n",
        "",
        "steps from the row the key reached, not the one it last tracked",
    ),
    Mutation(
        # Scrolling per event rather than per frame, so the speed follows the
        # pointer's report rate --- and a reader holding still at the edge stops
        # moving, which is the case the loop exists for.
        "thumbnails: scroll only while the pointer keeps moving",
        "src/lib/thumbnails.ts",
        "    this.edgeFrame = requestAnimationFrame(step);\n  }",
        "    step();\n  }",
        "scrolls the strip while the pointer rests against the bottom edge",
    ),
    Mutation(
        # Send a move that moves nothing. The model applies it, so the reader
        # gains an undo that visibly does nothing --- and a drag that ends where
        # it began becomes a journal entry.
        "edits: send a move onto the page's own slot",
        "src/lib/edits.ts",
        "    if (landing === from) return this.current;",
        "",
        "sends nothing for a move that changes no order",
    ),
    Mutation(
        # Answer a deleted page with a slot instead of nothing. Everything the
        # backend sends about a page --- a link, a comment, a destination --- then
        # lands on whichever page moved into the gap.
        "pages: answer a page that is gone with a slot rather than with nothing",
        "src/lib/pages.ts",
        "    return this.bySource.get(source);",
        "    return this.bySource.get(source) ?? source;",
        "says a deleted page is nowhere rather than answering with a slot",
    ),
    Mutation(
        # The fallback the class exists to refuse: right for every unedited
        # document, and asks for the wrong page in exactly the case it is for.
        "pages: fall back to the slot when it draws no page",
        "src/lib/pages.ts",
        "    return view === undefined ? undefined : baselineOf(view.source);\n  }\n\n  /**\n   * Where the page in a slot is drawn from",
        "    return view === undefined ? slot : (baselineOf(view.source) ?? slot);\n  }\n\n  /**\n   * Where the page in a slot is drawn from",
        "says a slot past the end is nowhere rather than falling back to itself",
    ),
    Mutation(
        # Compare two orders by which page of the file is in each slot rather
        # than by identity. A page restored by an undo is then "the same page",
        # and the viewer takes the cheap path that only re-reads the turns.
        # ⚠ **Structurally, not with `===`, and the first version got that wrong
        # for as long as it stood.** `PageSource` is an object, so `page.source
        # === other.source` is an *identity* comparison between two separately
        # built objects and is false almost everywhere -- which made `sameOrder`
        # answer false for two identical orders and reddened the positive test
        # rather than the negative one this names. The mutation was caught and
        # the harness said so, by printing the test that went red beside the one
        # it expected; without that line it would have read as a pass.
        #
        # Comparing the sources' *shape* is what the entry means, and it is the
        # defect a reader could actually write: two orders holding the same file
        # pages in the same slots under different page identities then compare
        # equal, so an undo that restored a page reads as no change at all.
        "pages: compare two orders by source rather than by identity",
        "src/lib/pages.ts",
        "    return this.views.every((page, slot) => page.id === other.views[slot]?.id);",
        "    return this.views.every(\n      (page, slot) =>\n        JSON.stringify(page.source) === JSON.stringify(other.views[slot]?.source),\n    );",
        "is false when the same sources arrive under different identities",
    ),
    Mutation(
        # Hand `findIndex`'s -1 back as though it were a slot. The viewer's
        # `?? Math.min(...)` does not fire on it, so the reader is put at "slot
        # minus one" --- and the page that was deleted reads as a page that
        # moved somewhere impossible rather than as a page that is gone.
        #
        # Deliberately not the mutation this looked like it wanted, which was to
        # follow a page by its `source` instead of its `id`. Ids are allocated
        # one per baseline page, so on every document that exists today
        # `source == id - 1` and the two are the same function --- a variant
        # rather than a gap, which `docs/TRAPS.md` says to check for before
        # strengthening anything. It becomes distinguishable when a page can be
        # duplicated, and `docmodel.rs` says what has to be proved first.
        "pages: report a page that has gone as the slot before the first",
        "src/lib/pages.ts",
        "    return found === -1 ? undefined : found;",
        "    return found;",
        "says nothing for a page that is no longer there",
    ),
    Mutation(
        # Draw a link whose own page has been deleted. It is hit-tested against
        # whichever page moved into that slot, so a cross-reference appears in
        # the middle of a page that never had one.
        "pages: keep a link whose page has been deleted",
        "src/lib/pages.ts",
        "    const slot = pages.slotOf(link.page);\n    if (slot === undefined) continue;",
        "    const slot = pages.slotOf(link.page) ?? link.page;",
        "leaves out a link on a page that is gone",
    ),
    Mutation(
        # Leave a destination pointing at a page that is not in the document.
        # `goToDestination` then scrolls to whatever is in that slot.
        "pages: leave a destination pointing at a page that has gone",
        "src/lib/pages.ts",
        "  const slot = pages.slotOf(target.page);\n  if (slot === undefined) return { kind: \"broken\" };",
        "  const slot = pages.slotOf(target.page);\n  if (slot === undefined) return target;",
        "keeps a link whose destination is gone, and calls it broken",
    ),
    Mutation(
        # List a comment on a page nobody can see. It opens against the page that
        # moved into the slot, attributing somebody's note to a page they never
        # wrote on.
        "pages: keep a comment on a page that has been deleted",
        "src/lib/pages.ts",
        "    const slot = pages.slotOf(comment.page);\n    if (slot === undefined) continue;",
        "    const slot = pages.slotOf(comment.page) ?? comment.page;",
        "moves a comment to the slot its page is in and drops the rest",
    ),
    Mutation(
        # Carry each page's state by slot rather than by identity. Every page
        # below the gap then inherits the size and the tile epoch of the page
        # that used to be there --- invisible on a document whose pages are all
        # the same size, which is most of them, so this is asserted here rather
        # than left to a window check on a corpus that might not show it.
        "scroller: carry a page's learned size by its slot rather than by its page",
        "src/lib/scroller.ts",
        "      const was = at.get(page.id);",
        "      const was = this.order.indexOf(page);",
        "carries a learned size to wherever the page moved to",
    ),
    Mutation(
        # Keep what was painted across a change of order. Every tile is placed by
        # the slot it was requested for, so after a deletion the surviving pixels
        # are in the wrong places rather than merely stale --- and a reply still
        # in flight is adopted, because the generation bump that would have
        # dropped it went with the call.
        "scroller: keep the tiles when the page order changes",
        "src/lib/scroller.ts",
        "    this.clearTiles();\n    this.dropPlaceholders();\n    this.estimate = this.meanKnownSize();",
        "    this.dropPlaceholders();\n    this.estimate = this.meanKnownSize();",
        "drops a tile that was rendering when the order changed",
    ),
    Mutation(
        # Turn the view instead of the page. Every statement about the page that
        # was turned still holds; only its neighbour can tell.
        "scroller: apply a page's turn to the whole view",
        "src/lib/scroller.ts",
        "    return this.opts.turns + (this.pageTurns[page] ?? 0);",
        "    return this.opts.turns + (this.pageTurns[0] ?? 0);",
        "lays the turned page out sideways and leaves its neighbour alone",
    ),
    Mutation(
        # Lay the page out upright and render it turned. The box keeps the old
        # shape and the tile drawn into it is the new one.
        "scroller: leave the page's own turn out of the layout",
        "src/lib/scroller.ts",
        "    return displayedSize(this.pageSize(page), this.effectiveTurns(page));",
        "    return displayedSize(this.pageSize(page), this.opts.turns);",
        "lays the turned page out sideways and leaves its neighbour alone",
    ),
    Mutation(
        # Let the geometry decide what to invalidate. A half turn moves no box,
        # so the old pixels stay on screen upside down.
        "scroller: invalidate a turned page only when its box moves",
        "src/lib/scroller.ts",
        "    this.invalidatePage(page);\n    const moved = this.applySizes();",
        "    const moved = this.applySizes();",
        "discards a page's pixels even when its box does not move",
    ),
    Mutation(
        # Ask the renderer for the view's turn alone. The page is laid out
        # sideways and drawn upright inside its own box.
        "scroller: request a tile without the page's own turn",
        "src/lib/scroller.ts",
        "      turns: this.requestTurns(key.page),",
        "      turns: this.opts.turns,",
        "asks the renderer for the page's turn composed with the view's",
    ),
    Mutation(
        # Turn every page's text, not the one that was edited. Selection then
        # lands wrongly on every page except the one that was turned.
        "text: apply a page's turn to the whole cache",
        "src/lib/text.ts",
        "    const turns = this.turns + (this.extra.get(page) ?? 0);",
        "    const turns = this.turns + (this.extra.values().next().value ?? 0);",
        "turns only the page the edit named",
    ),
    Mutation(
        # Drop every turned view rather than the one page's. Correct and
        # wasteful, and invisible through `peek` --- only the accounting can see
        # it, which is why there is one.
        # Re-aimed 2026-08-18: `setPageCrop` drops the same page's turned view,
        # so the one-line anchor now matches twice. Widened to the two lines
        # above it, which are `setPageTurns`'s and nothing else's.
        "text: clear the whole turned cache when one page is turned",
        "src/lib/text.ts",
        "    else this.extra.set(page, next);\n    this.turned.delete(page);",
        "    else this.extra.set(page, next);\n    this.turned.clear();",
        "drops only that page's turned view when its turn changes",
    ),
    Mutation(
        # Both rotations clockwise. The command reaches the right action and
        # turns the page the wrong way.
        "commands: rotate the page clockwise whichever way was asked",
        "src/lib/appcommands.ts",
        "      run: () => actions.rotatePage(-1),",
        "      run: () => actions.rotatePage(1),",
        "rotate the page with the sign the reader asked for",
    ),
    Mutation(
        # Offer Save on any open document. Pressing it on one nobody edited
        # rewrites every object id in the file, which is a change to a document
        # the reader did not change.
        "commands: offer Save whenever a document is open",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.isDirty(),",
        "      enabled: withDocument,",
        "offer Save only once there is something to save",
    ),
    Mutation(
        # Drop the document half of Save's guard and keep the dirty half. The
        # command then survives the document being closed, because `dirty` is a
        # variable and nothing clears it.
        "commands: guard Save on the journal alone",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.isDirty(),",
        "      enabled: () => actions.isDirty(),",
        "withholds Save with no document, however dirty the model claims to be",
    ),
    Mutation(
        # Let the chord through with nothing to save. The reader who presses
        # Cmd-S by reflex on an untouched document gets the backend's refusal
        # as an error message.
        "commands: reach the save action on Cmd-S with nothing to save",
        "src/lib/appcommands.ts",
        "    if (actions.isDirty()) actions.saveDocument();",
        "    actions.saveDocument();",
        "does nothing on Cmd-S with nothing to save",
    ),
    Mutation(
        # Route a save in place through the copy path. That path refuses the
        # source outright, so Save could never write anything -- and the reader
        # is told their document changed under them.
        "edits: send a save in place to the copy command",
        "src/lib/edits.ts",
        '    await call("save_document", { doc: this.doc, source });',
        '    await call("save_copy", { doc: this.doc, source });',
        "names the open document and no destination when it saves in place",
    ),
    Mutation(
        # Offer Undo with an empty journal. The palette then teaches a reader
        # that the command does nothing.
        "commands: offer Undo whenever a document is open",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.canUndo(),",
        "      enabled: withDocument,",
        "are withheld while the journal is empty",
    ),
    Mutation(
        # One flag for both halves of the journal. A document with an edit and
        # nothing undone then offers Redo as well.
        "commands: guard Redo on there being something to undo",
        "src/lib/appcommands.ts",
        "      enabled: () => withDocument() && actions.canRedo(),",
        "      enabled: () => withDocument() && actions.canUndo(),",
        "are offered separately, each on its own half of the journal",
    ),
    Mutation(
        # Take Cmd-Z from the find field. A reader correcting a typo silently
        # undoes a page rotation instead, and nothing connects the two.
        "keys: take Cmd-Z out of the text field a reader is typing in",
        "src/lib/appcommands.ts",
        "  } else if (matches(\"edit.undo\", event) && title && !inTextField(event)) {",
        "  } else if (matches(\"edit.undo\", event) && title) {",
        "leaves Cmd-Z to the text field a reader is typing in",
    ),
    Mutation(
        # The half of the guard a tag-name check cannot cover: a contenteditable
        # element is a text field and is not an INPUT. Aimed at the field rather
        # than at the spelling, because the spelling is what makes the guard
        # testable here at all --- see the note on the function.
        # Re-pathed 2026-08-18: `inTextField` moved to `keys.ts`, because the
        # viewer's own handler needs it too and two copies of "what is a text
        # field" would be two chances to disagree. The mutation is unchanged and
        # so is the test it names --- Cmd-Z from the find bar.
        "keys: miss a contenteditable target",
        "src/lib/keys.ts",
        "  if (target.isContentEditable === true) return true;",
        "  if (false) return true;",
        "leaves Cmd-Z to the text field a reader is typing in",
    ),
    Mutation(
        # `resumé` decomposed came back as three lines --- `resume`, the accent
        # alone, then the rest --- because an acute sits above the x-height and its
        # box does not touch a word with no ascender. `café` hides it: the `f`
        # reaches up far enough to drag the band into contact.
        "mark: let a combining mark open a band of its own",
        "src/lib/reading.ts",
        "    const mark = last >= 0 && combining(text.codes[index] ?? 0);",
        "    const mark = false;",
        "does not open a line of its own",
    ),
    Mutation(
        # The other direction: attach the mark to the character *after* it. The
        # line count is then right and the text reads `resum` `é` the wrong way
        # round, which a check on the number of lines cannot see.
        "mark: attach a combining mark to the character after it",
        "src/lib/reading.ts",
        "      const at = trailing.get(last) ?? [];\n      at.push(index);\n      trailing.set(last, at);\n      if (mark && placed(box)) {",
        "      const at = trailing.get(mark ? index : last) ?? [];\n      at.push(index);\n      trailing.set(mark ? index : last, at);\n      if (mark && placed(box)) {",
        "stays with the character it decorates",
    ),
    Mutation(
        # Key on the geometry rather than on the character: a small box high up.
        # It catches the accent and also catches a superscript, which is a
        # character in its own right with its own advance width.
        "mark: treat anything small and raised as a combining mark",
        "src/lib/reading.ts",
        "const COMBINING = /^[\\p{Mn}\\p{Me}]$/u;",
        "const COMBINING = /^[\\p{Mn}\\p{Me}0-9]$/u;",
        "keys on the character rather than on the box",
    ),
    Mutation(
        # Leave the mark out of the base's box. The line then reads correctly and
        # cannot be hit-tested at the top of the accent, which is the half a text
        # comparison is blind to.
        "mark: leave a combining mark out of its line's box",
        "src/lib/reading.ts",
        "          absorb(base.box, box);\n          base.extents = extentsOf(base.box, axes);",
        "",
        "is covered by its line's box",
    ),
    Mutation(
        "word: do not walk left from the clicked character",
        "src/lib/text.ts",
        "  while (from > 0 && classOf(codes[from - 1] ?? 0) === kind) from--;",
        "",
        "selects the run of letters a character sits in",
    ),
    Mutation(
        "word: do not walk right from the clicked character",
        "src/lib/text.ts",
        "  while (to < codes.length && classOf(codes[to] ?? 0) === kind) to++;",
        "",
        "selects the run of letters a character sits in",
    ),
    Mutation(
        "word: treat every character as a word character",
        "src/lib/text.ts",
        '  if (WORD_CHARACTER.test(char)) return "word";',
        '  if (char) return "word";',
        "selects the second word, not the whole line",
    ),
    Mutation(
        # Predicted against the hyphen test first, and it survived: a lone mark
        # comes out the same whether it is returned directly or walked outwards
        # from, since its neighbours are a different class either way. Only a
        # *run* of marks distinguishes the two, which nothing covered.
        "word: let a punctuation mark join the run beside it",
        "src/lib/text.ts",
        '  if (kind === "mark") return { from: at, to: at + 1 };',
        "",
        "selects one mark of a run of punctuation, not the run",
    ),
    Mutation(
        "word: do not clamp an index past the last character",
        "src/lib/text.ts",
        "  const at = Math.min(Math.max(index, 0), codes.length - 1);",
        "  const at = index;",
        "does not run past the ends of the page",
    ),
    Mutation(
        "word: drop combining marks from the word class",
        "src/lib/text.ts",
        "const WORD_CHARACTER = /[\\p{L}\\p{N}\\p{M}_]/u;",
        "const WORD_CHARACTER = /[\\p{L}\\p{N}_]/u;",
        "treats a combining mark as part of the word",
    ),
    Mutation(
        "line: miss the first character of a line",
        "src/lib/text.ts",
        "    if (at >= line.from && at < line.to) return line;",
        "    if (at > line.from && at < line.to) return line;",
        "includes the first character of a line",
    ),
    Mutation(
        "line: return the word instead of the line",
        "src/lib/text.ts",
        "  for (const line of linesOf(text)) {",
        "  for (const line of [wordAt(text, at)]) {",
        "selects the whole line, not the word under the pointer",
    ),
    Mutation(
        "clicks: measure the slop on x only",
        "src/lib/clicks.ts",
        "      Math.abs(x - this.x) <= MULTI_CLICK_SLOP_PX && Math.abs(y - this.y) <= MULTI_CLICK_SLOP_PX;",
        "      Math.abs(x - this.x) <= MULTI_CLICK_SLOP_PX;",
        "measures the slop on both axes",
    ),
    Mutation(
        "clicks: exclude the deadline instead of including it",
        "src/lib/clicks.ts",
        "    const soon = nowMs - this.atMs <= MULTI_CLICK_MS;",
        "    const soon = nowMs - this.atMs < MULTI_CLICK_MS;",
        "counts a click at exactly the deadline as part of the run",
    ),
    Mutation(
        "clicks: count upwards forever instead of wrapping",
        "src/lib/clicks.ts",
        "    this.count = near && soon ? (this.count % 3) + 1 : 1;",
        "    this.count = near && soon ? this.count + 1 : 1;",
        "wraps back to a single click after the third",
    ),
    Mutation(
        "clicks: keep the run's first position instead of the last",
        "src/lib/clicks.ts",
        "    this.x = x;\n    this.y = y;",
        "    if (this.count === 1) {\n      this.x = x;\n      this.y = y;\n    }",
        "measures the distance from the last click, not from where the run began",
    ),
    Mutation(
        "clicks: measure the gap from the run's first click",
        "src/lib/clicks.ts",
        "    this.atMs = nowMs;",
        "    if (this.count === 1) this.atMs = nowMs;",
        "measures the gap from the last click, not from the first",
    ),
    Mutation(
        # The along-line coordinate and its midpoint must use the same axis.
        "caret: use the horizontal coordinate on a turned page",
        "src/lib/text.ts",
        "const position = turns % 2 ? y : x;",
        "const position = x;",
        "splits on the reading axis when the page is turned",
    ),
    Mutation(
        "caret: on an upright page, never place it after the character",
        "src/lib/text.ts",
        "turns >= 2 ? position < middle : position > middle",
        "turns >= 2 ? position < middle : false",
        "puts the caret after a character the pointer is past the middle of",
    ),
    Mutation(
        "caret: fall back to the last character rather than the first",
        "src/lib/text.ts",
        "  if (best < 0) return 0;",
        "  if (best < 0) return text.codes.length;",
        "puts the caret at the start of a page that places no characters",
    ),
    Mutation(
        "nearest: ignore the weight, so a click lands a line away",
        "src/lib/text.ts",
        "    const distance = along * along + (across * ACROSS_LINE_WEIGHT) ** 2;",
        "    const distance = along * along + across ** 2;",
        "weights distance across the lines, not along them",
    ),
]
