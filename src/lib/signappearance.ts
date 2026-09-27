/**
 * What a visible signature shows: the panel between the chooser and the drag.
 *
 * When the reader picks **Visible** in *Sign document…*, this panel asks what
 * the appearance holds before any rectangle is placed --- an image (the saved
 * visual signature, one drawn or imported now through the Phase 4
 * `SignatureDialog`, or none), which of the three lines are drawn, and a reason
 * and location --- with a **live preview**. It is a module rather than markup in
 * `App.svelte` because that file is the layer no gate reaches (`AGENTS.md`):
 * the state, the preference and the order of what is asked are all here with
 * tests, and `App.svelte` supplies the preview command, the drawing dialog and
 * nothing else.
 *
 * ## The preview is the signing's own drawing
 *
 * Nothing here draws the appearance. {@link PanelShell.preview} is the
 * `sign_preview` command, which runs the worker's signing code
 * (`sign_prepare::prepare_visible`) over a blank page the size of
 * {@link PREVIEW_SIZE} and renders it with PDFium (`sign_prepare::preview`).
 * So the words, the layout and every refusal the reader sees are the ones the
 * signing will apply; the panel shows the refusal as the preview's message and
 * holds *Place on page* until the choices draw. The signing lays out again for
 * the rectangle actually placed, which is the same code at another size.
 *
 * ## What is remembered, and where
 *
 * Ticking *Remember as my default* keeps the choices --- whether an image is
 * shown, which lines are on, the reason and the location --- in `localStorage`
 * under {@link PREFERENCE_KEY}, the store tpdf uses for its per-machine
 * preferences (`tablabels.ts`, `update.ts`), guarded the same way: storage that
 * throws or holds anything unexpected gives the defaults. **The image is not
 * part of it**: pixels have their own protected store (`signaturestore.ts`), and
 * a preference only says "the saved one" or "none". An image drawn for one
 * signing and not remembered there is not kept anywhere.
 */

import type { SignatureImage } from "./signature";

/** Which lines are drawn, and the reason and location. Mirrors `sign_prepare::Options`. */
export interface AppearanceOptions {
  /** *Digitally signed by*. */
  label: boolean;
  /** The certificate's subject name. Never editable: whether it is drawn, only. */
  name: boolean;
  /** The signing time. */
  date: boolean;
  /** Why it is signed; blank for none. Written as `/Reason` too. */
  reason: string;
  /** Where it was signed; blank for none. Written as `/Location` too. */
  location: string;
}

/** A drawn appearance. Mirrors `sign_prepare::Preview`. */
export interface SignaturePreview {
  /** PNG bytes, two pixels a point. */
  png: number[];
  /** Pixels. */
  width: number;
  /** Pixels. */
  height: number;
}

/** What the panel answers: the image to draw, if any, and the options. */
export interface Appearance {
  image: SignatureImage | null;
  options: AppearanceOptions;
}

/** Where the image comes from. `drawn` is one made in this panel. */
export type ImageSource = "saved" | "drawn" | "none";

/** One of the three switchable lines. */
export type Line = "label" | "name" | "date";

/** What is kept between signings. */
export interface Preference {
  /** Whether an image is shown: the saved one, or none. */
  image: "saved" | "none";
  options: AppearanceOptions;
}

/**
 * The rectangle the preview is drawn for, in points: three to one, the shape a
 * signature line leaves room for, and the size `sign-probe` measures by default.
 */
export const PREVIEW_SIZE: [number, number] = [240, 80];

/** Where {@link Preference} lives. */
export const PREFERENCE_KEY = "tpdf.signatureAppearance";

/**
 * The longest reason or location, in characters. `appearance::MAX_NOTE_CHARS`
 * is the rule and refuses longer; this is the input's `maxlength`, and a stored
 * preference longer than it is not one tpdf wrote.
 */
export const MAX_NOTE_CHARS = 256;

/** The three lines, no reason, no location, and the saved image when there is one. */
export const DEFAULT_PREFERENCE: Preference = {
  image: "saved",
  options: { label: true, name: true, date: true, reason: "", location: "" },
};

type Store = Pick<Storage, "getItem" | "setItem">;

/** A copy of the defaults, so a caller changing it changes nothing shared. */
function defaults(): Preference {
  return { image: DEFAULT_PREFERENCE.image, options: { ...DEFAULT_PREFERENCE.options } };
}

/**
 * The remembered choices, or the defaults when there are none or they are not
 * what tpdf writes. **Whole or nothing**: one field of the wrong type means the
 * rest were not written by this code either, so none of them is trusted.
 */
