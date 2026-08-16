/**
 * The sidebar: the outline, and a strip of page thumbnails.
 *
 * `docs/PLAN.md` §8 wants four tabs here --- thumbnails, outline, annotations,
 * search results --- and all four now exist. The chrome was built with the first
 * one for exactly this reason: the *second* tab is otherwise the one that has to
 * introduce it, by which point something else is positioned against its absence.
 *
 * ## The tabs are not equals, and the code says so
 *
 * An outline is read once, bounded at 10,000 entries, and costs nothing to
 * produce. A thumbnail is a Pdfium render call --- 1.5 s each on the A0 sheet ---
 * on the single thread that also draws the page in front of the reader. So the
 * strip is told when it is hidden and told when the viewer is busy, and stops in
 * both cases; the outline needs neither. See `thumbnails.ts`.
 *
 * ## It is a tree, and that has to be true for a screen reader too
 *
 * A list of indented rows *looks* like a tree and is announced as a flat list
 * unless it says otherwise. So the rows carry `role="treeitem"` with
 * `aria-level`, `aria-expanded` where there is something to expand, and the
 * whole thing is one tab stop with a roving `tabindex` --- Tab into it once,
 * then arrow around. Tabbing through a thousand headings is the alternative,
 * and it is why so many outline panels are unusable without a mouse.
 *
 * ## Rows that do nothing say why
 *
 * `outline.rs` refuses to follow `/Launch`, `/URI` and `/GoToR` actions, and
 * reports destinations that resolve to no page. A row for one of those is drawn
 * and marked `aria-disabled`, with the reason next to it, rather than being
 * dropped: an entry silently missing from a table of contents reads as a bug in
 * tpdf, and an entry that ignores clicks reads as a worse one.
 *
 * ## The list is bounded, not virtualized
 *
 * Every visible row is a real element. The walk is capped at 10,000 entries
 * (`outline.rs`), which is what makes that affordable, and a collapsed subtree
 * costs nothing because its rows are never built. Bounding the input is the
 * honest version of a windowing implementation that does not exist --- and the
 * cap is reported, so a document that hits it is not shown a silently truncated
 * table of contents.
 */

import {
  Expansion,
  allRows,
  currentId,
  flatten,
  isNavigable,
  openFlagOf,
  reasonFor,
  type Outline,
  type Row,
} from "./outline";
import { CommentList, type CommentListOptions } from "./commentlist";
import type { Comments } from "./comments";
import { Results, type ResultsOptions } from "./results";
import { Thumbnails, type ThumbnailOptions } from "./thumbnails";

/** Indent per level, in CSS pixels. */
const INDENT = 14;

/** Panel width, in CSS pixels. Not resizable yet. */
const WIDTH = 260;

/** Which panel is showing. */
export type Tab = "outline" | "pages" | "results" | "comments";

export interface SidebarOptions {
  /** Called when a row is activated. `top` is points from the page's top. */
  onNavigate: (page: number, top: number | null) => void;
  /** What the results tab needs. */
  results: ResultsOptions;
  /** What the comments tab needs. */
  comments: CommentListOptions;
  /**
   * What the page strip needs, or absent for no strip at all.
   *
   * Optional because the sidebar outlives no document while the strip is about
   * one: everything it needs --- the document id, the page count, the geometry,
   * the cache to borrow from --- arrives with the file being opened.
   */
  pages?: ThumbnailOptions;
}

/**
 * The class on the sidebar's own element.
 *
 * Exported because a check counts these to find out how many sidebars are in
 * the document, and a second copy of the string would agree with this one right
 * up until somebody renamed one of them --- at which point the count becomes
 * zero and the check reports the good news that nothing was duplicated.
 */
export const SIDEBAR_CLASS = "tpdf-sidebar";

export class Sidebar {
  private readonly host: HTMLElement;
  private readonly opts: SidebarOptions;
  private readonly tablist: HTMLElement;
  private readonly tabs = new Map<Tab, HTMLButtonElement>();
  private readonly panels = new Map<Tab, HTMLElement>();
  private readonly notice: HTMLElement;
  private readonly tree: HTMLElement;
  private readonly strip: Thumbnails | null = null;
  private readonly hits: Results;
  private readonly notes: CommentList;
  private showing: Tab = "outline";
  private visibleNow = true;

