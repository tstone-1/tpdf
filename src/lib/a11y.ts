/**
 * What a screen reader finds when it reaches the document.
 *
 * `docs/PLAN.md` §8 states this as an architectural constraint rather than a
 * later pass, and §7's virtual-scrolling section repeats it: *"Accessibility
 * constrains this design and must be settled before it is built, not after."*
 * The reason is specific. A canvas-rendered, virtualized page list is
 * inaccessible **by default** --- there is no DOM text to read at all, and a
 * scroller that recycles its page containers destroys the reading cursor of
 * anyone using one. Neither is a styling problem that can be fixed afterwards.
 *
 * So this maintains a parallel DOM of the visible pages' text, and it does two
 * things that are easy to get wrong and impossible to notice without a screen
 * reader:
 *
 * **Elements are keyed by page and never recycled.** A page that stays visible
 * keeps the *same* element across every frame, so a reading cursor or the
 * keyboard focus inside it survives scrolling. Reusing a container for a
 * different page --- the obvious optimisation, and what a windowed list normally
 * does --- would move the cursor to different text under the user, silently.
 *
 * **Text is split into lines.** One 2,700-character blob per page is technically
 * present and practically unusable: moving by line is most of how a document is
 * read. The split reuses the geometry the selection already computes.
 *
 * ## What this is not
 *
 * It is *not* the pdf.js approach of positioning invisible text spans over the
 * glyphs, which additionally gives native selection and hit-testing. tpdf does
 * selection on the canvas with real character boxes, so the second half of that
 * is already covered and paid for; adding a second, positioned copy of the text
 * would give the browser two selectable representations of one page.
 *
 * ## Whose order it is read in
 *
 * `readingLines` answers that, and since 2026-08-01 it prefers the document's own
 * tags over the geometry where a page carries them --- so a margin note tagged
 * after the body is read after the body, however far up the page it sits. Nothing
 * here changed for it: the order arrives through the same call, which is why the
 * runs were put on `PageText` rather than fetched separately.
 *
 * Lines remain the granularity. A tagged run is a *paragraph*, and handing a
 * screen reader one element per paragraph is arguably closer to what the document
 * says --- it is an open question, and it is a change to this file alone.
 *
 * And it is **not verified against a screen reader**. What the check asserts is
 * that the text is present, correct, in reading order, and stable across
 * scrolling. Whether VoiceOver announces it *well* is not measured here, and no
 * claim is made about it.
 */

import { linkRunsIn, onPage as linksOnPage, type Link } from "./links";
import { readingBlocks, textOfRanges, type ReadingBlock } from "./reading";
import { type PageText } from "./text";

/**
 * The DOM element a tagged block is announced as.
 *
 * Headings are the whole of the win here, and they are not cosmetic: "jump to
 * the next heading" and "list the headings" are how a screen-reader user skims a
 * document, and neither works on a page of paragraphs however correctly ordered.
 * A PDF states its levels, so they are used rather than guessed --- `H1` through
 * `H6` map across, and a bare `H` becomes `h2`, since the document has said
 * "heading" and not which level.
 *
 * Everything else becomes a paragraph, and two cases are deliberately *not*
 * given their obvious element:
 *
 * - **Table cells.** `TD` outside a `<table>` is not a table cell, it is an
 *   element screen readers ignore or mis-announce, so emitting one would be worse
 *   than a paragraph. Building a real table needs to know which cells share a
 *   row, and {@link TaggedRun.path} carries element *types* --- two different
 *   `/TR`s have the identical path --- so the information is not there yet. It
 *   needs element identity from `structure.rs`, which is a backend change, and
 *   pretending otherwise would produce a table with one row per cell.
 * - **Figures.** The useful thing about a `/Figure` is its `/Alt` text, which is
 *   not read yet. A `<figure>` with the figure's own characters in it says
 *   nothing a paragraph does not.
 *
 * Exported for its tests. There is no DOM in the unit-test environment, so this
 * is the part of the layer a test can reach at all --- the elements themselves are
 * asserted by `viewercheck.ts` against a real webview, which is the stronger
 * evidence of the two and cannot cover a mapping table exhaustively.
 */
export function elementFor(tag: string | null): string {
  if (tag === null) return "p";
  const heading = /^H([1-6])$/.exec(tag);
  if (heading) return `h${heading[1]}`;
  return tag === "H" ? "h2" : "p";
}

/**
 * Hidden visually, present in the accessibility tree.
 *
 * `display:none` and `visibility:hidden` both remove an element from that tree,
 * which would make this file do nothing while looking correct --- so the
 * clipping form is load-bearing rather than stylistic.
 */
