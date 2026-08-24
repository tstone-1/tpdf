/**
 * The public README against the command registry, in both directions.
 *
 * WHY THIS EXISTS. On 2026-08-22 an outside review compared the README with the
 * registry and found it describing a materially less capable product than the
 * binary: editing had *just begun*, *the open file is never modified in place*
 * -- false since Save in place shipped in `26.8.5` -- and the "Not built yet"
 * list still named ink, shapes, text boxes and squiggly, all four registered
 * with keyboard shortcuts. A prospective user was being told this does less
 * than it does.
 *
 * ## The two directions, and why one of them was not enough
 *
 * **Nothing claimed absent may be registered.** Each bullet under "Not built
 * yet" carries `<!-- not-built: id -->`, so claiming a feature is missing means
 * stating the absence in a form the registry can contradict. That was the whole
 * check until 2026-08-24, and it is structurally narrow: it catches a bullet
 * whose command ships *under the name the bullet guessed*. Stamps went on being
 * listed as absent after shipping as `edit.stamp.approved` and three siblings,
 * because the bullet had guessed `edit.addStamp` -- there was nothing for the
 * check to contradict, and it was green throughout.
 *
 * **Every registered command is classified.** The invariant that catches that
 * runs the other way: a command is either named in a `<!-- built: -->` marker
 * in the README's own prose, or in {@link UNLISTED} with a reason. A capability
 * cannot then arrive unmentioned by being called something nobody predicted,
 * because there is no third state to arrive in. It is the shape
 * `viewer_sweep.py` uses for fixtures and `viewercheck.ts` uses for commands.
 *
 * ## Why this is a test rather than a gate script
 *
 * `scripts/check_readme_claims.py` held the first direction and scanned
 * `appcommands.ts` for `id: "..."` with a regex. **Eleven of the seventy-seven
 * registered commands have template-literal ids** -- seven colours mapped from
 * `PALETTE` and four stamps from an inline array -- so those ids are literals
 * nowhere on disk and that scan saw none of them. Measured on 2026-08-24: it reported
 * `[OK]` on a README claiming `edit.stamp.approved` was not built, which is the
 * exact error it was written for, and it counted 66 commands against a registry
 * holding 77.
 *
 * Importing the registry removes the second parser rather than improving it.
 * That is the same reasoning `scripts/check_mutation_test_files.py` records for
 * taking test names from `vitest list --json`: a name built in a loop cannot be
 * found by grep. The README comes in through Vite's `?raw`, so neither half
 * needs a filesystem and no Node type declarations are added to a project that
 * deliberately has none.
 *
 * ## What is still not checked
 *
 * Everything in the README that is not a command: the status paragraph, the
 * measurements, the security position. There is no honest mechanical test for
 * "does this paragraph describe the product", and a keyword list approximating
 * one would be a second inventory to drift. `BUILD.md`'s release checklist
 * carries that half, and it is a checklist rather than a check on purpose --
 * naming which half is weak beats implying both are strong.
 *
 * Nor does a `built:` marker say the prose beside it is *accurate*. It says the
 * command is claimed somewhere a reader will look. A bullet describing a
 * command wrongly passes here exactly as one describing it well.
 */

import { describe, expect, it } from "vitest";

import readme from "../../README.md?raw";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";

/** The heading whose bullets claim a command does *not* exist. */
const ABSENT_SECTION = "## Not built yet";

/** `<!-- built: view.zoomIn view.zoomOut -->` */
const BUILT = /<!--\s*built:\s*([^>]*?)\s*-->/g;

/** `<!-- not-built: edit.redactSelection -->` */
const ABSENT = /<!--\s*not-built:\s*([^>]*?)\s*-->/g;

/**
 * Registered commands the README deliberately does not mention, and why.
 *
 * A reason per command rather than per group, because the groups are the part
 * that changes: `nav.goToPage` and `nav.firstPage` are excluded for the same
 * sentence today and the day one of them grows a capability worth naming is the
 * day somebody has to read its own line.
 *
 * An entry here naming a command that is no longer registered fails, in the
 * direction that matters: an exclusion list outliving its subjects is how it
 * rots into a blanket permission.
 */
const UNLISTED: Record<string, string> = {
  // Opening and reloading are what a document viewer *is*. A README bullet
  // saying tpdf can open a file would be describing the category, not the
  // product, and the Status paragraph already says what it opens.
  "file.open": "opening a document is the category, not a feature of it",
  "file.reload": "re-reading the file is the other half of opening it",
  // Application furniture. Where the version came from and where the next one
  // comes from is the Releases link at the top; a bullet for each would be the
  // menu bar transcribed.
  "app.about": "the version is a menu item, and the Releases link is what a reader wants",
  "app.checkForUpdates": "updating is described by the Releases link, not by a command",
  "app.installUpdate": "the other half of the update flow, and the same answer",
  // Moving about a document. Five commands for one idea, and naming them
  // individually would describe the palette rather than what the product does.
  // Following a *link* is different and is claimed above, because a viewer that
  // cannot is worse and a reader cannot tell from the outside.
  "nav.nextPage": "paging through a document is what a scroller does, not a feature",
  "nav.previousPage": "the same idea in the other direction",
  "nav.firstPage": "a shortcut to an end of the same scroll",
  "nav.lastPage": "the other end of it",
  "nav.goToPage": "typing a page number is the palette's argument form, covered where it is",
  // The Escape half of a selection. "Text selection and copy" above claims the
  // capability; clearing one is not a second capability.
  "edit.clearSelection": "dropping a selection is not a capability beside making one",
};

