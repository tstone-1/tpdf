/**
 * What a screen reader is given, the document's structure, a search scoped to a selection, and text across a page break.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import type { DocumentInfo } from "../ipc";
import { usableRuns, readingLines, textOfRanges } from "../reading";
import { TextCache, type PageText } from "../text";
import { filePage } from "../pages";
import { Viewer, type ViewerStatus } from "../viewer";
import {
  check,
  drag,
  eventually,
  HEIGHT,
  key,
  settle,
  skip,
  wheel,
  WIDTH,
} from "./common";
import { GUESSED_TEXT_CHECK, manifestOf, UNMAPPED_PAGE } from "./search";
import { linkAnnouncementChecks } from "./links";
import { pickNeedle, preview } from "./invert";

/**
 * What a screen reader would find.
 *
 * Four claims, and only the first is the obvious one. That the text is *there*;
 * that it is the page's own text and not some other page's; that a page which
 * stays on screen keeps the **same element**, so a reading cursor survives a
 * scroll; and that the canvas is out of the tree so the text is not doubled by a
 * large empty region.
 *
 * The third is the one the architecture exists for --- `docs/PLAN.md` §8 says
 * recycling containers destroys focus --- and it is asserted by putting real
 * focus in the element and checking it is still there afterwards, with a control
 * that the page was still visible. "The element is still in the DOM" would pass
 * on a rebuilt element that had thrown the cursor away.
 *
 * What is **not** checked, and is not claimed: that any of this is *pleasant* to
 * listen to. That needs a screen reader and a person.
 */