const SR_ONLY =
  "position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;" +
  "clip:rect(0 0 0 0);clip-path:inset(50%);white-space:normal;border:0;";

/** The document's text, as much of it as is on screen. */
export class AccessibleText {
  private readonly host: HTMLElement;
  private readonly announcer: HTMLElement;
  private readonly pages = new Map<number, HTMLElement>();
  /**
   * What each built page was built from, so it can be built again.
   *
   * Kept only for the pages that are present --- dropped in `sync` alongside the
   * element --- so this is bounded by the viewport rather than by the document.
   */
  private readonly built = new Map<number, { content: PageText; unreadable: boolean }>();
  /**
   * How many pages the reader sees.
   *
   * Announced as "Page 3 of 40" and written into every page's `aria-setsize`,
   * so it is not `readonly`: deleting a page changes it while the document is
   * open, and a screen reader told the old total would be counting pages that
   * are not there.
   */
  private pageCount: number;
  /**
   * The document's links, so a cross-reference is announced as one.
   *
   * Held rather than passed to {@link sync} because they arrive on their own
   * chain, after first paint: a page built before they land is rebuilt when they
   * do, which is what {@link setLinks} does.
   */
  private links: readonly Link[] = [];
  private announced = "";

  constructor(root: HTMLElement, pageCount: number) {
    this.pageCount = pageCount;

    this.host = document.createElement("div");
    this.host.setAttribute("role", "document");
    this.host.setAttribute("aria-label", "Document text");
    this.host.style.cssText = SR_ONLY;
    root.appendChild(this.host);

    // Separate from the text, and polite: page changes are context, not an
    // interruption, and an assertive region would talk over the reading of the
    // page it is announcing.
    this.announcer = document.createElement("div");
    this.announcer.setAttribute("role", "status");
    this.announcer.setAttribute("aria-live", "polite");
    this.announcer.style.cssText = SR_ONLY;
    root.appendChild(this.announcer);
  }

  destroy(): void {
    this.host.remove();
    this.announcer.remove();
  }

  /** The element carrying page `page`'s text, if it is currently present. */
  elementFor(page: number): HTMLElement | null {
    return this.pages.get(page) ?? null;
  }

  /** Pages currently in the accessibility tree, in document order. */
  get present(): number[] {
    return [...this.pages.keys()].sort((a, b) => a - b);
  }

  /**
   * Brings the tree in line with what is on screen.
   *
   * Cheap to call every frame: a page already present is left completely alone,
   * which is the property the whole file exists for, and a page whose text has
   * not arrived is skipped rather than added empty.
   */
  sync(
    visible: readonly number[],
    text: (page: number) => PageText | null,
    unreadable: (page: number) => boolean = () => false,
  ): void {
    const wanted = new Set(visible);

    for (const [page, element] of this.pages) {
      if (wanted.has(page)) continue;
      element.remove();
      this.pages.delete(page);
      this.built.delete(page);
    }

    for (const page of [...wanted].sort((a, b) => a - b)) {
      if (this.pages.has(page)) continue;
      const content = text(page);
      if (!content) continue;
      const cannotRead = unreadable(page);
      const element = this.build(page, content, cannotRead);
      this.pages.set(page, element);
      this.built.set(page, { content, unreadable: cannotRead });
      this.insert(page, element);
    }
  }

  /**
   * Takes a new page count, dropping every page that is present.
   *
   * The count is baked into each page's `aria-setsize` and into the
   * announcement, so a stale one is read aloud. The elements go rather than
   * being patched: `sync` rebuilds a page it does not hold, and after a deletion
   * the page in a given slot is a different page.
   */
  setPageCount(pageCount: number): void {
    if (pageCount === this.pageCount) return;
    this.pageCount = pageCount;
    for (const element of this.pages.values()) element.remove();
    this.pages.clear();
    this.built.clear();
    this.announced = "";
  }

