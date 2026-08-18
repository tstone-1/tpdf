/**
 * Which page of the file is in which slot of the viewer.
 *
 * **Two vocabularies, and everything here is the translation between them.** The
 * viewer addresses pages by *slot*: slot 0 is the first page on screen, and every
 * array it holds --- sizes, boxes, tile epochs, thumbnail rows --- is indexed that
 * way. The backend addresses them by *source*: the page of the file the content
 * comes from, which is what a tile request, `page_text` and `search_page` all
 * mean by "page", and what the outline, the links and the comments name in their
 * answers.
 *
 * The two were the same number until a page could be deleted, which is why this
 * module did not exist before: a translation that is the identity function cannot
 * be told from a broken one by any test, and `docs/TRAPS.md` has that under *"a
 * property that holds by construction cannot test the thing it resembles"*.
 *
 * **This holds no rules.** It is built from the `pages` of an `EditState`, which
 * the model in Rust produced, and it answers questions about that answer. When
 * the model replies again, the map is replaced rather than updated --- the same
 * posture `edits.ts` takes, and for the same reason: a map that computed its own
 * next state would be a second implementation of the journal.
 */

import type { Comment } from "./comments";
import type { Link } from "./links";
import type { OutlineItem, Target } from "./outline";

/**
 * One mark a reader made, as the backend reports it.
 *
 * Mirrors `edits::MarkView`, and lives here rather than in `edits.ts` for the
 * reason the note above {@link PageView} gives: the viewer paints these and must
 * not import the module that talks to Tauri.
 */
export interface MarkView {
  id: number;
  /** The page it is on, by {@link PageView.id} --- never a slot. */
  page: number;
  /**
   * Four numbers per rectangle --- `left, top, right, bottom` --- in the page's
   * **display** space: points from the displayed page's top-left corner, after
   * the page's own `/Rotate` and before any turn the reader or an edit added.
   * The overlay applies those when it paints.
   */
  quads: number[];
  color: [number, number, number];
  /**
   * What the reader typed, which may be empty.
   *
   * **Attacker-controlled once a saved file is reopened**, so it is treated as
   * `annots.rs` treats a comment body: it reaches the DOM as text, and nothing
   * here may carry a URL. See `docs/THREAT-MODEL.md` T8.
   */
  note: string;
}

/**
 * One page of the working document, as the backend reports it.
 *
 * Mirrors `edits::PageView`. It lives here rather than in `edits.ts` so that the
 * modules that only need the *shape* --- the scroller, the thumbnails --- do not
 * have to import the module that talks to Tauri, which cannot be loaded outside
 * a webview.
 */
export interface PageView {
  /**
   * The model's identity for this page, sent back verbatim in a command.
   *
   * A `u64` in Rust and a JavaScript number here, which is exact for every id
   * the model can currently issue --- they are allocated from 1 upwards, one per
   * baseline page. **An allocator that ever issues an id past 2^53 breaks this
   * silently**, rotating whichever page the rounded value happens to name, so
   * that is a constraint on the allocator `docmodel.rs` says has yet to be
   * written rather than a property of this line.
   */
  id: number;
  /** Which page of the file supplies the content. Zero-based. */
  source: number;
  /** Quarter turns clockwise an edit has applied, on top of the page's own. */
  turns: number;
}

/**
 * The order the reader is looking at, and both directions of the translation.
 *
 * Immutable. Built once per state reply, and small: one entry per live page.
 */
export class PageMap {
  private readonly views: readonly PageView[];
  /**
   * Source page to slot.
   *
   * A page of the file appears at most once in the working document today,
   * because nothing duplicates a page --- `docmodel.rs` says so and says what
   * would have to be proved first. Were one to appear twice, this would answer
   * with the first of them, which is why {@link slotOf} says "the first slot".
   */
  private readonly bySource: Map<number, number>;

  constructor(views: readonly PageView[]) {
    this.views = views;
    this.bySource = new Map();
    for (let slot = 0; slot < views.length; slot++) {
      const source = views[slot]?.source;
      if (source === undefined || this.bySource.has(source)) continue;
      this.bySource.set(source, slot);
    }
  }

  /** How many pages the reader sees. */
  get length(): number {
    return this.views.length;
  }

  /** The pages, in reading order. */
  get pages(): readonly PageView[] {
    return this.views;
  }

  /** The page in a slot, or `undefined` if there is none. */
  at(slot: number): PageView | undefined {
    return this.views[slot];
  }

  /**
   * Which page of the file a slot draws, or `undefined` for a slot that is not
   * in the document.
   *
   * Deliberately not falling back to the slot. That fallback is the identity
   * function, it is right for every document nobody has edited, and it is wrong
   * in exactly the case this class exists for --- so a caller that cannot handle
   * a missing slot must not ask for a tile rather than ask for the wrong one.
   */
  sourceOf(slot: number): number | undefined {
    return this.views[slot]?.source;
  }

  /**
   * The first slot showing a page of the file, or `undefined` if it was deleted.
   *
   * The direction everything arriving from the backend needs: a link's rectangle,
   * a comment's page, an outline destination and a search result all name a page
   * of the file, and the viewer can only draw or scroll to a slot.
   */
  slotOf(source: number): number | undefined {
    return this.bySource.get(source);
  }

