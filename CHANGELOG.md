# Changelog

All notable changes to tpdf are recorded here.

Versioning is CalVer `YY.M.MICRO` --- see [`BUILD.md`](BUILD.md).

The first release is `26.8.0`, tagged 2026-08-03 and **published 2026-08-12**. Everything
below it predates any shipped binary: Phase 0 was a feasibility investigation, and what it
produced is measurements and a verdict on each load-bearing assumption rather than a viewer.
Those entries are kept rather than collapsed, so that the first release has a history instead
of a single "initial release" line.

(This preamble said "Nothing has shipped yet" until 2026-08-05, two days after the tag and
the installers it built. A file whose header contradicts its own top entry is read header
first. It then said only "tagged 2026-08-03" until 2026-08-12 --- true, and read by everyone
as *downloadable*, while the release sat as a draft that GitHub showed to nobody. Both dates
are given now because they are different facts, and only the second one means a reader can
have the binary.)

## [26.8.6] - Unreleased

### Which version am I running

- **About tpdf** says so, from the palette and from the tpdf menu. It asks the
  network nothing, which is the point: the reason to want the number is usually
  that something is wrong, and an answer that needs a working connection is no
  answer exactly then.

- The version is also on the empty window, under "Open a PDF, or drop one here".

- Until now nothing in the application reported a version at all. That is how a
  reader on 26.8.4 came to file a bug that 26.8.5 had already fixed --- there was
  no way to tell which of the two was running.

### Recent documents on the taskbar

- Right-clicking tpdf's taskbar icon now lists the documents you opened. Every
  way of opening one counts: dropping a file on the window, double-clicking in
  Explorer, a path on the command line, and the Open panel.

- It showed nothing before, and the reason was not a broken setting: nothing had
  ever told Windows a document was opened. tpdf's own recent list --- the one in
  the command palette --- is a separate thing that the operating system never
  sees, so having one never meant having the other.

- **Needs an installed tpdf.** The Windows jump list hangs off the Start Menu
  shortcut the installer writes, so a copy run straight out of a build directory
  will still show nothing.

- Not on macOS yet. The equivalent is the Dock icon's Recent Documents, and it
  has to be done on the main thread, which the open path does not currently
  guarantee.

### tpdf will not save over a file that changed while you had it open

- If another program writes to the file you are reading --- a colleague replacing
  it, a sync client landing a newer copy, a tool re-exporting over the top ---
  saving is refused, and your edits stay exactly where they are. Save them under
  another name, or open the file again to start from what is on disk now.

- Until now the only thing checked was whether the **number of pages** had
  changed. Anything that kept the page count went through: your edits were
  written onto a document they were never made on, and the result was a file that
  opened perfectly well and was wrong.

- **Save a copy** is checked too, for the same reason --- the difference is only
  that it cannot destroy the original.

- What is checked is the file's length, its modification time, and a hash of
  every byte, recorded when you opened it and compared again the moment before
  anything is written.

- If tpdf could not record what the file looked like when it opened, it refuses
  to save over it and says so. **Save a copy** still works in that case.

- The message you get says what is actually true at the moment you get it. There
  are two: one before anything is touched, which says your edits are still here
  and to save them under another name, and one from the moment just before the
  file is replaced, which says the document has been closed. Until now both said
  the first thing, so a refusal at the second point told you to save edits that
  were already gone --- in the same sentence as telling you the document was
  closed.

- A file whose timestamp moved but whose contents did not still saves. A backup
  tool, a sync client or a `touch` all do that, and the file in front of you is
  byte for byte the one you opened --- so being sent away from your own save over
  it would be a false alarm at the worst possible moment.

### Fixed: "Check for updates" appeared to do nothing when you were up to date

- It reported a newer version when there was one and said nothing at all when
  there was not, which is indistinguishable from a command that did not run. It
  now answers in every case, and names the running version while it does.

## [26.8.5] - 2026-08-19

### Draw a box on a page

- **Draw a box...** arms the pointer; the next drag on a page draws a red
  rectangle where you dragged. It is in the right-click menu on a page, in the
  Edit menu and in the palette. Like every other mark it takes a note, it is
  removable, and Undo puts it back.

- It is the first mark you place by drawing rather than by selecting text, so it
  is also the first that can go anywhere --- round a figure, a table, a signature
  block, a stamp somebody else left.

- The tool is armed for **one** box. After you draw it the pointer goes back to
  selecting text, so you can never be left in a drawing mode without noticing. A
  press that does not travel draws nothing and keeps the tool armed, and Escape
  drops it.

- The box is saved as a real `/Square` annotation with its own appearance, so
  Acrobat and Preview draw the same rectangle in the same place. It is an
  outline, not a fill: whatever you drew it around stays readable.

- A drag that runs off the edge of the page is trimmed to the page.

### Fixed: on a rotated or cropped page, a comment landed in the wrong place

- **Add comment** placed the bubble using the page as it appears on screen rather
  than as the file describes it. On an ordinary upright page those are the same
  thing, which is why it went unnoticed; on a page the file rotates, or one you
  had cropped, the bubble went somewhere else.

- Introduced in 26.8.4 and fixed before it reached any release.

### Save

- **Save** (⌘S) writes your edits into the file you opened. Until now the only
  way to keep a highlight, a turned page or a deletion was **Save a copy**, which
  leaves you reading the file you started with --- so every edit had to be saved
  somewhere else and then found again.

- It is offered only when there is something to save, and the document is
  reopened afterwards, at the page you were on. A save cannot be undone: the
  journal describes a file that has been replaced, so Undo starts again from
  what you just saved.

- A save that cannot be done leaves your document exactly as it was --- an
  encrypted file, a file that changed on disk under you, a page that cannot be
  written. Nothing is touched until every one of those has been checked.

- **Save a copy** is unchanged, and still refuses to write over the open
  document: that is what Save is for.

### Fixed: on Windows, an installed tpdf could not open any document at all

- However it was started --- the Start menu, a desktop icon, a double-clicked
  PDF, or the exe from a PowerShell prompt --- 26.8.4 on Windows showed **"this
  process has no stderr to share"** and opened nothing at all.

- It was not caught because the automated checks here read the app's own output,
  which on Windows means handing it a stderr it would not otherwise have. The
  checks therefore created the one condition under which it works.

- Reported by a reader on their first Windows install, on the day 26.8.4 shipped.
  macOS was never affected.

## [26.8.4] - 2026-08-18

### Crop a page to what is on it

- **Crop page to content** trims the margins away so the text fills the window.
  It measures where the ink actually is, so it works on a scan as well as on a
  page of type --- on the fixtures here it roughly doubles to quadruples how much
  of the screen is print. **Reset page crop** puts the file's own page back.
  Both are in the Page menu and in the palette.

- The crop is part of the document, so it is undoable, it travels with a page you
  move, and it is written into a saved copy as a real `/CropBox` --- other readers
  open the file cropped the way you left it.

- Comments, links and your own highlights stay on the words they were on. So does
  a highlight you make *while* the page is cropped, if you later change the crop
  or take it off.

- There is no crop-by-dragging yet.

### Reach your own marks from the keyboard

- **Next mark** (⌥⌘M) and **Previous mark** (⇧⌥⌘M) walk the marks you have
  made, in the order they sit in the document, opening each one's note as they
  arrive. They scroll to a mark that is off screen, they stop at either end
  rather than wrapping round, and they say so when they do. Both are in the Go
  menu and in the palette.

- The keyboard stays on the page while you walk, so the next press steps again.
  **Enter** moves it into the note to type.

- Until now a pointer was the only way to reach a mark at all: a highlight's
  note could not be read, changed or taken off without one.

### Fixed: keys typed into a note no longer move the page

- Typing a note that contained an "n" turned the page underneath it, "p" turned
  it back, Home jumped to the start of the document, and the space bar scrolled
  the note away. ⌘R turned the view and ⌘C wrote the page's selected text over
  whatever you had just copied out of the note.

### Underline and strike out

- **Underline selection** and **Strike out selection**, beside Highlight. All
  three are in the Edit menu, in the palette, and on a right-click over
  selected text --- which is the shortest route to them, since none has a
  keyboard chord.

- They are written as real `/Underline` and `/StrikeOut` annotations, in red, so
  Acrobat and Preview show them as what they are. The line is drawn in
  proportion to the text it marks, so a strikeout across a heading is a line
  rather than a hairline.

- The note box names the mark it is open on, and its button says *Remove
  underline* rather than *Remove highlight*. The Edit menu's item now says
  **Remove mark**: it is chosen with the pointer somewhere else, so it cannot
  know which one you mean.

### Fixed: comments and links on a page you rotated

- **A comment or a link on a page you turned is now where you can see it.**
  Rotating a *page* --- Rotate Right on one page, as against turning the whole
  view --- moved the picture but not the click: a sticky note stayed clickable
  where it used to be, and clicking where it now is did nothing.

- **Jumping to a bookmark or a search hit on a turned page lands on the page**
  rather than partway down an edge it no longer has, which is what turning the
  whole view has always done.

- **Back, Forward and reopening a document put you back on a turned page**
  rather than at an arbitrary point down it.

- **A page you turn before you have scrolled to it is now laid out the right
  shape.** It was being measured with its turn counted twice, so it came out
  with its width and height swapped and stayed that way until the document was
  reopened.

### Take a highlight off, or write a note on it

- **Click a highlight and a box opens.** Type in it and what you wrote is the
  note on that mark --- the same field Preview and Acrobat show when you click
  an annotation, and it is in the file when you save a copy.

- **The note is saved when the box closes.** Clicking away, pressing Escape or
  using the × all keep what you typed; there is no separate save, and nothing
  is discarded for pressing the wrong key.

- **Remove highlight** is in that box, and in the Edit menu while a note is
  open. Undo puts the mark back, with its note.

- Undo also steps over a note on its own, so a note you did not mean to write
  costs one press to take back.

- Not yet: a colour, a note on a comment that was already in the file, and
  reaching a mark from the keyboard --- clicking it is the only way in.

- Fixed: **Highlight selection was greyed out in the menu bar** exactly when
  there was a selection to highlight. It had always worked from the palette
  and the right-click menu.

### Highlight what you are reading

- **Drag across a line and choose Highlight selection**, and the words are
  marked. It is in the Edit menu, in the command palette, and in the
  right-click menu over a selection --- which is where your hand already is
  after a drag.

- **The highlight is a real annotation.** Save a copy and open it in Preview,
  Acrobat, or anything else that reads PDFs: the mark is there, in the file,
  with your text still legible through it. It is not a rectangle only tpdf
  knows about.

- **Undo takes it off**, like every other edit, and redo puts back the same
  mark rather than a copy of it.

- **It stays on the words when you turn the page or rotate the view.** A mark
  is stored against the page, not against how you were holding it.

- A selection running across a page break becomes one mark per page, because
  that is what a PDF annotation can be. Undo removes them one page at a time.

- **No keyboard shortcut, on purpose.** ⌘H hides the application on macOS, and
  a chord that does nothing whenever there is no selection teaches itself
  badly. Two keystrokes in the palette, one click in the right-click menu.

- Not yet: choosing a colour, and every other kind of annotation. Highlighting
  is the first. (Typing a note and taking a mark off arrived in the same
  release --- see above.)

### Right-click a page

- **Right-clicking a page thumbnail offers what you can do to that page** —
  rotate it, move it, delete it. Until now it offered *Reload*, which is the
  web view's own menu and would have thrown away your view of the document.

- **Right-clicking the document offers what you can do with a selection** —
  copy it, search inside it, clear it.

- The menu is built from the same command list as the palette and the menu
  bar, so the three cannot disagree, and each entry shows its shortcut.
  Anything that cannot run right now is left out rather than greyed, and a
  menu with nothing in it simply does not open.

- **The web view's own menu is gone everywhere else.** Its only entry reloaded
  the application.

### A menu bar on macOS

- **Every command tpdf has is now in the menu bar**, under File, Edit, Page,
  View, Go and Find. Until now the menu held only what macOS puts there itself,
  so unless you already knew ⌘K or ⌘\ there was no way to find the page strip —
  and therefore no way to find deleting or reordering a page.

- **The menu is built from the same list the palette uses**, so the two cannot
  disagree about what a command is called, what it does, or when it is
  available. A command greyed out in the menu is one the palette would not
  offer either.

- **Choosing something that needs a value opens the palette** ready for it.
  "Extract pages…" asks for `1-3,5` rather than doing nothing, because a menu
  has nowhere to type.

- **Some items deliberately show no shortcut.** ⌘Z, ⌘C and ⌘A keep meaning what
  they mean inside the find field, and `n` and `p` keep turning pages while you
  are typing — a menu shortcut is claimed before the page ever sees the key, so
  listing those would have taken them away from the text you were editing.

### ⌘\ works on a German keyboard, for the first time

- **Fixed.** Toggling the sidebar — and with it the page strip, and with it
  deleting and reordering pages — was bound to ⌘\, and `\` needs ⌥⇧7 on a
  German keyboard, so the shortcut has never once worked on that layout. It is
  now also the key in the same *position*, which is `#` there, and the menu
  says so.

- **Back and Forward stay broken on that layout, deliberately.** The key in
  ⌘]'s position is `+` on a German keyboard, and ⌘+ already zooms in. Giving
  Forward that position would make one keypress mean two things. Both commands
  are in the Go menu, which is now their route; choosing a different chord for
  them is a decision rather than a fix.

- **The palette names the key you can see**, not the one a US keyboard would
  have. It asks macOS what your keyboard prints and labels the shortcut with
  that, so the palette and the menu say the same thing — ⌘# here.

- Windows is unchanged. A menu bar there sits inside the window, and the
  palette stays that platform's route.

### Extract pages to a new file

- **Extract pages...**, in the command palette. Type `1-3,5` --- the form every
  print dialog already uses --- and the pages you named are written to a second
  file. The open document is not changed in any way: nothing to undo, nothing
  marked unsaved.

- **The pages come out in document order, whatever order you type them in.**
  `5,1` extracts pages 1 and 5. Extract produces a subset; rearranging a
  document is what the page strip and the two move commands are for, and one
  command quietly doing both would make `5,1` mean something you could not
  predict.

- **A range that runs backwards is refused rather than corrected.** `5-3` is a
  typo, and silently reading it as `3-5` hides it --- the same reason typing 900
  in a 775-page document is refused instead of taking you to the last page. An
  overlap is not a typo, so `1-3,2` is simply three pages.

- **The suggested filename says which pages are in it**, because that is the one
  thing you cannot tell from a file called `report copy.pdf`.

- Whatever you have already done to the document comes with it: a page you
  turned is extracted turned, and a page you moved is extracted from where you
  moved it to.

## [26.8.3] - 2026-08-17

### A screen reader heard a wrapped paragraph as one word

- **Fixed.** Where a document tags a paragraph and that paragraph wraps, the space between
  its lines was dropped, so the last word of one line ran into the first of the next. Present
  since the accessibility tree learned to read tagged blocks whole, on 2026-08-01.

- Only tagged documents were affected, and only paragraphs that wrap. A page whose blocks
  tpdf infers from the geometry was never joined and is unchanged.

### Drag a thumbnail to move a page

- **Drag a page in the strip** to put it somewhere else. A line shows where it will land,
  the strip scrolls when the pointer rests against either edge, and Escape abandons the
  drag. It runs the same edit the two palette commands run, so **Undo** puts it back.

- **A press is not a drag until it has travelled 6 px.** Clicking a thumbnail still goes to
  that page and changes nothing else --- without a threshold an unsteady click rearranges
  the document, which is the failure a reader meets first.

- **A drag that ends where it began does nothing**, including one released on either side
  of the page's own row. That is not a special case in the drag; it falls out of how a drop
  position is turned into a destination.

### Move a page

- **Move page up** and **Move page down**, in the command palette. They move the page the
  reader is on one slot in the working document; **Undo** puts it back. **Save a copy**
  and **Print** both write the document in the order the reader put it in.

- **No keyboard shortcut**, for a different reason than Delete page has none: there is no
  chord left that reads as "move a *page*" rather than "move the *view*", and rearranging
  a document is work a reader does in the page strip.

- **Off either end does nothing** rather than wrapping. A page that reappeared at the
  other end of the document would look, to the reader holding the key down, exactly like
  one that had been deleted.

- **A saved or printed copy keeps its bookmarks**, which is the opposite of what happens
  when a page is deleted. A bookmark names a page *object*, and moving a page leaves every
  object exactly where it is in the file --- so the entry follows its page to wherever the
  reader put it.

- **Printing a rearranged document prints it rearranged.** It would not have: a print job
  built its pages by removing the ones nobody asked for, so a selection came out in the
  order the file had rather than the order it was asked for. That was written down as
  intended behaviour and was harmless until this release; both writers now honour the
  order.

- **A moved page keeps its size, its crop and its rotation**, including where the file
  never stated them on the page itself. A PDF lets a page inherit those from the group it
  sits in, so moving a page between groups is where they silently change --- a page that
  comes out of the wrong group is a page at the wrong size, in a document that opens and
  looks plausible.

### Delete a page, and everything that has to move with it

- **Delete page**, in the command palette. It takes the page the reader is on out of the
  working document; **Undo** puts it back where it was, with its own rotation, because
  undo is replay rather than an inverse. **Save a copy** writes the document without it.

- **No keyboard shortcut, deliberately.** Every other page operation has one. This is the
  only command in the application that removes something a reader can see, and a
  mis-pressed chord that does that silently is worse than one extra keystroke --- it is
  two of them in the palette, which is what tpdf asks of every command.

- **The last page cannot be deleted**, because a document with no pages is not a document.
  The rule is the model's and the refusal is a message, rather than a guard the frontend
  keeps its own copy of.

- **A page of the file and a page on screen are now different numbers**, and that is the
  substance of this release rather than the command. Everything that addresses a page ---
  the tile requests, the text extraction, the search, the links, the comments, the
  outline, the page strip, the accessibility tree, the remembered place --- went through
  one translation, `src/lib/pages.ts`, built from the model's own answer.

- **A link on a deleted page goes with it; a link *into* one says so.** "Points at a page
  this document does not have" was already the wording for a destination that resolves
  nowhere, and it is exactly true of a page the reader has deleted. The outline keeps its
  shape: an entry whose page has gone is still a heading with its subsections under it,
  because dropping it would take a chapter out of the table of contents when its title
  page went.

- **Printing an edited document prints the edits.** It did not, and that was live from the
  day page rotation landed: a reader who turned page 3 and pressed print got page 3 the
  way it was on disk. A print job now carries which pages are left and how each is turned,
  read from the model rather than from the frontend's cache --- and a document nobody has
  edited is still handed to the printer byte for byte rather than rewritten to produce
  itself.

- **A saved copy loses the document's bookmarks when a page is deleted**, whole rather than
  entry by entry. Their destinations name pages that are no longer in the file, and the
  alternative is worse than it sounds: the pass that removes a deleted page's references
  takes the page reference *out of* each destination array, leaving `[/XYZ 0 792 0]` ---
  not a broken destination but a malformed one. Repairing them one by one is its own piece
  of work and is written up in `docs/TRAPS.md`.

- **A page that two page numbers share cannot be half-deleted, and tpdf says so.** A
  malformed `/Kids` array can name one page object twice; removing one of its numbers
  means removing an array *entry* rather than an object, which the deletion mechanism
  cannot express --- so it is refused by name rather than silently doing nothing. Removing
  both is accepted, and there is a control for that, since a blanket refusal would have
  passed the first check while denying the case that works.

- **Found on the way: a print job could turn the wrong page.** Resolving the plan against
  the document *after* the unwanted pages had been dropped looked up page numbers that had
  been renumbered by the drop --- so a job keeping pages 1 and 4 came back with page 4 at
  its original angle. Caught by an existing check, and only because its fixture keeps the
  first and last pages of a document whose four pages carry four different rotations.

- **Thirteen new checks in a real window, and the discriminating one is about identity.**
  A page count that went down by one is equally true of a viewer that dropped the wrong
  page, or the last one, or that renumbered without moving anything --- so what is asserted
  is that the slot below the gap now holds the page that was under it, compared by its
  text. Where a document's pages read alike, the check says that and skips rather than
  passing on a comparison that cannot fail. Three of the thirteen ask the backend for
  real, from inside the running app, so the command's own round trip is covered and not
  only the viewer's response to an order.

- **A saved copy still carries a deleted page's *content*.** The page leaves the page tree
  and its stream stays in the file as an object nothing points at, because a save is a
  serialisation rather than a sanitation --- the position `docs/THREAT-MODEL.md` already
  took, now with an operation where a reader could plausibly assume otherwise. It is
  residual risk 15 there. Removing a page's content for real is what redaction means, and
  redaction is not built.

### Turn a page in the document, undo it, and save a copy

- **tpdf can now change a document.** Rotating the *view* has always been possible and
  writes nothing; this turns a page in the file, which is what fixes a scan whose pages
  came in sideways. **Rotate page clockwise** and **Rotate page anticlockwise**, `⇧⌘R` and
  `⇧⌘L`, sitting beside the view rotations on `⌘R` and `⌘L` --- the same gesture on a
  different subject, and the titles carry the distinction the shortcut cannot.

- **Undo and Redo**, `⌘Z` and `⇧⌘Z`. Both are withheld while there is nothing to act on,
  in the palette *and* on the keyboard, because those are separate routes and a guard that
  only hid the row would leave the chord reaching an empty journal. `⌘Z` deliberately
  yields to whatever text field a reader is typing in: it is *the* text-undo chord, and
  taking it from the find bar would mean correcting a typo silently undid a page rotation.

- **Save a copy...**, `⇧⌘S`, writes the working document to a new file. A copy, never the
  open one --- and the refusal is real rather than a convention: saving in place would leave
  every journalled command replaying against a baseline that is gone, which is the rebase
  `docs/PLAN.md` §5 describes and has yet to be built.

- **An encrypted document is refused rather than quietly decrypted.** `lopdf` drops
  encryption on save without a word, so a copy of a restricted document would come out with
  every restriction gone and no reader any the wiser. 3 of the 39 PDFs in a real Downloads
  folder carry `/Encrypt`, so this is a case a reader meets.

- **A file that changed on disk since it was opened is refused too**, by page count. The
  turns a reader applied name pages that may no longer be there, and writing them onto
  whatever is in those positions is the kind of plausible wrong answer that is only found
  much later.

- **The write is atomic** --- a sibling temporary file renamed into place --- so an
  interrupted save leaves either the old file or the new one, never a PDF that opens and is
  missing pages.

- **The document model built on 2026-08-12 is now wired to the viewer**, which is what all
  of the above rests on. `docmodel.rs` had 26 tests and no user; a page's turn now goes
  model -> layout -> tiles -> text layer, and every consumer is handed the *sum* of the
  view's rotation and the page's own rather than deriving its own copy of it.

- **The checks for it are built around one difficulty: telling a page turn apart from a
  view rotation.** Every statement about the page that was turned --- its shape, its
  discarded tiles, its sideways text --- is equally true of a defect that rotated the whole
  view, so the assertions that carry the weight are the negative ones: a page nobody touched
  keeps its shape, its text stays upright, and `viewer.rotation` does not move. Proved by
  mutation in both directions.

- **The page a save writes and the page on screen come from one answer**, not two. The save
  plan is read from the same `EditState` the viewer draws from, so a saved copy and a
  rendered page cannot disagree about what the reader was looking at.

- **Two of the save tests could not fail, and the mutation run said so.** One asserted the
  *effective* rotation of a page nobody turned --- which is 90 whether the page states it or
  inherits it, so writing a value onto every page changed no number it read. The other was
  named for atomicity, cited the trap about planting the intermediate in its own docstring,
  and asserted only that the destination ended up correct, which a write straight through it
  also achieves. They now assert the absence of the `/Rotate` key, each with a control, and
  the rename through a hard-linked witness that a write-through would disturb.

- **One exclusion in the command sweep was corrected on the way.** `appcommands.test.ts`
  skipped every command whose id began with `edit.`, which was right while all of them
  reached the viewer and silently stopped covering the page operations the day they landed
  under the same prefix. The three selection commands are now named in full.

### Fixed: a page listed twice in the page tree was turned twice, and could be deleted

- **Two print defects and one save defect, all from one cause**, found by reading the loop
  and then reading `lopdf`'s page walk. That walk keeps no visited set, so a `/Kids` array
  naming one page object twice makes two page numbers resolve to the same object --- and
  any code that says "for each page, do this to its object" then does it twice, because the
  second visit sees what the first did.

- **Printing a rotated view of such a document turned the shared page 180 instead of 90.**
  Live since printing landed.

- **Printing a page range could delete the page you asked for**, which is the damaging one:
  the set of pages to remove was built from the dropped page numbers without asking whether
  a kept number named the same object, so "print page 1 only" removed the object page 1
  *is* and produced a blank sheet. Its `/Count` arithmetic was wrong from the other side,
  decrementing once per object where the page tree counts page numbers.

- **Saving refuses only a genuine conflict.** A per-page plan can ask one shared object for
  two different turns, and no output satisfies that --- page 3 cannot be at 90 and page 7 at
  180 when they are one page. Turns that agree are applied once. A blanket refusal was the
  obvious move and would have rejected the case that dominates: a document nobody edited,
  where every turn is zero and there is nothing to reconcile.

- **The tests assert the precondition, not only the outcome.** A future `lopdf` that
  deduplicates its page walk would make all three guards unreachable while their outcome
  assertions kept passing, so each fixture check asserts that two page numbers really do
  resolve to one object and says that this is where such a change shows up.

### Links go where they point, and Back brings you home

- **Clicking a cross-reference did nothing.** Measured before writing any of it: 16 of the 39
  PDFs in a real Downloads folder carry link annotations, one of them 7,694 of them with 6,617
  pointing inside itself, and the EU packaging regulation 284. In documents like those the
  links *are* the navigation.

- **Pressing a link goes where it points**, with the pointer changing over one so a reader can
  tell there is something to press --- a PDF's link rectangles are usually invisible. Both
  named-destination mechanisms resolve (the PDF 1.1 `/Dests` dictionary and the 1.2 `/Names`
  tree; a reader that knows one silently fails on every link in the half of the corpus using
  the other), and each fit takes its vertical coordinate from its own position in the
  destination array.

- **Back and Forward**, `⌘[` and `⌘]`, and in the palette. They record *positions*, so an
  outline row and a search result are on the same stack --- which is what anyone who has used
  a browser expects --- and the recording happens inside the one primitive all four jumps go
  through rather than at each of them.

- **A web link is refused and says so, and deliberately does not show where it pointed.** Same
  policy `/URI`, `/Launch` and `/GoToR` have always had in the outline, through the same type,
  so one class of action does not get two answers depending on where the reader met it. A URL
  is a string a stranger wrote, and a prompt built from one is a phishing surface; whether
  tpdf should ever open external links is now an open question in `docs/PLAN.md` §10 rather
  than an unstated omission.

- **The check written to compare tpdf's two destination resolvers found a defect in the older
  one on its first run.** `FPDFDest_GetLocationInPage` is implemented over PDFium's
  `CPDF_Dest::GetXYZ` and answers only for `/XYZ`, so every `/FitH` outline entry had been
  resolving to "no coordinate" and landing the reader at the top of the page rather than at
  the heading --- since `outline.rs` was written. It scrolls to the right *page*, which is why
  it read as a slightly loose viewer; the corpus had no `/FitH` entry either, so the gap in
  the code matched a gap in the fixtures exactly. Fixed with `FPDFDest_GetView`.

- **Next link / Previous link** (`⌥⌘L`, `⇧⌥⌘L`, and in the palette), Enter to follow the one
  the keyboard is on, Escape to step off, and a ring drawn over it that follows a scroll, a
  zoom and a rotation. Until this a keyboard reader could move by page, heading and search hit
  and could not follow a cross-reference at all. The order is computed before the view's
  rotation, so "next" means next in the document rather than next down the screen; it starts
  from the viewport rather than the top of the file; and it reports running out rather than
  wrapping.

- **A screen reader is told a link is a link.** The half a sighted reader never sees, and the
  hardest to notice missing: the words are announced either way, so a table of contents read as
  prose is indistinguishable from one read correctly unless somebody is listening. Each reading
  line is split into the characters a link covers and the ones it does not, and the first is
  handed over as a `role="link"` element --- never an `<a>`, because the gate that forbids
  creating any URL-bearing element is what keeps document text from becoming a navigation. A
  link tpdf declines to follow is announced as unavailable rather than left silently inert.

- **Not here:** creating or editing a link, and opening one in a browser.

### Jumping to a heading at the top of a page no longer shows the page before it

- **Every `/Fit` destination landed on the wrong page**, and so did every outline entry within
  6 points of a page top, and every destination at all on a rotated view. Jumping deliberately
  leaves a little air above a heading so it does not read as cut off; when the heading *is* the
  top of the page there is nothing above it, and what the air revealed was the previous page.
  The sidebar then highlighted whichever entry belonged to *that* page. Shipped in `26.8.0`
  and fixed here.

- **The tolerance written to prevent exactly this could not reach the case.** It compensates
  for the margin when the entry and the reader are on the same page, and the entry that gets
  dropped is dropped for being on a *later* page than the reader — before the tolerance is
  consulted at all. Correct, asserted, and structurally unable to see the failure it names.

- **Only `links.pdf` could catch it**, and the two corpora built to exercise outlines both
  passed: it is the only fixture in the tree with a `/Fit` entry, so it is the only one whose
  outline names a destination without a coordinate. The corpus built for links found it
  because its outline exists to be compared against the links.