export function readPreference(storage: () => Store = () => window.localStorage): Preference {
  let raw: string | null;
  try {
    raw = storage().getItem(PREFERENCE_KEY);
  } catch {
    return defaults();
  }
  if (raw === null) return defaults();
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return defaults();
  }
  if (typeof value !== "object" || value === null) return defaults();
  const { image, options } = value as { image?: unknown; options?: unknown };
  if (image !== "saved" && image !== "none") return defaults();
  if (typeof options !== "object" || options === null) return defaults();
  const o = options as Record<string, unknown>;
  const flag = (v: unknown): v is boolean => typeof v === "boolean";
  const text = (v: unknown): v is string => typeof v === "string" && v.length <= MAX_NOTE_CHARS;
  if (!flag(o.label) || !flag(o.name) || !flag(o.date) || !text(o.reason) || !text(o.location)) {
    return defaults();
  }
  return {
    image,
    options: { label: o.label, name: o.name, date: o.date, reason: o.reason, location: o.location },
  };
}

/** Keeps `preference`; `false` when storage refused, which changes nothing now. */
export function writePreference(
  preference: Preference,
  storage: () => Store = () => window.localStorage,
): boolean {
  try {
    storage().setItem(PREFERENCE_KEY, JSON.stringify(preference));
    return true;
  } catch {
    return false;
  }
}

/** What the panel's state needs from outside. */
export interface PanelShell {
  /** The reader's saved visual signature, or `null` when there is none. */
  saved: SignatureImage | null;
  /** `sign_preview` for these choices. A refusal is thrown, with the signing's words. */
  preview(image: SignatureImage | null, options: AppearanceOptions): Promise<SignaturePreview>;
  /** Puts a finished preview on screen. */
  show(preview: SignaturePreview): void | Promise<void>;
  /** Where the preference is kept. */
  storage?: () => Store;
}

/** Where the preview stands, for the words under it and for *Place on page*. */
export type PreviewState =
  | { kind: "drawing" }
  | { kind: "shown" }
  | { kind: "refused"; why: string };

/**
 * The panel's state, with no DOM: what is chosen, the preview's standing, and
 * what *Place on page* answers.
 *
 * **The latest preview wins.** Each change starts a preview and numbers it; an
 * answer for an earlier number is dropped, so a slow render for the choices
 * two clicks ago cannot paint over the current one --- nor, after the panel
 * has closed, paint at all.
 */
export class AppearancePanel {
  source: ImageSource;
  drawn: SignatureImage | null = null;
  options: AppearanceOptions;
  remember = false;
  state: PreviewState = { kind: "drawing" };
  #generation = 0;
  #closed = false;

  constructor(
    private readonly shell: PanelShell,
    preference: Preference = readPreference(shell.storage),
  ) {
    this.options = { ...preference.options };
    // A preference for the saved image with none saved is words alone: the
    // reader is not asked to draw one before they can sign.
    this.source = preference.image === "saved" && shell.saved ? "saved" : "none";
  }

  /** The image the choices draw. */
  image(): SignatureImage | null {
    if (this.source === "saved") return this.shell.saved;
    if (this.source === "drawn") return this.drawn;
    return null;
  }

  /** Chooses where the image comes from; a source with nothing in it is ignored. */
  setSource(source: ImageSource): boolean {
    if (source === "saved" && !this.shell.saved) return false;
    if (source === "drawn" && !this.drawn) return false;
    this.source = source;
    return true;
  }

  /** An image drawn or imported in this panel, which becomes the one shown. */
  useDrawn(image: SignatureImage): void {
    this.drawn = image;
    this.source = "drawn";
  }

  setLine(line: Line, on: boolean): void {
    this.options[line] = on;
  }

  setReason(text: string): void {
    this.options.reason = text;
  }

  setLocation(text: string): void {
    this.options.location = text;
  }

  /** Whether *Place on page* may be pressed: not while the choices are refused. */
  canContinue(): boolean {
    return !this.#closed && this.state.kind !== "refused";
  }

