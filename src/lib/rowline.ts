/**
 * What a panel row's first line says, for both panels that have rows.
 *
 * `marklist.ts` lists the reader's own marks and `commentlist.ts` lists a
 * document's annotations, and they are deliberately not the same panel --- one
 * is live model state that cannot fail, the other is a scan of a file that can.
 * The *first line of a row*, though, is one question asked twice, and it is
 * answered here rather than in each of them.
 *
 * The trap this avoids is not hypothetical. Two copies of the rule would each
 * have their own flatten, their own precedence and their own dimming, and the
 * failure would be silent: a change made to one panel's rule leaves the other
 * looking correct, because both still produce a plausible line. This repository
 * records that as *"Two copies of a distinction drift, and a mutation of one
 * survives"* --- and the marks panel's copy is exactly where a mutation of the
 * comments panel's would have survived.
 *
 * What is deliberately **not** shared is the fallback string. "No note" and
 * "Highlight, no comment" say different things because the panels are about
 * different things, so each caller supplies its own; sharing it would be the
 * opposite error, one vocabulary for two subjects.
 */

/**
 * One line of whatever was handed in.
 *
 * A row is one line high and a note or a comment body has real newlines in it.
 * Both of {@link rowLine}'s candidates go through it: the words a mark covers
 * run over the lines of the page they were taken from, so they arrive with
 * exactly the same problem.
 */
export function flatten(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * What a row's first line says, and whether a person wrote it.
 *
 * Three cases in one order, and the order is the argument. **What was typed
 * wins**, always: it is the one thing on the row somebody chose, and a highlight
 * noted "check this against §4" must not be listed by the sentence it sits on.
 * Where nothing was typed, the words the mark covers are what a reader would
 * recognise it by --- which is the whole point, because the alternative is a
 * column of nine rows that all say the same thing. And where there are neither,
 * the row falls back to what the caller supplies.
 *
 * `own` is false for both fallbacks, and a row draws it the way it already drew
 * the bare case: dimmed and italic. That is not decoration --- it is the only
 * thing separating a sentence a person wrote from a sentence the document did,
 * in a panel whose subject is what people wrote.
 */
export function rowLine(
  typed: string,
  covered: string,
  fallback: string,
): { text: string; own: boolean } {
  const wrote = flatten(typed);
  if (wrote) return { text: wrote, own: true };
  const words = flatten(covered);
  if (words) return { text: words, own: false };
  return { text: fallback, own: false };
}
