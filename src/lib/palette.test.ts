import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { CommandRegistry } from "./commands";
import { Palette } from "./palette";
import { FakeElement, installFakeDom, type FakeDom } from "./testdom";

/**
 * The palette's dismissal hook, which is what releases a file an insert holds
 * open while it asks for the pages.
 *
 * Every route out of argument mode is driven here because each one is a line
 * that could set `asking` to null on its own and skip the hook --- and the one
 * that did would leak a sandboxed worker pool per abandoned insert, with
 * nothing on screen to say so.
 */

let dom: FakeDom;
let restoreBody: () => void;

beforeEach(() => {
  dom = installFakeDom();
  const globals = globalThis as unknown as Record<string, unknown>;
  const doc = globals.document as Record<string, unknown>;
  // The two things the palette reads that the fake document leaves out.
  doc.body = new FakeElement("body");
  doc.activeElement = null;
  const previous = globals.HTMLElement;
  globals.HTMLElement = FakeElement;
  restoreBody = () => {
    if (previous === undefined) delete globals.HTMLElement;
    else globals.HTMLElement = previous;
  };
});

afterEach(() => {
  restoreBody();
  dom.restore();
});

/** A palette over one argument command, and what happened to it. */
function asked(enabled = () => true) {
  const events: string[] = [];
  const registry = new CommandRegistry();
  registry.register(
    {
      id: "edit.insertPages.range",
      title: "Insert pages from report.pdf",
      enabled,
      argument: {
        placeholder: "Pages",
        problem: (raw) => (raw === "bad" ? "not a page" : null),
        preview: () => "",
        run: (raw) => void events.push(`run:${raw}`),
        dismissed: () => void events.push("dismissed"),
      },
    },
    { id: "view.fitWidth", title: "Fit width", run: () => void events.push("fit") },
  );
  const palette = new Palette(registry);
  const body = (globalThis as unknown as { document: { body: FakeElement } }).document.body;
  const input = body.querySelector("input")!;
  const key = (k: string) => input.dispatch("keydown", { key: k });
  const type = (text: string) => {
    (input as unknown as { value: string }).value = text;
    input.dispatch("input", {});
  };
  palette.askFor("edit.insertPages.range");
  return { palette, events, key, type };
}

describe("leaving the palette's argument question", () => {
  it("is not a dismissal when the question is answered", () => {
    const { palette, events, key, type } = asked();
    expect(palette.isAsking).toBe(true);
    type("2-4");
    key("Enter");
    expect(events).toEqual(["run:2-4"]);
    expect(palette.isOpen).toBe(false);
  });

  it("is a dismissal on Escape back to the list, once", () => {
    const { palette, events, key } = asked();
    key("Escape");
    expect(palette.isAsking).toBe(false);
    expect(palette.isOpen).toBe(true);
    key("Escape");
    expect(events).toEqual(["dismissed"]);
  });

  it("is a dismissal when the palette is closed", () => {
    const { palette, events } = asked();
    palette.close();
    palette.close();
    expect(events).toEqual(["dismissed"]);
  });

  it("is a dismissal when the same question is asked again", () => {
    // `edit.insertPages` closes the palette before a second file is opened, so
    // this route is not how a second file replaces the first --- but a
    // question asked over itself has still been abandoned.
    const { palette, events } = asked();
    palette.askFor("edit.insertPages.range");
    expect(events).toEqual(["dismissed"]);
    expect(palette.isAsking).toBe(true);
  });

  it("is not reached by a refused answer, which leaves the question open", () => {
    const { palette, events, key, type } = asked();
    type("bad");
    key("Enter");
    expect(events).toEqual([]);
    expect(palette.isAsking).toBe(true);
  });

  it("is a dismissal when the command can no longer run as answered", () => {
    // Asked while enabled, withdrawn before the answer: the registry will not
    // run it, so what it holds must still be let go.
    let live = true;
    const { events, key, type } = asked(() => live);
    type("1");
    live = false;
    key("Enter");
    expect(events).toEqual(["dismissed"]);
  });
});
