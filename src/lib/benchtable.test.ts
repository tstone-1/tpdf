import { describe, expect, it } from "vitest";

import autobench from "./autobench.ts?raw";
import { pad } from "./benchtable";
import scrollbench from "./scrollbench.ts?raw";
import startup from "./startup.ts?raw";

describe("pad", () => {
  it("puts a label on the left and a figure on the right of its column", () => {
    // Mutation: swap `padStart` and `padEnd` -> red.
    expect(pad("ms", 5)).toBe("ms   ");
    expect(pad("12.5", 7, true)).toBe("   12.5");
    // Control: text as wide as the column, or wider, is left as it is.
    expect(pad("variant", 4)).toBe("variant");
  });

  it("is the one every benchmark table uses", () => {
    for (const source of [autobench, scrollbench, startup]) {
      expect(source).toContain('import { pad } from "./benchtable";');
      expect(source).not.toMatch(/function pad\(/);
    }
  });
});
