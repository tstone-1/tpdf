/**
 * The words a reader types for a command when they are not the title's.
 *
 * One table by command id, so that the vocabulary can be read and argued with
 * in one place, and so that a title can be reworded without losing the words
 * that led to it. `commands.ts` says how a keyword matches and what it scores.
 *
 * What belongs here: another program's name for the same thing ("combine",
 * "bookmarks") and the word for the result where the title names the action
 * ("compress" for a smaller copy). What does not: a word already in the title,
 * which the title match finds, a word for something the command does not do,
 * and the other spelling of a title's word, which {@link respelled} adds.
 *
 * Every keyword is lower case, since `keywordMatch` folds the query and not
 * the keyword. `keywords.test.ts` holds that, holds every id here against the
 * registry, and holds a list of typed words against what they must find.
 */

import type { CommandRegistry } from "./commands";

/** Keywords for the commands whose ids are written out. */
const NAMED: Record<string, readonly string[]> = {
  "file.mergeDocuments": ["combine", "join", "append", "concatenate"],
  "file.splitDocument": ["separate", "divide"],
  "file.extractPages": ["export pages", "save pages"],
  "file.compress": ["compress", "reduce", "shrink", "optimize", "optimise", "file size"],
  "file.recogniseText": ["ocr", "searchable", "scan", "text recognition"],
  "file.recogniseTextLanguage": ["ocr language"],
  "file.protect": ["encrypt", "protect", "lock", "secure", "security"],
  "file.unprotect": ["decrypt", "unlock", "unprotect", "remove password"],
  "file.signDocument": ["signature", "certificate", "digital signature", "esign", "e-sign", "timestamp"],
  "file.addCertificateFile": ["p12", "pfx", "pkcs12", "import", "signing key", "digital id"],
  "file.addValidationData": ["ltv", "revocation"],
  "file.findHiddenText": ["hidden", "invisible"],
  "file.fromPictures": ["image", "jpg", "jpeg", "png", "photo", "scan", "convert"],
  "file.properties": ["metadata", "info", "author"],
  "file.saveCopy": ["save as", "export"],
  "file.reload": ["refresh", "revert"],
  "file.print": ["printer"],
  "edit.redactRegion": ["blackout", "black out", "censor", "remove text"],
  "edit.redactSelection": ["blackout", "black out"],
  "edit.addSignature": ["initials", "handwritten"],
  "edit.addComment": ["note", "sticky note", "annotate", "annotation"],
  "edit.highlightSelection": ["marker"],
  "edit.strikeoutSelection": ["strikethrough", "strike through", "cross out"],
  "edit.squigglySelection": ["wavy"],
  "edit.addTextBox": ["type", "typewriter", "write", "add text"],
  "edit.editText": ["change text", "modify text", "correct"],
  "edit.fillForm": ["form"],
  "edit.addTextField": ["field"],
  "edit.addCheckbox": ["check box", "tick box"],
  "edit.addDropdown": ["combo box", "list"],
  "edit.addRadio": ["option buttons"],
  "edit.rotatePageClockwise": ["turn"],
  "edit.rotatePageCounterClockwise": ["turn"],
  "edit.deletePage": ["remove page"],
  "edit.movePageUp": ["reorder", "rearrange", "organize", "organise"],
  "edit.movePageDown": ["reorder", "rearrange", "organize", "organise"],
  "edit.insertBlankPage": ["add page", "new page"],
  "edit.insertPages": ["add pages", "import pages"],
  "edit.cropToDrag": ["trim"],
  "edit.cropToContent": ["trim", "margins"],
  "edit.draw": ["pen", "pencil", "ink"],
  "edit.drawBox": ["rectangle", "square", "shape"],
  "edit.drawEllipse": ["circle", "oval", "shape"],
  "edit.erase": ["eraser", "rubber"],
  "edit.removeMark": ["delete mark"],
  "edit.removeRedaction": ["delete redaction"],
  "view.invertPages": ["dark", "dark mode", "night", "night mode"],
  "view.singlePage": ["one page", "one column"],
  "view.facing": ["two pages", "two-up", "side by side pages", "spread"],
  "view.book": ["book view", "two pages", "spread"],
  "view.rightToLeft": ["manga", "rtl", "right to left"],
  "view.pageAtATime": ["single page", "non-continuous", "page by page"],
  "view.fullScreen": ["fullscreen", "maximise", "maximize"],
  "view.presentation": ["slideshow", "slides", "presentation mode"],
  "view.showOutline": ["bookmarks", "contents", "table of contents", "toc"],
  "view.showThumbnails": ["pages panel"],
  "view.sideBySide": ["compare", "split view"],
  "find.open": ["search"],
  "nav.goToPage": ["go to", "goto", "jump", "page number"],
  "app.installCommandLineTool": ["terminal", "cli", "shell"],
  "app.checkForUpdates": ["upgrade", "version"],
  "app.about": ["version", "licence", "license"],
};

/**
 * Stems spelled one way in American English and another in British.
 *
 * A title is written in the reader's locale (`i18n.ts`), and a reader types the
 * spelling they learned, which need not be their system's.
 */
const SPELLINGS: readonly (readonly [string, string])[] = [
  ["color", "colour"],
  ["center", "centre"],
  ["counterclockwise", "anticlockwise"],
  ["recogniz", "recognis"],
  ["organiz", "organis"],
  ["optimiz", "optimis"],
];

/** `word` in the other spelling, or null when it has only one. */
function otherSpelling(word: string): string | null {
  for (const [american, british] of SPELLINGS) {
    if (word.startsWith(american)) return british + word.slice(american.length);
    if (word.startsWith(british)) return american + word.slice(british.length);
  }
  return null;
}

/**
 * The keywords a title earns by containing a word with two spellings.
 *
 * Two for each such title: the word, so that `colour` finds "Color: yellow",
 * and the whole title respelled, so that a reader who types the title as they
 * would spell it finds it too. Derived from the title and not listed, so a
 * command added tomorrow with "color" in its name needs no entry here.
 */
export function respelled(title: string): string[] {
  const words = title.toLowerCase().split(/([^a-z]+)/);
  const others = words.map(otherSpelling);
  if (others.every((other) => other === null)) return [];
  const whole = words.map((word, index) => others[index] ?? word).join("");
  return [...others.filter((other): other is string => other !== null), whole];
}

/** The keywords written out above, by command id. */
export const KEYWORDS: Record<string, readonly string[]> = NAMED;

/**
 * Gives each registered command its keywords.
 *
 * As a getter and not a value, because a title can be one: "Move tab to right
 * side" is worded from where the tab is when it is asked for, and reading every
 * title here, once, would read it before there is a window to ask.
 */
export function attachKeywords(registry: CommandRegistry): void {
  for (const command of registry.all()) {
    const named = KEYWORDS[command.id] ?? [];
    Object.defineProperty(command, "keywords", {
      get: () => [...named, ...respelled(command.title)],
      enumerable: true,
      configurable: true,
    });
  }
}
