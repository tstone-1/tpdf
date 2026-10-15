/**
 * The release build, each page laid out at its own size, and ink drawn past the first page's width.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import type { DocumentInfo } from "../ipc";
import { Viewer, type ViewerStatus } from "../viewer";
import { check, settle, skip } from "./common";
import { preview } from "./invert";

/**
 * That a document can be released, through the command the app really calls.
 *
 * `backend-probe` pins what releasing *does* --- the process dies, the id is
 * refused afterwards and never handed out again, and the descriptors come back.
 * What it cannot see is this: whether the Tauri command exists under that name
 * and takes an argument called `doc`. A wrong name there fails only at runtime,
 * only when someone opens a second file, and only as a warning in a console
 * nobody is watching --- so the leak comes back with every check still green.
 *
 * Opened here rather than reusing the document under test, which the viewer is
 * still reading from.
 */
export async function releaseChecks(path: string): Promise<void> {
  const extra = await invoke<DocumentInfo>("open_document", { path });
  const release = async (doc: number): Promise<string> => {
    try {
      await invoke("close_document", { doc });
      return "";
    } catch (e) {
      return String(e);
    }
  };

  const first = await release(extra.id);
  check(
    "a document can be released through the command",
    first === "",
    first ? preview(first) : `released document ${extra.id}`,
  );

  // The control, and it is the half that pins the argument. "It returned no
  // error" is equally true of a command that ignored `doc` entirely, or of one
  // that quietly succeeded on an id it had never seen; a second release of the
  // same id has to be refused, and refused *by that id*.
  const again = await release(extra.id);
  check(
    "releasing the same document twice is refused",
    again.includes(`document ${extra.id}`) && again.includes("closed"),
    again ? preview(again) : "it accepted the second release",
  );
}

/** What a fixture's generator says each of its pages is, and where its ink is. */
interface GeometryManifest {
  /** Page 1's size, i.e. the one a uniform layout would use for everything. */
  first_page: { width_pt: number; height_pt: number };
  pages: {
    page: number;
    name: string;
    width_pt: number;
    height_pt: number;
    markers: { text: string; x: number; y: number; width_pt: number }[];
  }[];
}

const LAID_OUT = "every page is laid out at its own size";
const OFFSETS = "page offsets accumulate each page's own height";
const DRAWN = "an oversized page is drawn past page 1's width";

/**
 * How tall a band around a marker's baseline is sampled for ink, in points.
 *
 * `make_mixed_pdf.py` sets its markers in 12-point Helvetica and does not record
 * the size, so this is deliberately generous rather than exact. Generous is the
 * safe direction: extra rows are blank page, which adds white and can never
 * invent ink, so a band too tall weakens nothing.
 */
const MARKER_BAND_PT = 20;

/** Fraction of a sampled region that has to be page rather than surround. */
const PAGE_WHITE = 0.3;
/** Fraction that has to be ink, which one line of 12-point type easily clears. */
const PAGE_INK = 0.005;

/**
 * The layout, against a description of the document that this process did not
 * write.
 *
 * The second check in this file whose answer comes from outside, and the reason
 * it needs to be: everything the viewer knows about a page's size it learned
 * from the same backend it renders through, so a check comparing the layout
 * against `doc.pages` would be the writer agreeing with its own reader. The
 * fixture's generator states every page's size and where every marker was drawn,
 * `viewer_check.py` passes that file in, and this compares against it.
 *
 * All three skip on a document with no sidecar, which is every fixture but
 * `mixed.pdf` --- and on one whose pages are all the same size, which is what
 * they would silently pass on. A property with one value present is the same as
 * none: an assertion that offsets follow each page's own height is satisfied by
 * multiplying page 1's when every page *is* page 1's size.
 *
 * The pages are visited before anything is asserted, because the open is lazy:
 * page 1's geometry is all that arrives and the rest is learned from the text
 * extraction of each page as it comes on screen. A check that asserted straight
 * away would be asserting against the estimate.
 */
