/**
 * Reopening the tabs the reader had open, not only the last document.
 *
 * `session.rs` keeps the list; this decides what a launch does with it and
 * when the list is written. The opening itself is `App.svelte`'s, which owns
 * the handles.
 *
 * **Off until asked for.** A launch has always reopened the last document, and
 * that stays the default: every tab is an open document with a worker behind
 * it, and the tabs a reader left open are not a request to open them all each
 * morning. The list is written either way, so that turning the preference on,
 * or asking once for last time's tabs, has something to act on.
 *
 * **One tab is shown and the rest are opened behind it.** The tab the reader
 * was on opens first, by the same call a launch has always made, so the time
 * to the first page does not grow with the number of tabs. The others are
 * opened without a viewer and take their places in the strip afterwards.
 */

import { catalog } from "./i18n";
import { call } from "./ipc";
import type { Session, Sides } from "./session";

/** What a launch opens, and in what order the tabs end up. */
export interface LaunchPlan {
  /**
   * Opened one after the other, each shown as it opens. The last is the
   * document the reader is left looking at.
   */
  show: string[];
  /**
   * Whether {@link show} is the application's own idea rather than the
   * reader's. A document that no longer opens is then no error to report.
   */
  resuming: boolean;
  /** Opened without being shown, once {@link show} is on screen. */
  behind: string[];
  /** The tab order once every one of them is open. Empty leaves it alone. */
  order: string[];
  /**
   * The two sides the tabs were on, to be put back once they are open, or
   * null for one document in the window. See {@link sidesToRestore}.
   */
  sides: Sides | null;
}

/** The tabs a session recorded, when it recorded any. */
function recorded(session: Session): string[] {
  return session.tabs ?? [];
}

/**
 * What to open when the application starts.
 *
 * @param handed Files the launcher handed over: a double-click, a drop on the
 *               icon, an argument. They are what the reader asked for, so they
 *               are shown whatever else comes back.
 */
export function launchPlan(session: Session, handed: readonly string[]): LaunchPlan {
  const tabs = session.restore_tabs ? recorded(session) : [];
  if (handed.length) {
    return {
      show: [...handed],
      resuming: false,
      behind: tabs.filter((path) => !handed.includes(path)),
      // The reader's tabs where they were, and what they just asked for after.
      order: tabs.length ? [...tabs, ...handed.filter((path) => !tabs.includes(path))] : [],
      // What was handed over is what the reader came for, and it is not in
      // the record of the sides. It gets the whole window.
      sides: null,
    };
  }
  if (!tabs.length) {
    const last = session.places[0];
    return { show: last ? [last.path] : [], resuming: true, behind: [], order: [], sides: null };
  }
  const front = frontTab(session, tabs);
  return {
    show: [front],
    resuming: true,
    behind: tabs.filter((path) => path !== front),
    order: [...tabs],
    sides: session.sides ?? null,
  };
}

/**
 * The recorded sides, cut down to the tabs that opened.
 *
 * `open` is every open path in tab order and `active` the one the reader is
 * looking at. A tab that would not open is on neither side, and what is left
 * is a split only while both sides still have a tab: with the whole of one
 * side gone the window shows one document, which is what null says.
 */
export function sidesToRestore(
  sides: Sides | null,
  open: readonly string[],
  active: string | null,
): Sides | null {
  if (!sides || active === null || !open.includes(active)) return null;
  const right = open.filter((path) => sides.right.includes(path));
  if (right.length === 0 || right.length === open.length) return null;
  const beside = sides.beside !== null && open.includes(sides.beside)
    && right.includes(sides.beside) !== right.includes(active) ? sides.beside : null;
  return { right, beside, share: sides.share };
}

/**
 * The tab to show first: the one that was showing, or failing that the one
 * read most recently, or failing that the first.
 */
function frontTab(session: Session, tabs: readonly [string, ...string[]] | string[]): string {
  const active = session.active_tab;
  if (active && tabs.includes(active)) return active;
  const recent = session.places.find((place) => tabs.includes(place.path));
  return recent?.path ?? (tabs[0] as string);
}

/**
 * The tabs from last time that are not open now.
 *
 * `session` is the snapshot read at launch, so this is what the application
 * closed with and stays that for the whole run, however the tabs change. It
 * is what the one-off command reopens --- including a document on a volume
 * that was not mounted when the launch tried it.
 */