  private outline: Outline | null = null;
  /** Whether an answer has arrived, as distinct from an empty one having. */
  private loaded = false;
  private expansion = new Expansion();
  private rows: Row[] = [];
  /**
   * Every row, collapse ignored, computed once per outline.
   *
   * `setPosition` runs on every frame of a scroll, and rebuilding this there
   * would walk the whole tree sixty times a second on a document with ten
   * thousand entries.
   */
  private ordered: Row[] = [];
  private readonly elements = new Map<string, HTMLElement>();
  /** Last position asked about, so an unchanged frame costs one comparison. */
  private asked = { page: -1, top: -1 };

  /** Row the roving tabindex is on. */
  private focused: string | null = null;
  /** Row the reader is currently inside, from the scroll position. */
  private current: string | null = null;

  constructor(root: HTMLElement, opts: SidebarOptions) {
    this.opts = opts;

    this.host = document.createElement("aside");
    this.host.className = SIDEBAR_CLASS;
    this.host.setAttribute("aria-label", "Sidebar");
    this.host.style.cssText =
      "display:flex;flex-direction:column;height:100%;box-sizing:border-box;" +
      `flex:none;width:${WIDTH}px;` +
      "border-right:1px solid color-mix(in srgb, currentColor 15%, transparent);" +
      "font:13px/1.5 system-ui,-apple-system,sans-serif;overflow:hidden;";

    this.tablist = document.createElement("div");
    this.tablist.setAttribute("role", "tablist");
    this.tablist.setAttribute("aria-label", "Sidebar");
    this.tablist.style.cssText =
      "flex:none;display:flex;gap:0.2rem;padding:0.3rem 0.4rem;" +
      "border-bottom:1px solid color-mix(in srgb, currentColor 10%, transparent);";
    this.tablist.addEventListener("keydown", this.onTabKey);

    // A live region rather than static text: it appears after the tree has been
    // read, and a reader who has already moved on should still hear that the
    // outline they are navigating is not all of it.
    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText =
      "flex:none;padding:0.3rem 0.7rem;opacity:0.7;display:none;";

    this.tree = document.createElement("div");
    this.tree.setAttribute("role", "tree");
    this.tree.setAttribute("aria-label", "Document outline");
    this.tree.style.cssText = "flex:1;min-height:0;overflow:auto;padding:0.2rem 0;";
    this.tree.addEventListener("keydown", this.onKeyDown);
    // Focus can arrive without going through `focus(id)` --- a Tab into the
    // tree, a click the browser handled, a programmatic `element.focus()` ---
    // and a roving tabindex that does not follow it then aims every key at
    // whichever row happened to be tracked. The symptom is that arrow keys
    // silently operate on the wrong row, which is what made "collapsing a row
    // hides its children" report 7 rows before and 7 rows after.
    this.tree.addEventListener("focusin", (event) => {
      const id = (event.target as HTMLElement | null)?.dataset?.id;
      if (id !== undefined) this.focus(id);
    });

    const outlinePanel = this.panel("outline", "Outline");
    outlinePanel.append(this.notice, this.tree);
    this.host.append(this.tablist, outlinePanel);

    const pagesPanel = this.panel("pages", "Pages");
    this.host.appendChild(pagesPanel);
    if (opts.pages) this.strip = new Thumbnails(pagesPanel, opts.pages);

    const resultsPanel = this.panel("results", "Results");
    this.host.appendChild(resultsPanel);
    this.hits = new Results(resultsPanel, opts.results);

    const commentsPanel = this.panel("comments", "Comments");
    this.host.appendChild(commentsPanel);
    this.notes = new CommentList(commentsPanel, opts.comments);

    root.appendChild(this.host);
    this.selectTab("outline");
  }

  /** Builds one tab and its panel, wired to each other by id. */
  private panel(tab: Tab, label: string): HTMLElement {
    const button = document.createElement("button");
    button.setAttribute("role", "tab");
    button.id = `tpdf-tab-${tab}`;
    button.setAttribute("aria-controls", `tpdf-panel-${tab}`);
    button.textContent = label;
    button.style.cssText =
      "font:inherit;flex:1;padding:0.15rem 0.4rem;border:0;background:none;" +
      "color:inherit;cursor:default;border-radius:4px;";
    button.addEventListener("click", () => this.selectTab(tab));
    this.tablist.appendChild(button);
    this.tabs.set(tab, button);

    const element = document.createElement("div");
    element.setAttribute("role", "tabpanel");
    element.id = `tpdf-panel-${tab}`;
    element.setAttribute("aria-labelledby", button.id);
    element.style.cssText = "flex:1;min-height:0;display:flex;flex-direction:column;";
    this.panels.set(tab, element);
    return element;
  }

  destroy(): void {
    this.strip?.destroy();
    this.host.remove();
  }

  /** Which panel is showing. */
  get tab(): Tab {
    return this.showing;
  }

