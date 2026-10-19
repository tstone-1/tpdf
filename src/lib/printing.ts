/**
 * What the window says while Windows prints.
 *
 * On Windows tpdf rasterises every sheet itself before handing it to the
 * printer, and a dense vector page takes minutes, with nothing from the system
 * on screen meanwhile (`print_win.rs`). The backend reports each sheet; this
 * turns that into the toolbar's line. macOS and Linux send no such event: their
 * print systems show their own progress.
 */

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
  state: "printing" | "sent" | "failed";
}

/**
 * The toolbar's line for `progress`, or `null` to clear it.
 *
 * A failure clears the line: the reason is in the message Windows shows over
 * the window, and a line still saying "Printing" would contradict it.
 */
export function printingLine(progress: Printing): string | null {
  const { sent, of, state } = progress;
  if (state === "failed") return null;
  if (state === "sent") return of === 1 ? "Sent 1 page to the printer." : `Sent ${of} pages to the printer.`;
  return `Printing page ${Math.min(sent + 1, of)} of ${of}...`;
}
