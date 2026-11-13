/**
 * A message of several lines, as the message bar lays it out.
 *
 * A message whose parts are lines of their own --- the sentence after signing
 * (`signing.ts`) is the first --- is shown as its first line, then one row per
 * further line. A line `Label: value` becomes a row with the label in a column
 * of its own, and a value of the shape `verdict — explanation`, which is how
 * `integrity.ts` words every row of the properties dialog, keeps the verdict in
 * the reader's eye and the explanation in a quieter style beside it. Until
 * 2026-11-13 such a message was one paragraph: seven parts and five dates in a
 * single run of text.
 *
 * The parts put back together, label, value and detail, are the line exactly:
 * the window checks read the bar's `textContent` and compare it with the
 * sentence, so the layout must not change a character of it.
 */

/**
 * The colour of the message bar: red for a problem, green for something done
 * and read back, amber for an instruction the reader has to act on now ---
 * "Drag a rectangle where the signature should appear". Until 2026-11-13 there
 * were only the first two, and that instruction was drawn red, as a failure.
 */
export type MessageTone = "problem" | "success" | "attention";

/** The tone of a result: green when it succeeded, red otherwise. */
export function resultTone(success: boolean): MessageTone {
  return success ? "success" : "problem";
}

/** One line of a message. */
export interface MessageLine {
  /** `Timestamp`, for a line `Timestamp: …`; `null` for the first line and for a line with no label. */
  label: string | null;
  /** What follows the label, up to the first ` — `. */
  value: string;
  /** What follows the first ` — `, or `null` when there is none. */
  detail: string | null;
}

/** What separates a label from its value. */
export const LABEL_END = ": ";
/** What separates a verdict from its explanation. */
export const DETAIL_START = " — ";

/**
 * Longest a label is: `Widerruf des Zeitstempeldienstes` is 32 characters. A
 * colon further in is part of a sentence, not the end of a label.
 */
const LABEL_MAX = 40;

/** The lines of `message`; one line for a message without a line break. */
export function messageLines(message: string): MessageLine[] {
  return message.split("\n").map((line, at) => {
    if (at === 0) return { label: null, value: line, detail: null };
    const end = line.indexOf(LABEL_END);
    const labelled = end > 0 && end <= LABEL_MAX;
    const label = labelled ? line.slice(0, end) : null;
    const rest = labelled ? line.slice(end + LABEL_END.length) : line;
    const split = rest.indexOf(DETAIL_START);
    return split < 0
      ? { label, value: rest, detail: null }
      : { label, value: rest.slice(0, split), detail: rest.slice(split + DETAIL_START.length) };
  });
}

/** One line put back together: the inverse of {@link messageLines} for that line. */
export function joinedLine(line: MessageLine): string {
  const head = line.label === null ? "" : `${line.label}${LABEL_END}`;
  const tail = line.detail === null ? "" : `${DETAIL_START}${line.detail}`;
  return `${head}${line.value}${tail}`;
}