  /**
   * Replaces the links the tree marks up, rebuilding the pages already built.
   *
   * The rebuild is the point and it is not free: a page element is *never*
   * recycled while it stays visible, precisely so a reading cursor inside it
   * survives scrolling, and this throws that away for the pages on screen. It is
   * still right --- announcing a table of contents as prose for as long as the
   * reader stays on that page is the defect being fixed --- and it happens once
   * per document, just after first paint, rather than on any path a reader is
   * moving through.
   */
  setLinks(items: readonly Link[]): void {
    this.links = items;
    for (const [page, element] of [...this.pages]) {
      const from = this.built.get(page);
      if (!from) continue;
      // Removed and re-inserted through `insert` rather than swapped in place,
      // so a rebuilt page lands by the same rule a new one does. Two ways to put
      // a page into the tree is two orderings to keep agreeing, and the order
      // *is* the reading order for anyone using this.
      element.remove();
      this.pages.delete(page);
      const rebuilt = this.build(page, from.content, from.unreadable);
      this.pages.set(page, rebuilt);
      this.insert(page, rebuilt);
    }
  }

  /** Announces the page being read, when it changes. */
  announce(page: number): void {
    const message = `Page ${page + 1} of ${this.pageCount}`;
    if (message === this.announced) return;
    this.announced = message;
    this.announcer.textContent = message;
  }

  /**
   * Inserts a page's element in document order.
   *
   * Order in the DOM is reading order for a screen reader, and pages arrive in
   * whatever order their text does --- so appending would put page 4 before
   * page 3 whenever 3's extraction was the slower of the two.
   */
  private insert(page: number, element: HTMLElement): void {
    const after = this.present.find((other) => other > page);
    const before = after === undefined ? null : (this.pages.get(after) ?? null);
    this.host.insertBefore(element, before);
  }

  private build(page: number, content: PageText, unreadable = false): HTMLElement {
    const article = document.createElement("article");
    article.setAttribute("aria-label", `Page ${page + 1} of ${this.pageCount}`);
    // Focusable programmatically but not in the tab order: Tab through a
    // 775-page document would be a trap, and the page still needs to be a
    // target for "go to this page".
    article.tabIndex = -1;
    article.dataset.page = String(page);

    // A page whose fonts declare no character mapping has text of the right
    // length that means nothing --- PDFium reads glyph ids as character codes, so
    // `Encoding probe ABC` comes back as `(QFRGLQJ\x03SUREH\x03$%&`. Reading that
    // aloud is the worst of the three symptoms this fixes: a sighted reader sees
    // the search find nothing and can guess something is wrong, and a
    // screen-reader user is simply read nonsense with nothing to say it is not
    // the page. So the characters are withheld and the reason is given instead.
    //
    // Withheld rather than announced *alongside*: an element containing both
    // would be read out in full, which is the outcome being avoided.
    if (unreadable) {
      const note = document.createElement("p");
      note.textContent =
        "This page's text cannot be read. The document does not say what its characters mean.";
      article.appendChild(note);
      return article;
    }

    // `readingBlocks` rather than `linesOf`: a screen reader is handed the page
    // in the order it is read, which on a two-column page is not the order the
    // producer emitted it in. `linesOf` groups by index adjacency, so on such a
    // page it reads one line from each column in turn --- which is what it did.
    const here = linksOnPage(this.links, page);
    for (const block of readingBlocks(content)) {
      for (const element of this.elementsFor(content, block, here)) {
        article.appendChild(element);
      }
    }

    if (article.childElementCount === 0) {
      // A scan, or a page of pure vector art. Saying so beats an empty element,
      // which a screen reader passes over in silence and which is
      // indistinguishable from the layer being broken.
      const empty = document.createElement("p");
      empty.textContent = "This page has no extractable text.";
      article.appendChild(empty);
    }

    return article;
  }

  /**
   * One block as elements: one element if the document said so, else one a line.
   *
   * The granularity question, answered by who drew the boundary. A **tagged**
   * block is a paragraph the producer declared, so it is handed over whole and a
   * screen reader moves through its lines itself --- which it does better than
   * this can, since it re-wraps to the user's settings. An **inferred** block
   * came out of the XY-cut, whose boundaries are a guess, so its lines are kept
   * separate: an over-eager cut then costs a reader nothing, where merging on one
   * would silently join two columns into a paragraph.
   *
   * That is also why the line split existed in the first place --- one
   * 2,700-character blob per page is unusable --- and the reason it can be
   * dropped for a tagged block is that the block is a paragraph rather than a
   * page.
   */
  private elementsFor(
    content: PageText,
    block: ReadingBlock,
    links: readonly Link[],
  ): HTMLElement[] {
    if (block.tag === null) {
      const out: HTMLElement[] = [];
      for (const line of block.lines) {
        const paragraph = document.createElement("p");
        if (!fill(paragraph, content, line.ranges, links)) continue;
        out.push(paragraph);
      }
      return out;
    }
    // The tag comes from the document; the element name does not. `elementFor`
    // is total and answers "p" or "h1".."h6" for every input, so no URL-bearing
    // element can come out of here whatever the file asked for.
    // webview-sink-ok: `elementFor` is a total whitelist of "p" and "h1".."h6"
    const element = document.createElement(elementFor(block.tag));
    // Joined with a space rather than a newline: these are the lines of one
    // paragraph, and a line break inside a paragraph is a rendering decision the
    // producer made about the page, not part of what it says. The join is done
    // by passing every line's ranges as one list with a separator between them,
    // rather than by concatenating strings --- a link that runs over a line break
    // is then one element instead of two, which is what it is.
    const ranges = block.lines.flatMap((line, at) =>
      at === 0 ? line.ranges : [SEPARATOR, ...line.ranges],
    );
    if (!fill(element, content, ranges, links)) return [];
    // The document's own word for it, on every block including the ones that
    // become a paragraph. It is not announced --- it is here so that a
    // `/Figure`, a `/TD` or a type nobody has seen before is *visible* to a check
    // and to anyone reading the DOM, rather than being flattened into "p" with
    // nothing recording that something was thrown away.
    element.dataset.tag = block.tag;
    return [element];
  }
}

