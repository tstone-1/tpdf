/**
 * What happens to an edit refused before it ran. That `App.svelte` sends the
 * two edits that lost something that way through `ranOr` is held in
 * `recovery.test.ts`, which reads the component's text for the message bar.
 *
 * Checked by mutation: `scripts/mutation_rows/frontend/f10_app_wiring.py`.
 */

import { describe, expect, it } from "vitest";

import { ranOr, typedNotTaken } from "./edithold";
import { PendingImports } from "./pendingimport";

/** `applyEdit` while a task runs: null, and the edit never called. */
const refusing = () => Promise.resolve(null);
/** `applyEdit` when it may: the edit run on the one model. */
const running = <S>(edit: (model: string) => Promise<S>) => edit("model");

describe("an edit refused before it ran", () => {
  it("is reported once, with the refusal's answer", async () => {
    let told = 0;
    await expect(ranOr(refusing, () => Promise.resolve("state"), () => { told++; })).resolves.toBeNull();
    expect(told).toBe(1);
  });

  it("is not reported when the edit ran, whatever it answered", async () => {
    let told = 0;
    await expect(ranOr(running, (model) => Promise.resolve(`${model} after`), () => { told++; }))
      .resolves.toBe("model after");
    // The model refused it: said where the edit runs, which answers null.
    const failing = (edit: (model: string) => Promise<string>) => edit("model").catch(() => null);
    await expect(ranOr(failing, () => Promise.reject(new Error("page gone")), () => { told++; })).resolves.toBeNull();
    expect(told).toBe(0);
  });

  it("releases an insert's prepared file when the edit is refused while a task runs", async () => {
    const released: number[] = [];
    const imports = new PendingImports((_doc, pending) => released.push(pending));
    imports.hold(1, { pending: 7, pages: 3, name: "other.pdf" });
    const waiting = imports.take(1)!;
    await ranOr(refusing, () => Promise.resolve("state"), () => released.push(waiting.pending));
    expect(released).toEqual([7]);
  });
});

describe("words typed in a box that were not taken", () => {
  it("are in the sentence, since the box has closed", () => {
    expect(typedNotTaken("Check the date")).toContain("“Check the date”");
    expect(typedNotTaken("Check the date")).toContain("Not added");
  });

  it("are said as a removal when the box was emptied", () => {
    expect(typedNotTaken("  ")).toContain("Not removed");
  });
});
