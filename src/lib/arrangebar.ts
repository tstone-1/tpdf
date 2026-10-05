/**
 * The bar that appears beside several picked marks, one button per arrangement.
 *
 * ## Why it exists
 *
 * Two form fields pressed one after the other, and then nothing: the commands
 * that line them up were in the Arrange menu and in the palette, and nothing on
 * the page said so. A reader who has picked two rectangles has said what they
 * are about to do, so the arrangements are put where their eyes already are.
 *
 * ## It is a second door to the same commands, and nothing else
 *
 * Every button names a command by its id. What the button is called, whether
 * it can be pressed, why not when it cannot, and what a press does are all
 * asked of that command, each time the bar is drawn and again when it is
 * pressed. Nothing here knows how many marks an arrangement needs or what one
 * moves; a rule kept here as well would be a second rule the first time the
 * commands changed.
 *
 * ## Why the bar stops at ten
 *
 * It holds the arrangements *between* the picked marks: six alignments, two
 * spacings, one width, one height. *Same size* is the last two pressed one
 * after the other. *Duplicate* makes marks and does not arrange them, and the
 * two that centre on the page are about the page and work with one mark
 * picked, when there is no bar. All four stay in the menu and the palette.
 *
 * ## It never has the keyboard
 *
 * A reader with several marks picked moves them with the arrow keys, and those
 * go to the page. So no button can be reached with Tab or takes focus when it
 * is pressed, and after a press the next arrow key still nudges the marks.
 * Every command here has a menu item and a palette entry for a reader who does
 * not use a pointer.
 */

import type { CommandRegistry } from "./commands";
import { iconFrom, type IconPart } from "./icons";
import type { Anchor } from "./popup";

/** A command as the bar sees it, read when asked and not kept. */
export interface BarCommand {
  /** The command's title, as the palette shows it. */
  title: string;
  /** Whether it can run now. */
  enabled: boolean;
  /** What is missing when it cannot, in words that follow its name, or `null`. */
  why: string | null;
  /** Runs it, as choosing it from the menu does. */
  run: () => void;
}

/** What the bar needs from its host. */
export interface ArrangeBarOptions {
  /** The command with this id as it is now, or `undefined` if there is none. */
  command: (id: string) => BarCommand | undefined;
}

/**
 * A command out of the registry, in the form the bar asks for.
 *
 * `run` goes through the registry's own `run`, which is what the menu and the
 * palette call: it refuses a command that is not enabled and records the one
 * it ran as recent.
 */
export function barCommand(registry: CommandRegistry, id: string): BarCommand | undefined {
  const command = registry.find(id);
  if (!command) return undefined;
  return {
    title: command.title,
    enabled: command.enabled?.() ?? true,
    why: command.why?.() ?? null,
    run: () => void registry.run(id),
  };
}

interface BarButton {
  /** The command the button stands for. */
  readonly id: string;
  /** Its picture, on the 24 by 24 grid `icons.ts` draws on. */
  readonly parts: readonly IconPart[];
}

/**
 * The buttons, in the order they are drawn: the alignments, then spacing and
 * size. Each picture is ours: a rule for the edge or centre the marks are
 * taken to, and two filled rectangles of different lengths against it.
 */
export const BAR_GROUPS: readonly (readonly BarButton[])[] = [
  [
    { id: "edit.alignLeft", parts: [{ d: "M4 3v18" }, { rect: [8, 6, 12, 4, 1], filled: true }, { rect: [8, 14, 7, 4, 1], filled: true }] },
    { id: "edit.alignCenter", parts: [{ d: "M12 3v18" }, { rect: [5, 6, 14, 4, 1], filled: true }, { rect: [8, 14, 8, 4, 1], filled: true }] },
    { id: "edit.alignRight", parts: [{ d: "M20 3v18" }, { rect: [4, 6, 12, 4, 1], filled: true }, { rect: [9, 14, 7, 4, 1], filled: true }] },
    { id: "edit.alignTop", parts: [{ d: "M3 4h18" }, { rect: [6, 8, 4, 12, 1], filled: true }, { rect: [14, 8, 4, 7, 1], filled: true }] },
    { id: "edit.alignMiddle", parts: [{ d: "M3 12h18" }, { rect: [6, 5, 4, 14, 1], filled: true }, { rect: [14, 8, 4, 8, 1], filled: true }] },
    { id: "edit.alignBottom", parts: [{ d: "M3 20h18" }, { rect: [6, 4, 4, 12, 1], filled: true }, { rect: [14, 9, 4, 7, 1], filled: true }] },
  ],
  [
    // Two rules with the marks spaced between them.
    { id: "edit.distributeAcross", parts: [{ d: "M3 3v18" }, { d: "M21 3v18" }, { rect: [8, 7, 2, 10, 1], filled: true }, { rect: [14, 7, 2, 10, 1], filled: true }] },
    { id: "edit.distributeDown", parts: [{ d: "M3 3h18" }, { d: "M3 21h18" }, { rect: [7, 8, 10, 2, 1], filled: true }, { rect: [7, 14, 10, 2, 1], filled: true }] },
    // Two marks of one width, or one height, and an arrow along it.
    { id: "edit.sameWidth", parts: [{ rect: [5, 4, 14, 2, 1], filled: true }, { rect: [5, 18, 14, 2, 1], filled: true }, { d: "M5 12h14" }, { d: "m8 9-3 3 3 3" }, { d: "m16 9 3 3-3 3" }] },
    { id: "edit.sameHeight", parts: [{ rect: [4, 5, 2, 14, 1], filled: true }, { rect: [18, 5, 2, 14, 1], filled: true }, { d: "M12 5v14" }, { d: "m9 8 3-3 3 3" }, { d: "m9 16 3 3 3-3" }] },
  ],
];

