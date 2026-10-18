/**
 * The sidebar tab listing what changed in the words between two documents:
 * the last *Compare with*, one row a change.
 *
 * What the rows say and what stands above them is `comparison.ts`; this draws
 * it. A row is a real button, so it is reached with Tab and pressed with Enter
 * or Space by the platform, as `hiddenlist.ts`'s rows are. The words a change
 * removed are in a `<del>` and the words it added in an `<ins>`, so a screen
 * reader that announces either says which is which, and the colours match what
 * the two pages are painted with.
 *
 * **An empty result is not an empty panel.** The sentences are shown whenever a
 * comparison has been made, with or without rows: "the same words" is only
 * half an answer without the sentence saying nothing else was compared.
 */

import {
  MAX_ROWS,
  NOT_COMPARED,
  OUT_OF_DATE,
  rowLabel,
  sentences,
  type Change,
  type Compared,
} from "./comparison";
import { placeholder } from "./panelrow";

/** The wash behind removed words: the left page's paint, `REMOVED_FILL` in `viewer.ts`. */
const REMOVED_BACKGROUND = "rgba(230, 60, 60, 0.28)";
/** And behind added words: `ADDED_FILL`. */
const ADDED_BACKGROUND = "rgba(40, 170, 70, 0.28)";

export interface ChangeListOptions {
  /** Called when a row is pressed, with the index of the change it stands for. */
  onPick: (index: number) => void;
}

export class ChangeList {
  private readonly notice: HTMLElement;
  private readonly list: HTMLElement;
  private readonly opts: ChangeListOptions;
  private readonly rows: HTMLElement[] = [];
  /** Index of the current row, or -1. */
  private current = -1;
  private said: string[] = [];

  constructor(host: HTMLElement, opts: ChangeListOptions) {
    this.opts = opts;

    // A live region: the result arrives while the reader is waiting for it.
    this.notice = document.createElement("div");
    this.notice.setAttribute("role", "status");
    this.notice.style.cssText = "flex:none;padding:0.3rem 0.7rem;opacity:0.8;display:none;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "group");
    this.list.setAttribute("aria-label", "Changes");
    this.list.style.cssText = "flex:1;min-height:0;overflow-y:auto;";

    host.append(this.notice, this.list);
    this.setCompared(null);
  }

  /** Rows drawn. For the tests and the check harness. */
  get rowCount(): number {
    return this.rows.length;
  }

  /** The sentences above the rows, in order. For the tests and the check harness. */
  get status(): readonly string[] {
    return this.said;
  }

  /** Index of the current row, or -1. */
  get picked(): number {
    return this.current;
  }

  /** A drawn row, so a test can press it. */
  rowAt(index: number): HTMLElement | null {
    return this.rows[index] ?? null;
  }

  /**
   * Shows a comparison, or with `null` that none has been made.
   *
   * `null` is the state of every document when it is opened: a comparison is
   * about two documents as they were, and is not kept for another.
   */
  setCompared(compared: Compared | null): void {
    this.list.replaceChildren();
    this.notice.replaceChildren();
    this.rows.length = 0;
    this.current = -1;
    this.said = compared ? sentences(compared) : [];
    this.notice.style.display = compared ? "block" : "none";
    if (!compared) {
      this.list.appendChild(placeholder(NOT_COMPARED));
      return;
    }
    this.sayAll();
    compared.result.changes.slice(0, MAX_ROWS).forEach((change, index) => {
      this.list.appendChild(this.row(change, index));
    });
  }

  /**
   * Says the comparison no longer describes the documents, and keeps its rows.
   *
   * Kept rather than cleared: a reader part-way down the list has the changes
   * they had not reached yet in it, and they are still what the documents
   * differed by when compared. Pressing one still goes to its page.
   */
  setOutOfDate(): void {
    if (this.said.length === 0 || this.said[0] === OUT_OF_DATE) return;
    this.said = [OUT_OF_DATE, ...this.said];
    this.sayAll();
  }

  /** Marks row `index` as the current one, and keeps it in view. */
  setCurrent(index: number): void {
    this.mark(this.current, false);
    this.current = index;
    this.mark(index, true);
    this.rows[index]?.scrollIntoView?.({ block: "nearest" });
  }

  private sayAll(): void {
    this.notice.replaceChildren();
    for (const sentence of this.said) {
      const line = document.createElement("div");
      line.textContent = sentence;
      line.style.cssText = "margin:0.15rem 0;";
      this.notice.appendChild(line);
    }
  }

  /** One row: where the change is, then its words with a few either side. */
  private row(change: Change, index: number): HTMLElement {
    const row = document.createElement("button");
    row.type = "button";
    row.dataset.index = String(index);
    row.style.cssText =
      "display:block;width:100%;box-sizing:border-box;text-align:left;font:inherit;" +
      "color:inherit;background:none;border:0;padding:0.3rem 0.7rem;cursor:default;";

    const label = document.createElement("div");
    label.textContent = rowLabel(change);
    label.style.cssText = "opacity:0.6;font-size:0.85em;";

    const words = document.createElement("div");
    words.style.cssText = "overflow-wrap:anywhere;";
    const plain = (text: string) => {
      if (text) words.append(document.createTextNode(text));
    };
    const marked = (tag: "del" | "ins", text: string, background: string) => {
      if (!text) return;
      // Two literal element names rather than the variable: the sinks gate
      // cannot vouch for a computed one.
      const element = tag === "del" ? document.createElement("del") : document.createElement("ins");
      element.textContent = text;
      element.style.cssText = `background:${background};border-radius:2px;padding:0 1px;`;
      words.append(element);
    };
    plain(change.before ? `${change.before} ` : "");
    marked("del", change.removed, REMOVED_BACKGROUND);
    if (change.removed && change.added) plain(" ");
    marked("ins", change.added, ADDED_BACKGROUND);
    plain(change.after ? ` ${change.after}` : "");

    row.append(label, words);
    row.addEventListener("click", () => {
      this.setCurrent(index);
      this.opts.onPick(index);
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
