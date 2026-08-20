/**
 * The note on a highlight the reader made, and the button that takes it off.
 *
 * ## Why this is not `commentpopup.ts`
 *
 * That one shows what somebody else wrote in the file: an author, a date, a
 * kind, replies, and every field read-only. This one is a form over a single
 * string the reader owns, plus the one destructive action a mark has. They share
 * their placement (`popup.ts`) and nothing else, and folding them together would
 * mean a widget that is a form or a document depending on where its subject came
 * from.
 *
 * The asymmetry is real rather than an omission: a comment already in the file
 * cannot be edited here at all, because the model knows nothing about it --- it
 * was read by `annots.rs` out of the bytes, and the journal has no command that
 * names it. Editing those is its own increment; see `docs/PLAN.md` Phase 2.
 *
 * ## The note commits when the popup closes
 *
 * Not on every keystroke, which would put a journal entry between every two
 * letters and make undo useless. Not on a button either: a reader who types and
 * clicks away has said what they wanted to say, and the alternative is a box
 * that silently discards it.
 *
 * **Escape commits too.** It is the ordinary "I am done with this box" key here
 * rather than a cancel, because the thing it would cancel is text somebody just
 * typed. What it cannot do is lose work, which a discarding Escape does the
 * first time it is pressed by reflex.
 *
 * The one thing that does *not* commit is removing the mark: the note goes with
 * it, so committing first would journal a note onto a highlight that the very
 * next command deletes.
 *
 * ## The swatch row is six colours, not seven
 *
 * `markcolors.ts` has a seventh, its `DEFAULT_SWATCH`, and it is deliberately
 * not here. It means *each kind's own colour* --- a concept about the marks a
 * reader has not made yet --- and a row of swatches is a row of colours you can
 * see. Every default is reachable anyway, because yellow and red are in the row
 * and they are the defaults, byte for byte.
 *
 * The `Colour:` commands are the other half and they carry all seven, because
 * from the palette a reader is setting what the *next* mark will be as often as
 * recolouring this one.
 */

import type { MarkKind, MarkView } from "./pages";
import { place, POPUP_WIDTH, type Anchor } from "./popup";
import {
  cssColor,
  PALETTE,
  sameColor,
  type MarkColor,
  type Swatch,
} from "./markcolors";

/** What the popup does on the reader's behalf. The viewer supplies all three. */
export interface MarkPopupOptions {
  /** The note was changed and the popup is closing. Only ever with new text. */
  onNote: (mark: number, note: string) => void;
  /** Remove this mark. The popup closes without committing its note. */
  onRemove: (mark: number) => void;
  /**
   * Draw this mark in this colour. Only ever with a colour it is not already.
   *
   * Unlike {@link onNote}, which waits for the box to close: a swatch press is
   * the whole gesture and the reader is looking at the mark while they make it,
   * so holding it back would mean a colour that appears when the popup is
   * dismissed. One press, one journal entry, one undo.
   */
  onRecolor: (mark: number, color: MarkColor) => void;
  /** The popup asked to be closed --- Escape, or the close button. */
  onClose: () => void;
}

/** The note editor for one mark the reader made. */
/**
 * The word a reader sees for each kind.
 *
 * Ours, in their language, not the file's --- the same posture `comments.ts`
 * takes with `labelFor`. "Strikeout" rather than the PDF's `/StrikeOut`, which
 * is a name in a file and not a word anybody says.
 */
