/**
 * The note that opens when a reader clicks a mark on the page.
 *
 * The sidebar answers "what did people say about this document"; this answers
 * "what does *this* mark say", which is the question somebody actually has while
 * reading. Acrobat puts the same thing behind a hover, and a hover is the wrong
 * gesture for text you want to read: it closes when you move towards it.
 *
 * ## It is anchored to the mark, and follows it
 *
 * The popup is positioned from the comment's rectangle in window coordinates, so
 * it moves with the page under a scroll, a zoom or a rotation --- the caller
 * repositions it every frame while it is open. A popup that stayed where it was
 * opened would, one flick later, be pointing at a different paragraph and would
 * look like it belonged to that one.
 *
 * ## It is clamped to the viewport, never off it
 *
 * A comment near the right edge of a wide page would otherwise open past the
 * window. Preferred placement is to the right of the mark and level with its
 * top; when there is no room it flips to the left, and it is clamped vertically
 * either way. The clamp is the reason the mark's rectangle is passed in whole
 * rather than as a point.
 *
 * ## Nothing here builds markup from a document string
 *
 * `textContent` only, for every field that came out of the file. See
 * `commentlist.ts` and `docs/THREAT-MODEL.md` T8.
 */

import { bylineOf, labelFor, type Comment } from "./comments";
import { place, POPUP_WIDTH, type Anchor } from "./popup";

export { POPUP_WIDTH, type Anchor };

/** The note shown for one comment, with its replies under it. */
export class CommentPopup {
  private readonly host: HTMLElement;
  private readonly element: HTMLElement;
  private readonly onClose: () => void;
  private shown: number | null = null;

  constructor(host: HTMLElement, onClose: () => void) {
    this.host = host;
    this.onClose = onClose;

    this.element = document.createElement("div");
    this.element.setAttribute("role", "dialog");
    this.element.setAttribute("aria-label", "Comment");
    this.element.tabIndex = -1;
    this.element.style.cssText =
      "position:absolute;display:none;z-index:5;box-sizing:border-box;" +
      `width:${POPUP_WIDTH}px;max-height:60%;overflow-y:auto;` +
      // The right padding leaves room for the close button, which is positioned
      // against this element rather than flowing with the text.
      "padding:0.6rem 1.6rem 0.6rem 0.7rem;border-radius:8px;" +
      "background:Canvas;color:CanvasText;" +
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);" +
      "box-shadow:0 6px 24px rgba(0,0,0,0.25);" +
      "font:13px/1.45 system-ui,-apple-system,sans-serif;";
    // A press inside must not reach the page underneath, where it would start a
    // text selection and --- because the press lands outside the mark --- close
    // the popup the reader is trying to read.
    this.element.addEventListener("pointerdown", (event) => event.stopPropagation());
    this.element.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      this.onClose();
    });

    host.appendChild(this.element);
  }

  /** The comment currently shown, or `null`. */
  get openId(): number | null {
    return this.shown;
  }

  /** The popup element. For the check harness. */
  get node(): HTMLElement {
    return this.element;
  }

  /** What the popup says, read back out of the DOM. For the check harness. */
  get text(): string {
    return this.element.textContent ?? "";
  }

  /**
   * Shows `comment` with `replies` under it, anchored to `at`.
   *
   * `focus` moves the keyboard into the popup, which is right when the reader
   * asked for it from the sidebar --- they are already on the keyboard --- and
   * wrong when they clicked the mark, where it would take focus off the page and
   * stop the arrow keys scrolling.
   */
  show(comment: Comment, replies: readonly Comment[], at: Anchor, focus: boolean): void {
    this.shown = comment.id;
    this.element.replaceChildren(
      this.closeButton(),
      header(comment),
      ...(comment.subject.trim() ? [subject(comment)] : []),
      body(comment),
      ...replies.map((reply) => replyBlock(reply)),
    );
    this.element.style.display = "block";
    this.place(at);
    if (focus) this.element.focus();
  }

  /** Hides the popup. Safe to call when it is already hidden. */
  hide(): void {
    this.shown = null;
    this.element.style.display = "none";
    this.element.replaceChildren();
  }

  /**
   * The close affordance, for a reader who has no reason to guess at Escape.
   *
   * Escape and a press on the page both close the note, and neither is visible.
   * A mouse-only reader who opens one and wants it gone otherwise has to find
   * somewhere safe to click --- and the obvious place, back on the mark,
   * reopens it.
   */
  private closeButton(): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("aria-label", "Close comment");
    button.textContent = "\u00d7";
    button.style.cssText =
      "position:absolute;top:0.25rem;right:0.35rem;border:0;background:none;" +
      "color:inherit;opacity:0.55;font:inherit;font-size:1.1em;line-height:1;" +
      "cursor:default;padding:0.1rem 0.25rem;";
    button.addEventListener("pointerdown", (event) => {
      // Stops the popup's own handler from swallowing it, and the page's from
      // seeing a press it would treat as a click-away *and* a selection start.
      event.preventDefault();
      event.stopPropagation();
      this.onClose();
    });
    return button;
  }

  /** Moves the popup to a new anchor, without rebuilding it. See `popup.ts`. */
  place(at: Anchor): void {
    if (this.shown === null) return;
    place(this.host, this.element, at);
  }
}

