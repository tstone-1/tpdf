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
    const said = completeForMarking({ running: true, unread: 0, count: 3 }) ?? "";
    expect(said).toContain("still running");
    expect(said).toContain("Nothing was marked");
  });

  it("refuses when a page could not be searched, and says how many", () => {
    // Mutation: the `unread` test removed.
    expect(completeForMarking({ running: false, unread: 1, count: 3 })).toContain(
      "1 page could not be searched",
    );
    expect(completeForMarking({ running: false, unread: 4, count: 3 })).toContain(
      "4 pages could not be searched",
    );
  });

  it("lets a finished scan that read every page through, up to the count", () => {
    // The control: without it a function refusing everything passes the two
    // tests above.
    expect(completeForMarking({ running: false, unread: 0, count: 3 })).toBeNull();
    expect(
      completeForMarking({ running: false, unread: 0, count: MAX_MATCHES_TO_MARK + 1 }),
    ).not.toBeNull();
  });

  it("refuses a scan whose failed page is real, end to end", async () => {
    // The two halves joined: the count a real scan produces is the count the
    // decision refuses on.
    backend(3);
    const search = new Search(1, 6, () => {});
    await search.run("cat", 0);
    expect(
      completeForMarking({
        running: search.running,
        unread: search.unread,
        count: search.matches.length,
      }),
    ).toContain("1 page could not be searched");
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
      "const refusal = completeForMarking({",
      "running: searched.searching,",
      "unread: searched.searchUnread,",
      "count: searched.matchCount,",
    ])).toEqual([]);
    // Asked before the quads are read: a refusal after them would mark nothing
    // either, but would cost the round trips first.
    expect(marking.indexOf("completeForMarking(")).toBeLessThan(
      marking.indexOf("matchQuadsByPage()"),
    );
    expect(missingFrom(app, ["next.search.unread,"])).toEqual([]);
  });
});
