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
import { filePage } from "./pages";
import { DESTINATION_MARGIN_PT } from "./outline";
import { signatureCheck } from "./signaturecheck";

import { pause, Report, settle } from "./checkreport";
import { basename } from "./paths";
import { SIDEBAR_CLASS } from "./sidebar";
import type { Viewer } from "./viewer";
import type { Edits } from "./edits";
import type { PendingImport } from "./pendingimport";

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
  /**
   * `edit.insertPages` past its dialog: the file named rather than picked,
   * opened, and the palette asking which of its pages --- which the phase then
   * answers or dismisses through the palette's own input.
   */
  importPages: (path: string) => Promise<void>;
  /** The file waiting for its pages to be named, as the palette sees it. */
  pendingImport: () => PendingImport | null;
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
    case "tabs-rotation": {
      const [first] = expected.split("|");
      if (!first) throw new Error("a disposable fixture path is required");
      await host.open(first); await host.idle();
      const viewer = host.viewer()!;
      const page = host.edits()!.state.pages.length - 1;
      if (page < 2) throw new Error("rotation check needs at least three pages");
      const quiet = async () => {
        if (!await settle(() => viewer.idle, SETTLE_MS)) throw new Error("rotation did not settle");
        await pause(100);
      };
      // Learn the unequal preceding sheets before changing their layout.
      for (let index = 0; index <= page; index++) {
        viewer.goToPage(index); await quiet();
      }
      for (const fit of ["page", "width"] as const) {
        viewer.goToPage(page); viewer.setFit(fit); viewer.goToPage(page); await quiet();
        for (let turn = 1; turn <= 4; turn++) {
          host.run("view.rotateClockwise"); await host.idle(); await quiet();
          const rotated = viewer.currentZoom;
          // A reader can explicitly return to the target and refit it. The
          // rotation itself must choose that same scale without this correction.
          viewer.goToPage(page); viewer.setFit(fit); viewer.goToPage(page); await quiet();
          report.check(`rotation keeps ${fit} on the last sheet, turn ${turn}`,
            Math.abs(rotated - viewer.currentZoom) < 0.001,
            JSON.stringify({ rotated, refitted: viewer.currentZoom }));
        }
      }
      viewer.goToPage(page); viewer.setFit("page"); viewer.goToPage(page); await quiet();
      const fitted = viewer.currentZoom;
      const retained = host.edits()!.state.pages[page]!.id;
      for (let slot = page - 1; slot >= 0; slot--) {
        host.run("edit.movePageUp"); await host.idle(); await quiet();
        report.check(`moving the reading sheet to slot ${slot} keeps its fit`,
          host.edits()!.state.pages[slot]?.id === retained &&
          viewer.pageOrder[slot]?.id === retained && Math.abs(viewer.currentZoom - fitted) < 0.001,
          JSON.stringify({ fitted, moved: viewer.currentZoom }));
      }
      break;
    }
    case "import": {
      // `edit.insertPages`, end to end but the dialog: `first` is the opened
      // document and `other` a different file of at least three pages, whose
      // pages differ in their text from each other and from the first's, or
      // the checks on *which* page answered cannot fail.
      // `tabs_check.py --phase import --other <pdf>` supplies it.
      //
      // Three passes over the same file: the question dismissed, a range, and
      // a blank answer --- every page --- which the rest of the phase reads.
      const [first, other] = expected.split("|");
      if (!first || !other) throw new Error("the opened fixture and another file are required");
      await host.open(first); await host.idle();
      const quiet = async () => {
        if (!await settle(() => host.viewer()?.idle === true, SETTLE_MS)) throw new Error("the viewer did not settle");
        await pause(100);
      };
      const model = host.edits()!;
      const before = model.state.pages.length;
      const theirs = async (doc: number, page: number) =>
        String.fromCodePoint(...(await call("page_text", { doc, page: filePage(page), crop: null })).codes);
      // The palette's real input, through its real listeners: the dismissal is
      // what releases the file, and a check that called the release itself
      // would be testing a second route to it.
      const field = () => document.querySelector<HTMLInputElement>(".tpdf-palette input");
      const key = (k: string) =>
        field()?.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
      const answer = (text: string) => {
        const input = field();
        if (!input) throw new Error("the palette has no input");
        input.value = text;
        input.dispatchEvent(new InputEvent("input", { bubbles: true }));
        key("Enter");
      };
      host.viewer()!.goToPage(0); await quiet();

      await host.importPages(other);
      const asked = host.pendingImport();
      const prompt = field()?.placeholder ?? "";
      report.check("the palette asks which pages of the file, naming it and its count",
        asked !== null && prompt === `Pages of ${asked.name} (1-${asked.pages}); blank for all`, prompt);
      if (!asked) break;
      if (asked.pages < 3) throw new Error(`the other file needs three pages and has ${asked.pages}`);
      report.check("asking places nothing", host.edits()!.state.pages.length === before,
        String(host.edits()!.state.pages.length));
      key("Escape"); key("Escape"); await host.idle();
      report.check("dismissing the question inserts nothing and forgets the file",
        host.edits()!.state.pages.length === before && host.pendingImport() === null,
        JSON.stringify({ pages: host.edits()!.state.pages.length, waiting: host.pendingImport() }));
      // The dismissal's own release is posted, not awaited, so it is given a
      // moment to land. A second release then finds nothing to end; one that
      // finds the file still held means the dismissal never reached the backend.
      await pause(300);
      const stillHeld = await call("page_import_cancel", { doc: model.doc, pending: asked.pending });
      report.check("and the backend is no longer holding it", !stillHeld, String(stillHeld));

      await host.importPages(other);
      const count = host.pendingImport()?.pages ?? 0;
      answer(`2-${count}`); await host.idle(); await quiet();
      const ranged = host.edits()!.state;
      const placed = ranged.pages.slice(1, count).map((page) => ("imported" in page.source ? page.source.imported.page : -1));
      const wanted = Array.from({ length: count - 1 }, (_, index) => index + 1);
      report.check("a range inserts exactly those pages of the file, in its order, after the page being read",
        ranged.pages.length === before + count - 1 && placed.join() === wanted.join() &&
        !("imported" in (ranged.pages[0]?.source ?? {})),
        JSON.stringify({ pages: ranged.pages.length, placed }));
      const rangedFrom = ranged.sources?.[0]?.doc;
      if (rangedFrom !== undefined) {
        const second = await theirs(rangedFrom, 1);
        report.check("the file's first and second pages differ, so the next check can fail",
          second !== await theirs(rangedFrom, 0), "import");
        host.viewer()!.goToPage(1); await quiet();
        if (!await settle(() => host.viewer()?.textOn(1) != null, SETTLE_MS)) throw new Error("the ranged page's text did not arrive");
        const shownRanged = String.fromCodePoint(...host.viewer()!.textOn(1)!.codes);
        report.check("the first page inserted is the file's second", shownRanged === second,
          JSON.stringify({ shown: shownRanged.slice(0, 40), expected: second.slice(0, 40) }));
      }
      host.run("edit.undo"); await host.idle(); await quiet();
      report.check("one undo takes the range back out", host.edits()!.state.pages.length === before,
        String(host.edits()!.state.pages.length));
      host.viewer()!.goToPage(0); await quiet();

      await host.importPages(other);
      answer(""); await host.idle(); await quiet();
      const after = host.edits()!.state;
      const source = after.sources?.[0];
      report.check("a blank answer inserts every page of the file after the page being read",
        after.pages.length === before + count && "imported" in (after.pages[1]?.source ?? {}),
        JSON.stringify({ before, after: after.pages.length }));
      report.check("they are drawn from a handle that is not the opened document's",
        source !== undefined && source.doc !== model.doc && after.pages[1]?.from === source.doc,
        JSON.stringify(after.sources));
      if (!source) break;
      const expectedText = await theirs(source.doc, 0);
      report.check("the fixtures differ, so the next check can fail",
        expectedText !== await theirs(model.doc, 0), "import");
      // Text is extracted for visible pages only (`prefetchText`), and the
      // reading page can fill the window, so the inserted page is brought on
      // screen first; waiting while parked on page 1 waits for nothing.
      host.viewer()!.goToPage(1); await quiet();
      if (!await settle(() => host.viewer()?.textOn(1) != null, SETTLE_MS)) throw new Error("the imported page's text did not arrive");
      const shown = String.fromCodePoint(...host.viewer()!.textOn(1)!.codes);
      report.check("the imported page's text is the other file's", shown === expectedText,
        JSON.stringify({ shown: shown.slice(0, 40), expected: expectedText.slice(0, 40) }));
      const word = expectedText.split(/\s+/).find((w) => w.length > 3);
      report.check("the other file's first page has a word to search for", word !== undefined, expectedText.slice(0, 40));
      if (word) {
        host.viewer()!.search(word);
        if (!await settle(() => !host.viewer()!.searching, SETTLE_MS)) throw new Error("the search did not finish");
        const imported = new Set(after.pages.flatMap((page, slot) => ("imported" in page.source ? [slot] : [])));
        report.check("a search finds a word on an imported page",
          host.viewer()!.searchMatches.some((match) => imported.has(match.page)), word);
      }
      host.run("edit.undo"); await host.idle(); await quiet();
      report.check("one undo takes every imported page back out",
        host.edits()!.state.pages.length === before && host.edits()!.state.sources?.[0]?.doc === source.doc,
        JSON.stringify(host.edits()!.state.sources));
      host.run("edit.redo"); await host.idle(); await quiet();
      report.check("redo draws them through the same handle",
        host.edits()!.state.pages.length === after.pages.length && host.edits()!.state.pages[1]?.from === source.doc,
        String(host.edits()!.state.pages.length));
      host.run("file.save"); await host.idle(); await quiet();
      const saved = host.edits()!.state;
      report.check("the saved file holds the pages as its own",
        saved.pages.length === after.pages.length && !saved.dirty && !saved.sources?.length &&
        saved.pages.every((page) => "baseline" in page.source),
        JSON.stringify({ pages: saved.pages.length, dirty: saved.dirty }));
      report.check("and page 2 of it reads as the other file's first page",
        (await theirs(host.edits()!.doc, 1)) === expectedText, "import");
      break;
    }
    case "tabs-position": {
      const [first, second] = expected.split("|");
      if (!first || !second) throw new Error("two disposable fixture paths required");
      await host.open(first); await host.idle();
      const firstTab = host.tabs().find((tab) => tab.path === first)!;
      const lastPage = host.edits()!.state.pages.length - 1;
      if (lastPage < 2) throw new Error("position check needs at least three pages");
      const quiet = async () => {
        if (!await settle(() => host.viewer()?.idle === true, SETTLE_MS)) throw new Error("position did not settle");
        await pause(100);
      };
      for (const page of [1, lastPage]) {
        for (const fit of ["page", "width", "none"] as const) {
          const before = host.viewer()!;
          before.goToPage(page); await quiet();
          if (fit === "none") before.setZoomFixed(1.25);
          else before.setFit(fit);
          if (page === 1) before.goToDestination(page, 240 + DESTINATION_MARGIN_PT);
          else before.goToPage(page);
          await quiet();
          const saved = { ...before.position, zoom: before.currentZoom, fit: before.fitMode };
          const point = before.screenPoint(page, 50, 50);
          for (let round = 0; round < 2; round++) {
            await host.open(second); await host.idle();
            await host.activate(firstTab.id); await host.idle(); await quiet();
            const after = host.viewer()!;
            const actual = { ...after.position, zoom: after.currentZoom, fit: after.fitMode };
            const returned = after.screenPoint(page, 50, 50);
            report.check(`tab position page ${page + 1}, ${fit}, round ${round + 1}`,
              actual.page === saved.page && Math.abs(actual.top - saved.top) < 1 &&
              Math.abs(actual.zoom - saved.zoom) < 0.001 && actual.fit === saved.fit &&
              Math.abs(point.x - returned.x) < 1 && Math.abs(point.y - returned.y) < 1,
              JSON.stringify({ saved, actual, point, returned }));
          }
        }
      }
      break;
    }
    case "signed-save-cancel":
    case "signed-save-accept": {
      await host.open(expected); await host.idle();
      const original = host.edits()!.doc;
      const properties = await call("document_properties", {doc:original});
      report.check("the fixture has a digital signature", properties.signatures.some((s)=>s.signed), "signed save");
      host.run("edit.rotatePageClockwise"); await host.idle();
      report.check("the signed document has pending edits", host.edits()!.dirty, "signed save");
      host.run("file.save");
      const dialog = () => document.querySelector<HTMLDialogElement>(".signed-save-dialog[open]");
      if (!await settle(() => !!dialog(), 5000)) throw new Error("the signature warning did not appear");
      report.check("the warning states possible signature invalidation", dialog()!.textContent!.includes("Saving can invalidate them"), "signed save");
      const buttons = [...dialog()!.querySelectorAll("button")];
      report.check("Cancel has initial focus", document.activeElement === buttons.find((b)=>b.textContent === "Cancel"), "signed save");
      const accepted = phase.endsWith("accept");
      buttons.find((b)=>b.textContent === (accepted ? "Save anyway" : "Cancel"))!.click();
      await host.idle();
      report.check("the reader's save choice is respected", accepted
        ? !host.edits()!.dirty && host.edits()!.doc !== original
        : host.edits()!.doc === original && host.edits()!.dirty, "signed save");
      break;
    }
    case "textedit":
    case "textedit-w3c":
    case "textedit-dash":
    case "textedit-cff-unicode":
    case "textedit-cff-ligatures":
    case "textedit-passport":
    case "textedit-agenda":
    case "textedit-agenda-page2":
    case "textedit-multipage":
    case "textedit-list-child":
    case "textedit-wide-spacing":
    case "textedit-wrapped":
    case "textedit-overhang":
    case "textedit-cid-latin1":
    case "textedit-latin1": {
      const listChild = phase === "textedit-list-child";
      const passport = phase === "textedit-passport";
      const agendaPage2 = phase === "textedit-agenda-page2";
      const agenda = phase === "textedit-agenda" || agendaPage2;
      const dash = phase === "textedit-dash";
      const cffLigatures = phase === "textedit-cff-ligatures";
      const cffUnicode = phase === "textedit-cff-unicode";
      const overhang = phase === "textedit-overhang";
      const w3c = phase === "textedit-w3c";
      const cidLatin1 = phase === "textedit-cid-latin1" || overhang;
      const wrapped = phase === "textedit-wrapped";
      const wideSpacing = phase === "textedit-wide-spacing";
      const page = passport ? 15 : phase === "textedit-multipage" || wrapped || agendaPage2 ? 1 : 0;
      const original = listChild ? "SYNTHETIC SECOND" : passport ? "ILB 53 (09.22)" : cffLigatures ? "SYNTHETIC ffi ffi fi fl ff" : cffUnicode ? "SYNTHETIC \u2212\u00a0\u2018\u2019\u2013£" : agendaPage2 ? "Community Hub" : agenda ? "REGULAR" : dash ? "SYNTHETIC\u2013FIRST" : w3c ? "Dummy PDF file" : cidLatin1 ? "SYNTHETIC ÄÖÜ äöü ß" : phase === "textedit-latin1" ? "SYNTHETIC ÄÖÜ ß" : "SYNTHETIC FIRST";
      const replacement = listChild ? "EDITED SECOND" : passport ? "ILB 53" : cffLigatures ? "EDITED ffi fi fl ff" : cffUnicode ? "EDITED £\u2013\u2019\u2018\u00a0\u2212" : agendaPage2 ? "Community" : agenda ? "ANNUAL" : dash ? "EDITED\u2013FIRST" : w3c ? "Dummy PDF fill" : overhang ? "ÖÄÜ äöü ß" : cidLatin1 ? "ÄÖÜ äöü ß" : phase === "textedit-latin1" ? "GEPRÜFT ß" : "EDITED FIRST";
      const check = (name: string, ok: boolean) => report.check(name, ok, "text editing workflow");
      const [first, second] = expected.split("|");
      if (!first || !second) throw new Error("two disposable text fixture paths required");
      await host.open(first); await host.idle();
      if (passport) host.run("view.fitPage");
      host.viewer()!.goToPage(page); await host.idle();
      // idle() drains edits, not viewer frames. The edit command uses the page
      // reported by the viewer, so wait for the same page the reader sees.
      if (!await settle(() => host.viewer()?.idle === true &&
        document.querySelector('.navigation button[title="Go to page"]')?.textContent?.trim().startsWith(`${page + 1} /`) === true, SETTLE_MS)) {
        throw new Error("the requested text-edit page did not settle");
      }
      const originalTab = host.tabs().find((tab) => tab.path === first)!;
      const field = () => document.querySelector<HTMLTextAreaElement>(".text-edit-popup textarea");
      const target = () => {
        const targets = [...document.querySelectorAll<HTMLButtonElement>(".text-edit-run")];
        // A list label may precede the item body; select the authored text.
        return targets.find((button) => [original + (wrapped ? " " : ""), replacement].some((text) => button.getAttribute("aria-label") === `Edit: ${text}`));
      };
      const start = async () => {
        const previous = target();
        host.run("edit.editText");
        // Discovery replaces the editor asynchronously. An old target can still
        // be visible while the command waits for the worker's fresh reply.
        if (!await settle(() => {
          const current = target();
          return !!current && current !== previous && !current.disabled;
        }, SETTLE_MS)) throw new Error("fresh editable text targets did not appear");
        target()!.click();
        if (!await settle(() => !!field() && document.activeElement === field(), 3000)) throw new Error("text input did not receive focus");
      };
      const read = async (index = page) => String.fromCodePoint(...(await call("page_text", { doc: host.edits()!.doc, page: filePage(index), crop: null })).codes);
      const untouched = page === 1 ? await read(0) : "";
      if (page === 1) check("both source pages have their original text", untouched.includes(agenda ? "REGULAR" : original) && (await read(1)).includes(original));
      await start();
      check("source text is offered for replacement", field()!.value === original + (wrapped ? " " : ""));
      field()!.value = "DISCARDED DRAFT";
      document.querySelector<HTMLElement>(".text-edit-popup")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      check("Escape returns focus to the selected text target", document.activeElement === target());
      target()!.click();
      check("cancelled text is discarded when the target is reopened", field()!.value === original + (wrapped ? " " : ""));
      field()!.value = replacement;
      const cancel = document.querySelector<HTMLButtonElement>('.text-edit-popup button[aria-label="Cancel"]')!;
      cancel.focus({ preventScroll: true });
      const cancelEnter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
      cancel.dispatchEvent(cancelEnter);
      await host.idle();
      check("Enter on Cancel does not apply the draft", !host.edits()!.dirty && (await read()).includes(original));
      check("Enter on Cancel leaves button activation available", !cancelEnter.defaultPrevented);
      // Synthetic key events have no browser default click; exercise the
      // handler separately only after proving keydown did not take that action.
      cancel.click();
      target()!.click();
      check("Cancel button discards the keyboard draft", field()!.value === original + (wrapped ? " " : "") && !host.edits()!.dirty);
      const done = document.querySelector<HTMLButtonElement>('.text-editor button[aria-label="Done"]')!;
      done.focus({ preventScroll: true }); done.click();
      if (!await settle(() => !document.querySelector(".text-editor"), SETTLE_MS)) throw new Error("Done did not close text editing");
      check("Done returns keyboard control to the PDF", document.activeElement === document.querySelector(".surface"));
      if (passport) {
        const before = JSON.stringify(host.viewer()!.position);
        document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true }));
        check("arrow keys scroll the PDF after Done", await settle(() => JSON.stringify(host.viewer()!.position) !== before, 1000));
        host.viewer()!.goToPage(page);
        if (!await settle(() => host.viewer()?.idle === true, SETTLE_MS)) throw new Error("returning after keyboard navigation did not settle");
      }
      await start();
      if (!await settle(() => host.viewer()?.idle === true, SETTLE_MS)) throw new Error("text target layout did not settle");
      await pause(100);
      const hit = target()!.getBoundingClientRect();
      check("the text target is visible and has area", hit.width > (w3c || agenda || passport ? 5 : 50) && hit.height > 5 && hit.top >= 0);
      if (passport) {
        // Independent source matrix: 0 8 -8 0 382.6772 31.0394, on a
        // 555.591pt sheet. The source ink occupies this narrow vertical band.
        const viewer = host.viewer()!;
        const a = viewer.screenPoint(page, 374.6772, 473.8556), b = viewer.screenPoint(page, 384.6772, 524.5516);
        const editorRoot = document.querySelector<HTMLElement>(".text-editor")!;
        const editorBox = editorRoot.getBoundingClientRect();
        check("focusing the low label does not scroll the editor overlay", editorRoot.scrollTop === 0);
        check("the vertical target follows the authored text matrix", hit.height > hit.width * 2 &&
          hit.left >= editorBox.left + a.x - 5 && hit.right <= editorBox.left + b.x + 5 &&
          hit.top >= editorBox.top + a.y - 10 && hit.bottom <= editorBox.top + b.y + 5);
      }
      if (passport) {
        field()!.value = "DISCARDED OFFSCREEN DRAFT";
        host.viewer()!.goToStart();
        if (!await settle(() => host.viewer()?.idle === true && host.viewer()!.position.page === 0, SETTLE_MS))
          throw new Error("scrolling did not reach the first page");
        check("offscreen text targets leave keyboard navigation", target()!.hidden === true);
        const position = host.viewer()!.position;
        document.querySelector<HTMLElement>(".text-edit-popup")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        check("cancelling offscreen text focuses Done", document.activeElement === document.querySelector('.text-editor button[aria-label="Done"]'));
        check("offscreen cancellation keeps the reading position", JSON.stringify(host.viewer()!.position) === JSON.stringify(position));
        host.viewer()!.goToPage(page);
        if (!await settle(() => host.viewer()?.idle === true && target()?.hidden === false, SETTLE_MS))
          throw new Error("returning to the page did not restore its text target");
        target()!.click();
        check("offscreen cancellation discards the draft", field()!.value === original);
      }
      field()!.value = replacement; field()!.dispatchEvent(new Event("input", { bubbles: true }));
      // No Apply: switching tabs must drain the draft into its original document.
      await host.open(second); await host.idle();
      const otherTab = host.tabs().find((tab) => tab.id !== originalTab.id)!;
      check("the other tab remains unedited", !host.edits()!.state.dirty && (host.edits()!.state.text_edits?.length ?? 0) === 0);
      await host.activate(originalTab.id); await host.idle();
      check("tab switching commits the typed replacement", host.edits()!.state.text_edits?.[0]?.replacement === replacement);
      const pixels = async () => {
        if (!await settle(() => host.viewer()?.idle === true, SETTLE_MS)) throw new Error("text tiles did not settle");
        await pause(100);
        const viewer = host.viewer()!, canvas = viewer.compositedSurface;
        const context = canvas?.getContext("2d", { willReadFrequently: true });
        if (!canvas || !context) throw new Error("text check needs a readable composited surface");
        const a = viewer.screenPoint(page, passport ? 372 : agendaPage2 ? 110 : agenda ? 370 : w3c ? 55 : 35, passport ? 473 : agendaPage2 ? 40 : agenda ? 78 : w3c ? 68 : 40), b = viewer.screenPoint(page, passport ? 388 : agendaPage2 ? 200 : agenda ? 440 : w3c ? 190 : 250, passport ? 525 : agendaPage2 ? 67 : agenda ? 100 : w3c ? 90 : 70), dpr = devicePixelRatio;
        const left = Math.round(a.x*dpr), top = Math.round(a.y*dpr);
        const width = Math.round((b.x-a.x)*dpr), height = Math.round((b.y-a.y)*dpr);
        if (left < 0 || top < 0 || width < 1 || height < 1 || left+width > canvas.width || top+height > canvas.height) throw new Error(`text pixel sample is off screen: ${JSON.stringify({left,top,width,height,canvasWidth:canvas.width,canvasHeight:canvas.height})}`);
        const data = context.getImageData(left, top, width, height).data;
        if (!data.some((value, index) => index % 4 === 0 && value < 100)) throw new Error("text pixel sample contains no ink");
        return data;
      };
      if (wrapped) check("the journal retains the exact source space", host.edits()!.state.text_edits?.[0]?.original === original + " ");
      if (page === 1) {
        check("the edit belongs to the second page", host.edits()!.state.text_edits?.[0]?.page === 1);
        check("editing page two preserves page one before saving", (await read(0)) === untouched);
      }
      // The wide gap separates two geometric columns in this untagged fixture.
      // Check the same reading order before/after undo, with fresh edited text.
      const selectedContains = (text: string) => wideSpacing
        ? host.viewer()!.selectedText.trim() === `${text.split(" ")[0]} SYNTHETIC SECONDFIRST`
        : host.viewer()!.selectedText.includes(text);
      const editedPixels = await pixels();
      host.viewer()!.selectPage();
      if (!await settle(() => selectedContains(replacement), SETTLE_MS)) throw new Error(`selection does not contain the replacement: ${JSON.stringify(host.viewer()!.selectedText)}`);
      check("selection reads the unsaved replacement", !host.viewer()!.selectedText.includes(original));
      if (passport) {
        const selected = host.viewer()!.selectedText;
        for (let turn = 0; turn < 4; turn++) {
          const expectedRotation = (host.viewer()!.rotation + 1) % 4;
          host.run("view.rotateClockwise"); await host.idle();
          if (host.viewer()!.rotation !== expectedRotation) throw new Error("the requested view rotation did not apply");
          host.viewer()!.selectPage();
          if (!await settle(() => host.viewer()!.selectedText === selected, SETTLE_MS)) throw new Error("rotating the view changed mixed-direction selection order");
        }
        check("mixed-direction selection survives every view turn", true);
      }
      const edited = await read();
      check("unsaved extraction sees replacement and preserves adjacent text", edited.includes(replacement) && !edited.includes(original) && edited.includes(listChild ? "SYNTHETIC FIRST" : passport ? "Your passport" : agendaPage2 ? "Parish Council" : agenda ? "PARISH COUNCIL" : w3c ? "Dummy PDF fi" : "SYNTHETIC SECOND"));
      const matches = await call("search_page", { doc: host.edits()!.doc, page: filePage(page), query: replacement, options: { matchCase: true, wholeWord: false, regex: false } });
      check("unsaved search finds the replacement", matches.matches.length === 1);
      host.run("edit.undo"); await host.idle();
      check("undo restores source text", (await read()).includes(original) && !host.edits()!.state.dirty);
      const originalPixels = await pixels();
      check("undo repaints the original text on screen", originalPixels.length === editedPixels.length && originalPixels.some((value, index) => value !== editedPixels[index]));
      check("undo clears the stale selection", !host.viewer()!.selectedText);
      host.viewer()!.selectPage();
      if (!await settle(() => selectedContains(original), SETTLE_MS)) throw new Error("selection did not return to source text after undo");
      host.run("edit.redo"); await host.idle();
      check("redo restores edited text", (await read()).includes(replacement));
      const redoPixels = await pixels();
      check("redo restores exactly the edited pixels", redoPixels.length === editedPixels.length && redoPixels.every((value, index) => value === editedPixels[index]));
      // Use a source glyph: S in synthetic lines, l in the W3C subset.
      await start(); field()!.value = (passport ? "I" : agendaPage2 ? "C" : agenda ? "R" : w3c ? "l" : "S").repeat(80);
      document.querySelector<HTMLButtonElement>(".text-edit-apply")!.click();
      let refused = false; try { await host.idle(); } catch { refused = true; }
      check("an overflowing draft is refused without changing the journal", refused && host.edits()!.state.text_edits?.[0]?.replacement === replacement);
      document.querySelector<HTMLElement>(".text-edit-popup")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await host.idle();
      check("cancelling a refused draft returns focus to its text target", document.activeElement === target());
      if (overhang) {
        await start(); field()!.value = "ÄÖÜ äöü ß"; field()!.dispatchEvent(new Event("input", { bubbles: true }));
        document.querySelector<HTMLButtonElement>(".text-edit-apply")!.click();
        let refusedInk = ""; try { await host.idle(); } catch (error) { refusedInk = String(error); }
        // The leading A overhangs its origin. Every edit has carried the editor's
        // layout since 26.9.9, and the layout insets such a line by its overhang
        // (0.0176 pt here) so its ink starts at the box's edge. The refusal this
        // once expected, "replacement ink would exceed the original text bounds",
        // is the byte-patch writer's, which the application no longer sends.
        check("a shorter draft whose ink overhangs the left edge is inset into its box", refusedInk === "" && host.edits()!.state.text_edits?.[0]?.replacement === "ÄÖÜ äöü ß");
        document.querySelector<HTMLElement>(".text-edit-popup")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        await host.idle();
        host.run("edit.undo"); await host.idle();
        check("undo returns the line to the previous replacement", host.edits()!.state.text_edits?.[0]?.replacement === replacement && (await read()).includes(replacement));
      }
      host.run("file.save"); await host.idle();
      if (!await settle(() => !host.edits()?.state.dirty, SETTLE_MS)) throw new Error("text save did not finish");
      check("saved and reopened text matches the unsaved revision", (await read()).includes(replacement));
      if (page === 1) check("saving page two preserves page one", (await read(0)) === untouched);
      await host.activate(otherTab.id); await host.idle();
      check("saving did not change the other document", (await read()).includes(original) && !host.edits()!.state.dirty);
      break;
    }
    case "signatures": await signatureCheck(host, expected, report); break;
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
      const combo = () => document.querySelector<HTMLSelectElement>('.form-fields select:not([multiple])');
      const list = () => document.querySelector<HTMLSelectElement>('.form-fields select[multiple]');
      const radios = () => [...document.querySelectorAll<HTMLInputElement>('.form-fields input[type="radio"]')];
      const single = () => document.querySelector<HTMLSelectElement>('.form-fields select[size]:not([multiple])');
      const custom = () => document.querySelector<HTMLInputElement>('.form-fields input[list]');
      const mixed = !!combo();
      if (mixed) {
        check("mixed controls were discovered", radios().length === 2 && !!list());
        radios()[1]!.click(); await host.idle();
        check("choosing a radio clears its sibling", radios()[1]!.checked && !radios()[0]!.checked);
        host.run("edit.undo"); await host.idle();
        check("undo restores the radio group", radios()[0]!.checked && !radios()[1]!.checked);
        host.run("edit.redo"); await host.idle();
        check("redo restores the radio choice", radios()[1]!.checked && !radios()[0]!.checked);
        combo()!.value = "1"; combo()!.dispatchEvent(new Event("change")); await host.idle();
        check("dropdown uses the display label", combo()!.selectedOptions[0]?.textContent === "Second label");
        for (const option of list()!.options) option.selected = option.value === "0" || option.value === "2";
        list()!.dispatchEvent(new Event("change")); await host.idle();
        check("a list journals multiple selected indices", host.edits()?.state.forms?.some((f) => Array.isArray(f.value) && f.value.join(",") === "0,2") === true);
        single()!.value = "2"; single()!.dispatchEvent(new Event("change")); await host.idle();
        check("a single-selection list selects one item", single()!.selectedOptions.length === 1 && single()!.value === "2");
        custom()!.value = "Custom answer"; custom()!.dispatchEvent(new Event("change")); await host.idle();
        check("an editable dropdown accepts custom text", host.edits()?.state.forms?.some((f) => f.value === "Custom answer") === true);
        host.run("edit.undo"); await host.idle();
        check("undo restores an editable dropdown option", custom()?.value === "Alpha");
        host.run("edit.redo"); await host.idle();
        check("redo restores custom dropdown text", custom()?.value === "Custom answer");
        await host.activate(b.id); await host.activate(a.id);
        if (!await settle(() => !!combo() && !!list() && radios().length === 2, SETTLE_MS)) throw new Error("mixed controls did not remount");
        check("choice answers survive tab switching", combo()?.value === "1" && list()?.selectedOptions.length === 2 && radios()[1]?.checked === true);
      }
      host.run("file.save");
      check("saving freezes form edits", field()?.readOnly === true && document.querySelector<HTMLInputElement>('.form-fields input[type="checkbox"]')?.disabled === true);
      await host.idle();
      if (!await settle(() => field()?.value === "Grüße", SETTLE_MS)) throw new Error(`the saved field did not reopen: ${document.querySelector(".error")?.textContent ?? "no error shown"}`);
      check("save resets the journal", host.edits()?.state.dirty === false);
      check("saved controls are editable again", field()?.readOnly === false);
      const form = await call("document_form", { doc: host.edits()!.doc });
      check("both shared widgets reopen with the saved answer", form.widgets.filter((w) => w.value === "Grüße").length === 2);
      check("the checkbox reopens checked", form.widgets.some((w) => w.value === true));
      if (mixed) {
        check("saved dropdown preserves the second duplicate export", combo()?.value === "1");
        check("saved list preserves both selections", [...list()!.selectedOptions].map((o) => o.value).join(",") === "0,2");
        check("saved radio group has exactly one selected button", radios()[1]?.checked === true && radios()[0]?.checked === false);
        check("saved choices remain choice fields", form.widgets.filter((w) => w.control.kind === "choice").length === 4);
        check("single-list and custom dropdown answers reopen", single()?.value === "2" && custom()?.value === "Custom answer");
      }
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
      const background = document.getElementById(`document-tab-${a.id}`);
      background?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 30, clientY: 80 }));
      const tabActions = Array.from(document.querySelectorAll<HTMLElement>('.context-menu [role="menuitem"]'));
      check("tab menu offers reveal, copy path and close", tabActions.length === 3 &&
        /^Show in (Explorer|Finder)$/.test(tabActions[0]?.textContent ?? "") &&
        tabActions[1]?.textContent === "Copy file path" && tabActions[2]?.textContent === "Close");
      check("right-clicking a background tab keeps the active document", host.edits()?.doc === b.id);
      check("the header does not repeat the document filename", !document.querySelector("header .title"));
      let copiedPath = "";
      const clipboardWrite = navigator.clipboard.writeText;
      try {
        navigator.clipboard.writeText = async (value) => { copiedPath = value; };
        tabActions[1]?.click();
        await pause(50);
        check("copy path uses the clicked background tab", copiedPath === first && host.edits()?.doc === b.id);
      } finally { navigator.clipboard.writeText = clipboardWrite; }
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
      document.getElementById(`document-tab-${saved.id}`)?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 30, clientY: 80 }));
      Array.from(document.querySelectorAll<HTMLElement>('.context-menu [role="menuitem"]')).find((item) => item.textContent === "Close")?.click();
      if (!await settle(() => host.tabs().length === 1, SETTLE_MS)) throw new Error("tab menu did not close the background tab");
      await host.idle();
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
