/**
 * The command list the application actually has, and the window shortcuts.
 *
 * This was inside `App.svelte`, and `docs/PLAN.md` recorded the consequence as a
 * gap: *"Cmd-K itself and the command list `App.svelte` registers are covered by
 * nothing."* The palette check built its own four-command registry, so what it
 * proved was that the palette works --- not that any command a reader can type
 * is wired to anything. The same sentence `palette.ts` opens with says why: a
 * component that only exists inside `App.svelte` is a component the check cannot
 * reach, because `viewercheck.ts` runs *instead of* the shell booting.
 *
 * So the list moved out to where the check can reach it. Nothing about it
 * changed on the way --- the commands, their titles, their `enabled` guards and
 * their argument validators are the ones that were there, and `App.svelte` calls
 * {@link registerAppCommands} where it used to call `register` itself.
 *
 * ## What the seam is, and what it costs
 *
 * A command's `run` reaches one of two places. Most reach the {@link Viewer},
 * which the check owns a real one of, so those are end-to-end: type "fit width",
 * press Enter, and the viewer the reader would be looking at changes. The rest
 * reach the shell --- a file dialog, a print panel, a Svelte `$state` flag for
 * the sidebar --- and there is no shell in the check. Those arrive through
 * {@link AppActions}, which the check implements with recorders.
 *
 * Be precise about what that buys. For a shell action the check asserts the
 * *wiring* --- that this command, found by the name a reader types, reaches that
 * action exactly once --- and says nothing about what the action then does.
 * `toggleInvert` writing the preference to the session is not covered here and
 * is not claimed to be. The wiring is the half that was covered by nothing.
 */

import {
  pick as pickAttachment,
  placeholder as attachmentPlaceholder,
  preview as attachmentPreview,
  type Embedded,
} from "./attachments";
import { CommandRegistry } from "./commands";
import { attachKeywords } from "./keywords";
import { BINDINGS, copyInterfaceText, inTextField, isLinux, label, matches, type BoundCommand } from "./keys";
import {
  describeRange,
  describeSplit,
  parsePageRange,
  parseSplitPoints,
} from "./pageranges";
import { DISK_CHANGE_MODES, type DiskChangeMode } from "./diskwatch";
import { PALETTE } from "./markcolors";
import type { CloseWhich } from "./tabclose";
import { kindOf, SPREAD_TITLES, withKind, type SpreadKind } from "./spreads";
import { catalog, LOCALE_NAMES, message, UI_LOCALES, UI_LOCALE, type LanguageChoice, type UiLocale } from "./i18n";
import { NIBS } from "./marknibs";
import {
  pick as pickLanguage,
  placeholder as languagePlaceholder,
  preview as languagePreview,
  type Offered,
} from "./ocrlanguage";
import { PAGE_SIZES, PAGE_SIZE_NAMES, type PageSizeName } from "./pagesizes";
import type { MarkKind, StampName } from "./pages";
import type { Tab } from "./sidebar";
import type { Viewer } from "./viewer";
import { MAX_ZOOM, MIN_ZOOM, parseZoomPercent, percentOf } from "./zoom";
import {
  chosenPages,
  rangePlaceholder,
  rangePreview,
  type PreparedImport,
} from "./pendingimport";
import { lacks, type Arrangement } from "./arrange";
import { parseChoices, parseGroup } from "./fieldnames";
import { partnerPlaceholder, pickPartner } from "./panes";

/**
 * Every title and argument text the commands show, in each locale.
 *
 * Keys follow the command id where there is one. The ids themselves never
 * change with the language: menus, bindings and the window checks find a
 * command by its id.
 */
const TEXT = catalog({
  "en-US": {
    closeTab: "Close tab",
    closeAll: "Close all tabs",
    closeOtherTabs: "Close other tabs",
    closeTabsLeft: "Close tabs to the left",
    closeTabsRight: "Close tabs to the right",
    nextInFolder: "Open the next PDF in this folder",
    previousInFolder: "Open the previous PDF in this folder",
    reopenClosedTab: "Reopen closed tab",
    reopenLastTabs: "Reopen the tabs from last time",
    reopenTabsAtLaunch: "At launch: reopen all tabs",
    reopenLastDocumentAtLaunch: "At launch: reopen only the last document",
    clearRecents: "Clear recent documents",
    tabLabelsLarger: "Larger tab labels",
    tabLabelsSmaller: "Smaller tab labels",
    tabLabelsDefault: "Default tab label size",
    nextTab: "Next tab",
    previousTab: "Previous tab",
    sideBySide: "Show side by side with...",
    moveToOtherSide: (toRight: boolean) => `Move tab to ${toRight ? "right" : "left"} side`,
    bothSides: "Show this document on both sides",
    switchSides: "Switch sides",
    focusOtherSide: "Go to other side",
    scrollSeparately: "Scroll the two sides separately",
    scrollTogether: "Scroll both sides together",
    open: "Open document",
    fromPictures: "New document from pictures...",
    reload: "Reload from disk",
    onDiskChangeAsk: "When the file changes on disk: ask before reloading",
    onDiskChangeReload: "When the file changes on disk: reload automatically",
    onDiskChangeIgnore: "When the file changes on disk: do nothing",
    about: "About tpdf",
    checkForUpdates: "Check for updates",
    disableAutomaticUpdates: "Disable automatic update checks",
    enableAutomaticUpdates: "Enable automatic update checks",
    installUpdate: "Install update",
    restartForUpdate: "Restart to finish update",
    installCommandLineTool: "Install command-line tool…",
    uninstallCommandLineTool: "Uninstall command-line tool…",
    makeDefaultPdfApp: "Make tpdf the default PDF app",
    enableExplorerIntegration: "Show PDFs in Explorer with tpdf",
    disableExplorerIntegration: "Stop showing PDFs in Explorer with tpdf",
    findHiddenText: "Find text the pages do not show",
    compareWith: "Compare with...",
    properties: "Document properties",
    print: "Print",
    findOpen: "Find in document",
    findNext: "Find next",
    findPrevious: "Find previous",
    matchCase: "Find: match case on or off",
    wholeWord: "Find: whole words on or off",
    inSelection: "Find: in selection on or off",
    regex: "Find: regular expression on or off",
    zoomIn: "Zoom in",
    zoomOut: "Zoom out",
    fitWidth: "Fit width",
    fitPage: "Fit page",
    fitHeight: "Fit height",
    fitContent: "Fit content",
    actualSize: "Actual size",
    zoomTo: "Zoom to…",
    zoomPlaceholder: "Zoom, in percent",
    zoomRange: (min: number, max: number) => `Zoom, ${min} to ${max}%`,
    notAZoom: (raw: string, min: number, max: number) =>
      `"${raw}" is not a zoom between ${min}% and ${max}%`,
    zoomPreview: (percent: number) => `Zoom to ${percent}%`,
    rotateClockwise: "Rotate view clockwise",
    addFavourite: "Add this page to favorites…",
    favouritePlaceholder: "A name for this page, or Enter for its number",
    addFavouriteAs: (name: string) => `Add as "${name}"`,
    addFavouriteNumbered: "Add, named after the page",
    removeFavourite: "Remove this page from favorites",
    nextFavourite: "Next favorite in this document",
    previousFavourite: "Previous favorite in this document",
    outlineExpandAll: "Open every entry of the outline",
    outlineCollapseAll: "Close every entry of the outline",
    outlineToCurrent: "Show this page's entry in the outline",
    fullScreen: "Full screen, or back to the window",
    presentation: "Present one page at a time, or stop presenting",
    continuous: "Pages: scroll continuously",
    pageAtATime: "Pages: one page or pair at a time",
    rightToLeft: "Facing pages: read right to left, or left to right again",
    rotatePageClockwise: "Rotate page clockwise",
    addComment: "Add comment",
    drawBox: "Draw a box...",
    drawEllipse: "Draw an ellipse...",
    drawLine: "Add a line...",
    drawArrow: "Add an arrow...",
    drawPolygon: "Add a polygon, a corner per click...",
    drawPolyline: "Add connected lines, a corner per click...",
    addLink: "Add a link...",
    addPicture: "Add a picture from a file...",
    pastePicture: "Paste a picture from the clipboard",
    editText: "Edit existing text...",
    addSignature: "Place a signature picture, without a certificate...",
    stamp: (name: string) => `Stamp ${name}...`,
    addTextBox: "Add a text box...",
    addTextField: "Add a form field: text...",
    addMultilineField: "Add a form field: text on several lines...",
    addCheckbox: "Add a form field: checkbox...",
    addSignatureField: "Add a form field: signature...",
    addDropdown: "Add a form field: dropdown...",
    dropdownPlaceholder: "Choices, with a semicolon between them",
    dropdownPreview: (count: number) =>
      `${count === 1 ? "1 choice" : `${count} choices`}. Then drag the field onto the page.`,
    fieldBorderOn: "Form fields: draw a line round new text fields",
    fieldBorderOff: "Form fields: no line round new text fields",
    formEditOn: "Form fields: change the document's own fields",
    formEditOff: "Form fields: finish changing the document's fields",
    tabOrder: "Form fields: tab through them in reading order",
    addRadio: "Add a form field: radio buttons...",
    radioPlaceholder: "The name of the group, such as Payment",
    radioPreview: (group: string) =>
      `Drag a button of ${group} onto the page, and another for each choice. Escape finishes.`,
    fieldProperties: "Form fields: properties of the picked field...",
    duplicate: "Arrange: duplicate",
    alignLeft: "Arrange: align left",
    alignRight: "Arrange: align right",
    alignTop: "Arrange: align top",
    alignMiddle: "Arrange: align middle",
    alignBottom: "Arrange: align bottom",
    distributeAcross: "Arrange: distribute horizontally",
    distributeDown: "Arrange: distribute vertically",
    sameWidth: "Arrange: same width",
    sameHeight: "Arrange: same height",
    sameSize: "Arrange: same size",
    draw: "Draw freehand...",
    erase: "Erase marks...",
    highlightSelection: "Highlight selection",
    underlineSelection: "Underline selection",
    strikeoutSelection: "Strike out selection",
    squigglySelection: "Squiggly underline selection",
    nib: (name: string) => `Nib: ${name}`,
    editForeignMark: "Edit this comment",
    deleteComment: "Delete this comment",
    replyToComment: "Reply to this comment",
    removeMark: "Remove mark",
    copyMark: "Copy mark",
    cutMark: "Cut mark",
    pasteMark: "Paste mark",
    redactRegion: "Redact text or regions by dragging",
    redactSelection: "Redact selection",
    redactMatches: "Redact every search result",
    removeRedaction: "Remove this redaction",
    fillForm: "Fill form",
    cropToDrag: "Crop page by dragging",
    cropToContent: "Crop page to content",
    resetCrop: "Reset page crop",
    deletePage: "Delete page",
    insertBlankPage: "Insert blank page",
    insertSizedPage: (size: string) => `Insert blank ${size} page`,
    insertPages: "Insert pages from file...",
    insertPagesFrom: (file: string) => `Insert pages from ${file}`,
    insertPagesFromChosen: "Insert pages from the chosen file",
    pagesPlaceholder: "Pages",
    noFileWaiting: "No file is waiting to be inserted",
    movePageUp: "Move page up",
    movePageDown: "Move page down",
    undo: "Undo",
    redo: "Redo",
    save: "Save",
    saveCopy: "Save a copy...",
    signDocument: "Sign document…",
    signField: "Sign in the signature field…",
    addCertificateFile: "Add a certificate file for signing…",
    addValidationData: "Add long-term validation data…",
    noLanguageList: "No list of languages is waiting",
    compress: "Save a smaller copy...",
    saveRegionAsPicture: "Save a region as a picture...",
    saveAttachment: "Save an attachment...",
    saveAttachmentChoice: "Save which attachment",
    noAttachmentList: "No list of attachments is waiting",
    protect: "Save a copy with a password...",
    unprotect: "Save a copy without its password...",
    addPageNumbers: "Save a copy with page numbers...",
    addWatermark: "Save a copy with a watermark...",
    redactRasterCopy: "Redact to image-only copy...",
    redactCopy: "Redact and save as…",
    redactDocument: "Redact and save",
    extractPages: "Extract pages...",
    extractPlaceholder: "Pages, e.g. 1-3,5",
    splitDocument: "Split document...",
    splitPlaceholder: "Cut after pages, e.g. 3,7",
    mergeDocuments: "Merge documents...",
    nextPage: "Next page",
    previousPage: "Previous page",
    back: "Back",
    forward: "Forward",
    nextLink: "Next link",
    previousLink: "Previous link",
    nextMark: "Next mark",
    previousMark: "Previous mark",
    nextChange: "Next change",
    previousChange: "Previous change",
    firstPage: "Go to start",
    lastPage: "Go to end",
    goToPage: "Go to page…",
    pageNumberPlaceholder: "Page number",
    pageNumberRange: (pages: number) => `Page number, 1 to ${pages}`,
    notAPageNumber: (raw: string) => `"${raw}" is not a page number`,
    documentPages: (pages: number) => `This document has ${pages} page${pages === 1 ? "" : "s"}`,
    goToPagePreview: (page: number, pages: number) => `Go to page ${page} of ${pages}`,
    selectAll: "Select all on page",
    copy: "Copy selection",
    copyRegionAsPicture: "Copy a region as a picture",
    clearSelection: "Clear selection",
    toggleSidebar: "Toggle sidebar",
    showOutline: "Show outline",
    showThumbnails: "Show page thumbnails",
    showMarks: "Show my marks",
    showRedactions: "Show redactions",
  },
  "en-GB": {
    addFavourite: "Add this page to favourites…",
    removeFavourite: "Remove this page from favourites",
    nextFavourite: "Next favourite in this document",
    previousFavourite: "Previous favourite in this document",
  },
  "de-DE": {
    closeTab: "Tab schließen",
    closeAll: "Alle Tabs schließen",
    closeOtherTabs: "Andere Tabs schließen",
    closeTabsLeft: "Tabs links schließen",
    closeTabsRight: "Tabs rechts schließen",
    nextInFolder: "Nächstes PDF in diesem Ordner öffnen",
    previousInFolder: "Vorheriges PDF in diesem Ordner öffnen",
    reopenClosedTab: "Geschlossenen Tab wieder öffnen",
    reopenLastTabs: "Tabs vom letzten Mal wieder öffnen",
    reopenTabsAtLaunch: "Beim Start: alle Tabs wieder öffnen",
    reopenLastDocumentAtLaunch: "Beim Start: nur das letzte Dokument wieder öffnen",
    clearRecents: "Liste der zuletzt geöffneten Dokumente leeren",
    tabLabelsLarger: "Größere Tab-Beschriftungen",
    tabLabelsSmaller: "Kleinere Tab-Beschriftungen",
    tabLabelsDefault: "Standardgröße der Tab-Beschriftungen",
    nextTab: "Nächster Tab",
    previousTab: "Vorheriger Tab",
    sideBySide: "Nebeneinander anzeigen mit...",
    moveToOtherSide: (toRight: boolean) => `Tab auf die ${toRight ? "rechte" : "linke"} Seite verschieben`,
    bothSides: "Dieses Dokument auf beiden Seiten anzeigen",
    switchSides: "Seiten tauschen",
    focusOtherSide: "Zur anderen Seite wechseln",
    scrollSeparately: "Beide Seiten getrennt scrollen",
    scrollTogether: "Beide Seiten gemeinsam scrollen",
    open: "Dokument öffnen",
    fromPictures: "Neues Dokument aus Bildern...",
    reload: "Neu laden",
    onDiskChangeAsk: "Wenn sich die Datei auf dem Datenträger ändert: vor dem Neuladen fragen",
    onDiskChangeReload: "Wenn sich die Datei auf dem Datenträger ändert: automatisch neu laden",
    onDiskChangeIgnore: "Wenn sich die Datei auf dem Datenträger ändert: nichts tun",
    about: "Über tpdf",
    checkForUpdates: "Nach Updates suchen",
    disableAutomaticUpdates: "Automatische Suche nach Updates ausschalten",
    enableAutomaticUpdates: "Automatische Suche nach Updates einschalten",
    installUpdate: "Update installieren",
    restartForUpdate: "Neu starten, um das Update abzuschließen",
    installCommandLineTool: "Befehlszeilenwerkzeug installieren…",
    uninstallCommandLineTool: "Befehlszeilenwerkzeug deinstallieren…",
    makeDefaultPdfApp: "tpdf als Standard-App für PDFs festlegen",
    enableExplorerIntegration: "PDFs im Explorer mit tpdf anzeigen",
    disableExplorerIntegration: "PDFs im Explorer nicht mehr mit tpdf anzeigen",
    findHiddenText: "Text finden, den die Seiten nicht zeigen",
    compareWith: "Vergleichen mit...",
    properties: "Dokumenteigenschaften",
    print: "Drucken",
    findOpen: "Im Dokument suchen",
    findNext: "Nächsten Treffer suchen",
    findPrevious: "Vorherigen Treffer suchen",
    matchCase: "Suchen: Groß-/Kleinschreibung beachten ein oder aus",
    wholeWord: "Suchen: ganze Wörter ein oder aus",
    inSelection: "Suchen: in der Auswahl ein oder aus",
    regex: "Suchen: regulärer Ausdruck ein oder aus",
    zoomIn: "Vergrößern",
    zoomOut: "Verkleinern",
    fitWidth: "Seitenbreite",
    fitPage: "Ganze Seite",
    fitHeight: "Seitenhöhe",
    fitContent: "Inhalt einpassen",
    actualSize: "Originalgröße",
    zoomTo: "Zoomen auf…",
    zoomPlaceholder: "Zoom in Prozent",
    zoomRange: (min: number, max: number) => `Zoom, ${min} bis ${max} %`,
    notAZoom: (raw: string, min: number, max: number) =>
      `„${raw}“ ist kein Zoom zwischen ${min} % und ${max} %`,
    zoomPreview: (percent: number) => `Zoomen auf ${percent} %`,
    rotateClockwise: "Ansicht im Uhrzeigersinn drehen",
    addFavourite: "Diese Seite zu den Favoriten hinzufügen…",
    favouritePlaceholder: "Name für diese Seite oder Eingabe für die Seitenzahl",
    addFavouriteAs: (name: string) => `Als „${name}“ hinzufügen`,
    addFavouriteNumbered: "Hinzufügen, nach der Seitenzahl benannt",
    removeFavourite: "Diese Seite aus den Favoriten entfernen",
    nextFavourite: "Nächster Favorit in diesem Dokument",
    previousFavourite: "Vorheriger Favorit in diesem Dokument",
    outlineExpandAll: "Alle Einträge der Gliederung aufklappen",
    outlineCollapseAll: "Alle Einträge der Gliederung zuklappen",
    outlineToCurrent: "Eintrag dieser Seite in der Gliederung zeigen",
    fullScreen: "Vollbild oder zurück zum Fenster",
    presentation: "Seite für Seite präsentieren oder Präsentation beenden",
    continuous: "Seiten: fortlaufend scrollen",
    pageAtATime: "Seiten: einzeln oder paarweise blättern",
    rightToLeft: "Doppelseiten: von rechts nach links lesen oder wieder von links nach rechts",
    rotatePageClockwise: "Seite im Uhrzeigersinn drehen",
    addComment: "Kommentar hinzufügen",
    drawBox: "Rechteck zeichnen...",
    drawEllipse: "Ellipse zeichnen...",
    drawLine: "Linie hinzufügen...",
    drawArrow: "Pfeil hinzufügen...",
    drawPolygon: "Polygon hinzufügen, eine Ecke pro Klick...",
    drawPolyline: "Linienzug hinzufügen, eine Ecke pro Klick...",
    addLink: "Link hinzufügen...",
    addPicture: "Bild aus einer Datei hinzufügen...",
    pastePicture: "Bild aus der Zwischenablage einfügen",
    editText: "Vorhandenen Text bearbeiten...",
    addSignature: "Unterschriftsbild ohne Zertifikat platzieren...",
    stamp: (name: string) => `Stempel „${name}“...`,
    addTextBox: "Textkasten hinzufügen...",
    addTextField: "Formularfeld hinzufügen: Textfeld...",
    addMultilineField: "Formularfeld hinzufügen: mehrzeiliges Textfeld...",
    addCheckbox: "Formularfeld hinzufügen: Kontrollkästchen...",
    addSignatureField: "Formularfeld hinzufügen: Signaturfeld...",
    addDropdown: "Formularfeld hinzufügen: Auswahlliste...",
    dropdownPlaceholder: "Optionen, durch Semikolon getrennt",
    dropdownPreview: (count: number) =>
      `${count === 1 ? "1 Option" : `${count} Optionen`}. Ziehe dann das Feld auf die Seite.`,
    fieldBorderOn: "Formularfelder: Linie um neue Textfelder ziehen",
    fieldBorderOff: "Formularfelder: keine Linie um neue Textfelder",
    formEditOn: "Formularfelder: eigene Felder des Dokuments ändern",
    formEditOff: "Formularfelder: Ändern der Felder des Dokuments beenden",
    tabOrder: "Formularfelder: Tabulatorreihenfolge nach Lesereihenfolge festlegen",
    addRadio: "Formularfeld hinzufügen: Optionsfelder...",
    radioPlaceholder: "Der Name der Gruppe, etwa Zahlung",
    radioPreview: (group: string) =>
      `Ziehe eine Schaltfläche von „${group}“ auf die Seite und eine weitere für jede Option. Esc beendet.`,
    fieldProperties: "Formularfelder: Eigenschaften des ausgewählten Felds...",
    duplicate: "Anordnen: duplizieren",
    alignLeft: "Anordnen: links ausrichten",
    alignRight: "Anordnen: rechts ausrichten",
    alignTop: "Anordnen: oben ausrichten",
    alignMiddle: "Anordnen: vertikal mittig ausrichten",
    alignBottom: "Anordnen: unten ausrichten",
    distributeAcross: "Anordnen: horizontal verteilen",
    distributeDown: "Anordnen: vertikal verteilen",
    sameWidth: "Anordnen: gleiche Breite",
    sameHeight: "Anordnen: gleiche Höhe",
    sameSize: "Anordnen: gleiche Größe",
    draw: "Freihand zeichnen...",
    erase: "Markierungen radieren...",
    highlightSelection: "Auswahl hervorheben",
    underlineSelection: "Auswahl unterstreichen",
    strikeoutSelection: "Auswahl durchstreichen",
    squigglySelection: "Auswahl mit Wellenlinie unterstreichen",
    nib: (name: string) => `Linienstärke: ${name}`,
    editForeignMark: "Diesen Kommentar bearbeiten",
    deleteComment: "Diesen Kommentar löschen",
    replyToComment: "Auf diesen Kommentar antworten",
    removeMark: "Markierung entfernen",
    copyMark: "Markierung kopieren",
    cutMark: "Markierung ausschneiden",
    pasteMark: "Markierung einfügen",
    redactRegion: "Text oder Bereiche durch Ziehen schwärzen",
    redactSelection: "Auswahl schwärzen",
    redactMatches: "Alle Suchtreffer schwärzen",
    removeRedaction: "Diese Schwärzung entfernen",
    fillForm: "Formular ausfüllen",
    cropToDrag: "Seite durch Ziehen zuschneiden",
    cropToContent: "Seite auf den Inhalt zuschneiden",
    resetCrop: "Zuschnitt der Seite zurücksetzen",
    deletePage: "Seite löschen",
    insertBlankPage: "Leere Seite einfügen",
    insertSizedPage: (size: string) => `Leere Seite im Format ${size} einfügen`,
    insertPages: "Seiten aus Datei einfügen...",
    insertPagesFrom: (file: string) => `Seiten aus ${file} einfügen`,
    insertPagesFromChosen: "Seiten aus der gewählten Datei einfügen",
    pagesPlaceholder: "Seiten",
    noFileWaiting: "Es wartet keine Datei darauf, eingefügt zu werden",
    movePageUp: "Seite nach oben verschieben",
    movePageDown: "Seite nach unten verschieben",
    undo: "Rückgängig",
    redo: "Wiederholen",
    save: "Speichern",
    saveCopy: "Kopie speichern...",
    signDocument: "Dokument signieren…",
    signField: "Im Signaturfeld signieren…",
    addCertificateFile: "Zertifikatsdatei zum Signieren hinzufügen…",
    addValidationData: "Langzeitvalidierungsdaten hinzufügen…",
    noLanguageList: "Es wartet keine Liste von Sprachen",
    compress: "Kleinere Kopie speichern...",
    saveRegionAsPicture: "Bereich als Bild speichern...",
    saveAttachment: "Anhang speichern...",
    saveAttachmentChoice: "Welchen Anhang speichern",
    noAttachmentList: "Es wartet keine Liste von Anhängen",
    protect: "Kopie mit Passwort speichern...",
    unprotect: "Kopie ohne Passwort speichern...",
    addPageNumbers: "Kopie mit Seitenzahlen speichern...",
    addWatermark: "Kopie mit Wasserzeichen speichern...",
    redactRasterCopy: "Schwärzen als Kopie nur aus Bildern...",
    redactCopy: "Schwärzen und speichern unter…",
    redactDocument: "Schwärzen und speichern",
    extractPages: "Seiten extrahieren...",
    extractPlaceholder: "Seiten, z. B. 1-3,5",
    splitDocument: "Dokument aufteilen...",
    splitPlaceholder: "Trennen nach den Seiten, z. B. 3,7",
    mergeDocuments: "Dokumente zusammenführen...",
    nextPage: "Nächste Seite",
    previousPage: "Vorherige Seite",
    back: "Zurück",
    forward: "Vorwärts",
    nextLink: "Nächster Link",
    previousLink: "Vorheriger Link",
    nextMark: "Nächste Markierung",
    previousMark: "Vorherige Markierung",
    nextChange: "Nächste Änderung",
    previousChange: "Vorherige Änderung",
    firstPage: "Zum Anfang",
    lastPage: "Zum Ende",
    goToPage: "Gehe zu Seite…",
    pageNumberPlaceholder: "Seitenzahl",
    pageNumberRange: (pages: number) => `Seitenzahl, 1 bis ${pages}`,
    notAPageNumber: (raw: string) => `„${raw}“ ist keine Seitenzahl`,
    documentPages: (pages: number) => `Dieses Dokument hat ${pages} ${pages === 1 ? "Seite" : "Seiten"}`,
    goToPagePreview: (page: number, pages: number) => `Gehe zu Seite ${page} von ${pages}`,
    selectAll: "Alles auf der Seite auswählen",
    copy: "Auswahl kopieren",
    copyRegionAsPicture: "Bereich als Bild kopieren",
    clearSelection: "Auswahl aufheben",
    toggleSidebar: "Seitenleiste ein- oder ausblenden",
    showOutline: "Gliederung anzeigen",
    showThumbnails: "Seitenminiaturen anzeigen",
    showMarks: "Meine Markierungen anzeigen",
    showRedactions: "Schwärzungen anzeigen",
  },
  "de-DE-formal": {
    dropdownPreview: (count: number) =>
      `${count === 1 ? "1 Option" : `${count} Optionen`}. Ziehen Sie dann das Feld auf die Seite.`,
    radioPreview: (group: string) =>
      `Ziehen Sie eine Schaltfläche von „${group}“ auf die Seite und eine weitere für jede Option. Esc beendet.`,
  },
});

