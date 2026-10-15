/**
 * Turning, deleting and moving pages, and the text of a turned page.
 *
 * One phase of the window check, split out of `viewercheck.ts` on 2026-10-15,
 * which runs it; that file says what the check is and how it is run.
 */

import { invoke } from "@tauri-apps/api/core";
import { frame } from "../checkreport";
import type { DocumentInfo } from "../ipc";
import { Viewer, type ViewerStatus } from "../viewer";
import {
  check,
  eventually,
  key,
  settle,
  skip,
} from "./common";
import { preview } from "./invert";

/**
 * Turning one page of the document, which is the edit a save writes.
 *
 * The whole difficulty is telling this apart from a view rotation, and the
 * assertions are chosen for exactly that. A defect that turned the *view*
 * instead would satisfy every statement about the page that was turned --- it
 * would be the right shape, its tiles would be gone, its text would run
 * sideways --- so the checks that carry the weight are the two negative ones: a
 * page nobody touched must keep its shape, and `viewer.rotation` must not have
 * moved. Written with only the positive half, a `setPageTurns` that called
 * `rotateBy` would pass.
 *
 * Runs after {@link rotationChecks}, which leaves the view upright, and puts the
 * page back before returning --- every phase after this one inherits whatever
 * state it is left in, and the file's own history has eight assertions going red
 * across three phases for exactly that reason.
 */
