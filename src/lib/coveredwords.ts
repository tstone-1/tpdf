/**
 * The words each mark covers, by the model's id for it, for the marks panel.
 *
 * **Not in the model.** The model holds what the document will become, which
 * this is not: a saved PDF has no entry for the text a highlight sits on, so
 * this could never be read back and would be a field `save.rs` had to remember
 * to ignore. Nor reactive: the panel is imperative DOM repainted by hand.
 *
 * Filled by `markSelection` in `App.svelte`, which is the only way a mark that
 * covers words is made. Kept until the document changes rather than pruned
 * against the live marks: an undone mark comes back under the same id --- the
 * id is in the journalled command --- so a list pruned on undo would have redo
 * show "No note" for a highlight whose words it had just been displaying. The
 * cost is an entry per mark removed and not redone, each capped at
 * {@link COVERED_CHARS}.
 *
 * **Keyed by mark id, never by page slot.** A slot is renumbered by the next
 * deletion; a mark id is not (`docs/TRAPS.md`, *An id and a slot are both
 * `number`, so a mark drawn on the last page vanished*).
 *
 * One per mounted document (`LiveDocument.covered`), one kept per tab while it
 * has no viewer (`DocumentTab.covered`), and one shared by the two views of a
 * document shown on both sides, which `sharedByTwin` hands the second view.
 * Moved here from `App.svelte` so that what it keeps, and that a kept copy does
 * not follow the live one, can be tested.
 */

/**
 * Longest covered text kept per mark, in characters.
 *
 * The row is one line and the CSS ellipsis cuts it far shorter than this, so
 * nothing visible is lost. What it bounds is a reader who selects a hundred
 * dense pages and highlights them: the whole of every page would otherwise be
 * held here, beside the copy `TextCache` is already holding under its own
 * bound.
 */
export const COVERED_CHARS = 200;

export class CoveredWords {
  /** By mark id. Not `#private`, so that two copies compare by what they hold. */
  private readonly words = new Map<number, string>();

  /** Keeps the words mark `id` covers, cut to {@link COVERED_CHARS}. */
  note(id: number, text: string): void {
    this.words.set(id, text.slice(0, COVERED_CHARS));
  }

  /** The words mark `id` covers, or `""` for a mark with none known. */
  of(id: number): string {
    return this.words.get(id) ?? "";
  }

  /** How many marks have words. */
  get size(): number {
    return this.words.size;
  }

  /**
   * A copy, for a tab to keep when the reader switches away: the live one
   * goes on being written while another document is in front.
   */
  copy(): CoveredWords {
    const kept = new CoveredWords();
    for (const [id, words] of this.words) kept.words.set(id, words);
    return kept;
  }

  /**
   * Holds exactly what `kept` holds, in this object. In place rather than
   * replaced, because a second view of the document may share this one.
   */
  restore(kept: CoveredWords): void {
    if (kept === this) return;
    this.words.clear();
    for (const [id, words] of kept.words) this.words.set(id, words);
  }
}
