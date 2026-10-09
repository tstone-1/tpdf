/**
 * Which open document is on which side of the window.
 *
 * The window shows one document, or two side by side. Every tab belongs to a
 * side, each side has one tab in front, and one side is the focused one: the
 * side the toolbar, the sidebar, the find field and every command act on. With
 * no tab on the right there is no split, and the window is as it was before
 * sides existed.
 *
 * This holds the decisions and none of the window: which tab comes to the
 * front when the one in front leaves, when the split ends, and what has to be
 * mounted or torn down for the window to show what is decided here
 * ({@link Panes.plan}). `App.svelte` carries the plan out.
 *
 * **A side is not a place on screen.** The window has two page areas, numbered
 * 0 and 1 in the order they were built, and a viewer mounted in one stays
 * there. When the left side empties, the tabs on the right *become* the left
 * side and keep their page area; {@link Panes.slotOf} says which area a side
 * is drawn in, and the window orders the two areas by it. Swapping the sides
 * is the same move, so neither rebuilds a viewer.
 *
 * Tab order is not kept here. It is `DocumentTabs`', and every method that
 * needs it is handed the handles in that order.
 */

export type Side = "left" | "right";
/** One of the window's two page areas, in the order they were built. */
export type Slot = 0 | 1;

export function otherSide(side: Side): Side {
  return side === "left" ? "right" : "left";
}

/** What the window has to do to show what the model says. */
export interface Plan<Id extends number = number> {
  /** Viewers to tear down, each with the area it is in. */
  unmount: { id: Id; slot: Slot }[];
  /** Tabs to mount, each with the area it goes in. */
  mount: { id: Id; slot: Slot }[];
  /** The document the commands act on once that is done, or -1. */
  focus: Id;
}

/** What a side showing nothing has in front. */
const NONE = -1;

/**
 * `Id` is what a tab is named by. A plain number unless the caller has a type
 * of its own for it, as `App.svelte` has in `ViewId`.
 */
export class Panes<Id extends number = number> {
  /** The tabs on the right. Every other tab is on the left. */
  readonly #right = new Set<Id>();
  readonly #front: Record<Side, Id> = { left: NONE as Id, right: NONE as Id };
  #focused: Side = "left";
  #flipped = false;
  /** The document mounted in each page area, or -1. */
  readonly #mounted: [Id, Id] = [NONE as Id, NONE as Id];

  /** Whether two documents are shown side by side. */
  get split(): boolean {
    return this.#right.size > 0;
  }

  get focused(): Side {
    return this.#focused;
  }

  sideOf(id: Id): Side {
    return this.#right.has(id) ? "right" : "left";
  }

  /** The tab in front on `side`, or -1. */
  front(side: Side): Id {
    return this.#front[side];
  }

  /** The handles on `side`, in tab order. */
  on(side: Side, order: readonly Id[]): Id[] {
    return order.filter((id) => this.sideOf(id) === side);
  }

  /** The page area `side` is drawn in. */
  slotOf(side: Side): Slot {
    return (side === "left") !== this.#flipped ? 0 : 1;
  }

  /** The side drawn in page area `slot`. */
  sideIn(slot: Slot): Side {
    return this.slotOf("left") === slot ? "left" : "right";
  }

