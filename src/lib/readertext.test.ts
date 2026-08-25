/**
 * Three hyphens in a string a reader sees, across the whole frontend.
 *
 * WHY THIS EXISTS. Reported from use on 2026-08-25: Document properties printed
 * several literal `---`. Every one was a string carrying this repository's
 * *prose* spelling of an em dash -- the convention `AGENTS.md`, `BUILD.md` and
 * every doc comment here use, because a Markdown file is read as text. A string
 * literal is not a Markdown file, and the three hyphens are drawn as three
 * hyphens.
 *
 * Eighteen of them had shipped, in three modules: the properties readout, the
 * update notice and the after-merge message. `update.ts` is the one that shows
 * this was a slip rather than a decision -- it wrote `Downloading update —
 * ${percent}%` on line 200 with a real em dash and `tpdf ${version} ---
 * checking for updates` on line 226, twenty-six lines apart.
 *
 * ## Why the TypeScript parser rather than a regex
 *
 * The distinction that matters is *string literal or comment*, and it is not
 * one a regex can draw. The scan written first over these same files reported
 * seventy-three hits where there are nine, because a backtick inside a doc
 * comment -- `` `viewer.ts` `` is how every module here names its neighbours --
 * opens a template literal that then runs to the next backtick, swallowing the
 * prose between them. `typescript` is already a devDependency; asking the real
 * parser removes a second parser rather than improving it, which is the
 * reasoning {@link ../../README.md} check records for importing the registry
 * and `scripts/check_mutation_test_files.py` records for `vitest list --json`.
 *
 * ## Why the default is checked
 *
 * A module not named in {@link TRANSCRIPT} is checked. So a UI module added
 * tomorrow is covered by having been added, and the only way to escape is to
 * write down a reason. That is the opposite arrangement from the README check,
 * where an unclassified command is invisible and total classification is
 * therefore the only safe shape; here the safe default is free.
 *
 * ## What this does not see
 *
 * `.svelte` files, whose markup this parser cannot read. Their text is checked
 * by nothing, and the honest note is worth more than a second approximate
 * scanner: stripping HTML and CSS comments by hand is exactly the regex whose
 * failure is written up above. Today they hold no `---` outside comments.
 *
 * Nor does it see any other spelling of the same mistake -- `--` used as a
 * dash, `...` for an ellipsis where the rest of the application writes `…`.
 * Those are worth adding the day one of them is reported; guessing at them now
 * would be an inventory nobody has measured.
 */

import ts from "typescript";
import { describe, expect, it } from "vitest";

/**
 * Every non-test module under `src/lib`, by file name.
 *
 * Through Vite's `?raw` rather than a filesystem read, so this needs no Node
 * type declarations in a project that deliberately has none -- the same route
 * the README check takes.
 */
