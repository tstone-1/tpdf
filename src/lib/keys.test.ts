import { afterEach, describe, expect, it } from "vitest";

import {
  accelerator,
  BINDINGS,
  label,
  setPrintedKeys,
  matches,
  render,
  type Binding,
  type BoundCommand,
} from "./keys";

/**
 * The minimal shape {@link matches} reads.
 *
 * A plain object rather than a real `KeyboardEvent`: vitest runs in node here,
 * there is no DOM, and adding one to construct four fields would be a
 * dependency bought for a cast.
 */
function event(
  key: string,
  { accel = false, ctrl = false, shift = false, alt = false, code = "" } = {},
): KeyboardEvent {
  return {
    key,
    code,
    metaKey: accel,
    ctrlKey: ctrl,
    shiftKey: shift,
    altKey: alt,
  } as KeyboardEvent;
}

const ids = Object.keys(BINDINGS) as BoundCommand[];

/**
 * What the German layout prints on the keys a binding might name by position.
 *
 * Here because a `code` is a *position* and the whole hazard is that the
 * character above it moves. This is the layout the application is developed on
 * and the one whose behaviour was measured, so it is the one worth encoding:
 * without it, adding `code: "BracketRight"` to `nav.forward` looks harmless and
 * silently collides with ⌘+.
 */
const GERMAN: Record<string, string> = {
  Backslash: "#",
  BracketLeft: "ü",
  BracketRight: "+",
  Equal: "´",
  Minus: "ß",
  Slash: "-",
};

