/** Byte counts as the window prints them. */
import { catalog, UI_LOCALE, type UiLocale } from "./i18n";

const TEXT = catalog({
  "en-US": { unknown: "unknown", bytes: (count: string) => `${count} bytes`, number: "en-US" },
  "de-DE": { unknown: "unbekannt", bytes: (count: string) => `${count} Bytes`, number: "de-DE" },
});

/** A byte count, with a rounded size beside it once it is worth having. */
export function formatBytes(bytes: number, locale: UiLocale = UI_LOCALE): string {
  const text = TEXT(locale);
  if (!Number.isFinite(bytes) || bytes < 0) return text.unknown;
  const exact = text.bytes(Math.round(bytes).toLocaleString(text.number));
  if (bytes < 1024) return exact;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  // On the figure as it will be printed and not as it is: 1023.6 KB prints as
  // 1024 KB, which is 1.0 MB. The last unit has nowhere to go and keeps it.
  while (Math.round(value) >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // One decimal below ten, none above: 8.7 MB is worth the digit and 873 KB is
  // not, and "873.4 KB" beside an exact byte count is two spurious digits.
  // Decided on the rounded figure for the same reason as above: 9.96 with one
  // decimal is 10.0, which is not below ten.
  const tenths = value.toFixed(1);
  const rounded = Number(tenths) < 10 ? tenths : Math.round(value).toString();
  // German writes the decimal with a comma: 8,7 MB.
  const shown = text.number === "de-DE" ? rounded.replace(".", ",") : rounded;
  return `${shown} ${units[unit]} (${exact})`;
}