export async function pageRotationChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [
    "a page turn is recorded against that page",
    "the turned page has the turned shape",
    "a page nobody turned keeps its shape",
    "turning a page does not rotate the view",
    "a page turn discards that page's pixels",
    "recovers coverage after a page turn",
    "the text layer turns with the page",
    "a page nobody turned keeps its text upright",
    "turning a page back restores the layout",
    "a half turn discards the pixels its box did not move",
  ];

  if (doc.page_count < 2) {
    for (const name of names) skip(name, "the document has one page");
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const target = 0;
  const other = 1;
  const uprightTarget = viewer.pageBoxCssOf(target);
  const uprightOther = viewer.pageBoxCssOf(other);
  const uprightRotation = viewer.rotation;

  // A square page cannot report a quarter turn through its shape, and neither
  // can a page whose box is within the tolerance of square. Skipping is the
  // honest answer: the alternative is three assertions that hold whatever the
  // code does.
  const aspect = uprightTarget.width / uprightTarget.height;
  if (Math.abs(aspect - 1) < 0.05) {
    for (const name of names) {
      skip(name, `page 1 is square (${aspect.toFixed(3)}), so a turn is invisible`);
    }
    return;
  }

  // Read before the turn, so the text assertions compare against this document.
  const otherTextBefore = viewer.textOn(other);

  viewer.setPageTurns(target, 1);
  await frame();

  check(
    names[0] ?? "",
    viewer.pageExtraTurns(target) === 1 && viewer.pageExtraTurns(other) === 0,
    `page 1 turned ${viewer.pageExtraTurns(target) * 90}, ` +
      `page 2 turned ${viewer.pageExtraTurns(other) * 90}`,
  );

  const turnedTarget = viewer.pageBoxCssOf(target);
  const turnedAspect = turnedTarget.width / turnedTarget.height;
  check(
    names[1] ?? "",
    Math.abs(turnedAspect * aspect - 1) < 0.02,
    `${uprightTarget.width.toFixed(0)}x${uprightTarget.height.toFixed(0)} -> ` +
      `${turnedTarget.width.toFixed(0)}x${turnedTarget.height.toFixed(0)}, ` +
      `aspect ${aspect.toFixed(3)} then ${turnedAspect.toFixed(3)}`,
  );

  // The assertion that separates a page turn from a view rotation. A viewer that
  // turned everything would pass every check above this line.
  //
  // **Proportions, not pixels**, and the difference is a whole corpus. Fit-width
  // sizes the layout to the widest page, so turning page 1 to landscape makes it
  // the widest and every other page is legitimately rescaled: on `text-heavy` the
  // neighbour went 640x828 to 495x640 --- 22% smaller, and the *same* shape to
  // three decimals. Comparing absolute boxes called that a defect. Comparing the
  // ratio still catches what this check exists for, because a page that was
  // turned reports the reciprocal, which no rescale can produce.
  //
  // Found by the first sweep across all fourteen corpora, having been written and
  // watched pass on one. That is what the sweep is for.
  const turnedOther = viewer.pageBoxCssOf(other);
  const otherAspect = uprightOther.width / uprightOther.height;
  const turnedOtherAspect = turnedOther.width / turnedOther.height;
  const rescaled = Math.abs(turnedOther.width - uprightOther.width) >= 1;
  check(
    names[2] ?? "",
    Math.abs(turnedOtherAspect / otherAspect - 1) < 0.02,
    `page 2 was ${uprightOther.width.toFixed(0)}x${uprightOther.height.toFixed(0)}, ` +
      `now ${turnedOther.width.toFixed(0)}x${turnedOther.height.toFixed(0)} ` +
      `-- aspect ${otherAspect.toFixed(3)} then ${turnedOtherAspect.toFixed(3)}` +
      (rescaled ? ", rescaled by the wider page" : ", unmoved"),
  );

  check(
    names[3] ?? "",
    viewer.rotation === uprightRotation,
    `view turns ${uprightRotation} -> ${viewer.rotation}`,
  );

  // The control for the recovery check below: a turn that kept its tiles would
  // recover instantly, and "recovers" would have waited for nothing.
  check(
    names[4] ?? "",
    (seen.status?.any ?? 1) < 0.999,
    `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );
  await eventually(
    names[5] ?? "",
    () => (seen.status?.sharp ?? 0) >= 0.999,
    () => `sharp=${((seen.status?.sharp ?? 0) * 100).toFixed(1)}%`,
  );

  await pageTurnTextChecks(viewer, doc, target, other, otherTextBefore, names);

  // Back to where the phase found it, and asserted rather than assumed --- a
  // restore that silently did nothing would leave every later phase reading a
  // document with a sideways first page.
  viewer.setPageTurns(target, 0);
  await frame();
  const restored = viewer.pageBoxCssOf(target);
  check(
    names[8] ?? "",
    viewer.pageExtraTurns(target) === 0 &&
      Math.abs(restored.width - uprightTarget.width) < 1 &&
      Math.abs(restored.height - uprightTarget.height) < 1,
    `${turnedTarget.width.toFixed(0)}x${turnedTarget.height.toFixed(0)} -> ` +
      `${restored.width.toFixed(0)}x${restored.height.toFixed(0)}, ` +
      `wanted ${uprightTarget.width.toFixed(0)}x${uprightTarget.height.toFixed(0)}`,
  );
  await settle(() => viewer.idle);

  // The half turn, and it is the only thing that tests the ordering inside
  // `Scroller.setPageTurns`. That method discards the page's tiles *before* it
  // touches the geometry, because `applySizes` invalidates a page only when its
  // box dimensions change --- and 180 degrees swaps width and height twice, so
  // the box is identical and `applySizes` sees nothing.
  //
  // A quarter turn cannot show this: the box does change, so the geometry pass
  // invalidates the page whether the explicit call is there or not. Found by a
  // mutation that deleted the call and survived every check above, which is the
  // harness reporting a hole in the corpus rather than a hole in the code. The
  // bug it would have let through is plain to a reader: turn a page twice, and
  // the tiles on screen are still the ones from before the turn.
  // Full coverage FIRST, or the assertion below cannot fail. `settle(idle)` is
  // not the same thing: the viewer goes idle while tiles are still arriving, so
  // `any` was already under the threshold when the half turn happened and the
  // check passed whether the invalidation ran or not. The mutation survived a
  // whole run saying exactly that.
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const beforeHalf = viewer.pageBoxCssOf(target);
  viewer.setPageTurns(target, 2);
  await frame();
  const afterHalf = viewer.pageBoxCssOf(target);
  const boxHeld =
    Math.abs(afterHalf.width - beforeHalf.width) < 1 &&
    Math.abs(afterHalf.height - beforeHalf.height) < 1;
  check(
    names[9] ?? "",
    boxHeld && (seen.status?.any ?? 1) < 0.999,
    `box ${boxHeld ? "unmoved" : "MOVED"} at ` +
      `${afterHalf.width.toFixed(0)}x${afterHalf.height.toFixed(0)}, ` +
      `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );

  // Back to upright again, for the phase after this one.
  viewer.setPageTurns(target, 0);
  await settle(() => viewer.idle);
}

