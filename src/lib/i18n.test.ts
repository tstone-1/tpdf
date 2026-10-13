import { describe, expect, it } from "vitest";
import { message, resolveUiLocale } from "./i18n";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";

describe("UI language preferences", () => {
  it.each([
    [["en-US", "de-DE"], "en-US"],
    [["en-GB", "en-US"], "en-GB"],
    [["de-DE", "en-AU"], "en-GB"],
    [["en-CA"], "en-GB"],
    [["en-Latn-GB"], "en-GB"],
    [["en"], "en-US"],
    [["en_US", "en-GB"], "en-GB"],
    [["de-DE", "fr-FR"], "en-US"],
    [[], "en-US"],
  ] as const)("resolves %j to %s", (preferences, expected) => {
    expect(resolveUiLocale(preferences)).toBe(expected);
  });

  it.each(["en-US", "en-GB"] as const)("localizes command titles but preserves command ids in %s", (locale) => {
    const registry = new CommandRegistry();
    registerAppCommands(registry, {} as AppActions, locale);
    const color = locale === "en-US" ? "Color" : "Colour";
    expect(message("color", locale)).toBe(color);
    expect(message("markColor", locale)).toBe(`Mark ${color.toLowerCase()}`);
    expect(registry.find("edit.color.green")?.title).toBe(`${color}: green`);
    expect(registry.find("view.invertPages")?.title).toBe(`Invert page ${color.toLowerCase()}s`);
    // Every other title with a word the two locales spell differently.
    const us = locale === "en-US";
    const titles = [
      "view.rotateCounterClockwise", "edit.rotatePageCounterClockwise", "edit.alignCenter",
      "edit.centerOnPage", "edit.middleOnPage", "file.recogniseText",
      "file.recogniseTextLanguage", "file.recogniseTextLanguage.choice",
    ].map((id) => registry.find(id)?.title);
    expect(titles).toEqual([
      `Rotate view ${us ? "counter" : "anti"}clockwise`,
      `Rotate page ${us ? "counter" : "anti"}clockwise`,
      `Arrange: align ${us ? "center" : "centre"}`,
      `Arrange: ${us ? "center" : "centre"} on the page horizontally`,
      `Arrange: ${us ? "center" : "centre"} on the page vertically`,
      `Recogni${us ? "z" : "s"}e text and save as...`,
      `Recogni${us ? "z" : "s"}e text: language...`,
      `Language for recogni${us ? "z" : "s"}ing text`,
    ]);
  });
});
