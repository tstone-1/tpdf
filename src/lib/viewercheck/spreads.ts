/**
 * Facing pages on a real document: both pages of a row come sharp, and the gap
 * between them is in the middle of the screen.
 *
 * One phase of the window check, run by `viewercheck.ts`; that file says what
 * the check is and how it is run. `scrollerspread.test.ts` and
 * `viewerspread.test.ts` hold the arithmetic. What only a window can show is
 * whether the tile window asks for the right-hand page at all --- a page placed
 * correctly and never requested is a blank half of the screen --- and whether
 * the compositor puts the pages where the arithmetic says.
 */

import type { DocumentInfo } from "../ipc";
import { SINGLE, type Spread } from "../scroller";
import { Viewer, type ViewerStatus } from "../viewer";
import { check, settle, skip } from "./common";
import { meanLightness } from "./invert";

const FACING: Spread = { columns: 2, coverAlone: false, rightToLeft: false, continuous: true };

/** Leaves the view one page to a row, fitted and where it found it. */
export async function spreadChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const sharp = "both pages of a row come sharp under facing pages";
  const gap = "the gap between facing pages is in the middle of the screen";
  const back = "one page to a row covers the middle of the screen again";

  const surface = viewer.compositedSurface;
  if (!surface) {
    for (const name of [sharp, gap, back]) skip(name, "this layout composites per tile, so there is no single surface");
    return;
  }
  if (doc.page_count < 2) {
    for (const name of [sharp, gap, back]) skip(name, "the document has one page");
    return;
  }

  const fit = viewer.fitMode;
  const zoom = viewer.currentZoom;
  const page = viewer.position.page;

  // On a short landscape spread, the middle of the window can show the
  // next row, which may have only a left page. Sample the visible first row.
  const strip = (x: number, facing = false): number | null => {
    const ctx = surface.getContext("2d", { willReadFrequently: true });
    if (!ctx || !surface.clientHeight) return null;
    const scale = surface.height / surface.clientHeight;
    const first = viewer.screenPoint(0, 0, 0).y;
    const lastPage = facing ? 1 : 0;
    const last = viewer.screenPoint(lastPage, 0, 0).y;
    const top = Math.max(0, Math.ceil(Math.max(first, last) * scale));
    const bottom = Math.min(surface.height, Math.floor(Math.min(
      first + viewer.pageBoxCssOf(0).height,
      last + viewer.pageBoxCssOf(lastPage).height,
    ) * scale));
    const h = Math.floor((bottom - top) / 3);
    if (h < 1) throw new Error("The first spread has no visible row to sample");
    return meanLightness(ctx.getImageData(Math.floor(x) - 2, top + h, 4, h).data);
  };
  const settled = () => settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  viewer.setFit("width");
  viewer.goToPage(0);
  await settled();
  const middle = surface.width / 2;
  // The surround, read at the left edge, inside the fit-width margin.
  const surround = strip(3);
  const paperBefore = strip(middle);

  viewer.setSpread(FACING);
  viewer.goToPage(0);
  await settled();
  const coverage = seen.status?.sharp ?? 0;
  check(
    sharp,
    coverage >= 0.999,
    `sharp coverage ${(coverage * 100).toFixed(1)}% with pages 1 and 2 side by side`,
  );

  const gapNow = strip(middle, true);
  const left = strip(surface.width / 4, true);
  const right = strip((surface.width * 3) / 4, true);
  if (surround === null || paperBefore === null || gapNow === null || left === null || right === null) {
    skip(gap, "the surface could not be read back");
  } else if (Math.abs(paperBefore - surround) < 0.15) {
    // A page the colour of the surround cannot show a gap in it, and saying so
    // is the point: a check that passed here would be measuring nothing.
    skip(gap, `the page at ${(paperBefore * 100).toFixed(0)}% is too close to the surround at ${(surround * 100).toFixed(0)}%`);
  } else {
    const detail =
      `middle ${(gapNow * 100).toFixed(0)}%, surround ${(surround * 100).toFixed(0)}%, ` +
      `pages ${(left * 100).toFixed(0)}% and ${(right * 100).toFixed(0)}%`;
    check(
      gap,
      Math.abs(gapNow - surround) < 0.05 &&
        Math.abs(left - surround) > 0.1 &&
        Math.abs(right - surround) > 0.1,
      detail,
    );
  }

  viewer.setSpread(SINGLE);
  viewer.goToPage(0);
  await settled();
  const paperAfter = strip(middle);
  if (paperAfter === null || surround === null || paperBefore === null) {
    skip(back, "the surface could not be read back");
  } else if (Math.abs(paperBefore - surround) < 0.15) {
    skip(back, "the page is too close to the surround to tell them apart");
  } else {
    check(back, Math.abs(paperAfter - surround) > 0.1, `middle ${(paperAfter * 100).toFixed(0)}%`);
  }

  if (fit === "none") viewer.setZoomFixed(zoom);
  else viewer.setFit(fit);
  viewer.goToPage(page);
  await settle(() => viewer.idle);
}