export async function geometryChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [LAID_OUT, OFFSETS, DRAWN];
  const raw = await invoke<string | null>("geometry_manifest").catch(
    () => null,
  );
  if (!raw) {
    for (const name of names)
      skip(name, "no geometry sidecar for this fixture");
    return;
  }

  let manifest: GeometryManifest;
  try {
    manifest = JSON.parse(raw) as GeometryManifest;
  } catch (e) {
    for (const name of names) skip(name, `unreadable geometry sidecar: ${e}`);
    return;
  }

  const stated = manifest.pages.filter((entry) => entry.page < doc.page_count);
  const widths = new Set(stated.map((entry) => entry.width_pt));
  const heights = new Set(stated.map((entry) => entry.height_pt));
  if (widths.size < 2 && heights.size < 2) {
    for (const name of names) {
      skip(name, "every page of this fixture is the same size");
    }
    return;
  }

  // Every page visited, so every size is learned rather than estimated. The
  // wait is per page and its verdict is dropped: the checks below fail naming
  // the page that never resolved, which is more use than a second failure here.
  for (const entry of stated) {
    if (viewer.knowsPageSize(entry.page)) continue;
    viewer.goToPage(entry.page);
    await settle(() => viewer.knowsPageSize(entry.page));
  }

  const zoom = viewer.currentZoom;
  const unknown = stated.filter((entry) => !viewer.knowsPageSize(entry.page));
  const wrongBox = stated.filter((entry) => {
    const box = viewer.pageBoxCssOf(entry.page);
    return (
      Math.abs(box.width - entry.width_pt * zoom) > 1 ||
      Math.abs(box.height - entry.height_pt * zoom) > 1
    );
  });
  check(
    LAID_OUT,
    unknown.length === 0 && wrongBox.length === 0,
    unknown.length > 0
      ? `page ${(unknown[0]?.page ?? 0) + 1} never reported its size`
      : wrongBox.length === 0
        ? `${stated.length} pages, ${widths.size} widths and ${heights.size} heights`
        : `page ${(wrongBox[0]?.page ?? 0) + 1} is ` +
          `${viewer.pageBoxCssOf(wrongBox[0]?.page ?? 0).width.toFixed(0)} px wide, ` +
          `${((wrongBox[0]?.width_pt ?? 0) * zoom).toFixed(0)} wanted`,
  );

  // The gap between pages is the scroller's own constant and nothing out here
  // should be pinning the number; what is pinned is that the *same* gap
  // separates every pair and that the height between them is each page's own.
  // Derived from the first pair, so it is the later pages that are the claim ---
  // which is where a layout multiplying page 1's height and one accumulating
  // per page first disagree.
  const first = stated[0];
  const second = stated[1];
  if (!first || !second || stated.length < 3) {
    skip(OFFSETS, "fewer than three pages, so no offset can have accumulated");
  } else {
    const gap = viewer.pageTopCss(second.page) - first.height_pt * zoom;
    let expected = viewer.pageTopCss(second.page);
    const wrongTop: { page: number; got: number; want: number }[] = [];
    for (let index = 2; index < stated.length; index++) {
      const previous = stated[index - 1];
      const entry = stated[index];
      if (!previous || !entry) continue;
      expected += previous.height_pt * zoom + gap;
      const got = viewer.pageTopCss(entry.page);
      if (Math.abs(got - expected) > 1.5) {
        wrongTop.push({ page: entry.page, got, want: expected });
      }
    }
    const worst = wrongTop[0];
    check(
      OFFSETS,
      wrongTop.length === 0,
      worst
        ? `page ${worst.page + 1} starts at ${worst.got.toFixed(0)} px, ` +
            `${worst.want.toFixed(0)} wanted`
        : `${stated.length} pages, gap ${gap.toFixed(0)} px`,
    );
  }

  await drawnPastFirstPageCheck(viewer, manifest, stated, seen);
}

