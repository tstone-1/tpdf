/**
 * Signing with a certificate, through the window a reader signs in.
 *
 * `tabs_check.py --phase sign --identity <sha256>` runs it. **Never in the
 * gates or in CI**: it signs with a real key from the reader's keychain, so the
 * operating system asks the person at the machine to allow it, and it asks two
 * timestamp authorities over the network. `BUILD.md` has the command and what
 * the person clicks.
 *
 * Below the window every piece is tested --- `signing.ts`'s sequence,
 * `signtimestamp.ts`'s remembered choice, `commands::sign`'s held signature ---
 * and `tpdf-cli sign` has been driven against the real keychain. What none of
 * that reaches is `App.svelte`'s join of them, which `AGENTS.md` names as the
 * layer no gate reaches. So every step here goes the reader's way: the palette
 * opened from the toolbar and typed into, the chooser's own radio buttons and
 * checkbox, the question's own buttons, the message area and the properties
 * dialog read off the screen. The one exception is the save panel, which no
 * phase can drive; `saveanswer.ts` answers it with a path in the phase's own
 * directory, and records the name the window would have suggested.
 *
 * What is concluded from what is read is `signphase.ts`'s, with tests. Two
 * facts the screen cannot show are read from `sign_record`: how many times the
 * OS was asked for a key, and what signature is held. The count is exact both
 * ways at every step, and the steps that must ask once are what make the
 * steps that must ask nothing falsifiable: a count that did not count would
 * fail the first of them.
 *
 * Writes nothing but its copies: `tabs_check.py` checks afterwards, from
 * outside, which of them exist.
 */

import type { OpenCheckHost } from "./opencheck";
import { pause, settle, type Report } from "./checkreport";
import { call } from "./ipc";
import { basename } from "./paths";
import { closing, keyRequests, parseSignPhase, readBack, type ShownSection } from "./signphase";
import { CHOICE_KEY, LONG_TERM_KEY } from "./signtimestamp";
import { NOT_WRITTEN, signedName } from "./signing";

/** How long the viewer may take to settle after an open. */
const SETTLE_MS = 20_000;

/**
 * How long a signing may take. A person answers the keychain's prompt in
 * this, and a timestamp authority and the revocation requests answer over the
 * network; a bound that expired while the prompt was still up would read as a
 * refusal.
 */
const SIGN_MS = 300_000;

/** How long a dialog may take to appear once asked for. */
const DIALOG_MS = 5000;

export async function signingCheck(host: OpenCheckHost, expected: string, report: Report): Promise<void> {
  const { fixture, room, identity } = parseSignPhase(expected);
  const at = (name: string) => `${room}/${name}`;

  // The remembered choices, kept and cleared, so "nothing is preselected" is
  // about a reader who has never chosen rather than about the last run.
  const kept = [CHOICE_KEY, LONG_TERM_KEY].map((key) => [key, localStorage.getItem(key)] as const);
  for (const [key] of kept) localStorage.removeItem(key);
  try {
    await steps(host, report, fixture, identity, at);
  } finally {
    for (const [key, value] of kept) {
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    }
  }
}

