/**
 * Links and their announcement, the outline, and where a destination lands.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import type { DocumentInfo } from "../ipc";
import type { Links, Link } from "../links";
import {
  type WebTarget,
  type Outline,
  allRows,
  isNavigable,
  type Row,
} from "../outline";
import { Sidebar } from "../sidebar";
import { Viewer } from "../viewer";
import {
  check,
  HEIGHT,
  key,
  pointer,
  settle,
  skip,
  WIDTH,
} from "./common";
import { preview } from "./invert";

/**
 * Whether a screen reader is told that a cross-reference is a link.
 *
 * The half of the link work a sighted reader never sees, and the half that is
 * hardest to notice missing: the words are announced either way, so a table of
 * contents read as prose is indistinguishable from one read correctly unless
 * somebody is listening.
 *
 * Asserted against the **rendered tree** rather than against the links the
 * backend returned, and the two controls are what make it mean something: a
 * marked-up run must carry the role, and the page's ordinary text must not ---
 * a layer that put the role on every element would announce a page of links.
 */
export async function linkAnnouncementChecks(viewer: Viewer): Promise<void> {
  const names = [
    "a link in the page is announced as a link",
    "the text around it is not",
    "no element that could carry a URL is created",
  ];

  // Whichever page the links live on, since most of the corpus has none at all.
  const first = viewer.firstLinkPage;
  if (first === -1) {
    for (const name of names) skip(name, "the document has no links");
    return;
  }
  viewer.goToPage(first);
  await settle(() => viewer.idle);

  const page = viewer.accessibleText.elementFor(first);
  if (!page) {
    for (const name of names) skip(name, `page ${first + 1} is not in the tree yet`);
    return;
  }

  const marked = [...page.querySelectorAll('[role="link"]')];
  check(
    names[0] as string,
    marked.length > 0,
    `${marked.length} announced on page ${first + 1}, ` +
      `first ${JSON.stringify((marked[0]?.textContent ?? "").slice(0, 40))}`,
  );

  // The control. Everything the page says, minus what the roles cover, has to
  // be non-empty --- otherwise the role is on the page element itself and the
  // check above passes for a layer that marks everything.
  const all = (page.textContent ?? "").trim();
  const inside = marked.map((node) => node.textContent ?? "").join("");
  check(
    names[1] as string,
    all.length > inside.length && inside.length > 0,
    `${all.length} characters on the page, ${inside.length} inside links`,
  );

  // T8, asserted on what was built rather than by reading the source. The gate
  // forbids creating these; this is the same claim from the other end.
  const urlBearing = page.querySelectorAll("a, iframe, img, embed, object").length;
  check(
    names[2] as string,
    urlBearing === 0,
    `${urlBearing} URL-bearing element(s) in the page's accessible text`,
  );
}

/**
 * Links: following one, being refused one, and getting back.
 *
 * The load-bearing assertion is **where the viewer ended up**, not that a click
 * was accepted: a link that scrolls nowhere and a link that scrolls to the wrong
 * page both look like a press the viewer took. So the target page is read out of
 * the link's own resolved destination and compared against `position.page`, and
 * the viewer is sent somewhere else first --- a jump to a page it is already on
 * cannot fail.
 *
 * The refusal has its own control, and it is the one worth stating. A refused
 * link must *not* move the reader, and "did not move" is exactly what a press
 * that missed the rectangle also produces. So the check asserts two things at
 * once: the position is unchanged **and** the refusal was reported. Without the
 * second half it would pass for a press that landed on blank paper.
 */
