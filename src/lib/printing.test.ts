/**
 * The Windows print progress line, and the shape `commands/print.rs` sends it in.
 */

import { describe, expect, it } from "vitest";

import backend from "../../src-tauri/src/commands/print.rs?raw";
import { PROGRESS_EVENT, printingLine, type Printing } from "./printing";

describe("the event is commands/print.rs's", () => {
  it("has the same name", () => {
    expect(backend).toContain(`pub const PROGRESS_EVENT: &str = "${PROGRESS_EVENT}";`);
  });

  it("carries the fields the backend sends, and no other", () => {
    const body = /struct Printing \{([\s\S]*?)\n\}/.exec(backend)?.[1] ?? "";
    const fields = [...body.matchAll(/^\s{4}(\w+):/gm)].map((m) => m[1]);
    const sample: Printing = { sent: 0, of: 1, state: "printing" };
    expect(fields).toEqual(Object.keys(sample));
    for (const state of ["printing", "sent", "failed"]) expect(backend).toContain(`"${state}"`);
  });
});

describe("printingLine", () => {
  it("names the sheet being drawn, counted from 1", () => {
    expect(printingLine({ sent: 0, of: 5, state: "printing" })).toBe("Printing page 1 of 5...");
    expect(printingLine({ sent: 2, of: 5, state: "printing" })).toBe("Printing page 3 of 5...");
    // The last sheet's report arrives before "sent"; it never reads 6 of 5.
    expect(printingLine({ sent: 5, of: 5, state: "printing" })).toBe("Printing page 5 of 5...");
  });

  it("says how many were sent once the printer has them all", () => {
    expect(printingLine({ sent: 5, of: 5, state: "sent" })).toBe("Sent 5 pages to the printer.");
    expect(printingLine({ sent: 1, of: 1, state: "sent" })).toBe("Sent 1 page to the printer.");
  });

  it("clears the line on a failure, which Windows reports in its own message", () => {
    expect(printingLine({ sent: 2, of: 5, state: "failed" })).toBeNull();
  });
});