  /**
   * The first slot showing the page with this identity, or `undefined`.
   *
   * The direction a *mark* needs: the model keys a highlight by the page's id,
   * because a slot is not a name for a page, and the overlay draws in slots.
   * {@link slotOf} answers the same question for a baseline page number, which
   * is what links and comments carry --- two different keys because they come
   * from two different places, and conflating them would silently work until a
   * page moved.
   */
  slotOfId(id: number): number | undefined {
    for (let slot = 0; slot < this.views.length; slot++) {
      if (this.views[slot]?.id === id) return slot;
    }
    return undefined;
  }

  /** Quarter turns an edit has applied to the page in a slot, 0 to 3. */
  turnsOf(slot: number): number {
    return this.views[slot]?.turns ?? 0;
  }

  /** The model's identity for the page in a slot, for sending a command back. */
  idOf(slot: number): number | undefined {
    return this.views[slot]?.id;
  }

  /**
   * The page of the file each slot draws, in slot order. For the tests.
   *
   * Nothing in the application asks for the whole list --- every consumer asks
   * about one slot --- and it is here because an assertion reading
   * `[0, 2, 3]` says what a deletion did, where three separate `sourceOf`
   * calls say it three times.
   */
  sources(): number[] {
    return this.views.map((page) => page.source);
  }

  /**
   * Whether two maps put the same pages in the same slots.
   *
   * Identity and order only --- **not** the turns, which is the distinction the
   * viewer branches on: a turn is a change to one page that the layout can
   * absorb in place, and a change of order moves every slot after it and
   * invalidates everything keyed by one.
   */
  sameOrder(other: PageMap): boolean {
    if (this.views.length !== other.views.length) return false;
    return this.views.every((page, slot) => page.id === other.views[slot]?.id);
  }

  /**
   * Where the page that was in `slot` of `before` has gone, or `undefined`.
   *
   * By identity rather than by source, so it stays right when a page can be
   * duplicated. Used to carry per-slot state --- a learned page size, a tile
   * epoch --- across an edit rather than throwing it away.
   */
  slotFrom(before: PageMap, slot: number): number | undefined {
    const id = before.idOf(slot);
    if (id === undefined) return undefined;
    const found = this.views.findIndex((page) => page.id === id);
    return found === -1 ? undefined : found;
  }
}

/** The map of a document with nothing open, which has no pages. */
export const NO_PAGES = new PageMap([]);

/**
 * The links of the working document: rectangles in slots, targets in slots.
 *
 * A link is a rectangle **on** a page and a destination **to** a page, and both
 * are pages of the file as `links.rs` reports them. A link whose own page is
 * gone goes with it --- there is nowhere to draw it. A link whose *destination*
 * is gone stays, and becomes `broken`, which already means "points at a page
 * this document does not have" and is exactly what a reader has done by deleting
 * it.
 */
export function linksIn(items: readonly Link[], pages: PageMap): Link[] {
  const mapped: Link[] = [];
  for (const link of items) {
    const slot = pages.slotOf(link.page);
    if (slot === undefined) continue;
    mapped.push({ ...link, page: slot, target: targetIn(link.target, pages) });
  }
  return mapped;
}

/** The comments of the working document, those on deleted pages left out. */
export function commentsIn(
  items: readonly Comment[],
  pages: PageMap,
): Comment[] {
  const mapped: Comment[] = [];
  for (const comment of items) {
    const slot = pages.slotOf(comment.page);
    if (slot === undefined) continue;
    mapped.push({ ...comment, page: slot });
  }
  return mapped;
}

/**
 * The outline with every destination in slots, and the dead ones marked broken.
 *
 * The tree is kept whole: an entry whose page has gone is still a heading with
 * children under it, and dropping it would take a chapter's subsections out of
 * the table of contents because the chapter's title page was deleted.
 */
export function outlineIn(
  items: readonly OutlineItem[],
  pages: PageMap,
): OutlineItem[] {
  return items.map((item) => ({
    ...item,
    target: targetIn(item.target, pages),
    children: outlineIn(item.children, pages),
  }));
}

/**
 * One destination in slots, or `broken` if its page is not in the document.
 *
 * `broken` rather than a variant of its own. Its wording --- "points at a page
 * this document does not have" --- is already the true sentence for a page the
 * reader deleted, and a new variant would have to be mirrored in `outline.rs`'s
 * `Target`, where nothing can produce it.
 */
function targetIn(target: Target, pages: PageMap): Target {
  if (target.kind !== "page") return target;
  const slot = pages.slotOf(target.page);
  if (slot === undefined) return { kind: "broken" };
  return { ...target, page: slot };
}

/**
 * A map for a document nobody has edited: `count` pages, in order, unturned.
 *
 * The state the backend would answer with, built locally so that the viewer has
 * a map from the first frame rather than from the first reply. The ids match the
 * model's own allocation, which numbers the baseline pages from 1 --- stated in
 * `docmodel::Working::baseline` and mirrored here, because a viewer whose ids
 * were made up would send commands naming pages that do not exist.
 */
export function unedited(count: number): PageMap {
  return new PageMap(
    Array.from({ length: Math.max(0, count) }, (_unused, slot) => ({
      id: slot + 1,
      source: slot,
      turns: 0,
    })),
  );
}
