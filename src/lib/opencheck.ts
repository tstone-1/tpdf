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

import { invoke } from "@tauri-apps/api/core";

import { pause, Report, settle } from "./checkreport";

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

/** What the check needs from the running application. */
export interface OpenCheckHost {
  /** Path of the open document, or "". */
  path: () => string;
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
  const mode = await invoke<string | null>("opencheck_mode");
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
function name(path: string): string {
  return path.split("/").pop() ?? path;
}

async function run(host: OpenCheckHost, phase: string, expected: string): Promise<void> {
  switch (phase) {
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

    default:
      report.check("the phase is one this check knows", false, `unknown phase ${phase.slice(0, 20)}`);
  }
}
