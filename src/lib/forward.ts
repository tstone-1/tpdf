/**
 * LaTeX forward search, as the window reads it: `--forward-search file.tex:LINE`
 * on the command line, answered by `synctex_forward` (`synctex.rs`), which finds
 * the boxes that line made in the SyncTeX file beside the PDF.
 */

/** A rectangle on a page, in points from its top-left corner: `synctex::Rect`. */
export interface SourceRect {
  x: number;
  top: number;
  width: number;
  height: number;
}

/** Where a source line went: `synctex::Forward`. */
export interface Forward {
  /** Counted from 1. */
  page: number;
  /** The line whose boxes these are: the one asked for, or the nearest that made something. */
  line: number;
  boxes: SourceRect[];
}

/**
 * A `--forward-search` value: the source file and the line, counted from 1,
 * or what is wrong with it. Split at the last colon, so a Windows path keeps
 * its drive letter.
 */
export function parseForward(raw: string): { source: string; line: number } | { problem: string } {
  const typed = raw.trim();
  const said = `tpdf was asked to --forward-search "${typed}"`;
  const colon = typed.lastIndexOf(":");
  const source = colon < 0 ? "" : typed.slice(0, colon);
  const line = colon < 0 ? "" : typed.slice(colon + 1);
  if (source === "" || !/^[0-9]+$/.test(line)) {
    return { problem: `${said}, which is not a file and a line: give it as file.tex:LINE` };
  }
  const number = Number(line);
  if (number < 1) return { problem: `${said}, and lines count from 1` };
  return { source, line: number };
}

/**
 * What the window says once the boxes are shown: nothing when they are the
 * line asked for, and which line they are when that line made nothing.
 */
export function forwardSaid(found: Forward, asked: number): string | null {
  if (found.line === asked) return null;
  return `Line ${asked} made nothing on the page; showing line ${found.line}, the nearest that did.`;
}
