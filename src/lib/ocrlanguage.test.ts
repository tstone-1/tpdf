/**
 * What a typed language means, the sentences about it, and the choice held
 * while it is asked.
 */

import { describe, expect, it } from "vitest";

import app from "../App.svelte?raw";
import { functionIn, missingFrom } from "./sourcetext";
import commands from "./appcommands.ts?raw";
import {
  AUTOMATIC,
  CHOOSE_AGAIN,
  chosen,
  fellBack,
  INSTALL_MORE,
  label,
  nameOf,
  pick,
  placeholder,
  preview,
  RecognitionLanguage,
  type Offered,
} from "./ocrlanguage";

/** What Vision offers, shortened, with its two kinds of Chinese. */
const mac: Offered = {
  languages: ["en-US", "fr-FR", "de-DE", "zh-Hans", "zh-Hant", "da-DK"],
  installable: false,
};
/** A stock Windows install. */
const windows: Offered = { languages: ["en-US"], installable: true };

describe("the name of a language", () => {
  it("is the language and its region in English, beside the tag", () => {
    expect(nameOf("de-DE")).toBe("German (Germany)");
    expect(nameOf("en-US")).toBe("English (United States)");
    expect(label("de-DE")).toBe("German (Germany), de-DE");
  });

  it("is the tag alone for one that cannot be named", () => {
    // Not a tag at all, which `Intl.DisplayNames` refuses by throwing.
    expect(nameOf("not a tag")).toBe("not a tag");
    expect(label("not a tag")).toBe("not a tag");
  });

  it("is Automatic for no language", () => {
    expect(label(null)).toBe(AUTOMATIC);
  });
});

describe("pick", () => {
  it("takes a tag in any case, and in full before anything that begins the same", () => {
    expect(pick("de-DE", mac)).toEqual({ language: "de-DE" });
    expect(pick("  DE-de ", mac)).toEqual({ language: "de-DE" });
    // A list in which one tag begins another: `nb` in full is `nb`, where as
    // a beginning it would be two languages and never choosable.
    const nested: Offered = { languages: ["nb", "nb-NO"], installable: false };
    expect(pick("nb", nested)).toEqual({ language: "nb" });
    expect(pick("nb-", nested)).toEqual({ language: "nb-NO" });
  });

  it("takes a name, and the beginning of one tag or name", () => {
    expect(pick("German (Germany)", mac)).toEqual({ language: "de-DE" });
    expect(pick("germ", mac)).toEqual({ language: "de-DE" });
    expect(pick("fr", mac)).toEqual({ language: "fr-FR" });
  });

  it("takes automatic as the recogniser's own choice", () => {
    expect(pick("automatic", mac)).toEqual({ language: null });
    expect(pick("Auto", mac)).toEqual({ language: null });
  });

  it("refuses a beginning that several share, and names them", () => {
    // `d`: `de-DE` by its tag, `da-DK` by its tag and its name.
    const several = pick("d", mac);
    expect(several.problem).toBe(
      'Several begin with "d": German (Germany), de-DE; Danish (Denmark), da-DK',
    );
    expect(pick("zh", mac).problem).toContain("zh-Hans");
    // `a` begins `automatic` and nothing else here, so it is not ambiguous.
    expect(pick("a", mac)).toEqual({ language: null });
  });

  it("refuses a language the machine does not offer, and lists what it does", () => {
    expect(pick("ja-JP", mac).problem).toBe(
      'No language here begins with "ja-JP". ' +
        "This computer offers en-US, fr-FR, de-DE, zh-Hans, zh-Hant, da-DK.",
    );
  });

  it("says where more are installed only where they can be", () => {
    expect(pick("de-DE", windows).problem).toBe(
      `No language here begins with "de-DE". This computer offers en-US. ${INSTALL_MORE}`,
    );
    expect(INSTALL_MORE).toContain("Time & language");
    expect(INSTALL_MORE).toContain("Language & region");
    expect(pick("ja-JP", mac).problem).not.toContain("Settings");
  });

  it("asks for an answer rather than taking a blank one", () => {
    // A blank answer is not Automatic: Enter on an empty box must not undo a
    // choice the reader made once and forgot about.
    expect(pick("", mac).problem).toContain("Type a language, or automatic.");
    expect(pick("   ", windows).problem).toContain(INSTALL_MORE);
  });

  it("offers Automatic alone where the machine lists nothing", () => {
    const none: Offered = { languages: [], installable: false };
    expect(pick("automatic", none)).toEqual({ language: null });
    expect(pick("en-US", none).problem).toContain("offers no language to choose");
  });

  it("shortens a long list", () => {
    const many: Offered = {
      languages: Array.from({ length: 15 }, (_, i) => `x${i}-AA`),
      installable: false,
    };
    expect(pick("ja", many).problem).toContain("x11-AA and 3 more.");
  });
});

