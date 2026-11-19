/**
 * The two halves of the disk-change window phase that can be held without a
 * window: the request line it shares with `tabs_check.py`, and how long it
 * leaves a message on screen before it says the message held.
 */

import { describe, expect, it } from "vitest";

import script from "../../scripts/tabs_check.py?raw";
import { holdFor, REWRITE_REQUEST } from "./diskchangecheck";
import { WATCH_INTERVAL_MS } from "./diskwatch";

describe("the request for a change on disk", () => {
  // The script's own pattern, compiled here: a request it does not match is
  // never answered, and the phase then fails as a window that saw nothing.
  const pattern = /REWRITE_REQUEST = re\.compile\(r"(.+?)", re\.M\)/.exec(script)?.[1];

  it("is the line tabs_check.py answers, and only as a whole line", () => {
    expect(pattern).toBeDefined();
    const matcher = new RegExp(pattern!, "m");
    expect(matcher.test(`${REWRITE_REQUEST} 1`)).toBe(true);
    expect(matcher.test(`${REWRITE_REQUEST} 12`)).toBe(true);
    // A failure detail that quotes it is not a request.
    expect(matcher.test(`[FAIL] is tabs_check.py answering ${REWRITE_REQUEST} 3?`)).toBe(false);
  });
});

describe("how long a message is left on screen", () => {
  // `WATCH_INTERVAL_MS` is the constant the window's watch runs at, so a watch
  // that slows down lengthens the wait instead of making it too short to mean
  // anything.
  it("outlasts the three checks the watch needs, by two intervals", () => {
    expect(holdFor(0)).toBe(5 * WATCH_INTERVAL_MS);
  });

  it("outlasts what the control took on this machine, by two intervals", () => {
    expect(holdFor(4_200)).toBe(4_200 + 2 * WATCH_INTERVAL_MS);
  });
});
