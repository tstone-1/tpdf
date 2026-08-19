/**
 * A pointer drag: capture it, follow it, and know when it ends.
 *
 * ## Why this exists
 *
 * `viewer.ts` had two drags written out longhand --- the text selection and the
 * scrollbar thumb --- and each one hand-rolled the same four steps: capture the
 * pointer, add a `pointermove` and a `pointerup` listener, do the work, take
 * both listeners away again and release. Neither is complicated and both are
 * correct. What made a third copy worth refusing is that the *next* four are
 * already named in `docs/PLAN.md`: a rectangle a reader drags to crop, and ink,
 * shapes and text boxes, each of which that document records as blocked on "a
 * way to draw rather than a way to select".
 *
 * The trap index has *"two copies of a distinction drift, and a mutation of one
 * survives"*. Six copies of a listener pair is the same shape, and the failure
 * is quiet: a drag that forgets one `removeEventListener` goes on tracking the
 * pointer after the button is up, which looks like a viewer that has become
 * sticky rather than like a missing line.
 *
 * ## What it does not do
 *
 * It has no idea what is being dragged. It reports two client coordinates and a
 * verdict, and every question about pages, points, zoom or rotation belongs to
 * the caller --- which is why this file imports nothing. A drag over a page and
 * a drag down a scrollbar want completely different arithmetic and exactly the
 * same lifecycle, and this is the lifecycle.
 *
 * ## Cancelling is a first-class outcome, not an error
 *
 * {@link DragTarget.end} takes a `committed` flag rather than the caller reading
 * some other state to find out. A drag ends three ways --- the button comes up,
 * the browser takes the pointer away (`pointercancel`, which is what a touch
 * gesture turning into a scroll produces), or something asks it to stop, which
 * is what Escape does. Only the first means "do it". Handing back a rectangle
 * with no way to say which of the three happened is how an Escape ends up
 * drawing a box.
 */

/** The two coordinates a drag reads. A `PointerEvent` is one of these. */
export interface DragPoint {
  readonly clientX: number;
  readonly clientY: number;
}

/** What a caller does with a drag. */
export interface DragTarget {
  /**
   * The drag is starting. Return `false` to refuse it.
   *
   * A refusal is not a failure: it is how a caller says "not here" --- a press
   * that landed on no page, or a mode that is not armed. Nothing is captured
   * and no listener is added, so the press goes on to whatever would have had
   * it.
   */
  begin(at: DragPoint): boolean;
  /** The pointer moved. Called for every move between begin and end. */
  move(at: DragPoint): void;
  /**
   * The drag is over.
   *
   * `committed` is true only when the pointer was released normally. `at` is
   * the last point seen, which for a cancel is where the pointer was when it
   * was taken away rather than where it is now --- there is no "now" for a
   * cancel, and the last known point is the honest answer.
   */
  end(at: DragPoint, committed: boolean): void;
}

/**
 * One drag at a time, on one element.
 *
 * Construct it once and call {@link start} from a `pointerdown` handler. The
 * instance owns nothing between drags: an idle one has no listeners registered
 * and holds no capture.
 */
export class PointerDrag {
  private readonly host: HTMLElement;
  private readonly target: DragTarget;
  /** The live drag's pointer id and last point, or `null` when idle. */
  private live: { pointerId: number; at: DragPoint } | null = null;

  constructor(host: HTMLElement, target: DragTarget) {
    this.host = host;
    this.target = target;
  }

  /** Whether a drag is in progress. */
  get active(): boolean {
    return this.live !== null;
  }

  /**
   * Begins a drag from a `pointerdown`, or does nothing.
   *
   * Returns whether it took the press. A second press while one is live is
   * **refused rather than allowed to replace it**: two pointers on one surface
   * is a real thing (a second finger, a mouse and a pen), and the alternative
   * silently ends the first drag at wherever the second one started, which
   * commits a rectangle the reader did not draw.
   */
  start(event: PointerEvent): boolean {
    if (this.live) return false;
    const at = { clientX: event.clientX, clientY: event.clientY };
    if (!this.target.begin(at)) return false;
    this.live = { pointerId: event.pointerId, at };
    // Tolerated rather than required: `setPointerCapture` throws for a pointer
    // id the browser has no active pointer for, which is every id a synthetic
    // event carries, and the check harness dispatches synthetic ones. Losing
    // the capture costs a drag that ends at the window edge and nothing else.
    try {
      this.host.setPointerCapture(event.pointerId);
    } catch {
      // No such pointer.
    }
    this.host.addEventListener("pointermove", this.onMove);
    this.host.addEventListener("pointerup", this.onUp);
    this.host.addEventListener("pointercancel", this.onCancel);
    return true;
  }

  /**
   * Ends a live drag without committing it.
   *
   * Safe to call when nothing is live, which is what lets a caller wire it to
   * Escape without first asking whether a drag is in progress.
   */
  cancel(): void {
    this.finish(null, false);
  }

  /** Ends any live drag and leaves nothing registered. */
  dispose(): void {
    this.cancel();
  }

  private readonly onMove = (event: PointerEvent): void => {
    const live = this.live;
    if (!live || event.pointerId !== live.pointerId) return;
    live.at = { clientX: event.clientX, clientY: event.clientY };
    this.target.move(live.at);
  };

  private readonly onUp = (event: PointerEvent): void => {
    this.finish(event, true);
  };

  private readonly onCancel = (event: PointerEvent): void => {
    this.finish(event, false);
  };

  /**
   * Tears the drag down and tells the target, once.
   *
   * `event` is null for {@link cancel}, which has no event of its own; the
   * pointer id check is therefore skipped there, because a caller asking to
   * stop means the drag rather than one particular pointer.
   *
   * The listeners come off and `live` is cleared **before** the target is told,
   * so that a target whose `end` starts another drag --- or throws --- cannot
   * leave a half-registered one behind.
   */
  private finish(event: PointerEvent | null, committed: boolean): void {
    const live = this.live;
    if (!live) return;
    if (event && event.pointerId !== live.pointerId) return;
    const at = event ? { clientX: event.clientX, clientY: event.clientY } : live.at;
    this.live = null;
    this.host.removeEventListener("pointermove", this.onMove);
    this.host.removeEventListener("pointerup", this.onUp);
    this.host.removeEventListener("pointercancel", this.onCancel);
    try {
      this.host.releasePointerCapture(live.pointerId);
    } catch {
      // Never captured.
    }
    this.target.end(at, committed);
  }
}
