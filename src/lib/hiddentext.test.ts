/**
 * What the window says about text the pages do not show, and the names this
 * side shares with the backend by spelling.
 *
 * The sentence a check ends on is the backend's and is sent, so there is no
 * second copy of it here to hold equal. What is held is what this side adds
 * around it: that nothing is said without what was never looked at, and that a
 * passage is found again on a page the reader has since moved.
 */

import { describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import backend from "../../src-tauri/src/commands/hidden.rs?raw";
import readme from "../../README.md?raw";
import survey from "../../src-tauri/src/hidden/survey.rs?raw";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";
import {
  MAX_ROWS,
  NOT_CHECKED,
  PAGE_GONE,
  placeOf,
  PROGRESS_EVENT,
  progressLine,
  rowLabel,
  Runs,
  sentences,
  showPassage,
  UNSAVED,
  type CheckHost,
  type HiddenText,
  type Passage,
} from "./hiddentext";
import { functionIn, missingFrom } from "./sourcetext";

function passage(over: Partial<Passage> = {}): Passage {
  return { page: 3, text: "Jane Example", rect: [60, 100, 140, 112], characters: 11, offPage: false, ...over };
}

/** The backend's sentence about what it never looks at, as the source has it. */
function notLookedAt(): string | undefined {
  return /pub const NOT_LOOKED_AT: &str = "([^;]+)";/.exec(survey)?.[1]?.replace(/\\\n\s*/g, "");
}

function checked(over: Partial<HiddenText> = {}): HiddenText {
  return {
    found: [],
    summary: "No hidden text found: 1200 characters on 9 pages compared with the rendered page.",
    notLookedAt: notLookedAt() ?? "",
    unsaved: false,
    ...over,
  };
}

describe("what the window shares with the backend by spelling", () => {
  it("listens for the event the backend emits", () => {
    const declared = /pub const PROGRESS_EVENT: &str = "([^"]+)";/.exec(backend)?.[1];
    // The control: a pattern that stopped matching would compare against
    // `undefined`, which no event name equals.
    expect(declared).toBeDefined();
    expect(PROGRESS_EVENT).toBe(declared);
  });

  it("is sent the limits the README states for the command-line tool", () => {
    // One list of what the check never looks at, in three places: the backend
    // sends it, and the README says it of `tpdf hidden`.
    const said = notLookedAt();
    expect(said).toBeDefined();
    const list = /^Not looked at: (.+)\.$/.exec(said ?? "")?.[1];
    expect(list).toBeDefined();
    expect(readme.replace(/\s+/g, " ")).toContain(list);
  });

  it("names the command the panel tells a reader to run", () => {
    const registry = new CommandRegistry();
    const actions = new Proxy({}, { get: () => () => true }) as AppActions;
    registerAppCommands(registry, actions);
    const title = registry.all().find((entry) => entry.id === "file.findHiddenText")?.title;
    expect(title).toBeDefined();
    expect(NOT_CHECKED).toContain(`“${title}”`);
  });
});

describe("the line shown while pages are compared", () => {
  it("says which page of how many", () => {
    expect(progressLine({ page: 3, of: 12 })).toBe(
      "Comparing text with the pages: page 3 of 12...",
    );
  });
});

describe("what a result says above its rows", () => {
  it("never says nothing was found without what was not looked at", () => {
    const said = sentences(checked());
    expect(said).toEqual([
      "No hidden text found: 1200 characters on 9 pages compared with the rendered page.",
      "Not looked at: comments, form values, attachments, metadata and earlier versions kept in the file.",
    ]);
  });

  it("says that unsaved changes were not part of it, only when there were any", () => {
    expect(sentences(checked({ unsaved: true }))).toEqual([
      checked().summary,
      UNSAVED,
      checked().notLookedAt,
    ]);
    expect(sentences(checked())).not.toContain(UNSAVED);
  });

  it("says how many passages the list leaves out, only when it leaves any out", () => {
    const many = (count: number) =>
      checked({ found: Array.from({ length: count }, () => passage()), summary: "found" });
    expect(sentences(many(MAX_ROWS))).toEqual(["found", checked().notLookedAt]);
    expect(sentences(many(MAX_ROWS + 1))).toEqual([
      "found",
      `Showing the first ${MAX_ROWS} of ${MAX_ROWS + 1} passages.`,
      checked().notLookedAt,
    ]);
  });
});

