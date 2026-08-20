import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { MarkPopup } from "./markpopup";
import { DEFAULT_SWATCH, PALETTE } from "./markcolors";
import type { MarkView } from "./pages";
import type { Anchor } from "./popup";
import { installFakeDom, type FakeDom } from "./testdom";

/** One of the reader's own marks, with the fields a test is not about filled in. */
function mark(over: Partial<MarkView> & { id: number }): MarkView {
  return {
    kind: "highlight",
    page: 1,
    quads: [72, 100, 300, 118],
    strokes: [],
    color: [1, 0.9, 0.2],
    note: "",
    lines: [],
    ...over,
  };
}

/** A mark's rectangle on screen, in the host's coordinates. */
function anchor(over: Partial<Anchor> = {}): Anchor {
  return { left: 100, top: 200, right: 300, bottom: 218, ...over };
}

describe("MarkPopup", () => {
  let dom: FakeDom;
  let sent: string[];
  /** Every id the box reported opening on, and every close as `null`. */
  let opened: (number | null)[];
  let closed: number;

  beforeEach(() => {
    dom = installFakeDom();
    sent = [];
    opened = [];
    closed = 0;
  });

  afterEach(() => {
    dom.restore();
  });

  function popup(): MarkPopup {
    dom.root.clientWidth = 900;
    dom.root.clientHeight = 700;
    return new MarkPopup(dom.root as unknown as HTMLElement, {
      onNote: (id, note) => sent.push(`note:${id}:${note}`),
      onRecolor: (id, color) => sent.push(`color:${id}:${color.join(",")}`),
      onRemove: (id) => sent.push(`remove:${id}`),
      onClose: () => {
        closed += 1;
      },
      // A list of its own, and not `sent`: this fires on every open and close,
      // so folding it in would put a line in front of every assertion below
      // about what the box sends on the reader's behalf.
      onOpen: (id) => opened.push(id),
    });
  }

  /**
   * The box a reader types in.
   *
   * Found rather than indexed. It was `children[1]` and the swatch row took that
   * slot, which is the trap about a check named by its position in a list ---
   * every test here that types would have failed together, all of them pointing
   * at the note rather than at the row that moved it.
   */
  function box(note: MarkPopup): { value: string } {
    const field = (
      note.node as unknown as { children: { value?: string }[] }
    ).children.find((child) => typeof child.value === "string");
    if (!field) throw new Error("the popup has no box to type in");
    return field as { value: string };
  }

  /** Presses the button whose label is `text`. */
  function press(note: MarkPopup, text: string): void {
    const rows = note.node as unknown as {
      children: {
        children: { textContent: string; dispatch: (t: string, e: object) => void }[];
      }[];
    };
    for (const row of rows.children) {
      for (const child of row.children ?? []) {
        if (child.textContent === text) {
          child.dispatch("pointerdown", {});
          return;
        }
      }
    }
    throw new Error(`no button says ${text}`);
  }

  /** Every string the box shows, flattened out of its rows. */
  function labels(note: MarkPopup): string[] {
    const rows = note.node as unknown as {
      children: { textContent?: string; children?: { textContent: string }[] }[];
    };
    const found: string[] = [];
    for (const row of rows.children) {
      for (const child of row.children ?? []) found.push(child.textContent);
    }
    return found;
  }

  /** The swatch labelled `name`, as the fake DOM lets a test poke it. */
  function swatch(
    note: MarkPopup,
    name: string,
  ): {
    getAttribute: (n: string) => string | null;
    dispatch: (t: string, e: object) => void;
  } {
    const found = (
      note.colorButtons as unknown as {
        getAttribute: (n: string) => string | null;
        dispatch: (t: string, e: object) => void;
      }[]
    ).find((button) => button.getAttribute("aria-label") === name);
    if (!found) throw new Error(`no swatch called ${name}`);
    return found;
  }

  it("offers every colour a mark can be, and not the default", () => {
    // The row is six of `PALETTE`'s seven. The seventh means "each kind's own
    // colour", which is a fact about marks nobody has made yet and is not a
    // colour a swatch could be drawn in --- see the module note. Asserted from
    // `PALETTE` rather than from a list written twice, so a colour added there
    // appears here without this test being the thing that forgets.
    const note = popup();
    const shown = note.colorButtons.map((button) =>
      (button as unknown as { getAttribute: (n: string) => string | null }).getAttribute(
        "aria-label",
      ),
    );
    expect(shown).toEqual(
      PALETTE.filter((entry) => entry.rgb !== null).map((entry) => entry.name),
    );
    expect(shown).not.toContain(DEFAULT_SWATCH.name);
    expect(shown.length).toBeGreaterThan(1);
  });

  it("shows which colour the mark it is open on is drawn in", () => {
    const note = popup();
    note.show(mark({ id: 7, color: [0.35, 0.8, 0.35] }), anchor(), false);
    expect(swatch(note, "green").getAttribute("aria-pressed")).toBe("true");
    expect(swatch(note, "yellow").getAttribute("aria-pressed")).toBe("false");

    // And follows the mark, for the reason the kind's labels do: a box built
    // once would keep the first mark's colour ringed over the second's.
    note.show(mark({ id: 8, color: [1, 0.9, 0.2] }), anchor(), false);
    expect(swatch(note, "yellow").getAttribute("aria-pressed")).toBe("true");
    expect(swatch(note, "green").getAttribute("aria-pressed")).toBe("false");
  });

  it("sends a colour the mark is not, and nothing for the one it is", () => {
    const note = popup();
    note.show(mark({ id: 7, color: [1, 0.9, 0.2] }), anchor(), false);

    swatch(note, "green").dispatch("pointerdown", {});
    expect(sent).toEqual(["color:7:0.35,0.8,0.35"]);

    // The comparison `edits.ts` says lives here. Without it a reader pressing
    // the swatch that is already ringed spends an undo step on nothing --- and
    // the row is exactly where that press happens, because the ring is what
    // invites it.
    swatch(note, "green").dispatch("pointerdown", {});
    expect(sent).toEqual(["color:7:0.35,0.8,0.35"]);
  });

  it("rings the colour that was pressed before the model has answered", () => {
    // The row is what the reader is looking at while they press it, so it moves
    // its own ring rather than waiting to be shown again. Without this a press
    // looks ignored until the state reply comes back and the popup is redrawn.
    const note = popup();
    note.show(mark({ id: 7, color: [1, 0.9, 0.2] }), anchor(), false);
    swatch(note, "blue").dispatch("pointerdown", {});
    expect(swatch(note, "blue").getAttribute("aria-pressed")).toBe("true");
    expect(swatch(note, "yellow").getAttribute("aria-pressed")).toBe("false");
  });

  it("names the kind of mark it is open on", () => {
    // Both the header and the button, because both said "highlight" when a
    // highlight was the only mark there was. A box that says Highlight over an
    // underline is wrong in the one place the application knows which mark the
    // reader means -- the Edit menu's item cannot, being chosen with the
    // pointer somewhere else, which is why it says "Remove mark".
    const note = popup();
    for (const [kind, word] of [
      ["highlight", "Highlight"],
      ["underline", "Underline"],
      ["strikeout", "Strikeout"],
    ] as const) {
      note.show(mark({ id: 7, kind }), anchor(), false);
      expect(labels(note)).toContain(word);
      expect(labels(note)).toContain(`Remove ${word.toLowerCase()}`);
    }
  });

  it("relabels itself when a mark of another kind takes the box", () => {
    // The control for the check above, and the one that fails if the labels are
    // written once when the box is built: showing a highlight first and an
    // underline second is exactly what a reader does, and a box built once
    // would then say Highlight over the underline.
    const note = popup();
    note.show(mark({ id: 7, kind: "highlight" }), anchor(), false);
    note.show(mark({ id: 8, kind: "strikeout" }), anchor(), false);
    expect(labels(note)).toContain("Strikeout");
    expect(labels(note)).not.toContain("Highlight");
  });

  it("shows what the mark says", () => {
    const note = popup();
    note.show(mark({ id: 7, note: "ask about this" }), anchor(), false);

    expect(note.openId).toBe(7);
    expect(note.text).toBe("ask about this");
  });

  it("sends the whole note when it closes, and only once", () => {
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    box(note).value = "typed";
    note.hide();

    expect(sent).toEqual(["note:7:typed"]);
    // Closed already, so a second close is a no-op rather than a second command
    // for a note nobody has touched since.
    note.hide();
    expect(sent).toEqual(["note:7:typed"]);
  });

  it("sends nothing for a note that was opened and not typed in", () => {
    // The control for the check above, and the reason the popup compares at all:
    // committing on every close would put a journal entry in for a reader who
    // opened a note to read it, which nothing in the document would show.
    const note = popup();
    note.show(mark({ id: 7, note: "already said" }), anchor(), false);
    note.hide();

    expect(sent).toEqual([]);
  });

  it("sends nothing when the mark is going", () => {
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    box(note).value = "typed";
    note.hide(false);

    expect(sent).toEqual([]);
  });

  it("commits the first mark's note when a second one takes the box", () => {
    // A reader clicking straight from one highlight to another. Without this the
    // first note is lost, and the loss is silent -- the box simply shows the new
    // mark's text.
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    box(note).value = "about the first";
    note.show(mark({ id: 8, note: "the second" }), anchor(), false);

    expect(sent).toEqual(["note:7:about the first"]);
    expect(note.openId).toBe(8);
    expect(note.text).toBe("the second");
  });

  it("reports which mark the box is on, including that it is on none", () => {
    // The marks panel's selection follows this, and it is fired here rather than
    // at the viewer's four `hide` calls --- which carry five reasons between
    // them: Escape and the close button share one, and the others are removing
    // the mark, an undo taking it out from under the box, the mark scrolling off
    // the page, and the viewer being torn down.
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    expect(opened).toEqual([7]);
    note.hide();
    expect(opened).toEqual([7, null]);
  });

  it("says nothing for a close that closed nothing, or an open on the same mark", () => {
    // The control for the check above. Without the first clause a panel would
    // clear its selection every time anything called `hide`; without the second
    // it would be told about a reopen that changed nothing, and a selection that
    // scrolls itself into view would jump for it.
    const note = popup();
    note.hide();
    expect(opened).toEqual([]);
    note.show(mark({ id: 7 }), anchor(), false);
    note.show(mark({ id: 7 }), anchor(), false);
    expect(opened).toEqual([7]);
  });

  it("reports a switch between marks as one open, not a close and an open", () => {
    // A reader clicking straight from one highlight to another. `show` commits
    // the first note without hiding the box, so a panel told `null` in between
    // would blink its selection off and on.
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    note.show(mark({ id: 8 }), anchor(), false);
    expect(opened).toEqual([7, 8]);
  });

  it("removes without sending what was typed", () => {
    // Types first, deliberately: a popup that committed unconditionally on the
    // way out would pass this if the box were left empty.
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    box(note).value = "typed and then thrown away";
    press(note, "Remove highlight");

    expect(sent).toEqual(["remove:7"]);
  });

  it("puts the keyboard in the note, and does nothing when none is open", () => {
    // For the keyboard walk, which opens the box without taking focus so that
    // the next press of the walk key steps again rather than typing a letter.
    // The guard is not decoration and is not reachable from the viewer either:
    // the Enter arm there already tests `markOpen`, so a mutation of one of them
    // is invisible through the other. This is where it can be seen.
    const note = popup();
    const field = note.field as unknown as { focused: boolean };

    note.focusField();
    expect(field.focused).toBe(false);

    note.show(mark({ id: 7 }), anchor(), false);
    expect(field.focused).toBe(false);
    note.focusField();
    expect(field.focused).toBe(true);
  });

  it("asks to be closed rather than closing itself", () => {
    // Escape and the close button both report; what actually closes the popup is
    // the viewer, which also has to put the keyboard back on the page. A popup
    // that hid itself here would leave `markOpen` saying it was still open.
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    (note.node as unknown as { dispatch: (t: string, e: object) => void }).dispatch(
      "keydown",
      { key: "Escape" },
    );

    expect(closed).toBe(1);
    expect(note.openId).toBe(7);

    press(note, "×");
    expect(closed).toBe(2);
  });
});
