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

/** Absolute POSIX names may contain literal backslashes. */
export function pathSeparators(path: string): RegExp {
  return path.startsWith("/") ? /\/+/ : /[\\/]+/;
}

/** Last segment, falling back to the input for a trailing separator. */
export function basename(path: string): string {
  return path.split(pathSeparators(path)).at(-1) || path;
}

/** Windows drive and UNC paths may differ in case and separator spelling. */
export function pathKey(value: string): string {
  return /^[a-z]:[\\/]|^\\\\/i.test(value)
    ? value.replaceAll("\\", "/").toLowerCase() : value;
}
