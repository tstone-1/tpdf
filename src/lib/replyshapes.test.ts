/**
 * The committed reply samples against the TypeScript mirrors that read them.
 *
 * ## Why this exists
 *
 * `ipc.test.ts` diffs the command *names* between `generate_handler!` and the
 * `Commands` map, and says in its own header what it cannot do: the shapes are
 * types, and the boundary is where TypeScript stops. So the map's `reply` types
 * were read against the Rust signatures by hand --- and the failure that
 * produces is written down in `ipc.ts`'s header, where `DocumentInfo` had been
 * hand-declared four times and two of the copies had stopped listing the same
 * fields.
 *
 * `src-tauri/testdata/replies/<Type>.json` is what closes it. Rust writes those
 * files from real values of the seventeen named payload types
 * (`src-tauri/src/replies.rs`, regenerated with `TPDF_REPLIES=write`), and this
 * file reads the same bytes and holds them against the mirror. Nothing is
 * generated from anything: a renamed Rust field changes the sample, and the
 * sample then fails against a mirror that still has the old name.
 *
 * ## Two checks, because neither one closes both directions
 *
 * **The type check** is `satisfies` against a widened form of the mirror. A JSON
 * module's inferred type is widened by the compiler --- a string literal arrives
 * as `string`, a tuple as an array --- so the mirror is widened to match with
 * {@link Widen}, which is what lets the assignability question be asked at all.
 * It catches a field the sample no longer carries and a field whose JSON type
 * changed, at every depth. It cannot catch a field the sample carries and the
 * mirror does not know about: excess-property checking applies to fresh object
 * literals, and an imported module is not one.
 *
 * **The key check** is that direction. {@link SCHEMA} gives each type a kind per
 * key, typed `Record<keyof T, ...>` so the compiler refuses a key the mirror
 * does not declare and refuses to let one be left out --- the list is held to
 * the mirror by the compiler rather than by a reader. The sample's own keys are
 * then compared against it, so a Rust field added without a mirror is a failure
 * here rather than an `undefined` flowing into layout arithmetic somewhere.
 *
 * A mirror is allowed to be a strict subset, and {@link RegionPlan} is one:
 * `pages.ts` states the rule it follows and five of the nine fields do not
 * cross. So the comparison is not a set equality --- it is three questions, and
 * the third is what keeps the second honest. Every mirror key is sent; every key
 * sent is either mirrored or named in {@link UNMIRRORED} with a reason; and
 * every name in {@link UNMIRRORED} is a field that really is sent, so an
 * excusal cannot outlive the field it excuses.
 *
 * Depth is the honest limit: the key check is the top level of each payload, and
 * everything below it is covered by the type check alone.
 *
 * ## What the samples have to be
 *
 * Every key of every mirror is present in its sample. That is asserted rather
 * than assumed, and it is why `replies.rs` fills every `Option` and every `Vec`:
 * a `skip_serializing_if` field left empty writes no key, and a mirror can be
 * wrong for months about a field no sample mentions.
 */

import { describe, expect, it } from "vitest";

import type { Comments } from "./comments";
import type { CropGeometry } from "./crop";
import type { Applied, Copied, EditState, Merged, Split } from "./edits";
import type { DocumentInfo } from "./ipc";
import type { Links } from "./links";
import type { RegionPlan } from "./pages";
import type { Properties } from "./properties";
import type { Outline } from "./outline";
import type { ScrollBenchConfig } from "./scrollbench";
import type { PageMapping, PageMatches } from "./search";
import type { Session } from "./session";
import type { PageText } from "./text";

