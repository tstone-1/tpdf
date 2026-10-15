/**
 * Fit height and fit content in the running application: `tabs_check.py
 * --phase fit`, on a document with wide margins.
 *
 * `zoom.test.ts` and `inkbounds.test.ts` hold the arithmetic. What only the
 * window can show is fit content's measurement, which reads the pixels of a
 * rendered placeholder, and where the view lands: the printed part of the page
 * has to fill the window and be cut off at no edge. On a page printed nearly
 * to its edges fit content is fit page, and nothing here could fail, so the
 * phase first asserts that the fixture has margins to leave out.
 */

import type { OpenCheckHost } from "./opencheck";
import { settle, type Report } from "./checkreport";
import { CONTENT_MARGIN } from "./zoom";

const SETTLE_MS = 20_000;
/** How far apart two colours may be and still count as one. */
const SAME = 48;

/** The largest difference of one channel between the pixel at `at` and `colour`. */
function apart(rgba: Uint8ClampedArray, at: number, colour: readonly number[]): number {
  return Math.max(
    Math.abs(rgba[at]! - colour[0]!),
    Math.abs(rgba[at + 1]! - colour[1]!),
    Math.abs(rgba[at + 2]! - colour[2]!),
  );
}

export async function fitCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const [source] = expected.split("|");
  if (!source) throw new Error("a document is required");
  await host.open(source);
  await host.idle();
  const viewer = host.viewer();
  if (!viewer) throw new Error("no viewer");
  const settled = async () => {
    await settle(() => viewer.idle && (host.status()?.sharp ?? 0) >= 0.999, SETTLE_MS);
  };
  const surface = viewer.compositedSurface;
  if (!surface) throw new Error("this layout composites per tile, so there is no single surface");
  const ctx = surface.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("the surface could not be read back");
  const scale = window.devicePixelRatio || 1;

  host.run("view.fitHeight");
  await settled();
  // Read now and not before: the window's content area changes once the tab
  // bar arrives after the open, and the fit follows it.
  const windowHeight = surface.height / scale;
  const tall = viewer.pageBoxCss.height;
  report.check("Fit height makes the page as tall as the window",
    viewer.fitMode === "height" && Math.abs(tall - windowHeight) <= 1,
    `${viewer.fitMode}: page ${tall.toFixed(1)}px tall in ${windowHeight}px, zoom ${viewer.currentZoom.toFixed(3)}`);

  // The two colours that are not ink, read at fit width on the first page's
  // top-left corner: the surround inside the fit margin, and the paper just
  // inside the page.
  host.run("view.fitWidth");
  viewer.goToPage(0);
  await settled();
  const pixel = (x: number, y: number): number[] =>
    [...ctx.getImageData(Math.floor(x * scale), Math.floor(y * scale), 1, 1).data];
  const surround = pixel(4, 4);
  const paper = pixel(30, 4);
  if (apart(new Uint8ClampedArray(paper), 0, surround) <= SAME) throw new Error("the paper is the colour of the surround");

  /** The box round what is neither paper nor surround, in surface pixels, above the second page. */
  const inkBox = (): { left: number; top: number; right: number; bottom: number } | null => {
    const width = surface.width;
    const below = (viewer.pageTopCss(1) - viewer.offset) * scale;
    const high = Math.max(0, Math.min(surface.height, Math.floor(below)));
    if (high === 0) return null;
    const data = ctx.getImageData(0, 0, width, high).data;
    let left = width, right = -1, top = high, bottom = -1;
    for (let y = 0; y < high; y++) {
      for (let x = 0; x < width; x++) {
        const at = (y * width + x) * 4;
        if (apart(data, at, paper) <= SAME || apart(data, at, surround) <= SAME) continue;
        left = Math.min(left, x); right = Math.max(right, x);
        top = Math.min(top, y); bottom = Math.max(bottom, y);
      }
    }
    return right < 0 ? null : { left, top, right, bottom };
  };
  /** How much of the window, less the margins, the printed part fills across and down. */
  const filled = (box: { left: number; top: number; right: number; bottom: number }) => ({
    across: (box.right - box.left + 1) / (surface.width - 2 * CONTENT_MARGIN * scale),
    down: (box.bottom - box.top + 1) / (surface.height - 2 * CONTENT_MARGIN * scale),
  });
  const describe = (box: { left: number; top: number; right: number; bottom: number }): string => {
    const { across, down } = filled(box);
    return `ink ${box.left},${box.top} to ${box.right},${box.bottom} on ${surface.width}x${surface.height}: ` +
      `${(across * 100).toFixed(0)}% across, ${(down * 100).toFixed(0)}% down`;
  };

  host.run("view.fitPage");
  await settled();
  const pageZoom = viewer.currentZoom;
  const atPage = inkBox();
  // The control: at fit page the printed part must leave room, or fit content
  // has nothing to do and the checks below could not fail.
  const room = atPage === null ? 0 : 1 - Math.max(filled(atPage).across, filled(atPage).down);
  report.check("the fixture's printed part leaves room at fit page", room >= 0.1,
    atPage === null ? "nothing printed on screen" : describe(atPage));

  host.run("view.fitContent");
  await settled();
  // The placeholder can arrive after the first fit, which then fits again.
  await settled();
  const contentZoom = viewer.currentZoom;
  report.check("Fit content shows the printed part larger than fit page",
    viewer.fitMode === "content" && contentZoom > pageZoom * 1.05,
    `zoom ${(pageZoom * 100).toFixed(1)}% at fit page, ${(contentZoom * 100).toFixed(1)}% at fit content`);

  const box = inkBox();
  if (box === null) {
    report.check("Fit content fills the window with the printed part", false, "nothing printed on screen");
    return;
  }
  const { across, down } = filled(box);
  // The placeholder is 150 px wide, so the box it gives is a few pixels larger
  // than the ink; nine tenths leaves room for that.
  report.check("Fit content fills the window with the printed part", Math.max(across, down) >= 0.9, describe(box));
  const margin = (CONTENT_MARGIN / 2) * scale;
  report.check("Fit content cuts the printed part off at no edge",
    box.left >= margin && box.top >= margin &&
      box.right <= surface.width - margin && box.bottom <= surface.height - margin,
    describe(box));
}
