import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { DIALOG_CLASS, PasswordDialog } from "./passworddialog";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
});

afterEach(() => {
  dom.restore();
});

/** The dialog, and the pieces a reader touches. */
function open(): {
  dialog: PasswordDialog;
  backdrop: FakeElement;
  field: FakeElement & { value: string };
  cancel: FakeElement;
  unlock: FakeElement;
} {
  const host = dom.root;
  const dialog = new PasswordDialog(host as unknown as HTMLElement);
  const backdrop = host.children.find((c) => c.classList.contains(DIALOG_CLASS));
  if (!backdrop) throw new Error("the dialog did not mount");
  const panel = backdrop.children[0];
  if (!panel) throw new Error("the dialog has no panel");
  const field = panel.children.find((c) => c.tagName === "input") as FakeElement & {
    value: string;
  };
  const buttons = panel.children.find((c) => c.children.some((b) => b.tagName === "button"));
  const [cancel, unlock] = buttons?.children ?? [];
  if (!field || !cancel || !unlock) throw new Error("the dialog is missing a control");
  return { dialog, backdrop, field, cancel, unlock };
}

describe("PasswordDialog", () => {
  it("resolves with what was typed", async () => {
    const { dialog, field, unlock } = open();
    const answer = dialog.ask("locked.pdf", "This document is locked, and needs a password.");
    field.value = "swordfish";
    unlock.dispatch("click", {});
    await expect(answer).resolves.toBe("swordfish");
  });

  it("shows the file's name and the reason it was given", () => {
    const { dialog, backdrop } = open();
    void dialog.ask("quarterly.pdf", "That password did not open this document.");
    const shown = backdrop.children[0]?.children.map((c) => c.textContent) ?? [];
    expect(shown).toContain("quarterly.pdf");
    // The *second* wording, specifically. Only the worker that tried a password
    // knows to say this, so a dialog that showed its own fixed sentence would
    // tell a reader who just mistyped that the document is locked, which they
    // already knew.
    expect(shown).toContain("That password did not open this document.");
  });

  it.each([
    ["Cancel", (c: { cancel: FakeElement }) => c.cancel.dispatch("click", {})],
    ["Escape", (c: { backdrop: FakeElement }) => c.backdrop.dispatch("keydown", { key: "Escape" })],
  ])("resolves with null when dismissed by %s", async (_name, dismiss) => {
    const controls = open();
    const answer = controls.dialog.ask("locked.pdf", "locked");
    controls.field.value = "swordfish";
    dismiss(controls);
    // Null even though something was typed: dismissing is not submitting, and a
    // dialog that returned the field's contents on Escape would open the
    // document a reader had just decided not to open.
    await expect(answer).resolves.toBeNull();
  });

  it("treats a click on the backdrop as a dismissal and one on the panel as nothing", async () => {
    const { dialog, backdrop } = open();
    const answer = dialog.ask("locked.pdf", "locked");
    // A click that bubbled out of the panel is not a click on the backdrop, and
    // this is the control: without it the assertion below is satisfied by a
    // dialog that closes on any click at all, including one on its own field.
    backdrop.dispatch("click", { target: backdrop.children[0] });
    expect(dialog.isOpen).toBe(true);
    backdrop.dispatch("click", { target: backdrop });
    await expect(answer).resolves.toBeNull();
  });

  it("submits on Enter, because that is where the reader's hands are", async () => {
    const { dialog, backdrop, field } = open();
    const answer = dialog.ask("locked.pdf", "locked");
    field.value = "swordfish";
    backdrop.dispatch("keydown", { key: "Enter" });
    await expect(answer).resolves.toBe("swordfish");
  });

  it("reads an empty field as no answer rather than as an empty password", async () => {
    const { dialog, unlock } = open();
    const answer = dialog.ask("locked.pdf", "locked");
    unlock.dispatch("click", {});
    // The distinction is real to PDFium --- an empty *user* password is what most
    // permission-restricted documents carry --- but a reader who pressed Unlock
    // on an empty field has supplied nothing, and sending "" would retry the
    // attempt that already failed.
    await expect(answer).resolves.toBeNull();
  });

  it("clears the field on every close, however it closed", async () => {
    const { dialog, backdrop, field, unlock } = open();
    const submitted = dialog.ask("locked.pdf", "locked");
    field.value = "swordfish";
    unlock.dispatch("click", {});
    await submitted;
    expect(field.value).toBe("");

    const dismissed = dialog.ask("locked.pdf", "locked");
    field.value = "swordfish";
    backdrop.dispatch("keydown", { key: "Escape" });
    await dismissed;
    // The value is the secret, and a detached-but-live input is the one place
    // it would outlive its use.
    expect(field.value).toBe("");
  });

  it("settles an outstanding question when a second one is asked", async () => {
    const { dialog, field, unlock } = open();
    const first = dialog.ask("one.pdf", "locked");
    const second = dialog.ask("two.pdf", "locked");
    // The first must not be left hanging: whoever awaited it is holding an open
    // that never finishes, and nothing later can settle it.
    await expect(first).resolves.toBeNull();
    field.value = "swordfish";
    unlock.dispatch("click", {});
    await expect(second).resolves.toBe("swordfish");
  });

  it("is closed until asked, and closed again afterwards", async () => {
    const { dialog, unlock } = open();
    expect(dialog.isOpen).toBe(false);
    const answer = dialog.ask("locked.pdf", "locked");
    expect(dialog.isOpen).toBe(true);
    unlock.dispatch("click", {});
    await answer;
    expect(dialog.isOpen).toBe(false);
  });
});
