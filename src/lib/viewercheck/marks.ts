/**
 * Marks: their colours, notes, the panel that lists them, the keyboard, and comments.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import { INK_WIDTH } from "../markband";
import { swatch, PALETTE, DEFAULT_SWATCH } from "../markcolors";
import type { DocumentInfo } from "../ipc";
import type { Comments, Comment } from "../comments";
import {
  type MarkView,
  pageId,
  type RedactionView,
  markRows,
  unedited,
} from "../pages";
import { Sidebar } from "../sidebar";
import { Viewer } from "../viewer";
import {
  check,
  HEIGHT,
  key,
  pointer,
  settle,
  skip,
  WIDTH,
} from "./common";
import { coveredWordChecks, skipCoveredWords } from "./properties";

/**
 * A mark of the harness's own making, on the first page.
 *
 * Marks come from the model, and there is no model here --- so rather than drive
 * a highlight through the backend, the checks below hand the viewer the same
 * shape a state reply carries and press on it. What that leaves untested is the
 * model, which `docmodel.rs` and `edits.rs` test directly, and the mapping into
 * the file, which `annot-probe` measures against a rendered page. What it *does*
 * test is the half nothing else can reach: a rectangle on screen, a press
 * landing on it, and the note that opens.
 *
 * Deliberately not built from a selection, unlike {@link markQuadChecks}: this
 * is about the press rather than about where the rectangles came from, and a
 * synthetic one runs on the two corpora with no extractable text as well.
 *
 * Near the top of the page and well inside it, so the point is on screen once
 * the first page is, whatever the fixture's size --- `links-cropped.pdf`'s
 * displayed page is a fraction of its media box.
 */
export function syntheticMark(
  viewer: Viewer,
  id: number,
  note: string,
  /**
   * Where down the page the mark sits, as a fraction of its height.
   *
   * A parameter since the keyboard walk, which needs a *second* mark below the
   * first: a walk over one mark cannot tell "next" from "the only one there is",
   * and two at the same height cannot tell an order from a coincidence.
   */
  band = 0.06,
): MarkView {
  const size = viewer.pageSize(0);
  return {
    id,
    lines: [],
    // A highlight, because the note box's behaviour is what this phase tests
    // and every kind reaches it identically. What differs between the three is
    // written by `save.rs` and read back by `annot-probe`.
    kind: "highlight",
    stamp: null,
    // The *id* of the first page, which for an unedited document is 1 --- see
    // `pages.ts`. Sending 0 here would name no page and the mark would not be
    // drawn at all, which is the slot-for-identity confusion this type exists
    // to make visible.
    page: pageId(1),
    quads: [
      size.width_pt * 0.15,
      size.height_pt * band,
      size.width_pt * 0.6,
      size.height_pt * (band + 0.05),
    ],
    strokes: [],
    color: [1, 0.9, 0.2],
    width: INK_WIDTH,
    note,
  };
}

/**
 * One pending region on the first page, for a probe that needs one to exist.
 *
 * {@link syntheticMark}'s twin and placed the same way --- a band across the top
 * of the page, in the page's display space, which is the space
 * `RedactionView.area` is in. The *id* of the first page for that helper's
 * reason: a slot here would name no page and the region would not be drawn.
 */
export function syntheticRedaction(viewer: Viewer, id: number): RedactionView {
  const size = viewer.pageSize(0);
  return {
    id,
    page: pageId(1),
    area: [
      size.width_pt * 0.15,
      size.height_pt * 0.06,
      size.width_pt * 0.6,
      size.height_pt * 0.11,
    ],
  };
}

/** Where a synthetic mark's centre lands on screen, in client coordinates. */
function markCentre(viewer: Viewer, mark: MarkView): { x: number; y: number } {
  const [left, top, right, bottom] = [
    mark.quads[0] ?? 0,
    mark.quads[1] ?? 0,
    mark.quads[2] ?? 0,
    mark.quads[3] ?? 0,
  ];
  return viewer.screenPoint(0, (left + right) / 2, (top + bottom) / 2);
}

/** The names this phase reports, so a run that cannot start still prints them. */
const MARK_NOTE_CHECKS = [
  "a press on the reader's own mark opens its note",
  "the note shows what the mark says",
  "a press away from every mark opens no note",
  "closing a note that was typed in sends the whole note",
  "closing a note nobody typed in sends nothing",
  "a press on the page closes the note and keeps what was typed",
  "pressing the same mark again leaves what was typed alone",
  "removing a mark from its note sends a removal and no note",
  "a mark that goes while its note is open takes the note with it",
  "the keyboard walk opens a mark's note with no pointer at all",
  "the walk leaves the keyboard on the page and Enter moves it in",
  "the walk stops at the last mark rather than wrapping",
  "a key typed into a note does not move the page under it",
];