export function tabsToReopen(session: Session, isOpen: (path: string) => boolean): string[] {
  return recorded(session).filter((path) => !isOpen(path));
}

/** What opening tabs behind the one showing needs from the application. */
export interface TabHost {
  /** Opens `path` as a tab without showing it. Rejects when it will not open. */
  openBehind(path: string): Promise<void>;
  /** Puts the tabs in this order. */
  arrange(order: readonly string[]): void;
  /** Whether a document is on screen. */
  showing(): boolean;
  /** Shows the first tab, if there is one. */
  showFirst(): Promise<void>;
}

/**
 * Opens `behind` one at a time, and resolves with the ones that would not.
 *
 * One refusal does not stop the rest: a tab on a volume that is not mounted
 * says nothing about the tab beside it. If nothing is on screen afterwards ---
 * the tab that should have been shown was the one that failed --- the first
 * tab that did open is shown, so that a strip of tabs never sits over an empty
 * window.
 */
export async function openBehind(
  behind: readonly string[],
  order: readonly string[],
  host: TabHost,
): Promise<string[]> {
  const refused: string[] = [];
  for (const path of behind) {
    try {
      await host.openBehind(path);
    } catch {
      refused.push(path);
    }
  }
  if (order.length) host.arrange(order);
  if (!host.showing()) await host.showFirst();
  return refused;
}

/** What to tell a reader who asked for last time's tabs, or nothing. */
export function afterReopen(refused: readonly string[], name: (path: string) => string): string | null {
  if (!refused.length) return null;
  const first = name(refused[0] as string);
  return refused.length === 1 ? TEXT().one(first) : TEXT().many(first, refused.length - 1);
}

/**
 * The tabs from last time that a launch restoring them did not open: the
 * ones behind that `openBehind` refused, and the one to be shown first when
 * it is not open either. Only tabs of the recorded list, in `plan.order`:
 * a file handed over by the launcher reports its own failure, and the last
 * document resumed alone was never a list of tabs.
 */
export function unopenedAtLaunch(
  plan: LaunchPlan,
  refused: readonly string[],
  isOpen: (path: string) => boolean,
): string[] {
  const tried = [...(plan.resuming ? plan.show : []), ...refused];
  return [...new Set(tried.filter((path) => plan.order.includes(path) && !isOpen(path)))];
}

/** What to tell a reader whose tabs from last time did not all open at launch, or nothing. */
export function afterLaunch(unopened: readonly string[], name: (path: string) => string): string | null {
  if (!unopened.length) return null;
  const first = name(unopened[0] as string);
  return unopened.length === 1 ? TEXT().keptOne(first) : TEXT().keptMany(first, unopened.length - 1);
}

/**
 * The tab list to write: the open tabs, with `unopened` put back where they
 * stood in `order`, the list last time closed with.
 *
 * Each goes after the nearest tab before it in `order` that is in the list,
 * or first when there is none, so the list the next launch reads keeps last
 * time's order. One that is open now is already in `paths`.
 */
export function withUnopened(
  paths: readonly string[],
  unopened: readonly string[],
  order: readonly string[],
): string[] {
  const out = [...paths];
  for (const path of order) {
    if (!unopened.includes(path) || out.includes(path)) continue;
    const before = order.slice(0, order.indexOf(path)).reverse().find((other) => out.includes(other));
    out.splice(before === undefined ? 0 : out.indexOf(before) + 1, 0, path);
  }
  return out;
}

