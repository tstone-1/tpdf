/**
 * Signing with a certificate the reader already has: the sequence and its words.
 *
 * `docs/PLAN.md` §9 Phase 6 step 2. The backend is `sign_prepare.rs` (the
 * worker's revision), `sign_cms.rs` (the CMS and the splice, in the app
 * process) and `keystore.rs` (the OS store). This module is the frontend's
 * half, and it is here rather than in `App.svelte` because that file is the
 * layer no gate reaches (`AGENTS.md`): the order of the questions, what the
 * reader is told at each refusal, and the sentence after signing all live here
 * with tests, and `App.svelte` only supplies the four things that need the
 * shell --- the chooser, the save panel, the command and the message area.
 *
 * ## The order is a decision
 *
 * 1. **Unsaved edits refuse first**, before anything is asked of the OS: a
 *    signature is over the file, and the reader is looking at the file plus
 *    their edits. The backend refuses the same thing; saying it here saves a
 *    trip through a certificate chooser to be told.
 * 2. **The certificates are listed**, and none usable is a message naming the
 *    ones that were found and why each is not offered.
 * 3. **The reader chooses** a certificate and whether the signature is
 *    **invisible** (the default, and what signing did before it could be
 *    anything else) or **visible**.
 * 4. **A visible one is configured, then placed**, before the file is named:
 *    the reader's saved visual signature is read (Phase 4's store; none is not
 *    an error), the appearance panel (`signappearance.ts`) asks what it shows
 *    with a preview the worker draws, and then the viewer's crop drag is armed
 *    for one rectangle. Cancel or Escape in the panel, or Escape or anything
 *    else taking the tool during the drag, cancels the whole signing ---
 *    nothing has been asked of the OS yet, and nothing is written. The worker
 *    draws the appearance inside the revision it signs; nothing here draws it.
 * 5. **The reader names the new file** --- `<name>-signed.pdf` beside the
 *    original. The original is never written.
 * 6. **The result is what a worker read back from the written file**, one line
 *    per signature, the new one first. Its words are `integrity.ts`'s, so a
 *    signature the reader just made is described exactly as the properties
 *    dialog would describe it --- its timestamp too, when it carries one.
 *
 * ## A timestamp, asked for in the chooser
 *
 * The chooser also asks whether to add an RFC 3161 timestamp, and from which
 * authority (`signtimestamp.ts`): none by default, the reader's choice
 * remembered. The request is the backend's, made after the OS has signed. When
 * it fails, **nothing has been written**, and the reader chooses: try again,
 * sign without a timestamp, or cancel ({@link askAfterStampFailed}). The
 * backend holds the signature it already made meanwhile, so none of the three
 * asks the OS for the key again. Signing without a timestamp is only ever that
 * second, explicit answer.
 *
 * ## Long-term validation data, only with a timestamp
 *
 * Beside the timestamp, a checkbox asks whether to keep the signature
 * verifiable after the certificates expire (PAdES B-LT): the backend then
 * fetches the certificates' revocation data and adds it to the document
 * (`longterm.rs`). It can be ticked only while an authority is chosen, is
 * unticked until the reader ticks it, and is remembered like the timestamp.
 * When the data does not come, **nothing has been written**, and the reader
 * chooses again: try again, sign without it --- the timestamped signature,
 * with nothing asked of the key or the authority again --- or cancel
 * ({@link askAfterLongTermFailed}). A certificate its authority says is
 * revoked is not offered that choice: the backend refuses outright.
 */

import { basename } from "./paths";
import type { Integrity } from "./integrity";
import { WHY, authorityRow, revocationRow, timestampRow } from "./integrity";
import type { Revocation } from "./integrity";
import type { Timestamp } from "./properties";
import type { PageId } from "./pages";
import type { SignatureImage } from "./signature";
import type { Appearance, AppearanceOptions } from "./signappearance";
import type { SignTarget } from "./signfield";
import {
  SERVERS,
  addressProblem,
  readLongTerm,
  readStampChoice,
  stampUrl,
  writeLongTerm,
  writeStampChoice,
  type StampChoice,
} from "./signtimestamp";