/**
 * The names the swatch-row phase reports.
 *
 * A named object rather than an array indexed by position, which is what
 * {@link MARK_NOTE_CHECKS} is --- and this file already carries the trap: a
 * check named by its place in a list is renamed by whatever is appended to it.
 * Appending six colour checks to that array would have renamed nothing and
 * still made every `MARK_NOTE_CHECKS[n]` above harder to read.
 */
const MARK_COLOR_CHECK = {
  row: "the note box offers a swatch for every colour a mark can be",
  rings: "and rings the one the mark is drawn in",
  sends: "pressing a swatch asks for that colour",
  same: "pressing the swatch a mark already wears asks for nothing",
  open: "a colour chosen with a note open recolours that mark",
  closed: "and one chosen with no note open recolours nothing",
} as const;

const MARK_COLOR_CHECKS: string[] = Object.values(MARK_COLOR_CHECK);

/**
 * The swatch row: what it offers, what it rings, and what a press asks for.
 *
 * **Separate from `markpopup.test.ts`, which drives the same row against a fake
 * DOM.** What that cannot reach is the row in the *shipped* bundle inside a real
 * web view: whether the buttons are laid out at all, whether a `pointerdown` on
 * one reaches its handler through the popup's own press guard, and whether the
 * ring `showColor` writes survives the popup being reopened by the frame loop.
 *
 * The last two checks are the palette's route rather than the pointer's ---
 * {@link Viewer.recolorOpenMark}, which every `Colour:` command reaches through
 * `App.svelte`. They are here because the *pair* is the assertion: a command
 * that recoloured whatever mark it found would pass the first alone.
 *
 * `edits` is the harness's record of what the viewer asked the model for; there
 * is no model here, so the callback is the observable.
 */
export async function markColorChecks(viewer: Viewer, edits: string[]): Promise<void> {
  viewer.goToPage(0);
  await settle(() => viewer.idle);

  const yellow = swatch("yellow");
  const green = swatch("green");
  if (!yellow?.rgb || !green?.rgb) {
    // Unreachable while `PALETTE` holds them, and said rather than assumed: the
    // whole phase is written against two named colours, and a rename there
    // would otherwise report the viewer as broken.
    for (const name of MARK_COLOR_CHECKS) {
      skip(name, "the palette has no yellow or no green to press");
    }
    return;
  }

  // Spread into a fresh tuple: `MarkView` comes from Rust and is mutable, and
  // the palette's entries are `readonly` because they are a constant table.
  const mark: MarkView = {
    ...syntheticMark(viewer, 4245, ""),
    color: [...yellow.rgb],
  };
  viewer.setMarks([mark]);
  await frame();
  viewer.showMark(mark.id);

  const buttons = [...viewer.markPopup.querySelectorAll("button")].filter(
    (button) => PALETTE.some((entry) => entry.name === button.getAttribute("aria-label")),
  );
  const offered = PALETTE.filter((entry) => entry !== DEFAULT_SWATCH);
  check(
    MARK_COLOR_CHECK.row,
    buttons.length === offered.length,
    `${buttons.length} swatches for ${offered.length} colours: ` +
      `[${buttons.map((b) => b.getAttribute("aria-label")).join(", ")}]`,
  );

  const pressed = (name: string): string | null =>
    buttons
      .find((button) => button.getAttribute("aria-label") === name)
      ?.getAttribute("aria-pressed") ?? null;
  check(
    MARK_COLOR_CHECK.rings,
    pressed("yellow") === "true" && pressed("green") === "false",
    `yellow=${pressed("yellow")}, green=${pressed("green")} on a yellow mark`,
  );

  /** Presses the swatch labelled `name`, as a reader does. */
  const pressSwatch = (name: string): void => {
    buttons
      .find((button) => button.getAttribute("aria-label") === name)
      ?.dispatchEvent(
        new PointerEvent("pointerdown", { bubbles: true, cancelable: true }),
      );
  };

  edits.length = 0;
  pressSwatch("green");
  check(
    MARK_COLOR_CHECK.sends,
    edits.join(",") === `color:${mark.id}:${green.rgb.join(",")}`,
    `sent [${edits.join(", ")}]`,
  );

  // The control, and the reason the row does the comparison rather than the
  // model: the ring is what invites a second press, so the swatch that is
  // already on is the one a reader is most likely to hit twice.
  edits.length = 0;
  pressSwatch("green");
  check(
    MARK_COLOR_CHECK.same,
    edits.length === 0,
    `sent [${edits.join(", ")}] pressing the ring that was already on`,
  );

  // The palette's route. The mark is still yellow as far as the viewer knows ---
  // there is no model to answer, so `setMarks` is what a state reply would be ---
  // which is exactly why green is asked for and lands.
  edits.length = 0;
  viewer.showMark(mark.id);
  const acted = viewer.recolorOpenMark(green.rgb);
  check(
    MARK_COLOR_CHECK.open,
    acted && edits.join(",") === `color:${mark.id}:${green.rgb.join(",")}`,
    `acted=${acted}, sent [${edits.join(", ")}]`,
  );

  edits.length = 0;
  viewer.closeMark();
  const idle = viewer.recolorOpenMark(green.rgb);
  check(
    MARK_COLOR_CHECK.closed,
    !idle && edits.length === 0,
    `acted=${idle}, sent [${edits.join(", ")}] with no note open`,
  );

  viewer.setMarks([]);
  viewer.closeMark();
  await frame();
}

