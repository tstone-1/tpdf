/**
 * The one thing all three side panels draw that is not a row.
 *
 * `sidebar.ts` (the outline), `marklist.ts` (the reader's own marks) and
 * `commentlist.ts` (a document's annotations) are deliberately separate panels
 * --- one is a tree, one is live model state that cannot fail, one is a scan of
 * a file that can. What they share is the line they put in place of rows:
 * *reading*, *could not be read*, *there are none*. That line was written out
 * three times, byte for byte, in three modules.
 *
 * Shared for the reason `rowline.ts` states about a row's first line, and it is
 * the weaker version of the same argument: nothing about three identical copies
 * of a five-line function is dangerous today, and what drifts is the next
 * change --- a padding fixed in one panel, a colour in another, and three
 * panels that no longer look like one application. The failure is silent
 * because each copy on its own still produces a plausible placeholder.
 *
 * What is deliberately **not** shared is the text. "This document has no
 * outline", "You have not marked anything in this document" and "This document
 * has no comments" say different things because the panels are about different
 * things; sharing the wording would be the opposite error, one vocabulary for
 * three subjects. Same split as `rowline.ts`'s fallback string.
 */

/**
 * A dimmed line standing in for rows a panel has none of.
 *
 * Inline styles rather than a class, matching what the three copies did: these
 * panels build their DOM in TypeScript and carry no stylesheet of their own.
 */
export function placeholder(text: string): HTMLElement {
  const element = document.createElement("div");
  element.style.cssText = "padding:0.5rem 0.7rem;opacity:0.55;";
  element.textContent = text;
  return element;
}
