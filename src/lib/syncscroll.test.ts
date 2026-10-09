import { describe, expect, it } from "vitest";
import { ScrollLock, lineOf, readingAt, type Reading } from "./syncscroll";

const at = (page: number, fraction = 0): Reading => ({ page, fraction });
const free = (partnerAt: Reading) => ({ following: false, alone: false, partnerAt });

/** Document 1 at page 3, 40% down; document 2 two pages ahead. */
function locked() {
  const lock = new ScrollLock();
  lock.lock({ id: 1, at: at(3, 0.4), zoom: 1 }, { id: 2, at: at(5, 0.4), zoom: 2 });
  return lock;
}

const close = (got: Reading | null, page: number, fraction: number) => {
  expect(got).not.toBeNull();
  expect(got!.page).toBe(page);
  expect(got!.fraction).toBeCloseTo(fraction, 9);
};

describe("a reading as a line", () => {
  it("goes there and back", () => {
    expect(lineOf(at(3, 0.25))).toBe(3.25);
    expect(readingAt(3.25)).toEqual(at(3, 0.25));
    expect(readingAt(7)).toEqual(at(7, 0));
  });

  it("holds a place before the first page at the first page's top", () => {
    expect(readingAt(-1.5)).toEqual(at(0, 0));
  });
});

describe("two documents locked together", () => {
  it("keeps the second as far ahead as it was when they were locked", () => {
    const lock = locked();
    close(lock.moved(1, at(4, 0.1), free(at(5, 0.4))), 6, 0.1);
  });

  it("keeps the first as far behind when the second leads", () => {
    const lock = locked();
    close(lock.moved(2, at(9, 0.9), free(at(3, 0.4))), 7, 0.9);
  });

  it("answers nothing for a frame that moved nothing", () => {
    const lock = locked();
    expect(lock.moved(1, at(3, 0.4), free(at(5, 0.4)))).toBeNull();
    lock.moved(1, at(4), free(at(5, 0.4)));
    expect(lock.moved(1, at(4), free(at(6)))).toBeNull();
  });

  it("does not send a move back to the document it came from", () => {
    const lock = locked();
    const target = lock.moved(1, at(4, 0.4), free(at(5, 0.4)));
    close(target, 6, 0.4);
    // The partner's own frame, drawn while it is being moved.
    expect(lock.moved(2, target!, { following: true, alone: false, partnerAt: at(4, 0.4) })).toBeNull();
    // And its next frame, which shows the same place.
    expect(lock.moved(2, target!, free(at(4, 0.4)))).toBeNull();
  });

  it("lets a document held at its last page stay there, and brings it back in step", () => {
    const lock = locked();
    // Document 2 has 8 pages. The lock asks for page 11; the viewer stops at 7.
    close(lock.moved(1, at(9, 0.4), free(at(5, 0.4))), 11, 0.4);
    lock.moved(2, at(7, 0.2), { following: true, alone: false, partnerAt: at(9, 0.4) });
    // A tile arrives in document 2: a frame, and nothing moved. Document 1 is not dragged back.
    expect(lock.moved(2, at(7, 0.2), free(at(9, 0.4)))).toBeNull();
    // Scrolling document 1 back restores the two pages between them.
    close(lock.moved(1, at(4, 0.4), free(at(7, 0.2))), 6, 0.4);
  });

  it("stops at the first page when the offset would go before it", () => {
    const lock = locked();
    expect(lock.moved(2, at(1, 0), free(at(3, 0.4)))).toEqual(at(0, 0));
  });

  it("takes a new offset from a side scrolled alone, and moves nothing for it", () => {
    const lock = locked();
    expect(lock.moved(1, at(5, 0.4), { following: false, alone: true, partnerAt: at(5, 0.4) })).toBeNull();
    // They are level now, and stay level.
    close(lock.moved(1, at(6, 0.5), free(at(5, 0.4))), 6, 0.5);
    close(lock.moved(2, at(2, 0.5), free(at(6, 0.5))), 2, 0.5);
  });

  it("takes the offset the same way when the second document is the one scrolled alone", () => {
    const lock = locked();
    expect(lock.moved(2, at(4, 0.4), { following: false, alone: true, partnerAt: at(3, 0.4) })).toBeNull();
    close(lock.moved(1, at(6, 0), free(at(4, 0.4))), 7, 0);
  });

  it("names each document's partner, and knows a stranger", () => {
    const lock = locked();
    expect(lock.partnerOf(1)).toBe(2);
    expect(lock.partnerOf(2)).toBe(1);
    expect(lock.partnerOf(9)).toBe(-1);
    expect(lock.locks(9)).toBe(false);
    expect(lock.moved(9, at(1), free(at(1)))).toBeNull();
  });

  it("answers nothing once released", () => {
    const lock = locked();
    lock.release();
    expect(lock.locks(1)).toBe(false);
    expect(lock.moved(1, at(8), free(at(5, 0.4)))).toBeNull();
    expect(lock.zoomed(1, 3, { following: false, fitted: false })).toBeNull();
  });

  it("starts from the new places when locked again", () => {
    const lock = locked();
    lock.lock({ id: 1, at: at(0), zoom: 1 }, { id: 3, at: at(0), zoom: 1 });
    expect(lock.locks(2)).toBe(false);
    close(lock.moved(3, at(2, 0.5), free(at(0))), 2, 0.5);
  });
});