describe("the sentences", () => {
  it("say what is chosen now, and what an answer will do", () => {
    expect(placeholder(null)).toContain("now: Automatic");
    expect(placeholder("de-DE")).toContain("now: German (Germany), de-DE");
    expect(preview("de-DE")).toBe("Recognise text as German (Germany), de-DE");
    expect(preview(null)).toBe("Let the recogniser choose the language");
    expect(chosen("de-DE")).toBe("Text will be recognised as German (Germany), de-DE.");
    expect(chosen(null)).toBe("Text will be recognised in the language the recogniser chooses.");
  });

  it("say which language was missing and which command chooses another", () => {
    expect(fellBack("de-DE")).toBe(
      "German (Germany), de-DE is not available on this computer, " +
        `so the recogniser chose the language itself. ${CHOOSE_AGAIN}`,
    );
  });

  it("name the command by the title it is registered under", () => {
    const title = /id: "file\.recogniseTextLanguage",\s+title: "([^"]+)"/.exec(commands)?.[1];
    expect(title).toBeDefined();
    expect(CHOOSE_AGAIN).toContain(`"${title}"`);
  });
});

describe("RecognitionLanguage", () => {
  it("starts with the recogniser's own choice, and takes what the session held", () => {
    const state = new RecognitionLanguage();
    expect(state.language).toBeNull();
    state.restore("de-DE");
    expect(state.language).toBe("de-DE");
    for (const nothing of [undefined, null, "", 7]) {
      state.restore(nothing);
      expect(state.language).toBeNull();
    }
  });

  it("holds a list only while it is being asked about", () => {
    const state = new RecognitionLanguage();
    expect(state.question()).toBeNull();
    state.hold(mac);
    expect(state.question()).toBe(mac);
    state.drop();
    expect(state.question()).toBeNull();
  });

  it("fetches the machine's list and then asks the question about it", async () => {
    const state = new RecognitionLanguage();
    const log: string[] = [];
    await state.choose(
      async () => { log.push("fetch"); return mac; },
      // The palette reads the list as it opens, so it has to be held by now.
      () => log.push(state.question() === mac ? "ask, list held" : "ask, no list"),
      (problem) => log.push(`say ${problem}`),
    );
    expect(log).toEqual(["fetch", "ask, list held"]);
  });

  it("says why the list could not be fetched, and asks nothing", async () => {
    const state = new RecognitionLanguage();
    const log: string[] = [];
    await state.choose(
      async () => { throw new Error("no recogniser"); },
      () => log.push("ask"),
      (problem) => log.push(`say ${problem}`),
    );
    expect(log).toEqual(["say Error: no recogniser"]);
    expect(state.question()).toBeNull();
  });

  it("takes an answer once, says it, and stops asking", () => {
    const state = new RecognitionLanguage();
    state.hold(mac);
    expect(state.answer("german")).toBe(chosen("de-DE"));
    expect(state.language).toBe("de-DE");
    expect(state.question()).toBeNull();
    // No list is held any more, so a second answer changes nothing.
    expect(state.answer("fr-FR")).toBeNull();
    expect(state.language).toBe("de-DE");
  });

  it("goes back to the recogniser's own choice when asked to", () => {
    const state = new RecognitionLanguage();
    state.restore("de-DE");
    state.hold(mac);
    expect(state.answer("automatic")).toBe(chosen(null));
    expect(state.language).toBeNull();
  });

  it("keeps the choice when the answer names nothing", () => {
    const state = new RecognitionLanguage();
    state.restore("de-DE");
    state.hold(mac);
    expect(state.answer("ja-JP")).toBeNull();
    expect(state.language).toBe("de-DE");
    expect(state.question()).toBeNull();
  });
});

describe("the language's wiring in App.svelte", () => {
  // Read as text with the comments taken out, because `App.svelte` is the join
  // and nothing imports it. That sees that a line is there and not whether it
  // runs (`sourcetext.ts`), so these hold the hand-over and no decision.
  it("recognises text in the language chosen", () => {
    expect(missingFrom(functionIn(app, "async function recogniseText(): Promise<void> {"), [
      "await edits.ocrCopy(source, chosen, recognitionRun, recognitionLanguage.language),",
    ])).toEqual([]);
  });

  it("takes the choice from the session at launch, and has the session keep a new one", () => {
    expect(missingFrom(app, [
      "recognitionLanguage.restore(session.ocr_language);",
      'void call("session_set_ocr_language", { language: recognitionLanguage.language })',
    ])).toEqual([]);
  });

  it("asks through the module, which fetches the list first", () => {
    expect(missingFrom(functionIn(app, "async function chooseRecognitionLanguage(): Promise<void> {"), [
      "await recognitionLanguage.choose(",
      '() => call("ocr_languages"),',
      '() => palette?.askFor("file.recogniseTextLanguage.choice"),',
    ])).toEqual([]);
  });

  it("answers the palette's commands from the held list", () => {
    expect(missingFrom(app, [
      "const offered = recognitionLanguage.question();",
      "setRecognitionLanguage: (raw) => setRecognitionLanguage(raw),",
      "dropRecognitionLanguages: () => recognitionLanguage.drop(),",
    ])).toEqual([]);
  });
});
