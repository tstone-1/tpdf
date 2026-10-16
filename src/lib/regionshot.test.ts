import { describe, expect, it } from "vitest";

import { REGION_FLOOR, REGION_PIXELS, regionName, regionRequest, regionSaid } from "./regionshot";

const source = { doc: 4, page: 2, turns: 1, invert: true, crop: [10, 20, 300, 400] as const };

describe("regionRequest", () => {
  it("renders the region at the screen's scale, covering it in whole pixels", () => {
    const request = regionRequest(source, { left: 10.2, top: 20.7, right: 110.5, bottom: 70.1 }, 3);
    expect(request).toEqual({ ...source, scale: 3, x: 30, y: 62, width: 302, height: 149, format: "png" });
  });

  it("never goes under the floor, so a region from a page zoomed out stays readable", () => {
    const request = regionRequest(source, { left: 0, top: 0, right: 100, bottom: 50 }, 0.5);
    expect(request.scale).toBe(REGION_FLOOR);
    expect([request.width, request.height]).toEqual([200, 100]);
  });

  it("rounds the scale down to the thousandths the wire carries, and works the pixels out from that", () => {
    const request = regionRequest(source, { left: 0, top: 0, right: 100, bottom: 100 }, 2.34567);
    expect(request.scale).toBe(2.345);
    expect(request.width).toBe(235);
  });

  it("shrinks a region past the tile mapping to the largest scale that fits", () => {
    // A whole A4 page at an 8x zoom on a Retina screen: each side fits a
    // tile's sixteen bits, and the area is thirty times the mapping.
    const quad = { left: 0, top: 0, right: 595, bottom: 842 };
    expect(Math.max(quad.right, quad.bottom) * 16).toBeLessThan(65_535);
    const request = regionRequest(source, quad, 16);
    expect(request.width * request.height).toBeLessThanOrEqual(REGION_PIXELS);
    // The largest: a thousandth more would not fit.
    const next = (Math.round(request.scale * 1000) + 1) / 1000;
    expect(Math.ceil(quad.right * next) * Math.ceil(quad.bottom * next)).toBeGreaterThan(REGION_PIXELS);
    expect(request.scale).toBeLessThan(16);
  });

  it("steps back up when the shrink lands under the largest scale that fits", () => {
    // Found by a search over random regions: shrunk by what its area is over,
    // this one lands at 5.710, three thousandths under the largest that fits.
    const quad = { left: 229.93, top: 300.22, right: 372.3, bottom: 1201.8 };
    const request = regionRequest(source, quad, 17.114);
    expect(request.scale).toBe(5.713);
    expect(request.width * request.height).toBeLessThanOrEqual(REGION_PIXELS);
  });

  it("keeps a thin strip under a tile's sixteen-bit side", () => {
    const request = regionRequest(source, { left: 0, top: 0, right: 14_400, bottom: 2 }, 16);
    expect(request.width).toBeLessThanOrEqual(65_535);
  });
});

describe("regionName and regionSaid", () => {
  it("names the file after the document and the page", () => {
    expect(regionName("Report.PDF", 3)).toBe("Report page 3.png");
    expect(regionName(".pdf", 1)).toBe("region page 1.png");
  });

  it("says the size it copied or saved", () => {
    expect(regionSaid({ width: 302, height: 149 })).toBe("Copied the region as a picture, 302 by 149 pixels.");
    expect(regionSaid({ width: 2, height: 1 }, "a.png")).toBe("Saved the region as a.png, 2 by 1 pixels.");
  });
});