describe("a partner put in place", () => {
  it("is aimed where the offset says, moved or not", () => {
    const lock = locked();
    close(lock.aim(1, at(3, 0.4)), 5, 0.4);
    close(lock.aim(2, at(5, 0.4)), 3, 0.4);
    expect(lock.aim(9, at(1))).toBeNull();
  });

  it("does not lead from the place and zoom it was put at", () => {
    const lock = locked();
    lock.seen(2, at(6, 0.1), 2.5);
    expect(lock.moved(2, at(6, 0.1), free(at(4, 0.1)))).toBeNull();
    expect(lock.zoomed(2, 2.5, { following: false, fitted: false })).toBeNull();
    // A stranger is not recorded.
    lock.seen(9, at(1), 1);
    expect(lock.locks(9)).toBe(false);
  });
});

describe("zoom across the lock", () => {
  const set = { following: false, fitted: false };

  it("passes the factor of a zoom the reader set", () => {
    const lock = locked();
    expect(lock.zoomed(1, 1.25, set)).toBeCloseTo(1.25, 9);
    expect(lock.zoomed(2, 1, set)).toBeCloseTo(0.5, 9);
  });

  it("passes nothing when the zoom did not change", () => {
    const lock = locked();
    expect(lock.zoomed(1, 1, set)).toBeNull();
  });

  it("does not pass a zoom back, or one the window made", () => {
    const lock = locked();
    expect(lock.zoomed(2, 2.5, { following: true, fitted: false })).toBeNull();
    // Recorded all the same, so the next step is measured from it.
    expect(lock.zoomed(2, 5, set)).toBeCloseTo(2, 9);
    expect(lock.zoomed(1, 0.7, { following: false, fitted: true })).toBeNull();
    expect(lock.zoomed(1, 1.4, set)).toBeCloseTo(2, 9);
  });

  it("passes nothing from a zoom of nothing, and measures the next step from the real one", () => {
    // A viewer locked before it has been laid out reports no zoom yet. A
    // factor worked out from that is not a number to zoom the other side by.
    const lock = new ScrollLock();
    lock.lock({ id: 1, at: at(0), zoom: 0 }, { id: 2, at: at(0), zoom: 1 });
    expect(lock.zoomed(1, 1.25, set)).toBeNull();
    expect(lock.zoomed(1, 2.5, set)).toBeCloseTo(2, 9);
  });
});
