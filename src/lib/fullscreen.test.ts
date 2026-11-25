/**
 * Full screen and presentation: the window asked for, and the viewer put back.
 */

import { describe, expect, it } from "vitest";

import { ScreenMode, type ScreenViewer, type ScreenWindow } from "./fullscreen";
import { SINGLE, type Spread } from "./scroller";
import type { FitMode, Fitted } from "./zoom";

/** A viewer that records what it was set to. */
function viewer(spread: Spread = SINGLE, fit: FitMode = "width", zoom = 1.5): ScreenViewer & { log: string[] } {
  const log: string[] = [];
  const state = { spread, fit, zoom };
  return {
    log,
    get spread() { return state.spread; },
    get fitMode() { return state.fit; },
    get currentZoom() { return state.zoom; },
    setSpread(next: Spread) { state.spread = next; log.push(`spread ${next.columns} ${next.continuous ? "continuous" : "one at a time"}`); },
    setFit(mode: Fitted) { state.fit = mode; log.push(`fit ${mode}`); },
    setZoomFixed(next: number) { state.zoom = next; state.fit = "none"; log.push(`zoom ${next}`); },
  };
}

/** A window that records each change of full screen. */
function window(): ScreenWindow & { calls: boolean[] } {
  const calls: boolean[] = [];
  return { calls, setFullscreen: async (on: boolean) => { calls.push(on); } };
}

describe("Full screen", () => {
  it("fills the screen and leaves the layout alone", async () => {
    const screen = new ScreenMode();
    const shown = viewer();
    const win = window();
    await screen.full(win);
    expect(screen.mode).toBe("full");
    await screen.leave(win);
    expect(screen.mode).toBe("window");
    expect(win.calls).toEqual([true, false]);
    expect(shown.log).toEqual([]);
  });
});

describe("Presenting", () => {
  it("shows one page at a time, fitted to the page, and puts the reader's view back", async () => {
    const screen = new ScreenMode();
    const shown = viewer({ ...SINGLE, columns: 2 }, "width");
    const win = window();
    await screen.present(win);
    screen.show(shown);
    expect(screen.mode).toBe("presenting");
    expect(shown.log).toEqual(["spread 2 one at a time", "fit page"]);
    await screen.leave(win);
    expect(shown.log.slice(2)).toEqual(["spread 2 continuous", "fit width"]);
    expect(win.calls).toEqual([true, false]);
  });

  it("puts back a zoom the reader had set by hand", async () => {
    const screen = new ScreenMode();
    const shown = viewer(SINGLE, "none", 2.25);
    const win = window();
    await screen.present(win);
    screen.show(shown);
    await screen.leave(win);
    expect(shown.log.at(-1)).toBe("zoom 2.25");
  });

  it("restores each mount's own layout across a tab switch, unmount and reopen", async () => {
    const screen = new ScreenMode(), win = window();
    const a = viewer(SINGLE, "none", 1.5), b = viewer(SINGLE, "none", 0.75);
    await screen.present(win);
    screen.show(a);
    screen.show(b);
    screen.show(b);
    const kept = screen.layoutOf(a);
    expect(kept).toEqual({ spread: SINGLE, fit: "none", zoom: 1.5 });
    screen.unmounted(a);
    const reopened = viewer(kept.spread, kept.fit, kept.zoom);
    screen.show(reopened);
    await screen.leave(win);
    expect([reopened.fitMode, reopened.currentZoom, b.fitMode, b.currentZoom]).toEqual(["none", 1.5, "none", 0.75]);
    expect(b.spread).toEqual(SINGLE);
    expect(a.log).toEqual(["spread 1 one at a time", "fit page"]);
  });

  it("preserves the previous mode and layout after a native refusal and can retry", async () => {
    const screen = new ScreenMode(), win = window(), shown = viewer(SINGLE, "none", 1.5);
    const refused = { setFullscreen: async () => { throw Error("native refusal"); } };
    await expect(screen.full(refused)).rejects.toThrow("native refusal");
    await expect(screen.present(refused)).rejects.toThrow("native refusal");
    screen.show(shown);
    expect(screen.mode).toBe("window");
    expect(shown.log).toEqual([]);
    await screen.present(win);
    screen.show(shown);
    await expect(screen.leave(refused)).rejects.toThrow("native refusal");
    expect(screen.mode).toBe("presenting");
    expect(shown.fitMode).toBe("page");
    await screen.leave(win);
    expect(screen.mode).toBe("window");
    expect(shown.fitMode).toBe("none");
  });

  it("starts from full screen without asking the window again", async () => {
    const screen = new ScreenMode();
    const shown = viewer();
    const win = window();
    await screen.full(win);
    await screen.present(win);
    screen.show(shown);
    expect(win.calls).toEqual([true]);
    expect(screen.mode).toBe("presenting");
  });

  it("follows the window out of full screen when the platform's own control was used", async () => {
    const screen = new ScreenMode();
    const shown = viewer(SINGLE, "width");
    const win = window();
    await screen.present(win);
    screen.show(shown);
    screen.left();
    expect(screen.mode).toBe("window");
    expect(shown.log.at(-1)).toBe("fit width");
    // Not asked to leave a second time.
    expect(win.calls).toEqual([true]);
  });
});

describe("Blanking the screen while presenting", () => {
  it("blacks or whites out on b or w, and any key brings the page back", async () => {
    const screen = new ScreenMode();
    await screen.present(window());
    expect(screen.presentingKey("b")).toBe(true);
    expect(screen.blank).toBe("black");
    expect(screen.presentingKey("x")).toBe(true);
    expect(screen.blank).toBeNull();
    expect(screen.presentingKey("W")).toBe(true);
    expect(screen.blank).toBe("white");
  });

  it("leaves other keys alone, and does nothing outside a presentation", async () => {
    const screen = new ScreenMode();
    expect(screen.presentingKey("b")).toBe(false);
    await screen.full(window());
    expect(screen.presentingKey("b")).toBe(false);
    await screen.present(window());
    expect(screen.presentingKey("n")).toBe(false);
    expect(screen.blank).toBeNull();
  });

  it("is not blank after leaving", async () => {
    const screen = new ScreenMode();
    const win = window();
    await screen.present(win);
    screen.presentingKey("b");
    await screen.leave(win);
    expect(screen.blank).toBeNull();
  });
});