export async function linkChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  /** Everything the viewer has reported through `onError`, in order. */
  problems: readonly string[],
): Promise<void> {
  const names = [
    "pressing a link goes where it points",
    "going back returns to where the link was pressed",
    "going forward returns to the destination",
    "a refused link says so and moves nobody",
    "the pointer changes over a link",
    "the keyboard reaches a link, and draws a ring on it",
    "Enter follows the link the keyboard is on",
    "Escape takes the keyboard off the link",
    "walking off the end says so rather than wrapping",
  ];

  let links: Links;
  try {
    links = await invoke<Links>("document_links", { doc: doc.id });
  } catch (e) {
    check("reads the document's links", false, String(e));
    for (const name of names) skip(name, "the links could not be read");
    return;
  }
  check(
    "reads the document's links",
    Array.isArray(links.items),
    `${links.items.length} links in ${links.scan_ms.toFixed(2)} ms`,
  );

  viewer.setLinks(links.items);
  check(
    "the viewer takes the links",
    viewer.linkCount === links.items.length,
    `${viewer.linkCount} held for ${links.items.length} read`,
  );

  if (links.items.length === 0) {
    for (const name of names) skip(name, "the document has no links");
    return;
  }

  /** Presses the centre of a link's rectangle, having scrolled to its page. */
  const pressCentre = async (link: Link): Promise<{ x: number; y: number }> => {
    viewer.goToPage(link.page);
    await settle(() => viewer.idle);
    const at = viewer.screenPoint(
      link.page,
      (link.rect[0] + link.rect[2]) / 2,
      (link.rect[1] + link.rect[3]) / 2,
    );
    pointer(root, "pointerdown", at.x, at.y);
    pointer(root, "pointerup", at.x, at.y);
    await settle(() => viewer.idle);
    return at;
  };

  // Which destination pages can actually become the page being read, measured
  // by going to each one rather than inferred from a page count.
  //
  // The last page of a document shorter than the window can never reach the
  // top of the viewport, so `position.page` never names it and every "the
  // link went where it points" assertion fails on a viewer that did exactly
  // what it was told. `nav.goToPage` and its two siblings already carry a
  // guard for this and it was never carried across to the links --- which is
  // the second half of the trap that guard came from: when one check's
  // precondition is strengthened, its siblings have the same one.
  //
  // Probed up front, in one pass over the distinct destination pages, so that
  // no check below has to move the viewer to find out. A probe run inside the
  // Enter check would leave the reader on the page Enter is meant to take
  // them to, and a jump to a page you are already on cannot fail.
  //
  // **Both ends of a link, not just the destination.** Back has to return to
  // the page the link was pressed *on*, so an unobservable origin fails a
  // check whose subject is the history rather than the page --- measured on
  // `links-rotated.pdf`, where the only link sits on the last page and the
  // check reported `back taken to page 1, wanted 2` against a viewer that had
  // never been able to be on page 2.
  const reachable = new Map<number, boolean>();
  const probe = async (page: number): Promise<void> => {
    if (reachable.has(page)) return;
    viewer.goToPage(page);
    await settle(() => viewer.idle);
    reachable.set(page, viewer.position.page === page);
  };
  for (const item of links.items) {
    await probe(item.page);
    if (item.target.kind === "page") await probe(item.target.page);
  }
  /** Why a page cannot be landed on observably, or `null` if it can. */
  const unreachable = (page: number): string | null =>
    reachable.get(page)
      ? null
      : `page ${page + 1} of ${doc.page_count} cannot reach the top of the ` +
        `viewport, so landing on it is not observable`;

  // A link that goes to a page the viewer will not already be on, so "it moved"
  // is observable. Its own page is excluded for the same reason.
  const candidates = links.items.filter(
    (item) =>
      item.target.kind === "page" &&
      item.target.page !== item.page &&
      item.rect[2] - item.rect[0] > 2 &&
      item.rect[3] - item.rect[1] > 2,
  );
  const goes = candidates.find(
    (item) =>
      item.target.kind === "page" &&
      reachable.get(item.page) === true &&
      reachable.get(item.target.page) === true,
  );

  if (!goes || goes.target.kind !== "page") {
    const other = candidates[0];
    const why =
      other && other.target.kind === "page"
        ? (unreachable(other.page) ??
          unreachable(other.target.page) ??
          "no link points at a different page")
        : "no link points at a different page";
    for (const name of names.slice(0, 3)) skip(name, why);
  } else {
    const from = goes.page;
    await pressCentre(goes);
    const landed = viewer.position.page;
    check(
      names[0] as string,
      landed === goes.target.page,
      `pressed on page ${from + 1}, landed on ${landed + 1}, wanted ${goes.target.page + 1}`,
    );

    // Back, then forward. Asserted as *positions*, because a history that
    // recorded the wrong end would still change the page and still look right
    // from a single assertion.
    const before = viewer.historyDepths;
    const went = viewer.goBack();
    await settle(() => viewer.idle);
    check(
      names[1] as string,
      went && viewer.position.page === from,
      `back ${went ? "taken" : "refused"} to page ${viewer.position.page + 1}, wanted ${from + 1}` +
        ` (stack was ${before.back})`,
    );

    const forward = viewer.goForward();
    await settle(() => viewer.idle);
    check(
      names[2] as string,
      forward && viewer.position.page === goes.target.page,
      `forward ${forward ? "taken" : "refused"} to page ${viewer.position.page + 1}, ` +
        `wanted ${goes.target.page + 1}`,
    );
  }

  // A refused link. `problems` is the harness's own record of what `onError`
  // reported, which is what makes "did not move" mean something.
  const refused = links.items.find(
    (item) =>
      item.target.kind === "refused" &&
      item.rect[2] - item.rect[0] > 2 &&
      item.rect[3] - item.rect[1] > 2,
  );
  if (!refused) {
    skip(names[3] as string, "no link is refused");
  } else {
    const saidBefore = problems.length;
    await pressCentre(refused);
    const stayed = viewer.position.page === refused.page;
    const said = problems.length > saidBefore;
    check(
      names[3] as string,
      stayed && said,
      `on page ${viewer.position.page + 1} of ${doc.page_count}, ` +
        `${problems.length - saidBefore} message(s): ${JSON.stringify(problems.at(-1) ?? "")}`,
    );
  }

  // The pointer. It is the only thing that tells a reader a run of text can be
  // pressed, so an invisible link with no cursor is a feature nobody finds.
  // Asserted against the cursor *off* a link as well, or a surface that always
  // says "pointer" would pass.
  const any = links.items.find(
    (item) => item.rect[2] - item.rect[0] > 2 && item.rect[3] - item.rect[1] > 2,
  );
  if (!any) {
    skip(names[4] as string, "no link has a rectangle on the page");
  } else {
    viewer.goToPage(any.page);
    await settle(() => viewer.idle);
    const at = viewer.screenPoint(
      any.page,
      (any.rect[0] + any.rect[2]) / 2,
      (any.rect[1] + any.rect[3]) / 2,
    );
    pointer(root, "pointermove", at.x, at.y);
    await frame();
    const over = viewer.cursorName;
    // The far corner, which no rectangle in the corpus reaches.
    pointer(root, "pointermove", WIDTH - 20, HEIGHT - 20);
    await frame();
    const off = viewer.cursorName;
    check(
      names[4] as string,
      over === "pointer" && off !== "pointer",
      `over a link ${JSON.stringify(over)}, off it ${JSON.stringify(off)}`,
    );
  }

  // --- the keyboard, which is the only way to reach a link without a pointer.
  viewer.clearLinkFocus();
  viewer.goToStart();
  await settle(() => viewer.idle);

  const before = viewer.linkFocus;
  const stepped = viewer.stepLink(1);
  await settle(() => viewer.idle);
  check(
    names[5] as string,
    stepped && before === -1 && viewer.linkFocus !== -1 && viewer.linkRingShown,
    `focus ${before} -> ${viewer.linkFocus}, ring ${viewer.linkRingShown ? "drawn" : "absent"}`,
  );

  // Enter, on whichever link the keyboard reached. Asserted against the link's
  // own destination rather than "the page changed": a link that points at its
  // own page is legal, and a check reading only the page would call that a
  // failure.
  const onIt = links.items.find((item) => item.id === viewer.linkFocus);
  if (!onIt || onIt.target.kind !== "page") {
    skip(
      names[6] as string,
      onIt ? "the first link is not a page destination" : "the keyboard reached no link",
    );
  } else if (unreachable(onIt.target.page) !== null) {
    skip(names[6] as string, unreachable(onIt.target.page) ?? "");
  } else {
    const wanted = onIt.target.page;
    const key = new KeyboardEvent("keydown", { key: "Enter", bubbles: true });
    root.dispatchEvent(key);
    await settle(() => viewer.idle);
    check(
      names[6] as string,
      viewer.position.page === wanted,
      `Enter on #${onIt.id} landed on page ${viewer.position.page + 1}, wanted ${wanted + 1}`,
    );
  }

  // Escape. The control is that something was focused first, or "nothing is
  // focused afterwards" says nothing about the key.
  //
  // **Back to the top before stepping**, which is the control's precondition
  // rather than tidiness. Stepping starts from the viewport and reports
  // running out rather than wrapping, both deliberate --- so on a document
  // whose only link sits above where the Enter check left the reader, the step
  // correctly focuses nothing and the control cannot be established. That is
  // exactly `links-cropped.pdf`: one link, pointing at its own page, and the
  // check before this one follows it. The first keyboard check already does
  // this; the two below had inherited whatever position they were handed.
  viewer.clearLinkFocus();
  viewer.goToStart();
  await settle(() => viewer.idle);
  viewer.stepLink(1);
  const focused = viewer.linkFocus;
  root.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  if (focused === -1) {
    skip(names[7] as string, "stepping from the top of the document focused no link");
  } else {
    check(
      names[7] as string,
      viewer.linkFocus === -1 && !viewer.linkRingShown,
      `focus ${focused} -> ${viewer.linkFocus}, ` +
        `ring ${viewer.linkRingShown ? "drawn" : "absent"}`,
    );
  }

  // The end of the walk. Stepped forward past every link, which must report
  // rather than wrap --- and the assertion is that it *said* so, since a viewer
  // that silently did nothing produces the same final position.
  viewer.clearLinkFocus();
  viewer.goToStart();
  await settle(() => viewer.idle);
  const saidBefore = problems.length;
  for (let step = 0; step <= links.items.length; step += 1) viewer.stepLink(1);
  await settle(() => viewer.idle);
  const last = viewer.linkFocus;
  check(
    names[8] as string,
    last !== -1 && problems.length > saidBefore,
    `stopped on #${last} after ${links.items.length + 1} steps, ` +
      `${problems.length - saidBefore} message(s): ${JSON.stringify(problems.at(-1) ?? "")}`,
  );
  viewer.clearLinkFocus();
}

