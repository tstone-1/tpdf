/**
 * Labelling recently-read documents for the command palette.
 *
 * The list itself is not built here and is not new: `session.rs` already keeps
 * every document that has been read, most recent first, deduplicated by path and
 * truncated --- because that is what session restore needs. Reaching the second
 * one has simply never been possible, so a reader who wanted yesterday's other
 * document went through the file dialog for a file the application already knew
 * about.
 *
 * `docs/PLAN.md` §8 says every command is reachable in two keystrokes through
 * the palette, which decides the shape: recent documents are **commands**, not a
 * menu. They rank the same way, they are found by typing part of a name, and
 * nothing new has to be learned to reach them.
 *
 * ## What is actually hard here is the label
 *
 * A basename is what a reader recognises and is not unique --- `report.pdf` in
 * three client folders is the normal case, not the awkward one, and three
 * identical rows are worse than no list. The full path is unique and unreadable
 * at a glance, which is the only thing a list of eight rows is good for.
 *
 * So {@link labelsFor} shows the basename and lengthens **only the labels that
 * collide**, one directory at a time, until they are distinct. The common case
 * stays short; the ambiguous case says exactly as much as it must and no more.
 */

/** Recent documents the palette offers. */
export const MAX_RECENTS = 8;

/** The command id for the `index`-th recent document. */
export function recentCommandId(index: number): string {
  return `${RECENT_PREFIX}${index}`;
}

/**
 * The id prefix every recent-document command shares.
 *
 * The list is replaced whole whenever it changes, and the prefix is how the
 * registry knows which commands the replacement supersedes.
 */
export const RECENT_PREFIX = "file.recent.";

/** A path split into its segments, with the separator it was written with. */
function split(path: string): { segments: string[]; separator: string } {
  // Both separators, because a session file is not necessarily written by the
  // platform reading it: these are absolute paths recorded on whichever machine
  // last opened the document.
  const separator = path.includes("\\") && !path.includes("/") ? "\\" : "/";
  return { segments: path.split(/[\\/]+/).filter(Boolean), separator };
}

/** The last `depth` segments of a path, joined as they were written. */
function tail(path: string, depth: number): string {
  const { segments, separator } = split(path);
  return segments.slice(Math.max(0, segments.length - depth)).join(separator);
}

/**
 * Labels for `paths`: the basename, lengthened where two would collide.
 *
 * Only the colliding labels grow, and only until they differ, so one awkward
 * pair does not make every other row longer. Two labels that cannot be told
 * apart however far back they go --- the same path twice --- stop growing rather
 * than looping, which is what makes this terminate on any input.
 */
export function labelsFor(paths: readonly string[]): string[] {
  const depth = paths.map(() => 1);
  const longest = paths.map((path) => split(path).segments.length);

  for (;;) {
    const labels = paths.map((path, index) => tail(path, depth[index] ?? 1));

    const seen = new Map<string, number[]>();
    labels.forEach((label, index) => {
      const group = seen.get(label);
      if (group) group.push(index);
      else seen.set(label, [index]);
    });

    let grew = false;
    for (const group of seen.values()) {
      if (group.length < 2) continue;
      for (const index of group) {
        if ((depth[index] ?? 1) < (longest[index] ?? 1)) {
          depth[index] = (depth[index] ?? 1) + 1;
          grew = true;
        }
      }
    }
    // Nothing could be lengthened, so nothing will ever differ. Returning the
    // labels as they are beats looping, and duplicates here mean duplicate
    // paths, which the session store does not produce.
    if (!grew) return labels;
  }
}
