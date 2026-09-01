import { describe, expect, it } from "vitest";

import {
  afterCopy,
  afterMerge,
  afterRedaction,
  afterRefusal,
  beforeRedactingInPlace,
  beforeReload,
  refusalOf,
  type Offer,
} from "./recovery";

describe("refusalOf", () => {
  it("reads the message off a refusal object", () => {
    // The whole reason this exists. A command that rejects with an object gets
    // stringified to `[object Object]` by the obvious `String(e)`, and that is
    // what a reader saw the day printing's refusal stopped being a string.
    expect(refusalOf({ message: "report.pdf changed on disk", changed: true }).message).toBe(
      "report.pdf changed on disk",
    );
  });

  it("carries the flag the rules decide from", () => {
    // The message alone is the failure this seam was written for: a refusal
    // that reads correctly and arrives with no buttons is the one a reader
    // cannot act on.
    const failure = refusalOf({ message: "x", changed: true });
    expect(failure.changed).toBe(true);
    expect(afterRefusal(failure).offers).toEqual(["saveCopy", "reload"]);
  });

  it("falls back to the stringified value for a throw that is not a refusal", () => {
    // A panic, a transport failure, a `throw "boom"` in a plugin. There is no
    // object to read and the reader still has to be told something.
    expect(refusalOf("boom").message).toBe("boom");
    expect(refusalOf(new Error("boom")).message).toBe("boom");
    expect(refusalOf(null).message).toBe("null");
  });

  it("offers nothing for a throw that carries no flags", () => {
    // The control for the case above: a message with no flags behind it must
    // not acquire buttons on the way through.
    expect(afterRefusal(refusalOf("boom")).offers).toEqual([]);
  });

  it("takes a flag only where it is a boolean", () => {
    // `undefined` rather than truthy, and the direction is what matters: a
    // `reopen` of `"false"` read as true withholds both offers, leaving a
    // reader with edits, a changed file and nothing to press.
    const failure = refusalOf({ message: "x", changed: true, reopen: "false" });
    expect(failure.reopen).toBeUndefined();
    expect(afterRefusal(failure).offers).toEqual(["saveCopy", "reload"]);
  });

  it("reports an absent flag as absent rather than as false", () => {
    // `RefusalShape` draws that distinction deliberately -- the rules treat
    // the two alike and assert that they do -- so collapsing it here would move
    // a decision out of the place that is tested for it.
    const failure = refusalOf({ message: "x" });
    expect(failure.changed).toBeUndefined();
    expect(failure.reopen).toBeUndefined();
  });
});

describe("afterRefusal", () => {
  it("offers a copy and a reload when the file changed and the document survived", () => {
    // The case the whole thing exists for. The reader's edits are in the
    // journal, the file underneath is a different file, and both moves are
    // real: write the work somewhere, or start again from what is on disk.
    const prompt = afterRefusal({ message: "x changed on disk", changed: true });
    expect(prompt.offers).toEqual(["saveCopy", "reload"]);
  });

  it("puts the copy first, because reload is the one that spends the journal", () => {
    // Order is not cosmetic here: the two buttons sit side by side and one of
    // them destroys work. A test on the set alone would pass with them swapped.
    const prompt = afterRefusal({ message: "x", changed: true });
    expect(prompt.offers[0]).toBe("saveCopy");
  });

  it("offers nothing once the document is closed, because the window reopened it", () => {
    // `reopen` means the journal is spent AND `App.svelte` has already opened
    // the file again. So Reload would reload what is on screen and Save a copy
    // would copy a freshly-opened, unedited document. A button that looks like
    // help and does nothing is worse than no button: a reader presses it and
    // concludes the application is broken.
    const prompt = afterRefusal({ message: "x", changed: true, reopen: true });
    expect(prompt.offers).toEqual([]);
  });

  it("offers nothing for a refusal that is not about the file changing", () => {
    // The control, and the one that matters most. "A document must keep at
    // least one page" is fixed by putting a page back; a Reload button beside
    // it offers to discard the reader's work in exchange for nothing.
    const prompt = afterRefusal({
      message: "a document must keep at least one page",
    });
    expect(prompt.offers).toEqual([]);
  });

  it("offers nothing when the flag is absent rather than false", () => {
    // A backend that stops sending the field must not silently start offering
    // reloads. `undefined` and `false` have to mean the same thing here.
    const prompt = afterRefusal({ message: "x", reopen: true });
    expect(prompt.offers).toEqual([]);
  });

  it("offers both to a refusal that carries no reopen at all", () => {
    // The shape a print refusal arrives in: a message and `changed`, and no
    // `reopen` field of any kind, because nothing on that path closes the
    // document. The rule reads the flags rather than which command called it,
    // so this is the same case as a save's -- pinned because the alternative
    // reading, that a missing `reopen` is an unknown state worth withholding
    // buttons for, is the one that strands the reader.
    const prompt = afterRefusal({ message: "report.pdf changed on disk", changed: true });
    expect(prompt.offers).toEqual(["saveCopy", "reload"]);
  });

  it("offers nothing to a refusal that is not about the file, reopen or not", () => {
    // A print refused because the job would not read back, or because the
    // document is encrypted. Nothing about the file on disk has changed, so
    // Reload would discard the reader's work to fetch the same document again.
    expect(afterRefusal({ message: "the print job could not be read back", changed: false }).offers)
      .toEqual([]);
  });

  it("passes the message through untouched", () => {
    // The window shows what the backend said. Rewording it here would put a
    // second author on a sentence `save.rs` is careful about.
    const message = "report.pdf changed on disk since you opened it --- its length went from 4 to 5";
    expect(afterRefusal({ message, changed: true }).message).toBe(message);
  });
});

