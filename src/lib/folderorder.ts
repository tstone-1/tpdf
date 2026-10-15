/**
 * The next and previous PDF in a document's folder, in the order the file
 * manager shows them.
 *
 * The backend lists the folder (`folder_pdfs`); this sorts it and finds the
 * neighbour. Sorted as Finder and Explorer sort: by name, ignoring case and
 * accents, with a run of digits read as a number, so `page 2` comes before
 * `page 10`.
 */

import { basename } from "./paths";

const ORDER = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** `paths` in the file manager's order, by file name. */
export function folderOrder(paths: readonly string[]): string[] {
  return [...paths].sort((a, b) => ORDER.compare(basename(a), basename(b)) || (a < b ? -1 : a > b ? 1 : 0));
}

/**
 * The PDF `step` places after `current` in its folder, or `null` at either
 * end: the first and the last do not wrap round, so a reader stepping through
 * a folder is told where it ends.
 */
export function neighbourIn(paths: readonly string[], current: string, step: 1 | -1): string | null {
  const sorted = folderOrder(paths);
  const at = sorted.indexOf(current);
  if (at < 0) return null;
  return sorted[at + step] ?? null;
}
