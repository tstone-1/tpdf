import { describe, expect, it } from "vitest";

import { basename } from "./paths";

describe("basename", () => {
  it("takes the last segment of a POSIX path", () => {
    expect(basename("/Users/reader/Documents/report.pdf")).toBe("report.pdf");
  });

  it("takes the last segment of a Windows path", () => {
    // The regression this module exists for. The old `split("/")` returned the
    // whole string here, and it did so on the platform that ships --- every
    // path from the native dialog, from `take_launch_paths` and from the
    // single-instance forward is spelled this way.
    expect(basename("C:\\Users\\reader\\Documents\\report.pdf")).toBe("report.pdf");
  });

  it("takes the last segment of a path that mixes separators", () => {
    // Legal on Windows, and the case a fix that merely swapped one separator
    // for the other would still get wrong.
    expect(basename("C:\\Users\\reader/Documents\\report.pdf")).toBe("report.pdf");
    expect(basename("C:/Users/reader\\report.pdf")).toBe("report.pdf");
  });

  it("returns a bare file name unchanged", () => {
    // The control: a string with no separator must survive the split. Without
    // it, an implementation returning "" for everything would still pass the
    // two cases above if they were asserted only as "not the whole path".
    expect(basename("report.pdf")).toBe("report.pdf");
  });

  it("falls back to the whole path when it ends in a separator", () => {
    // `"a/b/".split("/")` ends in an empty string, and an empty label is worse
    // than the directory it came from --- a detail column reading `page 7` with
    // nothing in front of it names no document at all.
    expect(basename("/Users/reader/Documents/")).toBe("/Users/reader/Documents/");
    expect(basename("C:\\Users\\reader\\")).toBe("C:\\Users\\reader\\");
  });

  it("returns the empty string for the empty path", () => {
    // Reachable: `App.svelte` and both harnesses hold "" for "no document
    // open", and the label is computed before anything checks that.
    expect(basename("")).toBe("");
  });
});
