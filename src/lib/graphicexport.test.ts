/**
 * What the window decides for *Copy a graphic* and *Save a graphic...*
 * (`graphicexport.ts`). The export itself is `tests/cli/graphic.rs`.
 */

import { describe, expect, it } from "vitest";

import {
  clickedBox,
  clickRequest,
  formatFor,
  graphicChoices,
  graphicName,
  graphicRequest,
  graphicSaid,
  wrongEnding,
  type GraphicFound,
} from "./graphicexport";
import { regionRequest } from "./regionshot";

const DRAWING: GraphicFound = {
  objects: 1, original: null, no_svg: null, width: 2015, height: 363, extension: "", object: null, rect: null,
};
const JPEG: GraphicFound = { ...DRAWING, original: "jpg", no_svg: null, width: 157, height: 51 };
const TEXTUAL: GraphicFound = { ...DRAWING, objects: 2, no_svg: "it holds text, which the SVG cannot draw yet; save it as PNG" };

describe("a click", () => {
  it("is sent as a point-sized square centred where it landed", () => {
    const source = { doc: 1, page: 0 };
    const { request } = clickRequest(source, 100, 50, 2);
    const [x, y, width, height] = request.rect;
    expect(request.format).toBe("hit");
    // Mutation: the square not centred on the click -> red.
    expect((x + width / 2) / request.scale).toBeCloseTo(100, 0);
    expect((y + height / 2) / request.scale).toBeCloseTo(50, 0);
    expect(width / request.scale).toBeLessThan(3);
  });

  it("outlines the graphic in laid-out points, and nothing when the click found none", () => {
    const { request } = clickRequest({ doc: 1, page: 0 }, 100, 50, 2);
    const s = request.scale;
    const found = { ...DRAWING, object: 3, rect: [10 * s, 20 * s, 250 * s, 65 * s] as [number, number, number, number] };
    // Mutation: the rectangle not divided by the tile's scale -> red.
    expect(clickedBox(request, found)).toEqual({ left: 10, top: 20, right: 250, bottom: 65 });
    expect(clickedBox(request, DRAWING)).toBeNull();
    expect(clickedBox(request, { ...found, object: null })).toBeNull();
  });
});

describe("graphicRequest", () => {
  it("measures the region in the pixels of the tile a region copy would render", () => {
    const source = { doc: 4, page: 2, turns: 1, crop: [0, 0, 500, 700] as const };
    const quad = { left: 10.2, top: 20.7, right: 110.1, bottom: 60.3 };
    const tile = regionRequest(source, quad, 3);
    const { doc, request } = graphicRequest(source, quad, 3, "svg");
    // Mutation: x and y swapped, or the scale not the tile's -> red.
    expect(doc).toBe(4);
    expect(request).toEqual({
      page: 2, turns: 1, crop: [0, 0, 500, 700], scale: tile.scale,
      rect: [tile.x, tile.y, tile.width, tile.height], format: "svg",
    });
  });

  it("sends no crop for a page shown with the file's own", () => {
    const { request } = graphicRequest({ doc: 0, page: 0 }, { left: 0, top: 0, right: 5, bottom: 5 }, 1, "png");
    expect("crop" in request).toBe(false);
    expect(request.turns).toBe(0);
  });
});

describe("graphicChoices", () => {
  it("offers SVG first for a drawing, then PNG", () => {
    expect(graphicChoices(DRAWING).map((c) => c.format)).toEqual(["svg", "png"]);
  });

  it("offers the stored JPEG beside the PNG", () => {
    expect(graphicChoices(JPEG).map((c) => c.format)).toEqual(["svg", "original", "png"]);
    expect(graphicChoices({ ...JPEG, original: "png" }).map((c) => c.format)).toEqual(["svg", "png"]);
  });

  it("leaves SVG out when the backend says it cannot be made", () => {
    expect(graphicChoices(TEXTUAL).map((c) => c.format)).toEqual(["png"]);
  });
});

describe("formatFor", () => {
  it("reads the format from the chosen name's ending, in any case", () => {
    const choices = graphicChoices(JPEG);
    expect(formatFor("C:\\out\\logo.SVG", choices)).toBe("svg");
    expect(formatFor("/out/logo.jpeg", choices)).toBe("original");
    expect(formatFor("/out/logo.jpg", choices)).toBe("original");
    expect(formatFor("/out/logo.png", choices)).toBe("png");
  });

  it("answers null for an ending not offered, or no ending", () => {
    expect(formatFor("/out/logo.svg", graphicChoices(TEXTUAL))).toBeNull();
    expect(formatFor("/out/logo.jpg", graphicChoices(DRAWING))).toBeNull();
    expect(formatFor("/out.d/logo", graphicChoices(DRAWING))).toBeNull();
  });

  it("names the endings it would take when it refuses", () => {
    expect(wrongEnding("logo", graphicChoices(JPEG))).toBe(
      "logo was not saved: choose a name ending in .svg, .jpg, .jpeg, .png.",
    );
  });
});

describe("graphicName and graphicSaid", () => {
  it("suggests the document, the page and the first choice's ending", () => {
    expect(graphicName("DoC-PPWR-English-v2.pdf", 1, graphicChoices(DRAWING)[0]!)).toBe(
      "DoC-PPWR-English-v2 page 1 graphic.svg",
    );
    expect(graphicName(".pdf", 3, graphicChoices(TEXTUAL)[0]!)).toBe("graphic page 3 graphic.png");
  });

  it("takes the ending off whatever its case, and only the ending", () => {
    // Windows and scanners write `.PDF`. Mutation: drop the `i` flag -> red.
    const svg = graphicChoices(DRAWING)[0]!;
    expect(graphicName("SCAN0001.PDF", 2, svg)).toBe("SCAN0001 page 2 graphic.svg");
    expect(graphicName("Report.Pdf", 2, svg)).toBe("Report page 2 graphic.svg");
    // Control: a name that only contains the letters keeps them.
    expect(graphicName("pdf-notes.pdfx", 2, svg)).toBe("pdf-notes.pdfx page 2 graphic.svg");
  });

  it("says what was made, by what was made", () => {
    expect(graphicSaid(DRAWING)).toBe("Copied the graphic, 2015 by 363 pixels on a transparent background.");
    expect(graphicSaid({ ...DRAWING, extension: "svg" }, "logo.svg")).toMatch(/logo\.svg, a drawing that stays sharp/);
    expect(graphicSaid({ ...JPEG, extension: "jpg" }, "logo.jpg")).toMatch(/picture as logo\.jpg, as the document stores it/);
    expect(graphicSaid({ ...DRAWING, extension: "png" }, "logo.png")).toMatch(/logo\.png, 2015 by 363 pixels/);
  });
});
