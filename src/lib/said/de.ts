/** German for every sentence Rust writes with `said!`, keyed by its template.
 *
 * One file per area beside this one, each exporting its entries as `default`;
 * this file joins them. A key is the template exactly as it stands in the Rust
 * source, escapes resolved, and `said.test.ts` refuses a key with no `said!`
 * behind it, a `said!` with no key, and a key in two files. A wording uses the
 * template's placeholder names; a function gets them as strings, for the
 * sentences German has to bend around a number.
 *
 * The glossary in `docs/DETAIL.md` *Interface language* applies: "du", no
 * "Bitte", „…" quotation marks.
 */

export type Wording = string | ((args: Record<string, string>) => string);

/** Each area's entries, by file name. */
export const AREAS = import.meta.glob<Record<string, Wording>>(["./*.ts", "!./de.ts"], {
  import: "default",
  eager: true,
});

export const GERMAN: Record<string, Wording> = Object.assign({}, ...Object.values(AREAS));
