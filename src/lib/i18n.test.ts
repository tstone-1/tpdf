import { describe, expect, it } from "vitest";
import { catalog, chosenLocale, message, readLanguageChoice, resolveUiLocale, UI_LOCALE, writeLanguageChoice } from "./i18n";
import { registerAppCommands, type AppActions } from "./appcommands";
import { CommandRegistry } from "./commands";

describe("UI language preferences", () => {
  it.each([
    [["en-US", "de-DE"], "en-US"],
    [["en-GB", "en-US"], "en-GB"],
    [["de-DE", "en-AU"], "de-DE"],
    [["en-AU", "de-DE"], "en-GB"],
    [["en-CA"], "en-GB"],
    [["en-Latn-GB"], "en-GB"],
    [["en"], "en-US"],
    [["en_US", "en-GB"], "en-GB"],
    [["de-DE", "fr-FR"], "de-DE"],
    [["de-AT"], "de-DE"],
    [["fr-FR", "de-CH", "en-GB"], "de-DE"],
    [["fr-FR", "it-IT"], "en-US"],
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
      "file.recogniseTextLanguage", "file.recogniseTextLanguage.choice", "file.recognisePage",
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
      `Recogni${us ? "z" : "s"}e this page for search`,
    ]);
    const favourite = us ? "favorite" : "favourite";
    expect([
      "nav.addFavourite", "nav.removeFavourite", "nav.nextFavourite", "nav.previousFavourite",
    ].map((id) => registry.find(id)?.title)).toEqual([
      `Add this page to ${favourite}s…`,
      `Remove this page from ${favourite}s`,
      `Next ${favourite} in this document`,
      `Previous ${favourite} in this document`,
    ]);
  });

  it("titles every command in German, under the same ids", () => {
    const actions = {
      sides: () => ({ split: false, focused: "left" as const, others: [] }),
      syncScrolling: () => false,
      pendingImport: () => null,
    } as unknown as AppActions;
    const german = new CommandRegistry();
    registerAppCommands(german, actions, "de-DE");
    const american = new CommandRegistry();
    registerAppCommands(american, actions, "en-US");
    expect(german.all().map((command) => command.id)).toEqual(american.all().map((command) => command.id));
    expect(german.find("file.save")?.title).toBe("Speichern");
    expect(german.find("view.moveToOtherSide")?.title).toBe("Tab auf die rechte Seite verschieben");
    // A title the same in both is one nobody translated. The page layouts take
    // their titles from `spreads.ts`, and a title made of a name alone is the
    // same in every language.
    const exempt = new Set(["view.singlePage", "view.facing", "view.book"]);
    const same = german.all()
      .filter((command) => !exempt.has(command.id) && command.title === american.find(command.id)?.title)
      .map((command) => `${command.id}: ${command.title}`);
    expect(german.all().length).toBeGreaterThan(150);
    expect(same).toEqual([]);
  });
});

describe("the reader's choice of language", () => {
  const store = (kept: string | null, throws = false) => {
    const written: string[] = [];
    return {
      written,
      storage: () => ({
        getItem: () => { if (throws) throw new Error("denied"); return kept; },
        setItem: (_key: string, value: string) => { if (throws) throw new Error("denied"); written.push(value); },
      }),
    };
  };

  it("reads a kept choice, and anything else as the system's", () => {
    expect(readLanguageChoice(store("de-DE").storage)).toBe("de-DE");
    expect(readLanguageChoice(store("en-GB").storage)).toBe("en-GB");
    expect(readLanguageChoice(store("de-DE-formal").storage)).toBe("de-DE-formal");
    expect(readLanguageChoice(store("fr-FR").storage)).toBe("system");
    expect(readLanguageChoice(store(null).storage)).toBe("system");
    expect(readLanguageChoice(store("de-DE", true).storage)).toBe("system");
  });

  it("writes the choice, and says when storage refused it", () => {
    const kept = store(null);
    expect(writeLanguageChoice("de-DE", kept.storage)).toBe(true);
    expect(kept.written).toEqual(["de-DE"]);
    expect(writeLanguageChoice("de-DE", store(null, true).storage)).toBe(false);
  });

  it("takes a choice over the system's languages, and the system's when the choice is", () => {
    expect(chosenLocale("de-DE", ["en-US"])).toBe("de-DE");
    expect(chosenLocale("en-US", ["de-DE"])).toBe("en-US");
    expect(chosenLocale("system", ["de-DE"])).toBe("de-DE");
    expect(chosenLocale("de-DE-formal", ["en-US"])).toBe("de-DE-formal");
  });

  it("gives a German system du, which is how the German window has spoken since it was added", () => {
    expect(chosenLocale("system", ["de-AT"])).toBe("de-DE");
  });

  it("runs the unit tests in American English, whatever the machine is set to", () => {
    expect(UI_LOCALE).toBe("en-US");
  });
});

describe("a module's own catalog", () => {
  const TEXT = catalog({
    "en-US": { saved: "Saved", pages: (n: number) => `${n} pages`, colour: "Color" },
    "en-GB": { colour: "Colour" },
    "de-DE": { saved: "Gespeichert", pages: (n: number) => `${n} Seiten`, colour: "Farbe" },
    "de-DE-formal": { saved: "Von Ihnen gespeichert" },
  });

  it("answers in the locale asked for, and in the process's own by default", () => {
    expect(TEXT().saved).toBe("Saved");
    expect(TEXT("de-DE").pages(3)).toBe("3 Seiten");
    expect(TEXT("en-GB").colour).toBe("Colour");
    // British takes what it does not respell from American.
    expect(TEXT("en-GB").pages(2)).toBe("2 pages");
    // Sie takes what does not change from du.
    expect(TEXT("de-DE-formal").saved).toBe("Von Ihnen gespeichert");
    expect(TEXT("de-DE-formal").pages(2)).toBe("2 Seiten");
    expect(TEXT("de-DE").saved).toBe("Gespeichert");
  });

  it("refuses a German half with a key missing", () => {
    catalog({
      "en-US": { saved: "Saved", closed: "Closed" },
      // @ts-expect-error "closed" is missing, and the type says so.
      "de-DE": { saved: "Gespeichert" },
    });
  });

  it("refuses a German message that takes different parameters", () => {
    catalog({
      "en-US": { pages: (n: number) => `${n} pages` },
      // @ts-expect-error a string where the American message takes a count.
      "de-DE": { pages: "Seiten" },
    });
  });
});
