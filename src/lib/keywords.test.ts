/**
 * Tests for the palette's vocabulary: what a reader types, and what it finds.
 *
 * `TYPED` is the expectation and `keywords.ts` is one of the two things that
 * meet it; the titles are the other. The list is written here by hand and not
 * derived from the keyword table, because a list read out of the table would
 * agree with it whatever the table said. It began as the 135 words measured
 * against the palette on 2026-10-13, taken from `docs/DEMAND.md` and from the
 * menus of Acrobat, Preview and PDF-XChange Editor.
 *
 * A word must put its command in the first three rows, which is what a reader
 * sees without reading the list. Every command counts as enabled here: whether
 * "duplicate" finds its command is a question about words, and the answer
 * should not depend on whether a mark happens to be picked.
 */

import { describe, expect, it } from "vitest";

import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry, rank, type Command } from "./commands";
import type { UiLocale } from "./i18n";
import { KEYWORDS, respelled } from "./keywords";

/** Every application command, each one offered. */
function everyCommand(locale?: UiLocale): Command[] {
  const answers: Record<string, () => unknown> = {
    viewer: () => ({ searchScoped: false, hasSelection: false }),
    tabLabels: () => ({ canGrow: true, canShrink: true, isDefault: true }),
    sides: () => ({ split: false, focused: "left", others: [] }),
  };
  const actions = new Proxy(
    {},
    { get: (_target, key: string) => answers[key] ?? (() => undefined) },
  ) as AppActions;
  const registry = new CommandRegistry();
  registerAppCommands(registry, actions, locale);
  return registry.all().map((command) => ({ ...command, enabled: () => true }) as Command);
}

/** A typed word, and the id of the command it must find. */
const TYPED: [string, string][] = [
  // Merging, splitting, pages out.
  ["merge", "file.mergeDocuments"], ["combine", "file.mergeDocuments"],
  ["join", "file.mergeDocuments"], ["append", "file.mergeDocuments"],
  ["split", "file.splitDocument"], ["separate", "file.splitDocument"],
  ["extract", "file.extractPages"], ["export", "file.extractPages"],
  // A smaller file.
  ["compress", "file.compress"], ["reduce", "file.compress"], ["shrink", "file.compress"],
  ["smaller", "file.compress"], ["optimize", "file.compress"], ["file size", "file.compress"],
  // Recognising text.
  ["ocr", "file.recogniseText"], ["recognize", "file.recogniseText"],
  ["recognise", "file.recogniseText"], ["searchable", "file.recogniseText"],
  ["scan", "file.recogniseText"],
  // Passwords.
  ["password", "file.protect"], ["encrypt", "file.protect"], ["protect", "file.protect"],
  ["lock", "file.protect"], ["secure", "file.protect"],
  ["unlock", "file.unprotect"], ["decrypt", "file.unprotect"],
  ["remove password", "file.unprotect"],
  // Redaction.
  ["redact", "edit.redactRegion"], ["blackout", "edit.redactRegion"],
  ["black out", "edit.redactRegion"], ["censor", "edit.redactRegion"],
  // Signing.
  ["sign", "file.signDocument"], ["signature", "file.signDocument"],
  ["certificate", "file.signDocument"], ["esign", "file.signDocument"],
  ["timestamp", "file.signDocument"], ["initials", "edit.addSignature"],
  // Forms.
  ["fill", "edit.fillForm"], ["form", "edit.fillForm"], ["checkbox", "edit.addCheckbox"],
  ["dropdown", "edit.addDropdown"], ["radio", "edit.addRadio"],
  // Comments and marks.
  ["comment", "edit.addComment"], ["note", "edit.addComment"], ["sticky", "edit.addComment"],
  ["annotate", "edit.addComment"], ["highlight", "edit.highlightSelection"],
  ["marker", "edit.highlightSelection"], ["strikethrough", "edit.strikeoutSelection"],
  ["strike", "edit.strikeoutSelection"], ["cross out", "edit.strikeoutSelection"],
  ["underline", "edit.underlineSelection"], ["stamp", "edit.stamp.approved"],
  ["draw", "edit.draw"], ["pen", "edit.draw"], ["rectangle", "edit.drawBox"],
  ["circle", "edit.drawEllipse"], ["shape", "edit.drawBox"], ["erase", "edit.erase"],
  ["eraser", "edit.erase"], ["colour", "edit.color.yellow"], ["color", "edit.color.yellow"],
  // Text.
  ["text box", "edit.addTextBox"], ["add text", "edit.addTextBox"], ["type", "edit.addTextBox"],
  ["typewriter", "edit.addTextBox"], ["edit text", "edit.editText"],
  ["change text", "edit.editText"], ["edit", "edit.editText"],
  // Pages.
  ["rotate", "edit.rotatePageClockwise"], ["turn", "edit.rotatePageClockwise"],
  ["counterclockwise", "edit.rotatePageCounterClockwise"],
  ["anticlockwise", "edit.rotatePageCounterClockwise"],
  ["delete page", "edit.deletePage"], ["remove page", "edit.deletePage"],
  ["delete", "edit.deletePage"], ["reorder", "edit.movePageUp"],
  ["move page", "edit.movePageUp"], ["rearrange", "edit.movePageUp"],
  ["organize", "edit.movePageUp"], ["insert page", "edit.insertBlankPage"],
  ["add page", "edit.insertBlankPage"], ["blank", "edit.insertBlankPage"],
  ["crop", "edit.cropToDrag"], ["trim", "edit.cropToDrag"],
  // Looking and finding.
  ["dark", "view.invertPages"], ["night", "view.invertPages"], ["invert", "view.invertPages"],
  ["bookmarks", "view.showOutline"], ["contents", "view.showOutline"],
  ["toc", "view.showOutline"], ["outline", "view.showOutline"],
  ["thumbnails", "view.showThumbnails"], ["sidebar", "view.toggleSidebar"],
  ["search", "find.open"], ["find", "find.open"], ["zoom", "view.zoomIn"],
  ["fit", "view.fitWidth"], ["go to", "nav.goToPage"], ["jump", "nav.goToPage"],
  ["compare", "view.sideBySide"], ["split view", "view.sideBySide"],
  ["side by side", "view.sideBySide"], ["center", "edit.alignCenter"],
  ["duplicate", "edit.duplicate"], ["align", "edit.alignLeft"],
  // Files and the application.
  ["image", "file.fromPictures"], ["jpg", "file.fromPictures"], ["photo", "file.fromPictures"],
  ["picture", "file.fromPictures"], ["properties", "file.properties"],
  ["metadata", "file.properties"], ["info", "file.properties"], ["save as", "file.saveCopy"],
  ["save", "file.save"], ["open", "file.open"], ["close", "file.close"],
  ["print", "file.print"], ["undo", "edit.undo"], ["reload", "file.reload"],
  ["refresh", "file.reload"], ["hidden", "file.findHiddenText"],
  ["update", "app.checkForUpdates"], ["about", "app.about"],
  ["terminal", "app.installCommandLineTool"], ["cli", "app.installCommandLineTool"],
];

