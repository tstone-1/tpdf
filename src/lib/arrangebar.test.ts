/**
 * The bar beside several picked marks.
 *
 * Three things are held here: when there is a bar, where it goes, and that a
 * button is its command and nothing more. The last is held twice, once with a
 * recorder standing in for the commands and once with the application's own
 * registry, because the first cannot see a button whose id no command has.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { registerAppCommands, type AppActions } from "./appcommands";
import { ArrangeBar, BAR_GROUPS, BAR_HEIGHT, BAR_WIDTH, barCommand, barLabel, barSpot, type BarCommand } from "./arrangebar";
import { CommandRegistry } from "./commands";
import type { Anchor } from "./popup";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";

const IDS = [
  "edit.alignLeft",
  "edit.alignCenter",
  "edit.alignRight",
  "edit.alignTop",
  "edit.alignMiddle",
  "edit.alignBottom",
  "edit.distributeAcross",
  "edit.distributeDown",
  "edit.sameWidth",
  "edit.sameHeight",
];

/** Two marks in the middle of a 900 by 700 host: together 200 to 500 across, 300 to 420 down. */
const TWO: Anchor[] = [
  { left: 200, top: 300, right: 300, bottom: 320 },
  { left: 400, top: 400, right: 500, bottom: 420 },
];

let dom: FakeDom;
let ran: string[];
/** What the stand-in commands answer, by id; anything not named is enabled. */
let state: Record<string, Partial<BarCommand> | null>;

beforeEach(() => {
  dom = installFakeDom();
  ran = [];
  state = {};
});

afterEach(() => {
  dom.restore();
});

function bar(): ArrangeBar {
  return new ArrangeBar(dom.root as unknown as HTMLElement, {
    command: (id) => {
      const over = state[id];
      if (over === null) return undefined;
      return { title: `Arrange: ${id}`, enabled: true, why: null, run: () => ran.push(id), ...over };
    },
  });
}

const node = (made: ArrangeBar): FakeElement => made.node as unknown as FakeElement;
const button = (made: ArrangeBar, id: string): FakeElement => {
  const found = made.buttons.find((one) => one.id === id);
  if (!found) throw new Error(`no button for ${id}`);
  return found.button as unknown as FakeElement;
};
const drawn = (made: ArrangeBar): [number, number] => [
  Number.parseFloat(node(made).style.left ?? ""),
  Number.parseFloat(node(made).style.top ?? ""),
];
const onScreen = (made: ArrangeBar): boolean => node(made).style.display !== "none";

/** Whether two rectangles share any area. */
function overlap(a: Anchor, b: Anchor): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

describe("when there is an arrange bar", () => {
  it("appears with two marks picked, and not with one or none", () => {
    const made = bar();
    // Built hidden: the fake keeps `cssText` whole, so it is read there.
    expect(node(made).style.cssText).toContain("display:none");
    expect(made.visible).toBe(false);
    made.sync([]);
    expect([onScreen(made), made.visible]).toEqual([false, false]);
    made.sync([TWO[0]!]);
    expect([onScreen(made), made.visible]).toEqual([false, false]);
    made.sync(TWO);
    expect([onScreen(made), made.visible]).toEqual([true, true]);
    expect(node(made).parent).toBe(dom.root);
  });

  it("goes when fewer than two are left, and when it is hidden", () => {
    const made = bar();
    made.sync(TWO);
    made.sync([TWO[1]!]);
    expect([onScreen(made), made.visible]).toEqual([false, false]);
    made.sync(TWO);
    expect(onScreen(made)).toBe(true);
    made.hide();
    expect([onScreen(made), made.visible]).toEqual([false, false]);
  });

  it("steps aside while the note box is open, and comes back when it closes", () => {
    const made = bar();
    made.sync(TWO, true);
    expect(onScreen(made)).toBe(false);
    made.sync(TWO, false);
    expect(onScreen(made)).toBe(true);
    made.sync(TWO, true);
    expect(onScreen(made)).toBe(false);
  });

  it("is not there when no command answers, or when one of the ten does not", () => {
    for (const id of IDS) state[id] = null;
    const none = bar();
    none.sync(TWO);
    expect(onScreen(none)).toBe(false);
    // One missing is a registry and a table that disagree.
    state = { "edit.sameHeight": null };
    const holed = bar();
    holed.sync(TWO);
    expect(onScreen(holed)).toBe(false);
    // The control: with all ten there is one.
    state = {};
    const whole = bar();
    whole.sync(TWO);
    expect(onScreen(whole)).toBe(true);
  });
});

