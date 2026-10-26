/**
 * What the window says while Windows prints.
 *
 * On Windows tpdf rasterises every sheet itself before handing it to the
 * printer, and a dense vector page takes minutes, with nothing from the system
 * on screen meanwhile (`print_win.rs`). The backend reports each sheet; this
 * turns that into the toolbar's line. macOS and Linux send no such event: their
 * print systems show their own progress.
 */

import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    stoppedNothingSent: "Printing stopped. Nothing was sent to the printer.",
    stoppedSomeSent: "Printing stopped. The printer may still print the sheets it already had.",
    sent: (sheets: number) => (sheets === 1 ? "Sent 1 sheet to the printer." : `Sent ${sheets} sheets to the printer.`),
    printing: (sheet: number, of: number) => `Printing sheet ${sheet} of ${of}...`,
  },
  "de-DE": {
    stoppedNothingSent: "Drucken abgebrochen. Es wurde nichts an den Drucker gesendet.",
    stoppedSomeSent: "Drucken abgebrochen. Der Drucker druckt möglicherweise noch die Blätter, die er schon erhalten hat.",
    sent: (sheets: number) =>
      sheets === 1 ? "1 Blatt an den Drucker gesendet." : `${sheets} Blätter an den Drucker gesendet.`,
    printing: (sheet: number, of: number) => `Blatt ${sheet} von ${of} wird gedruckt...`,
  },
});

/**
 * The event that says how far a print has got.
 *
 * `PROGRESS_EVENT` in `commands/print.rs`; `printing.test.ts` holds the two
 * spellings together.
 */
export const PROGRESS_EVENT = "tpdf://print-progress";

/** What the event carries. `Printing` in `commands/print.rs`. */
export interface Printing {
  /** Sheets handed to the printer so far. */
  sent: number;
  /** Sheets in the job. */
  of: number;
  /** `stopped` is the reader's Stop, after which nothing more is sent. */
  state: "printing" | "sent" | "failed" | "stopped";
}

/**
 * The toolbar's line for `progress`, or `null` to clear it.
 *
 * Counted in sheets of paper, not pages: with several pages to a sheet, or
 * a poster, the two differ, and the sheet is what the printer is working on.
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
