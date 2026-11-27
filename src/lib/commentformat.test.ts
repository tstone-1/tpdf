import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { CommentFormatting, paintComment, readComment } from "./commentformat";
import { installFakeDom, type FakeDom } from "./testdom";

describe("Comment formatting", () => {
  let dom: FakeDom;
  beforeEach(() => { dom = installFakeDom(); });
  afterEach(() => dom.restore());

  it("keeps one editable copy of the text with combined inline styles", () => {
    const editor = new CommentFormatting();
    editor.load("a<&𐐷b", [[1, 5, 7]]);
    expect(readComment(editor.field)).toEqual({ text: "a<&𐐷b", styles: [[1, 5, 7]] });
    expect(editor.field.getAttribute("role")).toBe("textbox");
    expect(Array.from(editor.element.children).slice(0, 4).map(b => b.textContent)).toEqual(["B", "I", "U", "…"]);
    expect(editor.element.lastChild).toHaveProperty("hidden", true);
    expect(editor.field.querySelector("span")?.style.fontWeight).toBe("bold");
  });

  it("restores struck, lowered and raised text using the original characters", () => {
    const box = document.createElement("div");
    const styles: [number, number, number][] = [[1, 2, 4 | 8 | 16], [4, 5, 1 | 32]];
    paintComment(box, "H2Om2", styles);
    expect(readComment(box)).toEqual({ text: "H2Om2", styles });
    expect(box.querySelector("s")?.querySelector("sub")).toBeTruthy();
    expect(box.querySelector("sup")).toBeTruthy();
  });

  it("reads blank lines and the final editable line after Enter", () => {
    const field = document.createElement("div");
    field.append("A");
    for (let i = 0; i < 2; i++) {
      const line = document.createElement("div");
      line.append(document.createElement("br"));
      field.append(line);
    }
    expect(readComment(field).text).toBe("A\n\n");
  });

  it("reads style changes to the selected occurrence among repeated letters", () => {
    const box = document.createElement("div");
    paintComment(box, "aaa", [[1, 2, 2]]);
    expect(readComment(box).styles).toEqual([[1, 2, 2]]);
    box.querySelector("span")!.remove();
    expect(readComment(box)).toEqual({ text: "aa", styles: [] });
  });
});
