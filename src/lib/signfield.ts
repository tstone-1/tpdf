/**
 * The empty signature fields of the open document, as places to sign.
 *
 * A signature field that holds no signature is a place for one. The window
 * offers each as a button on its page, and *Sign in the signature field…*
 * takes the first; both hand the signing the field's name, which is what the
 * worker finds it by, and where it is, which is where a visible signature is
 * drawn. Which fields those are and where is decided here, so that it has
 * tests: `App.svelte` only joins it to the signing.
 */

import type { Form, FormWidget } from "./forms";
import type { PageId, PageView } from "./pages";

/** A field to sign: `signing.ts` sends the name, and draws in the rectangle. */
export interface SignTarget {
  /** The field's full name. */
  name: string;
  page: PageId;
  /** `left, top, right, bottom` in points, as the page is displayed. */
  rect: [number, number, number, number];
}

/** Only a named empty signature field can be addressed by the signing worker. */
export function isEmptySignature(widget: FormWidget): boolean {
  return widget.name !== "" && widget.control.kind === "signature" && !widget.control.signed;
}

// Scans are replaced on reopen. Pending edits live outside the scanned form.
const signable = new WeakMap<Form, readonly FormWidget[]>();

/**
 * The signature fields somebody can sign, in the order the form lists them.
 * A field shown in several places is left out: a signature goes in one.
 */
export function emptySignatureFields(form: Form | null): readonly FormWidget[] {
  if (!form) return [];
  const known = signable.get(form);
  if (known) return known;
  const counts = new Map<string, number>();
  for (const widget of form.widgets) counts.set(widget.name, (counts.get(widget.name) ?? 0) + 1);
  const fields = form.widgets.filter((widget) => isEmptySignature(widget) && counts.get(widget.name) === 1);
  signable.set(form, fields);
  return fields;
}

/**
 * Where a field is, for the signing, or `null` when its page is not one of the
 * file's as it is shown: signing refuses unsaved changes, so it then is not asked.
 */
export function signTarget(widget: FormWidget, pages: readonly PageView[]): SignTarget | null {
  const page = pages.find((one) => "baseline" in one.source && one.source.baseline === widget.page)?.id;
  if (page === undefined || !isEmptySignature(widget)) return null;
  return { name: widget.name, page, rect: [...widget.display_rect] };
}