export async function accessibilityChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  key(root, "Home");
  await settle(() => viewer.idle);
  await eventually(
    "the visible page reaches the accessibility tree",
    () => viewer.accessibleText.elementFor(0) !== null,
    () =>
      `pages present: ${viewer.accessibleText.present.join(", ") || "none"}`,
  );

  const extracted = await invoke<{ codes: number[] }>("page_text", {
    doc: doc.id,
    page: 0,
  }).catch(() => null);
  const spoken = spokenText(viewer.accessibleText.elementFor(0));

  // Three states, not two, and the third is why this branch was restructured on
  // 2026-08-02: a page can have text of the right length that means nothing.
  // `a11y.ts` withholds those characters and gives the reason instead, so the
  // check below --- "what is read out is the page's own text" --- is *false by
  // design* there and failed the first time `encodings.pdf` was ever opened in a
  // window. Which page that is comes from the fixture's manifest, never from
  // `unreadablePage`: branching on the subject's own answer would make a layer
  // that had stopped withholding skip the check instead of failing it.
  const manifest = await manifestOf();
  const guessed = new Set(
    (manifest?.pages ?? [])
      .filter((page) => page.name === UNMAPPED_PAGE)
      .map((page) => page.page),
  );

  if (!extracted || extracted.codes.length === 0) {
    check(
      "a page with no text says so rather than falling silent",
      spoken.includes("no extractable text"),
      `reads "${preview(spoken)}"`,
    );
    skip(
      "the text read out is the page's own text",
      "the page has no extractable text",
    );
    skip(GUESSED_TEXT_CHECK, "the page has no extractable text");
  } else if (guessed.has(0)) {
    skip(
      "a page with no text says so rather than falling silent",
      "this page has text",
    );
    skip(
      "the text read out is the page's own text",
      "this page's characters are a guess, so what is read out is deliberately " +
        "not them",
    );
    const characters = flatten(String.fromCodePoint(...extracted.codes));
    // The second half is the load-bearing one. An element holding the notice
    // *and* the characters would be read out in full, which is the outcome
    // being avoided, and a check for the notice alone passes on it.
    const sample = characters.slice(0, 8);
    check(
      GUESSED_TEXT_CHECK,
      spoken.includes("cannot be read") &&
        sample.length > 0 &&
        !spoken.includes(sample),
      `reads "${preview(spoken)}"; the page extracts as "${preview(characters)}"`,
    );
  } else {
    skip(
      "a page with no text says so rather than falling silent",
      "this page has text",
    );
    skip(GUESSED_TEXT_CHECK, "this page states what its characters mean");
    // Compared against an independent extraction, not against the viewer's
    // cache, so the layer cannot be confirmed by agreeing with itself.
    const expected = flatten(String.fromCodePoint(...extracted.codes));
    // Compared as a *multiset*, not as a string, and that is a real weakening
    // made for a real reason: the tree is now built in reading order, and on
    // `rotated-90.pdf` PDFium's extraction runs the other way --- so exact
    // equality started failing against a layer that had just been made correct.
    //
    // What survives is what this check is named for: the text read out is this
    // page's, entire, and not invented. What it can no longer say is anything
    // about the order, which is asserted instead by the two reading-order checks
    // against the fixture's own manifest and by `reading.test.ts`.
    const sorted = (text: string): string => [...text].sort().join("");
    const same = sorted(spoken) === sorted(expected);
    const moved = [...spoken].filter(
      (char, at) => char !== expected[at],
    ).length;
    check(
      "the text read out is the page's own text",
      same,
      same
        ? `${spoken.length} characters match the extraction, ${moved} in another position`
        : `reads ${spoken.length} characters, extraction has ${expected.length}: ` +
            `"${preview(spoken)}" vs "${preview(expected)}"`,
    );
  }

  await linkAnnouncementChecks(viewer);

  await structureChecks(doc, viewer.accessibleText.elementFor(0));

  // Hidden visually, present in the tree. `display:none` and
  // `visibility:hidden` both remove an element from the accessibility tree, so
  // either would make the whole layer do nothing while every other check here
  // still passed --- textContent reads the same from a hidden element.
  const host = viewer.accessibleText.elementFor(0)?.parentElement ?? null;
  const style = host ? getComputedStyle(host) : null;
  const box = host?.getBoundingClientRect();
  check(
    "the text is hidden visually but not from the tree",
    !!style &&
      style.display !== "none" &&
      style.visibility !== "hidden" &&
      !!box &&
      box.width <= 2 &&
      box.height <= 2,
    `display=${style?.display}, visibility=${style?.visibility}, ` +
      `box=${box?.width.toFixed(0)}x${box?.height.toFixed(0)}`,
  );

  check(
    "the canvas is hidden from the accessibility tree",
    [...root.querySelectorAll("canvas, div")].every(
      (element) =>
        !(element instanceof HTMLCanvasElement) ||
        element.closest("[aria-hidden='true']") !== null,
    ),
    `${root.querySelectorAll("canvas").length} canvases, all inside aria-hidden`,
  );

  // Focus survives a scroll that keeps the page on screen. The control is the
  // `if` below: on a document where a small scroll leaves the page, this proves
  // nothing and says so.
  const article = viewer.accessibleText.elementFor(0);
  article?.focus();
  const focused = document.activeElement === article;
  wheel(root, 60);
  await frame();
  const stillVisible = viewer.accessibleText.present.includes(0);

  if (!focused) {
    skip("focus in the text survives a scroll", "the element never took focus");
  } else if (!stillVisible) {
    skip(
      "focus in the text survives a scroll",
      "the scroll left the page entirely",
    );
  } else {
    check(
      "focus in the text survives a scroll",
      document.activeElement === article &&
        viewer.accessibleText.elementFor(0) === article,
      `same element=${viewer.accessibleText.elementFor(0) === article}, ` +
        `still focused=${document.activeElement === article}`,
    );
  }

  // And the other half: a page that leaves the screen leaves the tree, or a long
  // document would accumulate every page it had ever shown.
  const before = viewer.accessibleText.present.includes(0);
  key(root, "End");
  await settle(() => viewer.idle);
  const last = (seen.status?.page ?? 1) - 1;
  if (!before || doc.page_count < 2) {
    skip(
      "a page that leaves the screen leaves the tree",
      "the document has one screen",
    );
  } else {
    await eventually(
      "a page that leaves the screen leaves the tree",
      () =>
        !viewer.accessibleText.present.includes(0) &&
        viewer.accessibleText.present.includes(last),
      () =>
        `after End: pages ${viewer.accessibleText.present.join(", ") || "none"}`,
    );
  }
}

/**
 * What the page's blocks say, joined the way they are announced.
 *
 * Not `textContent`, which concatenates block elements with **nothing** between
 * them --- so a page of 56 lines read that way is 56 characters short of the
 * extraction it is being compared against, and the check reports a content
 * mismatch when the content is identical and only the separators are structural.
 * Joining the blocks is what makes the comparison about the text.
 */
function spokenText(article: HTMLElement | null): string {
  if (!article) return "";
  // Every child, not `querySelectorAll("p")`. A tagged page announces its
  // headings as `h1`--`h6`, and a selector naming one element silently stops
  // reading the page the day the layer gains another --- which would have shown
  // up here as the *page's text* being short by a heading, i.e. as a defect in
  // extraction rather than in this line.
  return flatten(
    [...article.children].map((child) => child.textContent ?? "").join(" "),
  );
}

