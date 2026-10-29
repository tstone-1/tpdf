/**
 * What the window check's command phase expects (`commands.ts`), as data both it
 * and `appcommands.test.ts` read. Until 2026-10-16 the lists lived inside the
 * check, which only the Ubuntu CI job runs, so a new command greyed until
 * something exists (`file.saveAttachment.choice`) reached `main` unlisted and
 * turned CI red. The unit test now holds the registry to the same lists in the
 * same state, so `vitest` finds it before a push.
 */

/** Registered commands withheld with one document open and nothing else: no selection, no search, no history, no attachment. In registry order. */
export function needsMoreThanADocument(linux: boolean): string[] {
  return [
    // One tab, so no other to close, and none closed earlier to reopen.
    "file.closeOtherTabs",
    "file.closeTabsLeft",
    "file.closeTabsRight",
    "file.reopenClosedTab",
    // No tabs from an earlier launch to reopen, and reopening all of them is
    // this fixture's launch preference, so its other half is not offered. Both
    // joined the registry with the tab restore and not this list; the first
    // run of this check after them, on 2026-10-04, found it.
    "file.reopenLastTabs",
    "file.reopenLastDocumentAtLaunch",
    // The fixture follows the system's language, so that choice is the
    // current one and is not offered.
    "app.language.system",
    // This fixture remembers no documents, so there is nothing to clear.
    "file.clearRecents",
    // This fixture has no other document to put beside the open one, and so
    // no two sides to switch or to go between. Declared with the commands.
    "view.sideBySide",
    "view.moveToOtherSide",
    "view.switchSides",
    "view.focusOtherSide",
    "view.syncScrolling",
    // This fixture's disk-change mode is `ask`, so that choice is the one not
    // offered. In registry order, as the whole list is.
    "file.onDiskChange.ask",
    // This fixture has automatic checking enabled, so only its disable command
    // is offered. Opening a document does not change that preference.
    "app.enableAutomaticUpdates",
    "app.installUpdate",
    // Needs an update already applied, which is a state a check run can never
    // be in: nothing here installs one. In registry order, immediately after
    // the install it follows.
    "app.restartForUpdate",
    // Needs a second open document to compare with, as the side-by-side
    // commands do. Declared with the command.
    "file.compareWith",
    "find.inSelection",
    // A document opens one page to a row and scrolling continuously, so those
    // two are the choices not offered, and right to left needs two pages to a
    // row.
    "view.singlePage",
    "view.continuous",
    "view.rightToLeft",
    // New text fields get a line round them here, so turning that on is not
    // offered. Found missing with the two above.
    "edit.fieldBorderOn",
    // The fixture's fake says the document has no form, and no field is being
    // changed.
    "edit.formEditOn",
    "edit.formEditOff",
    "edit.tabOrder",
    "edit.fieldProperties",
    "edit.duplicate",
    // Nothing is picked.
    "edit.alignLeft",
    "edit.alignCenter",
    "edit.alignRight",
    "edit.alignTop",
    "edit.alignMiddle",
    "edit.alignBottom",
    "edit.distributeAcross",
    "edit.distributeDown",
    "edit.sameWidth",
    "edit.sameHeight",
    "edit.sameSize",
    "edit.centerOnPage",
    "edit.middleOnPage",
    "edit.highlightSelection",
    "edit.underlineSelection",
    "edit.strikeoutSelection",
    "edit.squigglySelection",
    // The three a comment has to be OPEN for, guarded on the popup rather than
    // on a selection: a reader names the comment they mean by opening it, and a
    // document nobody has clicked into has none open. All three landed on
    // 2026-08-29 and 2026-08-30 without joining this list, which turned the
    // check red on a clean tree -- the third time in this file's history, and
    // the two comments below already say why it goes unread. **In registry
    // order**, which the check compares by `join`.
    "edit.editForeignMark",
    "edit.deleteComment",
    "edit.replyToComment",
    // Guarded on a note being open, which is how a reader names the mark they
    // mean. A document with no marks in it offers nothing to remove.
    //
    // **In registry order, which the check below compares by `join`.** The
    // three entries here were written in the order somebody thought of them and
    // put `edit.removeMark` last, where the registry has it first --- so the
    // check was red on a clean tree, and stayed red unread because the harness
    // that runs this baseline prints only its first three failures.
    "edit.removeMark",
    // Guarded as the removal is, on a mark being named, and declared with the
    // commands. Paste is not here: it is guarded on marks having been copied.
    "edit.copyMark",
    "edit.cutMark",
    // Joined on 2026-08-27 with the command itself, rather than after the
    // harness went red for it. Nothing runs this harness automatically -- it
    // needs a real window, so it is not a gate -- and the note above records
    // what that costs: a red that goes unread until somebody happens to look.
    // The declaration is part of adding a guarded command, not a repair.
    "edit.redactSelection",
    // Guarded on there being search matches, which a document nobody has
    // searched has none of. Declared with the command, as the one above was.
    "edit.redactMatches",
    // Guarded on a region being picked, which is how a reader names the region
    // they mean --- a document nobody has right-clicked a region on has none.
    // Declared with the command, as the two above were, rather than after the
    // harness went red for it.
    "edit.removeRedaction",
    // Guarded on a file waiting for its pages to be named, which only
    // `edit.insertPages` leaves behind. It shipped without its entry here and
    // the Windows run was the first to read the red, on both corpora.
    "edit.insertPages.range",
    // Guarded on the document being edited, which an untouched one is not. It
    // joined this list late: the guard landed with "Save over the file the
    // reader opened" and turned this check red, and the red went unread because
    // nothing runs this harness automatically --- it needs a real window, so it
    // is not a gate. The check itself behaved exactly as its comment above
    // promises; `file.saveCopy` is deliberately not here, because a copy of an
    // unedited document is a file a reader wants.
    "file.save",
    // Linux only (certificate files are added to tpdf there), so withheld
    // everywhere else, document or not; on Linux it needs none and is in
    // NEEDS_NO_DOCUMENT instead.
    ...(linux ? [] : ["file.addCertificateFile"]),
    // Guarded on the list of languages `file.recogniseTextLanguage` fetches.
    // Declared with the command, one place too early: the
    // registry lists it after `file.save`, and this list is compared in order.
    "file.recogniseTextLanguage.choice",
    // Guarded on the document carrying an attached file, and the corpora carry
    // none. Missing here from 2026-10-16, when it was registered, until the
    // Ubuntu job's run of this check the same day.
    "file.saveAttachment.choice",
    // Guarded on the jump history rather than on a document, which is why the
    // phase clears it below: both are correct to offer after a jump, and this
    // check is about what an open document alone earns. `nav.forward` joined
    // this list on 2026-08-23 with the guard that greys them, and `nav.back`
    // joined at the same moment for a reason the red line did not show --- it
    // was *offered* on this corpus, because the search phase had jumped to a
    // hit before this phase ran, and would have been withheld on a corpus with
    // no text for the search phase to find. A check whose expected answer
    // depends on which fixture it is looking at is one that fails somewhere
    // else later.
    "nav.back",
    "nav.forward",
    // Nothing has been compared, so there are no changes to step through.
    // Declared with the commands.
    "nav.nextChange",
    "nav.previousChange",
  ];
}

