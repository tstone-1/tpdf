import { describe, expect, it } from "vitest";
import { chooseTab, commentTarget, followStatus, type TabWork } from "./documentmount";
import { PageMap, pageId } from "./pages";
import type { Tab } from "./sidebar";

describe("commentTarget", () => {
  const pages = new PageMap([{ id: pageId(0), source: { baseline: 1 }, turns: 0 }]);

  it("retains the captured owner at slot zero", () => {
    expect(commentTarget({ object: [12, 0], pageId: pageId(0) }, pages))
      .toEqual({ object: [12, 0], page: 0 });
  });

  it("drops a missing object, missing owner or deleted page", () => {
    expect(commentTarget({ object: null, pageId: pageId(0) }, pages)).toBeNull();
    expect(commentTarget({ object: [3, 0] }, pages)).toBeNull();
    expect(commentTarget({ object: [3, 0], pageId: pageId(9) }, pages)).toBeNull();
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
