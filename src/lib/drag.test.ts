import { beforeEach, describe, expect, it } from "vitest";

import { PointerDrag, type DragPoint, type DragTarget } from "./drag";
import { FakeElement } from "./testdom";

/** What a target was told, in order, as strings a test can read at a glance. */
class Recorder implements DragTarget {
  readonly seen: string[] = [];
  /** Set false to make {@link begin} refuse. */
  accept = true;
  /** Called at the end of `end`, so a test can look at the drag from inside. */
  onEnd: (() => void) | null = null;

  begin(at: DragPoint): boolean {
    this.seen.push(`begin ${at.clientX},${at.clientY}`);
    return this.accept;
  }
  move(at: DragPoint): void {
    this.seen.push(`move ${at.clientX},${at.clientY}`);
  }
  end(at: DragPoint, committed: boolean): void {
    this.seen.push(`end ${at.clientX},${at.clientY} ${committed ? "commit" : "cancel"}`);
    this.onEnd?.();
  }
}

/** How many listeners of every kind the host is holding. */
function listenerCount(host: FakeElement): number {
  let total = 0;
  for (const set of host.listeners.values()) total += set.size;
  return total;
}

function down(id = 1, x = 10, y = 20): PointerEvent {
  return { pointerId: id, clientX: x, clientY: y } as unknown as PointerEvent;
}

describe("PointerDrag", () => {
  let host: FakeElement;
  let target: Recorder;
  let drag: PointerDrag;

  beforeEach(() => {
    host = new FakeElement("div");
    target = new Recorder();
    drag = new PointerDrag(host as unknown as HTMLElement, target);
  });

  describe("starting", () => {
    it("takes the press, captures the pointer and follows it", () => {
      expect(drag.start(down(7, 10, 20))).toBe(true);
      expect(drag.active).toBe(true);
      expect(host.captured.has(7)).toBe(true);

      host.dispatch("pointermove", { pointerId: 7, clientX: 30, clientY: 40 });
      host.dispatch("pointerup", { pointerId: 7, clientX: 30, clientY: 40 });

      expect(target.seen).toEqual(["begin 10,20", "move 30,40", "end 30,40 commit"]);
    });

    it("registers nothing when the target refuses", () => {
      target.accept = false;
      expect(drag.start(down())).toBe(false);
      expect(drag.active).toBe(false);
      expect(listenerCount(host)).toBe(0);
      // The capture matters as much as the listeners: a captured pointer that
      // nothing is listening for is a surface that has stopped receiving events
      // it never wanted in the first place.
      expect(host.captured.size).toBe(0);
    });

    it("refuses a second press rather than replacing the live drag", () => {
      drag.start(down(1, 10, 20));
      expect(drag.start(down(2, 500, 500))).toBe(false);

      // The refusal has to leave the *first* drag working, which is the whole
      // point of it: a second finger must not commit a rectangle at its own
      // starting point.
      host.dispatch("pointermove", { pointerId: 1, clientX: 30, clientY: 40 });
      host.dispatch("pointerup", { pointerId: 1, clientX: 30, clientY: 40 });
      expect(target.seen).toEqual(["begin 10,20", "move 30,40", "end 30,40 commit"]);
    });
  });

  describe("ending", () => {
    it("takes its listeners off and releases the capture", () => {
      drag.start(down(7));
      expect(listenerCount(host)).toBe(3);

      host.dispatch("pointerup", { pointerId: 7, clientX: 1, clientY: 2 });

      expect(drag.active).toBe(false);
      expect(listenerCount(host)).toBe(0);
      expect(host.captured.size).toBe(0);
    });

    it("is already torn down by the time the target is told", () => {
      // So a target that starts another drag from its own `end` --- which is
      // what a one-shot tool that re-arms would do --- cannot find a
      // half-registered one in its way.
      let activeInsideEnd: boolean | null = null;
      let listenersInsideEnd: number | null = null;
      target.onEnd = () => {
        activeInsideEnd = drag.active;
        listenersInsideEnd = listenerCount(host);
      };
      drag.start(down(7));
      host.dispatch("pointerup", { pointerId: 7, clientX: 1, clientY: 2 });

      expect(activeInsideEnd).toBe(false);
      expect(listenersInsideEnd).toBe(0);
    });

    it("reports a browser cancel as not committed", () => {
      drag.start(down(7, 10, 20));
      host.dispatch("pointermove", { pointerId: 7, clientX: 30, clientY: 40 });
      host.dispatch("pointercancel", { pointerId: 7, clientX: 30, clientY: 40 });

      expect(target.seen.at(-1)).toBe("end 30,40 cancel");
      expect(drag.active).toBe(false);
    });

    it("cancels at the last point seen, since a cancel has no point of its own", () => {
      drag.start(down(7, 10, 20));
      host.dispatch("pointermove", { pointerId: 7, clientX: 33, clientY: 44 });
      drag.cancel();

      expect(target.seen.at(-1)).toBe("end 33,44 cancel");
      expect(listenerCount(host)).toBe(0);
    });

    it("cancels at the starting point when the pointer never moved", () => {
      drag.start(down(7, 10, 20));
      drag.cancel();
      expect(target.seen.at(-1)).toBe("end 10,20 cancel");
    });

    it("does nothing when asked to cancel with no drag live", () => {
      drag.cancel();
      expect(target.seen).toEqual([]);
    });

    it("ends a live drag when disposed", () => {
      drag.start(down(7, 10, 20));
      drag.dispose();
      expect(target.seen.at(-1)).toBe("end 10,20 cancel");
      expect(listenerCount(host)).toBe(0);
    });
  });

  describe("a second pointer on the same surface", () => {
    it("ignores its moves", () => {
      drag.start(down(1, 10, 20));
      host.dispatch("pointermove", { pointerId: 2, clientX: 900, clientY: 900 });
      expect(target.seen).toEqual(["begin 10,20"]);
    });

    it("ignores its release, so the drag stays live", () => {
      drag.start(down(1, 10, 20));
      host.dispatch("pointerup", { pointerId: 2, clientX: 900, clientY: 900 });

      expect(drag.active).toBe(true);
      expect(target.seen).toEqual(["begin 10,20"]);

      // And the real one still ends it.
      host.dispatch("pointerup", { pointerId: 1, clientX: 30, clientY: 40 });
      expect(target.seen.at(-1)).toBe("end 30,40 commit");
    });
  });

  it("survives a host that has no such pointer to capture", () => {
    // Which is every synthetic event, and therefore every event the window
    // check harness dispatches. `setPointerCapture` throws `NotFoundError`
    // there; the drag has to work anyway, ending at the window edge instead of
    // past it.
    const hostile = new FakeElement("div");
    hostile.setPointerCapture = () => {
      throw new Error("NotFoundError");
    };
    hostile.releasePointerCapture = () => {
      throw new Error("NotFoundError");
    };
    const other = new PointerDrag(hostile as unknown as HTMLElement, target);

    expect(other.start(down(7, 10, 20))).toBe(true);
    hostile.dispatch("pointerup", { pointerId: 7, clientX: 30, clientY: 40 });
    expect(target.seen).toEqual(["begin 10,20", "end 30,40 commit"]);
    expect(listenerCount(hostile)).toBe(0);
  });
});
