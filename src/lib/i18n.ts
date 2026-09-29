/** UI wording only. Command ids, saved data and CLI/API reports stay locale-independent.
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
} as const;

type Message = keyof typeof EN_US;
const CATALOGS: Record<UiLocale, Record<Message, string>> = {
  "en-US": EN_US,
  "en-GB": {
    ...EN_US,
    color: "Colour",
    markColor: "Mark colour",
    invertPageColors: "Invert page colours",
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