/**
 * A page's text as a short comparable string, or `""` if it has none.
 *
 * What makes a page identifiable to a check that can only see the layout. A
 * count cannot tell "page 2 was removed" from "page 3 was removed and everything
 * renumbered", and neither can it tell a page that moved from a page that stayed
 * --- both phases below rest on this, and both skip rather than pass when two
 * pages come back with the same string.
 */
function fingerprint(
  text: { codes: number[]; width_pt: number } | null,
): string {
  return !text || text.codes.length === 0
    ? ""
    : `${text.codes.length}@${text.width_pt.toFixed(1)}:${text.codes
        .slice(0, 32)
        .join(",")}`;
}

/**
 * Removing a page from the document, which is the edit that moves every other.
 *
 * **What makes this hard to check is the same thing that makes it worth
 * checking**: a page count that went down by one is equally true of a viewer
 * that dropped the *wrong* page, or the last one, or that renumbered without
 * moving anything. So the assertion that carries the weight is the identity one
 * --- the slot below the gap must now be showing the page that was under it ---
 * and the only thing on this side that can tell one page from another is its
 * text. Where a document has none, the check says so and skips rather than
 * asserting a count that cannot fail.
 *
 * The order is driven directly rather than through the model. That is
 * deliberate and is the same seam {@link pageRotationChecks} uses: the journal,
 * the refusals and the undo replay are in Rust and have 437 tests there, and
 * what no unit test can reach is a real layout with real tiles rearranging
 * itself. The wire between the two --- `App.svelte` asking the backend and
 * handing the answer here --- is covered by neither, and `docs/PLAN.md` says so.
 *
 * Puts the document back before returning, asserted rather than assumed, because
 * every phase after this one inherits what it leaves.
 */
