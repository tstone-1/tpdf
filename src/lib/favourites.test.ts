/**
 * The favourites list's rules, and its palette entries.
 */

import { describe, expect, it } from "vitest";

import { favouriteCommands, Favourites, FAVOURITE_PREFIX } from "./favourites";

describe("Favourites", () => {
  it("names a page after its number when the reader gives no name", () => {
    const list = new Favourites();
    expect(list.add("/a.pdf", 4, "  ").name).toBe("Page 5");
    expect(list.add("/a.pdf", 6, "Results").name).toBe("Results");
  });

  it("renames a page marked twice rather than listing it twice", () => {
    const list = new Favourites();
    list.add("/a.pdf", 4, "First");
    list.add("/a.pdf", 4, "Second");
    expect(list.all).toEqual([{ path: "/a.pdf", page: 4, name: "Second" }]);
  });

  it("keeps the last available page and permits renaming it when the list is full", () => {
    const list = new Favourites();
    for (let page = 0; page < 500; page++) list.add("/a.pdf", page, "");
    expect(() => list.add("/b.pdf", 0, "Extra")).toThrow("All 500 favorites are in use");
    expect(list.all).toHaveLength(500);
    expect(list.has("/b.pdf", 0)).toBe(false);
    expect(list.add("/a.pdf", 499, "Last").name).toBe("Last");
    list.remove("/a.pdf", 499);
    expect(list.add("/b.pdf", 0, "New").name).toBe("New");
  });

  it("measures names in Unicode code points, as the stored session does", () => {
    const list = new Favourites();
    const name = "\u{1f600}".repeat(200);
    expect(list.add("/a.pdf", 1, name).name).toBe(name);
    expect(() => list.add("/a.pdf", 1, name + "x")).toThrow("at most 200 characters");
    expect(list.all[0]?.name).toBe(name);
  });

  it("removes a page, and says whether it was marked", () => {
    const list = new Favourites();
    list.add("/a.pdf", 4, "");
    list.add("/b.pdf", 4, "");
    expect(list.remove("/a.pdf", 4)).toBe(true);
    expect(list.remove("/a.pdf", 4)).toBe(false);
    expect(list.has("/b.pdf", 4)).toBe(true);
  });

  it("steps to the nearest favourite page of the same document, either way", () => {
    const list = new Favourites();
    for (const page of [9, 2, 5]) list.add("/a.pdf", page, "");
    list.add("/b.pdf", 3, "");
    // 5 and not /b.pdf's 3: another document's favourite is not a step here.
    expect(list.step("/a.pdf", 2, 1)).toBe(5);
    expect(list.step("/a.pdf", 6, -1)).toBe(5);
    expect(list.step("/a.pdf", 9, 1)).toBeNull();
    expect(list.step("/a.pdf", 2, -1)).toBeNull();
    expect(list.step("/a.pdf", 0, 1)).toBe(2);
  });
});

describe("favouriteCommands", () => {
  it("offers one entry per favourite, which goes to it", () => {
    const went: string[] = [];
    const commands = favouriteCommands(
      [{ path: "/docs/report.pdf", page: 6, name: "Results" }],
      (favourite) => went.push(`${favourite.path}:${favourite.page}`),
    );
    expect(commands.map((command) => [command.id, command.title])).toEqual([
      [`${FAVOURITE_PREFIX}0`, "Go to favorite: Results (report.pdf, page 7)"],
    ]);
    void commands[0]!.run?.();
    expect(went).toEqual(["/docs/report.pdf:6"]);
  });
});
