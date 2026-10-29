/**
 * The print preview: every sheet of paper the job will print, drawn, and on
 * Windows the print dialog itself.
 *
 * Windows' classic print dialog and GTK's have no preview of their own
 * (`print_win.rs`, *classic_dialog*), so without this the first picture of the
 * job a reader saw was on paper. On Windows the printer, the paper, the scale
 * and the pages to a sheet are chosen here too, beside the sheets they change:
 * the backend lays the job out (`print_layout.rs`) and prints exactly that, so
 * the preview is not an approximation of a dialog that comes after it. Linux
 * keeps GTK's dialog, which offers those choices itself, and shows each page
 * of the job on a sheet of its own size.
 *
 * The pages are pictures of the job's own bytes, drawn by a worker one at a
 * time as their sheets come into view, and kept while the dialog is open, so
 * a new layout puts them back without drawing them again. What they mean and
 * in what order they are asked for is `printpreview.ts`'s.
 *
 * Every string goes in through `textContent`, and a picture is set only when
 * it is a PNG `data:` URL (`isPicture`).
 */

import { isPicture } from "./compressdialog";
import { catalog } from "./i18n";
import type { PageSize, PrintPlacement, PrintScale, PrintSettings, PrintSetup, TwoSided } from "./ipc";
import {
  MOST_COPIES,
  MOST_PERCENT,
  PER_SHEET,
  SHEET_GAP,
  choosePages,
  defaultSettings,
  fitSheet,
  nextPicture,
  pageSheets,
  pictureWidth,
  placementBox,
  previewSummary,
  printableBox,
  readKept,
  sheetSummary,
  suggestLandscape,
  turnedPicture,
  visibleSheets,
  withKept,
  writeKept,
  type Sheet,
} from "./printpreview";
import { spokenError } from "./said";

/** Class on the backdrop, so the check harness can find it. */
export const DIALOG_CLASS = "tpdf-print-preview";

/**
 * Draws one page of the job, `width` pixels wide, as a PNG `data:` URL, with
 * its comments and other marks only when `marks` is set.
 */
export type Drawer = (index: number, width: number, marks: boolean) => Promise<string>;

/** Windows: asks the backend to set the printer up, with or without the driver's dialog. */
export interface Chooser {
  setup(settings: PrintSettings): Promise<PrintSetup>;
  driver(settings: PrintSettings): Promise<PrintSetup>;
}

/** What the reader decided: print with these settings (`null` without a chooser), or not. */
export type Answer = { print: true; settings: PrintSettings | null } | { print: false };

/** A page's picture as drawn so far. */
interface Picture {
  width: number;
  url: string;
}

