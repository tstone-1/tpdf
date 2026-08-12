/**
 * Tests for the update check and the download that follows it.
 *
 * What is being pinned is not "updates work" --- that needs a real endpoint and
 * a real signature, and `BUILD.md` schedules it as a manual step. It is the five
 * ways this turns into either a busy loop, a silent swap, or a header that lies:
 * a second operation starting on top of one already running, an install with no
 * check behind it, a `Finished` event treated as success, a missing
 * `Content-Length` reported as 0%, and a failed check that leaves a stale handle
 * behind for `install` to use.
 *
 * The last two are the quiet ones. A progress bar stuck at 0% reads as a slow
 * network, and an install running off a handle from a check that *failed* is the
 * one path here that could apply something the reader was never shown.
 *
 * Every test below was checked by mutating `update.ts` and confirming it went
 * red; the mutations are named on each block.
 */

import { describe, expect, it, vi } from "vitest";

import {
  percentOfDownload,
  Updates,
  updateLabel,
  type DownloadEvent,
  type UpdateHandle,
  type UpdaterApi,
} from "./update";

/** An update the fake updater will offer. */
function handle(version = "26.8.2", events: DownloadEvent[] = []): UpdateHandle {
  return {
    version,
    downloadAndInstall: async (onEvent) => {
      for (const e of events) onEvent(e);
    },
  };
}

/** An updater that answers with whatever it is given. */
function api(result: UpdateHandle | null | Error): UpdaterApi {
  return {
    check: async () => {
      if (result instanceof Error) throw result;
      return result;
    },
  };
}

describe("percentOfDownload", () => {
  it("reports a percentage when the total is known", () => {
    expect(percentOfDownload(50, 200)).toBe(25);
  });

  it("says nothing rather than zero when there is no total", () => {
    // Mutation: return 0 instead of null -> red. A bar reading 0% forever is
    // indistinguishable from a stalled download; "unknown" is the honest answer.
    expect(percentOfDownload(50, null)).toBeNull();
    expect(percentOfDownload(50, 0)).toBeNull();
  });

  it("clamps an overshoot, because a stale Content-Length is not our bug", () => {
    // Mutation: drop the Math.min -> red.
    expect(percentOfDownload(300, 200)).toBe(100);
  });
});

describe("Updates.check", () => {
  it("reports an available update with its version", async () => {
    const u = new Updates(api(handle("26.9.0")));
    expect(await u.check()).toEqual({ kind: "available", version: "26.9.0" });
  });

  it("reports being current when there is nothing newer", async () => {
    // The control for the test above: without it, a check that always claimed
    // an update would pass everything else here.
    const u = new Updates(api(null));
    expect(await u.check()).toEqual({ kind: "current" });
  });

  it("reports a failed check instead of throwing", async () => {
    const u = new Updates(api(new Error("no route to host")));
    const s = await u.check();
    expect(s.kind).toBe("failed");
    expect(s.kind === "failed" && s.message).toContain("no route to host");
  });

  it("does not start a second check while one is running", async () => {
    // Mutation: drop the #busy guard -> red. The launch check and a reader
    // typing "check for updates" genuinely race.
    let calls = 0;
    let release!: (v: UpdateHandle | null) => void;
    const slow: UpdaterApi = {
      check: () => {
        calls++;
        return new Promise((r) => (release = r));
      },
    };
    const u = new Updates(slow);
    const first = u.check();
    await u.check();
    expect(calls).toBe(1);
    release(null);
    await first;
    expect(calls).toBe(1);
  });

  it("clears the previous handle when a later check fails", async () => {
    // Mutation: leave #handle alone in the catch -> red. This is the path that
    // could install something the reader was never shown: an update found, then
    // a failed re-check, then install running off the stale handle.
    const u = new Updates({
      check: vi
        .fn<UpdaterApi["check"]>()
        .mockResolvedValueOnce(handle("26.9.0"))
        .mockRejectedValueOnce(new Error("offline")),
    });
    await u.check();
    await u.check();
    expect((await u.install()).kind).toBe("failed");
  });
});

