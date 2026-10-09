import type { DocumentInfo } from "./ipc";
import type { Edits } from "./edits";
import type { Place } from "./session";
import type { Tab } from "./sidebar";
import { PLAIN_SEARCH, type SearchOptions, type ScopeRange } from "./search";
import type { Offer } from "./recovery";
import type { ViewId } from "./views";

/** State kept while a document has no mounted viewer. Backend handles stay open. */
export interface DocumentTab {
  /**
   * What this tab is named by. The document's handle for the first view of a
   * document, which is every tab unless a document is shown twice (`views.ts`).
   */
  view: ViewId;
  doc: DocumentInfo;
  path: string;
  edits: Edits;
  place: Place | null;
  covered: Map<number, string>;
  query: string;
  findShown: boolean;
  searchOptions: SearchOptions;
  searchScope: readonly ScopeRange[] | null;
  sidebarTab: Tab;
  error: string | null;
  offers: Offer[];
  notice: string | null;
  redactedCopyPath: string | null;
}

/**
 * Everything a tab keeps beyond which file it is: what {@link keepState}
 * writes when the reader switches away and what `openDocument` reads back.
 *
 * **Typed here so a field cannot be kept on one side only.** Both halves used
 * to be written out in `App.svelte`, the one file no gate reaches: an
 * `Object.assign` of a literal on the way out, and nine `retained?.x ?? default`
 * reads spread through `openDocument` on the way back. A field added to
 * {@link DocumentTab} and to one of them compiled, and leaked across tabs.
 * Now `keepState` takes a whole {@link TabState} and {@link freshState} is a
 * whole {@link FreshState}, so a new field is a compile error in both until it
 * is written in both.
 */
export type TabState = Omit<DocumentTab, "view" | "doc" | "path">;

/**
 * The part of a tab that has a value before the reader has done anything.
 *
 * `edits` and `place` are not in it: the first is a model built for the file
 * and the second is the remembered or clamped place, and both are computed by
 * the opener rather than defaulted.
 */
export type FreshState = Omit<TabState, "edits" | "place">;

/** The state of a document nobody has touched yet. A new `Map` and array each call. */
export function freshState(): FreshState {
  return {
    covered: new Map(),
    query: "",
    findShown: false,
    searchOptions: PLAIN_SEARCH,
    searchScope: null,
    sidebarTab: "outline",
    error: null,
    offers: [],
    notice: null,
    redactedCopyPath: null,
  };
}

/** Records what the reader leaves behind when they switch away from `tab`. */
export function keepState(tab: DocumentTab, state: TabState): void {
  Object.assign(tab, state);
}

/**
 * What a tab being reopened restores, or {@link freshState} for a document
 * that was never kept.
 *
 * Applied at three points of `openDocument` rather than at one, because the
 * order is load-bearing there: the status line before the viewer mounts, the
 * covered words beside the model, the search and sidebar tab after the panels
 * exist. What this owns is that every one of them comes from the same record;
 * {@link restore} owns that every field of it is applied.
 */
export function restoredState(tab: DocumentTab | undefined): FreshState {
  if (!tab) return freshState();
  const { covered, query, findShown, searchOptions, searchScope, sidebarTab, error, offers, notice,
    redactedCopyPath } = tab;
  return { covered, query, findShown, searchOptions, searchScope, sidebarTab, error, offers, notice,
    redactedCopyPath };
}

/**
 * The three points of an open at which restored state can be applied, in the
 * order an open reaches them: before the viewer exists, when the document's
 * model is in place, and once the viewer and the panels are mounted.
 */
export const RESTORE_POINTS = ["unmounted", "model", "mounted"] as const;
export type RestorePoint = (typeof RESTORE_POINTS)[number];

/**
 * When each restored field is applied, and in what order within its point.
 *
 * Typed over every field, so a field added to {@link DocumentTab} does not
 * compile until it is given a point here. The order of the lines is the order
 * of application and one pair depends on it: showing the message clears the
 * path of the redacted copy, so the path is put back after it.
 */
const RESTORED_AT: { readonly [K in keyof FreshState]: RestorePoint } = {
  query: "unmounted",
  findShown: "unmounted",
  error: "unmounted",
  offers: "unmounted",
  notice: "unmounted",
  redactedCopyPath: "unmounted",
  covered: "model",
  searchOptions: "mounted",
  searchScope: "mounted",
  sidebarTab: "mounted",
};

/**
 * One function for every restored field, each handed its value and the whole
 * record it came from.
 *
 * **This is the half {@link restoredState}'s typing could not reach.** That a
 * field is kept and read back is a compile error to get wrong; that anything
 * *applies* what was read back was three hand-written places in
 * `openDocument`, in the one file no gate reaches, and a field nothing applied
 * compiled. Supplied as an object of this type, a new field has no entry and
 * the window does not build until someone has said what restoring it means.
 *
 * The record comes too because some fields are one fact shown together: a
 * message with its buttons, a search with its options and its scope. The
 * field that takes the others along reads them from it, and each of the
 * others says so with {@link restoredWith}.
 */
export type Restore = {
  [K in keyof FreshState]: (value: FreshState[K], kept: FreshState) => void;
};

/**
 * The entry for a field that another field's entry applies along with its
 * own. Does nothing; `field` is there so the pairing is written down where the
 * next reader looks for what restores this one.
 */
