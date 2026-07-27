/**
 * The document outline, as a list of rows a sidebar can draw.
 *
 * The tree arrives from `src-tauri/src/outline.rs` already bounded, already
 * cycle-free and with every refused action labelled --- everything hostile
 * about a PDF outline has been dealt with by the time it reaches here. What is
 * left is genuinely a UI problem: which rows are visible given what is
 * collapsed, and which row the reader is currently inside.
 *
 * Both are pure functions of the tree and a small amount of state, which is why
 * they are here rather than inside the sidebar. `outline.test.ts` exercises
 * them without a DOM.
 */

/** Where an entry points, mirroring `outline.rs`'s `Target`. */
export type Target =
  | { kind: "page"; page: number; top_pt: number | null }
  | { kind: "broken" }
  | { kind: "refused"; action: string }
  | { kind: "none" };

/** One entry, and everything under it. */
export interface OutlineItem {
  title: string;
  open: boolean;
  target: Target;
  children: OutlineItem[];
}

/** What the walk's bounds cut off. */
export interface OutlineLimits {
  cycles: number;
  too_deep: number;
  over_budget: boolean;
  titles_clipped: number;
}

/** A document's outline, as `document_outline` returns it. */
export interface Outline {
  items: OutlineItem[];
  total: number;
  limits: OutlineLimits;
  walk_ms: number;
}

/** One drawable row. */
export interface Row {
  /**
   * Position in the tree, as dot-separated child indices --- `"2.0.1"`.
   *
   * Deliberately not the title: outline entries repeat ("Introduction" under
   * every chapter is normal), and a collapse keyed on a title would fold every
   * namesake at once.
   */
  id: string;
  title: string;
  depth: number;
  target: Target;
  /** Whether this row has children at all, so a twisty is drawn or not. */
  hasChildren: boolean;
  /** Whether those children are currently shown. */
  expanded: boolean;
}

/**
 * Air left above a heading when jumping to it, in points.
 *
 * Lives here rather than in `viewer.ts` because it is one half of a pair: this
 * and {@link REACHED_TOLERANCE_PT} are only correct *relative to each other*,
 * and a constant that has to stay below another one belongs next to it.
 *
 * Points rather than CSS pixels so the gap is a fixed distance in the
 * document's own units --- in pixels it would be 32 pt of the page at the
 * lowest zoom stop and 4 pt at the highest, and no single tolerance could
 * bound it.
 */
export const DESTINATION_MARGIN_PT = 6;

/**
 * How far below the viewport's top edge an entry still counts as reached, in
 * points.
 *
 * Not a fudge factor. Jumping to a destination deliberately leaves
 * {@link DESTINATION_MARGIN_PT} of air above the heading, so on arrival the
 * heading is *below* the top edge by exactly that much --- and without this,
 * clicking an outline entry highlights the entry **before** it, which is the
 * first thing anyone would notice. Caught by the viewer check, not by
 * reasoning: `"" -> "0", wanted "1"`.
 *
 * **It must strictly exceed the margin**, which is asserted in `outline.test.ts`
 * rather than left as a comment: raising the margin past it silently brings the
 * bug back, and nothing about either line would look wrong.
 */
export const REACHED_TOLERANCE_PT = 8;

/** Whether an entry can be navigated to. */
export function isNavigable(target: Target): target is {
  kind: "page";
  page: number;
  top_pt: number | null;
} {
  return target.kind === "page";
}

/**
 * Why an entry does nothing, in words a reader can act on.
 *
 * A row that silently ignores a click is indistinguishable from a broken
 * viewer, and the three reasons are genuinely different: one is a heading, one
 * is a damaged file, and one is tpdf declining to open something.
 */
export function reasonFor(target: Target): string {
  switch (target.kind) {
    case "page":
      return "";
    case "broken":
      return "points at a page this document does not have";
    case "refused":
      return refusalWording(target.action);
    case "none":
      return "no destination";
  }
}

function refusalWording(action: string): string {
  switch (action) {
    case "launch":
      return "opens a program — not followed";
    case "uri":
      return "opens a web link — not followed";
    case "remote":
      return "opens another document — not followed";
    case "embedded":
      return "opens an embedded file — not followed";
    default:
      return "unsupported action — not followed";
  }
}

/** What `flatten` consults to decide whether a row's children are shown. */
export interface ExpansionState {
  isExpanded(id: string, open: boolean): boolean;
}