const NAMES: Record<MarkKind, string> = {
  highlight: "Highlight",
  underline: "Underline",
  strikeout: "Strikeout",
  // "Comment", never "Note". The note is the *text*, and every mark has one ---
  // a box headed "Note" beside a field holding the note would be naming the
  // field rather than the thing being removed.
  note: "Comment",
  // "Box", never "Square". The PDF subtype is `/Square` and the serde name is
  // `square`, and neither is what a reader would call the rectangle they just
  // dragged round a figure --- one that is actually square is the rare case.
  // The third spelling, and the same arrangement as `note` above.
  square: "Box",
  // "Drawing", never "Ink". `/Ink` is the file's spelling, `ink` is the serde
  // name, and inside this codebase "ink" already means how a mark is laid down
  // --- `Paint` in `save.rs`, `markBand` here. A reader who drew a line and
  // wants it gone is looking for the thing they drew, not for the substance.
  //
  // **And it is the word `comments.ts` already uses**, which was found rather
  // than arranged: `labelFor` has answered "Drawing" for a document's own
  // `/Ink` since the comment panel was built. The two tables name different
  // things --- that one labels marks the file arrived with, this one labels the
  // reader's --- so nothing forces them to agree, and a reader looking at both
  // panels would notice if they did not.
  ink: "Drawing",
};

export class MarkPopup {
  private readonly host: HTMLElement;
  private readonly element: HTMLElement;
  private readonly input: HTMLTextAreaElement;
  private readonly opts: MarkPopupOptions;
  private shown: number | null = null;
  /** The header's word, and the button's, both of which name the kind. */
  private readonly title: HTMLElement;
  private readonly remove: HTMLButtonElement;
  /** What the field held when it was last filled, so a no-op sends nothing. */
  private was = "";
  /** One button per swatch, in {@link offered} order, so `show` can mark one on. */
  private readonly swatches: HTMLButtonElement[] = [];
  /**
   * The swatches this row draws: every one that is a colour.
   *
   * Narrowed rather than filtered loosely, and the type is doing real work:
   * {@link swatches} is built one-for-one from this list and {@link showColor}
   * indexes the two together, so a `Swatch` in here whose `rgb` is `null` would
   * have no button and slide every ring one place along. With the predicate,
   * that state cannot be written.
   */
  private readonly offered: readonly (Swatch & { rgb: MarkColor })[] =
    PALETTE.filter((entry): entry is Swatch & { rgb: MarkColor } => entry.rgb !== null);

