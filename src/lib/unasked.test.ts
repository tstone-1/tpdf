/**
 * Where a report that arrived on its own is put. That `App.svelte` sends every
 * such report through it is held in `recovery.test.ts`, which reads the
 * component's text for the message bar already.
 *
 * Each test was checked by mutating `unasked.ts` or `App.svelte` and seeing it
 * fail; the rows are in `scripts/mutation_rows/frontend/f10_app_wiring.py`.
 */

import { describe, expect, it } from "vitest";

import { promptUnaskedNow, whereUnasked } from "./unasked";

describe("a report that arrived on its own", () => {
  it("a links notice does not replace a message on screen", () => {
    // A redaction's verdict, said a second before the links were read.
    expect(whereUnasked("Redacted, but not verified: the page could not be read.")).toBe("notice");
  });

  it("takes the message bar while it is empty", () => {
    expect(whereUnasked(null)).toBe("message");
    // The bar draws nothing for an empty message, so there is nothing to keep.
    expect(whereUnasked("")).toBe("message");
  });
});

describe("a prompt that arrived on its own and needs the bar's buttons", () => {
  it("waits while a message is on screen", () => {
    expect(promptUnaskedNow("Redacted, but not verified: the page could not be read.")).toBe(false);
  });

  it("is shown while the bar is empty", () => {
    expect(promptUnaskedNow(null)).toBe(true);
    expect(promptUnaskedNow("")).toBe(true);
  });
});
