/**
 * Source read as text, for the tests that hold a join no test can import.
 *
 * `App.svelte` is where the window's parts are joined, and nothing imports it:
 * a test can only read it. That is a weak instrument, and this module is where
 * its limits are written down once.
 *
 * **What a check made with it can see:** that a line is in the code, with the
 * comments taken out first. A line that was replaced and left behind in a
 * comment is then missing, which a search of the raw file would not notice.
 *
 * **What it cannot see:** whether the line runs. A line inside `if (false)`,
 * after a `return`, or in a function nothing calls is found exactly like one
 * that works. So nothing here is to stand in for a decision. A guard, an
 * order, a choice between two answers belongs in a `src/lib` function that the
 * component calls and a test calls too; what is left for a check on text is
 * that the component still hands that function its parts. Whether the parts do
 * what they should is the window checks' to say.
 *
 * Used by tests only.
 */

/** Characters after which a `/` starts a regular expression and not a division. */
const BEFORE_REGEX = "(,=:[!&|?{};+-*%<>~^";

/**
 * `source` with its comments removed: `//` to the end of the line, `/* ... *\/`
 * and, for a component's markup, `<!-- ... -->`.
 *
 * Strings, template literals and regular expressions are kept whole, so an
 * address in a string is not cut at its `//`. The code inside a template's
 * `${...}` is read as code.
 */
export function withoutComments(source: string): string {
  let out = "";
  let i = 0;
  // The brace depth each open `${` of a template was met at.
  const templates: number[] = [];
  let depth = 0;
  let last = "";

  /** Copies template text from `from`; answers where code resumes. */
  const template = (from: number): number => {
    let k = from;
    while (k < source.length) {
      if (source[k] === "\\") { k += 2; continue; }
      if (source[k] === "`") return k + 1;
      if (source.startsWith("${", k)) { templates.push(depth); return k + 2; }
      k++;
    }
    return source.length;
  };

  while (i < source.length) {
    const c = source[i] as string;
    if (source.startsWith("//", i)) {
      const end = source.indexOf("\n", i);
      i = end < 0 ? source.length : end;
      continue;
    }
    if (source.startsWith("/*", i)) {
      const end = source.indexOf("*/", i + 2);
      i = end < 0 ? source.length : end + 2;
      continue;
    }
    if (source.startsWith("<!--", i)) {
      const end = source.indexOf("-->", i + 4);
      i = end < 0 ? source.length : end + 3;
      continue;
    }
    if (c === '"' || c === "'") {
      let k = i + 1;
      while (k < source.length && source[k] !== c && source[k] !== "\n") k += source[k] === "\\" ? 2 : 1;
      out += source.slice(i, k + 1);
      i = k + 1;
      last = c;
      continue;
    }
    if (c === "`") {
      const end = template(i + 1);
      out += source.slice(i, end);
      i = end;
      last = "`";
      continue;
    }
    if (c === "/" && (last === "" || BEFORE_REGEX.includes(last))) {
      let k = i + 1;
      let inClass = false;
      while (k < source.length && source[k] !== "\n") {
        if (source[k] === "\\") { k += 2; continue; }
        if (source[k] === "[") inClass = true;
        else if (source[k] === "]") inClass = false;
        else if (source[k] === "/" && !inClass) break;
        k++;
      }
      out += source.slice(i, k + 1);
      i = k + 1;
      last = "/";
      continue;
    }
    if (c === "{") depth++;
    else if (c === "}") {
      if (templates[templates.length - 1] === depth) {
        templates.pop();
        const end = template(i + 1);
        out += source.slice(i, end);
        i = end;
        last = "`";
        continue;
      }
      depth--;
    }
    if (c.trim() !== "") last = c;
    out += c;
    i++;
  }
  return out;
}

/**
 * The code of the function that begins with `from`, up to the line that closes
 * it at `indent`, with comments removed.
 *
 * Throws when `from` is not there, or is there twice: a name that matched
 * nothing would otherwise answer the whole file, in which anything is found.
 */
export function functionIn(source: string, from: string, indent = "  "): string {
  const code = withoutComments(source);
  const start = code.indexOf(from);
  if (start < 0) throw new Error(`not in the source: ${from}`);
  if (code.indexOf(from, start + 1) >= 0) throw new Error(`in the source twice: ${from}`);
  const end = code.indexOf(`\n${indent}}`, start);
  if (end < 0) throw new Error(`no end found for: ${from}`);
  return code.slice(start, end);
}

/**
 * Which of `lines` are not in the code of `source`, each compared with the
 * white space inside it collapsed. Empty when all are there.
 *
 * A presence check and no more: see the module comment for what that is worth.
 */
export function missingFrom(source: string, lines: readonly string[]): string[] {
  const flat = (text: string) => text.replace(/\s+/g, " ").trim();
  const code = flat(withoutComments(source));
  return lines.filter((line) => !code.includes(flat(line)));
}