import Applied_ from "../../src-tauri/testdata/replies/Applied.json";
import Comments_ from "../../src-tauri/testdata/replies/Comments.json";
import Copied_ from "../../src-tauri/testdata/replies/Copied.json";
import CropGeometry_ from "../../src-tauri/testdata/replies/CropGeometry.json";
import DocumentInfo_ from "../../src-tauri/testdata/replies/DocumentInfo.json";
import EditState_ from "../../src-tauri/testdata/replies/EditState.json";
import Links_ from "../../src-tauri/testdata/replies/Links.json";
import Merged_ from "../../src-tauri/testdata/replies/Merged.json";
import Outline_ from "../../src-tauri/testdata/replies/Outline.json";
import PageMapping_ from "../../src-tauri/testdata/replies/PageMapping.json";
import PageMatches_ from "../../src-tauri/testdata/replies/PageMatches.json";
import PageText_ from "../../src-tauri/testdata/replies/PageText.json";
import Properties_ from "../../src-tauri/testdata/replies/Properties.json";
import RegionPlan_ from "../../src-tauri/testdata/replies/RegionPlan.json";
import ScrollBenchConfig_ from "../../src-tauri/testdata/replies/ScrollBenchConfig.json";
import Session_ from "../../src-tauri/testdata/replies/Session.json";
import Split_ from "../../src-tauri/testdata/replies/Split.json";

/**
 * A mirror type with every literal widened the way a JSON import widens.
 *
 * The compiler types `"highlight"` in a `.json` file as `string`, `[1, 2]` as
 * `number[]` and `1` as `number`, so a sample can never satisfy a mirror that
 * uses string-literal unions, branded numbers or tuples --- and every one of
 * those is in this surface (`MarkKind`, `PageId`, `MarkColor`). Widening the
 * *mirror* instead keeps the field names, the optionality and the nesting, which
 * is what the check is about, and drops only the precision the file format
 * cannot carry.
 *
 * A branded number (`number & { readonly __pageId: unique symbol }`) matches the
 * `number` arm, which is why it is tested before `object`.
 */
type Widen<T> = T extends string
  ? string
  : T extends number
    ? number
    : T extends boolean
      ? boolean
      : T extends readonly (infer E)[]
        ? Widen<E>[]
        : T extends object
          ? { [K in keyof T]: Widen<T[K]> }
          : T;

/** What a JSON value is, as far as the key check looks. */
type Kind = "string" | "number" | "boolean" | "array" | "object" | "null";

/** The kinds each key of `T` carries in its sample. */
type Shape<T> = Record<keyof T, readonly Kind[]>;

/**
 * Every payload's top level: the keys, and what each one is in the sample.
 *
 * `Shape<T>` is the whole point. A key the mirror does not declare is a
 * compile error here, and a key the mirror declares and this omits is one too
 * --- so this table cannot drift from the mirror without the build saying so,
 * and the run-time comparison below is then a comparison of the sample against
 * the mirror rather than against a second hand-written list.
 *
 * The kinds are the sample's, not the type's. `Properties.encryption` can be
 * `null` on the wire and is an object here, because the sample is built with
 * every optional field filled --- which is what makes `["object"]` a real
 * assertion rather than a list of everything that would be tolerated.
 */
