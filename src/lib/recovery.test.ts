import { describe, expect, it } from "vitest";

import { afterCopy, afterFailedSave, afterMerge, beforeReload } from "./recovery";

describe("afterFailedSave", () => {
  it("offers a copy and a reload when the file changed and the document survived", () => {
    // The case the whole thing exists for. The reader's edits are in the
    // journal, the file underneath is a different file, and both moves are
    // real: write the work somewhere, or start again from what is on disk.
    const prompt = afterFailedSave({ message: "x changed on disk", changed: true });
    expect(prompt.offers).toEqual(["saveCopy", "reload"]);
  });

  it("puts the copy first, because reload is the one that spends the journal", () => {
    // Order is not cosmetic here: the two buttons sit side by side and one of
    // them destroys work. A test on the set alone would pass with them swapped.
    const prompt = afterFailedSave({ message: "x", changed: true });
    expect(prompt.offers[0]).toBe("saveCopy");
  });

  it("offers nothing once the document is closed, because the window reopened it", () => {
    // `reopen` means the journal is spent AND `App.svelte` has already opened
    // the file again. So Reload would reload what is on screen and Save a copy
    // would copy a freshly-opened, unedited document. A button that looks like
    // help and does nothing is worse than no button: a reader presses it and
    // concludes the application is broken.
    const prompt = afterFailedSave({ message: "x", changed: true, reopen: true });
    expect(prompt.offers).toEqual([]);
  });

  it("offers nothing for a refusal that is not about the file changing", () => {
    // The control, and the one that matters most. "A document must keep at
    // least one page" is fixed by putting a page back; a Reload button beside
    // it offers to discard the reader's work in exchange for nothing.
    const prompt = afterFailedSave({
      message: "a document must keep at least one page",
    });
    expect(prompt.offers).toEqual([]);
  });

  it("offers nothing when the flag is absent rather than false", () => {
    // A backend that stops sending the field must not silently start offering
    // reloads. `undefined` and `false` have to mean the same thing here.
    const prompt = afterFailedSave({ message: "x", reopen: true });
    expect(prompt.offers).toEqual([]);
  });

  it("passes the message through untouched", () => {
    // The window shows what the backend said. Rewording it here would put a
    // second author on a sentence `save.rs` is careful about.
    const message = "report.pdf changed on disk since you opened it --- its length went from 4 to 5";
    expect(afterFailedSave({ message, changed: true }).message).toBe(message);
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
