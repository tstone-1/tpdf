/** Rust's sentences, in the window's language.
 *
 * What the coordinator and the worker say reaches the window as an English
 * string: an `Err(String)`, a `Failure`'s `message`, a reply's `why`. The CLI
 * prints the same strings, and they cross a process boundary as text, so they
 * stay text. Rust writes each such sentence with `said!` (`src-tauri/src/said.rs`),
 * which is `format!` under a name this side can find, and this module turns the
 * finished English sentence back into its template and arguments and renders
 * the German template instead.
 *
 * This is display only. Nothing decides what to *do* from these words ---
 * `failure.rs` explains why that must stay a field --- so a sentence this
 * cannot recognise costs a reader the translation of that sentence and
 * nothing else: it is shown in English, as it was before.
 *
 * `said.test.ts` holds the catalog to the Rust source both ways, and renders
 * every template with sample arguments to prove each one is recognised as
 * itself and as no other.
 */

import { UI_LOCALE, type UiLocale } from "./i18n";
import { GERMAN, type Wording } from "./said/de";

/** A template split into its literal text and its placeholders. */
export interface Template {
  /** The template exactly as Rust writes it, which is the catalog's key. */
  key: string;
  /** Placeholder names, in the order they appear. */
  names: string[];
  /** Anchored, and each placeholder a lazy capture. */
  pattern: RegExp;
  /** How many characters of the template are literal: the tiebreak. */
  literal: number;
}

/** `{name}`, `{name:?}`, `{name:.1}`; `{{` and `}}` are literal braces. */
const PLACEHOLDER = /\{\{|\}\}|\{([A-Za-z_][A-Za-z0-9_]*)(?::[^}]*)?\}/g;

function escape(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Parses one template. Throws on a positional `{}`, which German cannot reorder. */
export function parse(key: string): Template {
  const names: string[] = [];
  let source = "^";
  let literal = 0;
  let at = 0;
  for (const match of key.matchAll(PLACEHOLDER)) {
    const before = key.slice(at, match.index);
    source += escape(before);
    literal += before.length;
    if (match[0] === "{{" || match[0] === "}}") {
      source += escape(match[0][0]!);
      literal += 1;
    } else {
      names.push(match[1]!);
      source += "([\\s\\S]+?)";
    }
    at = match.index + match[0].length;
  }
  const rest = key.slice(at);
  if (/\{\}|\{:/.test(key.replace(/\{\{|\}\}/g, ""))) {
    throw new Error(`positional placeholder in ${JSON.stringify(key)}: name it`);
  }
  source += `${escape(rest)}$`;
  literal += rest.length;
  return { key, names, pattern: new RegExp(source), literal };
}

let parsed: Template[] | null = null;

function templates(): Template[] {
  // Longest literal first, so the first match is the most specific one.
  parsed ??= Object.keys(GERMAN).map(parse).sort((a, b) => b.literal - a.literal);
  return parsed;
}

/** The template a sentence was rendered from, and its arguments; `null` for none. */
export function recognise(sentence: string): { template: Template; args: Record<string, string> } | null {
  for (const template of templates()) {
    const found = template.pattern.exec(sentence);
    if (!found) continue;
    const args: Record<string, string> = {};
    template.names.forEach((name, i) => {
      args[name] = found[i + 1]!;
    });
    return { template, args };
  }
  return null;
}

function render(wording: Wording, args: Record<string, string>): string {
  if (typeof wording === "function") return wording(args);
  return wording.replace(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (_, name: string) => args[name] ?? `{${name}}`);
}

/**
 * A sentence from Rust, in the window's language.
 *
 * Arguments are translated too, because one sentence often carries another:
 * `the redaction did not run: {e}`. English, and anything unrecognised, comes
 * back unchanged.
 */
export function spoken(sentence: string, locale: UiLocale = UI_LOCALE, depth = 0): string {
  if (locale !== "de-DE" || depth > 8 || sentence.length === 0) return sentence;
  const found = recognise(sentence);
  if (!found) return sentence;
  const args: Record<string, string> = {};
  for (const [name, value] of Object.entries(found.args)) args[name] = spoken(value, locale, depth + 1);
  return render(GERMAN[found.template.key]!, args);
}

/** What a failed command threw, as a reader is shown it. Replaces `String(e)`. */
export function spokenError(thrown: unknown, locale: UiLocale = UI_LOCALE): string {
  const text = thrown instanceof Error ? thrown.message : String(thrown);
  return spoken(text, locale);
}