/**
 * The note on one of the reader's own marks: opening it, typing, and removing.
 *
 * Every assertion here has its control beside it, and two of them are the whole
 * point of the phase:
 *
 * - **Closing sends the note only when it changed.** The pair is a close after
 *   typing and a close after not typing, because a popup that sent on every
 *   close would put a journal entry in for a reader who opened a note to read
 *   it --- which is not visible in the state, only in the undo stack.
 * - **Removing sends no note.** The mark is going, so committing first would
 *   journal a note onto a highlight the next command deletes, and undo would
 *   need two presses. The check types *first*, so a popup that committed
 *   unconditionally fails it.
 *
 * `edits` is the harness's record of what the viewer asked the model for ---
 * there is no model, so the callback is the observable.
 */
export async function markNoteChecks(
  root: HTMLElement,
  viewer: Viewer,
  edits: string[],
): Promise<void> {
  viewer.goToPage(0);
  await settle(() => viewer.idle);

  const mark = syntheticMark(viewer, 4242, "what it already said");
  viewer.setMarks([mark]);
  await frame();

  const centre = markCentre(viewer, mark);
  const bounds = root.getBoundingClientRect();
  if (
    centre.y < bounds.top + 4 ||
    centre.y > bounds.bottom - 4 ||
    centre.x < bounds.left + 4 ||
    centre.x > bounds.right - 4
  ) {
    // A window too small to show the top of the first page. Said rather than
    // failed: the checks below would report the viewer as broken for a fault in
    // the harness's own arithmetic.
    for (const name of MARK_NOTE_CHECKS) {
      skip(name, `the mark's centre is off screen at (${centre.x}, ${centre.y})`);
    }
    viewer.setMarks([]);
    return;
  }

  const openBefore = viewer.markOpen;
  pointer(root, "pointerdown", centre.x, centre.y);
  pointer(root, "pointerup", centre.x, centre.y);
  check(
    MARK_NOTE_CHECKS[0] ?? "",
    openBefore === -1 && viewer.markOpen === mark.id,
    `open=${viewer.markOpen} for #${mark.id} at (${centre.x.toFixed(0)}, ` +
      `${centre.y.toFixed(0)}), before=${openBefore}`,
  );

  check(
    MARK_NOTE_CHECKS[1] ?? "",
    viewer.markNoteText === mark.note,
    `box holds ${JSON.stringify(viewer.markNoteText)}, mark says ${JSON.stringify(mark.note)}`,
  );

  // The control for the press: the same gesture, on paper the mark does not
  // cover. Below the mark rather than beside it, because a page narrower than
  // the window has window either side of it and a press there reaches no page
  // at all --- which would pass this check without testing the hit test.
  viewer.closeMark();
  const belowMark = viewer.screenPoint(
    0,
    (mark.quads[0] ?? 0) + 10,
    (mark.quads[3] ?? 0) + 40,
  );
  pointer(root, "pointerdown", belowMark.x, belowMark.y);
  pointer(root, "pointerup", belowMark.x, belowMark.y);
  check(
    MARK_NOTE_CHECKS[2] ?? "",
    viewer.markOpen === -1,
    `open=${viewer.markOpen} after a press at (${belowMark.x.toFixed(0)}, ` +
      `${belowMark.y.toFixed(0)})`,
  );

  const typed = "read this again";
  edits.length = 0;
  viewer.showMark(mark.id);
  // Found once and reused below: the popup builds its box in its constructor and
  // reuses it, so this is the same element every time it opens. Looking it up
  // again per check would read as though it might be a different one.
  const box = viewer.markPopup.querySelector("textarea");
  if (!box) {
    for (const name of MARK_NOTE_CHECKS.slice(3)) {
      skip(name, "the note has no box to type in");
    }
    viewer.setMarks([]);
    return;
  }
  box.value = typed;
  // Escape rather than `closeMark`, so what is exercised is the key a reader
  // presses --- the popup's own handler, which commits on the way out.
  key(viewer.markPopup, "Escape");
  check(
    MARK_NOTE_CHECKS[3] ?? "",
    edits.join(",") === `note:${mark.id}:${typed}`,
    `sent [${edits.join(", ")}]`,
  );

  edits.length = 0;
  viewer.showMark(mark.id);
  key(viewer.markPopup, "Escape");
  check(
    MARK_NOTE_CHECKS[4] ?? "",
    edits.length === 0 && viewer.markOpen === -1,
    `sent [${edits.join(", ")}], open=${viewer.markOpen}`,
  );

  // The other way out of the box, and the one a reader reaches for without
  // thinking: press the page. It closes the note and commits, exactly as Escape
  // does --- one rule, written once in `onSelectStart`.
  edits.length = 0;
  viewer.showMark(mark.id);
  box.value = "typed and then clicked away from";
  pointer(root, "pointerdown", belowMark.x, belowMark.y);
  pointer(root, "pointerup", belowMark.x, belowMark.y);
  check(
    MARK_NOTE_CHECKS[5] ?? "",
    edits.join(",") === `note:${mark.id}:typed and then clicked away from` &&
      viewer.markOpen === -1,
    `sent [${edits.join(", ")}], open=${viewer.markOpen}`,
  );

  // And the press that must *not* close it: the mark whose note is already
  // open. Reopening would refill the box from the model, which still holds the
  // note as it was, so what the reader has typed since would go -- silently,
  // and only for the second click on the mark they are working on.
  edits.length = 0;
  viewer.showMark(mark.id);
  box.value = "still being typed";
  pointer(root, "pointerdown", centre.x, centre.y);
  pointer(root, "pointerup", centre.x, centre.y);
  check(
    MARK_NOTE_CHECKS[6] ?? "",
    edits.length === 0 &&
      viewer.markOpen === mark.id &&
      viewer.markNoteText === "still being typed",
    `sent [${edits.join(", ")}], open=${viewer.markOpen}, box holds ` +
      `${JSON.stringify(viewer.markNoteText)}`,
  );
  viewer.closeMark();

  edits.length = 0;
  viewer.showMark(mark.id);
  box.value = "typed and then thrown away";
  const remove = [...viewer.markPopup.querySelectorAll("button")].find(
    (button) => button.textContent === "Remove highlight",
  );
  remove?.dispatchEvent(
    new PointerEvent("pointerdown", { bubbles: true, cancelable: true }),
  );
  check(
    MARK_NOTE_CHECKS[7] ?? "",
    edits.join(",") === `remove:${mark.id}` && viewer.markOpen === -1,
    `sent [${edits.join(", ")}], open=${viewer.markOpen}, button ${remove ? "found" : "missing"}`,
  );

  // The undo case: a mark can go while its note is open, and the note must
  // close without sending what was in it. Sending would be refused by a model
  // that no longer has the mark, and the reader would see an error for an undo
  // they asked for.
  edits.length = 0;
  viewer.setMarks([mark]);
  viewer.showMark(mark.id);
  box.value = "typed while it was being undone";
  viewer.setMarks([]);
  await frame();
  await frame();
  check(
    MARK_NOTE_CHECKS[8] ?? "",
    edits.length === 0 && viewer.markOpen === -1,
    `sent [${edits.join(", ")}], open=${viewer.markOpen}`,
  );

  viewer.setMarks([]);
  viewer.closeMark();

  await markKeyboardChecks(root, viewer);
}

