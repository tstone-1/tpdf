/** UI wording. Every string the window shows comes from a catalog: the shared
 * one below, for titles more than one module uses, or a module's own, made with
 * {@link catalog} beside the code that shows it. Command ids, saved data and
 * CLI/API reports stay locale-independent, and so do messages the worker writes
 * (phase 2 of German: until then they reach the window in English).
 *
 * A catalog's German half has to have every key its American half has, with the
 * same parameters; the type refuses one that does not, so a string added in
 * English cannot ship untranslated. British English lists only what it spells
 * differently and takes the rest from American.
 *
 * The language is chosen at startup: the reader's choice, kept under
 * {@link LANGUAGE_KEY}, or else the system's preferred languages in order.
 * Changing either takes a restart. The unit tests and the checks build always
 * run in American English.
 */
export type UiLocale = "en-US" | "en-GB" | "de-DE";

/** Every locale, in the order the Language setting offers them. */
export const UI_LOCALES: readonly UiLocale[] = ["en-US", "en-GB", "de-DE"];

/** A message: plain text, or worded from what it is about. */
type Text = string | ((...args: never[]) => string);

/** The American half of a catalog, which fixes its keys and parameters. */
export type Catalog = Record<string, Text>;

/** The same keys with the same parameters, which is what a translation supplies. */
export type Translation<T extends Catalog> = { [K in keyof T]: T[K] extends string ? string : T[K] };

/**
 * A module's own messages, in every locale.
 *
 * Returns a function rather than the messages, so that a test can ask for a
 * locale other than the one the process started in: `TEXT().saved`, or
 * `TEXT("de-DE").saved`. Each call is a lookup and builds nothing.
 */
export function catalog<const T extends Catalog>(texts: {
  "en-US": T;
  "de-DE": Translation<T>;
  "en-GB"?: Partial<Translation<T>>;
}): (locale?: UiLocale) => Translation<T> {
  const british = { ...texts["en-US"], ...texts["en-GB"] } as Translation<T>;
  const by: Record<UiLocale, Translation<T>> = {
    "en-US": texts["en-US"] as Translation<T>,
    "en-GB": british,
    "de-DE": texts["de-DE"],
  };
  return (locale = UI_LOCALE) => by[locale];
}

const SHARED = catalog({
  "en-US": {
    color: "Color",
    markColor: "Mark color",
    invertPageColors: "Invert page colors",
    defaultColorHint: "Default: yellow highlights and comments, red drawing marks",
    rotateViewCounter: "Rotate view counterclockwise",
    rotatePageCounter: "Rotate page counterclockwise",
    alignCenter: "Arrange: align center",
    centerOnPageAcross: "Arrange: center on the page horizontally",
    centerOnPageDown: "Arrange: center on the page vertically",
    recogniseText: "Recognize text and save as...",
    recogniseLanguage: "Recognize text: language...",
    recogniseLanguageChoice: "Language for recognizing text",
    recogniseAs: "Recognize text as",
    stopRecognising: "Stop recognizing text",
    recognisePage: "Recognize this page for search",
    recognisePageButton: "Recognize this page",
    recognisePageHint: "Can you see a word that search misses? Recognize this page without changing the PDF.",
    recogniseScanHint: "No page has text. Recognize every page and save a copy that can be searched.",
    recognisingPage: "Recognizing this page...",
    recognisedPage: "Recognized this page for search. The PDF is unchanged. Recognition lasts until you edit or leave this view; word highlights are approximate.",
  },
  "en-GB": {
    color: "Colour",
    markColor: "Mark colour",
    invertPageColors: "Invert page colours",
    rotateViewCounter: "Rotate view anticlockwise",
    rotatePageCounter: "Rotate page anticlockwise",
    alignCenter: "Arrange: align centre",
    centerOnPageAcross: "Arrange: centre on the page horizontally",
    centerOnPageDown: "Arrange: centre on the page vertically",
    recogniseText: "Recognise text and save as...",
    recogniseLanguage: "Recognise text: language...",
    recogniseLanguageChoice: "Language for recognising text",
    recogniseAs: "Recognise text as",
    stopRecognising: "Stop recognising text",
    recognisePage: "Recognise this page for search",
    recognisePageButton: "Recognise this page",
    recognisePageHint: "Can you see a word that search misses? Recognise this page without changing the PDF.",
    recogniseScanHint: "No page has text. Recognise every page and save a copy that can be searched.",
    recognisingPage: "Recognising this page...",
    recognisedPage: "Recognised this page for search. The PDF is unchanged. Recognition lasts until you edit or leave this view; word highlights are approximate.",
  },
  "de-DE": {
    color: "Farbe",
    markColor: "Markierungsfarbe",
    invertPageColors: "Seitenfarben umkehren",
    defaultColorHint: "Standard: gelbe Hervorhebungen und Kommentare, rote Zeichnungen",
    rotateViewCounter: "Ansicht gegen den Uhrzeigersinn drehen",
    rotatePageCounter: "Seite gegen den Uhrzeigersinn drehen",
    alignCenter: "Anordnen: zentriert ausrichten",
    centerOnPageAcross: "Anordnen: auf der Seite horizontal zentrieren",
    centerOnPageDown: "Anordnen: auf der Seite vertikal zentrieren",
    recogniseText: "Text erkennen und speichern unter...",
    recogniseLanguage: "Texterkennung: Sprache...",
    recogniseLanguageChoice: "Sprache für die Texterkennung",
    recogniseAs: "Text erkennen als",
    stopRecognising: "Texterkennung abbrechen",
    recognisePage: "Diese Seite für die Suche erkennen",
    recognisePageButton: "Diese Seite erkennen",
    recognisePageHint: "Siehst du ein Wort, das die Suche nicht findet? Erkenne diese Seite, ohne das PDF zu ändern.",
    recogniseScanHint: "Keine Seite enthält Text. Erkenne alle Seiten und speichere eine durchsuchbare Kopie.",
    recognisingPage: "Diese Seite wird erkannt...",
    recognisedPage: "Diese Seite ist jetzt durchsuchbar. Das PDF ist unverändert. Die Erkennung gilt, bis du etwas bearbeitest oder diese Ansicht verlässt; Wortmarkierungen sind ungefähr.",
  },
});

