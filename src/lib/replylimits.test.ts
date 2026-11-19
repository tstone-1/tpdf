/**
 * Every "could not be read" counter a reply carries is worded by the window.
 *
 * A reply's `limits` object is where the backend says a readout is partial,
 * and each Rust `Limits::any()` is documented as what the UI shows a notice
 * on. `replyshapes.test.ts` holds the top level of each sample against its
 * mirror and stops there, so a counter added inside `limits` could cross the
 * boundary, be dropped by a mirror that does not name it, and leave the readout
 * looking whole. Three properties counters did exactly that.
 *
 * So the keys are taken from the committed samples (written by Rust from real
 * values, `src-tauri/src/replies.rs`), never from the mirrors, and each one is
 * switched on alone: the wording function must then say something it does not
 * say with every counter at zero.
 */

import { describe, expect, it } from "vitest";

import comments from "../../src-tauri/testdata/replies/Comments.json";
import links from "../../src-tauri/testdata/replies/Links.json";
import outline from "../../src-tauri/testdata/replies/Outline.json";
import properties from "../../src-tauri/testdata/replies/Properties.json";
import { noticeFor as commentNotice, type CommentLimits } from "./comments";
import { noticeFor as linkNotice, type LinkLimits } from "./links";
import { sections, type Limits, type Properties } from "./properties";
import { noticeFor as outlineNotice } from "./sidebar";
import type { Outline } from "./outline";

/** The sample's counters, every one at its "nothing was cut" value. */
function zeroed(sample: Record<string, unknown>): Record<string, number | boolean> {
  return Object.fromEntries(
    Object.entries(sample).map(([key, value]) => [key, typeof value === "boolean" ? false : 0]),
  );
}

/** The same, with `key` alone switched on. */
function only(sample: Record<string, unknown>, key: string): Record<string, number | boolean> {
  return { ...zeroed(sample), [key]: typeof sample[key] === "boolean" ? true : 2 };
}

const READERS: [string, Record<string, unknown>, (limits: never) => unknown][] = [
  // The whole readout rather than `limitRows`, because `locked` is worded as a
  // section of its own at the top and not as a row at the foot.
  [
    "Properties",
    properties.limits,
    (limits: Limits) => JSON.stringify(sections({ ...(properties as unknown as Properties), limits })),
  ],
  ["Comments", comments.limits, (limits: CommentLimits) => commentNotice(limits)],
  ["Links", links.limits, (limits: LinkLimits) => linkNotice(limits)],
  ["Outline", outline.limits, (limits: Outline["limits"]) => outlineNotice(limits)],
];

describe("every counter in a reply's limits is worded", () => {
  for (const [name, sample, say] of READERS) {
    it(`${name}: each key, switched on alone, changes what the reader is told`, () => {
      const keys = Object.keys(sample);
      expect(keys.length).toBeGreaterThan(0);
      const quiet = say(zeroed(sample) as never);
      const unworded = keys.filter((key) => say(only(sample, key) as never) === quiet);
      expect(unworded).toEqual([]);
    });
  }

  it("the control: a key nothing reads is caught", () => {
    const [, sample, say] = READERS[0]!;
    const extended = { ...sample, a_counter_nobody_words: 1 };
    expect(say(only(extended, "a_counter_nobody_words") as never)).toBe(say(zeroed(extended) as never));
  });
});
