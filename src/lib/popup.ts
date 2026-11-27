/**
 * Where a popup goes, for the two that hang off a mark on the page.
 *
 * One implementation, because the placement rule is the whole of what makes a
 * popup feel attached to what it belongs to: preferred to the right of the mark
 * and level with its top, flipped to the left when there is no room, and clamped
 * into the window either way. Two copies would agree about the middle of a page
 * and disagree at the edges --- which is the only place the rule does anything.
 *
 * The two callers are `commentpopup.ts`, which shows what a stranger wrote in
 * the file, and `markpopup.ts`, which edits what the reader wrote themselves.
 * Both wait for the document model to accept edits before replacing the editor.
 */

/** Width of a popup, in CSS pixels. */
export const POPUP_WIDTH = 280;

/** Gap between the mark and the popup, in CSS pixels. */
const GAP = 10;

/** Margin kept between the popup and the window's edges, in CSS pixels. */
const MARGIN = 8;

/** A rectangle in the host's coordinates, which is what the viewer computes. */
export interface Anchor {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Moves `element` next to `at`, without rebuilding it.
 *
 * Called every frame while a popup is open, so it does no work beyond two style
 * writes --- and it reads the host's size each time rather than caching it,
 * because the window can be resized while a popup is open and a cached size
 * would put the clamp somewhere the window no longer is.
 */
export function place(host: HTMLElement, element: HTMLElement, at: Anchor): void {
  const width = host.clientWidth;
  const height = host.clientHeight;
  const own = element.offsetHeight || 0;

  // To the right of the mark, or to its left when there is no room. Compared
  // against the *window*, not against the page: a page narrower than the
  // window has room to its right even when the mark is at its edge.
  const rightOf = at.right + GAP;
  const left =
    rightOf + POPUP_WIDTH + MARGIN <= width
      ? rightOf
      : Math.max(MARGIN, at.left - GAP - POPUP_WIDTH);

  const top = Math.max(
    MARGIN,
    Math.min(at.top, Math.max(MARGIN, height - own - MARGIN)),
  );

  element.style.left = `${Math.round(left)}px`;
  element.style.top = `${Math.round(top)}px`;
}

/** One in-flight journal edit per popup. Only the latest navigation request
 * may replace its editor, and only once the journal accepts the edit. */
export class PopupSubmission {
  pending: Promise<void> | undefined;
  private request = 0;
  private rejected = false;

  // Scrolling can request a close on every frame. Wait for an explicit retry
  // after a refusal and never replace navigation already waiting on a write.
  get canCloseOnScroll(): boolean { return !this.pending && !this.rejected; }

  cancelNavigation(): void {
    this.request++;
    this.rejected = false;
  }

  after(save: () => void | Promise<void>, accepted: () => void, refused: () => void): void {
    const request = ++this.request;
    let pending: void | Promise<void>;
    try {
      pending = save();
    } catch {
      refused();
      return;
    }
    if (pending) {
      void pending.then(
        () => { if (request === this.request) accepted(); },
        () => { if (request === this.request) refused(); },
      );
    } else accepted();
  }

  write(field: { readOnly: boolean }, write: () => unknown, refused: () => void): void | Promise<void> {
    this.rejected = false;
    const readOnly = field.readOnly;
    field.readOnly = true;
    function check(answer: unknown): void {
      if (typeof answer === "string") throw new Error(answer);
    }
    try {
      const answer = write();
      if (answer && typeof (answer as PromiseLike<unknown>).then === "function") {
        this.pending = Promise.resolve(answer).then(check).catch(error => {
          this.rejected = true;
          refused();
          throw error;
        }).finally(() => {
          field.readOnly = readOnly;
          this.pending = undefined;
        });
        return this.pending;
      }
      check(answer);
    } catch (error) {
      this.rejected = true;
      refused();
      throw error;
    } finally {
      if (!this.pending) field.readOnly = readOnly;
    }
  }
}
