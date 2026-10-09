/**
 * The identity of one view of a document, as distinct from the document.
 *
 * A tab, an entry on a side of the window and a mounted viewer's record were
 * all named by the backend's handle of the document they show. That holds
 * while a document is shown once. Shown on both sides of the window it is one
 * handle and two of each of those, so what they are named by is a view, and a
 * view names its document and not the other way round.
 *
 * **A type of its own, because both are numbers.** A handle passed where a
 * view is wanted would find the first view of a document and never the
 * second, and nothing would say so until a document was open twice. With
 * {@link ViewId} branded it does not compile. The same reasoning as
 * `docs/TRAPS.md`, *An id and a slot are both `number`, so a mark drawn on the
 * last page vanished*.
 *
 * The first view of a document is named by the document's handle
 * ({@link viewOf}), so that everything written before a document could be
 * shown twice, the window checks included, reads as it did. A further view of
 * the same document gets a number no handle has ({@link ViewIds.another}).
 */

declare const viewBrand: unique symbol;
export type ViewId = number & { readonly [viewBrand]: true };

/**
 * A document's handle, as a backend command takes it: any number that is not
 * a {@link ViewId}.
 *
 * The other half of the brand. A view is not a handle, and the first view of a
 * document *equals* its handle, so `call("...", { doc: openView })` would work
 * until a document was shown twice and then name nothing the backend holds.
 * Every `doc` argument in `ipc.ts` is this type, so that line does not compile.
 * A plain `number` is one already; nothing is cast to it.
 */
export type DocHandle = number & { readonly [viewBrand]?: never };

/** No view: what a side showing nothing has in front, and what no tab is active reads as. */
export const NO_VIEW = -1 as ViewId;

/** The first view of the document with this handle. */
export function viewOf(doc: number): ViewId {
  return doc as ViewId;
}

/**
 * Where the ids of further views start. Handles are counted up from zero by
 * the backend for as long as it runs, each given once, and do not get here.
 */
export const FURTHER_VIEWS = 2 ** 40;

/** Whether `view` is a further view of a document, not its first. */
export function isFurther(view: ViewId): boolean {
  return view >= FURTHER_VIEWS;
}

/** Hands out the ids of further views, each once. */
export class ViewIds {
  #next = FURTHER_VIEWS;

  another(): ViewId {
    return this.#next++ as ViewId;
  }
}
