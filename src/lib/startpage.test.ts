/**
 * The blank page's rows, its keys, and its two ways of forgetting.
 *
 * `App.svelte` has the markup loop and nothing else, so every decision the page
 * makes is one of the functions below. Each test was checked by mutation ---
 * see `scripts/mutate_frontend.py`.
 */

import { describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import { functionIn, missingFrom } from "./sourcetext";
import { CommandRegistry } from "./commands";
import { MAX_RECENTS, RECENT_PREFIX } from "./recents";
import type { Place } from "./session";
import {
  behindWrites,
  focusAfterRemoval,
  folderOf,
  pageText,
  recentCommands,
  rowsFor,
  shortenHome,
  startMove,
  StartPage,
  UNOPENED,
  type StartKey,
  type StartRow,
  FOLDER_CHARS,
  tailOf,
} from "./startpage";

const HOME = "/Users/reader";

function place(path: string, page = 0): Place {
  return { path, page, top_pt: 0, zoom: 1, fit: "width", turns: 0, sidebar: false, page_count: 40 };
}

function key(name: string, modifier: Partial<StartKey> = {}): StartKey {
  return { key: name, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...modifier };
}

describe("a folder too long for its row", () => {
  it("is left alone up to the limit", () => {
    const fits = `/${"a".repeat(FOLDER_CHARS - 1)}`;
    expect(fits.length).toBe(FOLDER_CHARS);
    expect(tailOf(fits)).toBe(fits);
    expect(tailOf("~/Documents")).toBe("~/Documents");
  });

  it("keeps its end, from a separator, after an ellipsis", () => {
    const long = "/Users/reader/Library/CloudStorage/Provider/Shared/Clients/Acme/2026/Reports";
    const shown = tailOf(long);
    expect(shown.startsWith("…/")).toBe(true);
    expect(long.endsWith(shown.slice(1))).toBe(true);
    expect(shown.length).toBeLessThanOrEqual(FOLDER_CHARS);
    expect(shown).toBe("…/Provider/Shared/Clients/Acme/2026/Reports");
  });

  it("cuts a Windows folder at its own separator", () => {
    const long = "C:\\Users\\reader\\OneDrive - Company\\Shared Documents\\Clients\\Acme\\Reports";
    expect(tailOf(long)).toBe("…\\Shared Documents\\Clients\\Acme\\Reports");
  });

  it("cuts by characters where no separator is near", () => {
    const long = `/${"x".repeat(80)}`;
    expect(tailOf(long)).toBe(`…${"x".repeat(FOLDER_CHARS - 1)}`);
    // A separator far into what is kept would throw most of it away.
    const lastLong = `/${"a".repeat(40)}/${"b".repeat(25)}/cd`;
    expect(tailOf(lastLong)).toBe(`…${"a".repeat(18)}/${"b".repeat(25)}/cd`);
  });

  it("is what a row shows", () => {
    const path = "/Users/reader/Library/CloudStorage/Provider/Shared/Clients/Acme/2026/Reports/a.pdf";
    const [row] = rowsFor([{ path, page: 0, top_pt: 0, zoom: 1, fit: "none" } as unknown as Place], null);
    expect(row?.folder).toBe("…/Provider/Shared/Clients/Acme/2026/Reports");
  });
});

describe("folderOf", () => {
  it("is everything before the last separator, of either kind", () => {
    expect(folderOf("/Users/reader/Documents/report.pdf")).toBe("/Users/reader/Documents");
    expect(folderOf("C:\\Users\\reader\\Documents\\report.pdf")).toBe("C:\\Users\\reader\\Documents");
    // Mixed, which Windows allows: cut at the later one, and nothing rewritten.
    expect(folderOf("C:\\Users\\reader/Documents/report.pdf")).toBe("C:\\Users\\reader/Documents");
    expect(folderOf("C:/Users/reader\\report.pdf")).toBe("C:/Users/reader");
  });

  it("names a root by its separator", () => {
    expect(folderOf("/report.pdf")).toBe("/");
    expect(folderOf("C:\\report.pdf")).toBe("C:\\");
    expect(folderOf("C:/report.pdf")).toBe("C:/");
  });

  it("is empty for a bare name", () => {
    expect(folderOf("report.pdf")).toBe("");
  });
});

describe("shortenHome", () => {
  it("writes the home folder, and what is under it, with a tilde", () => {
    expect(shortenHome("/Users/reader/Documents", HOME)).toBe("~/Documents");
    expect(shortenHome("/Users/reader", HOME)).toBe("~");
    expect(shortenHome("/Users/reader/Documents", "/Users/reader/")).toBe("~/Documents");
  });

  it("leaves a folder that only begins like home", () => {
    expect(shortenHome("/Users/reader-old/Documents", HOME)).toBe("/Users/reader-old/Documents");
    expect(shortenHome("/Volumes/Share/Users/reader/x", HOME)).toBe("/Volumes/Share/Users/reader/x");
  });

  it("leaves every folder as written when home is not known", () => {
    for (const home of [null, undefined, ""]) {
      expect(shortenHome("/Users/reader/Documents", home)).toBe("/Users/reader/Documents");
    }
  });

  it("leaves a Windows folder as written, under a Windows home too", () => {
    // Nothing a reader pastes into Explorer expands a tilde.
    const home = "C:\\Users\\reader";
    expect(shortenHome("C:\\Users\\reader\\Documents", home)).toBe("C:\\Users\\reader\\Documents");
    expect(shortenHome("C:\\Users\\reader", home)).toBe("C:\\Users\\reader");
    // And one recorded on another machine is not under this home.
    expect(shortenHome("C:\\Users\\reader\\Documents", HOME)).toBe("C:\\Users\\reader\\Documents");
  });

  it("does not shorten anything when home is the root", () => {
    expect(shortenHome("/srv/papers", "/")).toBe("/srv/papers");
  });
});

describe("pageText", () => {
  it("says nothing on the first page and counts from one after it", () => {
    expect(pageText({ page: 0 })).toBe("");
    expect(pageText({ page: 1 })).toBe("page 2");
    expect(pageText({ page: 11 })).toBe("page 12");
  });
});

describe("rowsFor", () => {
  const places = [
    place("/Users/reader/Documents/acme/report.pdf", 11),
    place("/Users/reader/Documents/globex/report.pdf"),
    place("C:\\Users\\reader\\spec.pdf", 2),
  ];

  it("is a row per place, in order, with what the page shows", () => {
    expect(rowsFor(places, HOME)).toEqual([
      {
        path: "/Users/reader/Documents/acme/report.pdf",
        label: "acme/report.pdf",
        folder: "~/Documents/acme",
        page: "page 12",
        command: "file.recent.0",
        trouble: "",
        removing: "Remove acme/report.pdf from this list",
      },
      {
        path: "/Users/reader/Documents/globex/report.pdf",
        label: "globex/report.pdf",
        folder: "~/Documents/globex",
        page: "",
        command: "file.recent.1",
        trouble: "",
        removing: "Remove globex/report.pdf from this list",
      },
      {
        path: "C:\\Users\\reader\\spec.pdf",
        label: "spec.pdf",
        folder: "C:\\Users\\reader",
        page: "page 3",
        command: "file.recent.2",
        trouble: "",
        removing: "Remove spec.pdf from this list",
      },
    ] satisfies StartRow[]);
  });

  it("offers the newest few and no more", () => {
    const many = Array.from({ length: MAX_RECENTS + 3 }, (_, n) => place(`/Users/reader/${n}.pdf`));
    const rows = rowsFor(many, HOME);
    expect(rows.map((row) => row.label)).toEqual(
      many.slice(0, MAX_RECENTS).map((_, n) => `${n}.pdf`),
    );
  });

  it("marks the documents that would not open, and only those", () => {
    const rows = rowsFor(places, HOME, new Set(["/Users/reader/Documents/globex/report.pdf"]));
    expect(rows.map((row) => row.trouble)).toEqual(["", UNOPENED, ""]);
  });

  it("is no rows for no places", () => {
    expect(rowsFor([], HOME)).toEqual([]);
  });
});

describe("recentCommands", () => {
  it("makes the command each row names, and it opens that row's document", () => {
    const rows = rowsFor(
      [place("/Users/reader/a/report.pdf"), place("/Users/reader/b/report.pdf")],
      HOME,
    );
    const opened: string[] = [];
    const registry = new CommandRegistry();
    registry.replace(RECENT_PREFIX, recentCommands(rows, (path) => opened.push(path)));

    // Every row reaches a command, and the command is that row's.
    for (const row of rows) {
      expect(registry.find(row.command)?.title).toBe(`Open ${row.label}`);
      expect(registry.run(row.command)).toBe(true);
    }
    expect(opened).toEqual(rows.map((row) => row.path));
    // And the registry holds nothing recent the page does not show.
    expect(registry.all().map((command) => command.id)).toEqual(rows.map((row) => row.command));
  });
});

describe("startMove", () => {
  it("enters the list on the first Down, and on nothing else", () => {
    expect(startMove(key("ArrowDown"), -1, 3)).toEqual({ focus: 0 });
    for (const name of ["ArrowUp", "Home", "End", "Delete", "Backspace", "Enter", "a"]) {
      expect(startMove(key(name), -1, 3), name).toBeNull();
    }
  });

  it("walks the rows and stops at the ends", () => {
    expect(startMove(key("ArrowDown"), 0, 3)).toEqual({ focus: 1 });
    expect(startMove(key("ArrowDown"), 2, 3)).toEqual({ focus: 2 });
    expect(startMove(key("ArrowUp"), 2, 3)).toEqual({ focus: 1 });
    expect(startMove(key("ArrowUp"), 0, 3)).toEqual({ focus: 0 });
    expect(startMove(key("Home"), 2, 3)).toEqual({ focus: 0 });
    expect(startMove(key("End"), 0, 3)).toEqual({ focus: 2 });
  });

  it("removes the focused row on Delete and on Backspace", () => {
    expect(startMove(key("Delete"), 1, 3)).toEqual({ remove: 1 });
    expect(startMove(key("Backspace"), 2, 3)).toEqual({ remove: 2 });
  });

  it("leaves Enter and every other key to the button", () => {
    for (const name of ["Enter", " ", "Tab", "Escape", "x"]) {
      expect(startMove(key(name), 1, 3), name).toBeNull();
    }
  });

  it("leaves a chord alone", () => {
    for (const modifier of ["metaKey", "ctrlKey", "altKey", "shiftKey"] as const) {
      expect(startMove(key("Backspace", { [modifier]: true }), 1, 3), modifier).toBeNull();
      expect(startMove(key("ArrowDown", { [modifier]: true }), -1, 3), modifier).toBeNull();
    }
  });

  it("has nowhere to go in an empty list", () => {
    expect(startMove(key("ArrowDown"), -1, 0)).toBeNull();
    expect(startMove(key("Delete"), 0, 0)).toBeNull();
  });

  it("treats a row that has since gone as the last one", () => {
    expect(startMove(key("ArrowUp"), 7, 3)).toEqual({ focus: 1 });
    expect(startMove(key("Delete"), 7, 3)).toEqual({ remove: 2 });
  });
});

describe("focusAfterRemoval", () => {
  it("is the row that took the place, or the one above the last", () => {
    expect(focusAfterRemoval(0, 2)).toBe(0);
    expect(focusAfterRemoval(1, 2)).toBe(1);
    expect(focusAfterRemoval(2, 2)).toBe(1);
  });

  it("is no row when the list is empty", () => {
    expect(focusAfterRemoval(0, 0)).toBe(-1);
  });
});

describe("behindWrites", () => {
  it("changes the list only after every place already issued has landed", async () => {
    const log: string[] = [];
    let landed!: () => void;
    const writer = {
      flush: () => { log.push("flush"); },
      settled: () => new Promise<void>((resolve) => {
        landed = () => { log.push("settled"); resolve(); };
      }),
    };
    const done = behindWrites(writer, async () => { log.push("change"); });
    await Promise.resolve();
    await Promise.resolve();
    expect(log).toEqual(["flush"]);
    landed();
    expect(await done).toBeNull();
    expect(log).toEqual(["flush", "settled", "change"]);
  });

  it("hands back what a refused change said", async () => {
    const writer = { flush: () => {}, settled: () => Promise.resolve() };
    const said = await behindWrites(writer, () =>
      Promise.reject("could not write the session file /Users/reader/session.json: denied"),
    );
    expect(said).toBe("could not write the session file /Users/reader/session.json: denied");
  });
});

describe("the page's wiring in App.svelte", () => {
  // Read as text with the comments taken out, because `App.svelte` is the join
  // and nothing imports it. That sees that a line is there and not whether it
  // runs (`sourcetext.ts`), so these hold the hand-over and no decision.
  it("builds the palette's commands from the rows the page shows", () => {
    expect(missingFrom(functionIn(app, "function offerRecents(from: Session) {"), [
      "recentCommands(startPage.offer(from), (path) => void openPath(path)),",
    ])).toEqual([]);
  });

  it("runs a row's command, and the button's, through the registry", () => {
    expect(missingFrom(app, [
      "onclick={() => runStartCommand(row.command)}",
      'onclick={() => runStartCommand("file.open")}',
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "function runStartCommand(id: string) {"), ["commands.run(id);"])).toEqual([]);
  });

  it("tells the page when an open fails and when one succeeds", () => {
    expect(missingFrom(functionIn(app, "async function openDocument("), [
      "startPage.failed(path, e);",
      "startPage.opened(path);",
    ])).toEqual([]);
  });

  it("forgets behind the places already issued, and re-reads the list after", () => {
    // Behind the writes is `behindWrites`' doing, tested above. Here: that both
    // commands go through it, and what each function also calls.
    expect(missingFrom(functionIn(app, "async function forgetRecent(row: StartRow, index: number) {"), [
      'behindWrites(places, () => call("session_forget", { path: row.path }));',
      "if (said) say(said);",
      "await refreshRecents();",
    ])).toEqual([]);
    expect(missingFrom(functionIn(app, "async function clearRecents() {"), [
      'behindWrites(places, () => call("session_clear_places"));',
      "if (said) say(said);",
      "places.forgotten();",
      "notePlace();",
      "await refreshStartPage();",
    ])).toEqual([]);
  });

  it("enters the list from the window only when nothing has the focus", () => {
    expect(missingFrom(functionIn(app, "function onWindowKey(event: KeyboardEvent) {"), [
      "if (!title && (focused === null || focused === document.body) && startPageKey(event, -1)) return;",
    ])).toEqual([]);
  });
});

describe("StartPage", () => {
  const session = {
    places: [place("/Users/reader/Documents/report.pdf", 4), place("/Users/reader/notes.pdf")],
    home: HOME,
  };

  it("tells its rows when it is given a session", () => {
    const told: StartRow[][] = [];
    const page = new StartPage((rows) => told.push(rows));
    const rows = page.offer(session);
    expect(rows.map((row) => `${row.label} ${row.folder} ${row.page}`.trim())).toEqual([
      "report.pdf ~/Documents page 5",
      "notes.pdf ~",
    ]);
    expect(told).toEqual([rows]);
    expect(page.rows).toEqual(rows);
  });

  it("shows folders as written when the session names no home", () => {
    const page = new StartPage();
    expect(page.offer({ places: session.places })[0]?.folder).toBe("/Users/reader/Documents");
  });

  it("marks a document that would not open, and unmarks it when it does", () => {
    const told: StartRow[][] = [];
    const page = new StartPage((rows) => told.push(rows));
    page.offer(session);

    page.failed("/Users/reader/notes.pdf", { reason: "This file could not be read.", locked: false });
    expect(page.rows.map((row) => row.trouble)).toEqual(["", UNOPENED]);
    expect(told).toHaveLength(2);
    // A plain error marks it as well: not every failure is a refusal.
    page.failed("/Users/reader/Documents/report.pdf", new Error("document reports no pages"));
    expect(page.rows.map((row) => row.trouble)).toEqual([UNOPENED, UNOPENED]);

    page.opened("/Users/reader/notes.pdf");
    expect(page.rows.map((row) => row.trouble)).toEqual([UNOPENED, ""]);
    expect(told.at(-1)).toEqual(page.rows);
    // An ordinary open changes nothing here, and says nothing.
    const before = told.length;
    page.opened("/Users/reader/notes.pdf");
    expect(told).toHaveLength(before);
  });

  it("does not mark a document whose password was declined", () => {
    const page = new StartPage();
    page.offer(session);
    page.failed("/Users/reader/notes.pdf", { reason: "This document needs a password.", locked: true });
    expect(page.rows.map((row) => row.trouble)).toEqual(["", ""]);
  });

  it("keeps a mark across a re-read of the same list", () => {
    const page = new StartPage();
    page.offer(session);
    page.failed("/Users/reader/notes.pdf", "gone");
    page.offer({ ...session, places: [...session.places].reverse() });
    expect(page.rows.map((row) => row.trouble)).toEqual([UNOPENED, ""]);
  });

  it("drops the mark with the row, so a document remembered again starts clean", () => {
    const page = new StartPage();
    page.offer(session);
    page.failed("/Users/reader/notes.pdf", "gone");
    page.offer({ ...session, places: [session.places[0] as Place] });
    page.offer(session);
    expect(page.rows.map((row) => row.trouble)).toEqual(["", ""]);
  });
});