/**
 * The outline, and the sidebar showing it.
 *
 * Most of the corpus has no outline at all, so nearly everything here is
 * conditional --- and each condition is a `skip` with its reason rather than a
 * silently absent row. A check that vanishes on some documents cannot be told
 * apart from one that ran, which this file has already been caught doing once.
 *
 * Two assertions are the load-bearing ones and both carry their own control:
 *
 * - **Activating a row moves the viewer**, asserted against *not having been on
 *   that page already*. The target is chosen to be a page the viewer is not on.
 * - **A destination's y is respected**, asserted by comparing two entries on the
 *   *same* page. That is what can see a y-flip: both land on the right page
 *   either way, and only the distance from its top edge inverts. Comparing an
 *   entry against a different page's would pass under the flip.
 */
export async function outlineChecks(
  viewer: Viewer,
  sidebar: Sidebar,
  doc: DocumentInfo,
  /**
   * Where the sidebar's `onWebLink` records, so `webLinkCheck` can read it.
   *
   * Passed in rather than read from a module-level array: the recorder belongs
   * to the run that built the sidebar, and a shared one would carry an earlier
   * document's activations into this one's count.
   */
  webLinksActivated: WebTarget[],
): Promise<void> {
  let outline: Outline;
  try {
    outline = await invoke<Outline>("document_outline", { doc: doc.id });
  } catch (e) {
    check("reads the document's outline", false, String(e));
    return;
  }

  check(
    "reads the document's outline",
    outline.total === countItems(outline),
    `${outline.total} entries in ${outline.walk_ms.toFixed(2)} ms`,
  );

  sidebar.setOutline(outline);
  const rows = allRows(outline.items);

  if (rows.length === 0) {
    // Listed one by one rather than collapsed into a single "no outline" line:
    // the count of checks has to be the same whichever document is used, or a
    // check that silently stopped existing looks exactly like one that passed.
    const why = "the document has no outline";
    for (const name of [
      "the sidebar draws a row per entry",
      "the rows are a tree, not a list",
      "the whole tree is one tab stop",
      "collapsing a row hides its children",
      "expanding it again brings them back",
      "activating a row goes to its page",
      "a destination's y is measured from the page top",
      "scrolling moves the highlight to the right entry",
      "a refused action is drawn but does nothing",
      "a web link is reported rather than followed",
    ]) {
      skip(name, why);
    }
    return;
  }

  // Top-level rows plus the children of whatever the producer left open.
  const shown = sidebar.visible.length;
  check(
    "the sidebar draws a row per entry",
    shown > 0 && shown <= rows.length,
    `${shown} rows drawn of ${rows.length} entries`,
  );

  const treeitems = document.querySelectorAll(
    '.tpdf-sidebar [role="treeitem"]',
  );
  check(
    "the rows are a tree, not a list",
    treeitems.length === shown &&
      [...treeitems].every((row) => row.hasAttribute("aria-level")),
    `${treeitems.length} treeitems, all levelled=${[...treeitems].every((r) => r.hasAttribute("aria-level"))}`,
  );

  const tabbable = [...treeitems].filter(
    (row) => row instanceof HTMLElement && row.tabIndex === 0,
  );
  check(
    "the whole tree is one tab stop",
    tabbable.length === 1,
    `${tabbable.length} of ${treeitems.length} rows are tabbable`,
  );

  // Collapsing, with the control built in: a parent whose children are shown
  // now and gone afterwards. A parent that was already collapsed would satisfy
  // "the children are absent" without anything having happened.
  const parent = findExpandedParent(sidebar);
  if (!parent) {
    const why = "no row in this outline has visible children";
    skip("collapsing a row hides its children", why);
    skip("expanding it again brings them back", why);
  } else {
    const before = sidebar.visible.length;
    sidebar.elementFor(parent)?.focus();
    key(sidebar.elementFor(parent)!, "ArrowLeft");
    const after = sidebar.visible.length;
    check(
      "collapsing a row hides its children",
      after < before,
      `${before} rows -> ${after} after collapsing "${parent}"`,
    );
    key(sidebar.elementFor(parent)!, "ArrowRight");
    check(
      "expanding it again brings them back",
      sidebar.visible.length === before,
      `${after} rows -> ${sidebar.visible.length}`,
    );
  }

  // Navigation. The target is deliberately a page the viewer is not on.
  viewer.goToStart();
  await settle(() => viewer.idle);
  const here = viewer.position.page;
  const elsewhere = rows.find(
    (row) => isNavigable(row.target) && row.target.page !== here,
  );
  if (!elsewhere || !isNavigable(elsewhere.target)) {
    skip(
      "activating a row goes to its page",
      `every entry points at page ${here + 1}, which is where we are`,
    );
  } else {
    sidebar.reveal(elsewhere.id);
    const element = sidebar.elementFor(elsewhere.id);
    if (!element) {
      check(
        "activating a row goes to its page",
        false,
        `no row for ${elsewhere.id}`,
      );
    } else {
      element.focus();
      key(element, "Enter");
      await frame();
      check(
        "activating a row goes to its page",
        viewer.position.page === elsewhere.target.page,
        `"${preview(elsewhere.title)}" from page ${here + 1} to ` +
          `${viewer.position.page + 1}, wanted ${elsewhere.target.page + 1}`,
      );
    }
  }

  await destinationOffsetCheck(viewer, sidebar, rows);
  webLinkCheck(viewer, sidebar, rows, webLinksActivated);
  await highlightCheck(viewer, sidebar, rows);
  refusalCheck(viewer, sidebar, rows);
}

