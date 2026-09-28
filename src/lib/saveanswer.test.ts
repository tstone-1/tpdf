import { describe, expect, it } from "vitest";

import { SaveAnswers } from "./saveanswer";

describe("SaveAnswers", () => {
  it("shows the panel when nothing is queued", async () => {
    const answers = new SaveAnswers();
    let shown = 0;
    const panel = async () => {
      shown += 1;
      return "/picked.pdf";
    };
    expect(await answers.ask("a-signed.pdf", panel)).toBe("/picked.pdf");
    expect(shown).toBe(1);
    expect(answers.asked).toEqual(["a-signed.pdf"]);
  });

  it("answers a queued path once, without the panel, then shows it again", async () => {
    const answers = new SaveAnswers();
    let shown = 0;
    const panel = async () => {
      shown += 1;
      return null;
    };
    answers.queue("/room/digicert.pdf");
    expect(await answers.ask("a-signed.pdf", panel)).toBe("/room/digicert.pdf");
    expect(shown).toBe(0);
    expect(await answers.ask("b-signed.pdf", panel)).toBeNull();
    expect(shown).toBe(1);
    expect(answers.asked).toEqual(["a-signed.pdf", "b-signed.pdf"]);
  });
});
