/**
 * A functional check of file associations, from outside the application.
 *
 * A PDF reaches tpdf from three directions and they share almost no code:
 * `argv` on Windows and from a terminal, an Apple Event on macOS, and the file
 * dialog. Only the first two are checked here --- the dialog cannot be driven
 * without a person, and the viewer check already covers what happens once a
 * document is open.
 *
 * Like `sessioncheck.ts` and unlike every other harness here, **this does not
 * replace the application**. Opening a handed-over document is part of the boot,
 * and the interesting failure is a path arriving before the frontend exists, so
 * a check that called the commands directly would be testing a second
 * implementation of the thing most likely to be wrong.
 *
 * Two modes, and the second carries its own control:
 *
 * - `opened:<path>` --- a document is open once the boot settles, and it is that
 *   one. Used for every route that delivers before the frontend is listening.
 * - `arrives:<path>` --- **nothing** is open when the boot settles, and *then* a
 *   document arrives and it is that one. The first half is not decoration: it is
 *   what stops "the document arrived" being satisfied by one that was already
 *   there, which is precisely what the cold routes above produce.
 */

import { call } from "./ipc";

import { pause, Report, settle } from "./checkreport";
import { basename } from "./paths";
import { SIDEBAR_CLASS } from "./sidebar";
import type { Viewer } from "./viewer";
import type { Edits } from "./edits";

/** How long to wait for a document that should already be on its way. */
const SETTLE_MS = 20_000;

/** How long to wait for one handed over while the app is running. */
const ARRIVAL_MS = 30_000;

/**
 * How long to watch for a document that should *not* appear.
 *
 * Long enough that a slow open would have landed. A control that gives up too
 * early passes for the same reason the thing it guards would fail.
 */
const QUIET_MS = 2500;

/**
 * How long the `race` phase lets the DOM catch up after both opens resolve.
 *
 * The sidebar is mounted behind a `requestAnimationFrame` inside the open, so
 * an assertion taken the instant the promises settle can read a count that is
 * one short --- and one short is the value a passing run has.
 */
const SETTLE_MS_AFTER_RACE = 500;

/** What the check needs from the running application. */
export interface OpenCheckHost {
  /** Path of the open document, or "". */
  path: () => string;
  /** The application's own entry point for opening one, chain and all. */
  open: (path: string) => Promise<void>;
  /** Whether a viewer is mounted, for the `race` phase's end state. */
  hasViewer: () => boolean;
  tabs: () => readonly { id: number; path: string }[];
  viewer: () => Viewer | null;
  edits: () => Edits | null;
  activate: (id: number) => Promise<void>;
  close: (id: number) => Promise<void>;
  run: (id: string) => void;
  idle: () => Promise<void>;
}

const report = new Report();

/**
 * Runs the check if `TPDF_OPENCHECK` is set, then exits the process.
 *
 * Returns `false` when it was not requested, so the caller carries on into the
 * real application. Called after the boot has had its chance to open something,
 * because what is being checked is whether it did.
 */
export async function runOpenCheckIfRequested(host: OpenCheckHost): Promise<boolean> {
  const mode = await call("opencheck_mode");
  if (!mode) return false;

  const separator = mode.indexOf(":");
  const phase = separator < 0 ? mode : mode.slice(0, separator);
  const expected = separator < 0 ? "" : mode.slice(separator + 1);

  try {
    await run(host, phase, expected);
  } catch (e) {
    report.check("the phase ran", false, String(e));
  }

  await report.finish();
  return true;
}

/** The tail of a path, for a detail column that has to stay readable. */
const name = basename;

/** How many sidebars are mounted. More than one is the defect `race` looks for. */
function sidebars(): number {
  return document.querySelectorAll(`.${SIDEBAR_CLASS}`).length;
}