/**
 * How many marks have to be picked before there is a bar: an arrangement
 * between marks takes two of them. Which arrangements two are enough for is
 * the commands' answer, button by button.
 */
const BETWEEN = 2;

/** A button's side, in CSS pixels. */
const BUTTON = 24;
/** The space between two buttons, and round the rule between the groups. */
const SPACE = 2;
/** The space between the buttons and the bar's own border. */
const PAD = 3;
/** The gap kept between the bar and the marks it belongs to. */
const GAP = 10;
/** The least room kept between the bar and the host's edge. */
const MARGIN = 8;

const COUNT = BAR_GROUPS.reduce((sum, group) => sum + group.length, 0);
const RULES = BAR_GROUPS.length - 1;

/** The bar's width: the buttons, the rules between groups, the spaces, the padding and the border. */
export const BAR_WIDTH = COUNT * BUTTON + RULES + (COUNT + RULES - 1) * SPACE + 2 * PAD + 2;
/** The bar's height: one button, the padding and the border. */
export const BAR_HEIGHT = BUTTON + 2 * PAD + 2;

const HOVER = "color-mix(in srgb, currentColor 14%, transparent)";

/** The rectangle round every picked mark. */
function around(marks: readonly Anchor[]): Anchor {
  return {
    left: Math.min(...marks.map((mark) => mark.left)),
    top: Math.min(...marks.map((mark) => mark.top)),
    right: Math.max(...marks.map((mark) => mark.right)),
    bottom: Math.max(...marks.map((mark) => mark.bottom)),
  };
}

/** A value held between two bounds, the lower one winning when they cross. */
function within(value: number, low: number, high: number): number {
  return Math.max(low, Math.min(value, high));
}

/**
 * Where the bar goes for marks that fill `box`, in a host of this size, or
 * `null` when there is no place for it.
 *
 * Above the marks and centred on them; below when there is no room above;
 * then to their right, then to their left, level with their middle. Along the
 * other direction it is slid to stay inside the host, which never brings it
 * onto the marks: each place is a whole gap clear of them in its own
 * direction. With no clear place there is no bar, since one drawn over the
 * marks would hide what it arranges; and none for marks that are out of view.
 */
export function barSpot(
  host: { width: number; height: number },
  box: Anchor,
): { left: number; top: number } | null {
  const seen = box.right > 0 && box.left < host.width && box.bottom > 0 && box.top < host.height;
  if (!seen) return null;
  const across = within((box.left + box.right - BAR_WIDTH) / 2, MARGIN, host.width - BAR_WIDTH - MARGIN);
  const down = within((box.top + box.bottom - BAR_HEIGHT) / 2, MARGIN, host.height - BAR_HEIGHT - MARGIN);
  const places = [
    { left: across, top: box.top - GAP - BAR_HEIGHT },
    { left: across, top: box.bottom + GAP },
    { left: box.right + GAP, top: down },
    { left: box.left - GAP - BAR_WIDTH, top: down },
  ];
  const fits = (place: { left: number; top: number }): boolean =>
    place.left >= MARGIN
    && place.top >= MARGIN
    && place.left + BAR_WIDTH <= host.width - MARGIN
    && place.top + BAR_HEIGHT <= host.height - MARGIN;
  return places.find(fits) ?? null;
}

/**
 * A command's title as a button's name: what follows the group's name and its
 * colon, with a capital. "Arrange: align left" is "Align left".
 */
export function barLabel(title: string): string {
  const said = title.replace(/^[^:]*:\s*/, "");
  return said.charAt(0).toUpperCase() + said.slice(1);
}

/** The arrange bar for one viewer. */
export class ArrangeBar {
  private readonly host: HTMLElement;
  private readonly element: HTMLElement;
  private readonly opts: ArrangeBarOptions;
  /** One per command, in {@link BAR_GROUPS} order. */
  private readonly made: { id: string; button: HTMLButtonElement }[] = [];
  private shown = false;