describe("what a reader types", () => {
  const commands = everyCommand();

  it("is asked of a registry that has the commands", () => {
    // The control for the two below: a registry of nothing fails neither.
    expect(commands.length).toBeGreaterThan(100);
    expect(TYPED.length).toBeGreaterThan(100);
  });

  it("finds its command in the first three rows", () => {
    const lost = TYPED.filter(([word, id]) => {
      const rows = rank(word, commands).slice(0, 3);
      return !rows.some((row) => row.command.id === id);
    }).map(([word, id]) => `${word} -> ${id}`);
    expect(lost).toEqual([]);
  });

  it("names only commands that exist", () => {
    const known = new Set(commands.map((command) => command.id));
    expect(TYPED.filter(([, id]) => !known.has(id))).toEqual([]);
  });
});

describe("the keyword table", () => {
  const commands = everyCommand();

  it("names only commands that exist", () => {
    // A keyword for an id that was renamed is attached to nothing, and the
    // word goes back to finding nothing with no test above naming it.
    const known = new Set(commands.map((command) => command.id));
    expect(Object.keys(KEYWORDS).filter((id) => !known.has(id))).toEqual([]);
  });

  it("reaches the commands", () => {
    const missing = Object.entries(KEYWORDS).flatMap(([id, words]) => {
      const carried = commands.find((command) => command.id === id)?.keywords ?? [];
      return words.filter((word) => !carried.includes(word)).map((word) => `${id}: ${word}`);
    });
    expect(Object.keys(KEYWORDS).length).toBeGreaterThan(40);
    expect(missing).toEqual([]);
  });

  it("is lower case, since only the query is folded", () => {
    const words = Object.values(KEYWORDS).flat();
    expect(words.length).toBeGreaterThan(100);
    expect(words.filter((word) => word !== word.toLowerCase() || word.trim() !== word)).toEqual([]);
  });

  it("repeats no word of the command's own title", () => {
    // Such a keyword does nothing the title match does not, and outlives it.
    // One does something: six titles begin "Form fields:", so "form" found
    // Fill form in twelfth place by its title, and finds it first by a keyword
    // typed in full.
    const earned = new Set(["edit.fillForm: form"]);
    const repeats = commands.flatMap((command) =>
      (command.keywords ?? [])
        .filter((word) => command.title.toLowerCase().split(/[^a-z0-9]+/).includes(word))
        .filter((word) => !earned.has(`${command.id}: ${word}`))
        .map((word) => `${command.id}: ${word}`),
    );
    expect(repeats).toEqual([]);
  });
});

describe("a word the two locales spell differently", () => {
  it("is given back in the other spelling, alone and in the whole title", () => {
    expect(respelled("Color: yellow")).toEqual(["colour", "colour: yellow"]);
    expect(respelled("Colour: yellow")).toEqual(["color", "color: yellow"]);
    expect(respelled("Language for recognising text")).toEqual([
      "recognizing",
      "language for recognizing text",
    ]);
    expect(respelled("Rotate page anticlockwise")).toEqual([
      "counterclockwise",
      "rotate page counterclockwise",
    ]);
  });

  it("is nothing for a title with one spelling", () => {
    expect(respelled("Merge documents...")).toEqual([]);
  });

  /** A spelling, and the command it must find in either locale. */
  const EITHER: [string, string][] = [
    ["color", "edit.color.yellow"], ["colour", "edit.color.yellow"],
    ["invert page colours", "view.invertPages"], ["invert page colors", "view.invertPages"],
    ["center", "edit.alignCenter"], ["centre", "edit.alignCenter"],
    ["counterclockwise", "edit.rotatePageCounterClockwise"],
    ["anticlockwise", "edit.rotatePageCounterClockwise"],
    ["recognize", "file.recogniseText"], ["recognise", "file.recogniseText"],
    ["recognize text", "file.recogniseText"], ["recognise text", "file.recogniseText"],
  ];

  it.each(["en-US", "en-GB"] as const)("finds its command under both in %s", (locale) => {
    const commands = everyCommand(locale);
    const lost = EITHER.filter(([word, id]) => {
      const rows = rank(word, commands).slice(0, 3);
      return !rows.some((row) => row.command.id === id);
    }).map(([word, id]) => `${word} -> ${id}`);
    expect(lost).toEqual([]);
  });
});