type Message = keyof ReturnType<typeof SHARED>;

const COLOUR_REGIONS = new Set(["GB", "IE", "AU", "NZ", "CA", "ZA", "IN", "SG"]);

/** First supported language wins; unsupported or malformed preferences fall back to US English. */
export function resolveUiLocale(preferred: readonly string[]): UiLocale {
  for (const tag of preferred) {
    try {
      const locale = new Intl.Locale(tag);
      if (locale.language === "de") return "de-DE";
      if (locale.language !== "en") continue;
      return COLOUR_REGIONS.has(locale.region ?? "US") ? "en-GB" : "en-US";
    } catch {
      // A malformed preference must not prevent the application from starting.
    }
  }
  return "en-US";
}

/** The reader's choice of language: one locale, or the system's. */
export type LanguageChoice = UiLocale | "system";

/** Where the choice is kept. */
export const LANGUAGE_KEY = "tpdf.ui-language";

type Store = Pick<Storage, "getItem" | "setItem">;

function isChoice(value: unknown): value is LanguageChoice {
  return value === "system" || UI_LOCALES.includes(value as UiLocale);
}

/** The kept choice, or "system" when there is none or storage cannot be read. */
export function readLanguageChoice(storage: () => Store = () => window.localStorage): LanguageChoice {
  try {
    const kept = storage().getItem(LANGUAGE_KEY);
    return isChoice(kept) ? kept : "system";
  } catch {
    return "system";
  }
}

/** Keeps the choice; `false` when storage refused it. */
export function writeLanguageChoice(
  choice: LanguageChoice,
  storage: () => Store = () => window.localStorage,
): boolean {
  try {
    storage().setItem(LANGUAGE_KEY, choice);
    return true;
  } catch {
    return false;
  }
}

/** The locale a choice and the system's preferred languages come to. */
export function chosenLocale(choice: LanguageChoice, preferred: readonly string[]): UiLocale {
  return choice === "system" ? resolveUiLocale(preferred) : choice;
}

function startupLocale(): UiLocale {
  // The unit tests assert English wording, so they run in American English
  // whatever the machine running them is set to.
  if (import.meta.env.MODE === "test") return "en-US";
  // So do the window checks, for the same reason and one more: many of them
  // find a control by its English words on purpose, so that a check cannot
  // agree with the code it checks by reading the same catalog.
  if (__TPDF_CHECKS__) return "en-US";
  const preferred = typeof navigator === "undefined" ? [] : navigator.languages?.length
    ? navigator.languages : [navigator.language];
  return chosenLocale(typeof window === "undefined" ? "system" : readLanguageChoice(), preferred);
}

export const UI_LOCALE: UiLocale = startupLocale();

/** A message from the shared catalog. */
export function message(key: Message, locale: UiLocale = UI_LOCALE): string {
  return SHARED(locale)[key];
}

/** Each locale's name in its own language, for the Language setting. */
export const LOCALE_NAMES: Record<UiLocale, string> = {
  "en-US": "English (US)",
  "en-GB": "English (UK)",
  "de-DE": "Deutsch",
};
