import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CommentPopup, POPUP_WIDTH, type Anchor } from "./commentpopup";
import type { Comment } from "./comments";
import { installFakeDom, type FakeDom } from "./testdom";

/** One comment, with the fields a test is not about filled in plausibly. */
function comment(over: Partial<Comment> & { id: number }): Comment {
  return {
    page: 0,
    // Null by default: a fixture that is not about editing should not silently
    // claim its comment is editable. A test that IS about it says so.
    object: null,
    kind: "text",
    author: "Timo",
    body: "Check this figure.",
    subject: "",
    date: "2026-08-12 10:15",
    rect: [100, 100, 124, 124],
    quads: [],
    reply_to: null,
    hidden: false,
    ...over,
  };
}

/** A mark's rectangle on screen, in the host's coordinates. */
function anchor(over: Partial<Anchor> = {}): Anchor {
  return { left: 100, top: 200, right: 124, bottom: 224, ...over };
}

/** Every text node in the popup, which is what a reader sees. */
function words(popup: CommentPopup): string {
  const out: string[] = [];
  const walk = (node: { children: unknown[]; textContent: string }): void => {
    if (node.textContent) out.push(node.textContent);
    for (const child of node.children) {
      walk(child as { children: unknown[]; textContent: string });
    }
  };
  walk(popup.node as unknown as { children: unknown[]; textContent: string });
  return out.join(" ");
}