/** Who wrote it, when, and what kind of mark it is. */
function header(comment: Comment): HTMLElement {
  const row = document.createElement("div");
  row.style.cssText =
    "display:flex;gap:0.5rem;align-items:baseline;margin-bottom:0.35rem;";

  const who = document.createElement("strong");
  who.textContent = comment.author.trim() || "Unknown";
  who.style.cssText = "flex:1;min-width:0;overflow-wrap:anywhere;";

  const kind = document.createElement("span");
  kind.textContent = labelFor(comment.kind);
  kind.style.cssText = "flex:none;opacity:0.6;font-size:0.85em;";

  row.append(who, kind);

  const when = document.createElement("div");
  when.textContent = comment.date ?? "";
  when.style.cssText = "opacity:0.6;font-size:0.85em;margin-top:-0.3rem;";
  if (!comment.date) when.style.display = "none";

  const wrapper = document.createElement("div");
  wrapper.append(row, when);
  return wrapper;
}

/** The `/Subj` line, which Acrobat shows as a title above the body. */
function subject(comment: Comment): HTMLElement {
  const element = document.createElement("div");
  element.textContent = comment.subject;
  element.style.cssText = "font-weight:600;margin-bottom:0.2rem;overflow-wrap:anywhere;";
  return element;
}

/**
 * The comment's own words.
 *
 * `white-space:pre-wrap` because `annots.rs` keeps a body's paragraphs, and a
 * two-paragraph note rendered as one is a note somebody wrote differently.
 */
function body(comment: Comment): HTMLElement {
  const element = document.createElement("div");
  const text = comment.body.trim();
  element.textContent = text || `${labelFor(comment.kind)}, no comment`;
  element.style.cssText =
    "white-space:pre-wrap;overflow-wrap:anywhere;" +
    (text ? "" : "opacity:0.6;font-style:italic;");
  return element;
}

/** One reply, under a rule, with its own byline. */
function replyBlock(reply: Comment): HTMLElement {
  const element = document.createElement("div");
  element.style.cssText =
    "margin-top:0.55rem;padding-top:0.5rem;" +
    "border-top:1px solid color-mix(in srgb, currentColor 15%, transparent);";

  const who = document.createElement("div");
  who.textContent = bylineOf(reply);
  who.style.cssText = "opacity:0.6;font-size:0.85em;margin-bottom:0.15rem;";

  const said = document.createElement("div");
  said.textContent = reply.body;
  said.style.cssText = "white-space:pre-wrap;overflow-wrap:anywhere;";

  element.append(who, said);
  return element;
}
