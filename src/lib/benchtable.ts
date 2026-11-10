/**
 * The columns of the tables the benchmarks print to the log (`autobench.ts`,
 * `scrollbench.ts`, `startup.ts`). One definition, so the three tables line
 * up the same way and a change to it is a change to all of them.
 */

/** `text` filled with spaces to `width`: on the left for a figure, on the right for a label. */
export function pad(text: string, width: number, right = false): string {
  return right ? text.padStart(width) : text.padEnd(width);
}