/**
 * A marker range meaning "a space goes here", used to join a block's lines.
 *
 * An empty range is inert everywhere else --- `textOfRanges` emits nothing for
 * it and `linkRunsIn` iterates none of it --- so it survives the run splitting
 * without being a character that could fall inside a link.
 */
const SEPARATOR = { from: -1, to: -1 };

/**
 * Fills `element` with the text of `ranges`, marking the parts that are links.
 *
 * Returns whether anything was put in it, so a caller can drop an element that
 * would be empty --- a screen reader passes an empty element over in silence,
 * which is indistinguishable from the layer being broken.
 *
 * **A link becomes a `<span role="link">`, never an `<a>`.** That is not a
 * stylistic choice: `scripts/check_webview_sinks.py` refuses the creation of any
 * URL-bearing element anywhere in the frontend, which is what lets
 * `docs/THREAT-MODEL.md` T8 claim sufficiency from a grep. A span carrying a role
 * is announced as a link by every screen reader and can hold no URL at all, so
 * the security constraint and the accessible outcome want the same element.
 */
function fill(
  element: HTMLElement,
  content: PageText,
  ranges: readonly { from: number; to: number }[],
  links: readonly Link[],
): boolean {
  const runs = linkRunsIn(
    ranges.filter((range) => range !== SEPARATOR),
    content.boxes,
    links,
  );
  // Where the separators fall, so the join survives the run splitting: a run is
  // a stretch of character indices, and the space between two lines is not one.
  const breaks = new Set<number>();
  let seen = 0;
  for (const range of ranges) {
    if (range === SEPARATOR) breaks.add(seen);
    else seen += 1;
  }

  let text = "";
  /**
   * Everything written into the element, tracked here rather than read back
   * from `element.textContent` afterwards.
   *
   * Reading it back would be asking the DOM to aggregate its children's text,
   * which the real one does and the test double deliberately does not --- so the
   * emptiness test would have answered "empty" for every element under test
   * while working in the application. A check that disagrees with its subject
   * only under test is the worst of the two failures.
   */
  let all = "";
  /** Flushes the plain text accumulated so far. */
  const flush = (): void => {
    if (!text) return;
    element.appendChild(document.createTextNode(text));
    all += text;
    text = "";
  };

  let rangeAt = 0;
  for (const run of runs) {
    let piece = "";
    for (const range of run.ranges) {
      if (breaks.has(rangeAt)) piece += " ";
      piece += textOfRanges(content, [range]);
      rangeAt += 1;
    }
    if (!piece) continue;
    if (!run.link) {
      text += piece;
      continue;
    }
    flush();
    const span = document.createElement("span");
    span.setAttribute("role", "link");
    span.textContent = piece;
    // The destination as a number of ours, not a string of the document's ---
    // there is nothing here a file could have chosen. Present only for a link
    // that goes somewhere, so its absence is what says one does not.
    if (run.link.target.kind === "page") {
      span.dataset.page = String(run.link.target.page);
    } else {
      // Announced as unavailable rather than silently inert. A link tpdf
      // declines to follow is still a link the document drew, and a reader told
      // it is a link and then given nothing has been misled by us rather than by
      // the file.
      span.setAttribute("aria-disabled", "true");
    }
    element.appendChild(span);
    all += piece;
  }
  flush();
  return all.trim().length > 0;
}