describe("beforeReload", () => {
  it("says nothing on an unedited document", () => {
    // Reload is also what someone reaches for when they know they changed the
    // file. Confirming a reload that costs nothing trains people to click past
    // the one that costs something.
    expect(beforeReload(false)).toBeNull();
  });

  it("warns before discarding unsaved edits, and offers the copy first", () => {
    const prompt = beforeReload(true);
    expect(prompt).not.toBeNull();
    expect(prompt?.offers).toEqual(["saveCopy", "reload"]);
  });

  it("says what is lost rather than asking whether to continue", () => {
    // "Are you sure?" is a question a reader cannot answer without knowing what
    // it costs. The sentence has to name the thing.
    const prompt = beforeReload(true);
    expect(prompt?.message).toContain("discards");
    expect(prompt?.message).toContain("not saved");
  });
});

describe("beforeRedactingInPlace", () => {
  it("always warns, where a reload only warns when there is work to lose", () => {
    // There is no unedited case. The file the reader opened is about to stop
    // holding the words they marked whatever else is true of it, so a rule with
    // a quiet branch would have one that is wrong.
    expect(beforeRedactingInPlace("report.pdf").offers).toEqual(["saveCopy", "redact"]);
  });

  it("names the file, so a reader with two windows open knows which", () => {
    expect(beforeRedactingInPlace("report.pdf").message).toContain("report.pdf");
  });

  it("says what it costs rather than asking whether to continue", () => {
    // `beforeReload`'s rule, and it matters more here: the cost is the document
    // rather than the unsaved work, and "no original left" is the half a reader
    // is most likely to assume is untrue.
    const prompt = beforeRedactingInPlace("report.pdf");
    expect(prompt.message).toContain("no undo");
    expect(prompt.message).toContain("no original left");
  });

  it("offers the copy first, which is the only way left to keep one", () => {
    // Order, not membership: the working document is still unredacted while
    // this is on screen, so Save a copy is a real escape and not a courtesy.
    // The check above asserts the pair; this one is about which leads, and it
    // would pass on the reversed list if it read `toContain`.
    expect(beforeRedactingInPlace("report.pdf").offers[0]).toBe("saveCopy");
  });
});

describe("the offers these rules can return", () => {
  it("is the set App.svelte draws a button for", () => {
    // **The one check standing between a new `Offer` and a prompt with a dead
    // button.** Nothing renders `App.svelte`, so the `{#each offers}` block is
    // reachable from no unit test; what is reachable is every rule that decides
    // what goes in it. A variant added to `Offer` and returned by a rule turns
    // this red, and the message says where the arm goes.
    //
    // Written out rather than derived from the type: a check that reads `Offer`
    // to decide what `Offer` may contain agrees with itself whatever the type
    // says, which is this repository's own note about a writer and its own
    // reader. The cost is that adding a variant edits this line, and that is
    // the point.
    const drawn = new Set<Offer>(["saveCopy", "reload", "redact"]);
    const everything: Offer[] = [
      ...afterRefusal({ message: "x", changed: true }).offers,
      ...afterRefusal({ message: "x", changed: true, reopen: true }).offers,
      ...afterRefusal({ message: "x" }).offers,
      ...(beforeReload(true)?.offers ?? []),
      ...beforeRedactingInPlace("report.pdf").offers,
    ];
    // A rule that stopped offering anything would satisfy the loop below by
    // having nothing to check, which is the emptiness control every sweep here
    // is required to carry.
    expect(everything.length).toBeGreaterThan(0);
    for (const offer of everything) {
      expect(
        drawn.has(offer),
        `${offer} is offered by a rule and App.svelte has no arm for it`,
      ).toBe(true);
    }
  });
});

