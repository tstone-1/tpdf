/**
 * The sidebar tab listing text the pages do not show: what the last check of
 * the open document found, one row a passage.
 *
 * What the rows say and what stands above them is `hiddentext.ts`; this draws
 * it. A row is a real button, so it is reached with Tab and pressed with Enter
 * or Space by the platform, and this panel keeps no focus of its own to go
 * stale --- `rovinglists.test.ts` has what the five panels that do keep one
 * have cost. The list is capped ({@link MAX_ROWS}) and the cap is said.
 *
 * **An empty result is not an empty panel.** The sentences are shown whenever
 * a check has been run, with or without rows: what was not compared and what
 * is never looked at are most of the answer when nothing was found.
 */

import {
  MAX_ROWS,
  NOT_CHECKED,
  rowLabel,
  sentences,
  type HiddenText,
  type Passage,
} from "./hiddentext";
import { placeholder } from "./panelrow";

export interface HiddenListOptions {
  /** Called when a row is pressed, with the passage it stands for. */
  onPick: (passage: Passage) => void;
}

export class HiddenList {
  private readonly notice: HTMLElement;
  private readonly list: HTMLElement;
  private readonly opts: HiddenListOptions;
  private readonly rows: HTMLElement[] = [];
  /** Index of the row pressed last, or -1. */
  private current = -1;
  private said: string[] = [];

  constructor(host: HTMLElement, opts: HiddenListOptions) {
    this.opts = opts;

    // A live region: the result arrives while the reader is waiting for it.
    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText = "flex:none;padding:0.3rem 0.7rem;opacity:0.8;display:none;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "group");
    this.list.setAttribute("aria-label", "Text the pages do not show");
    this.list.style.cssText = "flex:1;min-height:0;overflow-y:auto;";

    host.append(this.notice, this.list);
    this.setChecked(null);
  }

  /** Rows drawn. For the tests and the check harness. */
  get rowCount(): number {
    return this.rows.length;
  }

  /** The sentences above the rows, in order. For the tests and the check harness. */
  get status(): readonly string[] {
    return this.said;
  }

  /** Index of the row pressed last, or -1. */
  get picked(): number {
    return this.current;
  }

  /** A drawn row, so a test can press it. */
  rowAt(index: number): HTMLElement | null {
    return this.rows[index] ?? null;
  }

  /** What a drawn row displays, read back out of the DOM. */
  rowText(index: number): { label: string; words: string } {
    const [label, words] = [...(this.rows[index]?.children ?? [])] as HTMLElement[];
    return { label: label?.textContent ?? "", words: words?.textContent ?? "" };
  }

  /**
   * Shows a check's result, or with `null` that none has been run.
   *
   * `null` is the state of every document when it is opened: a result is
   * about the file that was checked, and is not kept for another.
   */
  setChecked(checked: HiddenText | null): void {
    this.list.replaceChildren();
    this.notice.replaceChildren();
    this.rows.length = 0;
    this.current = -1;
    this.said = checked ? sentences(checked) : [];
    this.notice.style.display = checked ? "block" : "none";
    if (!checked) {
      this.list.appendChild(placeholder(NOT_CHECKED));
      return;
    }
    for (const sentence of this.said) {
      const line = document.createElement("div");
      line.textContent = sentence;
      line.style.cssText = "margin:0.15rem 0;";
      this.notice.appendChild(line);
    }
    checked.found.slice(0, MAX_ROWS).forEach((passage, index) => {
      this.list.appendChild(this.row(passage, index));
    });
  }

  /** One row: where the passage is, then its words. */
  private row(passage: Passage, index: number): HTMLElement {
    const row = document.createElement("button");
    row.type = "button";
    row.dataset.index = String(index);
    row.style.cssText =
      "display:block;width:100%;box-sizing:border-box;text-align:left;font:inherit;" +
      "color:inherit;background:none;border:0;padding:0.3rem 0.7rem;cursor:default;";

    const label = document.createElement("div");
    label.textContent = rowLabel(passage);
    label.style.cssText = "opacity:0.6;font-size:0.85em;";

    const words = document.createElement("div");
    words.textContent = passage.text;
    words.style.cssText = "overflow-wrap:anywhere;";

    row.append(label, words);
    row.addEventListener("click", () => {
      this.mark(this.current, false);
      this.current = index;
      this.mark(index, true);
      this.opts.onPick(passage);
    });
    this.rows.push(row);
    return row;
  }

  private mark(index: number, on: boolean): void {
    const row = this.rows[index];
    if (!row) return;
    if (on) row.setAttribute("aria-current", "true");
    else row.removeAttribute("aria-current");
    row.style.background = on ? "color-mix(in srgb, currentColor 12%, transparent)" : "none";
  }
}
