/**
 * What the window shows when no document is open: the documents read lately.
 *
 * The window with nothing in it said "Open a PDF, or drop one here." and left
 * the reader to the file dialog, for a file the application already knew. The
 * palette has offered those documents since `recents.ts`; this is the same list
 * where a reader who has not typed anything can see it.
 *
 * ## One list, and the palette's
 *
 * A row is not a second way to open a document. {@link rowsFor} decides which
 * documents are recent and what each is called, {@link recentCommands} builds
 * the palette's `file.recent.N` commands **from those rows**, and a row that is
 * clicked runs the command it names through the registry. So the page and the
 * palette cannot disagree about which documents are recent, in what order, or
 * what opening one does: there is one list and one `run`.
 *
 * ## What is deliberately not here
 *
 * - **No thumbnails.** A picture of the first page means opening the file, and
 *   every PDF is hostile input: the page would hand eight documents nobody
 *   asked for to a renderer on every launch, and would stall on a volume that
 *   is not mounted.
 * - **No timestamps.** The session stores where a document was left and not
 *   when. The order is the only thing it knows about time, and the order is
 *   shown.
 * - **No pinning.** It would be a second list with its own order, kept
 *   somewhere, and the reader has a file manager.
 * - **No check that a file exists.** One filesystem call per row at launch, on
 *   paths that may be on a disconnected share, to pre-empt a message the open
 *   already gives correctly. A row learns that its document would not open
 *   when the reader tries it, and says so from then on: {@link StartPage.failed}.
 */

import type { Command } from "./commands";
import { isOpenRefusal } from "./ipc";
import { labelsFor, MAX_RECENTS, recentCommandId } from "./recents";
import type { Place, Session, SessionWriter } from "./session";

/** One remembered document, as the page shows it. */
export interface StartRow {
  /** The document. Shown whole as the row's tooltip, and the key of the list. */
  path: string;
  /** Its name, lengthened where two rows would read the same. */
  label: string;
  /** The folder it is in, with home written `~` where that is honest. */
  folder: string;
  /** "page 12" when the reader left it past the first page, else empty. */
  page: string;
  /** The registry command that opens it. */
  command: string;
  /** What went wrong the last time it was tried, or empty. */
  trouble: string;
  /** The accessible name of the control that takes it off the list. */
  removing: string;
}

/** What a row says once its document has refused to open. */
export const UNOPENED = "could not be opened";

/**
 * The folder `path` is in, as it was written.
 *
 * Both separators, for `recents.ts`'s reason: a session file is read by
 * whichever machine has it, and records paths from whichever machine wrote
 * them. Cut at the last separator of either kind rather than split and joined,
 * so a path that mixes the two comes back with the ones it had.
 */
export function folderOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (cut < 0) return "";
  const folder = path.slice(0, cut);
  // The root is its separator: `/report.pdf` is in `/`, and `C:\report.pdf` is
  // in `C:\`, where `C:` alone would name the drive's current directory.
  if (folder === "" || /^[A-Za-z]:$/.test(folder)) return path.slice(0, cut + 1);
  return folder;
}

/** The most characters of a folder a row shows. */
export const FOLDER_CHARS = 48;

/**
 * The end of a folder too long for its row, after an ellipsis.
 *
 * A folder's end is what tells two documents apart, and a row cut by the
 * window loses exactly that. Cut here and by characters, since a line cut from
 * its start by the page's own layout drew its ellipsis over the text. The cut
 * is at a separator where one is near, so the first folder shown is a whole
 * name; the full path is the row's tooltip.
 */
export function tailOf(folder: string): string {
  if (folder.length <= FOLDER_CHARS) return folder;
  const tail = folder.slice(folder.length - (FOLDER_CHARS - 1));
  const at = tail.search(/[\\/]/);
  return `…${at >= 0 && at <= 16 ? tail.slice(at) : tail}`;
}

/**
 * `folder` with the reader's home written as `~`.
 *
 * Only for a home that is a POSIX path. `~` is what a shell and a file dialog
 * on macOS and Linux both accept, so it is a spelling the reader can use. On
 * Windows nothing a reader types into Explorer expands it, and a folder shown
 * as `~\Documents` would be one they cannot paste anywhere, so a Windows home
 * leaves the folder as written. That is also what happens to a path recorded
 * on another machine: it is not under this home, whatever it looks like.
 *
 * `home` is the platform's answer, carried by the launch's `session_load`; no
 * part of this guesses it from the paths.
 */
export function shortenHome(folder: string, home: string | null | undefined): string {
  if (!home || !home.startsWith("/")) return folder;
  const base = home.replace(/\/+$/, "");
  // Home being the root would turn every absolute path into `~/...`.
  if (base === "") return folder;
  if (folder === base) return "~";
  // The separator is part of the test: `/Users/reader-old` is not under
  // `/Users/reader`.
  return folder.startsWith(`${base}/`) ? `~${folder.slice(base.length)}` : folder;
}

/** "page N", counted from 1, when the place is past the first page. */
export function pageText(place: Pick<Place, "page">): string {
  return place.page > 0 ? `page ${place.page + 1}` : "";
}

/**
 * The rows for `places`, newest first: the documents the palette offers.
 *
 * @param unopened Paths that refused to open when last tried.
 */