/** The names this phase reports, so a run that cannot start still prints them. */
const MARK_PANEL_CHECKS = [
  "the marks panel lists every mark the reader has made",
  "a row says what the mark says and what kind it is",
  "activating a row opens that mark's note and goes to it",
  "pressing a mark on the page selects its row",
  "closing the note clears the panel's selection",
  "a row's remove control asks for that mark and does not open it",
  "a mark nothing was typed on is listed by the words it covers",
];

/**
 * The panel listing the reader's own marks, in a real window.
 *
 * What no unit test reaches, and it is the same three things every panel phase
 * here is for. A row's text is read back out of a **real** DOM --- the fake one
 * the unit tests run against computes no aggregate `textContent`, so a row that
 * built nothing and a row that built the right thing are distinguishable there
 * only by walking positions. Activating a row is a real `pointerdown` on a real
 * element, and what it has to achieve is a *scroll*, which needs pages with
 * heights. And the selection following a press on the page is the `onMark`
 * wiring end to end: the popup fires it, the viewer forwards it, the panel
 * marks the row.
 *
 * There is no model behind the harness, so `setMarks` on both sides is what an
 * edit-state reply would be --- the same arrangement `markNoteChecks` uses, and
 * for the same reason.
 */
export async function markPanelChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  removed: number[],
  covers: Map<number, string>,
): Promise<void> {
  // The *last* page, and low down it. Two things this has to survive, both of
  // which the corpus sweep found by failing:
  //
  //   * On a one-page document the two marks are on the same page, so they must
  //     not also be at the same *height* --- at one band they land on top of
  //     each other and a press meant for the first opens the second.
  //   * The last page cannot reach the top of the viewport, so "went to it"
  //     cannot be asserted as the viewer's page number: a scroll to the end
  //     clamps, and on `rotated-90` that leaves the page before it at the top
  //     while the mark is plainly on screen. The trap is already recorded under
  //     that name; the assertion below is that the mark is *visible*, which is
  //     what the check is actually about and is what a viewer that opened the
  //     note without scrolling fails.
  const last = doc.page_count - 1;
  viewer.goToPage(0);
  await settle(() => viewer.idle);

  const here = syntheticMark(viewer, 4246, "on the first page", 0.06);
  const away = markOnPage(viewer, 4247, last, "low on the last page", 0.8);
  const marks = [here, away];
  viewer.setMarks(marks);
  sidebar.setMarks(markRows(marks, unedited(doc.page_count)));
  await frame();

  check(
    MARK_PANEL_CHECKS[0] ?? "",
    sidebar.marks.rowCount === marks.length,
    `${sidebar.marks.rowCount} rows for ${marks.length} marks, status ` +
      `${JSON.stringify(sidebar.marks.status)}`,
  );

  const text = sidebar.marks.rowText(here.id);
  check(
    MARK_PANEL_CHECKS[1] ?? "",
    text.note === here.note && text.kind === "Highlight" && text.page === "1",
    `note=${JSON.stringify(text.note)}, kind=${JSON.stringify(text.kind)}, ` +
      `page=${JSON.stringify(text.page)}`,
  );

  const row = sidebar.marks.elementFor(away.id);
  if (!row) {
    for (const name of MARK_PANEL_CHECKS.slice(2)) {
      skip(name, "the mark has no row to press");
    }
    viewer.setMarks([]);
    sidebar.setMarks([]);
    return;
  }
  row.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await settle(() => viewer.idle);
  await frame();
  const bounds = root.getBoundingClientRect();
  const landed = centreOn(viewer, away, last);
  const onScreen =
    landed.y > bounds.top &&
    landed.y < bounds.bottom &&
    landed.x > bounds.left &&
    landed.x < bounds.right;
  check(
    MARK_PANEL_CHECKS[2] ?? "",
    viewer.markOpen === away.id && onScreen,
    `open=${viewer.markOpen} for #${away.id}, its centre at ` +
      `(${landed.x.toFixed(0)}, ${landed.y.toFixed(0)}) in ` +
      `(${bounds.left.toFixed(0)}..${bounds.right.toFixed(0)}, ` +
      `${bounds.top.toFixed(0)}..${bounds.bottom.toFixed(0)}), viewer on page ` +
      `${viewer.position.page + 1} of ${doc.page_count}, mark on ${last + 1}`,
  );
  viewer.closeMark();

  // Back to the first page, and pressed on the *page* rather than in the panel.
  // The row that lights up is the whole of the `onMark` wiring: the popup
  // reports which mark it opened on, the viewer forwards it, the panel marks
  // the row. Nothing in this phase told the panel which mark that was.
  viewer.goToPage(0);
  await settle(() => viewer.idle);
  sidebar.marks.select(null);
  const centre = markCentre(viewer, here);
  if (
    centre.y < bounds.top + 4 ||
    centre.y > bounds.bottom - 4 ||
    centre.x < bounds.left + 4 ||
    centre.x > bounds.right - 4
  ) {
    for (const name of MARK_PANEL_CHECKS.slice(3)) {
      skip(name, `the mark's centre is off screen at (${centre.x}, ${centre.y})`);
    }
    viewer.setMarks([]);
    sidebar.setMarks([]);
    return;
  }
  const before = sidebar.marks.selectedId;
  pointer(root, "pointerdown", centre.x, centre.y);
  pointer(root, "pointerup", centre.x, centre.y);
  await frame();
  check(
    MARK_PANEL_CHECKS[3] ?? "",
    before === -1 && sidebar.marks.selectedId === here.id,
    `selected=${sidebar.marks.selectedId} for #${here.id}, was ${before}`,
  );

  viewer.closeMark();
  await frame();
  check(
    MARK_PANEL_CHECKS[4] ?? "",
    sidebar.marks.selectedId === -1,
    `selected=${sidebar.marks.selectedId} with no note open`,
  );

  // The remove control, and this is the half no unit test can reach: the fake
  // DOM does not bubble, so the `stopPropagation` that keeps the row's own
  // `pointerdown` from firing first -- opening the note of the mark being taken
  // off -- has no failing case there. Here the events bubble, so "the note did
  // not open" is a real assertion rather than a property of the test double.
  const removeRow = sidebar.marks.elementFor(here.id);
  const control = removeRow
    ? ([...removeRow.children] as HTMLElement[]).find(
        (child) => child.dataset.part === "remove",
      )
    : undefined;
  if (!control) {
    skip(MARK_PANEL_CHECKS[5] ?? "", "the row has no remove control to press");
  } else {
    const asked = removed.length;
    const openBefore = viewer.markOpen;
    control.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    control.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    await frame();
    check(
      MARK_PANEL_CHECKS[5] ?? "",
      removed.length === asked + 1 &&
        removed.at(-1) === here.id &&
        viewer.markOpen === openBefore,
      `${removed.length - asked} ask(s), last=${removed.at(-1)} for #${here.id}, ` +
        `markOpen=${viewer.markOpen} (was ${openBefore})`,
    );
  }

  // The words a highlight sits on, standing in for the note nobody typed.
  //
  // **Two rows in one paint, and the noted one is the control.** The unit tests
  // already assert the substitution rule and the flag; what they cannot assert
  // is that the two rows come out *looking* different, because the fake DOM
  // they run against resolves no styles at all. A reader's own sentence and a
  // sentence lifted off the page are both text in the same column, and the only
  // thing separating them is that one is dimmed and italic -- so a panel that
  // drew them alike would pass every test there is and still tell a reader they
  // wrote something they did not.
  //
  // The bare mark is a copy of the noted one with a fresh id, so both are on the
  // same page at the same band. They overlap, which would matter if anything
  // here pressed them; nothing does.
  const bare = { ...here, id: here.id + 2, note: "" };
  const words = "the words this highlight sits on";
  covers.set(bare.id, words);
  const both = [here, bare];
  viewer.setMarks(both);
  sidebar.setMarks(markRows(both, unedited(doc.page_count)));
  await frame();
  const bareText = sidebar.marks.rowText(bare.id);
  const notedText = sidebar.marks.rowText(here.id);
  const style = (id: number): string => {
    const row = sidebar.marks.elementFor(id);
    const line = row?.querySelector('[data-part="note"]');
    return line ? getComputedStyle(line).fontStyle : "no row";
  };
  const bareStyle = style(bare.id);
  const notedStyle = style(here.id);
  check(
    MARK_PANEL_CHECKS[6] ?? "",
    bareText.note === words &&
      !bareText.own &&
      notedText.own &&
      bareStyle === "italic" &&
      notedStyle !== bareStyle,
    `covered row ${JSON.stringify(bareText.note)} own=${bareText.own} ` +
      `${bareStyle}, noted row ${JSON.stringify(notedText.note)} ` +
      `own=${notedText.own} ${notedStyle}`,
  );
  covers.delete(bare.id);

  viewer.setMarks([]);
  sidebar.setMarks([]);
  await frame();
}

