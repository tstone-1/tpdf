/**
 * *Save a smaller copy*, through the window a reader uses it in.
 *
 * `tests/cli/compress.rs` runs the writer and the estimate through the tool;
 * what no gate reaches is `App.svelte`: the palette entry, the dialog being
 * handed the estimate, each choice filling in with its size, the two pictures
 * of the page actually loading, the save panel coming after the dialog, and
 * the sentence afterwards. This phase is that half.
 *
 * `expected` is `document|directory`: a document whose one page is a picture
 * at 200 pixels an inch, and an empty directory for the copies.
 * `scripts/tabs_check.py --phase compress` makes both and reads the directory
 * afterwards.
 */

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import { suggestedName } from "./compress";
import { DIALOG_CLASS } from "./compressdialog";

const SETTLE_MS = 20_000;
const DIALOG_MS = 5000;
const ESTIMATE_MS = 60_000;
const WRITE_MS = 30_000;

export async function compressCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [source, directory] = expected.split("|");
  if (!source || !directory) throw new Error("a document and a directory are required");
  const at = (name: string) => `${directory}/${name}`;
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  const quiet = async () => {
    if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) throw new Error("the viewer did not settle");
    await pause(100);
  };
  /** A command through the palette, by the words a reader would type. */
  const run = async (typed: string, title: string) => {
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "Commands",
    );
    if (!button) throw new Error("the toolbar has no Commands button");
    button.click();
    const field = () => document.querySelector<HTMLInputElement>(".tpdf-palette input");
    if (!(await settle(() => field() !== null && field()!.offsetParent !== null, DIALOG_MS))) {
      throw new Error("the palette did not open");
    }
    field()!.value = typed;
    field()!.dispatchEvent(new InputEvent("input", { bubbles: true }));
    await pause(50);
    const highlighted =
      document.querySelector(".tpdf-palette [role=option][aria-selected=true]")?.textContent ?? "";
    if (!highlighted.includes(title)) throw new Error(`the palette highlights "${highlighted}"`);
    field()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  };
  const dialog = () => {
    const backdrop = document.querySelector<HTMLElement>(`.${DIALOG_CLASS}`);
    return backdrop && backdrop.style.display !== "none" ? backdrop : null;
  };
  const radios = () => [...(dialog()?.querySelectorAll<HTMLInputElement>("input[type=radio]") ?? [])];
  /** What each row says it comes to, in the dialog's order. */
  const outcomes = () =>
    radios().map((radio) => radio.parentElement?.children[2]?.textContent ?? "");
  const images = () => [...(dialog()?.querySelectorAll<HTMLImageElement>("img") ?? [])];
  const save = () =>
    [...(dialog()?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find(
      (b) => b.textContent === "Choose where to save...",
    );
  const choose = (index: number) => {
    const radio = radios()[index];
    if (!radio) throw new Error(`the dialog has no choice ${index}`);
    radio.click();
  };
  const press = (key: string) =>
    radios()[0]?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  const worked = () => {
    const said = outcomes().slice(0, 4);
    return said.length === 4 && said.every((text) => text !== "" && text !== "Working it out...");
  };

  // ---- 1. The dialog: four choices, each with its size.
  await host.open(source); await host.idle(); await quiet();
  const before = host.saveSuggestions().length;
  host.answerSave(at("small.pdf"));
  await run("smaller copy", "Save a smaller copy");
  const opened = await settle(() => radios().length === 5, DIALOG_MS);
  report.check("the command opens a dialog with four choices and the reader's own", opened, String(radios().length));
  if (!opened) return;
  report.check("starting on the choice that keeps every picture", radios()[0]!.checked, "the first radio");
  const sized = await settle(worked, ESTIMATE_MS);
  report.check("every choice is shown with what it comes to", sized, outcomes().join(" | "));
  if (!sized) return;
  report.check("and the three that shrink pictures are each smaller",
    outcomes().slice(1, 4).every((text) => / smaller$/.test(text)), outcomes().join(" | "));
  report.check("no name has been asked for yet", host.saveSuggestions().length === before,
    `${host.saveSuggestions().length} against ${before}`);

  // ---- 2. The smallest choice shows the page before and after.
  choose(3);
  const pictured = await settle(
    () => images().length === 2 && images().every((image) => image.complete && image.naturalWidth > 0),
    DIALOG_MS,
  );
  report.check("the smallest choice shows two pictures of the page, both loaded", pictured,
    images().map((image) => `${image.naturalWidth}x${image.naturalHeight}`).join(", "));
  const under = dialog()?.textContent ?? "";
  report.check("and says how far its picture is reduced",
    /goes from \d+ to 110 pixels an inch/.test(under), under.slice(-200));
  report.check("the two pictures differ", images()[0]?.src !== images()[1]?.src, "the same data");
  save()?.click();
  const said = await settle(() => shown().includes("Saved small.pdf"), WRITE_MS);
  report.check("the copy ends on a sentence with both sizes",
    said && /^Saved small\.pdf, about .+ where the document is .+\.$/.test(shown().trim()), shown().slice(0, 200));
  const suggestions = host.saveSuggestions();
  report.check("the save panel suggests <name> smaller.pdf",
    suggestions[suggestions.length - 1] === suggestedName(source), suggestions.join(", "));
  report.check("the dialog is gone and keeps no picture",
    dialog() === null && document.querySelectorAll(`.${DIALOG_CLASS} img[src]`).length === 0, "still shown");
  report.check("the open document is still the source", host.path() === source, host.path());

  // ---- 3. Dismissed: no name is asked for.
  const dismissedAt = host.saveSuggestions().length;
  await run("smaller copy", "Save a smaller copy");
  if (!(await settle(() => dialog() !== null, DIALOG_MS))) throw new Error("the dialog did not open again");
  press("Escape");
  await pause(300);
  report.check("Escape closes the dialog and asks for no name",
    dialog() === null && host.saveSuggestions().length === dismissedAt,
    `${host.saveSuggestions().length} against ${dismissedAt}`);

  // ---- 4. The reader's own numbers: refused when they are not numbers, used when they are.
  host.answerSave(at("tiny.pdf"));
  await run("smaller copy", "Save a smaller copy");
  if (!(await settle(() => radios().length === 5 && worked(), ESTIMATE_MS))) throw new Error("the dialog did not fill in");
  const [dpi, quality] = [...(dialog()?.querySelectorAll<HTMLInputElement>("input[type=text]") ?? [])];
  if (!dpi || !quality) throw new Error("the dialog has no fields for the reader's own numbers");
  const type = (field: HTMLInputElement, value: string) => {
    field.value = value;
    field.dispatchEvent(new Event("change", { bubbles: true }));
  };
  type(dpi, "five");
  await pause(100);
  const alert = () => dialog()?.querySelector('[role="alert"]')?.textContent ?? "";
  report.check("a resolution that is no number is refused in the dialog, and cannot be saved",
    alert() === "The resolution is a whole number from 20 to 1200." && radios()[4]!.checked && save()?.disabled === true,
    alert());
  type(dpi, "60");
  type(quality, "40");
  const own = await settle(() => / smaller$/.test(outcomes()[4] ?? "") && save()?.disabled === false, ESTIMATE_MS);
  report.check("the reader's own numbers are worked out and can be saved", own, outcomes().join(" | "));
  if (!own) return;
  save()?.click();
  const tiny = await settle(() => shown().includes("Saved tiny.pdf"), WRITE_MS);
  report.check("and the copy made with them is written", tiny, shown().slice(0, 200));

  // ---- 5. The smaller copy opens.
  await host.open(at("small.pdf")); await host.idle(); await quiet();
  report.check("the smaller copy opens", host.path() === at("small.pdf"), host.path());
}
