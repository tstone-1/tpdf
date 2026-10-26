/**
 * Which timestamp authority a signing asks, and what is remembered about it.
 *
 * `docs/PLAN.md` §9 Phase 6 step 3, increment B. The request itself is Rust's
 * (`tsa.rs`, in the app process): **the webview gains no network authority**,
 * and this module only chooses the address it hands `sign_document`. It is a
 * module rather than markup in `signing.ts`'s chooser so that the rules the
 * owner decided on 2026-09-28 are here with tests:
 *
 * - **A signing preselects {@link PRESELECTED}; nothing else preselects
 *   anything.** Until 2026-10-13 nothing was preselected anywhere, so a reader
 *   who had never chosen got no timestamp. The owner reversed that for a
 *   signing on that day: a reader who signs expects the time to be confirmed
 *   by somebody other than their own clock. The chooser shows the selection
 *   with the authority's name before anything is signed, says what is sent
 *   (a hash of the signature, nothing of the document), and *No timestamp* is
 *   one click. Only a reader who has **never chosen** gets the preselection:
 *   a remembered *none* stays none, and storage that throws or holds anything
 *   tpdf did not write still reads as none, so nothing unreadable ever turns
 *   into a request. Adding validation data to a signed document
 *   (`validationdata.ts`) keeps the old rule and preselects nothing.
 * - **A short list plus the reader's own URL.** {@link SERVERS} is the list
 *   `tsa::SERVERS` holds, and `signtimestamp.test.ts` compares the two through
 *   `wording.json`. *Other* takes an `http` or `https` address, which Rust
 *   judges again, because the webview is not what decides what is asked.
 * - **The choice is remembered** in `localStorage` under {@link CHOICE_KEY},
 *   the per-viewer store `signappearance.ts` uses, guarded the same way: storage
 *   that throws or holds anything tpdf did not write reads as none.
 */

import { catalog } from "./i18n";

/** A timestamp authority on the short list. Mirrors `tsa::Server`. */
export interface Server {
  /** What is remembered, and what `tpdf sign --timestamp` accepts. */
  name: string;
  /** As a reader reads it. */
  label: string;
  /** Where the request goes. */
  url: string;
}

/** `tsa::SERVERS`, measured live on 2026-09-28. */
export const SERVERS: readonly Server[] = [
  { name: "digicert", label: "DigiCert", url: "http://timestamp.digicert.com" },
  { name: "sectigo", label: "Sectigo", url: "https://timestamp.sectigo.com" },
  {
    name: "globalsign",
    label: "GlobalSign",
    url: "http://timestamp.globalsign.com/tsa/r6advanced1",
  },
];

/** What the reader chose: none, a server by name, or their own address. */
export interface StampChoice {
  /** `"none"`, a {@link Server.name}, or `"other"`. */
  server: string;
  /** The reader's own address; kept while *Other* is not chosen, so it is not retyped. */
  url: string;
}

/** Where the choice lives. */
export const CHOICE_KEY = "tpdf.signatureTimestamp";

/** The longest address remembered or accepted, in characters. */
export const MAX_URL_CHARS = 2048;

/** No timestamp. */
export function noStamp(): StampChoice {
  return { server: "none", url: "" };
}

/**
 * The authority a signing preselects for a reader who has never chosen. The
 * module note says why there is one. Sectigo, because of the three it is the
 * one asked over HTTPS (`docs/PLAN.md` §9 measured the other two as answering
 * over plain HTTP only): a request nobody picked by hand should be the one a
 * network in between cannot answer in the authority's place
 * (`docs/THREAT-MODEL.md` §T10).
 */
export const PRESELECTED = "sectigo";

/** What a signing's chooser shows a reader who has never chosen. */
export function preselectedStamp(): StampChoice {
  return { server: PRESELECTED, url: "" };
}

type Store = Pick<Storage, "getItem" | "setItem">;

/** Every value {@link StampChoice.server} may hold. */
function known(server: string): boolean {
  return server === "none" || server === "other" || SERVERS.some((s) => s.name === server);
}

/**
 * The remembered choice, or none. **Whole or nothing**, `readPreference`'s
 * rule: one field tpdf would not have written means none of it is trusted.
 *
 * `key` is where it lives: a signing's by default, and a key of its own for
 * the one other question that names an authority (`validationdata.ts`), so
 * that answering one never changes what the other remembers.
 *
 * `never` is what a reader who has never chosen gets --- nothing stored under
 * `key` at all. None by default; a signing passes {@link preselectedStamp}.
 * It is not what unreadable storage reads as: that is always none.
 */
