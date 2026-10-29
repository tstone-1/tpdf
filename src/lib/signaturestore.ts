import { call } from "./ipc";
import { SIGNATURE_KEY, validSignature, type SignatureImage } from "./signature";
import { catalog } from "./i18n";

/** The store's refusals; the signature dialog shows them. "Forget saved signature" is its button. */
const TEXT = catalog({
  "en-US": {
    oldDamaged: "The old saved signature is damaged. Use Forget saved signature to remove it.",
    differ: "The old and protected saved signatures differ. Use Forget saved signature before saving a new one.",
    unverified: "The protected signature could not be verified. The old copy has been retained.",
    changed: "The old saved signature changed during migration.",
    damaged: "The protected signature is damaged.",
    invalid: "Invalid signature pixels.",
  },
  "de-DE": {
    oldDamaged: "Die alte gespeicherte Unterschrift ist beschädigt. Entferne sie mit „Gespeicherte Unterschrift entfernen“.",
    differ: "Die alte und die geschützte gespeicherte Unterschrift unterscheiden sich. Wähle „Gespeicherte Unterschrift entfernen“, bevor du eine neue speicherst.",
    unverified: "Die geschützte Unterschrift konnte nicht überprüft werden. Die alte Kopie wurde behalten.",
    changed: "Die alte gespeicherte Unterschrift hat sich während der Übernahme geändert.",
    damaged: "Die geschützte Unterschrift ist beschädigt.",
    invalid: "Die Pixel der Unterschrift sind ungültig.",
  },
  "de-DE-formal": {
    oldDamaged: "Die alte gespeicherte Unterschrift ist beschädigt. Entfernen Sie sie mit „Gespeicherte Unterschrift entfernen“.",
    differ: "Die alte und die geschützte gespeicherte Unterschrift unterscheiden sich. Wählen Sie „Gespeicherte Unterschrift entfernen“, bevor Sie eine neue speichern.",
  },
});

type Action = { kind: "load" } | { kind: "save"; image: SignatureImage } | { kind: "forget" };
export type SignatureStorage = (action: Action) => Promise<SignatureImage | null>;
const protectedStore: SignatureStorage = (action) => call("signature_store", { action });
const same = (a: SignatureImage, b: SignatureImage) => a.width === b.width && a.height === b.height
  && a.rgba.length === b.rgba.length && a.rgba.every((v, i) => v === b.rgba[i]);

/** Move legacy pixels only after the protected copy has been independently read back. */
export async function migrateSignature(storage: Pick<Storage, "getItem" | "removeItem">,
  store: SignatureStorage): Promise<void> {
  const raw = storage.getItem(SIGNATURE_KEY);
  if (raw === null) return;
  const image: unknown = JSON.parse(raw);
  if (!validSignature(image)) throw new Error(TEXT().oldDamaged);
  const existing = await store({ kind: "load" });
  if (existing && !same(existing, image)) throw new Error(TEXT().differ);
  if (!existing) await store({ kind: "save", image });
  const saved = await store({ kind: "load" });
  if (!saved || !same(saved, image)) throw new Error(TEXT().unverified);
  if (storage.getItem(SIGNATURE_KEY) !== raw) throw new Error(TEXT().changed);
  storage.removeItem(SIGNATURE_KEY);
}

let migration: Promise<void> | undefined;
export function prepareSignatureStorage(): Promise<void> {
  return migration ??= migrateSignature(localStorage, protectedStore);
}
export async function loadSignature(): Promise<SignatureImage | null> {
  await prepareSignatureStorage();
  const image = await protectedStore({ kind: "load" });
  if (image !== null && !validSignature(image)) throw new Error(TEXT().damaged);
  return image;
}
export async function rememberSignature(image: SignatureImage): Promise<void> {
  await prepareSignatureStorage();
  if (!validSignature(image)) throw new Error(TEXT().invalid);
  await protectedStore({ kind: "save", image });
}
export async function forgetSignature(): Promise<void> {
  // Wait for any in-flight migration, including a failed one, before deleting.
  await migration?.catch(() => {});
  await protectedStore({ kind: "forget" });
  localStorage.removeItem(SIGNATURE_KEY);
  migration = Promise.resolve();
}
