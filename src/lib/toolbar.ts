import type { CommandRegistry } from "./commands";
import { PALETTE, type Swatch } from "./markcolors";
import { NIBS } from "./marknibs";
import { catalog, message, UI_LOCALE, type UiLocale } from "./i18n";
import type { IconName } from "./icons";
import type { MarkKind } from "./pages";

/**
 * The labels of the tool row, its menus and the window menu.
 *
 * A label is a shorter form of its command's title, in that title's words
 * (`toolbar.test.ts` holds it in English and in German), so a German label is
 * worded from the German title in `appcommands.ts` and not translated afresh.
 */
const TEXT = catalog({
  "en-US": {
    comment: "Comment",
    textBox: "Text box",
    editText: "Edit text",
    sign: "Sign",
    document: "Document",
    reload: "Reload from disk",
    saveCopy: "Save a copy...",
    compress: "Save a smaller copy...",
    saveRegionAsPicture: "Save a region as a picture...",
    saveAttachment: "Save an attachment...",
    protect: "Save a copy with a password...",
    unprotect: "Save a copy without its password...",
    addPageNumbers: "Save a copy with page numbers...",
    addWatermark: "Save a copy with a watermark...",
    signField: "Sign in the signature field...",
    addValidationData: "Add long-term validation data...",
    print: "Print...",
    properties: "Document properties",
    findHiddenText: "Find text the pages do not show",
    compareWith: "Compare with...",
    highlight: "Highlight",
    highlightSelection: "Highlight selection",
    underlineSelection: "Underline selection",
    strikeoutSelection: "Strike out selection",
    squigglySelection: "Squiggly underline selection",
    draw: "Draw",
    drawFreehand: "Draw freehand",
    box: "Box",
    ellipse: "Ellipse",
    line: "Line",
    arrow: "Arrow",
    polygon: "Polygon",
    connectedLines: "Connected lines",
    link: "Link",
    pictureFromFile: "Picture from a file...",
    pictureFromClipboard: "Picture from the clipboard",
    stampApproved: "Approved stamp",
    stampConfidential: "Confidential stamp",
    stampDraft: "Draft stamp",
    stampFinal: "Final stamp",
    signaturePicture: "Signature picture, without a certificate...",
    erase: "Erase marks",
    form: "Form",
    fillForm: "Fill form",
    addTextField: "Add a text field",
    addMultilineField: "Add a text field on several lines",
    addCheckbox: "Add a checkbox",
    addDropdown: "Add a dropdown...",
    addRadio: "Add radio buttons...",
    addSignatureField: "Add a signature field",
    fieldBorderOn: "Draw a line round new text fields",
    fieldBorderOff: "No line round new text fields",
    formEditOn: "Change the document's own fields",
    formEditOff: "Finish changing the document's fields",
    fieldProperties: "Properties of the picked field...",
    tabOrder: "Tab through fields in reading order",
    pages: "Pages",
    thumbnails: "Page thumbnails",
    rotatePageClockwise: "Rotate page clockwise",
    cropToDrag: "Crop by dragging",
    cropToContent: "Crop to content",
    resetCrop: "Reset crop",
    insertBlankPage: "Insert blank page",
    insertPages: "Insert pages from file...",
    movePageUp: "Move page up",
    movePageDown: "Move page down",
    extractPages: "Extract pages...",
    splitDocument: "Split document...",
    mergeDocuments: "Merge documents...",
    deletePage: "Delete page",
    redact: "Redact",
    redactRegion: "Redact text or regions by dragging",
    redactSelection: "Redact selection",
    redactMatches: "Redact every search result",
    showRedactions: "Show redactions",
    redactRasterCopy: "Redact to image-only copy...",
    redactCopy: "Redact and save as...",
    redactDocument: "Redact and save",
    width: "Width",
    documents: "Documents",
    favourites: "Favorites",
    outline: "Outline",
    sideBySide: "Side by side",
    settings: "Settings",
    unavailable: "Unavailable",
  },
  "en-GB": {
    favourites: "Favourites",
  },
  "de-DE": {
    comment: "Kommentar",
    textBox: "Textkasten",
    editText: "Text bearbeiten",
    sign: "Signieren",
    document: "Dokument",
    reload: "Neu laden",
    saveCopy: "Kopie speichern...",
    compress: "Kleinere Kopie speichern...",
    saveRegionAsPicture: "Bereich als Bild speichern...",
    saveAttachment: "Anhang speichern...",
    protect: "Kopie mit Passwort speichern...",
    unprotect: "Kopie ohne Passwort speichern...",
    addPageNumbers: "Kopie mit Seitenzahlen speichern...",
    addWatermark: "Kopie mit Wasserzeichen speichern...",
    signField: "Im Signaturfeld signieren...",
    addValidationData: "Langzeitvalidierungsdaten hinzufügen...",
    print: "Drucken...",
    properties: "Dokumenteigenschaften",
    findHiddenText: "Text finden, den die Seiten nicht zeigen",
    compareWith: "Vergleichen mit...",
    highlight: "Hervorheben",
    highlightSelection: "Auswahl hervorheben",
    underlineSelection: "Auswahl unterstreichen",
    strikeoutSelection: "Auswahl durchstreichen",
    squigglySelection: "Auswahl mit Wellenlinie unterstreichen",
    draw: "Zeichnen",
    drawFreehand: "Freihand zeichnen",
    box: "Rechteck",
    ellipse: "Ellipse",
    line: "Linie",
    arrow: "Pfeil",
    polygon: "Polygon",
    connectedLines: "Linienzug",
    link: "Link",
    pictureFromFile: "Bild aus einer Datei...",
    pictureFromClipboard: "Bild aus der Zwischenablage",
    // The stamp's own word stays English: it is the word the stamp draws.
    stampApproved: "Stempel „Approved“",
    stampConfidential: "Stempel „Confidential“",
    stampDraft: "Stempel „Draft“",
    stampFinal: "Stempel „Final“",
    signaturePicture: "Unterschriftsbild ohne Zertifikat...",
    erase: "Markierungen radieren",
    form: "Formular",
    fillForm: "Formular ausfüllen",
    addTextField: "Textfeld hinzufügen",
    addMultilineField: "Mehrzeiliges Textfeld hinzufügen",
    addCheckbox: "Kontrollkästchen hinzufügen",
    addDropdown: "Auswahlliste hinzufügen...",
    addRadio: "Optionsfelder hinzufügen...",
    addSignatureField: "Signaturfeld hinzufügen",
    fieldBorderOn: "Linie um neue Textfelder ziehen",
    fieldBorderOff: "Keine Linie um neue Textfelder",
    formEditOn: "Eigene Felder des Dokuments ändern",
    formEditOff: "Ändern der Felder des Dokuments beenden",
    fieldProperties: "Eigenschaften des ausgewählten Felds...",
    tabOrder: "Tabulatorreihenfolge nach Lesereihenfolge",
    pages: "Seiten",
    thumbnails: "Seitenminiaturen",
    rotatePageClockwise: "Seite im Uhrzeigersinn drehen",
    cropToDrag: "Durch Ziehen zuschneiden",
    cropToContent: "Auf den Inhalt zuschneiden",
    resetCrop: "Zuschnitt zurücksetzen",
    insertBlankPage: "Leere Seite einfügen",
    insertPages: "Seiten aus Datei einfügen...",
    movePageUp: "Seite nach oben verschieben",
    movePageDown: "Seite nach unten verschieben",
    extractPages: "Seiten extrahieren...",
    splitDocument: "Dokument aufteilen...",
    mergeDocuments: "Dokumente zusammenführen...",
    deletePage: "Seite löschen",
    redact: "Schwärzen",
    redactRegion: "Text oder Bereiche durch Ziehen schwärzen",
    redactSelection: "Auswahl schwärzen",
    redactMatches: "Alle Suchtreffer schwärzen",
    showRedactions: "Schwärzungen anzeigen",
    redactRasterCopy: "Schwärzen als Kopie nur aus Bildern...",
    redactCopy: "Schwärzen und speichern unter...",
    redactDocument: "Schwärzen und speichern",
    width: "Linienstärke",
    documents: "Dokumente",
    favourites: "Favoriten",
    outline: "Gliederung",
    sideBySide: "Nebeneinander",
    settings: "Einstellungen",
    unavailable: "Nicht verfügbar",
  },
});