/** One title per choice, each a whole sentence so the palette reads it alone. */
function diskChangeTitle(mode: DiskChangeMode, locale: UiLocale): string {
  const text = TEXT(locale);
  return { ask: text.onDiskChangeAsk, reload: text.onDiskChangeReload, ignore: text.onDiskChangeIgnore }[mode];
}

/**
 * What each stamp is called in the palette.
 *
 * Title case rather than the upper case a stamp is *drawn* in: the drawn word
 * shouts because a stamp does, and a command list that shouted four of its
 * entries would read as an error. `stampWord` in `pages.ts` is the drawn form,
 * and the two being different spellings of one name is deliberate --- the same
 * arrangement every `MarkKind` already has between its serde name, its PDF name
 * and the word a reader sees.
 *
 * English in every language, for now: the stamp draws `stampWord`, which is
 * English, and a German title naming a word the stamp does not draw would
 * promise something else. The two are translated together or not at all.
 */
const STAMP_TITLES: Record<StampName, string> = {
  approved: "Approved",
  confidential: "Confidential",
  draft: "Draft",
  final: "Final",
};

/**
 * Everything a command needs that is not the viewer.
 *
 * Deliberately not "the app": each member is one verb, so a check can record
 * which of them a command reached. A single `app` object with the whole
 * component behind it would make the seam untestable again.
 */
