import { beforeEach, expect, it, vi } from "vitest";
import { SearchRecovery } from "./searchrecovery";
import { TextCache, type PageText } from "./text";
import { filePage } from "./pages";
import { PLAIN_SEARCH } from "./search";
import { call } from "./ipc";

vi.mock("./ipc", () => ({ call: vi.fn() }));
const recovered: PageText = { codes: [70, 108, 117, 111, 114, 105, 110, 101],
  boxes: Array.from({ length: 8 }, () => [10, 20, 70, 30]).flat(),
  width_pt: 200, height_pt: 300, quarter_turns: 0, extract_ms: 0 };
const args = { doc: 3, page: filePage(1), query: "fluorine", options: PLAIN_SEARCH };
beforeEach(() => vi.mocked(call).mockReset());

it("replaces the garbled source for search and cached selection, without combining readings", async () => {
  const recovery = new SearchRecovery();
  const cache = new TextCache(3, recovery.text);
  vi.mocked(call).mockResolvedValue({ ...recovered, codes: [0xf046] } as never);
  expect((await cache.load(filePage(1)))?.codes).toEqual([0xf046]);
  recovery.put(3, 1, recovered);
  cache.invalidatePage(filePage(1));
  expect(await cache.load(filePage(1))).toEqual(recovered);
  await recovery.search(args);
  expect(call).toHaveBeenLastCalledWith("search_text", { page: 1, codes: recovered.codes,
    query: "fluorine", options: PLAIN_SEARCH, carry: undefined });
  recovery.clear();
  cache.invalidatePage(filePage(1));
  expect((await cache.load(filePage(1)))?.codes).toEqual([0xf046]);
  await recovery.search(args);
  expect(call).toHaveBeenLastCalledWith("search_page", args);
});

it("keeps unrelated documents and pages native, and stops mixed runs at the first page", async () => {
  const recovery = new SearchRecovery();
  recovery.put(3, 1, recovered);
  await recovery.search({ ...args, doc: 4 });
  expect(call).toHaveBeenLastCalledWith("search_page", { ...args, doc: 4 });
  await recovery.search({ ...args, page: filePage(0), pages: [filePage(0), filePage(1), filePage(2)] });
  expect(call).toHaveBeenLastCalledWith("search_page", { ...args, page: 0, pages: [0] });
  const carry = { marker: "previous page" } as never;
  await recovery.search({ ...args, pages: [filePage(1), filePage(2)], carry });
  expect(call).toHaveBeenLastCalledWith("search_text", expect.objectContaining({ page: 1, carry }));
});

it("bounds retained text instead of silently evicting an active search's source", () => {
  const recovery = new SearchRecovery();
  recovery.put(3, 0, { ...recovered, codes: Array(1_000_000).fill(65) });
  expect(() => recovery.put(3, 1, recovered)).toThrow("memory is full");
  recovery.put(3, 0, recovered);
  expect(recovery.size).toBe(1);
  recovery.put(3, 1, recovered);
  expect(recovery.size).toBe(2);
});
