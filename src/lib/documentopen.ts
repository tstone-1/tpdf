/**
 * The decisions an open makes, for `openDocument` in `App.svelte`.
 *
 * That function mounts a document: it tears the outgoing one down, builds the
 * viewer and the panels, and fills the component's variables. What it has to
 * *decide* on the way is here, where a test can call it: which page sizes the
 * viewer starts from, where the reader is put back, what a document shown
 * twice shares, which tab it is kept in, which side the view goes on, and what a failed open says,
 * takes with it and still says once another tab is shown. The component keeps the order and the wiring.
 *
 * Nothing here waits, and nothing here reads the component's variables.
 */

import { isOpenRefusal, type DocumentInfo, type PageSize } from "./ipc";
import { freshState, twinsOf, type DocumentTab, type FreshState, type KeptScope } from "./documenttabs";
import type { Edits } from "./edits";
import { clampPlace, type Place } from "./session";

/**
 * What an open found before it mounts anything, as `adoptModel` in
 * `App.svelte` takes it: the tab being returned to, the tab a save or a
 * reload opens the file again in and the handle that tab had, its other
 * views, the place to resume at and what the tab left behind.
 */
export interface OpenFound {
  retained: DocumentTab | undefined;
  replacing: DocumentTab | undefined;
  replaceId: number | undefined;
  twins: readonly DocumentTab[];
  resume: Place | null;
  kept: FreshState;
}

/**
 * The page sizes a viewer is built with, the first of which there must be.
 *
 * The whole table the open carried, not only its first entry. On a lazy
 * open --- the default, because collecting every page's size costs 86 ms
 * on a long document --- that *is* only the first entry, and the viewer
 * estimates the rest and corrects them as it reads. What it must not do is
 * discard sizes the backend already sent, which is what handing over
 * `pages[0]` alone did: with `TPDF_EAGER_GEOMETRY` set the whole document's
 * geometry arrived and every page after the first was still laid out at
 * page 1's.
 *
 * Throws for a document that reports no pages, which the open then fails on.
 */
export function pageTable(doc: Pick<DocumentInfo, "pages">): [PageSize, ...PageSize[]] {
  const page = doc.pages[0];
  if (!page) throw new Error("document reports no pages");
  return [page, ...doc.pages.slice(1)];
}

/**
 * Where an open puts the reader, or null for the top of a document nobody
 * has a place in.
 *
 * `kept` is the place of a tab being returned to and `override` the one a
 * caller already knows: a caller that knows where the reader is wins over the
 * startup snapshot in `places` --- see `reloadDocument`, which is the only one
 * that does.
 *
 * Fitted to the document as it is now, not as it was: the file may have
 * been rebuilt shorter since, and a viewer scrolled past its own last page
 * is a worse answer than the wrong page.
 */
export function placeToResume(
  kept: Place | null | undefined,
  override: Place | null,
  places: readonly Place[],
  path: string,
  pageCount: number,
): Place | null {
  const remembered = kept ?? override ?? places.find((place) => place.path === path);
  return remembered ? clampPlace(remembered, pageCount) : null;
}

/**
 * Points the other views of a document at what a save or a reload made of it.
 *
 * They are views of the file as it is now: the new handle, and the model
 * built for it.
 *
 * **A search each had confined to a selection is not kept.** The scope is
 * held with the ids of the pages it was taken on (`KeptScope`), and page ids
 * start again with every model: a file read again with as many pages has the
 * same ids in the same order, so `scopeToRestore` would put the ranges back
 * onto whatever text is there now. Nothing says the file holds what it held,
 * so a new model drops the scope and the search runs over the whole document.
 * The words and how they are matched stay.
 */
export function repoint<D, E>(
  twins: readonly { doc: D; edits: E; searchScope: KeptScope | null }[],
  doc: D,
  edits: E,
): void {
  for (const twin of twins) {
    twin.doc = doc;
    twin.edits = edits;
    twin.searchScope = null;
  }
}

/**
 * The tab an open keeps its document in: the one being returned to, as it
 * is, or a new one around the model just built.
 *
 * A save or a reload opens the file again in the place of `from.replacing`,
 * and nothing that tab kept is carried into the new one: its marks' words,
 * its messages and its search were about a model that has gone. For the
 * search's scope that is {@link repoint}'s reason. A tab returned to has the
 * model it had, and keeps everything.
 */
export function tabToKeep(
  view: DocumentTab["view"],
  doc: DocumentInfo,
  path: string,
  edits: Edits,
  from: OpenFound,
): DocumentTab {
  return from.retained ?? { view, doc, path, edits, place: from.resume, ...freshState() };
}

