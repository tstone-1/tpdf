import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DIALOG_CLASS, NewPasswordDialog } from "./newpassworddialog";
import { ADVICE, NOT_ASCII } from "./protect";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";

const { validate } = vi.hoisted(() => ({ validate: vi.fn() }));
vi.mock("./ipc", () => ({ call: validate }));

let dom: FakeDom;

beforeEach(() => {
  validate.mockReset().mockResolvedValue(undefined);
  dom = installFakeDom();
});

afterEach(() => {
  dom.restore();
});

type Field = FakeElement & { value: string };

/** The dialog, and the pieces a reader touches. */
function open(): {
  dialog: NewPasswordDialog;
  backdrop: FakeElement;
  first: Field;
  second: Field;
  problem: FakeElement;
  cancel: FakeElement;
  save: FakeElement;
  shown: () => (string | null)[];
} {
  const host = dom.root;
  const dialog = new NewPasswordDialog(host as unknown as HTMLElement);
  const backdrop = host.children.find((c) => c.classList.contains(DIALOG_CLASS));
  const panel = backdrop?.children[0];
  if (!backdrop || !panel) throw new Error("the dialog did not mount");
  const [first, second] = panel.children.filter((c) => c.tagName === "input") as Field[];
  const problem = panel.children.find((c) => c.getAttribute("role") === "alert");
  const buttons = panel.children.find((c) => c.children.some((b) => b.tagName === "button"));
  const [cancel, save] = buttons?.children ?? [];
  if (!first || !second || !problem || !cancel || !save) {
    throw new Error("the dialog is missing a control");
  }
  const shown = () => panel.children.map((c) => c.textContent);
  return { dialog, backdrop, first, second, problem, cancel, save, shown };
}

