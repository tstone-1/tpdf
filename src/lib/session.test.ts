import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clampPlace, samePlace, SessionWriter, type Place } from "./session";

/** A place, with every field set to something distinguishable. */
function place(overrides: Partial<Place> = {}): Place {
  return {
    path: "/tmp/a.pdf",
    page: 4,
    top_pt: 120,
    zoom: 1.5,
    fit: "none",
    turns: 1,
    sidebar: true,
    page_count: 10,
    ...overrides,
  };
}

describe("clampPlace", () => {
  it("leaves a page the document still has", () => {
    expect(clampPlace(place({ page: 4 }), 10).page).toBe(4);
  });

  it("clamps a page the document no longer has", () => {
    // The remembered document had 10 pages; this one has 3.
    expect(clampPlace(place({ page: 9 }), 3).page).toBe(2);
  });

  it("clamps a negative page to the first", () => {
    expect(clampPlace(place({ page: -2 }), 10).page).toBe(0);
  });

  it("survives a document reporting no pages", () => {
    // Math.max(0, -1) rather than -1: a viewer scrolled to page -1 is worse
    // than one showing nothing.
    expect(clampPlace(place({ page: 4 }), 0).page).toBe(0);
  });

  it("records the count the document has now, not the one remembered", () => {
    expect(clampPlace(place({ page_count: 10 }), 3).page_count).toBe(3);
  });

  it("keeps everything that is not the page", () => {
    const fitted = clampPlace(place({ page: 99, zoom: 2, turns: 3 }), 5);
    expect(fitted.zoom).toBe(2);
    expect(fitted.turns).toBe(3);
    expect(fitted.top_pt).toBe(120);
    expect(fitted.sidebar).toBe(true);
  });
});

describe("samePlace", () => {
  it("is true for two identical places", () => {
    expect(samePlace(place(), place())).toBe(true);
  });

  // One case per field, because a comparison that forgets one is a position
  // that silently stops being saved -- and only for that one way of moving.
  const moved: [string, Partial<Place>][] = [
    ["wheel paging", { wheel_pages: true }],
    ["path", { path: "/tmp/b.pdf" }],
    ["page", { page: 5 }],
    ["top_pt", { top_pt: 121 }],
    ["zoom", { zoom: 2 }],
    ["fit", { fit: "width" }],
    ["turns", { turns: 2 }],
    ["sidebar", { sidebar: false }],
  ];

  for (const [field, change] of moved) {
    it(`is false when ${field} differs`, () => {
      expect(samePlace(place(), place(change))).toBe(false);
    });
  }

  it("ignores the page count", () => {
    // Deliberately: the count is a property of the document, and it cannot
    // change while that document is open. Comparing it would only ever fire on
    // the clamp above, which is not the reader moving.
    expect(samePlace(place(), place({ page_count: 999 }))).toBe(true);
  });
});