export async function pageDeletionChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [
    "deleting a page leaves the document one page shorter",
    "the page below the deleted one moves up into its slot",
    "the page above the deleted one does not move",
    "the last slot is gone rather than left empty",
    "deleting a page does not rotate the view",
    "deleting a page discards what was painted",
    "recovers coverage after a page is deleted",
    "putting the page back restores the document",
    "the page that came back is the page that went",
    "the reader stays on the page they were reading",
    "the backend takes the page out of the working document",
    "the backend refuses a page it has already deleted",
    "undo puts the page back in the backend's answer",
  ];

  if (doc.page_count < 3) {
    // Only the viewer's half needs a middle page to delete. The three that ask
    // the backend need two pages and are left to say so for themselves --- on
    // `tagged.pdf` they are the only checks here that can run at all, and
    // skipping them with everything else would be a skip for a reason that is
    // not theirs.
    for (const name of names.slice(0, 10)) {
      skip(name, `the document has ${doc.page_count} page(s), and this deletes a middle one`);
    }
    await deleteCommandChecks(doc, names);
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle && (seen.status?.sharp ?? 0) >= 0.999);

  const target = 1;
  // Below the page that is about to go, so that "the reader follows their page"
  // has a direction. Where they end up is read back rather than assumed --- on a
  // short document the last page cannot reach the top of the viewport, which is
  // a trap this file has paid for once.
  viewer.goToPage(target + 1);
  await settle(() => viewer.idle);
  const readingAt = viewer.position.page;

  const before = viewer.pageOrder;
  const uprightRotation = viewer.rotation;
  const above = viewer.pageBoxCssOf(0);
  // Read while the pages are still where they were, so what the check compares
  // against is this document rather than the state it is asserting.
  const belowText = viewer.textOn(target + 1);
  const aboveText = viewer.textOn(0);
  const targetText = viewer.textOn(target);
  const lastSlot = before.length - 1;

  // Whether the two pages this phase swaps around can be told apart at all. On a
  // corpus whose pages carry the same text, "the page below moved up" and "the
  // page below did not move" produce the same reading --- a fixture where the
  // right rule and the wrong rule agree, which `AGENTS.md` says to skip rather
  // than to report as a pass.
  const targetPrint = fingerprint(targetText);
  const belowPrint = fingerprint(belowText);
  const distinct =
    targetPrint !== "" && belowPrint !== "" && targetPrint !== belowPrint;

  const without = before.filter((_unused, slot) => slot !== target);
  viewer.setPages(without);
  await frame();

  check(
    names[0] ?? "",
    (seen.status?.pageCount ?? -1) === before.length - 1,
    `${before.length} pages, now ${seen.status?.pageCount ?? -1}`,
  );

  const movedUp = fingerprint(viewer.textOn(target));
  if (!distinct) {
    skip(
      names[1] ?? "",
      belowPrint === "" || targetPrint === ""
        ? "the pages have no extractable text to tell them apart"
        : "pages 2 and 3 read alike, so moving one up is invisible here",
    );
  } else {
    // Identity by content. A count cannot tell "page 2 was removed" from "page 3
    // was removed and the pages renumbered", and those are different documents.
    check(
      names[1] ?? "",
      movedUp === belowPrint,
      `slot ${target + 1} held [${belowPrint.slice(0, 40)}], slot ${target} now ` +
        `holds [${movedUp.slice(0, 40)}]`,
    );
  }

  const aboveNow = viewer.pageBoxCssOf(0);
  const aboveTextNow = viewer.textOn(0);
  const shapeHeld =
    Math.abs(aboveNow.width / aboveNow.height - above.width / above.height) <
    0.02;
  const textHeld =
    !aboveText || !aboveTextNow
      ? true
      : aboveTextNow.codes.length === aboveText.codes.length;
  check(
    names[2] ?? "",
    shapeHeld && textHeld,
    `page 1 was ${above.width.toFixed(0)}x${above.height.toFixed(0)}, now ` +
      `${aboveNow.width.toFixed(0)}x${aboveNow.height.toFixed(0)}` +
      (aboveText && aboveTextNow
        ? `, text ${aboveText.codes.length} -> ${aboveTextNow.codes.length}`
        : ", no text to compare"),
  );

  const past = viewer.pageBoxCssOf(lastSlot);
  check(
    names[3] ?? "",
    past.width === 0 && past.height === 0,
    `slot ${lastSlot} reports ${past.width.toFixed(0)}x${past.height.toFixed(0)}`,
  );

  // The negative one, and the reason it is here: every statement above is also
  // true of a viewer that rebuilt itself from scratch, and a rebuild that lost
  // the reader's rotation is a defect they would meet immediately.
  check(
    names[4] ?? "",
    viewer.rotation === uprightRotation,
    `view turns ${uprightRotation} -> ${viewer.rotation}`,
  );

  // The control for the recovery below: pixels kept across a deletion would make
  // "recovers" a wait for something that had never stopped being true.
  check(
    names[5] ?? "",
    (seen.status?.any ?? 1) < 0.999,
    `any=${((seen.status?.any ?? 1) * 100).toFixed(1)}% one frame later`,
  );
  await eventually(
    names[6] ?? "",
    () => (seen.status?.sharp ?? 0) >= 0.999,
    () => `sharp=${((seen.status?.sharp ?? 0) * 100).toFixed(1)}%`,
  );

  // Where the reader is, after the page they were on has moved up one slot.
  // Inside this deletion rather than in one of its own: every `setPages` throws
  // both tiers away, and on the twelve A0 pages of `vector-multi` a tier-1
  // render alone costs about a second and a half --- a phase that deleted and
  // restored twice put the whole sweep past its timeout.
  if (readingAt <= target) {
    skip(
      names[9] ?? "",
      `the reader is on slot ${readingAt}, at or above the page being deleted, ` +
        "so following it moves them nowhere",
    );
  } else {
    check(
      names[9] ?? "",
      viewer.position.page === readingAt - 1,
      `was reading slot ${readingAt}, now on ${viewer.position.page} ` +
        `(wanted ${readingAt - 1})`,
    );
  }

  // Putting the whole order back, which is exactly what an undo produces: the
  // same pages, the same identities, the same slots.
  viewer.setPages(before);
  await frame();
  check(
    names[7] ?? "",
    (seen.status?.pageCount ?? -1) === before.length,
    `${seen.status?.pageCount ?? -1} pages, wanted ${before.length}`,
  );

  const restored = fingerprint(viewer.textOn(target));
  if (!distinct) {
    skip(
      names[8] ?? "",
      belowPrint === "" || targetPrint === ""
        ? "the pages have no extractable text to tell them apart"
        : "pages 2 and 3 read alike, so which one came back is invisible here",
    );
  } else {
    // The page in the target slot must be the one that was there before, not the
    // one that had moved up into it --- the failure a page count cannot see, and
    // the reason the restore is checked at all rather than assumed from the
    // count going back up.
    check(
      names[8] ?? "",
      restored === targetPrint,
      `slot ${target} holds [${restored.slice(0, 40)}], wanted ` +
        `[${targetPrint.slice(0, 40)}] and not [${belowPrint.slice(0, 40)}]`,
    );
  }
  await settle(() => viewer.idle);

  await deleteCommandChecks(doc, names);
}

