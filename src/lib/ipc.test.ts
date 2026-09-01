/**
 * The command map in `ipc.ts` against the registry in `lib.rs`.
 *
 * ## Why this exists
 *
 * The command name / argument / reply contract had one home on the Rust side ---
 * `generate_handler!`, plus the `writers` and `classified` gates that read it ---
 * and none on this one: every call site restated the name and its own guess at
 * the reply. The gates on the Rust side answer *does this command exist*;
 * nothing answered *does the frontend still agree about its shape*, and
 * `docs/TRAPS.md` carries a family of incidents where it did not --- a reply
 * parsed as the wrong shape reads as absence, and absence is the reassuring
 * branch.
 *
 * `Commands` in `ipc.ts` is that home. This is what stops it becoming a second
 * list to keep: a command added to the registry and not to the map is red here,
 * and so is a map entry naming a command the registry does not carry. What it
 * cannot check is the *shapes* --- those are types, and the boundary is where
 * TypeScript stops --- so the map's argument and reply types are still read
 * against the Rust signatures by hand. Names are the half a machine can hold.
 *
 * ## How it reads both
 *
 * By source text, through Vite's `?raw`, the way `readme.test.ts` reads the
 * README and the release workflow: there is no second parser, no filesystem and
 * nothing generated. Two regexes, and each has a refusal beneath it, because a
 * regex that stopped matching produces an empty list on both sides and two empty
 * lists agree perfectly.
 *
 * The keys are plain identifiers in a plain interface for exactly this reason. A
 * computed key, a mapped type or a union spread would all be legal TypeScript
 * and none of them can be enumerated from the source text by anything simpler
 * than a TypeScript parser.
 */

import { describe, expect, it } from "vitest";

import lib from "../../src-tauri/src/lib.rs?raw";
import ipc from "./ipc.ts?raw";

/** The `generate_handler!` list, which is the set of names the backend answers. */
const HANDLER = /generate_handler!\[([\s\S]*?)\]/;

/** The body of `export interface Commands`, ending at the first unindented `}`. */
const MAP = /\nexport interface Commands \{\n([\s\S]*?)\n\}\n/;

/** One entry of that body: a two-space-indented identifier opening a brace. */
const ENTRY = /^ {2}(\w+): \{/gm;

/** Every command name in `generate_handler!`, in the order it lists them. */
function registered(): string[] {
  const body = HANDLER.exec(lib)?.[1];
  if (body === undefined) return [];
  return body
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean);
}

/** Every key of `Commands`, in the order the interface declares them. */
function mapped(): string[] {
  const body = MAP.exec(ipc)?.[1];
  if (body === undefined) return [];
  return [...body.matchAll(ENTRY)].map((match) => match[1] ?? "");
}

/** The names appearing more than once in `names`. */
function repeated(names: readonly string[]): string[] {
  return [
    ...new Set(names.filter((name) => names.indexOf(name) !== names.lastIndexOf(name))),
  ];
}

describe("the command map against the backend registry", () => {
  const backend = registered();
  const frontend = mapped();

  // The refusals. Each of these reads exactly like a clean run when it is
  // allowed to pass quietly: a regex that has stopped matching gives an empty
  // list, and two empty lists have no disagreements between them.
  it("finds the registry", () => {
    expect(backend.length, "no `generate_handler![...]` found in lib.rs").toBeGreaterThan(0);
  });

  it("finds the command map", () => {
    expect(frontend.length, "no `export interface Commands` entries found in ipc.ts").toBeGreaterThan(0);
  });

  it("reads the registry as names rather than as whatever was between the commas", () => {
    // A comment or an attribute inside the macro would arrive here as a "name",
    // and would then be reported as a command the map has forgotten -- a
    // failure naming the wrong file.
    const odd = backend.filter((name) => !/^[a-z][a-z0-9_]*$/.test(name));
    expect(odd, "not an identifier, so the registry parse is wrong").toEqual([]);
  });

  it("names each command once on each side", () => {
    expect(repeated(backend), "registered more than once").toEqual([]);
    expect(repeated(frontend), "in the command map more than once").toEqual([]);
  });

  it("maps every command the backend registers", () => {
    const missing = backend.filter((name) => !frontend.includes(name));
    expect(missing, "registered in lib.rs and absent from ipc.ts's Commands").toEqual([]);
  });

  it("maps nothing the backend does not register", () => {
    // The direction that goes stale on its own: a command removed from the
    // registry leaves a map entry that still type-checks at every call site,
    // and the call then rejects at run time with "command not found".
    const stale = frontend.filter((name) => !backend.includes(name));
    expect(stale, "in ipc.ts's Commands and not registered in lib.rs").toEqual([]);
  });

  it("keeps the map in the registry's order", () => {
    // A property the map's own doc comment claims, so it is asserted rather
    // than described: the two lists are meant to be readable side by side, and
    // an entry appended to the end of one of them is how that stops being true.
    // Asserted after the two set comparisons above, so a genuine omission is
    // reported as an omission rather than as an ordering.
    expect(frontend).toEqual(backend);
  });
});
