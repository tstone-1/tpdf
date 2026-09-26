import type { DocumentInfo } from "./ipc";
import type { Edits } from "./edits";
import type { Place } from "./session";
import type { Tab } from "./sidebar";
import { PLAIN_SEARCH, type SearchOptions, type ScopeRange } from "./search";
import type { Offer } from "./recovery";

/** State kept while a document has no mounted viewer. Backend handles stay open. */
export interface DocumentTab {
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
export type TabState = Omit<DocumentTab, "doc" | "path">;

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
 * Read at three points of `openDocument` rather than applied at one, because
 * the order is load-bearing there: the status line before the viewer mounts,
 * the covered words beside the model, the search and sidebar tab after the
 * panels exist. What this owns is that every one of them comes from the same
 * record.
 */
export function restoredState(tab: DocumentTab | undefined): FreshState {
  if (!tab) return freshState();
  const { covered, query, findShown, searchOptions, searchScope, sidebarTab, error, offers, notice,
    redactedCopyPath } = tab;
  return { covered, query, findShown, searchOptions, searchScope, sidebarTab, error, offers, notice,
    redactedCopyPath };
}

/** Ordered ownership of open handles. Only removal permits backend release. */
export class DocumentTabs<T extends { doc: { id: number }; path: string }> {
  private entries: T[] = [];
  active = -1;

  get all(): readonly T[] { return this.entries; }

  find(id: number): T | undefined {
    return this.entries.find((tab) => tab.doc.id === id);
  }

  forPath(path: string): T | undefined {
    // Windows dialogs and launch events can spell the same drive path differently.
    const key = (value: string) => /^[a-z]:[\\/]|^\\\\/i.test(value)
      ? value.replaceAll("\\", "/").toLowerCase() : value;
    return this.entries.find((tab) => key(tab.path) === key(path));
  }

  keep(tab: T, replacing = tab.doc.id): void {
    const index = this.entries.findIndex((entry) => entry.doc.id === replacing);
    if (index < 0) this.entries.push(tab);
    else this.entries[index] = tab;
    this.active = tab.doc.id;
  }

  remove(id: number): T | undefined {
    const index = this.entries.findIndex((tab) => tab.doc.id === id);
    if (index < 0) return undefined;
    const [removed] = this.entries.splice(index, 1);
    if (this.active === id)
      this.active = this.entries[Math.min(index, this.entries.length - 1)]?.doc.id ?? -1;
    return removed;
  }

  neighbour(delta: number): T | undefined {
    if (!this.entries.length) return undefined;
    const index = this.entries.findIndex((tab) => tab.doc.id === this.active);
    return this.entries[(index + delta + this.entries.length) % this.entries.length];
  }
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
