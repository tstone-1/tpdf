import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { installFakeDom, type FakeDom } from "./testdom";
import {
  DEFAULT_FILL,
  FILLS,
  FillPicker,
  fillNote,
  isFill,
  readFill,
  writeFill,
  type Fill,
} from "./redactfill";

/** A store that keeps what it is given, or throws when asked to. */
function store(initial: Record<string, string> = {}, broken = false) {
  const kept = new Map(Object.entries(initial));
  return () => ({
    getItem: (key: string) => {
      if (broken) throw new Error("no storage");
      return kept.get(key) ?? null;
    },
    setItem: (key: string, value: string) => {
      if (broken) throw new Error("no storage");
      kept.set(key, value);
    },
  });
}

describe("the colour of a redaction's boxes", () => {
  it("is black unless another was chosen", () => {
    expect(DEFAULT_FILL).toBe("black");
    expect(FILLS[0]?.fill).toBe("black");
    expect(readFill(store())).toBe("black");
  });

  it("is kept between sessions", () => {
    const kept = store();
    expect(writeFill("red", kept)).toBe(true);
    expect(readFill(kept)).toBe("red");
    expect(writeFill("white", kept)).toBe(true);
    expect(readFill(kept)).toBe("white");
  });

  it("reads as black when what is stored is not one of the three", () => {
    // A redaction must not turn white, or any colour nobody chose, because a
    // stored value was damaged.
    expect(readFill(store({ "tpdf.redaction-fill": "White" }))).toBe("black");
    expect(readFill(store({ "tpdf.redaction-fill": "" }))).toBe("black");
    expect(readFill(store({}, true))).toBe("black");
    expect(writeFill("red", store({}, true))).toBe(false);
  });

  it("knows the three names and no other", () => {
    for (const name of ["black", "white", "red"]) expect(isFill(name)).toBe(true);
    for (const name of ["Black", "blue", "", null, 0]) expect(isFill(name)).toBe(false);
    expect(FILLS.map((one) => one.fill)).toEqual(["black", "white", "red"]);
  });

  it("says what white means, and nothing for the other two", () => {
    expect(fillNote("white")).toContain("cannot be seen on white paper");
    expect(fillNote("black")).toBe("");
    expect(fillNote("red")).toBe("");
  });
});

describe("the picker in the redactions panel", () => {
  let dom: FakeDom;
  let chosen: Fill[] = [];

  beforeEach(() => {
    dom = installFakeDom();
    chosen = [];
  });
  afterEach(() => dom.restore());

  it("opens on the kept colour and tells its owner of a new one, once", () => {
    const picker = new FillPicker({ current: "red", onChange: (fill) => chosen.push(fill) });
    expect(picker.fill).toBe("red");
    picker.choose("red");
    expect(chosen).toEqual([]);
    picker.choose("black");
    expect(chosen).toEqual(["black"]);
    expect(picker.fill).toBe("black");
  });

  it("shows the note while white is chosen and takes it away afterwards", () => {
    const picker = new FillPicker({ current: "black", onChange: (fill) => chosen.push(fill) });
    expect(picker.said).toBe("");
    picker.choose("white");
    expect(picker.said).toContain("cannot be seen on white paper");
    picker.choose("red");
    expect(picker.said).toBe("");
  });
});