/**
 * That the widest page's pixels reach past page 1's width.
 *
 * The half the offsets cannot see, and the one that loses content rather than
 * misplacing it: the tile grid comes from the page's own width, so a page laid
 * out at page 1's is never *asked for* past it and is drawn cropped, silently.
 * There is no error to look for, which is why this reads pixels.
 *
 * `make_mixed_pdf.py` places a marker a few points past page 1's right edge
 * precisely so that a layout which is generous rather than correct --- one that
 * rounds a column up, or adds a tile of slack --- still misses something. The
 * page's own far-right markers sit 500 points beyond A4 and would be lost by any
 * wrong answer at all, which makes them the easy case.
 *
 * Two conditions, and the second is the instrument's control. The marker's box
 * must be mostly *page* (so what is on screen there is paper rather than the
 * surround the crop would leave) and must contain ink (so the paper is not
 * merely blank). A third sample, taken to the left of the page where there can
 * be no paper, is what says "white" means page at all --- without it, a canvas
 * read back as uniform white would satisfy both.
 */
async function drawnPastFirstPageCheck(
  viewer: Viewer,
  manifest: GeometryManifest,
  stated: GeometryManifest["pages"],
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const widest = stated.reduce((a, b) => (b.width_pt > a.width_pt ? b : a));
  if (widest.width_pt <= manifest.first_page.width_pt) {
    skip(DRAWN, "no page of this fixture is wider than page 1");
    return;
  }
  const beyond = widest.markers
    .filter((marker) => marker.x > manifest.first_page.width_pt)
    .sort((a, b) => a.x - b.x)[0];
  if (!beyond) {
    skip(DRAWN, "the widest page has no ink past page 1's width");
    return;
  }

  viewer.goToPage(widest.page);
  await settle(() => viewer.idle);
  // Refitted here rather than relying on whatever the last fit left behind: the
  // sample has to be on screen, and a wide page at the previous page's scale
  // runs off the side of the window.
  viewer.setFit("width");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const surface = viewer.compositedSurface;
  const ctx = surface?.getContext("2d", { willReadFrequently: true }) ?? null;
  if (!surface || !ctx) {
    skip(
      DRAWN,
      "this layout composites per tile, so there is no single surface",
    );
    return;
  }

  const dpr = window.devicePixelRatio || 1;
  /** Fractions of white and of ink in a box given in the page's own points. */
  const sample = (
    x: number,
    width: number,
  ): { white: number; ink: number } | null => {
    // The marker's `y` is a baseline measured up from the page's bottom edge, as
    // PDF coordinates are; the viewer works down from the page's top.
    const top = widest.height_pt - beyond.y - MARKER_BAND_PT + 4;
    const a = viewer.screenPoint(widest.page, x, top);
    const b = viewer.screenPoint(widest.page, x + width, top + MARKER_BAND_PT);
    const left = Math.max(0, Math.round(Math.min(a.x, b.x) * dpr));
    const upper = Math.max(0, Math.round(Math.min(a.y, b.y) * dpr));
    const right = Math.min(surface.width, Math.round(Math.max(a.x, b.x) * dpr));
    const lower = Math.min(
      surface.height,
      Math.round(Math.max(a.y, b.y) * dpr),
    );
    if (right - left < 2 || lower - upper < 2) return null;
    const { data } = ctx.getImageData(left, upper, right - left, lower - upper);
    let white = 0;
    let ink = 0;
    for (let at = 0; at < data.length; at += 4) {
      const lightness =
        ((data[at] ?? 0) + (data[at + 1] ?? 0) + (data[at + 2] ?? 0)) /
        (3 * 255);
      if (lightness > 0.85) white++;
      if (lightness < 0.4) ink++;
    }
    const pixels = data.length / 4;
    return { white: white / pixels, ink: ink / pixels };
  };

  const marker = sample(beyond.x, beyond.width_pt);
  // Left of the page's own left edge, where there is nothing but surround. Same
  // band, same size, so the only difference is where it is.
  const outside = sample(-beyond.width_pt - 8, beyond.width_pt);
  if (!marker || !outside) {
    skip(DRAWN, "the sampled region fell outside the composited surface");
    return;
  }

  check(
    DRAWN,
    marker.white > PAGE_WHITE &&
      marker.ink > PAGE_INK &&
      outside.white < PAGE_WHITE,
    `"${beyond.text}" at ${beyond.x.toFixed(0)} pt: ` +
      `${(marker.white * 100).toFixed(0)}% page, ${(marker.ink * 100).toFixed(1)}% ink ` +
      `(off the page: ${(outside.white * 100).toFixed(0)}% page)`,
  );
}
