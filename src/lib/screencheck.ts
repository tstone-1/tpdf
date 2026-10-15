/**
 * Full screen and presentation in the running application: `tabs_check.py
 * --phase screen`.
 *
 * `fullscreen.test.ts` holds what each state changes in the viewer. What only
 * the window can show is whether the platform really goes full screen, whether
 * the chrome really hides, and whether tpdf follows the window when the window
 * leaves full screen without being asked to by tpdf --- the path the green
 * button on macOS takes, driven here by asking the window directly. It takes
 * the screen it runs on, so it is run on a machine nobody is working at.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";

const SETTLE_MS = 20_000;
/** macOS animates into and out of full screen, which takes about a second. */
const WINDOW_MS = 8000;

export async function screenCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [source] = expected.split("|");
  if (!source) throw new Error("a document is required");
  await host.open(source);
  await host.idle();
  if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) throw new Error("the viewer did not settle");

  const win = getCurrentWindow();
  /** Waits for the window to say it is, or is not, full screen. */
  const windowFull = async (want: boolean): Promise<boolean> => {
    const deadline = performance.now() + WINDOW_MS;
    while (performance.now() < deadline) {
      if ((await win.isFullscreen()) === want) {
        // And a little longer, so the resizes of the animation have arrived.
        await pause(500);
        return true;
      }
      await pause(100);
    }
    return false;
  };
  const headerShown = () => {
    const header = document.querySelector("header");
    return header !== null && getComputedStyle(header).display !== "none";
  };
  /**
   * A key pressed on the page, as a reader presses it: on the focused viewer,
   * from where it bubbles to the window if the viewer does not take it.
   */
  const key = (k: string) => {
    viewer().focus();
    (document.activeElement ?? document.body).dispatchEvent(
      new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }),
    );
  };
  const blank = () => document.querySelector('[data-testid="blank-screen"]');
  const viewer = () => {
    const shown = host.viewer();
    if (!shown) throw new Error("no viewer");
    return shown;
  };

  report.emit("[screen] full screen");
  const before = { spread: { ...viewer().spread }, fit: viewer().fitMode };
  report.check("the window starts out of full screen with its chrome", !(await win.isFullscreen()) && headerShown(),
    `full screen ${await win.isFullscreen()}, header ${headerShown()}`);
  host.run("view.fullScreen");
  const entered = await windowFull(true);
  report.check("full screen makes the window full screen", entered, `the window says full screen: ${entered}`);
  report.check("full screen hides the toolbar", !headerShown(), `header ${headerShown()}`);
  report.check("full screen leaves the layout alone",
    JSON.stringify(viewer().spread) === JSON.stringify(before.spread), JSON.stringify(viewer().spread));
  host.run("view.fullScreen");
  const back = await windowFull(false);
  report.check("full screen again goes back to the window", back, `the window says it left full screen: ${back}`);
  report.check("and the toolbar comes back", headerShown(), `header ${headerShown()}`);

  report.emit("[screen] presenting");
  host.run("view.presentation");
  const presenting = await windowFull(true);
  report.check("presenting makes the window full screen", presenting, `the window says full screen: ${presenting}`);
  report.check("presenting shows one page at a time, fitted to the page",
    viewer().spread.continuous === false && viewer().fitMode === "page",
    `continuous ${viewer().spread.continuous}, fit ${viewer().fitMode}`);
  const black = document.documentElement.classList.contains("presenting");
  report.check("presenting puts the pages on black", black, `presenting class on the root: ${black}`);
  const page = viewer().position.page;
  key(" ");
  await pause(300);
  // At fit page the page is in view whole, so Space is at its end at once.
  report.check("Space goes on to the next page", viewer().position.page === page + 1,
    `page ${page + 1} -> ${viewer().position.page + 1}`);
  key("b");
  await pause(100);
  report.check("b blacks out the screen", blank() !== null, `blank screen shown: ${blank() !== null}`);
  key("x");
  await pause(100);
  report.check("any key brings the page back", blank() === null, `blank screen shown: ${blank() !== null}`);

  key("Escape");
  const left = await windowFull(false);
  report.check("Esc leaves the presentation and full screen", left, `the window says it left full screen: ${left}`);
  report.check("leaving puts back the layout and the fit",
    viewer().spread.continuous === before.spread.continuous && viewer().fitMode === before.fit,
    `continuous ${viewer().spread.continuous}, fit ${viewer().fitMode}`);
  const still = document.documentElement.classList.contains("presenting");
  report.check("and the pages are no longer on black", !still, `presenting class on the root: ${still}`);

  report.emit("[screen] the window leaving full screen by itself");
  host.run("view.presentation");
  if (!(await windowFull(true))) {
    report.check("tpdf follows the window out of full screen", false, "the presentation did not start");
    return;
  }
  await win.setFullscreen(false);
  const followed = await settle(() => headerShown() && viewer().spread.continuous === before.spread.continuous, SETTLE_MS);
  report.check("tpdf follows the window out of full screen", followed,
    `header ${headerShown()}, continuous ${viewer().spread.continuous}`);
}
