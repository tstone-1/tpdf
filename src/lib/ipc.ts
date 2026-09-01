/**
 * The command boundary: which commands exist, and what each one takes and
 * answers.
 *
 * **`src-tauri/src/lib.rs` is the authority.** Its `generate_handler!` list is
 * the set of names and each `#[tauri::command]` signature is the pair of shapes;
 * when the two sides disagree, Rust is right and this file is stale. Field names
 * are the Rust identifiers verbatim, except on a struct carrying
 * `#[serde(rename_all = "camelCase")]` --- `search::Options`, `search::Match`
 * and `search::Carry` do, and their mirrors are camelCase for that reason and
 * that reason only. **Argument keys are the one place the convention is not
 * ours**: Tauri deserialises a command's parameters camelCase, so `startup_mark`
 * takes `atMs` for `at_ms`. It is the only multi-word parameter in the surface.
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
 *
 * The same shape at the level above it is what {@link Commands} answers: a
 * command name and its argument keys were restated at every call site, so the
 * contract existed on the Rust side --- the registry, plus the `writers` and
 * `classified` gates --- and in no single place here. Those gates catch a name
 * that stopped existing; nothing caught a *shape* that stopped agreeing.
 *
 * Every type import below is `import type`, so `verbatimModuleSyntax` erases it.
 * The runtime graph therefore still points one way --- callers import
 * {@link call} from here, and nothing here imports a caller --- while the map
 * can name the domain types that already exist rather than re-declaring a
 * second, drifting copy of each.
 */

import { invoke, type InvokeArgs } from "@tauri-apps/api/core";

import type { Comments } from "./comments";
import type { CropGeometry } from "./crop";
import type { Applied, Copied, EditState, Merged, Split } from "./edits";
import type { Links } from "./links";
import type { MarkColor } from "./markcolors";
import type { SectionSpec } from "./menubar";
import type { Outline } from "./outline";
import type {
  FilePage,
  MarkKind,
  PageId,
  RegionPlan,
  StampName,
} from "./pages";
import type { Properties } from "./properties";
import type { ScrollBenchConfig } from "./scrollbench";
import type {
  Carry,
  PageMapping,
  PageMatches,
  SearchOptions,
} from "./search";
import type { Place, Session } from "./session";
import type { PageText } from "./text";

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

/**
 * A mark as it is asked for, before the model has given it an identity.
 *
 * Mirrors `edits::NewMark`, which is the only argument shape in the surface
 * complex enough to have a name of its own. The three defaulted fields there ---
 * `strokes`, `stamp` and `reply_to` --- are required here: a caller on this side
 * always knows which kind it is sending, and the biconditionals the model
 * enforces between `kind`, `stamp` and `reply_to` are refusals rather than
 * omissions.
 */
export interface NewMark {
  kind: MarkKind;
  /** The page, by the identity a state reply gave it --- never a slot. */
  page: PageId;
  /** Four per rectangle, left/top/right/bottom in display space. Empty for ink. */
  quads: number[];
  /** One entry per stroke, each `x y x y ...` in display space. */
  strokes: number[][];
  /** Set for `stamp` and null for every other kind; the model refuses the rest. */
  stamp: StampName | null;
  /** The comment this one answers, as `[number, generation]`. */
  reply_to: readonly [number, number] | null;
  color: MarkColor;
  /** The nib, in points. Read by the ink arm and sent by all of them. */
  width: number;
  author: string;
  note: string;
}

/** Four numbers, in whichever space the command taking them names. */
type Rect = readonly [number, number, number, number];

/**
 * A command taking no arguments.
 *
 * `Record<never, never>` rather than `{}`, because `keyof` it is `never` --- and
 * that is what {@link Args} branches on to decide whether {@link call} takes a
 * second parameter at all.
 */
type NoArgs = Record<never, never>;