describe("the arrange bar's buttons", () => {
  it("are the ten arrangements between marks, in two groups with a rule between", () => {
    const made = bar();
    expect(made.buttons.map((one) => one.id)).toEqual(IDS);
    expect(BAR_GROUPS.map((group) => group.map((one) => one.id))).toEqual([IDS.slice(0, 6), IDS.slice(6)]);
    const tags = node(made).children.map((child) => child.getAttribute("role") ?? child.tagName);
    expect(tags).toEqual([...Array(6).fill("button"), "separator", ...Array(4).fill("button")]);
    expect(node(made).getAttribute("role")).toBe("toolbar");
    // Each draws a picture of its own and no words.
    const pictures = BAR_GROUPS.flat().map((one) => JSON.stringify(one.parts));
    expect(new Set(pictures).size).toBe(IDS.length);
    for (const one of made.buttons) {
      const inside = (one.button as unknown as FakeElement).children;
      expect(inside.map((child) => child.tagName), one.id).toEqual(["svg"]);
      expect(inside[0]!.children.length, one.id).toBeGreaterThan(2);
    }
  });

  it("each run their own command, once, and no other", () => {
    const made = bar();
    made.sync(TWO);
    for (const id of IDS) {
      ran.length = 0;
      button(made, id).dispatch("pointerdown", { button: 0 });
      expect(ran, id).toEqual([id]);
    }
  });

  it("are named by their command's title without its group, as label and as tooltip", () => {
    state = {
      "edit.alignLeft": { title: "Arrange: align left" },
      "edit.sameWidth": { title: "Arrange: same width" },
    };
    const made = bar();
    made.sync(TWO);
    const named = (id: string) => [button(made, id).getAttribute("aria-label"), (button(made, id) as unknown as { title: string }).title];
    expect(named("edit.alignLeft")).toEqual(["Align left", "Align left"]);
    expect(named("edit.sameWidth")).toEqual(["Same width", "Same width"]);
    expect(barLabel("Arrange: centre on the page: twice")).toBe("Centre on the page: twice");
    expect(barLabel("Plain")).toBe("Plain");
  });

  it("show a command that cannot run as disabled, say why, and do not run it", () => {
    state = {
      "edit.distributeAcross": { title: "Arrange: distribute horizontally", enabled: false, why: "needs three picked" },
      "edit.distributeDown": { title: "Arrange: distribute vertically", enabled: false, why: null },
      "edit.alignTop": { title: "Arrange: align top", why: "is never said of one that can run" },
    };
    const made = bar();
    made.sync(TWO);
    const shown = (id: string) => {
      const one = button(made, id);
      return [one.getAttribute("aria-disabled"), (one as unknown as { title: string }).title, one.style.opacity];
    };
    expect(shown("edit.distributeAcross")).toEqual(["true", "Distribute horizontally needs three picked", "0.35"]);
    // Nothing to say, as when the document is busy: the name alone.
    expect(shown("edit.distributeDown")).toEqual(["true", "Distribute vertically", "0.35"]);
    expect(shown("edit.alignTop")).toEqual(["false", "Align top", "1"]);
    expect(button(made, "edit.distributeAcross").getAttribute("aria-label")).toBe("Distribute horizontally");
    button(made, "edit.distributeAcross").dispatch("pointerdown", { button: 0 });
    button(made, "edit.distributeDown").dispatch("pointerdown", { button: 0 });
    expect(ran).toEqual([]);
  });

  it("follow their commands from one frame to the next, and ask again when pressed", () => {
    state = { "edit.distributeAcross": { enabled: false, why: "needs three picked" } };
    const made = bar();
    made.sync(TWO);
    expect(button(made, "edit.distributeAcross").getAttribute("aria-disabled")).toBe("true");
    // A third mark is picked: enabled on the next frame.
    state = {};
    made.sync([...TWO, { left: 600, top: 500, right: 700, bottom: 520 }]);
    expect(button(made, "edit.distributeAcross").getAttribute("aria-disabled")).toBe("false");
    expect((button(made, "edit.distributeAcross") as unknown as { title: string }).title).toBe("Edit.distributeAcross");
    // It stops being runnable between a frame and a press: the press asks.
    state = { "edit.alignLeft": { enabled: false } };
    button(made, "edit.alignLeft").dispatch("pointerdown", { button: 0 });
    expect(ran).toEqual([]);
    expect(button(made, "edit.alignLeft").getAttribute("aria-disabled")).toBe("false");
  });

  it("keep a press from the page and from the keyboard's focus, on a button and between two", () => {
    const made = bar();
    made.sync(TWO);
    const calls: string[] = [];
    const event = (name: string, extra: object = {}) => ({
      button: 0,
      preventDefault: () => calls.push(`${name}:prevented`),
      stopPropagation: () => calls.push(`${name}:stopped`),
      ...extra,
    });
    button(made, "edit.alignLeft").dispatch("pointerdown", event("button"));
    expect(calls).toEqual(["button:prevented", "button:stopped"]);
    expect(ran).toEqual(["edit.alignLeft"]);
    // A disabled one keeps it from the page just the same.
    state = { "edit.alignRight": { enabled: false } };
    button(made, "edit.alignRight").dispatch("pointerdown", event("off"));
    expect(calls.slice(2)).toEqual(["off:prevented", "off:stopped"]);
    node(made).dispatch("pointerdown", event("bar"));
    expect(calls.slice(4)).toEqual(["bar:prevented", "bar:stopped"]);
    // Nothing on the bar has the keyboard or can be given it with Tab.
    expect(node(made).focused).toBe(false);
    for (const one of made.buttons) {
      const pressed = one.button as unknown as FakeElement;
      expect([pressed.focused, pressed.tabIndex], one.id).toEqual([false, -1]);
    }
  });

  it("run nothing for a press with another button of the pointer", () => {
    const made = bar();
    made.sync(TWO);
    button(made, "edit.alignLeft").dispatch("pointerdown", { button: 2 });
    expect(ran).toEqual([]);
    button(made, "edit.alignLeft").dispatch("pointerdown", { button: 0 });
    expect(ran).toEqual(["edit.alignLeft"]);
  });

  it("light under the pointer when they can be pressed, and not otherwise", () => {
    state = { "edit.distributeDown": { enabled: false } };
    const made = bar();
    made.sync(TWO);
    const on = button(made, "edit.alignLeft");
    on.dispatch("pointerenter", {});
    expect(on.style.background).toContain("color-mix");
    on.dispatch("pointerleave", {});
    expect(on.style.background).toBe("none");
    const off = button(made, "edit.distributeDown");
    off.dispatch("pointerenter", {});
    expect(off.style.background ?? "").not.toContain("color-mix");
  });
});

