import { describe, expect, it } from "vitest";

import { ImportedLinks } from "./importedlinks";
import type { Link } from "./links";
import { PageMap, pageId, unedited, withSources } from "./pages";

/** The opened file's page 0, then page 3 of handle 40 and page 1 of handle 41. */
function twoFiles(): PageMap {
  return new PageMap(
    withSources(
      [
        { id: pageId(1), source: { baseline: 0 }, turns: 0 },
        { id: pageId(2), source: { imported: { source: 1, page: 3 } }, turns: 0 },
        { id: pageId(3), source: { imported: { source: 2, page: 1 } }, turns: 0 },
        { id: pageId(4), source: { imported: { source: 1, page: 0 } }, turns: 0 },
      ],
      [
        { source: 1, doc: 40 },
        { source: 2, doc: 41 },
      ],
    ),
  );
}

describe("ImportedLinks", () => {
  it("asks about each other file once, in the order its pages appear", () => {
    const links = new ImportedLinks();
    expect(links.wanted(twoFiles())).toEqual([40, 41]);
    // Asked, not answered: a scan in flight is not asked about again, and nor
    // is one that failed, or every edit would scan the file once more.
    expect(links.wanted(twoFiles())).toEqual([]);
  });

  it("asks about nothing for a document with no imported page", () => {
    expect(new ImportedLinks().wanted(unedited(4))).toEqual([]);
  });

  it("keeps each answer under the handle it came from", () => {
    const links = new ImportedLinks();
    const one: Link = { id: 0, page: 3, rect: [0, 0, 1, 1], target: { kind: "none" } };
    links.record(41, [one]);
    expect([...links.all.keys()]).toEqual([41]);
    expect(links.all.get(41)).toEqual([one]);
  });

  it("expires outstanding scans on text edits and preserves other pages and files", () => {
    const links = new ImportedLinks();
    const pageMap = twoFiles();
    links.wanted(pageMap);
    const first: Link = { id: 0, page: 0, rect: [0, 0, 1, 1], target: { kind: "none" } };
    const moved = { ...first, page: 3 };
    links.record(40, [first, moved]);
    const revision = links.revision(40);
    const baseline = links.invalidate([{ page: 2 }, { source: 1, page: 3 }], 7, [{ source: 1, doc: 40 }]);
    expect(baseline).toEqual(new Set([2]));
    expect(links.all.get(40)).toEqual([first]);
    expect(links.revision(40)).toBeGreaterThan(revision);
    expect(links.revision(41)).toBe(0);
    expect(links.wanted(pageMap)).toEqual([40]);
  });

  it("asks again after it is cleared", () => {
    // A different document is shown, and its handles are its own.
    const links = new ImportedLinks();
    links.wanted(twoFiles());
    links.record(40, []);
    links.clear();
    expect(links.all.size).toBe(0);
    expect(links.wanted(twoFiles())).toEqual([40, 41]);
  });
});
