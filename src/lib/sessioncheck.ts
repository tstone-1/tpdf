/**
 * A functional check of session restore, across two launches of the real app.
 *
 * Every other harness here replaces the application: they open a document, do
 * their work and exit, and the shell in `App.svelte` never boots. That cannot
 * work for this one. Restoring is *part of* the boot --- the wiring between the
 * viewer, the writer and the session file is most of what can be wrong --- so a
 * check that drove `session.ts` directly would be a second implementation
 * agreeing with itself. This repository has already been caught by that twice.
 *
 * So the app boots normally in both phases, and the check observes it through
 * the same handles `App.svelte` uses.
 *
 * The four phases, and what each is for:
 *
 * | phase | session file | argument | asserts |
 * |---|---|---|---|
 * | `record`  | fresh | a document | drives to a distinctive state and writes it |
 * | `default` | empty | a document | that state is **not** where the app opens by itself |
 * | `verify`  | recorded | none | the app came up in that state, having been told only by the file |
 * | `empty`   | empty | none | no document opens when nothing is remembered |
 *
 * `default` and `empty` are the controls, and neither is optional. Without
 * `default`, "restored to page 12 at 200%" is satisfied by an app that happens
 * to open there --- the failure mode this repository names as an assertion whose
 * precondition is already satisfied. Without `empty`, an app that reopened the
 * *last document it could find* by some other route would pass `verify`
 * perfectly.
 */

import { invoke } from "@tauri-apps/api/core";

import { pause, Report, settle as settleFor } from "./checkreport";
import type { Viewer } from "./viewer";

/** How long any single wait may take before the check gives up. */
const TIMEOUT_MS = 30_000;

const report = new Report();

/** Waits for a condition, returning whether it arrived. */
const settle = (predicate: () => boolean) => settleFor(predicate, TIMEOUT_MS);

const check = (name: string, ok: boolean, detail: string) => report.check(name, ok, detail);

/**
 * The state `record` drives to, and `verify` expects back.
 *
 * Every field differs from what a freshly opened document has --- page 0,
 * fit-width, upright, sidebar hidden --- because a restore that only agreed
 * with the default would be indistinguishable from no restore at all. The
 * `default` phase asserts exactly that, so this table is not merely a
 * convention.
 */
const TARGET = {
  /** Zero-based, and far enough in that no default lands on it. */
  page: 7,
  /** A zoom stop, reached by stepping, so `fitting` ends up false. */
  fitting: false,
  /** One quarter turn clockwise. */
  turns: 1,
  /** Open, where a fresh window has it closed. */
  sidebar: true,
  /**
   * Pages inverted, where a fresh window shows them as the document has them.
   *
   * The one field here that is *not* part of a place: it is a preference, saved
   * beside the list rather than inside an entry and written by its own command.
   * That is exactly why it needs covering here --- the place writer skips a
   * place equal to the last one it sent, so an inversion routed through it would
   * never be written at all, and nothing else in this file would notice.
   */
  invert: true,
};

/** What the check needs from the running application. */
export interface SessionCheckHost {
  /** Opens a document, exactly as a drop or the file dialog would. */
  open: (path: string) => Promise<void>;
  /** The live viewer, or null when no document is open. */
  viewer: () => Viewer | null;
  /** The element the viewer was mounted on, which is where its keys land. */
  root: () => HTMLElement | null;
  /** Path of the open document, or "". */
  path: () => string;
  /**
   * Pages in the open document, or 0 when nothing is open.
   *
   * Needed only to state a precondition, and it earns its place in this interface
   * by how the absence of it read. {@link TARGET.page} is 7, so this check cannot
   * run on a document with fewer than eight pages --- and without the guard below
   * it did not say so: `Viewer.goToPage` clamps to the last page, so a one-page
   * fixture reported *"it opens on the remembered page: page 0, wanted 7"* and a
   * four-page one *"page 2, wanted 7"*. Both read as a broken session restore,
   * stably and reproducibly, on a session restore that was working perfectly.
   */
  pageCount: () => number;
  /** Whether the sidebar is showing. */
  sidebarShown: () => boolean;
  /** Toggles it, through the same function the command and the key use. */
  toggleSidebar: () => void;
  /** Writes any outstanding place now. */
  flush: () => void;
  /**
   * Titles of the recent-document commands the palette is currently offering.
   *
   * The one thing about the recents list that no unit test can reach. The label
   * logic and the registry's group replacement are covered in `recents.test.ts`
   * and `commands.test.ts`; what is only true at runtime is that the chain
   * exists at all --- the session file Rust wrote, read back, turned into
   * commands, and registered. Each link is right on its own and none of them is
   * wired to the next by anything a compiler checks.
   */
  recentCommands: () => string[];
}

/** Dispatches a keydown at the viewer's root, the way the window would. */
function key(root: HTMLElement, k: string, accel = false): void {
  root.dispatchEvent(
    new KeyboardEvent("keydown", { key: k, metaKey: accel, bubbles: true, cancelable: true }),
  );
}

