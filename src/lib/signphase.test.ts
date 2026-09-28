import { describe, expect, it } from "vitest";

import type { Integrity, Revocation, Trust } from "./integrity";
import { signatureRows, type Certificate, type Signature, type Timestamp } from "./properties";
import { closing, keyRequests, parseSignPhase, readBack, type ShownSection } from "./signphase";
import { afterSigning, type Signed } from "./signing";

const ID = "2a144cdb0facc6919f9163d7776c3c2f7f35bbd23021c002d61c29c7f0819c74";
const AT = "2026-09-28 16:43:51 UTC";

const intact: Integrity = { verdict: "intact", why: null, digest: "SHA-256", method: "RSA" };

function certificate(cn: string): Certificate {
  return {
    subject: `CN=${cn}`,
    subject_cn: cn,
    issuer: `CN=${cn}`,
    issuer_cn: cn,
    serial: "01",
    from: "2026-09-27 00:00:00 UTC",
    until: "2026-09-28 00:00:00 UTC",
    self_issued: true,
    chain: 1,
    matched_signer: true,
    key_usage: null,
    extended_usage: null,
    authority: false,
    extensions_unread: 0,
  };
}

function none(): Revocation {
  return {
    standing: "none",
    why: null,
    source: null,
    issued: "",
    next: "",
    revoked: "",
    reason: null,
    basis: "attested",
    moment: AT,
    after_moment: false,
  };
}

/** What the phase's step 2 writes, as the worker reads it back: the case every check passes. */
function stamped(): Signature {
  const timestamp: Timestamp = {
    when: AT,
    authority: certificate("DigiCert SHA256 RSA4096 Timestamp Responder 2025 1"),
    integrity: intact,
    trust: { standing: "trusted", why: null, store: "mac", attested_at: "" },
    attested: true,
    revocation: none(),
    revocation_chain: null,
  };
  const trust: Trust = { standing: "untrusted", why: "root", store: "mac", attested_at: AT };
  return {
    field: "Signature1",
    signed: true,
    handler: "Adobe.PPKLite",
    kind: "ETSI.CAdES.detached",
    name: "",
    reason: "",
    location: "",
    when: "",
    covers_whole_file: true,
    covered_bytes: 1000,
    appended_bytes: 0,
    appendix: null,
    certification: 0,
    certificate: certificate("tpdf TEST SIGNER - not a real identity"),
    timestamp,
    integrity: intact,
    trust,
    revocation: none(),
    revocation_chain: null,
  };
}

function shown(...signatures: Signature[]): ShownSection[] {
  return [
    { title: "File", rows: [{ name: "Pages", value: "1" }] },
    ...signatures.map((s) => ({ title: `Signature — ${s.field}`, rows: signatureRows(s, 1000) })),
  ];
}

function failing(sections: ShownSection[]): string[] {
  return readBack(sections, "DigiCert").filter((v) => !v.ok).map((v) => v.name);
}

describe("parseSignPhase", () => {
  it("reads the fixture, the directory and the identity", () => {
    expect(parseSignPhase(`/a/in.pdf|/a/room|${ID.toUpperCase()}`)).toEqual({
      fixture: "/a/in.pdf",
      room: "/a/room",
      identity: ID,
    });
  });

  it("refuses to run without an identity, or with a name for one", () => {
    expect(() => parseSignPhase("/a/in.pdf|/a/room|")).toThrow("SHA-256");
    expect(() => parseSignPhase("/a/in.pdf|/a/room")).toThrow("an identity");
    expect(() => parseSignPhase("/a/in.pdf|/a/room|tpdf TEST SIGNER")).toThrow("64 hex digits");
    expect(() => parseSignPhase(`/a/in.pdf|/a/room|${ID}|extra`)).toThrow("an identity");
  });
});