describe("label", () => {
  it("renders the modifiers the binding actually declares", () => {
    expect(label("file.open")).toBe("⌘O");
    // Shift before Command, which is the platform's order.
    expect(label("find.previous")).toBe("⇧⌘G");
    expect(label("edit.clearSelection")).toBe("Esc");
    // Option before Command, and the letter still capitalised by the chord.
    expect(label("nav.goToPage")).toBe("⌥⌘G");
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

  it("orders the modifiers as the platform does, Option before Shift", () => {
    // Not reachable through `label`: no command holds both, so the ordering
    // between them was decided by a line of code that no id could exercise ---
    // and it disagreed with the comment two lines above it. `render` takes a
    // binding so that a combination nothing uses can still be asserted.
    expect(render({ keys: ["k"], accel: true, shift: true, alt: true })).toBe("⌥⇧⌘K");
    expect(render({ keys: ["k"], shift: true, alt: true })).toBe("⌥⇧K");
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
        const alt = "alt" in binding ? binding.alt : false;
        expect(matches(id, event(key, { accel, shift, alt })), `${id} / ${key}`).toBe(
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

  it("takes Ctrl as the accelerator as readily as Command", () => {
    // The Windows half of one table: `matches` reads `metaKey || ctrlKey` so the
    // same bindings serve both platforms. Nothing on the machine this is written
    // on ever sends the Ctrl spelling, so it has to be asked for --- otherwise
    // half the claim is untested and every shortcut fails on Windows.
    expect(matches("edit.copy", event("c", { ctrl: true }))).toBe(true);
    expect(
      matches("find.previous", event("g", { ctrl: true, shift: true })),
    ).toBe(true);
  });

  it("rejects Ctrl on a binding that asks for no accelerator", () => {
    // The control for the arm above, and the direction that matters on
    // Windows: Ctrl is an accelerator there, not a modifier to be ignored, so
    // Ctrl-P must reach Print and not also turn the page.
    expect(matches("nav.previousPage", event("p"))).toBe(true);
    expect(matches("nav.previousPage", event("p", { ctrl: true }))).toBe(false);
    expect(matches("edit.clearSelection", event("Escape", { ctrl: true }))).toBe(
      false,
    );
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

  it("distinguishes a chord from the same chord with Option", () => {
    // ⌘G is find-next and ⌥⌘G goes to a page. Until Option was added to the
    // table `matches` never looked at `altKey` at all, so every binding here
    // fired with Option held as well as without -- and ⌥⌘G would have been
    // find-next, whichever arm of the handler was tested first.
    expect(matches("find.next", event("g", { accel: true }))).toBe(true);
    expect(matches("find.next", event("g", { accel: true, alt: true }))).toBe(false);
    expect(matches("nav.goToPage", event("g", { accel: true, alt: true }))).toBe(true);
    expect(matches("nav.goToPage", event("g", { accel: true }))).toBe(false);
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
      // Alt belongs in the chord for the same reason the other two do, and it
      // was missing: ⌥⌘G and ⌘G hashed to one key, so this test failed the
      // moment a binding used Option -- correctly, since without the modifier
      // in the identity they really would be the same chord.
      const alt = "alt" in binding ? binding.alt : false;
      for (const key of binding.keys) {
        const chord =
          `${accel ? "accel+" : ""}${shift ? "shift+" : ""}${alt ? "alt+" : ""}${key}`;
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

  it("names no physical key that a German keyboard gives to another command", () => {
    // The check that stops the obvious next edit. `nav.back` and `nav.forward`
    // are as untypable on this layout as ⌘\ was, so adding `code` to them is the
    // natural symmetry -- and `BracketRight` is the `+` key here, which
    // `view.zoomIn` already claims. Both would match one press of ⌘+, and which
    // one fired would be whichever branch a handler tested first.
    //
    // So Back and Forward keep no position, and the pair stays untypable on a
    // German keyboard rather than half-fixed. The menu is their route now, and
    // moving them to a layout-safe chord is a decision about which chord, not a
    // bug fix.
    const positions = new Map<string, BoundCommand>();
    for (const id of ids) {
      const code = (BINDINGS[id] as Binding).code;
      if (code !== undefined) positions.set(code, id);
    }
    // The control. An empty map satisfies the loop below however wrong the
    // bindings are, and this file has already shipped one sweep that could not
    // fail for exactly that reason.
    expect(positions.size).toBeGreaterThan(0);

    for (const [code, id] of positions) {
      const prints = GERMAN[code];
      expect(prints, `${code} is not in the German table`).toBeDefined();
      const binding: Binding = BINDINGS[id];
      const clash = ids.filter((other) => {
        if (other === id) return false;
        const b: Binding = BINDINGS[other];
        return (
          b.keys.includes(prints as string) &&
          (b.accel ?? false) === (binding.accel ?? false) &&
          (b.shift ?? false) === (binding.shift ?? false) &&
          (b.alt ?? false) === (binding.alt ?? false)
        );
      });
      expect(clash, `${id} names ${code}, which prints ${prints} here`).toEqual([]);
    }
  });
});

describe("labelling a key by what the keyboard prints on it", () => {
  // Module state, so every test here puts it back. Without this the first one
  // to run decides what the rest of the file sees, which is the kind of order
  // dependence that shows up as one test failing only in a full run.
  afterEach(() => setPrintedKeys({}));

  it("names the key this keyboard shows, once the platform has said", () => {
    // The whole point of asking macOS. `Backslash` prints `#` on a German
    // keyboard, and a palette advertising ⌘\\ there teaches a chord that
    // cannot be typed -- while the menu bar, which resolves the key itself,
    // shows ⌘#. Two parts of one application disagreeing about one shortcut.
    setPrintedKeys({ Backslash: "#" });
    expect(label("view.toggleSidebar")).toBe("⌘#");
  });

  it("falls back to the declared character before the platform answers", () => {
    // The control, and the state every launch passes through: the lookup is a
    // round trip, so this is what is rendered until it returns, and what is
    // rendered forever on a platform that cannot answer.
    setPrintedKeys({});
    expect(label("view.toggleSidebar")).toBe("⌘\\");
  });

  it("leaves a binding that names no position alone", () => {
    // A position map is not a licence to relabel everything. `nav.back` has no
    // `code`, so what a keyboard prints at `BracketLeft` is none of its
    // business -- and on this layout that is `ü`, which would be a wrong label
    // for a chord that is still the `[` character.
    setPrintedKeys({ Backslash: "#", BracketLeft: "ü" });
    expect(label("nav.back")).toBe("⌘[");
    expect(label("file.open")).toBe("⌘O");
  });

  it("ignores a position nobody asked about", () => {
    setPrintedKeys({ Semicolon: "ö" });
    expect(label("view.toggleSidebar")).toBe("⌘\\");
  });

  it("replaces the whole map rather than merging into it", () => {
    // A layout change replaces what the keyboard prints; merging would leave
    // the previous layout's glyph on any position the new one does not name.
    setPrintedKeys({ Backslash: "#" });
    setPrintedKeys({ Minus: "ß" });
    expect(label("view.toggleSidebar")).toBe("⌘\\");
  });
});

describe("matching by position as well as character", () => {
  it("matches the physical key when the character is unreachable", () => {
    // A German keyboard, where the `\` character needs ⌥⇧7 and the key in the
    // US backslash position prints `#`. The character path cannot fire -- the
    // modifiers are wrong for it -- so this is the position path alone.
    expect(
      matches("view.toggleSidebar", event("#", { accel: true, code: "Backslash" })),
    ).toBe(true);
  });

  it("still matches the character when the layout can produce it", () => {
    // A US keyboard. Both paths agree here, which is why the defect was
    // invisible for as long as it was.
    expect(
      matches("view.toggleSidebar", event("\\", { accel: true, code: "Backslash" })),
    ).toBe(true);
  });

  it("does not match a position no binding named", () => {
    // The control for the two above: the position path must be a property of
    // this one binding, not something `matches` does for every event carrying a
    // code. Without it, `code` being ignored entirely would leave the US case
    // green and look like a working feature.
    expect(
      matches("view.rotateClockwise", event("#", { accel: true, code: "Backslash" })),
    ).toBe(false);
    expect(matches("nav.back", event("ü", { accel: true, code: "BracketLeft" }))).toBe(
      false,
    );
  });

  it("keeps the modifier checks on the position path", () => {
    // A position is not a licence to ignore the rest of the chord. ⇧⌘ on the
    // same key is not this binding, and nothing about matching by code should
    // change that.
    expect(
      matches(
        "view.toggleSidebar",
        event("#", { accel: true, shift: true, code: "Backslash" }),
      ),
    ).toBe(false);
    expect(matches("view.toggleSidebar", event("#", { code: "Backslash" }))).toBe(
      false,
    );
  });
});

describe("accelerator", () => {
  // Taking a binding rather than an id, for the reason `render` does: these are
  // the shapes the menu has to survive, and several of them are combinations no
  // command currently uses --- which is exactly where a rendering rule breaks
  // without anything going red.

  it("refuses a binding that holds no accelerator key", () => {
    // The rule that keeps `n` and Home out of the menu bar. A menu accelerator
    // is claimed before the web view sees the key, so a bare letter there is
    // taken out of every text field the application has.
    expect(accelerator({ keys: ["n"] })).toBeNull();
    expect(accelerator({ keys: ["Home"] })).toBeNull();
    // ...and the control: the same key *with* ⌘ is rendered, so the null above
    // is about the modifier rather than about the key.
    expect(accelerator({ keys: ["n"], accel: true })).toBe("CmdOrCtrl+N");
  });

  it("orders the modifiers the way the glyphs read", () => {
    expect(
      accelerator({ keys: ["l"], accel: true, alt: true, shift: true }),
    ).toBe("Alt+Shift+CmdOrCtrl+L");
  });

  it("upper-cases a letter and leaves a digit alone", () => {
    expect(accelerator({ keys: ["o"], accel: true })).toBe("CmdOrCtrl+O");
    expect(accelerator({ keys: ["0"], accel: true })).toBe("CmdOrCtrl+0");
  });

  it("refuses a punctuation key, whose position is not its character", () => {
    // Measured on the German layout this was developed against: an accelerator
    // names a physical key, `matches` reads the character it produced, and for
    // `\` those are different keys. Building the menu from the character table
    // advertised ⌘# for a command whose palette entry says ⌘\ -- read out of
    // the running application's own menu bar.
    expect(accelerator({ keys: ["\\"], accel: true })).toBeNull();
    expect(accelerator({ keys: ["["], accel: true })).toBeNull();
    // ...unless the binding names its physical key, which is the whole escape
    // hatch: a position is layout-independent where a character is not.
    expect(accelerator({ keys: ["\\"], code: "Backslash", accel: true })).toBe(
      "CmdOrCtrl+Backslash",
    );
    expect(accelerator({ keys: ["+", "="], accel: true })).toBeNull();
    expect(accelerator({ keys: ["-"], accel: true })).toBeNull();
  });

  it("refuses a key it cannot spell rather than guessing", () => {
    // No binding uses these, which is the point: the parser accepts an
    // accelerator it can read and silently claims whatever chord it read, so a
    // guess here would take a chord nobody chose. Reached only through this
    // test today, and worth keeping for the same reason `render`'s ordering is.
    expect(accelerator({ keys: ["ç"], accel: true })).toBeNull();
    expect(accelerator({ keys: ["F7"], accel: true })).toBeNull();
    expect(accelerator({ keys: [""], accel: true })).toBeNull();
  });

  it("renders most real bindings, and names the ones it will not", () => {
    // The sweep. Its control is the count on both sides: a rule that returned
    // null for everything would satisfy an each-one loop, and one that returned
    // a string for everything would satisfy a some-are-null loop.
    //
    // Through the widened type, as the collision test above does: the literal
    // type of an entry with no `accel` has no such property at all, so reading
    // it off the indexed access is a type error rather than a false.
    const withAccel = ids.filter((id) => (BINDINGS[id] as Binding).accel === true);
    const refused = withAccel.filter(
      (id) => accelerator(BINDINGS[id] as Binding) === null,
    );
    expect(withAccel.length).toBeGreaterThan(20);
    // Exactly the punctuation chords, named rather than counted: this is the
    // list a reader has to check against the menu when a shortcut is missing
    // from it, and a count would not say which.
    expect(refused.sort()).toEqual([
      "nav.back",
      "nav.forward",
      "view.zoomIn",
      "view.zoomOut",
    ]);
  });
});