/** A certificate the chooser may offer. Mirrors `sign_cms::Choice`. */
export interface Choice {
  /** What goes back to `sign_document` when it is picked. */
  id: string;
  subject: string;
  issuer: string;
  /** `YYYY-MM-DD HH:MM:SS UTC`. */
  expires: string;
  /** `RSA 3072`, `ECDSA P-256`. */
  method: string;
}

/** A certificate with a key that is not offered. Mirrors `sign_cms::Skipped`. */
export interface Skipped {
  subject: string;
  /** A clause: "it has expired". */
  why: string;
}

/** What `sign_identities` answers. Mirrors `sign_cms::Choices`. */
export interface Choices {
  usable: Choice[];
  skipped: Skipped[];
}

/** One signature in the written file. Mirrors `sign_cms::Checked`. */
export interface Checked {
  field: string;
  integrity: Integrity | null;
  ours: boolean;
  /** Its timestamp as the worker read it back, or `null` for none. */
  timestamp?: Timestamp | null;
  /** The signer's revocation, as the file's own data says: `good` after long-term data. */
  revocation?: Revocation | null;
  /** The archive timestamp a long-term signing writes after the signature (PAdES B-LTA). */
  archive?: boolean;
}

/** What `sign_document` answers. Mirrors `sign_cms::Signed`. */
export interface Signed {
  path: string;
  field: string;
  signatures: Checked[];
}

/**
 * A signing whose timestamp, or whose long-term data, did not come. Mirrors
 * `commands::sign::Unstamped`.
 */
export interface Unstamped {
  /** Why, as a sentence. */
  why: string;
  /** The signature the backend holds, for `sign_resume` and `sign_discard`. */
  pending: number;
  /** Which did not come: the timestamp, or the long-term data after it. */
  stage?: "timestamp" | "long_term";
}

/**
 * What signing has done in this process. Mirrors `commands::sign::SignRecord`;
 * read by the checks build's signing phase alone, never by the window.
 */
export interface SignRecord {
  /** How many times the OS was asked to sign with a key. */
  key_requests: number;
  /** What the held signature is waiting for, or `null` when none is held. */
  held: "timestamp" | "long_term" | null;
}

/**
 * What `sign_document` and `sign_resume` answer. Mirrors `commands::sign::Signing`:
 * exactly one half is set --- written and read back, or a timestamp that did not
 * come, with nothing written and the made signature held.
 */
export interface SignOutcome {
  signed: Signed | null;
  unstamped: Unstamped | null;
}

/**
 * What the chooser answers: the certificate, whether the signature shows, and
 * the timestamp authority to ask --- `null` for none, which asks nobody.
 */
export interface Chosen {
  identity: string;
  visible: boolean;
  timestamp: string | null;
  /** Long-term validation data as well; only ever with a timestamp. */
  longTerm?: boolean;
}

/** What the reader chose after a timestamp, or the long-term data, did not come. */
export type AfterStamp = "retry" | "without" | null;

/** What is said when the reader cancels after a timestamp did not come. */
export const NOT_WRITTEN = "Not signed: nothing was written.";

/** Where a visible signature goes. Mirrors `commands::sign::Placement`. */
export interface Placement {
  /** The page's id, as every page command carries it. */
  page: PageId;
  /** `left, top, right, bottom` in points, in the page's display space. */
  rect: [number, number, number, number];
  /** The image the reader chose, or `null` for words alone. */
  image: SignatureImage | null;
  /** Which lines are drawn, and the reason and location. */
  options: AppearanceOptions;
}

/**
 * What the reader is told while the placement is armed.
 *
 * Said in the message area as well as the status line's short label, because
 * the chooser that asked for it has just closed and nothing else on screen says
 * the next drag is a signature.
 */
export const PLACE =
  "Drag a rectangle where the signature should appear. Esc cancels signing.";

/** The refusal for a document with edits nobody has saved. */
export const UNSAVED =
  "Save your changes first: a signature covers the file as it is on disk, and this " +
  "document has edits that are not in it yet.";

/** The name the save panel suggests: `report.pdf` becomes `report-signed.pdf`. */
export function signedName(openPath: string): string {
  const stem = basename(openPath).replace(/\.pdf$/i, "");
  return `${stem}-signed.pdf`;
}

