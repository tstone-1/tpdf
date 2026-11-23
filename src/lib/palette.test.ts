import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { CommandRegistry } from "./commands";
import { pick, type Embedded } from "./attachments";
import { runMenuCommand } from "./menubar";
import { askedName, Palette } from "./palette";
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
  const palette = new Palette(registry, {
    turnPage: (forward) => void events.push(forward ? "page:next" : "page:previous"),
  });
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

  it("can save the named attachment after the native menu repeats the question", () => {
    const commands = new CommandRegistry();
    const palette = new Palette(commands);
    const choice = "file.saveAttachment.choice";
    let files: Embedded[] | null = null;
    let request = 0;
    const saved: string[] = [];
    commands.register({
      id: "file.saveAttachment", title: "Save an attachment",
      run: () => palette.askFor(choice, () => {
        request++;
        files = ["first.txt", `second-${request}.txt`].map((name) => ({ name, size: null, description: "" }));
      }),
    }, {
      id: choice, title: "Choose an attachment", enabled: () => files !== null,
      argument: {
        placeholder: "Name", problem: (raw) => "index" in pick(raw, files ?? []) ? null : "No such attachment",
        preview: () => "", dismissed: () => { files = null; },
        run: (raw) => {
          const selected = pick(raw, files ?? []);
          if (files && "index" in selected) saved.push(files[selected.index]!.name);
        },
      },
    });
    runMenuCommand(commands, palette, "file.saveAttachment");
    runMenuCommand(commands, palette, "file.saveAttachment");
    const body = document.body as unknown as FakeElement;
    const input = body.querySelector("input")!;
    input.value = "second-2";
    input.dispatch("input", {});
    input.dispatch("keydown", { key: "Enter" });
    expect(saved).toEqual(["second-2.txt"]);
    expect(palette.isOpen).toBe(false);
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

describe("the document behind a question", () => {
  // Picking "slides 2, 3-5 and 10" is done by scrolling through the deck with
  // the question open; a backdrop that took the wheel made that impossible.
  it("takes the wheel while a value is typed, and not while searching", () => {
    const { palette, key } = asked();
    expect(palette.isLettingDocumentThrough).toBe(true);
    key("Escape");
    expect(palette.isAsking).toBe(false);
    expect(palette.isLettingDocumentThrough).toBe(false);
  });

  it("stops taking it once the question is answered", () => {
    const { palette, key, type } = asked();
    type("2");
    key("Enter");
    palette.open();
    expect(palette.isLettingDocumentThrough).toBe(false);
  });

  it("turns pages on Page Up and Page Down from the input", () => {
    const { events, key } = asked();
    key("PageDown");
    key("PageUp");
    expect(events).toEqual(["page:next", "page:previous"]);
  });

  it("does not turn pages while a command is searched for", () => {
    const { events, key } = asked();
    key("Escape");
    key("PageDown");
    expect(events).toEqual(["dismissed"]);
  });
});

describe("a row found by a keyword", () => {
  /** Every text the palette has put on screen for `query`, in document order. */
  function shown(query: string): string[] {
    const registry = new CommandRegistry();
    registry.register({
      id: "file.compress",
      title: "Save a smaller copy...",
      keywords: ["compress"],
      run: () => {},
    });
    const palette = new Palette(registry);
    const body = (globalThis as unknown as { document: { body: FakeElement } }).document.body;
    const input = body.querySelector("input")!;
    palette.open();
    (input as unknown as { value: string }).value = query;
    input.dispatch("input", {});
    const texts: string[] = [];
    const walk = (node: FakeElement): void => {
      if (node.textContent) texts.push(node.textContent);
      node.children.forEach(walk);
    };
    walk(body);
    return texts;
  }

  it("shows the word that found it, since the title cannot", () => {
    expect(shown("compress")).toContain("compress");
  });

  it("shows no word when the title is what matched", () => {
    // The control: the same row, reached by its title, has only the title on it.
    const texts = shown("copy");
    expect(texts).toContain("copy");
    expect(texts).not.toContain("compress");
  });
});

describe("a command's question", () => {
  /** Every text on screen, in document order. */
  function texts(): string[] {
    const body = (globalThis as unknown as { document: { body: FakeElement } }).document.body;
    const found: string[] = [];
    const walk = (node: FakeElement): void => {
      if (node.textContent) found.push(node.textContent);
      node.children.forEach(walk);
    };
    walk(body);
    return found;
  }

  it("names the command and what it wants, before anything is typed", () => {
    asked();
    expect(texts()).toContain("Insert pages from report.pdf");
    expect(texts()).toContain("Pages");
  });

  it("still names them once an answer is typed, when the placeholder has gone", () => {
    const { type } = asked();
    type("1-3");
    expect(texts()).toContain("Insert pages from report.pdf");
    expect(texts()).toContain("Pages");
  });

  it("drops the dots that promised the question", () => {
    const registry = new CommandRegistry();
    registry.register({
      id: "edit.addDropdown",
      title: "Add a form field: dropdown...",
      argument: { placeholder: "Choices", problem: () => null, preview: () => "", run: () => {} },
    });
    new Palette(registry).askFor("edit.addDropdown");
    expect(texts()).toContain("Add a form field: dropdown");
  });

  it("names no command while the list of commands is shown", () => {
    const { key } = asked();
    key("Escape");
    expect(texts()).not.toContain("Pages");
  });
});

describe("what a screen reader is told while a value is typed", () => {
  // Measured with NVDA 2026.2 on the installed 26.11.8, 2026-11-15: after Enter
  // chose *Extract pages* it said nothing, and typing `3-4` was echoed letter
  // by letter with no word of the preview.

  /** Every element under `from` with `role="status"`. */
  function statuses(from: FakeElement): FakeElement[] {
    const found = from.getAttribute("role") === "status" ? [from] : [];
    return found.concat(...from.children.map(statuses));
  }

  function setUp(options: { namesAreSpoken?: boolean } = {}) {
    const registry = new CommandRegistry();
    registry.register({
      id: "file.extractPages",
      title: "Extract pages...",
      argument: {
        placeholder: "Pages, e.g. 1-3,5",
        problem: (raw) => (raw === "99" ? "There is no page 99" : null),
        preview: (raw) => (raw === "" ? "" : `Extract ${raw}`),
        run: () => {},
      },
    });
    const palette = new Palette(registry, options);
    const body = (globalThis as unknown as { document: { body: FakeElement } }).document.body;
    const input = body.querySelector("input")!;
    const type = (text: string) => {
      (input as unknown as { value: string }).value = text;
      input.dispatch("input", {});
    };
    const key = (k: string) => input.dispatch("keydown", { key: k });
    return { palette, body, input, type, key };
  }

  it("names the field by the question while it asks, and by the search after", () => {
    const { palette, input, key } = setUp();
    palette.open();
    expect(input.getAttribute("aria-label")).toBe("Run a command");
    palette.askFor("file.extractPages");
    expect(input.getAttribute("aria-label")).toBe("Extract pages: Pages, e.g. 1-3,5");
    key("Escape");
    expect(input.getAttribute("aria-label")).toBe("Run a command");
  });

  it("puts the preview and the refusal into one live region that is never rebuilt", () => {
    const { palette, body, type } = setUp();
    palette.askFor("file.extractPages");
    const [heard, ...more] = statuses(body);
    // One region: a second, inside the rebuilt list, is the one that stays silent.
    expect(more).toEqual([]);
    expect(heard?.textContent).toBe("");
    type("3-4");
    expect(statuses(body)).toEqual([heard]);
    expect(heard?.textContent).toBe("Extract 3-4");
    type("99");
    expect(statuses(body)).toEqual([heard]);
    expect(heard?.textContent).toBe("There is no page 99");
  });

  it("says the question in the live region where a new name is not spoken, until something is typed", () => {
    // VoiceOver in WebKit, measured 2026-11-15. Where names are spoken the
    // region stays empty, as the test above asserts, or NVDA says it twice.
    const { palette, body, type } = setUp({ namesAreSpoken: false });
    palette.askFor("file.extractPages");
    const [heard] = statuses(body);
    expect(heard?.textContent).toBe("Extract pages: Pages, e.g. 1-3,5");
    type("3-4");
    expect(heard?.textContent).toBe("Extract 3-4");
  });

  it("empties the live region when the question is left", () => {
    const { palette, body, type, key } = setUp();
    palette.askFor("file.extractPages");
    type("3-4");
    key("Escape");
    expect(statuses(body)[0]?.textContent).toBe("");
  });

  it("names a command without a question by its title alone", () => {
    expect(askedName({ id: "x", title: "Go to page…", argument: {
      placeholder: "", problem: () => null, preview: () => "", run: () => {} } })).toBe("Go to page");
  });
});