async function run(host: OpenCheckHost, phase: string, expected: string): Promise<void> {
  switch (phase) {
    case "forms": {
      const check = (name: string, ok: boolean) => report.check(name, ok, "form workflow");
      const [first, second] = expected.split("|");
      if (!first || !second) throw new Error("two disposable form paths required");
      await host.open(first);
      const a = host.tabs()[0];
      if (!a) throw new Error("first document did not open");
      const field = () => document.querySelector<HTMLInputElement>('.form-fields input[type="text"]');
      if (!await settle(() => !!field(), SETTLE_MS)) throw new Error("form controls did not mount");
      host.run("edit.fillForm");
      check("the palette focuses a form field", document.activeElement === field());
      field()!.value = "Grüße";
      // Switching while typing must flush to A before B becomes active.
      await host.open(second);
      const b = host.tabs().find((tab) => tab.id !== a.id)!;
      check("the second document has no form edits", (host.edits()?.state.forms?.length ?? 0) === 0);
      await host.activate(a.id);
      if (!await settle(() => field()?.value === "Grüße", SETTLE_MS)) throw new Error("the typed answer was lost on tab switch");
      check("the answer belongs to the original tab", host.edits()?.state.forms?.[0]?.value === "Grüße");
      host.run("edit.undo"); await host.idle();
      check("undo restores the file's answer", field()?.value === "OLD");
      host.run("edit.redo"); await host.idle();
      check("redo restores the typed answer", field()?.value === "Grüße");
      const checkbox = document.querySelector<HTMLInputElement>('.form-fields input[type="checkbox"]')!;
      checkbox.click(); await host.idle();
      check("a checkbox records a boolean answer", host.edits()?.state.forms?.some((f) => f.value === true) === true);
      field()!.focus();
      field()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }));
      check("Tab reaches the next field", document.activeElement === checkbox);
      host.run("file.save");
      check("saving freezes form edits", field()?.readOnly === true && checkbox.disabled);
      await host.idle();
      if (!await settle(() => field()?.value === "Grüße", SETTLE_MS)) throw new Error(`the saved field did not reopen: ${document.querySelector(".error")?.textContent ?? "no error shown"}`);
      check("save resets the journal", host.edits()?.state.dirty === false);
      check("saved controls are editable again", field()?.readOnly === false);
      const form = await call("document_form", { doc: host.edits()!.doc });
      check("both shared widgets reopen with the saved answer", form.widgets.filter((w) => w.value === "Grüße").length === 2);
      check("the checkbox reopens checked", form.widgets.some((w) => w.value === true));
      const current = host.edits()!;
      await host.activate(b.id);
      check("saving did not change the other tab", (host.edits()?.state.forms?.length ?? 0) === 0 && host.edits()?.state.dirty === false);
      const untouched = await call("document_form", { doc: b.id });
      check("the other file retains its original answer", untouched.widgets.some((w) => w.value === "OLD"));
      await host.activate(current.doc);
      if (!await settle(() => !!field(), SETTLE_MS)) throw new Error("form controls did not remount");
      field()!.value = ""; field()!.dispatchEvent(new Event("blur")); await host.idle();
      check("clearing is an edit, not an absent value", host.edits()?.state.forms?.some((f) => f.value === "") === true);
      host.run("edit.undo"); await host.idle();
      check("undo restores a cleared field", field()?.value === "Grüße");
      field()!.value = "x".repeat(21);
      await host.activate(b.id);
      check("an invalid draft blocks tab switching", host.edits()?.doc === current.doc && field()?.value === "x".repeat(21));
      field()!.value = "Grüße";
      await host.activate(b.id);
      check("correcting a draft permits switching again", host.edits()?.doc === b.id);
      break;
    }
    case "tabs": {
      const check = (name: string, ok: boolean) => report.check(name, ok,
        `${host.tabs().length} tabs, active ${basename(host.path())}`);
      const [first, second] = expected.split("|");
      if (!first || !second || first === second) throw new Error("two distinct fixture paths required");
      report.emit("[tabs] opening the first fixture");
      await host.open(first);
      const a = host.tabs()[0];
      if (!a || !host.viewer()) throw new Error("first document did not open");
      report.emit("[tabs] editing the first tab");
      host.viewer()!.setZoomFixed(1.25);
      host.run("edit.rotatePageClockwise");
      if (!await settle(() => host.edits()?.state.dirty === true, SETTLE_MS))
        throw new Error("rotation did not reach the document");
      host.run("edit.addComment");
      document.querySelector(".surface")?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      if (!await settle(() => (host.viewer()?.markOpen ?? -1) >= 0, SETTLE_MS))
        throw new Error("note did not open");
      const note = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Note"]')!;
      note.value = "Synthetic tab note";
      note.dispatchEvent(new Event("input", { bubbles: true }));
      report.emit("[tabs] opening the second fixture");
      await host.open(second);
      const b = host.tabs().find((tab) => tab.id !== a.id);
      check("both documents retain a tab", host.tabs().length === 2 && !!b);
      check("the second tab has no inherited edits", host.edits()?.state.dirty === false && host.edits()?.state.marks.length === 0);
      if (!b) return;
      const button = document.getElementById(`document-tab-${a.id}`);
      button?.click();
      if (!await settle(() => host.path() === first && !!host.viewer(), SETTLE_MS))
        throw new Error("clicking the first tab did not activate it");
      await pause(200);
      check("switching keeps the original backend handle", host.edits()?.doc === a.id);
      check("switching keeps page edits", host.edits()?.state.pages[0]?.turns === 1);
      check("switching commits the open note to its own tab", host.edits()?.state.marks[0]?.note === "Synthetic tab note");
      check("switching restores fixed zoom", Math.abs((host.viewer()?.currentZoom ?? 0) - 1.25) < 0.001 && host.viewer()?.fitMode === "none");
      await host.open(first);
      check("opening an existing path reuses its tab", host.tabs().length === 2 && host.edits()?.doc === a.id);
      await host.open(`${first}.missing`);
      check("a failed open leaves both tabs and the active document intact", host.tabs().length === 2 && host.edits()?.doc === a.id && host.hasViewer());
      host.run("file.save");
      await host.idle();
      check("saving replaces only the active handle", host.tabs().length === 2 && host.tabs()[0]?.path === first && host.tabs()[1]?.id === b.id && host.edits()?.doc !== a.id);
      check("saving resets the active journal", host.edits()?.state.dirty === false);
      await host.activate(b.id);
      check("saving leaves the other tab usable", host.path() === second && host.edits()?.doc === b.id && host.hasViewer());
      const saved = host.tabs().find((tab) => tab.path === first);
      if (!saved) throw new Error("saved tab disappeared");
      await host.close(saved.id);
      check("closing a background tab keeps the active document", host.tabs().length === 1 && host.edits()?.doc === b.id);
      let released = false;
      try { await call("edit_state", { doc: saved.id }); } catch { released = true; }
      check("closing releases the backend edit model", released);
      await host.close(b.id);
      check("closing the last tab leaves an empty window", host.tabs().length === 0 && !host.hasViewer() && host.path() === "");
      await host.open(second);
      check("opening after the last close still works", host.tabs().length === 1 && host.hasViewer());
      check("only one sidebar is mounted", sidebars() === 1);
      break;
    }
    case "opened": {
      const opened = await settle(() => host.path() !== "", SETTLE_MS);
      report.check(
        "a document opened without anyone asking for one",
        opened,
        opened ? name(host.path()) : "nothing opened",
      );
      if (!opened) return;
      report.check(
        "it is the document that was handed over",
        host.path() === expected,
        `${name(host.path())} vs ${name(expected)}`,
      );
      break;
    }

    case "arrives": {
      // The control, and it is the whole reason this mode is separate from
      // `opened`. Without it, "a document arrived" is satisfied by one that was
      // handed over at launch --- which is what every other phase produces, so
      // the mistake is not hypothetical.
      await pause(QUIET_MS);
      const quiet = host.path() === "";
      report.check(
        "nothing is open before one is handed over",
        quiet,
        quiet ? "empty, as a fresh launch should be" : `already showing ${name(host.path())}`,
      );
      if (!quiet) return;

      const arrived = await settle(() => host.path() !== "", ARRIVAL_MS);
      report.check(
        "a document handed to the running app opens",
        arrived,
        arrived ? name(host.path()) : "nothing ever arrived",
      );
      if (!arrived) return;
      report.check(
        "it is the document that was handed over",
        host.path() === expected,
        `${name(host.path())} vs ${name(expected)}`,
      );
      break;
    }

    // Two opens issued without waiting for the first, which is what a reader
    // produces by double-clicking a second file --- or by pressing Cmd-O twice.
    //
    // What it asserts is that `openPath`'s queue held, and the failure it is
    // named for was real: the two bodies interleaved, each read the *other's*
    // freshly-set document id as the outgoing one and released the file the
    // other was about to mount, and the second `new Viewer` overwrote the first
    // without destroying it --- two viewers with live listeners on one element,
    // and two sidebars, because `Sidebar` appends. Counting sidebars is what
    // makes one arm of that observable; the viewer leak has no DOM footprint,
    // an overwritten `Viewer` being a live object with no element of its own.
    //
    // **This is a smoke check, not a gate, and the difference was measured.**
    // With `openPath` mutated to call the body directly, this reports the
    // defect in roughly two runs out of three: which of the two opens lands
    // last is a race between two `invoke` round trips, and the run where the
    // right one happens to win looks exactly like a correct build. Three things
    // that ought to have fixed that did not. Repeating the round five times
    // inside one launch made it *worse* (one run in four), because only the
    // first round is cold --- the rest run against warmed workers and an
    // already-open document, and land in the same order every time. Pairing a
    // slow document with a fast one did not help, nor did a 336 MB one: the
    // ordering is decided by IPC scheduling and not by what either open costs.
    // The deterministic half of this property lives in `serial.test.ts`, which
    // is where a change to the queue itself will go red; what only this can
    // say is that `App.svelte` still routes opens through it.
    //
    // The second sidebar was never observed under that mutation --- the leak
    // needs both teardowns to fall between the two mounts, a narrower window
    // still. Kept because it names the failure that actually shipped, and
    // recorded here as unproven rather than left looking load-bearing.
    case "race": {
      const bar = expected.indexOf("|");
      const first = bar < 0 ? expected : expected.slice(0, bar);
      const second = bar < 0 ? "" : expected.slice(bar + 1);

      // Without this the round assertion cannot fail: "the second document won"
      // is satisfied by the first one when they are the same file.
      report.check(
        "the two documents are distinguishable",
        first !== "" && second !== "" && first !== second,
        `${name(first)} then ${name(second)}`,
      );
      if (first === second || second === "") return;

      // The state the assertions below must not already be in. A phase that
      // found a document open and one sidebar mounted would pass every check
      // that follows without either open having done anything.
      await pause(QUIET_MS);
      const before = sidebars();
      const empty = host.path() === "" && before === 0;
      report.check(
        "nothing is open before the two opens are issued",
        empty,
        empty ? "empty, as a fresh launch should be" : `${name(host.path())}, ${before} sidebars`,
      );
      if (!empty) return;

      // One round, and only one: this is the only cold one, and rounds after it
      // measured strictly worse than nothing (see above). The repetition that
      // does buy something is separate launches, which is the driver's job.
      //
      // Deliberately not awaited in turn: issuing both before either resolves is
      // the whole condition being tested.
      const a = host.open(first);
      const b = host.open(second);
      await Promise.allSettled([a, b]);
      // A sidebar is mounted behind a `requestAnimationFrame`, so settling the
      // promises is not the same as the DOM having caught up --- and one short
      // is exactly the count a passing run has.
      await pause(SETTLE_MS_AFTER_RACE);

      report.check(
        "the document that opened last is the one showing",
        host.path() === second,
        `${name(host.path())} vs ${name(second)}`,
      );
      const mounted = sidebars();
      report.check(
        "no second sidebar was left behind",
        mounted === 1,
        `${mounted} in the document`,
      );
      report.check(
        "the reader is left with a viewer",
        host.hasViewer(),
        host.hasViewer() ? "mounted" : "no viewer",
      );
      break;
    }

    default:
      report.check("the phase is one this check knows", false, `unknown phase ${phase.slice(0, 20)}`);
  }
}
