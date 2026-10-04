/**
 * Arranging several placed rectangles against each other: aligning their
 * edges or centres, spacing them evenly, giving them one size.
 *
 * Two things live here and nothing else does: which marks a reader has
 * picked, and where an arrangement puts them. Both are plain data, so both
 * are tested without a window. The viewer supplies the rectangles as they
 * are laid out on the page, which is the space a reader's "left" is in on a
 * turned view, and maps the answers back to the file.
 *
 * **The first mark picked is the one the others follow.** Aligning left puts
 * every left edge where the first mark's is; one width is the first mark's
 * width. A reader therefore chooses the reference by the order they pick in,
 * and nothing is moved to an edge no mark was on.
 */

import type { Quad } from "./markband";

/** Every arrangement there is a command for. */
export const ARRANGEMENTS = [
  "left",
  "center",
  "right",
  "top",
  "middle",
  "bottom",
  "distributeAcross",
  "distributeDown",
  "sameWidth",
  "sameHeight",
  "sameSize",
  "pageCenter",
  "pageMiddle",
] as const;

export type Arrangement = (typeof ARRANGEMENTS)[number];

/**
 * How many marks an arrangement needs before it does anything.
 *
 * Spacing needs three: with two there is nothing between the outer two to
 * space. Centring on the page moves one mark as readily as several.
 */
export function needs(how: Arrangement): number {
  if (how === "pageCenter" || how === "pageMiddle") return 1;
  if (how === "distributeAcross" || how === "distributeDown") return 3;
  return 2;
}

interface Size {
  width: number;
  height: number;
}

const width = (quad: Quad): number => quad.right - quad.left;
const height = (quad: Quad): number => quad.bottom - quad.top;

function moved(quad: Quad, dx: number, dy: number): Quad {
  return { left: quad.left + dx, top: quad.top + dy, right: quad.right + dx, bottom: quad.bottom + dy };
}

/** A rectangle shifted back onto the page, by the least that brings it on. */
function onPage(quad: Quad, page: Size): Quad {
  const dx = quad.left < 0 ? -quad.left : quad.right > page.width ? page.width - quad.right : 0;
  const dy = quad.top < 0 ? -quad.top : quad.bottom > page.height ? page.height - quad.bottom : 0;
  return moved(quad, dx, dy);
}

/**
 * Even gaps along one axis. The two outermost rectangles stay where they are
 * and the ones between are placed so the space between neighbours is equal.
 * Ordered by centre, so two that overlap keep the order a reader sees.
 */
function spread(quads: readonly Quad[], across: boolean): Quad[] {
  const start = (quad: Quad): number => (across ? quad.left : quad.top);
  const end = (quad: Quad): number => (across ? quad.right : quad.bottom);
  const order = quads
    .map((quad, at) => ({ at, mid: (start(quad) + end(quad)) / 2 }))
    .sort((a, b) => a.mid - b.mid || a.at - b.at)
    .map((entry) => entry.at);
  const first = quads[order[0] ?? 0];
  const last = quads[order[order.length - 1] ?? 0];
  if (!first || !last) return [...quads];
  const taken = quads.reduce((sum, quad) => sum + (end(quad) - start(quad)), 0);
  const gap = (end(last) - start(first) - taken) / (quads.length - 1);
  const out = [...quads];
  let at = start(first);
  for (const index of order) {
    const quad = quads[index];
    if (!quad) continue;
    const shift = at - start(quad);
    out[index] = across ? moved(quad, shift, 0) : moved(quad, 0, shift);
    at += end(quad) - start(quad) + gap;
  }
  return out;
}

/**
 * Where an arrangement puts each rectangle, in the order they were given.
 *
 * `quads[0]` is the first mark picked. Fewer than {@link needs} asks for come
 * back where they were, and so does anything not finite: a caller sends only
 * what differs from what it gave, so that comes to nothing.
 * Every answer lies on the page.
 */
