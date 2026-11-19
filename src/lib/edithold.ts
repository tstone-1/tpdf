/**
 * What happens to an edit `applyEdit` in `App.svelte` refused before it ran.
 *
 * `applyEdit` answers null without running the edit while a document task or
 * a hold is up: a save, a copy, an update downloading. Most edits cannot be
 * asked for then, since the commands that make them are greyed. Two kinds
 * still arrive, and each lost something in silence:
 *
 * - **Words typed in the box beside a mark or a comment.** The box can be
 *   opened and typed in while an update downloads, and the words are sent
 *   when it closes. Refused, they were gone with no word: the page drew them
 *   until the refusal and then the model's old text.
 * - **An insert from another file.** The file is prepared and held for the
 *   question that names its pages, and the answer is taken before the edit is
 *   asked for. Refused, the prepared file was released only when the document
 *   closed.
 *
 * {@link ranOr} is how both learn that the edit never ran.
 */

import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    notAdded: (words: string) =>
      `Not added, because the document cannot be changed while a task is running: “${words}”. Type it again once the task has finished.`,
    notCleared: "Not removed, because the document cannot be changed while a task is running. Remove it again once the task has finished.",
  },
  "de-DE": {
    notAdded: (words: string) =>
      `Nicht hinzugefügt, weil das Dokument nicht geändert werden kann, solange eine Aufgabe läuft: „${words}“. Gib es noch einmal ein, wenn die Aufgabe beendet ist.`,
    notCleared: "Nicht entfernt, weil das Dokument nicht geändert werden kann, solange eine Aufgabe läuft. Entferne es noch einmal, wenn die Aufgabe beendet ist.",
  },
  "de-DE-formal": {
    notAdded: (words: string) =>
      `Nicht hinzugefügt, weil das Dokument nicht geändert werden kann, solange eine Aufgabe läuft: „${words}“. Geben Sie es noch einmal ein, wenn die Aufgabe beendet ist.`,
    notCleared: "Nicht entfernt, weil das Dokument nicht geändert werden kann, solange eine Aufgabe läuft. Entfernen Sie es noch einmal, wenn die Aufgabe beendet ist.",
  },
});

/**
 * Asks `apply` to run `edit`, and calls `notRun` when it answers without
 * having run it: refused before it was queued, or for a document that had
 * gone by the time its turn came. Answers what `apply` answered.
 *
 * `notRun` is not called for an edit that ran and failed: the model's refusal
 * is said where the edit runs, and an import the model refused is released by
 * the backend.
 */
export async function ranOr<M, S>(
  apply: (edit: (model: M) => Promise<S>) => Promise<S | null>,
  edit: (model: M) => Promise<S>,
  notRun: () => void,
): Promise<S | null> {
  let ran = false;
  const answer = await apply((model) => {
    ran = true;
    return edit(model);
  });
  if (!ran) notRun();
  return answer;
}

/**
 * What the reader is told when what they typed in a box was not taken. The
 * words are in the sentence, since the box they were typed in has closed; a
 * box emptied to remove its words is said as that.
 */
export function typedNotTaken(words: string): string {
  return words.trim() === "" ? TEXT().notCleared : TEXT().notAdded(words);
}
