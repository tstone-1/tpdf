/**
 * The command palette: type a few letters, press Enter.
 *
 * `docs/PLAN.md` §8 makes this the primary route to every command and Phase 1
 * work rather than polish, on the grounds that Acrobat's problem is not missing
 * capability but unreachable capability. Two consequences shape what is here.
 *
 * **It shows each command's keybinding.** A palette that only ran commands would
 * make a keyboard-first application *less* keyboard-driven over time, because
 * nobody would ever learn the shortcut. Showing it turns every use into a
 * lesson. The binding is a label, though: the key handler is elsewhere, and
 * nothing checks that the two agree.
 *
 * **It is plain DOM, not a Svelte component.** The same reason `viewer.ts` is:
 * `viewercheck.ts` mounts these classes directly and dispatches real events at
 * them, and a component that only exists inside `App.svelte` is a component the
 * check cannot reach. Nothing here needs reactivity that a list of a dozen rows
 * cannot afford to rebuild.
 */

import type { CommandRegistry, Ranked } from "./commands";

/** Rows shown at once. More than this and the list scrolls. */
const MAX_ROWS = 12;

export class Palette {
  private readonly registry: CommandRegistry;
  private readonly backdrop: HTMLDivElement;
  private readonly input: HTMLInputElement;
  private readonly list: HTMLDivElement;

  private results: Ranked[] = [];
  private selected = 0;
  /** What to focus when the palette closes, so Escape does not lose the page. */
  private returnFocus: HTMLElement | null = null;

  constructor(registry: CommandRegistry) {
    this.registry = registry;

    this.backdrop = document.createElement("div");
    this.backdrop.className = "tpdf-palette";
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:50;" +
      "background:rgba(0,0,0,0.28);align-items:flex-start;justify-content:center;";

    const panel = document.createElement("div");
    panel.style.cssText =
      "margin-top:12vh;width:min(560px,90vw);border-radius:10px;overflow:hidden;" +
      "background:Canvas;color:CanvasText;box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.5 system-ui,-apple-system,sans-serif;";

    this.input = document.createElement("input");
    this.input.type = "text";
    this.input.placeholder = "Run a command";
    this.input.setAttribute("aria-label", "Run a command");
    this.input.style.cssText =
      "width:100%;box-sizing:border-box;border:0;outline:none;font:inherit;" +
      "font-size:15px;padding:0.7rem 0.9rem;background:transparent;color:inherit;" +
      "border-bottom:1px solid color-mix(in srgb, currentColor 15%, transparent);";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "listbox");
    this.list.style.cssText = `max-height:${MAX_ROWS * 32}px;overflow-y:auto;`;

    panel.append(this.input, this.list);
    this.backdrop.appendChild(panel);
    document.body.appendChild(this.backdrop);

    this.input.addEventListener("input", () => this.refresh());
    this.input.addEventListener("keydown", this.onKeyDown);
    // Clicking the backdrop, but not the panel, dismisses it.
    this.backdrop.addEventListener("pointerdown", (event) => {
      if (event.target === this.backdrop) this.close();
    });
  }

  destroy(): void {
    this.backdrop.remove();
  }

  /** Whether the palette is showing. */
  get isOpen(): boolean {
    return this.backdrop.style.display !== "none";
  }

  /** Titles currently listed, best first. For the check harness. */
  get visible(): string[] {
    return this.results.map((r) => r.command.title);
  }

  /** Title of the highlighted row, or "". For the check harness. */
  get highlighted(): string {
    return this.results[this.selected]?.command.title ?? "";
  }

  /**
   * Opens the palette with an empty query.
   *
   * The query is cleared rather than kept: a palette that reopens on the last
   * search makes the second use slower than the first, and the recents list
   * already covers "the thing I just did".
   */
  open(): void {
    this.returnFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    this.backdrop.style.display = "flex";
    this.input.value = "";
    this.refresh();
    this.input.focus();
  }

  close(): void {
    if (!this.isOpen) return;
    this.backdrop.style.display = "none";
    this.returnFocus?.focus();
  }

  /** Runs the highlighted command and closes. */
  runSelected(): void {
    const chosen = this.results[this.selected];
    // Close first: a command that moves focus --- the find field, a dialog ---
    // must not have it taken back by `returnFocus` a moment later.
    this.close();
    if (chosen) this.registry.run(chosen.command.id);
  }

  private readonly onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "ArrowDown") this.move(1);
    else if (event.key === "ArrowUp") this.move(-1);
    else if (event.key === "Enter") this.runSelected();
    else if (event.key === "Escape") this.close();
    else return;
    event.preventDefault();
    // The palette owns these keys entirely; letting them reach the viewer
    // underneath would scroll the page behind the open panel.
    event.stopPropagation();
  };

  private move(delta: number): void {
    const count = this.results.length;
    if (count === 0) return;
    this.selected = (this.selected + delta + count) % count;
    this.paint();
  }

  private refresh(): void {
    this.results = this.registry.search(this.input.value).slice(0, 64);
    this.selected = 0;
    this.paint();
  }

  private paint(): void {
    this.list.replaceChildren();

    if (this.results.length === 0) {
      const empty = document.createElement("div");
      empty.style.cssText = "padding:0.6rem 0.9rem;opacity:0.55;";
      empty.textContent = "No matching command";
      this.list.appendChild(empty);
      return;
    }

    this.results.forEach((result, index) => {
      const row = document.createElement("div");
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(index === this.selected));
      row.style.cssText =
        "display:flex;align-items:center;gap:0.75rem;padding:0.35rem 0.9rem;" +
        "cursor:default;" +
        (index === this.selected
          ? "background:color-mix(in srgb, currentColor 12%, transparent);"
          : "");

      const title = document.createElement("span");
      title.style.flex = "1";
      title.append(...highlight(result));
      row.appendChild(title);

      if (result.command.keys) {
        const keys = document.createElement("span");
        keys.textContent = result.command.keys;
        keys.style.cssText = "opacity:0.5;font-variant-numeric:tabular-nums;";
        row.appendChild(keys);
      }

      // `pointerdown` rather than `click`: the input has focus, and a click
      // would blur it first, which on some platforms closes the panel before
      // the row is ever notified.
      row.addEventListener("pointerdown", (event) => {
        event.preventDefault();
        this.selected = index;
        this.runSelected();
      });

      this.list.appendChild(row);
    });
  }
}

/** A command's title with the matched characters emboldened. */
function highlight(result: Ranked): Node[] {
  const { title } = result.command;
  const marks = new Set(result.positions);
  const nodes: Node[] = [];
  let run = "";
  let runMatched = false;

  const flush = (): void => {
    if (!run) return;
    if (runMatched) {
      const strong = document.createElement("strong");
      strong.textContent = run;
      nodes.push(strong);
    } else {
      nodes.push(document.createTextNode(run));
    }
    run = "";
  };

  for (let index = 0; index < title.length; index++) {
    const matched = marks.has(index);
    if (matched !== runMatched) {
      flush();
      runMatched = matched;
    }
    run += title[index];
  }
  flush();
  return nodes;
}