- **Back and Forward replayed a recorded position as though it were a destination**, so the
  same 6 points came off it again on every jump and a round trip drifted a page each time —
  further the more the reader travelled, which reads as "Back is unreliable" rather than as an
  off-by-one. Found on a 775-page document, where Back from the last page reported page 774 of
  775 and Forward returned to 773. Fixed before either command shipped.

### The window harness owns its list of corpora

- **The list lived in whatever shell loop somebody typed**, so a fixture built for a probe was
  swept as a corpus and produced eight red checks, none of them a defect — against a paragraph
  in `BUILD.md` that already said that fixture is separate *because* it reddens two of these
  checks.

- `scripts/viewer_sweep.py` is the list and is a gate. Every `testdata/*.pdf` is either a
  window corpus with a stated purpose or excluded with a stated reason; a fixture matching
  neither is an error rather than an omission, and a corpus named but absent from disk is an
  error too. It also prints the table `BUILD.md` carries, so the numbers there stop being
  transcribed by hand.

- **It asserts what the totals could not say:** every corpus reports the same check names,
  diffed as sets. A check that stops being printed and a check that starts skipping are
  identical in a count, and the count is what a person compares.

- **Its first version got that wrong in the way it was written to prevent.** It recovered the
  names by parsing the printed column, which is padded to 46 characters and does not truncate
  — so every longer name ran into its detail, 14 of 189 lines went unmatched, and the run
  reported two corpora agreeing about a set that was wrong on both sides. A check run now
  prints its own names as a `CHECK-NAMES-JSON` line, the sweep reads that and refuses a bundle
  that does not emit one, and it asserts the roll and the summary agree about how many checks
  there were — the comparison that would have caught it, between two numbers already printed
  on adjacent lines.

- **Two checks were asserting a precondition they had never established.** Following a link
  and activating a thumbnail both require the destination page to be able to reach the top of
  the viewport, which the last page of a short document never can; both now measure it and
  skip with the reason printed. The guard on a third asked how long the document was when the
  question was how far the jump had gone — the same quantity only from a standing start, and
  the check does not start from one.

### A check name that cannot be aimed at is now caught when it is added

- **The rule was written down and enforced by nothing.** A mutation is credited to the check it
  names by prefix, so a name that is a prefix of another can never be the sole target of one —
  and the harness only says so when somebody writes that mutation, possibly months later. It
  had been broken once already.

- All three families that are matched this way were measured and all three are clean: 189
  viewer check names, 75 and 30 from the search probe, 11 from the structure probe. The check
  runs on every sweep and every probe run now, and it names the shadowed name and what shadows
  it rather than reporting a count.

- **The first measurement of it covered 80% and 50% of the names.** Probe output pads to 52
  characters and does not truncate, so longer names run into their detail and a split on runs
  of spaces drops them silently — 15 of 75 in one probe, 15 of 30 in another. Both probes emit
  their names as a `CHECK-NAMES-JSON` line now, the same fix as the viewer harness.

### The crop-box work is now covered where it was only claimed

- **The probe that covers the text half exited 1 on the fixture it was prescribed for.**
  `text.rs`'s crop-box fix needs a live PDFium page, so `BUILD.md` points at `text-probe`
  rather than at `cargo test` — and both link fixtures are 36 rows of even text, which cannot
  detect a y-flip. Two of the probe's four controls landed on ink at 68–87% and were reported
  as failures, so the documented command was red for having chosen the wrong document. They
  are skips now, excluded from the exit code, with a note saying what a green run on such a
  page still proves: placement, not orientation. `text-base14.pdf` reports `[OK]` on all four
  unchanged, which is the control over the change.

- **And it does cover the fix, measured rather than assumed.** Removing the origin shift takes
  `links-cropped.pdf` from 96.4% to 74.8% — red against the 95% threshold — while the fixture
  with no crop box stays at 100%. Narrower than it sounds: a 50 pt inset moves each box by less
  than a line's height, so it is the threshold that catches it rather than a collapse to zero.

- **`annots.rs` had one crop-box test where `links.rs` had three**, for a rule the two modules
  implement separately on purpose. Its intersection clamp — the one that stops an oversized
  `/CropBox` scaling every comment rectangle against a page the renderer never uses — was
  reachable by no test at all. Added, with the two mutations its twin already had.

- **Two guards in `origin_pt` could not be tested at all**, being inline with an FFI call that
  needs a document and a loaded PDFium: normalising a crop box written corner-first, and
  refusing a non-finite one. They now live in a free function over four floats, with three
  tests and three mutations — including an ordinary box asserted inside the non-finite test,
  since an unconditional refusal would satisfy every assertion about refusing.

### Pages displayed from a `/CropBox` put text and links in the right place

- **A page has two boxes and tpdf read the wrong one.** `/MediaBox` is the sheet, `/CropBox` is
  the part displayed — and PDFium lays out, renders and measures the crop box, so the viewer's
  coordinates start at *that* corner. The link and comment scans read `/MediaBox`; text
  extraction was worse, mixing PDFium's cropped size with character boxes in the page's own
  space. Every rectangle and every character was offset by the difference, silently, on a page
  that looks entirely normal.

- **Measured with a control:** a page cropped to `[50 50 545 742]` landed its character boxes on
  ink **0%** of the time; the same page uncropped landed 100%. Both are 100% now. What
  discriminates is the crop box's *origin*, not its size — one that merely shrinks the page from
  (0, 0) was always handled correctly.

- **It is live.** One of the 43 PDFs on a real machine carries an off-origin crop box on all ten
  pages, offsetting every selection by about two thirds of a line. Its 7.8 points are small
  enough that the coarse check still passed on it, which is why the committed fixture insets by
  50: a fixture has to be able to fail.

### The differential that found one bug now runs on every document

- **It needed a manifest, so it ran on exactly one fixture.** `links-probe --mode agree`
  compares tpdf's two destination resolvers and had already found a real defect, but it asserted
  both sides against a file stating what the destinations should be — and only `links.pdf` has
  one. It now also resolves the *same outline* both ways and compares the two lists, which needs
  nothing stated, so any document with an outline is a test. Six assertions on one fixture became
  421 outline entries across 44 real files.

- **That turned a suspicion into a measurement.** PDFium's `FPDFBookmark_GetDest` returns null
  both for an entry with no `/Dest` and for one whose `/Dest` resolves nowhere, so the outline
  cannot tell a heading from a damaged link. Exactly one entry in 421 shows it. Not fixed —
  distinguishing them costs a second parse for one word on a row that is unreachable either way
  — but it is written down where the answer is produced, and the check allows that one pair by
  name and fails on any other difference.

### A locked document is told apart from a broken one

- **"Could not parse this as a PDF" was a wrong diagnosis, not a vague one.** Both open paths
  said the same thing whatever had happened, so a document that is entirely well formed and
  merely password-protected was announced as damaged --- and a reader who is told that goes
  looking for another copy of a file that was fine. 3 of the 39 PDFs in a real Downloads folder
  carry `/Encrypt`, so it is not a corner.

- PDFium keeps the reason and it costs one call. A file that needs a password now says so, an
  unsupported security scheme says that, and a file that genuinely is not a PDF says that ---
  four sentences chosen in our code, so no error path can carry a string the document wrote.
  tpdf still cannot *ask* for a password; the message says so rather than implying the file is
  broken.

### An empty scan no longer looks like an empty document

- **"This document has no comments" and "nothing could read this document" were the same
  answer.** Both whole-document scans walk the pages `lopdf` finds and bound themselves against
  a page count that came from PDFium; when the two disagree the loop runs zero times, the list
  comes back empty and no bound has tripped. Both now report how many pages they could not
  account for, and the sidebar says so.

- **No fixture on disk makes it fire**, which is stated rather than left to be found: swept
  across every fixture, the two parsers agree about page count on every document PDFium will
  open. The guard is defensive rather than demonstrated and its tests are synthetic, which is
  what `encoding.rs` already did for the same distinction.

- **A justification in the code was half wrong and is corrected.** `encoding.rs` and
  `docs/PLAN.md` both said `lopdf` reports zero pages for `incr-encrypted-pw.pdf` "which PDFium
  paginates normally". The first half is true; PDFium refuses to open that file at all, so the
  two parsers never both see it and it demonstrated nothing. The design it justified is
  unchanged.

### Comments can be read

- **A reviewed document opened as a document with coloured boxes in it.** PDFium already
  paints the marks --- `FPDF_ANNOT` has been on since the first render, and it generates an
  appearance stream where a file supplies none, measured at 637 of the 756 pixels inside a
  sticky note's own rectangle. What no reader could reach was the *text*: the author, the
  date, the body and the reply.

- **A fourth sidebar tab lists every comment**, threads replies under what they answer, and
  says so when the scan had to cut something. **Pressing a mark on the page opens its note**,
  anchored to the mark and following it through a scroll, a zoom and a rotation; Escape or a
  press elsewhere closes it. Picking a row opens the same note and moves the keyboard into it.

- **The scan is `lopdf` at document level rather than PDFium per page**, which is a
  measurement rather than a preference: PDFium's annotation API needs a loaded page, and
  `FPDF_LoadPage` costs up to 44 ms on a complex one, so listing a document's comments that
  way is a page load per page. It also does not expose `/IRT` at all, so a reply arrives there
  as an unrelated second note by another author.

- **A comment is the largest body of attacker-chosen prose tpdf has ever shown.** The kind is
  an enum of ours rather than the document's `/Subtype`, a date is rebuilt from parsed digits
  rather than passed through, and `no_comment_field_may_carry_a_url` destructures every field
  exhaustively --- so `docs/THREAT-MODEL.md` T8 still rests on a property of the code rather
  than on there being little text to show.

- **`testdata/comments.pdf` is the corpus**, four pages built to be awkward: three text-string
  encodings, a date that is not a date, a 60,000-character body, replies that point in a
  circle, a rectangle written backwards, one at 1e10, a hidden annotation, a `/Link` and a
  `/Widget` that are not comments, an `/Annots` array that is an indirect reference, and 1,200
  notes on one page --- with `comments-rotated.pdf` beside it for the page carrying
  `/Rotate 90`. `examples/comments-probe` reads both, 26/26 and 5/5, with a `--mode clean`
  control on a document that has none.

- **It found five defects in the fixture and the harnesses, and none in the product**, which
  is what a corpus written before the code it checks is for. A square rectangle maps to itself
  under a quarter turn, so it could not tell a correct rotation from a missing one. Three
  malformed `/Annots` entries written after the 1,200 notes were never reached. A sidecar named
  `comments-manifest.json` was bound to `TPDF_READING_MANIFEST` by its suffix alone and ended a
  window-harness run sixteen checks in. A `/Rotate 90` page inside an upright document makes it
  mixed-size and turned two rotation checks red against a viewer behaving as designed. And the
  page text had one-character words where the harness double-clicks. All five are in
  `docs/TRAPS.md`, and two of them cost a bisect to attribute correctly.

- **Every check ran, on all twelve corpora.** The window harness reports **171 names** on each
  --- 163 plus the eight added here --- with zero failures, and `comments.pdf` is the corpus
  where all eight run rather than skip. `BUILD.md` carries the measured table; the split moved
  by exactly one running and seven skipping on every other fixture, which is what says the new
  checks skip for a reason rather than vanishing.

- **Ten Rust mutations and eleven front-end ones** judge the new tests, and two of them
  survived the first run for the same reason: they were aimed at a *route into* a rule rather
  than at the rule. One was fixed by aiming inside the function; the other by writing the test
  that was actually missing --- that a body reaches the paragraph-keeping flattener at all,
  which no test had ever asserted.

### A flake removed from the release gate

- **Not a product change.** `viewer_check.py`'s broken-pattern search check waited on a
  condition that two different clocks could satisfy, and failed roughly twice in seven runs
  — in the harness the release checklist requires, where a flake is indistinguishable from a
  regression until somebody re-runs it.

- **The guard was satisfied by the event it existed to exclude.** `viewer.searching` is a
  live read; `seen` is a mirror filled a frame later. The delivery counter added to bridge
  them counted *any* status, and a scan emits one when it **starts** — so on a run where a
  frame lands mid-scan, the counter was satisfied by the start status, the live flag was
  already false, and the mirror still held `problem: ""`. Now both halves read the mirror,
  so only a status taken after the scan stopped will do.

- **The first attempt's reasoning was wrong, and a control is what said so.** It shipped
  with a check asserting the start status always arrives first; that check went red
  immediately, because the start and the completion normally land before the same frame and
  the mirror sees only the final state. The control was then deleted rather than kept: it
  asserted the race rather than the behaviour, so it could only pass on runs where the bug
  would not have fired. `docs/TRAPS.md` carries all three parts.

- **4/4 clean runs against 2 failures in the preceding 7**, name invariant unchanged at 163.

### The first layer of the editing foundation

- **Nothing user-facing yet.** `src-tauri/src/docmodel.rs` is the working document, the
  journal and undo/redo, for pages — the layer every page operation and every annotation in
  Phase 2 will address. Nothing is wired to the viewer, and nothing saves.

- **Commands name pages, never positions.** `Move { page, after }` puts a page behind a
  neighbouring *id*, or at the front. A position shifts under other commands, so a journal
  built from positions replays differently depending on what preceded it — which is the
  defect `docs/PLAN.md` §5 was rewritten to remove, and the type is what removes it.

- **Undo is replay from a snapshot, not an inverse per command.** An inverse is a second
  implementation that has to agree with the first, and the cases where they disagree are the
  ones undo is for. Under replay, undoing a deletion restores the page at its old position
  with its own rotation and crop for free; snapshots every 32 commands keep the cost bounded.

- **A refusal is named, never silent.** A command naming a deleted page and one naming a
  page that never existed are two different refusals, because a tombstone exists exactly to
  tell them apart. A refusal leaves the document and the journal untouched, and a refusal
  during *replay* panics — every entry was accepted against the state its predecessors
  produced, so one being refused means the model is broken and the rendered document would
  no longer be the one the journal describes.

- **26 tests, and 11 mutations judging them** — all 11 caught by the test named for each,
  including the two aimed at claims that were only made in a comment: the statement ordering
  inside a move, and the discard of snapshots the redo tail invalidated.

- **One of those eleven found a test that could not see what it looked like it covered.** The
  general property test walks a mixed journal and every prefix of it, but applies eight
  commands where the snapshot interval is 32, so it never has a snapshot to rebuild from. It
  was caught by a different test and the harness said so, because it compares which test went
  red rather than counting. Both that and a refusal that is not equal to itself when it
  carries a `NaN` are in `docs/TRAPS.md`.

## [26.8.2] - 2026-08-12

### tpdf can update itself

- **The problem it solves was demonstrated rather than imagined.** A defect was found,
  fixed, released and published, and the machine that reported it went on running a
  nine-day-old build, because nothing told it otherwise. Every release until now cost a
  manual download, which is also a quiet argument for batching fixes; from here, cutting a
  release for one fix is free.

- **Check on launch, decide for yourself.** One check per launch, and it is the only network
  request tpdf makes. If there is something newer the toolbar offers it; nothing is
  downloaded or installed until you click. A failed check says nothing and is not retried —
  a viewer that keeps dialling out after being told no is the behaviour this avoids. Two
  palette commands: *Check for updates*, always available, and *Install update and restart*,
  offered only while there is one to apply.

- **Not silent, deliberately.** Swapping the binary under somebody with a document open is
  rude, and for a reader an update is never urgent.

- **Every payload is signed, and verified before it is unpacked.** The signature is checked
  against a public key compiled into the binary, so the archive parsers this adds — `zip`
  and `tar`, which run in the app process — never see bytes an attacker chose. The endpoint
  is one pinned HTTPS URL that resolves only to a *published* release, so a draft offers
  nothing and publishing by hand stays the act that ships an update. `docs/THREAT-MODEL.md`
  §T9 is new and carries the four residual risks, including the one that does not go away:
  the release workflow can sign anything every installed copy will accept.

- **The launch check sits after every spike and check entry point returns**, which is what
  keeps all seventeen harnesses in this repository entirely offline.

- **48 new crates** (325 → 373), all permissive — the licence sweep runs over the whole tree
  and is a gate, not a glance at a README.

- **Costs found and fixed on the way.** `createUpdaterArtifacts` in the main config makes
  *every* build demand the private signing key, which would have put the one secret capable
  of forging an update onto every machine that builds; it lives in a CI-only overlay
  instead. Passing that overlay has two wrong forms that fail only on a tag push, and both
  were caught by control rather than by reading — see `docs/TRAPS.md`.

- **17 tests over the state machine, 7 mutations each killing the test named for it**, plus
  6 over the two new commands. Both are classified `undriven` in the window harness with
  reasons, so the completeness check stays honest: 32 commands registered, 26 driven, 6 not.
  `viewer_check.py` is 143/143 with 20 not applicable, 163 names. What none of that covers
  is a real endpoint and a real signature; `BUILD.md` step 12 schedules that by hand,
  because it cannot exist until two signed releases do.

## [26.8.1] - 2026-08-12

### A document rewritten on disk no longer fails silently

- **A truncated file killed a worker per tile, and said nothing.** The document is a
  `MAP_SHARED` mapping of the real file, which does not pin its length --- so a process that
  shortens a PDF while it is open leaves every page past the new end unbacked, and reading
  there is a `SIGBUS` rather than an error. The crash path then replaced the dead worker with
  one built from the same bytes, which faulted in the same place. A reader scrolling into the
  missing tail paid two process spawns and two faults *per tile*, for a region that could
  never render, and saw a blank area with nothing to explain it.

  Now diagnosed and latched: the pool compares the mapping's length against the file's, and
  once bytes are missing every request is refused without spawning anything. Measured at
  0.01 ms worst of twenty against 1.3 ms for the request that diagnosed it --- a spawn alone
  is ~12 ms, so the number is the evidence that none is happening.

- **The reader is told.** The refusal carries a sentence rather than a pipe error, the tile
  protocol answers **410** instead of 400, and the scroller stops asking rather than backing
  off --- a retry of a vanished document is another refusal, forever. What is already painted
  stays painted, because those tiles are the last true picture of that document there will
  be. 410 is the whole cross-language signal; the frontend never matches the message text.

- **The check is on the descriptor, not the path**, which is what makes it usable at all.
  Writing a temporary and renaming it over the original is how nearly everything replaces a
  file, and it leaves the mapping healthy --- the old inode is still there. A path-based check
  would condemn a perfectly good document every time the reader's own editor saved.

- **`rewrite-probe` is the evidence**, and it covers the two benign cases as well as the fatal
  one: a renamed-over file leaves the open document intact indefinitely, an in-place overwrite
  fails closed, and only a truncation faults. Eighteen checks, exit 0.

- Not fixed, because it cannot be: the fault itself. The file can be shortened between any
  check and the read that faults on it. What is guaranteed is fail-stop --- the page never
  renders --- and that the reader finds out.

- **An in-place rewrite with *valid* bytes is served silently, and this fix cannot see it.**
  Measured rather than left as a worry: two documents of identical structure and identical
  length, differing by one character per page, and writing the second over the first under an
  open document gives page 2 from revision A and page 190 from revision B, with no error. The
  length is unchanged, so the guard above has nothing to compare. Detecting it means comparing
  `mtime` on the mapped descriptor on some schedule, which is a watcher and a separate
  decision. The probe reports it and says on its own output that nothing detects it.

### Reload from disk

- **A `file.reload` command**, in the palette. The message above tells a reader to open the
  file again, and until now the only way to do that was ⌘O and re-picking it. It is useful
  either way, since a document rewritten in the background is not picked up at all.

- **No keyboard binding, deliberately.** ⌘R is the rotate chord, and moving a binding a reader
  already has is worse than a command reaching the palette alone --- which is what this
  application is built around. "Show outline" and "Show page thumbnails" have none either.

- **It keeps your place**, and that needed care: `session` is the snapshot loaded at launch and
  is never updated, because places are written over IPC. Reopening the current path would
  therefore restore where the reader was when the *application* started. The place is captured
  at the moment of reload and handed to the open, then clamped to the document's new length ---
  so reloading a file that got shorter lands somewhere that exists.

### What the first Windows run of the week's macOS work found

- **A belief about Windows became a measurement.** `Shm::backing_len` has recorded, as a
  belief nothing had ever exercised, that Windows holds a mapped file against truncation.
  It does: `rewrite-probe` gets `ERROR_USER_MAPPED_FILE` (os error 1224), so the fault the
  truncation scenario exists to provoke is unreachable here and the scenario reports it as
  the finding it is. The other three scenarios reproduce macOS exactly --- including the
  quiet one, where a valid in-place rewrite leaves page 2 on revision A and page 190 on
  revision B with no error anywhere.

- **The same refusal was fatal in the sibling scenario.** The service-level truncation
  discarded the OS error, recorded `"failed"`, and `return`ed --- which dropped three check
  names from the run rather than skipping them, so the Windows name set was 20 against
  macOS's 22 and the summary read like an ordinary single failure. Both are fixed: the set
  is 23 on both platforms and only the verdicts differ. The `return` had also been skipping
  the service's `close`, which the comment below it exists to prevent.

- **`file.reload` was never classified in `viewercheck.ts`**, so its completeness check ---
  every registered command is either driven or listed with the reason it cannot be --- was
  red on every corpus. The decision to cover the command by unit tests instead was right,
  and the reasoning that it would not move the 163-name invariant was also right; it is
  simply a different claim from "the harness stays green". Recorded in the `undriven` table
  now, which is where that decision belongs. The comment above it said "the two" while
  listing three, and is count-free now.

- **Six corpora green at 163 check names on Windows**, name sets diffed pairwise and
  byte-identical, with every ran/skipped split matching `BUILD.md`'s table exactly:
  `tagged` 132/31, `outline-hostile` 148/15, `columns` 137/26, `mixed` 138/25, `encodings`
  130/33, `multilingual` 130/33. Containment holds throughout --- 45--49 modules at peak
  across up to 691 samples, none of them the PDF parser.

- **Neither defect was reachable from CI**, and that is the durable part. `viewer_check.py`
  needs a real window, so both sat under a green 13/13 gate run and two green CI jobs.

### Two commands did their work on the wrong thread

- **`print_document` parsed the whole document on a runtime worker.** The previous
  release left this open by name. Being `async` put it off the main thread, which was the
  whole of the original argument and is half of one: it is `await` that yields a thread,
  not `async`, so a synchronous parse inside an `async fn` holds a runtime worker for its
  duration. It runs on the blocking pool now. This is deliberately *not* the choice the
  seven render-service bridges made --- they wait for work happening on the render thread,
  where a bigger pool only raises the bound; here the work is in the function.

- **`session_remember` wrote the session file on the thread the webview draws on.** A
  synchronous `#[tauri::command]` runs on the thread the IPC arrives on --- read from the
  macro rather than assumed --- and this one is on the scroll path, throttled to one write
  per second. Measured release-profile on a full 32-place session, 2,000 cycles: mean
  **0.911 ms**, p99 **1.381 ms**, max **13.870 ms**. The mean fits inside a 120 Hz frame
  and the maximum does not, so it was an occasional visible hitch rather than a steady
  cost. Both writers are on the blocking pool now.

- **The lock is the part that is easy to miss.** Both writers load, edit and save, which
  is only safe together, and that was true by accident: the main thread serialized them.
  Moving to the pool removes that, and `session_set_invert_pages` bypasses the frontend's
  write chain, so it really can overlap a place write. `SESSION_WRITE` is what the main
  thread used to be. Proved by a race test --- sixteen paths from two threads, twenty
  rounds, all of which must survive --- that fails on the first round with the guard
  removed.

- **`session_load` stays synchronous**, and already carried the comment saying why: it is
  read once during a ~50 ms startup budget, where a few kilobytes cost less than the round
  trip needed to hand it back later.

### The outline's arrow keys acted on the wrong row

- **ArrowLeft collapsed nothing, one run in three.** `sidebar.ts` looked the row up by
  `focused` --- a mirror of the DOM's focus kept by a `focusin` listener --- and `focusin`
  is not delivered when the document lacks system focus. The mirror then named a row the key
  had never reached, so ArrowLeft fell through to "step out to the parent" or to nothing at
  all. Real, not only a harness artifact: a reader whose window regained focus the wrong way
  gets arrow keys that operate somewhere else.

- **The row is resolved from `event.target` now**, once at the top of the handler, so
  `move`, `toParent` and `activate` cannot disagree about which row the key reached. This is
  the fix already applied to Enter in August; the arrows were recorded as fixed at the same
  time and never were --- what they had was the `focusin` listener, which the same paragraph
  had just finished explaining is not sufficient. `docs/TRAPS.md` carries both halves, and
  the earlier entry's claim is corrected rather than left to contradict the new one.

- **Covered by unit tests that were proved to discriminate.** The first pair passed against
  the unfixed code: `setOutline` leaves the mirror on the first row, so dispatching to that
  row cannot tell the mirror from the target. Only the mutation said so. They move the mirror
  off the target first, and both go red under the old lookup while the two Enter tests stay
  green.

- **Six runs of `outline-simple` on Windows**, three before the fix and three after; the
  three after are green. `BUILD.md`'s row for it is widened to `147--149 / 14--16`, the total
  being 163 in every one.

### The status label no longer shoves the toolbar it sits beside

- **Reported as the find toolbar being "briefly overlaid/replaced" during a fast scroll.**
  Nothing overlaid anything, and the toolbar's own markup was innocent. The header is a
  single flex row and the degraded-state label was second-to-last in it, so every dip in
  coverage put an element into the row and displaced everything to its left --- and because
  flex items shrink by default and `.find` carried a `width` but no `flex`, the search field
  was squeezed at the same moment. At scroll cadence that reads as a bar replacing the
  toolbar.

- **Two defects, and fixing either alone leaves a visible fault.** The label moved beside the
  document title, where it grows into slack the spacer already held, so its arrival moves
  nothing a reader is aiming at; the find field and the toggles are pinned with `flex: none`,
  leaving the title as the only item allowed to give way. Separately, `src/lib/degraded.ts`
  holds a transient state back until it has lasted 300 ms, so a scroll that resolves within a
  few frames says nothing at all.

- **Nothing about what counts as degraded changed**, and the delay is deliberately not
  applied to a failure: `failed > 0` is the one state waiting does not fix, and it can arrive
  with the frame loop already quiescent, so delaying it would suppress it rather than
  postpone it.

- **The judgement was already half-made one level down.** The coverage thresholds are `0.999`
  rather than `1` because a tile boundary landing a rounding step inside the viewport leaves
  a fraction of a percent uncovered, and that comment says in its own words that a status
  line which flickers on that is worse than none. The threshold answers whether a dip is
  real; nothing answered whether it is worth saying.

- **12 tests, each shown to go red**, by mutating the ordering that puts a failure ahead of
  slowness, the urgent early return, an episode clock that restarts on the wording rather
  than the episode, one that never restarts between episodes, and the `0.999` threshold. All
  five killed the test named for them. `viewer_check.py` on `text-heavy.pdf` reports
  `143/143, 20 not applicable` against a stashed baseline of the same 163 names.

## [26.8.0] - 2026-08-03

### The first release, and what cutting it found

- **This is the first tag.** Everything below this section is what the tree accumulated
  before anything shipped, kept rather than collapsed into one "initial release" line. What
  ships is a reader --- sandboxed parsing on both platforms, search, selection, outline,
  thumbnails, session restore and printing --- and nothing in it edits a document. The
  generated release body says so in its first two paragraphs now, because a README's "Not
  built yet" list sits a scroll below the fold and that is not where someone downloading a
  binary reads.

- **Four documents were wrong, and the code was not.** Running `BUILD.md`'s own checklist
  end to end found: its mutation-harness line quoting 23, 85 and 15 against an actual 36, 98
  and 31, and naming three runners where there are six; a dated `8/8` gate count read as
  current when there are twelve; `PLAN.md` asserting in the present tense that Windows is
  "entirely unverified", that "every PDF is still parsed in the app process", and that
  containment is "still not wired in" so "Windows fails open today" --- all three closed
  between 2026-07-29 and 2026-07-30, and the last of them contradicted by
  `Backend::default_here` returning `Backend::Worker` on both platforms. The stale claims are
  marked in place rather than deleted, since each names an obstacle the fix had to remove.
  The counts are gone rather than corrected: `--list` is the authority, and a tally in the
  document whose job is to schedule the run is exactly the thing nothing can turn red.

- **`docs/THREAT-MODEL.md` needed no correction, which is the first time.** §3's boundary
  table, §5's two SBPL profiles, §6's Windows row-by-row and §8's re-verify commands were
  checked against the tree: `SANDBOX_PROFILE` still lives where §5 points, both profiles
  match their source constants modulo line wrapping, `WORKER_MEMORY_CAP` is 1 GiB behind
  `JOB_OBJECT_LIMIT_PROCESS_MEMORY`, `JOB_OBJECT_LIMIT_JOB_TIME` is still set nowhere as §6
  says, `sweep::MAX_NESTING` is 256, and `Worker::footprint` still has no caller outside
  `pool-bench` --- which is residual risk 2 and is stated as such. Three consecutive rounds
  each found a claim that had become a description of an earlier phase; this one did not.

- **A mutation was refused rather than surviving, and the difference matters.**
  `mutate_frontend.py`'s anchor for *drop the characters PDFium placed nowhere* no longer
  matched `reading.ts`: the sliver conjunction landed the day before and rewrote that line, so
  the mutation was aimed at nothing. The harness said so instead of reporting a survivor,
  which is the whole point of validating anchors --- a survivor reads as a gap in the tests.
  Re-aimed, and 36 / 98 / 31 caught across the three harnesses with none surviving.

