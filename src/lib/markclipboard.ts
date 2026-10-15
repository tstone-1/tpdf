/**
 * Copying and pasting the reader's own marks, within a document and between
 * tabs.
 *
 * A mark is copied as its description --- kind, place, note, colour, width,
 * stamp --- and never as PDF objects, and a paste makes new marks through the
 * ordinary add path, so a pasted mark is checked by the model exactly as a
 * drawn one is. Held in the window rather than on the system clipboard: a
 * mark means nothing to another program, and the system clipboard keeps the
 * text a reader copied.
 *
 * No window here. The viewer says where on the page a paste lands, and
 * `App.svelte` makes the marks; this decides what can be copied and holds it.
 */

import type { MarkKind, MarkView, PageId, StampName } from "./pages";
import { hasStrokes } from "./markband";
import type { PicturePixels } from "./picture";

/** One copied mark. Geometry in the model's space for its page, as `MarkView` holds it. */
export interface CopiedMark {
  kind: MarkKind;
  /** The rectangles, four numbers each. For a kind drawn from strokes, the box round them. */
  quads: number[];
  strokes: number[][];
  note: string;
  color: [number, number, number];
  width: number;
  stamp: StampName | null;
  /** A signature picture's pixels, for `"signature"` alone. */
  image?: import("./signature").SignatureImage;
  /** A picture's pixels, for `"picture"` alone: its view carries only the size. */
  picture?: PicturePixels;
}

/** Where the copied marks came from: a document and a page in it. */
export interface CopiedFrom {
  doc: number;
  page: PageId;
}

/**
 * Why a kind cannot be copied, or `null` when it can.
 *
 * A highlight and the three lines under or through words are made of the words
 * they mark; pasted on another page they would mark nothing there. A form
 * field belongs to the document's form, which *Duplicate* handles.
 *
 * A signature picture and a picture are copied with their pixels: a
 * signature's are in its view, and a picture's the caller fetches.
 */
export function notCopied(kind: MarkKind): string | null {
  switch (kind) {
    case "highlight":
    case "underline":
    case "strikeout":
    case "squiggly":
      return "A mark made over words is not copied: on another page it would mark nothing";
    case "field":
      return "A form field is not copied; use Arrange: duplicate";
    case "note":
    case "square":
    case "ellipse":
    case "textbox":
    case "ink":
    case "line":
    case "arrow":
    case "polygon":
    case "polyline":
    case "stamp":
    case "link":
    case "signature":
    case "picture":
      return null;
  }
}

/**
 * The description of a mark, or why it is not copied. A picture needs its
 * pixels in `pixels`; one without them is not copied.
 */
export function describe(mark: MarkView, pixels?: PicturePixels): CopiedMark | string {
  const why = notCopied(mark.kind);
  if (why) return why;
  if (mark.kind === "picture" && !pixels) return "A picture whose pixels could not be read is not copied";
  if (mark.kind === "signature" && !mark.image) return "A signature picture without its pixels is not copied";
  return {
    kind: mark.kind,
    quads: [...mark.quads],
    strokes: mark.strokes.map((stroke) => [...stroke]),
    note: mark.note,
    color: [...mark.color],
    width: mark.width,
    stamp: mark.stamp,
    ...(mark.kind === "signature" && mark.image ? { image: mark.image } : {}),
    ...(mark.kind === "picture" && pixels ? { picture: pixels } : {}),
  };
}

/**
 * What a copied mark is sent to the model as: strokes and no rectangle for a
 * kind drawn from strokes, since the model derives that rectangle; the
 * rectangle and no strokes for the rest.
 */
export function shapeOf(mark: CopiedMark): { quads: number[]; strokes: number[][] } {
  return hasStrokes(mark.kind) ? { quads: [], strokes: mark.strokes } : { quads: mark.quads, strokes: [] };
}

/** The marks last copied, for every tab of the window. */
export class MarkClipboard {
  private held: readonly CopiedMark[] = [];
  private source: CopiedFrom | null = null;

  /**
   * Copies `marks`, replacing what was held. Answers how many were copied and
   * why the rest were not; nothing is replaced when none can be.
   */
  copy(
    marks: readonly MarkView[],
    from: CopiedFrom,
    pixels: ReadonlyMap<number, PicturePixels> = new Map(),
  ): { copied: number; refused: string[] } {
    const copied: CopiedMark[] = [];
    const refused: string[] = [];
    for (const mark of marks) {
      const one = describe(mark, pixels.get(mark.id));
      if (typeof one === "string") refused.push(one);
      else copied.push(one);
    }
    if (copied.length > 0) {
      this.held = copied;
      this.source = from;
    }
    return { copied: copied.length, refused };
  }

  /** The marks held, for a paste. */
  get marks(): readonly CopiedMark[] {
    return this.held;
  }

  /**
   * Whether a paste onto `page` of `doc` lands on the page the marks came
   * from, where it is set a step down and to the right so the copy is not
   * hidden under the original.
   */
  onItsOwnPage(doc: number, page: PageId): boolean {
    return this.source !== null && this.source.doc === doc && this.source.page === page;
  }
}