export function arrange(quads: readonly Quad[], how: Arrangement, page: Size): Quad[] {
  const first = quads[0];
  const sound = quads.every((quad) =>
    [quad.left, quad.top, quad.right, quad.bottom].every(Number.isFinite),
  );
  // No test of `needs(how)` here: one mark aligned with itself does not move,
  // and two spaced evenly are the outer two, which stay.
  if (!first || !sound) return [...quads];
  const each = (place: (quad: Quad) => Quad): Quad[] => quads.map((quad) => onPage(place(quad), page));
  switch (how) {
    case "left":
      return each((quad) => moved(quad, first.left - quad.left, 0));
    case "right":
      return each((quad) => moved(quad, first.right - quad.right, 0));
    case "center":
      return each((quad) => moved(quad, (first.left + first.right - quad.left - quad.right) / 2, 0));
    case "top":
      return each((quad) => moved(quad, 0, first.top - quad.top));
    case "bottom":
      return each((quad) => moved(quad, 0, first.bottom - quad.bottom));
    case "middle":
      return each((quad) => moved(quad, 0, (first.top + first.bottom - quad.top - quad.bottom) / 2));
    case "distributeAcross":
      return spread(quads, true).map((quad) => onPage(quad, page));
    case "distributeDown":
      return spread(quads, false).map((quad) => onPage(quad, page));
    case "sameWidth":
      return each((quad) => ({ ...quad, right: quad.left + width(first) }));
    case "sameHeight":
      return each((quad) => ({ ...quad, bottom: quad.top + height(first) }));
    case "sameSize":
      return each((quad) => ({ ...quad, right: quad.left + width(first), bottom: quad.top + height(first) }));
    case "pageCenter":
    case "pageMiddle": {
      // The picked marks as one block, so what a reader lined up stays lined up.
      const left = Math.min(...quads.map((quad) => quad.left));
      const right = Math.max(...quads.map((quad) => quad.right));
      const top = Math.min(...quads.map((quad) => quad.top));
      const bottom = Math.max(...quads.map((quad) => quad.bottom));
      const dx = how === "pageCenter" ? (page.width - left - right) / 2 : 0;
      const dy = how === "pageMiddle" ? (page.height - top - bottom) / 2 : 0;
      return each((quad) => moved(quad, dx, dy));
    }
  }
}

/**
 * The offset nearest the one asked for that keeps every rectangle on the
 * page: what several marks dragged or nudged together move by. They keep
 * their places against each other, so the one nearest an edge stops them all.
 */
export function together(
  quads: readonly Quad[],
  want: { dx: number; dy: number },
  page: Size,
): { dx: number; dy: number } {
  if (quads.length === 0 || !Number.isFinite(want.dx) || !Number.isFinite(want.dy)) return { dx: 0, dy: 0 };
  const left = Math.min(...quads.map((quad) => quad.left));
  const top = Math.min(...quads.map((quad) => quad.top));
  const right = Math.max(...quads.map((quad) => quad.right));
  const bottom = Math.max(...quads.map((quad) => quad.bottom));
  return {
    dx: Math.min(Math.max(want.dx, -left), page.width - right),
    dy: Math.min(Math.max(want.dy, -top), page.height - bottom),
  };
}

/** Whether an arrangement moved this rectangle by more than rounding. */
export function differs(a: Quad, b: Quad): boolean {
  const far = (x: number, y: number): boolean => Math.abs(x - y) > 0.01;
  return far(a.left, b.left) || far(a.top, b.top) || far(a.right, b.right) || far(a.bottom, b.bottom);
}

/**
 * The marks a reader has picked to arrange, in the order they were picked.
 *
 * All on one page: an arrangement is about rectangles a reader sees side by
 * side, and a pick on another page starts again there.
 */
export class Picked {
  private ids: number[] = [];
  private page: number | null = null;

  /** The picked marks, the first picked first. */
  list(): readonly number[] {
    return this.ids;
  }

  get count(): number {
    return this.ids.length;
  }

  has(id: number): boolean {
    return this.ids.includes(id);
  }

  /** Picks this mark and no other. */
  only(id: number, page: number): void {
    this.ids = [id];
    this.page = page;
  }

  /** Adds a mark, or takes it out if it is picked already. */
  toggle(id: number, page: number): void {
    if (this.page !== page) {
      this.only(id, page);
      return;
    }
    this.ids = this.has(id) ? this.ids.filter((other) => other !== id) : [...this.ids, id];
  }

  clear(): void {
    this.ids = [];
    this.page = null;
  }

  /** Drops every mark `live` no longer answers for: one removed or undone. */
  keep(live: (id: number) => boolean): void {
    this.ids = this.ids.filter(live);
  }
}

/** What a reader is told when they have picked several marks. */
export function pickedNotice(count: number): string {
  return `${count} picked. Arrange aligns, spaces and sizes them; the first one picked is the one the others follow.`;
}
