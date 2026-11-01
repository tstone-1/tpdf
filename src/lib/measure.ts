/**
 * Measuring on a page: lines the reader drags, and the scale one of them sets.
 *
 * On screen only, and that is a decision: nothing here reaches the edit model, so
 * a measurement cannot make a document dirty and cannot invalidate a digital
 * signature. The tab keeps them while it is not on screen, since a tab switch
 * rebuilds the viewer; closing, saving or reloading drops them, because each of
 * those opens the file afresh and its page ids start again. Saving one as a `/Line` annotation
 * with a `/Measure` dictionary is a later step, and this module's shape does not
 * stand in its way.
 *
 * **A line is held in the file's display space and keyed by a page's id**, as a
 * mark is. The id survives a deletion that renumbers every later slot, and the
 * space survives a turn or a crop, so a line stays on the paper it was drawn on.
 * Length is the same in that space as in the laid-out one, because a turn and a
 * crop move a line without stretching it, and it has no zoom in it at all.
 *
 * **The scale is per page**, since the sheets of one drawing set are often at
 * different scales, and a scale carried silently onto another page would print a
 * wrong number that looks exactly like a right one.
 */
import { catalog, UI_LOCALE, type UiLocale } from "./i18n";
import type { PageId } from "./pages";

const TEXT = catalog({
  "en-US": {
    scaleMark: (length: string) => `${length} (scale)`,
  },
  "de-DE": {
    scaleMark: (length: string) => `${length} (Maßstab)`,
  },
});

/** A point on a page, in points. */
export interface MeasurePoint {
  x: number;
  y: number;
}

/** One line a reader dragged, its ends in the file's display space. */
export interface MeasureLine {
  id: number;
  page: PageId;
  from: MeasurePoint;
  to: MeasurePoint;
}

/** A length as the reader typed it. */
export interface KnownLength {
  value: number;
  unit: string;
  /** Digits typed after the decimal mark, which is how precise later labels are. */
  decimals: number;
}

/** What one page's scale says. */
export interface PageScale {
  unitsPerPoint: number;
  unit: string;
  decimals: number;
  /** The line the reader named. */
  line: number;
}

/** The most digits a typed length keeps; `Intl` refuses more than 20 and nobody means 7. */
const MOST_DECIMALS = 6;

/**
 * A number with an optional decimal mark, then a unit of letters or a foot or
 * inch sign.
 *
 * The unit is only ever shown, so any word will do: `Fuß` and `km` are as good as
 * `cm`, and a list of units would refuse the one a surveyor uses. A unit is still
 * required, because "195" on a line would look like a measurement of nothing.
 *
 * One decimal mark, either kind, for the German comma. A thousands separator is
 * not accepted: `1.234,5` is refused, and the palette's preview shows how a single
 * mark was read before Enter is pressed.
 */
