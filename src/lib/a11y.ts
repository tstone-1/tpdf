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
 * And it is **not verified against a screen reader**. What the check asserts is
 * that the text is present, correct, in reading order, and stable across
 * scrolling. Whether VoiceOver announces it *well* is not measured here, and no
 * claim is made about it.
 */

import { linesOf, textOf, type PageText } from "./text";

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
  private readonly pageCount: number;
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
  sync(visible: readonly number[], text: (page: number) => PageText | null): void {
    const wanted = new Set(visible);

    for (const [page, element] of this.pages) {
      if (wanted.has(page)) continue;
      element.remove();
      this.pages.delete(page);
    }

    for (const page of [...wanted].sort((a, b) => a - b)) {
      if (this.pages.has(page)) continue;
      const content = text(page);
      if (!content) continue;
      const element = this.build(page, content);
      this.pages.set(page, element);
      this.insert(page, element);
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

  private build(page: number, content: PageText): HTMLElement {
    const article = document.createElement("article");
    article.setAttribute("aria-label", `Page ${page + 1} of ${this.pageCount}`);
    // Focusable programmatically but not in the tab order: Tab through a
    // 775-page document would be a trap, and the page still needs to be a
    // target for "go to this page".
    article.tabIndex = -1;
    article.dataset.page = String(page);

    for (const line of linesOf(content)) {
      const text = textOf(content, line.from, line.to).trim();
      if (!text) continue;
      const paragraph = document.createElement("p");
      paragraph.textContent = text;
      article.appendChild(paragraph);
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
}
