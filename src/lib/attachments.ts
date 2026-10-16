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

/** One embedded file: `attachments::Embedded`. */
export interface Embedded {
  name: string;
  description: string;
  /** The size the document states for it, if it states one. */
  size: number | null;
}

/** Kinds by the ending a name claims, lower case and without the dot. */
const KINDS: Record<string, string> = {
  pdf: "a PDF document",
  doc: "a Word document", docx: "a Word document",
  xls: "an Excel workbook", xlsx: "an Excel workbook", xlsm: "an Excel workbook",
  ppt: "a PowerPoint presentation", pptx: "a PowerPoint presentation",
  odt: "an OpenDocument text", ods: "an OpenDocument spreadsheet", odp: "an OpenDocument presentation",
  csv: "a table of comma-separated values", txt: "a text file", xml: "an XML file", json: "a JSON file",
  htm: "a web page", html: "a web page",
  png: "a picture", jpg: "a picture", jpeg: "a picture", gif: "a picture", tif: "a picture",
  tiff: "a picture", bmp: "a picture", webp: "a picture",
  zip: "an archive", "7z": "an archive", rar: "an archive", gz: "an archive", tar: "an archive",
  eml: "an e-mail", msg: "an e-mail",
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
  if (end === "") return "a file with no ending to say what it is";
  if (PROGRAMS.has(end)) return "a program or script";
  return KINDS[end] ?? `a .${end} file`;
}

/** One file in a line: its name, the kind it claims, and its stated size. */
export function describe(file: Embedded): string {
  const size = file.size === null ? "" : `, ${formatBytes(file.size)}`;
  return `${file.name} (${kindOf(file.name)}${size})`;
}

/** What the palette offers while it asks which file. */
export function placeholder(files: readonly Embedded[]): string {
  return `Which attachment: 1 to ${files.length}, or part of its name`;
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
      : { problem: `There is no attachment ${number}; there are ${files.length}` };
  }
  const wanted = typed.toLowerCase();
  const matching = files.flatMap((file, index) => (file.name.toLowerCase().includes(wanted) ? [index] : []));
  if (matching.length === 1) return { index: matching[0]! };
  return matching.length === 0
    ? { problem: `No attachment's name has "${typed}" in it` }
    : { problem: `${matching.length} attachments have "${typed}" in their names; type a number` };
}

/** What the palette shows under an answer that names one file. */
export function preview(raw: string, files: readonly Embedded[]): string {
  const picked = pick(raw, files);
  return "index" in picked ? `Saves ${describe(files[picked.index]!)}` : "";
}

/** What the window says once a file is saved. */
export function saved(file: Embedded, bytes: number, where: string): string {
  const warning = isProgram(file.name)
    ? " Its name says it is a program or script: open it only if you trust the document it came from."
    : "";
  return `Saved ${file.name} to ${where}, ${formatBytes(bytes)}.${warning}`;
}

/** The properties dialog's rows: one per file, numbered as the palette numbers them. */
export function attachmentRows(files: readonly Embedded[]): { name: string; value: string }[] {
  return files.map((file, index) => ({
    name: `Attachment ${index + 1}`,
    value: file.description ? `${describe(file)}: ${file.description}` : describe(file),
  }));
}
