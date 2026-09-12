import { call } from "./ipc";
import { SIGNATURE_KEY, validSignature, type SignatureImage } from "./signature";

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
  if (!validSignature(image)) throw new Error("The old saved signature is damaged. Use Forget saved signature to remove it.");
  const existing = await store({ kind: "load" });
  if (existing && !same(existing, image)) throw new Error("The old and protected saved signatures differ. Use Forget saved signature before saving a new one.");
  if (!existing) await store({ kind: "save", image });
  const saved = await store({ kind: "load" });
  if (!saved || !same(saved, image)) throw new Error("The protected signature could not be verified. The old copy has been retained.");
  if (storage.getItem(SIGNATURE_KEY) !== raw) throw new Error("The old saved signature changed during migration.");
  storage.removeItem(SIGNATURE_KEY);
}

let migration: Promise<void> | undefined;
export function prepareSignatureStorage(): Promise<void> {
  return migration ??= migrateSignature(localStorage, protectedStore);
}
export async function loadSignature(): Promise<SignatureImage | null> {
  await prepareSignatureStorage();
  const image = await protectedStore({ kind: "load" });
  if (image !== null && !validSignature(image)) throw new Error("The protected signature is damaged.");
  return image;
}
export async function rememberSignature(image: SignatureImage): Promise<void> {
  await prepareSignatureStorage();
  if (!validSignature(image)) throw new Error("Invalid signature pixels.");
  await protectedStore({ kind: "save", image });
}
export async function forgetSignature(): Promise<void> {
  // Wait for any in-flight migration, including a failed one, before deleting.
  await migration?.catch(() => {});
  await protectedStore({ kind: "forget" });
  localStorage.removeItem(SIGNATURE_KEY);
  migration = Promise.resolve();
}