describe("a passage's row", () => {
  it("names its page, and says when the words are outside it", () => {
    expect(rowLabel(passage())).toBe("Page 3");
    expect(rowLabel(passage({ offPage: true }))).toBe("Page 3, outside the page");
  });
});

describe("where a passage is in the document being read", () => {
  /** A document whose first page was deleted: file page `n` is in slot `n - 1`. */
  const shifted = (filePage: number) => (filePage === 0 ? undefined : filePage - 1);

  it("is the slot showing its page of the file, which counts from 0 there", () => {
    const asked: number[] = [];
    const place = placeOf(passage(), (filePage) => {
      asked.push(filePage);
      return filePage;
    });
    expect(asked).toEqual([2]);
    expect(place).toEqual({ slot: 2, rect: [60, 100, 140, 112] });
    // And not the page number as a slot: after a deletion the two differ.
    expect(placeOf(passage(), shifted)).toEqual({ slot: 1, rect: [60, 100, 140, 112] });
  });

  it("is nowhere once the page has been deleted", () => {
    expect(placeOf(passage({ page: 1 }), shifted)).toBeNull();
    expect(PAGE_GONE).toContain("no longer");
  });

  it("is the page alone for words outside it", () => {
    expect(placeOf(passage({ offPage: true, rect: [-400, 500, -300, 512] }), shifted)).toEqual({
      slot: 1,
      rect: null,
    });
  });
});

describe("which check Stop is for", () => {
  it("numbers each run from 1 and keeps the number after the run ends", () => {
    const runs = new Runs();
    expect([runs.running, runs.number]).toEqual([false, 0]);
    expect(runs.start()).toBe(1);
    expect([runs.running, runs.number]).toEqual([true, 1]);
    runs.finish();
    expect([runs.running, runs.number]).toEqual([false, 1]);
    // A second run is not the first: a stop sent for one must not stop the other.
    expect(runs.start()).toBe(2);
    expect(runs.number).toBe(2);
  });

  it("shows a page's line only while a check is running", () => {
    const runs = new Runs();
    const at = { page: 2, of: 5 };
    expect(runs.line(at)).toBeNull();
    runs.start();
    expect(runs.line(at)).toBe(progressLine(at));
    runs.finish();
    // An event that lands after the answer leaves no line behind.
    expect(runs.line(at)).toBeNull();
  });
});

describe("going to a passage", () => {
  const shifted = (filePage: number) => (filePage === 0 ? undefined : filePage - 1);
  const went = (which: Passage) => {
    const log: unknown[] = [];
    showPassage(which, shifted, {
      ring: (slot, rect) => log.push(["ring", slot, rect]),
      page: (slot) => log.push(["page", slot]),
      gone: () => log.push(["gone"]),
    });
    return log;
  };

  it("rings a passage on its page, in the slot the page is shown in", () => {
    expect(went(passage())).toEqual([["ring", 1, [60, 100, 140, 112]]]);
  });

  it("shows the page alone for words outside it", () => {
    expect(went(passage({ offPage: true, rect: [-400, 500, -300, 512] }))).toEqual([["page", 1]]);
  });

  it("says so, and goes nowhere, once the page has been deleted", () => {
    expect(went(passage({ page: 1 }))).toEqual([["gone"]]);
  });
});

