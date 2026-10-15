/**
 * The order of a folder, and the next and previous PDF in it.
 */

import { describe, expect, it } from "vitest";

import { folderOrder, neighbourIn } from "./folderorder";

const FOLDER = ["/d/page 10.pdf", "/d/Page 2.pdf", "/d/appendix.pdf", "/d/page 1.pdf"];

describe("folderOrder", () => {
  it("sorts by name as the file manager does: numbers by value, case ignored", () => {
    expect(folderOrder(FOLDER)).toEqual(["/d/appendix.pdf", "/d/page 1.pdf", "/d/Page 2.pdf", "/d/page 10.pdf"]);
  });
});

describe("neighbourIn", () => {
  it("finds the next and the previous PDF", () => {
    expect(neighbourIn(FOLDER, "/d/Page 2.pdf", 1)).toBe("/d/page 10.pdf");
    expect(neighbourIn(FOLDER, "/d/Page 2.pdf", -1)).toBe("/d/page 1.pdf");
  });

  it("stops at either end rather than going round", () => {
    expect(neighbourIn(FOLDER, "/d/page 10.pdf", 1)).toBeNull();
    expect(neighbourIn(FOLDER, "/d/appendix.pdf", -1)).toBeNull();
  });

  it("finds nothing for a document the folder no longer has", () => {
    expect(neighbourIn(FOLDER, "/d/gone.pdf", 1)).toBeNull();
  });
});