/**
 * Every command the backend registers, with what it takes and what it answers.
 *
 * **In `generate_handler!`'s own order**, so the two lists can be read side by
 * side and diffed line for line --- which is the property a gate over this map
 * would assert, and the reason the keys are plain identifiers rather than
 * anything computed.
 *
 * `reply` is the `Ok` side of the command's `Result`; a rejection carries the
 * `Err` side and is not typed here, because `invoke` rejects with `unknown` and
 * the two refusals a caller acts on ({@link OpenRefusal}, `SaveFailure`) are
 * recognised by shape at the site that acts on them.
 *
 * A Rust `Option` parameter is written two ways, deliberately. Optional (`?:`)
 * where the frontend really does omit the key --- `open_document`'s password on
 * the startup path, `print_document`'s document when a check prints a file it
 * has not opened. Required-and-nullable everywhere else, because `null` means
 * something there ("put the file's own box back", "insert at the front") and a
 * caller should have to say it rather than reach the same behaviour by
 * forgetting a key.
 */
export interface Commands {
  open_document: {
    args: { path: string; password?: string | undefined };
    reply: DocumentInfo;
  };
  page_rotate: {
    args: { doc: number; page: PageId; turns: number };
    reply: EditState;
  };
  page_crop: {
    args: { doc: number; page: PageId; to: Rect | null };
    reply: EditState;
  };
  page_content_box: {
    args: { doc: number; page: number };
    reply: [number, number, number, number] | null;
  };
  page_geometry: {
    args: { doc: number; page: FilePage; crop: Rect | null };
    reply: CropGeometry;
  };
  page_crop_box: {
    args: { doc: number; page: number; rect: Rect };
    reply: [number, number, number, number];
  };
  page_delete: { args: { doc: number; page: PageId }; reply: EditState };
  page_move: {
    args: { doc: number; page: PageId; after: PageId | null };
    reply: EditState;
  };
  page_insert: {
    args: {
      doc: number;
      after: PageId | null;
      size: readonly [number, number];
    };
    reply: EditState;
  };
  annot_mark: { args: { doc: number; mark: NewMark }; reply: EditState };
  annot_remove: {
    args: { doc: number; mark: number; sweep: number };
    reply: EditState;
  };
  redact_mark: {
    args: { doc: number; page: PageId; area: Rect };
    reply: EditState;
  };
  redact_remove: {
    args: { doc: number; redaction: number };
    reply: EditState;
  };
  redaction_plans: {
    args: { doc: number; page: FilePage; regions: readonly Rect[] };
    reply: RegionPlan[];
  };
  redact_copy: {
    args: { doc: number; source: string; path: string };
    reply: Applied;
  };
  redact_document: { args: { doc: number; source: string }; reply: Applied };
  annot_erase: {
    args: { doc: number; mark: number; remove: number[]; sweep: number };
    reply: EditState;
  };
  annot_note: {
    args: { doc: number; mark: number; note: string };
    reply: EditState;
  };
  annot_rewrite: {
    args: {
      doc: number;
      object: readonly [number, number];
      page: PageId;
      body: string;
    };
    reply: EditState;
  };
  annot_discard: {
    args: { doc: number; object: readonly [number, number]; page: PageId };
    reply: EditState;
  };
  annot_recolor: {
    args: { doc: number; mark: number; color: MarkColor };
    reply: EditState;
  };
  annot_move: {
    args: { doc: number; mark: number; dx: number; dy: number };
    reply: EditState;
  };
  edit_undo: { args: { doc: number }; reply: EditState };
  edit_redo: { args: { doc: number }; reply: EditState };
  edit_state: { args: { doc: number }; reply: EditState };
  save_document: { args: { doc: number; source: string }; reply: void };
  save_copy: {
    args: { doc: number; source: string; path: string };
    reply: Copied;
  };
  extract_pages: {
    args: { doc: number; source: string; path: string; slots: number[] };
    reply: Copied;
  };
  split_document: {
    args: { doc: number; source: string; path: string; groups: number[][] };
    reply: Split;
  };
  merge_documents: {
    args: { doc: number; source: string; path: string; others: string[] };
    reply: Merged;
  };
  keyboard_positions: { args: NoArgs; reply: Record<string, string> };
  /** The reply is the event name the menu emits on, or null when none was built. */
  set_menu: { args: { sections: SectionSpec[] }; reply: string | null };
  set_menu_enabled: {
    args: { state: Record<string, boolean> };
    reply: void;
  };
  close_document: { args: { doc: number }; reply: void };
  /** The reply counts the documents released. */
  release_documents: { args: NoArgs; reply: number };
  page_text: {
    args: { doc: number; page: FilePage; crop: Rect | null };
    reply: PageText;
  };
  search_page: {
    args: {
      doc: number;
      page: FilePage;
      query: string;
      options: SearchOptions;
      carry?: Carry | undefined;
    };
    reply: PageMatches;
  };
  document_outline: { args: { doc: number }; reply: Outline };
  document_comments: { args: { doc: number }; reply: Comments };
  document_links: { args: { doc: number }; reply: Links };
  document_properties: { args: { doc: number }; reply: Properties };
  document_mapping: { args: { doc: number }; reply: PageMapping[] };
  /** The reply is the name of the event a double-click delivers a path on. */
  launch_open_event: { args: NoArgs; reply: string };
  app_version: { args: NoArgs; reply: string };
  take_launch_paths: { args: NoArgs; reply: string[] };
  session_load: { args: NoArgs; reply: Session };
  session_remember: { args: { place: Place }; reply: void };
  session_set_invert_pages: { args: { invert: boolean }; reply: void };
  print_document: {
    args: {
      path: string;
      doc?: number | null;
      pages: number[] | null;
      turns: number;
    };
    reply: void;
  };
  process_elapsed_ms: { args: NoArgs; reply: number };
  autobench_path: { args: NoArgs; reply: string | null };
  viewercheck_path: { args: NoArgs; reply: string | null };
  viewercheck_scratch: { args: NoArgs; reply: string | null };
  reading_manifest: { args: NoArgs; reply: string | null };
  corpus_manifest: { args: NoArgs; reply: string | null };
  geometry_manifest: { args: NoArgs; reply: string | null };
  sessioncheck_mode: { args: NoArgs; reply: string | null };
  opencheck_mode: { args: NoArgs; reply: string | null };
  markcheck_mode: { args: NoArgs; reply: string | null };
  startup_path: { args: NoArgs; reply: string | null };
  scrollbench_config: { args: NoArgs; reply: ScrollBenchConfig | null };
  /** `atMs`, not `at_ms`: Tauri deserialises parameters camelCase. */
  startup_mark: { args: { name: string; atMs: number }; reply: void };
  startup_timeline: { args: NoArgs; reply: [string, number][] };
  startup_pre_main_ms: { args: NoArgs; reply: number | null };
  spike_print: { args: { text: string }; reply: void };
  spike_exit: { args: { code: number }; reply: void };
}

/**
 * The second parameter of {@link call}, or nothing at all.
 *
 * A tuple rather than an optional parameter so that the argument-less commands
 * are called as `call("app_version")` and every other one *must* pass its
 * arguments: an optional parameter would make `call("page_delete")` legal, which
 * is the omission this map exists to make impossible.
 */
type Args<K extends keyof Commands> = [keyof Commands[K]["args"]] extends [never]
  ? []
  : [args: Commands[K]["args"]];

/**
 * Invokes a backend command by name, with its arguments and its reply typed.
 *
 * The one cast in this file, and it is Tauri's shape rather than ours:
 * `InvokeArgs` is `Record<string, unknown>`, and an object type is assignable to
 * that only once the checker can see its keys --- which it cannot for a generic
 * `K`. The value being cast is the caller's own literal, already checked against
 * `Commands[K]["args"]` at the call site, so the cast widens a checked type
 * rather than standing in for a missing check.
 */
export function call<K extends keyof Commands>(
  command: K,
  ...rest: Args<K>
): Promise<Commands[K]["reply"]> {
  const [args] = rest as [Commands[K]["args"]?];
  return invoke<Commands[K]["reply"]>(
    command,
    args as InvokeArgs | undefined,
  );
}