export interface ToolItem {
  id: string;
  label: string;
  swatch?: Swatch;
  /** Drawn beside the label in the tool row. */
  icon?: IconName;
  /** A line above this item, where a menu's items fall into kinds. */
  rule?: boolean;
}

export interface ToolGroup {
  /** Stable identity: translated labels must never decide behavior. */
  id: "document" | "highlight" | "draw" | "form" | "pages" | "redact" | "color" | "width";
  label: string;
  /** Drawn beside the label on the group's own button. */
  icon?: IconName;
  items: ToolItem[];
}

/**
 * The buttons of the tool row, in `locale`.
 *
 * Presentation only: command execution and guards stay in the registry.
 */
export function toolActions(locale: UiLocale = UI_LOCALE): ToolItem[] {
  const text = TEXT(locale);
  return [
    { id: "edit.addComment", label: text.comment, icon: "comment" },
    { id: "edit.addTextBox", label: text.textBox, icon: "textBox" },
    { id: "edit.editText", label: text.editText, icon: "editText" },
    // Signing with a certificate, which draws the reader's signature image,
    // timestamps and locks. Until 2026-10-13 this button placed a picture with
    // no certificate behind it (`edit.addSignature`, still in the menu and the
    // palette under a name that says so).
    { id: "file.signDocument", label: text.sign, icon: "signature" },
  ];
}