  constructor(host: HTMLElement, opts: ArrangeBarOptions) {
    this.host = host;
    this.opts = opts;
    this.element = document.createElement("div");
    this.element.setAttribute("role", "toolbar");
    this.element.setAttribute("aria-label", "Arrange");
    // The border and the shadow are the note box's, so the two read as one
    // family; a step under it, which only matters if a reader drags the box
    // over the bar.
    this.element.style.cssText =
      "position:absolute;display:none;z-index:4;box-sizing:border-box;" +
      `width:${BAR_WIDTH}px;height:${BAR_HEIGHT}px;padding:${PAD}px;gap:${SPACE}px;` +
      "align-items:center;border-radius:8px;" +
      "background:Canvas;color:CanvasText;" +
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);" +
      "box-shadow:0 6px 24px rgba(0,0,0,0.25);" +
      "font:13px/1.45 system-ui,-apple-system,sans-serif;";
    // A press anywhere on the bar, between two buttons as much as on one,
    // must not reach the page: there it is a press beside the marks, which
    // picks nothing. Prevented as well, so it takes no focus from the page.
    this.element.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      event.stopPropagation();
    });

    BAR_GROUPS.forEach((group, at) => {
      if (at > 0) this.element.append(this.rule());
      for (const entry of group) this.element.append(this.button(entry));
    });
    host.appendChild(this.element);
  }

  /** The bar's element. For the check harness. */
  get node(): HTMLElement {
    return this.element;
  }

  /** The buttons with the command each stands for, in the order drawn. For the check harness. */
  get buttons(): readonly { id: string; button: HTMLButtonElement }[] {
    return this.made;
  }

  /** Whether the bar is on screen. */
  get visible(): boolean {
    return this.shown;
  }

  /**
   * Shows the bar beside these marks, moves it, or takes it away.
   *
   * `marks` are the picked marks' rectangles in the host's coordinates. Called
   * on every frame, which is what makes the bar follow a scroll, a zoom and a
   * drag, and what makes each button's state follow the commands.
   *
   * `held` is true while something else has the reader's attention on one of
   * these marks: the note box is open on it. The box is placed beside its mark
   * and can be dragged anywhere, so the bar steps aside for as long as it is
   * open and nothing has to keep the two apart.
   */
  sync(marks: readonly Anchor[], held = false): void {
    const at = marks.length >= BETWEEN && !held
      ? barSpot({ width: this.host.clientWidth, height: this.host.clientHeight }, around(marks))
      : null;
    // A host that answers for none of the commands has no bar, and one that
    // is missing any of them has none either: a bar with a hole in it is a
    // registry and a table that disagree, which a reader cannot do anything
    // about.
    const commands = at ? this.made.map((one) => this.opts.command(one.id)) : [];
    if (!at || commands.some((command) => command === undefined)) {
      this.hide();
      return;
    }
    this.made.forEach((one, index) => {
      const command = commands[index];
      if (command) this.dress(one.button, command);
    });
    this.element.style.left = `${Math.round(at.left)}px`;
    this.element.style.top = `${Math.round(at.top)}px`;
    this.element.style.display = "flex";
    this.shown = true;
  }

  /** Takes the bar off the screen. */
  hide(): void {
    this.shown = false;
    this.element.style.display = "none";
  }

  /** Gives a button its command's name and state as they are now. */
  private dress(button: HTMLButtonElement, command: BarCommand): void {
    const label = barLabel(command.title);
    button.setAttribute("aria-label", label);
    // Not the `disabled` attribute: a disabled button shows no tooltip in
    // every web view, and the tooltip is where a reader learns what is
    // missing.
    button.setAttribute("aria-disabled", String(!command.enabled));
    button.title = !command.enabled && command.why ? `${label} ${command.why}` : label;
    button.style.opacity = command.enabled ? "1" : "0.35";
  }

  /** The thin line between the two groups. */
  private rule(): HTMLElement {
    const rule = document.createElement("div");
    rule.setAttribute("role", "separator");
    rule.style.cssText =
      `flex:none;width:1px;height:${BUTTON - 6}px;` +
      "background:color-mix(in srgb, currentColor 25%, transparent);";
    return rule;
  }

  private button(entry: BarButton): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    // Out of the Tab order: see the module note.
    button.tabIndex = -1;
    button.style.cssText =
      `flex:none;width:${BUTTON}px;height:${BUTTON}px;padding:0;` +
      "display:flex;align-items:center;justify-content:center;" +
      "border:0;border-radius:5px;background:none;color:inherit;cursor:default;";
    button.append(iconFrom(entry.parts));
    button.addEventListener("pointerenter", () => {
      if (button.getAttribute("aria-disabled") !== "true") button.style.background = HOVER;
    });
    button.addEventListener("pointerleave", () => {
      button.style.background = "none";
    });
    button.addEventListener("pointerdown", (event) => {
      // For the reasons the bar's own handler gives, which this one stands in
      // front of.
      event.preventDefault();
      event.stopPropagation();
      // Only the main button: a press with any other is not a choice.
      if (event.button) return;
      // Asked again now and not read off the button, which shows what was
      // true when the last frame was drawn.
      const command = this.opts.command(entry.id);
      if (command?.enabled) command.run();
    });
    this.made.push({ id: entry.id, button });
    return button;
  }
}
