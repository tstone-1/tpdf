import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { MarkPopup } from "./markpopup";
import type { MarkView } from "./pages";
import type { Anchor } from "./popup";
import { installFakeDom, type FakeDom } from "./testdom";

/** One of the reader's own marks, with the fields a test is not about filled in. */
function mark(over: Partial<MarkView> & { id: number }): MarkView {
  return {
    kind: "highlight",
    page: 1,
    quads: [72, 100, 300, 118],
    color: [1, 0.9, 0.2],
    note: "",
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
  let closed: number;

  beforeEach(() => {
    dom = installFakeDom();
    sent = [];
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
      onRemove: (id) => sent.push(`remove:${id}`),
      onClose: () => {
        closed += 1;
      },
    });
  }

  /** The box, which is the popup's second child. */
  function box(note: MarkPopup): { value: string } {
    const field = (note.node as unknown as { children: { value: string }[] })
      .children[1];
    if (!field) throw new Error("the popup has no box to type in");
    return field;
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

  it("removes without sending what was typed", () => {
    // Types first, deliberately: a popup that committed unconditionally on the
    // way out would pass this if the box were left empty.
    const note = popup();
    note.show(mark({ id: 7 }), anchor(), false);
    box(note).value = "typed and then thrown away";
    press(note, "Remove highlight");

    expect(sent).toEqual(["remove:7"]);
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
