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
import { UI_LOCALE, type UiLocale } from "./i18n";

/** Keywords for the commands whose ids are written out. */
const NAMED: Record<string, readonly string[]> = {
  "file.mergeDocuments": ["combine", "join", "append", "concatenate"],
  "file.splitDocument": ["separate", "divide"],
  "file.extractPages": ["export pages", "save pages"],
  "file.compress": ["compress", "reduce", "shrink", "optimize", "optimise", "file size"],
  "file.recogniseText": ["ocr", "searchable", "scan", "text recognition"],
  "file.recognisePage": ["ocr", "search recovery", "missing word"],
  "file.recogniseTextLanguage": ["ocr language"],
  "file.protect": ["encrypt", "protect", "lock", "secure", "security"],
  "file.unprotect": ["decrypt", "unlock", "unprotect", "remove password"],
  "file.signDocument": ["signature", "certificate", "digital signature", "esign", "e-sign", "timestamp"],
  "file.addCertificateFile": ["p12", "pfx", "pkcs12", "import", "signing key", "digital id"],
  "file.addValidationData": ["ltv", "revocation"],
  "file.findHiddenText": ["hidden", "invisible"],
  "file.compareWith": ["diff", "differences", "changes", "versions", "redline"],
  "file.addPageNumbers": ["numbering", "number pages", "header", "footer", "bates"],
  "file.addWatermark": ["stamp every page", "draft", "confidential", "background text"],
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
  "edit.drawLine": ["rule", "stroke", "straight", "shape"],
  "edit.drawArrow": ["pointer", "line", "shape"],
  "edit.drawPolygon": ["triangle", "shape", "outline"],
  "edit.drawPolyline": ["polyline", "path", "zigzag", "shape"],
  "edit.addLink": ["hyperlink", "url", "jump", "goto"],
  "edit.addPicture": ["image", "photo", "logo", "insert"],
  "edit.pastePicture": ["image", "photo", "screenshot", "insert"],
  "edit.copyRegionAsPicture": ["snapshot", "image", "png", "area"],
  "file.saveRegionAsPicture": ["snapshot", "image", "png", "export", "area"],
  "file.saveAttachment": ["embedded", "extract", "file", "portfolio"],
  "edit.erase": ["eraser", "rubber"],
  "edit.removeMark": ["delete mark"],
  "edit.copyMark": ["duplicate annotation", "clipboard"],
  "edit.cutMark": ["move annotation", "clipboard"],
  "edit.pasteMark": ["annotation", "clipboard"],
  "edit.removeRedaction": ["delete redaction"],
  "view.invertPages": ["dark", "dark mode", "night", "night mode"],
  "view.singlePage": ["one page", "one column"],
  "view.facing": ["two pages", "two-up", "side by side pages", "spread"],
  "view.book": ["book view", "two pages", "spread"],
  "view.rightToLeft": ["manga", "rtl", "right to left"],
  "view.pageAtATime": ["single page", "non-continuous", "page by page"],
  "view.fullScreen": ["fullscreen", "maximise", "maximize"],
  "view.outlineExpandAll": ["expand all"],
  "view.outlineCollapseAll": ["collapse all"],
  "view.outlineToCurrent": ["where am i"],
  "file.nextInFolder": ["next file", "directory"],
  "nav.addFavourite": ["bookmark this page", "remember page"],
  "file.previousInFolder": ["previous file", "directory"],
  "view.presentation": ["slideshow", "slides", "presentation mode"],
  "view.showOutline": ["bookmarks", "contents", "table of contents", "toc"],
  "view.showThumbnails": ["pages panel"],
  "view.sideBySide": ["compare", "split view"],
  "find.open": ["search"],
  "nav.goToPage": ["go to", "goto", "jump", "page number"],
  "app.installCommandLineTool": ["terminal", "cli", "shell"],
  "app.enableExplorerIntegration": ["thumbnails", "preview pane", "windows explorer"],
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
  ["favorit", "favourit"],
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
  // Words are runs of letters of any script, so that a title with an umlaut
  // or an accent is not cut inside a word.
  const words = title.toLowerCase().split(/([^\p{L}]+)/u);
  const others = words.map(otherSpelling);
  if (others.every((other) => other === null)) return [];
  const whole = words.map((word, index) => others[index] ?? word).join("");
  return [...others.filter((other): other is string => other !== null), whole];
}

