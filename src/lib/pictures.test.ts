import { describe, expect, it } from "vitest";

import { EXTENSIONS, afterPictures, suggestedName } from "./pictures";

describe("suggestedName", () => {
  it("takes the first picture's name and makes it a PDF", () => {
    expect(suggestedName("/photos/Holiday.JPG")).toBe("Holiday.pdf");
    expect(suggestedName("/photos/scan 01.jpeg")).toBe("scan 01.pdf");
    expect(suggestedName("C:\\scans\\plan.png")).toBe("plan.pdf");
  });

  it("keeps a name whose ending is not a picture's", () => {
    expect(suggestedName("/photos/notes.v2")).toBe("notes.v2.pdf");
    // Only the ending: a name that merely contains one is left whole.
    expect(suggestedName("/photos/a.png.bak")).toBe("a.png.bak.pdf");
  });

  it("covers every extension the panel offers", () => {
    for (const extension of EXTENSIONS) {
      expect(suggestedName(`/p/x.${extension}`)).toBe("x.pdf");
    }
  });
});

describe("afterPictures", () => {
  it("names the document and counts its pages", () => {
    expect(afterPictures({ pages: 1 }, "/out/one.pdf")).toBe(
      "Saved one.pdf: 1 page, one for each picture.",
    );
    expect(afterPictures({ pages: 12 }, "/out/album.pdf")).toBe(
      "Saved album.pdf: 12 pages, one for each picture.",
    );
  });
});
