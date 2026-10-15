/**
 * A picture placed on a page: the pixels a worker decoded, the body that sends
 * them back with the mark, and the overlay's copy of each.
 *
 * The webview never decodes a picture file. The backend reads it and a
 * sandboxed worker decodes it (`signature_import::decode_picture`), upright and
 * scaled to what a page holds, and the answer is pixels: {@link readPicture}.
 * The drag that places it sends the pixels back with the mark, as bytes
 * (`picture.rs`), and the model refuses a picture past `signature.rs`'s limits
 * as it refuses any other mark.
 *
 * **The view carries a picture's size and not its pixels.** A state reply
 * goes back after every edit, and a picture is up to 16 MiB. So the overlay
 * asks for the pixels of each picture once, as bytes, and keeps them in a
 * {@link PictureCache} by mark id.
 */

/** Straight-alpha RGBA, four bytes a pixel, top row first. */
export interface PicturePixels {
  width: number;
  height: number;
  rgba: Uint8Array<ArrayBuffer>;
}

/** Longest side of a placed picture: `PICTURE_SIDE` in `signature.rs`. */
export const PICTURE_SIDE = 4096;
/** Most pixels in a placed picture: `PICTURE_PIXELS` in `signature.rs`. */
export const PICTURE_PIXELS = 4 * 1024 * 1024;

/**
 * The pixels in a decoding command's reply: width and height as four bytes
 * each, little endian, then four bytes a pixel (`picture_reply` in
 * `commands/edit.rs`). Refuses a reply whose parts do not add up, or that is
 * past a placed picture's limits, rather than drawing a wrong picture.
 */
export function readPicture(reply: ArrayBuffer): PicturePixels {
  const bad = new Error("The picture came back damaged.");
  if (reply.byteLength < 8) throw bad;
  const view = new DataView(reply);
  const width = view.getUint32(0, true), height = view.getUint32(4, true);
  if (!width || !height || width > PICTURE_SIDE || height > PICTURE_SIDE || width * height > PICTURE_PIXELS
      || reply.byteLength !== 8 + width * height * 4) throw bad;
  return { width, height, rgba: new Uint8Array(reply, 8) };
}

/**
 * The body `annot_picture` takes: the JSON part's length as four bytes, little
 * endian, then the JSON, then the pixels. `picture.rs` reads it.
 */
export function pictureBody(header: object, pixels: PicturePixels): Uint8Array {
  const json = new TextEncoder().encode(JSON.stringify(header));
  const body = new Uint8Array(4 + json.length + pixels.rgba.length);
  new DataView(body.buffer).setUint32(0, json.length, true);
  body.set(json, 4);
  body.set(pixels.rgba, 4 + json.length);
  return body;
}

/**
 * Turns pixels a quarter turn clockwise per `turns`, so a picture placed on a
 * turned page is held the way the page's own space has it, as
 * `rotateSignature` does for a signature.
 */
export function rotatePixels(pixels: PicturePixels, turns: number): PicturePixels {
  let result = pixels;
  for (let turn = 0; turn < ((turns % 4) + 4) % 4; turn++) {
    const width = result.height, height = result.width;
    // A word per pixel. A view needs its offset on a word, so a view that is
    // not on one is copied first.
    const own = result.rgba.byteOffset % 4 === 0 ? result.rgba : result.rgba.slice();
    const from = new Uint32Array(own.buffer, own.byteOffset, own.length / 4);
    const rgba = new Uint8Array(result.rgba.length);
    const to = new Uint32Array(rgba.buffer);
    for (let y = 0; y < result.height; y++) {
      for (let x = 0; x < result.width; x++) to[x * width + result.height - 1 - y] = from[y * result.width + x]!;
    }
    result = { width, height, rgba };
  }
  return result;
}

/** The kinds of pasted file a worker decodes: `decode_picture`'s two. */
const PASTED_TYPES = new Set(["image/png", "image/jpeg"]);

/**
 * The picture in a paste event's data, or null when it holds none.
 *
 * WebKit hands a pasted picture to the page as an `image/png` file, whatever
 * the pasteboard held (measured with TIFF alone); the file's bytes go to a
 * worker as a picture file's do, never to a decoder in the webview. The first
 * picture is taken when there are several, and any other kind of file is not
 * one.
 */
export function pastedPicture(data: { files: ArrayLike<File> } | null): File | null {
  if (!data) return null;
  for (const file of Array.from(data.files)) {
    if (PASTED_TYPES.has(file.type)) return file;
  }
  return null;
}

/** Whether any pixel can be seen: a picture with none would place nothing. */
export function visible(pixels: PicturePixels): boolean {
  for (let i = 3; i < pixels.rgba.length; i += 4) if (pixels.rgba[i] !== 0) return true;
  return false;
}

/** A canvas holding `pixels`, for the overlay to draw from. */
export function pictureCanvas(pixels: PicturePixels): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = pixels.width; canvas.height = pixels.height;
  const data = new Uint8ClampedArray(pixels.rgba.buffer, pixels.rgba.byteOffset, pixels.rgba.byteLength);
  canvas.getContext("2d")!.putImageData(new ImageData(data, pixels.width, pixels.height), 0, 0);
  return canvas;
}

/**
 * The pixels of the pictures on screen, by mark id, fetched once each.
 *
 * `get` answers what it has and starts a fetch for what it lacks, calling
 * `ready` when one lands so the overlay paints again. A fetch that fails is
 * forgotten, so the next paint asks again. `keep` drops every mark not named,
 * as the overlay's signature canvases are dropped.
 */
export class PictureCache<T> {
  private readonly held = new Map<number, T | "fetching">();

  constructor(
    private readonly fetch: (mark: number) => Promise<T>,
    private readonly ready: () => void,
  ) {}

  /** What is held for `mark`, or `null` while it is fetched. */
  get(mark: number): T | null {
    const held = this.held.get(mark);
    if (held !== undefined) return held === "fetching" ? null : held;
    this.held.set(mark, "fetching");
    this.fetch(mark).then(
      (value) => {
        if (this.held.get(mark) !== "fetching") return;
        this.held.set(mark, value);
        this.ready();
      },
      () => {
        if (this.held.get(mark) === "fetching") this.held.delete(mark);
      },
    );
    return null;
  }

  /** Drops every mark not in `live`. */
  keep(live: ReadonlySet<number>): void {
    for (const mark of this.held.keys()) if (!live.has(mark)) this.held.delete(mark);
  }
}
