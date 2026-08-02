/**
 * The transcript format {@link Report} prints, pinned against its readers.
 *
 * Four Python harnesses parse these lines. `scripts/session_check.py` and
 * `scripts/open_check.py` read the summary's arithmetic and look up individual
 * verdicts by name; `scripts/viewer_check.py` counts them; and
 * `scripts/mutate_viewer.py` decides whether a mutation was *caught* by reading
 * which names went red. None of that is reachable from `npm run test`: they need
 * a built application and, for three of them, an unlocked screen. So a format
 * change is discovered at the end of a rebuild-per-mutation run, as a harness
 * that finds nothing --- which is exactly what a clean tree also looks like.
 *
 * The patterns below are transcribed from those scripts and asserted against
 * real output, so the drift goes red here first, in a second.
 *
 * What is pinned is the **contract**: the marker each line begins with, the
 * separator between a name and its detail, the summary's wording and its
 * arithmetic, the exit code, and that a passing transcript contains no `[FAIL]`
 * anywhere. What is deliberately *not* pinned is the column the detail lands in.
 * Encoding that here would recreate the padded-column parse those harnesses were
 * rewritten to avoid --- and a test that goes red for a retune and green for a
 * broken marker is the wrong way round.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { Report } from "./checkreport";

const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);

/**
 * `MARKER` in `scripts/mutate_viewer.py`, `RESULT` in `scripts/session_check.py`.
 *
 * Both split a result line into its label and everything after it; the name is
 * then matched as a *prefix* of the second group rather than sliced out of a
 * column.
 */
const RESULT = /^\[(OK|FAIL|SKIP)\]\s+(.*)$/;

/** `SUMMARY` in all three of the harnesses that read a whole run. */
const SUMMARY = /^(\d+)\/(\d+) checks passed/;

/** One `invoke` call: the command, and the arguments object. */
type Call = [string, Record<string, unknown>];

/** Every line handed to the process so far, in order. */
function lines(): string[] {
  const out: string[] = [];
  for (const [command, args] of core.invoke.mock.calls as Call[]) {
    // `spike_print` is `println!`, so an embedded newline is a line break.
    if (command === "spike_print") out.push(...String(args.text).split("\n"));
  }
  return out;
}

/** The code the run asked to exit with, or null if it never asked. */
function exitCode(): number | null {
  for (const [command, args] of core.invoke.mock.calls as Call[]) {
    if (command === "spike_exit") return Number(args.code);
  }
  return null;
}

/** The result lines, without the blank line and the summary. */
function results(): string[] {
  return lines().filter((line) => RESULT.test(line));
}

/** Lets every queued microtask and timer callback run. */
function drain(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  core.invoke.mockReset();
});

describe("the line a result is printed as", () => {
  it("begins with the marker every harness splits on", async () => {
    const report = new Report();
    report.check("a check that passed", true, "detail");
    report.check("a check that failed", false, "detail");
    report.skip("a check that could not run", "the document has one page");
    await report.finish();

    expect(results()).toHaveLength(3);
    expect(results().map((line) => RESULT.exec(line)?.[1])).toEqual([
      "OK",
      "FAIL",
      "SKIP",
    ]);
  });

  it("puts the name first after the marker, so a prefix match finds it", async () => {
    const report = new Report();
    report.check("zoom follows the width", true, "1.0x");
    report.skip("the strip yields", "nothing to yield to");
    await report.finish();

    // `outcome_of` in session_check.py, and `caught` in mutate_viewer.py.
    const after = results().map((line) => RESULT.exec(line)?.[2] ?? "");
    expect(after[0]?.startsWith("zoom follows the width")).toBe(true);
    expect(after[1]?.startsWith("the strip yields")).toBe(true);
  });

  it("separates a name from its detail even when the name is past the pad", async () => {
    // The trap the harnesses were rewritten for: `padEnd` does not truncate, so
    // a long name is followed by a single space rather than by the column. A
    // name run together with its detail would still match the marker, and would
    // silently stop matching every expectation that names it.
    const long = "a check whose name is longer than any column".repeat(2);
    const report = new Report();
    report.check(long, true, "detail");
    await report.finish();

    const after = RESULT.exec(results()[0] ?? "")?.[2] ?? "";
    expect(after.startsWith(long)).toBe(true);
    expect(after.slice(long.length)).toBe(" detail");
  });

  it("says a skipped check is not applicable, in ASCII", async () => {
    const report = new Report();
    report.skip("a check", "there is no second page");
    await report.finish();

    // The padding is collapsed rather than reproduced: the wording is the
    // contract, the column is a formatting choice nobody should have to know.
    const after = RESULT.exec(results()[0] ?? "")?.[2] ?? "";
    expect(after.replace(/ {2,}/g, " ")).toBe(
      "a check not applicable --- there is no second page",
    );
    // Every line the report writes itself lands on a Windows console that is
    // cp1252, where a character outside it raises rather than prints.
    for (const line of lines()) expect(line).toMatch(/^[\x20-\x7e]*$/);
  });
});

