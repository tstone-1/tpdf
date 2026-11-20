/**
 * The reasons a redaction is *not verified*, grouped by what each asks of the
 * reader.
 *
 * Until 26.11.11 the reasons were one run of text joined by semicolons, and
 * after a copy the window showed only "Redaction not verified. Create an
 * image-only copy" with the reasons folded under *Technical details*. A reader
 * could not tell a copy of the name still on page 4, which they can fix, from a
 * part of the file the check cannot read, which they cannot. An external review
 * of 26.11.10 named that as the gap between a verdict and a document the reader
 * can share.
 *
 * ## The kind comes from Rust, never from the sentence
 *
 * `redact::ReasonKind` is set by the reader that produced each reason
 * (`redaction_steps::judge`), and `redact::Applied::kinds` carries one per
 * reason in `why`. A reason is translated and reworded over time, so a pattern
 * over its words would drift; the reader that wrote it knows what it found.
 *
 * ## A reply without kinds keeps the old sentence
 *
 * `kinds` is absent from an older reply and from the command-line tool's
 * report, and `cliwording.test.ts` holds the window's sentence equal to the
 * tool's for those. {@link reviewGroups} answers `null` whenever the kinds do
 * not pair one for one with the reasons, and the caller then says what it
 * always said. A list that does not pair is never guessed at.
 *
 * ## The lines are message-bar lines
 *
 * Each group is one `Label: advice — reasons` line, which `messagelines.ts`
 * lays out with the label in a column of its own. The advice comes before the
 * first ` — ` because a reason can hold one (the German of several does); the
 * split is at the first, so the reasons stay whole.
 */

import { catalog, type UiLocale } from "./i18n";
import { DETAIL_START, LABEL_END } from "./messagelines";

/** Mirrors `redact::ReasonKind`. */
export type ReasonKind = "found" | "pictures" | "unchecked";

/** The order the groups are shown in: what the reader can fix comes first. */
export const REVIEW_ORDER: readonly ReasonKind[] = ["found", "pictures", "unchecked"];

/** The reasons of one kind, in the order they arrived. */
export interface ReviewGroup {
  kind: ReasonKind;
  reasons: string[];
}

const TEXT = catalog({
  "en-US": {
    foundLabel: "Still in the file",
    foundAdvice: "mark it where named and redact the original again",
    picturesLabel: "Not read in pictures",
    picturesAdvice:
      "check these pictures yourself; an image-only copy removes everything inside the marked regions",
    uncheckedLabel: "Could not be checked",
    uncheckedAdvice:
      "the marked regions were removed, but the check cannot read this part of the file",
  },
  "de-DE": {
    foundLabel: "Noch in der Datei",
    foundAdvice: "markiere es an den genannten Stellen und schwärze das Original erneut",
    picturesLabel: "In Bildern nicht gelesen",
    picturesAdvice:
      "prüfe diese Bilder selbst; eine Kopie nur aus Bildern entfernt alles in den markierten Bereichen",
    uncheckedLabel: "Nicht prüfbar",
    uncheckedAdvice:
      "die markierten Bereiche wurden entfernt, aber diesen Teil der Datei kann die Prüfung nicht lesen",
  },
  "de-DE-formal": {
    foundAdvice: "markieren Sie es an den genannten Stellen und schwärzen Sie das Original erneut",
    picturesAdvice:
      "prüfen Sie diese Bilder selbst; eine Kopie nur aus Bildern entfernt alles in den markierten Bereichen",
  },
});

const KNOWN = new Set<string>(REVIEW_ORDER);

/**
 * `why` grouped by `kinds`, in {@link REVIEW_ORDER}, with empty groups left
 * out; `null` when the two do not pair one for one, or a kind is not one this
 * window knows.
 */
export function reviewGroups(
  why: readonly string[],
  kinds: readonly string[] | undefined,
): ReviewGroup[] | null {
  if (kinds === undefined || kinds.length !== why.length || why.length === 0) return null;
  if (!kinds.every((kind) => KNOWN.has(kind))) return null;
  return REVIEW_ORDER.map((kind) => ({
    kind,
    reasons: why.filter((_, at) => kinds[at] === kind),
  })).filter((group) => group.reasons.length > 0);
}

/** One message-bar line per group: `Label: advice — reason; reason`. */
export function reviewLines(groups: readonly ReviewGroup[], locale?: UiLocale): string[] {
  const text = TEXT(locale);
  const words: Record<ReasonKind, [string, string]> = {
    found: [text.foundLabel, text.foundAdvice],
    pictures: [text.picturesLabel, text.picturesAdvice],
    unchecked: [text.uncheckedLabel, text.uncheckedAdvice],
  };
  return groups.map((group) => {
    const [label, advice] = words[group.kind];
    return `${label}${LABEL_END}${advice}${DETAIL_START}${group.reasons.join("; ")}`;
  });
}
