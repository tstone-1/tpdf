/**
 * Every command id `docs/PLAN.md` names, against the command registry.
 *
 * WHY THIS EXISTS. On 2026-08-26 a sweep of every live `**Not done:**` note in
 * `docs/PLAN.md` found five that were false --- reordering pages by dragging a
 * thumbnail (built 2026-08-17), ink and an ellipse (both 2026-08-20), and two
 * separate sentences saying stamps were what remained (built 2026-08-23). Each
 * closing commit has the capability in its own subject line. The trap
 * *"A Not done note outlives the work that closes it"* had already prescribed
 * the grep that finds them; nothing asked, so nobody ran it.
 *
 * ## What this checks, and what it deliberately does not
 *
 * **Every command id the plan names must be registered.** The plan is prose
 * about capabilities, and when it reaches for an id it is making a checkable
 * claim: that a command by that name exists. A rename leaves those mentions
 * behind, and `PLAN.md:8800` records that renames happen here --- it discusses
 * `edit.cropToRectangle` and `edit.addStamp`, two predicted names, neither of
 * which shipped.
 *
 * **The other direction is not built, and the reason is worth stating rather
 * than leaving as an omission.** The obvious mirror is the one `readme.test.ts`
 * uses: a `<!-- not-built: id -->` marker on each absence claim, asserted not to
 * be registered. It was proposed, and dropped after reading the notes: **no live
 * `**Not done:**` in this file names a command id.** Inserting pages
 * (`PLAN.md:9296`) and split/merge (`:5903`) describe machinery, not commands.
 * So every marker would be a guess at a name nobody has chosen, and a guessed
 * name is exactly what makes such a marker green and useless --- the README's
 * bullet guessed `edit.addStamp` and stayed green through four stamp commands
 * shipping as `edit.stamp.*`. Writing guesses into the roadmap so a check can
 * agree with them is worse than not checking.
 *
 * Nor does this say a *sentence* about a command is accurate. It says the id
 * exists. A paragraph describing `edit.drawBox` wrongly passes here exactly as
 * one describing it well, and the five stale notes that motivated this would
 * **not** have been caught by it --- they name capabilities in English. That
 * half has no honest mechanical test, and naming which half is weak beats
 * implying both are strong.
 *
 * ## Why a test rather than a gate script
 *
 * The same reason `readme.test.ts` gives: eleven of the registered commands
 * have template-literal ids --- seven colours mapped from `PALETTE`, four stamps
 * from an inline array --- so they are literals nowhere on disk and a regex over
 * `appcommands.ts` cannot see them. Importing the registry removes the second
 * parser rather than improving it. `PLAN.md` arrives through Vite's `?raw`, so
 * neither half needs a filesystem.
 */

import { describe, expect, it } from "vitest";

import plan from "../../docs/PLAN.md?raw";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";

/**
 * Anything shaped like a command id: a known namespace, a dot, a name.
 *
 * Namespaces are listed rather than matched as `\w+` so that ordinary prose
 * containing a dotted word does not become a subject. It still over-matches ---
 * see {@link NOT_COMMANDS} --- and that is the right direction for a scan whose
 * failure mode should be a question rather than a silence.
 */
const ID_SHAPED = /\b(?:app|edit|file|find|nav|view)\.[a-zA-Z][a-zA-Z0-9.]*/g;

/**
 * Id-shaped strings in the plan that are not claims about a registered command.
 *
 * A reason each, and an entry naming a string that no longer appears in the
 * plan fails --- an exclusion list outliving its subjects is how it rots into a
 * blanket permission. That is the same rule `readme.test.ts`'s `UNLISTED`
 * carries and for the same reason.
 */
