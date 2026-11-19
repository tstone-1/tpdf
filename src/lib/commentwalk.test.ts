/**
 * The walk that fills in the words each bare highlight covers. What
 * `App.svelte` hands it is held in `documentopen.test.ts`, with the rest of
 * the component's text that suite reads.
 *
 * Checked by mutation: `scripts/mutation_rows/frontend/f10_app_wiring.py`.
 */

import { describe, expect, it } from "vitest";

import type { Comment } from "./comments";
import { CommentWords, fillCommentWords, type CommentWalk } from "./commentwalk";
import type { PageText } from "./text";

/** A highlight with nothing typed on it, which wants the words it covers. */
const bare = (id: number, page: number): Comment => ({
  id, page, object: null, kind: "highlight", author: "Timo", body: "", subject: "",
  date: "2026-08-12 10:15", rect: [100, 100, 124, 124], quads: [100, 700, 150, 712],
  reply_to: null, hidden: false, color: null,
});

/** A walk over `rounds`, one list of comments per round, the last repeated. */
function walkOver(rounds: Comment[][], over: Partial<CommentWalk> = {}) {
  const read: number[] = [];
  let round = 0;
  const walk: CommentWalk = {
    comments: () => rounds[Math.min(round++, rounds.length - 1)] ?? null,
    asked: new Set(),
    text: (slot) => { read.push(slot); return Promise.resolve(null); },
    found: () => true,
    ...over,
  };
  return { walk, read };
}

describe("the comment-word walk", () => {
  it("asks for each page that wants words once, front first", async () => {
    const { walk, read } = walkOver([[bare(1, 4), bare(2, 1), bare(3, 4)]]);
    await fillCommentWords(walk);
    expect(read).toEqual([1, 4]);
    expect([...walk.asked].sort()).toEqual([1, 2, 3]);
  });

  it("keeps asking for a comment a deletion moved into a slot already read", async () => {
    // Slots 5 and 6; slot 5 is read, then a page above both is deleted, so
    // the unanswered comment is on slot 5 now. A set of slots answered nothing
    // more, and that comment read "no comment" for the rest of the session.
    const { walk, read } = walkOver([
      [bare(1, 5), bare(2, 6)],
      [bare(1, 4), bare(2, 5)],
    ]);
    await fillCommentWords(walk);
    expect(read).toEqual([5, 5]);
    expect([...walk.asked].sort()).toEqual([1, 2]);
  });

  it("does not ask again for a page that could not be read", async () => {
    // The text never comes; the walk still ends, having asked once.
    const { walk, read } = walkOver([[bare(1, 2)]]);
    await fillCommentWords(walk);
    expect(read).toEqual([2]);
  });

  it("hands over the words each round finds", async () => {
    const found: [number, string][] = [];
    const { walk } = walkOver([[bare(1, 3)]], {
      text: () => Promise.resolve({ codes: [], boxes: [], width_pt: 600, height_pt: 800, quarter_turns: 0, extract_ms: 0 }),
      found: (words) => { found.push(...words); return true; },
    });
    await fillCommentWords(walk);
    expect(found.map(([id]) => id)).toEqual([1]);
  });

  it("stops when the document it began in has gone", async () => {
    const before = walkOver([[bare(1, 1), bare(2, 2)]], { found: () => false });
    await fillCommentWords(before.walk);
    expect(before.read).toEqual([1]);
    const gone = walkOver([[]], { comments: () => null });
    await fillCommentWords(gone.walk);
    expect(gone.read).toEqual([]);
  });
});

/** A page with no characters: every comment on it is answered with no words. */
const EMPTY: PageText = { codes: [], boxes: [], width_pt: 600, height_pt: 800, quarter_turns: 0, extract_ms: 0 };

/** What one call of {@link CommentWords.fill} is given, over one list of comments. */
function fillOver(items: () => Comment[] | null, read: number[], found: () => boolean = () => true) {
  return {
    comments: items,
    text: (slot: number) => { read.push(slot); return Promise.resolve(EMPTY); },
    found,
  };
}

describe("what one document's comment walk keeps", () => {
  it("keeps the words each page answers, by comment id", async () => {
    const state = new CommentWords();
    await state.fill(fillOver(() => [bare(1, 2), bare(2, 3)], []));
    expect([...state.words.keys()].sort()).toEqual([1, 2]);
    expect([...state.asked].sort()).toEqual([1, 2]);
  });

  it("asks no comment twice across calls, which is what makes a second call cheap", async () => {
    const state = new CommentWords();
    const read: number[] = [];
    await state.fill(fillOver(() => [bare(1, 2)], read));
    await state.fill(fillOver(() => [bare(1, 2)], read));
    expect(read).toEqual([2]);
  });

  it("asks across calls for a comment a deletion moved into a slot already read", async () => {
    // The trap "An 'already asked' set keyed by a slot is renumbered by the
    // next deletion", across two walks rather than inside one: slot 5 was
    // read, a page above is deleted, and the comment that was on slot 6 is
    // on slot 5 now. A set of slots kept between walks refused it for good.
    const state = new CommentWords();
    const read: number[] = [];
    await state.fill(fillOver(() => [bare(1, 5)], read));
    await state.fill(fillOver(() => [bare(1, 4), bare(2, 5)], read));
    expect(read).toEqual([5, 5]);
    expect(state.words.has(2)).toBe(true);
  });

  it("keeps no words for a document that has gone", async () => {
    const state = new CommentWords();
    await state.fill(fillOver(() => [bare(1, 2)], [], () => false));
    expect(state.words.size).toBe(0);
  });

  it("stands down while a comment walk is running, and lowers the flag when it ends", async () => {
    const state = new CommentWords();
    let answer: (text: PageText | null) => void = () => {};
    const read: number[] = [];
    const first = state.fill({
      comments: () => [bare(1, 2)],
      text: (slot) => { read.push(slot); return new Promise((resolve) => { answer = resolve; }); },
      found: () => true,
    });
    expect(state.filling).toBe(true);
    await state.fill(fillOver(() => [bare(2, 3)], read));
    expect(read).toEqual([2]);
    answer(EMPTY);
    await first;
    expect(state.filling).toBe(false);
  });

  it("lowers the comment walk's flag when it fails, so the next call walks", async () => {
    const state = new CommentWords();
    await expect(state.fill({
      comments: () => [bare(1, 2)],
      text: () => Promise.reject(new Error("no page")),
      found: () => true,
    })).rejects.toThrow("no page");
    expect(state.filling).toBe(false);
  });
});