  /** Whether the panel itself is on screen. */
  get shown(): boolean {
    return this.visibleNow;
  }

  /**
   * Shows or hides the whole panel.
   *
   * Not the caller's job to also tell the strip: a hidden panel and a hidden tab
   * are the same thing to it, and splitting the two across two call sites is how
   * one of them ends up forgotten and a strip renders behind a closed sidebar.
   */
  setVisible(shown: boolean): void {
    this.visibleNow = shown;
    this.host.style.display = shown ? "flex" : "none";
    this.strip?.setActive(shown && this.showing === "pages");
  }

  /** The page strip, or `null` when the sidebar was built without one. */
  get thumbnails(): Thumbnails | null {
    return this.strip;
  }

  /** The search-results panel. */
  get results(): Results {
    return this.hits;
  }

  /** The comments panel. */
  get comments(): CommentList {
    return this.notes;
  }

  /** Replaces the comments shown. `null` is a document that could not be read. */
  setComments(comments: Comments | null): void {
    this.notes.setComments(comments);
  }

  /**
   * Shows one panel and hides the others.
   *
   * The strip is told, and that is not cosmetic: a hidden strip that carried on
   * rendering would spend the render thread on pictures nobody can see, in
   * front of the tiles for the page they are actually reading. The results panel
   * needs no such call --- it costs nothing to hold and nothing to update, since
   * the work behind it is a scan that runs whether or not anybody is looking.
   */
  selectTab(tab: Tab): void {
    this.showing = tab;
    for (const [name, button] of this.tabs) {
      const on = name === tab;
      button.setAttribute("aria-selected", String(on));
      // A tablist is one tab stop, like the tree inside it.
      button.tabIndex = on ? 0 : -1;
      button.style.background = on
        ? "color-mix(in srgb, currentColor 12%, transparent)"
        : "";
      button.style.fontWeight = on ? "600" : "400";
      const panel = this.panels.get(name);
      if (panel) panel.style.display = on ? "flex" : "none";
    }
    this.strip?.setActive(this.visibleNow && tab === "pages");
  }

  /** Tells the page strip whether the viewer has work outstanding. */
  setViewerBusy(busy: boolean): void {
    this.strip?.setViewerBusy(busy);
  }

  /** Rotates the page strip to match the view. */
  setTurns(turns: number): void {
    this.strip?.setTurns(turns);
  }

  /** Inverts the thumbnails with the page, so the strip is not the odd one out. */
  setInvert(invert: boolean): void {
    this.strip?.setInvert(invert);
  }

  private readonly onTabKey = (event: KeyboardEvent): void => {
    const order: Tab[] = [...this.tabs.keys()];
    const at = order.indexOf(this.showing);
    let next = at;
    if (event.key === "ArrowRight") next = (at + 1) % order.length;
    else if (event.key === "ArrowLeft") next = (at + order.length - 1) % order.length;
    else return;
    event.preventDefault();
    const tab = order[next];
    if (!tab) return;
    this.selectTab(tab);
    this.tabs.get(tab)?.focus();
  };

  /** The panel element, so a caller can show or hide it. */
  get element(): HTMLElement {
    return this.host;
  }

  /** Titles of the rows currently drawn, in order. For the check harness. */
  get visible(): string[] {
    return this.rows.map((row) => row.title);
  }

  /** Id of the row the reader is inside, or "". For the check harness. */
  get currentRow(): string {
    return this.current ?? "";
  }

  /** Id of the row holding the roving tabindex, or "". For the check harness. */
  get focusedRow(): string {
    return this.focused ?? "";
  }

  /** The row elements, keyed by id. For the check harness. */
  elementFor(id: string): HTMLElement | null {
    return this.elements.get(id) ?? null;
  }

  /**
   * Replaces the outline shown.
   *
   * Three states, and they say three different things. Before this is called at
   * all, the outline has not arrived yet. An outline with no items is a
   * document that genuinely has none. `null` is a document whose outline could
   * not be read. Collapsing the first two --- the obvious simplification ---
   * makes a slow document look like an outline-less one for as long as it takes
   * to arrive, which is exactly when someone would be looking.
   */
  setOutline(outline: Outline | null): void {
    this.loaded = true;
    this.outline = outline;
    this.expansion = new Expansion();
    this.ordered = outline ? allRows(outline.items) : [];
    this.focused = null;
    this.current = null;
    this.asked = { page: -1, top: -1 };
    this.paint();
  }