const TEXT = catalog({
  "en-US": {
    one: (first: string) => `Could not reopen ${first}. It may have been moved, renamed or deleted.`,
    many: (first: string, more: number) =>
      `Could not reopen ${first} and ${more} more. They may have been moved, renamed or deleted.`,
    keptOne: (first: string) =>
      `Could not reopen ${first}. It stays with the tabs from last time and is tried again at the next start.`,
    keptMany: (first: string, more: number) =>
      `Could not reopen ${first} and ${more} more. They stay with the tabs from last time and are tried again ` +
      "at the next start.",
  },
  "de-DE": {
    one: (first: string) =>
      `${first} konnte nicht wieder geöffnet werden. Die Datei wurde vielleicht verschoben, umbenannt oder gelöscht.`,
    many: (first: string, more: number) =>
      `${first} und ${more} ${more === 1 ? "weiteres Dokument" : "weitere Dokumente"} konnten nicht wieder geöffnet werden. ` +
      "Sie wurden vielleicht verschoben, umbenannt oder gelöscht.",
    keptOne: (first: string) =>
      `${first} konnte nicht wieder geöffnet werden. Das Dokument bleibt bei den Tabs vom letzten Mal und wird ` +
      "beim nächsten Start erneut versucht.",
    keptMany: (first: string, more: number) =>
      `${first} und ${more} ${more === 1 ? "weiteres Dokument" : "weitere Dokumente"} konnten nicht wieder geöffnet werden. ` +
      "Die Dokumente bleiben bei den Tabs vom letzten Mal und werden beim nächsten Start erneut versucht.",
  },
});

/**
 * Writes the tab list when it changes.
 *
 * Chained for `SessionWriter`'s reason: `invoke` resolves out of order under
 * load, and a list written out of order records a tab that has been closed.
 * Not throttled, because a tab opens or closes when a reader does something,
 * not on every frame.
 */
export class TabRecorder {
  private last: string | null = null;
  private waiting: [string[], string | null, Sides | null] | null = null;
  private held = false;
  private queue: Promise<unknown> = Promise.resolve();
  /** Tabs from last time that did not open, and that order; see {@link keepUnopened}. */
  private unopened: string[] = [];
  private order: string[] = [];

  constructor(
    private readonly send: (
      paths: string[],
      active: string | null,
      sides: Sides | null,
    ) => Promise<unknown> = (paths, active, sides) => call("session_set_tabs", { paths, active, sides }),
  ) {}

  /**
   * Records the tabs as they are now, and the two sides they are on when the
   * window shows two. The same record twice is one write.
   */
  note(paths: readonly string[], active: string | null, sides: Sides | null = null): void {
    if (this.held) {
      this.waiting = [[...paths], active, sides && { ...sides, right: [...sides.right] }];
      return;
    }
    // One that has since opened is a tab like any other, and goes when it is closed.
    this.unopened = this.unopened.filter((path) => !paths.includes(path));
    const list = withUnopened(paths, this.unopened, this.order);
    const key = JSON.stringify([list, active, sides]);
    if (key === this.last) return;
    this.last = key;
    const kept = sides && { ...sides, right: [...sides.right] };
    const write = () => this.send(list, active, kept);
    // A failed write must not stop the ones after it, so the tail never rejects.
    this.queue = this.queue.then(write).catch((why: unknown) => {
      // Forgotten as well as logged. The session does not hold this record,
      // and remembering it as written would skip the same record noted again,
      // which is the write that would put it there. Only while it is still
      // the newest: a later record has replaced what this one would have said.
      if (this.last === key) this.last = null;
      console.warn(`could not record the open tabs: ${String(why)}`);
    });
  }

  /**
   * Stops writing until {@link release}.
   *
   * For the launch restore, during which the strip holds one tab, then two,
   * then three. Written as they arrive, a reader who quit halfway through
   * would find the tabs that had not opened yet gone from the next launch.
   */
  hold(): void {
    this.held = true;
  }

  /**
   * Keeps `unopened`, tabs from last time that this launch could not open, in
   * every list written from now on, at their places in `order`.
   *
   * A tab on a volume not mounted yet, or behind a password a launch does not
   * ask for, would otherwise be written out of the list by the first record
   * after the restore, and the next launch would not try it. Kept for the run;
   * one that opens later is an ordinary tab from then on.
   */
  keepUnopened(unopened: readonly string[], order: readonly string[]): void {
    this.unopened = [...unopened];
    this.order = [...order];
  }

  /** Writes whatever was noted while held, and writes as it goes from now on. */
  release(): void {
    this.held = false;
    const waiting = this.waiting;
    this.waiting = null;
    if (waiting) this.note(waiting[0], waiting[1], waiting[2]);
  }

  /** Resolves once every write issued so far has been answered. Never rejects. */
  settled(): Promise<void> {
    return this.queue.then(() => undefined, () => undefined);
  }
}
