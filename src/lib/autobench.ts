/**
 * The webview half of spike 0.1, run without a human.
 *
 * `tile-bench` answers what a tile costs to produce. This answers what it costs
 * to *deliver*: custom-scheme dispatch, body read, and decode into an
 * ImageBitmap. Those only exist inside a real webview, but a measurement that
 * needs someone to click a button is a measurement nobody repeats -- so with
 * `TPDF_AUTOBENCH=<file.pdf>` set, the app opens the document, measures, prints
 * on the process's stdout and exits.
 *
 * Variants are interleaved A,B,A,B across rounds and compared pairwise within a
 * round, for the same reason the Rust bench does it: wall clock on these
 * machines drifts several percent over minutes.
 */

import { call } from "./ipc";
import { fetchRequiredTile, type TileFormat } from "./tiles";

interface Variant {
  label: string;
  size: number;
  format: TileFormat;
  scale: number;
}

interface Row {
  label: string;
  round: number;
  /** Server-side Pdfium time, so delivery cost can be separated from it. */
  renderMs: number;
  encodeMs: number;
  transferMs: number;
  decodeMs: number;
  bytes: number;
}

/** The two questions worth asking: does format matter, does tile size matter? */
const VARIANTS: Variant[] = [
  { label: "raw  1024", size: 1024, format: "raw", scale: 2 },
  { label: "png  1024", size: 1024, format: "png", scale: 2 },
  { label: "raw  2048", size: 2048, format: "raw", scale: 2 },
  { label: "png  2048", size: 2048, format: "png", scale: 2 },
];

const ROUNDS = 6;

function median(values: number[]): number {
  if (values.length === 0) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  const hi = sorted[mid] ?? NaN;
  return sorted.length % 2 ? hi : ((sorted[mid - 1] ?? NaN) + hi) / 2;
}

function pad(text: string, width: number, right = false): string {
  return right ? text.padStart(width) : text.padEnd(width);
}

/**
 * Runs the benchmark if `TPDF_AUTOBENCH` is set, then exits the process.
 *
 * Returns false when no auto-bench was requested, so the normal spike UI can
 * carry on.
 */
export async function runAutobenchIfRequested(): Promise<boolean> {
  const path = await call("autobench_path");
  if (!path) return false;

  const lines: string[] = [];
  const log = (line = "") => lines.push(line);

  try {
    const info = await call("open_document", { path });

    const page = info.pages[0];
    if (!page) throw new Error("document has no pages");

    log(`file          ${path}`);
    log(`pages         ${info.page_count}`);
    log(`page 0        ${page.width_pt.toFixed(0)} x ${page.height_pt.toFixed(0)} pt`);
    log();

    const rows: Row[] = [];
    for (let round = 0; round < ROUNDS; round++) {
      for (const variant of VARIANTS) {
        // Centre of the page, matching tile-bench's choice: a corner tile of a
        // sparse page can be empty and would measure nothing.
        const fullWidth = Math.round(page.width_pt * variant.scale);
        const fullHeight = Math.round(page.height_pt * variant.scale);
        const width = Math.min(variant.size, fullWidth);
        const height = Math.min(variant.size, fullHeight);

        const tile = await fetchRequiredTile({
          doc: info.id,
          page: 0,
          scale: variant.scale,
          x: Math.max(0, (fullWidth - width) >> 1),
          y: Math.max(0, (fullHeight - height) >> 1),
          width,
          height,
          format: variant.format,
        });
        tile.bitmap.close();

        rows.push({
          label: variant.label,
          round,
          renderMs: tile.renderUs / 1000,
          encodeMs: tile.encodeUs / 1000,
          transferMs: tile.transferMs,
          decodeMs: tile.decodeMs,
          bytes: tile.bytes,
        });
      }
    }

    log(
      [
        pad("variant", 11),
        pad("render", 9, true),
        pad("encode", 9, true),
        pad("transfer", 10, true),
        pad("decode", 9, true),
        pad("deliver", 9, true),
        pad("KB", 9, true),
        pad("% of render", 13, true),
      ].join(" "),
    );
    log("-".repeat(84));

    for (const variant of VARIANTS) {
      const mine = rows.filter((r) => r.label === variant.label);
      const render = median(mine.map((r) => r.renderMs));
      const encode = median(mine.map((r) => r.encodeMs));
      const transfer = median(mine.map((r) => r.transferMs));
      const decode = median(mine.map((r) => r.decodeMs));
      // Delivery is everything the transport costs on top of producing pixels.
      const deliver = transfer + decode;

      log(
        [
          pad(variant.label, 11),
          pad(render.toFixed(2), 9, true),
          pad(encode.toFixed(2), 9, true),
          pad(transfer.toFixed(2), 10, true),
          pad(decode.toFixed(2), 9, true),
          pad(deliver.toFixed(2), 9, true),
          pad((median(mine.map((r) => r.bytes)) / 1024).toFixed(0), 9, true),
          pad(`${((deliver / render) * 100).toFixed(0)}%`, 13, true),
        ].join(" "),
      );
    }

    log();
    log("per-round delivery ms (transfer + decode), to expose warm-up and drift:");
    for (const variant of VARIANTS) {
      const series = rows
        .filter((r) => r.label === variant.label)
        .sort((a, b) => a.round - b.round)
        .map((r) => (r.transferMs + r.decodeMs).toFixed(2))
        .join("  ");
      log(`  ${pad(variant.label, 11)} ${series}`);
    }

    await call("spike_print", { text: lines.join("\n") });
    await call("spike_exit", { code: 0 });
  } catch (error) {
    await call("spike_print", {
      text: `[ERROR] autobench: ${error instanceof Error ? error.message : String(error)}`,
    });
    await call("spike_exit", { code: 1 });
  }

  return true;
}