/** The tool row's buttons in the language the window is in. */
export const TOOL_ACTIONS: ToolItem[] = toolActions();

/** The tool row's menus, in `locale`. */
export function toolGroups(locale: UiLocale = UI_LOCALE): ToolGroup[] {
  const text = TEXT(locale);
  return [
    {
      id: "document",
      label: text.document,
      icon: "document",
      // The File menu's commands that act on the open document, in its order.
      // Windows has no menu bar, so for a reader there this is that menu, and
      // what is not listed is reached through the palette alone. Signing has a
      // button of its own in the row.
      items: [
        { id: "file.reload", label: text.reload },
        { id: "file.saveCopy", label: text.saveCopy, rule: true },
        { id: "file.recogniseText", label: message("recogniseText", locale) },
        { id: "file.recognisePage", label: message("recognisePage", locale) },
        { id: "file.compress", label: text.compress },
        { id: "file.saveRegionAsPicture", label: text.saveRegionAsPicture },
        { id: "file.saveAttachment", label: text.saveAttachment },
        { id: "file.protect", label: text.protect },
        { id: "file.unprotect", label: text.unprotect },
        { id: "file.addPageNumbers", label: text.addPageNumbers },
        { id: "file.addWatermark", label: text.addWatermark },
        { id: "file.signField", label: text.signField, rule: true },
        { id: "file.addValidationData", label: text.addValidationData },
        { id: "file.print", label: text.print, rule: true },
        { id: "file.properties", label: text.properties, rule: true },
        { id: "file.findHiddenText", label: text.findHiddenText },
        { id: "file.compareWith", label: text.compareWith },
      ],
    },
    {
      id: "highlight",
      label: text.highlight,
      icon: "highlight",
      items: [
        { id: "edit.highlightSelection", label: text.highlightSelection },
        { id: "edit.underlineSelection", label: text.underlineSelection },
        { id: "edit.strikeoutSelection", label: text.strikeoutSelection },
        { id: "edit.squigglySelection", label: text.squigglySelection },
      ],
    },
    {
      id: "draw",
      label: text.draw,
      icon: "draw",
      items: [
        { id: "edit.draw", label: text.drawFreehand },
        { id: "edit.drawBox", label: text.box },
        { id: "edit.drawEllipse", label: text.ellipse },
        { id: "edit.drawLine", label: text.line },
        { id: "edit.drawArrow", label: text.arrow },
        { id: "edit.drawPolygon", label: text.polygon },
        { id: "edit.drawPolyline", label: text.connectedLines },
        { id: "edit.addLink", label: text.link },
        { id: "edit.addPicture", label: text.pictureFromFile },
        { id: "edit.pastePicture", label: text.pictureFromClipboard },
        { id: "edit.stamp.approved", label: text.stampApproved },
        { id: "edit.stamp.confidential", label: text.stampConfidential },
        { id: "edit.stamp.draft", label: text.stampDraft },
        { id: "edit.stamp.final", label: text.stampFinal },
        { id: "edit.addSignature", label: text.signaturePicture },
        { id: "edit.erase", label: text.erase, rule: true },
      ],
    },
    {
      // The menu bar's Form menu, in its order. Both border commands are listed
      // because only one of the two is enabled at a time, and the other is greyed.
      id: "form",
      label: text.form,
      icon: "form",
      items: [
        { id: "edit.fillForm", label: text.fillForm },
        { id: "edit.addTextField", label: text.addTextField },
        { id: "edit.addMultilineField", label: text.addMultilineField },
        { id: "edit.addCheckbox", label: text.addCheckbox },
        { id: "edit.addDropdown", label: text.addDropdown },
        { id: "edit.addRadio", label: text.addRadio },
        { id: "edit.addSignatureField", label: text.addSignatureField },
        { id: "edit.fieldBorderOn", label: text.fieldBorderOn },
        { id: "edit.fieldBorderOff", label: text.fieldBorderOff },
        { id: "edit.formEditOn", label: text.formEditOn },
        { id: "edit.formEditOff", label: text.formEditOff },
        { id: "edit.fieldProperties", label: text.fieldProperties },
        { id: "edit.tabOrder", label: text.tabOrder },
      ],
    },
    {
      id: "pages",
      label: text.pages,
      icon: "pages",
      items: [
        { id: "view.showThumbnails", label: text.thumbnails },
        { id: "edit.rotatePageClockwise", label: text.rotatePageClockwise },
        { id: "edit.rotatePageCounterClockwise", label: message("rotatePageCounter", locale) },
        { id: "edit.cropToDrag", label: text.cropToDrag },
        { id: "edit.cropToContent", label: text.cropToContent },
        { id: "edit.resetCrop", label: text.resetCrop },
        { id: "edit.insertBlankPage", label: text.insertBlankPage },
        { id: "edit.insertPages", label: text.insertPages },
        { id: "edit.movePageUp", label: text.movePageUp },
        { id: "edit.movePageDown", label: text.movePageDown },
        { id: "file.extractPages", label: text.extractPages },
        { id: "file.splitDocument", label: text.splitDocument },
        { id: "file.mergeDocuments", label: text.mergeDocuments },
        { id: "edit.deletePage", label: text.deletePage },
      ],
    },
    {
      id: "redact",
      label: text.redact,
      icon: "redact",
      items: [
        { id: "edit.redactRegion", label: text.redactRegion },
        { id: "edit.redactSelection", label: text.redactSelection },
        { id: "edit.redactMatches", label: text.redactMatches },
        { id: "view.showRedactions", label: text.showRedactions, rule: true },
        { id: "file.redactRasterCopy", label: text.redactRasterCopy, rule: true },
        { id: "file.redactCopy", label: text.redactCopy },
        { id: "file.redactDocument", label: text.redactDocument },
      ],
    },
    {
      id: "color",
      label: message("color", locale),
      items: PALETTE.map((entry) => ({
        id: `edit.color.${entry.id}`,
        label: entry.name[0]!.toUpperCase() + entry.name.slice(1),
        swatch: entry,
      })),
    },
    {
      id: "width",
      label: text.width,
      icon: "width",
      items: NIBS.map((entry) => ({
        id: `edit.nib.${entry.id}`,
        label: `${entry.name[0]!.toUpperCase()}${entry.name.slice(1)} (${entry.pt} pt)`,
      })),
    },
  ];
}