/** The keywords written out above, by command id. */
export const KEYWORDS: Record<string, readonly string[]> = NAMED;

/**
 * The German words a reader types for a command when they are not in its
 * German title. Added to the English ones above, not instead of them: a German
 * reader often knows the English word too ("ocr", "redact"). Same rules as
 * the English table: lower case, and no word the German title already has.
 */
export const GERMAN_KEYWORDS: Record<string, readonly string[]> = {
  "file.mergeDocuments": ["kombinieren", "verbinden", "anhängen"],
  "file.splitDocument": ["trennen", "teilen"],
  "file.extractPages": ["seiten exportieren", "seiten speichern"],
  "file.compress": ["komprimieren", "verkleinern", "dateigröße", "optimieren"],
  "file.recogniseText": ["durchsuchbar", "scan", "texterkennung"],
  "file.recognisePage": ["fehlendes wort", "texterkennung"],
  "file.protect": ["verschlüsseln", "schützen", "sperren", "kennwort", "sicherheit"],
  "file.unprotect": ["entschlüsseln", "entsperren", "passwort entfernen", "kennwort"],
  "file.signDocument": ["unterschrift", "unterschreiben", "zertifikat", "digitale signatur", "zeitstempel"],
  "file.addCertificateFile": ["importieren", "signaturschlüssel", "digitale id"],
  "file.findHiddenText": ["versteckt", "unsichtbar"],
  "file.compareWith": ["unterschiede", "änderungen", "versionen"],
  "file.addPageNumbers": ["nummerierung", "seiten nummerieren", "kopfzeile", "fußzeile"],
  "file.addWatermark": ["entwurf", "vertraulich", "hintergrundtext"],
  "file.fromPictures": ["foto", "konvertieren", "umwandeln", "scan"],
  "file.properties": ["metadaten", "info", "autor"],
  "file.saveCopy": ["speichern unter", "exportieren"],
  "file.reload": ["aktualisieren", "zurücksetzen"],
  "file.print": ["drucker"],
  // The three words a German title puts last, where the English one puts it
  // first: "schwärzen" ends seven titles, so it is a keyword of the one a
  // reader means by it, as "form" is of Fill form in English.
  "edit.redactRegion": ["schwärzen", "zensieren", "text entfernen", "abdecken"],
  "find.open": ["suchen"],
  "edit.redactSelection": ["zensieren", "abdecken"],
  "edit.addSignature": ["initialen", "unterschrift", "handschriftlich"],
  "edit.addComment": ["notiz", "haftnotiz", "anmerkung", "kommentieren"],
  "edit.highlightSelection": ["textmarker", "markieren"],
  "edit.strikeoutSelection": ["ausstreichen"],
  "edit.squigglySelection": ["wellig", "geschlängelt"],
  "edit.addTextBox": ["schreiben", "text hinzufügen", "schreibmaschine", "tippen"],
  "edit.editText": ["text ändern", "korrigieren"],
  "edit.fillForm": ["formularfeld"],
  "edit.addCheckbox": ["ankreuzfeld", "häkchen", "checkbox"],
  "edit.addDropdown": ["kombinationsfeld", "liste", "dropdown"],
  "edit.addRadio": ["radiobutton", "optionsschaltfläche"],
  "edit.rotatePageClockwise": ["drehen", "wenden"],
  "edit.rotatePageCounterClockwise": ["wenden"],
  "edit.deletePage": ["seite entfernen"],
  "edit.movePageUp": ["umsortieren", "neu anordnen", "reihenfolge"],
  "edit.movePageDown": ["umsortieren", "neu anordnen", "reihenfolge"],
  "edit.insertBlankPage": ["seite hinzufügen", "neue seite"],
  "edit.insertPages": ["seiten hinzufügen", "seiten importieren"],
  "edit.cropToDrag": ["beschneiden"],
  "edit.cropToContent": ["beschneiden", "ränder"],
  "edit.draw": ["stift", "bleistift", "tinte"],
  "edit.drawBox": ["viereck", "quadrat", "form"],
  "edit.drawEllipse": ["kreis", "oval", "form"],
  "edit.drawLine": ["strich", "gerade", "form"],
  "edit.drawArrow": ["form"],
  "edit.drawPolygon": ["dreieck", "vieleck", "form"],
  "edit.drawPolyline": ["polylinie", "pfad", "zickzack", "form"],
  "edit.addLink": ["hyperlink", "verknüpfung", "sprungziel"],
  "edit.addPicture": ["foto", "logo", "grafik", "einfügen"],
  "edit.pastePicture": ["foto", "screenshot", "grafik"],
  "edit.copyRegionAsPicture": ["schnappschuss", "screenshot", "png"],
  "file.saveRegionAsPicture": ["schnappschuss", "screenshot", "png", "exportieren"],
  "file.saveAttachment": ["eingebettet", "extrahieren", "datei"],
  "edit.erase": ["radiergummi", "löschen"],
  "edit.removeMark": ["markierung löschen"],
  "edit.copyMark": ["zwischenablage", "anmerkung"],
  "edit.cutMark": ["zwischenablage", "anmerkung"],
  "edit.pasteMark": ["zwischenablage", "anmerkung"],
  "edit.removeRedaction": ["schwärzung löschen"],
  "view.invertPages": ["dunkel", "dunkelmodus", "nachtmodus"],
  "view.singlePage": ["eine seite", "einspaltig"],
  "view.facing": ["zwei seiten", "doppelseite"],
  "view.book": ["buchansicht", "zwei seiten", "doppelseite"],
  "view.rightToLeft": ["manga"],
  "view.pageAtATime": ["einzelseite", "nicht fortlaufend"],
  "view.fullScreen": ["maximieren"],
  "view.outlineExpandAll": ["alle aufklappen"],
  "view.outlineCollapseAll": ["alle zuklappen"],
  "view.outlineToCurrent": ["wo bin ich"],
  "nav.addFavourite": ["lesezeichen", "seite merken"],
  "view.presentation": ["diashow", "folien", "präsentationsmodus"],
  "view.showOutline": ["lesezeichen", "inhalt", "inhaltsverzeichnis"],
  "view.showThumbnails": ["seitenübersicht", "vorschaubilder"],
  "view.sideBySide": ["vergleichen", "geteilte ansicht"],
  "nav.goToPage": ["springen", "seitennummer"],
  "app.installCommandLineTool": ["terminal", "kommandozeile", "konsole", "cli"],
  "app.enableExplorerIntegration": ["vorschau", "miniaturansichten"],
  "app.checkForUpdates": ["aktualisieren", "aktualisierung", "version"],
  "app.about": ["version", "lizenz"],
};

/**
 * Gives each registered command its keywords.
 *
 * As a getter and not a value, because a title can be one: "Move tab to right
 * side" is worded from where the tab is when it is asked for, and reading every
 * title here, once, would read it before there is a window to ask.
 *
 * In German, a command also carries its German keywords and its American
 * title, which `english` reads, so that English typing still finds it. The
 * English spellings are derived from that title, not from the German one.
 */
export function attachKeywords(
  registry: CommandRegistry,
  locale: UiLocale = UI_LOCALE,
  english?: (id: string) => string | undefined,
): void {
  const german = locale === "de-DE";
  for (const command of registry.all()) {
    const named = [...(KEYWORDS[command.id] ?? []), ...(german ? (GERMAN_KEYWORDS[command.id] ?? []) : [])];
    Object.defineProperty(command, "keywords", {
      get: () => {
        if (!english) return [...named, ...respelled(command.title)];
        const title = english(command.id);
        return title === undefined ? named : [...named, title.toLowerCase(), ...respelled(title)];
      },
      enumerable: true,
      configurable: true,
    });
  }
}