  constructor(host: HTMLElement, opts: MarkPopupOptions) {
    this.host = host;
    this.opts = opts;
    // Built here rather than in `header` and `actions`, which run once each in
    // the constructor: `show` has to rewrite both when a mark of another kind
    // takes the box over, and it can only do that if it holds them.
    this.title = document.createElement("strong");
    this.remove = document.createElement("button");

    this.element = document.createElement("div");
    this.element.setAttribute("role", "dialog");
    // Not the kind. The label names the *dialog*, and a screen reader announces
    // it when the box opens; a name that changed per mark would read as three
    // different dialogs, and the kind is the first thing inside it anyway.
    this.element.setAttribute("aria-label", "Mark note");
    this.element.tabIndex = -1;
    this.element.style.cssText =
      "position:absolute;display:none;z-index:5;box-sizing:border-box;" +
      `width:${POPUP_WIDTH}px;` +
      "padding:0.6rem 0.7rem;border-radius:8px;" +
      "background:Canvas;color:CanvasText;" +
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);" +
      "box-shadow:0 6px 24px rgba(0,0,0,0.25);" +
      "font:13px/1.45 system-ui,-apple-system,sans-serif;";
    // A press inside must not reach the page underneath, where it would start a
    // text selection and --- because the press lands outside the mark --- close
    // the popup the reader is typing in.
    this.element.addEventListener("pointerdown", (event) => event.stopPropagation());
    this.element.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      // Stopped here as well as prevented: the page's own handler reads Escape
      // as "clear the selection", and the reader is asking about this box.
      event.stopPropagation();
      this.opts.onClose();
    });

    this.input = document.createElement("textarea");
    this.input.setAttribute("aria-label", "Note");
    this.input.placeholder = "Note";
    this.input.rows = 3;
    this.input.style.cssText =
      "box-sizing:border-box;width:100%;resize:vertical;" +
      "background:transparent;color:inherit;font:inherit;" +
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);" +
      "border-radius:5px;padding:0.3rem 0.4rem;";

    this.element.append(this.header(), this.colors(), this.input, this.actions());
    host.appendChild(this.element);
  }

  /** The mark whose note is open, or `null`. */
  get openId(): number | null {
    return this.shown;
  }

  /** The popup element. For the check harness. */
  get node(): HTMLElement {
    return this.element;
  }

  /**
   * The note field itself. For the check harness.
   *
   * The element rather than a `focused` boolean, because the two harnesses ask
   * the question differently and neither can ask the other's way: `viewercheck`
   * compares it with `document.activeElement` in a real webview, and the vitest
   * fake DOM records `focus()` on the element instead. A getter answering one of
   * those would leave the other unable to see the keyboard at all.
   */
  get field(): HTMLTextAreaElement {
    return this.input;
  }

  /** The note as the box now holds it. For the check harness. */
  get text(): string {
    return this.input.value;
  }

  /**
   * Shows `mark`'s note, anchored to `at`.
   *
   * `focus` moves the keyboard into the box, which is what a reader who clicked
   * the mark to type on it wants --- unlike the comment popup, where focus would
   * take the arrow keys away from the page for no gain. There is nothing to read
   * here that is not editable.
   */
  show(mark: MarkView, at: Anchor, focus: boolean): void {
    // A second mark clicked while the first is open is still a close, and its
    // note has to be committed before this one takes the box over.
    if (this.shown !== null && this.shown !== mark.id) this.commit();
    this.shown = mark.id;
    // Both labels follow the mark, because the box is the one place that knows
    // which mark a reader means --- the Edit menu's item says "Remove mark",
    // since it is chosen with the pointer somewhere else entirely and cannot.
    this.title.textContent = NAMES[mark.kind];
    this.remove.textContent = `Remove ${NAMES[mark.kind].toLowerCase()}`;
    this.was = mark.note;
    this.input.value = mark.note;
    this.showColor(mark.color);
    this.element.style.display = "block";
    this.place(at);
    if (focus) this.input.focus();
  }

  /**
   * Puts the keyboard in the note field.
   *
   * For the route that opens the box without taking focus --- the keyboard walk
   * through marks, which leaves it on the page so that the next press steps
   * again. Does nothing when the box is closed, so a stray Enter on a document
   * with no note open cannot focus a hidden field.
   */
  focusField(): void {
    if (this.shown === null) return;
    this.input.focus();
  }

  /**
   * Hides the popup, committing the note unless told not to.
   *
   * `commit` is false where the mark is going or already gone --- a removal, or
   * an undo that took the highlight off the page under the box. Sending a note
   * for a mark the model no longer has is a refusal the reader would see as an
   * error, for doing nothing wrong.
   */
  hide(commit = true): void {
    if (this.shown === null) return;
    if (commit) this.commit();
    this.shown = null;
    this.element.style.display = "none";
    this.input.value = "";
  }

  /** Moves the popup to a new anchor, without rebuilding it. See `popup.ts`. */
  place(at: Anchor): void {
    if (this.shown === null) return;
    place(this.host, this.element, at);
  }

  /**
   * The swatch buttons, in {@link PALETTE} order. For the check harness.
   *
   * The elements rather than a "which one is on" number, for the reason
   * {@link field} gives: `viewercheck` presses one in a real webview and reads
   * `aria-pressed` back off it, and a number would be this file agreeing with
   * itself about a row nothing had looked at.
   */
  get colorButtons(): readonly HTMLButtonElement[] {
    return this.swatches;
  }

  /** Puts the row's pressed state on whichever swatch matches `color`. */
  private showColor(color: MarkColor): void {
    this.swatches.forEach((button, at) => {
      const rgb = this.offered[at]?.rgb ?? null;
      // `null` only for an index past the end, which the one-for-one build above
      // makes unreachable --- and it rings nothing rather than ringing the first
      // swatch, so an unreachable case stays the quiet one.
      button.setAttribute("aria-pressed", String(sameColor(rgb, color)));
      // Not a border colour, which would be invisible on the swatch whose own
      // colour it is. A ring outside the button reads on every one of them.
      button.style.boxShadow = sameColor(rgb, color)
        ? "0 0 0 2px Canvas, 0 0 0 4px CanvasText"
        : "none";
    });
  }

  /**
   * The colours this mark can be drawn in.
   *
   * Buttons rather than a `<select>`: a colour is a thing you point at, and the
   * whole row is one press away where a menu is two. `aria-pressed` is what
   * carries "this is the one it is" to a screen reader, since the ring around it
   * is not something a name can say.
   */
  private colors(): HTMLElement {
    const row = document.createElement("div");
    row.setAttribute("role", "group");
    row.setAttribute("aria-label", "Mark colour");
    row.style.cssText =
      "display:flex;gap:0.45rem;align-items:center;margin-bottom:0.45rem;";

    for (const entry of this.offered) {
      const rgb = entry.rgb;
      const button = document.createElement("button");
      button.type = "button";
      button.setAttribute("aria-label", entry.name);
      button.title = entry.name;
      button.style.cssText =
        `width:18px;height:18px;border-radius:50%;background:${cssColor(rgb)};` +
        "border:1px solid color-mix(in srgb, CanvasText 30%, transparent);" +
        "padding:0;cursor:default;";
      button.addEventListener("pointerdown", (event) => {
        // The popup's own handler would otherwise swallow it, and the page's
        // would read it as a press outside the mark.
        event.preventDefault();
        event.stopPropagation();
        const id = this.shown;
        if (id === null) return;
        // A colour the mark already is costs an undo step and changes nothing,
        // which is the comparison `edits.ts`'s `recolor` says lives here.
        if (button.getAttribute("aria-pressed") === "true") return;
        this.showColor(rgb);
        this.opts.onRecolor(id, rgb);
      });
      this.swatches.push(button);
      row.append(button);
    }
    return row;
  }

  /** Sends the note if it changed. */
  private commit(): void {
    const id = this.shown;
    if (id === null) return;
    const now = this.input.value;
    if (now === this.was) return;
    this.was = now;
    this.opts.onNote(id, now);
  }

  /** The kind, and a close affordance for a reader who will not guess Escape. */
  private header(): HTMLElement {
    const row = document.createElement("div");
    row.style.cssText =
      "display:flex;gap:0.5rem;align-items:baseline;margin-bottom:0.35rem;";

    const kind = this.title;
    kind.style.cssText = "flex:1;min-width:0;";

    const close = document.createElement("button");
    close.type = "button";
    close.setAttribute("aria-label", "Close note");
    close.textContent = "×";
    close.style.cssText =
      "border:0;background:none;color:inherit;opacity:0.55;font:inherit;" +
      "font-size:1.1em;line-height:1;cursor:default;padding:0 0.15rem;";
    close.addEventListener("pointerdown", (event) => {
      // Stops the popup's own handler from swallowing it, and the page's from
      // seeing a press it would treat as a click-away *and* a selection start.
      event.preventDefault();
      event.stopPropagation();
      this.opts.onClose();
    });

    row.append(kind, close);
    return row;
  }

  /** The one destructive action a mark has. */
  private actions(): HTMLElement {
    const row = document.createElement("div");
    row.style.cssText = "display:flex;justify-content:flex-end;margin-top:0.4rem;";

    const remove = this.remove;
    remove.type = "button";
    remove.style.cssText =
      "border:1px solid color-mix(in srgb, currentColor 25%, transparent);" +
      "border-radius:5px;background:none;color:inherit;font:inherit;" +
      "padding:0.2rem 0.5rem;cursor:default;";
    remove.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      event.stopPropagation();
      const id = this.shown;
      if (id === null) return;
      this.opts.onRemove(id);
    });

    row.append(remove);
    return row;
  }
}