/** The tool row's menus in the language the window is in. */
export const TOOL_GROUPS: ToolGroup[] = toolGroups();

/** One part of {@link WINDOW_MENU}, under its own heading. */
export interface MenuSection {
  label: string;
  /** Command ids. The menu shows each command's title, so it has no wording of its own. */
  items: string[];
}

/**
 * The header's menu where there is no menu bar, which is Windows.
 *
 * What the tool row's menus do not carry: commands about documents and tabs
 * and not about the open one, the settings, and the application's own. On
 * macOS these are in the menu bar and this menu is not drawn. Without it a
 * reader on Windows reached them through the palette alone, and "About",
 * "Check for updates" and every setting are things looked for in a menu.
 *
 * One menu and not three buttons: each part is short, and none is used often
 * enough to earn a place in the row.
 */
export function windowMenu(locale: UiLocale = UI_LOCALE): MenuSection[] {
  const text = TEXT(locale);
  return [
    {
      label: text.documents,
      items: [
        "file.fromPictures", "file.closeAll", "file.closeOtherTabs", "file.closeTabsLeft",
        "file.closeTabsRight", "file.reopenClosedTab", "file.nextInFolder", "file.previousInFolder",
        "file.reopenLastTabs", "file.clearRecents",
      ],
    },
    {
      label: text.favourites,
      items: ["nav.addFavourite", "nav.removeFavourite", "nav.nextFavourite", "nav.previousFavourite"],
    },
    {
      label: text.outline,
      items: ["view.outlineExpandAll", "view.outlineCollapseAll", "view.outlineToCurrent"],
    },
    {
      label: text.pages,
      items: [
        "view.singlePage", "view.facing", "view.book", "view.rightToLeft",
        "view.continuous", "view.pageAtATime", "view.fullScreen", "view.presentation",
      ],
    },
    {
      label: text.sideBySide,
      items: [
        "view.sideBySide", "view.moveToOtherSide", "view.bothSides", "view.switchSides",
        "view.focusOtherSide", "view.syncScrolling", "nav.nextChange", "nav.previousChange",
      ],
    },
    {
      label: text.settings,
      items: [
        "file.reopenTabsAtLaunch", "file.reopenLastDocumentAtLaunch",
        "file.onDiskChange.ask", "file.onDiskChange.reload", "file.onDiskChange.ignore",
        "file.recogniseTextLanguage",
        "app.language.system", "app.language.en-US", "app.language.en-GB", "app.language.de-DE",
        "view.tabLabelsLarger", "view.tabLabelsSmaller", "view.tabLabelsDefault",
      ],
    },
    {
      label: "tpdf",
      items: [
        "app.about", "app.checkForUpdates", "app.installUpdate", "app.restartForUpdate",
        "app.disableAutomaticUpdates", "app.enableAutomaticUpdates",
        "app.installCommandLineTool", "app.uninstallCommandLineTool", "app.makeDefaultPdfApp",
        "app.enableExplorerIntegration", "app.disableExplorerIntegration",
      ],
    },
  ];
}