/**
 * Dispatches a chord at the window, where `App.svelte`'s own handler listens.
 *
 * The viewer's root is the wrong target for these: its handler never sees them,
 * and a check that called the toggle directly would leave the binding itself
 * untested --- which for a shortcut advertised in the palette is the whole of
 * what can be wrong. A label teaching a chord that does nothing is worse than no
 * label.
 */
function windowKey(k: string, shift = false): void {
  window.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: k,
      metaKey: true,
      shiftKey: shift,
      bubbles: true,
      cancelable: true,
    }),
  );
}

/** How a phase describes the state it found, for a detail column. */
function describe(host: SessionCheckHost): string {
  const viewer = host.viewer();
  if (!viewer) return "no document open";
  const file = host.path().split("/").pop() ?? "";
  return (
    `${file} page ${viewer.position.page} zoom ${viewer.currentZoom.toFixed(2)} ` +
    `turns ${viewer.rotation} sidebar ${host.sidebarShown() ? "open" : "closed"}` +
    `${viewer.inverted ? " inverted" : ""}${viewer.isFitting ? " fitting" : ""}`
  );
}

/**
 * Drives the open document to {@link TARGET}, through the real input paths.
 *
 * Keys at the viewer's root rather than method calls, so what is exercised is
 * the handler a keyboard reaches. The exception is the page jump: there is no
 * "go to page N" shortcut yet, and pressing Down until page 7 arrives would be
 * a check of the scroller rather than of the session.
 */
async function driveToTarget(host: SessionCheckHost): Promise<void> {
  const viewer = host.viewer();
  const root = host.root();
  if (!viewer || !root) throw new Error("nothing open to drive");

  if (!host.sidebarShown()) host.toggleSidebar();
  // The advertised chord, not the function behind it: ⌘⇧I is a claim the palette
  // makes, and this is the only thing that tests it.
  if (viewer.inverted !== TARGET.invert) windowKey("I", true);
  key(root, "r", true);
  key(root, "-", true);
  viewer.goToPage(TARGET.page);
  await settle(() => viewer.idle);
}

/** Asserts the live app is in the state `record` left. */
function checkRestored(host: SessionCheckHost, expectedPath: string): void {
  const viewer = host.viewer();
  check("a document is open", viewer !== null, describe(host));
  if (!viewer) return;

  check(
    "it is the document that was remembered",
    host.path() === expectedPath,
    `${host.path()} vs ${expectedPath}`,
  );
  check(
    "it opens on the remembered page",
    viewer.position.page === TARGET.page,
    `page ${viewer.position.page}, wanted ${TARGET.page}`,
  );
  check(
    "it opens at the remembered rotation",
    viewer.rotation === TARGET.turns,
    `turns ${viewer.rotation}, wanted ${TARGET.turns}`,
  );
  check(
    "it opens at a fixed zoom, not fitted",
    viewer.isFitting === TARGET.fitting,
    `${viewer.isFitting ? "fitting" : "fixed"} at ${viewer.currentZoom.toFixed(2)}`,
  );
  check(
    "it opens with the sidebar as it was left",
    host.sidebarShown() === TARGET.sidebar,
    host.sidebarShown() ? "open" : "closed",
  );
  check(
    "it opens with pages inverted, as they were left",
    viewer.inverted === TARGET.invert,
    viewer.inverted ? "inverted" : "as the document has them",
  );
}

/**
 * Asserts a freshly opened document is *not* already in the target state.
 *
 * The control that makes `verify` mean something. Every field is compared, and
 * the check fails if *any* of them already matches --- not merely if all of
 * them do: a restore that only got the rotation right would otherwise be
 * covered by a default that happened to share the page.
 */
function checkNotAlreadyThere(host: SessionCheckHost): void {
  const viewer = host.viewer();
  check("a document opened without a session", viewer !== null, describe(host));
  if (!viewer) return;

  const same: string[] = [];
  if (viewer.position.page === TARGET.page) same.push("page");
  if (viewer.rotation === TARGET.turns) same.push("rotation");
  if (viewer.isFitting === TARGET.fitting) same.push("zoom mode");
  if (host.sidebarShown() === TARGET.sidebar) same.push("sidebar");
  if (viewer.inverted === TARGET.invert) same.push("inversion");

  check(
    "the default state is not the remembered one",
    same.length === 0,
    same.length === 0
      ? `differs in all five: ${describe(host)}`
      : `already matches on ${same.join(", ")} — the restore check would pass without restoring`,
  );
}

/**
 * Runs the session check if `TPDF_SESSIONCHECK` is set, then exits.
 *
 * Returns `false` when it was not requested, so the caller carries on into the
 * real application. Called *after* the boot's own restore, deliberately: what
 * is being checked is what that restore did.
 */
