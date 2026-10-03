import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { CHOICES, type Pictures, type Shrinkage } from "./compress";
import { CompressDialog, DIALOG_CLASS, isPicture } from "./compressdialog";
import { installFakeDom, settle, type FakeDom, type FakeElement } from "./testdom";

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
});

afterEach(() => {
  dom.restore();
});

type Field = FakeElement & { value: string; checked: boolean; disabled: boolean };

const BEFORE = 10_000_000;

/** What each way comes to: the finer the pictures, the larger the copy. */
function answerFor(pictures: Pictures | null): Shrinkage {
  const after = pictures === null ? 9_900_000 : pictures.dpi * pictures.quality * 40;
  return {
    bytesBefore: BEFORE,
    bytesAfter: after,
    pictures: 4,
    picturesChanged: pictures === null ? 0 : 3,
    sample:
      pictures === null
        ? null
        : {
            width: 320,
            height: 320,
            before: "data:image/png;base64,NOW",
            after: `data:image/png;base64,AT${pictures.dpi}`,
            page: 2,
            zoomPercent: 200,
            dpiBefore: 600,
            dpiAfter: pictures.dpi,
          },
  };
}

/** The dialog, and the pieces a reader touches. */
function open(estimate = async (pictures: Pictures | null) => answerFor(pictures)) {
  const host = dom.root;
  const dialog = new CompressDialog(host as unknown as HTMLElement);
  const backdrop = host.children.find((c) => c.classList.contains(DIALOG_CLASS));
  const panel = backdrop?.children[0];
  if (!backdrop || !panel) throw new Error("the dialog did not mount");
  const [heading, list, said, preview, problem, buttons] = panel.children as [
    FakeElement, FakeElement, FakeElement, FakeElement, FakeElement, FakeElement,
  ];
  /** The element down this path of children, or a failure that names it. */
  const at = (from: FakeElement, ...path: number[]): FakeElement => {
    let found = from;
    for (const index of path) {
      const next = found.children[index];
      if (!next) throw new Error(`the dialog has no child ${path.join(".")}`);
      found = next;
    }
    return found;
  };
  const radio = (row: number) => at(list, row, 0) as Field;
  const comes = (row: number) => at(list, row, 2).textContent;
  const [dpi, quality, jpeg] = [
    at(list, 4, 3, 0, 1),
    at(list, 4, 3, 1, 1),
    at(list, 4, 3, 2, 0),
  ] as [Field, Field, Field];
  const [before, after] = [at(preview, 0, 0), at(preview, 0, 1)];
  const under = at(preview, 1);
  const [cancel, save] = [at(buttons, 0), at(buttons, 1)] as [Field, Field];
  const asked: (Pictures | null)[] = [];
  const answer = dialog.ask("report.pdf", (pictures) => {
    asked.push(pictures);
    return estimate(pictures);
  });
  return {
    dialog, backdrop, heading, said, preview, problem, radio, comes, dpi, quality, jpeg,
    before, after, under, cancel, save, asked, answer,
  };
}

