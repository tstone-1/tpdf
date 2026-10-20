/**
 * The print preview: every sheet of the print job, drawn, before the
 * platform's print dialog opens.
 *
 * Windows' classic print dialog and GTK's have no preview of their own
 * (`print_win.rs`, *classic_dialog*), so without this the first picture of the
 * job a reader saw was on paper. The sheets are pictures of the job's own
 * bytes, drawn by a worker one at a time as they come into view; what they
 * mean and in what order they are asked for is `printpreview.ts`'s.
 *
 * Every string goes in through `textContent`, and a picture is set only when
 * it is a PNG `data:` URL (`isPicture`).
 */

import { isPicture } from "./compressdialog";
import type { PageSize } from "./ipc";
import {
  SHEET_GAP,
  fitSheet,
  nextToDraw,
  pictureWidth,
  previewSummary,
  visibleSheets,
} from "./printpreview";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-print-preview";

/** Draws one sheet, `width` pixels wide, as a PNG `data:` URL. */
export type Drawer = (index: number, width: number) => Promise<string>;

/** A modal showing the job's sheets, with Print and Cancel. Built once and reused. */
export class PrintPreviewDialog {
  private readonly backdrop: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly summary: HTMLElement;
  private readonly list: HTMLElement;
  private readonly print: HTMLButtonElement;
  private sheets: { box: HTMLElement; picture: HTMLImageElement; note: HTMLElement }[] = [];
  private heights: number[] = [];
  private asked = new Set<number>();
  private draw: Drawer | null = null;
  private busy = false;
  private shown = false;
  /** Counts openings, so a picture for an earlier one is dropped. */
  private run = 0;
  private pending: ((print: boolean) => void) | null = null;
  private returnFocus: HTMLElement | null = null;

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:70;" +
      "background:rgba(0,0,0,0.28);align-items:center;justify-content:center;";

    const panel = document.createElement("div");
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-modal", "true");
    panel.setAttribute("aria-label", "Print preview");
    panel.style.cssText =
      "display:flex;flex-direction:column;width:min(760px,94vw);height:min(900px,90vh);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;box-sizing:border-box;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0;font-size:15px;font-weight:600;";
    this.summary = document.createElement("p");
    this.summary.style.cssText = "margin:0.15rem 0 0.6rem;opacity:0.8;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "list");
    this.list.setAttribute("aria-label", "Pages to print");
    this.list.tabIndex = 0;
    this.list.style.cssText =
      "flex:1;min-height:0;overflow:auto;padding:12px 0;" +
      `display:flex;flex-direction:column;align-items:center;gap:${SHEET_GAP}px;` +
      "background:color-mix(in srgb, CanvasText 10%, Canvas);border-radius:6px;";

    const under = document.createElement("p");
    under.style.cssText = "margin:0.6rem 0 0;opacity:0.72;";
    under.textContent = "You choose the printer, the paper and the pages to print in the next step.";

    const buttons = document.createElement("div");
    buttons.style.cssText = "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.6rem;";
    const cancel = this.button("Cancel", () => this.settle(false));
    this.print = this.button("Print...", () => this.settle(true));
    this.print.style.fontWeight = "600";
    buttons.append(cancel, this.print);

    panel.append(this.heading, this.summary, this.list, under, buttons);
    this.backdrop.append(panel);
    host.append(this.backdrop);