describe("SessionWriter", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** A `send` that records what it was given. */
  function recorder(): { sent: Place[]; send: (p: Place) => Promise<void> } {
    const sent: Place[] = [];
    return {
      sent,
      send: (p) => {
        sent.push(p);
        return Promise.resolve();
      },
    };
  }

  /**
   * Drains microtasks without advancing the clock.
   *
   * Writes are chained through a promise, so even the leading-edge one reaches
   * `send` a microtask after the note rather than synchronously. That is the
   * ordering guarantee working, not a delay worth removing --- but it does mean
   * every assertion about what was sent has to settle first.
   */
  const settle = () => vi.advanceTimersByTimeAsync(0);

  it("writes the first note immediately", async () => {
    // A document opened and closed inside one interval must still be
    // remembered, so the throttle has a leading edge rather than only a
    // trailing one.
    const { sent, send } = recorder();
    new SessionWriter(send, 1000).note(place());
    await settle();
    expect(sent).toHaveLength(1);
  });

  it("collapses a burst into one trailing write", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    for (let page = 0; page < 20; page++) writer.note(place({ page }));
    await settle();
    expect(sent).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(1000);
    expect(sent).toHaveLength(2);
  });

  it("carries the last note of a burst, not the first", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 1 }));
    writer.note(place({ page: 2 }));
    writer.note(place({ page: 3 }));

    await vi.advanceTimersByTimeAsync(1000);
    expect(sent.map((p) => p.page)).toEqual([1, 3]);
  });

  it("does not write a place it has already written", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 2 }));
    await vi.advanceTimersByTimeAsync(1000);
    writer.note(place({ page: 2 }));
    await vi.advanceTimersByTimeAsync(1000);

    expect(sent).toHaveLength(1);
  });

  it("writes again once the reader moves on", async () => {
    // The control for the test above: "never writes twice" would pass it too.
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 2 }));
    await vi.advanceTimersByTimeAsync(1000);
    writer.note(place({ page: 3 }));
    await settle();

    expect(sent.map((p) => p.page)).toEqual([2, 3]);
  });

  it("writes the same place again once told the store has forgotten it", async () => {
    // The control is the test two above: without being told, it does not.
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 2 }));
    await vi.advanceTimersByTimeAsync(1000);
    writer.forgotten();
    writer.note(place({ page: 2 }));
    await settle();

    expect(sent.map((p) => p.page)).toEqual([2, 2]);
  });

  it("remembers a reopened file at the same place after its individual recent was removed", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);
    const opened = place();
    writer.note(opened);
    await writer.settled();
    writer.forgotten("/unrelated.pdf");
    writer.note(opened);
    writer.flush();
    await writer.settled();
    expect(sent).toEqual([opened]);
    writer.forgotten(opened.path);
    writer.note(opened);
    writer.flush();
    await writer.settled();
    expect(sent).toEqual([opened, opened]);
  });

  it("flushes an outstanding note without waiting for the interval", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 1 }));
    writer.note(place({ page: 2 }));
    await settle();
    expect(sent).toHaveLength(1);

    writer.flush();
    await settle();
    expect(sent.map((p) => p.page)).toEqual([1, 2]);
  });

  it("keeps a waiting place for each of two documents read in one interval", async () => {
    // Reading side by side notes both documents. With one waiting slot, the
    // second document's note replaced the first's and the first document's
    // last position was never written. Mutation: key `pending` by one
    // constant instead of `place.path` -> red.
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);
    const a = (page: number) => place({ path: "/tmp/a.pdf", page });
    const b = (page: number) => place({ path: "/tmp/b.pdf", page });

    writer.note(a(1));
    writer.note(a(2));
    writer.note(b(7));
    writer.note(a(3));
    await vi.advanceTimersByTimeAsync(1000);
    // The leading write, then both waiting places, the last noted last so the
    // store's most recent document is the one read last.
    expect(sent.map((p) => [p.path, p.page])).toEqual([
      ["/tmp/a.pdf", 1], ["/tmp/b.pdf", 7], ["/tmp/a.pdf", 3],
    ]);
    // Control: one document in one interval is still one trailing write.
    writer.note(a(4));
    writer.note(a(5));
    await vi.advanceTimersByTimeAsync(2000);
    expect(sent.map((p) => p.page).slice(3)).toEqual([5]);
  });

  it.each(["quit", "timer"])("keeps the revisited document most recent on %s", async (when) => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);
    const a = place({ path: "/tmp/a.pdf", page: 1 });
    const b = place({ path: "/tmp/b.pdf", page: 2 });
    writer.note(a);
    writer.note(b);
    writer.note(a);
    if (when === "quit") writer.flush();
    else await vi.advanceTimersByTimeAsync(1000);
    await writer.settled();
    expect(sent).toEqual([a, b, a]);
  });

  it("flushes the waiting place of every document", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);
    writer.note(place({ path: "/tmp/a.pdf", page: 1 }));
    writer.note(place({ path: "/tmp/a.pdf", page: 2 }));
    writer.note(place({ path: "/tmp/b.pdf", page: 9 }));
    writer.flush();
    await settle();
    expect(sent.map((p) => [p.path, p.page])).toEqual([["/tmp/a.pdf", 1], ["/tmp/a.pdf", 2], ["/tmp/b.pdf", 9]]);
  });

  it("drops a waiting place once the reader is back where it was written", async () => {
    // Page 1 written, page 2 waiting, back to page 1: the reader is on page 1.
    // Returning before removing the pending page 2 writes a stale position.
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);
    writer.note(place({ page: 1 }));
    writer.note(place({ page: 2 }));
    writer.note(place({ page: 1 }));
    await vi.advanceTimersByTimeAsync(1000);
    expect(sent.map((p) => p.page)).toEqual([1]);
  });

  it("flushes nothing when the last place is already written", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 1 }));
    writer.flush();
    writer.flush();
    await settle();
    expect(sent).toHaveLength(1);
  });

  it("waits for a flushed write to be answered, so a relaunch cannot outrun it", async () => {
    // **The one caller that can wait, and must.** `flush()` issues the write and
    // returns, which is right for `pagehide` and for a closing window; a
    // relaunch ends the process inside its very next call, so an unanswered
    // write is a lost reading position. Mutation: resolve `settled()` before the
    // queue -> red, because `answered` would still be false when it resolves.
    let answered = false;
    let land!: () => void;
    const writer = new SessionWriter((_p) => new Promise<void>((resolve) => {
      land = () => { answered = true; resolve(); };
    }), 1000);

    writer.note(place({ page: 1 }));
    let settledYet = false;
    void writer.settled().then(() => { settledYet = true; });
    await settle();
    expect(settledYet).toBe(false);

    land();
    await settle();
    expect(settledYet).toBe(true);
    expect(answered).toBe(true);
  });

  it("settles rather than rejecting when a write fails", async () => {
    // A caller about to end the process has nothing useful to do with the news,
    // and an unhandled rejection there would be the last thing it did.
    const writer = new SessionWriter(() => Promise.reject(new Error("no disk")), 1000);
    writer.note(place({ page: 1 }));
    await expect(writer.settled()).resolves.toBeUndefined();
  });

  it("drops a scheduled write when stopped", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.note(place({ page: 1 }));
    writer.note(place({ page: 2 }));
    writer.stop();

    await vi.advanceTimersByTimeAsync(5000);
    expect(sent.map((p) => p.page)).toEqual([1]);
  });

  it("ignores a note after being stopped", async () => {
    const { sent, send } = recorder();
    const writer = new SessionWriter(send, 1000);

    writer.stop();
    writer.note(place({ page: 1 }));
    await settle();
    expect(sent).toHaveLength(0);
  });

  it("never has two writes in flight at once", async () => {
    // `invoke` resolves out of order under load, and two writes racing means
    // the older place can land last and overwrite the newer one. The writes are
    // chained, so the second `send` is not even called until the first settles.
    let release = () => {};
    const sent: Place[] = [];
    const send = (p: Place): Promise<void> => {
      sent.push(p);
      return sent.length === 1
        ? new Promise<void>((resolve) => {
            release = resolve;
          })
        : Promise.resolve();
    };

    const writer = new SessionWriter(send, 1000);
    writer.note(place({ page: 1 }));
    writer.note(place({ page: 2 }));
    writer.flush();
    await settle();
    expect(sent.map((p) => p.page)).toEqual([1]);

    release();
    await settle();
    expect(sent.map((p) => p.page)).toEqual([1, 2]);
  });

  it("retries an identical position after a failed write", async () => {
    const send = vi.fn().mockRejectedValueOnce(Error("unavailable")).mockResolvedValue(undefined);
    const writer = new SessionWriter(send, 1000);
    const current = place({ page: 2 });
    writer.note(current);
    await writer.settled();
    writer.note(current);
    writer.flush();
    await writer.settled();
    writer.note(current);
    writer.flush();
    expect(send.mock.calls).toEqual([[current], [current]]);
    writer.stop();
  });

  it("keeps writing after one fails", async () => {
    // A rejected write must not poison the chain: one full disk should not stop
    // every later position from being recorded.
    const sent: Place[] = [];
    const send = (p: Place): Promise<void> => {
      sent.push(p);
      return sent.length === 1 ? Promise.reject(new Error("disk full")) : Promise.resolve();
    };

    const writer = new SessionWriter(send, 1000);
    writer.note(place({ page: 1 }));
    await vi.advanceTimersByTimeAsync(0);
    writer.note(place({ page: 2 }));
    await vi.advanceTimersByTimeAsync(1000);

    expect(sent.map((p) => p.page)).toEqual([1, 2]);
  });
});