const SCHEMA = {
  Applied: {
    regions: ["number"],
    shows: ["number"],
    changed: ["boolean"],
    verified: ["boolean"],
    why: ["array"],
  } satisfies Shape<Applied>,
  Comments: {
    items: ["array"],
    limits: ["object"],
    scan_ms: ["number"],
  } satisfies Shape<Comments>,
  Copied: {
    changed: ["boolean"],
  } satisfies Shape<Copied>,
  CropGeometry: {
    width_pt: ["number"],
    height_pt: ["number"],
    left: ["number"],
    top: ["number"],
  } satisfies Shape<CropGeometry>,
  DocumentInfo: {
    id: ["number"],
    pages: ["array"],
    page_count: ["number"],
    lazy_geometry: ["boolean"],
    open_ms: ["number"],
    at_ms: ["number"],
  } satisfies Shape<DocumentInfo>,
  EditState: {
    pages: ["array"],
    can_undo: ["boolean"],
    can_redo: ["boolean"],
    marks: ["array"],
    redactions: ["array"],
    notes: ["array"],
    discards: ["array"],
    dirty: ["boolean"],
  } satisfies Shape<EditState>,
  Links: {
    items: ["array"],
    limits: ["object"],
    scan_ms: ["number"],
  } satisfies Shape<Links>,
  Merged: {
    changed: ["boolean"],
    pages: ["number"],
    files: ["number"],
  } satisfies Shape<Merged>,
  Outline: {
    items: ["array"],
    total: ["number"],
    limits: ["object"],
    walk_ms: ["number"],
  } satisfies Shape<Outline>,
  PageMapping: {
    composite: ["number"],
    guessing: ["number"],
    truncated: ["boolean"],
  } satisfies Shape<PageMapping>,
  PageMatches: {
    page: ["number"],
    matches: ["array"],
    chars: ["number"],
    problem: ["string"],
    tail: ["object"],
    more: ["array"],
  } satisfies Shape<PageMatches>,
  PageText: {
    codes: ["array"],
    boxes: ["array"],
    height_pt: ["number"],
    width_pt: ["number"],
    quarter_turns: ["number"],
    extract_ms: ["number"],
    runs: ["array"],
  } satisfies Shape<PageText>,
  Properties: {
    version: ["string"],
    bytes: ["number"],
    pages: ["number"],
    revisions: ["number"],
    fields: ["array"],
    encryption: ["object"],
    signatures: ["array"],
    tagged: ["boolean"],
    language: ["string"],
    attachments: ["number"],
    xmp: ["object"],
    limits: ["object"],
    scan_ms: ["number"],
  } satisfies Shape<Properties>,
  RegionPlan: {
    shows: ["array"],
    taking: ["string"],
    unhandled: ["array"],
    images: ["array"],
  } satisfies Shape<RegionPlan>,
  ScrollBenchConfig: {
    path: ["string"],
    rounds: ["number"],
    frames: ["number"],
    warmup_frames: ["number"],
    px_per_frame: ["number"],
    tile_px: ["number"],
    zooms: ["array"],
    layouts: ["array"],
    cache_tiles: ["number"],
    max_in_flight: ["number"],
    prefetch_screens: ["number"],
    cancels: ["array"],
  } satisfies Shape<ScrollBenchConfig>,
  Session: {
    places: ["array"],
    invert_pages: ["boolean"],
  } satisfies Shape<Session>,
  Split: {
    changed: ["boolean"],
    paths: ["array"],
  } satisfies Shape<Split>,
} as const;

/**
 * The samples, by the name their file is called.
 *
 * The `satisfies` on each entry is the type check: it is the assignability
 * question asked once per payload, and it fails the build rather than a test ---
 * which is right, because a mirror that no longer describes the reply is not a
 * behaviour anybody can observe at run time.
 */
const SAMPLES: Record<keyof typeof SCHEMA, Record<string, unknown>> = {
  Applied: Applied_ satisfies Widen<Applied>,
  Comments: Comments_ satisfies Widen<Comments>,
  Copied: Copied_ satisfies Widen<Copied>,
  CropGeometry: CropGeometry_ satisfies Widen<CropGeometry>,
  DocumentInfo: DocumentInfo_ satisfies Widen<DocumentInfo>,
  EditState: EditState_ satisfies Widen<EditState>,
  Links: Links_ satisfies Widen<Links>,
  Merged: Merged_ satisfies Widen<Merged>,
  Outline: Outline_ satisfies Widen<Outline>,
  PageMapping: PageMapping_ satisfies Widen<PageMapping>,
  PageMatches: PageMatches_ satisfies Widen<PageMatches>,
  PageText: PageText_ satisfies Widen<PageText>,
  Properties: Properties_ satisfies Widen<Properties>,
  RegionPlan: RegionPlan_ satisfies Widen<RegionPlan>,
  ScrollBenchConfig: ScrollBenchConfig_ satisfies Widen<ScrollBenchConfig>,
  Session: Session_ satisfies Widen<Session>,
  Split: Split_ satisfies Widen<Split>,
};

/**
 * Every command module's source, so the seventeen can be counted rather than
 * remembered.
 *
 * A glob rather than a list, because a list is the thing that goes stale: an
 * eighteenth group under `src-tauri/src/commands/` arrives here on its own, and
 * a hand-written eleventh import would not. `mod.rs` is in it and carries no
 * command, which costs nothing and is one fewer name to keep right.
 */
