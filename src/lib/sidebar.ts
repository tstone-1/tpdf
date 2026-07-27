/**
 * The sidebar, and the outline in it.
 *
 * `docs/PLAN.md` §8 wants tabs here eventually --- thumbnails, annotations,
 * search results --- and only the outline exists today. The panel is
 * nonetheless a container with a header rather than a bare list, because the
 * alternative is that the *second* tab is the one that has to introduce the
 * chrome, and by then something else is already positioned against its absence.
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

/** Indent per level, in CSS pixels. */
const INDENT = 14;

/** Panel width, in CSS pixels. Not resizable yet. */
const WIDTH = 260;

export interface SidebarOptions {
  /** Called when a row is activated. `top` is points from the page's top. */
  onNavigate: (page: number, top: number | null) => void;
}

export class Sidebar {
  private readonly host: HTMLElement;
  private readonly opts: SidebarOptions;
  private readonly heading: HTMLElement;
  private readonly notice: HTMLElement;
  private readonly tree: HTMLElement;

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
    this.host.className = "tpdf-sidebar";
    this.host.setAttribute("aria-label", "Outline");
    this.host.style.cssText =
      "display:flex;flex-direction:column;height:100%;box-sizing:border-box;" +
      `flex:none;width:${WIDTH}px;` +
      "border-right:1px solid color-mix(in srgb, currentColor 15%, transparent);" +
      "font:13px/1.5 system-ui,-apple-system,sans-serif;overflow:hidden;";

    this.heading = document.createElement("div");
    this.heading.style.cssText =
      "flex:none;padding:0.4rem 0.7rem;font-weight:600;opacity:0.7;" +
      "border-bottom:1px solid color-mix(in srgb, currentColor 10%, transparent);";
    this.heading.textContent = "Outline";

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

    this.host.append(this.heading, this.notice, this.tree);
    root.appendChild(this.host);
  }

  destroy(): void {
    this.host.remove();
  }

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
    const row = this.rows.find((candidate) => candidate.id === this.focused);

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
      case " ":
        if (this.focused) this.activate(this.focused);
        break;
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