/** The commands offered with no document at all. In registry order. */
export function needsNoDocument(linux: boolean): string[] {
  return [
    // A preference, registered with the tab commands, which come first.
    "file.reopenTabsAtLaunch",
    // The interface languages that are not the fixture's choice, which is
    // the system's. Registered after the launch pair.
    "app.language.en-US",
    "app.language.en-GB",
    "app.language.de-DE",
    "app.language.de-DE-formal",
    "file.open",
    // Makes a document, so it needs none. Missing here from the day it was
    // registered until this check next ran, 2026-10-04.
    "file.fromPictures",
    // A preference rather than something done to a document; `ask` is the
    // fixture's mode and so is not among them.
    "file.onDiskChange.reload",
    "file.onDiskChange.ignore",
    "app.about",
    "app.checkForUpdates",
    "app.disableAutomaticUpdates",
    "app.installCommandLineTool",
    "app.uninstallCommandLineTool",
    "app.makeDefaultPdfApp",
    "app.enableExplorerIntegration",
    "app.disableExplorerIntegration",
    "edit.fieldBorderOff",
    // Linux only, and a document is not involved: the file is remembered for
    // every signing after. Missing here from the day it was registered until
    // the first Linux run of this check, 2026-10-14.
    ...(linux ? ["file.addCertificateFile"] : []),
    // A preference, set before or after the first scan is opened. Missing here
    // from 2026-10-05, when it was registered, until this check next ran.
    "file.recogniseTextLanguage",
  ];
}