describe("NewPasswordDialog", () => {
  it("keeps composition confirmation and cancellation inside the password field", async () => {
    const { dialog, backdrop, first, second } = open();
    const answer = dialog.ask("report.pdf");
    first.value = second.value = "composed";
    for (const key of ["Enter", "Escape"]) {
      let prevented = false;
      backdrop.dispatch("keydown", { key, target: first, isComposing: true, preventDefault: () => { prevented = true; } });
      expect(prevented).toBe(false);
      expect(dialog.isOpen).toBe(true);
    }
    backdrop.dispatch("keydown", { key: "Enter", target: first });
    await expect(answer).resolves.toBe("composed");
  });

  it("keeps Find inside the modal and lets Cancel handle Enter", async () => {
    const { dialog, backdrop, cancel } = open();
    const answer = dialog.ask("report.pdf");
    expect(backdrop.open).toBe(true);
    let stopped = false;
    let prevented = false;
    backdrop.dispatch("keydown", { key: "f", ctrlKey: true,
      stopPropagation: () => { stopped = true; }, preventDefault: () => { prevented = true; } });
    expect([stopped, prevented]).toEqual([true, true]);
    prevented = false;
    backdrop.dispatch("keydown", { key: "Enter", target: cancel, preventDefault: () => { prevented = true; } });
    expect(prevented).toBe(false);
    expect(dialog.isOpen).toBe(true);
    cancel.dispatch("click", {});
    await expect(answer).resolves.toBeNull();
    expect(backdrop.open).toBe(false);
  });

  it("resolves with a password typed the same way twice", async () => {
    const { dialog, first, second, save } = open();
    const answer = dialog.ask("report.pdf");
    first.value = "tr0ub4dor";
    second.value = "tr0ub4dor";
    save.dispatch("click", {});
    await expect(answer).resolves.toBe("tr0ub4dor");
    expect(dialog.isOpen).toBe(false);
  });

  it("names the file and says the password cannot be recovered", () => {
    const { dialog, shown } = open();
    void dialog.ask("report.pdf");
    expect(shown()).toContain("Password for a copy of report.pdf");
    expect(shown()).toContain(ADVICE);
  });

  it("stays open and says why when the two differ, then takes the correction", async () => {
    const { dialog, first, second, problem, backdrop } = open();
    let settled = false;
    const answer = dialog.ask("report.pdf").then((password) => {
      settled = true;
      return password;
    });
    first.value = "tr0ub4dor";
    second.value = "tr0ub4dir";
    backdrop.dispatch("keydown", { key: "Enter", target: first });
    await Promise.resolve();
    expect(settled).toBe(false);
    expect(dialog.isOpen).toBe(true);
    expect(problem.textContent).toBe("The two passwords are not the same.");
    // What was typed is still there to be corrected.
    expect(first.value).toBe("tr0ub4dor");
    second.value = "tr0ub4dor";
    backdrop.dispatch("keydown", { key: "Enter", target: first });
    await expect(answer).resolves.toBe("tr0ub4dor");
  });

  it("keeps a refused native password in both fields and accepts a correction", async () => {
    const { dialog, first, second, save, problem } = open();
    const answer = dialog.ask("report.pdf");
    first.value = second.value = "ab\u05d0";
    validate.mockRejectedValueOnce("Invalid writing direction");
    save.dispatch("click", {});
    await vi.waitFor(() => expect(problem.textContent).toBe("Invalid writing direction"));
    expect(dialog.isOpen).toBe(true);
    expect([first.value, second.value]).toEqual(["ab\u05d0", "ab\u05d0"]);
    first.value = second.value = "corrected";
    save.dispatch("click", {});
    await expect(answer).resolves.toBe("corrected");
  });

  it("ignores a successful validation after either field changes", async () => {
    const { dialog, first, second, save, problem } = open();
    const answer = dialog.ask("report.pdf");
    for (const field of [first, second]) {
      first.value = second.value = "original";
      let resolve!: () => void;
      validate.mockReturnValueOnce(new Promise<void>((done) => { resolve = done; }));
      save.dispatch("click", {});
      field.value = "edited";
      resolve();
      await Promise.resolve();
      expect(dialog.isOpen).toBe(true);
      expect(field.value).toBe("edited");
      expect(problem.textContent).toBe("");
    }
    first.value = second.value = "confirmed";
    save.dispatch("click", {});
    await expect(answer).resolves.toBe("confirmed");
  });

  it("ignores a refusal from a previous dialog even with identical fields", async () => {
    const { dialog, first, second, save, cancel, problem } = open();
    const old = dialog.ask("first.pdf");
    let reject!: (reason: string) => void;
    validate.mockReturnValueOnce(new Promise<void>((_, fail) => { reject = fail; }));
    first.value = second.value = "original";
    save.dispatch("click", {});
    cancel.dispatch("click", {});
    await expect(old).resolves.toBeNull();
    const current = dialog.ask("second.pdf");
    first.value = second.value = "original";
    reject("Old refusal");
    await Promise.resolve();
    expect(dialog.isOpen).toBe(true);
    expect(problem.textContent).toBe("");
    expect([first.value, second.value]).toEqual(["original", "original"]);
    save.dispatch("click", {});
    await expect(current).resolves.toBe("original");
  });

  it("does not save with nothing typed", () => {
    const { dialog, save, problem } = open();
    void dialog.ask("report.pdf");
    save.dispatch("click", {});
    expect(dialog.isOpen).toBe(true);
    expect(problem.textContent).toBe("Type a password.");
  });

  it("warns about Preview while the password has a character it does not accept", () => {
    const { dialog, first, problem } = open();
    void dialog.ask("report.pdf");
    first.value = "pässword";
    first.dispatch("input", {});
    expect(problem.textContent).toBe(NOT_ASCII);
    first.value = "password";
    first.dispatch("input", {});
    expect(problem.textContent).toBe("");
  });

  it.each([
    ["Cancel", (c: { cancel: FakeElement }) => c.cancel.dispatch("click", {})],
    ["Escape", (c: { backdrop: FakeElement }) => c.backdrop.dispatch("keydown", { key: "Escape" })],
    ["the backdrop", (c: { backdrop: FakeElement }) => c.backdrop.dispatch("click", { target: c.backdrop })],
  ])("resolves with null and clears both fields when dismissed by %s", async (_name, dismiss) => {
    const controls = open();
    const answer = controls.dialog.ask("report.pdf");
    controls.first.value = "tr0ub4dor";
    controls.second.value = "tr0ub4dor";
    dismiss(controls);
    await expect(answer).resolves.toBeNull();
    expect(controls.first.value).toBe("");
    expect(controls.second.value).toBe("");
  });

  it("clears both fields after a password was given", async () => {
    const { dialog, first, second, save } = open();
    const answer = dialog.ask("report.pdf");
    first.value = "tr0ub4dor";
    second.value = "tr0ub4dor";
    save.dispatch("click", {});
    await answer;
    expect(first.value).toBe("");
    expect(second.value).toBe("");
  });

  it("starts each question without the last one's refusal", () => {
    const { dialog, save, problem } = open();
    void dialog.ask("a.pdf");
    save.dispatch("click", {});
    expect(problem.textContent).not.toBe("");
    void dialog.ask("b.pdf");
    expect(problem.textContent).toBe("");
  });

  it("settles the first question when a second is asked", async () => {
    const { dialog } = open();
    const first = dialog.ask("a.pdf");
    void dialog.ask("b.pdf");
    await expect(first).resolves.toBeNull();
    expect(dialog.isOpen).toBe(true);
  });
});