describe("CompressDialog", () => {
  it("names the file and works out every ready choice, one after another", async () => {
    const d = open();
    expect(d.heading.textContent).toBe("A smaller copy of report.pdf");
    expect(d.dialog.isOpen).toBe(true);
    expect([0, 1, 2, 3].map(d.comes)).toEqual(Array(4).fill("Working it out..."));
    expect(d.comes(4)).toBe("");
    expect(d.save.disabled).toBe(true);
    await settle(60);
    expect(d.asked).toEqual(CHOICES.map((choice) => choice.pictures));
    expect([0, 1, 2, 3].map(d.comes)).toEqual([
      "9.4 MB, 1% smaller",
      "996 KB, 89% smaller",
      "439 KB, 95% smaller",
      "258 KB, 97% smaller",
    ]);
  });

  it("starts on the choice that loses nothing, with no picture to show", async () => {
    const d = open();
    await settle(60);
    expect(d.radio(0).checked).toBe(true);
    expect(d.said.textContent).toBe("From 9.5 MB to about 9.4 MB. No picture changes.");
    expect(d.preview.style.display).toBe("none");
    expect(d.save.disabled).toBe(false);
    d.save.dispatch("click", {});
    await expect(d.answer).resolves.toEqual({ pictures: null, shrinkage: answerFor(null) });
    expect(d.dialog.isOpen).toBe(false);
  });

  it("shows the selected choice's size and the page before and after", async () => {
    const d = open();
    await settle(60);
    d.radio(3).dispatch("change", {});
    expect([0, 1, 2, 3].map((at) => d.radio(at).checked)).toEqual([false, false, false, true]);
    expect(d.said.textContent).toBe(
      "From 9.5 MB to about 258 KB. 3 of 4 pictures are stored smaller.",
    );
    expect(d.preview.style.display).toBe("block");
    expect(d.before.getAttribute("src")).toBe("data:image/png;base64,NOW");
    expect(d.after.getAttribute("src")).toBe("data:image/png;base64,AT110");
    expect(d.under.textContent).toContain("from 600 to 110 pixels an inch");
    d.save.dispatch("click", {});
    await expect(d.answer).resolves.toEqual({
      pictures: { dpi: 110, quality: 60, jpeg: true },
      shrinkage: answerFor({ dpi: 110, quality: 60, jpeg: true }),
    });
  });

  it("works out the reader's own numbers when a field is left, and saves with them", async () => {
    const d = open();
    await settle(60);
    d.dpi.value = "96";
    d.dpi.dispatch("change", {});
    expect(d.radio(4).checked).toBe(true);
    expect(d.said.textContent).toBe("Working it out...");
    expect(d.save.disabled).toBe(true);
    await settle(60);
    const own = { dpi: 96, quality: 75, jpeg: true };
    expect(d.asked.at(-1)).toEqual(own);
    expect(d.comes(4)).toBe("281 KB, 97% smaller");
    d.jpeg.checked = false;
    d.jpeg.dispatch("change", {});
    await settle(60);
    expect(d.asked.at(-1)).toEqual({ ...own, jpeg: false });
    d.save.dispatch("click", {});
    const chosen = await d.answer;
    expect(chosen?.pictures).toEqual({ ...own, jpeg: false });
  });

  it("says what is wrong with numbers that are not numbers, and asks nothing", async () => {
    const d = open();
    await settle(60);
    const before = d.asked.length;
    d.quality.value = "101";
    d.quality.dispatch("change", {});
    await settle(60);
    expect(d.asked.length).toBe(before);
    expect(d.problem.textContent).toBe("The JPEG quality is a whole number from 1 to 100.");
    expect(d.comes(4)).toBe("Could not be worked out");
    expect(d.save.disabled).toBe(true);
    d.save.dispatch("click", {});
    expect(d.dialog.isOpen).toBe(true);
  });

  it("does not offer to save a copy that would not be smaller", async () => {
    const d = open(async (pictures) => ({ ...answerFor(pictures), bytesAfter: BEFORE }));
    await settle(60);
    expect(d.comes(0)).toBe("Not smaller");
    expect(d.said.textContent).toBe(
      "The document is 9.5 MB, and a copy made this way would not be smaller.",
    );
    expect(d.save.disabled).toBe(true);
    d.save.dispatch("click", {});
    d.backdrop.dispatch("keydown", {
      key: "Enter", preventDefault() {}, stopPropagation() {},
    });
    expect(d.dialog.isOpen).toBe(true);
  });

  it("shows why an estimate failed, on the row and under the choices", async () => {
    const d = open(async (pictures) => {
      if (pictures?.dpi === 150) throw new Error("the worker went away");
      return answerFor(pictures);
    });
    await settle(60);
    expect(d.comes(2)).toBe("Could not be worked out");
    expect(d.comes(3)).toBe("258 KB, 97% smaller");
    d.radio(2).dispatch("change", {});
    expect(d.problem.textContent).toBe("Error: the worker went away");
    expect(d.save.disabled).toBe(true);
  });

  it.each([
    ["Cancel", (d: ReturnType<typeof open>) => d.cancel.dispatch("click", {})],
    [
      "Escape",
      (d: ReturnType<typeof open>) =>
        d.backdrop.dispatch("keydown", {
          key: "Escape", preventDefault() {}, stopPropagation() {},
        }),
    ],
    [
      "a click outside",
      (d: ReturnType<typeof open>) => d.backdrop.dispatch("click", { target: d.backdrop }),
    ],
  ])("is dismissed by %s, with no choice and no picture kept", async (_, dismiss) => {
    const d = open();
    await settle(60);
    d.radio(3).dispatch("change", {});
    expect(d.before.getAttribute("src")).not.toBeNull();
    dismiss(d);
    await expect(d.answer).resolves.toBeNull();
    expect(d.dialog.isOpen).toBe(false);
    // The pictures of the reader's document are not kept.
    expect(d.before.getAttribute("src")).toBeNull();
  });

  it("puts nothing in a picture but a PNG the backend encoded", async () => {
    expect(isPicture("data:image/png;base64,iVBORw0KGgo=")).toBe(true);
    for (const url of [
      "https://example.invalid/a.png",
      "data:image/svg+xml;base64,PHN2Zz4=",
      "data:image/png;base64,",
      "data:image/png;base64,AAAA\" onerror=\"x",
      "javascript:alert(1)",
    ]) {
      expect(isPicture(url), url).toBe(false);
    }
    const d = open(async (pictures) => {
      const answer = answerFor(pictures);
      if (answer.sample) answer.sample.after = "https://example.invalid/a.png";
      return answer;
    });
    await settle(60);
    d.radio(3).dispatch("change", {});
    expect(d.preview.style.display).toBe("none");
    expect(d.after.getAttribute("src")).toBeNull();
    // The size is still the answer, and the copy can still be saved.
    expect(d.save.disabled).toBe(false);
  });

  it("drops an answer that arrives after the dialog was closed and opened again", async () => {
    const waiting: ((value: Shrinkage) => void)[] = [];
    const host = dom.root;
    const dialog = new CompressDialog(host as unknown as HTMLElement);
    const first = dialog.ask("old.pdf", () => new Promise((resolve) => waiting.push(resolve)));
    await settle(60);
    const second = dialog.ask("new.pdf", async (pictures) => answerFor(pictures));
    await expect(first).resolves.toBeNull();
    const panel = host.children.find((c) => c.classList.contains(DIALOG_CLASS))?.children[0];
    const firstRow = () => panel?.children[1]?.children[0]?.children[2]?.textContent;
    // The new opening is not held back by the estimate still running for the old.
    await settle(60);
    expect(firstRow()).toBe("9.4 MB, 1% smaller");
    // And that estimate's answer, late and for another file, changes nothing.
    waiting[0]?.({ ...answerFor(null), bytesAfter: 1 });
    await settle(60);
    expect(firstRow()).toBe("9.4 MB, 1% smaller");
    dialog.close();
    await expect(second).resolves.toBeNull();
  });
});
