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
  UNSAVED,
  type HiddenText,
  type Passage,
} from "./hiddentext";

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

/**
 * The join in `App.svelte`, which no test imports: read as text, each part
 * within the function it belongs to, so a line that moved out of it is missed.
 */
describe("the window's wiring", () => {
  /** The source from `from` up to the end of the function it begins. */
  function body(from: string): string {
    const start = app.indexOf(from);
    // The control: a name that is not there would give the whole file, in
    // which anything can be found.
    expect(start, from).toBeGreaterThan(-1);
    const end = app.indexOf("\n  }\n", start);
    expect(end, from).toBeGreaterThan(start);
    return app.slice(start, end);
  }

  it("sends the check with its run's number, and Stop names the same run", () => {
    const check = body("async function findHiddenText(): Promise<void> {");
    const numbered = check.indexOf("const run = hiddenRuns.start();");
    const sent = check.indexOf('await call("hidden_text", { doc: asked.doc, run });');
    expect(numbered).toBeGreaterThan(-1);
    // Numbered before it is sent, so a Stop pressed in between names this run.
    expect(sent).toBeGreaterThan(numbered);
    expect(app).toContain('call("hidden_text_cancel", { run: hiddenRuns.number })');
    expect(app).toContain("{#if findingHidden}<button");
  });

  it("puts the answer in the sidebar's tab and opens it, and ends the run whatever happened", () => {
    const check = body("async function findHiddenText(): Promise<void> {");
    const shown = check.indexOf("sidebar?.setHiddenText(checked);");
    expect(shown).toBeGreaterThan(-1);
    expect(check.indexOf('showTab("hidden");')).toBeGreaterThan(shown);
    // With the ring of the result it replaces taken away.
    expect(check.indexOf("viewer?.clearRegion();")).toBeGreaterThan(shown);
    // Only for the document that was asked about.
    expect(check.indexOf("if (edits !== asked) return;")).toBeGreaterThan(-1);
    expect(check.indexOf("if (edits !== asked) return;")).toBeLessThan(shown);
    const last = check.slice(check.indexOf("} finally {"));
    expect(last).toContain("hiddenRuns.finish();");
    expect(last).toContain("findingHidden = false;");
    expect(last).toContain("blockingTask = null;");
  });

  it("shows a page's line only through the guard", () => {
    expect(app).toContain("await listen<HiddenProgress>(HIDDEN_PROGRESS_EVENT, (event) => {");
    expect(app).toContain("blockingTask = hiddenRuns.line(event.payload) ?? blockingTask;");
  });

  it("takes a pressed row to its page through the page map, and rings it", () => {
    expect(app).toContain("hidden: { onPick: (passage) => showHidden(passage) },");
    const show = body("function showHidden(passage: Passage): void {");
    expect(show).toContain("placeOf(passage, (page) => edits?.map.slotOf(page))");
    expect(show).toContain("say(PAGE_GONE);");
    expect(show).toContain("viewer.showRegion(place.slot, place.rect);");
    // Words outside the page: the page, and no ring left from the row before.
    const outside = show.slice(show.indexOf("} else {"));
    expect(outside).toContain("viewer.clearRegion();");
    expect(outside).toContain("viewer.goToDestination(place.slot, null);");
  });

  it("takes the ring away when another tab is chosen", () => {
    expect(app).toContain('if (tab !== "hidden") viewer?.clearRegion();');
  });
});