/** The date part of an expiry, which is all a chooser needs to show. */
function day(expires: string): string {
  return expires.split(" ")[0] ?? expires;
}

/** One row of the chooser: who, from whom, until when, and how. */
export function choiceLabel(choice: Choice): string {
  return `${choice.subject} — issued by ${choice.issuer}, expires ${day(choice.expires)} (${choice.method})`;
}

/**
 * What to say when there is nothing to choose from.
 *
 * Names what *was* found, when anything was: a reader whose card holds only an
 * expired certificate is otherwise told they have none, and goes looking for
 * the wrong problem.
 */
export function nothingToChoose(choices: Choices): string {
  const none =
    "No certificate that can sign was found in your keychain or certificate store.";
  if (choices.skipped.length === 0) return none;
  const reasons = choices.skipped.map((s) => `${s.subject}: ${s.why}`).join("; ");
  return `${none} Found but not offered — ${reasons}.`;
}

/** A verdict in a few words, for a list of signatures. */
function verdict(integrity: Integrity | null): string {
  if (!integrity) return "not checked";
  switch (integrity.verdict) {
    case "intact":
      return "intact";
    case "weak":
      return "unchanged under SHA-1 only";
    case "altered":
      return "altered";
    case "broken":
      return "broken";
    case "unchecked":
      return `not checked (${integrity.why ? WHY[integrity.why] : "no reason given"})`;
  }
}

/**
 * The sentence after signing, from what a worker read back.
 *
 * Leads with the new signature and says where the file went; then every
 * earlier signature, because keeping those intact is the half of signing a
 * reader cannot see and most needs to be told. The new signature not being
 * intact is stated first and plainly --- it should not happen, since the app
 * refuses to write one its own check would not call intact, and a sentence that
 * buried it would be the reassuring branch.
 *
 * **One verdict is neither**: a new signature the read-back did not check
 * because the document's signatures together are past the hashing budget,
 * which is spent in the order the fields are listed and reaches the new one
 * last. The copy is not a failed one --- the signature was found intact over
 * the bytes that were then written --- and it was not read back intact
 * either, so the sentence says which check happened and which did not. Until
 * 2026-10-09 this was the failure above, said of a correct copy.
 */
export function afterSigning(signed: Signed): string {
  const name = basename(signed.path);
  const ours = signed.signatures.find((s) => s.ours);
  const earlier = signed.signatures.filter((s) => !s.ours && !s.archive);
  const archives = signed.signatures.filter((s) => s.archive);
  const unchecked =
    ours?.integrity?.verdict === "unchecked" && ours.integrity.why === "budget";
  if (!ours || (ours.integrity?.verdict !== "intact" && !unchecked)) {
    return (
      `${name} was written, but reading it back did not find the new signature ` +
      `${signed.field} intact: ${ours ? verdict(ours.integrity) : "it is not in the file"}. ` +
      "Do not rely on that copy."
    );
  }
  let text = unchecked
    ? `${name} was written. The new signature ${signed.field} was intact when tpdf checked it ` +
      `before writing; reading the file back did not check it again, because ${WHY.budget}.`
    : `Signed as ${signed.field} and saved to ${name}. Read back after writing, the signature is intact.`;
  // The timestamp is said of a signature read back intact: for one not
  // checked again there is no reading of it to report. (Its revocation rows
  // need no such rule: a standing is only ever beside an intact verdict.)
  //
  // The authority's standing too: over plain HTTP a token from an authority
  // other than the one asked can arrive and check out, and it must not read
  // like the one the reader chose (`docs/THREAT-MODEL.md` §T10).
  const stamp = unchecked ? null : ours.timestamp;
  if (stamp) {
    const by = stamp.authority?.subject_cn || stamp.authority?.subject || "";
    text += ` Timestamp: ${timestampRow(stamp.when, by, stamp.integrity, false).value}`;
    const authority = authorityRow(stamp.trust, stamp.authority?.from, stamp.authority?.until);
    if (authority) text += ` Timestamp authority: ${authority.value}`;
  }
  // Long-term data: said only when the file's own data answers `good`, which
  // is what a signing that added it must read back as. A signing that added
  // none reads `none`, and says nothing more than it did before.
  if (ours.revocation?.standing === "good") {
    const signer = revocationRow(ours.revocation);
    if (signer) text += ` Revocation: ${signer.value}`;
    const authority = revocationRow(stamp?.revocation ?? null, true);
    if (authority && stamp?.revocation?.standing === "good") {
      text += ` Authority revocation: ${authority.value}`;
    }
  }
  // The archive timestamp a long-term signing adds after the signature: named
  // apart, because it is neither earlier nor a signature anybody made.
  if (archives.length > 0) {
    const listed = archives.map((s) => `${s.field} ${verdict(s.integrity)}`).join(", ");
    text += ` Archive timestamp: ${listed}.`;
  }
  if (earlier.length > 0) {
    const listed = earlier.map((s) => `${s.field} ${verdict(s.integrity)}`).join(", ");
    text += ` Earlier signature${earlier.length === 1 ? "" : "s"}: ${listed}.`;
  }
  return text;
}

