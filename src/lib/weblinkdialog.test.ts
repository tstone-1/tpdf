import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Target, WebTarget } from "./outline";
import { installFakeDom, type FakeDom, type FakeElement } from "./testdom";
import {
  DIALOG_CLASS,
  WebLinkDialog,
  addressOf,
  confirmAndOpen,
  type WebAddress,
  type WebLinkDeps,
} from "./weblinkdialog";

let dom: FakeDom;

beforeEach(() => {
  dom = installFakeDom();
});

afterEach(() => {
  dom.restore();
});

/** The dialog, and the pieces a reader touches. */
function open(): {
  dialog: WebLinkDialog;
  backdrop: FakeElement;
  panel: FakeElement;
  cancel: FakeElement;
  openButton: FakeElement;
} {
  const host = dom.root;
  const dialog = new WebLinkDialog(host as unknown as HTMLElement);
  const backdrop = host.children.find((c) => c.classList.contains(DIALOG_CLASS));
  if (!backdrop) throw new Error("the dialog did not mount");
  const panel = backdrop.children[0];
  if (!panel) throw new Error("the dialog has no panel");
  const buttons = panel.children.find((c) =>
    c.children.some((b) => b.tagName === "button"),
  );
  const [cancel, openButton] = buttons?.children ?? [];
  if (!cancel || !openButton) throw new Error("the dialog is missing a button");
  return { dialog, backdrop, panel, cancel, openButton };
}

const ADDRESS: WebAddress = { host: "example.com", rest: "/spec#4" };

function webTarget(over: Partial<WebTarget> = {}): WebTarget {
  return { kind: "web", token: 0, host: "example.com", rest: "/spec#4", ...over };
}

describe("WebLinkDialog", () => {
  it("resolves true only when Open is pressed", async () => {
    const { dialog, openButton } = open();
    const answer = dialog.ask(ADDRESS);
    openButton.dispatch("click", {});
    await expect(answer).resolves.toBe(true);
  });

  it.each([
    ["Cancel", (parts: ReturnType<typeof open>) => parts.cancel.dispatch("click", {})],
    [
      "Escape",
      (parts: ReturnType<typeof open>) =>
        parts.backdrop.dispatch("keydown", { key: "Escape" }),
    ],
    [
      "a click on the backdrop",
      (parts: ReturnType<typeof open>) =>
        parts.backdrop.dispatch("click", { target: parts.backdrop }),
    ],
  ])("resolves false on %s", async (_name, dismiss) => {
    const parts = open();
    const answer = parts.dialog.ask(ADDRESS);
    dismiss(parts);
    await expect(answer).resolves.toBe(false);
  });

  /**
   * The posture the header argues for, asserted rather than left as prose.
   *
   * A reader who clicked a link expecting a cross-reference is about to press
   * Enter. If that opened the address, the confirmation would have interrupted
   * nothing --- so this is the one place where the ordinary dialog convention
   * is deliberately not followed, and it needs a test that fails if somebody
   * "fixes" it.
   */
  it("cancels on Enter rather than opening", async () => {
    const parts = open();
    const answer = parts.dialog.ask(ADDRESS);
    parts.backdrop.dispatch("keydown", { key: "Enter" });
    await expect(answer).resolves.toBe(false);
  });

  it("leaves Enter on a button to the browser, so Open stays reachable", async () => {
    const parts = open();
    const answer = parts.dialog.ask(ADDRESS);
    // The event whose target is a button: the handler must not intercept it,
    // so nothing settles and the promise is still outstanding.
    parts.backdrop.dispatch("keydown", { key: "Enter", target: parts.openButton });
    let settled = false;
    void answer.then(() => {
      settled = true;
    });
    await Promise.resolve();
    expect(settled).toBe(false);
    // And the button itself still works.
    parts.openButton.dispatch("click", {});
    await expect(answer).resolves.toBe(true);
  });

  it("focuses Cancel, not Open", () => {
    const parts = open();
    void parts.dialog.ask(ADDRESS);
    expect(parts.cancel.focused).toBe(true);
    expect(parts.openButton.focused).toBe(false);
  });

  it("shows the host and the path", () => {
    const parts = open();
    void parts.dialog.ask({ host: "xn--80ak6aa92e.com", rest: "/a/b" });
    const shown = parts.panel.children.map((c) => c.textContent);
    // The punycode form verbatim. A dialog that decoded it would be drawing the
    // homoglyph attack on the attacker's behalf --- see the module header.
    expect(shown).toContain("xn--80ak6aa92e.com");
    expect(shown).toContain("/a/b");
  });

  /**
   * The host and the path are separate elements, which is what lets one be
   * emphasised and the other not.
   *
   * Asserted because the obvious simplification --- one element holding
   * `host + rest` --- passes the check above and quietly loses the whole
   * display argument: a reader's eye would land on a path a stranger wrote.
   */
  it("keeps the host in an element of its own", () => {
    const parts = open();
    void parts.dialog.ask(ADDRESS);
    const host = parts.panel.children.find((c) => c.textContent === "example.com");
    const rest = parts.panel.children.find((c) => c.textContent === "/spec#4");
    expect(host).toBeDefined();
    expect(rest).toBeDefined();
    expect(host).not.toBe(rest);
  });

  it("settles the first question when a second is asked", async () => {
    const parts = open();
    const first = parts.dialog.ask(ADDRESS);
    const second = parts.dialog.ask({ host: "other.example", rest: "/" });
    await expect(first).resolves.toBe(false);
    parts.openButton.dispatch("click", {});
    await expect(second).resolves.toBe(true);
  });

  it("is closed once it has been answered", async () => {
    const parts = open();
    const answer = parts.dialog.ask(ADDRESS);
    expect(parts.dialog.isOpen).toBe(true);
    parts.cancel.dispatch("click", {});
    await answer;
    expect(parts.dialog.isOpen).toBe(false);
  });

  it("close() settles an outstanding question as a refusal", async () => {
    const parts = open();
    const answer = parts.dialog.ask(ADDRESS);
    parts.dialog.close();
    await expect(answer).resolves.toBe(false);
  });
});

