/**
 * A page whose search failed, and whether every match may then be marked.
 *
 * Its own file for `searchmapping.test.ts`'s reason: `vi.mock` is hoisted to the
 * whole file, and `search.test.ts` stays free of a mocked backend.
 *
 * The defect this pins (review of 2026-10-15, B1): a page whose `search_page`
 * threw was skipped and still counted as scanned, and "Redact every search
 * result" was offered while the scan ran. Either way the hits found so far were
 * marked as though they were all of them, and the occurrences on the other pages
 * stayed in the file under a redaction that read as verified.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import app from "../App.svelte?raw";
import { functionIn, missingFrom } from "./sourcetext";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);

const { Search, completeForMarking, MAX_MATCHES_TO_MARK } = await import("./search");

/** One page's answer with one hit, as `search::PageMatches` serialises. */
function hit(page: number) {
  return { page, matches: [{ page, start: 0, end: 3, rects: [] }], chars: 10 };
}

/**
 * A backend that answers every page with one hit, except `broken`, whose every
 * request (alone or in a run) throws.
 */
function backend(broken: number | null) {
  core.invoke.mockImplementation(
    (command: string, args: { page?: number; pages?: number[] }) => {
      if (command !== "search_page") return Promise.resolve([]);
      const asked = args.pages ?? [args.page ?? 0];
      if (broken !== null && asked.includes(broken)) {
        return Promise.reject(new Error("the worker did not answer"));
      }
      const [first, ...rest] = asked.map(hit);
      return Promise.resolve({ ...first, more: rest.length > 0 ? rest : undefined });
    },
  );
}

beforeEach(() => {
  core.invoke.mockReset();
});

describe("Search.unread", () => {
  it("counts a page whose search failed, and still finds the others' hits", async () => {
    // Mutation: the `this.unread++` in `visit`'s catch removed. The scan then
    // reads 5 of 5 searched with 4 hits, which is what it said before.
    backend(2);
    const search = new Search(1, 5, () => {});
    await search.run("cat", 0);
    expect(search.unread).toBe(1);
    // The page is still counted as asked about, which is why the count of
    // failures is needed at all, and the other four pages' hits are kept.
    expect(search.scanned).toBe(5);
    expect(search.matches.map((m) => m.page).sort()).toEqual([0, 1, 3, 4]);
  });

  it("is zero when every page answered, and a new scan starts from zero", async () => {
    // The control for the test above, and the reset: a count carried from one
    // scan to the next would refuse marking for a search that read every page.
    backend(2);
    const search = new Search(1, 5, () => {});
    await search.run("cat", 0);
    expect(search.unread).toBe(1);
    backend(null);
    await search.run("cat", 0);
    expect(search.unread).toBe(0);
    expect(search.matches).toHaveLength(5);
    search.clear();
    expect(search.unread).toBe(0);
  });
});

