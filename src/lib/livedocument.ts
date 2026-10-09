/**
 * The state of one document that has a viewer mounted, as one record.
 *
 * `App.svelte` holds this state as separate variables, because its commands
 * and its markup read them by name several hundred times: `viewer`, `edits`,
 * `status` and the rest all mean "of the document the reader is working in".
 * That is right while one document is mounted. With two side by side there are
 * two sets of values and one set of variables, so the variables hold the
 * focused document's values and the other document's values wait here, in a
 * record, until the reader clicks into it.
 *
 * What this module owns is that moving a document in and out of the variables
 * takes **every** field. {@link Slots} is typed over the whole record, so a
 * field added to {@link LiveDocument} does not compile until `App.svelte` has
 * said which variable holds it; the same reasoning as `Restore` in
 * `documenttabs.ts`, one level down. A field taken out and not put back is a
 * value of one document shown under the other's name, and nothing would say so.
 *
 * Not a tab. A tab (`DocumentTab`) is a document the backend holds open and
 * what the reader left behind in it; most tabs have no viewer. A live document
 * is the mounted viewer and everything built around it.
 */

import type { Comments } from "./comments";
import { DegradedLabel } from "./degraded";
import type { Edits } from "./edits";
import type { Form, FormLayer } from "./forms";
import { ImportedLinks } from "./importedlinks";
import type { Link } from "./links";
import type { Outline } from "./outline";
import type { RegionPlan } from "./pages";
import type { Properties } from "./properties";
import type { Offer } from "./recovery";
import type { Sidebar } from "./sidebar";
import type { TextEditor } from "./textedit";
import type { Viewer, ViewerStatus } from "./viewer";

/** Everything `App.svelte` keeps about the document it has mounted. */
export interface LiveDocument {
  /** The backend's handle, or -1 for no document. */
  openDoc: number;
  openPathName: string;
  openPageCount: number;
  title: string;

  /** The page area the viewer is mounted in. */
  surface: HTMLDivElement | null;
  viewer: Viewer | null;
  sidebar: Sidebar | null;
  textEditor: TextEditor | null;
  formLayer: FormLayer | null;
  edits: Edits | null;
  /** The last edit still on its way to the model. */
  pendingEdit: Promise<void>;

  status: ViewerStatus | null;
  dirty: boolean;
  degraded: string | null;
  /** The clock behind {@link degraded}, which is an episode of one document. */
  degradedGate: DegradedLabel;

  query: string;
  findShown: boolean;
  error: string | null;
  offers: Offer[];
  notice: string | null;
  redactedCopyPath: string | null;

  /** Answers about the file, as the backend sent them. */
  properties: Properties | null;
  rawLinks: readonly Link[];
  importedLinks: ImportedLinks;
  rawComments: Comments | null;
  rawOutline: Outline | null;
  formNames: string[];
  scannedForm: Form | null;
  formEditing: boolean;

  /** Lookups keyed by ids that mean nothing in another document. */
  covered: Map<number, string>;
  wordsAsked: Set<number>;
  commentWords: Map<number, string>;
  redactionWords: Map<number, string | null>;
  redactionPlans: Map<number, RegionPlan>;
  fillingWords: boolean;
  fillingRedactionWords: boolean;
}

/** What every field holds when no document is mounted. New collections each call. */
export function blankLive(): LiveDocument {
  return {
    openDoc: -1,
    openPathName: "",
    openPageCount: 0,
    title: "",
    surface: null,
    viewer: null,
    sidebar: null,
    textEditor: null,
    formLayer: null,
    edits: null,
    pendingEdit: Promise.resolve(),
    status: null,
    dirty: false,
    degraded: null,
    degradedGate: new DegradedLabel(),
    query: "",
    findShown: false,
    error: null,
    offers: [],
    notice: null,
    redactedCopyPath: null,
    properties: null,
    rawLinks: [],
    importedLinks: new ImportedLinks(),
    rawComments: null,
    rawOutline: null,
    formNames: [],
    scannedForm: null,
    formEditing: false,
    covered: new Map(),
    wordsAsked: new Set(),
    commentWords: new Map(),
    redactionWords: new Map(),
    redactionPlans: new Map(),
    fillingWords: false,
    fillingRedactionWords: false,
  };
}

/**
 * One variable per field of `T`: how to read it and how to replace it.
 *
 * Mapped over every key, so the object a caller supplies is complete or does
 * not compile.
 */
export type Slots<T> = {
  readonly [K in keyof T]: { get(): T[K]; set(value: T[K]): void };
};

/** Reads every slot into a new record. */
export function capture<T>(slots: Slots<T>): T {
  const record = {} as T;
  for (const key of Object.keys(slots) as (keyof T)[]) {
    record[key] = slots[key].get();
  }
  return record;
}

/** Writes every field of `record` into its slot. */
export function install<T>(slots: Slots<T>, record: T): void {
  for (const key of Object.keys(slots) as (keyof T)[]) {
    slots[key].set(record[key]);
  }
}