const COMMAND_SOURCES = import.meta.glob("../../src-tauri/src/commands/*.rs", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/**
 * Return shapes a TypeScript mirror cannot get interestingly wrong.
 *
 * The leaves that are left after `Result`, `Option` and `Vec` are peeled off. A
 * number is a number and a string is a string on both sides; what a mirror can
 * be wrong about is a *named* payload's fields, which is what the samples are
 * for. Anything reaching {@link namedPayloads} that is neither in here nor a
 * recognisable type name fails, so a new shape is a finding rather than a
 * silent omission from the list this file checks.
 */
const UNNAMED_LEAVES = new Set([
  "()",
  "String",
  "&'static str",
  "usize",
  "f64",
  "[f32; 4]",
  "[f64; 4]",
  "(String, f64)",
  "std::collections::HashMap<String, String>",
]);

/** The first top-level generic argument of `Result<A, B>`, as written. */
function okOf(text: string): string {
  const inner = text.slice("Result<".length, -1);
  let depth = 0;
  for (let i = 0; i < inner.length; i += 1) {
    const at = inner[i];
    if (at === "<" || at === "(" || at === "[") depth += 1;
    else if (at === ">" || at === ")" || at === "]") depth -= 1;
    else if (at === "," && depth === 0) return inner.slice(0, i).trim();
  }
  return inner.trim();
}

/** Every `#[tauri::command]`'s return type, as the source writes it. */
function returnTypes(): { commands: number; types: string[] } {
  let commands = 0;
  const types: string[] = [];
  for (const source of Object.values(COMMAND_SOURCES)) {
    // The signature is everything between the name and the brace that opens the
    // body, which is the only `{` that can appear before it: a parameter list
    // carries none, and a `where` clause is not used in this surface.
    const found = source.matchAll(
      /#\[tauri::command\]\n(?:#\[[^\n]*\]\n)*(?:pub )?(?:async )?fn \w+([\s\S]*?)\{\n/g,
    );
    for (const match of found) {
      commands += 1;
      const arrow = /->\s*([\s\S]+)$/.exec(match[1] ?? "");
      types.push(arrow?.[1] === undefined ? "()" : arrow[1].replace(/\s+/g, " ").trim());
    }
  }
  return { commands, types };
}

/** The named payload in `text`, or null when it is a shape with no name. */
function namedPayload(text: string): string | null {
  let leaf = text.startsWith("Result<") ? okOf(text) : text;
  for (;;) {
    const peeled = /^(?:Option|Vec)<([\s\S]*)>$/.exec(leaf);
    if (!peeled?.[1]) break;
    leaf = peeled[1].trim();
  }
  if (UNNAMED_LEAVES.has(leaf)) return null;
  const named = /^(?:[a-z_][A-Za-z0-9_]*::)*([A-Z][A-Za-z0-9_]*)$/.exec(leaf);
  if (named?.[1]) return named[1];
  throw new Error(`cannot classify the return type ${JSON.stringify(leaf)}`);
}

/**
 * Fields a sample carries that its mirror deliberately does not declare.
 *
 * One entry per field with the reason, because the alternative is a set
 * comparison that tolerates any extra key --- and "the frontend does not need
 * this one" and "the frontend has not noticed this one" are the same shape from
 * the outside. `RegionPlan` is the only such mirror today, and `pages.ts` states
 * the rule it follows: a plan's writer-only half, which the coordinator reads in
 * order to refuse a plan `lopdf` disagrees with, is nothing a panel can show.
 *
 * A name here that the sample does not carry fails, so the list cannot outlive
 * the field it excuses.
 */
const UNMIRRORED: Partial<Record<keyof typeof SCHEMA, Record<string, string>>> = {
  RegionPlan: {
    text_objects: "how many text operations, which only the writer's refusal reads",
    image_objects: "the same count for images, and the same reader",
    form_shows: "the form-level half of `shows`, addressed by (form, ordinal)",
    form_text_objects: "the form-level half of `text_objects`",
    area: "the region in the page's own space, which the writer re-derives against",
  },
};

/** What `value` is, in the vocabulary {@link SCHEMA} uses. */
function kindOf(value: unknown): Kind {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  const found = typeof value;
  if (found === "string" || found === "number" || found === "boolean") return found;
  return "object";
}

describe("the committed reply samples against the mirrors in ipc.ts", () => {
  // The refusal beneath everything else. Both tables are written by hand, and
  // two empty tables agree perfectly -- which is the shape this repository
  // records as a check that cannot fail.
  it("has a sample and a schema for every payload", () => {
    expect(Object.keys(SCHEMA).length).toBeGreaterThan(0);
    expect(Object.keys(SAMPLES).sort()).toEqual(Object.keys(SCHEMA).sort());
  });

  // Where the seventeen come from. Without this, the table above is a list
  // somebody wrote once, and the payload a new command answers with is covered
  // by nothing -- silently, which is the direction this repository records as
  // the expensive one.
  describe("the payloads the backend actually answers with", () => {
    const { commands, types } = returnTypes();

    it("reads the command modules", () => {
      // A glob that matched nothing gives an empty list of return types, and an
      // empty list agrees with any table at all.
      expect(Object.keys(COMMAND_SOURCES).length).toBeGreaterThan(0);
      expect(commands).toBeGreaterThan(0);
    });

    it("parses a signature for every command it finds", () => {
      // The regex above finds the attribute and the signature in one match, so
      // a signature it cannot read is a command it never counted. Counted the
      // other way here -- by the attribute alone -- so the two disagree when
      // the signature half stops matching.
      //
      // Anchored to the start of a line, and that is not tidiness: five of
      // these modules say `#[tauri::command]` in their prose, so a substring
      // count reads 72 where 67 commands exist and the control fails on a
      // healthy tree. `check_writers.py` records the same reading of a comment
      // as code, in the gate where it decided a security question.
      const attributes = Object.values(COMMAND_SOURCES).reduce(
        (total, source) => total + (source.match(/^#\[tauri::command\]$/gm)?.length ?? 0),
        0,
      );
      expect(commands).toBe(attributes);
    });

    it("names no payload the samples do not cover", () => {
      const named = [...new Set(types.map(namedPayload).filter((name) => name !== null))];
      // Sorted set equality, both ways: a command answering a new named type is
      // a payload with no sample, and a name in the table that no command
      // answers with is a sample nothing sends.
      expect(named.sort()).toEqual(Object.keys(SCHEMA).sort());
    });
  });

  for (const [name, shape] of Object.entries(SCHEMA)) {
    const sample = SAMPLES[name as keyof typeof SCHEMA];

    const unmirrored = UNMIRRORED[name as keyof typeof SCHEMA] ?? {};

    it(`${name} sends every key its mirror declares`, () => {
      // A key in the mirror and not the sample is a mirror describing a field
      // nothing sends -- and, because every sample is built with every optional
      // field filled, it is also how a field that stopped being sent shows up.
      const missing = Object.keys(shape).filter((key) => !(key in sample));
      expect(missing, `${name}.json`).toEqual([]);
    });

    it(`${name} sends nothing its mirror has not been told about`, () => {
      // The direction that goes stale on its own: a Rust field added without a
      // mirror arrives as `undefined` at whichever call site needed it. An
      // omission has to be declared in UNMIRRORED with a reason rather than
      // tolerated, which is the difference between a decision and an oversight.
      const strange = Object.keys(sample).filter(
        (key) => !(key in shape) && !(key in unmirrored),
      );
      expect(strange, `${name}.json`).toEqual([]);
    });

    it(`${name}'s declared omissions are fields it really sends`, () => {
      const stale = Object.keys(unmirrored).filter((key) => !(key in sample));
      expect(stale, `${name}: excused but not sent`).toEqual([]);
    });

    it(`${name} carries the JSON kinds its schema declares`, () => {
      const wrong = Object.entries(shape)
        .filter(([key]) => key in sample)
        .filter(([key, kinds]) => !(kinds as readonly Kind[]).includes(kindOf(sample[key])))
        .map(([key, kinds]) => `${key}: ${kindOf(sample[key])}, expected ${kinds.join(" or ")}`);
      expect(wrong, `${name}.json`).toEqual([]);
    });
  }
});
