/**
 * Whether a signing locks the document, asked as a question about the people.
 *
 * A signature can say that nothing may change after it (`sign_prepare.rs`,
 * *The lock*): readers that honour it then report any later change as one the
 * signer did not permit, and a locked document takes no further signature.
 *
 * So the chooser asks whether anybody else still has to sign, and:
 *
 * - **Nothing is selected until the reader answers**, and *Sign…* holds the
 *   chooser open until they do. Until 2026-10-15 this was a checkbox, ticked
 *   until the reader unticked it and remembered (the owner's decision of
 *   2026-10-13). The audit of 26.10.18 named the cost of that default: the
 *   person it stops is not the signer but the colleague who receives the
 *   locked copy and cannot countersign, and a consequence written at the end
 *   of a checkbox label is easy to pass. A reader who answers *No* gets the
 *   same lock as before; nobody gets one without choosing it.
 * - **The answer is not remembered.** Whether somebody else signs after you is
 *   a fact about this document, and a remembered answer would be the silent
 *   default again, one document later. The old checkbox's key,
 *   `tpdf.signatureLock`, is no longer read.
 */

import { catalog } from "./i18n";

const TEXT = catalog({
  "en-US": {
    question: "Does anyone else still need to sign this document?",
    others: "Yes, leave it open for their signatures",
    last: "No, I am the last to sign: lock it",
    lastHint:
      "The signature then says that nothing may change after it. A program that checks " +
      "signatures reports any later change, and no further signature can be added.",
    unanswered:
      "Say whether anyone else still needs to sign: a locked document takes no further signature.",
  },
  "de-DE": {
    question: "Muss noch jemand anderes dieses Dokument signieren?",
    others: "Ja, für weitere Signaturen offen lassen",
    last: "Nein, nach mir signiert niemand mehr: Dokument sperren",
    lastHint:
      "Die Signatur besagt dann, dass sich nach ihr nichts mehr ändern darf. Ein Programm, " +
      "das Signaturen prüft, meldet jede spätere Änderung, und es kann keine weitere Signatur " +
      "hinzugefügt werden.",
    unanswered:
      "Gib an, ob noch jemand anderes signieren muss: Ein gesperrtes Dokument nimmt keine " +
      "weitere Signatur an.",
  },
  "de-DE-formal": {
    unanswered: "Geben Sie an, ob noch jemand anderes signieren muss: Ein gesperrtes Dokument nimmt keine " +
      "weitere Signatur an.",
  },
});

const T = TEXT();

/** The radio group's name in the chooser. */
export const LOCK_NAME = "sign-lock";

/** The question, as the chooser asks it. */
export const LOCK_QUESTION = T.question;

/**
 * The two answers: the radio's value, its words, the smaller line under them
 * (empty for none), and whether it locks. What a lock does stays on screen
 * beside the answer that makes one, as the line under it.
 */
export const LOCK_ANSWERS: readonly { value: string; text: string; hint: string; lock: boolean }[] = [
  {
    value: "others",
    text: T.others,
    hint: "",
    lock: false,
  },
  {
    value: "last",
    text: T.last,
    hint: T.lastHint,
    lock: true,
  },
];

/** What *Sign…* says while the question has no answer. */
export const LOCK_UNANSWERED = T.unanswered;

/** Whether the chosen answer locks, or `null` while none is chosen or the value is not one of ours. */
export function lockOf(answer: string | undefined): boolean | null {
  return LOCK_ANSWERS.find((one) => one.value === answer)?.lock ?? null;
}