export interface AppActions {
  /** Focus an editable form field on its page. */
  fillForm: () => void;
  /**
   * The open document's surface, or null.
   *
   * A function rather than a value because the registry outlives any document:
   * it is read at call time, so closing and opening one does not need the
   * commands rebuilt.
   */
  viewer(): Viewer | null;
  /** How many pages the open document has, for the "Go to page" validator. */
  pageCount(): number;
  /** Ask for a file and open it. */
  openDocument(): void;
  /** Close the active document, checking for unsaved work. */
  closeDocument(): void;
  /** Close every open document, asking once if any has unsaved work. */
  closeAllDocuments(): void;
  /**
   * The window's two sides: whether two documents are shown, which side the
   * reader is working in, and the file names of the other open documents in
   * tab order.
   */
  sides(): { split: boolean; focused: "left" | "right"; others: string[] };
  /** Show the other document at `index` of `sides().others` beside this one. */
  showBeside(index: number): void;
  /** Move the tab in front to the side the reader is not in. */
  moveToOtherSide(): void;
  /** Exchange the two sides. */
  switchSides(): void;
  /** Show the focused document on the other side as well, as a second view of it. */
  showOnBothSides(): void;
  /** Work in the other side. */
  focusOtherSide(): void;
  /** Whether the two sides scroll together. */
  syncScrolling(): boolean;
  toggleSyncScrolling(): void;
  /** Whether a launch reopens every tab rather than the last document alone. */
  restoreTabs(): boolean;
  setRestoreTabs(restore: boolean): void;
  /** The interface language the reader chose, or "system". Applies at the next start. */
  languageChoice(): LanguageChoice;
  setLanguageChoice(choice: LanguageChoice): void;
  /** How many of last time's tabs are not open now. */
  tabsToReopen(): number;
  /** Open them, behind the document on screen. */
  reopenLastTabs(): void;
  /** How many remembered documents the palette and the blank page offer. */
  recentDocuments(): number;
  /** Forget every remembered document. See `startpage.ts`. */
  clearRecentDocuments(): void;
  /** The document tabs' label size, read for the resize commands' guards. */
  tabLabels(): { canGrow: boolean; canShrink: boolean; isDefault: boolean };
  /** Make the tab labels a step larger (1), smaller (-1) or the default (0). */
  resizeTabLabels(direction: -1 | 0 | 1): void;
  /** Move through the open document tabs. */
  nextDocument(delta: number): void;
  /** Number of open document tabs. */
  documentCount(): number;
  /** Open the current document's path again, keeping the reader's place. */
  reloadDocument(): void;
  /** What happens when the open file changes on disk. See `diskwatch.ts`. */
  diskChangeMode(): DiskChangeMode;
  setDiskChangeMode(mode: DiskChangeMode): void;
  /**
   * Whether an open is already in flight.
   *
   * Only the keyboard route consults it, which is how it was before this moved:
   * the Open *button* carries the same guard as `disabled`, and the palette row
   * carries none. Preserved rather than unified --- making the palette guard too
   * would be a behaviour change nobody asked for while moving code.
   */
  busyOpening(): boolean;
  /** Whether a document-wide task currently blocks opening and editing. */
  busyDocument?(): boolean;
  /** Hand the document to the platform print panel. */
  printDocument(): void;
  /** Put the caret in the find field. */
  focusFind(): void;
  /** Flip one matching option and rescan. */
  toggleSearchOption(which: "matchCase" | "wholeWord" | "regex"): void;
  /** Confine the search to the selection, or let it see the document again. */
  toggleSearchScope(): void;
  /** Show or hide the sidebar. */
  toggleSidebar(): void;
  /** Show one of the sidebar's tabs. */
  showTab(tab: Tab): void;
  /** Invert the page colours, and remember it. */
  toggleInvert(): void;
  /** Whether closing these tabs from the one in front would close any. */
  canCloseTabs(which: CloseWhich): boolean;
  /** Close every other tab, or those before or after the one in front. */
  closeTabs(which: CloseWhich): void;
  /** Whether a document closed in this session is not open again. */
  canReopenClosedTab(): boolean;
  /** Open again the document closed most recently. */
  reopenClosedTab(): void;
  /** Opens the next (`1`) or previous (`-1`) PDF in the folder of the one in front. */
  openInFolder(step: 1 | -1): void;
  /** Whether the page being read is a favourite. */
  isFavourite(): boolean;
  /** Marks the page being read as a favourite, named `name` or after its number. */
  addFavourite(name: string): void;
  /** Unmarks the page being read. */
  removeFavourite(): void;
  /** Whether this document has a favourite page after (`1`) or before (`-1`) this one. */
  canStepFavourite(step: 1 | -1): boolean;
  /** Goes to that favourite. */
  stepFavourite(step: 1 | -1): void;
  /** Whether the document in front has an outline. */
  hasOutline(): boolean;
  /**
   * Shows the outline, with every entry open (`all`), closed (`none`), or open
   * down to the entry the reader is in (`current`).
   */
  arrangeOutline(how: "all" | "none" | "current"): void;
  /** Fill the screen with the document, or go back to the window. */
  toggleFullScreen(): void;
  /** Present one page at a time on black, or stop presenting. */
  togglePresentation(): void;
  /** Say which version this is, without asking the network anything. */
  about(): void;
  /** Ask the update endpoint whether there is a newer tpdf. */
  checkForUpdates(): void;
  /**
   * Link the command-line tool as `/usr/local/bin/tpdf` (`install`), or remove
   * that link; on Windows, say where the installer put `tpdf-cli.exe`. The
   * backend decides and reads the result back (`clitool.rs`); this side shows
   * the sentence.
   */
  commandLineTool(install: boolean): void;
  /**
   * Whether installing (`install`) or uninstalling it has anything to do or to
   * say, as last read; true while that is not known (`clitoolstate.ts`).
   */
  commandLineToolOffered(install: boolean): boolean;
  /**
   * Make tpdf the application that opens PDFs; on Windows, open Settings where
   * that is chosen. Only ever run from this command: tpdf does not ask.
   */
  makeDefaultPdfApp(): void;
  /**
   * Turn tpdf's Explorer thumbnails and preview on (`on`) or off, asking first
   * before another program's handler is replaced (`explorerstate.ts`).
   */
  explorerIntegration(on: boolean): void;
  /** Whether turning it on (`on`) or off has anything to do; never off Windows. */
  explorerOffered(on: boolean): boolean;
  automaticUpdates(): boolean;
  setAutomaticUpdates(enabled: boolean): void;
  /** Download and apply the update the last check found. */
  applyUpdate(): void;
  /**
   * End this process and start it again, so an applied update takes effect.
   *
   * Asks about unsaved work first, exactly as closing the window does --- a
   * relaunch does not go through the window's close handler, so the question
   * has to be asked on this side. `update.ts`'s `finishUpdate` is where it is,
   * and where the reader's "no" is respected without changing the state.
   */
  restartForUpdate(): void;
  /**
   * Whether an update is downloaded and waiting for a relaunch.
   *
   * Read by the guards of both update commands that act, so that "Install update"
   * is offered only when there is one and is withdrawn once it is applied, and
   * "Restart to finish update" appears exactly when the install is the thing
   * already done --- the two states are different and a single "is there an
   * update" flag would leave the install live after it had already run and
   * would never say when a restart is what is left.
   */
  updateReady(): boolean;
  /** Whether the last check found an update that has not been applied. */
  updateAvailable(): boolean;
  /**
   * Turn the page the reader is on by `delta` quarter-turns, in the document.
   *
   * A shell action rather than a viewer method, because the model that decides
   * what a page's turn becomes is in the backend: the viewer is *told* the
   * answer. See `edits.ts`.
   */
  rotatePage(delta: number): void;
  deletePage(): void;
  /**
   * Put a blank page after the one the reader is on.
   *
   * A shell action for `rotatePage`'s reason, with one of its own: the page's
   * size comes from the scroller, which is the only thing that knows how big
   * the page in front of the reader is. See `edits.ts`.
   */
  insertBlankPage(): void;
  /**
   * Put a blank page of a named size after the one the reader is on.
   *
   * The sibling of {@link AppActions.insertBlankPage} and deliberately not a
   * parameter on it: one of the two asks the scroller how big the page in front
   * of the reader is and the other asks a table, so they share a destination
   * and nothing else. See `pagesizes.ts` for why the sizes are a menu of names
   * rather than a prompt for two numbers.
   */
  insertSizedPage(name: PageSizeName): void;
  /**
   * Put every page of a file the reader picks after the one they are on.
   *
   * A shell action because it opens a dialog and then an edit, and the page it
   * lands after is the viewer's; the backend opens the file and owns it. See
   * `edits.ts`'s `importPages`.
   */
  importPages(): void;
  /**
   * The file `importPages` opened and is waiting to be told which pages of, or
   * null. Read by the range question's `enabled`, title and placeholder, so it
   * is asked at the moment the palette asks rather than captured.
   */
  pendingImport(): PreparedImport | null;
  /**
   * Insert these pages of the waiting file, zero-based and in its order, after
   * the page the reader is on.
   */
  insertChosenPages(pages: number[]): void;
  /** Stop waiting for an answer, releasing the file. See `pendingimport.ts`. */
  dropImport(): void;
  /**
   * Crop the page the reader is on to the box its ink occupies, or put the
   * file's own box back.
   *
   * One action with two arguments rather than two actions, which is the shape
   * `rotatePage` has and for the same reason: two entry points into one call is
   * where the second one drifts.
   */
  cropPage(to: "content" | "reset" | "drag"): void;
  /**
   * Arm the redaction tool: the next drag marks a region for removal.
   *
   * **Its own action rather than a fourth argument to `cropPage`**, which is
   * the shape it most resembles and the one thing it must not share. The two
   * gestures are the same drag with opposite outcomes, and a single entry point
   * choosing between them on a string is one typo away from cropping a page a
   * reader asked to redact.
   *
   * Marking removes nothing. `docs/PLAN.md` §6 keeps applying behind a command
   * of its own so that a reader reviews the list first.
   */
  redactRegion(): void;
  /**
   * Marks the selected text for removal, one region per line it covers.
   *
   * The other way of saying what to remove, and the one that needs no aim: a
   * dragged rectangle is a guess about where the words are, and this names the
   * glyphs. `docs/PLAN.md` §6's step 1 either way --- it destroys nothing, the
   * regions join the review list, and undo takes them back off.
   *
   * Separate from {@link redactRegion} rather than a mode of it, for
   * {@link markSelection}'s reason one level up: a reader who has selected the
   * words wants them marked in one press, and a command that asked which way
   * would cost two.
   */
  redactSelection(): void;
  /**
   * Marks every search match for removal, one region per line each covers.
   *
   * Step 1's third and last shape, and the one that does work the other two
   * cannot: every occurrence of an order number across two hundred pages is not
   * a thing anybody drags or selects. The pattern is whatever is already in the
   * find field, so the reader has seen the results before they mark them ---
   * which is the same principle the review list is, one step earlier.
   *
   * Destroys nothing, like both its siblings.
   */
  redactMatches(): void;
  /** How many matches the last search found, for {@link redactMatches}. */
  matchCount(): number;
  /**
   * Move the page the reader is on `delta` slots along, in the document.
   *
   * A signed step rather than a pair of commands, so that the two palette
   * entries are one action with two arguments --- the same shape `rotatePage`
   * has, and for the same reason: two entry points into one call is where the
   * second one drifts.
   */
  movePage(delta: number): void;
  /** Step the edit journal back one command. */
  undoEdit(): void;
  /** Step the edit journal forward one command. */
  redoEdit(): void;
  /** Whether there is an edit to undo. */
  canUndo(): boolean;
  /** Whether there is an edit to redo. */
  canRedo(): boolean;
  /**
   * Highlight the selected text, as a real annotation on the document.
   *
   * A shell action rather than a viewer method, for the reason `rotatePage`
   * gives: the *selection* is the viewer's and the *mark* is the model's, and
   * what a highlight becomes --- its id, whether it is accepted at all --- is
   * the journal's answer, replayed on undo.
   */
  markSelection(kind: MarkKind): void;
  /**
   * Drops a comment on the page, at a point or wherever the shell decides.
   *
   * Separate from {@link markSelection} rather than a fourth kind passed to it,
   * even though the model *does* treat it as a fourth kind. The two take
   * different things --- one takes a selection, this takes a point --- and a
   * `markSelection("note")` would silently make a comment the size of whatever
   * happened to be selected, which is the shape of mistake a type should make
   * unsayable rather than a comment warn about.
   *
   * The point is `null` from the palette and the menu bar, which have no
   * pointer --- and with no point this **arms** the tool rather than placing
   * anything: the reader's next press on a page is where the bubble goes. It
   * used to place immediately, at `commentAt`'s no-pointer answer, which a
   * reader reported as a command that ignored where they were looking. A
   * right-click still places at once, since that gesture has already said
   * where. See `Viewer.armDraw`, `paintCommentGhost` and `placeComment`.
   */
  addComment(at: { clientX: number; clientY: number } | null): void;
  /**
   * Arms the box tool: the reader's next drag on a page draws one.
   *
   * **Arms rather than draws**, which is why it takes no rectangle and why it
   * is not `markSelection("square")`. The other three take a selection and this
   * one takes a gesture the reader has not made yet --- so unlike every command
   * beside it, running this changes nothing about the document and everything
   * about what the next press means. See `Viewer.armDraw` for why a mode is
   * unavoidable here and why it is one-shot.
   */
  drawBox(): void;
  /**
   * Arms the ellipse tool: the reader's next drag on a page draws one.
   *
   * A second command rather than an argument to {@link drawBox}, for the reason
   * {@link draw} gives below: a reader who wants a ring wants it in one press,
   * and a command that asked which shape afterwards would be two presses to say
   * one thing. The gesture it reads is the box's exactly --- two corners --- and
   * that is what makes them separate commands rather than separate modes.
   */
  drawEllipse(): void;
  /**
   * Arms one of the four shapes of straight lines: a line or an arrow, drawn
   * by one drag end to end, or a polygon or a polyline, drawn a corner per
   * click and finished with Enter. One method taking the kind, for
   * {@link stamp}'s reason: the four commands carry the distinction.
   */
  drawShape(kind: "line" | "arrow" | "polygon" | "polyline" | "link"): void;
  /**
   * Arms the stamp tool for one of the four standard stamps.
   *
   * **One method taking the name, where the two shapes above are two methods.**
   * The shapes differ in what is drawn and a stamp differs only in what it says,
   * so four methods here would be four copies of one line --- and the four
   * commands that call it already carry the distinction where a reader meets it,
   * which is the palette. `edit.color.*` is the same arrangement.
   */
  stamp(name: StampName): void;
  /**
   * Arms the text box tool: the reader's next drag on a page draws one.
   *
   * The same gesture as {@link drawBox} and {@link drawEllipse}, and a third
   * command for their reason. What differs is what happens *after* the drag: the
   * other two are finished when the reader lets go, and this one has only just
   * started --- the box is empty until they type into its note.
   */
  drawTextBox(): void;
  /**
   * Place a form field of this kind by dragging its rectangle.
   *
   * The gesture is {@link drawBox}'s. What it leaves is a field the next save
   * writes into the document's form, named `Text 1` or `Checkbox 1` until the
   * reader renames it in the box that opens. `options` are a dropdown's
   * choices, and are absent for every other kind.
   */
  drawField(kind: import("./pages").FieldKind, options?: string[]): void;
  /** Arm the drag that places radio buttons of the group `group`, one for each drag. */
  drawRadio(group: string): void;
  /** Whether a text field placed from now on gets a line round it. */
  fieldBorder(): boolean;
  setFieldBorder(border: boolean): void;
  /** How many fields the open document's own form has. */
  savedFields(): number;
  /** Whether the document's own fields are shown for moving, renaming and removing. */
  formEditing(): boolean;
  setFormEditing(on: boolean): void;
  /** Whether exactly one rectangle is picked and it is a form field. */
  fieldPicked(): boolean;
  /** Opens the picked field's properties. */
  fieldProperties(): void;
  /**
   * Whether the tab order can be asked for: the document has a form field,
   * its own or one placed, and the order is not asked for already.
   */
  canOrderTabs(): boolean;
  /** Asks for the fields to be tabbed through in reading order. */
  orderTabs(): void;
  /** How many placed rectangles are picked for arranging. */
  pickedMarks(): number;
  /** Makes a copy of each picked rectangle, beside it. */
  duplicatePicked(): void;
  /** Arranges the picked rectangles: aligns, spaces or sizes them. */
  arrange(how: Arrangement): void;
  /** Draw or import a visual signature, then place it on a page. */
  signature(): void;
  /** Choose a picture file, then place the picture on a page by a drag. */
  picture(): void;
  /** Take the picture on the clipboard, then place it on a page by a drag. */
  pastePicture(): void;
  /** Arm a drag over a page; the region it covers goes to the clipboard as a PNG. */
  copyRegion(): void;
  /** Arm a drag over a page; the region it covers is saved as a PNG file. */
  saveRegion(): void;
  /**
   * Read the document's embedded files: save the one there is, or ask which
   * with {@link attachmentsAsked}'s list. See `attachments.ts`.
   */
  saveAttachment(): void;
  /** The embedded files being asked about, or null when nothing is asked. */
  attachmentsAsked(): readonly Embedded[] | null;
  /** Save the file the reader's answer names. */
  pickAttachment(raw: string): void;
  /** The question was dismissed. */
  dropAttachments(): void;
  /**
   * Sends the platform's paste for ⌘V, where the web view needs it sent:
   * macOS, whose web view pastes only on a Paste menu item's action
   * (`paste` in `commands/menubar.rs`). Answers whether it did; elsewhere the
   * web view pastes on the key itself and this answers false.
   */
  pasteKey(): boolean;
  editText(): void;
  /**
   * Arms the freehand tool for one drawing.
   *
   * The same shape as {@link drawBox} and for the same reasons --- a mode,
   * one-shot, no chord --- and it is a second command rather than an argument to
   * the first because a reader who wants to draw wants it in one press. What
   * differs is only the gesture the armed tool reads: a box is two corners and
   * this is every point between them.
   */
  draw(): void;
  /**
   * Arms the eraser.
   *
   * {@link draw}'s counterpart, and the third command here that arms rather
   * than acts. It takes strokes out of a drawing and takes every other kind
   * whole, because nothing else has parts to lose.
   *
   * **Not the same command as {@link removeMark}, and the difference is how a
   * mark is named rather than what happens to it.** That one takes the mark
   * whose note is open --- the one the reader has already pointed at and named
   * --- and is chosen from a menu with the pointer somewhere else entirely.
   * This one is aimed: it takes what the nib crosses, which is the only way to
   * reach a highlight without opening its box first.
   */
  erase(): void;
  /** Takes the mark whose note is open off the page it is on. */
  removeMark(): void;
  /** Whether a mark's note is open, which is what names the mark to remove. */
  hasOpenMark(): boolean;
  /** Copies the named marks to the window's own clipboard, or cuts them. See `markclipboard.ts`. */
  copyMarks(cut: boolean): void;
  /** Pastes the copied marks onto the page in front. */
  pasteMarks(): void;
  /** Whether any marks have been copied. */
  canPasteMarks(): boolean;
  /**
   * Takes the picked region out of the list of what is to be removed.
   *
   * {@link removeMark}'s twin and named the way it is named for that command's
   * reason: this takes a *pending* region off, so it is the opposite of
   * applying and must not be reachable by anybody looking for it. Nothing is
   * destroyed either way --- `docs/PLAN.md` §6 keeps applying behind a command
   * of its own.
   */
  removeRedaction(): void;
  /**
   * Whether a region is picked, which is what names the region to remove.
   *
   * {@link hasOpenMark}'s twin. A redaction has no note box, so a right-click
   * on one picks it and the pick is what this reads --- see
   * `Viewer.pickRedaction`, and `contextmenu.ts` for why there is no second way
   * to name it.
   */
  hasPickedRedaction(): boolean;
  /**
   * Whether the comment on show is one the reader can rewrite.
   *
   * Two conditions, not one: a comment popup has to be open, and its annotation
   * has to be an object of its own. A comment the file wrote as a direct
   * dictionary inside a page's `/Annots` cannot be overridden by an incremental
   * update at all --- see `Comment.object`.
   */
  canEditComment(): boolean;
  /** Turns the open comment's body into an editor and puts the keyboard in it. */
  editComment(): void;
  /**
   * Whether the comment on show is one the reader can answer.
   *
   * The same two conditions {@link AppActions.canEditComment} states, and its
   * own member rather than a second reading of that one: `/IRT` names an
   * object, so a comment written as a direct dictionary cannot be answered
   * either --- but the two commands ask different questions and only happen to
   * agree today.
   */
  canReplyToComment(): boolean;
  /** Opens an empty editor whose text becomes a reply to the open comment. */
  replyToComment(): void;
  /**
   * Whether the comment on show is one the reader can delete.
   *
   * {@link AppActions.canEditComment}'s two conditions a third time, and its own
   * member for that entry's reason. **It does not ask whether one of the
   * reader's replies answers the comment**, which is the one refusal deleting
   * has that the other two do not: that is the model's knowledge, and it answers
   * with a message saying which order to do the two in.
   */
  canDeleteComment(): boolean;
  /** Deletes the comment on show. Does nothing if it cannot. */
  deleteComment(): void;
  /**
   * Picks the colour marks are drawn in, by a `markcolors.ts` swatch id.
   *
   * One call for both meanings --- what the next mark will be, and what the mark
   * whose note is open becomes. See `App.svelte`, where the rule is written out;
   * stating it here as well would be the second copy of it.
   */
  setMarkColor(id: string): void;
  /**
   * Picks how thick the next drawing is, by the id of a nib in `NIBS`.
   *
   * A string for [`setMarkColor`](AppActions.setMarkColor)'s reason: the command
   * ids are built from the table, so what crosses is the same word that made the
   * id, and the caller resolving it to a width would be a second reader of the
   * list.
   */
  setNib(id: string): void;
  /** Which swatch is picked. For the check harness. */
  markColor(): string;
  /** Whether there is a selection to mark. */
  hasSelection(): boolean;
  /** Write the working document over the file it was opened from. */
  saveDocument(): void;
  /** Whether anything differs from the file on disk. */
  isDirty(): boolean;
  /** Ask for a name and write the working document to it. */
  saveCopy(): void;
  /**
   * Writes a copy with every marked region removed.
   *
   * Separate from {@link saveCopy} rather than a flag on it, and the reason is
   * that one of them destroys content: a reader reaching for *Save a copy* must
   * never get a redaction, and a reader reaching for a redaction must be able to
   * find it by that word. Two commands is what makes both true.
   */
  redactCopy(): void;
  /** Ask for a name and write the redactions into a fresh image-only PDF. */
  redactRasterCopy(): void;
  /** Ask for a name and write a copy whose scanned pages can be searched. */
  recogniseText(): void;
  /** Recognise the current saved page for temporary search and selection. */
  recognisePage(): void;
  /** Compare the saved file's text with its pages and list what is not shown. */
  findHiddenText(): void;
  /**
   * Show the other document at `index` of `sides().others` beside this one,
   * compare their words and list the changes. This document is the older.
   */
  compareWith(index: number): void;
  /** How many changes the comparison on screen lists; 0 with none. */
  changeCount(): number;
  /** Go to the next (1) or previous (-1) change, on both sides. */
  stepChange(direction: 1 | -1): void;
  /** Fetch the languages this machine offers and ask which to recognise in. */
  chooseRecognitionLanguage(): void;
  /**
   * The languages being asked about and the one chosen now, or null when
   * {@link chooseRecognitionLanguage} has not asked. See `ocrlanguage.ts`.
   */
  recognitionLanguages(): { offered: Offered; current: string | null } | null;
  /** Take the reader's typed answer to that question. */
  setRecognitionLanguage(raw: string): void;
  /** The question was dismissed. */
  dropRecognitionLanguages(): void;
  /** Ask for a new password and a name, and write a copy that needs it. */
  protectCopy(): void;
  /** Ask for a name and write a copy that opens without the password. */
  unprotectCopy(): void;
  /** Ask what to write, then where, and write a copy with page numbers or a watermark. */
  addText(kind: "numbers" | "watermark"): void;
  /** Offer the ways to make a copy smaller, ask for a name and write it. */
  compressCopy(): void;
  /**
   * Removes every marked region from the file the reader opened.
   *
   * {@link redactCopy}'s destructive twin, and the pair exists for the reason
   * {@link saveDocument} and {@link saveCopy} are a pair: a reader who wants
   * their own file redacted should not have to write a second one and delete
   * the first, and a reader who wants a redacted copy should not have to risk
   * the original. Two commands is what makes both true.
   *
   * The document is closed by the write and reopened from the path, so the
   * journal --- the marks included --- is spent. There is no undo across it.
   */
  redactDocument(): void;
  /** Ask for a name and write the pages at `slots` to it, as a second file. */
  extractPages(slots: number[]): void;
  /** Ask for a stem and write the document to one numbered file per group. */
  splitDocument(groups: number[][]): void;
  /** Ask for documents to combine with this one, and for a name to write to. */
  mergeDocuments(): void;
  /** Ask for pictures and a name, and make a document with one page each. */
  fromPictures(): void;
  /**
   * Choose a certificate, name a new file, and sign the document into it.
   *
   * The sequence and its refusals are `signing.ts`; the original file is never
   * written.
   */
  signDocument(): void;
  /** Linux: choose a certificate file, have its password asked, and remember it for signing. */
  addCertificateFile(): void;
  /**
   * Add long-term validation data, and an archive timestamp, for the
   * signatures the open document already has, into a new file.
   *
   * The sequence and what it says first are `validationdata.ts`; the original
   * file is never written.
   */
  addValidationData(): void;
  /** How many empty signature fields the open document has to sign. */
  signableFields(): number;
  /** Signs the first of them, as a press on it does. */
  signField(): void;
  /** Show what the document says about itself. */
  showProperties(): void;
}

const LANGUAGE = catalog({
  "en-US": { language: "Language", system: "Language: follow the system" },
  "de-DE": { language: "Sprache", system: "Sprache: wie im System" },
});
/**
 * Registers every command the application has.
 *
 * Appends to whatever the registry already holds, so the recent-document group
 * that `recents.ts` maintains is unaffected by the order these are added in.
 */