/** What {@link signDocument} needs from the shell. */
export interface SigningShell {
  /**
   * Commits whatever the reader is still typing --- a form answer with the
   * caret in it, a note in its box --- and waits for the edit that makes.
   * Rejects when a draft cannot be committed.
   */
  settle(): Promise<void>;
  /** Whether the document has edits that are not in the file. Asked after {@link settle}. */
  dirty(): boolean;
  /** The file the document was opened from. */
  openPath: string;
  /** `sign_identities`. */
  list(): Promise<Choices>;
  /**
   * The chooser: the certificate and the appearance, or `null` for Cancel.
   * `field` is the signature field being signed, when there is one.
   */
  choose(choices: Choice[], field: string | null): Promise<Chosen | null>;
  /** The reader's saved visual signature, or `null` when there is none. */
  savedImage(): Promise<SignatureImage | null>;
  /** The appearance panel for `identity`: the choices, or `null` for Cancel. */
  appearance(identity: string, saved: SignatureImage | null): Promise<Appearance | null>;
  /** Arms the placement: where the reader dragged, or `null` for Escape. */
  place(): Promise<{ page: PageId; rect: [number, number, number, number] } | null>;
  /** The save panel, suggesting `suggested`: a path, or `null` for Cancel. */
  saveAs(suggested: string): Promise<string | null>;
  /**
   * `sign_document`, with `null` for an invisible signature and `null` for no
   * timestamp, and whether to add long-term validation data.
   */
  sign(
    identity: string,
    path: string,
    placement: Placement | null,
    timestamp: string | null,
    longTerm: boolean,
    field: string | null,
  ): Promise<SignOutcome>;
  /** The question after a timestamp did not come, with the reason. */
  stampFailed(why: string): Promise<AfterStamp>;
  /** The question after the long-term data did not come, with the reason. */
  longTermFailed(why: string): Promise<AfterStamp>;
  /**
   * `sign_resume`: the held signature, stamped by `timestamp` or, for `null`,
   * not --- and, with `longTerm`, its long-term data gathered again.
   */
  resume(pending: number, timestamp: string | null, longTerm: boolean): Promise<SignOutcome>;
  /** `sign_discard`: the held signature is dropped, and nothing is written. */
  discard(pending: number): Promise<void>;
}

/**
 * The whole sequence, in the module note's order.
 *
 * Answers the sentence to show, or `null` when the reader cancelled --- which
 * is an answer and not an event worth a message. A refusal from the backend is
 * thrown on, for the caller to show as it shows every other.
 *
 * With `field`, an empty signature field of the document, the signature goes
 * into that field: a visible one is drawn in the field's rectangle, so nothing
 * is dragged, and an invisible one is written into it all the same.
 */