describe("addressOf", () => {
  it("reads the two halves out of a web target", () => {
    expect(addressOf(webTarget())).toEqual(ADDRESS);
  });

  it.each<Target>([
    { kind: "page", page: 2, top_pt: null },
    { kind: "broken" },
    { kind: "refused", action: "uri" },
    { kind: "none" },
  ])("answers null for %o", (target) => {
    expect(addressOf(target)).toBeNull();
  });
});

describe("confirmAndOpen", () => {
  function deps(over: Partial<WebLinkDeps> = {}): WebLinkDeps {
    return {
      ask: vi.fn().mockResolvedValue(true),
      open: vi.fn().mockResolvedValue(undefined),
      onError: vi.fn(),
      ...over,
    };
  }

  it("asks before it opens, and opens what was asked about", async () => {
    const d = deps();
    await expect(confirmAndOpen(7, "links", webTarget({ token: 3 }), d)).resolves.toBe(
      true,
    );
    expect(d.ask).toHaveBeenCalledWith(ADDRESS);
    expect(d.open).toHaveBeenCalledWith(7, "links", 3);
  });

  /**
   * The one ordering that matters, and a spy on each call is not enough to pin
   * it: both would be recorded whichever way round they ran.
   */
  it("does not open when the reader says no", async () => {
    const d = deps({ ask: vi.fn().mockResolvedValue(false) });
    await expect(confirmAndOpen(1, "links", webTarget(), d)).resolves.toBe(false);
    expect(d.open).not.toHaveBeenCalled();
  });

  it("says nothing when the reader cancels", async () => {
    const d = deps({ ask: vi.fn().mockResolvedValue(false) });
    await confirmAndOpen(1, "links", webTarget(), d);
    // A cancellation is the reader's own decision. Reporting it in the status
    // line would tell them their choice had failed.
    expect(d.onError).not.toHaveBeenCalled();
  });

  it("carries the source through, because the two number independently", async () => {
    const d = deps();
    await confirmAndOpen(2, "outline", webTarget({ token: 5 }), d);
    expect(d.open).toHaveBeenCalledWith(2, "outline", 5);
  });

  it("reports the backend's own wording when the open fails", async () => {
    const d = deps({
      open: vi.fn().mockRejectedValue("this link is no longer available"),
    });
    await expect(confirmAndOpen(1, "links", webTarget(), d)).resolves.toBe(false);
    expect(d.onError).toHaveBeenCalledWith("this link is no longer available");
  });

  /**
   * A structured refusal, which `String(e)` alone renders `[object Object]`.
   *
   * The trap of that name is in `docs/TRAPS.md`; this is the case that would
   * put those nine characters in front of a reader.
   */
  it("reads a message out of a structured refusal", async () => {
    const d = deps({
      open: vi.fn().mockRejectedValue({ message: "the system could not open this link" }),
    });
    await confirmAndOpen(1, "links", webTarget(), d);
    expect(d.onError).toHaveBeenCalledWith("the system could not open this link");
  });

  it("falls back to wording of its own when the refusal is empty", async () => {
    const d = deps({ open: vi.fn().mockRejectedValue("") });
    await confirmAndOpen(1, "links", webTarget(), d);
    expect(d.onError).toHaveBeenCalledWith("this link could not be opened");
  });
});