/**
 * A synthetic mark in the top-left corner of one page, addressed by slot.
 *
 * {@link syntheticMark}'s sibling, and it exists because that one is fixed to
 * the first page --- which is right for the note checks and useless for a panel
 * row whose job is to take the reader somewhere. The page **id** is the slot
 * plus one, which is true of an unedited document and is the assumption the
 * whole harness runs on; see `pages.ts`.
 */
function markOnPage(
  viewer: Viewer,
  id: number,
  slot: number,
  note: string,
  band: number,
): MarkView {
  const size = viewer.pageSize(slot);
  return {
    id,
    kind: "highlight",
    stamp: null,
    page: pageId(slot + 1),
    quads: [
      size.width_pt * 0.15,
      size.height_pt * band,
      size.width_pt * 0.6,
      size.height_pt * (band + 0.05),
    ],
    strokes: [],
    color: [1, 0.9, 0.2],
    width: INK_WIDTH,
    note,
    lines: [],
  };
}

/**
 * Where a mark's centre lands on screen, on the page it is actually on.
 *
 * {@link markCentre} asks `screenPoint` about **page 0**, which is right for
 * every mark `syntheticMark` builds and wrong for one placed anywhere else ---
 * the answer is then a point on the first page, which on a multi-page document
 * is off screen exactly when the mark is on it.
 */