/**
 * Which document's values the slots hold, and the records of the others.
 *
 * The slots hold one document at a time, the *focused* one. Every other
 * mounted document is *parked*: its values are in a record here, keyed by its
 * handle. The focused document's handle is read from the slots through
 * `focusedId` and is not stored a second time, so the two cannot disagree.
 *
 * Nothing here destroys anything. A caller that ends a document tears its
 * viewer down first and then calls {@link clear} or {@link take}.
 */
export class Stage<T> {
  readonly #slots: Slots<T>;
  readonly #blank: () => T;
  readonly #focusedId: () => number;
  readonly #parked = new Map<number, T>();
  #lent = 0;

  /** `focusedId` answers -1 when the slots hold no document. */
  constructor(slots: Slots<T>, blank: () => T, focusedId: () => number) {
    this.#slots = slots;
    this.#blank = blank;
    this.#focusedId = focusedId;
  }

  /** The handle of the document in the slots, or -1. */
  get focused(): number {
    return this.#focusedId();
  }

  /**
   * Whether the slots are on loan to a parked document right now, inside
   * {@link within}. What is about the window and not about a document, the
   * menu bar's enablement for one, is not to be pushed from in there: it would
   * describe a document the reader is not working in.
   */
  get lent(): boolean {
    return this.#lent > 0;
  }

  /** The handles of the parked documents, in the order they were parked. */
  get parked(): number[] {
    return [...this.#parked.keys()];
  }

  /** Whether `id` is mounted, focused or parked. */
  holds(id: number): boolean {
    return id >= 0 && (id === this.focused || this.#parked.has(id));
  }

  /** Puts the slots back to no document. The focused document's values are dropped. */
  clear(): void {
    install(this.#slots, this.#blank());
  }

  /**
   * Moves the focused document into a record and leaves the slots blank, for
   * a second document to be opened into them. Does nothing with no document.
   */
  park(): void {
    const id = this.focused;
    if (id < 0) return;
    this.#parked.set(id, capture(this.#slots));
    install(this.#slots, this.#blank());
  }

  /**
   * Makes the parked document `id` the focused one; the document that was
   * focused is parked. False, and nothing moved, when `id` is not parked.
   */
  focus(id: number): boolean {
    const record = this.#parked.get(id);
    if (!record) return false;
    this.#parked.delete(id);
    const leaving = this.focused;
    if (leaving >= 0) this.#parked.set(leaving, capture(this.#slots));
    install(this.#slots, record);
    return true;
  }

  /**
   * Removes the parked document `id` and hands its record over, for the
   * caller to tear down. Undefined when `id` is not parked.
   */
  take(id: number): T | undefined {
    const record = this.#parked.get(id);
    this.#parked.delete(id);
    return record;
  }

  /**
   * Runs `work` with document `id` in the slots, and puts the focused document
   * back afterwards, also when `work` throws.
   *
   * For a reply that arrives for a document the reader is not working in: the
   * code that applies it reads the slots, so the slots are lent to that
   * document for the length of one synchronous call. What `work` writes to the
   * slots stays with `id`. `work` must not wait for anything: after an `await`
   * the slots belong to the focused document again.
   *
   * Returns `{ ran: false }` when `id` is not mounted, so that a reply for a
   * document that has gone is dropped by the caller and not applied to another.
   */
  within<R>(id: number, work: () => R): { ran: true; value: R } | { ran: false } {
    if (id < 0) return { ran: false };
    if (id === this.focused) return { ran: true, value: work() };
    const record = this.#parked.get(id);
    if (!record) return { ran: false };

    const focused = capture(this.#slots);
    install(this.#slots, record);
    this.#lent++;
    try {
      return { ran: true, value: work() };
    } finally {
      this.#lent--;
      // Read back before the focused values return, so what `work` changed is
      // kept. If `work` ended the document, its record is not put back.
      if (this.#focusedId() === id) this.#parked.set(id, capture(this.#slots));
      else this.#parked.delete(id);
      install(this.#slots, focused);
    }
  }
}

/**
 * A copy of `options` in which every function runs through `run`.
 *
 * For the callbacks a viewer and its panels are built with. They are written
 * against the slots, so each has to run with its own document in them, and
 * wrapping the whole object is what makes that true of a callback added later
 * as well as of the ones there today. `run` answers `undefined` for a document
 * that is no longer mounted, and the callback is then not called.
 *
 * Plain objects are copied and walked, so a group of callbacks nested in the
 * options is wrapped too. Arrays, class instances and everything else are
 * handed through as they are.
 */
export function scoped<T>(options: T, run: <R>(work: () => R) => R | undefined): T {
  if (typeof options === "function") {
    const call = options as unknown as (...args: unknown[]) => unknown;
    return ((...args: unknown[]) => run(() => call(...args))) as unknown as T;
  }
  if (options === null || typeof options !== "object") return options;
  if (Object.getPrototypeOf(options) !== Object.prototype) return options;
  const copy: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(options)) copy[key] = scoped(value, run);
  return copy as T;
}
