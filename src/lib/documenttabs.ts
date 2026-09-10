import type { DocumentInfo } from "./ipc";
import type { Edits } from "./edits";
import type { Place } from "./session";
import type { Tab } from "./sidebar";
import type { SearchOptions, ScopeRange } from "./search";
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