describe("completeForMarking", () => {
  it("refuses while the scan is still running", () => {
    // Mutation: the `running` test removed. Marking then took the hits found so
    // far, which is what the window did until 2026-10-15.
    const said = completeForMarking({ running: true, unread: 0, unsearchablePages: 0, count: 3 }) ?? "";
    expect(said).toContain("still running");
    expect(said).toContain("Nothing was marked");
  });

  it("refuses when a page could not be searched, and says how many", () => {
    // Mutation: the `unread` test removed.
    expect(completeForMarking({ running: false, unread: 1, unsearchablePages: 0, count: 3 })).toContain(
      "1 page could not be searched",
    );
    expect(completeForMarking({ running: false, unread: 4, unsearchablePages: 0, count: 3 })).toContain(
      "4 pages could not be searched",
    );
  });

  it("lets a finished scan that read every page through, up to the count", () => {
    // The control: without it a function refusing everything passes the two
    // tests above.
    expect(completeForMarking({ running: false, unread: 0, unsearchablePages: 0, count: 3 })).toBeNull();
    expect(
      completeForMarking({ running: false, unread: 0, unsearchablePages: 0, count: MAX_MATCHES_TO_MARK + 1 }),
    ).not.toBeNull();
  });

  it("refuses until mapping settles, and then until an unmapped page has readable text", async () => {
    let answer!: (pages: { composite: number; guessing: number; truncated: boolean }[]) => void;
    let asked!: () => void;
    const pending = new Promise<void>(resolve => { asked = resolve; });
    core.invoke.mockImplementation((command: string) => {
      if (command === "search_page") return Promise.resolve({
        ...hit(0), more: [{ page: 1, matches: [], chars: 8 }],
      });
      if (command === "document_mapping") {
        asked();
        return new Promise(resolve => { answer = resolve; });
      }
      throw new Error(command);
    });
    let recovered = false;
    const search = new Search(1, 2, () => {}, undefined, undefined, (_doc, page) => recovered && page === 1);
    const scan = search.run("cat", 0);
    await pending;
    expect(search.matches).toHaveLength(1);
    expect(search.markingRefusal).toContain("still running");
    answer([0, 1].map(guessing => ({ composite: 1, guessing, truncated: false })));
    await scan;
    expect(search.markingRefusal).toContain("1 page has no readable character mapping");
    recovered = true;
    expect(search.markingRefusal).toBeNull();
  });

  it.each(["failed", "short", "truncated"] as const)(
    "refuses bulk marking when a source mapping is %s", async outcome => {
      const mapped = { guessing: 0, truncated: false, composite: 0 };
      core.invoke.mockImplementation(async (command: string) => {
        if (command === "search_page") return { ...hit(0), more: [hit(1)] };
        if (outcome === "failed") throw new Error("no assessment");
        return outcome === "short" ? [mapped] : [mapped, { ...mapped, truncated: true }];
      });
      const search = new Search(1, 2, () => {});
      await search.run("cat", 0);
      expect(search.matches).toHaveLength(2);
      expect(search.markingRefusal).toContain("could not be checked");
      if (outcome !== "failed") {
        await search.run("cat", 0, undefined, [{ page: 0, from: 0, to: Infinity }]);
        expect(search.markingRefusal).toBeNull();
      }
    },
  );

  it("keeps a cancelled scan incomplete when its outstanding mapping reply arrives", async () => {
    let answer!: () => void;
    let asked!: () => void;
    const pending = new Promise<void>(resolve => { asked = resolve; });
    core.invoke.mockImplementation((command: string) => {
      if (command === "search_page") return Promise.resolve(hit(0));
      asked();
      return new Promise(resolve => {
        answer = () => resolve([{ guessing: 0, truncated: false, composite: 0 }]);
      });
    });
    const search = new Search(1, 1, () => {});
    const scan = search.run("cat", 0);
    await pending;
    search.cancel();
    answer();
    await scan;
    expect(search.matches).toHaveLength(1);
    expect(search.markingRefusal).toContain("search did not finish");
    await search.run("cat", 0);
    expect(search.markingRefusal).toBeNull();
  });

  it("refuses a scan whose failed page is real, end to end", async () => {
    // The two halves joined: the count a real scan produces is the count the
    // decision refuses on.
    backend(3);
    const search = new Search(1, 6, () => {});
    await search.run("cat", 0);
    expect(search.markingRefusal).toContain("1 page could not be searched");
  });
});

/**
 * What is left in `App.svelte`: that `redactMatches` hands the decision the
 * scan's parts, and that the panel is told the count. Read as text with the
 * comments taken out, which sees that a line is there and not whether it runs
 * (`sourcetext.ts`); the decision itself is tested above.
 */
describe("the window's wiring", () => {
  it("asks completeForMarking before marking, with the running scan and its unread pages", () => {
    const marking = functionIn(app, "async function redactMatches(): Promise<void> {");
    expect(missingFrom(marking, [
      "const refusal = searched.searchMarkRefusal;",
      "const matches = searched.searchMatches;",
      "const found = await searched.matchQuadsByPage(matches);",
      "model.trackSearchMarks(applyEdit(",
      "return owner.redactText(found);",
    ])).toEqual([]);
    // Asked before the quads are read: a refusal after them would mark nothing
    // either, but would cost the round trips first.
    expect(marking.indexOf("searched.searchMarkRefusal")).toBeLessThan(
      marking.indexOf("matchQuadsByPage(matches)"),
    );
    expect(marking.indexOf("const matches = searched.searchMatches;")).toBeLessThan(marking.indexOf("applyEdit("));
    expect(missingFrom(app, ["next.search.unread,"])).toEqual([]);
  });
});