/** Total entries in the tree, counted independently of the walk's own tally. */
function countItems(outline: Outline): number {
  return allRows(outline.items).length;
}

/** A visible row that currently shows children, or `null`. */
function findExpandedParent(sidebar: Sidebar): string | null {
  const rows = document.querySelectorAll<HTMLElement>(
    '.tpdf-sidebar [role="treeitem"][aria-expanded="true"]',
  );
  for (const row of rows) {
    const id = row.dataset.id;
    if (id && sidebar.elementFor(id)) return id;
  }
  return null;
}

/**
 * Two entries on one page: the lower one must land further down.
 *
 * This is the only check here a y-flip fails. Both entries resolve to the same
 * page under either convention, so page-level assertions are blind to it; what
 * inverts is which of the two is nearer the top.
 */
async function destinationOffsetCheck(
  viewer: Viewer,
  sidebar: Sidebar,
  rows: Row[],
): Promise<void> {
  const name = "a destination's y is measured from the page top";
  const byPage = new Map<number, Row[]>();
  for (const row of rows) {
    if (!isNavigable(row.target)) continue;
    const list = byPage.get(row.target.page) ?? [];
    list.push(row);
    byPage.set(row.target.page, list);
  }

  for (const [, group] of byPage) {
    const tops = group
      .map((row) => (isNavigable(row.target) ? row.target : null))
      .filter((target) => target !== null)
      .sort((a, b) => (a.top_pt ?? 0) - (b.top_pt ?? 0));
    // The highest and lowest on the page, whatever they are. Written first as
    // "one at the very top and one below 50 pt", which skipped on the only
    // fixture that has the pair --- its two entries sit at 240 and 440 pt, so
    // neither is at the top and the check reported itself inapplicable.
    const high = tops[0];
    const low = tops[tops.length - 1];
    if (!high || !low || (low.top_pt ?? 0) - (high.top_pt ?? 0) < 50) continue;

    sidebar.reveal(group[0]!.id);
    viewer.goToDestination(high.page, high.top_pt);
    await frame();
    const upper = viewer.offset;
    viewer.goToDestination(low.page, low.top_pt);
    await frame();
    const lower = viewer.offset;

    // A page at the very end of the document clamps both to `maxOffset`, which
    // would pass this by accident in one direction and fail it in the other.
    if (upper >= viewer.maxOffset - 1) {
      skip(name, "the shared page is the last one and both jumps clamp");
      return;
    }
    check(
      name,
      lower > upper,
      `page ${high.page + 1}: y=${high.top_pt ?? 0} lands at ${upper.toFixed(0)}, ` +
        `y=${low.top_pt} at ${lower.toFixed(0)}`,
    );
    return;
  }

  skip(name, "no page in this outline has two entries at different heights");
}

