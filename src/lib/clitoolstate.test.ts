/**
 * Tests for what greys the command-line tool's two commands.
 *
 * The rule is small; what is pinned is each way it could leave a reader in
 * front of a greyed command that has something to do: an answer kept after the
 * read behind it stopped working, an older answer arriving after a newer one,
 * a command that ran and was not followed by a second look, and a menu that
 * was not told the answer moved.
 *
 * Every test below was checked by mutating `clitoolstate.ts` or `App.svelte`
 * and confirming it went red; `scripts/mutate_frontend.py` has the mutations.
 */

import { describe, expect, it } from "vitest";
import app from "../App.svelte?raw";

import { CommandLineTool, type ToolState } from "./clitoolstate";

/** A read whose answer the test gives when it chooses to. */
function pending() {
  let settle!: (state: ToolState | null) => void;
  let fail!: (why: unknown) => void;
  const promise = new Promise<ToolState | null>((resolve, reject) => {
    settle = resolve;
    fail = reject;
  });
  return { promise, settle, fail };
}

/** The module over a backend that answers what the test queues. */
function harness() {
  const reads: ReturnType<typeof pending>[] = [];
  const log: string[] = [];
  let apply: (install: boolean) => Promise<string> = async () => "done";
  const tool = new CommandLineTool({
    read: () => {
      log.push("read");
      const next = pending();
      reads.push(next);
      return next.promise;
    },
    apply: (install) => {
      log.push(`apply:${install}`);
      return apply(install);
    },
    say: (text) => log.push(`say:${text}`),
    changed: () => log.push(`changed:${tool.offered(true)},${tool.offered(false)}`),
  });
  return {
    tool,
    reads,
    /** The read made by question `index`, which must have been asked. */
    read: (index: number) => {
      const asked = reads[index];
      if (!asked) throw new Error(`question ${index} was never asked`);
      return asked;
    },
    log,
    answerWith: (next: typeof apply) => { apply = next; },
    offered: () => [tool.offered(true), tool.offered(false)],
  };
}

/** Lets every settled promise run its continuations. */
const settled = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

describe("what the command-line tool's commands are greyed by", () => {
  it("offers both until something is known", () => {
    const { offered, reads } = harness();
    expect(offered()).toEqual([true, true]);
    expect(reads).toHaveLength(0);
  });

  it("greys install only when this copy is installed, and uninstall only when nothing is there", async () => {
    for (const [state, expected] of [
      [{ installed: false, occupied: false }, [true, false]],
      // Another copy's link, or somebody else's file: both have something to
      // do or to say.
      [{ installed: false, occupied: true }, [true, true]],
      [{ installed: true, occupied: true }, [false, true]],
    ] as const) {
      const { tool, read, offered } = harness();
      const asked = tool.refresh();
      read(0).settle(state);
      await asked;
      expect(offered()).toEqual(expected);
    }
  });

  it("offers both where there is nothing to read", async () => {
    const { tool, read, offered } = harness();
    const first = tool.refresh();
    read(0).settle({ installed: true, occupied: true });
    await first;
    const second = tool.refresh();
    read(1).settle(null);
    await second;
    expect(offered()).toEqual([true, true]);
  });

  it("forgets the last answer when a read fails, and does not reject", async () => {
    const { tool, read, offered, log } = harness();
    const first = tool.refresh();
    read(0).settle({ installed: true, occupied: false });
    await first;
    expect(offered()).toEqual([false, false]);
    const second = tool.refresh();
    read(1).fail("the PATH could not be read");
    await expect(second).resolves.toBeUndefined();
    expect(offered()).toEqual([true, true]);
    // And the menu was told both times, after the answer was in place.
    expect(log.filter((line) => line.startsWith("changed"))).toEqual([
      "changed:false,false",
      "changed:true,true",
    ]);
  });

  it("keeps the last answer while a slow read is out, and takes only the newest question's answer", async () => {
    const { tool, read, offered, log } = harness();
    const first = tool.refresh();
    read(0).settle({ installed: true, occupied: true });
    await first;

    const slow = tool.refresh();
    expect(offered()).toEqual([false, true]);
    const fast = tool.refresh();
    read(2).settle({ installed: false, occupied: false });
    await fast;
    expect(offered()).toEqual([true, false]);
    // The older question answers last, with what was true before: dropped.
    read(1).settle({ installed: true, occupied: true });
    await slow;
    expect(offered()).toEqual([true, false]);
    expect(log.filter((line) => line.startsWith("changed"))).toHaveLength(2);

    // A late failure of an older question forgets nothing either.
    const older = tool.refresh();
    const newer = tool.refresh();
    read(4).settle({ installed: true, occupied: true });
    await newer;
    read(3).fail("late");
    await older;
    expect(offered()).toEqual([false, true]);
  });

  it("shows what the backend said and then looks again, after a change and after a refusal", async () => {
    for (const [answer, said] of [
      [async () => "Installed.", "say:Installed."],
      [async () => { throw "Nothing was changed: the administrator prompt was cancelled."; },
        "say:Nothing was changed: the administrator prompt was cancelled."],
    ] as const) {
      const { tool, read, log, answerWith, offered } = harness();
      answerWith(answer);
      const ran = tool.run(true);
      await settled();
      // Asked only once the command had answered: a look taken before it
      // finished would read what was there before.
      expect(log).toEqual(["apply:true", said, "read"]);
      read(0).settle({ installed: true, occupied: true });
      await ran;
      expect(offered()).toEqual([false, true]);
      expect(log.at(-1)).toBe("changed:false,true");
    }
  });

  it("passes on which of the two was asked for", async () => {
    const { tool, read, log } = harness();
    const ran = tool.run(false);
    await settled();
    read(0).settle(null);
    await ran;
    expect(log[0]).toBe("apply:false");
  });
});

describe("the tool state's wiring in App.svelte", () => {
  // Source-level, because `App.svelte` is the join and nothing imports it.
  it("asks the backend's read-only command, and runs a command through the module", () => {
    expect(app).toContain('read: () => call("command_line_tool_state"),');
    expect(app).toContain('apply: (install) => call("command_line_tool", { install }),');
    expect(app).toContain("commandLineTool: (install) => void commandLineTool.run(install),");
    expect(app).toContain("say: (text) => (notice = text),");
  });

  it("greys the two commands by the module's answer", () => {
    expect(app).toContain(
      "commandLineToolOffered: (install) => commandLineTool.offered(install),",
    );
  });

  it("re-reads the menu when the answer moves", () => {
    const made = app.indexOf("const commandLineTool = new CommandLineTool({");
    expect(made).toBeGreaterThan(-1);
    expect(app.slice(made, app.indexOf("});", made))).toContain("changed: () => refreshMenu(),");
  });

  it("asks when the window comes to the front", () => {
    expect(app).toContain(
      'window.addEventListener("focus", () => void commandLineTool.refresh());',
    );
  });

  it("asks once at launch, after the first document was asked for and without waiting", () => {
    const opened = app.indexOf("for (const path of plan.show) await openPath(path, plan.resuming);");
    const asked = app.indexOf("      void commandLineTool.refresh();\n");
    expect(opened).toBeGreaterThan(-1);
    expect(asked).toBeGreaterThan(opened);
    // Not awaited anywhere: nothing in the boot waits for the answer.
    expect(app).not.toContain("await commandLineTool.refresh()");
  });
});