const TEXT = catalog({
  "en-US": {
    scaleShrink: "Shrink large pages to fit",
    scaleFit: "Fit to printable area",
    scaleActual: "Actual size",
    scaleCustom: "Custom",
    scalePoster: "Poster, across several sheets",
    dialog: "Print preview",
    sheetsToPrint: "Sheets to print",
    printer: "Printer",
    pages: "Pages",
    whichPages: "Which pages",
    pagesExample: "For example 1-3, 5",
    ofThose: "Of those, print",
    everyPage: "Every page",
    oddPages: "Odd pages only",
    evenPages: "Even pages only",
    lastFirst: "Last page first",
    copies: "Copies",
    paper: "Paper",
    orientation: "Orientation",
    portrait: "Portrait",
    landscape: "Landscape",
    perSheet: "Pages per sheet",
    scale: "Scale",
    scalePercent: "Scale in percent",
    scaleNote: "Each page is drawn as large as its part of the sheet allows.",
    turnPages: "Turn pages to fit the sheet",
    marks: "Comments and marks",
    printMarks: "Print them",
    leaveMarks: "Leave them out",
    twoSided: "Two-sided",
    off: "Off",
    longEdge: "Flip on long edge",
    shortEdge: "Flip on short edge",
    colour: "Color",
    inColour: "Color",
    blackWhite: "Black and white",
    printerSettings: "Printer settings...",
    cancel: "Cancel",
    print: "Print",
    printNext: "Print...",
    heading: (name: string) => `Print preview: ${name}`,
    nextStep: "You choose the printer, the paper and the pages to print in the next step.",
    copiesRange: (most: number) => `Copies is a whole number from 1 to ${most}.`,
    percentRange: (most: number) => `The scale is a whole number of percent from 1 to ${most}.`,
    all: "All",
    currentPage: (page: number) => `Current page (${page})`,
    custom: "Custom",
    sheet: (sheet: number, of: number) => `Sheet ${sheet} of ${of}`,
    page: (page: number) => `Page ${page}`,
    drawing: (page: number) => `Drawing page ${page}...`,
    notDrawn: (page: number) => `Page ${page} could not be drawn.`,
    notDrawnBecause: (page: number, reason: string) => `Page ${page} could not be drawn: ${reason}`,
  },
  "en-GB": {
    colour: "Colour",
    inColour: "Colour",
  },
  "de-DE": {
    scaleShrink: "Große Seiten verkleinern",
    scaleFit: "An druckbaren Bereich anpassen",
    scaleActual: "Originalgröße",
    scaleCustom: "Benutzerdefiniert",
    scalePoster: "Poster, über mehrere Blätter",
    dialog: "Druckvorschau",
    sheetsToPrint: "Zu druckende Blätter",
    printer: "Drucker",
    pages: "Seiten",
    whichPages: "Welche Seiten",
    pagesExample: "Zum Beispiel 1-3, 5",
    ofThose: "Davon drucken",
    everyPage: "Alle Seiten",
    oddPages: "Nur ungerade Seiten",
    evenPages: "Nur gerade Seiten",
    lastFirst: "Letzte Seite zuerst",
    copies: "Exemplare",
    paper: "Papier",
    orientation: "Ausrichtung",
    portrait: "Hochformat",
    landscape: "Querformat",
    perSheet: "Seiten pro Blatt",
    scale: "Skalierung",
    scalePercent: "Skalierung in Prozent",
    scaleNote: "Jede Seite wird so groß gezeichnet, wie ihr Teil des Blatts zulässt.",
    turnPages: "Seiten drehen, damit sie auf das Blatt passen",
    marks: "Kommentare und Markierungen",
    printMarks: "Mitdrucken",
    leaveMarks: "Weglassen",
    twoSided: "Beidseitig",
    off: "Aus",
    longEdge: "An der langen Kante wenden",
    shortEdge: "An der kurzen Kante wenden",
    colour: "Farbe",
    inColour: "Farbe",
    blackWhite: "Schwarzweiß",
    printerSettings: "Druckereinstellungen...",
    cancel: "Abbrechen",
    print: "Drucken",
    printNext: "Drucken...",
    heading: (name: string) => `Druckvorschau: ${name}`,
    nextStep: "Drucker, Papier und die zu druckenden Seiten wählst du im nächsten Schritt.",
    copiesRange: (most: number) => `Die Anzahl der Exemplare ist eine ganze Zahl von 1 bis ${most}.`,
    percentRange: (most: number) => `Die Skalierung ist eine ganze Prozentzahl von 1 bis ${most}.`,
    all: "Alle",
    currentPage: (page: number) => `Aktuelle Seite (${page})`,
    custom: "Benutzerdefiniert",
    sheet: (sheet: number, of: number) => `Blatt ${sheet} von ${of}`,
    page: (page: number) => `Seite ${page}`,
    drawing: (page: number) => `Seite ${page} wird gezeichnet...`,
    notDrawn: (page: number) => `Seite ${page} konnte nicht gezeichnet werden.`,
    notDrawnBecause: (page: number, reason: string) => `Seite ${page} konnte nicht gezeichnet werden: ${reason}`,
  },
  "de-DE-formal": {
    nextStep: "Drucker, Papier und die zu druckenden Seiten wählen Sie im nächsten Schritt.",
  },
});

/** The scale choices, in the order the list offers them. */
const SCALES: { value: PrintScale["kind"]; label: string }[] = [
  { value: "shrink", label: TEXT().scaleShrink },
  { value: "fit", label: TEXT().scaleFit },
  { value: "actual", label: TEXT().scaleActual },
  { value: "percent", label: TEXT().scaleCustom },
  { value: "poster", label: TEXT().scalePoster },
];

