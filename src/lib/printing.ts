/**
 * What the window says while Windows prints.
 *
 * On Windows tpdf rasterises every side itself before handing it to the
 * printer, and a dense vector page takes minutes, with nothing from the system
 * on screen meanwhile (`print_win.rs`). The backend reports each side; this
 * turns that into the toolbar's line. macOS and Linux send no such event: their
 * print systems show their own progress.
 */

import { catalog, UI_LOCALE, type UiLocale } from "./i18n";
import { spoken } from "./said";

const TEXT = catalog({
  "en-US": {
    failed: "The document was not printed.",
    stoppedNothingSent: "Printing stopped. Nothing was sent to the printer.",
    stoppedSomeSent: "Printing stopped. The printer may still print the sides it already had.",
    sent: (sides: number) => (sides === 1 ? "Sent 1 side to the printer." : `Sent ${sides} sides to the printer.`),
    printing: (side: number, of: number) => `Printing side ${side} of ${of}...`,
  },
  "de-DE": {
    failed: "Das Dokument wurde nicht gedruckt.",
    stoppedNothingSent: "Drucken abgebrochen. Es wurde nichts an den Drucker gesendet.",
    stoppedSomeSent: "Drucken abgebrochen. Der Drucker druckt möglicherweise noch die Druckseiten, die er schon erhalten hat.",
    sent: (sides: number) =>
      sides === 1 ? "1 Druckseite an den Drucker gesendet." : `${sides} Druckseiten an den Drucker gesendet.`,
    printing: (side: number, of: number) => `Druckseite ${side} von ${of} wird gedruckt...`,
  },
});

/**
 * The event that says how far a print has got.
 *
 * `PROGRESS_EVENT` in `commands/print.rs`; `printing.test.ts` holds the two
 * spellings together.
 */
export const PROGRESS_EVENT = "tpdf://print-progress";

/** A print operation failed after the platform accepted the request. */
export const FAILURE_EVENT = "tpdf://print-failure";

export function printFailure(detail: string, locale: UiLocale = UI_LOCALE): string {
  return `${TEXT(locale).failed}\n\n${spoken(detail, locale)}`;
}

/** What the event carries. `Printing` in `commands/print.rs`. */
export interface Printing {
  /** Sides handed to the printer so far. */
  sent: number;
  /** Sides in the job. */
  of: number;
  /** `stopped` is the reader's Stop, after which nothing more is sent. */
  state: "printing" | "sent" | "failed" | "stopped";
}

/**
 * The toolbar's line for `progress`, or `null` to clear it.
 *
 * Counts printed sides. Duplex printing can put two sides on one sheet of paper.
 *
 * A failure clears the line: the reason is in the message Windows shows over
 * the window, and a line still saying "Printing" would contradict it.
 */
export function printingLine(progress: Printing): string | null {
  const { sent, of, state } = progress;
  if (state === "failed") return null;
  if (state === "stopped") {
    return sent === 0 ? TEXT().stoppedNothingSent : TEXT().stoppedSomeSent;
  }
  if (state === "sent") return TEXT().sent(of);
  return TEXT().printing(Math.min(sent + 1, of), of);
}

/** Whether `progress` is of a print still being drawn, which Stop can stop. */
export function stoppable(progress: Printing): boolean {
  return progress.state === "printing";
}