/** Scrolling to an entry's page must move the highlight onto that entry. */
async function highlightCheck(
  viewer: Viewer,
  sidebar: Sidebar,
  rows: Row[],
): Promise<void> {
  const name = "scrolling moves the highlight to the right entry";
  const targets = rows.filter((row) => isNavigable(row.target));
  const first = targets[0];
  const later = targets.find(
    (row) =>
      isNavigable(row.target) &&
      isNavigable(first!.target) &&
      row.target.page > first!.target.page,
  );
  if (!first || !later || !isNavigable(later.target)) {
    skip(name, "this outline has no two entries on different pages");
    return;
  }

  viewer.goToStart();
  await settle(() => viewer.idle);
  const before = sidebar.currentRow;

  viewer.goToDestination(later.target.page, later.target.top_pt);
  await settle(() => viewer.idle);
  // The control: if the highlight was already on the target row, arriving there
  // proves nothing about whether it follows the scroll.
  if (before === later.id) {
    skip(name, `the highlight was already on "${preview(later.title)}"`);
    return;
  }
  check(
    name,
    sidebar.currentRow === later.id,
    `"${before}" -> "${sidebar.currentRow}", wanted "${later.id}" ` +
      `(${preview(later.title)})`,
  );
}

/** An entry carrying a refused action is drawn, marked, and inert. */
function refusalCheck(viewer: Viewer, sidebar: Sidebar, rows: Row[]): void {
  const name = "a refused action is drawn but does nothing";
  const refused = rows.find((row) => row.target.kind === "refused");
  if (!refused) {
    skip(name, "this outline has no /Launch, /URI or /GoToR entry");
    return;
  }

  sidebar.reveal(refused.id);
  const element = sidebar.elementFor(refused.id);
  const before = viewer.offset;
  element?.focus();
  if (element) key(element, "Enter");
  check(
    name,
    element !== null &&
      element.getAttribute("aria-disabled") === "true" &&
      viewer.offset === before,
    `"${preview(refused.title)}" disabled=${element?.getAttribute("aria-disabled")}, ` +
      `offset ${before.toFixed(0)} -> ${viewer.offset.toFixed(0)}`,
  );
}

