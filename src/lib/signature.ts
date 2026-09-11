/** Normalized, straight-alpha pixels shared with signature::Image. */
export interface SignatureImage { width: number; height: number; rgba: number[] }
export const SIGNATURE_KEY = "tpdf.saved-signature";

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

/** Read only data explicitly saved through the signature dialog. */
export function savedSignature(): SignatureImage | null {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(SIGNATURE_KEY) ?? "null");
    return validSignature(value) ? value : null;
  } catch { return null; }
}
