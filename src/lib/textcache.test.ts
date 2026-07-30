/**
 * The eviction policy of {@link TextCache}.
 *
 * In its own file rather than in `text.test.ts` because it needs the IPC mocked,
 * and `vi.mock` is hoisted to the whole file: the geometry tests next door are
 * pure and should stay reachable without a fake backend standing behind them.
 *
 * What is asserted is the *policy*, not the numbers. A test that pinned
 * `TEXT_CACHE_CHARS` to 400,000 would go red for a deliberate retune and green
 * for a cache that never evicts, which is the wrong way round, so the bound and
 * the floor are read from the module and the fixtures are sized against them.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  TEXT_CACHE_CHARS,
  TEXT_CACHE_FLOOR,
  TextCache,
  type PageText,
} from "./text";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);

/** A page of `chars` characters, each with a box, laid out left to right. */
function page(chars: number): PageText {
  const codes: number[] = [];
  const boxes: number[] = [];
  for (let i = 0; i < chars; i++) {
    codes.push(97 + (i % 26));
    boxes.push(i * 10, 0, i * 10 + 10, 12);
  }
  return {
    codes,
    boxes,
    width_pt: 600,
    height_pt: 800,
    quarter_turns: 0,
    extract_ms: 0,
  };
}

/** Characters per page for a fixture that needs many pages to exceed the bound. */
const SMALL = 1000;
/** Pages of {@link SMALL} needed to go one over the bound. */
const OVER = Math.floor(TEXT_CACHE_CHARS / SMALL) + 1;

beforeEach(() => {
  core.invoke.mockReset();
  core.invoke.mockImplementation(() => Promise.resolve(page(SMALL)));
});

describe("TextCache eviction", () => {
  it("holds everything that fits under the bound", async () => {
    const cache = new TextCache(1);
    const pages = Math.floor(TEXT_CACHE_CHARS / SMALL) - 1;
    for (let p = 0; p < pages; p++) await cache.load(p);
    expect(cache.cachedPages).toBe(pages);
    expect(cache.cachedChars).toBe(pages * SMALL);
    // The control for every eviction assertion below: without it, a cache that
    // dropped each page as it arrived would satisfy all of them.
    expect(cache.peek(0)).not.toBeNull();
  });

  it("drops pages once the bound is passed", async () => {
    const cache = new TextCache(1);
    for (let p = 0; p < OVER; p++) await cache.load(p);
    expect(cache.cachedChars).toBeLessThanOrEqual(TEXT_CACHE_CHARS);
    expect(cache.cachedPages).toBeLessThan(OVER);
  });

  it("drops the least recently used page, not the oldest arrival", async () => {
    const cache = new TextCache(1);
    for (let p = 0; p < OVER; p++) {
      // Page 0 is read on every step, so it is the oldest *arrival* throughout
      // and never the least recently *used*. A cache evicting by arrival order
      // drops it first; one evicting by use drops page 1.
      if (p > 0) cache.peek(0);
      await cache.load(p);
    }
    expect(cache.peek(0)).not.toBeNull();
    expect(cache.peek(1)).toBeNull();
  });

  it("counts a load of a page it already has as a use", async () => {
    // The same claim for the other entry point. `load` returns from the cache
    // without touching the backend, and a hit that skipped the bookkeeping would
    // leave a page a reader keeps returning to looking untouched.
    const cache = new TextCache(1);
    for (let p = 0; p < OVER; p++) {
      if (p > 0) await cache.load(0);
      await cache.load(p);
    }
    expect(cache.peek(0)).not.toBeNull();
    expect(cache.peek(1)).toBeNull();
  });

  it("never evicts the page that has just arrived", async () => {
    const cache = new TextCache(1);
    for (let p = 0; p < OVER; p++) await cache.load(p);
    // The scan starts at the old end, so the newest page survives however far
    // over the bound its arrival put the cache. A cache that dropped it would
    // re-fetch the page it is about to draw, every time.
    expect(cache.peek(OVER - 1)).not.toBeNull();
  });

  it("keeps a floor of pages larger than the bound itself", async () => {
    const cache = new TextCache(1);
    core.invoke.mockImplementation(() => Promise.resolve(page(TEXT_CACHE_CHARS)));
    for (let p = 0; p < TEXT_CACHE_FLOOR + 2; p++) await cache.load(p);
    // Every page on its own fills the budget, so a bound without a floor would
    // hold exactly one and re-fetch both halves of a two-page viewport forever.
    expect(cache.cachedPages).toBe(TEXT_CACHE_FLOOR);
    expect(cache.cachedChars).toBeGreaterThan(TEXT_CACHE_CHARS);
  });

  it("asks the backend again for a page it has dropped", async () => {
    // The other half of what eviction means, and the reason re-fetching is an
    // acceptable price: a dropped page must come back, not come back empty.
    const cache = new TextCache(1);
    for (let p = 0; p < OVER; p++) await cache.load(p);
    expect(cache.peek(0)).toBeNull();
    const calls = core.invoke.mock.calls.length;
    const again = await cache.load(0);
    expect(again?.codes.length).toBe(SMALL);
    expect(core.invoke.mock.calls.length).toBe(calls + 1);
  });

  it("drops the turned view with the page it was turned from", async () => {
    const cache = new TextCache(1);
    cache.setTurns(1);
    for (let p = 0; p < OVER; p++) {
      await cache.load(p);
      // `view` memoises on first use, so the turned copy only exists once
      // somebody has asked for the page. Reading it is what puts it in the map.
      cache.peek(p);
    }
    // Asserted on the *count*, not through `peek`. A stale turned view is
    // unreachable behaviourally --- `view` consults `pages` first and never
    // reaches `turned` for a page that has gone --- so an assertion that an
    // evicted page reads as null passes whether or not the view was dropped,
    // and the leak it is meant to catch stays invisible.
    expect(cache.retainedViews).toBe(cache.cachedPages);
    expect(cache.peek(0)).toBeNull();
    // And the pages that survived are still turned, so eviction has not quietly
    // reset the rotation for them.
    expect(cache.peek(OVER - 1)?.quarter_turns).toBe(1);
  });
});
