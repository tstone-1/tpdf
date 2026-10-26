/**
 * The files a document carries, as the window lists them and asks which one
 * to save (*Save an attachment*). The list is read in a worker with the
 * document's properties (`attachments::list`); the bytes are decoded in a
 * worker and written where the reader chose (`attachment_save`).
 *
 * **tpdf never opens an embedded file.** What it says about one is the kind
 * of file its name claims to be, which nothing checks: a name is the
 * document's to choose, and a program can be named `invoice.pdf`.
 */

import { formatBytes } from "./bytes";
import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    pdf: "a PDF document",
    word: "a Word document",
    excel: "an Excel workbook",
    powerPoint: "a PowerPoint presentation",
    openText: "an OpenDocument text",
    openSpreadsheet: "an OpenDocument spreadsheet",
    openPresentation: "an OpenDocument presentation",
    csv: "a table of comma-separated values",
    text: "a text file",
    xml: "an XML file",
    json: "a JSON file",
    webPage: "a web page",
    picture: "a picture",
    archive: "an archive",
    email: "an e-mail",
    noEnding: "a file with no ending to say what it is",
    program: "a program or script",
    otherEnding: (end: string) => `a .${end} file`,
    which: (count: number) => `Which attachment: 1 to ${count}, or part of its name`,
    noNumber: (number: number, count: number) => `There is no attachment ${number}; there are ${count}`,
    noName: (typed: string) => `No attachment's name has "${typed}" in it`,
    severalNames: (count: number, typed: string) => `${count} attachments have "${typed}" in their names; type a number`,
    saves: (described: string) => `Saves ${described}`,
    programWarning: " Its name says it is a program or script: open it only if you trust the document it came from.",
    saved: (name: string, where: string, size: string) => `Saved ${name} to ${where}, ${size}.`,
    row: (number: number) => `Attachment ${number}`,
  },
  "de-DE": {
    pdf: "ein PDF-Dokument",
    word: "ein Word-Dokument",
    excel: "eine Excel-Arbeitsmappe",
    powerPoint: "eine PowerPoint-Präsentation",
    openText: "ein OpenDocument-Text",
    openSpreadsheet: "eine OpenDocument-Tabelle",
    openPresentation: "eine OpenDocument-Präsentation",
    csv: "eine Tabelle mit kommagetrennten Werten",
    text: "eine Textdatei",
    xml: "eine XML-Datei",
    json: "eine JSON-Datei",
    webPage: "eine Webseite",
    picture: "ein Bild",
    archive: "ein Archiv",
    email: "eine E-Mail",
    noEnding: "eine Datei ohne Endung, die sagt, was sie ist",
    program: "ein Programm oder Skript",
    otherEnding: (end: string) => `eine .${end}-Datei`,
    which: (count: number) => `Welcher Anhang: 1 bis ${count} oder ein Teil seines Namens`,
    noNumber: (number: number, count: number) => `Es gibt keinen Anhang ${number}; es gibt ${count}`,
    noName: (typed: string) => `Kein Anhang hat „${typed}“ im Namen`,
    severalNames: (count: number, typed: string) =>
      `${count} Anhänge haben „${typed}“ im Namen; gib eine Nummer ein`,
    saves: (described: string) => `Speichert ${described}`,
    programWarning:
      " Laut Name ist die Datei ein Programm oder Skript: Öffne sie nur, wenn du dem Dokument vertraust, aus dem sie stammt.",
    saved: (name: string, where: string, size: string) => `${name} in ${where} gespeichert, ${size}.`,
    row: (number: number) => `Anhang ${number}`,
  },
});

type Texts = ReturnType<typeof TEXT>;
/** The messages that name a kind of file: the plain ones. */
type Kind = { [K in keyof Texts]: Texts[K] extends string ? K : never }[keyof Texts];

/** One embedded file: `attachments::Embedded`. */
export interface Embedded {
  name: string;
  description: string;
  /** The size the document states for it, if it states one. */
  size: number | null;
}

