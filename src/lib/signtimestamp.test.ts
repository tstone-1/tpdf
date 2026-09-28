import { describe, expect, it } from "vitest";

import wording from "../../src-tauri/testdata/cli/wording.json";
import {
  CHOICE_KEY,
  MAX_URL_CHARS,
  SERVERS,
  addressProblem,
  noStamp,
  readStampChoice,
  stampUrl,
  writeStampChoice,
} from "./signtimestamp";

/** A storage double holding `raw` under the key, or throwing. */
function holding(raw: string | null, throws = false) {
  return () => ({
    getItem: (key: string) => {
      if (throws) throw new Error("storage refused");
      return key === CHOICE_KEY ? raw : null;
    },
    setItem: () => {
      if (throws) throw new Error("storage refused");
    },
  });
}

describe("the timestamp authorities", () => {
  it("are the backend's list, name for name and address for address", () => {
    expect(SERVERS).toEqual(wording.timestamp_servers);
  });

  it("are asked only when chosen: none is what a reader who never chose gets", () => {
    expect(readStampChoice(holding(null))).toEqual(noStamp());
    expect(stampUrl(noStamp())).toBeNull();
    for (const server of SERVERS) {
      expect(stampUrl({ server: server.name, url: "" })).toBe(server.url);
    }
  });
});

describe("the remembered choice", () => {
  it("reads back what was written", () => {
    let kept: string | null = null;
    const storage = () => ({
      getItem: () => kept,
      setItem: (_key: string, value: string) => {
        kept = value;
      },
    });
    expect(writeStampChoice({ server: "other", url: "https://tsa.example/" }, storage)).toBe(true);
    expect(readStampChoice(storage)).toEqual({ server: "other", url: "https://tsa.example/" });
  });

  it("falls back to none, whole, for anything tpdf did not write", () => {
    for (const raw of [
      "not json",
      "null",
      "42",
      JSON.stringify({ server: "verisign", url: "" }),
      JSON.stringify({ server: 3, url: "" }),
      JSON.stringify({ server: "digicert" }),
      JSON.stringify({ server: "other", url: 7 }),
      JSON.stringify({ server: "other", url: "h".repeat(MAX_URL_CHARS + 1) }),
    ]) {
      expect(readStampChoice(holding(raw)), raw).toEqual(noStamp());
    }
    expect(readStampChoice(holding(null, true))).toEqual(noStamp());
    expect(writeStampChoice(noStamp(), holding(null, true))).toBe(false);
  });
});

describe("another authority's address", () => {
  it("is asked only over http or https, with no credentials in it", () => {
    for (const good of ["http://tsa.example", "https://tsa.example/x", "  https://tsa.example  "]) {
      expect(addressProblem(good), good).toBeNull();
    }
    for (const bad of [
      "",
      "tsa.example",
      "ftp://tsa.example/",
      "javascript:alert(1)",
      "https://user:pw@tsa.example/",
      `https://tsa.example/${"x".repeat(MAX_URL_CHARS)}`,
    ]) {
      expect(addressProblem(bad), bad).not.toBeNull();
    }
  });

  it("never turns a mistyped address into no timestamp by accident", () => {
    // `stampUrl` answers null for it --- and the chooser holds *Sign* until it
    // is fixed, which `signing.test.ts` holds.
    expect(stampUrl({ server: "other", url: "ftp://x" })).toBeNull();
    expect(stampUrl({ server: "other", url: " https://tsa.example/ " })).toBe("https://tsa.example/");
  });
});
