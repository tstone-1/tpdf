import { describe, expect, it } from "vitest";
import { OPENING_MS, SETTLE_MS, whenHeard, type FocusClock } from "./livetext";

/** A window whose focus and clock the test moves. */
function fakeWindow(focused = true) {
  let now = 0;
  const timers: { at: number; run: () => void }[] = [];
  const listeners: (() => void)[] = [];
  const clock: FocusClock = {
    hasFocus: () => focused,
    now: () => now,
    onFocus: (listener) => {
      listeners.push(listener);
      return () => {
        const at = listeners.indexOf(listener);
        if (at >= 0) listeners.splice(at, 1);
      };
    },
    later: (run, ms) => void timers.push({ at: now + ms, run }),
  };
  const advance = (ms: number) => {
    const until = now + ms;
    for (;;) {
      timers.sort((a, b) => a.at - b.at);
      const next = timers[0];
      if (!next || next.at > until) break;
      timers.shift();
      now = next.at;
      next.run();
    }
    now = until;
  };
  const blur = () => { focused = false; };
  const focus = () => {
    focused = true;
    for (const listener of [...listeners]) listener();
  };
  return { clock, advance, blur, focus, listening: () => listeners.length };
}

function writer(focused = true) {
  const win = fakeWindow(focused);
  const written: string[] = [];
  const say = whenHeard((text) => written.push(text), win.clock);
  return { ...win, written, say };
}

describe("stopping a writer", () => {
  it("takes its focus listener away and writes nothing it was still holding", () => {
    const { advance, blur, focus, listening, say, written } = writer();
    expect(listening()).toBe(1);
    blur();
    say("Page 3 of 8");
    say.stop();
    expect(listening()).toBe(0);
    focus();
    advance(OPENING_MS + SETTLE_MS);
    expect(written).toEqual([]);
    // And a text written after it goes nowhere, in a window that would
    // otherwise write it at once.
    say("Page 4 of 8");
    expect(written).toEqual([]);
    // Stopping twice is stopping once.
    say.stop();
    expect(listening()).toBe(0);
  });

  it("drops a text already waiting on a timer", () => {
    // Written as the window opens, so a timer is set for the end of the
    // opening; the writer stopped before it fires.
    const { advance, say, written } = writer();
    say("Page 1 of 8");
    say.stop();
    advance(OPENING_MS + SETTLE_MS);
    expect(written).toEqual([]);
  });

  it("is what destroying the screen-reader layer does", async () => {
    const { AccessibleText } = await import("./a11y");
    const { installFakeDom } = await import("./testdom");
    const dom = installFakeDom();
    try {
      const win = fakeWindow();
      const layer = new AccessibleText(dom.root as never, 1, () => null, win.clock);
      expect(win.listening()).toBe(1);
      layer.destroy();
      expect(win.listening()).toBe(0);
    } finally {
      dom.restore();
    }
  });
});

describe("whenHeard", () => {
  it("writes at once in a window that opened a while ago", () => {
    // A document opened later in a running window: its writer is new, and
    // nothing is held.
    const { advance, clock, written } = writer();
    advance(OPENING_MS);
    const later: string[] = [];
    const say = whenHeard((text) => later.push(text), clock);
    say("Page 2 of 8");
    expect(later).toEqual(["Page 2 of 8"]);
    expect(written).toEqual([]);
  });

  it("writes at once in a window that has had the focus a while", () => {
    const { advance, say, written } = writer();
    advance(OPENING_MS);
    say("Page 2 of 8");
    expect(written).toEqual(["Page 2 of 8"]);
  });

  it("holds a text written as the window opens until it has settled", () => {
    // The page announcement at launch, under VoiceOver's window phrase.
    const { advance, say, written } = writer();
    say("Page 1 of 8");
    expect(written).toEqual([]);
    advance(OPENING_MS - 1);
    expect(written).toEqual([]);
    advance(1);
    expect(written).toEqual(["Page 1 of 8"]);
  });

  it("does not cut the opening short when the window gets the focus early in it", () => {
    const { advance, focus, say, written } = writer();
    advance(500);
    focus();
    say("Page 1 of 8");
    advance(SETTLE_MS);
    expect(written).toEqual([]);
    advance(OPENING_MS);
    expect(written).toEqual(["Page 1 of 8"]);
  });

  it("holds a text written without the focus until the focus is back and settled", () => {
    // The report of an extraction, as the save sheet closes.
    const { advance, blur, focus, say, written } = writer();
    advance(OPENING_MS);
    blur();
    say("Wrote 2 pages to x.pdf.");
    advance(5000);
    expect(written).toEqual([]);
    focus();
    advance(SETTLE_MS - 1);
    expect(written).toEqual([]);
    advance(1);
    expect(written).toEqual(["Wrote 2 pages to x.pdf."]);
  });

  it("holds a text written just after the focus came back for the rest of the wait", () => {
    const { advance, blur, focus, say, written } = writer();
    advance(OPENING_MS);
    blur();
    focus();
    advance(100);
    say("Wrote 2 pages to x.pdf.");
    expect(written).toEqual([]);
    advance(SETTLE_MS - 100);
    expect(written).toEqual(["Wrote 2 pages to x.pdf."]);
  });

  it("writes only the latest of the texts held, a clearing included", () => {
    const { advance, blur, focus, say, written } = writer();
    advance(OPENING_MS);
    blur();
    say("first");
    say("second");
    say("");
    focus();
    advance(SETTLE_MS);
    expect(written).toEqual([""]);
  });
});