/** The window menu in the language the window is in. */
export const WINDOW_MENU: MenuSection[] = windowMenu();

/**
 * The commands a button in the header runs. `App.svelte` draws them, and they
 * are named here so that the snapshot below covers them and a test can count
 * them as on screen.
 */
export const HEADER_COMMANDS = [
  "file.save", "file.saveCopy", "file.print", "edit.undo", "edit.redo",
  "nav.previousPage", "nav.nextPage", "nav.goToPage", "view.zoomIn",
  "view.zoomOut", "view.fitWidth", "view.fitPage", "view.fitHeight",
  "view.fitContent", "view.actualSize", "view.zoomTo",
];

/** Refresh from the shell's status/edit notifications; guards are not reactive. */
export function toolbarState(
  registry: CommandRegistry,
): Record<string, { enabled: boolean; title: string }> {
  const ids = [
    ...HEADER_COMMANDS,
    ...TOOL_ACTIONS.map((item) => item.id),
    ...TOOL_GROUPS.flatMap((group) => group.items.map((item) => item.id)),
    ...WINDOW_MENU.flatMap((section) => section.items),
  ];
  return Object.fromEntries(ids.map((id) => {
    const command = registry.find(id);
    return [id, {
      enabled: command !== undefined && (command.enabled?.() ?? true),
      title: command
        ? `${command.title}${command.keys ? ` (${command.keys})` : ""}`
        : TEXT().unavailable,
    }];
  }));
}

