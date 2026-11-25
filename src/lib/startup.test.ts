import { expect, it, vi } from "vitest";
import { navigationMarks } from "./startup";

it("times the document's entry script even when a later chunk finishes last", () => {
  const entry = "https://example.org/assets/index-12.js";
  const select = vi.fn().mockReturnValue({ src: entry });
  vi.stubGlobal("document", { querySelector: select });
  vi.stubGlobal("performance", {
    getEntriesByType: (kind: string) => kind === "resource" ? [
      { name: entry, startTime: 4, responseEnd: 12 },
      { name: "https://example.org/assets/startup-34.js", startTime: 16, responseEnd: 30 },
    ] : [],
  });
  try {
    expect(navigationMarks()).toEqual([["module requested", 4], ["module received", 12]]);
    expect(select).toHaveBeenCalledWith('script[type="module"][src]');
  } finally { vi.unstubAllGlobals(); }
});