const LENGTH = /^([0-9]+(?:[.,][0-9]+)?|[.,][0-9]+)\s*(\p{L}[\p{L}\p{N}]{0,7}|["'′″])$/u;

/** What the reader typed as a real length, or `null` when it is not one. */
export function parseKnownLength(raw: string): KnownLength | null {
  const found = LENGTH.exec(raw.trim());
  const digits = found?.[1];
  const unit = found?.[2];
  if (digits === undefined || unit === undefined) return null;
  const plain = digits.replace(",", ".");
  const value = Number(plain);
  if (!Number.isFinite(value) || value <= 0) return null;
  const decimals = Math.min(plain.length - plain.indexOf(".") - 1, MOST_DECIMALS);
  return { value, unit, decimals: plain.includes(".") ? decimals : 0 };
}

/** A length in the window's language: `82.4 cm`, or `82,4 cm` in German. */
export function formatLength(
  value: number,
  unit: string,
  decimals = 1,
  locale: UiLocale = UI_LOCALE,
): string {
  const digits = new Intl.NumberFormat(locale, {
    maximumFractionDigits: decimals,
  }).format(value);
  return /^["'′″]$/.test(unit) ? `${digits}${unit}` : `${digits} ${unit}`;
}

/**
 * The end of a line while Shift is held: exactly along whichever axis it is
 * nearer to. Without Shift, the point the pointer is at.
 *
 * In the laid-out space, so "horizontal" is what the reader sees as horizontal on
 * a turned page, which is the only sense they can mean.
 */
export function straighten(from: MeasurePoint, to: MeasurePoint, held: boolean): MeasurePoint {
  if (!held) return to;
  return Math.abs(to.x - from.x) >= Math.abs(to.y - from.y)
    ? { x: to.x, y: from.y }
    : { x: from.x, y: to.y };
}

/** A point kept on the page: a line belongs to the page its drag started on. */
export function keepOnPage(
  at: MeasurePoint,
  page: { width: number; height: number },
): MeasurePoint {
  return {
    x: Math.min(Math.max(at.x, 0), page.width),
    y: Math.min(Math.max(at.y, 0), page.height),
  };
}

/** The distance between two points, in whatever unit they are in. */
export function distance(from: MeasurePoint, to: MeasurePoint): number {
  return Math.hypot(to.x - from.x, to.y - from.y);
}

/**
 * The two short strokes across a line's ends, `size` long either side.
 *
 * They are what makes the ends readable: a thin line stopping on a dimension
 * line in a plan is otherwise impossible to tell apart from it. Empty for a line
 * of no length, which has no direction to be across.
 */
export function endTicks(
  from: MeasurePoint,
  to: MeasurePoint,
  size: number,
): [MeasurePoint, MeasurePoint][] {
  const length = distance(from, to);
  if (length === 0) return [];
  const across = { x: (-(to.y - from.y) / length) * size, y: ((to.x - from.x) / length) * size };
  return [from, to].map((end) => [
    { x: end.x - across.x, y: end.y - across.y },
    { x: end.x + across.x, y: end.y + across.y },
  ]);
}

/**
 * Every line and scale of one open document.
 *
 * A class with its state in one place because `viewer.ts` is the only owner and
 * has enough already; everything here is testable without a page, a canvas or a
 * pointer.
 */
export class Measurements {
  private lines: MeasureLine[] = [];
  private readonly scales = new Map<PageId, PageScale>();
  private nextId = 1;

  /** How many lines there are. */
  get count(): number {
    return this.lines.length;
  }

  /** Every line, oldest first. */
  get all(): readonly MeasureLine[] {
    return this.lines;
  }

  add(page: PageId, from: MeasurePoint, to: MeasurePoint): MeasureLine {
    const line = { id: this.nextId++, page, from, to };
    this.lines.push(line);
    return line;
  }

  /**
   * The newest line on a page still in the document, which is the one a scale
   * refers to: the line the reader has just drawn. A line on a deleted page is
   * passed over, as nothing on screen could say which line was meant.
   */
  latest(inDocument: (page: PageId) => boolean): MeasureLine | null {
    for (let at = this.lines.length - 1; at >= 0; at--) {
      const line = this.lines[at];
      if (line && inDocument(line.page)) return line;
    }
    return null;
  }

  /**
   * Says that `line` is really `known` long, for every line on its page.
   *
   * Refuses a line of no length, which would divide by zero into a label reading
   * `Infinity`. The viewer does not keep one; this is the last place it could
   * become a number on screen.
   */
  setScale(line: MeasureLine, known: KnownLength): boolean {
    const points = distance(line.from, line.to);
    if (points === 0) return false;
    this.scales.set(line.page, {
      unitsPerPoint: known.value / points,
      unit: known.unit,
      decimals: Math.max(known.decimals, 1),
      line: line.id,
    });
    return true;
  }

  /** Whether a page has a scale. */
  scaled(page: PageId): boolean {
    return this.scales.has(page);
  }

  /**
   * What a length on a page reads: in the page's unit once it has a scale, and in
   * points until then. Worded in the window's language.
   */
  lengthLabel(page: PageId, points: number, locale: UiLocale = UI_LOCALE): string {
    const scale = this.scales.get(page);
    return scale
      ? formatLength(points * scale.unitsPerPoint, scale.unit, scale.decimals, locale)
      : formatLength(points, "pt", 1, locale);
  }

  /** What a line reads, with the reference named as the scale. */
  label(line: MeasureLine, locale: UiLocale = UI_LOCALE): string {
    const length = this.lengthLabel(line.page, distance(line.from, line.to), locale);
    return this.scales.get(line.page)?.line === line.id ? TEXT(locale).scaleMark(length) : length;
  }

  /** Drops every line and scale: a scale with nothing on screen to show it is a trap. */
  clear(): void {
    this.lines = [];
    this.scales.clear();
  }
}
