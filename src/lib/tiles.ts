/**
 * Tile fetching over the `tile://` custom scheme.
 *
 * Two transfer paths are implemented so spike 0.1 can measure them against each
 * other rather than assume:
 *
 *  - `raw`: uncompressed RGBA, decoded via ImageData. No encode/decode cost,
 *    ~4 bytes per pixel on the wire.
 *  - `png`: encoded, decoded by the browser's image pipeline off the main
 *    thread, much smaller on the wire.
 *
 * Note the audit claim that `createImageBitmap()` cannot consume raw pixels is
 * wrong --- it accepts an ImageData, which is what the raw path uses.
 */

export type TileFormat = "raw" | "png";

export interface TileRequest {
  doc: number;
  page: number;
  /** Device pixels per PDF point. */
  scale: number;
  x: number;
  y: number;
  width: number;
  height: number;
  format: TileFormat;
}

export interface TileResult {
  bitmap: ImageBitmap;
  /** Bytes received over the wire. */
  bytes: number;
  /** Time inside Pdfium, reported by the server. */
  renderUs: number;
  /** Server-side encode time; 0 for raw. */
  encodeUs: number;
  /** Wall time for fetch + body read, measured client side. */
  transferMs: number;
  /** Wall time to turn the body into an ImageBitmap. */
  decodeMs: number;
}

/**
 * Builds the tile URL.
 *
 * Scale travels as thousandths so the path stays integral and no locale-
 * dependent decimal formatting can creep in.
 */
export function tileUrl(req: TileRequest): string {
  const scale = Math.round(req.scale * 1000);
  const path = [req.doc, req.page, scale, req.x, req.y, req.width, req.height].join("/");
  return `tile://localhost/${path}?fmt=${req.format}`;
}

/** Fetches and decodes one tile, timing each stage separately. */
export async function fetchTile(req: TileRequest): Promise<TileResult> {
  const t0 = performance.now();
  const response = await fetch(tileUrl(req));
  if (!response.ok) {
    throw new Error(`tile ${req.page}@${req.x},${req.y}: ${await response.text()}`);
  }

  const buffer = await response.arrayBuffer();
  const t1 = performance.now();

  const renderUs = Number(response.headers.get("X-Render-Us") ?? 0);
  const encodeUs = Number(response.headers.get("X-Encode-Us") ?? 0);
  const width = Number(response.headers.get("X-Tile-Width") ?? req.width);
  const height = Number(response.headers.get("X-Tile-Height") ?? req.height);

  const bitmap =
    req.format === "raw"
      ? await createImageBitmap(
          new ImageData(new Uint8ClampedArray(buffer), width, height),
        )
      : await createImageBitmap(new Blob([buffer], { type: "image/png" }));

  const t2 = performance.now();

  return {
    bitmap,
    bytes: buffer.byteLength,
    renderUs,
    encodeUs,
    transferMs: t1 - t0,
    decodeMs: t2 - t1,
  };
}