function centreOn(
  viewer: Viewer,
  mark: MarkView,
  slot: number,
): { x: number; y: number } {
  const [left, top, right, bottom] = [
    mark.quads[0] ?? 0,
    mark.quads[1] ?? 0,
    mark.quads[2] ?? 0,
    mark.quads[3] ?? 0,
  ];
  return viewer.screenPoint(slot, (left + right) / 2, (top + bottom) / 2);
}

/**
 * Reaching a mark without a pointer, and typing in the note once you are there.
 *
 * The two halves are one increment and each is the other's precondition. The
 * walk is the only route to a mark that is not a press --- before it, a
 * highlight's note could not be read, changed or taken off from the keyboard at
 * all. The guard is what makes the box it lands in safe to type in: every key
 * this viewer handles used to fire while the reader was typing, so "n" turned
 * the page under the note and the space bar scrolled it away.
 *
 * In a real webview rather than only in vitest, because the thing being asserted
 * is that a key **bubbles** from the field to the root handler and is refused
 * there. The unit test dispatches on the root with a `target` of its own
 * choosing, which is a statement about the handler; this is a statement about
 * the DOM the handler is actually installed in.
 */
async function markKeyboardChecks(root: HTMLElement, viewer: Viewer): Promise<void> {
  viewer.goToPage(0);
  await settle(() => viewer.idle);

  const first = syntheticMark(viewer, 4244, "the first one", 0.06);
  const second = syntheticMark(viewer, 4245, "the second one", 0.2);
  viewer.setMarks([first, second]);
  await frame();

  const walked = viewer.stepMark(1);
  check(
    MARK_NOTE_CHECKS[9] ?? "",
    walked && viewer.markOpen === first.id,
    `stepped=${walked}, open=${viewer.markOpen}, wanted #${first.id}`,
  );

  const field = viewer.markNoteField;
  const beforeEnter = document.activeElement === field;
  key(root, "Enter");
  check(
    MARK_NOTE_CHECKS[10] ?? "",
    !beforeEnter && document.activeElement === field,
    `focused before Enter=${beforeEnter}, after=${document.activeElement === field}`,
  );

  // Back to the page, or the walk key below would go to the field the line
  // above just focused --- which is the guard doing its job, and not what this
  // check is about.
  root.focus();
  const toSecond = viewer.stepMark(1);
  const past = viewer.stepMark(1);
  check(
    MARK_NOTE_CHECKS[11] ?? "",
    toSecond && !past && viewer.markOpen === second.id,
    `to the second=${toSecond}, past the end=${past}, open=${viewer.markOpen}`,
  );

  // The pair. A key from the field must not move the page and the same key from
  // the page must move it: a guard tested only on its refusal is satisfied by a
  // viewer that ignores every key.
  viewer.goToPage(0);
  await settle(() => viewer.idle);
  const start = viewer.offset;
  field.dispatchEvent(
    new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }),
  );
  const typedInto = viewer.offset;
  key(root, "ArrowDown");
  const pressedOn = viewer.offset;
  check(
    MARK_NOTE_CHECKS[12] ?? "",
    typedInto === start && pressedOn > start,
    `offset ${start} -> ${typedInto} typing into the note, ` +
      `-> ${pressedOn} pressing on the page`,
  );

  viewer.setMarks([]);
  viewer.closeMark();
}

