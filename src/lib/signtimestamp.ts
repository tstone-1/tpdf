/**
 * Which timestamp authority a signing asks, and what is remembered about it.
 *
 * `docs/PLAN.md` §9 Phase 6 step 3, increment B. The request itself is Rust's
 * (`tsa.rs`, in the app process): **the webview gains no network authority**,
 * and this module only chooses the address it hands `sign_document`. It is a
 * module rather than markup in `signing.ts`'s chooser so that the rules the
 * owner decided on 2026-09-28 are here with tests:
 *
 * - **Nothing is preselected.** A reader who has never chosen gets no
 *   timestamp, and no request goes anywhere --- a signing that reaches a server
 *   the reader never picked would be the network authority used without being
 *   asked.
 * - **A short list plus the reader's own URL.** {@link SERVERS} is the list
 *   `tsa::SERVERS` holds, and `signtimestamp.test.ts` compares the two through
 *   `wording.json`. *Other* takes an `http` or `https` address, which Rust
 *   judges again, because the webview is not what decides what is asked.
 * - **The choice is remembered** in `localStorage` under {@link CHOICE_KEY},
 *   the per-viewer store `signappearance.ts` uses, guarded the same way: storage
 *   that throws or holds anything tpdf did not write reads as the default,
 *   which is none.
 */

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

/** No timestamp: what a reader who has never chosen gets. */
export function noStamp(): StampChoice {
  return { server: "none", url: "" };
}

type Store = Pick<Storage, "getItem" | "setItem">;

/** Every value {@link StampChoice.server} may hold. */
function known(server: string): boolean {
  return server === "none" || server === "other" || SERVERS.some((s) => s.name === server);
}

/**
 * The remembered choice, or none. **Whole or nothing**, `readPreference`'s
 * rule: one field tpdf would not have written means none of it is trusted.
 */
export function readStampChoice(storage: () => Store = () => window.localStorage): StampChoice {
  let raw: string | null;
  try {
    raw = storage().getItem(CHOICE_KEY);
  } catch {
    return noStamp();
  }
  if (raw === null) return noStamp();
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

/** Keeps `choice`; `false` when storage refused, which changes nothing now. */
export function writeStampChoice(
  choice: StampChoice,
  storage: () => Store = () => window.localStorage,
): boolean {
  try {
    storage().setItem(CHOICE_KEY, JSON.stringify(choice));
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
  const trimmed = text.trim();
  if (trimmed === "") return "Type the timestamp authority's address.";
  if (trimmed.length > MAX_URL_CHARS) return "That address is too long.";
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return "That is not an address: it should begin with http:// or https://.";
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    return "tpdf asks timestamp authorities over http:// or https:// only.";
  }
  if (url.username !== "" || url.password !== "") {
    return "An address with a user name or password in it is not asked.";
  }
  return null;
}

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