export async function signDocument(shell: SigningShell, field: SignTarget | null = null): Promise<string | null> {
  // Before `dirty` is read, and that order is the point. What is signed is
  // the file on disk, and a form answer commits only when its control loses
  // the keyboard: read first, the document says it is saved while the answer
  // is on screen, the chooser's dialog then takes the keyboard, and the commit
  // that makes is refused because a document task is running. The copy would
  // be signed without the answer and reported intact.
  await shell.settle();
  if (shell.dirty()) return UNSAVED;
  const choices = await shell.list();
  if (choices.usable.length === 0) return nothingToChoose(choices);
  const chosen = await shell.choose(choices.usable, field?.name ?? null);
  if (chosen === null) return null;
  let placement: Placement | null = null;
  if (chosen.visible) {
    // The saved image before the panel, so a store that cannot be read says so
    // before the reader has chosen anything.
    const saved = await shell.savedImage();
    const appearance = await shell.appearance(chosen.identity, saved);
    if (appearance === null) return null;
    const placed = field ? { page: field.page, rect: field.rect } : await shell.place();
    if (placed === null) return null;
    placement = { ...placed, image: appearance.image, options: appearance.options };
  }
  const path = await shell.saveAs(signedName(shell.openPath));
  if (!path) return null;
  // Long-term data only ever with a timestamp: the backend refuses it without
  // one, and this never asks.
  const longTerm = chosen.longTerm === true && chosen.timestamp !== null;
  let outcome = await shell.sign(
    chosen.identity, path, placement, chosen.timestamp, longTerm, field?.name ?? null,
  );
  // A timestamp or long-term data that did not come: nothing is written until
  // the reader says what to do, and signing without either is only ever that
  // answer.
  while (outcome.unstamped) {
    const { why, pending, stage } = outcome.unstamped;
    const afterLongTerm = stage === "long_term";
    const next = afterLongTerm ? await shell.longTermFailed(why) : await shell.stampFailed(why);
    if (next === null) {
      await shell.discard(pending);
      return NOT_WRITTEN;
    }
    outcome = afterLongTerm
      ? // The timestamp is already in the held signature; only the data is asked again.
        await shell.resume(pending, chosen.timestamp, next === "retry")
      : await shell.resume(
          pending,
          next === "retry" ? chosen.timestamp : null,
          next === "retry" && longTerm,
        );
  }
  if (!outcome.signed) throw new Error("The signing answered neither a signature nor a reason.");
  return afterSigning(outcome.signed);
}

/**
 * The chooser: one radio button per certificate, the first selected; the
 * appearance, invisible selected; and the timestamp, as the reader last chose
 * it, which is none until they choose one (`signtimestamp.ts`).
 *
 * A modal on the same surface as the signed-save warning. Built from
 * `textContent` only, since a certificate's subject is somebody else's text.
 */
