/**
 * Real input that reached the window while a window check was waiting.
 *
 * A window check takes the focus, and on a machine someone is working at, a
 * key they press can land in the check's window instead of their own. One Page
 * Down during `opencheck.ts`'s quiet pause moves the view from page 12 to 13,
 * and the check then reports a place that did not hold: a defect in the app,
 * by its wording. Measured 2026-11-12 on Windows 11 by pressing Page Down into
 * that pause, 5 failures in 5; the same phase passed 30 of 30 on a machine
 * nobody was typing at.
 *
 * Only `isTrusted` events count, so a check's own `dispatchEvent` calls are
 * never mistaken for a person.
 */

/** Starts listening on `target`; the returned function stops and says what came. */
export function listenForOutsideInput(target: EventTarget): () => string[] {
  const heard: string[] = [];
  const key = (event: Event): void => {
    if (event.isTrusted) heard.push((event as KeyboardEvent).key);
  };
  const wheel = (event: Event): void => {
    if (event.isTrusted) heard.push("wheel");
  };
  target.addEventListener("keydown", key, true);
  target.addEventListener("wheel", wheel, true);
  return () => {
    target.removeEventListener("keydown", key, true);
    target.removeEventListener("wheel", wheel, true);
    return heard;
  };
}

/** The detail to add to a failed check, or `""` when nothing came. */
export function outsideInputNote(heard: readonly string[]): string {
  if (heard.length === 0) return "";
  const what = heard.length === 1 ? "an input" : `${heard.length} inputs`;
  return `; the window received ${what} from outside the check meanwhile (${heard.join(", ")}): someone at the machine was using it`;
}