/** A modal showing the job's sheets, with Print and Cancel. Built once and reused. */
export class PrintPreviewDialog {
  private readonly backdrop: HTMLElement;
  private readonly heading: HTMLElement;
  private readonly summary: HTMLElement;
  private readonly list: HTMLElement;
  private readonly under: HTMLElement;
  private readonly side: HTMLElement;
  private readonly problem: HTMLElement;
  private readonly print: HTMLButtonElement;
  private readonly printer: HTMLSelectElement;
  private readonly range: HTMLSelectElement;
  private readonly pagesField: HTMLInputElement;
  private readonly subset: HTMLSelectElement;
  private readonly reverse: HTMLInputElement;
  private readonly copies: HTMLInputElement;
  private readonly rotate: HTMLInputElement;
  private readonly marksChoice: HTMLSelectElement;
  private readonly paper: HTMLSelectElement;
  private readonly orientation: HTMLSelectElement;
  private readonly perSheet: HTMLSelectElement;
  private readonly scale: HTMLSelectElement;
  private readonly percent: HTMLInputElement;
  private readonly scaleNote: HTMLElement;
  private readonly twoSided: HTMLSelectElement;
  private readonly twoSidedRow: HTMLElement;
  private readonly colour: HTMLSelectElement;
  private readonly colourRow: HTMLElement;
  private readonly driverButton: HTMLButtonElement;

  private pages: readonly PageSize[] = [];
  private sheets: Sheet[] = [];
  private boxes: { box: HTMLElement; note: HTMLElement; pictures: { page: number; picture: HTMLImageElement }[] }[] =
    [];
  private heights: number[] = [];
  private drawn = new Map<number, Picture>();
  private drawing: number | null = null;
  private draw: Drawer | null = null;
  private chooser: Chooser | null = null;
  private settings: PrintSettings | null = null;
  /** The page the reader is on, zero based, or -1 when there is none. */
  private current = -1;
  /** Whether the pictures drawn are with the marks; a change redraws them. */
  private marks = true;
  /** The settings waiting for the backend while an earlier request is out. */
  private queued: { settings: PrintSettings; driver: boolean } | null = null;
  private asking = false;
  private broken = false;
  private shown = false;
  /** Counts openings, so an answer for an earlier one is dropped. */
  private run = 0;
  private pending: ((answer: Answer) => void) | null = null;
  private returnFocus: HTMLElement | null = null;