- **The `text-heavy` row was corrected into a point estimate of something that varies.**
  Yesterday's commit replaced a derived `142 / 21` with a measured `143 / 20`; two runs this
  evening against the release bundle both report `142 / 21`. Nothing regressed --- the check
  that moves is the thumbnail-withdrawal race that `BUILD.md` already documents in prose, and
  this fixture is one of the two that note names as landing on both sides. Both rows are
  ranges now. Fixing the arithmetic reintroduced the shape of the original error, which is
  worth stating: a number measured once is still a point estimate of a quantity that varies.

- **The first tag went red on both runners, and the code was fine.** `release.yml`'s `gates`
  job was written from `ci.yml` and the copy dropped one step --- the one generating the
  fixtures a hosted runner can build --- so `print.rs`'s
  `a_third_parser_checks_a_job_built_from_a_document_we_did_not_write`, which needs
  `rotated.pdf`, failed on macOS and Windows while passing in CI and locally. The release
  gate was weaker than the gate it exists to satisfy: the rule this repository already states
  about hand-copied commands losing a `--locked`, with an entire step lost instead of a flag,
  and in the one place where a weaker gate is worst. The `release` job was correctly skipped,
  so nothing was published. Fixed twice over, and only the second one lasts: the list of
  runner-generatable fixtures is `scripts/ci_fixtures.py` now, so both workflows call one
  line, and a thirteenth gate --- `workflows` --- compares the two `gates` jobs step for step,
  every `uses:` with its pin and every `run:` body, in order. Names are not compared and a
  control proves it. Worth noting what actually caught the gap: `print.rs`'s
  `assert!(examined > 0)`, which had already caught the same thing on CI's very first run. A
  guard that turns "no fixtures" into a failure rather than a page of skips has now paid for
  itself twice.

- **Then the macOS leg ran for the first time and died on `***: no identity found`.** With the
  gates fixed, the second tag got both gate legs green, built and published the **Windows**
  leg successfully, and failed on *"Sign the vendored PDFium"* --- the one genuinely new part
  of the Apple half, since neither sibling repository ships a native library. Nothing had
  imported the certificate into a keychain yet: the step's own comment said the Tauri CLI
  imports it and no manual keychain step is needed, which is true of the CLI and false of the
  job, because the CLI's import happens inside `tauri-action`, two steps later. The dylib has
  to be signed *before* the bundler copies it, so the step's prerequisite was being created
  after it ran. The certificate is now imported into a keychain in `RUNNER_TEMP` during the
  preparation step, and the `***` in that error is GitHub masking the signing identity --- the
  one thing a reader needs is the thing the masking removes.

- **That fix was rehearsed locally against a synthetic certificate, and the rehearsal's
  verdict was wrong in the dangerous direction.** `security find-identity -v -p codesigning`
  reported zero identities, which reads as the fix not working; `-v` means *valid identities
  only*, and an `openssl` self-signed certificate is `CSSMERR_TP_NOT_TRUSTED` by construction,
  so it can never be valid whatever the import did. Loosening the check to make the rehearsal
  pass would have removed the one thing that catches a `.p12` shipped without its intermediate
  --- which otherwise surfaces forty minutes later at `notarytool`. The flag stays, the comment
  says why not to drop it, and the failure path now prints the unfiltered listing, since one
  identity present-but-untrusted and zero identities want completely different fixes.

- **The third rehearsal got the whole Apple path through, and then its verifier exited 127.**
  The dylib signed, `tauri-action` built and published, the `.app` notarized `Accepted`, the
  DMG notarized and stapled --- and *"Verify the macOS build is signed, notarized and
  stapled"* died on `mapfile`, a bash 4 builtin, because macOS ships bash 3.2 as `/bin/bash`
  and that is what a `run:` block gets. Under `set -euo pipefail` it aborted with no message
  of its own, which is the worst shape available here: the release was fine and the checker
  could not run. The tell was that none of the step's own guards printed an `::error::` line
  --- **when a step with explicit error messages fails without printing one, suspect the
  script before the subject.** Replaced with the portable `while IFS= read -r` form, and both
  halves reproduced locally under `/bin/bash`, which on this machine is the same 3.2.57 the
  runner has: `mapfile` exits 127 there, the new form returns the one dylib. Every shell body
  in both workflows now passes `/bin/bash -n` rather than whatever bash is on `PATH`.

- **The artifact was verified from outside the workflow, which is why that failure cost
  nothing.** rc3's DMG was downloaded from its draft and checked on a machine that had not
  built it: `spctl` accepts it as `source=Notarized Developer ID` with
  `origin=Developer ID Application: Timo Stein (NVX72G8SJ8)`, the ticket staples on both the
  DMG and the `.app`, both the app and the bundled `libpdfium.dylib` chain **Developer ID
  Application -> Developer ID Certification Authority -> Apple Root CA** with
  `flags=0x10000(runtime)`, and the payload holds exactly one engine and
  `THIRD-PARTY-NOTICES.md`. So signing a bundled native library for notarization --- the one
  part of this workflow with no precedent in any sibling repository --- works. `rc4` then
  went green with the verification step included.

- **Four rehearsal tags, each failing one step later than the last**, which is the shape of
  running a sequence end to end for the first time rather than bad luck: **the last step of a
  pipeline is its least-tested code, because everything before it has to succeed before it
  runs even once.** `BUILD.md`'s release checklist ended at the commit and never said to push
  a tag; the rehearsal habit is step 10 now, including the detail that deleting a tag does
  **not** delete its draft release.

- **The bundle was checked with the development library moved aside**, and then with the
  bundled one moved aside as well: `text-heavy` 142/142 and `vector-heavy` 91/91 with the
  documented skips, then a deliberate failure naming the paths it tried. A pass alone cannot
  say which candidate resolved; the failure does.

- **Three frontend majors taken**: vite 6 → 8, `@sveltejs/vite-plugin-svelte` 5 → 7 and
  TypeScript 5 → 6. TypeScript 7 is blocked and stays blocked --- `svelte-check@4` declares
  `typescript: ^5 || ^6`. Taking the first two together is forced, since the plugin peers on
  `vite: ^8`; `vitest@4.1.10` already declared `^6 || ^7 || ^8`, so nothing there had to move.
  Two consequences the gates caught rather than a human: `esm-env` no longer ships, so
  `THIRD-PARTY-NOTICES.md` was stale and the `notices` gate refused the build until it was
  regenerated; and `allowScripts` still named `esbuild@0.25.12`, which vite 8 does not
  install, leaving an allowlist entry for a package that is not in the tree. Emptied ---
  an allowlist naming something absent is how one rots into a blanket permission.

### The macOS corpus run, and a bound that had been raised in the wrong place

- **All eleven corpora are green on macOS at 163 check names**, name sets diffed pairwise and
  byte-identical, every ran/skipped split matching `BUILD.md`. This is the run the Windows
  handover asked for, and it clears the shared `reading.ts` rule --- the conjunction of an
  absolute and a relative sliver test --- on this platform's fonts as well as the other's:
  `a page reads in the order its generator laid it out` passes on both `multilingual.pdf` and
  `encodings.pdf`, which are the two documents that discriminate and are a different document
  on each machine.

- **`text-heavy.pdf`'s row was arithmetic and was one out**, at `142 / 21` where the machine
  that actually has the document reports `143 / 20`. It is measured now. A derived row in a
  column of measurements looks exactly like the rows either side of it.

- **Two timeouts bounded the same run, and raising the documented one changed nothing.**
  `viewer_check.py` moved its bound to 900 s with a comment naming the corpus that forced it;
  the app's own watchdog kept a 300 s default and calls `process::exit` itself, so the tighter
  budget decided and it was not the one being edited. Measured here: `vector-multi` needs
  **387 s**, so it could not pass unattended at any temperature, and `vector-heavy` **249 s**,
  which straddles the bound --- killed on one run and green on the next, same binary. Both are
  the fit-page setup on an A0 page, the operation that defeats spatial culling. The harness now
  derives `TPDF_VIEWERCHECK_TIMEOUT` from its own `--timeout`, so there is one number and the
  watchdog still fires first, which is what makes it print *where* a run stopped.

### A fix that moved its failure to another corpus

- **Nine spike binaries could not find PDFium on Windows without being told where it is.** They
  hardcoded `vendor/pdfium/lib`, which is right on macOS and wrong on Windows in the way that
  is hardest to read: `lib/` exists there too and holds the *import* library, so the path looks
  present and the bind fails much later. They take `tpdf_lib::PDFIUM_SUBDIR` now, which is the
  constant that exists for this. Its doc comment claimed four remained and it was nine --- a
  count in prose, inside the one place written to stop this being rediscovered, and the reason
  it was rediscovered a third time. It names the grep now instead of a number. The two
  binaries that keep a literal `lib` are macOS-only, where it is correct. All nine were run on
  Windows without `--lib` rather than only compiled.

- **qpdf is a requirement, not an option, and the prerequisites table said otherwise.**
  `testdata/make_hostile_pdf.py` shells out to it, so without qpdf there is no
  `hostile-manifest.json` and `sanitize-rewrite` cannot start --- while `BUILD.md` listed qpdf as
  "optional ... not needed to build or run". The script also failed opaquely, with a bare
  `FileNotFoundError` out of `CreateProcess` that named neither qpdf nor the fixture it was
  building. It checks up front now and says what is missing and what depends on it.

- **A page whose font reports no metrics at all is read as one line.** The rule added a day
  earlier --- refuse a character box under a tenth of a point across the line, and re-attach it
  by preceding index --- fixed the missing space in `café latte` and broke `encodings.pdf`.
  Page 2 of that fixture is set in a predefined CMap with no embedded font, so PDFium reports
  **every** character 0.018 pt tall; all of them were refused, nothing was placed, and the page
  came back as a single fragment with its two lines, 632 pt apart, joined into one. Read aloud
  and copied that way.

  The rule is a conjunction now: a box is bookkeeping when it is thin absolutely *and* thin
  against `typicalCross`, the median height of the page's placed characters. The two measured
  samples are three orders of magnitude apart on the second quantity and adjacent on the first,
  and `tagged.pdf`'s comma --- a third of its letters --- is clear of both and stays
  `SHORT_MARK`'s business. Both halves proved by mutation, one red test each; the median proved
  against a maximum, which survived the whole suite until a control was written for it.

  **Nothing aimed at the fix could have caught this.** Its own corpus went green, its unit
  tests went green, and macOS was green on all eleven because its substitute font has real
  metrics. What found it was re-running every corpus and diffing the name sets.

### The macOS half of the Windows work, and the defect only a unit test could reach

- **The five-file `worker.rs` split was a pure move, and the one thing that did not survive
  it was an import.** `use super::Request` sat at the test module's top while its only user
  is a `#[cfg(windows)]` test, so the platform that could see the problem was the one that
  could not compile it --- `cargo test` passed it as a warning and only `clippy
  --all-targets -- -D warnings` was fatal. It reads as `super::Request::Open` at the call
  site now, matching the `super::Worker::spawn` beside it, so there is no cfg-gated import
  to fall out of step again. Everything else in the split, `worker_handover.rs` and the
  macOS halves of `worker.rs` and `worker_shm.rs` included, compiled unchanged: gates
  12/12.
- **A space whose font parks its box off the line stays on the line.** `reading.ts` bands
  characters by overlap, and `msgothic.ttc` reports the space in `café latte` as *placed*,
  0.02 pt tall and 0.12 pt clear of the letters' band --- so it matched nothing, became a
  fragment of its own, and fell out of the line's ranges. The line read `cafélatte`, aloud
  and on the clipboard. A box under `SLIVER_PT` across the line is now re-attached by
  preceding index the way an unplaced character is, and deliberately *not* absorbed into the
  preceding box the way a combining mark is --- a mark is drawn over its base, a sliver
  would drag the line's box down to meet it.

  **The corpus could not have found this and cannot verify it.** The fixture's generator
  picks a font from what the machine has, so the folding page is `msgothic.ttc` on Windows
  and Arial Unicode here, and Arial Unicode puts its space inside the letters' band. What
  discriminates is a unit test carrying the measured geometry, which fails on any platform:
  removing the clause turns it red alone, and raising the threshold until it swallows a
  2.89 pt comma turns its control red instead.
- **`tpdf.log` lands where macOS puts logs**, which no Windows machine could check. Proved
  by forcing a diagnostic rather than by opening a document --- every `diag::note` site is
  an exceptional event, so a healthy run writes nothing and "no file" would have been
  indistinguishable from a wrong path. Killing a render worker mid-run creates
  `~/Library/Logs/com.timostein.tpdf/tpdf.log`, directory and all, with the line stamped;
  with `TPDF_LOG_FILE` set the line goes there instead and the default directory is not
  created, which is the half that makes it a measurement rather than a coincidence.
- **`fetch_pdfium.py --check` verifies the library on this machine now.** The July stamp
  predates the digest line, so it warned and fell back to an existence check exactly as
  designed; one re-fetch records the digest and the gate is strong.
- **All eleven corpora at 163 check names, byte-identical name sets**, diffed pairwise
  rather than counted, with `session_check.py` green on all four phases and both controls.
  `text-heavy` read 143/20 before the change and 142/21 after, which is the withdrawal race
  BUILD.md already documents and not a regression --- the names are the invariant, and they
  did not move.

### An independent review of the whole tree, and what fixing its findings changed

- **The sharpest finding was in a gate, not in the code.** `fetch_pdfium.py --check`
  certified a stamp it had itself written, and its only fact about the artefact was a
  `*pdfium*` glob that the import library satisfies alone --- so deleting or swapping
  `bin/pdfium.dll`, the parser the whole containment story is about, left the gate green.
  The stamp now records the extracted library's own digest and `--check` re-hashes it;
  a missing library, an altered byte and a wrong-platform stamp each turn it red, proved
  by doing each of those things.
- **Rotating the view no longer discards the document's tagged reading order.**
  `turnedView` returned every field but `runs`, so one quarter-turn silently demoted a
  tagged page to geometric order for the screen reader and for copy. Runs are index-based
  and rotation renumbers nothing; they pass through now, with a test that was red first.
- **A selection larger than the text cache copies correctly.** Copy re-read the cache
  after loading, and LRU order is page order for pages loaded once --- so past 400k
  characters the front was always evicted while the tail arrived, deterministically, and
  the error blamed the document. The text is taken from each page's own reply as it
  arrives; the cache is an optimization again rather than a correctness dependency.
- **A thumbnail render that finished before its withdrawal landed is dropped, not kept.**
  Rotation and inversion withdraw the in-flight request, but a request that had already
  completed returned in full and was stored in the old orientation, permanently --- the one
  of three render paths with no generation check now has one. The withdrawal counter also
  stopped charging rotation and inversion to the contention metric.
- **Four toolbar states joined the status change-detection summary** --- the three search
  options and the scoped flag, which could flip without a status event when the query was
  empty, leaving the button and its `aria-pressed` stuck. The scoped fact was also read
  from the searcher's last scan rather than from the viewer's own scope, so it was stale
  even when a status did fire.
- **The Windows title bar shows the file's name rather than its full path.** Three copies
  of a `/`-only basename split became one `paths.ts` helper that knows both separators.
- **`structure.rs` bounds its output as well as its walk.** Depth and element count were
  capped; the per-element mark count and the runs they emit were the document's to choose,
  a `elements x marks x chars` budget with one factor bounded. A page-wide mark budget and
  a run cap set the existing `truncated` flag, and the untagged-character count no longer
  wraps.
- **`parse_bmp` refuses a pixel array shorter than its header declares** --- the one field
  it did not validate was the one that decides how much memory GDI reads, including the
  palette and mask block read through the same pointer, and an `i32::MIN` height no longer
  aborts in debug.
- **The trap index has a gate.** The index in `AGENTS.md` had fallen four titles behind
  `docs/TRAPS.md` --- the same-commit rule held by convention, and the head commit had
  already broken it. `check_trap_index.py` is the twelfth gate: a title-set diff both
  ways, with the annotation rule the real index taught it. `README.md`'s count of the
  traps is now a shape, not a number, for the reason the number was twelve behind.
- **The sinks gate sees `setAttributeNS` and a computed `createElement`.** Both were
  outside its patterns while a computed `createElement` already existed in the tree ---
  whitelisted, but unexamined. Five rules now, each proved to fire by mutation, with an
  exemption marker that warns when it marks nothing.
- **The workflows pin their actions by commit, not by tag.** The release workflow runs
  with the signing secrets in scope, and a moved tag on a third-party action was the one
  supply-chain direction its fork threat model did not cover. Every SHA verified against
  its repository and tag before it was written down.
- **`worker.rs`'s header tells the truth about Windows again.** It opened with "Windows
  has none of this" above the Windows implementation --- the two-platform account it
  actually has now, and a sweep of every other module header against its `cfg`s found and
  fixed one more (`render.rs` said workers were the macOS default; they are the default on
  both).
- **`viewercheck.ts` reports through `checkreport.Report`.** The largest harness carried a
  private, already-drifted copy of the machinery the shared module was written to prevent;
  240 call sites migrated with the 109 check names byte-identical, `mutate_viewer.py`
  parses after the marker instead of slicing a column, and a new test pins the line format
  the Python parsers grep --- so format drift goes red here before it breaks a parser.
- **The wire types have one owner.** `DocumentInfo` and `PageSize` were hand-mirrored in
  four files, two of them already drifted to a subset; `ipc.ts` owns them now, with
  `render.rs` named as the authority.
- **The page-1 geometry assumption is stated at its true cost.** The scroller comment
  called a mixed-size document "a scrollbar problem"; it is content truncation, since the
  one size decides which tiles are ever requested. The comment and `docs/PLAN.md` now say
  what is actually built, and `testdata/make_mixed_pdf.py` generates the document that can
  discriminate the correction --- the corpus was uniform, so no check could go red on any
  of this.
- **And the correction landed.** The scroller holds one size per page, accumulates each
  page's own height into the next page's top, and derives the tile grid, the placeholder
  scale, the centring and the scrollbar extent per page; a page whose size is not known yet
  is laid out at the mean of the sizes that are, which is page 1's until a second arrives.
  Real sizes are learned from the text extraction the frame loop already performs for every
  visible page --- no new command, no second request --- and correcting one invalidates that
  page alone, re-anchors the reader on the page and the fraction through it, and refits if a
  fit is following. On `mixed.pdf` the A3 insert was drawn cropped to A4 with nothing on
  screen to say so; reinstating the uniform layout now turns three checks red, one of them
  reporting `0% page` where the page's own ink is.
- **What the coordinator diagnoses now survives the run.** Every worker and print
  diagnostic was an `eprintln!`, and a double-clicked Windows GUI process has no stderr, so
  the lines this codebase words most carefully were the ones a user could never send back.
  Nine parent-process sites go through `diag::note` now: stderr gets the line byte for byte
  as before --- that channel is what the harnesses parse, and a test re-runs the binary to
  prove no line moved off it --- and a UTC-stamped copy lands in `tpdf.log` under the
  platform's log directory, 256 KiB plus one predecessor, `TPDF_LOG_FILE` overriding.
  Residual risk 13 keeps the honest half: a worker's own dying words still evaporate,
  because a contained child holding a writable path would be a hole in the sandbox.
- **`worker.rs` is five files instead of 2,861 lines.** The wire protocol, the shared
  mapping, the macOS handover and the Windows command line each hold their own seam, with
  every public path re-exported so no consumer changed --- proven a redistribution rather
  than a rewrite by attributing every moved line to its original, and by 285 tests whose
  names are byte-identical before and after.
- **The JavaScript harness ships, and that is now a recorded decision.** 77 kB of the
  221 kB bundle is the checks --- measured two independent ways --- kept because they exist
  to observe the artifact that ships, because deleting the entire payload measures at the
  noise floor of a ~250 ms shell floor, and because the two spike commands they can reach
  add denial of service, not authority. The record in `AGENTS.md` names what would reopen
  it.

### The encoding path, opened in a window for the first time --- on Windows

- **The feature had never run.** `encoding.rs` to `document_mapping` to `Search` to the
  status the viewer emits to `Results` to `statusFor` is six hops, every one typechecked
  and unit-tested, and none of them had executed in a webview on either platform. Eleven
  gates, 352 frontend tests, 92 Rust tests, 95 frontend mutations and CI on two platforms
  all stayed green, correctly: none of them opens a document.
- **Three checks in `viewer_check.py` now do it** --- the backend's count reaching the
  frontend, the line a reader reads, and the accessibility layer withholding a guessed
  page's characters. On `encodings.pdf` the panel says *"2 matches. 1 page could not be
  searched --- the text there is not stored as readable characters."*, which is the case
  worth defending: a partial answer presented as a total one is the same defect in a
  quieter form.
- **The expectation comes from the fixture's generator, not from the subject.**
  `encodings.pdf` names its first page `no-mapping`, written by a program that has never
  heard of `encoding.rs`. Waiting for `unsearchablePages` to go positive --- the obvious
  shape --- would make a backend that always answered zero pass on every corpus, including
  the one built so that it cannot.
- **Two of the three run on every corpus rather than skipping**, so every document with
  nothing to report is the control. A one-sided check here is satisfied by a panel that
  says the sentence about every document.
- **And running it found one.** `a11y.ts` withholds a guessed page's characters and gives a
  reason instead, so the standing check *"the text read out is the page's own text"* is
  false by design there --- and went red the first time `encodings.pdf` was opened in a
  window, the day after the feature shipped. The check was wrong, not the code: it branches
  three ways now, on the manifest, so a layer that quietly stopped withholding fails rather
  than skips.
- **`Search.mappingKnown`**, one boolean carried on the status. Without it the negative
  assertion is satisfied before anything happens --- the count starts at the value being
  asserted, so a frontend that never asked the backend anything passes.
- **`encoding::` joined `mutate_rust.py`**, which covered `search::`, `structure::` and
  `text::` only, so nothing re-checked the new module. 5 mutations, all caught, 36/36
  overall. The one that matters keys the rule on `/Encoding` instead of the descendant's
  `/Ordering`; only the two synthetic diagonals catch it, because every page of
  `encodings.pdf` has those fields covarying.
- **A stale anchor in `mutate_frontend.py`**, left by the same day's rewrite of `statusFor`.
  The harness reported *"its anchor appears 0 times"*, which is it working --- but for a day
  the suite had 94 live mutations and a summary saying 95. 95/95 after re-aiming it.
- **`mutate_viewer.py` runs on Windows**, where `APP` was a macOS `.app` path, the build
  asked for a bundle type that does not exist here, and the probe runners pointed at
  `vendor/pdfium/lib` rather than `bin`. A `viewer-encodings` runner besides, and 3 new
  mutations --- one per new check, the middle one aimed at the *control* corpus because on
  `encodings.pdf` a panel that always says the line is indistinguishable from one that says
  it correctly. All three caught.
- **`searchmapping.test.ts` was in no mutation harness**, the same gap `encoding::` had on
  the Rust side: the file's own header says the truncated-versus-guessing distinction was
  established by an ad-hoc mutation, and nothing re-checked it afterwards. It is in
  `TEST_FILES` now, with 3 mutations --- folding *unknown* in with *unreadable*, which puts a
  warning on every encrypted document, and both directions of the answered flag. 98/98.
- 2 new unit tests, 3 new traps, 218 in the index.

### Windows, verified against the distributable

- **`BUILD.md` step 8, run for the first time since the notices landed.** The MSI payload is
  **four** files rather than the three recorded: `THIRD-PARTY-NOTICES.md` (469 KB) ships
  beside `tpdf.exe`, `tpdf_lib.dll` and `pdfium.dll`, and only an extraction can confirm
  that a file the licences require actually shipped.
- **With the development library moved aside, and with the negative control the step asks
  for.** Hiding the *bundled* `pdfium.dll` as well fails at `0/1 checks passed`, naming that
  exact path --- which is what makes the pass mean the bundled library resolved rather than
  the development tree the run can also see.
- **Build before hiding it, not after**: the bundler copies that DLL as a resource, so a
  build with it already moved aside dies at `resource path ... doesn't exist`, which reads
  like a broken checkout rather than like the sequence being wrong.
- **`multilingual.pdf` could not run here at all**, and the failure looked like a broken
  build: `viewer_check.py` read the app's output with the locale codec, so the first
  Japanese character in a check *detail* killed `communicate` inside its own reader thread
  with `UnicodeDecodeError`, leaving a traceback, exit 1 and a transcript file holding the
  word `None`. Fixing the decode moved it one step to `UnicodeEncodeError` on `print`,
  since Python's stdout encodes with the same codec. Both fixed --- the second in
  `live_output.py`, which is where all three harnesses get their streams. The same defect
  the day's own `mutate_rust.py` fix was about, surviving in the third harness.
- **Nine corpora measured on Windows**, against the extracted MSI with the development
  library moved aside: **160 check names** on every one, and eight of the nine are the macOS
  split plus exactly the arithmetic of the three new checks --- the same checks skipping on
  both platforms for the same documents, which is a stronger statement than a matching
  total. `multilingual.pdf` is the ninth and differs by one legitimately: its generator picks
  a font per page from what the machine has, so that fixture is a different document here.
- **One failing check on that corpus, and it is not new work.** The folding page reads back
  `cafélatte` where its manifest says `café latte`. PDFium's extraction *does* carry the
  space, so it is lost between extraction and the line's ranges; whether that is a gap in
  `reading.ts` or an artifact of the font this machine laid the page out in is **not**
  settled, and the green macOS run is no evidence either way. Recorded in `BUILD.md`.
- **The fixture list in `BUILD.md` generated neither `multilingual.pdf` nor
  `encodings.pdf`**, while the corpus table below it told a reader to run the viewer check
  against both --- so the instruction that produces a fixture and the instruction that
  consumes it disagreed, and the failure is an absent file reported as a broken bundle.
### A markup-sink gate, and the third state a page's text can be in

Recorded after the fact, on 2026-08-02: the six commits these describe added no changelog
entry, so the two largest changes of the day were absent from the release history.

- **`scripts/check_webview_sinks.py`**, the eleventh gate, enforcing `docs/THREAT-MODEL.md`
  T8 --- until then the one mitigation in that document held by convention rather than by a
  line. Document text reaches the DOM as data; the gate pins the narrow invariant that
  makes that checkable at all, **no markup-parsing sink anywhere in the frontend**, plus
  four rules closing the routes by which a string that cannot become markup can still
  become a navigation. It refuses a scan that found no files or no `setAttribute` calls,
  since a pattern that stops occurring passes exactly like a clean one.
- **The gate's own first version was not sufficient**, which is why the reasoning is
  written down rather than assumed: it enforced only that an attribute *name* be a literal,
  while the threat model claimed sufficiency --- and `setAttribute("href", row.title)`
  satisfies both. Correct about the tree in front of it, wrong about what it guaranteed.
- **`src-tauri/src/encoding.rs`** --- the third state between "this page has text" and
  "this page has none". A CID font with no `/ToUnicode` is ordinary in the wild and PDFium
  does not fail on it: it reads glyph ids as character codes and returns text of the right
  length, in the right places, with the right word lengths. The page is therefore not
  textless, so the one honest signal the find bar had did not fire, a reader searching for a
  word they can see was told there were no matches, and the accessibility tree read the
  nonsense out.
- **It is a `lopdf` question, not a PDFium one.** Garbage of the right length cannot be
  told from a language nobody here reads, so every rule over the code points is a heuristic
  that will call a real document broken. The file answers it directly: a font either
  declares what its glyphs mean or it does not. The operative field is the descendant's
  `/CIDSystemInfo /Ordering`, **not** the encoding name --- `/Encoding` decides code to CID
  and says nothing about CID to Unicode.
- **A page `lopdf` cannot account for is unknown, never clean.** The two parsers disagree
  about page counts more often than one would like and always in the dangerous direction:
  `lopdf` reports zero pages for `incr-encrypted-pw.pdf`, which PDFium paginates normally,
  and an empty answer reads as "no page has a problem".

### Public, MIT-licensed, and CI on both platforms

- **The repository is public and MIT-licensed.** `LICENSE` had to land *before* the flip,
  not after: a public repository with no licence is all-rights-reserved, and anyone who
  cloned in the gap would have had no grant.
- **No history rewrite was needed**, which is the expensive half of going public and the
  one that cannot be undone cheaply. All 108 commits across every ref were authored and
  committed as the `tstone-1` noreply address; there were no tags, no `refs/pull/*`, no
  forks and no workflow run logs to become visible. Verified by scanning every ref rather
  than by comparing local tags to remote ones --- the portfolio records a tag that survived
  a first cleanup round precisely because it was stale on *both* sides and therefore
  matched.
- **The pre-flip audit found two files, and neither was a secret.** The AGPL rationale
  named an employer and its internal systems as its worked example, and `ocr.rs` used the
  employer's name as a test string in five places. Both were associations rather than
  leaks, and the licensing argument reads the same without them. The scan that matters is
  for *values* across the tracked tree, not for gitignored *paths*: `testdata/private/`
  has been gitignored since the beginning against exactly this, and it turned out never to
  have existed, so the only real question was what had been pasted into source.
- **`.github/workflows/ci.yml`** --- the gates on `macos-latest` and `windows-2025`, on
  every push to `main` and every pull request. It invokes `scripts/gates.py` rather than
  restating its commands, so the gate list still lives in exactly one executable place.
  Written for a hostile fork: `pull_request` not `pull_request_target`, `contents: read`,
  and no secret referenced --- the six `APPLE_*` secrets survive the flip and stay confined
  to `release.yml`, which triggers on a tag push and is unreachable from a fork.