export function rowsFor(
  places: readonly Place[],
  home: string | null | undefined,
  unopened: ReadonlySet<string> = new Set(),
): StartRow[] {
  const offered = places.slice(0, MAX_RECENTS);
  const labels = labelsFor(offered.map((place) => place.path));
  return offered.map((place, index) => {
    const label = labels[index] ?? place.path;
    return {
      path: place.path,
      label,
      folder: tailOf(shortenHome(folderOf(place.path), home)),
      page: pageText(place),
      command: recentCommandId(index),
      trouble: unopened.has(place.path) ? UNOPENED : "",
      removing: `Remove ${label} from this list`,
    };
  });
}

/**
 * The palette's recent-document commands, one per row.
 *
 * Built from the rows and from nothing else, which is what makes a row and its
 * command the same document under the same name.
 */
export function recentCommands(
  rows: readonly StartRow[],
  open: (path: string) => void,
): Command[] {
  return rows.map((row) => ({
    id: row.command,
    // Prefixed with the verb so the row reads as a command next to "Zoom in"
    // rather than as a stray filename. Ranking is subsequence matching, so
    // typing part of the name still finds it.
    title: `Open ${row.label}`,
    run: () => open(row.path),
  }));
}

/** The part of a key event the page reads. */
export type StartKey = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

/** What a key pressed on the page asks for. */
export type StartMove = { focus: number } | { remove: number } | null;

/**
 * What a key means with row `current` focused, or none when it is negative.
 *
 * Nothing is focused when the window opens, so that its own keys work as they
 * did; the first Down is what enters the list. Up, Home, End and the two
 * removing keys mean something only once a row has the focus --- with none,
 * they are not this page's keys, and answering them would take Backspace and
 * Home away from whatever the reader was doing.
 *
 * The ends stop rather than wrap. A list of eight is short enough to see whole,
 * and a Down that lands on the first row again reads as nothing having moved.
 */
export function startMove(event: StartKey, current: number, count: number): StartMove {
  // A chord is somebody else's: ⌘Backspace and Shift+Home are not requests to
  // forget a document or to walk a list.
  if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return null;
  if (count <= 0) return null;
  if (current < 0) return event.key === "ArrowDown" ? { focus: 0 } : null;
  const last = count - 1;
  const at = Math.min(current, last);
  switch (event.key) {
    case "ArrowDown": return { focus: Math.min(at + 1, last) };
    case "ArrowUp": return { focus: Math.max(at - 1, 0) };
    case "Home": return { focus: 0 };
    case "End": return { focus: last };
    case "Delete":
    case "Backspace": return { remove: at };
    default: return null;
  }
}

/**
 * The row to focus once the one at `index` has gone, or -1 when none is left.
 *
 * The row that took its place, so that pressing Delete again removes the next
 * one down; from the last row, the one above it, which for the only row is
 * none.
 */
export function focusAfterRemoval(index: number, remaining: number): number {
  return Math.min(index, remaining - 1);
}

/**
 * Changes or reads the remembered list behind every place on its way there.
 *
 * A place is written through `SessionWriter`'s chain and this is not, so the
 * two can land in either order. A document closed a moment ago still has its
 * last place outstanding; forgetting it first and recording it second puts the
 * row back a second after the reader removed it, and reading the list first
 * leaves the document just closed off the top of it.
 *
 * @returns what to tell the reader when the change was refused, or null.
 */
export async function behindWrites(
  writer: Pick<SessionWriter, "flush" | "settled">,
  change: () => Promise<unknown>,
): Promise<string | null> {
  writer.flush();
  await writer.settled();
  try {
    await change();
    return null;
  } catch (e) {
    return String(e);
  }
}

/**
 * The page's state: which documents, where home is, and which would not open.
 *
 * A class with unit tests rather than three variables in `App.svelte`, which
 * is the layer no gate reaches.
 */
export class StartPage {
  #places: readonly Place[] = [];
  #home: string | null = null;
  readonly #unopened = new Set<string>();

  /** @param changed Told the rows whenever they are not what they were. */
  constructor(private readonly changed: (rows: StartRow[]) => void = () => {}) {}

  /** The rows as they are now. */
  get rows(): StartRow[] {
    return rowsFor(this.#places, this.#home, this.#unopened);
  }

  /**
   * Takes a session as the list.
   *
   * A document that is no longer in it has nothing left to be marked on, so
   * its mark goes too --- otherwise a file that failed once, was forgotten and
   * was later read successfully from somewhere else would come back marked.
   */
  offer(session: Pick<Session, "places" | "home">): StartRow[] {
    this.#places = session.places;
    this.#home = session.home ?? null;
    const listed = new Set(session.places.map((place) => place.path));
    for (const path of [...this.#unopened]) {
      if (!listed.has(path)) this.#unopened.delete(path);
    }
    return this.#tell();
  }

  /**
   * Records that `path` would not open, with what the open threw.
   *
   * A document behind a password whose reader declined to give one is not a
   * document that is gone, and is not marked: on the flag, never on the
   * wording, as `unlock.ts` decides the prompt.
   */
  failed(path: string, why: unknown): void {
    if (isOpenRefusal(why) && why.locked) return;
    this.#unopened.add(path);
    this.#tell();
  }

  /** Records that `path` opened, so it is not marked any more. */
  opened(path: string): void {
    if (this.#unopened.delete(path)) this.#tell();
  }

  #tell(): StartRow[] {
    const rows = this.rows;
    this.changed(rows);
    return rows;
  }
}
