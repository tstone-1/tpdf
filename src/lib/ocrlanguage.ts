/**
 * The language text is recognised in: what the reader can choose between, what
 * a typed answer means, and the sentences said about it.
 *
 * `App.svelte` keeps the `invoke`s; the choice, the list being asked about and
 * every word shown are here, so they have tests. The list comes from
 * `ocr_languages` in `src-tauri/src/commands/ocr.rs`, the choice is kept by
 * `session_set_ocr_language`, and *Recognise text* sends it with `ocr_copy`,
 * where `ocr_layer::choose` holds it against what the machine offers then.
 *
 * One language, or the engine's own choice. The command-line tool takes
 * several; a window that asked for an ordered list would be asking a question
 * most readers cannot answer.
 */

/** What the machine offers. `Offered` in `commands/ocr.rs`. */
export interface Offered {
  /** BCP-47 tags, in the engine's own spelling and order. */
  languages: string[];
  /** Whether more can be installed, which is Windows. */
  installable: boolean;
}

/** The word for the engine's own choice, as shown and as typed. */
export const AUTOMATIC = "Automatic";

/** Where to go after a language has gone missing. `appcommands.ts` has the title. */
export const CHOOSE_AGAIN = 'Choose another with "Recognise text: language...".';

/** Where a Windows reader adds a language. */
export const INSTALL_MORE =
  "More can be installed in Windows Settings, under Time & language, then Language & region.";

/**
 * `German (Germany)` for `de-DE`, or the tag when it has no name.
 *
 * The name is the webview's, in English like the rest of the window, and not
 * the platform's: one spelling on both systems, and no second list to keep. A
 * tag the webview cannot name is shown as it is, which is what Vision's own
 * `vi-VT` gets on some systems.
 */
export function nameOf(tag: string): string {
  try {
    const name = new Intl.DisplayNames(["en"], {
      type: "language",
      // `English (United States)`, not `American English`: the region reads
      // the same way for every language in the list.
      languageDisplay: "standard",
    }).of(tag);
    return name && name !== tag ? name : tag;
  } catch {
    return tag;
  }
}

/** `German (Germany), de-DE`, `de-DE` for a tag with no name, or `Automatic`. */
export function label(language: string | null): string {
  if (language === null) return AUTOMATIC;
  const name = nameOf(language);
  return name === language ? language : `${name}, ${language}`;
}

/** What a typed answer chose, or why it chose nothing. */
export type Picked = { language: string | null; problem?: undefined } | { problem: string };

/** `a, b and 3 more`: thirty tags must not fill the palette. */
function some(tags: string[]): string {
  const SHOWN = 12;
  const rest = tags.length - SHOWN;
  const head = tags.slice(0, SHOWN).join(", ");
  return rest > 0 ? `${head} and ${rest} more` : head;
}

/** What to say about the list itself, after a problem. */
function offering(offered: Offered): string {
  const said =
    offered.languages.length === 0
      ? "This computer offers no language to choose."
      : `This computer offers ${some(offered.languages)}.`;
  return offered.installable ? `${said} ${INSTALL_MORE}` : said;
}

/**
 * The language a typed answer names.
 *
 * A tag or a name in full is that language whatever else begins the same way;
 * otherwise the answer has to be the beginning of exactly one tag, name or
 * `automatic`. So `de` is German where `de-DE` is the only tag that starts so,
 * and `ch` is refused where two kinds of Chinese are offered, with both named.
 * Nothing is guessed: a wrong language makes every page read worse and says
 * nothing while it does.
 */
export function pick(raw: string, offered: Offered): Picked {
  const typed = raw.trim().toLowerCase();
  if (typed === "") {
    return { problem: `Type a language, or ${AUTOMATIC.toLowerCase()}. ${offering(offered)}` };
  }
  const choices: { language: string | null; words: string[] }[] = [
    { language: null, words: [AUTOMATIC.toLowerCase()] },
    ...offered.languages.map((tag) => ({
      language: tag,
      words: [tag.toLowerCase(), nameOf(tag).toLowerCase()],
    })),
  ];
  const whole = choices.find((choice) => choice.words.includes(typed));
  if (whole) return { language: whole.language };
  const begun = choices.filter((choice) => choice.words.some((word) => word.startsWith(typed)));
  const [only] = begun;
  if (only && begun.length === 1) return { language: only.language };
  if (begun.length > 1) {
    return {
      problem: `Several begin with "${raw.trim()}": ${begun
        .slice(0, 6)
        .map((choice) => label(choice.language))
        .join("; ")}${begun.length > 6 ? "; ..." : ""}`,
    };
  }
  return { problem: `No language here begins with "${raw.trim()}". ${offering(offered)}` };
}

/** What the palette's input shows before anything is typed. */
export function placeholder(current: string | null): string {
  return `Language, such as de-DE or German, or automatic (now: ${label(current)})`;
}

/** What choosing will do, shown while a usable answer is in the input. */
export function preview(language: string | null): string {
  return language === null
    ? "Let the recogniser choose the language"
    : `Recognise text as ${label(language)}`;
}

/** What is said once the choice is made. */
export function chosen(language: string | null): string {
  return language === null
    ? "Text will be recognised in the language the recogniser chooses."
    : `Text will be recognised as ${label(language)}.`;
}

/**
 * What is said after a recognition whose language the machine no longer had.
 *
 * `language` is `Recognised.languageUnavailable`. The copy is written and its
 * text may be poor, so the reader is told which language was missing and which
 * command chooses another; that command says where one is installed.
 */
export function fellBack(language: string): string {
  return (
    `${label(language)} is not available on this computer, ` +
    `so the recogniser chose the language itself. ${CHOOSE_AGAIN}`
  );
}

/**
 * The reader's choice, and the list held while the palette asks about it.
 *
 * The list is fetched when the question is asked and dropped when it is
 * answered or dismissed, so it is never older than the question: a language
 * installed a minute ago is in the next one.
 */
export class RecognitionLanguage {
  /** The chosen tag, or null for the engine's own choice. */
  language: string | null = null;
  private asking: Offered | null = null;

  /** What the session file remembered. Anything but a string is no choice. */
  restore(stored: unknown): void {
    this.language = typeof stored === "string" && stored !== "" ? stored : null;
  }

  /** Holds the list the palette is about to ask about. */
  hold(offered: Offered): void {
    this.asking = offered;
  }

  /** The list being asked about, or null when nothing is being asked. */
  question(): Offered | null {
    return this.asking;
  }

  /** The palette stopped asking. */
  drop(): void {
    this.asking = null;
  }

  /**
   * Takes the reader's answer: the sentence to say, or null when there was no
   * question or the answer names nothing. The question is over either way.
   */
  answer(raw: string): string | null {
    const offered = this.asking;
    this.asking = null;
    if (!offered) return null;
    const picked = pick(raw, offered);
    if (picked.problem !== undefined) return null;
    this.language = picked.language;
    return chosen(picked.language);
  }
}