  /**
   * Tells the sidebar where the reader is, so the right row is marked.
   *
   * Called from the viewer's frame loop, so it must be cheap when nothing has
   * changed: it recomputes an id from the row list and returns without touching
   * the DOM unless that id moved.
   */
  setPosition(page: number, top: number): void {
    // Before the outline guard, and not inside it: the page strip has to follow
    // the reader on a document with no outline at all, which is most of them.
    this.strip?.setCurrentPage(page);
    if (!this.outline) return;
    // Rounded, because the answer cannot change on a sub-point movement and
    // the scroll offset is a float that changes on every frame of a trackpad
    // flick.
    const rounded = Math.round(top);
    if (page === this.asked.page && rounded === this.asked.top) return;
    this.asked = { page, top: rounded };

    const next = currentId(this.ordered, page, top);
    if (next === this.current) return;

    if (this.current) this.mark(this.current, false);
    this.current = next;
    if (next) {
      this.mark(next, true);
      this.elements.get(next)?.scrollIntoView({ block: "nearest" });
    }
  }

  private mark(id: string, on: boolean): void {
    const element = this.elements.get(id);
    if (!element) return;
    if (on) element.setAttribute("aria-current", "true");
    else element.removeAttribute("aria-current");
    element.style.background = on
      ? "color-mix(in srgb, currentColor 10%, transparent)"
      : "";
  }

  private paint(): void {
    this.tree.replaceChildren();
    this.elements.clear();

    const limits = this.outline?.limits;
    if (limits && (limits.cycles || limits.too_deep || limits.over_budget)) {
      this.notice.style.display = "";
      this.notice.textContent = noticeFor(limits);
    } else {
      this.notice.style.display = "none";
      this.notice.textContent = "";
    }

    if (!this.loaded) {
      this.tree.appendChild(placeholder("Reading the outline…"));
      this.rows = [];
      return;
    }
    if (!this.outline) {
      this.tree.appendChild(placeholder("The outline could not be read."));
      this.rows = [];
      return;
    }
    if (this.outline.items.length === 0) {
      this.tree.appendChild(placeholder("This document has no outline."));
      this.rows = [];
      return;
    }

    this.rows = flatten(this.outline.items, this.expansion);
    // The roving tabindex has to land somewhere before the first Tab, and the
    // row the reader is in beats the first row when there is one.
    if (!this.focused || !this.rows.some((row) => row.id === this.focused)) {
      this.focused = this.current ?? this.rows[0]?.id ?? null;
    }

    for (const row of this.rows) {
      const element = this.build(row);
      this.elements.set(row.id, element);
      this.tree.appendChild(element);
    }
    if (this.current) this.mark(this.current, true);
  }

