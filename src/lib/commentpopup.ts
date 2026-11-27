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
 * ## The body is read-only until the reader asks for it
 *
 * Somebody else wrote this. A textarea sitting there from the moment the popup
 * opens would read as the reader's own note and would let a stray keystroke
 * alter a colleague's words, so editing is **armed** --- by the Edit button here
 * or by the `Edit this comment` command --- and the body is text until then.
 * That is the same posture "Redact text or regions by dragging" takes and for the same
 * reason: the cost of arming silently is worse than one more press.
 *
 * Once armed it behaves exactly as `markpopup.ts` does, and deliberately: the
 * text **commits when the box closes**, Escape included, because the thing an
 * Escape would cancel is text somebody just typed. Only a changed body is sent,
 * so opening the editor and closing it again journals nothing.
 *
 * A comment with no `object` of its own cannot be armed at all --- there is
 * nothing an incremental update could override --- and the button is absent
 * rather than disabled, because a disabled control that is never enabled for a
 * given comment is a promise the file cannot keep.
 *
 * ## Nothing here builds markup from a document string
 *
 * `textContent` only, for every field that came out of the file. See
 * `commentlist.ts` and `docs/THREAT-MODEL.md` T8. The one element that is not
 * `textContent` is the editor, whose `value` is the reader's own text going the
 * other way.
 */

import { CommentFormatting, paintComment, type CommentField, type CommentStyle } from "./commentformat";
import { bylineOf, labelFor, noCommentLabel, type Comment } from "./comments";
import { catalog } from "./i18n";
import { matches } from "./keys";
import { place, POPUP_WIDTH, PopupSubmission, type Anchor } from "./popup";

const TEXT = catalog({
  "en-US": {
    comment: "Comment",
    commentText: "Comment text",
    yourReply: "Your reply",
    previewOnly: "Only a preview of this comment is available.",
    edit: "Edit",
    reply: "Reply",
    closeComment: "Close comment",
    unknown: "Unknown",
  },
  "de-DE": {
    comment: "Kommentar",
    commentText: "Kommentartext",
    yourReply: "Deine Antwort",
    previewOnly: "Für diesen Kommentar ist nur eine Vorschau verfügbar.",
    edit: "Bearbeiten",
    reply: "Antworten",
    closeComment: "Kommentar schließen",
    unknown: "Unbekannt",
  },
  "de-DE-formal": {
    yourReply: "Ihre Antwort",
  },
});

export { POPUP_WIDTH, type Anchor };

/** What the popup does on the reader's behalf. The viewer supplies both. */
export interface CommentPopupOptions {
  /** Close this popup. */
  onClose: (restoreFocus?: boolean) => void;
  /**
   * The body was changed and the editor is closing. Only ever with new text.
   *
   * The whole comment rather than its object and page, because those are two
   * bare numbers and a slot and an id are both `number` --- the mistake this
   * repository has already paid for once. The caller translates.
   */
  onRewrite: (comment: Comment, body: string, styles?: CommentStyle[]) => unknown;
  /**
   * A reply was typed and the editor is closing. Only ever with text in it.
   *
   * The whole comment for {@link CommentPopupOptions.onRewrite}'s reason: the
   * caller needs the parent's object *and* its page *and* its rectangle, and
   * three bare numbers in a row is the mistake this repository has paid for.
   *
   * **Separate from `onRewrite` rather than a mode flag on it.** They are two
   * different operations on two different objects --- one overrides an
   * annotation the file wrote, the other adds one of the reader's own --- and a
   * boolean deciding which would put the distinction in the caller, where a
   * wrong value silently edits somebody else's comment instead of answering it.
   */
  onReply: (comment: Comment, body: string, styles?: CommentStyle[]) => unknown;
  /**
   * The reader deleted the comment on show.
   *
   * The whole comment for {@link CommentPopupOptions.onRewrite}'s reason: the
   * consumer needs the object and the page, and reading them back out of a
   * popup that has already closed is not possible.
   *
   * **Optional where the two above are required**, and that is a decision about
   * what a viewer with no model behind it should do. Editing and replying have
   * been wired since they shipped; deleting is newer, and the window harness
   * builds a popup to read what it draws rather than to change a document. A
   * required callback would make every such caller supply a no-op that says
   * nothing.
   */
  onDiscard?: (comment: Comment) => void;
}

