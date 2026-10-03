/**
 * A document made from pictures, from the window: which files the panel
 * offers, the name the document is suggested, and the sentence afterwards.
 *
 * `App.svelte` keeps the two panels and the `invoke`. The pages are made by
 * `src-tauri/src/imagepages.rs`, in a worker.
 */

import { basename } from "./paths";

/** What `write_images` reports. `Made` in `save.rs`. */
export interface Made {
  pages: number;
}

/** The extensions the open panel offers. The backend decides by content. */
export const EXTENSIONS = ["png", "jpg", "jpeg"];

/**
 * `holiday.jpg` gives `holiday.pdf`.
 *
 * The first picture's name, because it is the one a reader chose first and
 * the panel opens in its folder.
 */
export function suggestedName(first: string): string {
  return `${basename(first).replace(/\.(png|jpe?g)$/i, "")}.pdf`;
}

/**
 * What to say once the document is written.
 *
 * Said because the window is about to show a file the reader has not seen,
 * and the count is the evidence that every picture went in.
 */
export function afterPictures(made: Made, path: string): string {
  const pages = made.pages === 1 ? "1 page" : `${made.pages} pages`;
  return `Saved ${basename(path)}: ${pages}, one for each picture.`;
}