const NOT_COMMANDS: Record<string, string> = {
  // `PLAN.md:8800` discusses these two by name as names that were predicted and
  // not used: "named `edit.cropToRectangle` and `edit.addStamp`; what shipped
  // is ...". Excluding them is not excusing staleness --- the sentence is about
  // their absence, and it is correct.
  "edit.cropToRectangle": "named at :8800 as a predicted id that did not ship; the prose says so",
  "edit.addStamp": "the other half of that sentence, and what shipped is edit.stamp.*",
  // The third of the same kind, and the reason this table is a table rather
  // than two lines: *Splitting a document* records that the README's own
  // `not-built` marker guessed this name while the command shipped as
  // `file.splitDocument`, which is why the absence direction stayed green. The
  // prose is about the guess, so the guess has to be sayable.
  "edit.splitDocument": "named in the split section as the guess the README made and nothing used",
  // The fourth, and the only one naming an id the README still claims as
  // *not-built*. The plan's sentence is about that claim standing: the model
  // and both writers can edit a foreign comment, and nothing in the palette
  // can reach them, so the marker is correct and the id is deliberately
  // unregistered. The day a command by that name ships, this line fails and
  // says so.
  "edit.editForeignMark":
    "named as the README's still-standing not-built claim; the prose is about its absence",
  // A placeholder filename inside a command line: `TPDF_AUTOBENCH=<file.pdf>`
  // at :450 and `cupsfilter -d <queue> file.pdf` at :4349. The pattern cannot
  // tell a filename from an id without knowing every id, which is the thing it
  // is checking.
  "file.pdf": "a placeholder filename in two shell command lines, not an id",
};

/**
 * Every command the application registers, by id.
 *
 * The actions are a proxy answering every property with a function, which is
 * honest for this question: registration reads no action, because `enabled` and
 * `run` are called by the palette and never here. If registration ever does
 * read one, this returns `undefined` where a value belongs and fails loudly at
 * the moment it changes.
 */
function registeredIds(): string[] {
  const registry = new CommandRegistry();
  const unused = new Proxy({}, { get: () => () => undefined });
  registerAppCommands(registry, unused as unknown as AppActions);
  return registry.all().map((command) => command.id);
}

/**
 * Id-shaped strings in `text`, split by what kind of claim they make.
 *
 * A mention ending in a dot is a **family**, not a mangled id: the plan writes
 * `edit.stamp.{approved,confidential,draft,final}` and `edit.color.*` to name a
 * group built by one `map`. That is still a checkable claim -- some command in
 * that family exists -- and checking it that way keeps a renamed family red,
 * where excluding the prefix would make it invisible.
 */
function mentioned(text: string): { exact: string[]; families: string[] } {
  const raw = [...new Set([...text.matchAll(ID_SHAPED)].map((match) => match[0]))];
  return {
    exact: raw.filter((name) => !name.endsWith(".")),
    families: [...new Set(raw.filter((name) => name.endsWith(".")).map((name) => name.slice(0, -1)))],
  };
}

describe("PLAN.md against the command registry", () => {
  const registered = new Set(registeredIds());
  const { exact, families } = mentioned(plan);
  const claims = exact.filter((name) => !(name in NOT_COMMANDS));

  // The refusals. Each of these reads exactly like a clean run if it passes
  // quietly: an empty registry, an unreadable plan and a scan that matched
  // nothing all produce zero disagreements.
  it("finds the registry", () => {
    expect(registered.size).toBeGreaterThan(0);
  });

  it("finds the plan", () => {
    expect(plan.length).toBeGreaterThan(0);
    expect(plan, "PLAN.md does not look like the plan").toContain("**Not done:**");
  });

  it("finds command ids in the plan to check", () => {
    // If the plan is ever rewritten to stop naming ids, this check has nothing
    // to say and must report that rather than agreeing with everything.
    expect(claims.length).toBeGreaterThan(0);
  });

  it("names only commands that exist", () => {
    const missing = claims.filter((id) => !registered.has(id));
    expect(
      missing,
      `PLAN.md names ${missing.length} command id(s) that are not registered. ` +
        "Either the command was renamed and the plan was left behind, or the id " +
        "was predicted and never shipped -- in which case say so in the prose and " +
        "add it to NOT_COMMANDS with that reason.",
    ).toEqual([]);
  });

  it("names only command families that exist", () => {
    // `edit.color.*` is a claim that the family is there, and it stays red
    // through a rename of the family even though no single id is named.
    const empty = families.filter(
      (prefix) => ![...registered].some((id) => id.startsWith(`${prefix}.`)),
    );
    expect(
      empty,
      `PLAN.md names ${empty.length} command famil(ies) with no registered member. ` +
        "A family built by one `map` was renamed, and the plan was left behind.",
    ).toEqual([]);
  });

  it("finds command families to check", () => {
    expect(families.length).toBeGreaterThan(0);
  });

  it("keeps no exclusion for a string the plan no longer contains", () => {
    const orphans = Object.keys(NOT_COMMANDS).filter((id) => !exact.includes(id));
    expect(
      orphans,
      "NOT_COMMANDS entries naming strings absent from PLAN.md. An exclusion " +
        "that outlives its subject is a blanket permission nobody reads.",
    ).toEqual([]);
  });
});