    this.list.addEventListener("scroll", () => this.pump());
    this.backdrop.addEventListener("click", (event) => {
      if (event.target === this.backdrop) this.settle(false);
    });
    this.backdrop.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.settle(false);
        return;
      }
      // Stopped as well as defaulted away: the window's key handler binds Enter.
      // A focused button answers Enter itself: Cancel must not print.
      if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement)) {
        event.preventDefault();
        event.stopPropagation();
        this.settle(true);
      }
    });
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.shown;
  }

  /**
   * Shows the sheets of a job named `name`, drawing each with `draw` as it
   * comes into view. Resolves `true` for Print and `false` when dismissed.
   */
  ask(name: string, pages: readonly PageSize[], draw: Drawer): Promise<boolean> {
    this.settle(false);
    this.run += 1;
    this.draw = draw;
    this.asked = new Set();
    this.busy = false;
    this.heading.textContent = `Print preview: ${name}`;
    this.summary.textContent = previewSummary(pages);
    if (!this.shown) {
      const active = document.activeElement as { focus?: () => void } | null;
      this.returnFocus = typeof active?.focus === "function" ? (active as HTMLElement) : null;
    }
    this.shown = true;
    this.backdrop.style.display = "flex";
    this.layOut(pages);
    this.print.disabled = pages.length === 0;
    this.print.focus();
    this.pump();
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding. */
  close(): void {
    this.settle(false);
  }

  /** One box per sheet, sized before anything is drawn, so the list can scroll. */
  private layOut(pages: readonly PageSize[]): void {
    this.list.replaceChildren();
    this.sheets = [];
    this.heights = [];
    const maxWidth = Math.max(120, this.list.clientWidth - 48);
    // Tall enough that a portrait page reads, short enough that the next one shows.
    const maxHeight = Math.max(160, Math.round(this.list.clientHeight * 0.92));
    pages.forEach((page, index) => {
      const { width, height } = fitSheet(page, maxWidth, maxHeight);
      const box = document.createElement("div");
      box.setAttribute("role", "listitem");
      box.style.cssText =
        `position:relative;flex:none;width:${width}px;height:${height}px;background:#fff;` +
        "box-shadow:0 1px 6px rgba(0,0,0,0.3);";
      // webview-sink-ok: its `src` is only ever a PNG `data:` URL; see `pump`.
      const picture = document.createElement("img");
      picture.setAttribute("alt", `Page ${index + 1} of ${pages.length}`);
      // Hidden until it has a picture: an `img` with no `src` draws the
      // browser's broken-image icon in its corner.
      picture.style.cssText = "display:none;width:100%;height:100%;";
      const note = document.createElement("span");
      note.style.cssText =
        "position:absolute;inset:0;display:flex;align-items:center;justify-content:center;" +
        "padding:1rem;text-align:center;color:#555;";
      note.textContent = `Page ${index + 1}`;
      box.append(picture, note);
      this.list.append(box);
      this.sheets.push({ box, picture, note });
      this.heights.push(height);
    });
  }

  /** Asks for the next sheet near the view, one at a time. */
  private pump(): void {
    const draw = this.draw;
    if (!this.shown || this.busy || !draw) return;
    const visible = visibleSheets(this.heights, Math.max(0, this.list.scrollTop - 12), this.list.clientHeight);
    const index = nextToDraw(visible, this.sheets.length, this.asked);
    const sheet = index === null ? undefined : this.sheets[index];
    if (index === null || !sheet) return;
    const run = this.run;
    this.asked.add(index);
    this.busy = true;
    sheet.note.textContent = `Drawing page ${index + 1}...`;
    const width = pictureWidth(sheet.box.offsetWidth, window.devicePixelRatio);
    void draw(index, width)
      .then((url) => {
        if (run !== this.run || !this.shown) return;
        if (isPicture(url)) {
          // webview-sink-ok: a PNG `data:` URL the backend encoded from the job it drew, checked by `isPicture`.
          sheet.picture.setAttribute("src", url);
          sheet.picture.style.display = "block";
          sheet.note.textContent = "";
        } else {
          sheet.note.textContent = `Page ${index + 1} could not be drawn.`;
        }
      })
      .catch((error: unknown) => {
        if (run !== this.run || !this.shown) return;
        sheet.note.textContent = `Page ${index + 1} could not be drawn: ${String(error)}`;
      })
      .finally(() => {
        if (run !== this.run) return;
        this.busy = false;
        this.pump();
      });
  }

  /** Resolves once and hides the dialog. */
  private settle(print: boolean): void {
    const pending = this.pending;
    this.pending = null;
    if (this.shown) {
      this.shown = false;
      this.backdrop.style.display = "none";
      // The pictures are of the reader's document: not kept once it is closed.
      this.list.replaceChildren();
      this.sheets = [];
      this.heights = [];
      this.draw = null;
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(print);
  }

  private button(label: string, onClick: () => void): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.style.cssText =
      "padding:0.35rem 0.9rem;font:inherit;border-radius:6px;" +
      "border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:ButtonFace;color:ButtonText;cursor:pointer;";
    button.addEventListener("click", onClick);
    return button;
  }
}
