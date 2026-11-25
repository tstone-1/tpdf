import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { FakeElement } from "./testdom";
import { runScrollBenchIfRequested } from "./scrollbench";

const fixture = vi.hoisted(() => ({ pending: 0, output: "", exitCode: -1 }));
vi.mock("./ipc", () => ({
  async call(command: string, args: { text?: string; code?: number } = {}) {
    switch (command) {
      case "scrollbench_config": return { path: "scan.pdf", rounds: 2, frames: 2, warmup_frames: 1,
        px_per_frame: 10, tile_px: 100, zooms: [1, 1.25], layouts: ["tiles"], cache_tiles: 4,
        max_in_flight: 1, prefetch_screens: 0, cancels: [0], panes: [1] };
      case "open_document": return { id: 1, page_count: 1, pages: [{ width_pt: 600, height_pt: 800 }] };
      case "spike_print": fixture.output = args.text!; return;
      case "spike_exit": fixture.exitCode = args.code!; return;
      default: throw new Error(command);
    }
  },
}));
vi.mock("./scroller", () => ({
  Scroller: class {
    maxScroll = 300;
    stats = { delivered: 1, discarded: 0, abandoned: 0, requested: 1, bytes: 64, renderMs: 1, decodeMs: 1 };
    get outstanding() { return fixture.pending; }
    clearTiles() {}
    resetStats() {}
    frame() { return this.coverage(); }
    coverage() { return { sharp: 1, any: 1 }; }
  },
}));

beforeEach(() => {
  fixture.pending = 0;
  fixture.output = "";
  fixture.exitCode = -1;
  let timestamp = 0;
  vi.stubGlobal("requestAnimationFrame", (frame: FrameRequestCallback) => frame(timestamp += 16));
  vi.stubGlobal("document", {
    createElement: (tag: string) => new FakeElement(tag),
    body: new FakeElement("body"), visibilityState: "visible", hasFocus: () => true,
  });
  vi.stubGlobal("window", { innerWidth: 600, innerHeight: 400, devicePixelRatio: 1 });
});
afterEach(() => vi.unstubAllGlobals());

it("reports both fractional zoom variants with two rounds each", async () => {
  await runScrollBenchIfRequested();
  expect(fixture.exitCode, fixture.output).toBe(0);
  const series = fixture.output.split("per-round drops")[1]?.split("server-side")[0];
  expect(series).toMatch(/^\s*tiles\/1x\s+0\s+0\s*$/m);
  expect(series).toMatch(/^\s*tiles\/1\.25x\s+0\s+0\s*$/m);
});

it("fails before reporting comparisons when a variant cannot drain", async () => {
  fixture.pending = 1;
  await runScrollBenchIfRequested();
  expect(fixture.exitCode).toBe(1);
  expect(fixture.output).toMatch(/tiles\/1x in round 1 did not drain/);
  expect(fixture.output).not.toContain("per-round drops");
});
