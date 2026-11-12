import { describe, expect, it } from "vitest";
import { listenForOutsideInput, outsideInputNote } from "./outsideinput";

/** A target whose listeners are called with plain objects, so `isTrusted` can be true. */
function fakeTarget() {
  const listeners = new Map<string, (event: unknown) => void>();
  return {
    addEventListener: (type: string, fn: (event: unknown) => void) => listeners.set(type, fn),
    removeEventListener: (type: string) => listeners.delete(type),
    fire: (type: string, event: object) => listeners.get(type)?.(event),
    count: () => listeners.size,
  };
}

describe("input from outside a window check", () => {
  it("names a real key press and a real wheel turn", () => {
    const target = fakeTarget();
    const stop = listenForOutsideInput(target as unknown as EventTarget);
    target.fire("keydown", { isTrusted: true, key: "PageDown" });
    target.fire("wheel", { isTrusted: true });
    expect(stop()).toEqual(["PageDown", "wheel"]);
  });

  it("ignores the check's own dispatched events", () => {
    // Node's own EventTarget: a dispatched event is untrusted there as in a browser.
    const target = new EventTarget();
    const stop = listenForOutsideInput(target);
    target.dispatchEvent(new Event("keydown"));
    target.dispatchEvent(new Event("wheel"));
    expect(stop()).toEqual([]);
  });

  it("stops listening when asked", () => {
    const target = fakeTarget();
    const stop = listenForOutsideInput(target as unknown as EventTarget);
    stop();
    expect(target.count()).toBe(0);
  });

  it("adds nothing to the detail when nothing came, and says what came otherwise", () => {
    expect(outsideInputNote([])).toBe("");
    expect(outsideInputNote(["PageDown"])).toBe(
      "; the window received an input from outside the check meanwhile (PageDown): someone at the machine was using it",
    );
    expect(outsideInputNote(["a", "wheel"])).toContain("2 inputs");
  });
});
