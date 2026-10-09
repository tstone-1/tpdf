/**
 * Dragging a tab onto a side of the window.
 *
 * A tab pressed and carried over the page area is dropped on a side: the half
 * of the area the pointer is over, or with two sides showing, the side it is
 * over. This holds what a press has turned into and which side a drop would
 * land on; `App.svelte` listens for the pointer and carries the drop out.
 *
 * **A press is a click until it has travelled.** A tab is a button, and a
 * hand that presses one moves a pixel or two. Nothing here is a drag before
 * the pointer is {@link SLOP} from where it went down, and a press that ends
 * before that is the click it always was.
 *
 * Pointer events and no HTML drag and drop: the window takes file drops
 * through the shell, and on Windows that turns the page's own drag events off.
 */

import type { Side } from "./panes";

/** How far a pressed pointer travels before the tab is being dragged, in CSS pixels. */
export const SLOP = 6;

/** The page area of the window, in the pointer's coordinates. */
export interface DropArea {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** What a drop is decided against. */
export interface DropLayout {
  area: DropArea;
  /** Whether two sides are showing. */
  split: boolean;
  /** The share of the width the left side has, with two sides. */
  share: number;
  /** The side the dragged tab is on. */
  from: Side;
  /** How many tabs are open. */
  tabs: number;
}

/**
 * The side a tab let go at (`x`, `y`) lands on, or null for nowhere.
 *
 * Nowhere is outside the page area, the side the tab is already on while two
 * are showing, and anywhere at all with one tab open, which cannot be shown
 * beside itself. With one side showing, both halves are an answer: the right
 * half puts the tab on the right, and the left half puts it on the left with
 * every other tab on the right.
 */
export function dropSide(x: number, y: number, layout: DropLayout): Side | null {
  const { area } = layout;
  if (layout.tabs < 2 || area.width <= 0 || area.height <= 0) return null;
  if (x < area.left || x >= area.left + area.width) return null;
  if (y < area.top || y >= area.top + area.height) return null;
  const boundary = area.left + area.width * (layout.split ? layout.share : 0.5);
  const side: Side = x < boundary ? "left" : "right";
  return layout.split && side === layout.from ? null : side;
}

/** A tab being dragged, and the side it would land on if let go now. */
export interface Carried {
  id: number;
  side: Side | null;
}

export class TabDrag {
  #press: { id: number; x: number; y: number } | null = null;
  #carried: Carried | null = null;

  /** The tab being dragged, or null while nothing is, a press included. */
  get carried(): Carried | null {
    return this.#carried;
  }

  /** Whether a tab is pressed, dragged yet or not. */
  get pressed(): boolean {
    return this.#press !== null;
  }

  /** The pointer went down on tab `id`. */
  press(id: number, x: number, y: number): void {
    this.#press = { id, x, y };
    this.#carried = null;
  }

  /**
   * The pointer moved. Answers what is being dragged, which is nothing until
   * the pointer has left the place it was pressed.
   */
  move(x: number, y: number, layout: DropLayout): Carried | null {
    const press = this.#press;
    if (!press) return null;
    if (!this.#carried && Math.hypot(x - press.x, y - press.y) < SLOP) return null;
    this.#carried = { id: press.id, side: dropSide(x, y, layout) };
    return this.#carried;
  }

  /**
   * The pointer was let go. Answers the drag it ended, with the side read at
   * the last move, or null when the press never became one.
   */
  release(): Carried | null {
    const carried = this.#carried;
    this.cancel();
    return carried;
  }

  /** The press is over and nothing is dropped. */
  cancel(): void {
    this.#press = null;
    this.#carried = null;
  }
}
