import { describe, expect, it } from "vitest";
import { applyPlace, hasPlace, launchPage, launchZoom, type Opening, type Placeable } from "./launchplace";

/** A viewer that records what it was asked, in order. */
function recorder(): Placeable & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    goToPage: (page) => { calls.push(`page ${page}`); },
    setFit: (mode) => { calls.push(`fit ${mode}`); },
    setZoomFixed: (zoom) => { calls.push(`zoom ${zoom}`); },
  };
}

const at = (page: string | null, zoom: string | null): Opening => ({ path: "/a.pdf", page, zoom });

describe("A command line's place", () => {
  it("reads the four fits with a hyphen or a space, in any case", () => {
    expect(launchZoom("fit-width")).toEqual({ fit: "width" });
    expect(launchZoom("Fit Page")).toEqual({ fit: "page" });
    expect(launchZoom(" fit-height ")).toEqual({ fit: "height" });
    expect(launchZoom("fit  content")).toEqual({ fit: "content" });
  });

  it("reads a percentage by the palette's rule, and refuses one outside it", () => {
    expect(launchZoom("150")).toEqual({ zoom: 1.5 });
    expect(launchZoom("150%")).toEqual({ zoom: 1.5 });
    expect(launchZoom("5000")).toHaveProperty("problem");
    expect(launchZoom("width")).toHaveProperty("problem");
    expect(launchZoom("")).toHaveProperty("problem");
  });

  it("counts pages from 1 and refuses one past the end", () => {
    expect(launchPage("1", 3)).toEqual({ page: 0 });
    expect(launchPage("3", 3)).toEqual({ page: 2 });
    expect(launchPage("4", 3)).toEqual({ problem: 'tpdf was asked to open at --page "4", and this document has 3 pages' });
    expect(launchPage("0", 3)).toHaveProperty("problem");
    expect(launchPage("2.5", 3)).toEqual({ problem: 'tpdf was asked to open at --page "2.5", which is not a page number' });
    expect(launchPage("", 1)).toHaveProperty("problem");
    expect(launchPage("9", 1)).toEqual({ problem: 'tpdf was asked to open at --page "9", and this document has 1 page' });
  });

  it("sets the zoom before going to the page, which a zoom would move", () => {
    const viewer = recorder();
    expect(applyPlace(viewer, at("2", "fit-width"), 5)).toBeNull();
    expect(viewer.calls).toEqual(["fit width", "page 1"]);
    const fixed = recorder();
    expect(applyPlace(fixed, at("5", "200"), 5)).toBeNull();
    expect(fixed.calls).toEqual(["zoom 2", "page 4"]);
  });

  it("fits the content after the page, because it lands on the page in front", () => {
    const viewer = recorder();
    expect(applyPlace(viewer, at("3", "fit-content"), 5)).toBeNull();
    expect(viewer.calls).toEqual(["page 2", "fit content"]);
  });

  it("shows what it can and says what it cannot", () => {
    const viewer = recorder();
    const said = applyPlace(viewer, at("9", "fit-page"), 5);
    expect(viewer.calls).toEqual(["fit page"]);
    expect(said).toBe('tpdf was asked to open at --page "9", and this document has 5 pages.');
    const both = recorder();
    const twice = applyPlace(both, at("x", "huge"), 5);
    expect(both.calls).toEqual([]);
    expect(twice).toMatch(/^tpdf was asked to open at --page "x", which is not a page number\. tpdf was asked to open at --zoom "huge"/);
  });

  it("does nothing for an opening that asks for nothing", () => {
    const viewer = recorder();
    expect(hasPlace(at(null, null))).toBe(false);
    expect(hasPlace(at("", null))).toBe(true);
    expect(hasPlace(at(null, "150"))).toBe(true);
    expect(applyPlace(viewer, at(null, null), 5)).toBeNull();
    expect(viewer.calls).toEqual([]);
  });
});
