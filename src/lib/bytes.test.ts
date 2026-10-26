import { describe, expect, it } from "vitest";
import { formatBytes } from "./bytes";

describe("a byte count as the window prints it", () => {
  it("prints American English by default", () => {
    expect(formatBytes(512)).toBe("512 bytes");
    expect(formatBytes(9_123_456)).toBe("8.7 MB (9,123,456 bytes)");
    expect(formatBytes(-1)).toBe("unknown");
  });

  it("prints German with a decimal comma and a thousands point", () => {
    expect(formatBytes(512, "de-DE")).toBe("512 Bytes");
    expect(formatBytes(9_123_456, "de-DE")).toBe("8,7 MB (9.123.456 Bytes)");
    expect(formatBytes(894_000, "de-DE")).toBe("873 KB (894.000 Bytes)");
    expect(formatBytes(Number.NaN, "de-DE")).toBe("unbekannt");
  });
});