const SOURCES: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob("./*.ts", {
      query: "?raw",
      import: "default",
      eager: true,
    }) as Record<string, string>,
  )
    .map(([path, text]) => [path.replace(/^\.\//, ""), text] as const)
    .filter(([name]) => !name.endsWith(".test.ts")),
);

/** The prose spelling. What a reader must never be shown. */
const PROSE_DASH = "---";

/**
 * The value that is a separator sentinel rather than text.
 *
 * `contextmenu.ts` and `menubar.ts` both export `SEPARATOR = "---"`, and the
 * builders turn it into a real rule. Exempt by *value* rather than by module,
 * because those two modules are full of labels and exempting them wholesale
 * would leave the menu bar -- which is nothing but reader-facing text --
 * unchecked.
 */
const SENTINEL = "---";

/**
 * Modules whose strings are printed to a terminal, not drawn in the window.
 *
 * These write a check transcript that `viewer_check.py`, `session_check.py` and
 * `open_check.py` read back and that a person reads on a console. `---` is the
 * right spelling there: the repository has already paid for one encoding
 * failure in that direction, where `subprocess.run(text=True)` decoded a
 * transcript with the locale codec and died on the multilingual corpus.
 *
 * An entry naming a module that no longer exists fails. An exclusion list
 * outliving its subjects is how it rots into a blanket permission -- and unlike
 * a per-finding allowlist, an entry here is *not* stale merely because the
 * module currently holds no `---`. It states what the module's strings are for.
 */
const TRANSCRIPT: Record<string, string> = {
  "checkreport.ts": "the shared printer every unattended check writes through",
  "viewercheck.ts": "the window check's own transcript, read back by viewer_check.py",
  "sessioncheck.ts": "the session check's transcript, read back by session_check.py",
  "opencheck.ts": "the open-route check's transcript, read back by open_check.py",
  "markcheck.ts": "the mark-geometry check's transcript",
  "scrollbench.ts": "a benchmark's console output",
  "autobench.ts": "a benchmark's console output",
  "startup.ts": "the startup measurement's console output",
};

/** One string literal holding the prose dash. */
interface Finding {
  module: string;
  line: number;
  text: string;
}

/**
 * Every string literal in `text` that holds {@link PROSE_DASH}.
 *
 * Template literals are visited a piece at a time -- head, middle, tail -- which
 * is what the parser hands back and is also what is wanted: a dash in one piece
 * is a dash on screen regardless of what the interpolations around it evaluate
 * to. `node.text` is the cooked value, so an escaped form would be caught too.
 */
function findingsIn(module: string, text: string): Finding[] {
  const source = ts.createSourceFile(
    module,
    text,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const found: Finding[] = [];
  const walk = (node: ts.Node): void => {
    const literal =
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node);
    if (literal && node.text.includes(PROSE_DASH)) {
      const { line } = source.getLineAndCharacterOfPosition(node.getStart(source));
      found.push({ module, line: line + 1, text: node.text });
    }
    ts.forEachChild(node, walk);
  };
  walk(source);
  return found;
}

/**
 * Every finding in `sources` that is neither exempt module nor sentinel.
 *
 * Separate from the assertion so the *exemptions* can be tested against input
 * chosen to distinguish them. Run over the real tree they cannot be: no module
 * outside {@link TRANSCRIPT} holds a string that merely contains the sentinel,
 * so widening `===` to `includes` changes nothing and the mutation survives --
 * measured, which is why this function exists. An exemption whose strictness is
 * true by construction is not an exemption anybody has checked.
 */
function unwanted(sources: Record<string, string>): string[] {
  const bad: string[] = [];
  for (const [module, text] of Object.entries(sources)) {
    if (module in TRANSCRIPT) continue;
    for (const found of findingsIn(module, text)) {
      if (found.text === SENTINEL) continue;
      bad.push(`${module}:${found.line}: ${JSON.stringify(found.text)}`);
    }
  }
  return bad;
}

describe("text a reader is shown", () => {
  it("is read from a populated set of modules", () => {
    // The refusal that makes the rest mean anything. A glob that matched
    // nothing, or a `?raw` that handed back empty strings, passes every
    // assertion below exactly like a clean tree.
    expect(Object.keys(SOURCES).length).toBeGreaterThan(40);
    for (const [name, text] of Object.entries(SOURCES)) {
      expect(text.length, `${name} came back empty`).toBeGreaterThan(0);
    }
  });

  it("is parsed rather than pattern-matched, so a comment is not a string", () => {
    // The control for the parser itself, and for the exact failure the regex
    // version had: prose between two backticks in a doc comment.
    const sample = [
      "/** A comment --- with a dash, and `viewer.ts` --- named in backticks. */",
      'const shown = "one --- two";',
      "// A line comment --- with a dash.",
      "const built = `a ${x} --- b`;",
    ].join("\n");
    const found = findingsIn("sample.ts", sample);
    expect(found.map((f) => f.text)).toEqual(["one --- two", " --- b"]);
  });

  it("holds no prose dash outside the separator sentinel", () => {
    // Listed rather than counted: the failure a reader reports is one string,
    // and a count says nothing about which.
    expect(unwanted(SOURCES)).toEqual([]);
  });

  it("exempts the sentinel as a whole value, not as something a string contains", () => {
    // `Signature --- Signature1` was one of the eighteen shipped, and a
    // containment test would have exempted it. The real tree cannot tell the
    // two rules apart, so the input is built to.
    const synthetic = {
      "made-up.ts": ['export const SEPARATOR = "---";', 'const title = "Signature --- x";'].join(
        "\n",
      ),
    };
    expect(unwanted(synthetic)).toEqual(['made-up.ts:2: "Signature --- x"']);
  });

  it("exempts a transcript module by name, and nothing else in it", () => {
    // The other half of the pair, for the same reason: with the tree clean, a
    // table lookup and a blanket `continue` agree about every module.
    const synthetic = {
      "checkreport.ts": 'const a = "not applicable --- why";',
      "properties.ts": 'const b = "not applicable --- why";',
    };
    expect(unwanted(synthetic)).toEqual(['properties.ts:1: "not applicable --- why"']);
  });

  it("names no module in the transcript table that has gone", () => {
    const missing = Object.keys(TRANSCRIPT).filter((name) => !(name in SOURCES));
    expect(missing).toEqual([]);
  });

  it("gives every transcript module a reason, not a bare name", () => {
    for (const [name, reason] of Object.entries(TRANSCRIPT)) {
      expect(reason.length, `${name} is exempt with no reason`).toBeGreaterThan(20);
    }
  });
});
