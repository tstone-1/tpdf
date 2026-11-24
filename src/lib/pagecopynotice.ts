/**
 * What the window says after *Copy page for PowerPoint*.
 *
 * `pagecopy.rs` leaves out every text a clip cuts or something drawn after
 * it covers, because PowerPoint's *Ungroup* drops the clip and makes the
 * cover a shape that can be moved off the word, and the command
 * answers how many it left out. When that is not 0 the notice says so, so a
 * reader reusing a diagram knows a label may be missing from the slide. The
 * count includes text on optional layers whose visibility cannot be verified.
 */

import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    pageCopied: (page: number) =>
      `Copied page ${page} for PowerPoint. Paste it there and choose Ungroup to edit its shapes and text.`,
    leftOut: (count: number) =>
      count === 1
        ? " One piece of text is left out: tpdf could not confirm that the whole text was visible."
        : ` ${count} pieces of text are left out: tpdf could not confirm that each was fully visible.`,
  },
  "de-DE": {
    pageCopied: (page: number) =>
      `Seite ${page} für PowerPoint kopiert. Füge sie dort ein und wähle „Gruppierung aufheben“, um Formen und Text zu bearbeiten.`,
    leftOut: (count: number) =>
      count === 1
        ? " Ein Textstück fehlt in der Kopie: tpdf konnte nicht feststellen, ob es vollständig sichtbar war."
        : ` ${count} Textstücke fehlen in der Kopie: tpdf konnte nicht feststellen, ob alle vollständig sichtbar waren.`,
  },
  "de-DE-formal": {
    pageCopied: (page: number) =>
      `Seite ${page} für PowerPoint kopiert. Fügen Sie sie dort ein und wählen Sie „Gruppierung aufheben“, um Formen und Text zu bearbeiten.`,
  },
});

/** The notice for page `page` (1-based), copied with `leftOut` texts left out. */
export function pageCopyNotice(page: number, leftOut: number, locale?: Parameters<typeof TEXT>[0]): string {
  const text = TEXT(locale);
  return text.pageCopied(page) + (leftOut > 0 ? text.leftOut(leftOut) : "");
}