- **`SECURITY.md`**, with a scope section that says what a *correct* answer looks like: a
  redaction reported as `not verified` is by design and is not a finding, and PDFium's own
  defects belong to Chromium, which ships it to rather more people.
- Bundle metadata --- publisher, copyright, category, homepage, descriptions --- which was
  empty and would have shipped an installer with no author on it.
- **An app icon**, replacing the Tauri scaffold logo that had been in place since the first
  commit. A paper page on a charcoal tile with a black bar across it that overhangs both
  page edges, and a thin amber rule under the bar. The bar is a redaction, which is the one
  capability that is both the differentiator and a hard constraint in the design, and a
  solid horizontal is the shape most likely to survive 32 px. The vector source is
  `design/icon.svg` and the seventeen raster files regenerate from it with
  `npx tauri icon design/icon.svg`. It is kept **outside** `src-tauri/icons/` deliberately:
  this repository has already been bitten once by a bundler enumerating a directory and
  registering what it found there. Two earlier candidates were rejected after rendering
  them rather than by argument --- a paper tile with the same bar read as a list with a
  selected row once the page silhouette was gone, and a diagonal cut vanished into the dark
  tile at small sizes. The mobile icon sets `tauri icon` also emits were deleted; this is a
  macOS and Windows application.
- Two present-tense claims about a third-party notice file were written into `README.md`
  and `AGENTS.md` and then corrected in the same session before either was committed. The
  file does not exist; the obligation is real (PDFium is BSD-3-Clause, the crate tree is
  MIT / Apache-2.0, and both require notices reproduced in *binary* distributions) and it
  blocks the first release rather than the repository being public. `BUILD.md` step 6
  exists for this exact failure and it still took writing the sentence twice to catch it.

### A corrupted legal notice, and a pin that matched on the wrong axis

- **The notices file shipped FreeType's required attribution with the copyright sign
  destroyed.** `read_text` decoded with `errors="replace"`, and
  `vendor/pdfium/licenses/freetype.txt` is Latin-1, so its `0xA9` became U+FFFD ---
  `copyright <?> <year> The FreeType Project`, in the credit line that licence *requires*
  be reproduced. Exactly one replacement character in 469 KB, which is how it passed
  generation, review, the staleness gate and the cross-platform byte-equality check: the
  corruption is deterministic, so every one of those compared it against itself and agreed.
  Decoding is a codec chain now --- UTF-8, then cp1252, then latin-1, which maps every byte
  and cannot fail, so no path reaches `errors="replace"`. A fallback prints a `[note]`
  naming the file and codec. This is the quiet direction of the `text=True` bug above: that
  one raised and cost two rounds of debugging, this one substituted silently and cost a
  wrong legal notice in every installer built since the file was added.
- **`check_toolchain.py` now asserts the host triple, not only the version.** A bare
  `channel = "1.97.1"` carries no target triple, so rustup resolves it against its *default
  host triple* --- a different setting from the default *toolchain*, with nothing keeping
  the two in step. On the Windows desktop the default toolchain was
  `stable-x86_64-pc-windows-msvc` and the default host was `x86_64-pc-windows-gnu`, so
  adding the pin silently moved that machine from MSVC to GNU; rustc reported the pinned
  1.97.1, clippy and rustfmt matched its commit hash, the gate said `[OK]`, and the build
  died three gates later on a missing `dlltool.exe`. CI cannot catch it --- GitHub's windows
  runners default to MSVC, so the pin resolves correctly there and stays green forever.
  Fixed on the machine with `rustup set default-host`, and in the gate so the laptop and any
  fresh clone are told rather than left to discover it. Proved with the real GNU toolchain
  still installed: same version, same hashes, every pre-existing check passing, only the new
  one firing.

### Third-party notices, and what CI found on its first run

- **`THIRD-PARTY-NOTICES.md`** and `scripts/third_party_notices.py` that generates it ---
  325 crates, 4 npm packages and 16 PDFium components --- shipped inside both installers as
  a bundle resource. This is the binary-distribution obligation that a root `LICENSE`
  satisfies nothing of, and it is now the ninth gate rather than a step someone remembers.
- **The licensing sweep this project rests on was structurally incomplete, and the gap was
  the part that parses PDFs.** `cargo metadata` sees 531 packages and cannot see inside a
  prebuilt blob, so the **fourteen C++ libraries compiled into libpdfium** --- FreeType,
  ICU, libjpeg-turbo, libpng, libtiff, Little CMS, OpenJPEG, zlib, Abseil, AGG, fast_float,
  simdutf, llvm-libc --- had never been checked against "no AGPL or GPL, ever". They are now,
  from `vendor/pdfium/licenses/`. Two GPL strings are in there and both are benign:
  `icu.txt` covers ICU4C's autotools scripts under the Autoconf exception, and
  `llvm-libc.txt` is Apache-2.0 WITH LLVM-exception, whose GPLv2 clause *waives* Apache
  terms rather than imposing GPL ones. Allowlisted by file and by mechanism, with a warning
  if an entry names a file that has gone.
- **The npm half was a guess and is now a measurement.** The first draft named the shipping
  packages by hand and was wrong in both directions --- `tslib` is not emitted at all, and
  `esm-env` is, as a Svelte transitive marked `"dev": true`. It produced the right *count*,
  four, which is the part worth keeping: a total that matches is not evidence that the set
  matches. It now reads `dist/assets/*.js.map`, the bundler's own account of what it emitted.
- All three of the gate's failure modes were proved by mutation before it was trusted, and
  the script was restored by byte digest afterwards rather than by eye.

- **`.github/workflows/ci.yml` ran for the first time and failed on both platforms, for two
  unrelated and both correct reasons.**
- **Windows had been broken for two days.** `examples/ocr_probe.rs` imported
  `tpdf_lib::ocr_vision` unconditionally, and that module is `#[cfg(target_os = "macos")]`
  --- so clippy, `cargo test` and `cargo build --examples` all failed there while the same
  commit showed 9/9 on a Mac. Gates run where you are standing, and a green sweep is a
  statement about one machine that reads exactly like a statement about the product. Fixed
  with the repository's established shape: a refusing `main`, a dispatching `main`, and the
  body in `src/probes/ocr_probe.rs` reached by `#[path]`. `ocr-probe` still passes 6/6 on
  macOS after the move.
- **macOS failed because a test did its job.** `print.rs`'s third-parser check asserts
  `examined > 0` so that six `[SKIP]` lines cannot pass as six successes, and CI generated
  no fixtures. `ci.yml` now generates the two dependency-free ones and **states in the
  workflow which it deliberately does not** --- the fonttools ones embed a per-runner system
  font, and `make_incremental_pdf.py` writes ~550 MB on purpose.
- **And the notices file was a function of the platform that generated it**, found on the
  next CI run when macOS went 9/9 and Windows failed `notices` alone. `bblanchon/pdfium-binaries`
  ships the same fifteen licence files in both archives and **nine differ**: eight by line
  endings, which `read_text` already normalised and which were therefore invisible --- the
  failure that survives is the one whose sibling you handle by habit --- and `licenses/pdfium.txt`
  by carrying a `//` comment prefix on macOS and none on Windows. Stripped per line now, and
  the property is testable rather than claimed: `--cross-check <other-pdfium-dir>` renders
  against a second archive and requires byte equality. Run it after any pin bump.
- Diagnosed **from a Mac**, by staging the Windows archive with `fetch_pdfium.py --platform
  win-x64 --dest`, rather than by waiting on CI. The input that differs can usually be fetched.
- The first fix for it did not work and is recorded: `normalise_licence_text` originally
  stripped the prefix only when 80% of a file's lines carried it, but `pdfium.txt` is
  PDFium's licence followed by a dozen others and only the first block is prefixed --- 27
  lines of 196 --- so the guard declined and the verification still failed. A threshold
  chosen from an imagined input, off by a factor of five.
- `--check` now prints the **diff** rather than the word "stale", which is the change that
  made any of this diagnosable. A gate that fails on a machine you are not sitting at is
  only actionable if its message carries the evidence.
- **The Rust toolchain is pinned** --- `rust-toolchain.toml` at 1.97.1 with clippy and
  rustfmt. There was no pin before: the Macs used whatever rustup had and CI used whatever
  `stable` was that morning, so with `-D warnings` a new lint could turn `main` red with
  nobody having changed anything, landing on whoever pushed next rather than on whoever
  chose to upgrade.
- **The pin is enforced, not merely present, and that was the whole difficulty.**
  `RUSTUP_TOOLCHAIN` overrides `rust-toolchain.toml` completely and silently, and all three
  workflow jobs used `dtolnay/rust-toolchain@stable`. Adding the file alone would have given
  the worst outcome available: the pin visible in the repository, absent from every CI build,
  green either way. Both workflows now run `rustup show`, and `scripts/check_toolchain.py`
  is the new **first** gate --- if the compiler is not the one we think, every result after it
  is about a different toolchain.
- Proved by mutation, which also settled the premise rather than trusting an action's
  documentation: `RUSTUP_TOOLCHAIN=beta` produced rustc 1.98.0 against a 1.97.1 pin.
- Two mistakes kept in the trap. A toolchain's components have **unrelated** version schemes
  --- rustc 1.97.1, clippy 0.1.97, rustfmt 1.9.0-stable --- so the first draft's "the minors
  match" failed on a correct toolchain; the real oracle is the shared **commit hash**. And
  the mutation proving the gate was run through `| tail`, so the exit code read was `tail`'s
  and printed `exit=0` for a run that had correctly failed --- in the one command whose entire
  purpose was reading an exit code, with an entry in this repository saying not to.
- `gates.py`'s summary column is derived from the gate names now; `toolchain` is nine
  characters and the hardcoded width of 8 broke the alignment the day it was added. Nothing
  parses that output, but a hardcoded column width is the same shape as the padded-column
  trap.
- **The pin is verified on both platforms**: `pinned=1.97.1 rustc=1.97.1
  RUSTUP_TOOLCHAIN=(unset)` on macOS and Windows alike, so `rustup show` plus the file does
  what the action would have prevented.
- **And the Windows `notices` failure was never staleness.** The script had **crashed**, and
  `gates.py` printed that gate's static reason --- "THIRD-PARTY-NOTICES.md is stale" ---
  which sent two rounds of investigation after a content difference. `subprocess.run(...,
  text=True)` decodes with the locale codec, cp1252 on Windows, and `cargo metadata` emits
  UTF-8 containing `0x81`: it comes from a crate author's name, `Emilio Cobos Álvarez`, whose
  `Á` is `C3 81`. `.stdout` then arrives as `None` and `json.loads(None)` raises a TypeError
  about JSON types that mentions no encoding.
- This is a trap **this repository already had an entry for**, reintroduced in a new script
  on the same byte, in a session that had read it. Fixed with `encoding="utf-8"` in
  `third_party_notices.py`, `check_toolchain.py`, and `mutate_rust.py` --- the last a
  pre-existing latent instance in a harness documented as passing on Windows, which reads
  `cargo test` output from a crate whose sources contain that byte.
- `gates.py` prints the **exit code** on failure and words the reason as "usually means". A
  checker that dies and a checker that reports a failure are different events, and they had
  the same label.
- 5 new traps, 211 in the index.

### Phase 1 --- case folding in search

- **The fold case-folds rather than lowercasing.** `char::to_lowercase` is the operation for
  *displaying* text and leaves `ß` alone, since it is already lowercase; `default_case_fold` is
  the one Unicode defines for caseless *matching*, which is what a find bar does. Taken as a
  decision, not a fix: the same operation folds `ﬁ` to `fi`, which `search.rs` had refused.
- **It fixed two of the three consequences, not three** --- the prediction was wrong and the
  difference matters. `strasse` now finds `Straße` and `οδος` now finds `ΟΔΟΣ`. `istanbul` still
  does not find `İstanbul`, because that difference is a **combining mark** rather than a case:
  `İ` folds to `i` + U+0307 exactly as it lowercased. Removing the dot is accent stripping, a
  separate decision; Unicode's Turkic mapping would do it and also folds `I` to `ı`, which is
  right only for Turkish and nothing here knows a document's language.
- **And the ligature cost is near-theoretical.** U+FB01 is in the same Alphabetic Presentation
  Forms block as the Arabic this corpus had already shown PDFium normalising, so a page typeset
  `ﬁnal` arrives as five plain letters and the fold's ligature rule is never reached from the page
  side. It is reached from the *query* side: a reader who pastes a ligature into the find bar used
  to get nothing and now gets the word. The trade turned out one-sided.
- `caseless` (MIT), whose only new transitive is `unicode-normalization` (MIT OR Apache-2.0) ---
  checked with `cargo metadata` over all 531 packages rather than read off a README.
- One asymmetry that cannot be removed: a **pattern** is not folded, because a regex source is
  not text. It gets the engine's `i` flag, so the length-changing folds do not apply on the
  pattern side.
- A `ﬁnal ﬂour` line on the folding page, 5 new unit tests including one pinning what folding
  does **not** fix, and 2 new mutations; the redundant second one was removed after running it
  showed the first already catches both halves. 2 new traps, 206 in the index.

### Phase 1 --- encodings that are absent, broken or predefined

- **`testdata/make_encodings_pdf.py`**, the other half of the Phase 1 sentence about *"malformed
  encodings and custom CMaps"*. Three pages: a CID font with **no `/ToUnicode` at all**, one whose
  `/ToUnicode` maps CIDs to lone surrogates, and `/UniJIS-UCS2-H` over a **non-embedded**
  KozMinPro. A separate corpus from `multilingual.pdf` because the subject differs --- those pages
  are correct documents in other scripts, these are documents whose statement of what their bytes
  mean is missing or wrong. `examples/search-probe` reads both unchanged: 23/23 with 7 not
  applicable.
- **A defect in the regex path, and a total one.** A pattern was compiled case-sensitively against
  a haystack the fold had already lowercased, so with match-case off **any uppercase letter in a
  pattern matched nothing**. `compile` now sets the `i` flag --- folding the pattern source is not
  an option, since it would turn `\S` into `\s` and `[A-Z]` into `[a-z]`. It survived because
  `compile`'s own doc comment asserted the invariant it was breaking, and because the harness
  builds its pattern from a word on the page, so every corpus with ordinary prose agreed by
  accident.
- **A code-point index sliced with `String.prototype.slice`** in the cross-page phrase check ---
  in the two lines directly under a comment about checking the index spaces. Sliced by code point
  now.
- **What the corpus established about PDFium.** With no `/ToUnicode` it does not fail: it returns
  eighteen characters of plausible garbage for eighteen drawn, and the page is **not textless**,
  so the one honest signal the find bar has does not fire and a reader searching for a word they
  can see gets *no matches*. There is a third state between "text" and "no text" that nothing
  represents; a detector needs no heuristic (the font either declares a `/ToUnicode` or it does
  not) and surfacing it is a product decision, recorded in `docs/PLAN.md` rather than built.
  Separately, the vendored build **does** carry the predefined Adobe-Japan1 CMaps.
- **The replacement-character path is now reached by a fixture**, where it had been covered by
  unit tests alone --- and the same page pairs two unrelated broken entries into one astral
  character, which is what decoding UTF-16 means and is pinned so the length is not read as a
  defect later.
- 3 new unit tests on the regex switch, 2 new mutations for it, and a new `encodings` runner with
  2 more --- all caught. All **ten** corpora report the same 157 check names, sets diffed pairwise.
  Seven new traps, 204 in the index.

### Phase 1 --- search on text that is not English

- **`testdata/make_multilingual_pdf.py`**, the corpus `docs/PLAN.md` Phase 1 names as one of two
  items not to be estimated as viewer polish. Four pages, one property each: Japanese with no word
  separators and a Latin token inside the run, Arabic in base letters *and* in presentation forms,
  a folding page (NFC against NFD, a Turkish dotted capital, Greek final sigma, `ß`), and a code
  point above the BMP. Identity-H over a subsetted CIDFontType2 throughout, per-page font selection
  with a hard error naming the missing character, and a self-check that refuses to write a fixture
  that has stopped discriminating.
- **`codes` is now what it always claimed to be: one Unicode scalar per index.**
  `FPDFText_GetUnicode` is a UTF-16 API, so PDFium reported U+20000 as two lone surrogates with one
  box each --- `char::from_u32` refuses both, the fold dropped them, and a CJK Extension B ideograph
  was **unfindable while plainly visible on the page**. `extract` joins the pair and unions the
  boxes; an unpaired surrogate becomes U+FFFD rather than being dropped, which would shorten the
  page and shift every box after it. JavaScript had been reassembling the halves by accident, so
  only a Rust-side assertion could see it.
- **A combining mark no longer opens a line of its own.** An acute sits above the x-height and its
  box does not touch a word with no ascender --- measured, a 0.96 pt gap --- so `resumé` written
  decomposed read as three lines and the accessibility tree announced them that way. `café` hides
  it, because the `f` drags the band up into contact. Keyed on `\p{Mn}`/`\p{Me}` rather than on
  geometry, which answers the question about the character instead of about where it was drawn.
- **`examples/search-probe`**, 60/60 with 9 not applicable: per-page text against the manifest,
  per-query hit **counts**, and per-hit assertions that the indices address the text the match
  claims and that every hit is paintable. Nineteen queries, each labelled *stated*, *measured* or
  *decided* so that a product decision cannot be mistaken for a measurement.
- **What the corpus established about PDFium**, both the opposite of what was assumed: it maps
  Arabic **presentation forms to base letters** on extraction, so a base-letter query finds shaped
  text and a presentation-form query finds nothing; and it recovers **logical order** from a
  right-to-left run, so a query in reading order matches.
- **What it established about our fold**, all one cause and all now recorded as decisions:
  `strasse` does not find `Straße`, `istanbul` does not find `İstanbul`, and Greek `οδος` finds
  neither spelling. `search.rs` had documented *"`ß` lowercases to `ss`"*, which is false --- `ß`
  uppercases to `SS`. Lowercasing is not case folding; taking case folding means a dependency and
  also means folding `ﬁ` to `fi`, which the same module says it does not do.
- **Two harness defects the corpus exposed.** `viewer_check.py`'s word picker was `/[A-Za-z]{5,}/`,
  so **seventeen** search checks skipped on a Japanese page while printing *"page 1 has no
  extractable text"* about forty-nine characters --- unexercised checks *and* a false reason. Twelve
  run now. And the drag check had no precondition for "there is text where I dragged", so a page
  with lines spread down an A4 sheet read as a defect.
- 10 new unit tests (surrogate pairing, and the index translation no fixture reaches), 5 new
  front-end tests, and 9 new mutations --- 3 Rust, 4 front-end, 2 through the new `search` runner of
  `scripts/mutate_viewer.py` --- all caught. All **nine** corpora report the same 157 check names,
  sets diffed pairwise.

### Phase 1 --- a document's own reading order, read and proved

- **`src-tauri/src/structure.rs`** reads a page's `/StructTree`: the runs in the order the
  document says they should be read, each with the element's type as the document spells it
  (`H1`, `P`, `Note`, `TD`) and the path it sits at. Answers the half of `docs/PLAN.md`'s
  accessibility note that called geometric reading order *"a real limitation rather than a
  missing nicety"*.
- **No second extraction.** `FPDFText_GetTextObject` gives the page object a character was
  drawn by and `FPDFPageObj_GetMarkedContentID` gives that object's mark, so a character index
  resolves to a marked-content id directly and a run lands in **the same character indices**
  selection, search and the accessibility tree already use. Parsing the content stream and
  correlating it with the extractor would have been the third independent extraction here,
  which is the failure `text.rs` opens by warning about.
- **`testdata/make_tagged_pdf.py`**, and the discrimination is the point: page 1's margin note
  reads third by geometry and last by the tags, page 2 is a control tagged in the order geometry
  would infer anyway, and the generator refuses to write a fixture that has lost the difference.
  Poppler's `pdftotext` reads page 1 geometrically, which is external evidence that the fixture
  is not merely self-consistent. `qpdf --check` accepts the file.
- **`examples/structure-probe`**, 10/10 on the tagged fixture with the untagged control. It
  asserts the order matches the tags **and** does not match the geometry, and resolves every run
  through a fresh extraction of the page rather than trusting the run's own report.
- **The bound and the honest invariant.** The tree is hostile input like the outline, so the walk
  is bounded in depth and elements and the truncation is reported. "Every character is claimed"
  is *not* the invariant --- PDFium's generated separator between two text objects belongs to
  neither element --- so what is asserted is that nothing **visible** is left out.
- 7 unit tests on the span logic, and 4 mutations against the probe, all caught.
  `scripts/mutate_viewer.py` now drives two harnesses, selected with `--runner`; the structure
  one needs no webview and runs in 15 s a mutation.
- One new trap, index now 177.

### Phase 1 --- headings announced as headings, and a harness that names its silence

- **`H1`--`H6` reach the accessibility tree as `h1`--`h6`**, and a bare `/H` as `h2`. This is
  the reason to read element types at all: "jump to the next heading" and "list the headings"
  are how a screen-reader user skims, and neither works on a page of paragraphs however well
  ordered. Closes the first half of what `docs/PLAN.md` listed as *"absent: headings and table
  semantics"*.
- **Granularity follows who drew the boundary.** A tagged block is handed over as one element,
  because the producer declared it; an inferred block is handed over line by line, because the
  XY-cut's boundaries are a guess and merging on a wrong one joins two columns into a
  paragraph. `ReadingBlock.tag` is `null` for the inferred case, and that `null` means
  *inferred*, not *unknown* --- filling it with `"P"` destroys the distinction the split exists
  for. `readingLines` is now written in terms of `readingBlocks`, so the two cannot disagree
  about the order.
- **Table cells and figures deliberately keep `<p>`.** A `TD` outside a `<table>` is not a
  cell, and building a real table needs to know which cells share a **row** --- `TaggedRun.path`
  carries element *types*, so two different `/TR`s are indistinguishable. That needs element
  identity from `structure.rs`; faking it produces a table with one row per cell. A `/Figure`
  needs its `/Alt` text, which is not read yet. Every block carries `data-tag` so a type nobody
  handled is visible rather than silently flattened.
- **`spokenText` selected `p`** and would have read a tagged page short by its headings ---
  surfacing as the page's *text* being wrong rather than the selector being narrow. Widened to
  every child. Four new checks, 157 names on all eight corpora.
- **`viewer_check.py` now says which silence a timeout was.** A suspended page and a page stuck
  in a loop both present as no output and a live process, and they want opposite responses.
  `diagnose_silence` samples CPU **time** twice before the kill --- a delta, because a single
  `ps -o %cpu` is a lifetime average on macOS and a page that worked and then got suspended
  reads as busy. Three bands: suspended, alive and waiting, alive and spinning. All three proved
  against real processes. `mutate_viewer.py` inherits it; `session_check.py` and `open_check.py`
  do not, and `BUILD.md` says so.
- **The fixture gained a second heading level**, because it had to: with one `/H1` on the page,
  the mutation that announces every heading as `h1` produced the right answer and the check
  passed. A property with one value present is the same as none. `structure-probe` is 10/10 on
  the five-block page.
- 11 new frontend tests, 91 frontend mutations and 5 tagged-viewer mutations, all caught. Three
  new traps, index now 187.

### Phase 1 --- a tagged page is read in the order the document says

- **`reading.ts` believes the tags where it has them.** `usableRuns` is the whole decision ---
  the runs are used when they claim every *visible* character, and the geometry otherwise ---
  and it is the only place either route is chosen, so the accessibility tree and the copy path
  cannot disagree about which one ran. Both consumers go through `readingLines`, so neither
  needed a call-site change; the runs travel on `PageText`, which already crosses the worker
  boundary.
- **The tags decide the order of the blocks; the geometry still decides the lines inside one.**
  A tagged run is a paragraph and a screen reader is handed lines. Emitting a paragraph per
  element is a separate question and is now a change to `a11y.ts` alone.
- **Every character keeps its place.** Runs do not claim PDFium's generated separators, and
  emitting only the claimed characters lost six of them --- the page came back six characters
  shorter than the page. Each character now has an owner, so the tagged order is a permutation
  of the page, which is the invariant worth asserting and was not being asserted.
- **A comma no longer opens a line of its own.** Pre-existing in the *geometric* path and found
  by the new fixture: a comma drops below the baseline and overlaps its line by 46% of itself,
  under the banding threshold, and the 0.01 pt-tall spaces then matched the comma's new band. One
  line came back as `inthemaincolumnandclosesthesection` and a second holding `, .` and six
  spaces --- read aloud and copied exactly like that. A box too short to be a line of text now
  joins the line it touches.
- **`tagged.pdf` is the eighth corpus** for `viewer_check.py`, and its manifest gained the three
  fields that harness already reads, so the reading-order check asserts its lines in tagged order
  against a file a different program wrote. Two new checks, 155 names on every corpus, sets
  diffed pairwise.
- Adding it exposed **three checks whose preconditions were written as assertions** and had never
  met a two-page document; all three now skip with the reason printed. `mutate_viewer.py` grew a
  `viewer-tagged` runner and now refuses a mutation whose expected check skipped in the baseline
  --- which would have reported SURVIVED.
- 11 new frontend tests and 1 Rust test; 85 frontend mutations, 23 Rust, 17 viewer, all caught.
  Seven new traps, index now 184.

### Phase 1 --- the command list, moved somewhere a check can reach it

- **`src/lib/appcommands.ts`**: the twenty-nine commands and the window-key routing move out
  of `App.svelte`, which is where `docs/PLAN.md` recorded them as *"covered by nothing"* ---
  structurally, since `viewercheck.ts` runs instead of that component ever booting. The move
  was verified mechanically: ids and titles identical and in order, every comment preserved,
  and `App.svelte` outside the two moved blocks byte-identical to `HEAD` bar three imports.
- **36 new checks in `viewercheck.ts`**, 153 names on every one of the seven corpora and the
  name sets diffed pairwise rather than counted. Every command is driven the way a reader
  drives it --- open the palette, type the title, press Enter --- with the title asserted to
  rank first beforehand, and the ones reaching the viewer asserted against a real viewer
  moving. ⌘K goes through the real routing, asserted closed first and with a bare `k` asserted
  not to open it.
- **A coverage audit that cannot be reopened silently.** Every command is classified as driven
  against the viewer, driven against a recorded action, or not driven *with the reason*; the
  check asserts the table and the registry are the same set, so a command added tomorrow turns
  it red until somebody decides how it is covered.
- **`scripts/mutate_viewer.py`**, the third mutation harness --- the other two drive `cargo
  test` and `vitest`, and none of this is reachable from either. 15 mutations, 15 caught. Its
  own cross-check fired on the first run: it read `viewer_check.py`'s stderr as well as its
  stdout and counted the wrapper's `[FAIL] exit 1` as a check, so every mutation came back off
  by exactly one and was reported as a **broken run** rather than as caught or survived.
- Two defects in the new checks, both already-named traps: the phase left the viewer rotated
  and turned eight later assertions red across three phases, and the `enabled` guard was
  written around a shared mutable binding whose behaviour this repository cannot account for
  (see the trap of that name --- the check now builds a second registry instead).

### Phase 1 --- the three things search could not do

- **Regular expressions.** A third option, matched against the same *folded* sequence a literal
  gets, so a pattern and a literal mean the same thing by the same switches and a hit stays in
  the character indices the highlight already uses. Two consequences pinned by tests and stated
  in the module docs: `\n` never occurs, and `^` anchors to the page rather than to a printed
  line. `regex` was already in the tree transitively, `MIT OR Apache-2.0` read out of `cargo
  metadata`, so declaring it adds no package.
- **A pattern that does not compile is reported, not answered.** `PageMatches` carries a
  `problem`, the walk stops on the first, and the find bar shows the reason where the counter
  goes --- "no matches" for `foo(` is a statement about the document.
- **Search within a selection.** The scope is a snapshot taken when the reader scopes the
  search, not a live reading: clicking on the page dismisses a selection, and a live scope
  would widen to the whole document while the label still said otherwise. Applied to the
  results rather than to the haystack, so the whole-word boundary still reads the characters
  either side of a hit on the page and a snippet still shows the page's own context.
- **Matching across a page boundary.** The tail of each page is handed to the request about the
  next one; a straddling hit is anchored where it starts, carries `endPage`, and is highlighted
  on both pages. The break is **whitespace** --- a page's text does not end with any, so a plain
  concatenation reads `rasterappearance` and matches nothing --- and a word the break splits is
  not rejoined, exactly as a word a line break splits is not. The wrapped walk's one unexamined
  join is closed by a single extra request.
- 10 new Rust tests (46 in `search::`, 221 in the library), 5 new checks in the running app.
  The scoped check took three attempts to become able to fail: first it compared against the
  document total, which the page list alone explains; then it computed its own precondition
  from the results, so a mutation that stopped clipping turned it into a `[SKIP]` instead of
  turning it red.
- **A mutation found a latent hang.** Deleting the whitespace-only guard produced a run with
  no result rather than a red test: `all` is true of an empty sequence, so that guard was also
  what stopped an empty needle advancing the literal walk by zero forever. The empty needle is
  refused first and on its own now, and deleting the whitespace guard turns a test red the way
  a mutation should.
- Five new traps, index now 176.

### Phase 1 --- a first OCR engine behind those interfaces

- **`src-tauri/src/ocr_vision.rs`**, macOS Vision implementing `Recogniser`. One crate added,
  `objc2-vision`, `Zlib OR Apache-2.0 OR MIT`, read out of `cargo metadata` rather than
  assumed from the rest of that family; it is the only new package in the tree, since
  `objc2-core-graphics` and `objc2-core-foundation` were already there transitively.