export function registerAppCommands(
  registry: CommandRegistry,
  actions: AppActions,
  locale: UiLocale = UI_LOCALE,
): void {
  /**
   * Every command the application has, in one place.
   *
   * Built once and outliving any document, so the palette works before a file
   * is open --- "Open document" is the command someone reaches for first. The
   * rest guard on `viewer`, which is read at call time rather than captured, so
   * closing and opening a document does not need the registry rebuilt.
   *
   * The `keys` strings are labels the palette displays, and they are **derived**
   * from the same table `viewer.ts`'s key handler and {@link handleWindowKey} match
   * against --- see `keys.ts`. They were hand-written beside those handlers with
   * nothing checking the two agreed, and the gap was not hypothetical: ⌘O was
   * advertised and reached no handler at all, and ⌘P turned the page as well as
   * printing, because the viewer's `p` arm tested the key without the modifier.
   */
  const available = () => !(actions.busyDocument?.() ?? false);
  const withDocument = () => actions.viewer() !== null && available();
  /** Offered while `kind` is not the layout already shown; changes to it. */
  const layoutCommand = (kind: SpreadKind) => ({
    enabled: () => {
      const viewer = actions.viewer();
      return viewer !== null && withDocument() && kindOf(viewer.spread) !== kind;
    },
    run: () => {
      const viewer = actions.viewer();
      viewer?.setSpread(withKind(viewer.spread, kind));
    },
  });
  /** When an arrangement is offered, what is missing when it is not, and what it runs. */
  const arranges = (how: Arrangement) => {
    const why = () => lacks(how, actions.pickedMarks());
    return {
      enabled: () => withDocument() && why() === null,
      why,
      run: () => actions.arrange(how),
    };
  };

  registry.register(
    {
      id: "file.close",
      title: TEXT(locale).closeTab,
      keys: label("file.close"),
      enabled: withDocument,
      run: () => actions.closeDocument(),
    },
    {
      id: "file.closeAll",
      title: TEXT(locale).closeAll,
      enabled: withDocument,
      run: () => actions.closeAllDocuments(),
    },
    // From the tab in front; a tab's own menu offers the same three for the
    // tab it was opened on. Unsaved work is asked about once for all of them.
    {
      id: "file.closeOtherTabs",
      title: TEXT(locale).closeOtherTabs,
      enabled: () => withDocument() && actions.canCloseTabs("others"),
      run: () => actions.closeTabs("others"),
    },
    {
      id: "file.closeTabsLeft",
      title: TEXT(locale).closeTabsLeft,
      enabled: () => withDocument() && actions.canCloseTabs("left"),
      run: () => actions.closeTabs("left"),
    },
    {
      id: "file.closeTabsRight",
      title: TEXT(locale).closeTabsRight,
      enabled: () => withDocument() && actions.canCloseTabs("right"),
      run: () => actions.closeTabs("right"),
    },
    // Through a folder, a document at a time: the tab moves on rather than a
    // tab being added for each, unless it has unsaved work.
    {
      id: "file.nextInFolder",
      title: TEXT(locale).nextInFolder,
      enabled: withDocument,
      run: () => actions.openInFolder(1),
    },
    {
      id: "file.previousInFolder",
      title: TEXT(locale).previousInFolder,
      enabled: withDocument,
      run: () => actions.openInFolder(-1),
    },
    {
      // A browser's chord, where a reader looks for it. The document opens
      // where it was left, as a remembered one does.
      id: "file.reopenClosedTab",
      title: TEXT(locale).reopenClosedTab,
      keys: label("file.reopenClosedTab"),
      enabled: () => available() && actions.canReopenClosedTab(),
      run: () => actions.reopenClosedTab(),
    },
    {
      // Offered whatever the preference below says, which is the point of it:
      // a reader who does not want every tab back every morning still wants
      // them back on the morning after the application was closed by mistake.
      id: "file.reopenLastTabs",
      title: TEXT(locale).reopenLastTabs,
      enabled: () => actions.tabsToReopen() > 0 && available(),
      run: () => actions.reopenLastTabs(),
    },
    // A preference, in the shape the automatic-update pair has: one command
    // per choice, each offered while it is not the current one.
    {
      id: "file.reopenTabsAtLaunch",
      title: TEXT(locale).reopenTabsAtLaunch,
      enabled: () => !actions.restoreTabs(),
      run: () => actions.setRestoreTabs(true),
    },
    {
      id: "file.reopenLastDocumentAtLaunch",
      title: TEXT(locale).reopenLastDocumentAtLaunch,
      enabled: () => actions.restoreTabs(),
      run: () => actions.setRestoreTabs(false),
    },
    // The interface language, the same shape: one command per choice, each
    // offered while it is not the current one. A language's own name is not
    // translated, so a reader who cannot read the current language finds theirs.
    ...(["system", ...UI_LOCALES] as const).map((choice: LanguageChoice) => ({
      id: `app.language.${choice}`,
      title: choice === "system"
        ? LANGUAGE(locale).system
        : `${LANGUAGE(locale).language}: ${LOCALE_NAMES[choice]}`,
      enabled: () => actions.languageChoice() !== choice,
      run: () => actions.setLanguageChoice(choice),
    })),
    {
      // Not withheld while a document is busy: it changes a list, and no
      // document. Withheld when the list is empty, so that a reader who runs
      // it sees it go, which is the only sign it worked that the palette has.
      id: "file.clearRecents",
      title: TEXT(locale).clearRecents,
      enabled: () => actions.recentDocuments() > 0,
      run: () => actions.clearRecentDocuments(),
    },
    {
      id: "view.tabLabelsLarger",
      title: TEXT(locale).tabLabelsLarger,
      enabled: () => withDocument() && actions.tabLabels().canGrow,
      run: () => actions.resizeTabLabels(1),
    },
    {
      id: "view.tabLabelsSmaller",
      title: TEXT(locale).tabLabelsSmaller,
      enabled: () => withDocument() && actions.tabLabels().canShrink,
      run: () => actions.resizeTabLabels(-1),
    },
    {
      id: "view.tabLabelsDefault",
      title: TEXT(locale).tabLabelsDefault,
      enabled: () => withDocument() && !actions.tabLabels().isDefault,
      run: () => actions.resizeTabLabels(0),
    },
    {
      id: "view.nextTab",
      title: TEXT(locale).nextTab,
      keys: label("view.nextTab"),
      enabled: () => withDocument() && actions.documentCount() > 1,
      run: () => actions.nextDocument(1),
    },
    {
      id: "view.previousTab",
      title: TEXT(locale).previousTab,
      keys: label("view.previousTab"),
      enabled: () => withDocument() && actions.documentCount() > 1,
      run: () => actions.nextDocument(-1),
    },
    {
      // Asks for the other document by name, where a tab's own menu moves the
      // tab the reader pressed: from the keyboard there is no tab under the
      // pointer, and choosing the partner is the whole of the question.
      id: "view.sideBySide",
      title: TEXT(locale).sideBySide,
      enabled: () => withDocument() && actions.sides().others.length > 0,
      argument: {
        get placeholder() {
          return partnerPlaceholder(actions.sides().others);
        },
        problem: (raw: string) => {
          const picked = pickPartner(raw, actions.sides().others);
          return "problem" in picked ? picked.problem : null;
        },
        preview: (raw: string) => {
          const others = actions.sides().others;
          const picked = pickPartner(raw, others);
          return "index" in picked ? (others[picked.index] ?? "") : "";
        },
        run: (raw: string) => {
          const picked = pickPartner(raw, actions.sides().others);
          if ("index" in picked) actions.showBeside(picked.index);
        },
      },
    },
    {
      // Named for where the tab goes, which is the side the reader is not in.
      id: "view.moveToOtherSide",
      get title() {
        return TEXT(locale).moveToOtherSide(actions.sides().focused === "left");
      },
      enabled: () => withDocument() && actions.sides().others.length > 0,
      run: () => actions.moveToOtherSide(),
    },
    {
      // No second document needed, which is what tells it from the commands
      // above: the text on one side and its figures or notes on the other.
      id: "view.bothSides",
      title: TEXT(locale).bothSides,
      enabled: () => withDocument(),
      run: () => actions.showOnBothSides(),
    },
    {
      id: "view.switchSides",
      title: TEXT(locale).switchSides,
      enabled: () => withDocument() && actions.sides().split,
      run: () => actions.switchSides(),
    },
    {
      id: "view.focusOtherSide",
      title: TEXT(locale).focusOtherSide,
      enabled: () => withDocument() && actions.sides().split,
      run: () => actions.focusOtherSide(),
    },
    {
      // One command whose title says what running it does, like the button on
      // the divider it is the keyboard's way to.
      id: "view.syncScrolling",
      get title() {
        return actions.syncScrolling() ? TEXT(locale).scrollSeparately : TEXT(locale).scrollTogether;
      },
      enabled: () => withDocument() && actions.sides().split,
      run: () => actions.toggleSyncScrolling(),
    },
    {
      id: "file.open",
      title: TEXT(locale).open,
      keys: label("file.open"),
      enabled: available,
      run: () => actions.openDocument(),
    },
    {
      // Beside Open, because it also begins with no document: the pictures
      // are chosen in the platform's panel and the result is opened.
      id: "file.fromPictures",
      title: TEXT(locale).fromPictures,
      enabled: available,
      run: () => actions.fromPictures(),
    },
    {
      // No keyboard binding, and not an oversight. ⌘R is already the rotate
      // chord, and moving a binding a reader already has is worse than a
      // command reaching the palette alone --- which is the shape this
      // application is built around. "Show outline" and "Show page thumbnails"
      // have none either.
      //
      // It exists because the backend now tells a reader whose file was
      // truncated to open it again, and until this there was no way to do that
      // except ⌘O and re-picking the file. It is useful either way: with the
      // watch below set to "do nothing", this is how you ask for the new file.
      id: "file.reload",
      title: TEXT(locale).reload,
      enabled: withDocument,
      run: () => actions.reloadDocument(),
    },
    // What happens when another program rewrites the open file: one command per
    // choice, each offered while it is not the current one --- the shape the
    // automatic-update pair has. Always enabled otherwise, since this is a
    // preference and not something done to a document.
    ...DISK_CHANGE_MODES.map((mode) => ({
      id: `file.onDiskChange.${mode}`,
      title: diskChangeTitle(mode, locale),
      enabled: () => actions.diskChangeMode() !== mode,
      run: () => actions.setDiskChangeMode(mode),
    })),
    {
      // Always enabled and asking nothing of the network, which is the whole
      // difference from the command below it. "Which version am I running" is a
      // question a reader has *because* something is wrong, and an answer that
      // needs a working endpoint is no answer in exactly that case.
      //
      // It exists because nothing in the application answered it at all until
      // 2026-08-19, and the cost was a bug report: a reader on `26.8.4` hit the
      // Windows defect where an app with no console could open no document, and
      // could not tell whether the release fixing it was the one they had.
      id: "app.about",
      title: TEXT(locale).about,
      run: () => actions.about(),
    },
    {
      // No binding, same reasoning as the two above: a command nobody presses
      // by accident belongs in the palette. Always enabled, deliberately ---
      // asking again after a failed check is the only way back, and a guard
      // that withheld it while `failed` would strand the reader on a launch
      // that happened to have no network.
      id: "app.checkForUpdates",
      title: TEXT(locale).checkForUpdates,
      run: () => actions.checkForUpdates(),
    },
    {
      id: "app.disableAutomaticUpdates",
      title: TEXT(locale).disableAutomaticUpdates,
      enabled: () => actions.automaticUpdates(),
      run: () => actions.setAutomaticUpdates(false),
    },
    {
      id: "app.enableAutomaticUpdates",
      title: TEXT(locale).enableAutomaticUpdates,
      enabled: () => !actions.automaticUpdates(),
      run: () => actions.setAutomaticUpdates(true),
    },
    {
      // Two guards rather than one, because "there is an update" and "it is
      // already applied" are different states and this must be withdrawn in the
      // second. Applying twice is not harmful, but a command that stays live
      // after it has run says the first run did not work.
      id: "app.installUpdate",
      // **"and restart" came off on 2026-09-21, because it did not.** On macOS
      // the plugin replaces the `.app` on disk and leaves this process running
      // the old code, so the command installed and then sat there while the
      // header said a restart was needed and offered nothing to press. A title
      // may promise less than a command performs --- on Windows the installer
      // does close and reopen tpdf --- but never more, and `docs/TRAPS.md` has
      // why that direction is the one that costs. The restart is its own
      // command below, offered exactly when it is the step that remains.
      title: TEXT(locale).installUpdate,
      enabled: () => actions.updateAvailable() && !actions.updateReady(),
      run: () => actions.applyUpdate(),
    },
    {
      // The other half, and it exists at all because the header used to say
      // "restart to finish" beside a disabled button: the reader was told what
      // to do and given no way to do it but quitting the application by hand.
      //
      // Guarded on `updateReady` alone, which is the state `install` leaves
      // behind. That is why the two guards are not mirror images: `install`
      // needs an update found *and* not yet applied, this one needs it applied.
      id: "app.restartForUpdate",
      title: TEXT(locale).restartForUpdate,
      enabled: () => actions.updateReady(),
      run: () => actions.restartForUpdate(),
    },
    {
      // Each of the pair is withheld when it has nothing to do, since
      // 2026-10-06: Install once this copy's tool is what a terminal gets,
      // Uninstall when there is nothing at the tool's paths (on Windows, when
      // its folder is not on the PATH). Until then both were always live, on
      // the argument that the answer is in the filesystem and not on this
      // side --- and that half still holds. The backend reads the filesystem
      // again each time either one runs and decides from what it finds then;
      // the guard is a hint read beforehand (`clitoolstate.ts`), asked again
      // after either command and whenever the window comes to the front.
      //
      // It errs towards offering. Not known --- before the first answer, after
      // a read that failed, on a platform with nothing to read --- is live. A
      // link to another copy of tpdf leaves Install live, because installing
      // repoints it. Somebody else's file at a path leaves both live, because
      // running either is what says the file is there and was left alone, and
      // a greyed item says nothing. No binding, like every command a reader
      // runs once.
      id: "app.installCommandLineTool",
      title: TEXT(locale).installCommandLineTool,
      enabled: () => actions.commandLineToolOffered(true),
      run: () => actions.commandLineTool(true),
    },
    {
      id: "app.uninstallCommandLineTool",
      title: TEXT(locale).uninstallCommandLineTool,
      enabled: () => actions.commandLineToolOffered(false),
      run: () => actions.commandLineTool(false),
    },
    {
      // Always enabled: whether tpdf is the default is the system's to say,
      // and the backend asks it each time.
      id: "app.makeDefaultPdfApp",
      title: TEXT(locale).makeDefaultPdfApp,
      run: () => actions.makeDefaultPdfApp(),
    },
    {
      // Windows only, and greyed everywhere else for good: Explorer is the
      // only file manager it applies to. Off until the reader runs this; the
      // installer registers nothing (`docs/PLAN.md` 7.8). Each is greyed when
      // it has nothing to do, from the backend's last answer.
      id: "app.enableExplorerIntegration",
      title: TEXT(locale).enableExplorerIntegration,
      enabled: () => actions.explorerOffered(true),
      run: () => actions.explorerIntegration(true),
    },
    {
      id: "app.disableExplorerIntegration",
      title: TEXT(locale).disableExplorerIntegration,
      enabled: () => actions.explorerOffered(false),
      run: () => actions.explorerIntegration(false),
    },
    {
      // Named for what it lists, in a reader's words: "hidden text" would name
      // the kind without saying hidden from what. No ellipsis, because nothing
      // is asked: it reads the saved file, writes nothing, and its answer is a
      // tab of the sidebar.
      id: "file.findHiddenText",
      title: TEXT(locale).findHiddenText,
      enabled: withDocument,
      run: () => actions.findHiddenText(),
    },
    {
      // Asks for the other document by name, as "Show side by side with..."
      // does, and shows it there: the two documents are read beside each
      // other, and the one the reader is in is taken as the older. A document
      // not yet open is opened first, which is what makes it a choice here.
      id: "file.compareWith",
      title: TEXT(locale).compareWith,
      enabled: () => withDocument() && actions.sides().others.length > 0,
      argument: {
        get placeholder() {
          return partnerPlaceholder(actions.sides().others);
        },
        problem: (raw: string) => {
          const picked = pickPartner(raw, actions.sides().others);
          return "problem" in picked ? picked.problem : null;
        },
        preview: (raw: string) => {
          const others = actions.sides().others;
          const picked = pickPartner(raw, others);
          return "index" in picked ? (others[picked.index] ?? "") : "";
        },
        run: (raw: string) => {
          const picked = pickPartner(raw, actions.sides().others);
          if ("index" in picked) actions.compareWith(picked.index);
        },
      },
    },
    {
      id: "file.properties",
      title: TEXT(locale).properties,
      // No shortcut, on the same reasoning as "Extract pages...": this is a
      // command a reader reaches deliberately rather than by muscle memory, and
      // a chord is a *global key claim* rather than a label --- see
      // `docs/TRAPS.md`. Acrobat's Cmd-D is free here and was deliberately not
      // taken, because claiming a key for a dialog opened once a month costs
      // every other command that might want it later.
      enabled: withDocument,
      run: () => actions.showProperties(),
    },
    {
      // No page-range field of our own, deliberately: the system panel has one,
      // and its numbers refer to the document we hand over --- which is every
      // page, so they mean what the reader thinks they mean. `print::build`
      // takes a range because thumbnail-selection printing will need it, not
      // because anything asks for one today.
      id: "file.print",
      title: TEXT(locale).print,
      keys: label("file.print"),
      enabled: withDocument,
      run: () => actions.printDocument(),
    },
    {
      id: "find.open",
      title: TEXT(locale).findOpen,
      keys: label("find.open"),
      enabled: withDocument,
      run: () => actions.focusFind(),
    },
    {
      id: "find.next",
      title: TEXT(locale).findNext,
      keys: label("find.next"),
      enabled: withDocument,
      run: () => actions.viewer()?.nextMatch(),
    },
    {
      id: "find.previous",
      title: TEXT(locale).findPrevious,
      keys: label("find.previous"),
      enabled: withDocument,
      run: () => actions.viewer()?.prevMatch(),
    },
    {
      // Titled by what pressing it does, not by what the setting is called: a
      // palette lists verbs, and "Match case" beside a checkbox that is already
      // on reads as a description of the state rather than as a command.
      id: "find.matchCase",
      title: TEXT(locale).matchCase,
      keys: label("find.matchCase"),
      enabled: withDocument,
      run: () => actions.toggleSearchOption("matchCase"),
    },
    {
      id: "find.wholeWord",
      title: TEXT(locale).wholeWord,
      keys: label("find.wholeWord"),
      enabled: withDocument,
      run: () => actions.toggleSearchOption("wholeWord"),
    },
    {
      // Enabled on having something to scope *to*, which is what makes a
      // silently-does-nothing command unnecessary: a reader with no selection
      // sees it greyed, with the reason being that nothing is selected. It
      // stays enabled while scoped so there is a way back out.
      id: "find.inSelection",
      title: TEXT(locale).inSelection,
      keys: label("find.inSelection"),
      enabled: () => {
        const viewer = actions.viewer();
        return (
          available() && viewer !== null && (viewer.searchScoped || viewer.hasSelection)
        );
      },
      run: () => actions.toggleSearchScope(),
    },
    {
      // Titled by what it turns on rather than by the word "regex", which is
      // what a reader who wants it would type and not what a reader who does
      // not would understand. Both spellings are in the title so the palette
      // finds it either way.
      id: "find.regex",
      title: TEXT(locale).regex,
      keys: label("find.regex"),
      enabled: withDocument,
      run: () => actions.toggleSearchOption("regex"),
    },
    {
      id: "view.zoomIn",
      title: TEXT(locale).zoomIn,
      keys: label("view.zoomIn"),
      enabled: withDocument,
      run: () => actions.viewer()?.zoomStep(1),
    },
    {
      id: "view.zoomOut",
      title: TEXT(locale).zoomOut,
      keys: label("view.zoomOut"),
      enabled: withDocument,
      run: () => actions.viewer()?.zoomStep(-1),
    },
    {
      id: "view.fitWidth",
      title: TEXT(locale).fitWidth,
      keys: label("view.fitWidth"),
      enabled: withDocument,
      run: () => actions.viewer()?.setFit("width"),
    },
    {
      id: "view.fitPage",
      title: TEXT(locale).fitPage,
      keys: label("view.fitPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.setFit("page"),
    },
    {
      id: "view.fitHeight",
      title: TEXT(locale).fitHeight,
      enabled: withDocument,
      run: () => actions.viewer()?.setFit("height"),
    },
    {
      // SumatraPDF's Fit Content: the page's content without its blank
      // margins, measured on the rendered page (`inkbounds.ts`).
      id: "view.fitContent",
      title: TEXT(locale).fitContent,
      enabled: withDocument,
      run: () => actions.viewer()?.setFit("content"),
    },
    {
      // 100% means one CSS pixel per PDF point, which is not the same as one
      // inch on the desk --- that would need the display's physical size, which
      // no browser reports honestly. Every other reader calls this number
      // "actual size" and means the same thing by it.
      id: "view.actualSize",
      title: TEXT(locale).actualSize,
      keys: label("view.actualSize"),
      enabled: withDocument,
      run: () => actions.viewer()?.setZoomFixed(1),
    },
    {
      // The second command that takes a value. The zoom ladder is deliberately
      // coarse --- each stop throws away every tile --- so a reader who wants
      // 175% cannot step to it, and before this there was nothing to type it
      // into either.
      id: "view.zoomTo",
      title: TEXT(locale).zoomTo,
      keys: label("view.zoomTo"),
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).zoomPlaceholder,
        problem: (raw: string) => {
          if (raw.trim() === "")
            return TEXT(locale).zoomRange(percentOf(MIN_ZOOM), percentOf(MAX_ZOOM));
          if (parseZoomPercent(raw) === null) {
            return TEXT(locale).notAZoom(raw.trim(), percentOf(MIN_ZOOM), percentOf(MAX_ZOOM));
          }
          return null;
        },
        preview: (raw: string) =>
          TEXT(locale).zoomPreview(percentOf(parseZoomPercent(raw) ?? 1)),
        run: (raw: string) => {
          const zoom = parseZoomPercent(raw);
          if (zoom !== null) actions.viewer()?.setZoomFixed(zoom);
        },
      },
    },
    {
      // Preview's bindings, not Acrobat's: Acrobat rotates on Shift-Cmd-+ and
      // Shift-Cmd-−, and on this keyboard those produce the same `key` as the
      // zoom shortcuts they would then collide with.
      //
      // Rotating the *view* only. The file is untouched; rotating pages in the
      // document is a page operation and belongs with the ones that write.
      id: "view.rotateClockwise",
      title: TEXT(locale).rotateClockwise,
      keys: label("view.rotateClockwise"),
      enabled: withDocument,
      run: () => actions.viewer()?.rotateBy(1),
    },
    {
      id: "view.rotateCounterClockwise",
      title: message("rotateViewCounter", locale),
      keys: label("view.rotateCounterClockwise"),
      enabled: withDocument,
      run: () => actions.viewer()?.rotateBy(-1),
    },
    // The page layouts: one command per choice, each offered while it is not
    // the current one, the shape `file.onDiskChange.*` has. Per document, and
    // remembered with its place.
    // Written out rather than mapped from `SPREAD_KINDS`, so that each id is a
    // literal `check_classified.py` can find.
    { id: "view.singlePage", title: SPREAD_TITLES.single, ...layoutCommand("single") },
    { id: "view.facing", title: SPREAD_TITLES.facing, ...layoutCommand("facing") },
    { id: "view.book", title: SPREAD_TITLES.book, ...layoutCommand("book") },
    // Favourites: pages the reader marked, across every document. Each one is
    // also a palette entry of its own (`favourites.ts`, `FAVOURITE_PREFIX`).
    {
      id: "nav.addFavourite",
      title: TEXT(locale).addFavourite,
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).favouritePlaceholder,
        problem: () => null,
        preview: (raw: string) =>
          (raw.trim() ? TEXT(locale).addFavouriteAs(raw.trim()) : TEXT(locale).addFavouriteNumbered),
        run: (raw: string) => actions.addFavourite(raw),
      },
    },
    {
      id: "nav.removeFavourite",
      title: TEXT(locale).removeFavourite,
      enabled: () => withDocument() && actions.isFavourite(),
      run: () => actions.removeFavourite(),
    },
    {
      id: "nav.nextFavourite",
      title: TEXT(locale).nextFavourite,
      enabled: () => withDocument() && actions.canStepFavourite(1),
      run: () => actions.stepFavourite(1),
    },
    {
      id: "nav.previousFavourite",
      title: TEXT(locale).previousFavourite,
      enabled: () => withDocument() && actions.canStepFavourite(-1),
      run: () => actions.stepFavourite(-1),
    },
    // The outline, opened or closed as a whole, or down to where the reader is.
    {
      id: "view.outlineExpandAll",
      title: TEXT(locale).outlineExpandAll,
      enabled: () => withDocument() && actions.hasOutline(),
      run: () => actions.arrangeOutline("all"),
    },
    {
      id: "view.outlineCollapseAll",
      title: TEXT(locale).outlineCollapseAll,
      enabled: () => withDocument() && actions.hasOutline(),
      run: () => actions.arrangeOutline("none"),
    },
    {
      id: "view.outlineToCurrent",
      title: TEXT(locale).outlineToCurrent,
      enabled: () => withDocument() && actions.hasOutline(),
      run: () => actions.arrangeOutline("current"),
    },
    {
      // Full screen leaves the layout as it is; presenting is full screen with
      // one page at a time at fit page on black. Esc leaves either.
      id: "view.fullScreen",
      title: TEXT(locale).fullScreen,
      keys: label("view.fullScreen"),
      enabled: withDocument,
      run: () => actions.toggleFullScreen(),
    },
    {
      id: "view.presentation",
      title: TEXT(locale).presentation,
      enabled: withDocument,
      run: () => actions.togglePresentation(),
    },
    // Continuous, or one page (or pair) at a time: SumatraPDF's continuous
    // switch, as a pair of choices so that each says what it does.
    {
      id: "view.continuous",
      title: TEXT(locale).continuous,
      enabled: () => withDocument() && actions.viewer()?.spread.continuous === false,
      run: () => {
        const viewer = actions.viewer();
        if (viewer) viewer.setSpread({ ...viewer.spread, continuous: true });
      },
    },
    {
      id: "view.pageAtATime",
      title: TEXT(locale).pageAtATime,
      enabled: () => withDocument() && actions.viewer()?.spread.continuous === true,
      run: () => {
        const viewer = actions.viewer();
        if (viewer) viewer.setSpread({ ...viewer.spread, continuous: false });
      },
    },
    {
      // SumatraPDF's manga mode. Only with two pages to a row, where there is
      // a direction to change.
      id: "view.rightToLeft",
      title: TEXT(locale).rightToLeft,
      enabled: () => withDocument() && actions.viewer()?.spread.columns === 2,
      run: () => {
        const viewer = actions.viewer();
        if (viewer) viewer.setSpread({ ...viewer.spread, rightToLeft: !viewer.spread.rightToLeft });
      },
    },
    {
      // ⇧⌘R beside ⌘R, because these two are the same gesture on two different
      // subjects and a reader who knows one should be able to guess the other.
      // The titles carry the distinction that matters --- "view" against
      // "page" --- since the shortcut cannot.
      id: "edit.rotatePageClockwise",
      title: TEXT(locale).rotatePageClockwise,
      keys: label("edit.rotatePageClockwise"),
      enabled: withDocument,
      run: () => actions.rotatePage(1),
    },
    {
      id: "edit.rotatePageCounterClockwise",
      title: message("rotatePageCounter", locale),
      keys: label("edit.rotatePageCounterClockwise"),
      enabled: withDocument,
      run: () => actions.rotatePage(-1),
    },
    {
      // **The only mark command with no `hasSelection` guard**, because a
      // comment is not made *of* a selection --- it is dropped on the page. It
      // is offered on any open document, which is also what makes it reachable
      // from the palette and the menu bar, where there is no pointer and
      // therefore no place a selection-shaped command could act.
      //
      // "Add comment" rather than "Add note": the note is the *text* inside it,
      // and every mark has one of those. `markpopup.ts` uses the same word for
      // the same reason.
      //
      // **No ellipsis, and it was considered.** `edit.drawBox` carries one
      // because it arms a tool and asks for a gesture next, and from the palette
      // and the menu bar this now does exactly that. From the right-click menu
      // it does not: that route names a point, so the comment is placed at once
      // and there is nothing further to ask. One title serves all three, so an
      // ellipsis would be a promise broken in the one place a reader has already
      // said where. The status line names the armed tool instead, which is the
      // route that needed telling.
      id: "edit.addComment",
      title: TEXT(locale).addComment,
      enabled: withDocument,
      run: () => actions.addComment(null),
    },
    {
      // **"Draw a box" rather than "Box selection"**, and the verb is the whole
      // difference: the three below act on what is already selected, and this
      // one asks the reader to do something next. The ellipsis says so in the
      // way the platform's own menus do --- `runMenuCommand` uses one for a
      // command that opens the palette to ask for a value, and this is the same
      // promise about a different kind of asking.
      //
      // No `hasSelection` guard, for the comment's reason above: a box is not
      // made *of* a selection. It is offered on any open document, which is
      // also what makes it reachable from the palette and the menu bar, where
      // there is no pointer at all --- arming from there is exactly as useful,
      // since the gesture comes afterwards either way.
      //
      // No key. A one-shot mode a reader can enter by accident and then not
      // recognise is worse than one they have to ask for. Comment, text box
      // and text editing have bare letters since 2026-10-14 (see `BINDINGS`):
      // they are the frequent ones, and every armed tool is now named in the
      // toolbar's row with a button to leave it.
      id: "edit.drawBox",
      title: TEXT(locale).drawBox,
      enabled: withDocument,
      run: () => actions.drawBox(),
    },
    {
      // Beside the box, because it is the box's gesture and a reader choosing
      // between the two is choosing a shape rather than a tool. Everything the
      // entry above says about arming a mode, about having no `hasSelection`
      // guard and about having no chord applies here unchanged.
      //
      // "an ellipse", not "a circle": `/Circle` is the file's word and one a
      // reader drags out is almost never circular, which is `markpopup.ts`'s
      // argument for the label and the same one `edit.drawBox` makes about
      // "square".
      id: "edit.drawEllipse",
      title: TEXT(locale).drawEllipse,
      enabled: withDocument,
      run: () => actions.drawEllipse(),
    },
    // The four shapes of straight lines, beside the two shapes, as SumatraPDF
    // offers them. No keys, for the box's reason above. "Add", not "Draw", so
    // that typing "draw" still finds *Draw freehand* among the first three.
    {
      id: "edit.drawLine",
      title: TEXT(locale).drawLine,
      enabled: withDocument,
      run: () => actions.drawShape("line"),
    },
    {
      id: "edit.drawArrow",
      title: TEXT(locale).drawArrow,
      enabled: withDocument,
      run: () => actions.drawShape("arrow"),
    },
    {
      id: "edit.drawPolygon",
      title: TEXT(locale).drawPolygon,
      enabled: withDocument,
      run: () => actions.drawShape("polygon"),
    },
    {
      id: "edit.drawPolyline",
      title: TEXT(locale).drawPolyline,
      enabled: withDocument,
      run: () => actions.drawShape("polyline"),
    },
    {
      // A rectangle to click, dragged like a box; where it goes is typed in
      // the box that opens on it, as a page number or a web address.
      id: "edit.addLink",
      title: TEXT(locale).addLink,
      enabled: withDocument,
      run: () => actions.drawShape("link"),
    },
    {
      // A PNG or JPEG decoded in a worker, then placed by a drag as a
      // signature picture is; it keeps its proportions.
      id: "edit.addPicture",
      title: TEXT(locale).addPicture,
      enabled: withDocument,
      run: () => actions.picture(),
    },
    {
      // The picture on the clipboard, decoded in a worker as a file's is and
      // placed by a drag the same way. ⌘V does it on the page; in a text field
      // ⌘V pastes text.
      id: "edit.pastePicture",
      title: TEXT(locale).pastePicture,
      keys: label("edit.pastePicture"),
      enabled: withDocument,
      run: () => actions.pastePicture(),
    },
    {
      id: "edit.editText",
      title: TEXT(locale).editText,
      enabled: withDocument,
      run: () => actions.editText(),
    },
    {
      // A picture and nothing more, and named so since 2026-10-13: next to
      // "Sign document…" the old "Place signature image..." read as signing.
      id: "edit.addSignature",
      title: TEXT(locale).addSignature,
      enabled: withDocument,
      run: () => actions.signature(),
    },
    ...(["approved", "confidential", "draft", "final"] as const).map(
      (name) => ({
        // **One command per stamp, and the alternative was one that asks.** The
        // palette can take a value --- `nav.goToPage` does --- and a stamp is not
        // that shape: the four are a menu of four things a reader picks between,
        // not a parameter they supply, and typing "draft" into a prompt is
        // slower than typing it into the palette that is already open. The same
        // argument `edit.color.*` makes, and this follows its spelling so the
        // two families read alike.
        //
        // The ellipsis is the shape tools': the command arms a tool, and what
        // happens next is a drag the reader makes.
        id: `edit.stamp.${name}`,
        title: TEXT(locale).stamp(STAMP_TITLES[name]),
        enabled: withDocument,
        run: () => actions.stamp(name),
      }),
    ),
    {
      // The third tool that reads a drag, after the two shapes. Everything the
      // box's entry says about arming a mode applies unchanged.
      //
      // "Add a text box...", not "Draw...": the other two are finished by the
      // gesture and this one is not --- the drag places an empty box and the
      // words come afterwards, so a verb about drawing would describe the wrong
      // half of it.
      id: "edit.addTextBox",
      title: TEXT(locale).addTextBox,
      enabled: withDocument,
      run: () => actions.drawTextBox(),
    },
    // Three commands for one gesture, as the shapes are: what a reader chooses
    // is what appears, and a kind picked in a dialog after the drag would be a
    // question about a rectangle they can no longer see being drawn.
    {
      id: "edit.addTextField",
      title: TEXT(locale).addTextField,
      enabled: withDocument,
      run: () => actions.drawField("text"),
    },
    {
      id: "edit.addMultilineField",
      title: TEXT(locale).addMultilineField,
      enabled: withDocument,
      run: () => actions.drawField("multiline"),
    },
    {
      id: "edit.addCheckbox",
      title: TEXT(locale).addCheckbox,
      enabled: withDocument,
      run: () => actions.drawField("checkbox"),
    },
    {
      // An empty signature field: a place somebody signs with a certificate,
      // here or in another program.
      id: "edit.addSignatureField",
      title: TEXT(locale).addSignatureField,
      enabled: withDocument,
      run: () => actions.drawField("signature"),
    },
    {
      // The choices are asked for before the drag: a dropdown with none is
      // not a field, and the name box that opens afterwards has one line.
      id: "edit.addDropdown",
      title: TEXT(locale).addDropdown,
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).dropdownPlaceholder,
        problem: (raw: string) => {
          const parsed = parseChoices(raw);
          return "problem" in parsed ? parsed.problem : null;
        },
        preview: (raw: string) => {
          const parsed = parseChoices(raw);
          return "options" in parsed
            ? TEXT(locale).dropdownPreview(parsed.options.length)
            : "";
        },
        run: (raw: string) => {
          const parsed = parseChoices(raw);
          // Unreachable through the palette, which refuses to run a command
          // whose `problem` answered.
          if ("options" in parsed) actions.drawField("dropdown", parsed.options);
        },
      },
    },
    // A preference about the fields placed next, in the shape the launch pair
    // has: one command per choice, each offered while it is not the current.
    {
      id: "edit.fieldBorderOn",
      title: TEXT(locale).fieldBorderOn,
      enabled: () => !actions.fieldBorder(),
      run: () => actions.setFieldBorder(true),
    },
    {
      id: "edit.fieldBorderOff",
      title: TEXT(locale).fieldBorderOff,
      enabled: () => actions.fieldBorder(),
      run: () => actions.setFieldBorder(false),
    },
    // Changing the fields a document already has. A pair, each offered while
    // the other state holds, in the shape the line preference above has. On
    // only for a document with a form: with none there is nothing to change.
    {
      id: "edit.formEditOn",
      title: TEXT(locale).formEditOn,
      enabled: () => withDocument() && actions.savedFields() > 0 && !actions.formEditing(),
      run: () => actions.setFormEditing(true),
    },
    {
      id: "edit.formEditOff",
      title: TEXT(locale).formEditOff,
      enabled: () => withDocument() && actions.formEditing(),
      run: () => actions.setFormEditing(false),
    },
    {
      // Rows from the top, each from the left, on every page. A form made
      // here is in that order already; this is for one from elsewhere, and
      // for one whose fields were moved about after they were made.
      id: "edit.tabOrder",
      title: TEXT(locale).tabOrder,
      enabled: () => withDocument() && actions.canOrderTabs(),
      run: () => actions.orderTabs(),
    },
    {
      // The group is asked for before the drag: it is what makes several
      // buttons one question, and it is each button's name.
      id: "edit.addRadio",
      title: TEXT(locale).addRadio,
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).radioPlaceholder,
        problem: (raw: string) => {
          const parsed = parseGroup(raw);
          return "problem" in parsed ? parsed.problem : null;
        },
        preview: (raw: string) => {
          const parsed = parseGroup(raw);
          return "group" in parsed ? TEXT(locale).radioPreview(parsed.group) : "";
        },
        run: (raw: string) => {
          const parsed = parseGroup(raw);
          if ("group" in parsed) actions.drawRadio(parsed.group);
        },
      },
    },
    // Offered for one field: one placed in this session, or one of the file
    // picked while those are being changed. A panel about several fields
    // would have to say what it shows where they differ.
    {
      id: "edit.fieldProperties",
      title: TEXT(locale).fieldProperties,
      enabled: () => withDocument() && actions.fieldPicked(),
      run: () => actions.fieldProperties(),
    },
    // Arranging the placed rectangles a reader has picked: form fields, boxes,
    // ellipses and text boxes. One command per arrangement, each offered when
    // enough are picked for it to do something. Written out one by one, so
    // each id is a literal the classification gate can read.
    //
    // "Centre" and "middle" are a presentation program's words for the two
    // directions, used because a reader who has aligned boxes anywhere has met
    // them there: centre is left to right, middle is top to bottom.
    {
      // A copy of each picked rectangle, a step down and to the right, picked
      // in place of the originals so that the next drag moves the copies.
      id: "edit.duplicate",
      title: TEXT(locale).duplicate,
      keys: label("edit.duplicate"),
      enabled: () => withDocument() && actions.pickedMarks() >= 1,
      run: () => actions.duplicatePicked(),
    },
    { id: "edit.alignLeft", title: TEXT(locale).alignLeft, ...arranges("left") },
    { id: "edit.alignCenter", title: message("alignCenter", locale), ...arranges("center") },
    { id: "edit.alignRight", title: TEXT(locale).alignRight, ...arranges("right") },
    { id: "edit.alignTop", title: TEXT(locale).alignTop, ...arranges("top") },
    { id: "edit.alignMiddle", title: TEXT(locale).alignMiddle, ...arranges("middle") },
    { id: "edit.alignBottom", title: TEXT(locale).alignBottom, ...arranges("bottom") },
    { id: "edit.distributeAcross", title: TEXT(locale).distributeAcross, ...arranges("distributeAcross") },
    { id: "edit.distributeDown", title: TEXT(locale).distributeDown, ...arranges("distributeDown") },
    { id: "edit.sameWidth", title: TEXT(locale).sameWidth, ...arranges("sameWidth") },
    { id: "edit.sameHeight", title: TEXT(locale).sameHeight, ...arranges("sameHeight") },
    { id: "edit.sameSize", title: TEXT(locale).sameSize, ...arranges("sameSize") },
    { id: "edit.centerOnPage", title: message("centerOnPageAcross", locale), ...arranges("pageCenter") },
    { id: "edit.middleOnPage", title: message("centerOnPageDown", locale), ...arranges("pageMiddle") },
    {
      // Beside the box, and everything the entry above says about arming a mode
      // applies here unchanged. No chord for the same reason: a one-shot mode a
      // reader enters by accident and does not recognise is worse than one they
      // have to ask for.
      //
      // "Draw freehand", not "Ink": `/Ink` is the file's word and `ink` is the
      // wire's, and inside this codebase "ink" already names how a mark is laid
      // down rather than which mark it is. "Freehand" is also what separates it
      // from the box above at a glance, which is what a menu is for.
      id: "edit.draw",
      title: TEXT(locale).draw,
      enabled: withDocument,
      run: () => actions.draw(),
    },
    {
      // Beside the pen, because it is the other tool that stays armed and a
      // reader who has just drawn something is the reader who wants it.
      //
      // **"Erase marks", not "Erase drawing", since 2026-08-23**, and the
      // comment here used to argue the other way: *a bare "Erase" beside
      // "Remove mark" would read as a second, blunter way to delete anything*.
      // That was right while the nib took strokes out of drawings and nothing
      // else. It now takes any mark it crosses, so it *is* the blunter way to
      // delete anything --- and a title still promising drawings would send a
      // reader looking for another command to take a highlight off.
      //
      // The plural is what separates it from "Remove mark", which is singular
      // and takes the one mark a reader has named in its note box. This one is
      // aimed with the hand and takes what it touches. "Marks" rather than
      // "annotations" for `nameOf`'s reason: it is the word the panel, the note
      // box and the menu already use for the reader's own.
      //
      // The ellipsis matches the two tools above it and says the same thing
      // they do --- the command arms something rather than acting now.
      id: "edit.erase",
      title: TEXT(locale).erase,
      enabled: withDocument,
      run: () => actions.erase(),
    },
    {
      // **`h`, since 2026-10-14**, a bare letter as the page keys are (see
      // `BINDINGS`). The note that stood here said a chord for a command that
      // needs a selection "teaches itself badly", because a press with nothing
      // selected does nothing. ⌘C does nothing then too; and select, then `h`,
      // is the shortest way to the most frequent mark. ⌘H stays the system's
      // hide-application chord.
      id: "edit.highlightSelection",
      title: TEXT(locale).highlightSelection,
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("highlight"),
    },
    {
      // The same shape as the highlight above, and deliberately three entries
      // rather than one command that asks which kind: a reader who wants a
      // strikeout wants it in one press, and a palette that answered "Mark
      // selection" and then asked again would cost two. No letter of their
      // own: the highlight is the frequent one and has `h`, and these two are
      // a palette search away.
      id: "edit.underlineSelection",
      title: TEXT(locale).underlineSelection,
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("underline"),
    },
    {
      id: "edit.strikeoutSelection",
      title: TEXT(locale).strikeoutSelection,
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("strikeout"),
    },
    {
      // The fourth and last of them: PDF 32000-1 lists exactly four subtypes
      // that carry `/QuadPoints`, and with this one tpdf writes all four. A
      // fourth entry rather than an argument, for the reason the note above
      // gives about the other three.
      //
      // "Squiggly underline", not "Squiggly": the word alone names a shape
      // rather than an action, and it sits directly under "Underline selection"
      // where a reader is choosing between two lines under the same words.
      id: "edit.squigglySelection",
      title: TEXT(locale).squigglySelection,
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.markSelection("squiggly"),
    },
    // Built from `PALETTE` rather than written out, because a colour added there
    // and not here would be in the swatch row and reachable from nowhere else.
    // Seven separate commands rather than one that asks which colour, for the
    // reason the three selection marks above are three: a reader who wants green
    // wants it in one press, and a palette that answered "Mark colour" and then
    // asked again would cost two.
    //
    // No chords. Every letter that would suit is taken by navigation, and a
    // colour is not a thing anybody reaches for often enough to spend one on.
    ...PALETTE.map((entry) => ({
      id: `edit.color.${entry.id}`,
      // "Colour:" leads for `find.matchCase`'s reason --- it groups the run in a
      // list sorted by title, and it is what a reader searches for when they do
      // not know this application calls the colour "pink". The colour itself is
      // lowercase because it is a word in a sentence rather than a name.
      title: `${message("color", locale)}: ${entry.name}`,
      // Enabled on any open document, with or without a mark open: with one it
      // recolours, without one it arms the next mark. Guarding on `hasOpenMark`
      // would grey out exactly the case a reader uses to choose before marking.
      enabled: withDocument,
      run: () => actions.setMarkColor(entry.id),
    })),
    // Built from `NIBS` for `PALETTE`'s reason above: a nib added there and not
    // here would be a width nothing can reach. Four separate commands rather
    // than one that asks, on the same argument --- a reader who wants a broad
    // pen wants it in one press.
    //
    // **Enabled on any open document, and not on "a pen is armed"**, which is
    // the tempting guard and the wrong one. A reader picks the nib and *then*
    // takes the pen out at least as often as the reverse, and greying the
    // choice until a tool is armed would make the commoner order impossible.
    // It is the same reasoning the colours above state, and it lands here for a
    // different reason: a colour applies to an open mark too, and a nib never
    // does, so this had no second justification to fall back on.
    ...NIBS.map((entry) => ({
      id: `edit.nib.${entry.id}`,
      // "Nib:" leads for `Colour:`'s reason --- it groups the run in a list
      // sorted by title, and it is the word a reader searches for when they do
      // not know whether this application says "pen", "weight" or "thickness".
      title: TEXT(locale).nib(entry.name),
      enabled: withDocument,
      run: () => actions.setNib(entry.id),
    })),
    {
      // Takes the mark whose note is open, because that is the one the reader
      // has named. There is no "the mark under the pointer" here: a menu item
      // is chosen with the pointer somewhere else entirely, and the open note
      // is the application's own record of which mark is being worked on.
      // **Editing a comment somebody else wrote**, which until now the README
      // listed as not built. The command exists as well as the popup's own Edit
      // button for the reason every command here does: `docs/PLAN.md` asks that
      // everything be reachable in two keystrokes, and a reader who has opened a
      // note with the keyboard should not have to find a button with the mouse.
      //
      // It arms rather than acts, so there is nothing destructive behind it ---
      // the words are the reader's to type and undo takes the whole edit back.
      // No keyboard binding, because the popup has to be open for it to mean
      // anything and a chord that silently does nothing most of the time teaches
      // a reader to distrust it.
      id: "edit.editForeignMark",
      // "this comment" rather than "comment": the subject is whichever note is
      // open, which the reader has already pointed at, and the palette is chosen
      // with the pointer somewhere else. Same reasoning as "Remove mark", one
      // step further --- that one could not say which mark it meant, and this one
      // can say which *kind* of thing it means.
      title: TEXT(locale).editForeignMark,
      enabled: () => withDocument() && actions.canEditComment(),
      run: () => actions.editComment(),
    },
    {
      // **Acts rather than arming**, which is where it parts company with the
      // two commands around it: editing and replying open a box for the reader
      // to type in, and there is nothing to type here. Undo is what makes that
      // safe to do without asking, exactly as it is for deleting a page.
      //
      // No keyboard binding, for the reason the one above has none --- the popup
      // has to be open for it to mean anything, so a key that did nothing at
      // every other moment would be a key spent on almost nothing.
      id: "edit.deleteComment",
      // "this comment" for the two commands beside it, and the verb is *delete*
      // rather than *remove*: "Remove mark" takes one of the reader's own marks
      // off the page, and a reader choosing between the two in a palette is
      // choosing between their annotation and somebody else's. Two words for one
      // act would be worse than the asymmetry.
      title: TEXT(locale).deleteComment,
      enabled: () => withDocument() && actions.canDeleteComment(),
      run: () => actions.deleteComment(),
    },
    {
      // Beside the command above, and arming rather than acting for its reason.
      // The two are deliberately separate: answering a comment and changing what
      // it says are different acts on different objects, and a reader who meant
      // one is badly served by discovering they did the other.
      //
      // No keyboard binding, for the reason the one above has none --- the popup
      // has to be open for it to mean anything.
      id: "edit.replyToComment",
      // "this comment" for the reason above: the subject is whichever note is
      // open, and the palette is chosen with the pointer somewhere else.
      title: TEXT(locale).replyToComment,
      enabled: () => withDocument() && actions.canReplyToComment(),
      run: () => actions.replyToComment(),
    },
    {
      id: "edit.removeMark",
      // Not "Remove highlight", which was right while a highlight was the only
      // mark there was. A menu item naming one of three kinds is wrong twice
      // out of three times, and is chosen with the pointer somewhere else, so
      // it cannot say which mark it means the way the note box can.
      title: TEXT(locale).removeMark,
      enabled: () => withDocument() && actions.hasOpenMark(),
      run: () => actions.removeMark(),
    },
    // Marks, copied as their description and pasted through the ordinary add
    // path, in this tab or another. No keys: ⌘C and ⌘V copy and paste text.
    {
      id: "edit.copyMark",
      title: TEXT(locale).copyMark,
      enabled: () => withDocument() && actions.hasOpenMark(),
      run: () => actions.copyMarks(false),
    },
    {
      id: "edit.cutMark",
      title: TEXT(locale).cutMark,
      enabled: () => withDocument() && actions.hasOpenMark(),
      run: () => actions.copyMarks(true),
    },
    {
      id: "edit.pasteMark",
      title: TEXT(locale).pasteMark,
      enabled: () => withDocument() && actions.canPasteMarks(),
      run: () => actions.pasteMarks(),
    },
    {
      // **Mark a region for removal.** The first half of `docs/PLAN.md` §6:
      // nothing is destroyed, the region joins a list, and undo takes it back
      // off.
      //
      // **No keyboard binding**, for the reason "Delete page" has none: this is
      // the entry to the one workflow in the application that destroys content
      // a reader can see, and a mis-pressed chord that arms it silently is a
      // worse first experience than one extra keystroke. Two keystrokes in the
      // palette is what `docs/PLAN.md` asks of every command.
      //
      // The title leads with the verb a reader is looking for and says what the
      // gesture is, exactly as "Crop page by dragging" does --- and they sit
      // near each other in the palette on purpose, because a reader who has
      // confused the two should meet both.
      id: "edit.redactRegion",
      title: TEXT(locale).redactRegion,
      enabled: withDocument,
      run: () => actions.redactRegion(),
    },
    {
      // **The other way to say what to remove**, and the one a reader reaches
      // for first: selecting the words needs no aim, where a dragged rectangle
      // is a guess about where they are. It sits directly under the drag so
      // that a reader who found one has met the other.
      //
      // Guarded on the selection the way the four mark commands are, and for
      // the same reason rather than by copying them: with nothing selected it
      // would mark nothing, and a command that runs and does nothing reads as a
      // broken command. No chord, for the reason "Redact text or regions by
      // dragging" has none: this workflow removes content, and a stray key
      // that arms it is worse than one extra keystroke.
      id: "edit.redactSelection",
      title: TEXT(locale).redactSelection,
      enabled: () => withDocument() && actions.hasSelection(),
      run: () => actions.redactSelection(),
    },
    {
      // **The third way**, and the only one that scales: an order number on two
      // hundred pages is not something a reader drags or selects. It takes the
      // pattern from the find field rather than asking for one, so the reader
      // has already seen what it will mark --- and it is guarded on there being
      // matches for `edit.redactSelection`'s reason, since with none it would
      // mark nothing and read as broken.
      //
      // "every search result" rather than "matches": the reader is looking at a
      // results panel, and the title should name what is in front of them.
      id: "edit.redactMatches",
      title: TEXT(locale).redactMatches,
      enabled: () => withDocument() && actions.matchCount() > 0,
      run: () => actions.redactMatches(),
    },
    {
      // **The way back off**, and until it existed there were only two: the
      // review panel's remove control, which a reader had to know to open, and
      // undo, which is chronological --- a reader who dragged six regions and
      // wants the second one back cannot get there by undoing. Right-clicking
      // the region is the gesture that says *this one*, and it produced a menu
      // about the selection instead. Reported from use.
      //
      // Guarded on a region being picked, exactly as "Remove mark" is guarded
      // on a note being open, and for that command's reason rather than by
      // copying it: with nothing picked it would remove nothing, and a command
      // that runs and does nothing reads as a broken command.
      //
      // No keyboard binding, for the reason "Redact text or regions by dragging" has
      // none: every command in this workflow is one keystroke more expensive
      // than it could be, on purpose.
      //
      // "Remove" rather than "Unredact" or "Cancel": the review panel's control
      // already says *Remove region*, and one act with two names in two places
      // is worse than the slight asymmetry with the verb "Redact" above it.
      id: "edit.removeRedaction",
      title: TEXT(locale).removeRedaction,
      enabled: () => withDocument() && actions.hasPickedRedaction(),
      run: () => actions.removeRedaction(),
    },
    {
      id: "edit.fillForm",
      title: TEXT(locale).fillForm,
      enabled: withDocument,
      run: () => actions.fillForm(),
    },
    {
      // **Crop by dragging.** The one page operation only the reader can decide:
      // a figure out of a plate, one column of two, a scan with a hand in the
      // corner. Measuring the ink cannot answer any of those, because there is
      // nothing wrong with what it measures --- the reader simply wants less
      // than all of it.
      //
      // First of the three, and that ordering is the recommendation: this is
      // what "crop" means to somebody who has not read the other two, and a
      // palette ranks by the order it is given for equal scores.
      //
      // It arms rather than acting, so it is the one crop command whose outcome
      // the window harness can wait for --- the other two are IPC replies and
      // are excused there for that reason. See `armCrop`.
      id: "edit.cropToDrag",
      title: TEXT(locale).cropToDrag,
      enabled: withDocument,
      run: () => actions.cropPage("drag"),
    },
    {
      // **Crop to content**, which is still the better answer for the case it
      // fits --- a scan, or an article whose margins are wider than its column.
      // It needs no gesture at all, which is why it exists beside the drag
      // rather than being replaced by it.
      //
      // Measured from a low-resolution render rather than from the page's
      // objects, which is what makes it work on a scan at all: see `content.rs`,
      // where a scanned page is one image object covering the sheet and the
      // object union is therefore the sheet.
      id: "edit.cropToContent",
      title: TEXT(locale).cropToContent,
      enabled: withDocument,
      run: () => actions.cropPage("content"),
    },
    {
      // Offered whether or not this page is cropped, like Undo and unlike
      // "Remove mark": whether a crop is in force is a property of whichever
      // page the reader has scrolled to, and a command that came and went as
      // they scrolled would be worse than one that sometimes does nothing.
      id: "edit.resetCrop",
      title: TEXT(locale).resetCrop,
      enabled: withDocument,
      run: () => actions.cropPage("reset"),
    },
    {
      // **No keyboard binding, and that is the decision rather than an
      // omission.** Every other page operation has one, and this is the only
      // command in the application that removes something a reader can see. It
      // is undoable, which is the argument for a chord --- and a mis-pressed
      // chord that silently removes a page from a document somebody is halfway
      // through reading is a worse first experience than one extra keystroke. It
      // is two keystrokes away in the palette, which is what `docs/PLAN.md` asks
      // of every command.
      id: "edit.deletePage",
      title: TEXT(locale).deletePage,
      enabled: withDocument,
      run: () => actions.deletePage(),
    },
    {
      // No binding, for the reason the deletion above has none turned around:
      // this adds rather than removes, so a mis-press costs an undo and not a
      // page --- but there is no chord that reads as "insert a page" and is not
      // already something else, and the palette is two keystrokes.
      //
      // **Named for the blank page rather than for inserting**, because
      // inserting pages *from another file* is the other half of the same
      // README bullet and has a command of its own, `edit.insertPages`, below.
      id: "edit.insertBlankPage",
      title: TEXT(locale).insertBlankPage,
      enabled: withDocument,
      run: () => actions.insertBlankPage(),
    },
    ...PAGE_SIZE_NAMES.map((name) => ({
      // **One command per size, and the alternative was one that asks.** The
      // same argument `edit.stamp.*` makes and this follows its spelling: a
      // reader who wants A4 has a name, not two numbers, and typing the name
      // into a prompt is slower than typing it into the palette that is already
      // open.
      //
      // No ellipsis, unlike the stamps: nothing further is asked of the reader
      // and the page appears, which is what the entry above promises too.
      //
      // The title puts the size *inside* "Insert blank ... page" rather than
      // after it, so no title here is a strict prefix of another --- that tie is
      // decided by registration order, and `docs/TRAPS.md` records it shipping
      // once. The whole-title bonus in `commands.ts` would settle it anyway;
      // this costs nothing and does not lean on it.
      id: `edit.insertPage.${name}`,
      title: TEXT(locale).insertSizedPage(PAGE_SIZES[name].title),
      enabled: withDocument,
      run: () => actions.insertSizedPage(name),
    })),
    {
      // The other half of the README bullet the blank page is half of, and the
      // id that bullet has named since before it was built. The ellipsis is a
      // promise the title keeps: a dialog asks which file.
      //
      // Which pages is asked next, by `edit.insertPages.range` below; the ones
      // chosen land in the file's own order after the page being read, and one
      // undo takes them all back out.
      id: "edit.insertPages",
      title: TEXT(locale).insertPages,
      enabled: withDocument,
      run: () => actions.importPages(),
    },
    {
      // The second question `edit.insertPages` asks, once the file is open and
      // its page count known: which of its pages. Reached only through that
      // command, which opens the palette straight into it with `askFor` --- so
      // it is enabled only while a file is waiting, and a disabled command is
      // not listed, which keeps it out of the palette the rest of the time
      // rather than showing a row that does nothing.
      //
      // **A blank answer is every page**, which is what the command did before
      // it asked. `chosenPages` has why extract does not share that default.
      //
      // `dismissed` is the other half of the pair and not optional here: the
      // file is open in a worker pool while this asks, and a reader who presses
      // Escape has decided against it. The parse runs in each callback for
      // `file.extractPages`' reason.
      id: "edit.insertPages.range",
      get title() {
        const file = actions.pendingImport();
        return file ? TEXT(locale).insertPagesFrom(file.name) : TEXT(locale).insertPagesFromChosen;
      },
      enabled: () => withDocument() && actions.pendingImport() !== null,
      argument: {
        get placeholder() {
          const file = actions.pendingImport();
          return file ? rangePlaceholder(file) : TEXT(locale).pagesPlaceholder;
        },
        problem: (raw: string) => {
          const file = actions.pendingImport();
          if (!file) return TEXT(locale).noFileWaiting;
          return chosenPages(raw, file).problem ?? null;
        },
        preview: (raw: string) => {
          const file = actions.pendingImport();
          return file ? rangePreview(raw, file) : "";
        },
        run: (raw: string) => {
          const file = actions.pendingImport();
          const range = file ? chosenPages(raw, file) : null;
          // Unreachable through the palette, which refuses to run a command
          // whose `problem` answered; a guard for `file.extractPages`' reason.
          if (!range?.slots) return;
          actions.insertChosenPages(range.slots);
        },
        dismissed: () => actions.dropImport(),
      },
    },
    {
      // No binding either, and for a different reason than the deletion above:
      // there is no chord left that reads as "move a page" rather than as "move
      // the view", and a reader who wants to rearrange a document is going to
      // want the page strip anyway. `docs/PLAN.md` has dragging as the half this
      // increment did not build; these two are the command it will call.
      //
      // Off either end does nothing rather than wrapping. A reader who holds a
      // key down expects a page to stop at the end of a document, and a page
      // that reappeared at the other end would be a deletion as far as the eye
      // is concerned.
      id: "edit.movePageUp",
      title: TEXT(locale).movePageUp,
      enabled: withDocument,
      run: () => actions.movePage(-1),
    },
    {
      id: "edit.movePageDown",
      title: TEXT(locale).movePageDown,
      enabled: withDocument,
      run: () => actions.movePage(1),
    },
    {
      // Guarded on there being something to undo rather than merely on a
      // document being open. A palette that offers Undo with an empty journal
      // teaches a reader that the command does nothing, which is the same
      // lesson a broken one teaches.
      id: "edit.undo",
      title: TEXT(locale).undo,
      keys: label("edit.undo"),
      enabled: () => withDocument() && actions.canUndo(),
      run: () => actions.undoEdit(),
    },
    {
      id: "edit.redo",
      title: TEXT(locale).redo,
      keys: label("edit.redo"),
      enabled: () => withDocument() && actions.canRedo(),
      run: () => actions.redoEdit(),
    },
    {
      // Guarded on there being something to save, for the reason `edit.undo`
      // is guarded on there being something to undo. It is the opposite of the
      // guard on `file.saveCopy` below, and deliberately: a copy of an unedited
      // document is a file a reader wants, and rewriting the open file to say
      // exactly what it already says is not.
      id: "file.save",
      title: TEXT(locale).save,
      keys: label("file.save"),
      enabled: () => withDocument() && actions.isDirty(),
      run: () => actions.saveDocument(),
    },
    {
      // Offered on any open document, not only an edited one. Saving an
      // unedited copy is a thing readers do --- it is how you get a file out of
      // a downloads folder --- and a command that appears only after an edit is
      // one nobody finds.
      id: "file.saveCopy",
      title: TEXT(locale).saveCopy,
      keys: label("file.saveCopy"),
      enabled: withDocument,
      run: () => actions.saveCopy(),
    },
    {
      // Offered on any open document, for `file.saveCopy`'s reason: a command
      // that appears only when it can succeed is one nobody finds. A document
      // with unsaved edits is told to save first, by `signing.ts`, rather than
      // not being offered the command at all.
      //
      // "Sign document…", not "Sign": the ellipsis is the chooser and the save
      // panel that follow, and the result is a new file.
      id: "file.signDocument",
      title: TEXT(locale).signDocument,
      enabled: withDocument,
      run: () => actions.signDocument(),
    },
    {
      // The same signing, into a signature field the document has: the first
      // empty one. A press on a field on its page signs that one.
      id: "file.signField",
      title: TEXT(locale).signField,
      enabled: () => withDocument() && actions.signableFields() >= 1,
      run: () => actions.signField(),
    },
    {
      // Linux only: macOS and Windows sign from the system's store, where a
      // certificate file is imported with the system's own tools. Needs no
      // open document; the file is remembered for every signing after.
      id: "file.addCertificateFile",
      title: TEXT(locale).addCertificateFile,
      enabled: () => isLinux(),
      run: () => actions.addCertificateFile(),
    },
    {
      // For a document somebody has already signed: no key is used and no
      // signature is made. Offered on any open document, as signing is: one
      // with no signature is told so at once, which is an answer a reader
      // looking for the command would otherwise have to guess.
      //
      // The ellipsis is the dialog and the save panel that follow, and the
      // result is a new file.
      id: "file.addValidationData",
      title: TEXT(locale).addValidationData,
      enabled: withDocument,
      run: () => actions.addValidationData(),
    },
    {
      // "and save as", for `file.redactCopy`'s reason: the open document is not
      // changed, and the title says where the result goes. Offered on any open
      // document, a clean one with text included --- that one is told its pages
      // already have text, which is an answer a reader looking for the command
      // would otherwise have to guess.
      id: "file.recogniseText",
      title: message("recogniseText", locale),
      enabled: withDocument,
      run: () => actions.recogniseText(),
    },
    {
      id: "file.recognisePage",
      title: message("recognisePage", locale),
      enabled: withDocument,
      run: () => actions.recognisePage(),
    },
    {
      // A preference, not something done to a document, so it is offered with
      // none open: a reader can set it before the first scan. The ellipsis is
      // kept by the palette, which asks next; the list it asks about is the
      // machine's and is fetched first, which is why this is a `run` and the
      // question is the command below --- `edit.insertPages`' shape.
      id: "file.recogniseTextLanguage",
      title: message("recogniseLanguage", locale),
      run: () => actions.chooseRecognitionLanguage(),
    },
    {
      // The question `file.recogniseTextLanguage` asks once the list is here.
      // Enabled only while that list is held, so it is not listed otherwise.
      // `dismissed` drops the list: the next question fetches a fresh one, in
      // which a language installed since is present.
      id: "file.recogniseTextLanguage.choice",
      title: message("recogniseLanguageChoice", locale),
      enabled: () => actions.recognitionLanguages() !== null,
      argument: {
        get placeholder() {
          return languagePlaceholder(actions.recognitionLanguages()?.current ?? null);
        },
        problem: (raw: string) => {
          const asked = actions.recognitionLanguages();
          if (!asked) return TEXT(locale).noLanguageList;
          return pickLanguage(raw, asked.offered).problem ?? null;
        },
        preview: (raw: string) => {
          const asked = actions.recognitionLanguages();
          const picked = asked ? pickLanguage(raw, asked.offered) : null;
          return picked && picked.problem === undefined ? languagePreview(picked.language) : "";
        },
        run: (raw: string) => actions.setRecognitionLanguage(raw),
        dismissed: () => actions.dropRecognitionLanguages(),
      },
    },
    {
      // A copy, because shrinking pictures cannot be undone: the open
      // document and the file it came from keep every pixel.
      id: "file.compress",
      title: TEXT(locale).compress,
      enabled: withDocument,
      run: () => actions.compressCopy(),
    },
    {
      // *Copy a region as a picture*, to a PNG file rather than the clipboard.
      // A worker renders and encodes it; the web view names the region only.
      id: "file.saveRegionAsPicture",
      title: TEXT(locale).saveRegionAsPicture,
      enabled: withDocument,
      run: () => actions.saveRegion(),
    },
    {
      // A file the document carries, saved where the reader chooses and never
      // opened. With one there is no question; with several the palette asks
      // which, below, once the list is read. Offered on every document: one
      // with none is told so, which a missing command would not say.
      id: "file.saveAttachment",
      title: TEXT(locale).saveAttachment,
      enabled: withDocument,
      run: () => actions.saveAttachment(),
    },
    {
      // The question `file.saveAttachment` asks when there are several.
      // Enabled only while that list is held, so it is not listed otherwise.
      id: "file.saveAttachment.choice",
      title: TEXT(locale).saveAttachmentChoice,
      enabled: () => actions.attachmentsAsked() !== null,
      argument: {
        get placeholder() {
          return attachmentPlaceholder(actions.attachmentsAsked() ?? []);
        },
        problem: (raw: string) => {
          const files = actions.attachmentsAsked();
          if (!files) return TEXT(locale).noAttachmentList;
          const picked = pickAttachment(raw, files);
          return "problem" in picked ? picked.problem : null;
        },
        preview: (raw: string) => attachmentPreview(raw, actions.attachmentsAsked() ?? []),
        run: (raw: string) => actions.pickAttachment(raw),
        dismissed: () => actions.dropAttachments(),
      },
    },
    {
      // Copies, like every command that changes what a file is: the open
      // document keeps the password it has, or none.
      id: "file.protect",
      title: TEXT(locale).protect,
      enabled: withDocument,
      run: () => actions.protectCopy(),
    },
    {
      // Offered on any open document rather than only on one that asked for a
      // password: a document that never asked is told why there is nothing to
      // remove, which a missing command would not say.
      id: "file.unprotect",
      title: TEXT(locale).unprotect,
      enabled: withDocument,
      run: () => actions.unprotectCopy(),
    },
    {
      // A copy rather than an edit of the open document, as the password and
      // the smaller copy are: the numbers count the pages of the file written,
      // and a reader keeps an unnumbered original.
      id: "file.addPageNumbers",
      title: TEXT(locale).addPageNumbers,
      enabled: withDocument,
      run: () => actions.addText("numbers"),
    },
    {
      id: "file.addWatermark",
      title: TEXT(locale).addWatermark,
      enabled: withDocument,
      run: () => actions.addText("watermark"),
    },
    {
      // A separate fallback rather than a mode of `file.redactCopy`: the result
      // has a different capability contract, and the title must say so before a
      // reader chooses it. The confirmation and destination dialog live at the
      // shell boundary in `App.svelte`.
      id: "file.redactRasterCopy",
      title: TEXT(locale).redactRasterCopy,
      enabled: withDocument,
      run: () => actions.redactRasterCopy(),
    },
    {
      // **"Redact and save as", not "Redact".** Nothing here changes the open
      // document, and a command called Redact reads as one that does --- which
      // is the difference between a reader who checks the new file and one who
      // closes the old one believing it is clean. The name says where the
      // result goes, which is the whole of what is safe about this step.
      //
      // Guarded on the document rather than on there being a region marked: the
      // palette is how a command is *found*, and a reader who wants to know
      // whether tpdf can redact should meet it and be told what to do, rather
      // than not meet it at all. `lib.rs` refuses with "nothing in this document
      // is marked for removal", which is the sentence that answers them.
      id: "file.redactCopy",
      title: TEXT(locale).redactCopy,
      enabled: withDocument,
      run: () => actions.redactCopy(),
    },
    {
      // **"Redact and save", beside "Redact and save as..."**, which is the same
      // distinction Save and Save a copy draw and the reason neither of these is
      // called plain "Redact": the name has to say where the result goes,
      // because that is the whole of what separates them. The ellipsis carries
      // the rest --- one asks for a name, this one does not, because it already
      // has the only name it can write to.
      //
      // Guarded on the document rather than on there being a region marked, for
      // its sibling's reason: the palette is how a command is found, and a
      // reader who wants to know whether tpdf can redact in place should meet it
      // and be told what to do. `lib.rs` refuses with "nothing in this document
      // is marked for removal".
      //
      // No shortcut. Every chord that reads as this one is a save, and a slip
      // between Save and a command that destroys content is the one slip this
      // application must not make cheap.
      id: "file.redactDocument",
      title: TEXT(locale).redactDocument,
      enabled: withDocument,
      run: () => actions.redactDocument(),
    },
    {
      // Takes a selection where `file.saveCopy` takes none, and is otherwise
      // the same operation over fewer pages --- it shares the whole write path,
      // so an encrypted document is refused here for the reason it is refused
      // there.
      //
      // The parse runs twice, in `problem` and again in `run`, and that is
      // deliberate: `parsePageRange` is pure and cheap, and the alternative is
      // a validated value cached between two callbacks that the palette is free
      // to invoke in either order. Two calls to one function cannot disagree;
      // a cache and its writer can.
      id: "file.extractPages",
      title: TEXT(locale).extractPages,
      // No shortcut, for the reason the two move commands have none: there is
      // no chord left that reads as "extract", and this is a command a reader
      // reaches deliberately rather than by muscle memory.
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).extractPlaceholder,
        problem: (raw: string) =>
          parsePageRange(raw, actions.pageCount()).problem ?? null,
        preview: (raw: string) => {
          const range = parsePageRange(raw, actions.pageCount());
          return range.slots ? describeRange(range.slots) : "";
        },
        run: (raw: string) => {
          const range = parsePageRange(raw, actions.pageCount());
          // Cannot be reached through the palette, which refuses to run a
          // command whose `problem` answered. Written as a guard rather than a
          // `!` because the two callbacks are independent entry points and this
          // one is what actually writes a file.
          if (!range.slots) return;
          actions.extractPages(range.slots);
        },
      },
    },
    {
      // Grouped with extract and merge, and a `file.` command for their reason:
      // it changes nothing about the open document and what it produces is
      // somewhere else.
      //
      // **The argument is the cuts, not the files.** `3,7` on ten pages writes
      // three files, 1-3, 4-7 and 8-10. `parseSplitPoints` states why the
      // numbers are the last page of a file rather than the first of the next,
      // and why "every N pages" is a different grammar that is not this one.
      //
      // The parse runs twice, in `problem` and again in `run`, which is
      // `file.extractPages`' arrangement and carries its reason: the function
      // is pure and cheap, and the alternative is a validated value cached
      // between two callbacks the palette may invoke in either order.
      id: "file.splitDocument",
      title: TEXT(locale).splitDocument,
      // No shortcut, for the reason extract and merge have none: no chord reads
      // as "split", and this is reached deliberately.
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).splitPlaceholder,
        problem: (raw: string) =>
          parseSplitPoints(raw, actions.pageCount()).problem ?? null,
        preview: (raw: string) => {
          const split = parseSplitPoints(raw, actions.pageCount());
          return split.groups ? describeSplit(split.groups) : "";
        },
        run: (raw: string) => {
          const split = parseSplitPoints(raw, actions.pageCount());
          // Unreachable through the palette, which refuses to run a command
          // whose `problem` answered. A guard rather than a `!` for
          // `file.extractPages`' reason: the two callbacks are independent
          // entry points and this is the one that writes files.
          if (!split.groups) return;
          actions.splitDocument(split.groups);
        },
      },
    },
    {
      // The mirror of `file.extractPages`, and grouped with it rather than
      // under an edit prefix for the reason that one is a `file.` command:
      // neither changes the open document. Extract reads part of one file,
      // merge reads all of several, and both produce a file somewhere else.
      //
      // **No argument**, where extract takes a page range. What a merge needs
      // is a list of files, and the palette's argument is a line of text ---
      // paths typed into it would be a second file chooser that cannot
      // complete, cannot browse and cannot say whether what was typed exists.
      // So the picking happens in the platform's own dialog, which is also
      // where `file.open` sends the reader.
      id: "file.mergeDocuments",
      title: TEXT(locale).mergeDocuments,
      // No shortcut, for `file.extractPages`' reason: no chord reads as
      // "merge", and this is reached deliberately rather than by reflex.
      enabled: withDocument,
      run: () => actions.mergeDocuments(),
    },
    {
      id: "nav.nextPage",
      title: TEXT(locale).nextPage,
      keys: label("nav.nextPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.nextPage(),
    },
    {
      id: "nav.previousPage",
      title: TEXT(locale).previousPage,
      keys: label("nav.previousPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.previousPage(),
    },
    {
      // Back exists because following a cross-reference without one is a trap:
      // it moves a reader into the middle of something and leaves them to
      // remember the page number they came from. It records positions rather
      // than links, so a jump from the outline or a search result is on the
      // same stack --- which is what a reader who has used a browser expects.
      id: "nav.back",
      title: TEXT(locale).back,
      keys: label("nav.back"),
      // **Greys when there is nowhere to go**, since 2026-08-23. Both of these
      // were guarded on `withDocument` alone, so the menu offered Back on a
      // document nobody had jumped in and the press did nothing --- and the
      // viewer's `onNavigate` existed to fix exactly that and was consumed by
      // nothing, which the wiring gate carried as its one exemption.
      //
      // `canGoBack` is `History`'s own answer, so the guard and the action read
      // one fact. A predicate written here --- "has the reader followed a link"
      // --- would be a second copy of the stack's state and would go stale the
      // first time Forward was pressed.
      enabled: () => withDocument() && (actions.viewer()?.canGoBack ?? false),
      run: () => {
        actions.viewer()?.goBack();
      },
    },
    {
      id: "nav.forward",
      title: TEXT(locale).forward,
      keys: label("nav.forward"),
      enabled: () => withDocument() && (actions.viewer()?.canGoForward ?? false),
      run: () => {
        actions.viewer()?.goForward();
      },
    },
    {
      // The only way to reach a link without a pointer. Until this existed a
      // reader on the keyboard could move by page, heading and search hit and
      // could not follow a cross-reference at all -- which on a document whose
      // table of contents is its navigation is most of the document.
      id: "nav.nextLink",
      title: TEXT(locale).nextLink,
      keys: label("nav.nextLink"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepLink(1);
      },
    },
    {
      id: "nav.previousLink",
      title: TEXT(locale).previousLink,
      keys: label("nav.previousLink"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepLink(-1);
      },
    },
    {
      // The only way to reach a mark of the reader's own without a pointer.
      // Until this existed a highlight's note could not be read, changed or
      // taken off from the keyboard at all --- the whole subsystem was reachable
      // by one input device.
      //
      // `nav` rather than `edit`, and that is not filing: it *moves* the reader.
      // Editing is what the note box and "Remove mark" then do, and both are
      // already commands of their own.
      id: "nav.nextMark",
      title: TEXT(locale).nextMark,
      keys: label("nav.nextMark"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepMark(1);
      },
    },
    {
      id: "nav.previousMark",
      title: TEXT(locale).previousMark,
      keys: label("nav.previousMark"),
      enabled: withDocument,
      run: () => {
        actions.viewer()?.stepMark(-1);
      },
    },
    {
      // On both sides at once: a change is a place in each document, and
      // stepping one side alone would leave the other showing the change
      // before.
      id: "nav.nextChange",
      title: TEXT(locale).nextChange,
      enabled: () => withDocument() && actions.changeCount() > 0,
      run: () => actions.stepChange(1),
    },
    {
      id: "nav.previousChange",
      title: TEXT(locale).previousChange,
      enabled: () => withDocument() && actions.changeCount() > 0,
      run: () => actions.stepChange(-1),
    },
    {
      id: "nav.firstPage",
      title: TEXT(locale).firstPage,
      keys: label("nav.firstPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.goToStart(),
    },
    {
      id: "nav.lastPage",
      title: TEXT(locale).lastPage,
      keys: label("nav.lastPage"),
      enabled: withDocument,
      run: () => actions.viewer()?.goToEnd(),
    },
    {
      // The first command that takes a value. On a 775-page document there was
      // no way to reach page 400 at all: Home, End and one page at a time.
      //
      // Numbers here are the ones printed on the page --- one-based --- and
      // `goToPage` is zero-based, so the conversion happens once, here, next to
      // the text that says "of {pageCount}". `problem` refuses out of range
      // rather than clamping: a reader who types 900 in a 775-page document has
      // made a mistake, and silently going to the last page hides it.
      id: "nav.goToPage",
      title: TEXT(locale).goToPage,
      keys: label("nav.goToPage"),
      enabled: withDocument,
      argument: {
        placeholder: TEXT(locale).pageNumberPlaceholder,
        problem: (raw: string) => {
          const pages = actions.pageCount();
          const trimmed = raw.trim();
          if (trimmed === "") return TEXT(locale).pageNumberRange(pages);
          if (!/^[0-9]+$/.test(trimmed))
            return TEXT(locale).notAPageNumber(trimmed);
          const page = Number(trimmed);
          if (page < 1 || page > pages) {
            return TEXT(locale).documentPages(pages);
          }
          return null;
        },
        preview: (raw: string) =>
          TEXT(locale).goToPagePreview(Number(raw.trim()), actions.pageCount()),
        run: (raw: string) =>
          actions.viewer()?.goToPage(Number(raw.trim()) - 1),
      },
    },
    {
      id: "edit.selectAll",
      title: TEXT(locale).selectAll,
      keys: label("edit.selectAll"),
      enabled: withDocument,
      run: () => actions.viewer()?.selectPage(),
    },
    {
      id: "edit.copy",
      title: TEXT(locale).copy,
      keys: label("edit.copy"),
      enabled: withDocument,
      run: () => void actions.viewer()?.copySelection(),
    },
    {
      // The page as a picture rather than as text: a figure, a signature, a
      // table to paste into a mail. A drag says which part, and a worker
      // renders it at the screen's sharpness (`regionshot.ts`). Through the
      // web view's clipboard, which takes a PNG on macOS (measured
      // 2026-10-16); where it refuses, the window says why.
      id: "edit.copyRegionAsPicture",
      title: TEXT(locale).copyRegionAsPicture,
      enabled: withDocument,
      run: () => actions.copyRegion(),
    },
    {
      id: "edit.clearSelection",
      title: TEXT(locale).clearSelection,
      keys: label("edit.clearSelection"),
      enabled: withDocument,
      run: () => actions.viewer()?.clearSelection(),
    },
    {
      // Named for both things a reader might type. One command with one
      // binding rather than two commands sharing one, which would show the
      // same shortcut twice in the palette and teach that it does two things.
      id: "view.toggleSidebar",
      title: TEXT(locale).toggleSidebar,
      keys: label("view.toggleSidebar"),
      enabled: withDocument,
      run: () => actions.toggleSidebar(),
    },
    {
      // Two commands rather than one "switch tab", because the palette is how a
      // command is *found*: someone looking for thumbnails types "thumb", and a
      // command called "Switch sidebar tab" is not what they would type.
      id: "view.showOutline",
      title: TEXT(locale).showOutline,
      enabled: withDocument,
      run: () => actions.showTab("outline"),
    },
    {
      id: "view.showThumbnails",
      title: TEXT(locale).showThumbnails,
      enabled: withDocument,
      run: () => actions.showTab("pages"),
    },
    {
      // "Show my marks", not "Show marks": the panel lists what *this* reader
      // made, and the document's own annotations are in the comments tab
      // beside it. A reader with both open needs the two titles to say which
      // is which, and "Annotations" would name either.
      id: "view.showMarks",
      title: TEXT(locale).showMarks,
      enabled: withDocument,
      run: () => actions.showTab("marks"),
    },
    {
      // "Show redactions", not "Show regions": a reader looking for this has the
      // word *redact* in mind --- it is what the tool that made these is called
      // --- and "region" names the shape rather than the purpose. The panel's
      // own tab carries the same word for the same reason.
      id: "view.showRedactions",
      title: TEXT(locale).showRedactions,
      enabled: withDocument,
      run: () => actions.showTab("redactions"),
    },
    {
      // "Invert page colours", not "Dark mode". The chrome is already dark when
      // the desktop is, so a command called dark mode would appear to do nothing
      // for the reader who most expects it to --- and what this actually does is
      // change how the document looks, which is worth saying out loud.
      id: "view.invertPages",
      title: message("invertPageColors", locale),
      keys: label("view.invertPages"),
      enabled: withDocument,
      run: () => actions.toggleInvert(),
    },
  );
  // In a language other than English, each command also answers to its
  // American title, so a reader who types the English words finds it. Read
  // from a registry of the same commands in American English, through the
  // command and not copied, because some titles are worded when asked.
  if (locale.startsWith("en")) {
    attachKeywords(registry, locale);
    return;
  }
  const english = new CommandRegistry();
  registerAppCommands(english, actions, "en-US");
  attachKeywords(registry, locale, (id) => english.find(id)?.title);
}

/** The part of the palette the window shortcuts drive. */
export interface PaletteLike {
  readonly isOpen: boolean;
  open(): void;
  close(): void;
  askFor(id: string): void;
}

/**
 * Re-renders every command's advertised shortcut from the binding table.
 *
 * Called once, after the platform has said what this keyboard prints --- see
 * `keys.ts`'s {@link setPrintedKeys} and `src-tauri/src/keylayout.rs`. A command
 * registers with the label that can be rendered synchronously, which for a
 * binding naming a physical key is the character it declares; this replaces that
 * with the key the reader can actually see on the keyboard in front of them.
 *
 * Mutating the registered commands rather than rebuilding the registry, because
 * a rebuild would drop the recent-command list and re-run every registration for
 * a change that touches one string. A command with no binding is left alone: its
 * `keys` is undefined and the palette shows no shortcut, which is right.
 */
export function relabelCommands(registry: CommandRegistry): void {
  for (const command of registry.all()) {
    if (command.id in BINDINGS) command.keys = label(command.id as BoundCommand);
  }
}

/** What {@link handleWindowKey} reaches for. */
export interface WindowKeyDeps {
  actions: AppActions;
  /** The palette, or null before the shell has built one. */
  palette(): PaletteLike | null;
  /**
   * Whether a document is open.
   *
   * `App.svelte` answered this with the window title, and every binding but two
   * is gated on it. Kept distinct from `actions.viewer() !== null`, which is
   * what a command's `enabled` asks: they agree in practice and they are not the
   * same question, and collapsing them here would be a change disguised as a
   * move.
   */
  hasDocument(): boolean;
  /** Re-read the recent-document list behind an opening palette. */
  refreshRecents(): void;
}

/**
 * Opens the palette, or closes an open one.
 *
 * Extracted from ⌘K's arm in {@link handleWindowKey} so the toolbar's palette
 * button reaches the same code rather than a second copy of it. That button is
 * not a convenience: `menu.rs` puts the native menu bar behind
 * `#[cfg(target_os = "macos")]` on purpose, so on Windows the palette is the
 * only route to most of this application's commands, and it was advertised
 * nowhere on screen --- a reader had to already know ⌘K to find anything. A
 * second implementation here would be a second place to forget the recents
 * refresh, which is the disagreement `keys.ts` exists to make impossible for
 * labels and this exists to make impossible for one command.
 */
export function togglePalette(
  deps: Pick<WindowKeyDeps, "palette" | "refreshRecents">,
): void {
  const palette = deps.palette();
  // Toggling rather than reopening: Cmd-K on an open palette is a request
  // to get rid of it, not to clear the query someone is halfway through.
  if (palette?.isOpen) palette.close();
  else {
    palette?.open();
    // Opened first and refreshed behind it. The list only changes when a
    // document is opened, so it is almost always already right, and blocking
    // a keystroke on a file read to cover the case where it is not would
    // make every use of the palette pay for it.
    deps.refreshRecents();
  }
}

/**
 * The shortcuts that belong to the window rather than to the surface.
 *
 * Matched through `keys.ts`, which is where the palette's labels come from
 * too --- see the note there, and note that ⌘K is now in that table as well.
 * It was the one exception, on the reasoning that the palette is not a row in
 * itself so no label could disagree with it; the toolbar button added for
 * Windows renders a label for it, which ended the exception.
 */
export function handleWindowKey(
  event: KeyboardEvent,
  deps: WindowKeyDeps,
): void {
  if (copyInterfaceText(event, async (text) => navigator.clipboard.writeText(text))) return;
  const palette = deps.palette();
  const { actions } = deps;
  const title = deps.hasDocument();

  if (matches("file.close", event) && title) {
    event.preventDefault();
    if (!actions.busyOpening()) actions.closeDocument();
  } else if (matches("view.nextTab", event) || matches("view.previousTab", event)) {
    event.preventDefault();
    if (!actions.busyOpening()) actions.nextDocument(event.shiftKey ? -1 : 1);
  } else if (matches("app.palette", event)) {
    event.preventDefault();
    togglePalette(deps);
  } else if (matches("nav.goToPage", event) && title) {
    // Straight into the palette's argument mode. The shortcut and the palette
    // row reach the same code, which is the point of `askFor` -- a second way
    // to ask for a page number is a second thing to keep right.
    event.preventDefault();
    palette?.askFor("nav.goToPage");
  } else if (matches("view.zoomTo", event) && title) {
    event.preventDefault();
    palette?.askFor("view.zoomTo");
  } else if (matches("file.open", event)) {
    // ⌘O was advertised in the palette and reached nothing at all: the label
    // was written by hand and no handler was ever added for it, which is the
    // exact disagreement the shared table exists to make impossible.
    //
    // Prevented whether or not an open is already running, but only issued
    // when one is not --- the same guard the Open button carries as
    // `disabled`. Without it the keyboard is the one path that can stack file
    // dialogs, and the second chooser's document then waits behind the first
    // in `opens` for no reason anyone asked for.
    event.preventDefault();
    if (!actions.busyOpening()) actions.openDocument();
  } else if (matches("find.open", event) && title) {
    event.preventDefault();
    actions.focusFind();
  } else if (matches("file.print", event)) {
    // Prevented whether or not a document is open --- note the missing
    // `&& title` that every other binding here has. WKWebView's own Cmd-P
    // prints the *page*: the chrome, the toolbar, and a scaled-down
    // screenshot of whatever tiles happen to be painted. On the empty state
    // that is a picture of the words "Open a PDF, or drop one here."
    event.preventDefault();
    actions.printDocument();
  } else if (matches("view.toggleSidebar", event) && title) {
    event.preventDefault();
    actions.toggleSidebar();
  } else if (matches("view.invertPages", event) && title) {
    event.preventDefault();
    actions.toggleInvert();
  } else if (matches("file.reopenClosedTab", event)) {
    event.preventDefault();
    if (actions.canReopenClosedTab()) actions.reopenClosedTab();
  } else if (matches("view.fullScreen", event) && title) {
    event.preventDefault();
    actions.toggleFullScreen();
  } else if (matches("find.matchCase", event) && title) {
    event.preventDefault();
    actions.toggleSearchOption("matchCase");
  } else if (matches("find.wholeWord", event) && title) {
    event.preventDefault();
    actions.toggleSearchOption("wholeWord");
  } else if (matches("find.regex", event) && title) {
    event.preventDefault();
    actions.toggleSearchOption("regex");
  } else if (matches("find.inSelection", event) && title) {
    event.preventDefault();
    actions.toggleSearchScope();
  } else if (matches("edit.rotatePageClockwise", event) && title) {
    event.preventDefault();
    actions.rotatePage(1);
  } else if (matches("edit.rotatePageCounterClockwise", event) && title) {
    event.preventDefault();
    actions.rotatePage(-1);
  } else if (matches("edit.duplicate", event) && title && actions.pickedMarks() >= 1) {
    // Only with something picked: with nothing to copy the chord is left alone.
    event.preventDefault();
    actions.duplicatePicked();
  } else if (matches("file.save", event) && title) {
    // Guarded on there being something to save, exactly as ⌘Z is guarded on
    // there being something to undo. ⌘S is the chord readers press by reflex on
    // a document they have not touched, and the backend answers that with a
    // refusal --- correctly, since it is what a stale frontend deserves --- but
    // a message about nothing having happened is not what a reflex wants back.
    event.preventDefault();
    if (actions.isDirty()) actions.saveDocument();
  } else if (matches("file.saveCopy", event) && title) {
    event.preventDefault();
    actions.saveCopy();
  } else if (
    matches("edit.selectAll", event) &&
    title &&
    !inTextField(event) &&
    !event.defaultPrevented
  ) {
    // ⌘A and ⌘C belong to the surface while the surface has the keyboard: its
    // own handler matches both and prevents the default, which is what
    // `defaultPrevented` reads here. This is the first arm in this function that
    // needs such a guard --- every other chord above is one the viewer does not
    // claim, so the two lists have never overlapped before.
    //
    // What the viewer's handler cannot see is a reader whose focus is in the
    // chrome. Click the document's name in the toolbar and the event never
    // reaches the viewer's root at all, so before this arm ⌘A fell through to
    // the web view, whose select-all takes the *toolbar* --- the Open button,
    // every find toggle, and the find field's contents. Reported from use on
    // Windows, where there is no menu bar to claim the chord first.
    //
    // Guarded on the field for the reason `NO_ACCELERATOR` in `menubar.ts`
    // gives: inside the find bar ⌘A means *this field*, and taking it there
    // would stop a reader replacing a query they had half typed.
    event.preventDefault();
    actions.viewer()?.selectPage();
  } else if (matches("edit.pastePicture", event)) {
    // In a text field too, and with no document: on macOS nothing else pastes,
    // not even text into the find field or the palette, because the menu has
    // no Paste item for the web view to answer. What the paste does is the web
    // view's: text in a field, a `paste` event anywhere else.
    if (actions.pasteKey()) event.preventDefault();
  } else if (
    matches("edit.copy", event) &&
    title &&
    !inTextField(event) &&
    !event.defaultPrevented
  ) {
    // The same defect as ⌘A above and the same three guards. Left out of a fix
    // for select-all alone it would be the quieter half: ⌘A on the chrome did
    // something visibly wrong, ⌘C there did nothing at all, which reads as a
    // copy that silently failed.
    event.preventDefault();
    void actions.viewer()?.copySelection();
  } else if (title && !inTextField(event) && !event.defaultPrevented && matches("edit.highlightSelection", event)) {
    // The bare letters for the frequent edits; see `BINDINGS`. Guarded on the
    // field for every one of them, where ⌘Z is the only chord above that needs
    // it: a letter is what a reader types into the find bar. With nothing
    // selected `h` is left alone, as ⌘C is.
    //
    // **A held key acts once.** The selection stays after a highlight, so
    // before 2026-10-15 holding `h` past the system's repeat delay laid one
    // highlight per repeat over the same words, each its own undo step. The
    // three tools below are refused on a repeat for the same reason: a tool
    // armed again by a held key is a press the reader did not make.
    if (actions.hasSelection()) {
      event.preventDefault();
      if (!event.repeat) actions.markSelection("highlight");
    }
  } else if (title && !inTextField(event) && !event.defaultPrevented && matches("edit.addComment", event)) {
    event.preventDefault();
    if (!event.repeat) actions.addComment(null);
  } else if (title && !inTextField(event) && !event.defaultPrevented && matches("edit.addTextBox", event)) {
    event.preventDefault();
    if (!event.repeat) actions.drawTextBox();
  } else if (title && !inTextField(event) && !event.defaultPrevented && matches("edit.editText", event)) {
    event.preventDefault();
    if (!event.repeat) actions.editText();
  } else if (matches("edit.undo", event) && title && !inTextField(event)) {
    // The `inTextField` guard is on these two and on nothing else here, and the
    // asymmetry is deliberate rather than an oversight. Every other binding
    // above is a chord no text field claims, so taking it from the find bar is
    // what a reader wants. Cmd-Z is the exception: it is *the* text-undo chord
    // on both platforms, and stealing it would mean a reader correcting a typo
    // in the find field silently undid a page rotation instead.
    event.preventDefault();
    if (actions.canUndo()) actions.undoEdit();
  } else if (matches("edit.redo", event) && title && !inTextField(event)) {
    event.preventDefault();
    if (actions.canRedo()) actions.redoEdit();
  }
}