/**
 * Every command the application registers, by id.
 *
 * The actions are a proxy answering every property with a function, and that is
 * honest for this question rather than lazy: registration reads no action -- the
 * `enabled` and `run` closures are called by the palette, never here -- so a
 * stub with fifty members would suggest they are exercised. If registration
 * ever *does* read one, this returns `undefined` where a number belongs and the
 * failure is loud at the moment it changes.
 */
function registeredIds(): string[] {
  const registry = new CommandRegistry();
  const unused = new Proxy({}, { get: () => () => undefined });
  registerAppCommands(registry, unused as unknown as AppActions);
  return registry.all().map((command) => command.id);
}

/** Every id named by `pattern` in `text`, in the order they appear. */
function claims(text: string, pattern: RegExp): string[] {
  const found: string[] = [];
  for (const match of text.matchAll(pattern)) {
    const list = match[1];
    if (list !== undefined) found.push(...list.split(/\s+/).filter(Boolean));
  }
  return found;
}

/** The ids appearing more than once in `names`. */
function repeated(names: readonly string[]): string[] {
  return [...new Set(names.filter((name) => names.indexOf(name) !== names.lastIndexOf(name)))];
}

describe("the README against the command registry", () => {
  const registered = registeredIds();
  const at = readme.indexOf(ABSENT_SECTION);
  const absentSection = at === -1 ? "" : readme.slice(at).split("\n## ")[0] ?? "";
  const elsewhere = at === -1 ? readme : readme.slice(0, at);
  const builtClaims = claims(elsewhere, BUILT);
  const absentClaims = claims(absentSection, ABSENT);

  // The four refusals. Each of these reads exactly like a clean run if it is
  // allowed to pass quietly: an empty registry, a missing section and a scan
  // that found no markers all produce zero disagreements.
  it("finds the registry", () => {
    expect(registered.length).toBeGreaterThan(0);
  });

  it("finds the section that carries the absence claims", () => {
    expect(at, `README.md has no '${ABSENT_SECTION}' heading`).not.toBe(-1);
    expect(absentSection.length).toBeGreaterThan(0);
  });

  it("finds absence claims to check", () => {
    // If the markers are ever stripped -- by a rewrite, or by somebody who
    // thought they were noise -- the check has to say so rather than agree.
    expect(absentClaims.length).toBeGreaterThan(0);
  });

  it("finds built claims to check", () => {
    expect(builtClaims.length).toBeGreaterThan(0);
  });

  it("claims nothing absent that the application registers", () => {
    const shipped = absentClaims.filter((id) => registered.includes(id));
    expect(shipped, "listed under 'Not built yet' and registered").toEqual([]);
  });

  it("claims nothing built that the application does not register", () => {
    // A renamed command leaves its marker behind, and a marker naming nothing is
    // a bullet that has quietly stopped being checked.
    const stale = builtClaims.filter((id) => !registered.includes(id));
    expect(stale, "claimed built in the README and not registered").toEqual([]);
  });

  it("claims each command once, in one direction", () => {
    expect(repeated(builtClaims), "claimed built more than once").toEqual([]);
    expect(repeated(absentClaims), "claimed absent more than once").toEqual([]);
    // The third assertion **cannot be the only red**, and that was measured
    // rather than reasoned about: an id in both lists is either registered, in
    // which case the absence check fires beside it, or not, in which case the
    // stale-marker check does. Both were run. It stays because the message
    // names the mistake -- a bullet copied from one section to the other -- and
    // the two checks that fire with it name the symptom instead.
    const both = builtClaims.filter((id) => absentClaims.includes(id));
    expect(both, "claimed built and absent at once").toEqual([]);
  });

  it("keeps the absence claims out of the prose and the built claims out of the list", () => {
    // The two markers mean opposite things, so a `built:` under "Not built yet"
    // is a contradiction the section split would otherwise swallow.
    expect(claims(absentSection, BUILT), "'built:' under 'Not built yet'").toEqual([]);
    expect(claims(elsewhere, ABSENT), "'not-built:' outside 'Not built yet'").toEqual([]);
  });

  it("excludes only commands that exist", () => {
    const stale = Object.keys(UNLISTED).filter((id) => !registered.includes(id));
    expect(stale, "excluded by name and not registered").toEqual([]);
  });

  it("does not both claim and exclude a command", () => {
    const both = builtClaims.filter((id) => id in UNLISTED);
    expect(both, "claimed built in the README and excluded here").toEqual([]);
  });

  it("classifies every registered command", () => {
    // The invariant the stamp episode needed: a command is claimed in the
    // README or excluded here, and there is no third place for one to sit.
    const unclassified = registered.filter(
      (id) => !builtClaims.includes(id) && !(id in UNLISTED),
    );
    expect(
      unclassified,
      "registered, and neither claimed in the README nor excluded with a reason",
    ).toEqual([]);
  });

  it("gives every exclusion a reason worth reading", () => {
    // A one-word reason is an allowlist doing the classifying instead of a
    // person, which is the failure mode this table has rather than drift.
    for (const [id, reason] of Object.entries(UNLISTED)) {
      expect(reason.split(/\s+/).length, `${id} has a reason of `).toBeGreaterThan(4);
    }
  });
});