  private build(row: Row): HTMLElement {
    const navigable = isNavigable(row.target);
    const element = document.createElement("div");
    element.setAttribute("role", "treeitem");
    element.setAttribute("aria-level", String(row.depth + 1));
    if (row.hasChildren) {
      element.setAttribute("aria-expanded", String(row.expanded));
    }
    if (!navigable) element.setAttribute("aria-disabled", "true");
    element.tabIndex = row.id === this.focused ? 0 : -1;
    element.dataset.id = row.id;
    element.style.cssText =
      "display:flex;align-items:baseline;gap:0.35rem;cursor:default;" +
      `padding:0.15rem 0.6rem 0.15rem ${6 + row.depth * INDENT}px;` +
      (navigable ? "" : "opacity:0.6;");

    const twisty = document.createElement("span");
    twisty.setAttribute("aria-hidden", "true");
    twisty.style.cssText = "width:0.8em;flex:none;opacity:0.6;";
    twisty.textContent = row.hasChildren ? (row.expanded ? "▾" : "▸") : "";
    if (row.hasChildren) {
      twisty.style.cursor = "pointer";
      twisty.addEventListener("pointerdown", (event) => {
        // Stops the row's own handler from also navigating: clicking the
        // twisty is a request to unfold, not to go there.
        event.preventDefault();
        event.stopPropagation();
        this.toggle(row.id);
      });
    }

    const title = document.createElement("span");
    title.style.cssText = "flex:1;overflow-wrap:anywhere;";
    title.textContent = row.title || "(untitled)";

    element.append(twisty, title);

    const reason = reasonFor(row.target);
    if (reason) {
      const note = document.createElement("span");
      note.textContent = reason;
      note.style.cssText = "opacity:0.7;font-size:0.85em;flex:none;";
      element.appendChild(note);
    }

    element.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.focus(row.id);
      this.activate(row.id);
    });

    return element;
  }

  private toggle(id: string): void {
    if (!this.outline) return;
    this.expansion.toggle(id);
    this.focused = id;
    this.paint();
    this.elements.get(id)?.focus();
  }

  private focus(id: string): void {
    if (this.focused === id) return;
    const previous = this.focused ? this.elements.get(this.focused) : null;
    if (previous) previous.tabIndex = -1;
    this.focused = id;
    const element = this.elements.get(id);
    if (element) element.tabIndex = 0;
  }

  /** Moves focus by `delta` rows and puts the keyboard there. */
  private move(delta: number): void {
    if (this.rows.length === 0) return;
    const at = this.rows.findIndex((row) => row.id === this.focused);
    const next = Math.max(0, Math.min((at < 0 ? 0 : at) + delta, this.rows.length - 1));
    const target = this.rows[next];
    if (!target) return;
    this.focus(target.id);
    this.elements.get(target.id)?.focus();
  }

  /** Navigates to a row, if it points anywhere. */
  private activate(id: string): void {
    const row = this.rows.find((candidate) => candidate.id === id);
    if (!row || !isNavigable(row.target)) return;
    this.opts.onNavigate(row.target.page, row.target.top_pt);
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    // The event's target is authoritative: it *is* the element the key landed
    // on, while `focused` is a mirror kept by the `focusin` listener, and a
    // document without system focus moves `activeElement` without delivering
    // that event. Enter derived this for itself and every other key read the
    // mirror, so whenever it was stale ArrowLeft collapsed nothing and
    // ArrowDown stepped from a row the reader was not on --- and because it
    // depends on whether the window holds system focus, it presented as a
    // check that failed one run in three rather than as a bug. Reconciled once
    // here instead, so `move`, `toParent` and `activate` all agree about which
    // row the key reached. The mirror stays as the fallback for a key that
    // arrived on the tree rather than on a row. See the trap.
    const from = (event.target as HTMLElement | null)?.dataset?.id ?? this.focused;
    if (from && from !== this.focused && this.elements.has(from)) this.focus(from);
    const row = this.rows.find((candidate) => candidate.id === from);

    switch (event.key) {
      case "ArrowDown":
        this.move(1);
        break;
      case "ArrowUp":
        this.move(-1);
        break;
      case "Home":
        this.move(-this.rows.length);
        break;
      case "End":
        this.move(this.rows.length);
        break;
      case "ArrowRight":
        // Expand, or step into an already-expanded subtree. The two-step
        // behaviour is what every tree widget does and is why Right is not
        // simply "expand".
        if (row?.hasChildren && !row.expanded) this.toggle(row.id);
        else if (row?.hasChildren) this.move(1);
        else return;
        break;
      case "ArrowLeft":
        // Collapse, or step out to the parent.
        if (row?.hasChildren && row.expanded) this.toggle(row.id);
        else if (row) this.toParent(row);
        else return;
        break;
      case "Enter":
      case " ": {
        // `from` is resolved once at the top of this handler, for the reason
        // stated there. It used to be derived here and nowhere else, which is
        // how the arrows kept reading the stale mirror for as long as they did.
        if (from) this.activate(from);
        break;
      }
      default:
        return;
    }

    event.preventDefault();
    // The viewer underneath scrolls on arrows and Home/End; a tree that let
    // them through would move the page as well as the selection.
    event.stopPropagation();
  };

  /** Moves focus to the row's parent, if it has one. */
  private toParent(row: Row): void {
    const cut = row.id.lastIndexOf(".");
    if (cut < 0) return;
    const parent = row.id.slice(0, cut);
    this.focus(parent);
    this.elements.get(parent)?.focus();
  }

  /** Expands every ancestor of `id` and focuses it. For "reveal in outline". */
  reveal(id: string): void {
    if (!this.outline) return;
    const items = this.outline.items;
    this.expansion.reveal(id, (ancestor) => openFlagOf(items, ancestor));
    this.focused = id;
    this.paint();
    this.elements.get(id)?.scrollIntoView({ block: "nearest" });
  }
}

function placeholder(text: string): HTMLElement {
  const element = document.createElement("div");
  element.style.cssText = "padding:0.5rem 0.7rem;opacity:0.55;";
  element.textContent = text;
  return element;
}

/** What the limits banner says, naming the bound that fired. */
function noticeFor(limits: Outline["limits"]): string {
  const parts: string[] = [];
  if (limits.cycles) parts.push("loops back on itself");
  if (limits.too_deep) parts.push("nests deeper than tpdf will follow");
  if (limits.over_budget) parts.push("has more entries than tpdf will show");
  return `This outline ${parts.join(", ")} — what is shown is incomplete.`;
}
