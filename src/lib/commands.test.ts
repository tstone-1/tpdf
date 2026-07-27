/**
 * Tests for the command registry and its ranking.
 *
 * These are the first tests on the front end, and they exist because ranking is
 * the one piece here that is pure logic with an answer that can be *wrong*
 * rather than merely ugly --- the viewer's behaviour is asserted in a real
 * webview by `viewercheck.ts`, which is the right tool for a scroll and the
 * wrong one for "does `fw` find Fit width".
 *
 * Every test below was checked by mutating the code it covers and confirming it
 * went red. Assertions are on *ordering* rather than on scores wherever
 * possible: a score is an implementation detail that can be retuned, and a test
 * that pins one would have to be rewritten every time the tuning changes, which
 * is how a suite stops being run.
 */

import { describe, expect, it } from "vitest";
import { CommandRegistry, fuzzyMatch, rank, type Command } from "./commands";

/** A command that records whether it ran. */
function command(id: string, title: string, extra: Partial<Command> = {}): Command {
  return { id, title, run: () => {}, ...extra };
}

/** Titles of a ranked list, in order. */
function titles(ranked: ReturnType<typeof rank>): string[] {
  return ranked.map((r) => r.command.title);
}

describe("fuzzyMatch", () => {
  it("matches characters that are not adjacent", () => {
    const hit = fuzzyMatch("zi", "Zoom in");
    expect(hit).not.toBeNull();
    expect(hit?.positions).toEqual([0, 5]);
  });

  it("reports where it matched, so the highlight cannot disagree", () => {
    const hit = fuzzyMatch("width", "Fit width");
    expect(hit?.positions).toEqual([4, 5, 6, 7, 8]);
  });

  it("requires the characters in order", () => {
    expect(fuzzyMatch("iz", "Zoom in")).toBeNull();
  });

  it("fails when a character is absent", () => {
    expect(fuzzyMatch("zq", "Zoom in")).toBeNull();
  });

  it("ignores case in both directions", () => {
    expect(fuzzyMatch("ZI", "Zoom in")).not.toBeNull();
    expect(fuzzyMatch("zi", "ZOOM IN")).not.toBeNull();
  });

  it("scores a word start above the same letter mid-word", () => {
    // The second title is not a plausible command, deliberately: it puts the
    // `w` at the same index as the first, so the *only* difference between the
    // two scores is whether a word starts there. A realistic pair would differ
    // in position as well, and then this would be measuring two things --- the
    // mistake the consecutive test below started as.
    const wordStart = fuzzyMatch("w", "Fit width")?.score ?? 0;
    const midWord = fuzzyMatch("w", "Fitxwidth")?.score ?? 0;
    expect(wordStart).toBeGreaterThan(midWord);
  });

  it("scores a consecutive run above a scattered one", () => {
    // Both titles put the first `o` at index 1, so adjacency is the only thing
    // that differs. Written first as "Zoom in" against "Open document", where
    // the scattered one *won* --- its match starts at index 0 and at a word
    // boundary, which are worth more than the run. Two effects, one comparison,
    // and the test was measuring the wrong one.
    const consecutive = fuzzyMatch("oo", "Zoom")?.score ?? 0;
    const scattered = fuzzyMatch("oo", "Go to page")?.score ?? 0;
    expect(consecutive).toBeGreaterThan(scattered);
  });

  it("treats an empty query as a match on everything", () => {
    const hit = fuzzyMatch("", "Zoom in");
    expect(hit).toEqual({ score: 0, positions: [] });
  });
});

describe("rank", () => {
  const commands = [
    command("view.fitWidth", "Fit width"),
    command("find.next", "Find next"),
    command("edit.copy", "Copy"),
  ];

  it("finds the command whose words the query abbreviates", () => {
    expect(titles(rank("fw", commands))[0]).toBe("Fit width");
  });

  it("excludes a command that does not match at all", () => {
    expect(titles(rank("zzz", commands))).toEqual([]);
  });

  it("returns everything in registration order for an empty query", () => {
    expect(titles(rank("", commands))).toEqual(["Fit width", "Find next", "Copy"]);
  });

  it("lifts recents to the front of an empty query", () => {
    expect(titles(rank("", commands, ["edit.copy"]))).toEqual([
      "Copy",
      "Fit width",
      "Find next",
    ]);
  });

  it("never offers a disabled command", () => {
    const guarded = [
      command("edit.copy", "Copy", { enabled: () => false }),
      command("view.fitWidth", "Fit width"),
    ];
    expect(titles(rank("", guarded))).toEqual(["Fit width"]);
    expect(titles(rank("copy", guarded))).toEqual([]);
  });

  it("breaks a tie towards the more recent command", () => {
    const tied = [command("a.find", "Find"), command("b.find", "Find")];
    expect(rank("find", tied, ["b.find"])[0]?.command.id).toBe("b.find");
  });

  it("does not let recency beat a better match", () => {
    // "Copy" is recent, but `f` starts a word in "Fit width" and only appears
    // mid-word in "Copy" -- typing something specific has to win over history,
    // or the palette stops answering what was asked.
    const pair = [command("view.fitWidth", "Fit width"), command("edit.copy", "Coffee")];
    expect(titles(rank("f", pair, ["edit.copy"]))[0]).toBe("Fit width");
  });
});

describe("CommandRegistry", () => {
  it("runs a command by id", () => {
    let ran = 0;
    const registry = new CommandRegistry();
    registry.register(command("a", "A", { run: () => void ran++ }));
    expect(registry.run("a")).toBe(true);
    expect(ran).toBe(1);
  });

  it("refuses an unknown id rather than throwing", () => {
    expect(new CommandRegistry().run("nope")).toBe(false);
  });

  it("refuses a disabled command, so a stale shortcut does nothing", () => {
    let ran = 0;
    const registry = new CommandRegistry();
    registry.register(
      command("a", "A", { enabled: () => false, run: () => void ran++ }),
    );
    expect(registry.run("a")).toBe(false);
    expect(ran).toBe(0);
  });

  it("remembers what was run, most recent first and without duplicates", () => {
    const registry = new CommandRegistry();
    registry.register(command("a", "A"), command("b", "B"));
    registry.run("a");
    registry.run("b");
    registry.run("a");
    expect(registry.recents()).toEqual(["a", "b"]);
  });

  it("keeps the recents list short", () => {
    const registry = new CommandRegistry();
    const ids = ["a", "b", "c", "d", "e", "f", "g"];
    registry.register(...ids.map((id) => command(id, id.toUpperCase())));
    for (const id of ids) registry.run(id);
    expect(registry.recents()).toEqual(["g", "f", "e", "d", "c"]);
  });
});