/** The note shown for one comment, with its replies under it. */
export class CommentPopup {
  private readonly host: HTMLElement;
  private readonly element: HTMLElement;
  private readonly opts: CommentPopupOptions;
  private readonly submission = new PopupSubmission();
  private shown: number | null = null;
  /** The comment on show, kept so a commit knows what it is committing to. */
  private subject: Comment | null = null;
  /** Its replies, kept because arming the editor repaints the whole popup. */
  private replies: readonly Comment[] = [];
  /**
   * Which operation the open editor commits to, or `null` when none is open.
   *
   * **The discriminator, and it is not a second copy of "is the editor open".**
   * `editor` is the element and this is what it means; both go to `null`
   * together, in {@link CommentPopup.commit}, so there is one place they can
   * disagree and it is three lines long.
   */
  private mode: "body" | "reply" | null = null;
  /** The editor, present only while a comment is armed. */
  private editor: CommentField | null = null;
  /** What the body said when the editor opened, so an unchanged one sends nothing. */
  private was = "";
  private formatting: CommentFormatting | null = null;

  constructor(host: HTMLElement, opts: CommentPopupOptions) {
    this.host = host;
    this.opts = opts;

    this.element = document.createElement("div");
    this.element.setAttribute("role", "dialog");
    this.element.setAttribute("aria-label", TEXT().comment);
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
      this.opts.onClose();
    });

    host.appendChild(this.element);
  }

  /** The comment currently shown, or `null`. */
  get openId(): number | null {
    return this.shown;
  }

  get canCloseOnScroll(): boolean { return this.submission.canCloseOnScroll; }

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
  show(comment: Comment, replies: readonly Comment[], at: Anchor, focus: boolean, opened?: () => void): void {
    if (this.shown === comment.id && this.editing) {
      if (opened) this.submission.cancelNavigation();
      this.place(at);
      if (focus) this.editor?.focus();
      opened?.();
      return;
    }
    this.submission.after(() => this.writeDraft(), () => {
      this.display(comment, replies, at, focus);
      opened?.();
    }, () => this.editor?.focus());
  }

  private display(comment: Comment, replies: readonly Comment[], at: Anchor, focus: boolean): void {
    this.shown = comment.id;
    this.subject = comment;
    this.replies = replies;
    this.editor = null;
    this.mode = null;
    this.paint();
    this.element.style.display = "block";
    this.place(at);
    if (focus) this.element.focus();
  }

  /**
   * Whether the comment on show can be rewritten.
   *
   * What the `Edit this comment` command asks before offering itself. Two
   * data requirements apply: an addressable annotation and a complete body.
   */
  get editable(): boolean {
    return this.shown !== null && this.subject?.object != null && !this.subject.body_clipped && !this.subject.contents_locked;
  }

  get hasDraft(): boolean {
    const text = this.editor?.value;
    return text !== undefined && (text !== this.was || !!this.formatting?.changed) && (this.mode === "body" || text.trim() !== "");
  }

  /** Whether the editor is open, so a second arming is not a reset. */
  get editing(): boolean {
    return this.editor !== null;
  }

  /**
   * Turns the body into an editor and puts the keyboard in it.
   *
   * Does nothing for a comment that cannot be rewritten, and nothing when the
   * editor is already open --- a second press would otherwise throw away what
   * the reader has typed by rebuilding the box around it.
   */
  edit(): void {
    this.arm("body");
  }

  /**
   * Whether the comment on show can be answered.
   *
   * A reply references its parent's object through `/IRT`. A clipped comment
   * still has that identity when the file stored it as an indirect object.
   */
  get replyable(): boolean {
    return this.shown !== null && this.subject?.object != null;
  }

  /**
   * Whether the comment on show can be deleted.
   *
   * Deletion requires the object identity, including for a clipped body.
   *
   * **It does not ask whether a reply answers the comment**, which is the one
   * refusal deleting has that the other two do not. That is the model's
   * knowledge, not the popup's: the popup is handed the replies it draws and
   * has no way to tell one the reader wrote from one the file came with. The
   * model refuses and says which order to do the two in, which is what a reader
   * can act on.
   */
  get deletable(): boolean {
    return this.shown !== null && this.subject?.object != null && !this.subject.locked;
  }

  /**
   * Sends the comment on show to be deleted, and closes the popup.
   *
   * **Closed here rather than left to the state reply**, and the difference is
   * a frame: the reply rebuilds the panel from a list this comment is no longer
   * in, so the popup would be showing a comment nothing else knows about until
   * it arrived. Hiding first makes the deletion look like what it is.
   *
   * Does nothing for a comment that cannot be deleted, which is
   * {@link CommentPopup.edit}'s shape.
   */
  remove(): void {
    if (!this.deletable) return;
    const comment = this.subject;
    if (!comment) return;
    this.hide(true, () => this.opts.onDiscard?.(comment));
  }

  /**
   * Opens an empty editor whose text becomes a new comment answering this one.
   *
   * {@link CommentPopup.edit}'s twin, and the arming is shared --- what differs
   * is where the words go, which is the one thing a reader would not forgive
   * getting wrong.
   */
  reply(): void {
    this.arm("reply");
  }

  /**
   * Builds the editor for whichever operation is being armed.
   *
   * Does nothing for a comment that cannot take it, and nothing when an editor
   * is already open --- a second press would otherwise throw away what the
   * reader has typed by rebuilding the box around it, and *switching* mode
   * mid-typing would send their words to the wrong place, which is worse.
   */
  private arm(mode: "body" | "reply"): void {
    const allowed = mode === "body" ? this.editable : this.replyable;
    if (!allowed || this.editing) return;
    const comment = this.subject;
    if (!comment) return;
    // A reply starts empty and a rewrite starts at what the comment says. `was`
    // is what a commit compares against, so for a reply it is the empty string
    // --- which is what makes an editor closed untouched send nothing.
    this.was = mode === "body" ? comment.body : "";
    this.mode = mode;
    this.formatting = mode === "reply" || comment.kind !== "freetext" ? new CommentFormatting() : null;
    const box = this.formatting?.field ?? document.createElement("textarea");
    if (this.formatting) this.formatting.load(this.was, mode === "body" ? comment.styles : []);
    else box.value = this.was;
    box.setAttribute("aria-label", mode === "body" ? TEXT().commentText : TEXT().yourReply);
    if (!this.formatting) (box as HTMLTextAreaElement).rows = 4;
    if (!this.formatting) box.style.cssText =
      "width:100%;box-sizing:border-box;resize:vertical;font:inherit;" +
      "color:inherit;background:Field;margin:0.1rem 0 0.2rem;" +
      "border:1px solid color-mix(in srgb, currentColor 35%, transparent);" +
      "border-radius:4px;padding:0.3rem;";
    // Escape reaches the popup's own handler and closes the whole thing, which
    // commits. What must not happen is the *page* seeing these keys: a reader
    // typing "n" in a comment would otherwise page forward.
    box.addEventListener("keydown", (event) => {
      if (event.key !== "Escape" && !matches("file.save", event)) event.stopPropagation();
    });
    this.editor = box;
    // **Repainted rather than swapped in place.** The obvious implementation
    // finds the body and calls `replaceWith` on it; that needs an attribute
    // selector and a method the popup does not otherwise use, and it would work
    // in a browser and throw under the unit suite's DOM --- which is the
    // direction that ships. One builder, two modes, and the editor takes the
    // body's position because that is where the reader's eye already is.
    this.paint();
    box.focus();
  }

  /** Hides the popup, committing the body unless told not to. */
  hide(commit = true, closed?: () => void): void {
    const close = () => this.dismiss(closed);
    if (commit) this.submission.after(() => this.writeDraft(), close, () => this.editor?.focus());
    else close();
  }

  private dismiss(closed?: () => void): void {
    this.submission.cancelNavigation();
    this.shown = null;
    this.subject = null;
    this.replies = [];
    this.editor = null;
    this.mode = null;
    this.element.style.display = "none";
    this.element.replaceChildren();
    closed?.();
  }

  /** Keeps a refused rewrite or reply in its own editor for correction. */
  async settleDraft(restoreFocus = true): Promise<void> {
    const field = this.editor;
    const comment = this.subject;
    await this.writeDraft();
    if (this.subject === comment && this.editor === field) this.opts.onClose(restoreFocus);
  }

  private writeDraft(): void | Promise<void> {
    if (this.submission.pending) return this.submission.pending;
    const comment = this.subject;
    if (!comment) return;
    const field = this.editor;
    const mode = this.mode;
    const before = this.was;
    const format = this.formatting;
    if (field && mode && this.hasDraft) {
      const previousStyles = format?.baseline ?? [];
      if (format) format.baseline = format.styles;
      const words = field.value;
      this.was = words;
      const write = mode === "body" ? this.opts.onRewrite : this.opts.onReply;
      return this.submission.write(field, () => write(comment, words, ...(format?.args ?? [])), () => {
        if (this.editor === field) { this.was = before; if (format) format.baseline = previousStyles; }
      });
    }
  }

  /**
   * Builds the popup's children for whichever mode it is in.
   *
   * One builder for both, so a field cannot appear in the read view and go
   * missing from the edit view --- the two differ in exactly two places, and
   * both are visible here.
   */
  private paint(): void {
    const comment = this.subject;
    if (!comment) return;
    const box = this.editor;
    this.element.replaceChildren(
      this.closeButton(),
      header(comment),
      ...(comment.subject.trim() ? [subject(comment)] : []),
      ...(box && this.formatting ? [this.formatting.element] : []),
      box ?? body(comment),
      // Absent rather than disabled for a comment the file wrote as a direct
      // dictionary --- see the module note. Gone while the editor is open,
      // because arming what is already armed is not an action.
      ...(this.editable && !box ? [this.editButton()] : []),
      ...(this.replyable && !box ? [this.replyButton()] : []),
      ...this.replies.map((reply) => replyBlock(reply)),
    );
  }

  /**
   * The affordance that arms the editor.
   *
   * A button rather than making the body itself clickable: a reader selecting
   * text to copy it must not find they have started editing, and a comment is
   * something people quote far more often than they change.
   */
  private editButton(): HTMLElement {
    return this.armButton(TEXT().edit, () => this.edit());
  }

  /**
   * One button, for both affordances that arm the editor.
   *
   * Shared for the reason the builder above is shared: two copies of this would
   * be free to drift in padding, in opacity, and --- the one that matters --- in
   * whether they stop the press reaching the page, where it would close the
   * popup the reader is pressing inside.
   */
  private armButton(label: string, arm: () => void): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.style.cssText =
      "margin-top:0.35rem;margin-right:0.3rem;font:inherit;font-size:0.85em;" +
      "cursor:default;background:none;color:inherit;opacity:0.75;" +
      "padding:0.1rem 0.4rem;" +
      "border:1px solid color-mix(in srgb, currentColor 30%, transparent);" +
      "border-radius:4px;";
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      arm();
    });
    return button;
  }

  /**
   * The affordance that arms a reply.
   *
   * Beside Edit rather than replacing it: answering a comment and changing what
   * it says are different acts on different objects, and a reader who meant one
   * would be badly served by discovering they did the other. Built by the same
   * helper so the two cannot drift apart in look or in behaviour.
   */
  private replyButton(): HTMLElement {
    return this.armButton(TEXT().reply, () => this.reply());
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
    button.setAttribute("aria-label", TEXT().closeComment);
    button.textContent = "\u00d7";
    button.style.cssText =
      "position:absolute;top:0.25rem;right:0.35rem;border:0;background:none;" +
      "color:inherit;opacity:0.55;font:inherit;font-size:1.1em;line-height:1;" +
      "cursor:default;padding:0.1rem 0.25rem;";
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      this.opts.onClose();
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
  who.textContent = comment.author.trim() || TEXT().unknown;
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
  element.textContent = comment.body_clipped
    ? [text, TEXT().previewOnly].filter(Boolean).join("\n\n")
    : text || noCommentLabel(comment.kind);
  if (comment.styles?.length && !comment.body_clipped) paintComment(element, comment.body, comment.styles);
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

  element.append(who, body(reply));
  return element;
}
