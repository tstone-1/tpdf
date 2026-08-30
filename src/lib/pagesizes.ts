/**
 * The page sizes a reader can insert, and what each one measures.
 *
 * ## Why a table rather than a prompt
 *
 * *Insert blank page* has always made a page the size of the one you are
 * looking at, which is right almost always and unhelpful exactly when it is
 * not: the page you are on is A5, or a scan at some size nobody chose, and the
 * page you want to add is A4. The alternative to this table is a command that
 * asks for two numbers, and a reader who wants A4 does not have two numbers ---
 * they have a name. So the names are the menu, one command each, which is the
 * argument `edit.stamp.*` and `edit.color.*` both make and this follows their
 * spelling so the three families read alike.
 *
 * {@link https://www.iso.org/standard/36631.html ISO 216} gives the A series in
 * millimetres and PDF measures in points of 1/72 inch, so every number below is
 * a conversion and none of them is round. The US sizes are the opposite --- they
 * are *defined* in inches, so they land on whole points, and that is a fact
 * worth asserting rather than a coincidence worth trusting.
 *
 * ## Portrait only, and turning is the answer for the rest
 *
 * Every entry is taller than it is wide. Landscape variants would double a list
 * of five for a choice the reader already has: a page tpdf made turns like any
 * other, so *Insert blank A4 page* followed by *Rotate clockwise* is a
 * landscape A4, and the writer puts the turn in `/Rotate` where every reader
 * honours it.
 *
 * ## What this is not
 *
 * Not a table the backend knows about. `Command::Insert` carries two `f64` and
 * `save.rs` writes them into `/MediaBox`; neither has a notion of "A4". This is
 * the one statement of what the names mean, so it cannot drift from a second
 * copy the way `markband.ts` has to and says so.
 */

/** A named size, in PDF points. */
export interface NamedSize {
  /** What the palette calls it. */
  readonly title: string;
  /** Width in points, always the smaller of the two. */
  readonly width: number;
  /** Height in points. */
  readonly height: number;
}

/**
 * The sizes, in the order the palette lists them.
 *
 * A4 first because it is the one the old bullet apologised for not offering,
 * then its two neighbours in the series, then the two US sizes. Registration
 * order is what decides a tie in `commands.ts`, so this order is the answer to
 * "which one did I mean" for a query that matches several.
 */
export const PAGE_SIZES = {
  a4: { title: "A4", width: 595.276, height: 841.89 },
  a3: { title: "A3", width: 841.89, height: 1190.551 },
  a5: { title: "A5", width: 419.528, height: 595.276 },
  letter: { title: "US Letter", width: 612, height: 792 },
  legal: { title: "US Legal", width: 612, height: 1008 },
} as const satisfies Record<string, NamedSize>;

/** One of {@link PAGE_SIZES}' keys. */
export type PageSizeName = keyof typeof PAGE_SIZES;

/**
 * The names, in the table's own order.
 *
 * `Object.keys` widens to `string[]`, which would put the checking of a size
 * name back on every caller; this is the one place that assertion is made.
 */
export const PAGE_SIZE_NAMES = Object.keys(PAGE_SIZES) as PageSizeName[];

/**
 * What a named size measures, as the pair `Command::Insert` takes.
 *
 * A function rather than two field reads at the call site, and it is the whole
 * reason this is worth a module: the caller is `App.svelte`, which no unit test
 * reaches, so an argument built there --- and transposed there --- would be
 * checked by nothing. Here it has a mutation and a test.
 */
export function dimensionsOf(name: PageSizeName): [number, number] {
  const size = PAGE_SIZES[name];
  return [size.width, size.height];
}