/**
 * What the other views of `tab`'s document already hold, as `read` answers
 * for the first of them that answers at all, or undefined.
 *
 * Two views of one document read one list of what its marks cover. `read`
 * answers undefined for a view that is not mounted. Every twin is asked, in
 * the tab row's order, before the first answer is picked.
 */
export function sharedByTwin<T extends { edits: unknown }, R>(
  tabs: readonly T[],
  tab: T | undefined,
  read: (twin: T) => R | undefined,
): R | undefined {
  if (!tab) return undefined;
  return twinsOf(tabs, tab).map(read).find((held) => held !== undefined);
}

/** The three things {@link placeOnSide} tells the pane model. `Panes` is one. */
export interface SidePlacer<Id> {
  replaced(before: Id, after: Id): void;
  fronted(id: Id): void;
  opened(id: Id): void;
}

/**
 * Tells the pane model where a view that has just been given its model goes.
 *
 * A tab returned to is in front of the side it is on. A save's new handle
 * takes the old one's side, which is `replacing`, and a document opened for
 * the first time joins the side the reader is working in.
 */
export function placeOnSide<Id>(
  panes: SidePlacer<Id>,
  view: Id,
  replacing: Id | undefined,
  returning: boolean,
): void {
  if (replacing !== undefined) panes.replaced(replacing, view);
  if (returning || replacing !== undefined) panes.fronted(view);
  else panes.opened(view);
}

/**
 * Takes a view that an open could not finish out of the sides and the tabs.
 *
 * The sides are told first and with the row as it still is, this view
 * included: that order is how the side it was in front of finds a neighbour.
 */
export function dropView<Id>(
  panes: { closed(id: Id, order: readonly Id[]): void },
  tabs: { readonly all: readonly { view: Id }[]; remove(id: Id): unknown },
  view: Id,
): void {
  panes.closed(view, tabs.all.map((entry) => entry.view));
  tabs.remove(view);
}

/**
 * Takes out of the sides and the tabs every view a failed open leaves without
 * a document, and then has the tab row drawn again.
 *
 * `own` is the view of the handle the open was given, or undefined when the
 * handle was a tab's own, which keeps its tab. `twins` are the other views of
 * the document the open was replacing: they go only when `replaced` says the
 * outgoing document had been torn down, because they went down with it for a
 * handle that never arrived. Before that they are still on screen.
 *
 * `refresh` is called once and last, on every path. Called between the two
 * removals, as it was, it drew a row that still held the twins.
 */
export function dropAbandoned<Id>(
  panes: { closed(id: Id, order: readonly Id[]): void },
  tabs: { readonly all: readonly { view: Id }[]; remove(id: Id): unknown },
  left: { own: Id | undefined; replaced: boolean; twins: readonly { view: Id }[] },
  refresh: () => void,
): void {
  if (left.own !== undefined) dropView(panes, tabs, left.own);
  if (left.replaced) for (const twin of left.twins) dropView(panes, tabs, twin.view);
  refresh();
}

/**
 * What a failed open tells the reader, or null for nothing.
 *
 * A document that was open last time and is not there now is not a failure
 * the reader caused, so `resuming` says nothing and the window simply comes up
 * empty. A refusal carries its own sentence; anything else is shown as it is.
 */
export function openFailure(why: unknown, resuming: boolean): string | null {
  if (resuming) return null;
  return isOpenRefusal(why) ? why.reason : String(why);
}

/**
 * What the reader is told once the window has gone back to a tab it already
 * had, or null to leave that tab's own message as it is.
 *
 * `failed` is what the open that could not finish said: a reload that failed
 * after its document was torn down, whose tab is then mounted again on the
 * handle it had. Mounting a tab puts back the message that tab kept, which is
 * nothing or an older one, so the reason is said again afterwards and wins
 * over it: the reader pressed Reload and is looking at the file as it was.
 * `refused` is what a tab that would not mount again said. Both are told when
 * there are both, the first failure first, since the second does not explain
 * why the document is not as the reader asked.
 */
export function toldAfterFallback(failed: string | null, refused: string | null): string | null {
  if (failed === null || refused === null) return failed ?? refused;
  return failed === refused ? failed : `${failed}\n${refused}`;
}

/**
 * Whether something that acts on the focused document may be started for
 * `view`: the reader is in that document, or `take` brings them there.
 *
 * A press on a place for a signature in a form is the case. Signing reads the
 * focused document and starts a moment after it is asked for, so asking for
 * it "as" another document would sign whichever one is focused by then.
 * `take` is not called when the reader is already there.
 */
export function readerIsIn<Id>(view: Id, focused: Id, take: () => boolean): boolean {
  return view === focused || take();
}