  /** Draws the current choices; resolves when this preview is shown, refused or superseded. */
  async refresh(): Promise<void> {
    if (this.#closed) return;
    const generation = ++this.#generation;
    this.state = { kind: "drawing" };
    const options = { ...this.options };
    try {
      const preview = await this.shell.preview(this.image(), options);
      if (this.#closed || generation !== this.#generation) return;
      await this.shell.show(preview);
      if (this.#closed || generation !== this.#generation) return;
      this.state = { kind: "shown" };
    } catch (error) {
      if (this.#closed || generation !== this.#generation) return;
      this.state = { kind: "refused", why: error instanceof Error ? error.message : String(error) };
    }
  }

  /** The answer for *Place on page*, keeping the choices first when asked to. */
  finish(): Appearance {
    this.#closed = true;
    if (this.remember) {
      writePreference(
        { image: this.source === "none" ? "none" : "saved", options: { ...this.options } },
        this.shell.storage,
      );
    }
    return { image: this.image(), options: { ...this.options } };
  }

  /** Cancel: nothing is remembered, and no preview still on its way is shown. */
  cancel(): void {
    this.#closed = true;
  }
}

/** Paints a preview's PNG onto a canvas, decoded by the webview. */
export async function paintPreview(canvas: HTMLCanvasElement, preview: SignaturePreview): Promise<void> {
  const bitmap = await createImageBitmap(new Blob([new Uint8Array(preview.png)], { type: "image/png" }));
  try {
    canvas.width = preview.width;
    canvas.height = preview.height;
    const context = canvas.getContext("2d");
    context?.clearRect(0, 0, canvas.width, canvas.height);
    context?.drawImage(bitmap, 0, 0);
  } finally {
    bitmap.close();
  }
}

/** What {@link askAppearance} needs from the shell. */
export interface AskShell {
  saved: SignatureImage | null;
  preview: PanelShell["preview"];
  /** The Phase 4 drawing dialog: an image, or `null` when it was cancelled. */
  draw(): Promise<SignatureImage | null>;
  storage?: () => Store;
  /** Replaces {@link paintPreview}, for a test with no decoder. */
  paint?: (canvas: HTMLCanvasElement, preview: SignaturePreview) => void | Promise<void>;
  /** How long typing waits before the preview is redrawn, in milliseconds. */
  typingDelay?: number;
}

/**
 * The panel as a modal dialog: the preview, the image, the three lines, the
 * reason and location, *Remember as my default*, Cancel and *Place on page*.
 *
 * Built from `textContent` and `value` only. Answers the choices, or `null`
 * for Cancel or Escape --- after which nothing is remembered and nothing still
 * drawing is shown.
 */
export function askAppearance(shell: AskShell): Promise<Appearance | null> {
  const previous = document.activeElement as HTMLElement | null;
  const canvas = document.createElement("canvas");
  const paint = shell.paint ?? paintPreview;
  const panel = new AppearancePanel({
    saved: shell.saved,
    preview: shell.preview,
    show: (preview) => paint(canvas, preview),
    ...(shell.storage ? { storage: shell.storage } : {}),
  });

  const dialog = document.createElement("dialog");
  dialog.className = "sign-appearance-dialog";
  dialog.setAttribute("aria-label", "Signature appearance");
  dialog.style.cssText =
    "width:min(560px,90vw);padding:22px;border:1px solid #8885;border-radius:12px;" +
    "background:Canvas;color:CanvasText;box-shadow:0 15px 70px #0005";
  const heading = document.createElement("h2");
  heading.textContent = "Signature appearance";
  heading.style.margin = "0 0 8px";
  const help = document.createElement("p");
  help.textContent =
    "Choose what the signature shows. You place it on a page next; it is laid out again " +
    "for the rectangle you drag.";

  canvas.setAttribute("role", "img");
  canvas.setAttribute("aria-label", "Preview of the signature");
  canvas.width = PREVIEW_SIZE[0] * 2;
  canvas.height = PREVIEW_SIZE[1] * 2;
  canvas.style.cssText =
    `width:${PREVIEW_SIZE[0]}px;height:${PREVIEW_SIZE[1]}px;display:block;margin:0 auto;` +
    "background:white;border:1px solid #999;border-radius:4px";
  const status = document.createElement("p");
  status.setAttribute("role", "status");
  status.style.cssText = "min-height:1.5em;font-size:13px;margin:6px 0";

  const group = (label: string) => {
    const set = document.createElement("fieldset");
    set.style.cssText = "border:none;padding:0;margin:10px 0;display:flex;gap:6px 14px;flex-wrap:wrap";
    const legend = document.createElement("legend");
    legend.textContent = label;
    legend.style.cssText = "font-weight:600;margin-bottom:4px";
    set.append(legend);
    return set;
  };
  const labelled = (input: HTMLInputElement, text: string) => {
    const label = document.createElement("label");
    label.append(input, ` ${text}`);
    return label;
  };

  // The image.
  const images = group("Image");
  const radio = (value: ImageSource, text: string) => {
    const input = document.createElement("input");
    input.type = "radio";
    input.name = "sign-appearance-image";
    input.value = value;
    images.append(labelled(input, text));
    return input;
  };
  const saved = radio("saved", "Saved signature");
  const drawn = radio("drawn", "The one drawn now");
  const none = radio("none", "No image");
  const draw = document.createElement("button");
  draw.type = "button";
  draw.textContent = "Draw or import…";
  images.append(draw);

  // The lines.
  const lines = group("Text");
  const check = (line: Line, text: string) => {
    const input = document.createElement("input");
    input.type = "checkbox";
    input.name = `sign-appearance-${line}`;
    lines.append(labelled(input, text));
    return input;
  };
  const checks: Record<Line, HTMLInputElement> = {
    label: check("label", "“Digitally signed by”"),
    name: check("name", "Your name, from the certificate"),
    date: check("date", "Date and time"),
  };

  // The reason and location.
  const notes = group("Optional");
  const field = (name: string, text: string) => {
    const input = document.createElement("input");
    input.type = "text";
    input.name = name;
    input.maxLength = MAX_NOTE_CHARS;
    input.placeholder = text;
    input.setAttribute("aria-label", text);
    input.style.cssText = "flex:1 1 200px;padding:5px";
    notes.append(input);
    return input;
  };
  const reason = field("sign-appearance-reason", "Reason");
  const location = field("sign-appearance-location", "Location");

  const remember = document.createElement("input");
  remember.type = "checkbox";
  remember.name = "sign-appearance-remember";
  const keeping = labelled(remember, "Remember as my default");

  const footer = document.createElement("div");
  footer.style.cssText = "display:flex;gap:10px;justify-content:flex-end;margin-top:12px";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.textContent = "Cancel";
  const place = document.createElement("button");
  place.type = "button";
  place.textContent = "Place on page…";
  footer.append(cancel, place);

  dialog.append(heading, help, canvas, status, images, lines, notes, keeping, footer);
  document.body.append(dialog);

  /** Puts the state back on the controls. */
  const render = () => {
    saved.disabled = !shell.saved;
    drawn.disabled = !panel.drawn;
    saved.checked = panel.source === "saved";
    drawn.checked = panel.source === "drawn";
    none.checked = panel.source === "none";
    for (const line of ["label", "name", "date"] as const) checks[line].checked = panel.options[line];
    const state = panel.state;
    status.textContent =
      state.kind === "drawing" ? "Drawing the preview…" : state.kind === "refused" ? state.why : "";
    place.disabled = !panel.canContinue();
  };
  const redraw = () => {
    const pending = panel.refresh();
    render();
    void pending.then(render);
  };
  let typing: ReturnType<typeof setTimeout> | undefined;
  const redrawSoon = () => {
    clearTimeout(typing);
    typing = setTimeout(redraw, shell.typingDelay ?? 200);
  };

  reason.value = panel.options.reason;
  location.value = panel.options.location;
  for (const input of [saved, drawn, none]) {
    input.addEventListener("change", () => {
      if (input.checked) panel.setSource(input.value as ImageSource);
      redraw();
    });
  }
  for (const line of ["label", "name", "date"] as const) {
    checks[line].addEventListener("change", () => {
      panel.setLine(line, checks[line].checked);
      redraw();
    });
  }
  reason.addEventListener("input", () => {
    panel.setReason(reason.value);
    redrawSoon();
  });
  location.addEventListener("input", () => {
    panel.setLocation(location.value);
    redrawSoon();
  });
  remember.addEventListener("change", () => {
    panel.remember = remember.checked;
  });
  draw.addEventListener("click", () => {
    void shell.draw().then((image) => {
      if (image) panel.useDrawn(image);
      redraw();
    });
  });

  return new Promise((resolve) => {
    let settled = false;
    const finish = (answer: Appearance | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(typing);
      if (answer === null) panel.cancel();
      dialog.close();
      dialog.remove();
      previous?.focus();
      resolve(answer);
    };
    cancel.addEventListener("click", () => finish(null));
    place.addEventListener("click", () => {
      // Typing reaches the state at once and only the redraw waits, so text
      // not yet previewed is still the reader's choice here; the signing checks
      // it as it checks everything.
      if (!panel.canContinue()) return;
      finish(panel.finish());
    });
    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      finish(null);
    });
    dialog.addEventListener("close", () => finish(null));
    dialog.addEventListener("keydown", (event) => event.stopPropagation());
    dialog.showModal();
    redraw();
    place.focus();
  });
}