export function restoredWith(_field: keyof FreshState): () => void {
  return () => {};
}

/** Applies every field of `kept` that belongs at `point`, in {@link RESTORED_AT}'s order. */
export function restore(kept: FreshState, point: RestorePoint, apply: Restore): void {
  const one = <K extends keyof FreshState>(key: K) => apply[key](kept[key], kept);
  for (const key of Object.keys(RESTORED_AT) as (keyof FreshState)[]) {
    if (RESTORED_AT[key] === point) one(key);
  }
}

/**
 * The open tabs, in order. A tab is named by its `view`; two tabs may show one
 * document, and the handle is then released when the last of them is removed.
 */
export class DocumentTabs<T extends { view: number; path: string }> {
  private entries: T[] = [];
  active: T["view"] = -1;

  get all(): readonly T[] { return this.entries; }

  find(id: T["view"]): T | undefined {
    return this.entries.find((tab) => tab.view === id);
  }

  /**
   * A tab showing the file at `path`. With the file shown in two tabs, the one
   * named `prefer` is the answer when it is one of them, and the first in the
   * row otherwise.
   */
  forPath(path: string, prefer?: T["view"]): T | undefined {
    // Windows dialogs and launch events can spell the same drive path differently.
    const key = (value: string) => /^[a-z]:[\\/]|^\\\\/i.test(value)
      ? value.replaceAll("\\", "/").toLowerCase() : value;
    const showing = this.entries.filter((tab) => key(tab.path) === key(path));
    return showing.find((tab) => tab.view === prefer) ?? showing[0];
  }

  keep(tab: T, replacing: T["view"] = tab.view): void {
    const index = this.entries.findIndex((entry) => entry.view === replacing);
    if (index < 0) this.entries.push(tab);
    else this.entries[index] = tab;
    this.active = tab.view;
  }

  /**
   * Adds a tab without bringing it to the front, for a document opened behind
   * the one the reader is looking at. A document already listed is left alone.
   */
  add(tab: T): void {
    if (!this.find(tab.view)) this.entries.push(tab);
  }

  /**
   * Adds a tab right after the tab named `after`, without bringing it to the
   * front: a second view of a document, beside the first in the row. At the
   * end when `after` is not a tab.
   */
  addAfter(tab: T, after: T["view"]): void {
    if (this.find(tab.view)) return;
    const index = this.entries.findIndex((entry) => entry.view === after);
    this.entries.splice(index < 0 ? this.entries.length : index + 1, 0, tab);
  }

  /**
   * Puts the tabs in the order of `paths`, which is the order they had when
   * the application was last closed. A tab `paths` does not name keeps its
   * place after the ones it does, in the order it already had.
   */
  arrange(paths: readonly string[]): void {
    const rank = (tab: T) => {
      const at = paths.indexOf(tab.path);
      return at < 0 ? paths.length : at;
    };
    // `sort` is stable, which is what keeps the unnamed tabs in their order.
    this.entries.sort((a, b) => rank(a) - rank(b));
  }

  /**
   * Moves a tab to the start or the end of the row. With two sides each row
   * shows its own tabs in this order, so the start of the whole order is the
   * start of the tab's own row.
   */
  moveTo(id: T["view"], where: "start" | "end"): void {
    const index = this.entries.findIndex((tab) => tab.view === id);
    if (index < 0) return;
    const [tab] = this.entries.splice(index, 1);
    if (!tab) return;
    if (where === "start") this.entries.unshift(tab);
    else this.entries.push(tab);
  }

  remove(id: T["view"]): T | undefined {
    const index = this.entries.findIndex((tab) => tab.view === id);
    if (index < 0) return undefined;
    const [removed] = this.entries.splice(index, 1);
    if (this.active === id)
      this.active = this.entries[Math.min(index, this.entries.length - 1)]?.view ?? -1;
    return removed;
  }

  neighbour(delta: number): T | undefined {
    if (!this.entries.length) return undefined;
    const index = this.entries.findIndex((tab) => tab.view === this.active);
    return this.entries[(index + delta + this.entries.length) % this.entries.length];
  }
}

/**
 * The other tabs showing the document `tab` shows.
 *
 * One document is one edit model, so the model is what two views of it have
 * in common. Not the path: a file saved under another name keeps its tab, and
 * not the handle, which a save replaces.
 */
export function twinsOf<T extends { edits: unknown }>(tabs: readonly T[], tab: T): T[] {
  return tabs.filter((other) => other !== tab && other.edits === tab.edits);
}

/** One tab for each document: a document shown twice is counted once. */
export function oneEach<T extends { edits: unknown }>(tabs: readonly T[]): T[] {
  return tabs.filter((tab, index) => tabs.findIndex((other) => other.edits === tab.edits) === index);
}

/** A save may reopen its document; transitions wait outside their own queue. */
export class DocumentTasks {
  private pending: Promise<void> = Promise.resolve();
  busy = false;

  constructor(private changed: (busy: boolean) => void = () => {}) {}

  run(body: () => Promise<void>): Promise<void> {
    if (this.busy) return this.pending;
    this.busy = true;
    this.changed(true);
    this.pending = Promise.resolve().then(body).finally(() => {
      this.busy = false;
      this.changed(false);
    });
    return this.pending;
  }

  async idle(): Promise<void> {
    while (this.busy) await this.pending.catch(() => {});
  }
}