/**
 * The `page_delete` command itself, against the model in the backend.
 *
 * The rest of the phase drives `Viewer.setPages` directly, which is the seam
 * that lets a check watch a real layout rearrange itself --- and says nothing
 * about the command a reader actually runs. This half asks the backend, so the
 * round trip is covered: the command is registered, it names a page by the
 * identity a state reply gave it, and it answers with a document one page
 * shorter.
 *
 * What neither half covers is `App.svelte`, which carries the answer from one to
 * the other. The harness runs instead of the shell.
 *
 * The model is left as it was found. The undo is asserted rather than assumed,
 * because every phase after this one reads a document that this could otherwise
 * have left a page short.
 */
async function deleteCommandChecks(
  doc: DocumentInfo,
  names: string[],
): Promise<void> {
  interface State {
    pages: { id: number; source: number; turns: number }[];
    can_undo: boolean;
  }

  const state = await invoke<State>("edit_state", { doc: doc.id }).catch(
    () => null,
  );
  const doomed = state?.pages[1];
  if (!state || !doomed) {
    for (const name of names.slice(10)) {
      skip(name, "the backend has no edit model with a page to spare");
    }
    return;
  }

  const after = await invoke<State>("page_delete", {
    doc: doc.id,
    page: doomed.id,
  }).catch((e: unknown) => String(e));
  if (typeof after === "string") {
    check(names[10] ?? "", false, `the command failed: ${preview(after)}`);
    for (const name of names.slice(11)) skip(name, "the deletion did not happen");
    return;
  }

  check(
    names[10] ?? "",
    after.pages.length === state.pages.length - 1 &&
      !after.pages.some((page) => page.id === doomed.id),
    `${state.pages.length} pages, now ${after.pages.length}; the deleted id is ` +
      (after.pages.some((page) => page.id === doomed.id) ? "still there" : "gone"),
  );

  // The tombstone, which is the distinction a frontend one state behind needs:
  // an id that never existed and an id that was deleted are different answers.
  const again = await invoke<State>("page_delete", {
    doc: doc.id,
    page: doomed.id,
  }).then(
    () => "",
    (e: unknown) => String(e),
  );
  check(
    names[11] ?? "",
    again.includes("deleted"),
    again ? preview(again) : "it accepted the second deletion",
  );

  const undone = await invoke<State>("edit_undo", { doc: doc.id }).catch(
    () => null,
  );
  check(
    names[12] ?? "",
    undone?.pages.length === state.pages.length &&
      undone.pages.some((page) => page.id === doomed.id),
    `${after.pages.length} pages, now ${undone?.pages.length ?? -1}, wanted ` +
      `${state.pages.length} with the deleted page among them`,
  );
}

