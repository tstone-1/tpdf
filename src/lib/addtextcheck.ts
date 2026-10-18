/**
 * *Save a copy with page numbers* and *Save a copy with a watermark*, through
 * the window a reader uses them in.
 *
 * `tests/cli/addtext.rs` runs the writer through the tool; what no gate reaches
 * is `App.svelte`: the two palette entries, the dialog coming before the save
 * panel, a refused text keeping the dialog open, Escape asking for no name,
 * the sentence afterwards, and the open document staying the source. This
 * phase is that half.
 *
 * `expected` is `document|directory`: a document of at least two pages, and an
 * empty directory for the copies. `scripts/tabs_check.py --phase add-text`
 * makes both and reads what the copies say afterwards.
 */

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import { DIALOG_CLASS } from "./addtextdialog";
import { suggestedName } from "./addtext";

const DIALOG_MS = 5000;
const WRITE_MS = 30_000;

/** Typed into the dialog; `tabs_check.py` looks for it on page 1 of the copy. */
export const NUMBERED_TEXT = "Seite {page} von {pages}";

/** Typed into the dialog: a script Helvetica cannot draw, set in the embedded Noto Sans. */
export const GREEK_TEXT = "\u03a3\u03b5\u03bb\u03af\u03b4\u03b1 {page} \u03b1\u03c0\u03cc {pages}";

export async function addTextCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [source, directory] = expected.split("|");
  if (!source || !directory) throw new Error("a document and a directory are required");
  const at = (name: string) => `${directory}/${name}`;
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
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
  const dialog = () => document.querySelector<HTMLDialogElement>(`dialog.${DIALOG_CLASS}[open]`);
  const field = (label: string) => dialog()?.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`) ?? null;
  const select = () => dialog()?.querySelector<HTMLSelectElement>("select") ?? null;
  const alert = () => dialog()?.querySelector('[role="alert"]')?.textContent ?? "";
  const press = (key: string) =>
    field("Text")?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));

  await host.open(source); await host.idle();
  const pages = host.edits()?.state.pages.length ?? 0;
  if (pages < 2) throw new Error("the phase needs a document of two pages or more");

  // ---- 1. Page numbers: a field that is not one is refused, then the copy is written.
  const before = host.saveSuggestions().length;
  host.answerSave(at("numbered.pdf"));
  await run("page numbers", "Save a copy with page numbers");
  const opened = await settle(() => dialog() !== null, DIALOG_MS);
  report.check("the command opens the dialog", opened, "no dialog");
  if (!opened) return;
  report.check("the dialog starts from the default text",
    field("Text")?.value === "Page {page} of {pages}", field("Text")?.value ?? "");
  field("Text")!.value = "Seite {seite}";
  press("Enter");
  await pause(100);
  report.check("a field that is not one is refused in the dialog",
    dialog() !== null && alert().includes("{seite} is not a field"), alert());
  report.check("before any name is asked for", host.saveSuggestions().length === before,
    `${host.saveSuggestions().length} against ${before}`);
  field("Text")!.value = NUMBERED_TEXT;
  select()!.value = "top-right";
  press("Enter");
  const said = await settle(() => shown().includes("Saved numbered.pdf"), WRITE_MS);
  report.check("the copy ends on its sentence", said &&
    shown().trim() === "Saved numbered.pdf with page numbers.", shown().slice(0, 200));
  const suggestions = host.saveSuggestions();
  report.check("the save panel suggests <name> numbered.pdf",
    suggestions[suggestions.length - 1] === suggestedName(source, "numbers"), suggestions.join(", "));
  report.check("the open document is still the source, unedited",
    host.path() === source && host.edits()?.state.dirty === false, host.path());

  // ---- 2. Dismissed: no name is asked for.
  const dismissedAt = host.saveSuggestions().length;
  await run("watermark", "Save a copy with a watermark");
  if (!(await settle(() => dialog() !== null, DIALOG_MS))) throw new Error("the dialog did not open again");
  press("Escape");
  await pause(300);
  report.check("Escape closes the dialog and asks for no name",
    dialog() === null && host.saveSuggestions().length === dismissedAt,
    `${host.saveSuggestions().length} against ${dismissedAt}`);

  // ---- 3. A watermark on the first page alone.
  host.answerSave(at("watermarked.pdf"));
  await run("watermark", "Save a copy with a watermark");
  if (!(await settle(() => dialog() !== null, DIALOG_MS))) throw new Error("the dialog did not open a third time");
  report.check("the watermark starts from its own defaults",
    field("Text")?.value === "DRAFT" && select()?.value === "center" && field("Opacity, percent")?.value === "30",
    `${field("Text")?.value} ${select()?.value} ${field("Opacity, percent")?.value}`);
  field("Pages")!.value = "1";
  press("Enter");
  const marked = await settle(() => shown().includes("Saved watermarked.pdf"), WRITE_MS);
  report.check("the watermarked copy ends on its sentence", marked &&
    shown().trim() === "Saved watermarked.pdf with a watermark.", shown().slice(0, 200));

  // ---- 4. Greek page numbers: the dialog passes them on, the backend embeds a font.
  host.answerSave(at("greek.pdf"));
  await run("page numbers", "Save a copy with page numbers");
  if (!(await settle(() => dialog() !== null, DIALOG_MS))) throw new Error("the dialog did not open a fourth time");
  field("Text")!.value = GREEK_TEXT;
  press("Enter");
  const greek = await settle(() => shown().includes("Saved greek.pdf"), WRITE_MS);
  report.check("Greek page numbers end on their sentence", greek &&
    shown().trim() === "Saved greek.pdf with page numbers.", `${alert()} ${shown().slice(0, 200)}`);
}
