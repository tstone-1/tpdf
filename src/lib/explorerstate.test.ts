import { describe, expect, it } from "vitest";
import { ExplorerIntegration, replaceQuestion, type ExplorerState } from "./explorerstate";

const edge = { kind: "other", name: "PDF Preview Handler" } as const;
/** TEENYWEENY as measured: no thumbnail provider, Edge's preview handler. */
const fresh: ExplorerState = { thumbnail: { kind: "nobody" }, preview: edge };

function harness(states: (ExplorerState | null | Error)[], answer = true) {
  const pick = (i: number): ExplorerState | null | Error => states[Math.min(i, states.length - 1)] ?? null;
  const log: string[] = [];
  let next = 0;
  const integration = new ExplorerIntegration({
    read: () => {
      const state = pick(next++);
      return state instanceof Error ? Promise.reject(state) : Promise.resolve(state);
    },
    apply: (on, replace) => {
      log.push(`apply ${on} ${replace}`);
      return Promise.resolve(on ? "on" : "off");
    },
    confirm: (question) => {
      log.push(`ask ${question}`);
      return Promise.resolve(answer);
    },
    say: (text) => log.push(`say ${text}`),
    changed: () => log.push("changed"),
  });
  return { integration, log };
}

describe("Explorer integration", () => {
  it("offers both before the first answer, and neither off Windows", async () => {
    const { integration } = harness([null]);
    expect([integration.offered(true), integration.offered(false)]).toEqual([true, true]);
    await integration.refresh();
    expect([integration.offered(true), integration.offered(false)]).toEqual([false, false]);
  });

  it("offers turning on while a slot is not tpdf's, and off while one is", async () => {
    const half: ExplorerState = { thumbnail: { kind: "tpdf" }, preview: edge };
    const all: ExplorerState = { thumbnail: { kind: "tpdf" }, preview: { kind: "tpdf" } };
    const { integration } = harness([fresh, half, all]);
    await integration.refresh();
    expect([integration.offered(true), integration.offered(false)]).toEqual([true, false]);
    await integration.refresh();
    expect([integration.offered(true), integration.offered(false)]).toEqual([true, true]);
    await integration.refresh();
    expect([integration.offered(true), integration.offered(false)]).toEqual([false, true]);
  });

  it("a failed read is not known, and offers both", async () => {
    const { integration } = harness([fresh, new Error("registry")]);
    await integration.refresh();
    expect(integration.offered(false)).toBe(false);
    await integration.refresh();
    expect([integration.offered(true), integration.offered(false)]).toEqual([true, true]);
  });

  it("asks before replacing another program's handler and passes the answer on", async () => {
    for (const answer of [true, false]) {
      const { integration, log } = harness([fresh], answer);
      await integration.run(true);
      expect(log.filter((l) => l.startsWith("ask")).length).toBe(1);
      expect(log).toContain(`apply true ${answer}`);
      expect(log).toContain("say on");
    }
  });

  it("does not ask when nothing would be replaced, nor when turning off", async () => {
    const free: ExplorerState = { thumbnail: { kind: "nobody" }, preview: { kind: "tpdf" } };
    const { integration, log } = harness([free]);
    await integration.run(true);
    await integration.run(false);
    expect(log.some((l) => l.startsWith("ask"))).toBe(false);
    expect(log).toContain("apply true false");
    expect(log).toContain("apply false false");
  });

  it("shows a refusal the way it shows a success, and reads again after either", async () => {
    const { integration, log } = harness([fresh]);
    const refusing = new ExplorerIntegration({
      read: () => Promise.resolve(fresh),
      apply: () => Promise.reject("tpdf_shell.dll is not there"),
      confirm: () => Promise.resolve(false),
      say: (text) => log.push(`say ${text}`),
      changed: () => log.push("changed"),
    });
    await refusing.run(true);
    expect(log).toEqual(["say tpdf_shell.dll is not there", "changed"]);
    void integration;
  });

  it("names what would be replaced, one or both", () => {
    expect(replaceQuestion(fresh)).toBe(
      "In Explorer, the preview pane comes from PDF Preview Handler. Replace it with tpdf? " +
        "Turning tpdf's off later puts back what it replaced.",
    );
    const both = replaceQuestion({ thumbnail: { kind: "other", name: "Acrobat" }, preview: edge });
    expect(both).toContain("PDF thumbnails come from Acrobat, and the preview pane comes from PDF Preview Handler");
    expect(both).toContain("Replace them");
    expect(replaceQuestion({ thumbnail: { kind: "tpdf" }, preview: { kind: "nobody" } })).toBeNull();
  });
});