/**
 * Moving a page: the layout rearranges, and nothing is lost on the way.
 *
 * The sibling of {@link pageDeletionChecks}, and it exists separately because a
 * move fails in a way a deletion cannot: **the page count does not move**. Every
 * observable a deletion is caught by --- one page shorter, an empty last slot,
 * coverage dropping --- reads the same for a move that worked and a move that
 * did nothing at all. So what is asserted here is identity, by the text on each
 * page, and the length is asserted to have *stayed*.
 *
 * A later page is moved to the **front** rather than an early one to the back,
 * which is not arbitrary: the reader stands on the page that moves, and only a
 * page that can reach the top of the viewport can be checked for having been
 * followed. The last page of a short document cannot, which is a trap this file
 * has already paid for.
 *
 * Two `setPages` calls and no coverage wait, deliberately. Each one throws both
 * tiers away and on the twelve A0 pages of `vector-multi` a tier-1 render costs
 * about a second and a half; that the pixels are discarded and come back is
 * {@link pageDeletionChecks}'s to assert, and asserting it twice would buy a
 * minute of sweep for nothing.
 *
 * Puts the document back before returning, asserted rather than assumed.
 */
export async function pageMoveChecks(
  root: HTMLElement,
  viewer: Viewer,
  doc: DocumentInfo,
  seen: { status: ViewerStatus | null },
): Promise<void> {
  const names = [
    "moving a page leaves the document exactly as long",
    "the moved page is in the slot it was moved to",
    "the page that was there moves down rather than away",
    "the reader follows the page they were reading",
    "the moved page and the one it displaced keep their sizes",
    "putting the order back restores the document",
    "the backend moves the page in the working document",
    "the backend refuses a page moved behind itself",
    "undo puts a moved page back where it started",
  ];

  if (doc.page_count < 3) {
    for (const name of names.slice(0, 6)) {
      skip(
        name,
        `the document has ${doc.page_count} page(s), and this moves the third one`,
      );
    }
    await moveCommandChecks(doc, names);
    return;
  }

  key(root, "Home");
  await settle(() => viewer.idle);

  const target = 2;
  viewer.goToPage(target);
  await settle(() => viewer.idle);
  const readingAt = viewer.position.page;

  const before = viewer.pageOrder;
  const movedText = fingerprint(viewer.textOn(target));
  const frontText = fingerprint(viewer.textOn(0));
  const distinct =
    movedText !== "" && frontText !== "" && movedText !== frontText;

  // Both pages have been laid out by now --- the sweep started at Home and the
  // reader has just scrolled to `target` --- so these are measured sizes rather
  // than the estimate a page nobody has seen is given.
  const movedBox = viewer.pageBoxCssOf(target);
  const frontBox = viewer.pageBoxCssOf(0);
  const shape = (box: { width: number; height: number }): number =>
    box.height === 0 ? 0 : box.width / box.height;
  const shapesDiffer = Math.abs(shape(movedBox) - shape(frontBox)) > 0.02;

  const after = [...before];
  const [moved] = after.splice(target, 1);
  if (!moved) return;
  after.unshift(moved);
  viewer.setPages(after);
  await frame();

  check(
    names[0] ?? "",
    (seen.status?.pageCount ?? -1) === before.length,
    `${before.length} pages, now ${seen.status?.pageCount ?? -1}`,
  );

  if (!distinct) {
    const why =
      movedText === "" || frontText === ""
        ? "the pages have no extractable text to tell them apart"
        : `pages 1 and ${target + 1} read alike, so a move between them is invisible here`;
    skip(names[1] ?? "", why);
    skip(names[2] ?? "", why);
  } else {
    const atFront = fingerprint(viewer.textOn(0));
    check(
      names[1] ?? "",
      atFront === movedText,
      `slot 0 holds [${atFront.slice(0, 40)}], wanted [${movedText.slice(0, 40)}]`,
    );
    // The other direction, and it is what separates a move from a copy: the page
    // that was at the front is still in the document, one slot further down.
    const displaced = fingerprint(viewer.textOn(1));
    check(
      names[2] ?? "",
      displaced === frontText,
      `slot 1 holds [${displaced.slice(0, 40)}], wanted [${frontText.slice(0, 40)}]`,
    );
  }

  if (readingAt !== target) {
    skip(
      names[3] ?? "",
      `the reader is on slot ${readingAt} rather than the page being moved`,
    );
  } else {
    // By identity, which is the whole reason `Viewer.setPages` takes an order
    // rather than a count: a reader looking at a page that moved is still
    // looking at that page.
    check(
      names[3] ?? "",
      viewer.position.page === 0,
      `was reading slot ${readingAt}, now on ${viewer.position.page} (wanted 0)`,
    );
  }

  // What the layout carries by *identity* rather than by slot. A scroller that
  // re-indexed its learned sizes by position would give the moved page the size
  // of whatever used to be at the front. Only observable where the two pages are
  // different shapes, and `mixed.pdf` is the one corpus that is.
  //
  // **Both slots are asserted, and one of them would not be enough.** The other
  // way to lose a size is to lose them all --- the carry-forward returning
  // nothing rather than the wrong thing --- and then every page falls back to
  // the same estimate. A comparison against one measured shape cannot see that,
  // because the estimate is free to land within tolerance of it, and on this
  // corpus it does: the mutation that drops the carry-forward entirely left this
  // check green while reddening a deletion check that reads absolute boxes.
  // Comparing *both* closes it by arithmetic rather than by luck. If every page
  // shares one estimate, that estimate would have to be within 0.02 of two
  // shapes that `shapesDiffer` has just established are further apart than that,
  // so at least one half must fail whatever the estimate happens to be.
  //
  // The comparison is of shape rather than of size because fit-width rescales
  // every page when the widest one changes slot --- which is a trap of its own,
  // and here the moved page really does land at twice the width it was measured
  // at while being the same page.
  if (!shapesDiffer) {
    skip(
      names[4] ?? "",
      `pages 1 and ${target + 1} are the same shape, so a size that moved to the ` +
        "wrong page reads exactly like one that travelled",
    );
  } else {
    const landed = viewer.pageBoxCssOf(0);
    const displacedBox = viewer.pageBoxCssOf(1);
    check(
      names[4] ?? "",
      Math.abs(shape(landed) - shape(movedBox)) <= 0.02 &&
        Math.abs(shape(displacedBox) - shape(frontBox)) <= 0.02,
      `the page was ${movedBox.width.toFixed(0)}x${movedBox.height.toFixed(0)} ` +
        `and landed as ${landed.width.toFixed(0)}x${landed.height.toFixed(0)}; ` +
        `the page it displaced was ${frontBox.width.toFixed(0)}x${frontBox.height.toFixed(0)} ` +
        `and is now ${displacedBox.width.toFixed(0)}x${displacedBox.height.toFixed(0)}`,
    );
  }

  viewer.setPages(before);
  await frame();
  const restored = fingerprint(viewer.textOn(target));
  check(
    names[5] ?? "",
    (seen.status?.pageCount ?? -1) === before.length &&
      (!distinct || restored === movedText),
    `${seen.status?.pageCount ?? -1} pages, slot ${target} holds ` +
      (distinct ? `[${restored.slice(0, 40)}]` : "text nothing can tell apart"),
  );
  await settle(() => viewer.idle);

  await moveCommandChecks(doc, names);
}

