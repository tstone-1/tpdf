/** UI wording only. A command title that is spelled differently in the two
 * locales comes from here, and `keywords.ts` makes the palette find it under
 * the other spelling too. Prose outside the catalog is still British. Command ids, saved data and CLI/API reports stay locale-independent.
 * Add complete translated catalogs before advertising another UI language.
 * The webview exposes the user's preferred languages in preference order.
 * Locale is selected at startup; restart after changing the system language.
 */
export type UiLocale = "en-US" | "en-GB";

const EN_US = {
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
} as const;

type Message = keyof typeof EN_US;
const CATALOGS: Record<UiLocale, Record<Message, string>> = {
  "en-US": EN_US,
  "en-GB": {
    ...EN_US,
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
};

const COLOUR_REGIONS = new Set(["GB", "IE", "AU", "NZ", "CA", "ZA", "IN", "SG"]);

/** First supported language wins; unsupported or malformed preferences fall back to US English. */
export function resolveUiLocale(preferred: readonly string[]): UiLocale {
  for (const tag of preferred) {
    try {
      const locale = new Intl.Locale(tag);
      if (locale.language !== "en") continue;
      return COLOUR_REGIONS.has(locale.region ?? "US") ? "en-GB" : "en-US";
    } catch {
      // A malformed preference must not prevent the application from starting.
    }
  }
  return "en-US";
}

export const UI_LOCALE = resolveUiLocale(
  typeof navigator === "undefined" ? [] : navigator.languages?.length
    ? navigator.languages : [navigator.language],
);

export function message(key: Message, locale: UiLocale = UI_LOCALE): string {
  return CATALOGS[locale][key];
}