describe("the summary", () => {
  it("counts only the checks that ran, and names the rest", async () => {
    const report = new Report();
    report.check("one", true, "");
    report.check("two", true, "");
    report.check("three", false, "");
    report.skip("four", "not this document");
    await report.finish();

    const summary = lines().find((line) => SUMMARY.test(line)) ?? "";
    expect(summary).toBe("2/3 checks passed, 1 not applicable");
    const found = SUMMARY.exec(summary);
    expect([found?.[1], found?.[2]]).toEqual(["2", "3"]);
  });

  it("says nothing about skips when there were none", async () => {
    const report = new Report();
    report.check("one", true, "");
    await report.finish();

    expect(lines().find((line) => SUMMARY.test(line))).toBe("1/1 checks passed");
  });

  it("lands at the start of a line, which is where the patterns anchor", async () => {
    const report = new Report();
    report.check("one", true, "");
    await report.finish();

    // All three harnesses anchor `^`; two of them apply it per line and the
    // third with `re.M`. Either way a summary appended to a result line is
    // invisible to them.
    const at = lines().findIndex((line) => SUMMARY.test(line));
    expect(at).toBeGreaterThan(0);
    expect(lines()[at]).toMatch(SUMMARY);
  });
});

describe("what a passing run must not contain", () => {
  it("prints no [FAIL] anywhere when nothing failed", async () => {
    const report = new Report();
    report.check("one", true, "detail");
    report.skip("two", "not this document");
    await report.finish();

    // `open_check.py` reads a phase as green with a plain substring test:
    // `SUMMARY.search(text) and "[FAIL]" not in text`.
    expect(lines().join("\n")).not.toContain("[FAIL]");
  });

  it("prints [FAIL] when something did", async () => {
    const report = new Report();
    report.check("one", false, "detail");
    await report.finish();

    expect(lines().join("\n")).toContain("[FAIL]");
  });
});

describe("the exit code", () => {
  it("is 1 when a check failed", async () => {
    const report = new Report();
    report.check("one", true, "");
    report.check("two", false, "");
    await report.finish();

    expect(exitCode()).toBe(1);
  });

  it("is 0 when none did, skips included", async () => {
    // The control. A code that is always 1 fails the test above's twin, and a
    // code that is always 0 passes this one --- neither says anything alone.
    const report = new Report();
    report.check("one", true, "");
    report.skip("two", "not this document");
    await report.finish();

    expect(exitCode()).toBe(0);
  });

  it("is asked for after the last line has landed", async () => {
    const report = new Report();
    report.check("one", true, "");
    await report.finish();

    const commands = (core.invoke.mock.calls as Call[]).map(([command]) => command);
    expect(commands[commands.length - 1]).toBe("spike_exit");
    expect(commands.filter((command) => command === "spike_exit")).toHaveLength(1);
  });
});

describe("the printing chain", () => {
  it("does not hand a line over until the one before it has landed", async () => {
    // Why the chain exists: `invoke` resolves out of order under load, and a
    // shuffled transcript is worse than a late one. Fire-and-forget would issue
    // both calls at once, which is what this refuses.
    let land = (): void => {};
    core.invoke.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          land = resolve;
        }),
    );

    const report = new Report();
    report.check("first", true, "");
    report.check("second", true, "");
    await drain();
    expect(core.invoke).toHaveBeenCalledTimes(1);

    land();
    await drain();
    expect(core.invoke).toHaveBeenCalledTimes(2);
  });

  it("prints a result as it is recorded, without waiting for the summary", async () => {
    // A run that stops midway prints what it reached. Buffering until `finish`
    // makes a partial run indistinguishable from one that never started, which
    // is the failure this module was extracted to stop repeating.
    const report = new Report();
    report.check("first", true, "");
    await drain();

    expect(results()).toHaveLength(1);
    expect(exitCode()).toBeNull();
  });
});