- **`ocr-probe`**, which runs the engine on real rendered pages rather than on synthetic
  geometry. 6/6 on `text-base14`, `text-cid` and `rotated`; `outline-simple` and `form` 5/5;
  `columns` 2/2 with the rest honestly skipped; `vector-heavy` 1/1 against the inverted
  claim, since a page with no text is one where reading nothing is the correct answer and
  reading something is the engine inventing it.
- **The coordinate flip is now verified against the engine, not against arithmetic.** Vision
  reports boxes normalized with the origin bottom-left; everything here is points, top-left,
  y down. `normalised_to_points` had five green unit tests that could not have caught a
  wrong assumption about what Vision *means*, so the probe asserts content-at-a-position:
  the word the document places highest must come back highest. Removing the flip turns that
  red with `read gap -119 pt against 123 pt in the document`, and takes both gate checks and
  two unit tests with it.
- **Three defects came out of running it, all in the checking rather than the gate**, and each
  is now a trap: an engine's bounding box is looser than the pixels it was given, so strict
  containment rejected a control the engine had plainly read; a strip cropped flush to a span
  clips its ascenders and the engine misreads its own text; and matching a span by substring
  found a different occurrence than the one measured, which let the y-flip check pass by 1 pt
  on an 842 pt page. Index now 171.
- 19 tests. The 8 mutations still catch 8, each tripping the predicted test.

### Phase 1 --- the OCR interfaces

- **`src-tauri/src/ocr.rs`**, answering `docs/PLAN.md` §10 question 10 by defining the
  interfaces rather than by deciding which phase owns them. §9 was right and §8's
  enumeration of Phase 1 had a gap. No engine is implemented: which engine runs is a
  platform question, and the part that has to be correct is above it.
- **The verdict is three-valued.** Search and redaction verification want opposite things
  from an empty OCR result --- for search a poor answer, for §6 step 4 the entire claim, and
  §6 step 4 is the only check that can speak about an image carrier at all. `Legibility` is
  `Illegible` / `Legible` / `NotVerified`; every engine failure lands in the third. A
  two-valued verdict must report failure as one of its two, and it is always the clean one,
  because a failure produces no findings.
- **`Illegible` is reachable only through a positive control** the engine had to read back
  from the same probe image, in a band appended to it, matched by position rather than by
  string, and **sized from the smallest box the redaction covered**. A control drawn at 48 pt
  proves an engine reads 48 pt and says nothing about the 6 pt footnote that was redacted.
- **`RedactedPixels` can only be constructed from an `Illegible` verdict**, so §6's rule that
  OCR runs only on already-redacted pixels is carried by the type rather than by a comment ---
  the same move `worker.rs` makes with `PreWorker`/`WarmWorker`.
- **OCR does not run in the parser worker, measured rather than argued.** Vision under
  `SANDBOX_PROFILE` is killed by SIGTRAP; with all of `/System/Library` readable it fails with
  `nilError`; it needs general `file-read`, which is what the profile most needs to withhold
  from a process parsing a hostile document. It does not need that boundary --- a recogniser
  consumes a fixed-size RGBA buffer we rendered, not attacker-authored structure --- so
  `OCR_SANDBOX_PROFILE` keeps no-network and no-writes, in a separate process because the
  first rung is an engine aborting its host. `scripts/vision_sandbox_probe.swift` reproduces
  the ladder; it applies the profile post-launch, because `sandbox-exec` would apply it before
  `exec` and die in the loader instead.
- 13 tests, all shown to fail: 8 mutations, 8 caught, and each tripped the test predicted for
  it before the run.
- Two new traps, index now 170: *"A control that is easier than the check certifies nothing"*
  and *"macOS Vision cannot run in the parser worker's sandbox, and it aborts rather than
  refusing"*.

### Measurement --- the macOS half of the boundary cost, and a cross-check that disagreed

- **`latency-bench` runs on macOS**, closing the "compiles but has never executed there"
  qualifier it shipped with. Expected shape reproduced exactly on all three fixtures --- 3/3,
  3/3, and 3/4 with the documented `[SKIP]` on `vector-heavy` --- exit 0 throughout, and its
  four mutations re-proved here rather than taken on trust (4/4 caught, control green first,
  file restored by bytes and verified by digest against `HEAD`). The production worker's
  per-tile boundary cost is **0.071--0.103 ms** on macOS against 0.269--0.309 ms on Windows,
  ~3.5x rather than the 1.5--1.8x the other render constants differ by.
- **No sandbox font substitution.** The Mac run existed partly to check this, since a
  sandboxed PDFium has previously substituted fonts silently while still returning `ok`.
  In-process and worker renders agree to within 0.25% on all three fixtures.
- **The cross-check against `worker-bench --mode latency` disagreed by an order of
  magnitude, and the older harness was wrong.** Its `transport` is a residual and it
  baselines on `ping`, a variant that never renders, so the render-noise floor is left in
  the answer. On `text-base14` the subtraction error is 0.014 ms against a reported 0.015 ms;
  on `vector-heavy` it is 46.7 ms against a reported 46.6 ms, and the correctly baselined
  figure goes **negative**. `worker-bench` now prints its in-process residual and the
  `inproc`-baselined figure beside the two `ping`-baselined ones, and warns when the error is
  as large as the answer --- which is every fixture measured so far. Proved able to stay
  silent, because a warning that cannot not-fire is a constant.
- **The affected number was already hedged where it was written, and the hedge had not
  travelled.** `docs/PLAN.md` §3 says the shared-memory figure "is indistinguishable from the
  in-process residual"; the same 0.11 ms is quoted flat in the Phase 0 verdict table, in
  question 10's answer, and in `docs/THREAT-MODEL.md`. All three now carry it. No conclusion
  moves --- the boundary is cheap on every version of the number, and the production figure is
  still ~30x under the 3.0 ms webview hand-off.
- New trap, index now 168: *"A baseline that skips the expensive step leaves its noise in
  the answer"*. `AGENTS.md`'s own count of its own index was six behind; the sentence now
  names `grep -c '^### ' docs/TRAPS.md` as the authority instead of asserting a number.

### Phase 0 --- feasibility spikes

All seven spikes have documented verdicts and the exit criterion is met. The evidence is
in `docs/PLAN.md`; the traps each one paid for are in `AGENTS.md`.

- **Render pipeline.** Raw pixels over the Tauri custom scheme, 1024²--2048² tiles.
  PDFium charges ~1 s per *render call* on a dense A0 page regardless of how small the
  request is, so small tiles multiply a constant rather than dividing the work.
- **Process architecture.** A worker boundary costs 6 µs of control latency and 0.11 ms
  to move a 4 MB tile through shared memory, against 3.0 ms to hand the same tile to the
  webview. Four workers give 3.9× throughput on a 4P+6E machine. macOS only.
- **Startup.** Warm, cold, and first-launch-after-build are three regimes; the last two
  are the OS. The shell floor is ~250 ms before any application code runs. Warm start is
  276 ms with lazy page geometry and a non-default menu, against a 300 ms target.
- **Text-object round trip.** Both routes reproduce the page with zero collateral pixels,
  but only surgical `lopdf` operator rewriting preserves marked content, and only it
  detects an out-of-subset character instead of silently drawing `.notdef`.
- **Sanitized full rewrite.** A collected `lopdf` rewrite matches QPDF on every hostile
  fixture, so QPDF is not required --- but `lopdf`'s own collection is quadratic and the
  mark-and-sweep has to be ours.
- **Incremental save.** The update section stays under a kilobyte whatever the document
  weighs, and beats a full rewrite 8.2× on disk at 337 MB. Signatures stay
  cryptographically intact and stop being trusted, at every DocMDP level.
- **Threat model.** `docs/THREAT-MODEL.md`, with the sandbox profile it implies. The
  vendored PDFium has no V8 and no XFA compiled in, so document JavaScript cannot run
  rather than being switched off.

Known failure, recorded rather than smoothed over: the A0 vector page sustains 60 fps
over a screen that is 0--4% sharp. Frame rate cannot distinguish a viewer that keeps up
from one that has given up, so the criterion now carries a coverage floor.

Closed in Phase 1 on 2026-07-27 --- by the progressive render API and stale-request
withdrawal, and **not** by the worker pool, which was measured before being built and
takes a screenful of that page from 8.19 s to 2.55 s rather than to anything scrollable.
Read the closure narrowly: the page never falls below its tier-1 placeholder, which is
what the criterion asks, and it stays 6--10% sharp while moving, which is not a good
experience.

### Added

- **Interactive form-field values are drawn by the cancellable renderer.** The raw PDFium
  path now retains a pinned form environment for the document, notifies it as cached pages
  open and close, and overlays `FPDF_FFLDraw` only after a complete base render. A cancelled
  tile remains an explicitly incomplete tile rather than receiving a complete widget over
  partial page pixels.

  A generated text widget with a value and no stored appearance stream makes the pass
  observable: before the fix the safe and progressive paths differed in 4,587 bytes; after
  it they are byte-identical both uninterrupted and through a forced pause/resume. The probe
  that proves it also now finds PDFium through the shared platform path --- on Windows it had
  still looked in macOS's `lib/` directory and told the reader to reinstall a valid DLL.

- **Text comes off a multi-column page in the order it is read.** A PDF carries no reading
  order --- only glyphs at positions, in whatever sequence its producer emitted them --- so a
  two-column page whose producer wrote line by line across the gutter copied as `alpha one
  beta one alpha two beta two`. `src/lib/reading.ts` recovers the order from the geometry by
  recursive XY-cut, which handles a heading spanning both columns as the same operation on
  the other axis. Wired into copy and into the screen-reader tree.

  Rotation is carried inside it: every rule is written over "along a line" and "across the
  lines", with the direction each runs derived from the backend's own coordinate mapping.
  Without the directions the order is right at 0° and 90° and exactly reversed at 180° and
  270°. On `rotated-90.pdf`, where PDFium extracts the lines backwards, 493 of 534
  characters now come out in a different --- and correct --- position; on a single-column
  document, none do.

  A drag still selects a contiguous range of character *indices*, so on such a page it takes
  in more than was dragged over. Making the drag geometric means carets carrying a reading
  position, which is a change to the selection model and is deliberately not in this.

  20 unit tests, 12 mutations, and two functional checks against a manifest written by the
  fixture's generator rather than by anything under test --- plus a differential one that
  needs no manifest, two pages laid out alike and emitted oppositely having to read alike.
  `viewer_check.py` is at **109 names** across seven corpora.

  Two existing checks rested on the assumption this removes, and were corrected rather than
  quietly relaxed; a third turned out to be decoration, and a precondition was wrong twice
  before it was right. All in `docs/TRAPS.md`.

- **Fit page, actual size, and a zoom you can type.** `⌘9` fits the whole page in the
  window, `⌘1` is 100%, and `⌥⌘Z` asks for a percentage through the same palette argument
  the page jump uses --- the zoom ladder is deliberately coarse, so 175% was previously
  unreachable. The toolbar's zoom readout is now the button that opens it, and its tooltip
  says what the zoom is following.

  Under it, the fit stopped being a boolean. Both fits have to survive a resize *and* a
  rotation, so the viewer remembers *which* one to re-apply rather than merely that it is
  fitting something; the old `fitting` flag is gone rather than kept beside the mode,
  including out of the session file, because two records of one fact drift. The arithmetic
  moved to `src/lib/zoom.ts`, which needs no DOM: 18 unit tests and 12 mutations, each
  caught by the test named for it. Six functional checks take `viewer_check.py` to **107
  names**, identical across all six corpora.

  One of those six could not fail, and the mutation is what said so --- it measured the page
  against the element's own width, which is 12 px wider than the width a page is fitted into,
  the scrollbar sitting in a gutter over that edge. The run still went red, through an
  *older* check, which is exactly why a count is not evidence for a new one. In
  `docs/TRAPS.md`.

- **A Windows distributable builds** --- an MSI and an NSIS installer from `npm run tauri build`.
  It did not before, and the cause is a rule about this repository's layout rather than a Tauri
  bug: **`src/bin/` must contain only declared bin sources.** The bundler enumerates that
  directory and registers the first entry no `[[bin]]` `path =` claims; a `.rs` file is always
  claimed, a *subdirectory* never is. So `src/bin/backend_probe/`, which held only `imp.rs`,
  became a phantom binary named `backend_probe` --- pointing at an executable that does not exist
  and colliding with the component id WiX derives from the real `backend-probe.exe`. Those two
  `imp.rs` bodies now live in `src/probes/`, reached by `#[path]`, which leaves module parentage
  and every `super::` in them unchanged.

  It had never been caught because Windows packaging had never been attempted. The installer does
  ship all 17 probe and benchmark executables, about 35 MB of spikes; that follows from declaring
  them `[[bin]]` in the bundled crate, is identical on macOS, and wants its own change.

- **A second launch on Windows hands its document to the running app**, as it does on macOS.
  It used to be a second process with its own window and its own worker pool ---
  `RunEvent::Opened` is macOS-only and nothing filled the gap. `tauri-plugin-single-instance`
  (Apache-2.0 OR MIT, no new crate refused by the licensing rule) now forwards the second
  process's argv to the first and exits it; the callback feeds the same `Launch` queue and emits
  the same `OPEN_EVENT` as every other route in, so there is one path for "open this document"
  rather than two that can drift. It also unminimises and focuses the window, because a handover
  that loaded the document behind whatever the reader was looking at would read as nothing having
  happened.

  `open_check.py` now runs **five of six** phases on Windows, up from four. Proved by mutation:
  disabling the plugin turns the phase red with *"nothing ever arrived"* while its control still
  passes. The remaining skip is the cold double-click, which is not a gap --- Explorer hands the
  path over in `argv`, already covered by the `argv` phase.

- **Printing works on Windows**, which was the last user-facing capability the platform lacked
  --- `present_job` returned `Err("printing is implemented on macOS only")`. `print_win.rs` reads
  the job back with `Windows.Data.Pdf`, the operating system's own PDF stack and the direct
  counterpart of the PDFKit readback on macOS: independent of the `lopdf` that wrote the job and
  the PDFium that drew what the reader saw, so it can attest that the output is readable by
  something else. It then rasterises each page onto a printer device context, because Windows has
  no in-box PDF print API at any layer and every Windows PDF viewer does the same. `PrintDlgW`
  runs the panel, on its own thread so the modal loop cannot freeze the window behind it.

  Windows output is therefore **raster at 300 dpi** where macOS is vector. Pages are requested
  from WinRT as BMP rather than the default PNG, which is what lets `StretchDIBits` take the
  bytes directly and keeps an image decoder out of the tree entirely.

  Adds the `windows` crate, and no crate to the dependency graph: it is already there
  transitively through Tauri's WebView2 stack, and it is `MIT OR Apache-2.0`.

- **`print-probe` drives the whole print path to a real spooler, without paper.** "Microsoft
  Print to PDF" is a real driver, and naming an output file in `DOCINFOW.lpszOutput` stops it
  raising a save dialog --- so everything except the panel runs unattended and the result is
  re-read by the OS parser. 8/8 checks. It asserts **ink** rather than a page count, because a
  wrong `BITMAPINFO`, a DC in the wrong mapping mode and a bad blit rectangle all produce the
  right number of blank sheets; mutating the blit away leaves the page count green and only the
  ink red. It also reads its own module table: 80 modules mapped, none named pdfium, with
  `Windows.Data.Pdf.dll` named beside it as what *is* mapped --- printing parses in the app
  process on both platforms, and what the boundary buys is that the parser doing it is not ours.

  It found a defect in the code it was written to check, which is the point of writing it:
  **every page was printed at half physical size.** A DIB rendered at 300 dpi was placed onto a
  600 dpi printer DC unit-for-unit, and for a page small enough that the fit-scale never engages
  there was nothing to correct it --- a wide even margin that looks deliberate. The probe's
  original oracle, printed ink over sent ink with an order-of-magnitude band, read `0.49` and
  passed; the same formula then failed at `0.01` on an A0 page purely because the paper is 16×
  smaller in area. What holds for both is predicting where the ink should land, from the source
  page's extent scaled by the page-to-sheet ratio: 1% error on the reference run, 48% against the
  reverted bug.

- **Large-format pages no longer allocate half a gigabyte per sheet.** `PRINT_DPI` was applied
  relative to the *page*, so an A0 page rasterised to 9933x14043 --- 532 MB as BGRA --- for a
  sheet that can show 9 MB of it, and `print-probe` on twelve A0 pages did not finish in two
  minutes. Pages now render at the resolution that yields 300 dpi *after* the fit to paper. The
  constant's own doc comment had done the arithmetic for A4, which is the page size that makes it
  look reasonable.

  What remains, measured and not a defect: one A0 page of 200,000 vector operations takes
  **2m51s**, nearly all of it inside the OS rasteriser and largely independent of resolution. A
  raster print path inherits that, and macOS avoids it entirely by handing vectors to
  `NSPrintOperation`. Avoiding it here needs `IPrintDocumentPackageTarget`, which GDI cannot
  express; not started.

- **Three of `print.rs`'s four third-parser checks now run on Windows**, taking `cargo test
  --lib print::` from 14 checks there to 18. They were `#[cfg(target_os = "macos")]` because
  PDFKit used to be the only independent parser available, which said nothing about the property
  under test --- so printing, the one subsystem whose output leaves the process, had no
  independent readback check on Windows at all. Shown to buy real coverage rather than merely
  existing: breaking `effective_rotation` turns both rotation checks red here, including
  `rotated.pdf`'s which-pages-survived case. The fourth needs per-page text, which
  `Windows.Data.Pdf` has none of, so it pins the page count and prints a `[SKIP]` naming the gap.

- **The job object's own two limits are measured**, having been claimed by `win-sandbox-probe`'s
  table since it was written and probed by nothing. Its three authority probes are all
  integrity-level properties, so every rung reported on `lowil` and above while
  `JOB_OBJECT_LIMIT_PROCESS_MEMORY` and `ActiveProcessLimit` went unexercised. With the
  uncontained rung as the control: `bare` commits 1 GB and starts a second process; every rung
  with a job is refused with `1455` (commit charge) and `1816` (process quota). Windows charges
  *committed* memory, so a bomb is refused before a byte of it exists --- a step earlier than the
  resident-memory polling macOS is limited to.

- **A Windows worker renders, contained.** `Worker::spawn` builds one off macOS for the first
  time: created suspended, dropped to low integrity, assigned to its job object before it
  executes an instruction, then handed two pipes and the document and tile sections as
  inherited handles named in argv. `worker-probe` is the proof --- **11/11 checks** on
  `text-base14`, `text-cid`, `vector-heavy` and `rotated`, tiles **pixel-identical** to the
  in-process render, plus text extraction, outlines and search across the boundary. The font
  substitution the macOS sandbox caused did not recur, as `win-sandbox-probe` predicted.

  `Worker` carries both platforms as per-platform **type aliases**, not an enum: a `Contained`
  where macOS has a `Child`, a `File` where it has `ChildStdin`/`ChildStdout`. Two methods have
  two bodies (`pid`, `epitaph`) and the rest are unchanged, so every macOS line in `worker.rs`
  is byte-identical --- which matters because none of this can be re-verified on macOS from a
  Windows machine, and a diff touching only Windows code is the strongest available statement
  about what cannot have regressed.

  Three findings came out of testing it rather than writing it. The parent must close its copy
  of the reply pipe's write end or a dead worker is indistinguishable from a slow one --- and
  the check for that has to bound its own wait, because the failure it looks for *is* a hang.
  An epitaph asked the instant a pipe reaches EOF says **"still running"**, since handles close
  before the process object is signalled; `Contained::epitaph` now waits 100 ms, and liveness
  polling still does not. And `TerminateJobObject` exited with `1`, indistinguishable from a
  worker failing on its own, where unix has "killed by signal 11" to say otherwise --- so a kill
  now uses a customer-flagged NTSTATUS the epitaph names in words.

  Pre-spawning is unimplemented there and says why --- a Windows child is given its document at
  `CreateProcess`, so one started before a file is chosen has nothing to be handed.
  `Worker::spawn_shared` takes every open instead, at the ~6.6 ms macOS saves.

- **`backend-probe` runs on Windows, and passes.** The probe was `#[cfg(target_os = "macos")]`;
  its four platform primitives now have Windows bodies --- Toolhelp for its own module list and
  for finding its worker children in the process table, `GetProcessHandleCount` for descriptors,
  and `TerminateProcess` for a hostile kill from outside the pool, which is deliberately not
  `Contained::kill` because the pool has to notice a death it did not cause.

  **36/41, 5 skipped** on `text-base14`; **39/41, 2 skipped** on `vector-heavy`, where a tile is
  slow enough for the withdrawal checks to run rather than skip. No failures. The boundary, the
  pixel comparisons, capacity, crash restart, replacement, retirement, close and descriptor
  return all pass, and the 41 check *names* are unchanged, which is the cross-platform invariant.

  **The two failures it first reported were its own, and the correction is the point.** They read
  as a pool grown to six that keeps one, beside a handle count that never moved --- two
  independent observations agreeing on "created, used and destroyed rather than pooled", which
  was written into three documents as an open defect. Both were honest and neither could say
  *when* it was taken: `settled_descriptors` waits up to five seconds for a pre-spawned spare,
  Windows has none, and the wait's verdict was discarded, so it spent its whole bound every call
  --- longer than the four-second idle timeout the phase runs at. The instrument retired the pool
  and then measured it. The spare clause is now asked for only where a spare can exist, under a
  single named `PRESPAWNS` shared with the spare-lifetime skip, and a wait that expires prints a
  `[WARN]` instead of passing for a slow one. **Nothing in `workers.rs` changed.**

- **Windows pre-spawns workers too.** A worker can now be started, contained and warmed before
  a file is chosen on both platforms. The handover is the only part that differs: macOS sends a
  descriptor as `SCM_RIGHTS`, Windows `DuplicateHandle`s the document section **into the running
  child's handle table** and sends a `Handover` line naming the number it wrote --- the direction
  integrity levels permit, so it crosses the boundary structurally rather than by luck. A message
  of its own rather than a `Request` variant, which makes a second handover unsayable instead of
  something the child must refuse. Containment is unchanged and unconditional: the child is
  created suspended, dropped to low integrity and put in its job before it executes an
  instruction, whether or not it has a document yet.

  Measured with `prespawn-bench`: **8.4--9.6 ms saved per open**. The saving has a different
  *shape* from macOS and that is the finding --- there ~7.4 ms of it is the system-font walk, here
  ~1.4 ms is, so on Windows pre-spawning buys the fixed floor (`CreateProcess`, the loader,
  mapping `pdfium.dll`, the token, the job) rather than font enumeration. First Windows numbers
  in the repository, labelled as such.

  `backend-probe` now runs the spare checks there: **37/41** on `text-base14` and `text-cid`,
  **38/41** on `outline-hostile`, **40/41** on `vector-heavy`, no failures, with the spare
  identified and excluded from the pool at open and taken with its service at the end.
  `viewer_check.py` re-run on four corpora, since this changes the app's own behaviour --- all
  green, 44 modules at peak, no `pdfium` among them.

  Two things it broke on the way, both in checks rather than in code. `closing gives back every
  descriptor opening took` went red at *137 / 145 / 142* --- one spare's worth --- because its
  three samples were raw and an `open` starts a replacement spare on another thread; macOS was
  winning that race and Windows does not, so they go through `settled_descriptors` now. And the
  test asserting that `prespawn` refuses on Windows failed on its own, which is the evidence the
  behaviour changed; it is replaced by one pinning `PRESPAWNS` against what `prespawn` actually
  does, proved able to fail by restoring the stale value.

- **`pool-bench`, `prespawn-bench` and `tile-bench` run on Windows.** The first two gated the
  `--render-worker` re-exec on `#[cfg(unix)]`, left from before `worker_child` compiled there; a
  binary that re-execs itself as a worker and then refuses to be one is not degraded, it is
  unrunnable. All three hardcoded `vendor/pdfium/lib`, which on Windows exists and holds the
  import library, so the failure lands at `LoadLibraryExW`. `tile-bench` also gained a real
  `peak_rss_mb` there (`GetProcessMemoryInfo`/`PeakWorkingSetSize`) in place of `NaN`, keeping
  the `NaN`-on-failure contract.

  **`tile-bench` had never refused anything** --- the documented list of four blocked binaries was
  wrong about two of them, in the direction a list written by reading always errs. `worker-bench`
  is the one real refusal, and its reason is accurate: its own POSIX worker, fd passing and SBPL
  bisection, sharing no mechanism with the job-object model.

- **The render constants are measured on Windows, and `docs/PLAN.md` §4 holds with worse numbers.**
  Same generated A0 fixture, same PDFium pin. Spatial culling intact --- a 256² tile is **3.8%** of
  a full render against 4.3% on macOS --- and the per-render floor is real but larger: **~1.3 s**
  against ~1 s, with a full page at **35.1 s / 88.3 s** for 1x / 2x against 22.8 s / 48.4 s. The
  ratios that drove the architecture reproduce; every absolute number is **1.5--1.8x worse**, so a
  latency budget written against the macOS figures is optimistic here by about a third. Peak RSS
  532 MB. Cross-checked against `backend-probe`'s independent 1536 ms 512² render of the same
  document on the same machine before being believed. The cheap-page half is flat (0.6--0.9
  ms/Mpixel, no floor), which confirms the asymmetry the plan bets on but is **not** a
  cross-platform comparison --- macOS measured a fixture this machine has not generated.

- **A pool buys a screenful 3.6x on Windows, and nothing past six.** `pool-bench` on six 1024²
  tiles of the A0 page: monotone gains to six workers and nothing at eight, the same shape as
  macOS's 3.22x-and-nothing, with six stable to 0.01x across two runs. The intermediate sizes are
  reported as **not** conclusions --- pool 4 moved 1.99x to 2.29x between identical runs and the
  per-round warm figures span 20%, so only the six and the flat eight are outside the spread.

- **A dying worker's diagnostic is one write.** Rust's stderr is unbuffered and `write_fmt`
  issues a write per format piece, so with every worker of every pool inheriting one handle the
  pieces interleave --- a `pool-bench` run of ~120 workers ended holding `[worker] ` with no
  message, which is indistinguishable from a worker that failed with an empty reason and is the
  one thing that line exists to rule out. It is a `format!` and a single `write_all` now, verified
  by making a worker fail and reading its message back. Every error path reaching it produces
  non-empty text, checked; the fragment did not recur on a stderr channel of its own, which is
  also why the capture channel is now part of the trap.

- **`worker-bench`'s refusal named a Windows design that was measured out.** It cited "restricted
  tokens" and "named section objects"; `win-sandbox-probe` established that a restricting SID
  stops the loader before `main`, so containment is a low-integrity token in a job object, and the
  sections are anonymous because a name is something another process can open. A wrong reason on a
  refusal is worse than a vague one --- it is a design instruction, and someone reading it to build
  the spike would have built the two rejected things. It now also says what a spike would measure
  that nothing else does. Two stale `// The child half exists only on unix` comments removed.

- **The threat model's strongest claim is unverified on Windows, and now says so.**
  `worker-bench --mode engine` is the check behind "there is no engine to disable" and "XFA is not
  built in". It spawns nothing --- it reads the library file --- yet sat inside a `#[cfg(unix)]`
  module, so it had never run on Windows and the claim was untested there rather than merely
  unmeasured. Moved to file scope; on Windows it reports **`[NOT VERIFIED]`**, because the shipped
  `pdfium.dll` carries no local C++ symbols (`CPDF_Document` absent), so `v8::` and `CXFA_` being
  absent from it means nothing. That is the harness's second control working as designed.
  `docs/THREAT-MODEL.md` now scopes both claims to macOS and states that on Windows they rest on
  the asserted asset name and pinned digest --- a claim about which file was fetched, not about
  what is in it.

  It also reads the **PE export table**, the one dimension stripping cannot hide, and prints it
  *before* the stripped-binary exit rather than after --- the run that most needs it was showing
  nothing. **460 exports, four XFA-named**: `FPDF_LoadXFA` and `FPDF_GetXFAPacket{Count,Name,
  Content}`. Surface, not a contradiction --- the three `GetXFAPacket*` calls read `/XFA` streams
  out of an AcroForm dict and need no XFA engine. Whether `FPDF_LoadXFA` is a stub is open, and
  unlike JavaScript it is behaviourally decidable: an `/XFA` fixture makes
  `FPDF_GetXFAPacketCount > 0` a positive control. The old text said the property "cannot be
  tested behaviourally", which is true of JS and over-generalised to XFA.

  Both counts cross-checked against an independent Python PE parse before being written down, and
  every branch exercised: non-PDFium `[FAIL]`s, a non-PE file that passes both controls says "not
  a PE image" rather than printing a zero, a missing `--lib` exits 2, other modes still refuse.
  The bump checklist in `BUILD.md` had the macOS-only `vendor/pdfium/lib` hardcoded; it now names
  both platforms.

- **The last two macOS-only harnesses run on Windows, and one needed nothing.**
  `session_check.py` passed all four phases on the first attempt, both controls included --- it
  takes a binary rather than a bundle and `webview_guard` already returns early off darwin, so
  there was nothing to port. `open_check.py` needed a real port and now runs **four of its six
  phases** there (`argv`, `beats`, `control`, and all four launches of `race`). The two that
  cannot print `[SKIP]` with the reason, so the phase-name list is identical on both platforms.

  Those two skips record a **measured platform divergence** that was previously unstated in
  either direction: `RunEvent::Opened` is macOS-only and no single-instance plugin is linked, so
  on Windows **a second launch is a second process** --- two `tpdf.exe`, two windows, two worker
  pools, where macOS produces one app that swaps documents. Whether that is the behaviour to want
  is a product decision; the *emit* branch `running` exists to exercise is simply unreachable
  there. `HANDS_OVER_TO_RUNNING` is the one place that distinction lives, and each branching
  phase name is a constant rather than a literal at both call sites.

  That makes **four** documented-blocker lists found wrong this week, always by over-reporting.
  The trap now carries the tally: of six benchmarks and harnesses listed as macOS-only, two were
  genuinely gated, one was trapped behind a `cfg` it never needed, one had only a hardcoded path,
  one needed nothing at all, and one was two-thirds portable.

