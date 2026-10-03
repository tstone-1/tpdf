/**
 * *Recognise text and save as*, through the window a reader uses it in.
 *
 * `tests/cli/ocr.rs` runs the command's backend on a scan and compares the
 * words; what no gate reaches is `App.svelte`: the palette entry, the save
 * panel's suggestion, the line that says which page is being read, the Stop
 * button, the sentence afterwards and the copy being opened. This phase is
 * that half. The save panel is answered by `saveanswer.ts`, as a signing's is.
 *
 * `expected` is `scan|long|directory`: a one-page scan whose picture shows
 * `WORD`, the same page several times over, and an empty directory for the
 * copies. `scripts/tabs_check.py --phase recognise` makes all three and checks
 * afterwards which files the directory holds.
 */

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import { SAVE_FIRST, suggestedName } from "./recognise";

/** A word `testdata/text-base14.pdf` draws once, which the scan is made from. */
const WORD = "quartz";

const SETTLE_MS = 20_000;
const DIALOG_MS = 5000;
/** A first recognition on a machine compiles the engine's models: 24 s measured. */
const READ_MS = 120_000;

export async function recogniseCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [scan, long, directory] = expected.split("|");
  if (!scan || !long || !directory) throw new Error("a scan, a longer scan and a directory are required");
  const at = (name: string) => `${directory}/${name}`;
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  const task = () => document.querySelector('[data-testid="blocking-task"]')?.textContent ?? "";
  const stop = () => document.querySelector<HTMLButtonElement>('[data-testid="stop-recognition"]');
  const quiet = async () => {
    if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) throw new Error("the viewer did not settle");
    await pause(100);
  };
  /** How many matches a search of the open document finds. */
  const found = async (word: string) => {
    host.viewer()!.search(word);
    if (!(await settle(() => !host.viewer()!.searching, SETTLE_MS))) throw new Error("the search did not finish");
    return host.viewer()!.searchMatches.length;
  };
  /** The command through the palette, opened from the toolbar and typed into. */
  const run = async () => {
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "Commands",
    );
    if (!button) throw new Error("the toolbar has no Commands button");
    button.click();
    const field = () => document.querySelector<HTMLInputElement>(".tpdf-palette input");
    if (!(await settle(() => field() !== null && field()!.offsetParent !== null, DIALOG_MS))) {
      throw new Error("the palette did not open");
    }
    field()!.value = "Recognise text";
    field()!.dispatchEvent(new InputEvent("input", { bubbles: true }));
    await pause(50);
    const highlighted =
      document.querySelector(".tpdf-palette [role=option][aria-selected=true]")?.textContent ?? "";
    if (!highlighted.includes("Recognise text and save as")) {
      throw new Error(`the palette highlights "${highlighted}"`);
    }
    field()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  };

  // ---- 1. The control: the scan has nothing to find.
  await host.open(scan); await host.idle(); await quiet();
  report.check("the control: a search of the scan finds nothing", (await found(WORD)) === 0, WORD);
  report.check("the message area starts clear", shown() === "", shown().slice(0, 80));

  // ---- 2. Recognise, with every line the toolbar shows collected on the way.
  const lines = new Set<string>();
  let stoppable = false;
  const watch = setInterval(() => {
    if (task()) lines.add(task());
    if (stop()) stoppable = true;
  }, 10);
  const copy = at("copy.pdf");
  host.answerSave(copy);
  await run();
  const said = await settle(() => shown().includes("Text was added"), READ_MS);
  clearInterval(watch);
  report.check("the recognition ends on its sentence", said, shown().slice(0, 200));
  report.check("which names the copy and counts its one page",
    shown().includes("Saved copy.pdf. Text was added to 1 of 1 page ("), shown().slice(0, 200));
  const suggestions = host.saveSuggestions();
  report.check("the save panel suggests <name> searchable.pdf",
    suggestions[suggestions.length - 1] === suggestedName(scan), suggestions.join(", "));
  report.check("the toolbar said which page was being read",
    [...lines].some((line) => line === "Recognising text: page 1 of 1..."), [...lines].join(" / "));
  report.check("and offered to stop while it did", stoppable, String(stoppable));
  report.check("the line and the button are gone afterwards", task() === "" && stop() === null, task());

  // ---- 3. The copy is what the window now shows, and it can be searched.
  await host.idle(); await quiet();
  report.check("the copy is the open document", host.path() === copy, host.path());
  report.check("a search of the copy finds the word", (await found(WORD)) > 0, WORD);

  // ---- 4. Unsaved changes: told to save, and no panel is asked for.
  await host.open(scan); await host.idle(); await quiet();
  const asked = host.saveSuggestions().length;
  await host.apply((edits) => edits.rotate(0, 1));
  // Asserted before the command runs: with no edit in the model the command
  // would ask for a name, nothing is queued, and a native panel would open.
  if (!host.edits()?.dirty) throw new Error("the rotation did not reach the model");
  await run();
  const refused = await settle(() => shown().trim() === SAVE_FIRST, DIALOG_MS);
  report.check("a document with unsaved changes is told to save first", refused, shown().slice(0, 120));
  report.check("before any name is asked for", host.saveSuggestions().length === asked,
    `${host.saveSuggestions().length} against ${asked}`);
  await host.apply((edits) => edits.undo());

  // ---- 5. Stop: pressed as soon as it is offered, on a document long enough
  //         for the press to land between two pages.
  await host.open(long); await host.idle(); await quiet();
  host.answerSave(at("stopped.pdf"));
  await run();
  const offered = await settle(() => stop() !== null, READ_MS);
  stop()?.click();
  const stopped = await settle(() => shown().includes("was stopped"), READ_MS);
  report.check("Stop is offered and ends the recognition with its own sentence",
    offered && stopped && shown().includes("No copy was written"), shown().slice(0, 120));
  report.check("the document that was being read is still the open one", host.path() === long, host.path());
  report.check("and nothing is left running", task() === "" && stop() === null, task());
}