describe("where the arrange bar goes", () => {
  const HOST = { width: 900, height: 700 };

  it("is above the marks and centred on them", () => {
    const made = bar();
    made.sync(TWO);
    // The marks span 200 to 500 across and start at 300; the gap is 10.
    expect(drawn(made)).toEqual([Math.round(350 - BAR_WIDTH / 2), 300 - 10 - BAR_HEIGHT]);
    expect(node(made).style.cssText).toContain(`width:${BAR_WIDTH}px;height:${BAR_HEIGHT}px`);
  });

  it("is below them when there is no room above", () => {
    const box = { left: 200, top: 20, right: 500, bottom: 140 };
    expect(barSpot(HOST, box)).toEqual({ left: 350 - BAR_WIDTH / 2, top: 150 });
    // The control: one pixel of room more than it needs, and it is above.
    const room = { ...box, top: 8 + BAR_HEIGHT + 10 };
    expect(barSpot(HOST, room)).toEqual({ left: 350 - BAR_WIDTH / 2, top: 8 });
    expect(barSpot(HOST, { ...room, top: room.top - 1 })?.top).toBe(150);
  });

  it("is beside them when they fill the height: on the right, or else on the left", () => {
    const tall = { left: 100, top: 10, right: 300, bottom: 690 };
    expect(barSpot(HOST, tall)).toEqual({ left: 310, top: 350 - BAR_HEIGHT / 2 });
    const right = { left: 700, top: 10, right: 880, bottom: 690 };
    expect(barSpot(HOST, right)).toEqual({ left: 700 - 10 - BAR_WIDTH, top: 350 - BAR_HEIGHT / 2 });
  });

  it("is not there when no place is clear of the marks, or when they are out of view", () => {
    expect(barSpot(HOST, { left: 100, top: 10, right: 800, bottom: 690 })).toBeNull();
    // Scrolled off above, off below, and off either side.
    expect(barSpot(HOST, { left: 200, top: -200, right: 500, bottom: -60 })).toBeNull();
    expect(barSpot(HOST, { left: 200, top: 760, right: 500, bottom: 900 })).toBeNull();
    expect(barSpot(HOST, { left: -400, top: 300, right: -100, bottom: 400 })).toBeNull();
    expect(barSpot(HOST, { left: 1000, top: 300, right: 1300, bottom: 400 })).toBeNull();
    // The control for each: one pixel in view and there is a place.
    expect(barSpot(HOST, { left: 200, top: -200, right: 500, bottom: 1 })).not.toBeNull();
    expect(barSpot(HOST, { left: 200, top: 699, right: 500, bottom: 900 })).not.toBeNull();
    expect(barSpot(HOST, { left: -400, top: 300, right: 1, bottom: 400 })).not.toBeNull();
    expect(barSpot(HOST, { left: 899, top: 300, right: 1300, bottom: 400 })).not.toBeNull();
    const made = bar();
    made.sync(TWO);
    made.sync([{ left: 100, top: 10, right: 400, bottom: 300 }, { left: 500, top: 400, right: 800, bottom: 690 }]);
    expect(onScreen(made)).toBe(false);
  });

  it("is held inside the host at each edge", () => {
    // Marks at the left and at the right edge: slid in to the margin.
    expect(barSpot(HOST, { left: 0, top: 300, right: 60, bottom: 320 })).toEqual({ left: 8, top: 300 - 10 - BAR_HEIGHT });
    expect(barSpot(HOST, { left: 860, top: 300, right: 900, bottom: 320 })).toEqual({ left: 900 - 8 - BAR_WIDTH, top: 300 - 10 - BAR_HEIGHT });
    // Beside tall marks that reach past the top, or past the bottom.
    expect(barSpot(HOST, { left: 100, top: -600, right: 300, bottom: 30 })?.top).toBe(40);
    expect(barSpot(HOST, { left: 100, top: -900, right: 300, bottom: 695 })).toEqual({ left: 310, top: 8 });
    expect(barSpot(HOST, { left: 100, top: 5, right: 300, bottom: 2000 })).toEqual({ left: 310, top: 700 - 8 - BAR_HEIGHT });
    // A host too narrow for the bar has none.
    expect(barSpot({ width: BAR_WIDTH + 15, height: 700 }, { left: 100, top: 300, right: 160, bottom: 320 })).toBeNull();
    expect(barSpot({ width: BAR_WIDTH + 16, height: 700 }, { left: 100, top: 300, right: 160, bottom: 320 })).toEqual({ left: 8, top: 300 - 10 - BAR_HEIGHT });
    // And one too low for it has none.
    expect(barSpot({ width: 900, height: BAR_HEIGHT + 15 }, { left: 100, top: 5, right: 160, bottom: 20 })).toBeNull();
  });

  it("is never over the marks and never outside the host, wherever they are", () => {
    let placed = 0;
    let sides = 0;
    for (let left = -150; left <= 850; left += 50) {
      for (let top = -150; top <= 650; top += 50) {
        for (const [wide, high] of [[40, 16], [300, 120], [700, 60], [120, 660], [880, 680]] as const) {
          const box = { left, top, right: left + wide, bottom: top + high };
          const at = barSpot(HOST, box);
          if (!at) continue;
          placed += 1;
          const own = { left: at.left, top: at.top, right: at.left + BAR_WIDTH, bottom: at.top + BAR_HEIGHT };
          expect(overlap(own, box), JSON.stringify(box)).toBe(false);
          expect(own.left >= 8 && own.top >= 8 && own.right <= 892 && own.bottom <= 692, JSON.stringify(box)).toBe(true);
          if (own.bottom > box.top && own.top < box.bottom) sides += 1;
        }
      }
    }
    // Not a loop over nothing, and the places beside the marks were among them.
    expect(placed).toBeGreaterThan(500);
    expect(sides).toBeGreaterThan(20);
  });

  it("follows the marks when it is placed again", () => {
    const made = bar();
    made.sync(TWO);
    const before = drawn(made);
    // The page scrolls 60 px up and the marks go with it.
    made.sync(TWO.map((mark) => ({ ...mark, top: mark.top - 60, bottom: mark.bottom - 60 })));
    expect(drawn(made)).toEqual([before[0], before[1] - 60]);
    // And when the host is resized under it: the clamp is the new width's.
    dom.root.clientWidth = 420;
    made.sync(TWO);
    expect(drawn(made)).toEqual([420 - 8 - BAR_WIDTH, before[1]]);
  });
});

