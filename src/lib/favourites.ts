/**
 * Favourites: pages the reader marked to come back to.
 *
 * One list across every document, kept in the session (`session.rs`,
 * `Favourite`). This holds the list's rules --- one entry per page, a name
 * when the reader gives none, the next and previous in a document --- and the
 * palette entries that go to each, so `App.svelte` only wires them.
 */

import type { Command } from "./commands";
import { basename } from "./paths";
import type { Favourite } from "./session";

/** The palette entries' ids: one per favourite, rebuilt with the list. */
export const FAVOURITE_PREFIX = "nav.favourite.";

export class Favourites {
  private list: Favourite[];

  constructor(list: readonly Favourite[] = []) {
    this.list = [...list];
  }

  /** Every favourite, in the order they were added. */
  get all(): readonly Favourite[] {
    return this.list;
  }

  /** Whether page `page` of `path` is a favourite. */
  has(path: string, page: number): boolean {
    return this.list.some((one) => one.path === path && one.page === page);
  }

  /**
   * Marks a page, named `name` or, with none, after the page. Marking a page
   * already marked renames it rather than adding it twice.
   */
  add(path: string, page: number, name: string): Favourite {
    const added = { path, page, name: name.trim() || `Page ${page + 1}` };
    const at = this.list.findIndex((one) => one.path === path && one.page === page);
    if (at >= 0) this.list[at] = added;
    else this.list.push(added);
    return added;
  }

  /** Unmarks a page, answering whether it was marked. */
  remove(path: string, page: number): boolean {
    const before = this.list.length;
    this.list = this.list.filter((one) => !(one.path === path && one.page === page));
    return this.list.length !== before;
  }

  /**
   * The favourite page of `path` after (`1`) or before (`-1`) `page`, or
   * `null` past the last or before the first.
   */
  step(path: string, page: number, step: 1 | -1): number | null {
    const pages = this.list.filter((one) => one.path === path).map((one) => one.page);
    const beyond = pages.filter((one) => (step > 0 ? one > page : one < page));
    if (beyond.length === 0) return null;
    return step > 0 ? Math.min(...beyond) : Math.max(...beyond);
  }
}

/** A palette entry for each favourite, which goes to its page. */
export function favouriteCommands(
  list: readonly Favourite[],
  go: (favourite: Favourite) => void,
): Command[] {
  return list.map((favourite, index) => ({
    id: `${FAVOURITE_PREFIX}${index}`,
    title: `Go to favourite: ${favourite.name} (${basename(favourite.path)}, page ${favourite.page + 1})`,
    run: () => go(favourite),
  }));
}
