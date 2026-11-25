/**
 * The disk watch waits for the message bar, in the running application:
 * `tabs_check.py --phase disk-change`.
 *
 * `unasked.ts` says a file that changed on disk is asked about, or reloaded
 * unasked, only once the bar is empty, and `App.svelte` asks `promptUnaskedNow`
 * in the callback it hands `DiskWatch`. `diskwatch.test.ts` holds the watch and
 * `unasked.test.ts` the rule; the join between them and the bar lives in the
 * component, which no unit test imports. What only the window can show is that
 * a real message on screen holds the prompt back, and that the prompt comes
 * once the reader has dismissed it.
 *
 * **The webview cannot write a file**, so the change is made from outside:
 * {@link REWRITE_REQUEST} is a line this prints, and `tabs_check.py` appends a
 * PDF comment to the open copy each time it reads one. Appended rather than
 * replaced, because a truncation of a file another process has mapped is
 * refused on Windows, and the comparison is by content: a timestamp alone is
 * not a change (`docs/THREAT-MODEL.md`, the paragraph on the disk watch).
 * Whether the bytes landed is read through the application's own
 * `document_stamp`, so a request nobody answered fails here, by name.
 *
 * Three passes on one document, each with a change of its own:
 *
 * 1. The control. The bar is empty, so the prompt comes within a few checks.
 *    Without it the two passes after it would pass on a window whose watch never
 *    runs (a hidden page, a mode left at `ignore`).
 * 2. Mode `ask` with a message on screen: after longer than the control took,
 *    and at least two checks more, the message is still the one put there and
 *    no Reload is offered. Dismissed, the prompt comes.
 * 3. Mode `reload` with a message on screen: the document is not reloaded while
 *    the message stands, and is once it is dismissed.
 *
 * The message is one a reader gets by asking: *Open the previous PDF in this
 * folder* from the first of the two copies, which has none before it.
 */

import { call } from "./ipc";
import { afterDiskChange, readDiskChangeMode, WATCH_INTERVAL_MS } from "./diskwatch";
import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";

/**
 * The line that asks `tabs_check.py` to change the open copy on disk. The
 * script matches it as written, so it is exported for the test that holds the
 * two files to one spelling.
 */
export const REWRITE_REQUEST = "[disk-change] rewrite";

/**
 * How long a message is left on screen before the check says it held.
 *
 * Longer than the control's own answer took, so the wait is measured against
 * this window and this machine rather than a guess, and never less than two
 * intervals past the three checks the watch needs to act on a change (one to
 * see the new stamp, one to see it hold, one to report). A shorter wait would
 * pass because the watch had not got round to it yet, which is the failure
 * looking like the success.
 */
export function holdFor(controlMs: number): number {
  return Math.max(controlMs, 3 * WATCH_INTERVAL_MS) + 2 * WATCH_INTERVAL_MS;
}

const SETTLE_MS = 20_000;
/** How long a change may take to be seen once nothing holds it back. */
const ANSWER_MS = 10_000;

/** The sentence the prompt ends in, without the name, in the window's language. */
function promptTail(): string {
  const prompt = afterDiskChange("ask", false, "");
  return prompt !== null && prompt !== "reload" ? prompt.message.trim() : "";
}