describe("readBack", () => {
  it("passes the copy a timestamped signing by a self-issued certificate writes", () => {
    const verdicts = readBack(shown(stamped()), "DigiCert");
    expect(verdicts.filter((v) => !v.ok)).toEqual([]);
    expect(verdicts.length).toBe(7);
  });

  it("fails a copy with no signature, or with two", () => {
    expect(failing(shown())).toContain("the saved copy carries one signature");
    expect(failing(shown(stamped(), { ...stamped(), field: "Signature2" }))).toEqual([
      "the saved copy carries one signature",
    ]);
  });

  it("fails a signature that does not read intact", () => {
    const altered = { ...stamped(), integrity: { ...intact, verdict: "altered" as const } };
    expect(failing(shown(altered))).toContain("its signature reads intact");
  });

  it("fails a timestamp that is not attested, or is another authority's", () => {
    const broken = stamped();
    broken.timestamp = { ...broken.timestamp!, integrity: { ...intact, verdict: "broken" } };
    expect(failing(shown(broken))).toContain("its timestamp reads attested by DigiCert");
    const other = stamped();
    other.timestamp = { ...other.timestamp!, authority: certificate("Somebody Else") };
    expect(failing(shown(other))).toEqual(["its timestamp reads attested by DigiCert"]);
  });

  it("fails an authority nobody vouches for", () => {
    const stranger = stamped();
    stranger.timestamp = {
      ...stranger.timestamp!,
      trust: { standing: "untrusted", why: "root", store: "mac", attested_at: "" },
    };
    expect(failing(shown(stranger))).toEqual(["its timestamp authority reads trusted"]);
  });

  it("fails a signer read as trusted, or judged now rather than at the timestamp", () => {
    const trusted = { ...stamped(), trust: { ...stamped().trust!, standing: "trusted" as const, why: null } };
    expect(failing(shown(trusted))).toEqual([
      "its signer reads untrusted at a root nobody vouches for",
      "its signer is judged at the time the timestamp attests",
    ]);
    const now = { ...stamped(), trust: { ...stamped().trust!, attested_at: "" } };
    expect(failing(shown(now))).toEqual(["its signer is judged at the time the timestamp attests"]);
  });

  it("fails revocation data the signing did not add, and a revocation row gone missing", () => {
    const good = stamped();
    good.revocation = { ...none(), standing: "good", source: "ocsp", issued: AT };
    expect(failing(shown(good))).toEqual(["every revocation row reads not checked"]);
    const unread = stamped();
    unread.timestamp = { ...unread.timestamp!, revocation: null };
    expect(failing(shown(unread))).toEqual(["every revocation row reads not checked"]);
  });
});

describe("closing", () => {
  function signed(longTerm: boolean, archive: boolean): Signed {
    const ours = stamped();
    return {
      path: "/tmp/room/digicert.pdf",
      field: "Signature1",
      signatures: [
        {
          field: "Signature1",
          integrity: intact,
          ours: true,
          timestamp: ours.timestamp,
          revocation: longTerm ? { ...none(), standing: "good", source: "ocsp", issued: AT } : none(),
        },
        ...(archive ? [{ field: "Signature2", integrity: intact, ours: false, archive: true }] : []),
      ],
    };
  }

  it("passes the sentence a timestamped signing without long-term data ends with", () => {
    const verdicts = closing(afterSigning(signed(false, false)), "digicert.pdf", "DigiCert");
    expect(verdicts.filter((v) => !v.ok)).toEqual([]);
  });

  it("fails a sentence naming another file, no timestamp, or long-term data", () => {
    const sentence = afterSigning(signed(false, false));
    expect(closing(sentence, "sectigo.pdf", "DigiCert").filter((v) => !v.ok).length).toBe(1);
    const unstamped = signed(false, false);
    unstamped.signatures[0]!.timestamp = null;
    expect(
      closing(afterSigning(unstamped), "digicert.pdf", "DigiCert").map((v) => v.ok),
    ).toEqual([true, false, true]);
    expect(
      closing(afterSigning(signed(true, false)), "digicert.pdf", "DigiCert").map((v) => v.ok),
    ).toEqual([true, true, false]);
    expect(
      closing(afterSigning(signed(false, true)), "digicert.pdf", "DigiCert").map((v) => v.ok),
    ).toEqual([true, true, false]);
  });
});

describe("keyRequests", () => {
  it("holds the count to exactly what the step asked for, both ways", () => {
    expect(keyRequests("once", 3, 4, 1).ok).toBe(true);
    expect(keyRequests("none again", 4, 4, 0).ok).toBe(true);
    expect(keyRequests("none again", 4, 5, 0).ok).toBe(false);
    expect(keyRequests("once", 3, 3, 1).ok).toBe(false);
    expect(keyRequests("once", 3, 5, 1).ok).toBe(false);
  });
});