export function askIdentity(
  choices: Choice[],
  storage?: () => Pick<Storage, "getItem" | "setItem">,
  field: string | null = null,
): Promise<Chosen | null> {
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  dialog.className = "sign-identity-dialog";
  dialog.setAttribute("aria-label", "Sign document");
  dialog.style.cssText =
    "max-width:560px;padding:22px;border:1px solid #8885;border-radius:12px;" +
    "font:13px/1.55 system-ui,-apple-system,sans-serif;" +
    "background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
  const heading = document.createElement("h2");
  heading.textContent = "Sign document";
  const help = document.createElement("p");
  help.textContent =
    "Choose the certificate to sign with. The signed document is saved as a new file; " +
    "the original is not changed. Your operating system may ask for access to the key " +
    "or for a PIN — tpdf never sees it.";
  const list = document.createElement("div");
  list.setAttribute("role", "radiogroup");
  list.style.cssText = "display:flex;flex-direction:column;gap:6px;margin:12px 0";
  const radios = choices.map((choice, at) => {
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "sign-identity";
    radio.value = choice.id;
    radio.checked = at === 0;
    const label = document.createElement("label");
    label.append(radio, ` ${choiceLabel(choice)}`);
    list.append(label);
    return radio;
  });
  // The appearance: invisible first and selected, because it is what signing
  // did before there was a choice and what a reader who reads nothing gets.
  const shows = document.createElement("div");
  shows.setAttribute("role", "radiogroup");
  shows.setAttribute("aria-label", "Appearance");
  shows.style.cssText = "display:flex;flex-direction:column;gap:6px;margin:12px 0";
  const appearance = (value: string, text: string, checked: boolean) => {
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "sign-appearance";
    radio.value = value;
    radio.checked = checked;
    const label = document.createElement("label");
    label.append(radio, ` ${text}`);
    shows.append(label);
    return radio;
  };
  // In a signature field the field is where it shows, so visible is what a
  // reader who pressed the field expects and nothing is dragged.
  appearance("invisible", "Invisible — the signature is in the file, not on a page", field === null);
  const visible = appearance(
    "visible",
    field === null
      ? "Visible — choose what it shows, with a preview, then drag a rectangle on a page"
      : `Visible — choose what it shows, with a preview; it is drawn in the field ${field}`,
    field !== null,
  );
  // The timestamp: as last chosen, and none until a reader chooses one. No
  // server is ever preselected --- choosing one is what lets tpdf ask it.
  const remembered = readStampChoice(storage);
  const stamps = document.createElement("div");
  stamps.setAttribute("role", "radiogroup");
  stamps.setAttribute("aria-label", "Timestamp");
  stamps.style.cssText = "display:flex;flex-direction:column;gap:6px;margin:12px 0";
  const stampNote = document.createElement("p");
  stampNote.textContent =
    "A timestamp authority can confirm when the signature was made. tpdf sends it a " +
    "hash of the new signature — nothing of the document — and writes nothing if " +
    "no timestamp that checks out comes back.";
  stamps.append(stampNote);
  const stamp = (value: string, text: string) => {
    const radio = document.createElement("input");
    radio.type = "radio";
    radio.name = "sign-timestamp";
    radio.value = value;
    radio.checked = remembered.server === value;
    const label = document.createElement("label");
    label.append(radio, ` ${text}`);
    stamps.append(label);
    return radio;
  };
  const stampRadios = [
    stamp("none", "No timestamp — the time is your computer's clock"),
    ...SERVERS.map((server) => stamp(server.name, `Timestamp from ${server.label}`)),
    stamp("other", "Timestamp from another authority:"),
  ];
  const otherUrl = document.createElement("input");
  otherUrl.type = "url";
  otherUrl.value = remembered.url;
  otherUrl.placeholder = "https://";
  otherUrl.setAttribute("aria-label", "Timestamp authority address");
  const problem = document.createElement("p");
  problem.setAttribute("role", "alert");
  stamps.append(otherUrl, problem);
  // Long-term data: a checkbox, ticked only as the reader last left it, and
  // only while an authority is chosen --- it rests on the timestamp.
  const longTerm = document.createElement("input");
  longTerm.type = "checkbox";
  longTerm.name = "sign-long-term";
  longTerm.checked = readLongTerm(storage);
  const longTermLabel = document.createElement("label");
  longTermLabel.append(
    longTerm,
    " Keep it verifiable after the certificates expire — tpdf also asks the " +
      "certificate authorities whether the certificates are revoked, adds their " +
      "answers to the document, and asks the timestamp authority once more for a " +
      "timestamp over the whole. Needs a timestamp.",
  );
  stamps.append(longTermLabel);
  const noneStamp = stampRadios[0];
  const syncLongTerm = () => {
    longTerm.disabled = noneStamp?.checked === true;
  };
  for (const radio of stampRadios) radio.addEventListener("change", syncLongTerm);
  syncLongTerm();
  const footer = document.createElement("div");
  footer.style.cssText = "display:flex;gap:10px;justify-content:flex-end";
  const cancel = document.createElement("button");
  cancel.textContent = "Cancel";
  const next = document.createElement("button");
  next.textContent = "Sign…";
  footer.append(cancel, next);
  dialog.append(heading, help, list, shows, stamps, footer);
  document.body.append(dialog);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (chosen: Chosen | null) => {
      if (settled) return;
      settled = true;
      dialog.close();
      dialog.remove();
      previous?.focus();
      resolve(chosen);
    };
    cancel.addEventListener("click", () => finish(null));
    next.addEventListener("click", () => {
      const identity = radios.find((radio) => radio.checked)?.value;
      if (identity === undefined) return finish(null);
      const chosenStamp: StampChoice = {
        server: stampRadios.find((radio) => radio.checked)?.value ?? "none",
        url: otherUrl.value,
      };
      // *Other* with an address tpdf would not ask holds the chooser open: a
      // typo must not turn into a signing with no timestamp.
      if (chosenStamp.server === "other") {
        const wrong = addressProblem(chosenStamp.url);
        if (wrong !== null) {
          problem.textContent = wrong;
          return;
        }
      }
      writeStampChoice(chosenStamp, storage);
      writeLongTerm(longTerm.checked, storage);
      const timestamp = stampUrl(chosenStamp);
      finish({
        identity,
        visible: visible.checked,
        timestamp,
        longTerm: longTerm.checked && timestamp !== null,
      });
    });
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      finish(null);
    });
    dialog.addEventListener("close", () => finish(null));
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.showModal();
    radios[0]?.focus();
  });
}