export async function diskChangeCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [first, second] = expected.split("|");
  if (!first || !second) throw new Error("two disposable copies in one folder required");
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent?.trim() ?? "";
  const reloadOffer = () => document.querySelector<HTMLButtonElement>('[data-testid="offer-requestReload"]');
  const dismiss = () => document.querySelector<HTMLButtonElement>('[data-testid="problem-dismiss"]')?.click();
  const doc = () => host.edits()?.doc ?? -1;
  const tail = promptTail();
  // `includes`, not `endsWith`: the bar's text is read off the element that
  // also holds the offers, so the buttons' labels follow the sentence. The
  // first run in a window failed on exactly that, with the prompt on screen.
  const asking = () => reloadOffer() !== null && tail !== "" && shown().includes(tail);
  const state = () =>
    `bar "${shown().slice(0, 120)}", reload offered ${reloadOffer() !== null}, document ${doc()}, ` +
    `page ${document.visibilityState}`;

  let requests = 0;
  /** Asks for a change and waits until the application's own stamp has moved. */
  const rewrite = async (label: string): Promise<void> => {
    const path = host.path();
    const stamp = async () => (await call("document_stamp", { doc: doc(), path }))[1] ?? null;
    const before = await stamp();
    requests += 1;
    report.emit(`${REWRITE_REQUEST} ${requests}`);
    const deadline = performance.now() + SETTLE_MS;
    while ((await stamp()) === before) {
      if (performance.now() > deadline) throw new Error(`${label}: the copy on disk did not change; is tabs_check.py answering ${REWRITE_REQUEST}?`);
      await pause(100);
    }
  };

  // The reader's own choice is put back at the end: the mode is kept in the
  // webview's storage, which a checks build may share with the reader's.
  const kept = readDiskChangeMode();
  report.emit("[disk-change] opening the first copy");
  await host.open(first);
  await host.idle();
  if (!await settle(() => host.viewer()?.idle === true, SETTLE_MS)) throw new Error("the viewer did not settle");
  // Refused when it is already the mode, which leaves the mode as wanted.
  host.run("file.onDiskChange.ask");

  try {
    // 1. The control: nothing on screen, so the change is asked about.
    const clear = shown() === "";
    report.check("the control starts with an empty bar", clear, state());
    if (!clear) return;
    await rewrite("control");
    const started = performance.now();
    const answered = await settle(asking, ANSWER_MS);
    const controlMs = performance.now() - started;
    report.check("with the bar empty, a change on disk is asked about at once", answered,
      `${Math.round(controlMs)} ms; ${state()}`);
    if (!answered) return;
    const invited = doc();
    await host.apply((edits) => edits.rotate(edits.state.pages[0]!.id, 1));
    reloadOffer()?.click();
    const discard = () => document.querySelector<HTMLButtonElement>('[data-testid="offer-reload"]');
    report.check("Reload checks edits made after the invitation",
      await settle(() => discard() !== null, SETTLE_MS) && doc() === invited
        && document.querySelector('[data-testid="offer-saveCopy"]') !== null, state());
    discard()?.click();
    report.check("the confirmed discard reloads the document",
      await settle(() => doc() >= 0 && doc() !== invited, SETTLE_MS), state());
    await host.idle();

    // 2. Mode `ask`, a message on screen.
    host.run("file.previousInFolder");
    await settle(() => shown() !== "", SETTLE_MS);
    const message = shown();
    report.check("the reader's own message is on screen", message !== "" && !asking(), state());
    const opened = doc();
    await rewrite("ask");
    const held = holdFor(controlMs);
    await pause(held);
    report.check("the message stays while the file has changed on disk",
      shown() === message && reloadOffer() === null && doc() === opened, `${held} ms later: ${state()}`);
    dismiss();
    report.check("once it is dismissed, the change is asked about", await settle(asking, ANSWER_MS), state());
    report.check("and nothing was reloaded while it was asked", doc() === opened, state());
    dismiss();
    await settle(() => shown() === "", SETTLE_MS);

    // 3. Mode `reload`, a message on screen: the reload nobody is asked about
    // clears the bar on the way in, so it waits too.
    host.run("file.onDiskChange.reload");
    host.run("file.previousInFolder");
    await settle(() => shown() !== "", SETTLE_MS);
    const again = shown();
    report.check("the message is on screen again", again !== "" && !asking(), state());
    const before = doc();
    await rewrite("reload");
    await pause(held);
    report.check("an automatic reload waits while a message is on screen",
      shown() === again && doc() === before, `${held} ms later: ${state()}`);
    dismiss();
    const reloaded = await settle(() => doc() !== before && doc() >= 0, ANSWER_MS);
    report.check("and reloads once it is dismissed", reloaded && host.path() === first, state());
  } finally {
    host.run(`file.onDiskChange.${kept}`);
  }
}
