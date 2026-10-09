import { describe, expect, it } from "vitest";
import { Panes, otherSide, partnerPlaceholder, pickPartner, type Slot } from "./panes";

/** Three tabs open in one side, the first in front and mounted in area 0. */
function three() {
  const panes = new Panes();
  const order = [1, 2, 3];
  panes.opened(3);
  panes.opened(2);
  panes.opened(1);
  panes.mounted(1, 0);
  return { panes, order };
}

/** Carries a plan out the way the window does, and returns what it did. */
function carryOut(panes: Panes) {
  const plan = panes.plan();
  for (const { id } of plan.unmount) panes.unmounted(id);
  for (const { id, slot } of plan.mount) panes.mounted(id, slot);
  return plan;
}

const areas = (panes: Panes) => [panes.mountedIn(0), panes.mountedIn(1)];

describe("one side", () => {
  it("is what a window has before anything is moved", () => {
    const { panes, order } = three();
    expect(panes.split).toBe(false);
    expect(panes.focused).toBe("left");
    expect(panes.front("left")).toBe(1);
    expect(panes.front("right")).toBe(-1);
    expect(panes.on("left", order)).toEqual([1, 2, 3]);
    expect(panes.on("right", order)).toEqual([]);
    expect(panes.plan()).toEqual({ unmount: [], mount: [], focus: 1 });
  });

  it("cannot focus the side that shows nothing", () => {
    const { panes } = three();
    expect(panes.focus("right")).toBe(false);
    expect(panes.focused).toBe("left");
  });

  it("asks for the tab brought to the front to replace the one mounted", () => {
    const { panes } = three();
    panes.fronted(2);
    expect(panes.plan()).toEqual({
      unmount: [{ id: 1, slot: 0 }], mount: [{ id: 2, slot: 0 }], focus: 2,
    });
  });

  it("does not swap", () => {
    const { panes, order } = three();
    panes.swap(order);
    expect(panes.slotOf("left")).toBe(0);
    expect(panes.on("left", order)).toEqual([1, 2, 3]);
  });
});

describe("moving a tab to the right", () => {
  it("splits, shows the next tab where it was, and focuses the moved one", () => {
    const { panes, order } = three();
    panes.move(1, "right", order);
    expect(panes.split).toBe(true);
    expect(panes.front("left")).toBe(2);
    expect(panes.front("right")).toBe(1);
    expect(panes.focused).toBe("right");
    // The moved document was mounted in the left area, so it is rebuilt.
    expect(panes.plan()).toEqual({
      unmount: [{ id: 1, slot: 0 }],
      mount: [{ id: 2, slot: 0 }, { id: 1, slot: 1 }],
      focus: 1,
    });
  });

  it("shows the tab before it when the moved tab was last on its side", () => {
    const { panes, order } = three();
    panes.fronted(3);
    panes.move(3, "right", order);
    expect(panes.front("left")).toBe(2);
  });

  it("leaves the front tab alone when a background tab moves", () => {
    const { panes, order } = three();
    panes.move(3, "right", order);
    expect(panes.front("left")).toBe(1);
    expect(panes.front("right")).toBe(3);
    expect(panes.plan()).toEqual({ unmount: [], mount: [{ id: 3, slot: 1 }], focus: 3 });
  });

  it("does not split a window with one tab", () => {
    const panes = new Panes();
    panes.opened(7);
    panes.mounted(7, 0);
    panes.move(7, "right", [7]);
    expect(panes.split).toBe(false);
    expect(panes.sideOf(7)).toBe("left");
    expect(panes.front("left")).toBe(7);
    // The only tab is the left side now, drawn in the other area.
    expect(panes.slotOf("left")).toBe(1);
    expect(panes.plan()).toEqual({
      unmount: [{ id: 7, slot: 0 }], mount: [{ id: 7, slot: 1 }], focus: 7,
    });
  });
});