- **A harness that prints as it goes wrote nothing until it exited.** `BUILD.md` claims these
  scripts stream their results so a partial run names where it stopped. True in a terminal, false
  under a redirect --- Python block-buffers stdout off a tty, so an `open_check.py` run held **zero
  bytes** for twelve minutes, indistinguishable from one that died at import. `scripts/live_output.py`
  makes all three line-buffered explicitly, called rather than left as an import side effect.
  A/B'd at the same four-second mark: **0 bytes against 38**. The hazard was already written down
  as a caution in the cross-repo memory and had been read in the same session --- which is the
  argument for making it a line of code instead.

  It paid for itself the same hour. With streaming, an `open_check.py` run finished in **45 s**;
  the attempt immediately before it, without, sat at zero bytes for **17 minutes** while the app
  it launched held **0.00 CPU** --- hung at the first phase. Both harnesses need a clean process
  table on Windows: a leftover `tpdf.exe` hangs the next run, reproduced twice and cleared both
  times by killing the strays. `webview_guard` returns early off darwin, so nothing guards
  occlusion there, and the tell is the CPU figure rather than the clock.

  `worker_pids` matches a child on its **image name** there rather than on argv, because
  Toolhelp reports a parent pid and an image but no command line. Weaker, and sufficient for a
  stated reason rather than an assumed one: the `caffeinate` shape that forced the argv match is
  a macOS wrapper with no Windows counterpart, and the `--spare-lifetime` child is never started
  because pre-spawning is unimplemented. That check now skips with that reason instead of
  failing.

  `tpdf_lib::PDFIUM_SUBDIR` and `PDFIUM_LOADABLE` are public, because the "`lib/` exists on
  Windows and holds the wrong thing" trap had by then cost two binaries on two separate days.
  Four spike binaries still hardcode `lib`; the next one ported takes the constant.

- **Windows stops failing open.** `Backend::default_here()` selects workers on both platforms
  that have a boundary, which is now macOS and Windows rather than macOS alone. One word of
  code; the rest of the work is the evidence, because the `[WARN]` this replaces was our own
  bookkeeping and removing the line that prints it would have looked identical to fixing it.

  `scripts/win_modules.py` reads the app process's loaded module list from **outside** it,
  through Toolhelp, and `viewer_check.py` now launches the app rather than blocking on it so it
  can sample throughout the run and take the union --- the parser is mapped only while a
  document is open, so one look could miss it either way. The module count is printed beside
  the verdict: an enumeration that read *nothing* reports "not mapped" exactly as containment
  does, so a peak of zero is a broken observation and never a pass.

  Run **before** the change it reported the parser mapped, 47 modules at peak. That control is
  why the pass afterwards means anything. After: `outline-simple`, `outline-hostile`,
  `rotated-90` and `vector-heavy` all green with unchanged ran/skipped splits (81/5, 81/5,
  75/11, 52/34), no `[WARN]`, and 44--45 modules at peak with no `pdfium` among them.

  Both `render.rs` tests went red on the one-word change, which is what they are for. One named
  macOS as the only platform with a boundary and now states the platform list independently of
  the code --- deliberate duplication, since sharing a predicate would make it agree with
  whatever the code said. The other stopped naming a platform at all: the uncontained mark is on
  the timeline exactly when the default is the uncontained backend. Mutating the mark to be
  recorded unconditionally fails it; mutating it to never be recorded does **not**, on either
  platform that now has a boundary, because the branch no longer executes --- measured, and
  written down rather than left to be rediscovered.

- **The viewer runs on Windows**, and `viewer_check.py` passes there unmodified. Four corpora,
  each reporting the **86 check names** that are the invariant, with ran/skipped splits inside
  the macOS ranges: `outline-simple` 81/5, `outline-hostile` 81/5, `rotated-90` 75/11,
  `vector-heavy` 52/34, no failures. The harness needed no porting --- `webview_guard` already
  returns early off darwin, and WebView2 wants no bundle identity, so a plain `tpdf.exe` runs
  where macOS needs an `.app`. Windows is still **not supported**: it has no containment, the
  backend falls back to in-process, and it fails open rather than refusing.

- **The uncontained backend announces itself.** Off macOS `Backend::default_here()` falls
  back to in-process, and until now nothing recorded that a document had been parsed in the
  app process --- the refusal in `Worker::spawn` guards `TPDF_BACKEND=worker`, a path the
  default never takes. It now records `render::UNSANDBOXED_MARK` on the startup timeline and
  prints a `[WARN]`, once per process. Visibility, not containment, and deliberately not a
  refusal: refusing would make Windows useless rather than uncontained, which is a product
  decision rather than a defect to fix in passing. It matters more now that the viewer
  actually works there.

  The check asserts both halves from one run --- marked where there is no sandbox, *not*
  marked where there is --- because either alone passes with the code wrong. Two mutations:
  removing the mark turns it red; recording it unconditionally **survives on Windows**, and
  that is stated rather than hidden, since the assertion that would catch it is in the macOS
  branch and no run on this machine reaches it.

- **`sandbox_win`: the containment the probe measured, as a module.** A job object (memory
  cap, one process, kill-on-close, die-on-unhandled-exception) and a low integrity level,
  applied to a child that is created **suspended** so the job exists before the child runs an
  instruction --- assigning a job to a running process is a race the process can win, and a
  limit that is usually applied in time is not a limit.

  It fixes the shortcut `bin/win_sandbox_probe.rs` documented in itself: inheritance is
  narrowed by `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` to an explicit set rather than handing the
  child every inheritable handle the parent holds. Marking handles inheritable and naming
  them in the list are two halves of one decision, so `spawn_contained` does both and neither
  can be forgotten separately.

  Two checks were written asserting the opposite of what Windows does, and both were
  corrected by running the call rather than by reasoning: an empty handle list is refused
  (`ERROR_BAD_LENGTH`), so "inherit nothing" is modelled as `Option<AttributeList>` and
  reaches `CreateProcess` as `bInheritHandles: FALSE`; and a zero memory cap is refused by
  the kernel (`ERROR_INVALID_PARAMETER`), not silently accepted. `Job::assign` and
  `make_inheritable` are `unsafe`, because a safe function over a raw `HANDLE` hides a real
  liveness obligation --- a recycled handle value applies the operation to someone else's
  object rather than failing cleanly.

  `WORKER_MEMORY_CAP` is a **real** kernel bound, which is the one place the Windows story is
  stronger than the macOS one: `docs/THREAT-MODEL.md` §T3 records that macOS refuses every
  relevant rlimit and the substitute poll can bound a leak but never a burst. Nothing calls
  this module yet.

- **The worker's child half compiles on Windows**, and a contained child can check that it
  *is* contained. The module was `#[cfg(unix)]` and `lib.rs` refused `--render-worker`
  anywhere else; both are gone. Exactly three functions knew the platform and each is now one
  function with two bodies rather than the module being absent: `adopt_tile` and
  `adopt_document`, because macOS inherits a mapping on a number agreed before `exec` while
  Windows inherits a handle whose *value* has to be told to the child in argv; and
  `establish_boundary`.

  That last one is the asymmetry worth stating. macOS **applies** `sandbox_init` and fails
  loudly if it cannot. Windows has nothing left to apply --- the token is chosen at
  `CreateProcess` and is in force from the first instruction --- so it **verifies** instead:
  `integrity_level` reads the process's own mandatory label, `in_any_job` answers
  `IsProcessInJob`. Neither is sufficient alone, and the second is not sufficient even with
  the first, because a debugger or a terminal host puts a process in a job for reasons of its
  own; a `false` disproves containment and a `true` does not prove it.

  A handle may travel in argv where a path may not: the value means nothing in another
  process and inheritance is what makes it live, so it grants nothing, whereas a path would
  be authority a low-integrity child could act on --- low integrity governs writes, not reads.
  Parsed as `usize`, tested with a value above `u32::MAX`, because narrowing would not fail,
  it would produce a *different* valid-looking handle.

  Deleting the `cfg(not(unix))` refusal is the part that needed proving rather than the port.
  It was never load-bearing; `establish_boundary` is, and now has a test that an uncontained
  process is refused. That test does not run on macOS, deliberately: there the call would
  *succeed*, leaving every later test in the process inside a sandbox with no filesystem and
  failing for reasons unrelated to what they assert.

  The containment policy is a pure function of the two facts, and that split is a finding
  rather than tidiness. Written as one function it could not be tested --- a test runner fails
  the integrity clause and returns, so the job clause was unreachable and deleting it outright
  passed every test. Two further mutations were **identities** and read exactly like missing
  coverage: a mandatory-label SID has one sub-authority, so indexing its first instead of its
  last changes nothing; and `!=` versus `>` on the level differ only for a level *stricter*
  than low, which is why a test now asserts untrusted integrity counts as contained.

- **A contained child gets pipes, and one was heard.** `spawn_contained` takes an optional
  `Stdio` and sets `STARTF_USESTDHANDLES`. `STARTUPINFO` cannot say "this stream, leave the
  others alone" --- the flag makes the child take all three --- so a stream left null is a child
  with no stderr rather than a child with the parent's, and `Stdio` has no per-stream
  `Option`. stderr is shared rather than piped for the reason `worker_child.rs` gives:
  nothing is reading a third pipe at the moment a worker dies.

  The stdio handles are folded into the inherit list by `spawn_contained`, not by the caller;
  with a handle list present, a standard handle the child is told to use and that is not in
  the list is simply not inherited. `pipe()` marks **neither** end inheritable, because
  `CreatePipe`'s security attributes would mark both and the end the parent keeps must never
  reach the child --- a worker holding the read end of its own reply pipe can watch every
  answer it gives.

  The test is the result rather than the code: `cmd.exe` at low integrity inside a job runs
  and a known string comes back. One test rather than four, because the four failure modes
  are indistinguishable from outside --- wrong flag, handle missing from the list, handle not
  inheritable, parent's copy of the write end left open --- and the last is a hang, not an
  error.

- **A parent can watch a contained child**: `try_wait`, `kill`, `wait_timeout`, `epitaph`,
  matching what `std::process::Child` offers on the other platform. Two findings, both in
  `docs/TRAPS.md`.

  `GetExitCodeProcess` reports `STILL_ACTIVE` for a live process, and `STILL_ACTIVE` **is
  259** --- an exit code any process may legitimately choose. Telling the two apart by value is
  wrong for exactly that one input, and a worker that really exited 259 would read as running
  forever while the pool waited on something already gone. Liveness comes from
  `WaitForSingleObject` with a zero timeout instead, and the code is read only once that has
  answered.

  The lifecycle test could not fail, and how that surfaced is the more useful half. Mutating
  `kill` into a no-op did not turn it red --- it made the run take **177 seconds** against a
  180-second harness timeout, and the harness printed `test result: ok` and `[HUNG]` in the
  same output without noticing those contradict. The assertion was "kill, then wait for the
  exit code", and an unbounded wait has two outcomes: pass, or block forever. A blocked test
  is not a failing test. With `wait_timeout` the same mutation fails in 10.02 seconds and
  names the test.

  Still nothing spawns a Windows worker: `Worker` holds a `std::process::Child`, and these
  are the pieces its Windows half will be built from.

- **`Shm` is real on Windows.** Every off-unix constructor previously returned "render
  workers are implemented on macOS only" --- which reads like a containment decision and was
  the absence of an implementation wearing the language of a policy. It is now a nameless
  section object: `CreateFileMappingW` with a null `lpName`, so it is reachable only through
  a handle, which is the same property the unlinked temp file buys on the POSIX side. A
  section holds its own reference to what backs it, so `map_file` closes the file handle and
  a child needs only the section --- there is no Windows analogue of passing a descriptor
  alongside.

  `raw_fd` is the one method not carried over: a `HANDLE` is pointer-sized and an `i32` is
  not, so returning one would truncate on 64-bit into a value that still looks like a
  plausible descriptor. `raw_handle` and `from_handle` replace it.

  Four mutations. Swapping the halves of the 32-bit length split turns three checks red;
  making the document mapping `PAGE_READWRITE` turns two red with `ERROR_ACCESS_DENIED`,
  because Windows refuses a writable section over a read-only file handle exactly as `mmap`
  refuses `PROT_WRITE` --- which is what makes an otherwise unprovable property provable
  without a faulting write. Stripping `FILE_MAP_WRITE` is caught too, but as a
  `0xC0000005` process death with **no** `test result:` lines at all, which is why the run
  was checked for positive evidence rather than grepped for `FAILED`. The fourth survives:
  reversing the drop order changes nothing, and the comment claiming it leaked was simply
  wrong. The order is kept and the comment now says no test pins it.

  `Worker::spawn` still refuses off macOS, which is the refusal that was always about the
  sandbox rather than about missing code. Its check needed a real fixture to keep meaning
  that: it passed `"nonexistent.pdf"`, which was fine while every constructor refused
  identically and would have gone red at "could not open" the moment `map_file` worked.

- **What Windows containment can be, measured** (`bin/win-sandbox-probe`). macOS gets its
  boundary from `sandbox_init`; Windows has no counterpart, so containment there is assembled
  from a job object, an integrity level and a restricted token, and which combination still
  lets PDFium render is not documented anywhere. Six rungs, each rendering the same tile in a
  re-exec'd child and compared **pixel for pixel** against an in-process render --- pixels
  because the macOS work already caught a sandboxed PDFium returning `ok` while silently
  substituting a typeface, and the default fixture is `text-base14.pdf` because base-14 faces
  are not embedded and must be found on the system.

  **A job object plus low integrity is the answer**: byte-identical output, and the child
  loses the authority to write `%USERPROFILE%` or `OpenProcess` the parent. It does not lose
  *reads* --- an integrity level governs writes --- which is why the child is handed its
  document and its output as inherited handles and never a path, the Windows analogue of the
  macOS `dup2`. A restricting SID is the stronger rung and dies in the loader at
  `STATUS_DLL_NOT_FOUND` before `main`, needing Chromium's initial-token / lockdown-token
  handover to reach.

  Two rungs are marked diagnostic and excluded from the verdict, because with `restricted`
  failing either ingredient was a plausible cause and one row cannot attribute it; the
  restricting SID turned out to be the whole cause. Excluding them was not cosmetic --- the
  verdict took the last row that worked, which was the one that denies nothing.

  Both mutations went red at the assertion aimed at: disabling handle inheritance broke the
  control and the probe said so in those words, and flipping one byte of the child's output
  turned `identical` to `NO` and reported a one-byte difference. **Nothing uses this yet** ---
  `RenderService` still selects in-process off macOS, so Windows fails open exactly as before.
  `windows-sys` is MIT/Apache and was already in the tree transitively, so the dependency adds
  no crate; checked with `cargo metadata` rather than assumed.

- **A `bins` gate** (`cargo build --locked --bins`), because none of the other gates links a
  binary. `scripts/gates.py` reported 7/7 while `npm run tauri build` failed on the same tree:
  clippy stops at metadata, and `cargo test` links each `[[bin]]` with `main` replaced by the
  harness's own, so `backend_probe.rs`'s two unguarded dyld symbols were dead code the linker
  dropped. Proved to fail before being trusted --- red in 5.7 s against the un-gated file, in
  the debug profile, checked separately from the release observation because the finding is
  precisely that linking depends on how the target was built.

- **Five checks on the tile origin** (`src/lib/tiles.test.ts`), asserting both platform
  spellings. Four mutations, each matching its prediction: hardcoding the macOS scheme in the
  URL turns one red, in the origin three, dropping the memo one, and encoding the whole path
  two. The mutation harness's own cross-check fired on its first run --- it parsed twice as
  many failures as vitest's summary, because `FAIL ` matches the file-level block as well as
  each test --- and reported a broken run rather than either number.

- **A print job is now checked against documents `lopdf` did not write.** Every other test
  of `print::build` feeds it a fixture the same serialiser produced, so the module was a
  writer tested against its own reader with only the read-back independent --- and printing
  is the one subsystem here whose output leaves the process. The new check builds a subset
  with a quarter turn from five generated corpora, takes both the page list and the expected
  rotations from PDFKit rather than from `lopdf`, and skips loudly per fixture, naming for
  each whether its rotations discriminate *which* pages survived or only the count.
  `rotated.pdf` is the one that does, carrying four rotations on four identical pages.

  Shown to fail by three mutations: a composed rotation that ignores what the page carries
  (`[90, 90]` where `[90, 0]` is right), a selection that keeps the first N pages instead of
  the ones asked for (`[90, 180]`), and a `/Count` left contradicting its `/Kids` --- which
  every `lopdf`-side check passes while PDFKit reports four pages for a two-page job, the
  two real ones and two it manufactures to satisfy the count.

- **`scripts/open_check.py` drives two overlapping opens** (`race`), the case `openPath`'s
  queue exists for, issued from inside the app because Launch Services hands over one
  document at a time. Four cold launches, since repeating the round *within* a launch was
  measured and is worse --- only the first round is cold, and the rest run against warmed
  workers and land in the same order every time.

- **A per-request deadline on worker calls** (`TPDF_CALL_MS`, default 30 000 ms; zero means
  zero, unreadable values fall back). A request that does not answer within the deadline now
  kills its worker and returns an error --- previously it held one of the pool's service
  threads forever, a handful of such requests wedged rendering for every open document, and
  `close` hung on its drain. The kill is announced (`[render] worker <pid>: no reply in
  <n> s; killing it`) and is not retried: a crash retry costs milliseconds, a deadline retry
  costs another deadline of a service thread. `docs/THREAT-MODEL.md` T3 is corrected to
  match what ships --- the deadline is wired, `RLIMIT_CPU` is measured and deliberately not
  set, and the footprint poll is measured and *not* wired, which the section previously
  stated as an operating mitigation.

- **A text layer** (`src-tauri/src/text.rs`), which selection, search and the accessibility
  tree will all read --- one extraction rather than three that disagree. It carries one
  Unicode scalar per PDFium character index and no string: `FPDFText_GetText` extracts UCS-2
  and drops characters it cannot represent, so its string and the indices the boxes are keyed
  by diverge on exactly the documents nobody tests with.

  Extraction costs **1.42 ms** on a 2,725-character page with the page already loaded, and
  **43.2 ms** on the A0 sheet where almost all of it is `FPDF_LoadPage`. That sheet has zero
  extractable characters, which search will have to say out loud rather than return nothing.
- **Text selection and copy.** Drag to select, across pages; Cmd-A for the page, Escape to
  clear, Cmd-C to copy. Highlights are drawn on an overlay canvas above the tiles, so the
  class that owns the tile cache does not also have to know what a selection is. A copy waits
  for any page whose text has not arrived --- a fast drag can reach the clipboard before the
  extraction does, and silently copying the loaded part is a bug found in someone else's
  document.

  **Double-click selects a word and triple-click the line it is on**, and a drag begun with
  either extends by whole units instead of dropping back to characters --- which is what makes
  dragging out a sentence land on word boundaries the way it does everywhere else. A fourth
  click has no larger unit to reach for, so it wraps back to a caret rather than repeating the
  line.

  Word edges are runs of letters, digits and combining marks: correct wherever words are
  separated by something, and wrong for Chinese, Japanese and Thai, where a double-click takes
  a whole clause. `Intl.Segmenter` would know better and is deliberately not used --- it
  segments a *string*, and this layer works in code-point indices precisely because
  `FPDFText_GetText` drops characters and desynchronises the two spaces.

  The click counter is keyed on where the *document* was clicked rather than the screen, so a
  scroll, zoom, rotation or page jump between two clicks ends the run by construction instead
  of by a `reset()` call at each of those sites. Units are found from the character under the
  pointer rather than the caret beside it: the caret after a word's last glyph names the
  following space, and a word selection built on that selects the gap.
- **Recent documents in the command palette.** ⌘K, then type part of a name. The list itself
  is not new --- `session.rs` has kept every document read, most recent first, since session
  restore needed it --- but reaching the second entry has never been possible, so a reader
  who wanted yesterday's *other* document went through the file dialog for a file tpdf
  already knew about.

  They are commands rather than a menu, because §8 says every command is reachable in two
  keystrokes through the palette. The registry gained `replace(prefix, commands)` to swap the
  group when it changes, which also drops the replaced ids from the recently-run list --- inert
  today and wrong the moment an id is reused for a different document, which is what these
  ids do.

  Labels show the basename and lengthen **only where two collide**, one directory at a time.
  `report.pdf` in three client folders is the normal case, and three identical rows are worse
  than no list; a full path is unique and unreadable at a glance.

- **A search-results sidebar tab.** Every hit in the document, one row each: the page number
  and the words around the match, with the match emboldened. Picking a row moves the document
  to it. `12 of 5712` in the find bar says how much there is and nothing about what is in it.

  The snippets come from the backend, and that is not an optimisation: the words around a hit
  are on the *page*, and the frontend does not have the page --- Rust extracts the text,
  matches, and drops it again. Building them here would mean re-fetching every page a hit is
  on. A `Match` now carries `before`, `hit` and `after`, three strings rather than a string
  and two offsets, because an offset into a snippet would be a third index space beside the
  page's code points and JavaScript's UTF-16.

  Rows are appended rather than rebuilt --- a 775-page scan reports 775 times --- and capped
  at 2,000 with the cap *stated*, while the match count stays exact. Both mutation harnesses
  now refuse a mutation whose expected test does not exist: one of these named a functional
  check that vitest cannot run, and the pass reported SURVIVED, which reads as a gap in the
  suite rather than a mistake in the harness.

- **A bound on the front-end text cache.** Least-recently-used, 400,000 characters --- about
  16 MB --- with a floor of eight pages kept whatever they cost. It was unbounded, and search
  is what made that matter: a whole-document scan never touches it, but *stepping through* the
  results loads every page a hit lands on, so 5,712 matches over 775 pages is 775 pages of
  characters retained by somebody holding down ⌘G. `peek` counts as a use, so the pages on
  screen are always the youngest and are the last that could be dropped.

  Two of the eight tests written for it could not fail, and both are recorded. One covered a
  correction in `remember` that is unreachable --- `load` returns from the cache before it
  issues a request --- so the code went with the test. The other asserted that an evicted page
  reads as null, which passes whether or not the *rotated* copy of it was dropped too, because
  nothing reaches that map for a page `pages` has lost. A leak no behaviour can see needs an
  accounting observable, not a cleverer assertion.

- **Matching case and whole words, in find.** ⌥⌘C and ⌥⌘W, two toggles beside the find field,
  or the palette. Both default to off, so a reader who never touches them gets the search that
  was there before. A toggle rescans the current query rather than filtering what it found:
  deciding whether a hit is a whole word needs the characters *next to* it, and having those
  on the front end means shipping a 775-page document's text to answer a question about a
  dozen hits.

  Matching case turns off half the fold and nothing else --- whitespace still collapses and
  soft hyphens still disappear, because neither is about case. Whole word is `\b` over the
  folded sequence, so a soft hyphen does not break a word and a line break does. Its word
  class is letters, digits and underscore and deliberately **not** combining marks, which the
  front end's own word selection does count; the consequence is that a whole-word search for
  `cafe` still matches a decomposed `café`, which is what the unrestricted search does anyway.

  `scripts/mutate_rust.py` is new: the backend had no mutation harness, and `search.rs` is its
  densest logic. 16 mutations, each caught by the test named for it. Writing it exposed two
  defects elsewhere --- `keys.ts` rendered Shift before Option while its own comment said the
  reverse, unreachable because no binding held both; and the new harness reproduced, through
  `shutil.copy2`, the mtime-restore defect already recorded here as a `mv` problem. It was
  never a `mv` problem, and cargo served the last mutation to every run afterwards.

- **Go to page, and commands that take a value.** ⌥⌘G, or "Go to page…" in the palette, turns
  the palette's input into a value field with a placeholder, live validation and a preview of
  what Enter will do. A 775-page document previously had no way to reach page 400 at all:
  Home, End, and one page at a time.

  The mechanism is general. A command declares a `CommandArgument` --- `placeholder`,
  `problem`, `preview`, `run` --- and the palette does the typing; `Command` became a union so
  one can no longer be declared with neither `run` nor `argument`, a shape that would
  type-check, list in the palette and do nothing when chosen. Escape steps back to the command
  list rather than closing, so a mistyped number does not cost the palette as well.

  A page past the end is **refused, not clamped**: someone typing 900 into a 775-page document
  has made a mistake, and silently landing on the last page hides it. The registry re-checks
  the value the palette gives it, which is what makes it safe to call from a keybinding.

  Adding ⌥⌘G required fixing `matches`, which **never looked at `altKey`**: every binding
  matched with Option held as well as without, so ⌥⌘F opened find and ⌥⌘G ran find-next. The
  same both-directions bug the Shift check exists to prevent, one modifier over.

- **Find in document.** Cmd-F, search-as-you-type, Enter and Cmd-G to step through hits,
  Shift for backwards, Escape to drop it. Every hit on a visible page is highlighted and the
  current one differently. The scan starts at the page being read and wraps, so a reader on
  page 700 is shown the next hit rather than the first one in the document.

  Matching is in Rust over the **same character codes selection reads**, not through
  `FPDFText_FindStart` --- PDFium's search would have been shorter and answers in positions
  into its own extracted string, which is a second index space beside the one the text layer
  exists to be the only one of. A hit is therefore a range of the indices the boxes are keyed
  by, and highlighting one is the selection code with a different colour.

  Case is ignored, runs of whitespace collapse so a phrase spanning a line break still
  matches, and soft hyphens are dropped. Ligatures, accents and hyphen-broken words are
  deliberately **not** normalised: each would make the highlight cover characters the query
  did not contain.

  A whole-document scan of the 775-page corpus for a word that is not in it --- the worst
  case --- takes **843 ms**, about 1.1 ms per page, essentially all of it extraction. A
  document with no extractable text says so rather than reporting no matches.
- **A command palette on Cmd-K**, and a command registry under it. `docs/PLAN.md` §8 calls
  the palette "the thesis, not a garnish": the complaint about Acrobat is unreachable
  capability, not missing capability, so a palette only helps if every command is in it ---
  which means commands have to be data rather than branches of a key handler. That handler
  had reached fifteen branches. Fourteen commands are registered today and the next feature
  registers rather than growing the chain.

  Ranking is subsequence matching scored like a code editor's --- word starts, then
  consecutive runs, then position --- so `fw` finds "Fit width". It returns the matched
  positions and the palette bolds them, because a highlight that disagreed with the ranking
  would be worse than none. Each row shows its keybinding, so the palette teaches shortcuts
  instead of replacing them. Recents break ties and cannot beat a better match.
- **A screen-reader text layer** (`src/lib/a11y.ts`). `docs/PLAN.md` §8 states accessibility
  as an architectural constraint rather than a later pass, and this lands before thumbnails
  and an outline are built on the same scroller --- everything added first is more that would
  have to be rewritten.

  A canvas-rendered, virtualized page list has **no DOM text at all**, so a screen reader
  finds an empty scrolling region. This maintains a parallel, visually hidden DOM of the
  visible pages' text, split into lines from the same character geometry the selection uses.
  Elements are keyed by page and **never recycled**: a page that stays on screen keeps the
  same element, so a reading cursor inside it survives a scroll. The tiles and the selection
  overlay are `aria-hidden`, and the page number is announced through a polite `role=status`.

  Not verified against a screen reader, and not claimed to be: the checks assert that the
  text is present, is the page's own, and survives scrolling. Reading order also comes from
  geometry rather than from a tagged PDF's `/StructTree`, which is strictly worse for a
  document that has one.
