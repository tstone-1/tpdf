/**
 * The check for text a document's pages do not show, from the window: the line
 * it shows while it compares, what it says when it is done, and where a
 * passage it found is.
 *
 * `App.svelte` keeps the `invoke` and the Stop button; what is decided is here,
 * so it has tests. The comparison is `src-tauri/src/hidden.rs`, the walk over
 * the pages and the sentence it ends on are `hidden/survey.rs`, and both are
 * `tpdf hidden`'s too: the window is sent the sentence and words none of it
 * again. The list is `hiddenlist.ts`.
 *
 * **The answer is one-sided.** A listed passage is in the file. When nothing
 * is listed, nothing was *found*, and {@link sentences} always says what was
 * not compared and what the check never looks at. Nothing here may shorten
 * that to "clean".
 */

/**
 * The event that says which page is being compared.
 *
 * `PROGRESS_EVENT` in `commands/hidden.rs`; `hiddentext.test.ts` holds the two
 * spellings together.
 */
export const PROGRESS_EVENT = "tpdf://hidden-progress";

/** The page being compared, counted from 1, and how many there are. */
export interface Progress {
  page: number;
  of: number;
}

/** One passage a page does not show. `Passage` in `hidden/survey.rs`. */
export interface Passage {
  /** The page of the **file** it is on, counted from 1. */
  page: number;
  /** The words, in the order the page's text has them. */
  text: string;
  /**
   * `[left, top, right, bottom]` in points from the top-left corner of the
   * page as the file displays it: a link's frame, before the reader's crop or
   * turn.
   */
  rect: [number, number, number, number];
  /** How many of its characters were judged hidden. */
  characters: number;
  /** Whether the words lie outside the page altogether. */
  offPage: boolean;
}

/** What a finished check answers. `HiddenText` in `commands/hidden.rs`. */
export interface HiddenText {
  found: Passage[];
  /** `tpdf hidden`'s last line: what was found and what was not compared. */
  summary: string;
  /** What the check never looks at, in the backend's words. */
  notLookedAt: string;
  /** Whether the document had changes that are not saved, which were not checked. */
  unsaved: boolean;
}

/** The line shown before the first page has been reported. */
export const STARTING = "Comparing text with the pages...";

/** The line shown while a page is being compared. */
export function progressLine(at: Progress): string {
  return `Comparing text with the pages: page ${at.page} of ${at.of}...`;
}

/** What the panel says before any check has been run on the open document. */
export const NOT_CHECKED =
  "Not checked yet. “Find text the pages do not show” compares the text in this " +
  "document with what its pages look like.";

/** What a result says when the document had unsaved changes. */
export const UNSAVED = "The saved file was checked. Your unsaved changes were not part of it.";

/** What a passage's row is told when its page has since been deleted. */
export const PAGE_GONE = "That page is no longer in the document.";

/** Rows drawn at once. Beyond this the panel says how many it is not showing. */
export const MAX_ROWS = 500;

/**
 * What the panel says above its rows, a sentence to a line.
 *
 * The backend's summary first, which carries what was not compared; then the
 * cap, when rows were left out; then that unsaved changes were not checked;
 * and last, always, what the check never looks at. An empty result gets all of
 * it, which is the case it is for.
 */
export function sentences(checked: HiddenText): string[] {
  const said = [checked.summary];
  if (checked.found.length > MAX_ROWS) {
    said.push(`Showing the first ${MAX_ROWS} of ${checked.found.length} passages.`);
  }
  if (checked.unsaved) said.push(UNSAVED);
  said.push(checked.notLookedAt);
  return said;
}

/** What a row says before the words: `Page 4`, or `Page 4, outside the page`. */
export function rowLabel(passage: Passage): string {
  return passage.offPage ? `Page ${passage.page}, outside the page` : `Page ${passage.page}`;
}

/**
 * Where a passage is in the document the reader is looking at.
 *
 * `slotOf` is the page map's: a page of the file to the slot showing it, or
 * `undefined` once the reader has deleted that page. A passage outside the
 * page has no rectangle a reader could be shown --- it is past the edge of
 * what is drawn --- so its answer is the page alone.
 */
export function placeOf(
  passage: Passage,
  slotOf: (filePage: number) => number | undefined,
): { slot: number; rect: [number, number, number, number] | null } | null {
  const slot = slotOf(passage.page - 1);
  if (slot === undefined) return null;
  return { slot, rect: passage.offPage ? null : passage.rect };
}

/**
 * Goes to a passage: `ring` for one with a place on its page, `page` for one
 * outside the page, which has its page and nothing on it to ring, and `gone`
 * for one whose page the reader has since deleted. Exactly one is called.
 */
export function showPassage(
  passage: Passage,
  slotOf: (filePage: number) => number | undefined,
  on: {
    ring(slot: number, rect: [number, number, number, number]): void;
    page(slot: number): void;
    gone(): void;
  },
): void {
  const place = placeOf(passage, slotOf);
  if (!place) on.gone();
  else if (place.rect) on.ring(place.slot, place.rect);
  else on.page(place.slot);
}

/** What the window supplies to one check. The order they are used in is {@link Runs.check}'s. */
export interface CheckHost {
  /** Shows that a check has begun, and waits until the window has drawn that. */
  begun(): Promise<void>;
  /** Sends the check, as the run numbered `run`. */
  ask(run: number): Promise<HiddenText>;
  /** Whether the document that was asked about is still the one in the window. */
  current(): boolean;
  /** Puts the answer in front of the reader. */
  show(checked: HiddenText): void;
}

/**
 * Which check Stop is for, and whether one is running.
 *
 * A run's number and not a flag, for `Cancel`'s reason in `commands/ocr.rs`:
 * Stop is offered before the command has been sent, so a stop can arrive first,
 * and a number needs no clearing. Counted from 1, because 0 is the backend's
 * "nothing was asked to stop".
 */
export class Runs {
  private last = 0;
  private open = false;

  /** Begins a run and returns its number. */
  start(): number {
    this.last += 1;
    this.open = true;
    return this.last;
  }

  /** Ends the run. */
  finish(): void {
    this.open = false;
  }

  get running(): boolean {
    return this.open;
  }

  /** The number of the run begun last. */
  get number(): number {
    return this.last;
  }

  /**
   * The line for a progress event, or null when no check is running.
   *
   * Guarded, because an event can land after the command has answered and
   * would otherwise leave its line on screen with nothing running.
   */
  line(at: Progress): string | null {
    return this.open ? progressLine(at) : null;
  }

  /** Hands `show` the line for a progress event, and nothing when no check is running. */
  report(at: Progress, show: (line: string) => void): void {
    const line = this.line(at);
    if (line !== null) show(line);
  }

  /** Stop: hands `send` the number of the run begun last, which is the one on screen. */
  stop(send: (run: number) => unknown): void {
    send(this.last);
  }

  /**
   * One check, from its number to its answer.
   *
   * Numbered before the window shows that it has begun, so a Stop pressed at
   * any moment names this run. The answer is shown only when the document it
   * is about is still the one in the window: it is dropped, not shown under
   * another document's name. The run is ended whatever happened, a refusal
   * included, which is passed on to the caller.
   */
  async check(host: CheckHost): Promise<void> {
    const run = this.start();
    try {
      await host.begun();
      const checked = await host.ask(run);
      if (!host.current()) return;
      host.show(checked);
    } finally {
      this.finish();
    }
  }
}
