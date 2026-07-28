import { describe, expect, it } from "vitest";

import { BINDINGS, label, matches, type BoundCommand } from "./keys";

/**
 * The minimal shape {@link matches} reads.
 *
 * A plain object rather than a real `KeyboardEvent`: vitest runs in node here,
 * there is no DOM, and adding one to construct four fields would be a
 * dependency bought for a cast.
 */
function event(
  key: string,
  { accel = false, shift = false } = {},
): KeyboardEvent {
  return {
    key,
    metaKey: accel,
    ctrlKey: false,
    shiftKey: shift,
  } as KeyboardEvent;
}

const ids = Object.keys(BINDINGS) as BoundCommand[];

describe("label", () => {
  it("renders the modifiers the binding actually declares", () => {
    expect(label("file.open")).toBe("⌘O");
    // Shift before Command, which is the platform's order.
    expect(label("find.previous")).toBe("⇧⌘G");
    expect(label("edit.clearSelection")).toBe("Esc");
  });

  it("capitalises a letter only when it is part of a chord", () => {
    // `⌘O` and `n` are how the two read in a menu, and the difference is not
    // cosmetic to someone hunting for the key they were told to press.
    expect(label("edit.copy")).toBe("⌘C");
    expect(label("nav.nextPage")).toBe("n");
    expect(label("nav.lastPage")).toBe("End");
  });

  it("uses the display spelling where the key is not its own name", () => {
    // `-` reads as a hyphen next to `+`; the palette wants a minus sign.
    expect(label("view.zoomOut")).toBe("⌘−");
    expect(label("view.zoomIn")).toBe("⌘+");
  });
});

describe("matches", () => {
  it("accepts an event built from the binding's own declaration", () => {
    // The round trip this module exists for. A label is rendered from the same
    // modifiers `matches` tests, so the two cannot drift --- but that is only
    // true while every binding really does accept what it declares.
    for (const id of ids) {
      const binding = BINDINGS[id];
      for (const key of binding.keys) {
        const accel = "accel" in binding ? binding.accel : false;
        const shift = "shift" in binding ? binding.shift : false;
        expect(matches(id, event(key, { accel, shift })), `${id} / ${key}`).toBe(
          true,
        );
      }
    }
  });

  it("rejects the same key without the accelerator it asks for", () => {
    // The control. Without it, a `matches` that ignored modifiers entirely
    // would pass the round trip above for every binding.
    expect(matches("edit.copy", event("c", { accel: true }))).toBe(true);
    expect(matches("edit.copy", event("c"))).toBe(false);
  });

  it("rejects an accelerator a binding does not ask for", () => {
    // ⌘P is Print. It used to also reach the viewer's `p`, because that arm
    // tested the key and not the modifier --- so printing quietly turned the
    // page as well.
    expect(matches("nav.previousPage", event("p"))).toBe(true);
    expect(matches("nav.previousPage", event("p", { accel: true }))).toBe(false);
  });

  it("distinguishes a chord from the same chord with shift", () => {
    // ⌘G and ⇧⌘G are different commands; a shift test in one direction only
    // would make find-next fire for both.
    expect(matches("find.next", event("g", { accel: true }))).toBe(true);
    expect(matches("find.next", event("g", { accel: true, shift: true }))).toBe(
      false,
    );
    expect(
      matches("find.previous", event("g", { accel: true, shift: true })),
    ).toBe(true);
    expect(matches("find.previous", event("g", { accel: true }))).toBe(false);
  });

  it("accepts either spelling of a key whose shifted form differs", () => {
    // `key` carries the shifted form and varies by layout: ⌘+ arrives as `+` on
    // one keyboard and `=` on another, and ⇧⌘I as `I` on a US layout and `i` on
    // some others. Matching one spelling makes a shortcut that works on one
    // keyboard, which is not a failure any test on this machine would show.
    expect(matches("view.zoomIn", event("+", { accel: true }))).toBe(true);
    expect(matches("view.zoomIn", event("=", { accel: true }))).toBe(true);
    expect(
      matches("view.invertPages", event("I", { accel: true, shift: true })),
    ).toBe(true);
    expect(
      matches("view.invertPages", event("i", { accel: true, shift: true })),
    ).toBe(true);
  });
});

describe("the binding table", () => {
  it("gives no two commands the same chord", () => {
    // A collision is a shortcut that fires two commands, and which one wins is
    // whichever arm the handler tests first --- invisible in the palette, which
    // would cheerfully advertise both.
    const seen = new Map<string, BoundCommand>();
    for (const id of ids) {
      const binding = BINDINGS[id];
      const accel = "accel" in binding ? binding.accel : false;
      const shift = "shift" in binding ? binding.shift : false;
      for (const key of binding.keys) {
        const chord = `${accel ? "accel+" : ""}${shift ? "shift+" : ""}${key}`;
        expect(seen.get(chord), `${chord} is both ${seen.get(chord)} and ${id}`)
          .toBeUndefined();
        seen.set(chord, id);
      }
    }
  });

  it("gives every binding at least one key to match on", () => {
    for (const id of ids) {
      expect(BINDINGS[id].keys.length, id).toBeGreaterThan(0);
    }
  });
});