/**
 * The `page_move` command itself, against the model in the backend.
 *
 * The same seam {@link deleteCommandChecks} covers, for the same reason: the
 * rest of the phase drives `Viewer.setPages` directly and says nothing about the
 * command a reader runs. Here the round trip is the subject --- the command is
 * registered, it takes two identities rather than a destination index, and the
 * order it answers with is the one asked for.
 *
 * The model is left as it was found, asserted rather than assumed.
 */
async function moveCommandChecks(
  doc: DocumentInfo,
  names: string[],
): Promise<void> {
  interface State {
    pages: { id: number; source: number; turns: number }[];
    can_undo: boolean;
  }

  const state = await invoke<State>("edit_state", { doc: doc.id }).catch(
    () => null,
  );
  const first = state?.pages[0];
  const last = state?.pages[state.pages.length - 1];
  if (!state || !first || !last || state.pages.length < 2) {
    for (const name of names.slice(6)) {
      skip(name, "the backend has no edit model with two pages to swap");
    }
    return;
  }

  const wanted = [...state.pages.slice(1), first].map((page) => page.id);
  const after = await invoke<State>("page_move", {
    doc: doc.id,
    page: first.id,
    after: last.id,
  }).catch((e: unknown) => String(e));
  if (typeof after === "string") {
    check(names[6] ?? "", false, `the command failed: ${preview(after)}`);
    for (const name of names.slice(7)) skip(name, "the move did not happen");
    return;
  }

  const got = after.pages.map((page) => page.id);
  check(
    names[6] ?? "",
    got.length === wanted.length && got.every((id, at) => id === wanted[at]),
    `ids ${preview(got.join(","))}, wanted ${preview(wanted.join(","))}`,
  );

  // A page cannot be its own anchor: that names no position, and answering it
  // with "nothing happened" would leave a reader's undo holding a command that
  // did nothing.
  const itself = await invoke<State>("page_move", {
    doc: doc.id,
    page: first.id,
    after: first.id,
  }).then(
    () => "",
    (e: unknown) => String(e),
  );
  check(
    names[7] ?? "",
    itself.includes("after itself"),
    itself ? preview(itself) : "it accepted a page as its own anchor",
  );

  const undone = await invoke<State>("edit_undo", { doc: doc.id }).catch(
    () => null,
  );
  const back = undone?.pages.map((page) => page.id) ?? [];
  const original = state.pages.map((page) => page.id);
  check(
    names[8] ?? "",
    back.length === original.length && back.every((id, at) => id === original[at]),
    `ids ${preview(back.join(","))}, wanted ${preview(original.join(","))}`,
  );
}