describe("one check, from its number to its answer", () => {
  /** A host that records what it is asked, in order. */
  function host(over: Partial<CheckHost> = {}) {
    const log: string[] = [];
    const answer = checked();
    const made: CheckHost = {
      begun: async () => { log.push("begun"); },
      ask: async (run) => { log.push(`ask ${run}`); return answer; },
      current: () => { log.push("current?"); return true; },
      show: (shown) => { log.push(shown === answer ? "show" : "show another"); },
      ...over,
    };
    return { log, made };
  }

  it("is numbered before the window shows it has begun, and sent under that number", async () => {
    const runs = new Runs();
    runs.start();
    runs.finish();
    const stopped: number[] = [];
    const { log, made } = host({
      begun: async () => {
        // Stop is on screen from here on: a press now names the run about to be sent.
        expect(runs.running).toBe(true);
        runs.stop((run) => stopped.push(run));
      },
    });
    await runs.check(made);
    expect(stopped).toEqual([2]);
    expect(log).toEqual(["ask 2", "current?", "show"]);
    expect(runs.running).toBe(false);
  });

  it("drops an answer about a document that is no longer the one in the window", async () => {
    const runs = new Runs();
    const { log, made } = host({ current: () => false });
    await runs.check(made);
    expect(log).toEqual(["begun", "ask 1"]);
    expect(runs.running).toBe(false);
  });

  it("ends the run when the check is refused, and passes the refusal on", async () => {
    const runs = new Runs();
    const { log, made } = host({ ask: async () => { throw new Error("stopped"); } });
    await expect(runs.check(made)).rejects.toThrow("stopped");
    expect(log).toEqual(["begun"]);
    expect(runs.running).toBe(false);
    // An event that lands after it leaves no line behind.
    const lines: string[] = [];
    runs.report({ page: 1, of: 2 }, (line) => lines.push(line));
    expect(lines).toEqual([]);
  });

  it("hands a page's line on only while a check is running", async () => {
    const runs = new Runs();
    const lines: string[] = [];
    const at = { page: 2, of: 5 };
    const { made } = host({ begun: async () => runs.report(at, (line) => lines.push(line)) });
    runs.report(at, (line) => lines.push(line));
    await runs.check(made);
    runs.report(at, (line) => lines.push(line));
    expect(lines).toEqual([progressLine(at)]);
  });

  it("stops the run begun last, also after it has ended", () => {
    const runs = new Runs();
    const stopped: number[] = [];
    runs.start();
    runs.start();
    runs.stop((run) => stopped.push(run));
    runs.finish();
    runs.stop((run) => stopped.push(run));
    expect(stopped).toEqual([2, 2]);
  });
});

/**
 * What is left in `App.svelte`: that the window hands these functions their
 * parts. Read as text with the comments taken out, which sees that a line is
 * there and not whether it runs (`sourcetext.ts`); every decision that used to
 * be checked this way is in the functions tested above.
 */
describe("the window's wiring", () => {
  it("runs the check through Runs.check, and Stop and the progress line through the same Runs", () => {
    const check = functionIn(app, "async function findHiddenText(): Promise<void> {");
    expect(missingFrom(check, [
      "await hiddenRuns.check({",
      'ask: (run) => call("hidden_text", { doc: asked.doc, run }),',
      "current: () => edits === asked,",
      "sidebar?.setHiddenText(checked);",
      "viewer?.clearRegion();",
      'showTab("hidden");',
    ])).toEqual([]);
    expect(missingFrom(app, [
      'onclick={() => hiddenRuns.stop((run) => void call("hidden_text_cancel", { run }))}',
      "await listen<HiddenProgress>(HIDDEN_PROGRESS_EVENT, (event) => {",
      "hiddenRuns.report(event.payload, (line) => { blockingTask = line; });",
    ])).toEqual([]);
  });

  it("takes a pressed row through showPassage, and the ring away when another tab is chosen", () => {
    expect(missingFrom(functionIn(app, "function showHidden(passage: Passage): void {"), [
      "showPassage(passage, (page) => model.map.slotOf(page), {",
      "page: (slot) => { shown.clearRegion(); shown.goToDestination(slot, null); },",
      "gone: () => say(PAGE_GONE),",
    ])).toEqual([]);
    expect(missingFrom(app, [
      "hidden: { onPick: (passage) => showHidden(passage) },",
      'if (tab !== "hidden") viewer?.clearRegion();',
    ])).toEqual([]);
  });
});
