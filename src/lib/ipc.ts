/**
 * The shapes the backend sends over the command boundary, declared once.
 *
 * Every type here mirrors a `serde`-serialized struct in
 * `src-tauri/src/render.rs`, and **that file is the authority**: when the two
 * disagree, Rust is right and this one is stale. Field names are snake_case
 * because serde emits the Rust identifiers verbatim --- there is no `rename_all`
 * on either struct, so a name here is a name there.
 *
 * Nothing enforces the mirror. There is no code generation at this seam, which
 * is precisely why the mirror is not allowed to exist more than once: a rename
 * on the Rust side compiles green on both sides of the boundary and surfaces as
 * an `undefined` flowing into layout arithmetic --- a page laid out at `NaN`
 * rather than an error anyone can read. TypeScript cannot see across an
 * `invoke`, so the best available property is that correcting the mirror is a
 * single edit rather than a search.
 *
 * It was a search. `DocumentInfo` was hand-declared in four places, and two of
 * those copies had already stopped being the same type --- they listed `id`,
 * `pages` and `page_count` and omitted the other three fields. That is the
 * benign direction of the drift (a subset still type-checks against the real
 * reply), and it is worth naming anyway, because it is what the divergence looks
 * like *before* it is a bug: four declarations that nobody was comparing.
 */

/**
 * Page geometry in PDF points.
 *
 * Sent up front so the scroller can size the document without rendering
 * anything --- see `render.rs`, where the same struct says the same thing.
 */
export interface PageSize {
  width_pt: number;
  height_pt: number;
}

/**
 * Why `open_document` refused, when it did.
 *
 * The one refusal a reader can answer carries a flag rather than a recognisable
 * sentence, so `App.svelte` decides to prompt on `locked` and never on the
 * wording. `progressive::Refusal` is the same shape on the Rust side, which is
 * where both fields are written.
 */
export interface OpenRefusal {
  /** What to show. Chosen in `progressive.rs`; never from the document. */
  reason: string;
  /**
   * The document is encrypted and the password it was given --- which may have
   * been none --- did not open it.
   *
   * It says nothing about *whether* one was tried: PDFium reports the same error
   * either way, so only `reason` distinguishes a first ask from a retry.
   */
  locked: boolean;
}

/**
 * Whether a thrown value is an `OpenRefusal` rather than an ordinary error.
 *
 * Tauri serialises a command's `Err` and rejects with it, so what arrives is a
 * plain object and not an `Error` --- `instanceof` cannot be the test, and
 * `String(e)` on one reads `[object Object]`.
 */
export function isOpenRefusal(value: unknown): value is OpenRefusal {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as OpenRefusal).reason === "string" &&
    typeof (value as OpenRefusal).locked === "boolean"
  );
}

/** Result of `open_document`. */
export interface DocumentInfo {
  id: number;
  /** Geometry of every page, or only of page 1 when the open was lazy. */
  pages: PageSize[];
  /** Pages in the document, which is known even when their sizes are not. */
  page_count: number;
  /** Whether `pages` is the whole table or only its first entry. */
  lazy_geometry: boolean;
  /** Time spent opening, i.e. parse and cross-reference repair. */
  open_ms: number;
  /** Milliseconds since process start when the open completed. */
  at_ms: number;
}