/**
 * The text layer's half of a page turn, on the page and on its neighbour.
 *
 * Split out so that the two text assertions skip together and for one stated
 * reason: half the corpus has no extractable text, and a text check on such a
 * document is not a failure, it is a question the document cannot answer.
 */
async function pageTurnTextChecks(
  viewer: Viewer,
  doc: DocumentInfo,
  target: number,
  other: number,
  otherBefore: { quarter_turns: number; width_pt: number } | null,
  names: string[],
): Promise<void> {
  const turned = names[6] ?? "";
  const untouched = names[7] ?? "";
  const shown = viewer.textOn(target);
  if (!shown || shown.codes.length === 0) {
    skip(turned, "the page has no extractable text");
    skip(untouched, "the page has no extractable text");
    return;
  }

  // From the backend, so the comparison is against the document rather than
  // against the same cache being checked.
  const raw = await invoke<{ quarter_turns: number; width_pt: number }>(
    "page_text",
    { doc: doc.id, page: target },
  ).catch(() => null);
  if (!raw) {
    skip(turned, "the page's text could not be fetched a second time");
    skip(untouched, "the page's text could not be fetched a second time");
    return;
  }

  const wanted = (raw.quarter_turns + viewer.pageExtraTurns(target)) % 4;
  check(
    turned,
    shown.quarter_turns === wanted && shown.width_pt !== raw.width_pt,
    `page is /Rotate ${raw.quarter_turns * 90}, turned ${viewer.pageExtraTurns(target) * 90}: ` +
      `the text layer reports /Rotate ${shown.quarter_turns * 90} ` +
      `(wanted ${wanted * 90}) on a ${shown.width_pt.toFixed(0)} pt wide page ` +
      `(the document says ${raw.width_pt.toFixed(0)})`,
  );

  const otherNow = viewer.textOn(other);
  if (!otherBefore || !otherNow) {
    skip(untouched, "the neighbouring page has no text layer to compare");
    return;
  }
  check(
    untouched,
    otherNow.quarter_turns === otherBefore.quarter_turns &&
      otherNow.width_pt === otherBefore.width_pt,
    `page 2 was /Rotate ${otherBefore.quarter_turns * 90} at ` +
      `${otherBefore.width_pt.toFixed(0)} pt wide, now /Rotate ` +
      `${otherNow.quarter_turns * 90} at ${otherNow.width_pt.toFixed(0)} pt`,
  );
}