- **The document outline, and a sidebar** (`src-tauri/src/outline.rs`, `src/lib/outline.ts`,
  `src/lib/sidebar.ts`). Cmd-`\` shows a real `role=tree` --- one tab stop with a roving
  tabindex, arrow keys to move, collapse and expand, and the entry the reader is currently
  inside highlighted as they scroll. Clicking one goes to the destination's *position* on
  the page, not merely to the page.

  This is the first feature whose input is openly hostile, and not by inference: PDFium's
  own documentation for `FPDFBookmark_GetNextSibling` says the caller must handle circular
  references. The walk carries a visited set, a depth limit and an item budget, each
  catching what the others cannot, and reports whatever any of them cut rather than showing
  a truncated table of contents as a complete one. 44 entries of a deliberately malformed
  outline --- two cycles, a 200-level chain, a 50,000-character title --- walk in **1.6 ms**;
  an ordinary one in **0.17 ms**.

  Building that fixture found a real defect: **`FPDFBookmark_GetDest` follows the bookmark's
  action without checking its type**, so a `/GoToR` meaning "open other.pdf at page 1" comes
  back as an ordinary destination and resolves against the open document. Reading the action
  first removes the fallback's opportunity to fire. `/Launch`, `/URI`, `/GoToR` and
  `/EmbeddedGoTo` entries are shown, marked and explained rather than dropped or silently
  inert.

  17 mutations, all caught --- one only after the test it aimed at was rewritten, having
  been unable to fail. Nine viewer checks cover the sidebar itself, three of which went red
  before the fixes they prompted: a roving tabindex that did not follow real focus, an
  arrival highlighting the entry *before* the one clicked, and a fixture whose lines were
  all identical.
- **Page thumbnails in the sidebar** (`src/lib/thumbnails.ts`), as its second tab. The first
  feature that competes with the reader for the renderer: a 150 px thumbnail of the A0 sheet
  costs **1.52 s**, PDFium charges that per render *call*, and the render service is one FIFO
  thread. So the strip keeps at most one request outstanding and **withdraws it whenever the
  viewer has work** --- through the same progressive-API cancellation a stale tile uses, which
  returns in 0.25--24 ms. The viewer waits tens of milliseconds for a thumbnail instead of a
  second and a half, and the withdrawn page is asked for again once things settle.

  A hidden strip renders nothing at all. Rows exist only for the visible window plus an
  overscan, so `aria-setsize` and `aria-posinset` are load-bearing rather than decorative.
  Tier 1 is *read* --- the placeholder and the thumbnail are the same bitmap, so the page
  being read appears instantly --- and deliberately not written, since tier 1 is permanent
  and one entry per page is 98 MB on the 775-page corpus.

  Twelve mutations, all caught by the check each was aimed at --- after two of the new
  checks turned out to be wrong in ways only mutation could show.
  One could be **switched off by the defect it was aimed at**: it skipped when every row was
  built, so deleting windowing made it report itself inapplicable rather than fail. The other
  was bounded the wrong way round --- "some thumbnail was borrowed" passes *harder* when a
  missing in-flight guard borrows the same page on every scroll, which is what it was doing.
  A new twelve-page fixture, `vector-multi.pdf`, is the only document where a thumbnail is
  slow enough to collide with the viewer at all; elsewhere those checks skip and say so.
- **Front-end unit tests, and a seventh quality gate.** `vitest`, over command ranking and
  line splitting --- the first front-end logic with an answer that can be *wrong* rather than
  merely ugly.
  The plan had said `npm run test` would land when there was something for it to check.
  Twenty-two mutations against the new code, all caught; one branch was deleted rather than
  tested, because nothing could make it fail.
- `src-tauri/src/bin/text_probe.rs` --- checks the page-space to device-space flip against
  **pixels**, per character, and carries a control that fails the run if the wrong convention
  would also pass. On the four small fixtures: 100% against 4.1--4.8%. On the dense corpus the
  wrong convention scores **69.9%**, so that page cannot tell the conventions apart and the
  probe says so instead of reporting the 100%.
- **A viewer a person can drive** (`src/lib/viewer.ts`). Open a PDF from the file dialog or
  by dropping it on the window, then scroll it with a trackpad, a wheel, the arrow and page
  keys, Home and End, or the scrollbar; zoom by the Cmd-`+`/`-` ladder, a pinch, or Cmd-0
  for fit-width, which then tracks the window. It drives the same `Scroller` the benchmark
  drives, deliberately: the class that knows what a frame costs is not also the class that
  knows where the finger went.

  **The frame loop idles.** It runs only while the scroll is moving or the scroller has work
  that has not reached the screen. A viewer that ran the benchmark's fixed loop would hold a
  core awake for as long as it was open.

  **The status line reports the degraded state** `docs/PLAN.md` §9 recorded as owed --- and
  reports the two failures separately, since "no page yet" and "a blurry page" are different
  things. Both numbers are the scroller's own coverage measurement, so a reader is told the
  same number the benchmark reports.
- **A functional check of the viewer** (`src/lib/viewercheck.ts`, `scripts/viewer_check.py`).
  Opens a document in a real webview, dispatches real wheel and key events at the viewer's
  own root, and asserts twenty behaviours. Three of them are controls: idling is asserted in
  both directions, every coverage recovery is preceded by an assertion that the tiles were
  actually discarded first, and a zero-length drag must select nothing.

  Without the second of those, "covers the last page" passed instantly on coverage the first
  screen had already established, while its own detail line read `page 1/775`. Eleven
  deliberate mutations across two passes, one at a time. Nine were caught; one was an
  identity that tested nothing; one found a guard --- `Selection.isEmpty` --- that no mutation
  could break, now deleted.

  The selection assertion is the second attempt at one. The first checked that the dragged
  text was a **substring** of the page's text, which cannot fail: a selection is a contiguous
  range of character indices, so its string is a substring however wrong the boxes are.
  Inverting the y-flip in `text.rs` passed all twenty checks and returned real words from the
  wrong part of the page. What discriminates is ordering --- text dragged near the top of the
  page must come from earlier in the page's text than text dragged further down.

  It does not take focus. The window has to stay visible, because WebKit suspends an occluded
  page, but raising it over whatever someone is doing every time a check runs is its own bug.
  `scroll_bench.py` still focuses on purpose: an unfocused window is throttled, and a
  frame-rate benchmark would be measuring the throttle.
- **Cancellable rendering** (`src-tauri/src/progressive.rs`). PDFium's progressive API,
  driven on raw `FPDF_DOCUMENT` / `FPDF_PAGE` / `FPDF_BITMAP` handles, because
  `pdfium-render` keeps every handle accessor `pub(crate)` and the safe wrapper therefore
  cannot reach it. A render can now be abandoned from another thread in 0.25--24 ms where
  it previously ran to completion over 6.3 s. Uncancelled output is byte-identical to the
  existing path. Not yet wired into the viewer --- see `docs/PLAN.md` §Phase 1.
- **Stale tiles are withdrawn from the renderer** (`render.rs`, `protocol.rs`,
  `tiles.ts`, `scroller.ts`). Every tile request carries an id; `tile://localhost/cancel/<id>`
  withdraws it. One that has not started is dropped without rendering, one already running
  is abandoned through the progressive API. The viewer's render service now runs on the raw
  handles throughout, which also removes a full-tile copy --- Pdfium renders straight into
  the buffer that is handed on, where the safe path's `as_rgba_bytes()` allocated and copied
  a second 16 MB at 2048².

  Measured against the coverage floor rather than the frame rate, withdrawal being the
  variant: **inert on the text corpus** (100% sharp either way, nothing withdrawn) and on
  the A0 sheet it removes the waste without buying coverage --- five finished-then-discarded
  tiles per round become zero, and the visible area stays 6% sharp. The A0 page still fails
  the criterion; that is the worker pool, not the queue.
- **A page-handle cache on `RawDocument`.** `FPDF_LoadPage` re-parses the page on every
  call --- PDFium caches nothing --- which is 0.18 ms on the text corpus and **44.3 ms on
  the A0 sheet**. Loading per tile request, as `render.rs` still does, costs a six-tile
  screenful 266 ms of re-parsing on the document that is already too slow.
- `src-tauri/src/bin/progressive_probe.rs` --- measures the above: pixel identity against
  the safe path, poll frequency and the latency it bounds, and what a cancelled bitmap
  actually contains. Its `identity` mode fails a run in which nothing paused, so a passing
  result cannot be one that never exercised pausing.
- **The first tests a change can break** --- 26 of them, over the request-withdrawal state
  machine (`src-tauri/src/queue.rs`, extracted from `render.rs` so the orderings can be
  driven directly instead of provoked) and the `tile://` URL parser. Each was verified by
  mutating the code it covers and confirming the expected test failed; that pass found a
  guard no mutation could break, now deleted, and a test that asserted the wrong half of the
  property it was named for.
- The scroll benchmark drains a variant's outstanding requests before the next one starts,
  and reports the tiles each round withdrew beside the ones it threw away. Without the
  drain the two variants share a render queue: whichever ran first measured better, and
  swapping them swapped the result.
- **Printing** (`⌘P`, macOS). tpdf hands the operating system a **PDF, never pixels** ---
  measured: `cupsfilter -d <queue>` against a PDF-native printer returns the input file byte
  for byte, so rasterising first could only throw information away. What is ours is deciding
  *which* PDF: everything unrotated is handed over untouched, a page range deletes pages in
  place so nothing loses an inherited `/Resources`, and the reader's view rotation composes
  onto each page's effective `/Rotate`. PDFKit paginates and runs the panel; the panel's own
  page-range field is why there is no range UI here.

  Every job is re-read with PDFKit --- a parser that did not write it --- before the panel
  opens. That is not ceremony: a page table left contradicting its own `Kids` array passes
  every `lopdf` check and makes PDFKit report five pages for a two-page document, the extra
  three being blank sheets it manufactures to satisfy the count.

  Page deletion is ours rather than `lopdf::delete_pages`, which runs a quadratic graph walk
  once per deleted page: keeping two pages of a 775-page document costs 620 ms there and
  1.2 ms here, for byte-identical output.

  **Windows is not implemented**, and says so with an error rather than doing nothing.
- **Every document is parsed in a sandboxed worker process** (`src-tauri/src/render.rs`).
  The one Phase 0 constraint that had never reached the running program: the boundary
  existed and was measured, but the viewer still opened documents in the app process.
  `RenderService` now runs on either backend, defaulting to workers on macOS, with
  `TPDF_BACKEND=in-process` selecting the control --- and refusing any other value, because a
  typo that silently ran the other implementation would make every comparison between them
  meaningless.

  What says it really moved is not a comment: `backend-probe` reads the **dynamic linker's**
  image table and finds no `libpdfium` mapped in a process that has just opened a 775-page
  document and rendered a tile from it. A startup mark of our own would only report what our
  code believes it did.

  The boundary is transparent on six corpora --- tiles byte for byte, and the same page
  geometry, character boxes, search ranges and outlines. It costs **11--16 ms at startup**
  of a ~50 ms application budget: 3.1 ms to spawn and 8.9 ms for the child to bind PDFium,
  sandbox itself and parse. Warm start is 287--295 ms against a 300 ms target, so the margin
  lazy page geometry bought has largely been spent.

  A withdrawal now has two halves that do different jobs --- the parent's queue decides what
  the caller sees, the wire withdrawal decides whether the worker keeps burning CPU --- and
  the first check for it could not have failed, since `Abandoned` is what the parent
  produces on its own. It now asserts the latency too: 2.2 ms against a 1,125 ms render.

  **Windows still refuses rather than running unsandboxed**, and so defaults to in-process.
- **A worker is started, sandboxed and font-warmed before any document is chosen.** Opening a
  file then costs **0.3--1.1 ms** instead of 8--17 ms, because the process is already past its
  link, its `sandbox_init` and PDFium's system-font walk. The A0 sheet keeps 48 ms of its 56 ---
  that is page parse, which no pre-spawn can remove, and it is the row that says the
  measurement is not merely reporting zero.

  The document is handed over **after** the sandbox, as an `SCM_RIGHTS` descriptor: a
  pre-spawned worker has already dropped the authority to open a file, so a path would be
  useless to it even if it were trusted. `bin/fdpass_probe.rs` proves that crossing, with the
  control that the child cannot read `/etc/hosts` at the time.

  One spare, not a pool of them --- it is for the *first* worker of a document, which is what a
  reader waits on. A spare that dies falls back to an ordinary spawn rather than failing the
  open.

  A mutation pass afterwards found that **none of the three mechanisms this added was visible
  to any check**: deleting the font warm, skipping the readiness wait and dropping `FD_CLOEXEC`
  each left `backend-probe` green on every corpus. Two were real and are now pinned, and the
  third turned out to be unreachable defence:

  - `bin/prespawn_bench.rs` asserts and exits non-zero instead of printing a table. The
    comparison is between a base-14 fixture and an embedded-font one, because the gap between
    them *is* the system-font walk that warming pays early: 0.35 against 0.80 ms warm, 9.96
    against 0.84 ms with the warm deleted, over a 3.7 ms bound.
  - `backend-probe` gained "a spare does not outlive the service that started it", which runs
    this binary as a short-lived service and asserts the spare died with it. It needs a second
    process because the leak cannot be seen from inside the one that has the socket open.
  - `PreWorker::wait_warm` now *consumes* its receiver and returns a `WarmWorker`, the only
    type `adopt` accepts. The runtime check it replaces could not be made to fail, because a
    spare is only ever published warm --- but that was enforced in another module, so the
    ordering moved into the type rather than being deleted. Skipping the wait no longer
    compiles.

- **Tiles of one page render in several worker processes at once.** The worker backend is
  served by a pool of threads over one job queue, and each document has a pool of processes
  they draw from. A screenful of the A0 sheet goes from **3.46 s to 0.83 s, 4.2x**, measured
  through the service itself with interleaved rounds; a cheap page gains 2.7x. Six workers is
  where the curve flattens --- neither the core count nor the performance-core count.

  **Growth is lazy**: a document opens with one worker and gains another only under
  contention, so a reader turning one page at a time never pays for a second parse of it.
  A fully grown pool on the A0 sheet is about 290 MB, which is given back again once the
  scrolling stops --- see the retirement entry below.

  The in-process backend is deliberately *not* pooled: concurrent PDFium in one process is
  undefined behaviour whatever the handles are.

  Two of five mutations first survived, and both pointed at the design rather than the tests.
  With one thread per worker the pool's own capacity bound was unreachable --- the thread
  count was enforcing it --- which also meant six tiles of a slow document could occupy every
  thread and starve a second document whose workers were idle. Threads are now `pool + 2`.
- **An idle worker is retired, so a burst of scrolling no longer decides what the session
  keeps.** A worker untouched for 30 seconds is killed, down to one per document. On the A0
  sheet that returns **242.5 MB of a 289.9 MB pool** and charges the screenful after the
  pause **+65 ms on 811 ms**; on the text corpus, 56 MB and +15 ms. Both measured over two
  runs by `pool-bench --mode retire`, pairwise within interleaved rounds.

  **One worker is kept rather than zero.** Nothing breaks at zero --- the checkout path
  spawns from an empty pool and the close drain is trivially satisfied by it --- but the
  saving is one process against a spawn and a full re-parse charged to the next page turn,
  which is the moment someone is watching. Retiring to one already returns five sixths.

  The reaper thread holds a **weak** handle to the pool. A strong one would keep every worker
  and every document mapping alive after the last handle to the service was dropped, which is
  a larger leak than the one being fixed and is invisible to any check running against a live
  service --- so `backend-probe` now drops a service and asks the OS whether its processes
  went with it.

  Eight checks, and six mutations all caught. The one that matters is the *control*: a sample
  taken before the timeout expires, without which "the pool shrank to one" is equally
  satisfied by a reaper that kills everything on every sweep.
- **A document is released when the reader moves to another file.** Until now nothing ever
  removed one, so a session that opened a dozen files held a dozen documents --- which the
  process boundary turned from a heap allocation into a dozen sandboxed children at
  7.8--48.2 MB each.

  A released id leaves a **hole** rather than being removed, and is never handed out again.
  The `Vec` index is the id, so removing the entry renumbers every document after it and a
  request naming the closed one is answered in full from a file the caller never asked about
  --- demonstrated by mutation, which returned a perfectly good tile of the wrong document.
  Whether a request might still be in flight needs no answer at all: the render thread is
  FIFO, so a close lands behind everything already queued.
- **A worker that dies is replaced, and the request retried once.** Isolation that ends the
  reading session is isolation nobody wants: a crash caused by anything other than the
  request in hand is now invisible to the reader. The replacement is handed the **same
  document mapping**, not the same path, so a file rewritten in between cannot silently
  become what is on screen --- and a 337 MB scan is not read twice. A live worker that
  answers with an error is *not* replaced; only one the kernel says has exited.

  The bound on a crash loop is the single retry rather than a restart budget. A page that
  faults deterministically then costs one process per attempt, which is bounded by the
  reader's own requests --- a counter on top would be defence nothing could reach, and
  `AGENTS.md` says to delete those rather than keep them.

  `backend-probe` kills the worker out of the OS process table and asserts the same pixels
  come back from a *different* pid. Six mutations, five caught; the survivor is recorded
  with its reason. Two of the findings were about the checks: `SIGSEGV` does not kill a Rust
  process the first time it is sent, and a check nested inside a lookup for the thing under
  test disappears rather than failing when the defect removes it.
- **A parent that does not trust its worker's arithmetic.** A reply states how many bytes of
  the shared mapping it wrote; that claim is checked against the mapping's size and, for raw
  pixels, against `width x height x 4` exactly. Reply lines are bounded at 32 MB, because
  `read_line` on a pipe is otherwise unbounded and a worker made to emit an endless one would
  take the app down with it --- perfect isolation, dead application.
- `scripts/fetch_pdfium.py` --- installs the pinned PDFium build (`chromium/7881`),
  verifying its SHA256 before extracting and refusing a V8 asset. A clean clone could not
  previously build: `vendor/pdfium/` is gitignored and nothing fetched it.
- `scripts/gates.py` --- runs every quality gate and *is* the definition of them, so the
  checklist in `BUILD.md` cannot drift from what actually gates.
- `BUILD.md` and this changelog.

### Changed

- **Document opens are serialised by `src/lib/serial.ts`** rather than by four lines inside
  `App.svelte`. The behaviour is unchanged --- one open at a time, in call order, a failure
  never stopping the next --- but the invariant now has tests that can fail, which it could
  not while it lived in a component with no harness. The end-to-end check that exercises it
  through the running app is a race, and a race is a smoke test rather than a gate: measured
  with the queue removed, it reports the defect in roughly two runs out of three.

  Writing it turned up unreachable code of its own. The chain was built with both `then`
  arms calling the body, copied from the original; a mutation reducing it to one survived
  the whole suite, because the tail is assigned a promise with both outcomes flattened and
  therefore can never reject. The arm is gone and the line that makes it impossible now says
  so. Three mutations of what remains --- no queue, no flattening, a tail that never advances
  --- each go red on the test aimed at them.

- **The tile-retry backoff moved into its own module** (`src/lib/backoff.ts`), with unit
  tests for the properties the scroller relies on: a failed request is not reissued before
  its wait, each further failure doubles the wait up to 8 s, an already-due entry reports no
  wake (the busy-loop guard), success forgets the entry. The clock is a parameter, which is
  also the fix for a dropped wake — the frame and the retry scheduler previously read the
  clock at two different moments, and an entry falling due between the two readings got no
  wake and stayed blank until unrelated input. Tile and thumbnail failures also now name
  their reason on the console, once per request rather than once per attempt.

- **`displayedSize` exists once** (exported from `scroller.ts`) instead of three times ---
  the odd-turn dimension swap was independently implemented in the scroller, the viewer and
  the page strip, and a rotation fix applied to one would not have reached the other two.

- **The eager startup open is only collected for the path it was started on.** A first open
  naming a different file than `TPDF_STARTUP` now falls through to a normal open instead of
  silently receiving the pre-opened document.

- **The worker pool moved out of `render.rs` into `workers.rs`.** That file held the service,
  both backends, the pool, the spare slot and the reaper at 1,958 lines. Nothing changed in
  the move — verified by asserting every top-level item HEAD defined exists in exactly one of
  the two files, that no test name was lost, and that the moved block diffs clean against
  `HEAD` apart from the accessors that replaced `RenderService` reaching into `SpareSlot`'s
  fields. `render.rs` keeps the service, the `Engine` trait and the in-process control;
  `pool_size` and friends are re-exported on the path the benchmarks already import.


- **Lazy page geometry is the default**, with `TPDF_EAGER_GEOMETRY` to restore the walk.
  Enumerating every page of a 775-page document costs 86 ms on the critical path to buy a
  scrollbar exactness the scroller estimates anyway; it is what takes warm startup from
  374 ms to inside the 300 ms target, and shipping the opposite default meant the Phase 0
  exit criterion was met by a variant nobody ran. Warm start measures **276 ms median,
  267--293**, with the dialog plugin now linked in.
- **The single-canvas `viewport` layout is what the viewer uses**, applying the verdict
  §4 reached and had not yet acted on.
- The app's window is the document, not the spike harness: the manual A/B benchmark button
  and its `src/lib/bench.ts` are gone, superseded by the automated `autobench` path that
  every published transfer measurement was actually taken with.
- `scroll_bench.py` and `viewer_check.py` share their lock-screen and display guards
  (`scripts/webview_guard.py`) rather than carrying two copies of a long message that only
  matters at the moment someone is working out why nothing happened.
- **Corrected the worker-pool scaling claim.** `AGENTS.md` and `docs/PLAN.md` §3 said to
  size the pool from the performance-core count, on the strength of 3.89x across four
  workers. That was one tile from each of many pages of the text corpus; across six tiles
  of one A0 page --- what a viewport actually asks for --- the same machine gives 2.56x on
  four, 3.22x on six and nothing at eight. `worker-bench` grew a `--grid` work list to be
  able to ask, since its old one walked pages and the A0 fixture has one.
- **The scroll benchmark reports a coverage floor**, the worst single frame of the worst
  round, beside the mean it already reported. "Never below the tier-1 placeholder" is a
  claim about a minimum, and a mean that rounds to 100% is equally consistent with a frame
  that showed nothing --- so the criterion could not be tested by the number that was being
  read for it.
- **The single-canvas scroll layout should be the default, not the fallback.** Over ~3,300
  timed frames it dropped no frames where the canvas-per-tile layout dropped three and
  stalled once, at identical coverage, and its per-frame cost is 3--4x lower.
- **Corrected a load-bearing architectural claim.** `AGENTS.md`, `docs/PLAN.md`,
  `docs/THREAT-MODEL.md`, `render.rs`, `Cargo.toml` and `worker_bench.rs` all stated that
  `pdfium-render`'s `thread_safe` feature serializes every PDFium call behind one global
  mutex, and that multiple document handles therefore render sequentially but safely. It
  does not, and they do not. There is no mutex in the crate's native path; the feature
  only makes `Pdfium` `Send + Sync`. Two threads rendering the A0 page **segfault**, while
  four threads on a simple document returned pixel-correct tiles six times out of six at a
  3.85x speedup — then crashed on the next round. `src/bin/thread_probe.rs` is the
  measurement. The conclusion (render in worker processes) is unchanged; its justification
  is now that threads are undefined behaviour rather than merely futile.

