/**
 * The dialog that shows what a document says about itself.
 *
 * ## Why a dialog and not a sixth sidebar tab
 *
 * The sidebar is for things you navigate *alongside* the page --- an outline you
 * click through, a comment list you step down, search results you walk. A
 * properties readout is none of those: it is a thing you open, read, and
 * dismiss, and it never wants to be on screen at the same time as the page it
 * describes.
 *
 * There is also a measured cost. `docs/TRAPS.md` records that five tab labels
 * already want 318 px of the sidebar's 247, which clipped one of them out of
 * reach entirely; the row wraps now, so a sixth would not vanish, but it would
 * spend a second row of chrome on every document for a panel most readers open
 * rarely.
 *
 * ## Everything it shows comes from `properties.ts`
 *
 * This file builds elements out of [`Section`]s and does nothing else. That
 * split is deliberate --- every decision in a properties readout is a decision
 * that can be *wrong*, and a wrong one is a confident false statement about a
 * document somebody is about to rely on. Those all live in a pure module with
 * tests. What is left here cannot be wrong in that way.
 *
 * ## Document text reaches the DOM here, and only as text
 *
 * A `/Producer` string, a signer's stated reason and a custom `/Info` key are
 * all attacker-controlled, exactly like an outline title. `docs/THREAT-MODEL.md`
 * T8 holds the same way it does everywhere else: every value arrives through
 * `textContent`, no element created here carries a URL-bearing attribute, and
 * the `sinks` gate is what keeps that true rather than this comment.
 */

import { type Properties, type Section, sections } from "./properties";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-properties";

/**
 * A modal readout of a document's properties.
 *
 * Built once and reused, like the palette: rebuilding the element on every open
 * would be simpler and would lose the reader's scroll position on a document
 * they are stepping back and forth in.
 */
export class PropertiesDialog {
  private readonly backdrop: HTMLElement;
  private readonly panel: HTMLElement;
  private readonly body: HTMLElement;
  private readonly heading: HTMLElement;
  /** What to focus when it closes, so Escape does not lose the page. */
  private returnFocus: HTMLElement | null = null;

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:60;" +
      "background:rgba(0,0,0,0.28);align-items:flex-start;justify-content:center;";

    this.panel = document.createElement("div");
    this.panel.setAttribute("role", "dialog");
    this.panel.setAttribute("aria-modal", "true");
    this.panel.setAttribute("aria-label", "Document properties");
    this.panel.tabIndex = -1;
    this.panel.style.cssText =
      "margin-top:8vh;width:min(620px,92vw);max-height:78vh;overflow:auto;" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;";

    this.heading = document.createElement("h2");
    this.heading.textContent = "Document properties";
    this.heading.style.cssText =
      "margin:0;padding:0.85rem 1rem;font-size:15px;font-weight:600;" +
      "border-bottom:1px solid color-mix(in srgb, CanvasText 14%, transparent);" +
      "position:sticky;top:0;background:Canvas;";

    this.body = document.createElement("div");
    this.body.style.cssText = "padding:0.4rem 1rem 1rem;";

    this.panel.append(this.heading, this.body);
    this.backdrop.append(this.panel);
    host.append(this.backdrop);

    // A click on the backdrop itself, never one that bubbled out of the panel.
    this.backdrop.addEventListener("click", (event) => {
      if (event.target === this.backdrop) this.close();
    });
    this.backdrop.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      this.close();
    });
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.backdrop.style.display !== "none";
  }

  /**
   * Shows a readout, or the reason there is not one.
   *
   * `null` is the state before the answer arrives and is a legitimate thing to
   * show: the parse is lazy and can take a moment on a large document, and a
   * dialog that opened empty and filled in silently would read as a document
   * that states nothing about itself.
   */
  show(properties: Properties | null, problem: string): void {
    this.body.replaceChildren();

    if (problem) {
      this.body.append(this.message(problem, true));
    } else if (!properties) {
      this.body.append(this.message("Reading the document...", false));
    } else {
      for (const section of sections(properties)) {
        this.body.append(this.section(section));
      }
    }

    this.open();
  }

  private open(): void {
    if (!this.isOpen) {
      this.returnFocus =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
    }
    this.backdrop.style.display = "flex";
    this.panel.focus();
  }

  close(): void {
    if (!this.isOpen) return;
    this.backdrop.style.display = "none";
    this.returnFocus?.focus();
    this.returnFocus = null;
  }

  /** One block: a heading, its rows, and any caveat under them. */
  private section(section: Section): HTMLElement {
    const block = document.createElement("section");
    block.style.cssText = "margin-top:1rem;";

    const title = document.createElement("h3");
    title.textContent = section.title;
    title.style.cssText =
      "margin:0 0 0.35rem;font-size:11px;font-weight:600;letter-spacing:0.04em;" +
      "text-transform:uppercase;opacity:0.62;";
    block.append(title);

    const list = document.createElement("dl");
    list.style.cssText =
      "display:grid;grid-template-columns:minmax(7rem,auto) 1fr;" +
      "gap:0.2rem 0.9rem;margin:0;";
    for (const row of section.rows) {
      const name = document.createElement("dt");
      name.textContent = row.name;
      name.style.cssText = "opacity:0.68;";
      const value = document.createElement("dd");
      value.textContent = row.value;
      value.style.cssText = row.warn
        ? "margin:0;font-weight:600;"
        : "margin:0;overflow-wrap:anywhere;";
      list.append(name, value);
    }
    block.append(list);

    if (section.note) {
      const note = document.createElement("p");
      note.textContent = section.note;
      note.style.cssText =
        "margin:0.45rem 0 0;font-size:11.5px;line-height:1.5;opacity:0.66;";
      block.append(note);
    }

    return block;
  }

  /** A single sentence in the body, for waiting or for a failure. */
  private message(text: string, failed: boolean): HTMLElement {
    const line = document.createElement("p");
    line.textContent = text;
    line.style.cssText = failed
      ? "margin:1rem 0 0;font-weight:600;"
      : "margin:1rem 0 0;opacity:0.7;";
    return line;
  }
}
