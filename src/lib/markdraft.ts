/**
 * What a reader is typing in the box beside a mark, for the page to draw.
 *
 * A text box's words and a form field's name are drawn on the page, and the
 * box they are typed in is beside it. The model hears a note once, when that
 * box closes (`markpopup.ts` says why), so until then the page would show the
 * words the model has: an empty rectangle while a reader types into a new text
 * box. A draft is what the page draws in their place.
 *
 * ## A draft is not an edit
 *
 * Nothing here reaches the journal. The note is sent when the box closes, as
 * it always was, and that is the one step undo has for it. What this holds is
 * thrown away: on close, on Escape, when another mark takes the box, and when
 * the model answers a commit.
 *
 * ## Which kinds have one
 *
 * {@link drafted} is the list: a text box and a form field, the two kinds
 * whose note the page draws. A note on a highlight, a comment, a drawing, a
 * stamp or a signature is shown nowhere but in the box being typed in, so
 * there is nothing on the page for a draft of it to replace.
 *
 * ## A text box's lines are the backend's
 *
 * `MarkView.lines` says why the page does not break lines itself. A draft
 * follows the same rule: its lines are asked for, from the function the model
 * wraps with (`annot_draft_lines`), and until an answer arrives the page keeps
 * what it last drew. Answers can arrive in any order, so each question has a
 * number and only the answer to a draft's newest one is taken.
 *
 * ## A commit keeps the draft until the model has answered
 *
 * The box closes at once and the model answers later. Dropping the draft on
 * close would draw the model's old words for the time between; so a draft
 * whose text was sent is held, still drawn, until the edit that carried it has
 * settled. If that edit is refused, the page then shows what the model has,
 * which is the truth.
 */

import type { MarkKind, MarkView } from "./pages";

/** How the page draws a kind's note: wrapped lines, one name, or not at all. */
export type Drafted = "lines" | "name" | null;

/** How the page draws this kind's note, and so what a draft of it replaces. */
export function drafted(kind: MarkKind): Drafted {
  if (kind === "textbox") return "lines";
  if (kind === "field") return "name";
  return null;
}

/** What the drafts are given. */
export interface MarkDraftDeps {
  /**
   * The lines a text box running from `left` to `right` draws `note` in, or
   * `undefined` when there is nobody to ask.
   */
  wrap: (note: string, left: number, right: number) => Promise<string[]> | undefined;
  /** What the page draws may have changed. */
  changed: () => void;
}

/** One mark's draft. */
interface Draft {
  readonly mark: number;
  text: string;
  /** The lines for some earlier or the current text, or `null` before any answer. */
  lines: readonly string[] | null;
  /** The number of the newest question asked for this draft, or nought. */
  asked: number;
  /** The edges that question was asked for. */
  left: number;
  right: number;
}

/** The drafts the page draws: one being typed, and any whose commit is unanswered. */
export class MarkDrafts {
  private readonly deps: MarkDraftDeps;
  /** The draft of the mark whose box is open. */
  private live: Draft | null = null;
  /** Drafts whose text was sent to the model and not yet answered, by mark. */
  private readonly held = new Map<number, Draft>();
  private questions = 0;

  constructor(deps: MarkDraftDeps) {
    this.deps = deps;
  }

  /**
   * The box opened on `mark`, or closed (`null`).
   *
   * Whatever was being typed for another mark is no longer drawn. A draft that
   * was sent has already left through {@link committed}.
   */
  opened(mark: number | null): void {
    if (!this.live || this.live.mark === mark) return;
    this.live = null;
    this.deps.changed();
  }

  /** The box beside `mark` now holds `text`. */
  typed(mark: MarkView, text: string): void {
    const how = drafted(mark.kind);
    if (how === null) return;
    const draft: Draft = this.live?.mark === mark.id
      ? this.live
      : { mark: mark.id, text, lines: null, asked: 0, left: 0, right: 0 };
    draft.text = text;
    this.live = draft;
    if (how === "lines") this.ask(draft, mark);
    this.deps.changed();
  }

  /**
   * The model sent `mark` again while its box is open.
   *
   * A text box resized under its draft has lines broken for the old width, so
   * they are asked for again. Nothing is asked when the edges are the same.
   */
  refit(mark: MarkView): void {
    const draft = this.live;
    if (!draft || draft.mark !== mark.id || drafted(mark.kind) !== "lines") return;
    if (draft.left === edges(mark).left && draft.right === edges(mark).right) return;
    this.ask(draft, mark);
  }

  /**
   * `note` was sent to the model for `mark`, and `sent` settles when the model
   * has answered and its marks have been handed to the page.
   * Anything that is not a promise counts as settled already.
   *
   * The draft stays drawn until then. One whose text is not what was sent is
   * dropped instead: it would show words the model was never given.
   */
  committed(mark: number, note: string, sent: unknown): void {
    const draft = this.live;
    if (!draft || draft.mark !== mark) return;
    this.live = null;
    if (draft.text !== note) {
      this.deps.changed();
      return;
    }
    this.held.set(mark, draft);
    const done = (): void => {
      // By identity: a later commit for the same mark holds its own draft.
      if (this.held.get(mark) !== draft) return;
      this.held.delete(mark);
      this.deps.changed();
    };
    Promise.resolve(sent).then(done, done);
  }

  /** The lines the page draws for a text box: its draft's, or the model's. */
  linesOf(mark: MarkView): readonly string[] {
    return this.of(mark.id)?.lines ?? mark.lines;
  }

  /**
   * A mark's note as it stands in the box: what is being typed, or the model's.
   *
   * What the page draws for a form field's name. Only a kind {@link drafted}
   * names has a draft at all, so for every other kind this is the model's note.
   */
  noteOf(mark: MarkView): string {
    return this.of(mark.id)?.text ?? mark.note;
  }

  /** The draft drawn for a mark. The one being typed comes before one that is held. */
  private of(mark: number): Draft | undefined {
    return this.live?.mark === mark ? this.live : this.held.get(mark);
  }

  /** Asks for `draft`'s lines, and takes the answer if it is still the newest. */
  private ask(draft: Draft, mark: MarkView): void {
    const asked = ++this.questions;
    draft.asked = asked;
    ({ left: draft.left, right: draft.right } = edges(mark));
    const reply = this.deps.wrap(draft.text, draft.left, draft.right);
    if (!reply) return;
    reply.then(
      (lines) => {
        // An answer to an older question, or for a draft no longer drawn.
        if (draft.asked !== asked || this.of(draft.mark) !== draft) return;
        draft.lines = lines;
        this.deps.changed();
      },
      // A preview that could not be wrapped keeps what it last showed. Typing
      // is never stopped for it, and the model says what is wrong on commit.
      () => {},
    );
  }
}

/** The left and right edge of a mark's first rectangle, as the model sent them. */
function edges(mark: MarkView): { left: number; right: number } {
  return { left: mark.quads[0] ?? 0, right: mark.quads[2] ?? 0 };
}