describe("the arrange bar on the application's commands", () => {
  /** The registry as the application builds it, over actions that record. */
  function registered(picked: number, busy = false): { registry: CommandRegistry; fired: string[] } {
    const fired: string[] = [];
    const named: Partial<AppActions> = {
      viewer: () => ({}) as ReturnType<AppActions["viewer"]>,
      busyDocument: () => busy,
      pickedMarks: () => picked,
      arrange: (how) => void fired.push(`arrange:${how}`),
    };
    // Every other action answers nothing: none of them is reached from here.
    const actions = new Proxy(named, {
      get: (target, key) => (target as Record<string | symbol, unknown>)[key] ?? (() => undefined),
    }) as AppActions;
    const registry = new CommandRegistry();
    registerAppCommands(registry, actions);
    return { registry, fired };
  }

  const HOWS = ["left", "center", "right", "top", "middle", "bottom", "distributeAcross", "distributeDown", "sameWidth", "sameHeight"];

  it("has a registered Arrange command behind every button, and a press runs that arrangement", () => {
    const { registry, fired } = registered(3);
    const made = new ArrangeBar(dom.root as unknown as HTMLElement, { command: (id) => barCommand(registry, id) });
    made.sync([...TWO, { left: 600, top: 500, right: 700, bottom: 520 }]);
    expect(onScreen(made)).toBe(true);
    for (const id of IDS) {
      expect(registry.find(id)?.title, id).toMatch(/^Arrange: /);
      expect(button(made, id).getAttribute("aria-disabled"), id).toBe("false");
      button(made, id).dispatch("pointerdown", { button: 0 });
    }
    expect(fired).toEqual(HOWS.map((how) => `arrange:${how}`));
    // Through the registry's own `run`, which is what records a command as used.
    expect(registry.recents()[0]).toBe("edit.sameHeight");
    expect((button(made, "edit.alignCenter") as unknown as { title: string }).title).toBe(
      barLabel(registry.find("edit.alignCenter")!.title),
    );
  });

  it("greys spacing with two picked and says it needs three, as the command does", () => {
    const { registry, fired } = registered(2);
    const made = new ArrangeBar(dom.root as unknown as HTMLElement, { command: (id) => barCommand(registry, id) });
    made.sync(TWO);
    const off = IDS.filter((id) => button(made, id).getAttribute("aria-disabled") === "true");
    expect(off).toEqual(["edit.distributeAcross", "edit.distributeDown"]);
    // The same answer the registry gives the menu and the palette.
    expect(off).toEqual(IDS.filter((id) => registry.find(id)?.enabled?.() === false));
    expect((button(made, "edit.distributeAcross") as unknown as { title: string }).title).toBe(
      "Distribute horizontally needs three picked",
    );
    expect((button(made, "edit.distributeDown") as unknown as { title: string }).title).toBe(
      "Distribute vertically needs three picked",
    );
    button(made, "edit.distributeAcross").dispatch("pointerdown", { button: 0 });
    expect(fired).toEqual([]);
  });

  it("greys every button while the document is busy, with nothing to say about how many are picked", () => {
    const { registry, fired } = registered(3, true);
    const made = new ArrangeBar(dom.root as unknown as HTMLElement, { command: (id) => barCommand(registry, id) });
    made.sync(TWO);
    expect(IDS.filter((id) => button(made, id).getAttribute("aria-disabled") === "true")).toEqual(IDS);
    expect((button(made, "edit.alignLeft") as unknown as { title: string }).title).toBe("Align left");
    button(made, "edit.alignLeft").dispatch("pointerdown", { button: 0 });
    expect(fired).toEqual([]);
  });

  it("answers nothing for an id no command has", () => {
    const { registry } = registered(3);
    expect(barCommand(registry, "edit.alignNowhere")).toBeUndefined();
    expect(barCommand(registry, "edit.alignLeft")).toMatchObject({ title: "Arrange: align left", enabled: true, why: null });
    // A command with no `enabled` of its own can always run, and has no reason not to.
    const plain = new CommandRegistry();
    plain.register({ id: "x.y", title: "Group: thing", run: () => {} });
    expect(barCommand(plain, "x.y")).toMatchObject({ enabled: true, why: null });
  });
});
