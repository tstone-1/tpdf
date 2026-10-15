/**
 * Closing several tabs at once, and reopening the last one closed.
 *
 * `App.svelte` closes each tab through the one `closeTab` that already knows
 * about the two sides and the two views of one document. This decides which
 * tabs a command means, and remembers what was closed.
 */

/** The tabs closed together: every other one, or those before or after one. */
export type CloseWhich = "others" | "left" | "right";

/**
 * The tabs `which` closes, from the tab `id`.
 *
 * `row` is the strip the tab is in --- with two sides, its own side's --- and
 * left and right are taken in it, because that is the strip the reader sees
 * them in. Other tabs are every tab in the window but `id`.
 */
export function tabsToClose<T>(all: readonly T[], row: readonly T[], id: T, which: CloseWhich): T[] {
  if (which === "others") return all.filter((tab) => tab !== id);
  const at = row.indexOf(id);
  if (at < 0) return [];
  return which === "left" ? row.slice(0, at) : row.slice(at + 1);
}

/** How many closed documents are remembered for reopening. */
export const CLOSED_KEPT = 20;

/**
 * The documents closed in this session, most recent last.
 *
 * Paths, because a document reopened by path opens where it was left: the
 * session remembers each document's place.
 */
export class ClosedTabs {
  private paths: string[] = [];

  /** Remembers a document closed just now. */
  closed(path: string): void {
    this.paths = this.paths.filter((one) => one !== path);
    this.paths.push(path);
    if (this.paths.length > CLOSED_KEPT) this.paths.shift();
  }

  /** The most recent document closed that is not open again, or none. */
  latest(isOpen: (path: string) => boolean): string | null {
    for (let at = this.paths.length - 1; at >= 0; at--) {
      const path = this.paths[at]!;
      if (!isOpen(path)) return path;
    }
    return null;
  }

  /** Takes a document off the list once it is reopened. */
  reopened(path: string): void {
    this.paths = this.paths.filter((one) => one !== path);
  }
}