/**
 * The question after a timestamp did not come: try again, sign without one, or
 * cancel. **Nothing has been written**, and the dialog says so --- and the
 * signature already made is what either of the first two writes, so the OS is
 * not asked again. Cancel is the default, and Escape is Cancel: a reader who
 * reads nothing gets no file rather than a signature without the timestamp
 * they asked for.
 */
export function askAfterStampFailed(why: string): Promise<AfterStamp> {
  return askAfter({
    className: "sign-timestamp-dialog",
    label: "No timestamp",
    heading: "The timestamp could not be added",
    why,
    help:
      "Nothing has been written. The signature is made; you can try the timestamp again, " +
      "or save it without a timestamp, and the key is not asked for again either way.",
    without: "Sign without a timestamp",
  });
}

/**
 * The question after the long-term data did not come: try again, sign without
 * it, or cancel --- {@link askAfterStampFailed}'s question one step later.
 * Without it, the signature keeps its timestamp; neither the key nor the
 * authority is asked again. Cancel is the default, and Escape is Cancel.
 */
export function askAfterLongTermFailed(why: string): Promise<AfterStamp> {
  return askAfter({
    className: "sign-long-term-dialog",
    label: "No long-term validation data",
    heading: "The long-term validation data could not be added",
    why,
    help:
      "Nothing has been written. The signature is made and timestamped; you can try again, " +
      "or save it without the long-term data — it keeps its timestamp. The key is not asked " +
      "for again either way.",
    without: "Sign without long-term data",
  });
}

/** What {@link askAfter} shows. */
interface AfterQuestion {
  className: string;
  label: string;
  heading: string;
  why: string;
  help: string;
  without: string;
}

/** The dialog both questions are: the reason, then Cancel, without, or Try again. */
function askAfter(question: AfterQuestion): Promise<AfterStamp> {
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement("dialog");
  dialog.className = question.className;
  dialog.setAttribute("aria-label", question.label);
  dialog.style.cssText =
    "max-width:560px;padding:22px;border:1px solid #8885;border-radius:12px;" +
    "font:13px/1.55 system-ui,-apple-system,sans-serif;" +
    "background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
  const heading = document.createElement("h2");
  heading.textContent = question.heading;
  const reason = document.createElement("p");
  reason.textContent = `${question.why}.`;
  const help = document.createElement("p");
  help.textContent = question.help;
  const footer = document.createElement("div");
  footer.style.cssText = "display:flex;gap:10px;justify-content:flex-end";
  const cancel = document.createElement("button");
  cancel.textContent = "Cancel";
  const without = document.createElement("button");
  without.textContent = question.without;
  const retry = document.createElement("button");
  retry.textContent = "Try again";
  footer.append(cancel, without, retry);
  dialog.append(heading, reason, help, footer);
  document.body.append(dialog);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (answer: AfterStamp) => {
      if (settled) return;
      settled = true;
      dialog.close();
      dialog.remove();
      previous?.focus();
      resolve(answer);
    };
    cancel.addEventListener("click", () => finish(null));
    without.addEventListener("click", () => finish("without"));
    retry.addEventListener("click", () => finish("retry"));
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      finish(null);
    });
    dialog.addEventListener("close", () => finish(null));
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.showModal();
    cancel.focus();
  });
}