/**
 * The comments: the panel that lists them, and the note that opens on the page.
 *
 * Most of the corpus has no annotations at all, so nearly everything here is
 * conditional --- and every condition is a `skip` with its reason rather than a
 * missing row, for the reason this whole file is arranged around.
 *
 * Two assertions carry the weight and each carries its own control:
 *
 * - **Pressing a mark opens its note**, asserted after checking that no note was
 *   open beforehand. Without that, a check that opened one for its own
 *   convenience would pass on a press that did nothing.
 * - **The note says what the file says**, compared against the body the backend
 *   returned rather than against "some text is showing". A popup rendering an
 *   empty string is a popup, and the check that only asks whether one is open
 *   cannot tell the two apart.
 *
 * The press lands on the *centre* of a comment's rectangle. That is not a
 * convenience either: `hitTest` allows a few points of slack around the edge, so
 * a press aimed at a corner would pass whether or not the rectangle is where the
 * scan says it is.
 */
export async function commentChecks(
  root: HTMLElement,
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
): Promise<void> {
  const names = [
    "the sidebar lists every comment",
    "a reply is drawn under the comment it answers",
    "pressing a mark on the page opens its note",
    "the note shows what the comment says",
    "a reply appears in the note with its own author",
    "pressing away from a mark closes the note",
    "activating a row opens that comment's note",
  ];

  let comments: Comments;
  try {
    comments = await invoke<Comments>("document_comments", { doc: doc.id });
  } catch (e) {
    check("reads the document's comments", false, String(e));
    for (const name of names) skip(name, "the comments could not be read");
    skipCoveredWords("the comments could not be read");
    return;
  }
  check(
    "reads the document's comments",
    Array.isArray(comments.items),
    `${comments.items.length} comments in ${comments.scan_ms.toFixed(2)} ms`,
  );

  sidebar.setComments(comments);
  viewer.setComments(comments.items);

  if (comments.items.length === 0) {
    for (const name of names) skip(name, "the document has no comments");
    skipCoveredWords("the document has no comments");
    return;
  }

  check(
    "the sidebar lists every comment",
    sidebar.comments.rowCount === comments.items.length,
    `${sidebar.comments.rowCount} rows for ${comments.items.length} comments`,
  );

  const reply = comments.items.find(
    (item) => item.reply_to !== null && comments.items.some((other) => other.id === item.reply_to),
  );
  if (!reply) {
    skip("a reply is drawn under the comment it answers", "no comment is a reply");
  } else {
    const row = sidebar.comments.elementFor(reply.id);
    const describes = row?.getAttribute("aria-describedby") ?? "";
    check(
      "a reply is drawn under the comment it answers",
      describes === `tpdf-comment-${reply.reply_to}`,
      `describedby=${describes || "(none)"} for a reply to #${reply.reply_to}`,
    );
  }

  // A mark with a real rectangle that the document shows. A hidden one is
  // deliberately not clickable, and one with no area was never placed.
  const mark = comments.items.find(
    (item) =>
      !item.hidden && item.rect[2] - item.rect[0] > 2 && item.rect[3] - item.rect[1] > 2,
  );
  if (!mark) {
    for (const name of names.slice(2)) {
      skip(name, "no comment has a rectangle on the page");
    }
    // The covered-word checks need a bare mark with rectangles, not a mark the
    // press phase can reach, so they are stood down separately rather than being
    // folded into the loop above.
    skipCoveredWords("no comment has a rectangle on the page");
    return;
  }

  viewer.goToPage(mark.page);
  await frame();
  await frame();
  await frame();
  const centre = viewer.screenPoint(
    mark.page,
    (mark.rect[0] + mark.rect[2]) / 2,
    (mark.rect[1] + mark.rect[3]) / 2,
  );
  // The control: nothing may be open before the press, or "a note is open"
  // afterwards says nothing about the press.
  const wasOpen = viewer.commentOpen;
  pointer(root, "pointerdown", centre.x, centre.y);
  pointer(root, "pointerup", centre.x, centre.y);
  check(
    "pressing a mark on the page opens its note",
    wasOpen === -1 && viewer.commentOpen === mark.id,
    `open=${viewer.commentOpen} for #${mark.id} at (${centre.x.toFixed(0)}, ${centre.y.toFixed(0)}), ` +
      `before=${wasOpen}`,
  );

  const said = viewer.commentText;
  const wanted = mark.body.trim() || mark.author.trim();
  check(
    "the note shows what the comment says",
    wanted.length > 0 ? said.includes(wanted) : said.length > 0,
    `note says ${JSON.stringify(said.slice(0, 60))}, wanted ${JSON.stringify(wanted.slice(0, 60))}`,
  );

  const answered = comments.items.filter((item) => item.reply_to === mark.id);
  if (answered.length === 0) {
    skip(
      "a reply appears in the note with its own author",
      "nothing replies to the comment under the pointer",
    );
  } else {
    const first = answered[0] as Comment;
    check(
      "a reply appears in the note with its own author",
      said.includes(first.body.trim()) &&
        (first.author.trim() === "" || said.includes(first.author.trim())),
      `note carries ${answered.length} repl${answered.length === 1 ? "y" : "ies"}`,
    );
  }

  // Away from every mark on this page: the far corner of the viewport, which no
  // rectangle in the corpus reaches. Asserted as a *change*, so a note that had
  // already closed itself does not read as this press closing it.
  const openBefore = viewer.commentOpen;
  pointer(root, "pointerdown", WIDTH - 20, HEIGHT - 20);
  pointer(root, "pointerup", WIDTH - 20, HEIGHT - 20);
  check(
    "pressing away from a mark closes the note",
    openBefore !== -1 && viewer.commentOpen === -1,
    `open ${openBefore} -> ${viewer.commentOpen}`,
  );

  // The *last* comment with a rectangle, and the viewer is sent back to the
  // top first: activating a row has to take the reader to the comment, and a
  // row for something already on screen cannot tell that from a popup that
  // opened where it stood. On a one-page document the two are the same comment
  // and the movement half is vacuous, which is why the detail prints both pages.
  const far = [...comments.items]
    .reverse()
    .find(
      (item) =>
        !item.hidden && item.rect[2] - item.rect[0] > 2 && item.rect[3] - item.rect[1] > 2,
    );
  const row = far ? sidebar.comments.elementFor(far.id) : null;
  if (!far || !row) {
    skip("activating a row opens that comment's note", "the comment has no row");
    skipCoveredWords("the comment has no row");
    return;
  }
  viewer.goToStart();
  await settle(() => viewer.idle);
  row.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
  await frame();
  await frame();
  check(
    "activating a row opens that comment's note",
    viewer.commentOpen === far.id && viewer.position.page === far.page,
    `open=${viewer.commentOpen} for #${far.id}, viewer on page ${viewer.position.page + 1} ` +
      `of ${doc.page_count}, comment on ${far.page + 1}`,
  );
  // Left closed, so nothing after this runs with a note over the page.
  viewer.closeComment();

  await coveredWordChecks(viewer, sidebar, comments.items);
}