async function steps(
  host: OpenCheckHost,
  report: Report,
  fixture: string,
  identity: string,
  at: (name: string) => string,
): Promise<void> {
  const shown = () => document.querySelector('[data-testid="problem"]')?.textContent ?? "";
  const record = () => call("sign_record");
  const quiet = async () => {
    if (!(await settle(() => host.viewer()?.idle === true, SETTLE_MS))) {
      throw new Error("the viewer did not settle");
    }
    await pause(100);
  };
  const verdicts = (list: { name: string; ok: boolean; detail: string }[]) => {
    for (const v of list) report.check(v.name, v.ok, v.detail);
  };

  /** A command through the palette, opened from the toolbar and typed into. */
  const palette = async (query: string, title: string) => {
    const button = [...document.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent === "Commands",
    );
    if (!button) throw new Error("the toolbar has no Commands button");
    button.click();
    const field = () => document.querySelector<HTMLInputElement>(".tpdf-palette input");
    if (!(await settle(() => field() !== null && field()!.offsetParent !== null, DIALOG_MS))) {
      throw new Error("the palette did not open");
    }
    field()!.value = query;
    field()!.dispatchEvent(new InputEvent("input", { bubbles: true }));
    await pause(50);
    const highlighted =
      document.querySelector(".tpdf-palette [role=option][aria-selected=true]")?.textContent ?? "";
    if (!highlighted.includes(title)) {
      throw new Error(`"${query}" highlights "${highlighted}" in the palette, not "${title}"`);
    }
    field()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  };

  const chooser = () => document.querySelector<HTMLDialogElement>(".sign-identity-dialog[open]");
  const inChooser = <T extends Element>(selector: string) => chooser()?.querySelector<T>(selector) ?? null;
  const stamp = (value: string) => inChooser<HTMLInputElement>(`input[name="sign-timestamp"][value="${value}"]`);
  const longTerm = () => inChooser<HTMLInputElement>('input[name="sign-long-term"]');
  const checkedStamps = () =>
    [...(chooser()?.querySelectorAll<HTMLInputElement>('input[name="sign-timestamp"]') ?? [])]
      .filter((r) => r.checked)
      .map((r) => r.value);
  const button = (root: Element | null, label: string) =>
    [...(root?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find((b) => b.textContent === label) ?? null;

  /** *Sign document…* from the palette, up to its chooser. */
  const openChooser = async () => {
    await palette("Sign document", "Sign document…");
    if (!(await settle(() => chooser() !== null, DIALOG_MS))) {
      throw new Error(`Sign document… opened no chooser: ${shown().slice(0, 160)}`);
    }
  };

  /** The identity and *Invisible* in the chooser, the way a reader picks them. */
  const pickIdentity = (label: string) => {
    const offered = [...(chooser()?.querySelectorAll<HTMLInputElement>('input[name="sign-identity"]') ?? [])];
    const mine = offered.find((r) => r.value === identity);
    report.check(`${label}: the chooser offers the identity asked for`, !!mine,
      offered.map((r) => r.parentElement?.textContent?.trim().slice(0, 60)).join("; ") || "none offered");
    if (!mine) throw new Error(`the identity ${identity} is not in the chooser`);
    mine.click();
    const invisible = inChooser<HTMLInputElement>('input[name="sign-appearance"][value="invisible"]');
    report.check(`${label}: Invisible is the appearance chosen`, invisible?.checked === true, String(invisible?.checked));
    invisible?.click();
  };

  /**
   * *Sign…*, with the save panel answered by `path`. Answers what the message
   * area said before, because an earlier signing's sentence can still be on
   * screen and must not be read as this one's.
   */
  const sign = (path: string): string => {
    const before = shown();
    host.answerSave(path);
    const go = button(chooser(), "Sign…");
    if (!go) throw new Error("the chooser has no Sign… button");
    go.click();
    return before;
  };

  // ---- 1. The fixture, and the chooser as a reader who never chose sees it.
  await host.open(fixture); await host.idle(); await quiet();
  const original = host.tabs().find((t) => t.path === fixture);
  if (!original) throw new Error("the fixture did not open");
  report.check("the message area starts clear", shown() === "", shown().slice(0, 80));
  const r0 = await record();
  report.check("nothing is held before the first signing", r0.held === null, String(r0.held));

  await openChooser();
  pickIdentity("first signing");

  // ---- 2. Nothing preselected; DigiCert; no long-term data; saved.
  report.check("no timestamp authority is preselected",
    JSON.stringify(checkedStamps()) === JSON.stringify(["none"]), checkedStamps().join(","));
  report.check("long-term data cannot be ticked without a timestamp",
    longTerm()?.disabled === true && longTerm()?.checked === false,
    `disabled=${longTerm()?.disabled} checked=${longTerm()?.checked}`);
  stamp("digicert")?.click();
  report.check("choosing an authority enables long-term data, unticked",
    longTerm()?.disabled === false && longTerm()?.checked === false,
    `disabled=${longTerm()?.disabled} checked=${longTerm()?.checked}`);
  const before1 = sign(at("digicert.pdf"));
  const first = await settle(() => shown() !== before1 && shown() !== "", SIGN_MS);
  await pause(200);
  report.check("the first signing reaches a sentence", first && shown().includes("Signed as "), shown().slice(0, 200));
  const suggestions = host.saveSuggestions();
  const suggested = suggestions[suggestions.length - 1] ?? "";
  report.check("the save panel suggests <name>-signed.pdf beside the original",
    suggested === signedName(fixture), `${suggested} for ${basename(fixture)}`);
  verdicts(closing(shown(), "digicert.pdf", "DigiCert"));
  const r1 = await record();
  verdicts([keyRequests("the first signing asks the OS for the key once", r0.key_requests, r1.key_requests, 1)]);
  report.check("nothing is held after it is written", r1.held === null, String(r1.held));

  // ---- 3. The saved copy, in the properties dialog.
  await host.open(at("digicert.pdf")); await host.idle(); await quiet();
  await palette("Document properties", "Document properties");
  const panel = () => document.querySelector<HTMLElement>('.tpdf-properties [role="dialog"]');
  const read = () =>
    [...(panel()?.querySelectorAll("section") ?? [])].map((section): ShownSection => {
      const cells = [...section.querySelectorAll("dt, dd")];
      const rows: ShownSection["rows"] = [];
      for (let i = 0; i + 1 < cells.length; i += 2) {
        rows.push({ name: cells[i]!.textContent ?? "", value: cells[i + 1]!.textContent ?? "" });
      }
      return { title: section.querySelector("h3")?.textContent ?? "", rows };
    });
  const filled = await settle(() => read().some((s) => s.title.startsWith("Signature")), SIGN_MS);
  report.check("the properties dialog reads the saved copy", filled, (panel()?.textContent ?? "").slice(0, 120));
  verdicts(readBack(read(), "DigiCert"));
  panel()?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  await host.idle();

  // ---- 4. The original again: Sectigo, long-term data ticked, refused.
  await host.activate(original.id); await host.idle(); await quiet();
  await openChooser();
  report.check("the chooser remembers DigiCert, and long-term data unticked",
    JSON.stringify(checkedStamps()) === JSON.stringify(["digicert"]) && longTerm()?.checked === false,
    `${checkedStamps().join(",")} long-term=${longTerm()?.checked}`);
  pickIdentity("second signing");
  stamp("sectigo")?.click();
  longTerm()?.click();
  report.check("long-term data is ticked", longTerm()?.checked === true, String(longTerm()?.checked));
  const before2 = sign(at("sectigo.pdf"));
  const question = () => document.querySelector<HTMLDialogElement>(".sign-long-term-dialog[open]");
  const asked = await settle(() => question() !== null || shown() !== before2, SIGN_MS);
  const text = question()?.textContent ?? "";
  report.check("the long-term data is refused with a question, not a message",
    asked && question() !== null, (text || shown()).slice(0, 200));
  report.check("the refusal names the revocation data the certificate does not publish",
    text.includes("does not say where its revocation data is published"), text.slice(0, 240));
  report.check("it offers Try again, Sign without long-term data and Cancel",
    ["Try again", "Sign without long-term data", "Cancel"].every((l) => button(question(), l) !== null),
    [...(question()?.querySelectorAll("button") ?? [])].map((b) => b.textContent).join(", "));
  const r2 = await record();
  verdicts([keyRequests("the second signing asks the OS for the key once", r1.key_requests, r2.key_requests, 1)]);
  report.check("the signature is held, waiting for its long-term data", r2.held === "long_term", String(r2.held));

  // ---- 5. Sign without long-term data: written, and the key not asked again.
  button(question(), "Sign without long-term data")?.click();
  const second = await settle(() => shown() !== before2 && shown().includes("Signed as "), SIGN_MS);
  report.check("signing without long-term data writes the copy", second, shown().slice(0, 200));
  verdicts(closing(shown(), "sectigo.pdf", "Sectigo"));
  const r3 = await record();
  verdicts([keyRequests("signing without long-term data asks the OS for nothing", r2.key_requests, r3.key_requests, 0)]);
  report.check("nothing is held once it is written", r3.held === null, String(r3.held));

  // ---- 6. Sign document… again: Sectigo and long-term data remembered.
  await openChooser();
  report.check("the chooser remembers Sectigo, and long-term data ticked",
    JSON.stringify(checkedStamps()) === JSON.stringify(["sectigo"]) && longTerm()?.checked === true,
    `${checkedStamps().join(",")} long-term=${longTerm()?.checked}`);

  // ---- The Cancel path: refused again, cancelled, nothing written, nothing held.
  pickIdentity("third signing");
  const before3 = sign(at("cancelled.pdf"));
  const again = await settle(() => question() !== null || shown() !== before3, SIGN_MS);
  report.check("the third signing is refused with the same question", again && question() !== null,
    (question()?.textContent ?? shown()).slice(0, 160));
  const r4 = await record();
  verdicts([keyRequests("the third signing asks the OS for the key once", r3.key_requests, r4.key_requests, 1)]);
  report.check("its signature is held while the question is open", r4.held === "long_term", String(r4.held));
  button(question(), "Cancel")?.click();
  // Trimmed: the message area's text carries a trailing space from its markup,
  // and the first run with the owner (2026-09-28) read the right sentence and
  // failed on that space alone.
  const cancelled = await settle(() => shown().trim() === NOT_WRITTEN, DIALOG_MS);
  report.check("Cancel says nothing was written", cancelled, shown().slice(0, 120));
  const r5 = await record();
  report.check("Cancel drops the held signature", r5.held === null, String(r5.held));
  verdicts([keyRequests("Cancel asks the OS for nothing", r4.key_requests, r5.key_requests, 0)]);
  await pause(100);
}