  /** A tab was opened without being shown. It joins the focused side. */
  joined(id: Id): void {
    if (this.#focused === "right") this.#right.add(id);
  }

  /** A tab came to the front of its side, and the reader is working in it. */
  fronted(id: Id): void {
    const side = this.sideOf(id);
    this.#front[side] = id;
    this.#focused = side;
  }

  /** A document was opened in the focused side and is in front there. */
  opened(id: Id): void {
    this.joined(id);
    this.fronted(id);
  }

  /**
   * A save gave the document `before` a new handle. It keeps its side, its
   * place in front and its page area.
   */
  replaced(before: Id, after: Id): void {
    if (before === after) return;
    if (this.#right.delete(before)) this.#right.add(after);
    for (const side of ["left", "right"] as const) {
      if (this.#front[side] === before) this.#front[side] = after;
    }
    for (const slot of [0, 1] as const) {
      if (this.#mounted[slot] === before) this.#mounted[slot] = after;
    }
  }

  /**
   * Makes `side` the focused one. False when it shows nothing, which is the
   * case for the right side with no split.
   */
  focus(side: Side): boolean {
    if (this.#front[side] < 0) return false;
    this.#focused = side;
    return true;
  }

  /**
   * Moves a tab to `to`, in front there and focused. `order` is every open
   * handle in tab order. The side it left shows its next tab, or the one
   * before when it was last; a side left with none ends the split.
   */
  move(id: Id, to: Side, order: readonly Id[]): void {
    const from = this.sideOf(id);
    if (from !== to) {
      if (this.#front[from] === id) this.#front[from] = this.#neighbour(id, from, order);
      if (to === "right") this.#right.add(id);
      else this.#right.delete(id);
    }
    this.#front[to] = id;
    this.#focused = to;
    this.#settle(order);
  }

  /**
   * A tab is being closed. `order` is every open handle in tab order,
   * including this one. Its side shows the neighbour; a side left with none
   * ends the split.
   */
  closed(id: Id, order: readonly Id[]): void {
    const side = this.sideOf(id);
    if (this.#front[side] === id) this.#front[side] = this.#neighbour(id, side, order);
    this.#right.delete(id);
    this.#settle(order.filter((entry) => entry !== id));
  }

  /** Every tab is gone. */
  cleared(): void {
    this.#right.clear();
    this.#front.left = NONE as Id;
    this.#front.right = NONE as Id;
    this.#focused = "left";
  }

  /**
   * Exchanges the two sides: every tab changes side, and the focused document
   * stays the focused document. Nothing is remounted, because the page areas
   * change sides with their tabs. Does nothing with no split.
   */
  swap(order: readonly Id[]): void {
    if (!this.split) return;
    const left = this.on("left", order);
    this.#right.clear();
    for (const id of left) this.#right.add(id);
    [this.#front.left, this.#front.right] = [this.#front.right, this.#front.left];
    this.#focused = otherSide(this.#focused);
    this.#flipped = !this.#flipped;
  }

  /**
   * Puts every tab on the side a launch read back: the handles in `right` on
   * the right and the rest on the left, `beside` in front of the side the
   * reader is not in, and `active`, the document they are looking at, in front
   * of its own side and focused. `order` is every open handle in tab order.
   *
   * The viewer `active` already has stays where it is: when its side would be
   * drawn in the other page area, the two areas change sides instead. A
   * handle that is not open is ignored, and with nothing left on one of the
   * sides there is no split.
   */
  arrange(right: readonly Id[], beside: Id, active: Id, order: readonly Id[]): void {
    this.#right.clear();
    for (const id of right) this.#right.add(id);
    for (const side of ["left", "right"] as const) {
      const mine = this.on(side, order);
      this.#front[side] = [active, beside].find((id) => mine.includes(id)) ?? mine[0] ?? (NONE as Id);
    }
    if (order.includes(active)) this.#focused = this.sideOf(active);
    this.#settle(order);
    const area = this.#mounted.indexOf(active);
    if (area >= 0 && this.slotOf(this.sideOf(active)) !== area) this.#flipped = !this.#flipped;
  }

  /** A viewer for `id` now exists in page area `slot`. */
  mounted(id: Id, slot: Slot): void {
    this.#mounted[slot] = id;
  }

  /** The viewer for `id` is gone. */
  unmounted(id: Id): void {
    for (const slot of [0, 1] as const) {
      if (this.#mounted[slot] === id) this.#mounted[slot] = NONE as Id;
    }
  }

  /** The document mounted in page area `slot`, or -1. */
  mountedIn(slot: Slot): Id {
    return this.#mounted[slot];
  }

  /**
   * What differs between the tabs in front and the viewers that exist.
   *
   * A document in front on one side and mounted in the other side's area is
   * torn down and mounted again: a viewer does not change areas.
   */
  plan(): Plan<Id> {
    const plan: Plan<Id> = { unmount: [], mount: [], focus: this.#front[this.#focused] };
    for (const slot of [0, 1] as const) {
      const wanted = this.#front[this.sideIn(slot)];
      const there = this.#mounted[slot];
      if (wanted === there) continue;
      if (there >= 0) plan.unmount.push({ id: there, slot });
      if (wanted >= 0) plan.mount.push({ id: wanted, slot });
    }
    return plan;
  }

  /** The tab that takes `id`'s place in front of `side`: the next, else the one before. */
  #neighbour(id: Id, side: Side, order: readonly Id[]): Id {
    const mine = this.on(side, order);
    const at = mine.indexOf(id);
    const rest = mine.filter((entry) => entry !== id);
    return rest[Math.min(at, rest.length - 1)] ?? (NONE as Id);
  }

  /**
   * Restores what holds between any two operations: tabs only on the right
   * are the left side, a side with tabs has one in front, and the focused side
   * shows something whenever either does.
   */
  #settle(order: readonly Id[]): void {
    for (const id of [...this.#right]) {
      if (!order.includes(id)) this.#right.delete(id);
    }
    if (this.#right.size > 0 && this.on("left", order).length === 0) {
      // The right side is all there is, so it is the left side now and keeps
      // its page area.
      this.#right.clear();
      this.#front.left = this.#front.right;
      this.#front.right = NONE as Id;
      this.#flipped = !this.#flipped;
      this.#focused = "left";
    }
    if (this.#right.size === 0) this.#front.right = NONE as Id;
    if (this.#front[this.#focused] < 0 && this.#front[otherSide(this.#focused)] >= 0) {
      this.#focused = otherSide(this.#focused);
    }
  }
}

/**
 * Which of `names` the reader means by what they typed.
 *
 * For "Show side by side with...", which asks for the other document by name.
 * A name typed in full wins over a longer name that contains it; otherwise the
 * words have to be part of exactly one name. Nothing typed is an answer only
 * when there is one document to choose.
 */
export function pickPartner(
  raw: string,
  names: readonly string[],
): { index: number } | { problem: string } {
  const typed = raw.trim().toLowerCase();
  const lower = names.map((name) => name.toLowerCase());
  if (names.length === 0) return { problem: "No other document is open" };
  if (typed === "") {
    return names.length === 1 ? { index: 0 } : { problem: "Type part of a document's name" };
  }
  const exact = lower.indexOf(typed);
  if (exact >= 0 && lower.lastIndexOf(typed) === exact) return { index: exact };
  const matching = lower.flatMap((name, index) => (name.includes(typed) ? [index] : []));
  const only = matching[0];
  if (only === undefined) return { problem: "No other open document has that in its name" };
  if (matching.length > 1) {
    return { problem: `${matching.length} open documents match. Type more of the name` };
  }
  return { index: only };
}

/** The names the question offers, cut to what fits the field. */
export function partnerPlaceholder(names: readonly string[]): string {
  const shown = names.slice(0, 3).join(", ");
  return names.length > 3 ? `${shown} and ${names.length - 3} more` : shown;
}
