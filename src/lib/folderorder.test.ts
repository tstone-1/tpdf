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
  it("finds the next and the previous PDF", async () => {
    expect(await neighbourIn(FOLDER, "/d/Page 2.pdf", 1, async () => false)).toBe("/d/page 10.pdf");
    expect(await neighbourIn(FOLDER, "/d/Page 2.pdf", -1, async () => false)).toBe("/d/page 1.pdf");
  });

  it("skips aliases in either direction and stops when only aliases remain", async () => {
    const paths = ["a", "b", "c", "d"];
    const alias = async (p: string) => p === "b" || p === "c";
    expect(await neighbourIn(paths, "a", 1, alias)).toBe("d");
    expect(await neighbourIn(paths, "d", -1, alias)).toBe("a");
    expect(await neighbourIn(paths.slice(0, 3), "a", 1, alias)).toBeNull();
  });

  it("locates a differently spelled current filename by filesystem identity", async () => {
    const same = async (p: string) => p === "/d/Page 2.pdf" || p === "/d/appendix.pdf";
    expect(await neighbourIn(FOLDER, "/d/page 2.pdf", 1, same)).toBe("/d/page 10.pdf");
    expect(await neighbourIn(FOLDER, "/d/page 2.pdf", -1, same)).toBe("/d/page 1.pdf");
  });

  it("stops at either end rather than going round", async () => {
    expect(await neighbourIn(FOLDER, "/d/page 10.pdf", 1, async () => false)).toBeNull();
    expect(await neighbourIn(FOLDER, "/d/appendix.pdf", -1, async () => false)).toBeNull();
  });

  it("finds nothing for a document the folder no longer has", async () => {
    expect(await neighbourIn(FOLDER, "/d/gone.pdf", 1, async () => false)).toBeNull();
  });
});