export function readStampChoice(
  storage: () => Store = () => window.localStorage,
  key: string = CHOICE_KEY,
  never: () => StampChoice = noStamp,
): StampChoice {
  let raw: string | null;
  try {
    raw = storage().getItem(key);
  } catch {
    return noStamp();
  }
  if (raw === null) return never();
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return noStamp();
  }
  if (typeof value !== "object" || value === null) return noStamp();
  const { server, url } = value as { server?: unknown; url?: unknown };
  if (typeof server !== "string" || !known(server)) return noStamp();
  if (typeof url !== "string" || url.length > MAX_URL_CHARS) return noStamp();
  return { server, url };
}

/** Keeps `choice` under `key`; `false` when storage refused, which changes nothing now. */
export function writeStampChoice(
  choice: StampChoice,
  storage: () => Store = () => window.localStorage,
  key: string = CHOICE_KEY,
): boolean {
  try {
    storage().setItem(key, JSON.stringify(choice));
    return true;
  } catch {
    return false;
  }
}

/**
 * Why `text` is not an address tpdf asks, or `null` when it is.
 *
 * A first look for the reader's sake, so *Other* can say what is wrong before
 * anything is signed. It is not the rule: `tsa::authority` is, in Rust, and it
 * refuses anything this lets through that it would not.
 */
export function addressProblem(text: string): string | null {
  const said = TEXT();
  const trimmed = text.trim();
  if (trimmed === "") return said.empty;
  if (trimmed.length > MAX_URL_CHARS) return said.tooLong;
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return said.notAddress;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return said.scheme;
  }
  if (url.username !== "" || url.password !== "") {
    return said.credentials;
  }
  return null;
}

/** What {@link addressProblem} says. Server labels are names and stay as they are. */
const TEXT = catalog({
  "en-US": {
    empty: "Type the timestamp authority's address.",
    tooLong: "That address is too long.",
    notAddress: "That is not an address: it should begin with http:// or https://.",
    scheme: "tpdf asks timestamp authorities over http:// or https:// only.",
    credentials: "An address with a user name or password in it is not asked.",
  },
  "de-DE": {
    empty: "Gib die Adresse des Zeitstempeldienstes ein.",
    tooLong: "Diese Adresse ist zu lang.",
    notAddress: "Das ist keine Adresse: Sie muss mit http:// oder https:// beginnen.",
    scheme: "tpdf fragt Zeitstempeldienste nur über http:// oder https:// an.",
    credentials: "Eine Adresse, die einen Benutzernamen oder ein Passwort enthält, wird nicht angefragt.",
  },
});

/**
 * The address the signing asks, or `null` for no timestamp.
 *
 * *Other* with an address {@link addressProblem} refuses answers `null` too,
 * and the chooser holds *Sign* until it is fixed --- so `null` here always
 * means the reader chose none, never that a typo turned a timestamp off.
 */
export function stampUrl(choice: StampChoice): string | null {
  if (choice.server === "other") {
    return addressProblem(choice.url) === null ? choice.url.trim() : null;
  }
  return SERVERS.find((s) => s.name === choice.server)?.url ?? null;
}

/**
 * Where the long-term choice lives: whether a signing with a timestamp also
 * gathers the certificates' revocation data and adds it to the document
 * (PAdES B-LT, `longterm.rs`). Unticked until the reader ticks it, and only
 * ever asked for together with a timestamp.
 */
export const LONG_TERM_KEY = "tpdf.signatureLongTerm";

/**
 * The remembered long-term choice, or `false`. Only the exact value tpdf
 * writes reads as ticked: anything else --- storage that throws, a value tpdf
 * did not write --- is the default, which asks nobody for anything more.
 */
export function readLongTerm(storage: () => Store = () => window.localStorage): boolean {
  try {
    return storage().getItem(LONG_TERM_KEY) === "true";
  } catch {
    return false;
  }
}

/** Keeps the long-term choice; `false` when storage refused. */
export function writeLongTerm(
  longTerm: boolean,
  storage: () => Store = () => window.localStorage,
): boolean {
  try {
    storage().setItem(LONG_TERM_KEY, longTerm ? "true" : "false");
    return true;
  } catch {
    return false;
  }
}