/** What the viewer reports as armed; `ViewerStatus.armed`'s type. */
export type ArmedTool = MarkKind | "crop" | "redact" | "place" | "region" | null;

/** The kinds a chosen colour changes. A stamp and a signature keep their own. */
const COLORED: readonly ArmedTool[] = [
  "note", "square", "ellipse", "textbox", "ink", "line", "arrow", "polygon", "polyline",
];

/** The kinds drawn as a line, which is what a width is the width of. */
const STROKED: readonly ArmedTool[] = ["square", "ellipse", "ink", "line", "arrow", "polygon", "polyline"];

/**
 * Which of the colour and width controls the armed tool's status row carries.
 *
 * They used to sit at the end of the tool row always, and took a row of their
 * own in a window of the default width, for two settings that only matter to
 * the next mark. Now each is offered exactly while the tool that is armed would
 * use it, in the row that names that tool --- a row that is already there, so
 * arming a tool moves the page once and choosing a colour never does.
 *
 * A highlight is not an armed tool: it is applied to a selection in one click.
 * Its colour is chosen in the Highlight menu, beside the four kinds, and that
 * is deliberately not this function's business --- a control that appeared on
 * the tool row whenever text was selected would shift the toolbar under a
 * reader who is in the middle of dragging out a selection.
 *
 * `drawing` is asked separately from `armed` because the viewer reports an armed
 * pen through `drawing` alone --- see `ViewerStatus.armed`.
 */
export function styleOptions(tool: {
  armed: ArmedTool;
  drawing: boolean;
  erasing: boolean;
}): { color: boolean; width: boolean } {
  if (tool.erasing) return { color: false, width: false };
  return {
    color: tool.drawing || COLORED.includes(tool.armed),
    width: tool.drawing || STROKED.includes(tool.armed),
  };
}

/** The least room a menu keeps between itself and the window's edge, in pixels. */
export const POPUP_MARGIN = 8;

/**
 * How far to move a menu sideways so that all of it is inside the window.
 *
 * A menu opens under its button, aligned to one of its edges, and that is
 * decided in the stylesheet without knowing where the button is. At 1,000 px
 * the More button wraps to the start of a second row, and its menu, aligned to
 * the button's right edge, opened to the left of the window with only its last
 * letters showing; the Redact menu at the end of the row can run off the right
 * the same way. So the menu is measured once it is open and moved by this much.
 *
 * `left` and `right` are the menu's edges and `viewport` the window's width.
 * Positive moves right. A menu wider than the window is put at the left margin,
 * because the start of each line is the part that says which item it is.
 */
export function popupShift(left: number, right: number, viewport: number): number {
  if (left < POPUP_MARGIN || right - left > viewport - 2 * POPUP_MARGIN) {
    return POPUP_MARGIN - left;
  }
  const limit = viewport - POPUP_MARGIN;
  return right > limit ? limit - right : 0;
}