/**
 * The document's own reading order, where it reaches the tree.
 *
 * `structure_probe` already asserts that the tags are read correctly, against a
 * fixture a different program wrote. What it cannot say is that anything
 * *consumes* them: the runs travel on `PageText` and the decision to use them is
 * `usableRuns`, and a wiring mistake anywhere along that path leaves a viewer
 * that reads the tree perfectly and shows the geometry's answer.
 *
 * So the assertion is differential and it is made twice. The same page is laid
 * out through `readingLines` with the runs and with them stripped, which is the
 * geometric answer by construction --- and the first check is that those two
 * *differ*, because on a fixture where they agree the second one is vacuous and
 * would pass whether or not a single tag was read. The second is that the
 * paragraphs actually in the accessibility tree are the tagged answer.
 *
 * Reading the DOM rather than calling `readingLines` again is the point of the
 * second one: it is the only thing here that can see the tree being built from
 * something else.
 */
async function structureChecks(
  doc: DocumentInfo,
  article: HTMLElement | null,
): Promise<void> {
  const names = [
    "a tagged page's reading order is not the one its geometry gives",
    "the accessibility tree is built in the order the tags give",
    "a heading is announced as a heading, at the document's own level",
    "nothing the document did not call a heading becomes one",
  ] as const;
  const text = await new TextCache(doc.id).load(filePage(0));
  const runs = text ? usableRuns(text) : null;
  if (!text || !runs) {
    const why = !text
      ? "the page's text did not arrive"
      : text.runs && text.runs.length > 0
        ? "this page's tags do not cover all of its visible text"
        : "this document carries no structure tree";
    for (const name of names) skip(name, why);
    return;
  }

  /** A page's non-empty lines, in whatever order the text it is given implies. */
  const lines = (of: PageText): string[] =>
    readingLines(of)
      .map((line) => flatten(textOfRanges(of, line.ranges)))
      .filter((line) => line.length > 0);

  const tagged = lines(text);
  // Stripped rather than a separate code path: the geometric answer is exactly
  // what this file's own layer produces for a document with no tags, so there is
  // no second implementation here to be wrong in its own way.
  const geometric = lines({ ...text, runs: [] });
  check(
    names[0],
    tagged.length > 0 && tagged.join("\n") !== geometric.join("\n"),
    `${runs.length} runs, ${tagged.length} lines; tags start "${preview(tagged[0] ?? "")}", ` +
      `geometry starts "${preview(geometric[0] ?? "")}"`,
  );

  const spoken = spokenText(article);
  check(
    names[1],
    spoken === flatten(tagged.join(" ")) &&
      spoken !== flatten(geometric.join(" ")),
    `tree reads "${preview(spoken)}"`,
  );

  // Headings, which are the reason to read the types at all: "jump to the next
  // heading" is how a screen-reader user skims, and it works on `h1`--`h6` and on
  // nothing else. Asserted against the *document's* types rather than a count, so
  // a page with no heading skips instead of passing vacuously.
  const wanted = runs.filter((run) => /^H[1-6]?$/.test(run.tag));
  const headings = [...(article?.children ?? [])].filter((child) =>
    /^H[1-6]$/.test(child.tagName),
  );
  const levels = headings
    .map((child) => child.tagName.toLowerCase())
    .join(", ");
  if (wanted.length === 0) {
    skip(names[2], "this page's tags contain no heading");
  } else {
    check(
      names[2],
      headings.length === wanted.length &&
        wanted.every(
          (run, at) =>
            headings[at]?.tagName.toLowerCase() ===
            (run.tag === "H" ? "h2" : run.tag.toLowerCase()),
        ),
      `tags say [${wanted.map((r) => r.tag).join(", ")}], tree has [${levels}]`,
    );
  }

  // And the other half, over *every* block rather than the headings: a layer that
  // emitted `h1` for everything would pass the check above, which only asks that
  // the headings it wanted are present. Each block carries the document's own word
  // for it in `data-tag`, so the whole mapping is checkable in one pass, and
  // `data-tag` exists for exactly this --- a type flattened to `p` with nothing
  // recording it cannot be told from a type nobody handled.
  const expected = (tag: string | null): string => {
    if (tag === null) return "p";
    const level = /^H([1-6])$/.exec(tag);
    if (level) return `h${level[1]}`;
    return tag === "H" ? "h2" : "p";
  };
  const wrong = [...(article?.children ?? [])]
    .map((child) => ({
      tag: child.getAttribute("data-tag"),
      is: child.tagName.toLowerCase(),
    }))
    .filter((block) => block.is !== expected(block.tag));
  check(
    names[3],
    wrong.length === 0,
    wrong.length === 0
      ? `${article?.children.length ?? 0} blocks, each the element its type asks for`
      : wrong.map((b) => `${b.tag} became <${b.is}>`).join(", "),
  );
}

