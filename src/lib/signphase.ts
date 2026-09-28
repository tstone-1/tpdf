/**
 * What the window's signing phase decides, apart from the window.
 *
 * `signingcheck.ts` drives *Sign document…* through the palette, the chooser,
 * the question after long-term data did not come and the properties dialog,
 * and reads what each one put on screen. Everything it concludes from what it
 * read is here instead, with tests, for the reason `AGENTS.md` gives for
 * `App.svelte`: a phase that needs a screen, the network and a keychain prompt
 * is run by hand, so its own judgements have to be provable without any of
 * those. `signphase.test.ts` feeds each function the rows and sentences
 * `properties.ts` and `signing.ts` really produce, then the ones a regression
 * would, and requires each check to go red on its own case.
 */

/** The phase's arguments: `tabs_check.py --phase sign` sends them. */
export interface SignPhase {
  /** A disposable copy of the fixture, which is signed and never written. */
  fixture: string;
  /** A directory of its own, where each signing writes its copy. */
  room: string;
  /** The identity to sign with: the SHA-256 of its certificate, lower-case hex. */
  identity: string;
}

/**
 * `TPDF_OPENCHECK`'s value after `sign:` --- `fixture|room|identity` --- or a
 * thrown reason. The identity is a certificate's SHA-256 and nothing else: a
 * name would let the phase sign with whichever certificate happens to match it,
 * and there is no default, because the phase signs with a real key.
 */
export function parseSignPhase(expected: string): SignPhase {
  const [fixture, room, identity, ...rest] = expected.split("|");
  if (!fixture || !room || identity === undefined || rest.length > 0) {
    throw new Error("the signing phase needs a fixture, a directory and an identity");
  }
  const id = identity.trim().toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(id)) {
    throw new Error(
      `the identity must be the SHA-256 of its certificate, 64 hex digits, not "${identity}"`,
    );
  }
  return { fixture, room, identity: id };
}

/** One check's outcome, for `Report.check`. */
export interface Verdict {
  name: string;
  ok: boolean;
  detail: string;
}

/** One block of the properties dialog, as read off the screen. */
export interface ShownSection {
  title: string;
  rows: { name: string; value: string }[];
}

/** A value, cut for a detail column. */
function cut(text: string, length = 160): string {
  return text.length > length ? `${text.slice(0, length)}…` : text;
}

/**
 * What the properties dialog must say about a copy signed with a timestamp
 * from `authority` and no long-term data, by a self-issued certificate.
 *
 * Each expectation is the one a regression would break by itself: the
 * signature intact; the token intact *and attested*, by the authority chosen
 * rather than whoever answered; that authority trusted, judged now; the signer
 * untrusted at a root nobody vouches for, judged at the time the token
 * attests; and every revocation row *not checked*, because a signing without
 * long-term data adds none and a reader must never be told otherwise.
 */
export function readBack(sections: ShownSection[], authority: string): Verdict[] {
  const signed = sections.filter(
    (section) =>
      section.title.startsWith("Signature") &&
      section.rows.some((row) => row.name === "Integrity"),
  );
  const verdicts: Verdict[] = [
    {
      name: "the saved copy carries one signature",
      ok: signed.length === 1,
      detail: signed.map((section) => section.title).join(", ") || "none",
    },
  ];
  const rows = signed[0]?.rows ?? [];
  const row = (name: string) => rows.find((r) => r.name === name)?.value ?? "";
  const integrity = row("Integrity");
  verdicts.push({
    name: "its signature reads intact",
    ok: integrity.startsWith("intact —"),
    detail: cut(integrity),
  });
  const stamped = row("Timestamped");
  verdicts.push({
    name: `its timestamp reads attested by ${authority}`,
    ok: stamped.includes(", attested by ") && stamped.includes(authority),
    detail: cut(stamped),
  });
  const vouched = row("Timestamp authority");
  verdicts.push({
    name: "its timestamp authority reads trusted",
    ok: vouched.startsWith("trusted —"),
    detail: cut(vouched),
  });
  const trust = row("Trust");
  verdicts.push({
    name: "its signer reads untrusted at a root nobody vouches for",
    ok: trust.startsWith("not trusted") && trust.includes("its chain ends at a root"),
    detail: cut(trust),
  });
  verdicts.push({
    name: "its signer is judged at the time the timestamp attests",
    ok: /^not trusted, judged at [^—]+, the time the timestamp attests —/.test(trust),
    detail: cut(trust),
  });
  const revocations = rows.filter((r) => /revocation$/i.test(r.name));
  verdicts.push({
    name: "every revocation row reads not checked",
    // The signer's and the authority's at least: fewer is a row gone missing,
    // and an empty list would make "every" true of nothing.
    ok:
      revocations.length >= 2 &&
      revocations.every((r) => r.value.startsWith("not checked —")),
    detail: revocations.map((r) => `${r.name}: ${cut(r.value, 40)}`).join("; ") || "none",
  });
  return verdicts;
}

/**
 * What the sentence after signing must say: written to `name`, read back
 * intact, timestamped by `authority` --- and, since no long-term data was
 * added, nothing about revocation.
 */
export function closing(sentence: string, name: string, authority: string): Verdict[] {
  return [
    {
      name: `the closing sentence says ${name} was written and read back intact`,
      ok:
        sentence.includes(`and saved to ${name}.`) &&
        sentence.includes("Read back after writing, the signature is intact."),
      detail: cut(sentence),
    },
    {
      name: `the closing sentence names the timestamp from ${authority}`,
      ok:
        sentence.includes(" Timestamp: ") &&
        sentence.includes(", attested by ") &&
        sentence.includes(authority) &&
        sentence.includes(" Timestamp authority: trusted —"),
      detail: cut(sentence, 400),
    },
    {
      // The file named, so the two signings that write one do not share a
      // check name, which the harness refuses (first run, 2026-09-28).
      name: `the closing sentence for ${name} claims no revocation data`,
      ok: !sentence.includes(" Revocation: ") && !sentence.includes(" Archive timestamp: "),
      detail: cut(sentence, 400),
    },
  ];
}

/**
 * Whether the OS was asked for a key `expected` more times between two
 * readings of `sign_record`. Exact, both ways: one request too many is the
 * defect the question after a failed timestamp exists to prevent, and one too
 * few is a count that is not counting --- which would make every "asked for
 * nothing" beside it true for no reason.
 */
export function keyRequests(name: string, before: number, after: number, expected: number): Verdict {
  return {
    name,
    ok: after - before === expected,
    detail: `${before} -> ${after}, expected +${expected}`,
  };
}