- **Rotating the view**, clockwise on Cmd-R and anticlockwise on Cmd-L, both also in the
  palette. Preview's bindings rather than Acrobat's, whose Shift-Cmd-`+`/`−` produce the same
  `key` as the zoom shortcuts on this keyboard. It turns the *view* and never the document:
  rotating pages in the file is a page operation and belongs with the ones that write.

  PDFium's render call takes a rotation and composes it with the page's own `/Rotate`, so the
  renderer's half is one argument threaded from the tile URL down --- plus the dimension swap
  it needs, since PDFium fits the page into the rect it is given and passing the upright size
  squeezes a landscape page rather than turning it. Character boxes cannot go the same way,
  being a property of the document, so they are turned in our own code where the cache hands
  them out. The two implementations are tied by a rule asserted over all sixteen
  combinations: turning a device box after `to_device` must equal `to_device` of the summed
  turn --- which is how the frontend's turn inherits the verification `text-probe --mode
  align` did against pixels.

  **Both cache tiers go.** A zoom step keeps the tier-1 placeholder because it is only
  stretched; a rotated one is a different picture, and keeping it would leave the page
  sideways under its own sharp tiles. So a rotation on the A0 sheet goes grey for the ~1.5 s
  that placeholder costs to produce again.

  **An outline destination is not placed while the view is rotated.** It carries an offset
  down an upright page; at a quarter turn that axis is the screen's horizontal one, and at a
  half turn it counts upwards while the reader scrolls down. Navigation and the outline
  highlight fall back to page granularity --- which is what `/Fit` means, and what
  `outline.rs` already returns for a destination it cannot place.

  Fourteen mutations, all caught by the check aimed at them. Three of the six new checks
  exist because a mutation survived first: "the same lines come back out of a rotated page"
  derived its drag positions from the very boxes it was testing and so passed with the text
  layer never told about the rotation; nothing in the harness looked at a pixel, so dropping
  the rotation from the tile URL passed everything; and the viewer and the scroller each keep
  a rotation, so a scroller laying every page out upright survived a check that only measured
  the zoom.

- **Session restore.** tpdf reopens the document you were reading, on the page you were on,
  at the zoom and rotation you had, with the sidebar as you left it --- and opening a document
  you have read before puts you back where you were in it. One place per document, 32 of
  them, kept in `session.json` in the app config directory and written through a temp file
  and a rename so a crash mid-write leaves the previous session rather than a truncated file.

  **A malformed session file is an empty session, never an error**, and a field out of range
  is repaired rather than refused --- the opposite of what the tile protocol does with a bad
  parameter, because a file that has sat on disk across upgrades is not a live instruction,
  and rejecting it would discard every other document's place over one bad number. A
  remembered page is clamped to what the document has *now*: a path is not an identity, and
  the file may have been rebuilt shorter since.

  Positions are written at most once a second, and chained through one promise --- not for
  throughput, but because `invoke` resolves out of order under load and two writes a second
  apart can otherwise land in the other order, the older place overwriting the newer.

  Checked by `scripts/session_check.py` across **four launches of the real app**, because
  restoring is part of the boot and a harness that replaced the application --- which is what
  every other one here does --- would be checking a second implementation. Two of the four
  assert nothing about restoring: that the app does *not* open in the remembered state by
  itself, and that nothing opens when nothing is remembered. Without the first, "restored to
  page 7" is satisfied by an app that happens to open there.

- **Dark mode.** Two things wear that name and only one was missing. The chrome already
  followed the system, being built on `Canvas` / `CanvasText`; the scrollbar and the surround
  around the page had escaped that and now follow it too. The surround needs two literals
  rather than a formula, since it has to be darker than the paper in *both* themes.

  The page gets an explicit command instead --- **Invert page colours**, ⌘⇧I. Named that and
  not "Dark mode" because the chrome is already dark when the desktop is, so a command called
  dark mode would appear to do nothing for the reader who most expects it to. Inverting a
  document changes what it looks like, and a reader who darkened their desktop has not asked
  for that, so it is never inferred from the system theme.

  It inverts HSL **lightness**, holding hue and saturation, so blue headings stay blue where a
  plain `255 - c` would turn them yellow. That has a closed form --- chroma is unchanged by the
  inversion, so every channel moves by the same `255 - max - min` --- which needs no float, can
  never clamp, and is an exact involution. Applied in the renderer rather than as a CSS filter,
  because a filter is applied by the compositor and its pixels cannot be read back: a check
  could then only assert that a style was set.

  Photographs come out as negatives with the right hues, as they do in every reader that
  offers this. That is why the mode is off by default and asked for explicitly.

- **File associations.** Double-clicking a PDF, "Open With", dragging one onto the icon, and
  `tpdf file.pdf` from a terminal all open it. Declared as `role: Viewer` rather than Tauri's
  default `Editor`, deliberately: `Editor` tells Launch Services tpdf can edit a PDF, and it
  cannot yet. Rank stays `Default` --- not `Owner`, since tpdf does not create PDFs, and not
  `Alternate`, which ranks it below every other viewer.

  A macOS double-click puts nothing in `argv` --- it is an Apple Event, and it can arrive
  before the webview exists --- so paths are queued until the frontend is listening and
  emitted directly after, with the drain and the flag flip under one lock. The event's name
  is fetched from Rust rather than duplicated as a constant on both sides, because a constant
  that drifts fails by silence: the app keeps working and merely stops noticing documents
  opened while it is already running.

  A handed-over document beats a remembered one, since someone who double-clicked a file is
  asking for that file. Checked by `scripts/open_check.py` across nine launches and 31
  checks, six of them controls --- and the checks themselves by eleven mutations, all of
  which behaved as predicted, including one predicted to survive.

### Added

- **A release workflow, firing on a CalVer tag and on nothing else.**
  `.github/workflows/release.yml` gates both platforms by invoking `scripts/gates.py` — not
  by re-listing its commands — then builds, signs, notarizes and publishes a draft release.
  macOS is Apple Silicon only: `fetch_pdfium.py` installs one architecture, and an x86_64
  slice carrying an arm64 engine would fail at bind time on a machine nothing here can test.

  The part with no precedent in the portfolio is signing the bundled `libpdfium.dylib` —
  neither `screenpick` nor `dblitz` ships a native library, and notarization requires every
  Mach-O in the bundle to be Developer ID signed with the hardened runtime. It is signed in
  `vendor/` before the bundler copies it, which holds whether or not Tauri re-signs nested
  resources.

  **Nothing in the macOS half has run yet.** Its verification step fails rather than warns,
  because a skipped notarization exits 0 and yields an app Gatekeeper rejects on any machine
  that has never seen it.

### Changed

- **The installers no longer ship the development spikes.** All 17 probe and benchmark
  harnesses were `[[bin]]` targets of the crate Tauri bundles, so every installer carried
  them — a sandbox prober and a hostile-document harness included. They are `[[example]]`
  targets now, which the bundler does not enumerate: the MSI payload is three files, and the
  MSI went 16.7 → 8.0 MB with the NSIS setup 8.8 → 5.8 MB. On macOS this is also 17 fewer
  binaries for the hardened runtime to sign and notarize.

  Invocations move with them — `--example <name>` rather than `--bin`, and artifacts land in
  `target/release/examples/`. Clear out any probe executables left in `target/release/`;
  nothing rebuilds them and an older documented path still resolves to a frozen copy.

  The `bins` gate takes `--examples` now, and that flag is not decoration: the file that
  motivated the gate is one of the moved ones, so without it the gate would link only the app
  and pass in under a second looking exactly as it did when it covered seventeen targets. An
  undefined symbol called from an example's `main` turns it red with `LNK2019`.

- **`backend-probe`'s Windows figures were a commit behind, and read as a missing check.**
  `BUILD.md` and `AGENTS.md` recorded `37/41 ... 40/41` there against 42 on macOS, and the
  gap was carried as an open question --- which check is macOS-only? --- with the parent's
  memory poll as the candidate, since `worker-probe` really does skip that one on Windows.
  None is. The 41s were taken at `df1ca61` and `9fb728f` added a check immediately after, so
  the two counts differed by a commit rather than by a platform. Re-measured on Windows:
  **38/42, 38/42, 39/42, 40/42** across `text-base14`, `text-cid`, `outline-hostile` and
  `vector-heavy`, no failures, and the name sets byte-identical across all four when diffed
  rather than counted. `BUILD.md`'s flat *"all 42 names appear"* was right as written and
  stays flat; the proposal to weaken it into a per-platform statement is what the
  mismeasurement would have cost. New trap: *Two counts from two commits are not a platform
  difference*.

- **`latency-bench`, closing the last measurable Windows gap.** `worker-bench --mode latency`
  decomposes what one tile costs --- render, encode, the parent reading it, and everything left
  over --- and cannot run off unix: it carries its own worker, `dup2` handover, socket pair and
  SBPL bisection. Its own refusal named that decomposition as the single thing a Windows spike
  would measure that nothing else does. This is that spike, and deliberately not a port: it
  drives the **production** worker, so it is portable by construction and macOS can cross-check
  it against an implementation sharing no worker code with it. **Every figure below is Windows,
  and it has never been run on macOS** --- that it compiles there is a claim about a compiler,
  not a result.

  There is no `pipe` variant, which is a finding rather than an omission --- production sends
  every tile payload through the shared mapping and never inline, so a pipe row would measure a
  route no tile takes. Differencing `raw` against `png` recovers the same quantity from two
  paths that are real.

  Windows, 1024² tile: crossing the boundary costs **0.263--0.283 ms**, a round trip carrying no
  tile **0.039--0.068 ms**, and moving bytes through the mapping **0.0051--0.0058 ms per 100 KB**.
  The boundary cost is a property of the boundary, so the number to read is that it varies by
  0.02 ms across fixtures whose render times differ by three orders of magnitude.

  Its own defects were found by running it rather than by reading it, and each is now a trap. It
  misparsed the outline reply as an array, so a defaulted zero printed *"the document has no
  outline"* for `outline-simple.pdf` while its own control timing four lines above said
  otherwise. It estimated the boundary cost by subtracting two ~2.7 s end-to-end figures, which
  on the A0 sheet reported **-265.822 ms**. It guarded payload differencing on ordering rather
  than materiality, so a page that barely compresses divided noise by a 68 KB gap. And the check
  added to stop the bad estimator returning was itself too weak twice over: requiring only that
  the figure be positive caught the defect just on the runs where the noise happened to fall
  that way, and the replacement compared a spread against a figure derived separately, so a
  mutation moved one and left the other sound. Both now come from one per-round vector.

  Verdicts go through a recorder that pads every label to seven at column 1 --- an indented
  `[SKIP]` had been invisible to the repository's own width recipe, which then passed by never
  examining it --- and the tag vocabulary is back to the four every other harness emits, a
  `[NOTE]` invented here having been dropped silently by anything grepping the set. The run ends
  with `N/M checks passed` and exits non-zero on a failure. 4/4 mutations caught, restored by
  bytes and verified by digest.

- **Every reference to a spike's path now resolves.** The 2026-07-31 move of 17 harnesses from
  `[[bin]]` to `[[example]]` left 34 `bin/<name>.rs` references across docs and doc comments
  naming a path that no longer exists. Repointed with each target verified to exist; the dated
  entries in this file and the trap describing the move itself keep their original paths, because
  a historical record naming a historical path is correct.

### Fixed

- **The installers shipped no PDF engine.** `tauri.conf.json` declared no `bundle.resources`,
  so nothing ever copied PDFium into a bundle, and the resource-directory fallback in
  `pdfium_library_dir` pointed at a directory the bundler never created. Every installer built
  before this produced an app that opens a window and cannot parse a document on any machine
  without this repository checked out at the same absolute path. `tauri.windows.conf.json` and
  `tauri.macos.conf.json` now carry the library, because the two archives disagree about where
  the loadable one lives.

  It survived because every check ran where the dev tree exists, so the bundled branch was
  never exercised — `viewer_check.py` against the bundle passed either way. Now proved against
  the extracted MSI with the dev library moved aside: **102/102 checks passed on the bundled
  library alone**, against a negative control with no PDFium reachable that fails and names the
  path it looked in. The lookup tries two bundled candidates, because Tauri's WiX template
  ignores a resource map's target directory and puts the DLL beside the executable.

- **The macOS half of that fix did not work, and the check found it** (2026-07-31, verified on
  a Mac). `"...libpdfium.dylib": "pdfium/"` produced neither bundled layout: the macOS bundler
  reads the value as a target *path* and renamed the dylib to the **file**
  `Contents/Resources/pdfium`. Both candidates missed, and a bundle with the dev tree hidden
  reported `0/1 checks passed` with three `could not load Pdfium` lines. `tauri.macos.conf.json`
  now names the file (`"pdfium/libpdfium.dylib"`); with the dev library hidden the same bundle
  reports **102/102 checks passed, 7 not applicable, 109 names**. `tauri.windows.conf.json` is
  unchanged on purpose — WiX ignores the target either way, and that platform cannot be re-run
  from a Mac.

  Every cheap observation agreed with the working case: the build exits 0, the bundle is the
  right size, `find` prints a path containing `pdfium`, and `viewer_check.py` from the repo
  passes. What discriminates is `-type f` against `-type d`. See the trap of that name.

- **`backend-probe` and `worker-probe` mis-aligned their own output on the rows that passed.**
  Both built the verdict as `"[{}] {name} ..."` with `OK` or `FAIL` interpolated, and `[OK]` is
  two characters shorter than `[FAIL]`/`[SKIP]` — so passing rows started two columns to the
  left, in the terminal and in anything parsing them. `BUILD.md`'s documented `cut -c8-47`
  recipe for extracting a check-name set then sliced those rows short, and diffing three
  `backend-probe` corpora reported **"the name sets diverge"** for three runs whose sets were
  identical. The count agreed throughout, which is what made it look like a real regression
  rather than a broken read. Both labels are padded to seven now, matching every other harness,
  so one recipe reads all of them; `prespawn-bench`'s summary line had the same shape and is
  fixed with them. `BUILD.md` says the `8` is a property of the harness and how to check it.

- **The Rust test gate printed a bare `error:` line while passing.** Two Windows checks spawn
  a worker whose child is the libtest harness, which has no `--render-worker` dispatch and
  says so on the stderr every worker inherits by design. That refusal is the checks' control,
  but it landed on the gate transcript as `error: Unrecognized option: 'render-worker'` above
  its own `ok` line, so a clean run of 205 tests was indistinguishable from one that failed
  and reported it badly. A test-only guard now points the process's stderr at the null device
  for the length of the spawn and restores it on drop --- the console changes, the child does
  not, and no `cfg(test)` branch enters the code under test. The gate transcript now contains
  no `error:` line at all.

  **The `#[cfg(unix)]` arm compiled and ran on macOS for the first time on 2026-07-31, and the
  claim it was written on turns out to be false there.** The noise does not occur on macOS:
  removing the `install()` call changed no output over 40 runs, while the same harness invoked
  directly prints `error: Unrecognized option: 'render-worker'` and exits 101. Holding the
  `PreWorker` for 400 ms before dropping it makes the line appear exactly once, which names the
  mechanism — `prespawn` returns as soon as `fork`/`exec` is issued, the test drops the child
  at once, and the kill lands while it is still in dyld, before libtest parses argv. Windows
  creates the process suspended and resumes it, and loses that race. The guard is kept rather
  than deleted, because the impossibility lives in another type's drop timing rather than in
  this arm, and the doc comment now says so.

  Proved by removing the guard, which puts the line back. `docs/TRAPS.md` records why that
  mutation has to be run single-threaded: the window is process-wide, so with the module's
  other checks running beside it the deletion printed nothing and read as a guard nothing
  needed.

- **Enter on a page thumbnail or an outline row could go to the wrong place.** Both the strip
  and the outline tree activated `focused` --- their own record of which row has focus, kept up
  to date by a `focusin` listener --- rather than the row the key actually reached. That record
  is a mirror, and `focusin` is not guaranteed to arrive: a document without system focus moves
  `activeElement` without delivering any focus event. A stale mirror sends the reader to
  whatever it still names, which is page 1, since it starts there. Both now read the row from
  the event itself and keep the mirror only for a key that landed on the container rather than
  on a row.

  Found from a single `viewer_check.py` failure on `vector-multi` that has never recurred, so
  it is an identification by mechanism rather than by catching it twice --- `docs/TRAPS.md` has
  what that does and does not establish. Each class has a unit test that was shown to fail
  first and a control on the fallback; `sidebar.ts` had no unit tests before this.

- **A harness phase could time out with no transcript at all, because render workers inherit the
  app's stdout.** `open_check.py`'s handover phase captured the app through a `PIPE` and waited on
  `communicate()`, which returns only at EOF --- and the workers are re-execs of the same binary
  holding the same descriptor, so one outliving the app by a moment produced `run timed out` with
  nothing printed before it. That reads as the app hanging on one phase while an adjacent phase
  passes 7/7. It now redirects to a file and waits on the *process*, so there is no EOF to wait
  for, and a timeout keeps whatever the run had already written.

  Two smaller fixes fell out of it. `scripts/stray.py` clears leftover instances of the binary
  under test before the first launch and prints a `[WARN]` naming the pids when it finds any: on
  Windows a stray instance **silently absorbs** every later launch through the single-instance
  plugin, so a run that needed clearing is a run whose earlier phases are suspect. And the
  handover's scratch directory tolerates cleanup errors, because the inherited handle can still
  hold the log file for a moment after every check has passed.

- **`session_check.py` reported a wrong page instead of a wrong fixture.** Its target page is 7
  and `Viewer.goToPage` clamps to the last page, so a document with fewer than eight pages gave
  *"it opens on the remembered page: page 0, wanted 7"* --- stably, on a session restore that was
  working perfectly (verified afterwards at 7/7 on twenty pages). There is a named check for the
  precondition now. The first version of it read the count immediately after the viewer appeared
  and reported "0 pages" for every document, because the status it comes from is published a frame
  later; it waits for the value and keeps "never became known" distinct from "too short".

  **The named check did not finish the job, and the rest of it is fixed here.** It fails inside
  the `record` phase, which returns --- but the Python driver cannot see that, accumulates with
  `ok &= report(...)` and launched the other three phases anyway. So a short fixture still
  produced eleven failures, ten of them describing a restore that was never attempted, and the
  transcript still *ended* on `it opens on the remembered page: page 0, wanted 7` and `session
  restore is not verified`. The correction was only visible above the noise it was meant to
  correct, and these harnesses are redirected to a file and read from the tail --- which is why
  `live_output.py` exists at all.

  The driver now reads that check's verdict out of the transcript, skips the recorded-file
  comparison and the remaining three phases *by name*, and ends with `[FAIL] session restore was
  not tested: <fixture> has too few pages to reach page 7`. On `text-base14.pdf`: eleven failures
  to one, three launches not made, exit code still 1. The good fixture is unchanged --- 19 checks,
  same name list, exit 0.

  The check's name is duplicated into `session_check.py` to make that possible, which is a
  coupling rather than an assertion, so its *absence* from the transcript is reported as a failure
  of the script rather than read as "the fixture is fine". Proved by mutation: renaming it turns a
  green run red with *"this script cannot find a check named ... it has been renamed in
  sessioncheck.ts, so the too-short-fixture path below is now dead code"*. Verdicts are matched by
  splitting on the `[FAIL]` label rather than by column, because `Report` pads names to a fixed
  width and a parser that encodes the padding breaks silently the day a name grows past it.

- **The render deadline was not a deadline on Windows, and said it was.** `workers::kill_pid` ---
  the only thing that bounds a request that never comes back --- was `#[cfg(not(unix))] fn
  kill_pid(_pid: u32) {}`, and its own comment had named the trigger in advance: *"if a worker
  ever starts on Windows this has to become `TerminateProcess`, or the deadline silently stops
  being one."* Workers started there the day before.

  It did not merely fail to enforce. `kill_overdue` counted the pid, set the killed flag, and
  printed *"worker killed for exceeding its deadline"* --- so the caller got a deadline error, the
  log recorded a kill, and the process went on holding a hung PDFium render forever: one leaked
  worker per hung document, with a line in the log saying otherwise. The three tests covering the
  mechanism were `#[cfg(unix)]` too, so the platform where the guard had stopped working was also
  the platform where nothing tested it, and the suite was green.

  Now `OpenProcess` + `TerminateProcess(sandbox_win::KILLED_EXIT)`, using a distinct exit code
  because Windows has no signal number to carry "did not choose to exit". The three tests are
  un-gated, with a `ping`-based sleeper --- Windows has no `sleep`, and `timeout.exe` exits
  immediately under a redirected stdin, which would have made every assertion pass for the wrong
  reason. Proved by mutation: restoring the no-op turns
  `the_supervisor_kills_the_process_holding_an_overdue_call` red with `ExitStatus(0)` and takes
  the suite from 0.06 s to 5.08 s, the sleeper's own lifetime.

- **No tile was ever painted on Windows, and nothing reported an error.** `tiles.ts` fetched
  `tile://localhost/...`, which WebView2 cannot resolve --- it registers no custom URI schemes,
  so Tauri serves them at `http://tile.localhost/...` there. PDFium bound, the document parsed,
  pages laid out, the frame loop ran and every coverage check read `sharp=0.0%`: everything
  that does not need a tile worked. The origin now comes from Tauri's own `convertFileSrc`,
  for the origin *only* --- handed a whole path it percent-encodes the separators the server
  splits on. `shell.html` carried a second copy of the URL and now derives it the same way.
  The CSP names `http://tile.localhost` beside `tile:`; it already named `http://ipc.localhost`
  beside `ipc:`, so the convention was known and had been applied to one scheme and not the
  other.

- **`progressive::bind` was public so probes would not copy it, and lived where they could
  not reach it.** The doc comment said as much --- *"public so a probe can exercise this
  binding rather than a copy of it"* --- while the function sat in `worker_child`, which is
  `#[cfg(unix)]`. So on Windows the one shared binding was the one unreachable, and three
  probes had already written their own. Moved to `progressive`, re-exported from
  `worker_child` so `fdpass_probe.rs` still imports it beside the genuinely macOS-only
  `apply_sandbox`.

- **`viewer_check.py` discarded a passing run's stderr**, so every warning the app prints was
  invisible to exactly the runs that succeed. Adding the uncontained-backend `[WARN]` and
  then seeing a full-marks Windows run show no trace of it is how this surfaced; the first
  reading was that the warning had not fired. `[WARN]` lines are now echoed on a passing run,
  and nothing else is, so the webview's ordinary teardown noise does not come back with them.

- **`backend-probe` did not link off macOS**, which is what broke `npm run tauri build` on
  Windows. It is now a thin entry point over `backend_probe/imp.rs` that refuses off macOS ---
  the shape `fdpass_probe.rs` already used, and the honest one: every claim the probe makes is
  about a worker backend that cannot exist there, so it exits 2 with a reason rather than
  printing a table nobody should read.

- **A tile or thumbnail that arrived after its owner was destroyed leaked its bitmap.**
  Three paths, all the same shape: teardown withdraws everything outstanding, but withdrawal
  races the renderer, so anything that had already finished still lands. The scroller pushed
  it onto an arrival queue drained by a frame loop that no longer runs; the page strip kept
  it in a map that had just been emptied, including a *copy* it makes of a bitmap the
  scroller owns. An `ImageBitmap` is GPU-backed and released only by `close()`, so each of
  those is memory held until the process exits, once per tile in the race window.

  The guard is `src/lib/lifetime.ts`, and the reason it is a class rather than the boolean
  the earlier fixes used is that a boolean would not have fixed these: a continuation that
  sees a dead owner and merely returns early leaks exactly as much as one that queues the
  bitmap. `Lifetime.claim(live, dispose)` makes the disposal a required argument, so the
  guard cannot be written without saying what happens to the value it declines. The viewer's
  own `destroyed` flag is now one of these, unchanged in behaviour.

  Nine mutations, each caught by the test aimed at it --- except one that survived and was
  the point of running them: the strip's borrow path was unreachable from its fixture, whose
  `placeholderFor` returned null, so the disposal there had no test at all until a fixture
  that actually borrows was written. Every disposal test is paired with a control asserting
  a live arrival is still kept, since an owner that closed everything would pass the first
  set perfectly while drawing nothing.

- **A document closed while a text extraction was outstanding left the old viewer's frame
  loop running.** `destroy()` set no flag and `wake()` restarted the loop unconditionally, so
  a text load landing after destroy --- guaranteed, since the loader never rejects ---
  resurrected the dead viewer: fresh tile requests for a closed document, re-woken by its own
  backoff every 8 s for the life of the process, and status callbacks overwriting the *new*
  document's header and sidebar. A `destroyed` flag is now set first in `destroy()` and
  checked at the single choke point every continuation reaches.

- **"Select all on page" issued an unbounded stream of extraction calls on a page whose text
  could not be read** --- the retry re-entered on every resolution and a failed load caches
  nothing, so each iteration was a fresh IPC invoke, surviving destroy and document close.
  The continuation now re-enters only when text actually arrived.

- **A file that failed to open tore down the reader's current document.** The error path
  cleared the title --- which unmounts the document body --- even when the failure happened
  before anything about the current document was touched, leaving a live viewer on detached
  DOM and a header describing a document with no body under it. The cleanup now runs only if
  the old document was already released, and the header state is cleared together with the
  title, never separately.

- **Copying a selection spanning a page whose text could not be read put a silently
  incomplete string on the clipboard** --- the exact bug the copy path documents itself as
  existing to prevent. Completeness is now re-checked after the loads; a copy that cannot be
  completed reports instead of writing, as does a clipboard that refuses the write.

- **A post-fork descriptor shuffle could close the descriptor it had just installed.** `dup`
  returns the lowest free descriptor, so the scratch copy could land on a target number
  (document on 3, tile on 5, hole at 4), where the second `dup2` overwrote it and the cleanup
  then closed the installed copy --- a worker dying on a closed fd, intermittently, as a
  function of the parent's fd-table holes. Scratch descriptors are now identified against the
  same table that drives the installs, so the two cannot drift.

- **The page strip kept fetching after the document closed; Cmd-O could stack file dialogs;
  a pending find-debounce could fire at the newly opened document; a `tile://` request posted
  after the render service stopped left the webview's fetch pending forever.** Four small
  teardown holes, each now closed where the state lives.

- **A tile that failed was re-requested every frame, forever.** `Scroller.request()` runs on
  each frame and issues any wanted tile that is neither resident nor in flight; the failure
  paths deleted the in-flight entry and recorded nothing, so the next frame asked again — and
  the frame loop could not idle out, because the re-issued requests kept `pendingWork` above
  zero. Under the worker backend each attempt costs a `kill`, a fresh `fork`/`exec` and a full
  re-parse, so a page that faults deterministically had the application spawning and killing
  sandboxed processes at display cadence for as long as the document stayed open, with nobody
  touching the machine.

  `docs/THREAT-MODEL.md` §7 stated this was "bounded by the reader's own requests". It was
  not: the reader makes one and the frame loop made the rest, which is a bound written in
  prose and enforced nowhere. Now a per-request exponential backoff (250 ms doubling to 8 s),
  cleared only by a reader's own zoom, rotation or inversion — nothing on the frame path
  clears it. `Viewer` schedules exactly one wake per backoff so a transient failure still
  recovers, and `nextRetryMs` deliberately reports nothing for a request already due, or that
  wake would rebuild the busy loop one level up. `thumbnails.ts` gets the same treatment
  through its own `failed` set, and `RunStats`/`ViewerStatus` now carry a `failed` count, so a
  renderer erroring on everything no longer looks identical to one that is merely slow.

- **Two document opens could interleave, closing a live document and leaving two viewers on
  one element.** `openPath` suspends three times while mutating `openDoc`, `viewer`, `sidebar`
  and `openPathName`, and two of its six callers fire it without awaiting anything — the drop
  handler and the `OPEN_EVENT` listener. Double-clicking a second PDF while a large one was
  still opening had each call read the *other's* freshly-set `openDoc` as its `outgoing` and
  release the document the other was about to build a viewer on, while the second
  `new Viewer` overwrote the first without destroying it: two sets of live `wheel`, `keydown`
  and `pointerdown` listeners on the same element, and two sidebars in the DOM, since
  `Sidebar` appends rather than replacing. Opens are now serialised through a promise chain.
  The body no longer awaits `firstPaint()`, so a queued open waits for the real work and not
  for a one-second grace period that has nothing to do with it.

- **A tile request was bounded by the wire format and not by the mapping it is delivered
  through.** `protocol::parse` accepted any size up to 65535², and the refusal happened in the
  worker *after* `progressive::render_tile` had allocated `width × height × 4` and drawn into
  it — about 17 GB at the maximum, inside the process holding the attacker's document. Now
  refused at parse time. `doc`, `x` and `y` are range-checked there too rather than `as`-cast:
  a negative document number silently became `u32::MAX` and an origin past `i32` wrapped to a
  plausible one, which is the quiet coercion that parser refuses everywhere else.

- **The recursive graph walks in the print path had no depth bound.** `sweep::references` and
  `print::forget_in_object` run on a document we did not write, in the **app process**, and
  recursed until the stack ran out. Both now stop at `sweep::MAX_NESTING` (256) and **refuse**
  rather than truncating: a mark-and-sweep that stops early has an incomplete reachable set,
  so it would delete live objects and hand back a document that still parses and has holes in
  it. `collect` and `drop_pages` propagate that as an error.

- **⌘O was advertised in the palette and reached no handler at all, and ⌘P turned the page as
  well as printing.** The palette's shortcut labels were hand-written strings sitting twenty
  lines from the handlers that implement them, with nothing checking the two agreed — which
  `App.svelte` said out loud and called "a real gap and a small one". Both defects were in
  that gap: no ⌘O branch was ever written, and the viewer's `p` arm tested the key without the
  modifier, so it sat below the ⌘-guarded arms and caught ⌘P on the way past. Bindings are now
  data in `src/lib/keys.ts`; the label is *rendered from* the same modifiers `matches` tests,
  so the two cannot drift, and the table is covered by `keys.test.ts`.

- **`Queue` tracked one in-flight request while the pool ran several.** `inflight` was an
  `Option<(u64, CancelToken)>`, correct when the render service was one FIFO thread and wrong
  once the worker backend served the same queue from `pool + 2`: a second claim evicted the
  first, so withdrawing the older of two concurrent renders matched nothing in either table
  and cancelled nothing. The worker's own copy of the queue still stopped the render, so
  nothing looked broken — a safety net that could not fire. Now a `HashMap`, with `release`
  keyed on the request.

- **Closing a document left its renders in the queue.** `Scroller.destroy()` withdrew only
  tier-2 requests, and only when the `cancel` variant flag was set — a flag that exists so the
  benchmark can measure what withdrawal is worth. A teardown is not a variant: everything
  outstanding is now withdrawn unconditionally, so the outgoing document's tiles stop sitting
  in front of the first page of the file the reader has just opened. The placeholder arrival
  queue is closed with it.

- **`copySelection` issued one extraction per selected page at once.** A selection dragged to
  the end of the 775-page corpus named 775 pages and `Promise.all` put all of them on the FIFO
  queue that also draws the page in front of the reader — the cost `prefetchText` and
  `TextCache` both go out of their way to avoid, re-entering through the copy path. Chunked at
  16, rather than capped: a copy has to be complete.

- **`backend-probe` had a vanishing check of its own**, the second found in a day and by the
  same method. "The page asked for is one a wrong page number would betray" disappeared on
  one-page documents rather than skipping, and the only trace was the name count moving from
  32 to 31 between corpora. All six now report 32.

- **A viewer check vanished instead of skipping, on every one-page document.**
  `searchesFromHere` records two check names, and its two early returns skipped only the
  first --- so on a document with one page, `"finds something from the end of the document"`
  did not pass, did not fail, and did not appear at all. It had been that way since the check
  was written, through every green run and every mutation pass.

  Nothing red found it. It surfaced as an inconsistency *between corpora*: 86 check names on
  five of them and 85 on `text-cid`. That invariant --- the set of names is fixed, and a count
  that moves is itself a defect --- was written down when a check disappeared inside an
  `if let`; this is the first time it has caught one. A static scan for names that are
  recorded but never skipped is not a substitute: it returns 48 candidates, nearly all false,
  because a skip can be reached through a `const` or a call it cannot see. Diffing the name
  sets across corpora costs one `diff` and names the missing check exactly.

- **The test guarding `path_from_url`'s scheme check could not fail.** The behaviour was
  always correct; the test was not. `Url::to_file_path` rejects `https://example.com/a.pdf` on
  its own, because the host is a domain --- so deleting our scheme check broke nothing, which
  by the standing rule marks it a guard to delete. It is not: a `localhost` host is treated as
  *no host at all* whatever the scheme, so `https://localhost/a.pdf` resolves to `/a.pdf`. A
  second case covers that direction and goes red alone when the guard is removed.

- **A macOS double-click crashed the app before it could open anything.** `RunEvent::Opened`
  fires *before* Tauri's setup hook, so state registered there is not yet managed and
  `state::<Launch>()` panicked --- on precisely the path it existed to serve. No error, no
  output, an empty window, and `EXC_CRASH SIGABRT` in the crash reports. Registered on the
  builder now, before the event loop exists, and read with `try_state` so the same mistake
  would cost one document rather than the launch.

- **Every automated check reported success through its exit code, whatever it printed.**
  `AppHandle::exit(code)` ends Tauri's event loop; `App::run` then returns normally, `run()`
  returns, `main` returns unit, and the process exits **0** regardless. So
  `scripts/viewer_check.py`'s closing `return completed.returncode` could not fail, and had
  not been able to since it was written. The mutation harnesses read `[FAIL]` lines out of
  the transcript rather than `$?`, which is why their results were nonetheless correct --- the
  exit code was the one consumer with nothing to cross-check it.

  `spike_exit` now flushes stdout and calls `std::process::exit(code)`. Verified in **both**
  directions: a failing run exits 1 and a passing one exits 0, because a fix tested only
  against failure would be satisfied by exiting 1 unconditionally.

- **Character boxes and outline destinations on a page carrying `/Rotate`.** PDFium reports
  the page size *after* rotation and renders to match, but reports character boxes and
  destination coordinates in the page's own *unrotated* space --- so the flip against the
  reported height was correct at `/Rotate 0` and wrong at every other value. Measured with
  `text-probe --mode align` on a new fixture: 100% of character boxes landed on ink at 0 and
  **0.0% at 90, 180 and 270**. Every selection, every search highlight and the whole
  screen-reader reading order was elsewhere on any scanned page, in tidy rectangles.

  The turn is one function with two callers, and the probe now reports what each *wrong*
  rotation scores rather than only what the flip does --- on a rotated page those are
  different questions.

  Fixing it exposed a second defect that the first did not imply: characters are grouped into
  lines by vertical overlap, which on a page whose text runs down the screen puts each one on
  its own line, so the screen reader read the page **letter by letter**. Every text assertion
  still passed; what caught it was a comparison against an independent extraction, 877
  characters against 534.

  Reading the rotation needs a loaded page, which took the outline walk from **0.17 ms to
  7.5 ms** on a twelve-page fixture --- about 1 ms per distinct page named. The outline is
  therefore now requested after the first screen is painted rather than at open, since the
  render thread is FIFO. Thirteen mutations, all caught.


- **The viewer check printed nothing unless it reached the end.** Every result was buffered
  and emitted in one block, so a run that stopped midway was indistinguishable from one that
  never started --- which is exactly what happened when an occluded window suspended the
  page, and is why that took an afternoon to identify rather than a minute. Results now
  print as they are recorded, chained through one promise so the transcript cannot arrive
  out of order.
- **The watchdog says when a page was never executed at all.** Every spike entry point
  begins by asking Rust for its path, which records a `webview alive` mark; a timeout
  without one now prints that the page never ran a line of JavaScript, and why, instead of a
  mark list that has to be interpreted. It fires on a raw `cargo build` binary --- which
  runs no webview content at all, WKWebView needing the bundle identity --- and stays quiet
  on a bundled one.
- **`TPDF_RAISE=1`** raises the window for a check that has nowhere visible to put one.
  WebKit suspends a page whose window is fully covered, and an unlocked screen is not a
  visible window. Opt-in: raising a window over someone's work on every run is its own bug.
- **The sidebar's roving tabindex now follows focus that arrives from outside it** --- a Tab
  into the tree or a programmatic focus previously left every arrow key aimed elsewhere.
- **An outline destination no longer highlights the entry before the one clicked.** The air
  left above a heading on arrival is measured in points rather than CSS pixels, with a
  matching tolerance in the highlight.
- **A zero-length render slice ran to completion instead of pausing immediately.** The
  pause deadline used 0 as its "no deadline" sentinel, and `Instant` on Apple Silicon ticks
  at 41.67 ns --- so arming a zero slice right after taking the origin produced a genuinely
  zero elapsed time and hit the sentinel. Intermittent, and invisible to every identity
  check, because a render that never pauses is byte-identical to one that never had to.
- The PDFium install path assumed the macOS archive layout. Windows ships the loadable
  DLL at `bin/pdfium.dll` and only an import library in `lib/`. The fetch script now knows
  both; `pdfium_library_dir()` in `src-tauri/src/lib.rs` still does not, and is recorded
  as a known Windows defect.
