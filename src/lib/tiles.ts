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
  /**
   * Names this request so it can later be withdrawn with {@link cancelTile}.
   *
   * Omitted, or zero, the request cannot be cancelled and will be rendered
   * whatever the viewport does next. Ids must be unique for the life of the
   * process --- use {@link nextRequestId}.
   */
  rid?: number;
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
  const rid = req.rid ? `&rid=${req.rid}` : "";
  return `tile://localhost/${path}?fmt=${req.format}${rid}`;
}

let lastRequestId = 0;

/**
 * A fresh request id.
 *
 * Monotonic and never zero, because zero is the server's "not withdrawable"
 * sentinel. Counting from a module-level variable rather than per caller keeps
 * ids unique across every scroller and benchmark in the page, which is what the
 * server assumes.
 */
export function nextRequestId(): number {
  lastRequestId += 1;
  return lastRequestId;
}

/**
 * Withdraws a tile request.
 *
 * Fire-and-forget, and safe at any point in the request's life: an id the server
 * no longer knows is ignored. A request that has not started is dropped without
 * being rendered; one already running is abandoned at Pdfium's next pause.
 *
 * This is a second request rather than an `AbortController` on the first,
 * because aborting a `fetch()` over a custom scheme stops the response being
 * read and does not stop the render producing it.
 */
export function cancelTile(rid: number): void {
  if (!rid) return;
  void fetch(`tile://localhost/cancel/${rid}`).catch(() => {
    // The withdrawal is an optimisation. Losing one costs a render, not
    // correctness, and there is no useful recovery from here.
  });
}

/**
 * Fetches and decodes one tile, timing each stage separately.
 *
 * Returns `null` when the server abandoned the request because it was withdrawn
 * --- which is a normal outcome, not a failure, and deliberately not an empty
 * tile: there is nothing to draw, and a caller that painted one would erase
 * whatever it already had there.
 */
export async function fetchTile(req: TileRequest): Promise<TileResult | null> {
  const t0 = performance.now();
  const response = await fetch(tileUrl(req));
  if (!response.ok) {
    throw new Error(`tile ${req.page}@${req.x},${req.y}: ${await response.text()}`);
  }
  if (response.status === 204) return null;

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

/**
 * Fetches a tile that is not withdrawable, for callers that always want it.
 *
 * Forces `rid` to zero, so an abandonment is a broken contract rather than an
 * outcome and is reported as one instead of being handled away at four call
 * sites that could never see it.
 */
export async function fetchRequiredTile(req: TileRequest): Promise<TileResult> {
  const result = await fetchTile({ ...req, rid: 0 });
  if (!result) {
    throw new Error(`tile ${req.page}@${req.x},${req.y} was abandoned but never withdrawn`);
  }
  return result;
}
