/**
 * Labelling recent documents.
 *
 * The whole module is one decision --- how much of a path to show --- and it has
 * an answer that can be wrong in two directions: too short and two rows are
 * indistinguishable, too long and eight rows of absolute paths are unreadable.
 *
 * Every test below was checked by mutation --- see `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import { labelsFor, recentCommandId, RECENT_PREFIX } from "./recents";

describe("labelsFor", () => {
  it("shows only the basename when that is enough", () => {
    expect(labelsFor(["/Users/t/Documents/notes.pdf", "/Users/t/spec.pdf"])).toEqual([
      "notes.pdf",
      "spec.pdf",
    ]);
  });

  it("lengthens a colliding pair until it is distinct", () => {
    expect(labelsFor(["/work/acme/report.pdf", "/work/globex/report.pdf"])).toEqual([
      "acme/report.pdf",
      "globex/report.pdf",
    ]);
  });

  it("lengthens only the labels that collide", () => {
    // The point of doing this per label rather than picking one depth for the
    // list: one awkward pair must not make every other row longer.
    expect(
      labelsFor(["/work/acme/report.pdf", "/work/globex/report.pdf", "/home/notes.pdf"]),
    ).toEqual(["acme/report.pdf", "globex/report.pdf", "notes.pdf"]);
  });

  it("keeps lengthening while a pair is still ambiguous", () => {
    // One extra directory is not always enough, and stopping after one is the
    // obvious implementation that produces two identical rows anyway.
    expect(
      labelsFor(["/a/shared/2026/report.pdf", "/b/shared/2026/report.pdf"]),
    ).toEqual(["a/shared/2026/report.pdf", "b/shared/2026/report.pdf"]);
  });

  it("stops rather than looping when two labels can never differ", () => {
    // The same path twice cannot be disambiguated however far back it goes. The
    // session store does not produce this, and a loop that only exits on success
    // hangs the application if anything ever does.
    //
    // It grows to the whole path before giving up, which is the honest answer:
    // it says everything it knows and the two are still the same document.
    expect(labelsFor(["/a/x.pdf", "/a/x.pdf"])).toEqual(["a/x.pdf", "a/x.pdf"]);
  });

  it("labels a path with one segment", () => {
    expect(labelsFor(["x.pdf"])).toEqual(["x.pdf"]);
    expect(labelsFor([""])).toEqual([""]);
  });

  it("keeps the separator the path was written with", () => {
    // A session file records absolute paths from whichever machine last opened
    // the document, so a Windows path can be read on a Mac. A label that
    // rewrote the separator would show a path that exists nowhere.
    expect(
      labelsFor(["C:\\Users\\t\\acme\\report.pdf", "C:\\Users\\t\\globex\\report.pdf"]),
    ).toEqual(["acme\\report.pdf", "globex\\report.pdf"]);
  });

  it("returns one label per path, in order", () => {
    // Positional: `App.svelte` zips these against the paths to build commands,
    // so a reordering would put every row's name on the wrong document.
    const paths = ["/a/one.pdf", "/b/two.pdf", "/c/three.pdf"];
    expect(labelsFor(paths)).toEqual(["one.pdf", "two.pdf", "three.pdf"]);
  });
});

describe("recentCommandId", () => {
  it("shares the prefix the registry replaces by", () => {
    // The two have to agree or the list grows without bound: `replace` removes
    // by prefix, and an id that does not carry it is never removed.
    expect(recentCommandId(0).startsWith(RECENT_PREFIX)).toBe(true);
    expect(recentCommandId(3)).not.toBe(recentCommandId(4));
  });
});
