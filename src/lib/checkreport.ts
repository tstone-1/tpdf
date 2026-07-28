/**
 * The reporting half of an unattended functional check.
 *
 * Shared rather than copied, because the thing it encodes is a bug this
 * repository has already paid an afternoon for and would pay again the first
 * time one harness drifted from the other: **results are printed as they are
 * recorded, and the lines are chained.**
 *
 * Buffering until the end means a run that stops midway prints *nothing*, which
 * is identical to a run that never started and to one whose first line never
 * executed --- and diagnosing which cost most of 2026-07-27. Chaining rather
 * than awaiting at the call site keeps `check` synchronous while stopping the
 * transcript from arriving shuffled, since `invoke` resolves out of order under
 * load.
 */

import { invoke } from "@tauri-apps/api/core";

export type Outcome = "ok" | "fail" | "skip";

const LABEL: Record<Outcome, string> = {
  ok: "[OK]  ",
  fail: "[FAIL]",
  skip: "[SKIP]",
};

/** One check's verdict, kept so the summary can count them. */
export interface Result {
  name: string;
  outcome: Outcome;
  detail: string;
}

/** Collects results and prints each one as it lands. */
export class Report {
  private readonly results: Result[] = [];
  private printing: Promise<unknown> = Promise.resolve();

  /** @param width Column the detail is aligned to. */
  constructor(private readonly width = 46) {}

  /** Prints a line that is not a result. */
  emit(line: string): void {
    this.printing = this.printing.then(() => invoke("spike_print", { text: line }));
  }

  private record(name: string, outcome: Outcome, detail: string): void {
    this.results.push({ name, outcome, detail });
    this.emit(`${LABEL[outcome]} ${name.padEnd(this.width)} ${detail}`);
  }

  check(name: string, ok: boolean, detail: string): void {
    this.record(name, ok ? "ok" : "fail", detail);
  }

  /**
   * Records a check this input cannot exercise.
   *
   * Printed rather than omitted: a control that quietly disappears on some
   * inputs is indistinguishable from one that ran.
   */
  skip(name: string, why: string): void {
    this.record(name, "skip", `not applicable --- ${why}`);
  }

  /** Prints the summary and ends the process with the verdict's exit code. */
  async finish(): Promise<void> {
    const failed = this.results.filter((r) => r.outcome === "fail").length;
    const skipped = this.results.filter((r) => r.outcome === "skip").length;
    const ran = this.results.length - skipped;
    this.emit(
      `\n${ran - failed}/${ran} checks passed` +
        (skipped ? `, ${skipped} not applicable` : ""),
    );
    // The lines went out one at a time; this is where the last is known to have
    // landed. `spike_exit` really does set the exit code --- see AGENTS.md.
    await this.printing;
    await invoke("spike_exit", { code: failed === 0 ? 0 : 1 });
  }
}

/** Resolves on the next animation frame. */
export function frame(): Promise<void> {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

/** Waits a fixed time, for the cases where there is nothing to wait *on*. */
export function pause(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Waits for a condition, returning whether it arrived before the deadline. */
export async function settle(predicate: () => boolean, timeoutMs: number): Promise<boolean> {
  const deadline = performance.now() + timeoutMs;
  while (!predicate()) {
    if (performance.now() > deadline) return false;
    await frame();
  }
  return true;
}
