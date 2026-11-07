/**
 * *Copy page for PowerPoint* says when the copy leaves text out
 * (`pagecopynotice.ts`). The count is the command's reply, from `pagecopy.rs`;
 * `tests/cli/pagecopy.rs` holds the count against a fixture.
 */

import { describe, expect, it } from "vitest";

import { pageCopyNotice } from "./pagecopynotice";

describe("pageCopyNotice", () => {
  it("says only that the page was copied when nothing is left out", () => {
    // Mutation: add the sentence at 0 -> red.
    expect(pageCopyNotice(3, 0, "en-US")).toBe(
      "Copied page 3 for PowerPoint. Paste it there and choose Ungroup to edit its shapes and text.",
    );
  });

  it("tells one from many in English", () => {
    expect(pageCopyNotice(3, 1, "en-US")).toMatch(
      /Ungroup to edit its shapes and text\. One piece of text is left out: the page hides some or all of it,/,
    );
    expect(pageCopyNotice(3, 2, "en-US")).toMatch(/text\. 2 pieces of text are left out: the page hides some or all of them,/);
  });

  it("tells one from many in German, and the formal German addresses the reader as Sie", () => {
    expect(pageCopyNotice(3, 1, "de-DE")).toMatch(/Füge sie dort ein.* Ein Textstück fehlt in der Kopie/);
    expect(pageCopyNotice(3, 4, "de-DE")).toMatch(/ 4 Textstücke fehlen in der Kopie/);
    expect(pageCopyNotice(3, 4, "de-DE-formal")).toMatch(/Fügen Sie sie dort ein.* 4 Textstücke fehlen/);
  });
});
