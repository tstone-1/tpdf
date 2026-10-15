import { describe, expect, it } from "vitest";
import { chooseTab, commentTarget, followStatus, type TabWork } from "./documentmount";
import type { PageId } from "./pages";
import type { Tab } from "./sidebar";

const id = (n: number) => n as PageId;

describe("commentTarget", () => {
  it("names the comment's object and the identity of its page, not its slot", () => {
    // Slot 2 is page 7: a page was moved above it. Handing the slot through is
    // the id-and-slot mix-up the translation exists for.
    expect(commentTarget({ object: [12, 0], page: 2 }, (slot) => (slot === 2 ? id(7) : undefined)))
      .toEqual({ object: [12, 0], page: 7 });
  });

  it("addresses nothing for a comment written as a direct dictionary", () => {
    expect(commentTarget({ object: null, page: 0 }, () => id(0))).toBeNull();
  });

  it("addresses nothing on a slot the model no longer has", () => {
    expect(commentTarget({ object: [3, 0], page: 9 }, () => undefined)).toBeNull();
  });

  it("keeps page 0, which is a page and not a missing one", () => {
    expect(commentTarget({ object: [3, 0], page: 0 }, () => id(0))).toEqual({ object: [3, 0], page: 0 });
  });
});

describe("chooseTab", () => {
  const record = (tab: Tab): string[] => {
    const done: string[] = [];
    const work: TabWork = {
      commentWords: () => done.push("comment words"),
      redactionWords: () => done.push("redaction words"),
      clearRegion: () => done.push("clear region"),
    };
    chooseTab(tab, work);
    return done;
  };

  it("finds the comments' words only when the comments tab is chosen", () => {
    expect(record("comments")).toEqual(["comment words", "clear region"]);
  });

  it("finds the regions' words only when the redactions tab is chosen", () => {
    expect(record("redactions")).toEqual(["redaction words", "clear region"]);
  });

  it("leaves the ring over a passage while the hidden-text list is shown", () => {
    expect(record("hidden")).toEqual([]);
  });

  it("takes the ring away, and walks nothing, for any other tab", () => {
    for (const tab of ["outline", "pages", "results", "marks"] as Tab[]) {
      expect(record(tab)).toEqual(["clear region"]);
    }
  });
});

describe("followStatus", () => {
  const strip = () => {
    const seen: unknown[] = [];
    return {
      seen,
      setViewerBusy: (busy: boolean) => seen.push(["busy", busy]),
      setTurns: (turns: number) => seen.push(["turns", turns]),
      setInvert: (invert: boolean) => seen.push(["invert", invert]),
    };
  };

  it("tells the strip the viewer is busy while any work is outstanding", () => {
    const one = strip();
    followStatus(one, { pending: 1, turns: 0, invert: false });
    expect(one.seen).toContainEqual(["busy", true]);
    const none = strip();
    followStatus(none, { pending: 0, turns: 0, invert: false });
    expect(none.seen).toContainEqual(["busy", false]);
  });

  it("passes the rotation and the inversion as the status has them", () => {
    const one = strip();
    followStatus(one, { pending: 0, turns: 3, invert: true });
    expect(one.seen).toEqual([["busy", false], ["turns", 3], ["invert", true]]);
  });

  it("does nothing before there is a sidebar", () => {
    expect(() => followStatus(null, { pending: 2, turns: 1, invert: true })).not.toThrow();
  });
});