describe("afterMerge", () => {
  it("says how many documents went in and how many pages came out", () => {
    // The counts are the whole point: a reader who picked three files cannot
    // tell from the destination that all three were read.
    const said = afterMerge({ changed: false, pages: 47, files: 3 });
    expect(said).toContain("3 other documents");
    expect(said).toContain("47 pages");
  });

  it("says it in the singular for one document and one page", () => {
    // Not decoration. "1 other documents" is the shape a reader reads as a
    // defect in everything else on screen.
    const said = afterMerge({ changed: false, pages: 1, files: 1 });
    expect(said).toContain("1 other document ");
    expect(said).toContain("1 page ");
    expect(said).not.toContain("documents");
    expect(said).not.toContain("pages");
  });

  it("is never silent, unlike a copy", () => {
    // The deliberate asymmetry with `afterCopy`, pinned so that making the two
    // consistent later is a decision rather than a tidy-up.
    expect(afterMerge({ pages: 2, files: 1 })).not.toBe("");
    expect(afterCopy({ changed: false })).toBeNull();
  });

  it("adds the changed-source warning without dropping the counts", () => {
    // Both facts are true and the reader needs both. An early return for the
    // changed case would keep the sentence and lose the evidence.
    const said = afterMerge({ changed: true, pages: 9, files: 2 });
    expect(said).toContain("9 pages");
    expect(said).toContain("changed on disk");
  });
});

describe("afterCopy", () => {
  it("says nothing about an ordinary copy", () => {
    // Success is silent everywhere else in the copy path, and a banner over the
    // page a reader is looking at is not an acknowledgement.
    expect(afterCopy({ changed: false })).toBeNull();
    expect(afterCopy({})).toBeNull();
  });

  it("says the copy came from a newer file, and does not call it an error", () => {
    // The file is written and is the best tpdf can produce. What the reader
    // must not have to discover is which document it was built from.
    const said = afterCopy({ changed: true });
    expect(said).not.toBeNull();
    expect(said).toContain("changed on disk");
    expect(said).toContain("written");
  });
});

describe("what to say after a redaction", () => {
  const clean = { regions: 2, shows: 3, verified: true, why: [] };

  it("always says something, unlike a copy", () => {
    // The asymmetry with `afterCopy`, pinned so that making the two agree is a
    // decision somebody has to take rather than a tidy-up. A copy that worked
    // is silent because the file appearing is the acknowledgement; a redaction
    // has destroyed content on the strength of a claim, and the claim is what
    // the reader needs to see.
    expect(afterCopy({ changed: false })).toBeNull();
    expect(afterRedaction(clean)).not.toBe("");
  });

  it("says the file was read back and the words are not in it", () => {
    expect(afterRedaction(clean)).toBe(
      "Redacted 2 regions, 3 removals. tpdf read the file back and none of the " +
        "removed words are in it.",
    );
  });

  it("counts one region and one removal without saying regions", () => {
    expect(afterRedaction({ ...clean, regions: 1, shows: 1 })).toContain(
      "1 region, 1 removal.",
    );
  });

  it("names every reason it could not prove the file clean", () => {
    // Never a bare success is half of `docs/PLAN.md` §6 step 4; this is the
    // other half, and the sentence has to carry what the reader does next.
    const said = afterRedaction({
      regions: 1,
      shows: 1,
      verified: false,
      why: ["page 3: object 0 is of kind image", "a stream would not decode"],
    });
    expect(said).toContain("could not prove the file is clean");
    expect(said).toContain("page 3: object 0 is of kind image");
    expect(said).toContain("a stream would not decode");
    expect(said).toContain("Treat it as unredacted");
  });

  it("adds the changed-source note without dropping the verdict", () => {
    // Two facts, and neither replaces the other: what was written, and which
    // document it was built from.
    const said = afterRedaction({ ...clean, changed: true });
    expect(said).toContain("read the file back");
    expect(said).toContain("changed on disk");
  });
});
