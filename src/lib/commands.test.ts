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

/**
 * A command that records whether it ran.
 *
 * `extra` names the fields explicitly rather than `Partial<Command>`, which
 * stopped type-checking when `Command` became a union: a partial of a union
 * admits an object with neither `run` nor `argument`, which is the shape the
 * union exists to forbid.
 */
function command(
  id: string,
  title: string,
  extra: { keys?: string; enabled?: () => boolean; run?: () => void } = {},
): Command {
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

/** A command that takes a value, recording every value it was run with. */
function pageCommand(ran: string[], pages = 10): Command {
  return {
    id: "nav.goToPage",
    title: "Go to page…",
    argument: {
      placeholder: "Page number",
      problem: (raw) => {
        const trimmed = raw.trim();
        if (trimmed === "") return `Page number, 1 to ${pages}`;
        if (!/^[0-9]+$/.test(trimmed)) return `"${trimmed}" is not a page number`;
        const page = Number(trimmed);
        return page < 1 || page > pages ? `This document has ${pages} pages` : null;
      },
      preview: (raw) => `Go to page ${Number(raw.trim())} of ${pages}`,
      run: (raw) => void ran.push(raw),
    },
  };
}

describe("commands that take an argument", () => {
  it("runs with the value it was given", () => {
    const ran: string[] = [];
    const registry = new CommandRegistry();
    registry.register(pageCommand(ran));
    expect(registry.run("nav.goToPage", "7")).toBe(true);
    expect(ran).toEqual(["7"]);
  });

  it("refuses to run without one", () => {
    // The shape the union forbids at compile time, arriving at run time from a
    // caller that has only an id --- a keybinding, a restored session.
    const ran: string[] = [];
    const registry = new CommandRegistry();
    registry.register(pageCommand(ran));
    expect(registry.run("nav.goToPage")).toBe(false);
    expect(ran).toEqual([]);
  });

  it("refuses a value its own check rejects", () => {
    // Checked in the registry rather than trusted to the palette. The palette
    // will not offer a bad value; the caller that skips that check is the one
    // written later.
    const ran: string[] = [];
    const registry = new CommandRegistry();
    registry.register(pageCommand(ran, 10));
    expect(registry.run("nav.goToPage", "11")).toBe(false);
    expect(registry.run("nav.goToPage", "nine")).toBe(false);
    expect(registry.run("nav.goToPage", "")).toBe(false);
    expect(ran).toEqual([]);
  });

  it("refuses a value for a command that takes none", () => {
    let ran = 0;
    const registry = new CommandRegistry();
    registry.register(command("view.zoomIn", "Zoom in", { run: () => void ran++ }));
    expect(registry.run("view.zoomIn", "3")).toBe(false);
    expect(ran).toBe(0);
  });

  it("does not record a refused command as recent", () => {
    // Otherwise a shortcut pressed with no value would push the command to the
    // top of the palette's list without ever having run.
    const ran: string[] = [];
    const registry = new CommandRegistry();
    registry.register(pageCommand(ran));
    registry.run("nav.goToPage");
    expect(registry.recents()).toEqual([]);
    registry.run("nav.goToPage", "3");
    expect(registry.recents()).toEqual(["nav.goToPage"]);
  });

  it("finds a command by id", () => {
    const registry = new CommandRegistry();
    registry.register(command("a", "A"));
    expect(registry.find("a")?.title).toBe("A");
    expect(registry.find("b")).toBeUndefined();
  });

  it("explains an empty value rather than saying nothing", () => {
    // The palette asks before anything is typed, so the empty case is the
    // first thing a reader sees and has to read as instructions.
    const spec = pageCommand([], 775).argument;
    expect(spec?.problem("")).toBe("Page number, 1 to 775");
    expect(spec?.problem("400")).toBeNull();
    expect(spec?.preview("400")).toBe("Go to page 400 of 775");
  });
});

describe("replace", () => {
  /** A registry holding two fixed commands and a replaceable group. */
  function registry(): { reg: CommandRegistry; ran: string[] } {
    const ran: string[] = [];
    const reg = new CommandRegistry();
    reg.register(
      { id: "view.zoomIn", title: "Zoom in", run: () => void ran.push("view.zoomIn") },
      { id: "file.open", title: "Open…", run: () => void ran.push("file.open") },
    );
    return { reg, ran };
  }

  it("swaps the group and leaves everything else alone", () => {
    const { reg } = registry();
    reg.replace("file.recent.", [
      { id: "file.recent.0", title: "Open a.pdf", run: () => {} },
      { id: "file.recent.1", title: "Open b.pdf", run: () => {} },
    ]);
    reg.replace("file.recent.", [
      { id: "file.recent.0", title: "Open c.pdf", run: () => {} },
    ]);
    expect(reg.all().map((c) => c.id)).toEqual([
      "view.zoomIn",
      "file.open",
      "file.recent.0",
    ]);
    expect(reg.find("file.recent.0")?.title).toBe("Open c.pdf");
  });

  it("does not remove a command whose id merely contains the prefix", () => {
    // `startsWith`, not `includes`: an id is a path from general to specific and
    // a prefix match is the only one that means "this group".
    const { reg } = registry();
    reg.register({ id: "x.file.recent.0", title: "Decoy", run: () => {} });
    reg.replace("file.recent.", []);
    expect(reg.find("x.file.recent.0")).toBeDefined();
  });

  it("forgets that a replaced command was recent", () => {
    // These ids are reused for different documents, so a stale entry would rank
    // the *new* document at position 3 because the *old* one was run.
    const { reg } = registry();
    reg.replace("file.recent.", [
      { id: "file.recent.0", title: "Open a.pdf", run: () => {} },
    ]);
    reg.run("file.recent.0");
    expect(reg.recents()).toContain("file.recent.0");
    reg.replace("file.recent.", [
      { id: "file.recent.0", title: "Open b.pdf", run: () => {} },
    ]);
    expect(reg.recents()).not.toContain("file.recent.0");
  });

  it("leaves the recents of commands it did not replace", () => {
    // The control: a `recent` list cleared wholesale would satisfy the test
    // above and lose the ranking the palette exists to provide.
    const { reg } = registry();
    reg.run("view.zoomIn");
    reg.replace("file.recent.", []);
    expect(reg.recents()).toContain("view.zoomIn");
  });
});

