/**
 * Taking a file name off a path, on both platforms.
 *
 * Three copies of `path.split("/").pop() ?? path` had accumulated --- the window
 * title in `App.svelte` and a detail column in each of two harnesses --- and all
 * three were wrong in the same direction on the platform that ships. Windows
 * paths arrive from the native open dialog, from `take_launch_paths` and from
 * the single-instance forward with `\` separators, so `pop()` returns the whole
 * path and the title bar shows `C:\Users\...\report.pdf` where it should show
 * `report.pdf`.
 *
 * It is the shape this repository already has an entry for: one constant, or
 * here one expression, standing for a distinction that is not the same on both
 * platforms. Extracted rather than fixed in place because a two-line helper
 * copied three times is a fix that has to be made three times, and the third
 * copy is the one that gets missed.
 *
 * Deliberately not `@tauri-apps/api/path`'s `basename`: that is an async
 * `invoke` round trip to the backend, and every caller here is a synchronous
 * label --- a window title recomputed on open, a detail column printed beside a
 * check result. A round trip per label buys nothing that a split does not.
 */

/**
 * The last segment of `path`, treating both `/` and `\` as separators.
 *
 * Both, unconditionally, rather than "the separator this platform uses": the
 * frontend has no reliable way to ask, a Windows path can legally mix the two,
 * and a POSIX file name containing a backslash is a pathological case nobody
 * here produces. Returns `path` unchanged when there is no separator, and when
 * the path *ends* in one --- a trailing separator leaves an empty last segment,
 * which is a worse label than the input it came from.
 */
export function basename(path: string): string {
  const last = path.split(/[\\/]/).pop();
  return last === undefined || last === "" ? path : last;
}