export async function runSessionCheckIfRequested(host: SessionCheckHost): Promise<boolean> {
  const mode = await invoke<string | null>("sessioncheck_mode");
  if (!mode) return false;

  const [phase, ...rest] = mode.split(":");
  const argument = rest.join(":");

  try {
    await run(host, phase ?? "", argument);
  } catch (e) {
    check("the phase ran", false, String(e));
  }

  await report.finish();
  return true;
}

async function run(host: SessionCheckHost, phase: string, argument: string): Promise<void> {
  switch (phase) {
    case "record": {
      await host.open(argument);
      const opened = await settle(() => host.viewer() !== null);
      check("the document opened", opened, describe(host));
      if (!opened) return;

      // The precondition, stated before anything can misattribute it. `goToPage`
      // clamps to the last page, so on a short document every later phase reports
      // the *wrong page* rather than the wrong fixture --- see `pageCount` on the
      // host interface for what that looked like. Named as a check so it appears in
      // the summary rather than only on the console: a run that cannot test page
      // restore has to say so where the counts are read.
      // Waited for, because the status the count comes from is published a frame or
      // two after the viewer exists --- so reading it straight after `opened` gives
      // **0 for every document**, and the guard then refuses the long fixtures it was
      // written to admit. Caught by running it: `text-base14.pdf` reported "0 pages"
      // where 1 is the truth, and 0 is not a page count, it is "not yet".
      //
      // The three outcomes are kept distinct on purpose. "Never became known" is not
      // "too short": one is a fixture to swap, the other is a viewer that did not
      // finish opening, and collapsing them would send a reader to the wrong place.
      const known = await settle(() => host.pageCount() > 0);
      const pages = host.pageCount();
      const longEnough = known && pages > TARGET.page;
      check(
        "the document is long enough to test page restore",
        longEnough,
        !known
          ? "the page count never became known, so this cannot be judged --- the " +
              "document did not finish opening"
          : longEnough
            ? `${pages} pages, and page ${TARGET.page} is the target`
            : `${pages} pages, but page ${TARGET.page} is the target --- rerun with a ` +
              `document of at least ${TARGET.page + 1} pages`,
      );
      if (!longEnough) return;

      await driveToTarget(host);
      // Asserted, not assumed. Written as `check(..., true, ...)` first, which
      // is decoration however relevant it reads --- and it is the precondition
      // for all three phases after this one, so a drive that quietly did
      // nothing would send them off to explain a restore that had nothing to
      // restore.
      const driven = host.viewer();
      check(
        "it reached the state to be remembered",
        driven !== null &&
          driven.position.page === TARGET.page &&
          driven.rotation === TARGET.turns &&
          driven.isFitting === TARGET.fitting &&
          host.sidebarShown() === TARGET.sidebar &&
          driven.inverted === TARGET.invert,
        describe(host),
      );
      // Without this the run would exit inside the writer's interval and the
      // trailing write would never happen -- which is the same path the window
      // closing takes.
      host.flush();
      // The write is an IPC call and the process is about to exit; letting it
      // land is the whole point of the phase. The script that drives this reads
      // the file afterwards, so a write that did not finish fails there rather
      // than passing quietly here.
      await pause(500);
      break;
    }

    case "default": {
      await host.open(argument);
      await settle(() => host.viewer() !== null);
      checkNotAlreadyThere(host);
      break;
    }

    case "verify": {
      // Nothing is opened here. Whatever is on screen was put there by the
      // boot, from the session file alone.
      const opened = await settle(() => host.viewer() !== null);
      if (!opened) {
        check("a document is open", false, "nothing was restored");
        return;
      }
      await settle(() => host.viewer()?.idle === true);
      checkRestored(host, argument);

      // The document that was just restored is by definition the most recently
      // read one, so it must be offered. Asserted on the *first* entry rather
      // than on membership: the list is most-recent-first, and a list that
      // merely contains it would pass for one in an arbitrary order.
      const offered = host.recentCommands();
      const name = host.path().split(/[\\/]+/).pop() ?? "";
      check(
        "the document just restored is offered as a recent one",
        offered[0]?.endsWith(name) === true && name !== "",
        `${offered.length} recent commands, first is ${offered[0] ?? "(none)"}; ` +
          `the open document is ${name || "(none)"}`,
      );
      break;
    }

    case "empty": {
      // Give a restore that should not happen every chance to happen.
      await pause(1500);
      check(
        "no document opens when nothing is remembered",
        host.viewer() === null,
        describe(host),
      );
      // The control for the check above. A recents list that was populated from
      // somewhere other than the session --- or one built once and never
      // replaced --- would offer something here, where the session file has
      // been removed and there is nothing to offer.
      check(
        "nothing is offered as recent when nothing is remembered",
        host.recentCommands().length === 0,
        `${host.recentCommands().length} recent commands: ` +
          `${host.recentCommands().join(", ") || "(none)"}`,
      );
      break;
    }

    default:
      check("the phase is one this check knows", false, `unknown phase ${phase.slice(0, 20)}`);
  }
}