/** Collapses whitespace, so a line break is not a difference in content. */
function flatten(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * A search confined to what the reader selected.
 *
 * The discriminating part is not that a scoped search finds fewer things --- a
 * search that found nothing would do that too. It is that the *same* query,
 * unscoped, finds strictly more, and that everything the scoped one found lies
 * inside the range that was drawn. Both directions, because a scope that
 * excluded everything and a scope that excluded nothing are the two ways this
 * can be wrong and each looks fine from the other side.
 */
export async function scopedSearchChecks(
  root: HTMLElement,
  viewer: Viewer,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const NAMES = [
    "a scoped search looks only inside the selection",
    "and the same query unscoped finds more",
  ] as const;
  const skipBoth = (why: string): void => {
    for (const name of NAMES) skip(name, why);
  };

  viewer.goToStart();
  viewer.clearSelection();
  await settle(() => viewer.idle);

  const done = (): boolean =>
    !viewer.searching &&
    (seen.status?.search.scanned ?? 0) >= (seen.status?.search.toScan ?? 1);

  // A drag that starts part-way along a line and ends part-way down the page,
  // so the scope is a *range within* a page. Dragging the whole page would
  // leave both of the range's ends untested: a mutation relaxing the start
  // bound survived exactly that, because the selection began at character 0 and
  // "at or after 0" is true of everything.
  drag(root, [WIDTH / 2, HEIGHT / 5], [WIDTH - 120, HEIGHT * 0.55]);
  await settle(() => viewer.idle);
  const selected = viewer.selectedText;
  if (selected === "") {
    skipBoth("dragging over the page selected nothing");
    return;
  }

  // The query comes out of the selection, so it is certain to be inside the
  // scope. Taken from the page instead it might not be, and a scoped search
  // that finds nothing satisfies "fewer than before" perfectly.
  const needle = pickNeedle([...selected].map((ch) => ch.codePointAt(0) ?? 0));
  if (!needle) {
    skipBoth(
      `the ${selected.length} selected characters yielded no word to search for`,
    );
    return;
  }

  viewer.search(needle);
  await settle(done);
  const whole = viewer.searchMatches.length;
  // Hits on the selection's own page, which is the number that makes this a
  // check on the *range*. A scoped scan only asks about the pages in the scope,
  // so comparing against the whole document proves the page list and says
  // nothing about the two ends of it --- and two mutations relaxing exactly
  // those ends survived while this compared against `whole`.
  const onFirstPage = viewer.searchMatches.filter((m) => m.page === 0);
  if (whole < 2) {
    skipBoth(
      `"${needle}" occurs ${whole} time(s) in the document, so a narrower search proves nothing`,
    );
    viewer.clearSearch();
    viewer.clearSelection();
    return;
  }

  if (!viewer.scopeSearchToSelection()) {
    skipBoth("the selection could not be scoped to");
    return;
  }
  if (onFirstPage.length < 2) {
    skipBoth(
      `"${needle}" occurs ${onFirstPage.length} time(s) on page 1, so clipping the range proves nothing`,
    );
    viewer.clearSearch();
    viewer.clearSelection();
    return;
  }
  await settle(done);
  const scoped = viewer.searchMatches.slice();
  const outside = scoped.filter((m) => m.page !== 0 || m.endPage !== undefined);

  // Both ends of the range, and both measured against the **scope** rather than
  // against the matches that came back. Two mutations relaxing one bound each
  // survived a version that used the results: widening a bound widens the
  // numbers the precondition is computed from too, so the check turned itself
  // into a `[SKIP]` instead of going red --- a defect that switches off the
  // check that would have caught it.
  const range = viewer.searchScopeRanges?.[0];
  const droppedBefore = range
    ? onFirstPage.filter((m) => m.start < range.from).length
    : 0;
  const droppedAfter = range
    ? onFirstPage.filter((m) => m.end > range.to).length
    : 0;
  if (!range || droppedBefore === 0 || droppedAfter === 0) {
    skipBoth(
      `"${needle}": the scope is ${range ? `[${range.from}, ${range.to})` : "absent"}, with ` +
        `${droppedBefore} of page 1's hits before it and ${droppedAfter} after it --- ` +
        `both ends need something to drop`,
    );
    viewer.clearSearchScope();
    viewer.clearSearch();
    viewer.clearSelection();
    return;
  }
  check(
    NAMES[0],
    viewer.searchScoped &&
      outside.length === 0 &&
      scoped.length > 0 &&
      scoped.length === onFirstPage.length - droppedBefore - droppedAfter,
    `"${needle}": ${scoped.length} matches in [${range.from}, ${range.to}), and page 1 has ` +
      `${onFirstPage.length} with ${droppedBefore} before that range and ${droppedAfter} ` +
      `after it (${whole} in the document); ${outside.length} outside the scope`,
  );

  viewer.clearSearchScope();
  await settle(done);
  check(
    NAMES[1],
    !viewer.searchScoped &&
      viewer.searchMatches.length === whole &&
      whole > scoped.length,
    `unscoped -> ${viewer.searchMatches.length}, was ${whole} before scoping and ` +
      `${scoped.length} while scoped`,
  );

  viewer.clearSearch();
  viewer.clearSelection();
  await settle(() => viewer.idle);
}

/**
 * A phrase that runs over a page break.
 *
 * The query is built from the document rather than written here: the last word
 * on page 1 and the first word on page 2, which by construction occur in that
 * order with nothing but the break between them. A matcher that looks at one
 * page at a time cannot find it, which is what makes the check discriminating
 * without needing a fixture built for it.
 *
 * Two things are asserted, and the second is the one that matters. That
 * *something* was found is weak --- a hit anywhere would satisfy it. So each
 * half is resolved against the page it claims to be on, through a fresh
 * extraction rather than through the matcher's own snippet, and has to be the
 * word that page really ends or begins with.
 */
export async function crossPageChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const NAMES = [
    "a phrase is found across a page break",
    "each half of it lands on the page it names",
  ] as const;
  const skipBoth = (why: string): void => {
    for (const name of NAMES) skip(name, why);
  };

  if (doc.page_count < 2) {
    skipBoth("a break needs two pages");
    return;
  }
  // The **codes**, not a string, and that distinction is the whole reason this
  // helper does not return one. A match's `start` and `end` are code point indices,
  // and `String.prototype.slice` counts UTF-16 code units --- so on a page holding a
  // character above the BMP the two spaces differ by one per such character, and a
  // slice comes back one short. `encodings.pdf` is where it showed: two broken
  // `/ToUnicode` entries decode to one astral character, and the right-hand half of
  // the phrase lost its last letter while the comment below claimed to be checking
  // the index spaces against the pages.
  const codesOf = async (page: number): Promise<number[] | null> => {
    const got = await invoke<{ codes: number[] }>("page_text", {
      doc: doc.id,
      page,
    }).catch(() => null);
    return got ? got.codes : null;
  };
  const textOf = async (page: number): Promise<string | null> => {
    const codes = await codesOf(page);
    return codes ? String.fromCodePoint(...codes) : null;
  };
  /** A page's text between two **code point** indices. */
  const sliceOf = async (
    page: number,
    from: number,
    to?: number,
  ): Promise<string> => {
    const codes = await codesOf(page);
    return codes ? String.fromCodePoint(...codes.slice(from, to)) : "";
  };
  const first = await textOf(0);
  const second = await textOf(1);
  const last = /(\S+)\s*$/.exec(first ?? "")?.[1];
  const head = /^\s*(\S+)/.exec(second ?? "")?.[1];
  if (!last || !head) {
    skipBoth("page 1 or page 2 has no extractable text");
    return;
  }
  const query = `${last} ${head}`;
  if (query.length > 128) {
    skipBoth(`"${preview(query)}" is longer than a break is looked across`);
    return;
  }

  viewer.goToStart();
  await settle(() => viewer.idle);
  viewer.search(query);
  await settle(
    () =>
      !viewer.searching && (seen.status?.search.scanned ?? 0) >= doc.page_count,
  );

  const across = viewer.searchMatches.find((m) => m.endPage !== undefined);
  check(
    NAMES[0],
    across !== undefined,
    `"${preview(query)}" -> ${viewer.searchMatches.length} matches, ` +
      `${viewer.searchMatches.filter((m) => m.endPage !== undefined).length} of them across a break`,
  );

  if (!across) {
    skip(NAMES[1], "nothing was found to resolve");
    viewer.clearSearch();
    return;
  }
  // Re-extracted, so the two index spaces are checked against the pages rather
  // than against the reply that reported them --- and sliced by code point, which
  // is the space the indices are in. See `sliceOf`.
  const left = await sliceOf(across.page, across.start);
  const right = await sliceOf(across.endPage ?? -1, 0, across.end);
  check(
    NAMES[1],
    across.page === 0 &&
      across.endPage === 1 &&
      left.trim() === last &&
      right.trim() === head,
    `page ${across.page} from ${across.start} is "${preview(left)}" (wanted "${preview(last)}"), ` +
      `page ${across.endPage} to ${across.end} is "${preview(right)}" (wanted "${preview(head)}")`,
  );
  viewer.clearSearch();
}
