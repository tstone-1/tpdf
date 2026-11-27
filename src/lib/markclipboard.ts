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

import type { CommentStyle } from "./commentformat";
import { catalog } from "./i18n";
import type { MarkKind, MarkView, PageId, StampName } from "./pages";
import { hasStrokes } from "./markband";
import { rotatePixels, type PicturePixels } from "./picture";
import { rotateSignature } from "./signature";
import { viewRect } from "./comments";
import { intoCrop, type CropGeometry } from "./crop";

const TEXT = catalog({
  "en-US": {
    nib: "The stroke width cannot be preserved on this page.",
    reply: "A reply must stay with its comment and cannot be cut or copied.",
    words: "A mark made over words is not copied: on another page it would mark nothing",
    field: "A form field is not copied; use Arrange: duplicate",
    pixels: "A picture whose pixels could not be read is not copied",
    signature: "A signature picture without its pixels is not copied",
  },
  "de-DE": {
    nib: "Die Strichbreite lässt sich auf dieser Seite nicht beibehalten.",
    reply: "Eine Antwort muss bei ihrem Kommentar bleiben und lässt sich nicht ausschneiden oder kopieren.",
    words: "Eine Textmarkierung lässt sich nicht kopieren, weil ihr auf einer anderen Seite der zugehörige Text fehlt.",
    field: "Ein Formularfeld lässt sich mit dem Befehl zum Duplizieren vervielfältigen.",
    pixels: "Das Bild konnte nicht kopiert werden, weil seine Bilddaten nicht lesbar waren.",
    signature: "Das Unterschriftsbild konnte ohne seine Bilddaten nicht kopiert werden.",
  },
});

/** One copied mark. Geometry in the model's space for its page, as `MarkView` holds it. */
export interface CopiedMark {
  kind: MarkKind;
  /** The rectangles, four numbers each. For a kind drawn from strokes, the box round them. */
  quads: number[];
  strokes: number[][];
  note: string;
  styles?: CommentStyle[];
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
  unit: number;
  frame: { turns: number; crop: CropGeometry; width_pt: number; height_pt: number };
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
export function notCopied(kind: MarkKind, isReply = false): string | null {
  if (isReply) return TEXT().reply;
  switch (kind) {
    case "highlight":
    case "underline":
    case "strikeout":
    case "squiggly":
      return TEXT().words;
    case "field":
      return TEXT().field;
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
  const why = notCopied(mark.kind, mark.is_reply);
  if (why) return why;
  if (mark.kind === "picture" && !pixels) return TEXT().pixels;
  if (mark.kind === "signature" && !mark.image) return TEXT().signature;
  return {
    kind: mark.kind,
    quads: [...mark.quads],
    strokes: mark.strokes.map((stroke) => [...stroke]),
    note: mark.note, ...(mark.styles?.length ? { styles: mark.styles } : {}),
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
  private pending: Promise<void> | null = null;
  private generation = 0;

  /** Copy can need the worker for page units or pixels. The last request owns the clipboard. */
  async prepare(read: (current: () => boolean) => Promise<void>): Promise<void> {
    const generation = ++this.generation;
    const current = () => generation === this.generation;
    this.pending = read(current);
    try { await this.pending; }
    finally { if (current()) this.pending = null; }
  }

  get canPaste(): boolean {
    return this.pending !== null || this.held.length > 0;
  }

  async readyMarks(): Promise<readonly CopiedMark[]> {
    const generation = this.generation;
    await this.pending;
    return generation === this.generation ? this.held : [];
  }

  /**
   * Copies `marks`, replacing what was held. Answers how many were copied and
   * why the rest were not; nothing is replaced when none can be.
   */
  copy(
    marks: readonly MarkView[],
    from: CopiedFrom,
    pixels: ReadonlyMap<number, PicturePixels> = new Map(),
    requireAll = false,
  ): { copied: number; refused: string[] } {
    const copied: CopiedMark[] = [];
    const refused: string[] = [];
    for (const mark of marks) {
      const one = describe(mark, pixels.get(mark.id));
      if (typeof one === "string") refused.push(one);
      else copied.push(one);
    }
    if (requireAll && refused.length > 0) return { copied: 0, refused };
    if (copied.length > 0) {
      this.held = copied;
      this.source = from;
    }
    return { copied: copied.length, refused };
  }

  /** A cut waiting for pixels is cancelled when the selection changes. */
  sameSelection(captured: readonly number[], current: readonly number[]): boolean {
    const ids = new Set(current);
    return captured.length === current.length && captured.every((id) => ids.has(id));
  }

  /** The marks held, for a paste. */
  get marks(): readonly CopiedMark[] {
    return this.held;
  }

  /** Return displayed geometry for the destination's fit check and pixels in its storage orientation. */
  forPaste(unit: number, destinationTurns: number): readonly CopiedMark[] {
    if (!this.source) return [];
    const { frame } = this.source;
    const scale = this.source.unit / unit;
    const rotate = frame.turns - destinationTurns;
    if (scale === 1 && !frame.turns && !rotate && !frame.crop.left && !frame.crop.top) return this.held;
    // The annotation command clamps native nib widths outside this range.
    // Refuse the group before placing its first mark if a stroke would change.
    if (this.held.some(mark => hasStrokes(mark.kind) && (mark.width * scale < 0.25 || mark.width * scale > 24))) {
      throw new Error(TEXT().nib);
    }
    const display = (left: number, top: number, right: number, bottom: number) =>
      viewRect(intoCrop([left, top, right, bottom], frame.crop), frame.turns, frame.width_pt, frame.height_pt);
    return this.held.map(mark => {
      const quads: number[] = [];
      for (let i = 0; i < mark.quads.length; i += 4) {
        const box = display(mark.quads[i]!, mark.quads[i + 1]!, mark.quads[i + 2]!, mark.quads[i + 3]!);
        quads.push(box.left * scale, box.top * scale, box.right * scale, box.bottom * scale);
      }
      const strokes = mark.strokes.map(stroke => {
        const points: number[] = [];
        for (let i = 0; i < stroke.length; i += 2) {
          const x = stroke[i]!, y = stroke[i + 1]!;
          const point = display(x, y, x, y);
          points.push(point.left * scale, point.top * scale);
        }
        return points;
      });
      return { ...mark, quads, strokes, width: mark.width * scale,
        ...(mark.picture && { picture: rotatePixels(mark.picture, rotate) }),
        ...(mark.image && { image: rotateSignature(mark.image, rotate) }) };
    });
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