describe("Updates.install", () => {
  it("does nothing without a check behind it", async () => {
    // Mutation: drop the !handle guard -> red (it throws instead).
    const u = new Updates(api(handle()));
    expect(await u.install()).toEqual({ kind: "idle" });
  });

  it("becomes ready only when the promise resolves, not on Finished", async () => {
    // Mutation: settle on the Finished event -> red. The event fires when the
    // bytes arrived; the promise resolves when they were verified and written,
    // and an update that fails its signature check fails between the two.
    const u = new Updates({
      check: async () => ({
        version: "26.9.0",
        downloadAndInstall: async (onEvent) => {
          onEvent({ event: "Finished" });
          throw new Error("signature did not verify");
        },
      }),
    });
    await u.check();
    const s = await u.install();
    expect(s.kind).toBe("failed");
    expect(s.kind === "failed" && s.message).toContain("signature");
  });

  it("reports progress against a known total", async () => {
    const seen: (number | null)[] = [];
    const u = new Updates(
      api(
        handle("26.9.0", [
          { event: "Started", data: { contentLength: 100 } },
          { event: "Progress", data: { chunkLength: 25 } },
          { event: "Progress", data: { chunkLength: 25 } },
        ]),
      ),
      (s) => {
        if (s.kind === "downloading") seen.push(s.percent);
      },
    );
    await u.check();
    await u.install();
    expect(seen).toEqual([null, 25, 50]);
  });

  it("accumulates chunks rather than reporting the last one", async () => {
    // Mutation: assign instead of += -> red. With equal chunks the two are
    // indistinguishable, so the chunks here are deliberately different sizes.
    const seen: (number | null)[] = [];
    const u = new Updates(
      api(
        handle("26.9.0", [
          { event: "Started", data: { contentLength: 100 } },
          { event: "Progress", data: { chunkLength: 10 } },
          { event: "Progress", data: { chunkLength: 30 } },
        ]),
      ),
      (s) => {
        if (s.kind === "downloading" && s.percent !== null) seen.push(s.percent);
      },
    );
    await u.check();
    await u.install();
    expect(seen).toEqual([10, 40]);
  });

  it("reports an unknown total as unknown throughout", async () => {
    const seen: (number | null)[] = [];
    const u = new Updates(
      api(handle("26.9.0", [{ event: "Started", data: {} }, { event: "Progress", data: { chunkLength: 10 } }])),
      (s) => {
        if (s.kind === "downloading") seen.push(s.percent);
      },
    );
    await u.check();
    await u.install();
    expect(seen).toEqual([null, null]);
  });
});

describe("updateLabel", () => {
  it("says nothing for the three states a reader did not ask about", () => {
    // Mutation: return a string for `checking` or `current` -> red. A viewer
    // that announces "you are up to date" on every launch is noise.
    expect(updateLabel({ kind: "idle" })).toBeNull();
    expect(updateLabel({ kind: "checking" })).toBeNull();
    expect(updateLabel({ kind: "current" })).toBeNull();
  });

  it("says nothing about a failed check", () => {
    // Deliberate: a launch with no network must not put an error in the header
    // of a document viewer. The palette reports it to whoever asked.
    expect(updateLabel({ kind: "failed", message: "offline" })).toBeNull();
  });

  it("names the version on offer, and the restart when one is waiting", () => {
    expect(updateLabel({ kind: "available", version: "26.9.0" })).toBe("Update to 26.9.0");
    expect(updateLabel({ kind: "ready", version: "26.9.0" })).toContain("restart");
  });

  it("distinguishes a download with a total from one without", () => {
    expect(updateLabel({ kind: "downloading", version: "1", percent: null })).toBe(
      "Downloading update",
    );
    expect(updateLabel({ kind: "downloading", version: "1", percent: 40 })).toContain("40%");
  });
});
