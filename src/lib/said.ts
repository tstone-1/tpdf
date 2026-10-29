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

import { isGerman, UI_LOCALE, type UiLocale } from "./i18n";
import { GERMAN, SIE, type Wording } from "./said/de";

/** A template split into its literal text and its placeholders. */
export interface Template {
  /** The template exactly as Rust writes it, which is the catalog's key. */
  key: string;
  /** Placeholder names, in the order they appear. */
  names: string[];
  /**
   * The literal text around the placeholders: one more than `names`, the
   * first before the first placeholder and the last after the last.
   */
  segments: string[];
  /** How many characters of the template are literal: the tiebreak. */
  literal: number;
}

/** `{name}`, `{name:?}`, `{name:.1}`; `{{` and `}}` are literal braces. */
const PLACEHOLDER = /\{\{|\}\}|\{([A-Za-z_][A-Za-z0-9_]*)(?::[^}]*)?\}/g;

/** Parses one template. Throws on a positional `{}`, which German cannot reorder. */
export function parse(key: string): Template {
  const names: string[] = [];
  const segments = [""];
  let at = 0;
  for (const match of key.matchAll(PLACEHOLDER)) {
    segments[segments.length - 1] += key.slice(at, match.index);
    if (match[0] === "{{" || match[0] === "}}") {
      segments[segments.length - 1] += match[0][0]!;
    } else {
      names.push(match[1]!);
      segments.push("");
    }
    at = match.index + match[0].length;
  }
  segments[segments.length - 1] += key.slice(at);
  if (/\{\}|\{:/.test(key.replace(/\{\{|\}\}/g, ""))) {
    throw new Error(`positional placeholder in ${JSON.stringify(key)}: name it`);
  }
  const literal = segments.reduce((sum, segment) => sum + segment.length, 0);
  return { key, names, segments, literal };
}

/**
 * The arguments `sentence` gives `template`'s placeholders, or `null`.
 *
 * Each placeholder takes at least one character and ends at the first
 * occurrence of the literal after it. That is the split an anchored regex of
 * lazy groups finds, `^L0(.+?)L1(.+?)...Ln$`, because a later end for one
 * placeholder only starts the search for the next literal later and so can
 * never succeed where the earliest end failed. It was that regex until
 * 2026-10-28, and its backtracking took 6.6 s on a 4 KB sentence built from a
 * template's own separators. A field name comes from the document, so a
 * hostile PDF could hold the window's thread; this is linear in the sentence.
 */
function split(template: Template, sentence: string): string[] | null {
  const { segments } = template;
  const last = segments[segments.length - 1]!;
  if (segments.length === 1) return sentence === last ? [] : null;
  if (!sentence.startsWith(segments[0]!) || !sentence.endsWith(last)) return null;
  const end = sentence.length - last.length;
  const values: string[] = [];
  let start = segments[0]!.length;
  for (let i = 1; i < segments.length - 1; i += 1) {
    const at = sentence.indexOf(segments[i]!, start + 1);
    if (at < 0 || at + segments[i]!.length > end) return null;
    values.push(sentence.slice(start, at));
    start = at + segments[i]!.length;
  }
  if (end - start < 1) return null;
  values.push(sentence.slice(start, end));
  return values;
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
    const found = split(template, sentence);
    if (!found) continue;
    const args: Record<string, string> = {};
    template.names.forEach((name, i) => {
      args[name] = found[i]!;
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
  if (!isGerman(locale) || depth > 8 || sentence.length === 0) return sentence;
  const found = recognise(sentence);
  if (!found) return sentence;
  const args: Record<string, string> = {};
  for (const [name, value] of Object.entries(found.args)) args[name] = spoken(value, locale, depth + 1);
  const key = found.template.key;
  return render((locale === "de-DE-formal" ? SIE[key] : undefined) ?? GERMAN[key]!, args);
}

/** What a failed command threw, as a reader is shown it. Replaces `String(e)`. */
export function spokenError(thrown: unknown, locale: UiLocale = UI_LOCALE): string {
  const text = thrown instanceof Error ? thrown.message : String(thrown);
  return spoken(text, locale);
}
