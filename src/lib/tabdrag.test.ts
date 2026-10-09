import { describe, expect, it } from "vitest";
import { SLOP, TabDrag, dropSide, type DropLayout } from "./tabdrag";

const area = { left: 200, top: 80, width: 1000, height: 600 };
const one: DropLayout = { area, split: false, share: 0.5, from: "left", tabs: 3 };
const two: DropLayout = { area, split: true, share: 0.3, from: "left", tabs: 3 };

describe("the side a tab is dropped on", () => {
  it("is the half of the page area it is over, with one side showing", () => {
    expect(dropSide(201, 300, one)).toBe("left");
    expect(dropSide(699, 300, one)).toBe("left");
    expect(dropSide(700, 300, one)).toBe("right");
    expect(dropSide(1199, 300, one)).toBe("right");
  });

  it("is nowhere outside the page area", () => {
    expect(dropSide(199, 300, one)).toBeNull();
    expect(dropSide(1200, 300, one)).toBeNull();
    // The row of tabs is above it and is where the drag started.
    expect(dropSide(700, 79, one)).toBeNull();
    expect(dropSide(700, 680, one)).toBeNull();
    expect(dropSide(700, 80, one)).toBe("right");
    expect(dropSide(10, 10, { ...one, area: { ...area, width: 0 } })).toBeNull();
  });

  it("is the other side with two showing, divided where the divider is", () => {
    expect(dropSide(499, 300, two)).toBeNull();
    expect(dropSide(500, 300, two)).toBe("right");
    expect(dropSide(499, 300, { ...two, from: "right" })).toBe("left");
    expect(dropSide(500, 300, { ...two, from: "right" })).toBeNull();
  });

  it("is nowhere with one tab open", () => {
    expect(dropSide(900, 300, { ...one, tabs: 1 })).toBeNull();
  });
});

describe("a tab pressed", () => {
  it("is a click until the pointer has travelled", () => {
    const drag = new TabDrag();
    drag.press(4, 300, 20);
    expect(drag.pressed).toBe(true);
    expect(drag.move(300 + SLOP - 1, 20, one)).toBeNull();
    expect(drag.carried).toBeNull();
    expect(drag.release()).toBeNull();
    expect(drag.pressed).toBe(false);
  });

  it("is dragged once it has, and stays dragged back at the start", () => {
    const drag = new TabDrag();
    drag.press(4, 300, 20);
    expect(drag.move(300, 20 + SLOP, one)).toEqual({ id: 4, side: null });
    expect(drag.move(900, 300, one)).toEqual({ id: 4, side: "right" });
    expect(drag.move(300, 20, one)).toEqual({ id: 4, side: null });
    expect(drag.carried).toEqual({ id: 4, side: null });
  });

  it("is dropped on the side it was last over", () => {
    const drag = new TabDrag();
    drag.press(4, 300, 20);
    drag.move(900, 300, one);
    drag.move(400, 300, one);
    expect(drag.release()).toEqual({ id: 4, side: "left" });
    expect(drag.carried).toBeNull();
    expect(drag.release()).toBeNull();
  });

  it("drops nothing when the drag is cancelled, and moves nothing unpressed", () => {
    const drag = new TabDrag();
    expect(drag.move(900, 300, one)).toBeNull();
    drag.press(4, 300, 20);
    drag.move(900, 300, one);
    drag.cancel();
    expect(drag.release()).toBeNull();
    expect(drag.move(900, 300, one)).toBeNull();
  });

  it("starts again from a second press", () => {
    const drag = new TabDrag();
    drag.press(4, 300, 20);
    drag.move(900, 300, one);
    drag.press(5, 320, 20);
    expect(drag.carried).toBeNull();
    expect(drag.move(321, 20, one)).toBeNull();
  });
});
