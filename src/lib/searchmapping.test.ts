/**
 * Tests for the third state: a page whose text is present and meaningless.
 *
 * `search.test.ts` next door is pure and stays that way --- `vi.mock` is hoisted
 * to the whole file, so mocking the backend there would put a mock under tests
 * that are about option comparison and nothing else. `textcache.test.ts` split
 * for the same reason and says so.
 *
 * What is pinned here is the distinction that costs the most to get wrong.
 * `encoding.rs` reports three outcomes per page and the frontend must collapse
 * them to two:
 *
 *   guessing > 0   the document declares no character mapping   -> tell the reader
 *   truncated      nobody could tell                            -> say nothing
 *   neither        the document states what its glyphs mean     -> say nothing
 *
 * Folding `truncated` in with `guessing` is the tempting mistake --- both are
 * "not known to be fine" --- and it puts a warning on every encrypted document,
 * which `lopdf` cannot paginate at all. That is a false alarm on a file the
 * reader can search perfectly well, and it was caught here by mutation rather
 * than by reasoning: the whole-suite pass survived that change until this file
 * existed.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);

const { Search } = await import("./search");

/** One page's mapping verdict, as `encoding::PageMapping` serialises. */
function mapping(guessing: number, truncated = false) {
  return { composite: 1, guessing, truncated };
}

/** A reply for `search_page` that finds nothing on a page that has text. */
function noHits(page: number) {
  return { page, matches: [], chars: 40 };
}

/**
 * Runs a one-page scan that finds nothing, against the given mapping reply.
 *
 * The scan has to *complete* and find *nothing*, because that is the only path
 * that asks for the mapping at all --- a reader who finds hits never pays for
 * the parse.
 */
async function scanFindingNothing(pages: unknown[]): Promise<number> {
  core.invoke.mockImplementation((command: string, args: { page?: number }) => {
    if (command === "search_page") return Promise.resolve(noHits(args.page ?? 0));
    if (command === "document_mapping") return Promise.resolve(pages);
    throw new Error(`unexpected command ${command}`);
  });
  const searcher = new Search(1, pages.length, () => {});
  await searcher.run("cat", 0);
  return searcher.unsearchablePages;
}

beforeEach(() => {
  core.invoke.mockReset();
});

describe("Search.unsearchablePages", () => {
  it("counts a page the document declares no mapping for", () => {
    // The case the whole feature exists for.
    return expect(scanFindingNothing([mapping(1)])).resolves.toBe(1);
  });

  it("does not count a page nobody could judge", async () => {
    // Unknown is not unreadable. `lopdf` reports zero pages for an encrypted
    // document that PDFium paginates normally, so every page of it comes back
    // truncated -- and warning there would be a false alarm on a file the reader
    // can search perfectly well.
    await expect(scanFindingNothing([mapping(0, true), mapping(0, true)])).resolves.toBe(0);
  });

  it("counts only the pages that are actually unreadable", async () => {
    // The mixed document, which is the realistic one: a scanned insert in an
    // otherwise ordinary file. Reporting the whole document would overstate it.
    await expect(
      scanFindingNothing([mapping(0), mapping(1), mapping(0, true), mapping(2)]),
    ).resolves.toBe(2);
  });

  it("says nothing about an ordinary document", async () => {
    // The control, and the one that matters most: across 36 fixtures and ~1,700
    // pages exactly one page should ever trip this, so a rule that fired on a
    // normal document would be worse than the defect it fixes.
    await expect(scanFindingNothing([mapping(0), mapping(0)])).resolves.toBe(0);
  });

  it("asks the backend once, however many times it is prompted", async () => {
    core.invoke.mockImplementation((command: string, args: { page?: number }) => {
      if (command === "search_page") return Promise.resolve(noHits(args.page ?? 0));
      if (command === "document_mapping") return Promise.resolve([mapping(1)]);
      throw new Error(`unexpected command ${command}`);
    });
    const searcher = new Search(1, 1, () => {});
    await searcher.run("cat", 0);
    searcher.ensureMapping();
    searcher.ensureMapping();
    await searcher.run("dog", 0);
    // Flush the fetch the second scan may have started.
    await Promise.resolve();

    const asks = core.invoke.mock.calls.filter(
      (call: unknown[]) => call[0] === "document_mapping",
    );
    expect(asks).toHaveLength(1);
  });

  it("reports nothing rather than something false when the backend fails", async () => {
    // A refusal is not evidence of a problem. The reader is told nothing, which
    // is the status quo, rather than told a document is broken because a parse
    // did not finish.
    core.invoke.mockImplementation((command: string, args: { page?: number }) => {
      if (command === "search_page") return Promise.resolve(noHits(args.page ?? 0));
      return Promise.reject(new Error("worker died"));
    });
    const searcher = new Search(1, 1, () => {});
    await searcher.run("cat", 0);
    expect(searcher.unsearchablePages).toBe(0);
  });
});
