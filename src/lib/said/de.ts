/** German for every sentence Rust writes with `said!`, keyed by its template.
 *
 * One file per area beside this one, each exporting its entries as `default`;
 * this file joins them. A key is the template exactly as it stands in the Rust
 * source, escapes resolved, and `said.test.ts` refuses a key with no `said!`
 * behind it, a `said!` with no key, and a key in two files. A wording uses the
 * template's placeholder names; a function gets them as strings, for the
 * sentences German has to bend around a number.
 *
 * The glossary in `docs/DETAIL.md` *Interface language* applies: "du" here
 * and "Sie" in `sie`, no "Bitte", „…" quotation marks.
 *
 * A file's `sie` export holds the same sentence addressing the reader with
 * "Sie", for each entry whose German says "du"; `formal.test.ts` holds that.
 */

export type Wording = string | ((args: Record<string, string>) => string);

type Area = { default: Record<string, Wording>; sie?: Record<string, Wording> };

// Whole modules, not `import: "sie"`: the production build refuses a named import that
// an area does not export, and most areas have no "Sie" entries.
const MODULES = import.meta.glob<Area>(["./*.ts", "!./de.ts"], { eager: true });

/** Each area's entries, by file name. */
export const AREAS = Object.fromEntries(Object.entries(MODULES).map(([file, area]) => [file, area.default]));

export const GERMAN: Record<string, Wording> = Object.assign({}, ...Object.values(AREAS));

/** The "Sie" wordings of every area that has them. */
export const SIE: Record<string, Wording> = Object.assign({}, ...Object.values(MODULES).map((area) => area.sie ?? {}));
