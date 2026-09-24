/**
 * The size of the document tabs' labels, which the reader can step up and down.
 *
 * A per-viewer convenience, so it lives in `localStorage` and every read and
 * write is guarded: storage that throws or holds something unexpected leaves
 * the default, and a write that fails still changes the size for this session.
 */

/** The sizes offered, in CSS pixels, smallest first. */
export const TAB_LABEL_SIZES: readonly number[] = [9, 10, 11, 12, 13, 14, 16];
const SMALLEST = 9;
const LARGEST = 16;
/** The default: smaller than the 13 px body text, which made the strip tall. */
export const DEFAULT_TAB_LABEL_SIZE = 11;

const KEY = "tpdf.tabLabelSize";

type Store = Pick<Storage, "getItem" | "setItem">;

export class TabLabelSize {
  #px: number = DEFAULT_TAB_LABEL_SIZE;

  constructor(private storage: () => Store = () => window.localStorage) {
    try {
      const saved = Number(this.storage().getItem(KEY));
      if (TAB_LABEL_SIZES.includes(saved)) this.#px = saved;
    } catch { /* The default stands. */ }
  }

  /** The current size in CSS pixels. */
  get px(): number { return this.#px; }

  get canGrow(): boolean { return this.#px < LARGEST; }
  get canShrink(): boolean { return this.#px > SMALLEST; }
  get isDefault(): boolean { return this.#px === DEFAULT_TAB_LABEL_SIZE; }

  /** One step larger or smaller (`1` or `-1`), or back to the default (`0`). */
  step(direction: -1 | 0 | 1): number {
    const at = TAB_LABEL_SIZES.indexOf(this.#px);
    const next = direction === 0
      ? DEFAULT_TAB_LABEL_SIZE
      : TAB_LABEL_SIZES[Math.min(TAB_LABEL_SIZES.length - 1, Math.max(0, at + direction))]
        ?? DEFAULT_TAB_LABEL_SIZE;
    this.#px = next;
    try { this.storage().setItem(KEY, String(next)); } catch { /* Kept for this session. */ }
    return next;
  }
}
