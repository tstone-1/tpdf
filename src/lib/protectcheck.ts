/**
 * *Save a copy with a password* and *Save a copy without its password*,
 * through the window a reader uses them in.
 *
 * `tests/cli/protect.rs` runs the writer through the tool; what no gate
 * reaches is `App.svelte`: the two palette entries, the new-password dialog
 * coming before the save panel, a refused repeat keeping the dialog open, the
 * sentence afterwards, and the password that opens the document being the one
 * a removal is written with. This phase is that half.
 *
 * `expected` is `document|directory`: a document with text and no password,
 * and an empty directory for the copies. `scripts/tabs_check.py --phase
 * protect` makes both and reads the directory afterwards.
 */

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import { DIALOG_CLASS as NEW_PASSWORD } from "./newpassworddialog";
import { DIALOG_CLASS as PASSWORD } from "./passworddialog";
import { suggestedName } from "./protect";

/** A word `testdata/text-base14.pdf` draws. */
const WORD = "quartz";
/** Typed into the dialog; `tabs_check.py` opens the copy with the same one. */
const SECRET = "tr0ub4dor";

const SETTLE_MS = 20_000;
const DIALOG_MS = 5000;
const WRITE_MS = 30_000;

export async function protectCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [source, directory] = expected.split("|");
  if (!source || !directory) throw new Error("a document and a directory are required");
  const at = (name: string) => `${directory}/${name}`;
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  const quiet = async () => {
    if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) throw new Error("the viewer did not settle");
    await pause(100);
  };
  const found = async (word: string) => {
    host.viewer()!.search(word);
    if (!(await settle(() => !host.viewer()!.searching, SETTLE_MS))) throw new Error("the search did not finish");
    return host.viewer()!.searchMatches.length;
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
  const dialog = (name: string) => {
    const backdrop = document.querySelector<HTMLElement>(`.${name}`);
    return backdrop && backdrop.style.display !== "none" ? backdrop : null;
  };
  const fields = (name: string) => [...(dialog(name)?.querySelectorAll<HTMLInputElement>("input") ?? [])];
  const press = (name: string, key: string) =>
    fields(name)[0]?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  const alert = () => dialog(NEW_PASSWORD)?.querySelector('[role="alert"]')?.textContent ?? "";

  // ---- 1. A password, mistyped once: the dialog stays, then the copy is written.
  await host.open(source); await host.idle(); await quiet();
  report.check("the control: the document has the word", (await found(WORD)) > 0, WORD);
  const before = host.saveSuggestions().length;
  host.answerSave(at("locked.pdf"));
  await run("copy with a password", "Save a copy with a password");
  const asked = await settle(() => fields(NEW_PASSWORD).length === 2, DIALOG_MS);
  report.check("the command asks for a new password, twice", asked, String(fields(NEW_PASSWORD).length));
  if (!asked) return;
  const [first, second] = fields(NEW_PASSWORD);
  first!.value = SECRET; second!.value = `${SECRET}x`;
  press(NEW_PASSWORD, "Enter");
  await pause(100);
  report.check("a repeat that differs is refused in the dialog",
    dialog(NEW_PASSWORD) !== null && alert() === "The two passwords are not the same.", alert());
  report.check("before any name is asked for", host.saveSuggestions().length === before,
    `${host.saveSuggestions().length} against ${before}`);
  second!.value = SECRET;
  press(NEW_PASSWORD, "Enter");
  const said = await settle(() => shown().includes("Saved locked.pdf"), WRITE_MS);
  report.check("the copy ends on its sentence", said &&
    shown().trim() === "Saved locked.pdf. It needs the new password to open.", shown().slice(0, 200));
  report.check("the dialog is gone and holds no password",
    dialog(NEW_PASSWORD) === null && first!.value === "" && second!.value === "", first!.value);
  const suggestions = host.saveSuggestions();
  report.check("the save panel suggests <name> protected.pdf",
    suggestions[suggestions.length - 1] === suggestedName(source, true), suggestions.join(", "));
  report.check("the open document is still the source", host.path() === source, host.path());

  // ---- 2. Dismissed: no name is asked for.
  const dismissedAt = host.saveSuggestions().length;
  await run("copy with a password", "Save a copy with a password");
  if (!(await settle(() => dialog(NEW_PASSWORD) !== null, DIALOG_MS))) throw new Error("the dialog did not open again");
  fields(NEW_PASSWORD)[0]!.value = SECRET;
  press(NEW_PASSWORD, "Escape");
  await pause(300);
  report.check("Escape closes the dialog and asks for no name",
    dialog(NEW_PASSWORD) === null && host.saveSuggestions().length === dismissedAt,
    `${host.saveSuggestions().length} against ${dismissedAt}`);

  // ---- 3. A document with no password has none to remove.
  host.answerSave(at("refused.pdf"));
  await run("copy without", "Save a copy without its password");
  const none = await settle(() => shown().includes("has no password"), WRITE_MS);
  report.check("a document with no password is told there is none to remove", none, shown().slice(0, 160));

  // ---- 4. The protected copy asks for its password, and is unprotected with it.
  const opening = host.open(at("locked.pdf"));
  const prompted = await settle(() => fields(PASSWORD).length === 1, SETTLE_MS);
  report.check("the protected copy asks for a password when opened", prompted, String(prompted));
  if (!prompted) return;
  fields(PASSWORD)[0]!.value = SECRET;
  press(PASSWORD, "Enter");
  await opening; await host.idle(); await quiet();
  report.check("and opens with the one that was typed", host.path() === at("locked.pdf") && (await found(WORD)) > 0, host.path());
  host.answerSave(at("open.pdf"));
  await run("copy without", "Save a copy without its password");
  const removed = await settle(() => shown().includes("Saved open.pdf"), WRITE_MS);
  report.check("the copy without it ends on its sentence", removed &&
    shown().trim() === "Saved open.pdf. It opens without a password.", shown().slice(0, 200));
  const last = host.saveSuggestions();
  report.check("the save panel suggests <name> unprotected.pdf",
    last[last.length - 1] === suggestedName(at("locked.pdf"), false), last.join(", "));

  // ---- 5. That copy opens unasked.
  await host.open(at("open.pdf")); await host.idle(); await quiet();
  report.check("the unprotected copy opens with no prompt and has the word",
    dialog(PASSWORD) === null && host.path() === at("open.pdf") && (await found(WORD)) > 0, host.path());
}