describe("CommentPopup", () => {
  let dom: FakeDom;
  let closed: number;

  beforeEach(() => {
    dom = installFakeDom();
    closed = 0;
  });

  afterEach(() => {
    dom.restore();
  });

  /** Every rewrite the popup sent, so a test can say it sent none. */
  let rewrites: { object: [number, number]; body: string }[] = [];

  function popup(): CommentPopup {
    // A host with a size, since the placement clamps against it.
    dom.root.clientWidth = 900;
    dom.root.clientHeight = 700;
    rewrites = [];
    return new CommentPopup(dom.root as unknown as HTMLElement, {
      onClose: () => {
        closed += 1;
      },
      onRewrite: (comment, body) => {
        // Recorded rather than asserted here: a recorder that threw would fail
        // the test that *provoked* the call rather than the one reading it.
        if (comment.object) rewrites.push({ object: comment.object, body });
      },
    });
  }

  it("shows nothing until it is asked to", () => {
    const note = popup();
    expect(note.openId).toBeNull();
    // Read out of `cssText` rather than `style.display`: the initial state is
    // set as one declaration string, and the fake DOM stores assignments
    // verbatim rather than parsing them into properties. `hide()` writes the
    // property, which is why the test below can read it.
    expect(note.node.style.cssText).toContain("display:none");
  });

  it("says who wrote it, when, and what they said", () => {
    const note = popup();
    note.show(comment({ id: 4 }), [], anchor(), false);
    expect(note.openId).toBe(4);
    const said = words(note);
    expect(said).toContain("Timo");
    expect(said).toContain("2026-08-12 10:15");
    expect(said).toContain("Check this figure.");
    expect(said).toContain("Note");
  });

  it("carries the replies with their own authors", () => {
    const note = popup();
    note.show(
      comment({ id: 0 }),
      [comment({ id: 1, author: "Reviewer", body: "It does not.", reply_to: 0 })],
      anchor(),
      false,
    );
    const said = words(note);
    expect(said).toContain("Reviewer");
    expect(said).toContain("It does not.");
  });

  it("says what kind of mark it is when nobody wrote anything", () => {
    // A highlight with no words is still a mark somebody made, and a blank box
    // reads as a defect.
    const note = popup();
    note.show(comment({ id: 0, kind: "highlight", body: "   " }), [], anchor(), false);
    expect(words(note)).toContain("Highlight, no comment");
  });

  it("shows the subject when there is one, and nothing when there is not", () => {
    const note = popup();
    note.show(comment({ id: 0, subject: "Figure 3" }), [], anchor(), false);
    const withSubject = note.node.children.length;
    expect(words(note)).toContain("Figure 3");
    note.show(comment({ id: 0, subject: "" }), [], anchor(), false);
    expect(note.node.children.length).toBe(withSubject - 1);
  });

  it("forgets the comment when it hides", () => {
    const note = popup();
    note.show(comment({ id: 7 }), [], anchor(), false);
    note.hide();
    expect(note.openId).toBeNull();
    expect(note.node.style.display).toBe("none");
    // Emptied rather than merely hidden: a check reading the note's text after
    // a close would otherwise be told what the last comment said.
    expect(words(note)).toBe("");
  });

  it("opens to the right of the mark when there is room", () => {
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor({ right: 124 }), false);
    expect(note.node.style.left).toBe("134px");
  });

  it("flips to the left of the mark when there is not", () => {
    // A mark near the right edge of a 900-wide window: 800 + 10 + 280 is past
    // it, so the note goes to the mark's left.
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor({ left: 780, right: 800 }), false);
    expect(note.node.style.left).toBe(`${780 - 10 - POPUP_WIDTH}px`);
  });

  it("never opens off the top of the window", () => {
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor({ top: -400, bottom: -380 }), false);
    expect(Number(note.node.style.top?.replace("px", ""))).toBeGreaterThanOrEqual(0);
  });

  it("moves without being rebuilt", () => {
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor(), false);
    const before = note.node.children.length;
    note.place(anchor({ left: 300, top: 400, right: 324, bottom: 424 }));
    expect(note.node.style.left).toBe("334px");
    expect(note.node.style.top).toBe("400px");
    expect(note.node.children.length).toBe(before);
  });

  it("ignores a move while it is closed", () => {
    // Called every frame by the viewer, including frames where nothing is open.
    const note = popup();
    note.place(anchor({ left: 500 }));
    expect(note.node.style.left).toBeUndefined();
  });

  it("closes on its own button", () => {
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor(), false);
    const button = note.node.children[0] as unknown as {
      getAttribute: (name: string) => string | null;
      dispatch: (type: string, event: object) => void;
    };
    expect(button.getAttribute("aria-label")).toBe("Close comment");
    button.dispatch("pointerdown", {});
    expect(closed).toBe(1);
  });

  it("closes on Escape", () => {
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor(), false);
    (note.node as unknown as { dispatch: (t: string, e: object) => void }).dispatch(
      "keydown",
      { key: "Escape" },
    );
    expect(closed).toBe(1);
    // The control: another key is not a close, or the handler is a close button
    // wearing a keyboard's clothes.
    (note.node as unknown as { dispatch: (t: string, e: object) => void }).dispatch(
      "keydown",
      { key: "a" },
    );
    expect(closed).toBe(1);
  });

  /** The popup's editor, or `null` while the body is text. */
  function editorOf(note: CommentPopup) {
    const found = (note.node as unknown as { children: { tagName: string }[] }).children.find(
      (child) => child.tagName === "textarea",
    );
    return (found ?? null) as unknown as { value: string; focused: boolean } | null;
  }

  it("offers no editor for a comment the file wrote without an object", () => {
    // The structural limit, and it is what `editable` answers. An Edit button
    // here would be a promise the file cannot keep.
    const note = popup();
    note.show(comment({ id: 1 }), [], anchor(), false);
    expect(note.editable).toBe(false);
    expect(words(note)).not.toContain("Edit");
    // And arming it anyway does nothing rather than throwing: the command's
    // `enabled` is a guard, not the only one.
    note.edit();
    expect(editorOf(note)).toBeNull();
  });

  it("arms an editor over the body and sends the new text when it closes", () => {
    const note = popup();
    note.show(comment({ id: 1, object: [12, 0] }), [], anchor(), false);
    expect(note.editable).toBe(true);
    expect(note.editing).toBe(false);

    note.edit();
    const box = editorOf(note);
    expect(box?.value).toBe("Check this figure.");
    // The keyboard goes into the box, or the reader has to click it as well as
    // press Edit.
    expect(box?.focused).toBe(true);
    expect(note.editing).toBe(true);
    // Nothing is sent while it is open: a journal entry between every two
    // letters is what makes undo useless.
    expect(rewrites).toEqual([]);

    if (box) box.value = "what I think";
    note.hide();
    expect(rewrites).toEqual([{ object: [12, 0], body: "what I think" }]);
  });

  it("sends nothing when the editor closes with the text it opened with", () => {
    // The control for the test above, and it is the case a reader reaches by
    // pressing Edit and changing their mind. An undo step for nothing is worse
    // than no undo step.
    const note = popup();
    note.show(comment({ id: 1, object: [12, 0] }), [], anchor(), false);
    note.edit();
    note.hide();
    expect(rewrites).toEqual([]);
  });

  it("sends nothing when the popup is hidden without committing", () => {
    // What a document being closed does, and what a comment disappearing out
    // from under the popup does. The text is lost, which is the smaller harm --
    // the alternative writes it to an object nobody is looking at.
    const note = popup();
    note.show(comment({ id: 1, object: [12, 0] }), [], anchor(), false);
    note.edit();
    const box = editorOf(note);
    if (box) box.value = "typed and thrown away";
    note.hide(false);
    expect(rewrites).toEqual([]);
  });

  it("commits the first comment when a second one takes the box over", () => {
    // A reader who opens another note has finished with this one, and the
    // words are already typed. `markpopup.ts` makes the same call.
    const note = popup();
    note.show(comment({ id: 1, object: [12, 0] }), [], anchor(), false);
    note.edit();
    const box = editorOf(note);
    if (box) box.value = "about the first";
    note.show(comment({ id: 2, object: [34, 0] }), [], anchor(), false);
    expect(rewrites).toEqual([{ object: [12, 0], body: "about the first" }]);
    // And the second opens read-only rather than inheriting the editor.
    expect(note.editing).toBe(false);
  });

  it("does not throw away what is typed when Edit is pressed twice", () => {
    // A second arming must not rebuild the box around the text. The failure is
    // silent: the reader keeps typing into a box whose value was just reset.
    const note = popup();
    note.show(comment({ id: 1, object: [12, 0] }), [], anchor(), false);
    note.edit();
    const box = editorOf(note);
    if (box) box.value = "half a sentence";
    note.edit();
    expect(editorOf(note)?.value).toBe("half a sentence");
  });

  it("takes the keyboard only when asked", () => {
    // Opened from the sidebar the reader is already on the keyboard; opened by
    // pressing the mark, taking focus would stop the arrow keys scrolling.
    const note = popup();
    note.show(comment({ id: 0 }), [], anchor(), false);
    expect((note.node as unknown as { focused: boolean }).focused).toBe(false);
    note.show(comment({ id: 0 }), [], anchor(), true);
    expect((note.node as unknown as { focused: boolean }).focused).toBe(true);
  });
});
