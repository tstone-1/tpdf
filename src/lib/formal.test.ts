/**
 * The "Sie" German against the "du" German.
 *
 * WHY THIS EXISTS. `de-DE` addresses the reader with "du" and `de-DE-formal`
 * is an overlay on it that lists only the sentences that change. A German
 * sentence added later with "du" and no overlay entry would show "du" in the
 * window that promised "Sie", and no type sees it: the overlay is a `Partial`.
 *
 * So this reads every catalog in `src/` with the TypeScript parser, the two
 * components included, and every `said/` area with its `sie` export, and holds
 * three rules. A German entry that says "du" (a pronoun, or one of the
 * imperatives in {@link IMPERATIVES}) has a "Sie" entry. A "Sie" entry says no
 * "du" and differs from its German. A "Sie" entry names a German key.
 *
 * The imperatives are a list, so a sentence that addresses the reader only with
 * a verb not on it escapes the first rule; add the verb when one turns up.
 */
import { describe, expect, test } from "vitest";
import ts from "typescript";

const SOURCES = import.meta.glob<string>(["../**/*.ts", "../**/*.svelte", "!../**/*.test.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/**
 * The du-imperatives the German uses, lower case. Nouns spelled alike
 * ("Suche", "Sperre") are left out, and so is "gehe": "Gehe zu" is the menu
 * label German software uses whatever the form of address.
 */
const IMPERATIVES = [
  "aktiviere", "beende", "behandle", "benenne", "bestätige", "bewahre", "deaktiviere", "drücke",
  "entferne", "ergänze", "erkenne", "erstelle", "füge", "gib", "halte", "installiere",
  "klicke", "korrigiere", "lass", "lege", "lies", "markiere", "nimm", "nutze", "öffne",
  "platziere", "probiere", "prüfe", "schließe", "setze", "signiere", "speichere", "starte",
  "tippe", "verwende", "vergleiche", "versuche", "wähle", "warte", "wende", "ziehe", "zeichne",
  "mach", "verlass", "schneide", "drehe", "lösche", "widerrufe", "schwärze", "führe", "behalte", "schalte", "ändere", "zieh", "verschiebe",
];

// `dein` and its endings only: "deinstallieren" is not a pronoun.
const PRONOUN = /(?<![\p{L}])(du|dich|dir|dein|deine|deiner|deines|deinem|deinen)(?![\p{L}])/iu;
const IMPERATIVE = new RegExp(`(?<![\\p{L}])(${IMPERATIVES.join("|")})(?![\\p{L}])`, "iu");

/** Whether a wording, as source text, addresses the reader with "du". */
function saysDu(source: string): boolean {
  // A placeholder is not a word: `{dir}` is a folder.
  const words = source.replace(/\$?\{[A-Za-z_][A-Za-z0-9_]*\}/g, " ");
  return PRONOUN.test(words) || IMPERATIVE.test(words);
}

interface Pair {
  where: string;
  german: Map<string, string>;
  formal: Map<string, string>;
}

const nameOf = (node: ts.PropertyName): string =>
  ts.isIdentifier(node) || ts.isStringLiteral(node) ? node.text : node.getText();

function entries(object: ts.Expression | undefined): Map<string, string> {
  while (object && (ts.isSatisfiesExpression(object) || ts.isAsExpression(object) || ts.isParenthesizedExpression(object))) {
    object = object.expression;
  }
  const found = new Map<string, string>();
  if (!object || !ts.isObjectLiteralExpression(object)) return found;
  for (const property of object.properties) {
    if (ts.isPropertyAssignment(property)) found.set(nameOf(property.name), property.initializer.getText());
  }
  return found;
}

/** Every German half with its "Sie" half, by where it is. */
function pairs(): Pair[] {
  const out: Pair[] = [];
  for (const [path, raw] of Object.entries(SOURCES)) {
    const script = path.endsWith(".svelte") ? (raw.match(/<script lang="ts">([\s\S]*?)<\/script>/)?.[1] ?? "") : raw;
    const source = ts.createSourceFile(path, script, ts.ScriptTarget.Latest, true);
    const said = /\/said\/(?!de\.ts$)[^/]+\.ts$/.test(path);
    let areaGerman: Map<string, string> | undefined;
    let areaFormal = new Map<string, string>();
    const visit = (node: ts.Node): void => {
      if (ts.isCallExpression(node) && node.expression.getText() === "catalog") {
        const halves = new Map<string, ts.Expression>();
        const argument = node.arguments[0];
        if (argument && ts.isObjectLiteralExpression(argument)) {
          for (const property of argument.properties) {
            if (ts.isPropertyAssignment(property)) halves.set(nameOf(property.name), property.initializer);
          }
        }
        const line = source.getLineAndCharacterOfPosition(node.getStart()).line + 1;
        out.push({ where: `${path}:${line}`, german: entries(halves.get("de-DE")), formal: entries(halves.get("de-DE-formal")) });
      }
      if (said && ts.isExportAssignment(node)) areaGerman = entries(node.expression);
      if (said && ts.isVariableStatement(node) && node.modifiers?.some((m) => m.kind === ts.SyntaxKind.ExportKeyword)) {
        for (const declaration of node.declarationList.declarations) {
          if (declaration.name.getText() === "sie") areaFormal = entries(declaration.initializer);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
    if (areaGerman) out.push({ where: path, german: areaGerman, formal: areaFormal });
  }
  return out;
}

describe("German with Sie", () => {
  const all = pairs();

  test("the parser finds the catalogs and the said areas", () => {
    // Without these the rules below pass on nothing.
    expect(all.filter((pair) => pair.where.includes("/said/")).length).toBeGreaterThan(5);
    expect(all.some((pair) => pair.where.includes("App.svelte"))).toBe(true);
    expect(all.filter((pair) => [...pair.german.values()].some(saysDu)).length).toBeGreaterThan(20);
  });

  test("every German entry that says du has a Sie entry", () => {
    const missing = all.flatMap((pair) =>
      [...pair.german].filter(([key, text]) => saysDu(text) && !pair.formal.has(key))
        .map(([key, text]) => `${pair.where} ${key}: ${text.slice(0, 100)}`));
    expect(missing, missing.join("\n")).toEqual([]);
  });

  test("a Sie entry says no du, differs from its German, and names a German key", () => {
    const wrong = all.flatMap((pair) =>
      [...pair.formal].flatMap(([key, text]) => {
        const german = pair.german.get(key);
        if (german === undefined) return [`${pair.where} ${key}: no German entry`];
        if (saysDu(text)) return [`${pair.where} ${key}: still says du: ${text.slice(0, 100)}`];
        if (text === german) return [`${pair.where} ${key}: the same as the German`];
        return [];
      }));
    expect(wrong, wrong.join("\n")).toEqual([]);
  });
});
