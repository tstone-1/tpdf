/** Normalized, straight-alpha pixels shared with signature::Image. */
export interface SignatureImage { width: number; height: number; rgba: number[] }
export const SIGNATURE_KEY = "tpdf.saved-signature";

const MAX_IMPORT_BYTES = 10 * 1024 * 1024;
const MAX_IMPORT_PIXELS = 8 * 1024 * 1024;

/** Read dimensions before handing compressed input to the browser's decoder. */
export function signatureDimensions(bytes: Uint8Array): { width: number; height: number } {
  const invalid = () => new Error("Choose a valid, still PNG or JPEG image smaller than 10 MB and 8 megapixels.");
  if (bytes.length > MAX_IMPORT_BYTES) throw invalid();
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const bounded = (width: number, height: number) => {
    if (!width || !height || width > 8192 || height > 8192 || width * height > MAX_IMPORT_PIXELS) throw invalid();
    return { width, height };
  };
  if (bytes.length >= 33 && [137,80,78,71,13,10,26,10].every((v,i) => bytes[i] === v)) {
    if (view.getUint32(8) !== 13 || view.getUint32(12) !== 0x49484452) throw invalid();
    const size = bounded(view.getUint32(16), view.getUint32(20));
    let data = false, end = false;
    for (let offset = 8; offset < bytes.length;) {
      if (offset + 12 > bytes.length) throw invalid();
      const length = view.getUint32(offset), kind = view.getUint32(offset + 4);
      if (length > bytes.length - offset - 12 || kind === 0x6163544c
          || (offset !== 8 && kind === 0x49484452)) throw invalid();
      if (kind === 0x49444154) data = true;
      offset += length + 12;
      if (kind === 0x49454e44) { end = length === 0 && offset === bytes.length; break; }
    }
    if (!data || !end) throw invalid();
    return size;
  }
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) throw invalid();
  let size: { width: number; height: number } | null = null;
  let scan = false;
  for (let offset = 2; offset < bytes.length;) {
    if (scan) {
      while (offset < bytes.length && bytes[offset] !== 0xff) offset++;
    }
    if (bytes[offset++] !== 0xff) throw invalid();
    while (bytes[offset] === 0xff) offset++;
    const marker = bytes[offset++];
    if (scan && (marker === 0 || (marker !== undefined && marker >= 0xd0 && marker <= 0xd7))) continue;
    if (marker === 0xd9) { if (size && scan && offset === bytes.length) return size; throw invalid(); }
    if (marker === undefined || offset + 2 > bytes.length) throw invalid();
    const length = view.getUint16(offset);
    if (length < 2 || offset + length > bytes.length) throw invalid();
    if (marker === 0xda) { if (!size) throw invalid(); scan = true; }
    if (marker >= 0xc0 && marker <= 0xcf && ![0xc4,0xc8,0xcc].includes(marker)) {
      if (![0xc0,0xc1,0xc2].includes(marker) || size || length < 8 || bytes[offset+2] !== 8) throw invalid();
      size = bounded(view.getUint16(offset+5), view.getUint16(offset+3));
    }
    // DNL changes a frame's height; it is outside the supported import grammar.
    if (marker === 0xdc) throw invalid();
    offset += length;
  }
  throw invalid();
}

/** Refuse oversized input before decoding, and verify the decoder's dimensions. */
export async function decodeSignature(file: Blob,
  decode: (blob: Blob) => Promise<ImageBitmap> = createImageBitmap): Promise<ImageBitmap> {
  if (file.size > MAX_IMPORT_BYTES) throw new Error("Choose an image smaller than 10 MB.");
  const bytes = new Uint8Array(await file.arrayBuffer());
  const expected = signatureDimensions(bytes);
  const image = await decode(file);
  // EXIF orientation may transpose a JPEG's dimensions; its pixel count and
  // largest side stay bounded by the same preflight limits.
  if ((image.width !== expected.width || image.height !== expected.height)
      && (image.width !== expected.height || image.height !== expected.width)) {
    image.close();
    throw new Error("The decoded image dimensions disagree with its header.");
  }
  return image;
}

export function validSignature(value: unknown): value is SignatureImage {
  if (!value || typeof value !== "object") return false;
  const image = value as SignatureImage;
  return Number.isInteger(image.width) && image.width > 0 && image.width <= 512
    && Number.isInteger(image.height) && image.height > 0 && image.height <= 512
    && image.width * image.height <= 512 * 256
    && Array.isArray(image.rgba) && image.rgba.length === image.width * image.height * 4
    && image.rgba.every((v) => Number.isInteger(v) && v >= 0 && v <= 255)
    && image.rgba.some((v, i) => i % 4 === 3 && v > 0);
}

/** Crop transparent margins without changing the signature's colours. */
export function trimSignature(width: number, height: number, pixels: Uint8ClampedArray): SignatureImage | null {
  let left = width, top = height, right = -1, bottom = -1;
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    if (pixels[(y * width + x) * 4 + 3]! > 0) {
      left = Math.min(left, x); right = Math.max(right, x);
      top = Math.min(top, y); bottom = Math.max(bottom, y);
    }
  }
  if (right < left) return null;
  const w = right - left + 1, h = bottom - top + 1;
  const rgba: number[] = [];
  for (let y = top; y <= bottom; y++) {
    for (let i = (y * width + left) * 4; i < (y * width + left + w) * 4; i++) rgba.push(pixels[i]!);
  }
  return { width: w, height: h, rgba };
}

/** A canvas is also the overlay's synchronous image source; no decoder is involved. */
export function signatureCanvas(image: SignatureImage): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = image.width; canvas.height = image.height;
  canvas.getContext("2d")!.putImageData(new ImageData(new Uint8ClampedArray(image.rgba), image.width, image.height), 0, 0);
  return canvas;
}

/** Fit within the dragged rectangle while preserving the signature's proportions. */
export function fitSignature(rect: { left: number; top: number; right: number; bottom: number }, image: SignatureImage) {
  const scale = Math.min((rect.right - rect.left) / image.width, (rect.bottom - rect.top) / image.height);
  return { ...rect, right: rect.left + image.width * scale, bottom: rect.top + image.height * scale };
}

/** Store pixels in the same unturned display space as the mark's rectangle. */
export function rotateSignature(image: SignatureImage, turns: number): SignatureImage {
  let result = image;
  for (let turn = 0; turn < ((turns % 4) + 4) % 4; turn++) {
    const width = result.height, height = result.width;
    const rgba = new Array<number>(result.rgba.length);
    for (let y = 0; y < result.height; y++) for (let x = 0; x < result.width; x++) {
      const target = (x * width + result.height - 1 - y) * 4;
      const source = (y * result.width + x) * 4;
      for (let c = 0; c < 4; c++) rgba[target+c] = result.rgba[source+c]!;
    }
    result = { width, height, rgba };
  }
  return result;
}

/** Rotate pixels with their page, including turns made after placing the signature. */
export function drawSignature(ctx: CanvasRenderingContext2D, image: HTMLCanvasElement,
  left: number, top: number, width: number, height: number, turns: number): void {
  ctx.save(); ctx.translate(left + width/2, top + height/2); ctx.rotate(turns * Math.PI/2);
  const w = turns % 2 ? height : width, h = turns % 2 ? width : height;
  ctx.drawImage(image, -w/2, -h/2, w, h); ctx.restore();
}
