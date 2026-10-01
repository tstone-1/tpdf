import { describe, expect, it } from "vitest";

import {
  afterDiskChange,
  DEFAULT_DISK_CHANGE_MODE,
  DISK_CHANGE_MODES,
  DiskWatch,
  MODE_KEY,
  readDiskChangeMode,
  writeDiskChangeMode,
  type DiskProbe,
} from "./diskwatch";

/** A probe whose answers a test sets, and which counts the reads it was asked for. */
function scripted(opened: string | null = "10:1") {
  const state = {
    opened,
    now: opened as string | null,
    differs: true as boolean | null,
    reads: 0,
    failing: false,
  };
  const probe: DiskProbe = {
    stamp: () =>
      state.failing
        ? Promise.reject(new Error("no backend"))
        : Promise.resolve([state.opened, state.now]),
    differs: () => {
      state.reads++;
      return Promise.resolve(state.differs);
    },
  };
  return { state, probe };
}

describe("DiskWatch", () => {
  it("says nothing about a file that has not moved, and reads nothing", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    for (let i = 0; i < 5; i++) await watch.check(1, "a.pdf");
    expect(reported).toEqual([]);
    expect(state.reads).toBe(0);
  });

  it("reports a changed file once, and only after the stamp held still", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    await watch.check(1, "a.pdf");
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    // First sight of a new stamp: the writer may not be done.
    expect(reported).toEqual([]);
    expect(state.reads).toBe(0);
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1]);
    for (let i = 0; i < 3; i++) await watch.check(1, "a.pdf");
    expect(reported).toEqual([1]);
    expect(state.reads).toBe(1);
  });

  it("waits for a file that is still being written", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    for (const now of ["11:2", "12:3", "13:4"]) {
      state.now = now;
      await watch.check(1, "a.pdf");
    }
    expect(reported).toEqual([]);
    expect(state.reads).toBe(0);
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1]);
  });

  it("reports a second change after the first", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    state.now = "30:3";
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1, 1]);
  });

  it("compares a touched file by content once, and stays quiet", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.now = "10:99";
    state.differs = false;
    for (let i = 0; i < 5; i++) await watch.check(1, "a.pdf");
    expect(reported).toEqual([]);
    expect(state.reads).toBe(1);
  });

  it("treats a content comparison that could not answer as a change", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.now = "20:2";
    state.differs = null;
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1]);
  });

  it("says nothing when either stamp could not be read", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.now = null;
    for (let i = 0; i < 3; i++) await watch.check(1, "a.pdf");
    state.now = "20:2";
    state.opened = null;
    for (let i = 0; i < 3; i++) await watch.check(1, "a.pdf");
    expect(reported).toEqual([]);
    expect(state.reads).toBe(0);
  });

  it("reports again when the report was not acted on", async () => {
    const { state, probe } = scripted();
    let acting = false;
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), acting));
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1, 1]);
    acting = true;
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1, 1, 1]);
  });

  it("starts over for another document", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    // The same stamps under another id are another document's, so the verdict
    // reached for document 1 says nothing about them.
    await watch.check(2, "b.pdf");
    expect(reported).toEqual([1]);
    await watch.check(2, "b.pdf");
    expect(reported).toEqual([1, 2]);
  });

  it("forgets a change that was undone", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    state.now = "10:1";
    await watch.check(1, "a.pdf");
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    // Seen once before the file went back, so this is a first sight again.
    expect(reported).toEqual([]);
  });

  it("survives a backend that rejects, and asks again", async () => {
    const { state, probe } = scripted();
    const reported: number[] = [];
    const watch = new DiskWatch(probe, (doc) => (reported.push(doc), true));
    state.failing = true;
    await expect(watch.check(1, "a.pdf")).resolves.toBeUndefined();
    state.failing = false;
    state.now = "20:2";
    await watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    expect(reported).toEqual([1]);
  });

  it("drops a check that overlaps one still running", async () => {
    let release: (stamps: [string, string]) => void = () => {};
    let asked = 0;
    const probe: DiskProbe = {
      stamp: () => {
        asked++;
        return new Promise((resolve) => (release = resolve));
      },
      differs: () => Promise.resolve(true),
    };
    const watch = new DiskWatch(probe, () => true);
    const first = watch.check(1, "a.pdf");
    await watch.check(1, "a.pdf");
    expect(asked).toBe(1);
    release(["10:1", "10:1"]);
    await first;
    void watch.check(1, "a.pdf");
    expect(asked).toBe(2);
  });
});

describe("afterDiskChange", () => {
  it("does nothing when the reader chose not to look", () => {
    expect(afterDiskChange("ignore", false, "a.pdf")).toBeNull();
    expect(afterDiskChange("ignore", true, "a.pdf")).toBeNull();
  });

  it("asks, and offers a reload", () => {
    expect(afterDiskChange("ask", false, "a.pdf")).toEqual({
      message: "a.pdf changed on disk.",
      offers: ["reload"],
    });
  });

  it("reloads without asking only when there is nothing to lose", () => {
    expect(afterDiskChange("reload", false, "a.pdf")).toBe("reload");
    const prompt = afterDiskChange("reload", true, "a.pdf");
    expect(prompt).not.toBe("reload");
    expect(prompt).toEqual(afterDiskChange("ask", true, "a.pdf"));
  });

  it("puts Save a copy first when edits are at stake", () => {
    const prompt = afterDiskChange("ask", true, "a.pdf");
    expect(prompt).toMatchObject({ offers: ["saveCopy", "reload"] });
    expect((prompt as { message: string }).message).toContain("unsaved edits");
  });
});

describe("the remembered mode", () => {
  function store(initial: Record<string, string> = {}) {
    const held = new Map(Object.entries(initial));
    return {
      getItem: (key: string) => held.get(key) ?? null,
      setItem: (key: string, value: string) => void held.set(key, value),
    };
  }

  it("is ask for a reader who never chose", () => {
    expect(DEFAULT_DISK_CHANGE_MODE).toBe("ask");
    expect(readDiskChangeMode(() => store())).toBe("ask");
  });

  it("round-trips every mode", () => {
    for (const mode of DISK_CHANGE_MODES) {
      const kept = store();
      expect(writeDiskChangeMode(mode, () => kept)).toBe(true);
      expect(kept.getItem(MODE_KEY)).toBe(mode);
      expect(readDiskChangeMode(() => kept)).toBe(mode);
    }
  });

  it("reads anything tpdf did not write as the default", () => {
    expect(readDiskChangeMode(() => store({ [MODE_KEY]: "always" }))).toBe("ask");
    expect(readDiskChangeMode(() => store({ [MODE_KEY]: "" }))).toBe("ask");
  });

  it("survives storage that throws", () => {
    const broken = () => {
      throw new Error("denied");
    };
    expect(readDiskChangeMode(broken)).toBe("ask");
    expect(writeDiskChangeMode("reload", broken)).toBe(false);
  });
});