/**
 * An entry carrying a web link is drawn live, reports, and moves nothing.
 *
 * The counterpart of {@link refusalCheck}, and the three assertions are the
 * three ways this shipped wrong in the unit tests before they were written.
 * The row must **not** be `aria-disabled`, because `isNavigable` answers "is a
 * page in this document" and a web target is not one --- the natural writing
 * greys it. The activation must reach `onWebLink`, which is what says the
 * branch runs before the refusal test rather than after it. And the viewer must
 * not have moved, because a web link is not a jump.
 *
 * Nothing opens: the harness's `onWebLink` records rather than calling
 * `open_web_link`, so no browser window arrives over the window this is
 * measuring. What is *not* covered here is the dialog and the OS opener ---
 * `weblinkdialog.test.ts` has the first and nothing automated has the second.
 */
function webLinkCheck(
  viewer: Viewer,
  sidebar: Sidebar,
  rows: Row[],
  activated: WebTarget[],
): void {
  const name = "a web link is reported rather than followed";
  const web = rows.find((row) => row.target.kind === "web");
  if (!web) {
    skip(name, "this outline has no http or https /URI entry");
    return;
  }

  sidebar.reveal(web.id);
  const element = sidebar.elementFor(web.id);
  const before = viewer.offset;
  const seen = activated.length;
  element?.focus();
  if (element) key(element, "Enter");
  check(
    name,
    element !== null &&
      element.getAttribute("aria-disabled") === null &&
      activated.length === seen + 1 &&
      viewer.offset === before,
    `"${preview(web.title)}" disabled=${element?.getAttribute("aria-disabled")}, ` +
      `reported ${activated.length - seen}, ` +
      `offset ${before.toFixed(0)} -> ${viewer.offset.toFixed(0)}`,
  );
}