describe("two sides", () => {
  function split() {
    const made = three();
    made.panes.move(1, "right", made.order);
    carryOut(made.panes);
    return made;
  }

  it("has each front tab mounted in its own area", () => {
    const { panes } = split();
    expect(areas(panes)).toEqual([2, 1]);
    expect(panes.plan()).toEqual({ unmount: [], mount: [], focus: 1 });
  });

  it("changes focus without mounting anything", () => {
    const { panes } = split();
    expect(panes.focus("left")).toBe(true);
    expect(panes.plan()).toEqual({ unmount: [], mount: [], focus: 2 });
  });

  it("opens a new document in the focused side", () => {
    const { panes } = split();
    panes.opened(9);
    expect(panes.sideOf(9)).toBe("right");
    expect(panes.front("right")).toBe(9);
    panes.focus("left");
    panes.opened(10);
    expect(panes.sideOf(10)).toBe("left");
  });

  it("adds a tab opened behind to the focused side without showing it", () => {
    const { panes } = split();
    panes.joined(9);
    expect(panes.sideOf(9)).toBe("right");
    expect(panes.front("right")).toBe(1);
  });

  it("brings a tab to the front of the side it is on, and focuses that side", () => {
    const { panes } = split();
    panes.fronted(3);
    expect(panes.focused).toBe("left");
    expect(panes.front("left")).toBe(3);
    expect(panes.front("right")).toBe(1);
  });

  it("keeps side, front and area through a save's new handle", () => {
    const { panes } = split();
    panes.replaced(1, 11);
    expect(panes.sideOf(11)).toBe("right");
    expect(panes.front("right")).toBe(11);
    expect(areas(panes)).toEqual([2, 11]);
    panes.replaced(2, 12);
    expect(panes.front("left")).toBe(12);
    expect(areas(panes)).toEqual([12, 11]);
    expect(panes.plan().mount).toEqual([]);
  });

  it("swaps sides without rebuilding either viewer", () => {
    const { panes, order } = split();
    panes.swap(order);
    expect(panes.on("left", order)).toEqual([1]);
    expect(panes.on("right", order)).toEqual([2, 3]);
    expect(panes.front("left")).toBe(1);
    expect(panes.front("right")).toBe(2);
    // The focused document is still document 1, now on the left.
    expect(panes.focused).toBe("left");
    expect(panes.slotOf("left")).toBe(1);
    expect(panes.plan()).toEqual({ unmount: [], mount: [], focus: 1 });
    panes.swap(order);
    expect(panes.slotOf("left")).toBe(0);
    expect(panes.focused).toBe("right");
  });

  it("ends the split when the right side's only tab is closed", () => {
    const { panes, order } = split();
    panes.closed(1, order);
    expect(panes.split).toBe(false);
    expect(panes.focused).toBe("left");
    expect(panes.plan()).toEqual({ unmount: [{ id: 1, slot: 1 }], mount: [], focus: 2 });
  });

  it("ends the split when the right side's only tab moves back", () => {
    const { panes, order } = split();
    panes.move(1, "left", order);
    expect(panes.split).toBe(false);
    expect(panes.front("left")).toBe(1);
    const plan = carryOut(panes);
    expect(plan.unmount).toEqual([{ id: 2, slot: 0 }, { id: 1, slot: 1 }]);
    expect(plan.mount).toEqual([{ id: 1, slot: 0 }]);
    expect(areas(panes)).toEqual([1, -1]);
  });

  it("makes the right side the left one when the left empties, keeping its viewer", () => {
    const { panes, order } = split();
    panes.focus("left");
    panes.closed(3, order);
    panes.closed(2, [1, 2]);
    expect(panes.split).toBe(false);
    expect(panes.sideOf(1)).toBe("left");
    expect(panes.front("left")).toBe(1);
    expect(panes.focused).toBe("left");
    expect(panes.slotOf("left")).toBe(1);
    // Document 2's viewer goes; document 1's stays where it is.
    expect(panes.plan()).toEqual({ unmount: [{ id: 2, slot: 0 }], mount: [], focus: 1 });
  });

  it("shows a side's neighbour when its front tab is closed", () => {
    const { panes, order } = split();
    panes.closed(2, order);
    expect(panes.split).toBe(true);
    expect(panes.front("left")).toBe(3);
    expect(panes.plan()).toEqual({
      unmount: [{ id: 2, slot: 0 }], mount: [{ id: 3, slot: 0 }], focus: 1,
    });
  });

  it("moves focus off a side that was left showing nothing", () => {
    const { panes, order } = split();
    panes.focus("left");
    panes.move(2, "right", order);
    panes.move(3, "right", order);
    expect(panes.split).toBe(false);
    expect(panes.focused).toBe("left");
    expect(panes.front("left")).toBe(3);
    expect(panes.on("left", order)).toEqual([1, 2, 3]);
  });

  it("forgets everything when every tab is gone", () => {
    const { panes } = split();
    panes.cleared();
    expect(panes.split).toBe(false);
    expect(panes.front("left")).toBe(-1);
    expect(panes.focused).toBe("left");
  });
});

describe("sides and areas", () => {
  it("are each other's inverse, flipped or not", () => {
    const { panes, order } = three();
    panes.move(1, "right", order);
    for (let round = 0; round < 2; round++) {
      for (const slot of [0, 1] as Slot[]) {
        expect(panes.slotOf(panes.sideIn(slot))).toBe(slot);
      }
      panes.swap(order);
    }
    expect(otherSide("left")).toBe("right");
    expect(otherSide("right")).toBe("left");
  });
});

describe("naming the other document", () => {
  const names = ["report.pdf", "report-draft.pdf", "Invoice.pdf"];

  it("takes a name typed in full over a longer one containing it", () => {
    expect(pickPartner("report.pdf", names)).toEqual({ index: 0 });
    expect(pickPartner("  INVOICE.PDF ", names)).toEqual({ index: 2 });
  });

  it("takes part of a name when one document has it", () => {
    expect(pickPartner("draft", names)).toEqual({ index: 1 });
    expect(pickPartner("inv", names)).toEqual({ index: 2 });
  });

  it("asks for more when several match, and says when none does", () => {
    expect(pickPartner("report", names)).toEqual({
      problem: "2 open documents match. Type more of the name",
    });
    expect(pickPartner("zebra", names)).toEqual({
      problem: "No other open document has that in its name",
    });
  });

  it("does not choose between two documents with one name", () => {
    expect(pickPartner("a.pdf", ["a.pdf", "a.pdf"])).toEqual({
      problem: "2 open documents match. Type more of the name",
    });
  });

  it("answers nothing typed only when there is one document to choose", () => {
    expect(pickPartner("", ["only.pdf"])).toEqual({ index: 0 });
    expect(pickPartner(" ", names)).toEqual({ problem: "Type part of a document's name" });
    expect(pickPartner("x", [])).toEqual({ problem: "No other document is open" });
  });

  it("offers the first three names and counts the rest", () => {
    expect(partnerPlaceholder(["a.pdf"])).toBe("a.pdf");
    expect(partnerPlaceholder(["a", "b", "c", "d", "e"])).toBe("a, b, c and 2 more");
  });
});