/**
 * The expansion state of a tree.
 *
 * Stored as the *exceptions* to each entry's own `open` flag rather than as the
 * set of expanded ids, so a document whose producer marked everything open does
 * not need ten thousand entries in a set before anything is drawn.
 */
export class Expansion implements ExpansionState {
  private readonly toggled = new Set<string>();

  /** Whether the row at `id` shows its children. */
  isExpanded(id: string, open: boolean): boolean {
    return this.toggled.has(id) ? !open : open;
  }

  toggle(id: string): void {
    if (this.toggled.has(id)) this.toggled.delete(id);
    else this.toggled.add(id);
  }

  /** Forces a row open or closed. Returns whether anything changed. */
  set(id: string, open: boolean, expanded: boolean): boolean {
    if (this.isExpanded(id, open) === expanded) return false;
    this.toggle(id);
    return true;
  }

  /** Expands every row on the path to `id`, so a deep row can be revealed. */
  reveal(id: string, openOf: (id: string) => boolean): void {
    const parts = id.split(".");
    // Every proper prefix, i.e. the ancestors --- not `id` itself, which does
    // not need to be expanded to be visible.
    for (let count = 1; count < parts.length; count++) {
      const ancestor = parts.slice(0, count).join(".");
      this.set(ancestor, openOf(ancestor), true);
    }
  }
}

/** Ignores collapse entirely. `allRows` is `flatten` through this. */
export const EXPAND_ALL: ExpansionState = { isExpanded: () => true };

/** Flattens the tree into the rows that are currently visible. */
export function flatten(items: OutlineItem[], expansion: ExpansionState): Row[] {
  const rows: Row[] = [];

  const walk = (nodes: OutlineItem[], prefix: string, depth: number): void => {
    nodes.forEach((node, index) => {
      const id = prefix === "" ? String(index) : `${prefix}.${index}`;
      const hasChildren = node.children.length > 0;
      const expanded = hasChildren && expansion.isExpanded(id, node.open);
      rows.push({
        id,
        title: node.title,
        depth,
        target: node.target,
        hasChildren,
        expanded,
      });
      if (expanded) walk(node.children, id, depth + 1);
    });
  };

  walk(items, "", 0);
  return rows;
}

/** Looks up an entry's own `open` flag by row id. */
export function openFlagOf(items: OutlineItem[], id: string): boolean {
  let nodes = items;
  let found: OutlineItem | undefined;
  for (const part of id.split(".")) {
    found = nodes[Number(part)];
    if (!found) return true;
    nodes = found.children;
  }
  return found?.open ?? true;
}

/**
 * Every row in the tree, collapse ignored.
 *
 * Which row the reader is inside must not depend on what is folded away --- a
 * collapsed chapter is still the chapter they are in, and highlighting its
 * parent instead would make the highlight jump around as rows are folded.
 */
export function allRows(items: OutlineItem[]): Row[] {
  return flatten(items, EXPAND_ALL);
}

/**
 * The entry the reader is currently inside, by id, or `null`.
 *
 * "The last navigable row at or before where we are" is the obvious rule and it
 * is wrong on any outline that is not monotonic --- and nothing requires one to
 * be. An entry that jumps backwards (an index, a foreword listed last) would
 * make every row after it the answer for the whole document.
 *
 * So the rule is *the row whose destination is furthest into the document among
 * those at or before the current position*, with document order breaking ties.
 * On a well-formed outline that is the same answer; on a scrambled one it is
 * the only defensible one.
 *
 * `top` is measured in points from the top of `page`, the same units the
 * destinations carry. An entry with no coordinate is treated as the top of its
 * page, which is what a `/Fit` destination means.
 */
export function currentId(rows: Row[], page: number, top: number): string | null {
  let best: { id: string; page: number; top: number } | null = null;

  for (const row of rows) {
    if (!isNavigable(row.target)) continue;
    const rowTop = row.target.top_pt ?? 0;
    if (row.target.page > page) continue;
    if (row.target.page === page && rowTop > top + REACHED_TOLERANCE_PT) continue;

    if (
      best === null ||
      row.target.page > best.page ||
      (row.target.page === best.page && rowTop >= best.top)
    ) {
      best = { id: row.id, page: row.target.page, top: rowTop };
    }
  }

  return best?.id ?? null;
}