  constructor(host: HTMLElement) {
    this.backdrop = document.createElement("div");
    this.backdrop.className = DIALOG_CLASS;
    this.backdrop.style.cssText =
      "position:fixed;inset:0;display:none;z-index:70;" +
      "background:rgba(0,0,0,0.28);align-items:center;justify-content:center;";

    const panel = document.createElement("div");
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-modal", "true");
    panel.setAttribute("aria-label", TEXT().dialog);
    panel.style.cssText =
      "display:flex;flex-direction:column;width:min(1040px,96vw);height:min(900px,92vh);" +
      "border-radius:10px;background:Canvas;color:CanvasText;" +
      "box-shadow:0 12px 48px rgba(0,0,0,0.35);" +
      "font:13px/1.55 system-ui,-apple-system,sans-serif;padding:1rem;box-sizing:border-box;";

    this.heading = document.createElement("h2");
    this.heading.style.cssText = "margin:0;font-size:15px;font-weight:600;";
    this.summary = document.createElement("p");
    this.summary.style.cssText = "margin:0.15rem 0 0.6rem;opacity:0.8;";

    const body = document.createElement("div");
    body.style.cssText = "flex:1;min-height:0;display:flex;gap:1rem;";

    this.list = document.createElement("div");
    this.list.setAttribute("role", "list");
    this.list.setAttribute("aria-label", TEXT().sheetsToPrint);
    this.list.tabIndex = 0;
    this.list.style.cssText =
      "flex:1;min-width:0;min-height:0;overflow:auto;padding:12px 0;" +
      `display:flex;flex-direction:column;align-items:center;gap:${SHEET_GAP}px;` +
      "background:color-mix(in srgb, CanvasText 10%, Canvas);border-radius:6px;";

    this.side = document.createElement("div");
    this.side.style.cssText =
      "flex:none;width:250px;overflow:auto;display:none;flex-direction:column;gap:0.55rem;";
    this.printer = this.select(this.side, TEXT().printer);
    this.range = this.select(this.side, TEXT().pages);
    this.pagesField = this.input(this.side, TEXT().whichPages, "text");
    this.pagesField.placeholder = TEXT().pagesExample;
    this.subset = this.select(this.side, TEXT().ofThose);
    this.options(this.subset, [
      ["all", TEXT().everyPage],
      ["odd", TEXT().oddPages],
      ["even", TEXT().evenPages],
    ]);
    this.reverse = this.checkbox(this.side, TEXT().lastFirst);
    this.copies = this.input(this.side, TEXT().copies, "number");
    this.copies.min = "1";
    this.copies.max = String(MOST_COPIES);
    this.paper = this.select(this.side, TEXT().paper);
    this.orientation = this.select(this.side, TEXT().orientation);
    this.options(this.orientation, [
      ["portrait", TEXT().portrait],
      ["landscape", TEXT().landscape],
    ]);
    this.perSheet = this.select(this.side, TEXT().perSheet);
    this.options(
      this.perSheet,
      PER_SHEET.map((count) => [String(count), String(count)]),
    );
    this.scale = this.select(this.side, TEXT().scale);
    this.options(
      this.scale,
      SCALES.map((scale) => [scale.value, scale.label]),
    );
    this.percent = this.input(this.side, TEXT().scalePercent, "number");
    this.percent.min = "1";
    this.percent.max = String(MOST_PERCENT);
    this.scaleNote = document.createElement("p");
    this.scaleNote.style.cssText = "margin:-0.3rem 0 0;opacity:0.72;";
    this.scaleNote.textContent = TEXT().scaleNote;
    this.side.append(this.scaleNote);
    this.rotate = this.checkbox(this.side, TEXT().turnPages);
    this.marksChoice = this.select(this.side, TEXT().marks);
    this.options(this.marksChoice, [
      ["print", TEXT().printMarks],
      ["leave", TEXT().leaveMarks],
    ]);
    this.twoSided = this.select(this.side, TEXT().twoSided);
    this.options(this.twoSided, [
      ["off", TEXT().off],
      ["long", TEXT().longEdge],
      ["short", TEXT().shortEdge],
    ]);
    this.twoSidedRow = this.twoSided.parentElement as HTMLElement;
    this.colour = this.select(this.side, TEXT().colour);
    this.options(this.colour, [
      ["colour", TEXT().inColour],
      ["black", TEXT().blackWhite],
    ]);
    this.colourRow = this.colour.parentElement as HTMLElement;
    this.driverButton = this.button(TEXT().printerSettings, () => this.change(true));
    this.driverButton.style.alignSelf = "flex-start";
    this.side.append(this.driverButton);
    this.problem = document.createElement("p");
    this.problem.setAttribute("role", "alert");
    this.problem.style.cssText = "margin:0;color:#b42318;";
    this.side.append(this.problem);

    body.append(this.list, this.side);

    this.under = document.createElement("p");
    this.under.style.cssText = "margin:0.6rem 0 0;opacity:0.72;";

    const buttons = document.createElement("div");
    buttons.style.cssText = "display:flex;gap:0.5rem;justify-content:flex-end;margin-top:0.6rem;";
    const cancel = this.button(TEXT().cancel, () => this.settle(false));
    this.print = this.button(TEXT().print, () => this.settle(true));
    this.print.style.fontWeight = "600";
    buttons.append(cancel, this.print);

    panel.append(this.heading, this.summary, body, this.under, buttons);
    this.backdrop.append(panel);
    host.append(this.backdrop);

    this.list.addEventListener("scroll", () => this.pump());
    for (const control of [
      this.printer,
      this.range,
      this.subset,
      this.reverse,
      this.paper,
      this.orientation,
      this.perSheet,
      this.scale,
      this.rotate,
      this.marksChoice,
      this.twoSided,
      this.colour,
      this.pagesField,
      this.copies,
      this.percent,
    ]) {
      control.addEventListener("change", () => this.change(false, control));
    }
    this.backdrop.addEventListener("click", (event) => {
      if (event.target === this.backdrop) this.settle(false);
    });
    this.backdrop.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.settle(false);
        return;
      }
      // Stopped as well as defaulted away: the window's key handler binds Enter.
      // A focused button answers Enter itself, so Cancel must not print, and
      // in a field it commits what was typed.
      const target = event.target;
      const typing =
        target instanceof HTMLButtonElement || target instanceof HTMLInputElement || target instanceof HTMLSelectElement;
      if (event.key === "Enter" && !typing) {
        event.preventDefault();
        event.stopPropagation();
        this.settle(true);
      }
    });
  }

  /** Whether it is on screen. */
  get isOpen(): boolean {
    return this.shown;
  }

  /**
   * Shows the job named `name`, whose pages measure `pages`, drawing each
   * page with `draw` as its sheet comes into view. With a `chooser` (Windows)
   * the printer and its settings are chosen here and the sheets are the
   * layout on its paper; without, each page is a sheet of its own. `current`
   * is the page the reader is on, zero based, which *Current page* prints.
   */
  ask(
    name: string,
    pages: readonly PageSize[],
    draw: Drawer,
    chooser: Chooser | null = null,
    current = -1,
  ): Promise<Answer> {
    this.settle(false);
    this.run += 1;
    this.pages = pages;
    this.current = current >= 0 && current < pages.length ? current : -1;
    this.marks = true;
    this.draw = draw;
    this.chooser = chooser;
    this.drawn = new Map();
    this.drawing = null;
    this.queued = null;
    this.asking = false;
    this.broken = false;
    this.heading.textContent = TEXT().heading(name);
    this.summary.textContent = previewSummary(pages);
    this.side.style.display = chooser ? "flex" : "none";
    this.under.textContent = chooser ? "" : TEXT().nextStep;
    this.under.style.display = chooser ? "none" : "block";
    this.print.textContent = chooser ? TEXT().print : TEXT().printNext;
    this.problem.textContent = "";
    if (!this.shown) {
      const active = document.activeElement as { focus?: () => void } | null;
      this.returnFocus = typeof active?.focus === "function" ? (active as HTMLElement) : null;
    }
    this.shown = true;
    this.backdrop.style.display = "flex";
    if (chooser) {
      this.settings = withKept(defaultSettings(pages), readKept(), pages[0]);
      this.marks = this.settings.marks;
      this.fill(this.settings);
      // Nothing to show until a printer has given its paper.
      this.lay([]);
      this.list.setAttribute("aria-busy", "true");
      this.request(this.settings, false);
    } else {
      this.settings = null;
      this.lay(pageSheets(pages));
    }
    this.update();
    this.print.focus();
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  /** Closes with no answer, settling anything outstanding. */
  close(): void {
    this.settle(false);
  }

  /** The controls' state, as settings; a refusal when what was typed cannot be. */
  private read(): PrintSettings | string {
    const base = this.settings ?? defaultSettings(this.pages);
    const pages = choosePages(
      {
        range: this.range.value as "all" | "current" | "custom",
        text: this.pagesField.value,
        current: this.current,
        subset: this.subset.value as "all" | "odd" | "even",
        reverse: this.reverse.checked,
      },
      this.pages.length,
    );
    if (typeof pages === "string") return pages;
    const copies = Number(this.copies.value);
    if (!Number.isInteger(copies) || copies < 1 || copies > MOST_COPIES) {
      return TEXT().copiesRange(MOST_COPIES);
    }
    const kind = this.scale.value as PrintScale["kind"];
    let scale: PrintScale;
    if (kind === "percent" || kind === "poster") {
      const percent = Number(this.percent.value);
      if (!Number.isInteger(percent) || percent < 1 || percent > MOST_PERCENT) {
        return TEXT().percentRange(MOST_PERCENT);
      }
      scale = { kind, percent };
    } else {
      scale = { kind };
    }
    return {
      printer: this.printer.value || base.printer,
      paper: this.paper.value === "" ? null : Number(this.paper.value),
      landscape: this.orientation.value === "landscape",
      copies,
      twoSided: this.twoSidedRow.style.display === "none" ? null : (this.twoSided.value as TwoSided),
      colour: this.colourRow.style.display === "none" ? null : this.colour.value === "colour",
      // A poster is one page to a sheet; the list is set back to show it.
      perSheet: kind === "poster" ? 1 : Number(this.perSheet.value),
      scale,
      rotate: this.rotate.checked,
      marks: this.marksChoice.value === "print",
      pages,
    };
  }

  /** A control changed, or Printer settings... was pressed. */
  private change(driver: boolean, control?: HTMLElement): void {
    if (!this.shown || !this.chooser) return;
    const read = this.read();
    if (typeof read === "string") {
      this.problem.textContent = read;
      this.broken = true;
      this.update();
      return;
    }
    let settings = read;
    if (control === this.printer) {
      // The new printer's own paper, sides and colour, not the last one's.
      settings = { ...settings, paper: null, twoSided: null, colour: null };
    }
    if (control === this.perSheet) {
      settings = { ...settings, landscape: suggestLandscape(this.pages[0], settings.perSheet) };
      this.orientation.value = settings.landscape ? "landscape" : "portrait";
    }
    if (control === this.scale && settings.scale.kind === "poster") this.perSheet.value = "1";
    if (control === this.range && this.range.value === "custom") this.pagesField.focus();
    if (settings.marks !== this.marks) {
      // Every picture so far is of the other choice, so none is kept.
      this.marks = settings.marks;
      this.drawn = new Map();
    }
    this.problem.textContent = "";
    this.broken = false;
    this.settings = settings;
    this.request(settings, driver);
  }

  /** Sends settings to the backend, one request at a time; the latest waits. */
  private request(settings: PrintSettings, driver: boolean): void {
    const chooser = this.chooser;
    if (!chooser) return;
    if (this.asking) {
      this.queued = { settings, driver: driver || (this.queued?.driver ?? false) };
      return;
    }
    this.asking = true;
    this.update();
    const run = this.run;
    const asked = driver ? chooser.driver(settings) : chooser.setup(settings);
    void asked
      .then((setup) => {
        if (run !== this.run || !this.shown) return;
        // A newer change is waiting: this answer is already out of date.
        if (this.queued) return;
        this.apply(setup);
      })
      .catch((error: unknown) => {
        if (run !== this.run || !this.shown) return;
        this.problem.textContent = String(error);
        this.broken = true;
      })
      .finally(() => {
        if (run !== this.run) return;
        this.asking = false;
        const next = this.queued;
        this.queued = null;
        if (next && this.shown) this.request(next.settings, next.driver);
        else this.update();
      });
  }

  /** Shows what the printer answered: its choices in the controls, its paper in the sheets. */
  private apply(setup: PrintSetup): void {
    const device = setup.device;
    const base = this.settings ?? defaultSettings(this.pages);
    this.settings = {
      ...base,
      printer: device.printer,
      paper: device.paper,
      landscape: device.landscape,
      twoSided: device.twoSided,
      colour: device.colour,
    };
    this.options(
      this.printer,
      setup.printers.map((name) => [name, name]),
    );
    this.printer.value = device.printer;
    this.options(
      this.paper,
      device.papers.map((paper) => [String(paper.id), paper.name]),
    );
    this.paper.value = String(device.paper);
    this.orientation.value = device.landscape ? "landscape" : "portrait";
    this.twoSidedRow.style.display = device.twoSided === null ? "none" : "flex";
    if (device.twoSided !== null) this.twoSided.value = device.twoSided;
    this.colourRow.style.display = device.colour === null ? "none" : "flex";
    if (device.colour !== null) this.colour.value = device.colour ? "colour" : "black";
    this.problem.textContent = "";
    this.broken = false;
    this.list.removeAttribute("aria-busy");
    const pages = setup.sheets.reduce((sum, sheet) => sum + sheet.length, 0);
    this.summary.textContent = sheetSummary(setup.sheets.length, pages, this.settings.copies);
    this.lay(setup.sheets.map((placements) => ({ paper: setup.paper, placements })));
  }

  /** Puts the settings into the controls before the first answer. */
  private fill(settings: PrintSettings): void {
    // The kept printer is offered before the list arrives, so the first
    // request names it and the list does not lose it.
    this.options(this.printer, settings.printer ? [[settings.printer, settings.printer]] : []);
    this.options(this.paper, []);
    this.options(this.range, [
      ["all", TEXT().all],
      ...(this.current >= 0 ? [["current", TEXT().currentPage(this.current + 1)] as const] : []),
      ["custom", TEXT().custom],
    ]);
    this.range.value = "all";
    this.pagesField.value = "";
    this.subset.value = "all";
    this.reverse.checked = false;
    this.copies.value = String(settings.copies);
    this.orientation.value = settings.landscape ? "landscape" : "portrait";
    this.perSheet.value = String(settings.perSheet);
    this.scale.value = settings.scale.kind;
    this.percent.value =
      settings.scale.kind === "percent" || settings.scale.kind === "poster" ? String(settings.scale.percent) : "100";
    this.rotate.checked = settings.rotate;
    this.marksChoice.value = settings.marks ? "print" : "leave";
    this.twoSidedRow.style.display = "none";
    this.colourRow.style.display = "none";
  }

  /** Enables what applies now. */
  private update(): void {
    const poster = this.scale.value === "poster";
    const percent = this.scale.value === "percent" || poster;
    const shared = this.perSheet.value !== "1";
    this.scale.disabled = shared;
    this.perSheet.disabled = poster;
    // A poster's parts are never turned: the page is as large as it is.
    this.rotate.disabled = poster && !shared;
    (this.percent.parentElement as HTMLElement).style.display = percent && !shared ? "flex" : "none";
    (this.pagesField.parentElement as HTMLElement).style.display = this.range.value === "custom" ? "flex" : "none";
    this.scaleNote.style.display = shared ? "block" : "none";
    this.driverButton.disabled = this.asking;
    this.print.disabled =
      this.pages.length === 0 || (this.chooser !== null && (this.broken || this.asking || this.sheets.length === 0));
  }

  /** One box per sheet, sized before anything is drawn, so the list can scroll. */
  private lay(sheets: Sheet[]): void {
    const scrolled = this.list.scrollTop;
    this.list.replaceChildren();
    this.sheets = sheets;
    this.boxes = [];
    this.heights = [];
    this.drawing = null;
    const maxWidth = Math.max(120, this.list.clientWidth - 48);
    // Tall enough that a portrait sheet reads, short enough that the next one shows.
    const maxHeight = Math.max(160, Math.round(this.list.clientHeight * 0.92));
    sheets.forEach((sheet, index) => {
      const { width, height } = fitSheet(
        { width_pt: sheet.paper.width, height_pt: sheet.paper.height },
        maxWidth,
        maxHeight,
      );
      const box = document.createElement("div");
      box.setAttribute("role", "listitem");
      box.setAttribute("aria-label", TEXT().sheet(index + 1, sheets.length));
      box.style.cssText =
        `position:relative;flex:none;width:${width}px;height:${height}px;background:#fff;` +
        "box-shadow:0 1px 6px rgba(0,0,0,0.3);";
      const area = document.createElement("div");
      const printable = printableBox(sheet.paper);
      area.style.cssText =
        `position:absolute;overflow:hidden;left:${printable.left}%;top:${printable.top}%;` +
        `width:${printable.width}%;height:${printable.height}%;`;
      const pictures = sheet.placements.map((placed) => {
        const at = placementBox(placed, sheet.paper);
        const holder = document.createElement("div");
        holder.style.cssText =
          `position:absolute;left:${at.left}%;top:${at.top}%;` + `width:${at.width}%;height:${at.height}%;`;
        // webview-sink-ok: its `src` is only ever a PNG `data:` URL; see `pump`.
        const picture = document.createElement("img");
        picture.setAttribute("alt", TEXT().page(placed.page + 1));
        // The picture is of the page upright; a turned page is turned here,
        // a quarter anticlockwise about its middle, as the printer turns it.
        const turned = turnedPicture(placed);
        const inside = turned ?? { left: 0, top: 0, width: 100, height: 100 };
        // Hidden until it has a picture: an `img` with no `src` draws the
        // browser's broken-image icon in its corner.
        picture.style.cssText =
          `position:absolute;display:none;left:${inside.left}%;top:${inside.top}%;` +
          `width:${inside.width}%;height:${inside.height}%;` +
          (turned ? "transform:rotate(-90deg);" : "");
        holder.append(picture);
        area.append(holder);
        const known = this.drawn.get(placed.page);
        if (known) this.show(picture, known.url);
        return { page: placed.page, picture };
      });
      const note = document.createElement("span");
      note.style.cssText =
        "position:absolute;inset:0;display:flex;align-items:center;justify-content:center;" +
        "padding:1rem;text-align:center;color:#555;pointer-events:none;";
      box.append(area, note);
      this.list.append(box);
      this.boxes.push({ box, note, pictures });
      this.heights.push(height);
    });
    this.list.scrollTop = scrolled;
    this.update();
    this.pump();
  }

  /** How wide a placed page's picture should be to be sharp where it is shown, in pixels. */
  private wanted(sheet: number, placed: PrintPlacement): number {
    const box = this.boxes[sheet]?.box;
    const paper = this.sheets[sheet]?.paper;
    if (!box || !paper) return 1;
    // The picture is upright, so a turned page's width is its height on the sheet.
    const across = placed.turned ? placed.height : placed.width;
    return pictureWidth((across / paper.width) * box.offsetWidth, window.devicePixelRatio);
  }

  /** Asks for the next page near the view whose picture is missing or too small, one at a time. */
  private pump(): void {
    const draw = this.draw;
    if (!this.shown || this.drawing !== null || !draw) return;
    const visible = visibleSheets(this.heights, Math.max(0, this.list.scrollTop - 12), this.list.clientHeight);
    const widths = new Map([...this.drawn].map(([page, picture]) => [page, picture.width]));
    const next = nextPicture(this.sheets, visible, (sheet, placed) => this.wanted(sheet, placed), widths);
    if (!next) return;
    const { sheet, page, width } = next;
    const run = this.run;
    const marks = this.marks;
    this.drawing = page;
    const note = this.boxes[sheet]?.note;
    if (note && !this.drawn.has(page)) note.textContent = TEXT().drawing(page + 1);
    void draw(page, width, marks)
      .then((url) => {
        // Drawn for the other choice of marks: the next pump draws it again.
        if (run !== this.run || !this.shown || marks !== this.marks) return;
        if (isPicture(url)) {
          this.drawn.set(page, { width, url });
          for (const box of this.boxes) {
            for (const shown of box.pictures) if (shown.page === page) this.show(shown.picture, url);
          }
          for (const box of this.boxes) if (box.pictures.some((p) => p.page === page)) box.note.textContent = "";
        } else if (note) {
          note.textContent = TEXT().notDrawn(page + 1);
        }
      })
      .catch((error: unknown) => {
        if (run !== this.run || !this.shown || marks !== this.marks) return;
        // Recorded as drawn, at no width, so it is not asked for again and again.
        this.drawn.set(page, { width: Number.MAX_SAFE_INTEGER, url: "" });
        if (note) note.textContent = TEXT().notDrawnBecause(page + 1, spokenError(error));
      })
      .finally(() => {
        if (run !== this.run) return;
        this.drawing = null;
        this.pump();
      });
  }

  private show(picture: HTMLImageElement, url: string): void {
    if (!isPicture(url)) return;
    // webview-sink-ok: a PNG `data:` URL the backend encoded from the job it drew, checked by `isPicture`.
    picture.setAttribute("src", url);
    picture.style.display = "block";
  }

  /** Resolves once and hides the dialog. */
  private settle(print: boolean): void {
    const pending = this.pending;
    this.pending = null;
    const settings = this.settings;
    // Kept for the next print only once this one is printed with them.
    if (print && settings && this.chooser) writeKept(settings);
    if (this.shown) {
      this.shown = false;
      this.backdrop.style.display = "none";
      // The pictures are of the reader's document: not kept once it is closed.
      this.list.replaceChildren();
      this.sheets = [];
      this.boxes = [];
      this.heights = [];
      this.drawn = new Map();
      this.draw = null;
      this.chooser = null;
      this.returnFocus?.focus();
      this.returnFocus = null;
    }
    pending?.(print ? { print: true, settings } : { print: false });
  }

  /** A labelled row in `parent` holding a control. */
  private row(parent: HTMLElement, label: string, control: HTMLElement): void {
    const row = document.createElement("label");
    row.style.cssText = "display:flex;flex-direction:column;gap:0.15rem;";
    const text = document.createElement("span");
    text.textContent = label;
    text.style.cssText = "font-weight:600;font-size:12px;";
    control.style.cssText =
      "font:inherit;padding:0.25rem 0.35rem;border-radius:5px;min-height:28px;box-sizing:border-box;" +
      "border:1px solid color-mix(in srgb, CanvasText 28%, transparent);background:Field;color:FieldText;";
    row.append(text, control);
    parent.append(row);
  }

  private select(parent: HTMLElement, label: string): HTMLSelectElement {
    const select = document.createElement("select");
    this.row(parent, label, select);
    return select;
  }

  private input(parent: HTMLElement, label: string, type: string): HTMLInputElement {
    const input = document.createElement("input");
    input.type = type;
    this.row(parent, label, input);
    return input;
  }

  /** A checkbox with its label beside it, in `parent`. */
  private checkbox(parent: HTMLElement, label: string): HTMLInputElement {
    const row = document.createElement("label");
    row.style.cssText = "display:flex;align-items:center;gap:0.4rem;";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.style.margin = "0";
    const text = document.createElement("span");
    text.textContent = label;
    row.append(box, text);
    parent.append(row);
    return box;
  }

  /** Replaces a list's options, each `[value, label]`. */
  private options(select: HTMLSelectElement, options: readonly (readonly [string, string])[]): void {
    const kept = select.value;
    select.replaceChildren(
      ...options.map(([value, label]) => {
        const option = document.createElement("option");
        option.value = value;
        option.textContent = label;
        return option;
      }),
    );
    if (options.some(([value]) => value === kept)) select.value = kept;
  }

  private button(label: string, onClick: () => void): HTMLButtonElement {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.style.cssText =
      "padding:0.35rem 0.9rem;font:inherit;border-radius:6px;" +
      "border:1px solid color-mix(in srgb, CanvasText 28%, transparent);" +
      "background:ButtonFace;color:ButtonText;cursor:pointer;";
    button.addEventListener("click", onClick);
    return button;
  }
}