/** Kinds by the ending a name claims, lower case and without the dot. */
const KINDS: Record<string, Kind> = {
  pdf: "pdf",
  doc: "word", docx: "word",
  xls: "excel", xlsx: "excel", xlsm: "excel",
  ppt: "powerPoint", pptx: "powerPoint",
  odt: "openText", ods: "openSpreadsheet", odp: "openPresentation",
  csv: "csv", txt: "text", xml: "xml", json: "json",
  htm: "webPage", html: "webPage",
  png: "picture", jpg: "picture", jpeg: "picture", gif: "picture", tif: "picture",
  tiff: "picture", bmp: "picture", webp: "picture",
  zip: "archive", "7z": "archive", rar: "archive", gz: "archive", tar: "archive",
  eml: "email", msg: "email",
};

/** Endings a system runs when the file is opened: the kind a reader is warned about. */
const PROGRAMS = new Set([
  "exe", "msi", "bat", "cmd", "com", "scr", "ps1", "vbs", "js", "jar",
  "sh", "command", "app", "dmg", "pkg", "deb", "rpm", "appimage", "lnk",
]);

/** The ending a name claims, lower case, or "" for none. */
function ending(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 && dot < name.length - 1 ? name.slice(dot + 1).toLowerCase() : "";
}

/** Whether the name claims a kind of file the system runs when it is opened. */
export function isProgram(name: string): boolean {
  return PROGRAMS.has(ending(name));
}

/** The kind of file a name claims to be, as a reader says it. */
export function kindOf(name: string): string {
  const end = ending(name);
  if (end === "") return TEXT().noEnding;
  if (PROGRAMS.has(end)) return TEXT().program;
  const kind = Object.hasOwn(KINDS, end) ? KINDS[end] : undefined;
  return kind === undefined ? TEXT().otherEnding(end) : TEXT()[kind];
}

/** One file in a line: its name, the kind it claims, and its stated size. */
export function describe(file: Embedded): string {
  const size = file.size === null ? "" : `, ${formatBytes(file.size)}`;
  return `${file.name} (${kindOf(file.name)}${size})`;
}

/** What the palette offers while it asks which file. */
export function placeholder(files: readonly Embedded[]): string {
  return TEXT().which(files.length);
}

/**
 * Which file a typed answer names: a number from 1, or text that is part of
 * exactly one name. A problem when it names none or more than one.
 */
export function pick(raw: string, files: readonly Embedded[]): { index: number } | { problem: string } {
  const typed = raw.trim();
  if (typed === "") return { problem: placeholder(files) };
  if (/^\d+$/.test(typed)) {
    const number = Number(typed);
    return number >= 1 && number <= files.length
      ? { index: number - 1 }
      : { problem: TEXT().noNumber(number, files.length) };
  }
  const wanted = typed.toLowerCase();
  const matching = files.flatMap((file, index) => (file.name.toLowerCase().includes(wanted) ? [index] : []));
  if (matching.length === 1) return { index: matching[0]! };
  return matching.length === 0
    ? { problem: TEXT().noName(typed) }
    : { problem: TEXT().severalNames(matching.length, typed) };
}

/** What the palette shows under an answer that names one file. */
export function preview(raw: string, files: readonly Embedded[]): string {
  const picked = pick(raw, files);
  return "index" in picked ? TEXT().saves(describe(files[picked.index]!)) : "";
}

/** What the window says once a file is saved. */
export function saved(file: Embedded, bytes: number, where: string): string {
  const warning = isProgram(file.name) ? TEXT().programWarning : "";
  return `${TEXT().saved(file.name, where, formatBytes(bytes))}${warning}`;
}

/** The properties dialog's rows: one per file, numbered as the palette numbers them. */
export function attachmentRows(files: readonly Embedded[]): { name: string; value: string }[] {
  return files.map((file, index) => ({
    name: TEXT().row(index + 1),
    value: file.description ? `${describe(file)}: ${file.description}` : describe(file),
  }));
}
