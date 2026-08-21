#!/usr/bin/env python3
"""Breaks a Rust module on purpose, one edit at a time.

The counterpart to `mutate_frontend.py`, and it exists for the same reason: a
test that has only ever passed looks exactly like one that cannot fail. Each
mutation below names the test it is *expected* to turn red, and a mutation that
nothing caught is reported as a defect in the suite rather than shrugged at.

`search.rs` is the module it covers first. It is the densest piece of pure logic
in the backend --- a fold, an index map back through it, and two options that
each change what is accepted --- and every one of its assertions is over a
fixture the module itself never wrote, which is what makes mutation the only
thing that can say whether they bite.

`text.rs`, `structure.rs`, `encoding.rs`, `docmodel.rs`, `annots.rs`, `links.rs`,
`progressive.rs`, `edits.rs` and `save.rs` are covered too, and `FILTERS` below is
the list of record. `encoding.rs` is the one to be careful about, because its
tests are the *only* thing that can catch its central mutation: `encodings.pdf`
has `/Encoding` and `/Ordering` covarying on every page, so a rule keyed on the
wrong one of the two passes every fixture on disk. A harness without those
mutations would look thorough and prove nothing about the field that decides.

Two properties carried over from the front-end harness, both because
`docs/TRAPS.md` records what their absence costs:

**It cross-checks.** Every run derives the failure count two ways -- by counting
libtest's per-test `FAILED` lines and by reading its summary line -- and a
disagreement is a broken run rather than either answer.

**A run that produced no summary is not a pass.** A compile error from a bad
mutation produces no failing-test lines, which is exactly what a surviving
mutation looks like. It is the likeliest outcome here and the one that would
otherwise read as good news.

**It builds into its own target directory** (`src-tauri/target/mutations`) and
**runs only the test each mutation names**, falling back to the whole suite when
that test does not go red. Both are about cost rather than coverage, and the
whole table went from 4.4 hours to 405 s on 2026-08-21 --- `docs/TRAPS.md`,
"A harness that edits source files pays for the editor watching them", has the
measurements and why the fallback is the part that keeps it honest.

Usage:
    scripts/mutate_rust.py            # every mutation
    scripts/mutate_rust.py --list
    scripts/mutate_rust.py --only save
    scripts/mutate_rust.py --since HEAD~3   # only what the diff touched
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_output import stream_results  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / "src-tauri"

#: This harness builds into its own target directory, and it is not tidiness.
#:
#: Every mutation writes a file under `src-tauri/src`, and an editor with
#: rust-analyzer open answers each of those writes with
#: `cargo check --workspace --all-targets`. That check takes the build
#: directory's lock, so the mutation's own `cargo test` queues behind a
#: whole-workspace re-check --- measured on 2026-08-21 with cargo saying so in as
#: many words, `Blocking waiting for file lock on build directory`, and a
#: no-op `cargo test --lib --no-run` taking 28.2 s where it takes 0.2 s alone.
#: Multiply that by the table and the run is hours longer than the work in it.
#:
#: Inside `target/` so the existing ignore rule covers it, and persistent so the
#: cost is one cold build ever rather than one per run. It also stops the editor
#: from reporting diagnostics against a tree that is mutated at the time.
MUT_TARGET = CRATE / "target" / "mutations"
CARGO_ENV = {**os.environ, "CARGO_TARGET_DIR": str(MUT_TARGET)}

#: Which platform this is, in the vocabulary `Mutation.only_on` uses.
HERE = "macos" if sys.platform == "darwin" else "windows" if sys.platform == "win32" else "linux"

#: Only these modules' tests are run, so an unrelated failure elsewhere cannot be
#: read as a mutation being caught. libtest takes several filters and ORs them,
#: but only after `--`: `cargo test --lib a:: b::` is cargo's own argument error,
#: which is worth knowing because it looks like the feature being unsupported.
FILTERS = [
    "search::",
    "structure::",
    "text::",
    "encoding::",
    "docmodel::",
    "annots::",
    "links::",
    "progressive::",
    "edits::",
    "save::",
    # Added 2026-08-16 with the shared-page fix. `print.rs` had been mutated by
    # nothing, so its module was never in this list --- and the three mutations
    # written for that fix named tests the harness could not see, because this
    # list is what `--list` is filtered by. It refused to start and said which
    # three, which is that guard doing its job rather than a bug in it.
    "print::",
    # Added 2026-08-17 with the page-tree module. Its own tests were invisible to
    # the harness for the same reason `print::` was: this list is what selects
    # them, and a mutation whose test cannot be seen reports SURVIVED.
    "pagetree::",
    # Added 2026-08-18 with the crop. Fourth time this list has been forgotten
    # and fourth time the guard is what said so, before six mutations could
    # report SURVIVED for tests it simply could not see.
    "content::",
    # Added 2026-08-17 with the menu bar. The guard fired a third time and said
    # exactly which mutation it was, before the mutation could report SURVIVED.
    #
    # `menu.rs` is macOS-only, so on Windows this filter matches nothing and the
    # mutation naming its test is refused rather than run. That is the right
    # answer there --- the code under it does not exist on that platform --- and
    # it is worth knowing before the refusal reads as a broken harness.
    "menu::",
    # Added 2026-08-17 with the keyboard-layout lookup, macOS-only for the
    # same reason `menu::` is: Carbon does not exist on Windows.
    "keylayout::",
    # Added 2026-08-19 with the version display. `lib.rs`'s own root test module
    # had never been selected by anything here, so the test asserting the four
    # version files agree could not go red -- and the guard said so before the
    # mutation aimed at it could report SURVIVED. Fifth time; the list is the
    # thing this harness most reliably forgets, and the guard is the thing that
    # most reliably catches it.
    "tests::",
    # Added 2026-08-19 with the external-modification check. Sixth time, and this
    # one was found by looking rather than by the guard: no mutation named a
    # `fingerprint::` test, so nothing refused to start --- a module whose tests
    # are invisible AND unaimed-at is silent in both directions.
    "fingerprint::",
    # Added 2026-08-19 with the Jump List. Written at the same time as the
    # mutations rather than after them, which is the whole lesson of the five
    # times this list was forgotten and the sixth time it was not even noticed.
    "recentdocs::",
]


@dataclass(frozen=True)
class Mutation:
    """One edit, and the test whose job it is to notice."""

    name: str
    path: str
    before: str
    after: str
    expect: str
    #: The one platform this mutation can run on, or None for all of them.
    #:
    #: `menu.rs` and `keylayout.rs` are macOS-only, so on Windows `cargo test`
    #: never compiles them and the tests they name do not exist. The guard below
    #: reports that correctly -- "it cannot go red, so this mutation would report
    #: SURVIVED" -- and then refuses the whole run over it, which is the right
    #: answer for an unknown name and the wrong one for an absent platform. It
    #: blocked every mutation in this table on Windows from the day `menu::` was
    #: added until 2026-08-19, and nothing said so, because the whole table had
    #: not been run there since.
    #:
    #: **Declared, never inferred.** A mutation with no `only_on` still refuses
    #: the run when its test cannot be found -- which is the case the guard was
    #: written for and the one that must stay loud. Skipping on a name the
    #: harness merely failed to see is how a mutation quietly stops being able
    #: to fail.
    only_on: str | None = None


MUTATIONS = [
    Mutation(
        # Remove the CALL, not the guard. `fingerprint.rs`'s own tests prove the
        # comparison works and say nothing about whether anything asks it --- the
        # trap is "a guard is only covered when a mutation removes the call", and
        # this is that mutation.
        "save: stop asking whether the file changed under the document",
        "src/save.rs",
        "            Some(opened_as.agrees_with(source).map_err(Refusal::changed)?)",
        "            Fingerprint::of(source).ok()",
        "a_save_in_place_is_refused_when_the_file_changed_under_it",
    ),
    Mutation(
        # Fail open instead of closed: treat "we could not look" as permission.
        # The reader's file is what is at stake, and the fallback the message
        # names has to stay reachable, which is a second test's job.
        "save: let a save in place proceed with no fingerprint at all",
        "src/save.rs",
        "    if plan.opened_as.is_none() {",
        "    if false {",
        "a_save_in_place_with_no_fingerprint_is_refused_and_points_at_save_a_copy",
    ),
    Mutation(
        # Refuse the copy too, which is what it did until 2026-08-19 and which
        # closed the only door the in-place refusal points at. The reader is told
        # to save their edits under another name and Save a copy is refused by
        # the same guard one function down.
        "save: refuse a copy from a source that changed, stranding the reader",
        "src/save.rs",
        "    let copy = planned_bytes(source, plan, OnChange::Proceed)?;",
        "    let copy = planned_bytes(source, plan, OnChange::Refuse)?;",
        "a_copy_is_written_when_the_source_changed_and_reports_it",
    ),
    Mutation(
        # And the other direction, which is the dangerous one: let a save in
        # place proceed over a file that changed, because a copy may. The
        # asymmetry is the whole design and a single word carries it.
        "save: let a save in place tolerate a changed file, as a copy does",
        "src/save.rs",
        "    let planned = planned_bytes(source, plan, OnChange::Refuse)?;",
        "    let planned = planned_bytes(source, plan, OnChange::Proceed)?;",
        "a_save_in_place_is_refused_when_the_file_changed_under_it",
    ),
    Mutation(
        # Report every copy as unchanged, so a copy built from a document the
        # reader is not looking at reads as one that is. The file is written
        # either way, which is what makes this invisible without the flag.
        "save: report a copy from a changed source as unchanged",
        "src/save.rs",
        "                changed = true;",
        "                changed = false;",
        "a_copy_is_written_when_the_source_changed_and_reports_it",
    ),
    Mutation(
        # Flatten `/InkList` into one path. The end of each stroke is then joined
        # to the start of the next by a line the reader never drew, which looks
        # like a drawing rather than like a defect --- and `--mode strokes`
        # measures the same thing in pixels by asserting the band between two
        # strokes is empty.
        "save: draw every stroke as one path rather than one each",
        "src/save.rs",
        "            for stroke in strokes {",
        "            for stroke in strokes.iter().take(1) {",
        "each_stroke_is_its_own_path_in_the_appearance_stream",
    ),
    Mutation(
        # Mitre the joins, which is the default. A hand-drawn corner turns at
        # whatever angle the hand made, and a mitre on a sharp one spikes out to
        # a point that reads as a rendering fault.
        "save: leave a drawing's joins mitred, as a box's are",
        "src/save.rs",
        '        Paint::Path => (INK_WIDTH, "1 J 1 j "),',
        '        Paint::Path => (INK_WIDTH, ""),',
        "each_stroke_is_its_own_path_in_the_appearance_stream",
    ),
    Mutation(
        # Write `/InkList` on every kind. The appearance stream is unchanged, so
        # every pixel check still passes and every reader still draws the right
        # thing --- what breaks is the file's own account of what it holds.
        "save: write an /InkList on kinds that have no strokes",
        "src/save.rs",
        "    if paint(mark.kind) == Paint::Path {\n        dictionary.set(\n            \"InkList\",",
        "    if true {\n        dictionary.set(\n            \"InkList\",",
        "the_ink_list_is_written_for_ink_and_for_nothing_else",
    ),
    Mutation(
        # Accept a mark whose kind and shape disagree. Nothing a reader can do
        # reaches this, so the only thing that can catch it is the test named
        # for it -- which is the case a rule with no failing case is in.
        "docmodel: let a kind and a shape disagree",
        "src/docmodel.rs",
        "        if mark.strokes.is_empty() != (mark.kind != MarkKind::Ink) {",
        "        if false {",
        "a_kind_and_a_shape_that_disagree_are_refused_both_ways_round",
    ),
    Mutation(
        # Ask `covers_area` of ink too. Its rectangle is padded by half a line
        # width, so a stroke that never moved still covers area and the mark is
        # accepted -- invisible on the page and unfindable in the panel, which is
        # an unsaved change the reader cannot see.
        "docmodel: judge ink empty by its rectangle rather than by its length",
        "src/docmodel.rs",
        "        let empty = if mark.kind == MarkKind::Ink {\n            !mark.strokes.iter().any(Stroke::is_drawable)",
        "        let empty = if false {\n            !mark.strokes.iter().any(Stroke::is_drawable)",
        "ink_that_never_moved_is_refused_though_its_rectangle_covers_area",
    ),
    Mutation(
        # Stop padding the bounds. A straight vertical stroke then has a
        # rectangle of no width, which `covers_area` refuses -- so ruling a line
        # down a margin is answered with "that mark covers nothing".
        "edits: take ink's rectangle tight against the strokes",
        "src/edits.rs",
        "            quads = Stroke::bounds(&strokes, (crate::save::INK_WIDTH / 2.0) as f32)",
        "            quads = Stroke::bounds(&strokes, 0.0)",
        # **Not the `docmodel` test that looks like the right one.** That one
        # builds its `Mark` by hand, so it exercises `Stroke::bounds` and says
        # nothing about whether anything calls it -- and this mutation reported
        # SURVIVED against it, which is the harness finding a real gap rather
        # than a variant. The test that reaches the derivation is on this side.
        "a_drawings_rectangle_is_derived_here_and_padded_by_half_a_line",
    ),
    Mutation(
        # Hand the platform the path as it arrived. Windows files a relative one
        # against the *shell's* current directory and AppKit resolves it against
        # the *process's* --- two different wrong files from one mistake, which
        # is why `resolved` is one function rather than two, and why this is one
        # mutation that runs on both platforms rather than a pair that can drift.
        "recentdocs: file the path as given rather than resolving it",
        "src/recentdocs.rs",
        "    std::fs::canonicalize(path).ok()",
        "    Some(path.to_path_buf())",
        "a_relative_path_is_made_absolute_rather_than_passed_through",
    ),
    Mutation(
        # Leave the verbatim prefix on. The shell does not understand `\\?\`, so
        # the entry shows the prefix in its label and does not open when clicked
        # --- which looks like the feature working, from a distance.
        "recentdocs: leave the verbatim prefix on the path the shell is given",
        "src/recentdocs.rs",
        '    let text = text.strip_prefix(r"\\\\?\\").unwrap_or(&text);',
        "    let text = &text[..];",
        "a_path_the_shell_is_given_is_absolute_nul_terminated_and_not_verbatim",
        only_on="windows",
    ),
    Mutation(
        # Drop the terminator. `SHARD_PATHW` says the pointer is a NUL-terminated
        # wide string, so without one the shell reads past the buffer -- which is
        # a garbage entry on a good day.
        "recentdocs: hand the shell a string with no terminator",
        "src/recentdocs.rs",
        "            .chain(std::iter::once(0))",
        "            .chain(std::iter::empty())",
        "a_path_the_shell_is_given_is_absolute_nul_terminated_and_not_verbatim",
        only_on="windows",
    ),
    Mutation(
        # File a document that is not there. The reader gets an entry that opens
        # nothing, which is worse than the absence this module fixes.
        "recentdocs: file a path that could not be resolved",
        "src/recentdocs.rs",
        "    std::fs::canonicalize(path).ok()",
        "    Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))",
        "a_file_that_is_not_there_is_not_filed",
    ),
    Mutation(
        # Build a *string* URL rather than a *file* URL. `URLWithString:` parses
        # its argument as a URL, so an ASCII path comes back with no scheme at
        # all and `isFileURL` is false --- and a path with a space in it comes
        # back nil outright, which is the ordinary case rather than an edge.
        # This is the constructor that looks right, and the test's own fixture
        # is what decides whether it can be told apart.
        "recentdocs: hand AppKit a string URL rather than a file URL",
        "src/recentdocs.rs",
        "    objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(text))",
        '    objc2_foundation::NSURL::URLWithString(&objc2_foundation::NSString::from_str(text))\n        .expect("url")',
        "a_url_the_menu_is_given_is_an_absolute_file_url",
        only_on="macos",
    ),
    Mutation(
        # Collapse the split back: put the way-out sentence into the bare fact,
        # which is where it lived until 2026-08-19. The pre-rename refusal then
        # tells a reader their edits are still here and to save them under
        # another name, two clauses before telling them the document is closed.
        "fingerprint: give the bare fact the advice that belongs to the deep check",
        "src/fingerprint.rs",
        '        "{} changed on disk since you opened it --- {how}",',
        '        "{} changed on disk since you opened it --- {how}. Your edits are still here: save them under another name.",',
        "the_last_look_before_the_rename_refuses_a_source_that_moved",
    ),
    Mutation(
        # And the other direction: drop the advice from the deep check, leaving a
        # reader stopped mid-save with a diagnosis and nowhere to go.
        "fingerprint: refuse the deep check with no way out",
        "src/fingerprint.rs",
        '            .map_err(|fact| format!("{fact}. {WAY_OUT}"))',
        "            .map_err(|fact| fact)",
        "the_deep_check_tells_the_reader_what_they_can_still_do",
    ),
    Mutation(
        # Let the last look pass whatever it sees. This is the guard covering the
        # window the staging split opens -- between the deep check and the rename
        # the document is read, rewritten and closed, and nothing else is looking.
        "save: let the last look before the rename agree with anything",
        "src/save.rs",
        "    if let Err(why) = staged.verified.agrees_shallowly(source) {",
        "    if let Err(why) = Ok::<(), String>(()) {",
        "the_last_look_before_the_rename_refuses_a_source_that_moved",
    ),
    Mutation(
        # Refuse, and leave the staged file behind. Nothing else is tracking it by
        # then, so it stays beside the reader's document under a name they never
        # chose and never sees a reader again.
        "save: leave the staged file behind when the last look refuses",
        "src/save.rs",
        "        let _ = std::fs::remove_file(&staged.path);",
        "        let _ = &staged.path;",
        "the_last_look_before_the_rename_refuses_a_source_that_moved",
    ),
    Mutation(
        # The comparison the whole module exists for. It was proved by nothing
        # until 2026-08-19: both tests named for the digest passed with this line
        # deleted, because `agrees_with` called `agrees_shallowly` first and the
        # mtime branch produced a message they could not tell apart. The fix was
        # to stop deferring to the timestamp, not to write a cleverer assertion.
        "fingerprint: stop comparing the digest",
        "src/fingerprint.rs",
        "        if now.digest != self.digest {",
        "        if false {",
        "a_rewrite_of_the_same_length_is_caught_by_the_digest_and_not_by_the_length",
    ),
    Mutation(
        # Hash the first chunk and stop. A digest of the first 64 KiB agrees with
        # itself perfectly and is blind to every byte after it, which on a PDF is
        # nearly the whole document -- the header and the first objects rarely
        # move.
        "fingerprint: hash only the first chunk of the file",
        "src/fingerprint.rs",
        "            hasher.update(&buffer[..read]);",
        "            hasher.update(&buffer[..read]);\n            break;",
        "a_file_larger_than_one_chunk_hashes_every_chunk",
    ),
    Mutation(
        # Put the deference back: make the deep check consult the timestamp it
        # deliberately ignores. This is the false refusal, not a missing one --
        # the file is byte-for-byte what the reader opened and the save is
        # refused anyway, because something touched it.
        "fingerprint: let the deep check defer to the modification time",
        "src/fingerprint.rs",
        "        self.len_agrees(path)?;",
        "        self.agrees_shallowly(path)?;",
        "a_file_whose_timestamp_moved_but_whose_bytes_did_not_still_saves",
    ),
    Mutation(
        # The cheap half. A length that changed is conclusive and costs no read,
        # and it is the only thing standing between a 337 MB file and a full hash
        # on the common failure.
        "fingerprint: stop comparing the length",
        "src/fingerprint.rs",
        "        if meta.len() != self.len {",
        "        if false {",
        "a_file_that_grew_is_refused_without_reading_it",
    ),
    Mutation(
        # The shallow check's only real content. Removing it leaves a guard that
        # compares a length it was already given and calls that a check -- and
        # this is the guard standing between staging and the rename, where
        # nothing else is looking.
        "fingerprint: forget the modification time in the shallow check",
        "src/fingerprint.rs",
        "        if now.is_some() && self.modified_ns.is_some() && now != self.modified_ns {",
        "        if false {",
        "a_file_whose_timestamp_moved_but_whose_bytes_did_not_still_saves",
    ),
    Mutation(
        # Report a version of our own rather than the crate's. This is the whole
        # failure mode the version display can have: a confident wrong answer,
        # which is worse than the nothing it replaced, because a reader acts on
        # it. Nothing else in the build compares these files -- `BUILD.md` lists
        # the four and lists them only.
        "version: report a hardcoded version rather than the crate's",
        "src/lib.rs",
        '    env!("CARGO_PKG_VERSION")',
        '    "26.0.0"',
        "the_version_files_agree_with_the_crate",
    ),
    Mutation(
        # Ask the layout for an action it does not define. A status other than
        # zero is the only thing standing between a garbage buffer and a label,
        # and the buffer is uninitialised on that path.
        "keylayout: ask the layout for an action it does not define",
        "src/keylayout.rs",
        "const ACTION_DISPLAY: u16 = 3;",
        "const ACTION_DISPLAY: u16 = 300;",
        "every_position_answers_with_a_single_visible_glyph",
        only_on="macos",
    ),
    Mutation(
        # Rename the tag the frontend writes. Nothing in either language reads
        # the other's spelling, so this fails at runtime as a menu bar that does
        # not appear -- inside a command whose only visible effect is its
        # absence, on the one platform that has a menu bar at all.
        "menu: rename the wire tag the frontend sends",
        "src/menu.rs",
        '#[serde(tag = "kind", rename_all = "lowercase")]',
        '#[serde(tag = "type", rename_all = "lowercase")]',
        "a_separator_and_a_command_are_told_apart_by_their_tag",
        only_on="macos",
    ),
    Mutation(
        # Report the selection's length as the file's. `write_copy` compares the
        # baseline against the source's real page count to catch a file that
        # changed under the open one, so this turns every extract of a subset
        # into that error -- or, worse, makes a genuine external modification
        # invisible when the numbers happen to agree.
        "edits: give a subset plan the selection's length as its baseline",
        "src/edits.rs",
        "            baseline: model.baseline(),\n            opened_as: opened_as.clone(),\n            pages,\n            marks,",
        "            baseline: pages.len() as u32,\n            opened_as: opened_as.clone(),\n            pages,\n            marks,",
        "a_subset_plan_names_the_pages_asked_for_and_keeps_the_file_s_baseline",
    ),
    Mutation(
        # Silently sort instead of refusing. Extract would then reorder as well
        # as select, which is the one thing its own note says it must not do --
        # and `5,1` would produce a document no reader could have predicted.
        "edits: accept slots out of order instead of refusing them",
        "src/edits.rs",
        '        if taken.windows(2).any(|pair| pair[0] >= pair[1]) {\n            return Err("the pages are not in document order".into());\n        }',
        "        taken.sort_unstable();",
        "slots_out_of_order_are_refused_rather_than_silently_reordering",
    ),
    Mutation(
        # Let an empty selection through to the writer. `write_copy` refuses an
        # empty plan too, so the document is safe either way -- what is lost is
        # the message, which stops describing what the reader typed.
        "edits: let an empty selection reach the writer",
        "src/edits.rs",
        '        if slots.is_empty() {\n            return Err("no pages were named".into());\n        }',
        "        if false {\n            return Err(String::new());\n        }",
        "an_empty_selection_is_refused_here_rather_than_by_the_writer",
    ),
    Mutation(
        # The composition. Setting the turn rather than adding it produces a
        # document whose every turned page ends at the same angle, which is
        # correct on the whole corpus except the one fixture that carries four
        # different rotations.
        "pagetree: set the rotation instead of composing onto the page's own",
        "src/pagetree.rs",
        "        let composed = (effective_rotation(doc, *id) + extra * 90).rem_euclid(360);",
        "        let composed = (extra * 90).rem_euclid(360);",
        "a_turn_composes_with_the_rotation_the_page_already_had",
    ),
    Mutation(
        # Turn a shared page once per page NUMBER. One implementation now serves
        # the save and the print path, so this reddens a test in each --- it is
        # credited to the print one because that defect was live in shipped code,
        # and `a_page_reached_twice_is_turned_once` covers the save side.
        "pagetree: turn a shared page once per page number rather than once per object",
        "src/pagetree.rs",
        "    Ok(order.into_iter().map(|id| (id, chosen[&id].0)).collect())",
        "    Ok(plan.to_vec())",
        "a_page_named_twice_is_turned_once",
    ),
    Mutation(
        # Delete a page object that a KEPT page number also names. The damaging
        # member of the family: printing "page 1" produces a blank sheet.
        "pagetree: drop a page object that a kept page number also names",
        "src/pagetree.rs",
        "        .filter(|id| !kept.contains(id))",
        "        .filter(|id| kept.contains(id) || !kept.contains(id))",
        "a_page_a_kept_number_also_names_is_not_dropped",
    ),
    Mutation(
        # Decrement `/Count` once per doomed OBJECT rather than once per page
        # NUMBER, leaving a tree that claims a page it does not have.
        "pagetree: charge a shared page's deletion to /Count once, not once per number",
        "src/pagetree.rs",
        "    for number in numbers {",
        "    for number in numbers.iter().take(1) {",
        "a_shared_page_costs_the_tree_one_count_per_number_it_answered_to",
    ),
    Mutation(
        # Compose each page's turn without reconciling the objects, which is the
        # loop that was there before `agreed_turns` existed: a shared page's
        # second visit reads the 90 the first wrote and leaves 180.
        "save: turn a shared page once per page number rather than once per object",
        "src/save.rs",
        "    apply_turns(&mut doc, &agreed_turns(&turns)?)?;",
        "    apply_turns(&mut doc, &turns)?;",
        "a_page_reached_twice_is_turned_once",
    ),
    Mutation(
        # Accept a plan that asks one shared page for two different turns, and
        # silently apply whichever came first.
        "save: accept two different turns for one shared page",
        "src/pagetree.rs",
        "            Some(&(first, first_at)) if first != extra => {",
        "            Some(&(first, first_at)) if first != extra && false => {",
        "a_page_reached_twice_cannot_be_turned_two_ways",
    ),
    Mutation(
        # The over-refusal direction: refuse a shared page even when nothing
        # conflicts, which would deny saving an unedited document whose page tree
        # happens to be malformed.
        "save: refuse a shared page even when its turns agree",
        "src/pagetree.rs",
        "            Some(&(first, first_at)) if first != extra => {",
        "            Some(&(first, first_at)) if first == extra || first != extra => {",
        "a_page_reached_twice_is_saved_normally_when_nothing_conflicts",
    ),
    Mutation(
        # Write a rotation onto a page nobody turned. Invisible on every fixture
        # whose pages state their own rotation; on one that inherits it, the value
        # written is whatever `effective_rotation` answered, which is 0 whenever
        # its 64-hop walk gives up. It survived until the test stopped asserting
        # the effective rotation --- which is 90 either way --- and started
        # asserting that the untouched page states no `/Rotate` of its own.
        "pagetree: write a rotation onto every page, turned or not",
        "src/pagetree.rs",
        "        if extra == 0 {",
        "        if false {",
        "a_page_that_was_not_turned_keeps_an_inherited_rotation",
    ),
    Mutation(
        # Reparent every page and take nothing with it. `/MediaBox`, `/Rotate`,
        # `/Resources` and `/CropBox` are inheritable, so a page that hung under
        # a tree node stating one loses it the moment it hangs off the root ---
        # producing a document that opens, looks plausible, and has a page at
        # the wrong size or the wrong angle.
        "pagetree: reorder without carrying what a page inherited",
        "src/pagetree.rs",
        "                    pushes.push((page, key, value));",
        "                    let _ = value;",
        "a_reordered_page_takes_what_it_inherited_with_it",
    ),
    Mutation(
        # The other direction: write every inherited attribute onto every page.
        # It costs pages nobody moved their byte-for-byte identity, and for
        # `/Rotate` it is the flattening `apply_turns` deliberately avoids --- a
        # page whose `/Parent` chain outruns the bound reads back as upright.
        "pagetree: push an inherited value down even when the root already supplies it",
        "src/pagetree.rs",
        "                Some(value) if Some(&value) != from_root[at].as_ref() => {",
        "                Some(value) => {",
        "a_page_that_inherits_from_the_root_is_not_written_to",
    ),
    Mutation(
        # Leave `/Count` at whatever the file declared. The tree then claims
        # pages a reader walking `/Kids` will not find, which is a document
        # every parser is entitled to reject.
        "pagetree: leave the rebuilt tree claiming the page count the file had",
        "src/pagetree.rs",
        "    tree.set(\"Count\", order.len() as i64);",
        "",
        "the_flattened_tree_counts_what_it_holds_and_owns_every_page",
    ),
    Mutation(
        # Rebuild `/Kids` and leave every page pointing at the node it used to
        # hang under. The document reads correctly --- `get_pages` walks down
        # from the root --- and every walk *up* a `/Parent` chain, this module's
        # included, resolves against an ancestry that is no longer there.
        "pagetree: rebuild the tree without telling the pages who their parent is",
        "src/pagetree.rs",
        "            .set(\"Parent\", Object::Reference(root));",
        "            .set(\"NotParent\", Object::Reference(root));",
        "the_flattened_tree_counts_what_it_holds_and_owns_every_page",
    ),
    Mutation(
        # Rebuild the tree for every save. The document is identical either way,
        # so nothing a reader sees can tell --- and every page of every copy has
        # been reparented for a plan that moved nothing.
        "save: rebuild the page tree even when nothing moved",
        "src/save.rs",
        "    let moved = plan\n        .pages\n        .windows(2)\n        .any(|two| two[0].source >= two[1].source);",
        "    let moved = true;",
        "a_plan_in_document_order_leaves_the_page_tree_as_it_found_it",
    ),
    Mutation(
        # The print half of the same defect, and it was live in shipped code as
        # a documented property: a subset came out in document order whatever
        # order it was asked for.
        "print: hand the printer the pages in the file's order rather than the job's",
        "src/print.rs",
        "    if wanted.windows(2).any(|two| two[0].number >= two[1].number) {",
        "    if false {",
        "a_job_prints_its_pages_in_the_order_it_lists_them",
    ),
    Mutation(
        # And its over-application, which no page a reader sees can distinguish.
        "print: rebuild the page tree for a job whose pages never moved",
        "src/print.rs",
        "    if wanted.windows(2).any(|two| two[0].number >= two[1].number) {",
        "    if true {",
        "a_job_in_document_order_keeps_the_page_tree_the_file_had",
    ),
    Mutation(
        # Drop the anchor and put every moved page at the front. Half of all
        # moves then land where they were asked to, which is the shape that
        # survives a test suite reading only the page count.
        "edits: move every page to the front whatever anchor was named",
        "src/edits.rs",
        "                after: after.map(PageId::from_raw),",
        "                after: None,",
        "a_moved_page_lands_behind_the_page_it_named_and_keeps_its_identity",
    ),
    Mutation(
        # Call a plan the file on disk whenever it has every page unturned,
        # which a reordered document does. The print path then hands the file
        # over byte for byte and the reader's rearrangement never reaches paper.
        "edits: read a plan's length and turns as meaning it is the file on disk",
        "src/edits.rs",
        "                .all(|(at, page)| page.source as usize == at && page.turns % 4 == 0)",
        "                .all(|(_at, page)| page.turns % 4 == 0)",
        "a_plan_after_a_move_is_out_of_document_order",
    ),
    Mutation(
        # Keep every page whatever the plan says. The copy then comes out with
        # the page the reader deleted still in it, which is the whole feature
        # silently doing nothing.
        "save: keep a page the plan does not name",
        "src/save.rs",
        "            .filter(|number| !kept.contains(number))",
        "            .filter(|_number| false)",
        "a_third_parser_sees_the_pages_that_were_kept_and_not_the_one_that_was_not",
    ),
    Mutation(
        # Resolve each plan entry one page late. Every turn then lands on the
        # page after the one it was aimed at --- which on an unedited document is
        # invisible, because every turn is zero.
        "save: aim each turn at the page after the one the plan named",
        "src/save.rs",
        "        .filter_map(|page| Some((*pages.get(page.source as usize)?, page.turns)))",
        "        .filter_map(|page| Some((*pages.get(page.source as usize + 1)?, page.turns)))",
        "a_turn_on_a_page_after_the_deleted_one_lands_where_it_was_aimed",
    ),
    Mutation(
        # Let a shared page be half-deleted. `drop_pages` keeps any object a
        # surviving number names, so the deletion silently does nothing and the
        # copy has the page in it.
        "save: accept a deletion that removes one of two numbers naming one page",
        "src/save.rs",
        "        unshared(&pages, &kept, &dropped)?;",
        "        let _ = unshared(&pages, &kept, &dropped);",
        "deleting_one_of_two_numbers_that_are_one_page_is_refused",
    ),
    Mutation(
        # The over-refusal direction of the same guard: refuse a deletion whose
        # doomed page nothing else names, which is every ordinary deletion.
        "save: refuse a deletion that no kept page shares",
        "src/save.rs",
        "        let Some(shared) = kept.iter().find(|keep| at(keep) == Some(id)) else {",
        "        let Some(shared) = kept.iter().find(|_keep| true) else {",
        "deleting_both_numbers_of_a_shared_page_is_not_refused",
    ),
    Mutation(
        # Write a reordered plan in the order the file already has. The pages
        # come out in the wrong order, in a file that opens and prints.
        #
        # It named the refusal this replaced --- and when that test went, the
        # harness refused to start rather than reporting the mutation survived.
        # Same edit, same defect, and now a test that asserts the pages came out
        # where the reader put them instead of that the save declined to try.
        "save: write a reordered plan in file order rather than in the reader's",
        "src/save.rs",
        "        .any(|two| two[0].source >= two[1].source)",
        "        .any(|_two| false)",
        "a_plan_whose_pages_have_moved_comes_out_in_the_order_the_reader_put_them",
    ),
    Mutation(
        # Keep the outline after pages have gone. Its destinations name objects
        # that are not there, and `drop_pages` has already emptied the arrays
        # they lived in --- so what survives is a malformed destination rather
        # than a dead one.
        "save: keep the outline of a document that lost pages",
        "src/save.rs",
        "        drop_outline(&mut doc)?;",
        "        let _ = &mut doc;",
        "deleting_a_page_drops_the_outline_and_keeping_them_all_does_not",
    ),
    Mutation(
        # Delete the page in the slot rather than the one that was named.
        # Identical on an unedited document and wrong the moment one moves.
        "edits: delete the page in the slot rather than the page that was named",
        "src/edits.rs",
        "            Command::Delete {\n                page: PageId::from_raw(page),",
        "            Command::Delete {\n                page: PageId::from_raw(page + 1),",
        "a_deleted_page_leaves_the_order_and_the_ones_after_it_move_up",
    ),
    Mutation(
        # Call every plan the file on disk. A print job then hands over the
        # original bytes, and the reader's deletions and turns are not on paper.
        "edits: report an edited document as the file on disk",
        "src/edits.rs",
        "            .all(|(at, page)| page.source as usize == at && page.turns % 4 == 0)",
        "            .all(|(at, page)| page.source as usize == at || page.turns % 4 == 0)",
        "only_an_unedited_document_is_the_file_on_disk",
    ),
    Mutation(
        # Print the file rather than the working document. The pages the reader
        # deleted come out of the printer.
        "print: print the file even when the model says it has been edited",
        "src/print.rs",
        "    if plan.is_identity() {",
        "    if true {",
        "an_edited_document_prints_the_pages_the_model_kept",
    ),
    Mutation(
        # Apply the view rotation and drop each page's own edit, which is what
        # the print path did until a page could be turned in the document.
        "print: apply the view rotation and not the page's own edit",
        "src/print.rs",
        "            Some((id, ((i16::from(page.turns) + view).rem_euclid(4)) as u8))",
        "            Some((id, (view.rem_euclid(4)) as u8))",
        "each_page_takes_its_own_edit_and_the_view_rotation_on_top",
    ),
    Mutation(
        # Save an encrypted document anyway. `lopdf` drops the encryption
        # silently, so the result opens with every restriction gone and nothing
        # anywhere says so.
        "save: let an encrypted document through",
        "src/save.rs",
        "    if doc.trailer.has(b\"Encrypt\") {",
        "    if false {",
        "an_encrypted_document_is_refused_rather_than_quietly_decrypted",
    ),
    Mutation(
        # Accept a plan that does not describe the file on disk. The turns then
        # land on whichever pages happen to be in those positions.
        "save: accept a plan of the wrong length",
        "src/save.rs",
        "    if pages.len() != plan.baseline as usize {",
        "    if false {",
        "a_plan_that_does_not_match_the_file_on_disk_is_refused",
    ),
    Mutation(
        # Overwrite the open document. The journal then replays against a
        # baseline that no longer exists.
        "save: allow writing over the source",
        "src/save.rs",
        "    if same_file(source, out) {",
        "    if false {",
        "saving_over_the_open_document_is_refused",
    ),
    Mutation(
        # Put the save in place during the staging, which is the shape the write
        # had before it was split. The document is then replaced while a worker
        # still has it mapped --- which succeeds, and leaves that worker serving
        # the file that used to be there.
        "save: put a save in place during the staging rather than after the close",
        "src/save.rs",
        "    let path = stage(source, &planned.bytes)?;",
        "    let path = write_atomically(source, &planned.bytes)\n        .map(|()| source.with_extension(PARTIAL))?;",
        "staging_a_save_in_place_writes_beside_the_source_and_leaves_it_alone",
    ),
    Mutation(
        # Report the commit as done without renaming anything. The reader is told
        # their document was saved, the file on disk is the one they opened, and
        # the staged copy of their work sits beside it under a name nothing reads.
        "save: report a commit that never renamed anything",
        "src/save.rs",
        "    commit(staged, source)\n}",
        "    let _ = (staged, source);\n    Ok(())\n}",
        "committing_a_staged_save_puts_the_edits_in_the_file_the_reader_opened",
    ),
    Mutation(
        # Stage before the guards run, which is the ordering the `reopen: false`
        # half of `SaveFailure` rests on. Every refusal then leaves a partial
        # file next to the reader's document under a name they never chose.
        "save: stage a save in place before its guards have run",
        "src/save.rs",
        "    let planned = planned_bytes(source, plan, OnChange::Refuse)?;",
        "    let early = stage(source, b\"\")?;\n    let planned = planned_bytes(source, plan, OnChange::Refuse)?;\n    let _ = early;",
        "a_refused_save_in_place_stages_nothing",
    ),
    Mutation(
        # Compare the paths as strings. Two spellings of one file then read as
        # two files, and the guard above passes while the file is overwritten.
        "save: two spellings of one path are two files",
        "src/save.rs",
        "        (Ok(a), Ok(b)) => a == b,",
        "        (Ok(_), Ok(_)) => false,",
        "saving_over_the_open_document_is_refused",
    ),
    Mutation(
        # Copy the bytes into place instead of renaming. An interrupted save then
        # leaves a truncated PDF where the reader's file was.
        "save: write straight to the destination rather than renaming into it",
        "src/save.rs",
        "    let partial = out.with_extension(PARTIAL);",
        "    let partial = out.to_path_buf();",
        "the_destination_is_replaced_whole_rather_than_written_through",
    ),
    Mutation(
        # A command that names a page by its position rather than its identity.
        # Identical on an unedited document, and wrong the moment a page moves.
        "edits: rotate the page in the slot rather than the page that was named",
        "src/edits.rs",
        "            Command::Rotate {\n                page: PageId::from_raw(page),",
        "            Command::Rotate {\n                page: PageId::from_raw(page + 1),",
        "a_turn_lands_on_the_page_it_named_and_nowhere_else",
    ),
    Mutation(
        # Report a document as unedited whenever it looks like the file on disk.
        # A rotate-and-rotate-back then reads as clean, and the reader is not
        # told there is an unsaved journal.
        "edits: read dirty off the working document instead of the journal",
        "src/edits.rs",
        "    let (applied, _) = model.depth();",
        "    let (applied, _) = (0usize, 0usize);",
        "a_turn_lands_on_the_page_it_named_and_nowhere_else",
    ),
    Mutation(
        # Keep the previous document's journal under a reused handle. The render
        # service reuses document numbers, so this is a real sequence.
        "edits: keep the model already under a reopened handle",
        "src/edits.rs",
        "        self.docs.lock().expect(\"edits lock\").insert(\n            doc,\n            Open {",
        "        self.docs.lock().expect(\"edits lock\").entry(doc).or_insert(\n            Open {",
        "reopening_under_a_reused_handle_does_not_inherit_the_previous_journal",
    ),
    Mutation(
        # Collapse the two refusals. An id that never existed and an id that was
        # deleted are different diagnoses, and the tombstone exists to keep them
        # apart.
        "edits: report every unknown page as deleted",
        "src/edits.rs",
        "        Refusal::NoSuchPage(_) => \"no such page\".into(),",
        "        Refusal::NoSuchPage(_) => \"that page has been deleted\".into(),",
        "an_id_no_document_ever_had_is_refused_by_name",
    ),
    Mutation(
        # Back to lowercasing, which is what stood here until 2026-08-01 and is the
        # whole subject of the change: `ß` is already lowercase, so it survives the
        # fold and `strasse` cannot find `Straße`.
        "fold: lowercase instead of case-folding",
        "src/search.rs",
        "            for folded in std::iter::once(ch).default_case_fold() {",
        "            for folded in ch.to_lowercase() {",
        "a_sharp_s_folds_to_two_letters",
    ),
    # There is deliberately no second mutation for the Greek half. Written and run:
    # the one above already turns `both_greek_sigmas_fold_together` red, because
    # lowercasing gets Greek wrong in its own way --- it maps `Σ` to `σ` and leaves
    # `ς` alone, so one word's two spellings land in different buckets. A second
    # mutation that pre-lowercased the input before the loop broke six *other* tests
    # and not that one, because it also defeated `match_case`.
    Mutation(
        # A pattern compiled case-sensitively against a haystack the fold has
        # already lowercased: any uppercase letter in a pattern then matches
        # nothing. It shipped that way, and the doc comment above `compile`
        # asserted the invariant it was breaking.
        "regex: compile a pattern case-sensitively whatever the option says",
        "src/search.rs",
        "        .case_insensitive(!match_case)",
        "        .case_insensitive(false)",
        "an_upper_case_pattern_matches_lower_case_text",
    ),
    Mutation(
        # The other direction: ignore case even when asked not to. Only a test
        # that turns the option *on* can see it.
        "regex: always ignore case, whatever the option says",
        "src/search.rs",
        "        .case_insensitive(!match_case)",
        "        .case_insensitive(true)",
        "an_upper_case_pattern_still_distinguishes_case_when_asked",
    ),
    Mutation(
        # No corpus reaches this: it fires only on a page that is both tagged and
        # carries a character above the BMP, and neither fixture is both. A
        # mutation switching the whole translation off passed `search-probe` and
        # `structure-probe` alike, which is why the arithmetic was split out and is
        # judged here instead.
        "text: leave the tagged runs in PDFium's index space",
        "src/text.rs",
        "    if ours.len() == len + 1 {\n        // No pair anywhere, so the two spaces are the same one.\n        return;\n    }",
        "    return;\n    #[allow(unreachable_code)]",
        "a_run_after_a_pair_moves_back_by_the_units_it_saved",
    ),
    Mutation(
        # The opposite: translate even when there is nothing to translate. It is
        # the identity, so only a fixture with no pair can tell.
        "text: round a run's end outwards to include a half-covered pair",
        "src/text.rs",
        "    let at = |index: u32| ours.get(index as usize).copied().unwrap_or(len as u32);",
        "    let at = |index: u32| ours.get(index as usize + 1).copied().unwrap_or(len as u32);",
        "a_run_ending_inside_a_pair_comes_back_empty",
    ),
    Mutation(
        # The defect the multilingual corpus was built to look for, and it was
        # there: `FPDFText_GetUnicode` is a UTF-16 API, so an astral code point
        # arrives as two lone surrogates. `char::from_u32` refuses both, the fold
        # drops them, and a CJK Extension B ideograph is unfindable while being
        # perfectly visible on the page.
        "text: report a surrogate pair as two characters",
        "src/text.rs",
        "    match next {\n        Some(low) if (0xDC00..0xE000).contains(&low) => {\n            (0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00), 2)\n        }\n        _ => (REPLACEMENT, 1),\n    }",
        "    let _ = next;\n    (REPLACEMENT, 1)",
        "a_surrogate_pair_becomes_one_scalar_over_two_units",
    ),
    Mutation(
        # The other direction: pair anything that follows a high surrogate. This
        # consumes a real character, so the page comes back one short and every
        # box after it shifts.
        "text: pair a high surrogate with whatever follows it",
        "src/text.rs",
        "        Some(low) if (0xDC00..0xE000).contains(&low) => {",
        "        Some(low) => {",
        "a_high_surrogate_followed_by_anything_else_is_replaced",
    ),
    Mutation(
        # A lone surrogate dropped rather than replaced. It looks tidier and it
        # shortens the page silently, which is the one thing an index space may
        # not do.
        "text: treat a lone low surrogate as two units wide",
        "src/text.rs",
        "    if !(0xD800..0xDC00).contains(&code) {\n        return (REPLACEMENT, 1);\n    }",
        "    if !(0xD800..0xDC00).contains(&code) {\n        return (REPLACEMENT, 2);\n    }",
        "a_lone_low_surrogate_is_replaced_and_never_paired_backwards",
    ),
    Mutation(
        "fold: keep every whitespace character instead of collapsing runs",
        "src/search.rs",
        "                if chars.last() == Some(&' ') {\n                    continue;\n                }",
        "",
        "a_run_of_spaces_matches_one_space",
    ),
    Mutation(
        "fold: treat a soft hyphen as a character",
        "src/search.rs",
        "            if ch == SOFT_HYPHEN {\n                continue;\n            }",
        "",
        "a_soft_hyphen_is_not_a_character",
    ),
    Mutation(
        # Written first as `to_ascii_lowercase().to_lowercase()`, which SURVIVED
        # -- and it should have: `to_ascii_lowercase` is the identity on every
        # non-ASCII character and agrees with `to_lowercase` on the rest, so the
        # composition is exactly `to_lowercase` and the edit changed nothing. A
        # mutation that changes nothing looks precisely like a test that cannot
        # fail, and the entry of that name in docs/TRAPS.md is about the minute
        # spent strengthening a test that was already fine.
        "fold: fold ASCII only, dropping the Unicode mapping",
        "src/search.rs",
        "            for folded in std::iter::once(ch).default_case_fold() {",
        "            for folded in [ch.to_ascii_lowercase()] {",
        "a_multi_character_lowercase_still_maps_back",
    ),
    Mutation(
        "fold: ignore the option and always lower-case",
        "src/search.rs",
        "            if match_case {\n                chars.push(ch);\n                source.push(index);\n                continue;\n            }",
        "",
        "matching_case_distinguishes_what_ignoring_it_conflated",
    ),
    Mutation(
        # The other direction of the same switch. Together they say the flag is
        # read, rather than that one of its two values happens to work.
        "fold: ignore the option and never lower-case",
        "src/search.rs",
        "            if match_case {",
        "            if true {",
        "case_is_ignored_in_both_directions",
    ),
    Mutation(
        "match: end the span by arithmetic instead of through the source map",
        "src/search.rs",
        "        let stop = hay.source[end - 1] as usize + 1;",
        "        let stop = start + needle.chars.len();",
        "a_multi_character_lowercase_still_maps_back",
    ),
    Mutation(
        "match: let occurrences overlap",
        "src/search.rs",
        "            // `aa` occurs once in `aaa`, not twice.\n            at += needle.chars.len();",
        "            // `aa` occurs once in `aaa`, not twice.\n            at += 1;",
        "matches_do_not_overlap",
    ),
    Mutation(
        "match: run a query of only whitespace",
        "src/search.rs",
        "    } else if needle.chars.iter().all(|ch| *ch == ' ') {\n        return Ok(Vec::new());\n    }",
        "    }",
        "an_empty_query_matches_nothing",
    ),
    Mutation(
        "whole word: check the left boundary only",
        "src/search.rs",
        "            || (boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))\n                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))",
        "            || boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))",
        "a_whole_word_search_bounds_both_ends_independently",
    ),
    Mutation(
        "whole word: check the right boundary only",
        "src/search.rs",
        "            || (boundary(at.checked_sub(1).map(|i| hay.chars[i]), Some(hay.chars[at]))\n                && boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied()))",
        "            || boundary(Some(hay.chars[end - 1]), hay.chars.get(end).copied())",
        "a_whole_word_search_bounds_both_ends_independently",
    ),
    Mutation(
        "whole word: treat the end of the page as not a boundary",
        "src/search.rs",
        "        _ => true,",
        "        _ => false,",
        "a_word_may_end_at_the_page_rather_than_at_a_boundary",
    ),
    Mutation(
        "whole word: require both neighbours to be non-word, not a boundary",
        "src/search.rs",
        "        (Some(left), Some(right)) => !(is_word(left) && is_word(right)),",
        "        (Some(left), Some(right)) => !is_word(left) && !is_word(right),",
        "a_whole_word_search_skips_the_word_it_is_part_of",
    ),
    Mutation(
        "whole word: count punctuation as part of a word",
        "src/search.rs",
        "    ch.is_alphanumeric() || ch == '_'",
        "    !ch.is_whitespace()",
        "a_whole_word_search_skips_the_word_it_is_part_of",
    ),
    Mutation(
        "whole word: skip the whole span after rejecting a candidate",
        "src/search.rs",
        "                // caught the restructure that introduced the regex path.\n                at += 1;",
        "                // caught the restructure that introduced the regex path.\n                at += needle.chars.len();",
        "a_rejected_candidate_does_not_hide_the_one_overlapping_it",
    ),
    Mutation(
        # Predicted against `a_match_is_found_where_it_is` first, and that was
        # simply wrong: its needle is a word with spaces on both sides, so it
        # matches identically whether or not the boundary test runs. What
        # notices is the *count* on the mixed fixture, where two of the four
        # occurrences are inside longer words -- so the discriminating assertion
        # is the one that pins what the plain search finds, not the one that
        # finds anything at all.
        "whole word: apply the boundary test whether or not it was asked for",
        "src/search.rs",
        "        !options.whole_word",
        "        false",
        "a_whole_word_search_skips_the_word_it_is_part_of",
    ),
    Mutation(
        "context: take the words after the hit from before it",
        "src/search.rs",
        "            before: slice_of(&text.codes, start.saturating_sub(CONTEXT_CHARS)..start),",
        "            before: slice_of(&text.codes, start..(start + CONTEXT_CHARS).min(text.codes.len())),",
        "a_hit_carries_the_words_on_either_side_of_it",
    ),
    Mutation(
        "context: run off the end of the page instead of clamping",
        "src/search.rs",
        "                stop..(stop + CONTEXT_CHARS).min(text.codes.len()),",
        "                stop..(stop + CONTEXT_CHARS),",
        "context_stops_at_the_ends_of_the_page",
    ),
    Mutation(
        "context: take everything before the hit, not a bounded window",
        "src/search.rs",
        "start.saturating_sub(CONTEXT_CHARS)..start",
        "0..start",
        "context_is_bounded_and_the_hit_is_not",
    ),
    Mutation(
        "context: show the query instead of what the page says",
        "src/search.rs",
        "            hit: exact_of(&text.codes, start..stop),",
        "            hit: query.to_string(),",
        "the_hit_is_the_page_text_and_not_the_query",
    ),
    Mutation(
        "context: collapse the whitespace inside the hit as well",
        "src/search.rs",
        "            hit: exact_of(&text.codes, start..stop),",
        "            hit: slice_of(&text.codes, start..stop),",
        "context_collapses_line_breaks_but_the_hit_keeps_them",
    ),
    Mutation(
        "context: leave the line breaks in the words around the hit",
        "src/search.rs",
        "        if ch.is_whitespace() {\n            if !out.ends_with(' ') {\n                out.push(' ');\n            }\n            continue;\n        }",
        "",
        "context_collapses_line_breaks_but_the_hit_keeps_them",
    ),
    Mutation(
        "page: report the query's length rather than the page's",
        "src/search.rs",
        "                chars: text.len() as u32,\n                problem: None,",
        "                chars: query.chars().count() as u32,\n                problem: None,",
        "a_page_with_no_text_reports_it_rather_than_no_matches",
    ),
    Mutation(
        # The invariant the wire carries: runs present means runs complete. A
        # truncated walk is a reading order missing an unknown part of the page,
        # and a consumer cannot tell which part.
        "structure: offer a truncated walk's runs anyway",
        "src/structure.rs",
        "        if self.truncated {\n            return Vec::new();\n        }",
        "",
        "a_truncated_walk_offers_nothing",
    ),
    Mutation(
        # The one that matters, and the one no fixture can catch. `/Encoding`
        # decides code -> CID and says nothing about CID -> Unicode; the
        # descendant's `/Ordering` is what supplies it. Both fields covary on
        # every page of `encodings.pdf` --- Identity-H with Identity, UniJIS with
        # Japan1 --- so this rule passes the corpus completely. Only the two
        # synthetic diagonals in `encoding.rs` reach it, and they are why the
        # module carries unit tests at all.
        "encoding: key on the font's /Encoding name instead of the ordering",
        "src/encoding.rs",
        "    let info = descendant.get(b\"CIDSystemInfo\").ok()?;\n"
        "    let info = resolve_dict(document, info).ok()?;",
        "    let info = descendant.get(b\"CIDSystemInfo\").ok()?;\n"
        "    let info = resolve_dict(document, info).ok()?;\n"
        "    let _ = info;\n"
        "    return Some(\n"
        "        font.get(b\"Encoding\")\n"
        "            .ok()\n"
        "            .and_then(|object| object.as_name().ok())\n"
        "            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())\n"
        "            .unwrap_or_default()\n"
        "            .starts_with(\"Identity\"),\n"
        "    );\n"
        "    #[allow(unreachable_code)]",
        "identity_encoding_over_a_known_ordering_is_not_a_guess",
    ),
    Mutation(
        # A `/ToUnicode` states the mapping whatever else the font says, so
        # ignoring it reports a document that answers the question as one that
        # does not. Only the fixture carrying one can tell.
        "encoding: ignore a /ToUnicode entirely",
        "src/encoding.rs",
        "    if font.get(b\"ToUnicode\").is_ok() {\n        return Some(false);\n    }",
        "",
        "a_tounicode_settles_it_even_over_identity_ordering",
    ),
    Mutation(
        # The control on the composite rule, inverted. A Type1 font with no
        # `/ToUnicode` is most PDFs ever made, and judging one reports the world
        # as broken while every test above still passes.
        "encoding: consider simple fonts as well as composite ones",
        "src/encoding.rs",
        "    font.get(b\"Subtype\")\n"
        "        .and_then(Object::as_name)\n"
        "        .map(|name| name == b\"Type0\")\n"
        "        .unwrap_or(false)",
        "    let _ = font;\n    true",
        "a_simple_font_is_not_considered",
    ),
    Mutation(
        # `None` is "this font cannot be judged", and dropping it silently makes
        # the page's answer clean on evidence nobody has --- which is the exact
        # lie the module was written one level up to stop.
        "encoding: treat a font that cannot be judged as clean",
        "src/encoding.rs",
        "            None => mapping.truncated = true,",
        "            None => {}",
        "a_font_that_cannot_be_judged_is_reported_as_unknown",
    ),
    Mutation(
        # `Identity` means "these numbers are glyph indices in this font", so
        # there is no table to consult and PDFium is guessing. Listing it as
        # mappable is the plausible mistake, and it turns the whole module off.
        # The handover that specified this mutation recorded
        # `a_page_lopdf_cannot_account_for_is_unknown` as its catcher, which also
        # goes red; the test named for the rule is the one aimed at here.
        "encoding: list Identity as an ordering PDFium can map",
        "src/encoding.rs",
        'const MAPPABLE_ORDERINGS: [&str; 5] = ["Japan1", "GB1", "CNS1", "Korea1", "KR"];',
        'const MAPPABLE_ORDERINGS: [&str; 6] =\n'
        '    ["Japan1", "GB1", "CNS1", "Korea1", "KR", "Identity"];',
        "identity_ordering_without_a_tounicode_is_a_guess",
    ),
    # ------------------------------------------------------------------
    # docmodel.rs --- the working document and its journal.
    #
    # Every test in that module drives the model directly, so none of them is
    # over a fixture anyone else wrote and all of them could in principle be
    # tautologies. Two of the mutations below are aimed at claims made only in a
    # comment: the statement ordering inside `Move`, and the stale-snapshot
    # `retain`. `docs/TRAPS.md` records a comment that claimed an ordering
    # mattered where no mutation could show it, which is what these are for.
    # ------------------------------------------------------------------
    Mutation(
        # The ordering the comment above these two statements claims is
        # load-bearing. Reading the anchor's position first overshoots by one
        # whenever the moved page sits ahead of the anchor -- and leaves the
        # other direction correct, which is why only one of the two move tests
        # is named.
        "docmodel: read the move anchor's position before removing the page",
        "src/docmodel.rs",
        "                let from = self.position(page);\n"
        "                self.order.remove(from);\n"
        "                let to = match after {\n"
        "                    None => 0,\n"
        "                    Some(anchor) => self.position(anchor) + 1,\n"
        "                };",
        "                let from = self.position(page);\n"
        "                let to = match after {\n"
        "                    None => 0,\n"
        "                    Some(anchor) => self.position(anchor) + 1,\n"
        "                };\n"
        "                self.order.remove(from);",
        "a_page_moved_after_one_that_follows_it_lands_immediately_after_it",
    ),
    Mutation(
        # Insert before the anchor rather than after it. The off-by-one in the
        # other direction, and the likelier of the two to be written.
        "docmodel: move a page in front of its anchor instead of behind it",
        "src/docmodel.rs",
        "                    Some(anchor) => self.position(anchor) + 1,",
        "                    Some(anchor) => self.position(anchor),",
        "a_page_moved_after_one_that_follows_it_lands_immediately_after_it",
    ),
    Mutation(
        # "No anchor" means the front. Sending it to the back instead is what a
        # reader would see as the drag going to the wrong end of the document.
        "docmodel: send an unanchored move to the back",
        "src/docmodel.rs",
        "                    None => 0,",
        "                    None => self.order.len(),",
        "a_page_moved_with_no_anchor_goes_to_the_front",
    ),
    Mutation(
        # Collapse the two refusals into one. The model still refuses, the
        # document is still correct, and the only thing lost is the distinction
        # between an id that was deleted and one that never existed -- which is
        # the whole reason `docs/PLAN.md` asks for tombstones.
        "docmodel: report a deleted page as one that never existed",
        "src/docmodel.rs",
        "            Err(Refusal::PageDeleted(id))",
        "            Err(Refusal::NoSuchPage(id))",
        "a_command_naming_a_deleted_page_is_refused_as_deleted",
    ),
    Mutation(
        # Delete without tombstoning. Indistinguishable from a correct delete in
        # the document itself: the page is gone from the order and from the
        # table, and only a later command naming it can tell.
        "docmodel: delete a page without tombstoning its id",
        "src/docmodel.rs",
        "                self.graves.insert(page);",
        "                let _ = &mut self.graves;",
        "a_command_naming_a_deleted_page_is_refused_as_deleted",
    ),
    Mutation(
        # A document with no pages is not a document.
        "docmodel: allow the last page to be deleted",
        "src/docmodel.rs",
        "                if self.order.len() == 1 {",
        "                if false {",
        "the_last_page_cannot_be_deleted",
    ),
    Mutation(
        # Let rotation accumulate past three. Every value below four is right, so
        # only a test that turns a page four times can see it -- and a viewer
        # that never turns a page more than twice would never show it either.
        "docmodel: let quarter turns accumulate without wrapping",
        "src/docmodel.rs",
        "                p.extra_turns = (i16::from(p.extra_turns) + i16::from(turns)).rem_euclid(4) as u8;",
        "                p.extra_turns = (i16::from(p.extra_turns) + i16::from(turns)).max(0) as u8;",
        "a_rotation_accumulates_and_wraps_at_four",
    ),
    Mutation(
        # Accept a crop box of zero width or height. It is the boundary, so every
        # inverted box is still refused and only the degenerate one gets through.
        "docmodel: accept a crop box enclosing no area",
        "src/docmodel.rs",
        "        self.urx > self.llx && self.ury > self.lly",
        "        self.urx >= self.llx && self.ury >= self.lly",
        "a_crop_enclosing_no_area_is_refused",
    ),
    Mutation(
        # Keep snapshots the redo tail's discard has invalidated. Nothing looks
        # wrong until a rebuild passes through one, and then every page after it
        # is built from commands that were thrown away.
        "docmodel: keep snapshots that the discarded redo tail produced",
        "src/docmodel.rs",
        "        self.snapshots.retain(|&at, _| at <= self.cursor);",
        "        self.snapshots.retain(|_, _| true);",
        "a_stale_snapshot_is_dropped_when_the_redo_tail_is_discarded",
    ),
    Mutation(
        # Do not discard the redo tail on a new command, which turns the journal
        # into a tree whose branches share a cursor.
        "docmodel: keep the redo tail when a new command is applied",
        "src/docmodel.rs",
        "        self.journal.truncate(self.cursor);",
        "        self.journal.truncate(self.journal.len());",
        "applying_after_an_undo_discards_the_redo_tail",
    ),
    Mutation(
        # Decode a PDF text string as Latin-1, which is what every "it is nearly
        # ASCII" implementation does and what this one deliberately does not.
        # The two agree on the accented range and disagree over 0x80--0x9F,
        # where PDFDocEncoding has punctuation.
        # Aimed inside `pdf_doc_encoded` rather than at one of its two call
        # sites: the first draft mutated the flush inside the loop, which a body
        # with no control characters never reaches, and it survived. That is the
        # trap about a mutation aimed at code no fixture reaches, met here.
        "annots: decode a text string as Latin-1 rather than PDFDocEncoding",
        "src/annots.rs",
        "        Ok(text) => text.chars().skip(1).collect(),",
        "        Ok(_) => run.iter().map(|&byte| byte as char).collect(),",
        "pdfdocencoding_is_not_latin1",
    ),
    Mutation(
        # Flatten a comment's body the way a title is flattened. Every visible
        # character survives; only the paragraphs are lost, which is why a
        # fixture asserting the words would pass.
        # Judged by `a_documents_body_keeps_its_paragraphs`, which reads a body
        # out of a document. The obvious candidate --- the test that calls
        # `sanitize_body` directly --- cannot see this at all, because what is
        # broken here is which flattener a body is *routed* to.
        "annots: collapse a body's newlines, as a one-line title would",
        "src/annots.rs",
        "    if keep_paragraphs {\n        sanitize_body(&decoded, limit)\n    } else {",
        "    if false {\n        sanitize_body(&decoded, limit)\n    } else {",
        "a_documents_body_keeps_its_paragraphs",
    ),
    Mutation(
        # The mirror: route every field through the body flattener, so an author
        # carrying a newline reaches a one-line byline with the newline in it.
        "annots: keep an author's newlines, as a body would",
        "src/annots.rs",
        "        crate::outline::sanitize_title(&decoded, limit)",
        "        sanitize_body(&decoded, limit)",
        "an_author_is_flattened_to_one_line",
    ),
    Mutation(
        # Drop a body's paragraph breaks in the flattener itself, which is the
        # rule rather than the routing. Judged by the pure test, and the two
        # together are what say both halves work.
        "annots: drop a body's paragraph breaks",
        "src/annots.rs",
        "            pending_breaks = (pending_breaks + 1).min(2);\n            pending_space = false;\n            continue;\n        }\n        if ch.is_whitespace()",
        "            pending_breaks = 0;\n            pending_space = true;\n            continue;\n        }\n        if ch.is_whitespace()",
        "a_body_keeps_its_newlines_where_a_title_would_not",
    ),
    Mutation(
        # Take `/Rect` as written. A producer may write either corner first, and
        # the specification says a consumer shall normalise it --- so this is
        # invisible on every fixture whose rectangles happen to be written the
        # usual way round.
        "annots: trust /Rect's corner order",
        "src/annots.rs",
        "    let left = values[0].min(values[2]);\n    let right = values[0].max(values[2]);",
        "    let left = values[0];\n    let right = values[2];",
        "a_rectangle_written_backwards_is_normalised",
    ),
    Mutation(
        # Ignore the page's own rotation. Every rectangle is still on its page,
        # still the right size and still in the right order --- only in the
        # wrong place, which no count can see.
        "annots: place a rectangle without the page's /Rotate",
        "src/annots.rs",
        "    let placed = crate::text::to_device(\n        turns,",
        "    let placed = crate::text::to_device(\n        0,",
        "a_rotated_page_places_a_rectangle_in_display_space",
    ),
    Mutation(
        # Clamp nothing. A rectangle at 1e10 then reaches the viewer, which
        # places a marker somewhere it can never scroll to.
        "annots: leave a rectangle wherever the file put it",
        "src/annots.rs",
        "        placed[0].clamp(0.0, width),",
        "        placed[0],",
        "a_rectangle_off_the_page_is_clamped_to_it",
    ),
    Mutation(
        # Report a `/Link` or a `/Widget` as a kind nobody knows, which puts a
        # permanent "some comments were dropped" notice on every document that
        # has a hyperlink in it.
        "annots: count a link and a form field as unreadable kinds",
        "src/annots.rs",
        "            if !Kind::is_not_a_comment(subtype) {",
        "            if true {",
        "a_link_and_a_widget_are_not_comments_and_are_not_counted_as_unknown",
    ),
    Mutation(
        # Accept a reply link without checking that walking up from it
        # terminates. The panel then walks a cycle with no visited set of its
        # own, which is a hang rather than a wrong row.
        "annots: accept a reply link that closes a loop",
        "src/annots.rs",
        "        if looped {\n            limits.cycles += 1;",
        "        if false {\n            limits.cycles += 1;",
        "a_reply_cycle_is_broken_and_counted",
    ),
    Mutation(
        # Accept any string of digits as a date. Month 13 and hour 99 then reach
        # the panel, which shows them.
        "annots: accept a date outside the calendar",
        "src/annots.rs",
        "    if !(1000..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {",
        "    if false {",
        "a_string_that_is_not_a_date_produces_no_date",
    ),
    Mutation(
        # Rebuild from the newest snapshot rather than the newest one at or below
        # the target. Correct whenever undo has not crossed a snapshot, which is
        # most of the time.
        #
        # `a_journal_replays_to_the_state_it_was_applied_to` was named here first
        # and is the wrong test: it walks a mixed journal and every prefix of it,
        # but applies eight commands where SNAPSHOT_EVERY is 32, so it never has
        # a snapshot to pick the wrong one of. The harness reported it caught by
        # something else, which is the cross-check earning its keep --- the
        # mutation was aimed at code that test does not reach.
        "docmodel: rebuild from the newest snapshot, wherever it is",
        "src/docmodel.rs",
        "            .filter(|&at| at <= upto)",
        "            .filter(|&at| at <= upto.max(usize::MAX))",
        "a_rebuild_never_starts_from_a_snapshot_ahead_of_its_target",
    ),
    # --- links.rs ----------------------------------------------------------
    Mutation(
        # Read `/Dest` before `/A`, which is the ordering `outline.rs` had to
        # learn the hard way: a `/GoToR` carries a `/D` that resolves perfectly
        # against *this* document, so the link jumps to a plausible page of
        # another file's numbering instead of being refused.
        "links: take /Dest before the action that overrides it",
        "src/links.rs",
        '    if let Ok(action) = annot.get(b"A") {',
        '    if let Ok(dest) = annot.get(b"Dest") {\n        return destination(dest, document, numbers, geometry, limits);\n    }\n    if let Ok(action) = annot.get(b"A") {',
        "an_action_beats_a_dest_sitting_beside_it",
    ),
    Mutation(
        # Follow an action tpdf does not know instead of declining it. The arm
        # still refuses the four named ones, so every other refusal assertion
        # stays green --- which is why the unknown case needs its own test.
        "links: follow an unknown action's destination",
        "src/links.rs",
        '            _ => refused("unsupported"),',
        "            _ => match action.get(b\"D\") {\n                Ok(dest) => destination(dest, document, numbers, geometry, limits),\n                Err(_) => Target::Broken,\n            },",
        "an_unknown_action_is_refused_rather_than_followed",
    ),
    Mutation(
        # Read every fit's top from `/XYZ`'s position. `/FitH top` then reads the
        # element after the top, which on a real destination is absent --- so the
        # link lands at the page's top and looks like it works.
        "links: read every fit's top from XYZ's position",
        "src/links.rs",
        '        b"XYZ" => 3,\n        b"FitH" | b"FitBH" => 2,\n        b"FitR" => 5,',
        '        b"XYZ" | b"FitH" | b"FitBH" | b"FitR" => 3,',
        "each_fit_takes_its_top_from_its_own_position",
    ),
    Mutation(
        # Flip the destination offset against the page the *link* is on rather
        # than the page it lands on. Invisible on any document of uniform pages,
        # which is every fixture here but the one written for it.
        "links: flip the offset against the wrong page",
        "src/links.rs",
        "    let shown = *geometry.get(page as usize)?;",
        "    let shown = *geometry.first()?;",
        "the_offset_is_flipped_against_the_page_it_lands_on",
    ),
    Mutation(
        # Treat `/F` as a boolean rather than testing bit 2. Every real link sets
        # `/F 4` (Print), so this drops all of them --- which is why the control
        # in that test is a printing link rather than an unflagged one.
        "links: treat any /F as hidden",
        "src/links.rs",
        "            .is_some_and(|flags| flags & 0b10 != 0);",
        "            .is_some_and(|flags| flags != 0);",
        "a_hidden_link_is_not_clickable_and_a_printing_one_is",
    ),
    Mutation(
        # Give up on a name tree by reporting the name missing rather than the
        # bound firing. The link is `Broken` either way, so only the limit can
        # tell a hostile tree from an honest typo.
        "links: report an exhausted name tree as a missing name",
        "src/links.rs",
        "        Found::Exhausted => {\n            limits.unresolved_names += 1;\n            Target::Broken\n        }",
        "        Found::Exhausted => Target::Broken,",
        "a_cyclic_name_tree_is_given_up_on_and_counted",
    ),
    Mutation(
        # Charge a limit for an ordinary missing name too, which makes every
        # broken link in a healthy document look like a truncated scan. The
        # control for the mutation above, failing in the other direction.
        "links: charge a limit for a name that is simply absent",
        "src/links.rs",
        "        Found::Missing => Target::Broken,",
        "        Found::Missing => {\n            limits.unresolved_names += 1;\n            Target::Broken\n        }",
        "a_missing_name_is_broken_without_charging_a_limit",
    ),
    Mutation(
        # Look names up only in the name tree, dropping the PDF 1.1 dictionary.
        # A reader that knows one mechanism silently fails to follow every link
        # in whichever half of the corpus uses the other.
        "links: forget the PDF 1.1 /Dests dictionary",
        "src/links.rs",
        '    if let Ok(dests) = catalog.get(b"Dests") {',
        '    if let (false, Ok(dests)) = (true, catalog.get(b"Dests")) {',
        "a_named_destination_resolves_through_the_flat_dictionary",
    ),
    Mutation(
        # Trust `/Rect`'s corner order, which §12.5.2 says a consumer shall
        # normalise. Invisible on every rectangle written the usual way round.
        "links: trust /Rect's corner order",
        "src/links.rs",
        "            values[0].min(values[2]) as f64 - ox,\n            values[1].min(values[3]) as f64 - oy,\n            values[0].max(values[2]) as f64 - ox,\n            values[1].max(values[3]) as f64 - oy,",
        "            values[0] as f64 - ox,\n            values[1] as f64 - oy,\n            values[2] as f64 - ox,\n            values[3] as f64 - oy,",
        "a_rectangle_written_backwards_is_normalised",
    ),
    Mutation(
        # List zero-area links, which puts a target in the list no reader can
        # reach and every hit test walks past.
        "links: list a rectangle with no area",
        "src/links.rs",
        "        if rect[2] - rect[0] <= 0.0 || rect[3] - rect[1] <= 0.0 {\n            continue;\n        }",
        "        if false {\n            continue;\n        }",
        "a_zero_area_rectangle_is_left_out",
    ),
    Mutation(
        # Count a comment sharing the `/Annots` array as an unreadable entry,
        # which makes every reviewed document report a truncated link scan.
        "links: count a non-link annotation as unreadable",
        "src/links.rs",
        "            Ok(_) => continue,",
        "            Ok(_) => {\n                limits.unreadable += 1;\n                continue;\n            }",
        "annotations_that_are_not_links_are_skipped_without_complaint",
    ),
    Mutation(
        # Answer "no links" for a document whose pages could not be read. The
        # scan returns an empty list either way; only the limit can tell a
        # document with no links from one nothing could look at.
        "links: report pages lopdf could not read as nothing missing",
        "src/links.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = 0;",
        "a_page_lopdf_cannot_account_for_is_reported",
    ),
    Mutation(
        # Charge a deficit for every document, which puts a warning on every file
        # tpdf opens and trains a reader to ignore the one that matters.
        "links: charge a page deficit even when the parsers agree",
        "src/links.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = page_count.saturating_sub(pages.len()) + 1;",
        "a_document_both_parsers_agree_about_reports_nothing_missing",
    ),
    Mutation(
        # Underflow when lopdf reads further than PDFium paginates, reporting the
        # largest number the type can hold as a page deficit.
        "links: subtract page counts without saturating",
        "src/links.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = page_count.wrapping_sub(pages.len());",
        "seeing_more_pages_than_claimed_is_not_a_deficit",
    ),
    Mutation(
        # The same silence in the comment scan, which is where the shape was
        # copied from --- so the two must be broken separately to prove each.
        "annots: report pages lopdf could not read as nothing missing",
        "src/annots.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = 0;",
        "a_page_lopdf_cannot_account_for_is_reported",
    ),
    Mutation(
        # And its control, in the other direction.
        "annots: charge a page deficit even when the parsers agree",
        "src/annots.rs",
        "    limits.pages_missed = page_count.saturating_sub(pages.len());",
        "    limits.pages_missed = page_count.saturating_sub(pages.len()) + 1;",
        "a_document_both_parsers_agree_about_reports_nothing_missing",
    ),
    Mutation(
        # One sentence for every failure, which is what stood here before: a
        # document that is well formed and merely locked is then announced as
        # damaged, and the reader goes looking for another copy of a good file.
        "progressive: give every open failure the same reason",
        "src/progressive.rs",
        '        err::PASSWORD => "This document needs a password, and tpdf cannot ask for one yet.".into(),',
        '        err::PASSWORD => "This file is not a PDF, or it is damaged beyond reading.".into(),',
        "each_reason_says_something_different",
    ),
    Mutation(
        # Report success as the reason. Reachable --- PDFium can return a null
        # handle with no error set --- and it reads as though the open worked.
        "progressive: report no-error as the reason a document did not open",
        "src/progressive.rs",
        '        _ => "This document could not be opened, and PDFium did not say why.".into(),',
        '        0 => "No error.".into(),\n        _ => "This document could not be opened, and PDFium did not say why.".into(),',
        "success_is_not_reported_as_a_reason",
    ),
    Mutation(
        # Walk the outline breadth-first: siblings before children. Both lists
        # still hold every entry, so only an order-sensitive comparison notices
        # -- and `links-probe --mode agree` compares two lists positionally.
        "links: walk the outline siblings-first rather than pre-order",
        "src/links.rs",
        "        if let Ok(Object::Reference(child)) = dict.get(b\"First\") {\n            walk_outline(\n                document,\n                *child,\n                numbers,\n                geometry,\n                seen,\n                out,\n                limits,\n                depth - 1,\n            );\n        }\n        node = match dict.get(b\"Next\") {",
        "        node = match dict.get(b\"Next\") {",
        "the_outline_walk_is_pre_order",
    ),
    Mutation(
        # Drop the visited set. A /Next chain that loops then does not return a
        # wrong answer -- it does not return, which is why the test asserts a
        # length rather than a value.
        "links: follow an outline chain that loops",
        "src/links.rs",
        "        if out.len() >= MAX_OUTLINE_ITEMS || !seen.insert(id) {",
        "        if out.len() >= MAX_OUTLINE_ITEMS {",
        "a_looping_outline_chain_terminates",
    ),
    Mutation(
        # Answer "no destination" for an entry whose /Dest does not resolve,
        # which is the PDFium behaviour this oracle exists to be independent of.
        # An oracle that reproduced the defect would agree perfectly and say
        # nothing.
        "links: call an unresolvable outline destination absent rather than broken",
        "src/links.rs",
        "    match annot.get(b\"Dest\") {\n        Ok(dest) => destination(dest, document, numbers, geometry, limits),",
        "    match annot.get(b\"Dest\") {\n        Ok(dest) => match destination(dest, document, numbers, geometry, limits) {\n            Target::Broken => Target::None,\n            other => other,\n        },",
        "a_destination_that_resolves_nowhere_is_broken_not_absent",
    ),
    Mutation(
        # Lay the page out from /MediaBox, which is what stood here before.
        # PDFium lays it out from /CropBox, so every rectangle on a cropped page
        # is out by the difference -- silently, on a page that looks normal.
        #
        # Aimed at `pagetree.rs` since the arithmetic moved there to serve the
        # mark writer as well. The test it expects is still `links.rs`'s: that
        # module is the caller that has a cropped fixture, and one mutation
        # reddening a test in another module is the point of sharing the code.
        "pagetree: place rectangles against the media box rather than the crop box",
        "src/pagetree.rs",
        "    let shown = match box_of(b\"CropBox\") {",
        "    let shown = match None::<[f32; 4]> {",
        "a_cropped_page_places_a_rectangle_in_the_crop_box_s_space",
    ),
    Mutation(
        # Take the crop box's *size* and ignore its origin. Subtler than the one
        # above and wrong in the same way: the page is the right shape and every
        # rectangle on it is shifted.
        "pagetree: use the crop box's size but not its origin",
        "src/pagetree.rs",
        "        origin: (shown[0], shown[1]),",
        "        origin: (0.0, 0.0),",
        "a_cropped_page_places_a_rectangle_in_the_crop_box_s_space",
    ),
    Mutation(
        # Trust a crop box larger than the sheet, so the page is displayed bigger
        # than its own paper and every rectangle is scaled against a size the
        # renderer never uses.
        "pagetree: trust a crop box larger than the media box",
        "src/pagetree.rs",
        "    let shown = match box_of(b\"CropBox\") {\n        Some(crop) => [\n            crop[0].max(media[0]),\n            crop[1].max(media[1]),\n            crop[2].min(media[2]),\n            crop[3].min(media[3]),\n        ],",
        "    let shown = match box_of(b\"CropBox\") {\n        Some(crop) => crop,",
        "a_crop_box_larger_than_the_page_is_intersected_with_it",
    ),
    Mutation(
        # The same blindness in the comment scan, which computes its geometry
        # separately -- so the two must be broken separately to prove each.
        "annots: place rectangles against the media box rather than the crop box",
        "src/annots.rs",
        "    let crop = box_of(b\"CropBox\").map(|crop| {",
        "    let crop = None::<[f32; 4]>.map(|crop: [f32; 4]| {",
        "a_cropped_page_places_a_comment_in_the_crop_box_s_space",
    ),
    Mutation(
        # `links.rs` carried three crop-box mutations and `annots.rs` one, for
        # code the two modules compute independently. The origin half of the
        # comment scan was reachable by no mutation, and the intersection half
        # by no *test* -- see the mirror added to annots.rs the same day.
        "annots: use the crop box's size but not its origin",
        "src/annots.rs",
        "        (width, height, turns, shown[0], shown[1])",
        "        (width, height, turns, 0.0, 0.0)",
        "a_cropped_page_places_a_comment_in_the_crop_box_s_space",
    ),
    Mutation(
        # Trust a crop box larger than the sheet. The twin of the links mutation
        # of the same name, against separate code with its own arithmetic.
        "annots: trust a crop box larger than the media box",
        "src/annots.rs",
        "    let crop = box_of(b\"CropBox\").map(|crop| {\n"
        "        [\n"
        "            crop[0].max(media[0]),\n"
        "            crop[1].max(media[1]),\n"
        "            crop[2].min(media[2]),\n"
        "            crop[3].min(media[3]),\n"
        "        ]\n"
        "    });",
        "    let crop = box_of(b\"CropBox\");",
        "a_crop_box_larger_than_the_page_is_intersected_with_it",
    ),
    Mutation(
        # `corner_of` was inline in `origin_pt` until 2026-08-16, where it needed
        # a live PDFium page and no unit test could reach it -- so both of its
        # rules were guarded by nothing, in the function every character, link
        # and comment position is measured from.
        # Re-aimed 2026-08-18: `corner_of` was folded into `normalised`, which
        # answers with the whole rectangle rather than one corner, because the
        # crop a reader sets needs all four numbers. The rule and its test are
        # the same; only where they live moved.
        "progressive: take a crop box's corners in the order they were written",
        "src/progressive.rs",
        "        box_pt[0].min(box_pt[2]),\n        box_pt[1].min(box_pt[3]),\n        box_pt[0].max(box_pt[2]),\n        box_pt[1].max(box_pt[3]),",
        "        box_pt[0],\n        box_pt[1],\n        box_pt[2],\n        box_pt[3],",
        "a_crop_box_written_backwards_still_yields_its_lower_left",
    ),
    Mutation(
        # One NaN here is a whole page of NaN boxes, and a NaN comparison fails
        # silently rather than loudly -- so it reads as an empty text layer.
        "progressive: let a non-finite crop box through",
        "src/progressive.rs",
        "    if !ok || !box_pt.iter().all(|value| value.is_finite()) {",
        "    if !ok {",
        "a_crop_box_with_a_non_finite_corner_is_refused",
    ),
    Mutation(
        # Trust the out-parameters of a call PDFium refused, which leaves them
        # holding whatever they held.
        "progressive: trust a crop box PDFium would not answer for",
        "src/progressive.rs",
        "    if !ok || !box_pt.iter().all(|value| value.is_finite()) {",
        "    if false || !box_pt.iter().all(|value| value.is_finite()) {",
        "a_crop_box_pdfium_would_not_answer_for_is_the_origin",
    ),
    Mutation(
        # Replace the page's `/Annots` rather than extending it. A page that had
        # no comments is unaffected, which is most pages and every fixture
        # written by hand -- and a page that had one loses it the moment a reader
        # highlights anything.
        "save: replace a page's /Annots instead of extending it",
        "src/save.rs",
        "        Some(Object::Array(mut array)) => {\n            array.push(Object::Reference(annotation));",
        "        Some(Object::Array(mut array)) => {\n            array.clear();\n            array.push(Object::Reference(annotation));",
        "a_mark_is_written_whatever_shape_the_page_s_annots_is_in",
    ),
    Mutation(
        # Write the annotation object and leave it off the page. The file grows
        # by a perfectly well-formed annotation that is on no page, which every
        # reader reports as a document with no comments -- and which any check
        # counting objects passes.
        "save: write the mark object without listing it on the page",
        "src/save.rs",
        "        let annotation = doc.add_object(dictionary);\n        attach(doc, page, annotation)?;",
        "        let _annotation = doc.add_object(dictionary);",
        "a_marked_page_lists_the_mark_in_its_own_annots",
    ),
    Mutation(
        # Let a mark be written onto a page object that two page numbers share.
        # It appears on both, which is not what the reader asked for and not
        # something the file records as a mistake.
        "save: allow a mark on a page two numbers share",
        "src/save.rs",
        "        if kept\n            .iter()\n            .filter(|number| pages.get(**number as usize - 1) == Some(&page))\n            .count()\n            > 1\n        {",
        "        if false {",
        "a_mark_on_a_page_two_numbers_share_is_refused",
    ),
    Mutation(
        # Scope the shared-page refusal to the whole file rather than to this
        # mark's page. Every refusal test still passes; what breaks is the
        # ordinary case, where one malformed page makes the rest unmarkable.
        "save: refuse a mark anywhere in a file that has a shared page",
        "src/save.rs",
        "            .filter(|number| pages.get(**number as usize - 1) == Some(&page))",
        "            .filter(|number| pages.iter().filter(|p| Some(*p) == pages.get(**number as usize - 1)).count() > 1)",
        "a_mark_on_an_unshared_page_of_a_document_that_has_a_shared_one_is_written",
    ),
    Mutation(
        # Keep a quad that covers nothing. A `/QuadPoints` entry of zero area is
        # one some readers draw as nothing at all and others draw as a hairline,
        # so the mark's appearance stops being the document's.
        "save: keep quads that cover no area",
        "src/save.rs",
        "        .filter(|quad| quad.covers_area())",
        "        .filter(|_| true)",
        "a_mark_whose_quads_all_collapse_is_refused_rather_than_written_empty",
    ),
    Mutation(
        # Let a plan carrying a mark call itself the file on disk. The print path
        # then hands over the original bytes, and a reader who highlighted a
        # document prints one without the highlights -- with nothing failing,
        # because what it printed is a perfectly good file.
        "edits: call a plan with marks in it the file itself",
        "src/edits.rs",
        "        self.marks.is_empty()\n            && self.pages.len() == self.baseline as usize",
        "        self.pages.len() == self.baseline as usize",
        "a_plan_carrying_a_mark_is_not_the_file_on_disk",
    ),
    Mutation(
        # Drop the marks a subset plan should carry. An extract of pages a reader
        # highlighted comes out unmarked, which looks like a feature nobody
        # implemented rather than one that was lost on the way.
        "edits: leave the marks out of a plan",
        "src/edits.rs",
        "    pages\n        .iter()\n        .flat_map(|view| {",
        "    pages\n        .iter()\n        .take(0)\n        .flat_map(|view| {",
        "a_mark_is_carried_into_the_plan_for_the_page_it_is_on",
    ),
    Mutation(
        # Write out the note the mark was made with rather than the one it says
        # now. A reader who types on a highlight and saves gets a file with the
        # highlight in it and an empty note, and everything on screen still says
        # what they typed -- so nothing is wrong until the file is reopened.
        "edits: write the note a mark was made with rather than what it says",
        "src/edits.rs",
        "                    note: model.note_of(*mark).to_string(),",
        "                    note: String::new(),",
        "a_note_reaches_the_reader_and_the_writer_as_the_same_words",
    ),
    Mutation(
        # The same on the other side of the boundary. The model has the note,
        # the file gets the note, and the box the reader types in opens empty
        # every time they come back to it.
        "edits: leave the note out of the state the frontend redraws from",
        "src/edits.rs",
        "                note: model.note_of(id).to_string(),",
        "                note: String::new(),",
        "a_note_reaches_the_reader_and_the_writer_as_the_same_words",
    ),
    Mutation(
        # Rewind the allocator with the cursor. This is the failure `docmodel`'s
        # module note names and deferred until something issued an id: the mark
        # a reader undid gives its number back, and the next mark is created
        # wearing it -- so a redo restores "the" mark and gets somebody else's.
        "docmodel: give an undone mark's id back to the allocator",
        "src/docmodel.rs",
        "        self.cursor -= 1;\n        self.now = self.rebuild(self.cursor);",
        "        self.cursor -= 1;\n        self.next_mark = self.next_mark.saturating_sub(1);\n        self.now = self.rebuild(self.cursor);",
        "an_id_spent_by_an_undone_mark_is_never_issued_again",
    ),
    Mutation(
        # Spend the id before the preconditions are checked. Nothing visible
        # changes -- the mark is still refused -- which is the point: the ids
        # then run ahead of the marks, and the only thing that can see it is the
        # accounting observable.
        "docmodel: issue a mark's id before checking that it covers anything",
        "src/docmodel.rs",
        "        if empty {\n            return Err(Refusal::EmptyMark);\n        }\n        self.now.live(mark.page)?;\n\n        let id = MarkId(self.next_mark);",
        "        let id = MarkId(self.next_mark);\n        self.next_mark += 1;\n        if empty {\n            return Err(Refusal::EmptyMark);\n        }\n        self.now.live(mark.page)?;\n\n        let id = MarkId(id.get());",
        "a_mark_covering_nothing_is_refused_and_spends_no_id",
    ),
    Mutation(
        # Demand that every quad have area. A selection running to the end of a
        # line yields a real rectangle followed by an empty one, so this refuses
        # ordinary highlights -- and passes every case in the refusal test,
        # which only ever hands it marks where nothing has area.
        "docmodel: require every quad to cover area rather than any",
        "src/docmodel.rs",
        "            !mark.quads.iter().any(|quad| quad.covers_area())",
        "            !mark.quads.iter().all(|quad| quad.covers_area())",
        "one_quad_with_area_is_enough",
    ),
    Mutation(
        # Drop a deleted page's marks without tombstoning their ids. The marks
        # vanish correctly and a command naming one afterwards is then told it
        # never existed, which is the wrong diagnosis rather than a coarse one.
        "docmodel: let a deleted page's marks go without tombstoning them",
        "src/docmodel.rs",
        "                for mark in self.marks.remove(&page).unwrap_or_default() {\n                    self.mark_graves.insert(mark);",
        "                for mark in self.marks.remove(&page).unwrap_or_default() {\n                    let _ = mark;",
        "deleting_a_page_takes_its_marks_and_undo_brings_both_back",
    ),
    Mutation(
        # Leave an empty vector behind. Every behaviour is identical; what
        # changes is that a working document compares unequal to one that was
        # never annotated, which is what a snapshot rebuild is checked against.
        "docmodel: leave an empty mark list under a page key",
        "src/docmodel.rs",
        "                if list.is_empty() {\n                    self.marks.remove(&page);\n                }",
        "                let _ = list.is_empty();",
        "a_document_annotated_and_cleared_compares_equal_to_one_that_never_was",
    ),
    Mutation(
        # Keep the bodies of commands the redo tail discarded. Nothing behaves
        # differently and no document is wrong -- the table simply grows for as
        # long as a reader keeps annotating and undoing.
        "docmodel: keep mark bodies whose commands were discarded",
        "src/docmodel.rs",
        "                Command::Annotate { mark, note, .. } => {\n                    self.marks.remove(&mark);\n                    self.notes.remove(&note);\n                }",
        "                Command::Annotate { .. } => {}",
        "an_id_spent_by_an_undone_mark_is_never_issued_again",
    ),
    Mutation(
        # Keep the note when the mark's page is deleted. The mark is gone from
        # every list, so nothing on screen or in a written file differs -- and
        # the note is still reachable through a mark this document no longer
        # has, which an undo then restores twice over.
        "docmodel: keep a note when the page it is on is deleted",
        "src/docmodel.rs",
        "                    self.notes.remove(&mark);\n                }\n            }\n            Command::Move",
        "                }\n            }\n            Command::Move",
        "a_marks_note_goes_with_it_and_comes_back_with_it",
    ),
    Mutation(
        # Keep the note when the mark is taken off the page. Same shape as the
        # entry above and a different arm: the map's keys are meant to be
        # exactly the live marks, and a leftover makes a document that was
        # annotated and un-annotated compare unequal to one that never was --
        # which is what a snapshot rebuild is checked against.
        "docmodel: keep a note when its mark is taken off the page",
        "src/docmodel.rs",
        "                self.mark_graves.insert(mark);\n                self.notes.remove(&mark);",
        "                self.mark_graves.insert(mark);",
        "a_mark_that_is_removed_says_nothing",
    ),
    Mutation(
        # Keep note versions whose commands went with the discarded redo tail.
        # No behaviour differs at all: the versions are unreachable, the ids are
        # never re-issued, and a reader who types and undoes in a loop grows the
        # table forever. `note_bodies` is the only observable there is.
        "docmodel: keep note versions whose commands were discarded",
        "src/docmodel.rs",
        "                Command::Renote { note, .. } => {\n                    self.notes.remove(&note);\n                }",
        "                Command::Renote { .. } => {}",
        "a_note_in_the_discarded_redo_tail_goes_with_it",
    ),
    Mutation(
        # Issue the note's id before checking the mark is live. A refused note
        # then spends a version that nothing can ever read, which is the same
        # accounting `annotate` states for mark ids and the same reason.
        "docmodel: issue a note's id before checking the mark",
        "src/docmodel.rs",
        "        self.now.live_mark(mark)?;\n        let note = self.issue_note(note);",
        "        let note = self.issue_note(note);\n        self.now.live_mark(mark)?;",
        "a_refused_note_spends_nothing",
    ),
    Mutation(
        # Answer "no such mark" for one that was removed. The coarse diagnosis,
        # and the wrong one: it says the mark never existed to a caller whose
        # own undo took it off a moment ago.
        "docmodel: report a removed mark as one that never existed",
        "src/docmodel.rs",
        "            .ok_or(if self.mark_graves.contains(&mark) {",
        "            .ok_or(if false {",
        "a_note_names_a_mark_and_is_refused_by_name",
    ),
    Mutation(
        # Report the marks in whatever order the map iterates. The overlay and
        # the writer both take this as reading order, and a map's order is not
        # one -- it is stable within a run and unrelated to the document.
        "docmodel: report marks in map order rather than page order",
        "src/docmodel.rs",
        "        self.order\n            .iter()\n            .flat_map(|page| self.marks_on(*page).iter().map(|mark| (*page, *mark)))\n            .collect()",
        "        self.marks\n            .iter()\n            .flat_map(|(page, list)| list.iter().map(|mark| (*page, *mark)))\n            .collect()",
        "marks_come_back_in_page_order_after_the_pages_move",
    ),
    Mutation(
        # Undo the flip regardless of the turn. Correct at /Rotate 0 -- which is
        # most documents and every fixture written by hand -- and a quarter turn
        # out on the scanned pages that carry a rotation, which is exactly where
        # a highlight would land beside the words it was made from.
        "text: map a display box back with the arm for no rotation",
        "src/text.rs",
        "        1 => [top, left, bottom, right],",
        "        1 => [left, h0 - bottom, right, h0 - top],",
        "a_display_box_maps_back_to_the_page_box_it_came_from",
    ),
    Mutation(
        # Emit one arm's corners the wrong way round. `/QuadPoints` built from
        # an improper rectangle is one PDF 32000-1 tells consumers to normalise,
        # and the ones that do not draw nothing at all.
        "text: emit a mapped-back rectangle with its corners swapped",
        "src/text.rs",
        "        _ => [w0 - bottom, h0 - right, w0 - top, h0 - left],",
        "        _ => [w0 - top, h0 - left, w0 - bottom, h0 - right],",
        "a_mapped_back_rectangle_is_proper",
    ),
    Mutation(
        # Write an underline as a highlight. It draws correctly, because the
        # appearance stream is ours and is unaffected -- and Acrobat, Preview
        # and this application's own sidebar all report it as a highlight. The
        # failure that looks like nothing is wrong.
        "save: write an underline under the highlight's subtype",
        "src/save.rs",
        '        MarkKind::Underline => b"Underline",',
        '        MarkKind::Underline => b"Highlight",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Treat a line as a wash: it then fills its whole quad, multiplied, at
        # 40%. One predicate deciding four things, which is why one mutation
        # reddens three tests.
        # Re-aimed when `is_wash` and `is_note` were replaced by one `ink`
        # question: the arm it named said `=> false` and now says `=> Ink::Line`.
        "save: draw the two line kinds as washes",
        "src/save.rs",
        "        MarkKind::Underline | MarkKind::StrikeOut => Paint::Line,",
        "        MarkKind::Underline | MarkKind::StrikeOut => Paint::Wash,",
        "a_line_is_opaque_and_a_wash_is_not",
    ),
    Mutation(
        # Centre the underline on the quad's bottom edge, which is the obvious
        # reading of "under" and puts half the rule outside the `/BBox`. Every
        # reader clips it, and the result looks like a thinner line rather than
        # like a defect -- which is why the assertion is about the bound and not
        # about how it looks.
        "save: centre an underline on the edge it should sit on",
        "src/save.rs",
        "        MarkKind::Underline => (bottom, thickness),",
        "        MarkKind::Underline => (bottom - thickness / 2.0, thickness),",
        "a_line_stays_inside_the_quad_it_marks",
    ),
    Mutation(
        # And a strikeout drawn at the bottom, which is an underline with the
        # wrong subtype. Every check keyed on the subtype passes; only where the
        # rule sits tells them apart.
        "save: draw a strikeout where an underline goes",
        "src/save.rs",
        "        MarkKind::StrikeOut => (bottom + full / 2.0 - thickness / 2.0, thickness),",
        "        MarkKind::StrikeOut => (bottom, thickness),",
        "a_strikeout_crosses_the_text_and_an_underline_sits_under_it",
    ),
    Mutation(
        # Take the colour off the wire as it arrives. JSON refuses `NaN` and
        # `Infinity` as literals, which is what makes this look safe -- and
        # `1e40` is valid JSON and is `f32::INFINITY` once it is an `f32`, which
        # `format!` writes into a content stream as `inf`.
        "edits: take a colour off the wire without bringing it into range",
        "src/edits.rs",
        "                    color: want.color.map(channel),",
        "                    color: want.color,",
        "a_colour_off_the_wire_is_brought_into_the_range_the_model_promises",
    ),
    Mutation(
        # The boundary rather than the writer: ignore what the caller asked for.
        # Every mark is then a highlight however it was chosen, and the file is
        # correct for whatever the mark claims to be -- so `annot-probe` agrees
        # with it and only a test that names the kind on both sides can see it.
        "edits: write every mark as a highlight whatever was asked for",
        "src/edits.rs",
        "                    kind: want.kind,",
        "                    kind: MarkKind::Highlight,",
        "the_kind_the_caller_asked_for_reaches_the_plan_and_the_reply",
    ),
    Mutation(
        # The eraser's whole reason for a `quads_of` accessor: read the body
        # instead, which still holds the rectangle the drawing had before a
        # stroke was rubbed out. Every field the reply carries is otherwise
        # correct, so only a test that erases and then reads the rectangle can
        # see it.
        "edits: send the rectangle the drawing was made with, not the one it has",
        "src/edits.rs",
        "                quads: model\n                    .quads_of(id)",
        "                quads: mark\n                    .quads",
        "the_reply_carries_the_rectangle_the_drawing_has_now",
    ),
    Mutation(
        # The same read on the path that reaches a FILE rather than the window.
        # A document could then look right on screen and save the erased stroke,
        # which is the worse direction of the two.
        "edits: write the drawing the file was opened with, not the one on screen",
        "src/edits.rs",
        "                    strokes: model.strokes_of(*mark).to_vec(),",
        "                    strokes: body.strokes.clone(),",
        "a_saved_file_is_written_from_what_survived_the_eraser",
    ),
    Mutation(
        # Refuse instead of removing. The model is right to refuse a drawing of
        # nothing; this is the layer that knows the sweep meant "get rid of it",
        # and without it a reader who rubs out the last stroke gets an error
        # message and a drawing that is still there.
        "edits: refuse a sweep that takes the last stroke instead of removing the drawing",
        "src/edits.rs",
        "        if keep.iter().any(Stroke::is_drawable) {",
        "        if true {",
        "erasing_the_last_stroke_takes_the_drawing_with_it",
    ),
    Mutation(
        # Act on the half of the gesture that made sense. The stroke it did
        # understand is erased on the strength of a sweep aimed at a drawing
        # this is not -- and the reply looks entirely normal.
        "edits: let a sweep name a stroke that is not there",
        "src/edits.rs",
        "        if let Some(past) = remove.iter().find(|&&at| at >= held) {",
        "        if let Some(past) = None::<usize>.as_ref() {",
        "a_gesture_aimed_at_a_stroke_that_is_not_there_is_refused_whole",
    ),
    Mutation(
        # Keep the rectangle the strokes used to occupy. `Stroke::bounds` is
        # called in two places -- when a drawing is made and when it is erased --
        # and this is the second one.
        "docmodel: derive an erased drawing's rectangle from nothing",
        "src/docmodel.rs",
        "        let quads = Stroke::bounds(&strokes, crate::save::INK_WIDTH as f32 / 2.0)",
        "        let quads = Stroke::bounds(&strokes, 1000.0)",
        "erasing_a_stroke_leaves_the_others_and_shrinks_the_rectangle",
    ),
    Mutation(
        # Leave the version behind when the mark goes. A mark removed and
        # restored by undo then comes back at whatever version an erasure left
        # it on rather than the one the journal says.
        "docmodel: let a removed drawing keep the version it was erased to",
        "src/docmodel.rs",
        "                self.inks.remove(&mark);",
        "                let _ = &mark;",
        "a_removed_drawing_forgets_which_version_it_was_on",
    ),
]

#: libtest prints `test <name> ... FAILED` per failure and a `test result:` line.
FAILED_TEST = re.compile(r"^test (\S+) \.\.\. FAILED$", re.M)
SUMMARY = re.compile(r"^test result: \w+\. \d+ passed; (\d+) failed", re.M)
#: `--list` prints `search::tests::a_match_is_found_where_it_is: test`.
LISTED_TEST = re.compile(r"^(\S+): test$", re.M)


def changed_files(ref: str) -> set[str] | None:
    """Repo-relative paths differing from `ref`, working tree included.

    Two questions, because they have different answers and both matter: what
    the commits since `ref` touched, and what is edited right now and not
    committed. A run that read only the first would skip exactly the mutation
    aimed at the code being written.
    """
    out: set[str] = set()
    for cmd in (
        ["git", "diff", "--name-only", f"{ref}...HEAD"],
        ["git", "diff", "--name-only", "HEAD"],
        ["git", "ls-files", "--others", "--exclude-standard"],
    ):
        done = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
        if done.returncode != 0:
            return None
        out |= {line.strip() for line in done.stdout.splitlines() if line.strip()}
    return out


def run_tests(only: str | None = None) -> tuple[set[str], int | None, str]:
    """Runs the suite, returning failed names, the summary count and the log.

    `only` narrows libtest's filter to the one test a mutation names, and that
    is what makes a full table affordable. The twelve tests that reach macOS
    frameworks -- `save.rs` and `print.rs`'s third-parser checks, and
    `keylayout.rs` -- take 17 to 40 s each while the other 595 take 0.6 s
    between them, so running everything for every mutation spends ~35 s to
    exercise one assertion. Measured 2026-08-21.

    The caller runs the full suite anyway whenever the narrow one finds nothing
    red, which is the case where the diagnostic matters: SURVIVED and "something
    else caught it" are different findings and both need the whole set.
    """
    done = subprocess.run(
        ["cargo", "test", "--lib", "--", *([only] if only else FILTERS)],
        cwd=CRATE,
        env=CARGO_ENV,
        capture_output=True,
        text=True,
        # As in `mutate_frontend.py`: `text=True` alone decodes with the locale
        # codec, cp1252 on Windows. `search.rs` holds characters whose UTF-8
        # contains byte 0x81, which is undefined there, and a failing test that
        # echoes one would take the whole harness down mid-run rather than
        # reporting a survivor.
        encoding="utf-8",
        errors="replace",
        timeout=900,
    )
    out = done.stdout + done.stderr
    # Split on the marker and take the name -- never a fixed column.
    names = set(FAILED_TEST.findall(out))
    summary = SUMMARY.search(out)
    counted = int(summary.group(1)) if summary else None
    return names, counted, out


def all_test_names() -> set[str]:
    """Every test the filter selects, from libtest's own listing."""
    done = subprocess.run(
        ["cargo", "test", "--lib", "--", *FILTERS, "--list"],
        cwd=CRATE,
        env=CARGO_ENV,
        capture_output=True,
        text=True,
        timeout=900,
    )
    out = done.stdout + done.stderr
    return {m for m in LISTED_TEST.findall(out)}


# --- cropping a page -----------------------------------------------------
MUTATIONS += [
    Mutation(
        # Count a transparent pixel as ink. The renderer leaves the overhang
        # beyond the page as it found the buffer, so every page comes back
        # uncroppable.
        "content: read the paper beyond the page as ink",
        "src/content.rs",
        "            if px[3] == 0 || (px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE) {",
        "            if px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE {",
        "transparent_pixels_are_paper_too",
    ),
    Mutation(
        # Require every channel to be dark. Red text on white paper is then
        # blank, and a page of coloured type has no content box.
        "content: read only black as ink",
        "src/content.rs",
        "            if px[3] == 0 || (px[0] >= WHITE && px[1] >= WHITE && px[2] >= WHITE) {",
        "            if px[3] == 0 || px[0] >= WHITE || px[1] >= WHITE || px[2] >= WHITE {",
        "a_single_channel_of_colour_is_ink",
    ),
    Mutation(
        # Treat the far edges as inclusive. Every crop is a pixel short on two
        # sides, which at the scan's resolution is three points of ink.
        "content: take the ink bounds as a closed rectangle",
        "src/content.rs",
        "            right = right.max(x + 1);\n            bottom = bottom.max(y + 1);",
        "            right = right.max(x);\n            bottom = bottom.max(y);",
        "one_inked_pixel_bounds_itself_and_nothing_else",
    ),
    Mutation(
        # Read past the end of a buffer smaller than it claims, in the process
        # holding the reader's document.
        "content: trust a buffer's claimed size",
        "src/content.rs",
        "    if pixels.len() < width * height * 4 {\n        return None;\n    }",
        "",
        "a_buffer_smaller_than_it_claims_is_refused",
    ),
    Mutation(
        # A threshold at pure white. Antialiasing puts a near-white fringe around
        # every glyph, so the box is found one pixel outside the ink.
        "content: find ink one pixel outside the ink",
        "src/content.rs",
        "const WHITE: u8 = 240;",
        "const WHITE: u8 = 255;",
        "a_near_white_fringe_is_paper_and_a_grey_is_not",
    ),
    Mutation(
        # No margin. The scan under-reports, so cropping to exactly the measured
        # box shaves descenders.
        "content: crop to exactly the measured ink",
        "src/content.rs",
        "pub const MARGIN_PT: f64 = 2.0;",
        "pub const MARGIN_PT: f64 = 0.0;",
        "the_margin_is_larger_than_a_pixel_of_the_scan",
    ),
    Mutation(
        # Write the crop onto the page's parent, which is inheritable -- so
        # cropping one page crops every page hanging under that node.
        "pagetree: write a crop where every page inherits it",
        "src/pagetree.rs",
        "        doc.get_object_mut(*id)\n            .and_then(Object::as_dict_mut)\n            .map_err(|e| format!(\"page {id:?} is not a dictionary: {e}\"))?\n            .set(\n                \"CropBox\",",
        "        let parent = doc\n            .get_object(*id)\n            .and_then(Object::as_dict)\n            .and_then(|d| d.get(b\"Parent\"))\n            .and_then(Object::as_reference)\n            .unwrap_or(*id);\n        doc.get_object_mut(parent)\n            .and_then(Object::as_dict_mut)\n            .map_err(|e| format!(\"page {id:?} is not a dictionary: {e}\"))?\n            .set(\n                \"CropBox\",",
        "a_crop_lands_on_the_page_and_not_on_its_parent",
    ),
    Mutation(
        # Write a crop that shares no area with the sheet. The page renders as
        # nothing, in whatever reader opens the file.
        "pagetree: write a crop that misses the page",
        "src/pagetree.rs",
        "        if box_pt[2] <= box_pt[0] || box_pt[3] <= box_pt[1] {",
        "        if false {",
        "a_crop_that_misses_the_page_is_refused",
    ),
    Mutation(
        # Skip the intersection with the sheet. A producer's oversized crop box
        # is written straight through.
        "pagetree: write a crop without clamping it to the sheet",
        "src/pagetree.rs",
        "        let box_pt = [\n            want[0].max(media[0]),\n            want[1].max(media[1]),\n            want[2].min(media[2]),\n            want[3].min(media[3]),\n        ];",
        "        let box_pt = *want;",
        "a_crop_outside_the_page_is_brought_back_onto_it",
    ),
    Mutation(
        # Let the last of two crops for one page win, silently, for both.
        "save: let one page be cropped two ways",
        "src/save.rs",
        "        match chosen.get(&page.source) {\n            None => {",
        "        match None::<&([f64; 4], usize)> {\n            None => {",
        "one_page_cropped_two_ways_is_refused_and_cropped_one_way_is_not",
    ),
    Mutation(
        # Write a crop box onto every page of every document tpdf saves.
        "save: plan a crop for a page nobody cropped",
        "src/save.rs",
        "        let Some(want) = page.crop else { continue };",
        "        let want = page.crop.unwrap_or([0.0, 0.0, 1.0, 1.0]);",
        "a_plan_with_no_crop_writes_no_crop_box",
    ),
    Mutation(
        # Drop the crop between the model and the writer. The reader sees a
        # cropped page and every file they save is uncropped.
        "edits: leave the crop out of the plan",
        "src/edits.rs",
        "                crop: page.crop.map(|r| [r.llx, r.lly, r.urx, r.ury]),",
        "                crop: None,",
        "a_crop_reaches_the_reply_and_the_plan_and_clearing_it_removes_it",
    ),
    Mutation(
        # Fill the box instead of stroking it. Every file-level assertion about a
        # /Square -- subtype, rectangle, no quads, an /AP exists -- is satisfied
        # equally by a solid block of colour, and a solid block hides the figure
        # the box was drawn around.
        "save: fill a box rather than stroking its edge",
        "src/save.rs",
        'content.push_str(&format!("{x} {y} {width} {height} re S',
        'content.push_str(&format!("{x} {y} {width} {height} re f',
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Stroke every kind. The check written for the box passes unchanged --
        # which is what its control is for.
        # Anchored on the wash's arm rather than on `re f`, which occurs twice:
        # the anchor gate requires exactly one occurrence, and a mutation that
        # could land in either of two places is one nobody can reason about.
        "save: stroke a wash rather than filling it",
        "src/save.rs",
        '                let (x, y) = (quad[0], quad[1]);\n                let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);\n                content.push_str(&format!("{x} {y} {width} {height} re f',
        '                let (x, y) = (quad[0], quad[1]);\n                let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);\n                content.push_str(&format!("{x} {y} {width} {height} re S',
        "the_wash_and_the_rules_fill_rather_than_stroke",
    ),
    Mutation(
        # Write the text box's words as a UTF-8 literal instead of WinAnsi hex.
        # Every ASCII text box stays perfect and every German one draws `Ã¼` --
        # the encoding defect this would have shipped with, and the reason the
        # writer uses hex at all.
        "save: write a text box's words as UTF-8 rather than WinAnsi",
        "src/save.rs",
        '        let byte = if code <= 0xff { code as u8 } else { b\' \' };\n'
        '        out.push_str(&format!("{byte:02X}"));',
        '        for byte in ch.to_string().into_bytes() {\n'
        '            out.push_str(&format!("{byte:02X}"));\n'
        '        }',
        "a_text_box_draws_its_words_as_winansi_hex_rather_than_a_literal",
    ),
    Mutation(
        # Stop writing /DA. The mark still displays from its /AP everywhere, and
        # becomes uneditable in every reader but this one -- which no rendering
        # check can see, because nothing about the picture changes.
        "save: leave /DA off a text box",
        "src/save.rs",
        "    if mark.kind == MarkKind::TextBox {",
        "    if false {",
        "a_text_box_carries_the_da_the_specification_requires_and_nothing_else_does",
    ),
    Mutation(
        # Give every mark a font in its appearance resources. Harmless-looking,
        # and it is the control for the `/Font` assertion: a check that only ever
        # says "the text box has one" passes equally if everything does.
        "save: put a font in every mark's appearance resources",
        "src/save.rs",
        "    if style == Paint::Text {",
        "    if true {",
        "a_text_box_draws_its_words_as_winansi_hex_rather_than_a_literal",
    ),
    Mutation(
        # Push the empty leftover as a line. The trailing-blank defect a test
        # found while being written for termination: one spurious line in a
        # one-paragraph box, and a line of displacement for every paragraph after
        # the first in a longer one.
        "textbox: push an empty line after a word that was broken",
        "src/textbox.rs",
        "        if !line.is_empty() {\n            lines.push(line);\n        } else if paragraph.is_empty() {",
        "        if !line.is_empty() || !paragraph.is_empty() {\n            lines.push(line);\n        } else if paragraph.is_empty() {",
        "a_width_too_small_for_any_character_still_terminates",
    ),
    Mutation(
        # Never break a word that does not fit. It emits one line wider than the
        # box, the /BBox clips it, and the text disappears at the edge.
        "textbox: let a word wider than the box overflow rather than breaking it",
        "src/textbox.rs",
        "            let mut rest = word;\n            while advance(rest, size) > width {",
        "            let mut rest = word;\n            while false {",
        "a_word_wider_than_the_box_is_broken_rather_than_overflowing",
    ),
    Mutation(
        # Accept anything. A reader pastes Greek, tpdf draws it with a system
        # font on the overlay, and the saved file has substituted glyphs.
        "textbox: accept text Helvetica cannot write",
        "src/textbox.rs",
        # Re-aimed after `cargo fmt` collapsed the closure onto one line -- the
        # anchor did not drift, the formatter moved it, which the `anchors` gate
        # caught in 0.1 s. Run that gate after any `cargo fmt` that touches a
        # file a mutation points into.
        "        .all(|ch| ch == '\\n' || (' '..='~').contains(&ch) || ('\\u{a0}'..='\\u{ff}').contains(&ch))",
        "        .all(|ch| {\n            let _ = ch;\n            true\n        })",
        "what_helvetica_cannot_write_is_refused_and_what_it_can_is_not",
    ),
    Mutation(
        # Draw the squiggle as a flat rule. /Squiggly still goes in the file, so
        # every reader files it as a squiggle and draws an underline -- and the
        # subtype test passes it. The mirror of the mutation below it.
        "save: draw a squiggle as a flat rule",
        "src/save.rs",
        "        MarkKind::Squiggly => Paint::Wave,",
        "        MarkKind::Squiggly => Paint::Line,",
        "a_squiggle_is_a_stroked_zigzag_in_a_band_taller_than_a_rule",
    ),
    Mutation(
        # Write the underline's subtype for a squiggle. Our own /AP draws the
        # right wave, so the mark looks correct in tpdf and is an underline to
        # everything else.
        "save: write /Underline for a squiggle",
        "src/save.rs",
        '        MarkKind::Squiggly => b"Squiggly",',
        '        MarkKind::Squiggly => b"Underline",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Give the squiggle the underline's band. The wave is then drawn inside a
        # rule's height -- a 7%-tall zigzag, which at body size is a fuzzy line
        # and reads as bad antialiasing rather than as the wrong mark. It also
        # closes the strip every discriminating check reads.
        "save: fit a squiggle into an underline's band",
        "src/save.rs",
        "        MarkKind::Squiggly => (bottom, full * SQUIGGLE_HEIGHT),",
        "        MarkKind::Squiggly => (bottom, thickness),",
        "a_squiggle_is_a_stroked_zigzag_in_a_band_taller_than_a_rule",
    ),
    Mutation(
        # Drop the squiggle out of the quad-carrying kinds. It is a text-markup
        # subtype and the specification lists /QuadPoints on it, so a reader that
        # positions from quads finds none and falls back to /Rect -- which is the
        # union of the run and is wrong the moment a mark spans two lines.
        "save: stop writing quads for a squiggle",
        "src/save.rs",
        "        MarkKind::Highlight | MarkKind::Underline | MarkKind::Squiggly | MarkKind::StrikeOut",
        "        MarkKind::Highlight | MarkKind::Underline | MarkKind::StrikeOut",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Draw the ellipse as a rectangle. `/Circle` still goes in the file, so
        # every reader files it under "ellipse" and every one of them draws a
        # box -- the subtype and the appearance disagreeing, which is the one
        # thing neither the subtype test nor a pixel count of the whole quad can
        # see on its own.
        "save: draw an ellipse with the box's rectangle",
        "src/save.rs",
        "        MarkKind::Ellipse => Paint::Ellipse,",
        "        MarkKind::Ellipse => Paint::Outline,",
        "an_ellipse_is_drawn_as_four_curves_and_not_as_a_rectangle",
    ),
    Mutation(
        # Write the box's subtype for an ellipse. Our own /AP draws the right
        # curve, so the mark looks correct in tpdf and is a rectangle to
        # everything else -- the mirror of the mutation above, and the reason
        # the two tests are separate.
        "save: write /Square for an ellipse",
        "src/save.rs",
        '        MarkKind::Ellipse => b"Circle",',
        '        MarkKind::Ellipse => b"Square",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Leave the ellipse's path open. The fourth arc ends where the first
        # began, so the shape is right and the join at three o'clock is a cap
        # instead -- a nick in a thick stroke rather than a missing curve.
        "save: leave an ellipse's path open rather than closing it",
        "src/save.rs",
        '                content.push_str("h S',
        '                content.push_str("S',
        "an_ellipse_is_drawn_as_four_curves_and_not_as_a_rectangle",
    ),
    Mutation(
        # Stroke on the quad's own edge. Half of every side falls outside the
        # appearance stream's /BBox, which clips it -- so the box comes out with
        # hairline edges, which looks like a thin border and not like a bug.
        "save: stroke a box on its edge rather than inset by half the stroke",
        "src/save.rs",
        "    let inset = OUTLINE_WIDTH / 2.0;",
        "    let inset = 0.0;",
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Never set the stroke colour. `rg` sets the fill's and does not imply
        # `RG`, so the box comes out black -- which reads as a colour that was
        # ignored rather than one that was never set.
        "save: set only the fill colour, so the stroke comes out black",
        "src/save.rs",
        "{r} {g} {b} rg {r} {g} {b} RG {width} w {joins}",
        "{r} {g} {b} rg {width} w {joins}",
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Write /QuadPoints on every kind, as the writer did before a mark
        # existed that must not carry them. Most readers ignore an unlisted key
        # and one day something does not.
        "save: write text-markup quads on a box and a comment too",
        "src/save.rs",
        "    if is_text_markup(mark.kind) {",
        "    if true {",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Give a box no appearance stream, as a comment correctly gets none.
        # Nothing synthesises a rectangle, so the annotation is in the file,
        # findable, removable -- and invisible.
        "save: leave a box's appearance to the reader, as a comment's is",
        "src/save.rs",
        "        let appearance = if paint(mark.kind) == Paint::None {",
        "        let appearance = if !is_text_markup(mark.kind) {",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Keep a colour version whose command was discarded with the redo tail.
        # Invisible in every document the model can produce -- which is exactly
        # why the accounting observable exists, and exactly how its twin below
        # leaked for a week with nothing reading it.
        "docmodel: keep a colour after the command naming it is discarded",
        "src/docmodel.rs",
        "                Command::Recolor { color, .. } => {\n"
        "                    self.colors.remove(&color);\n"
        "                }",
        "                Command::Recolor { .. } => {}",
        "a_colour_in_the_discarded_redo_tail_goes_with_it",
    ),
    Mutation(
        # The same for the eraser's table, and this one is not hypothetical: the
        # arm did not exist until the colour work went in, and a reader erasing
        # and undoing in a loop grew `inks` forever.
        "docmodel: keep a drawing after the command naming it is discarded",
        "src/docmodel.rs",
        "                Command::Reink { ink, .. } => {\n"
        "                    self.inks.remove(&ink);\n"
        "                }",
        "                Command::Reink { .. } => {}",
        "a_drawing_in_the_discarded_redo_tail_goes_with_it",
    ),
    Mutation(
        # Leave a removed mark pointing at a colour version. Undo then brings the
        # mark back in whatever colour it had been recoloured to rather than the
        # one the journal says it was made in.
        "docmodel: leave a removed mark's colour behind",
        "src/docmodel.rs",
        "                self.inks.remove(&mark);\n                self.colors.remove(&mark);",
        "                self.inks.remove(&mark);",
        "a_removed_mark_forgets_which_colour_it_was_on",
    ),
    Mutation(
        # Answer a mark's colour out of its body. Everything still works and
        # every recolour is silently ignored -- on screen and in the file, since
        # both readers go through this one accessor.
        "docmodel: answer a mark's colour out of its body",
        "src/docmodel.rs",
        "        self.now\n            .color_of(mark)\n            .and_then(|color| self.colors.get(&color))",
        "        None.and_then(|color: ColorId| self.colors.get(&color))",
        "recolouring_changes_what_a_mark_is_drawn_in_and_undo_puts_it_back",
    ),
    Mutation(
        # Spend the id before checking the mark. A refused recolour then leaks a
        # colour version per press, which no document can show.
        "docmodel: spend a colour id before checking the mark is there",
        "src/docmodel.rs",
        "        self.now.live_mark(mark)?;\n        let color = self.issue_color(color);",
        "        let color = self.issue_color(color);\n        self.now.live_mark(mark)?;",
        "recolouring_a_mark_that_is_not_there_is_refused_before_an_id_is_spent",
    ),
    Mutation(
        # Reply with the colour the mark was made in. The overlay paints from
        # this field, so a recolour would be accepted, journalled, saved -- and
        # invisible until the file was reopened, which is the shape of wrong the
        # overlay work was done to end.
        "edits: reply with the colour the mark was made in",
        "src/edits.rs",
        "                color: model.color_of(id),",
        "                color: mark.color,",
        "the_reply_carries_the_colour_the_mark_has_now",
    ),
    Mutation(
        # And the writer's half, which the reply's test cannot see: the mark
        # looks right on screen and saves in its first colour.
        "edits: write the colour the mark was made in",
        "src/edits.rs",
        "                    color: model.color_of(*mark),",
        "                    color: body.color,",
        "a_saved_file_is_written_in_the_colour_the_mark_has_now",
    ),
    Mutation(
        # Let a recolour past the clamp. `1e40` is valid JSON, becomes `inf` in
        # an `f32`, and `format!` writes the three letters into a content stream
        # -- a file no reader can parse, through the command that only changes an
        # appearance.
        "edits: let a recolour past the clamp a new mark goes through",
        "src/edits.rs",
        "            .recolor(MarkId::from_raw(mark), color.map(channel))",
        "            .recolor(MarkId::from_raw(mark), color)",
        "a_colour_that_is_not_a_number_is_clamped_at_this_door_too",
    ),
]

# --- what a document says about itself ------------------------------------
MUTATIONS += [
    Mutation(
        # Read the encryption *after* decrypting, which reading the code alone
        # suggests is fine. `lopdf::decrypt` removes the trailer entry and the
        # object it points at, so the same call then answers "not encrypted" for
        # a document that plainly is, and every permission goes with it.
        "docinfo: ask about the encryption after decrypting rather than before",
        "src/docinfo.rs",
        "    let encryption = read_encryption(&document);",
        "    let encryption: Option<Encryption> = None;",
        "a_document_that_needs_a_password_says_so_rather_than_reporting_nothing",
    ),
    Mutation(
        # Report a locked document's structure tree as absent. "No" and "could
        # not look" are different claims and only one is true here; the `Option`
        # is what carries the difference.
        "docinfo: report a locked document as untagged rather than unknown",
        "src/docinfo.rs",
        '        tagged: catalog.map(|c| c.has(b"StructTreeRoot")),',
        '        tagged: Some(catalog.is_some_and(|c| c.has(b"StructTreeRoot"))),',
        "a_document_that_needs_a_password_says_so_rather_than_reporting_nothing",
    ),
    Mutation(
        # Ignore the revision, so bits 9 to 12 are read as permissions on a
        # revision-2 document. All four are set in `P = -60` because the number
        # is negative, so this reports a document forbidding accessibility
        # extraction as permitting it.
        "docinfo: read the reserved permission bits whatever the revision",
        "src/docinfo.rs",
        "    let old = revision < 3;",
        "    let old = false;",
        "revision_2_reads_the_reserved_bits_as_the_coarser_ones_they_stand_for",
    ),
    Mutation(
        # Take any `/Reference` entry's level. A `/FieldMDP` transform's `/P` is
        # a field-locking mode, not a certification level, so this reports a
        # signature locking one form field as certifying the whole document.
        "docinfo: take any transform's level, not the DocMDP one",
        "src/docinfo.rs",
        '        if name_of(document, reference, b"TransformMethod") != "DocMDP" {',
        "        if false {",
        "the_certification_level_comes_from_the_docmdp_reference_and_no_other",
    ),
    Mutation(
        # Check only where the signed range ends. A range starting past zero
        # leaves the file's head unsigned and still ends where the file does ---
        # the half of the check that reading the code calls redundant.
        "docinfo: check only where the signed range ends, not where it starts",
        "src/docinfo.rs",
        "        }) && numbers.first() == Some(&0);",
        "        });",
        "a_range_that_skips_the_start_of_the_file_is_not_whole_coverage",
    ),
    Mutation(
        # Sum every number in `/ByteRange`. Offsets are not lengths, so this
        # reports a signature as covering far more than it does --- wrong in the
        # direction that reassures.
        "docinfo: add up every byte-range number, offsets included",
        "src/docinfo.rs",
        "            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())",
        "            .map(|pair| u64::try_from(pair[0] + pair[1]).unwrap_or_default())",
        "a_signature_covering_the_file_says_so_and_one_that_does_not_says_that",
    ),
    Mutation(
        # Let the catalog's `/Version` win whatever it says. It exists so an
        # incremental update can *raise* the version, never lower it.
        "docinfo: let the catalog's version win even when it is earlier",
        "src/docinfo.rs",
        "    if catalog > header {",
        "    if !catalog.is_empty() {",
        "the_later_of_the_header_and_the_catalog_version_wins",
    ),
    Mutation(
        # Count a name tree's entries rather than its pairs. `/Names` interleaves
        # key and value, so this reports twice as many attachments as there are.
        "docinfo: count a name tree's entries rather than its pairs",
        "src/docinfo.rs",
        "        .map_or(0, |array| array.len() / 2)",
        "        .map_or(0, std::vec::Vec::len)",
        "attachments_are_counted_as_pairs_because_a_name_tree_interleaves_them",
    ),
    Mutation(
        # Shorten a value and say nothing. The readout then looks whole while a
        # property is silently truncated, which is the failure every `Limits`
        # field here exists to prevent.
        "docinfo: clip a value without counting that anything was clipped",
        "src/docinfo.rs",
        "    limits.values_clipped += 1;",
        "    limits.values_clipped += 0;",
        "a_value_that_would_fill_the_dialog_is_clipped_and_counted",
    ),
    Mutation(
        # Read the four claimed strings off the *field* rather than off the
        # signature. They live in `/V`, so every one comes back empty and the
        # dialog shows a signature nobody signed with no reason and no date.
        "docinfo: read a signature's claims off the field, not the signature",
        "src/docinfo.rs",
        '    out.reason = text(sig, b"Reason");',
        '    out.reason = text(field, b"Reason");',
        "what_the_signer_says_is_carried_through_unchanged",
    ),
]


# --- the certificate a signature was made with ----------------------------
MUTATIONS += [
    Mutation(
        # Take the first certificate in the set instead of the one the signer
        # actually points at. The set is *unordered* and normally carries the
        # chain, so on any real document this names a certificate authority as
        # the signer roughly as often as it names the signer. Every fixture here
        # has a chain of one, which is why the check that catches this is the
        # `matched_signer` assertion and not the name.
        "docinfo: take the first certificate rather than the signer's",
        "src/docinfo.rs",
        """    let (certificate, matched_signer) = match (matched, certificates.as_slice()) {
        (Some(certificate), _) => (certificate, true),
        (None, [only]) => (*only, false),
        (None, _) => return None,
    };""",
        """    let (certificate, matched_signer) = (certificates[0], false);""",
        "each_signed_fixture_carries_its_own_certificate",
    ),
    Mutation(
        # Report a blob that could not be parsed as a document with no
        # certificate. The two readings are opposite --- absent is about the
        # file, unread is about tpdf --- and this is the direction that makes
        # tpdf's own failure look like the document's silence.
        "docinfo: report an unreadable certificate as an absent one",
        "src/docinfo.rs",
        """        None => {
            limits.certificates_unread += 1;
            None
        }""",
        """        None => None,""",
        "a_blob_that_is_not_der_is_reported_as_unread_rather_than_absent",
    ),
    Mutation(
        # The mirror: count an all-zero placeholder as a failure. A signature is
        # written by reserving a span and filling it, so this would put a "could
        # not read" notice on a document with nothing wrong with it.
        "docinfo: treat a reserved-but-empty placeholder as a parse failure",
        "src/docinfo.rs",
        "    let last = raw.iter().rposition(|b| *b != 0)?;",
        """    let Some(last) = raw.iter().rposition(|b| *b != 0) else {
        *unread += 1;
        return None;
    };""",
        "an_untouched_placeholder_is_absent_rather_than_unread",
    ),
    Mutation(
        # Hand the padded blob to the decoder. The trailing zeros are not DER and
        # a decoder is entitled to reject them, so every certificate would read
        # as unparseable --- the failure that looks like a broken dependency.
        "docinfo: parse the blob without stripping its reserved padding",
        "src/docinfo.rs",
        "    let trimmed = &raw[..=last];",
        "    let trimmed = &raw[..];",
        "each_signed_fixture_carries_its_own_certificate",
    ),
    Mutation(
        # Drop the size bound, so an attacker-chosen megabyte of nested DER goes
        # to the parser. The blob is the most attacker-controlled thing in the
        # document and this is the only thing standing between it and a parser.
        "docinfo: parse a certificate blob of any size",
        "src/docinfo.rs",
        """    if trimmed.len() > bound {
        *unread += 1;
        return None;
    }""",
        "",
        "a_bound_a_valid_blob_exceeds_refuses_it_rather_than_parsing_it",
    ),
    Mutation(
        # Compare issuers by their common name rather than by their encoded
        # bytes. Two different authorities may use the same CN, and a DN is
        # equal by encoding, not by the one attribute a person reads.
        "docinfo: match a signer by common name rather than by encoded issuer",
        "src/docinfo.rs",
        """                certificate.tbs_certificate.serial_number == both.serial_number
                    && certificate.tbs_certificate.issuer.to_der().ok() == both.issuer.to_der().ok()""",
        """                common_name(&certificate.tbs_certificate.issuer)
                    == common_name(&both.issuer)""",
        "a_certificate_from_the_right_issuer_but_the_wrong_serial_is_not_the_signer",
    ),
    Mutation(
        # Call every certificate self-issued. `self_issued` is one of exactly two
        # unhedged statements the signature section makes, so a wrong one is a
        # confident false claim about who vouched for a signer.
        "docinfo: call every certificate self-issued",
        "src/docinfo.rs",
        "        self_issued: tbs.subject.to_der().ok() == tbs.issuer.to_der().ok(),",
        "        self_issued: true,",
        "a_certificate_somebody_else_issued_is_not_called_self_issued",
    ),
    Mutation(
        # List the signature fields in whatever order the queue pops them, which
        # is the reverse of document order. Every fact about every signature is
        # still correct; only which signature each belongs to is wrong, so a
        # reader is shown the right names against the wrong fields. Invisible on
        # any one-signature fixture, which was every signed fixture until
        # `incr-two-signers.pdf`.
        "docinfo: list the signature fields in the reverse of document order",
        "src/docinfo.rs",
        """    // Reversed so the queue pops in document order, which is the order a reader
    // sees the fields in every other application.
    queue.reverse();""",
        "",
        "two_signers_are_told_apart_and_neither_is_reported_as_the_authority",
    ),
    Mutation(
        # Take the leaf's issuer as the signer's own name. Both leaves here are
        # issued by one root, so this reports "tpdf test root CA" for both
        # signatures --- the certificate-authority-as-signer mistake, which is
        # what the whole `SignerInfo.sid` walk exists to avoid.
        "docinfo: report the issuer's name as the subject's",
        "src/docinfo.rs",
        "        subject_cn: common_name(&tbs.subject),",
        "        subject_cn: common_name(&tbs.issuer),",
        "two_signers_are_told_apart_and_neither_is_reported_as_the_authority",
    ),
    Mutation(
        # Stop recursing into `/Kids`, so a signature field grouped under a node
        # is walked straight past. The document then reports no signature at all,
        # which is indistinguishable from one that has none -- and PDFium does
        # exactly this, so the differential cannot catch it either.
        "docinfo: do not walk into a field node's kids",
        "src/docinfo.rs",
        """            if depth < 8 {
                for kid in kids.iter().rev() {
                    queue.push((kid, depth + 1, name.clone()));
                }
            } else {""",
        """            if false {
                for kid in kids.iter().rev() {
                    queue.push((kid, depth + 1, name.clone()));
                }
            } else {""",
        "a_signature_field_under_kids_is_found_rather_than_walked_past",
    ),
    Mutation(
        # Report the leaf's own `/T` rather than the fully qualified name. Every
        # fixture but one has unique leaf names, so this is right about all of
        # them and wrong about what it means: `/T` is unique among siblings only.
        "docinfo: name a field by its own /T alone",
        "src/docinfo.rs",
        "        let name = qualified_name(&prefix, &text_of(document, field, b\"T\"));",
        "        let name = text_of(document, field, b\"T\");",
        "two_fields_with_the_same_leaf_name_are_told_apart_by_the_groups_above_them",
    ),
    Mutation(
        # Give an unnamed node a level of the name anyway, which puts an empty
        # component in the middle -- `top..Signature1`, a string no other reader
        # shows -- and gives a wholly unnamed chain the name ".".
        "docinfo: make an unnamed field node a level of the name",
        "src/docinfo.rs",
        "        (_, true) => prefix.to_string(),",
        "        (_, true) => format!(\"{prefix}.\"),",
        "a_node_with_no_name_of_its_own_is_not_a_level_of_the_name",
    ),
    Mutation(
        # Hand each kid an empty prefix instead of its parent's name, so the
        # ancestry is walked and then thrown away one level at a time.
        "docinfo: forget the ancestry when descending into kids",
        "src/docinfo.rs",
        "                    queue.push((kid, depth + 1, name.clone()));",
        "                    queue.push((kid, depth + 1, String::new()));",
        "a_signature_field_under_kids_is_found_rather_than_walked_past",
    ),
    Mutation(
        # Report an extension that would not decode as an absent one. The two
        # are opposite claims -- absent places no limit on the key, malformed
        # places an unknown one -- and absent is the reassuring branch.
        "docinfo: read a malformed extension as an absent one",
        "src/docinfo.rs",
        """        Err(_) => {
            *unread += 1;
            None
        }""",
        "        Err(_) => None,",
        "an_extension_that_will_not_decode_is_counted_rather_than_read_as_absent",
    ),
    Mutation(
        # Swap the first two key usage bits. This SURVIVED when it was aimed at
        # the fixture test, and correctly: a real signing certificate sets both
        # bits, so permuting two selected entries yields the same list. The test
        # it names now sets one bit at a time, which is the only shape that can
        # tell a table from a permutation of it.
        "docinfo: name the first two key usage bits the wrong way round",
        "src/docinfo.rs",
        """        (KeyUsages::DigitalSignature, "Digital signature"),
        (KeyUsages::NonRepudiation, "Non-repudiation"),""",
        """        (KeyUsages::NonRepudiation, "Digital signature"),
        (KeyUsages::DigitalSignature, "Non-repudiation"),""",
        "each_key_usage_bit_is_named_by_the_name_rfc_5280_gives_it",
    ),
    Mutation(
        # Reorder two whole rows, keeping every bit paired with its own name.
        # The single-bit half of that test cannot see this -- each of its
        # assertions is over a one-element list -- so it is the all-bits control
        # beneath them that catches it, which is what that control is for.
        "docinfo: list the key usage bits out of specification order",
        "src/docinfo.rs",
        """        (KeyUsages::KeyEncipherment, "Key encipherment"),
        (KeyUsages::DataEncipherment, "Data encipherment"),""",
        """        (KeyUsages::DataEncipherment, "Data encipherment"),
        (KeyUsages::KeyEncipherment, "Key encipherment"),""",
        "each_key_usage_bit_is_named_by_the_name_rfc_5280_gives_it",
    ),
    Mutation(
        # Read only child elements, not attributes. Three of the eight claims in
        # a 41-document corpus are written as attributes, and a document that
        # claims nothing is the overwhelming majority -- so the silence this
        # produces looks exactly like an ordinary file.
        "xmp: read a conformance claim only when it is a child element",
        "src/xmp.rs",
        """                read_attributes(
                    &reader,
                    &element,
                    &mut out,
                    &mut pdfa_part,
                    &mut pdfa_conformance,
                );
                stack.push(property);""",
        "                stack.push(property);",
        "a_claim_written_as_attributes_is_read_as_the_same_claim",
    ),
    Mutation(
        # Match the conventional prefix instead of the namespace URI. Right
        # about every document a producer wrote conventionally, wrong in both
        # directions on the ones that did not.
        "xmp: identify a property by its prefix rather than its namespace",
        "src/xmp.rs",
        '        (NS_PDFAID, "part") => Some(Property::PdfaPart),',
        '        (_, "part") => Some(Property::PdfaPart),',
        "a_claim_is_identified_by_its_namespace_and_not_by_its_prefix",
    ),
    Mutation(
        # Take the first text event as the whole value. Correct for every value
        # with no entity in it, which is most of them, and silently truncating
        # for the rest.
        "xmp: take the first fragment of a value as the whole value",
        "src/xmp.rs",
        """                if let Some((_, _, buffer)) = &mut pending {
                    append(buffer, &value, &mut out.unread);
                }
            }
            Ok(Event::GeneralRef(reference)) => {""",
        """                if let Some((_, _, buffer)) = &mut pending {
                    if buffer.is_empty() {
                        append(buffer, &value, &mut out.unread);
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {""",
        "a_value_arriving_in_pieces_is_put_back_together",
    ),
    Mutation(
        # Resolve an entity this module does not know, which is the shape of the
        # attack the refusal exists to stop. The point is not that a resolver
        # would expand it -- it is that the refusing branch is load-bearing
        # rather than decoration.
        "xmp: resolve an entity this module does not know",
        "src/xmp.rs",
        """                let Ok(resolved) = quick_xml::escape::unescape(&spelled) else {
                    out.unread = true;
                    continue;
                };""",
        """                let resolved = quick_xml::escape::unescape(&spelled)
                    .unwrap_or(std::borrow::Cow::Borrowed("lol"));""",
        "a_billion_laughs_packet_is_neither_expanded_nor_followed",
    ),
    Mutation(
        # Drop the nesting bound, so a chain a document chose the depth of is
        # followed to the end.
        "xmp: follow a packet's nesting to whatever depth it claims",
        "src/xmp.rs",
        "                if stack.len() >= MAX_DEPTH {",
        "                if stack.len() >= usize::MAX {",
        "nesting_past_the_bound_stops_and_is_reported",
    ),
    Mutation(
        # Abandon an oversized packet in silence, so it reads as a document that
        # says nothing about itself.
        "xmp: drop an oversized packet without saying so",
        "src/xmp.rs",
        """    if packet.len() > MAX_PACKET {
        out.unread = true;
        return out;
    }""",
        """    if packet.len() > MAX_PACKET {
        return out;
    }""",
        "a_packet_larger_than_the_cap_is_reported_rather_than_dropped",
    ),
    Mutation(
        # Bound the finished string rather than the accumulation, so a document
        # can make this hold whatever it likes and then clip it.
        "xmp: let a value grow without bound and clip it at the end",
        "src/xmp.rs",
        """    if buffer.len() >= MAX_VALUE {
        *unread = true;
        return;
    }""",
        "",
        "a_value_assembled_past_the_cap_stops_accumulating_and_says_so",
    ),
    Mutation(
        # Report a malformed packet as a document that claims nothing, dropping
        # what was read before the damage as well as the notice.
        "xmp: read a malformed packet as a document claiming nothing",
        "src/xmp.rs",
        """            Err(_) => {
                out.unread = true;
                break;
            }""",
        """            Err(_) => {
                out = Xmp {
                    bytes: out.bytes,
                    ..Xmp::default()
                };
                break;
            }""",
        "a_packet_that_stops_making_sense_keeps_what_it_had_and_says_so",
    ),
    Mutation(
        # Require a conformance letter beside the part. PDF/A-4 dropped it, so
        # this reports nothing for the newest standard in the family.
        "xmp: require a conformance letter beside a PDF/A part",
        "src/xmp.rs",
        "    if !pdfa_part.is_empty() {",
        "    if !pdfa_part.is_empty() && !pdfa_conformance.is_empty() {",
        "a_part_with_no_conformance_letter_is_still_a_claim",
    ),
    Mutation(
        # Stop consulting the catalog's /Metadata, so every document reports no
        # packet -- which is true of most of them.
        "docinfo: report every document as carrying no metadata packet",
        "src/docinfo.rs",
        "        xmp: catalog.and_then(|c| read_xmp(&document, c)),",
        "        xmp: catalog.and_then(|_| None),",
        "a_conformance_claim_in_the_metadata_stream_reaches_the_readout",
    ),
    Mutation(
        # Read only unfiltered streams. The specification prefers those and most
        # producers write them, so this is right about the common case and blind
        # to Acrobat's.
        "docinfo: read a metadata stream only when it carries no filter",
        "src/docinfo.rs",
        """    let packet = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());""",
        "    let packet = stream.content.clone();",
        "a_conformance_claim_in_the_metadata_stream_reaches_the_readout",
    ),
    Mutation(
        # Read any CMS's encapsulated content as a TSTInfo. Every ordinary
        # signature then yields a "timestamp" made of whatever a
        # GeneralizedTime can be built from -- a plausible number attributed to
        # an authority, which is the worst outcome this module has.
        "docinfo: read any CMS content as a timestamp",
        "src/docinfo.rs",
        """    if signed.encap_content_info.econtent_type.to_string() != "1.2.840.113549.1.9.16.1.4" {
        return None;
    }""",
        "",
        "a_token_relabelled_as_something_else_is_not_read_as_a_timestamp",
    ),
    Mutation(
        # Read the fourth field as genTime. Off by one in a positional skip,
        # which is the failure the positional skip has to be bounded against.
        "docinfo: take the field before genTime as the attested time",
        "src/docinfo.rs",
        "    for _ in 0..4 {",
        "    for _ in 0..3 {",
        "the_time_a_timestamp_authority_attested_is_read",
    ),
    Mutation(
        # Drop the tag check, so a TSTInfo that is not a SEQUENCE has its value
        # walked as if it were one.
        "docinfo: read a TSTInfo without checking it is a sequence",
        "src/docinfo.rs",
        """    if sequence.tag() != der::Tag::Sequence {
        return None;
    }""",
        "",
        "a_tst_info_that_is_not_shaped_like_one_yields_no_time",
    ),
    Mutation(
        # Take the first of several values on a timestamp attribute. There is
        # nothing to choose between them, so this is a guess presented as an
        # authority's statement.
        "docinfo: guess when a timestamp attribute carries several values",
        "src/docinfo.rs",
        """    let [value] = attribute.values.as_slice() else {
        *unread += 1;
        return None;
    };""",
        """    let Some(value) = attribute.values.as_slice().first() else {
        *unread += 1;
        return None;
    };""",
        "a_timestamp_attribute_carrying_more_than_one_value_is_refused",
    ),
    Mutation(
        # Look for the timestamp in the SIGNED attributes. A token is minted
        # after the signature exists, so it cannot be inside what the signature
        # covers -- and every real signature would then report none.
        "docinfo: look for the timestamp among the signed attributes",
        "src/docinfo.rs",
        """    let attribute = signer
        .unsigned_attrs""",
        """    let attribute = signer
        .signed_attrs""",
        "the_time_a_timestamp_authority_attested_is_read",
    ),
    Mutation(
        # Report a token that would not parse as a signature nobody timestamped,
        # which is the ordinary case and therefore the reassuring one.
        "docinfo: read an unreadable timestamp token as an absent one",
        "src/docinfo.rs",
        """    match parse_timestamp_token(&token) {
        Some(timestamp) => Some(timestamp),
        None => {
            *unread += 1;
            None
        }
    }""",
        "    parse_timestamp_token(&token)",
        "a_token_that_will_not_parse_is_counted_rather_than_read_as_absent",
    ),
    Mutation(
        # Report a key usage naming nothing as no extension at all, which turns
        # "this key is for nothing" into "no limit was placed on this key".
        "docinfo: read an empty key usage as an absent one",
        "src/docinfo.rs",
        """    Some(
        named
            .into_iter()
            .filter(|(bit, _)| usage.0.contains(*bit))
            .map(|(_, name)| name.to_string())
            .collect(),
    )""",
        """    let list: Vec<String> = named
        .into_iter()
        .filter(|(bit, _)| usage.0.contains(*bit))
        .map(|(_, name)| name.to_string())
        .collect();
    (!list.is_empty()).then_some(list)""",
        "a_certificate_stating_no_usage_and_one_stating_none_are_told_apart",
    ),
    Mutation(
        # Drop an extended key usage OID this module cannot name. The reader is
        # then shown the purposes we happen to know and told nothing about the
        # rest, which reads as the issuer having named only those.
        "docinfo: drop an extended key usage nobody here can name",
        "src/docinfo.rs",
        "        other => return other.to_string(),",
        "        _ => \"\",",
        "an_extended_usage_this_module_cannot_name_is_shown_as_its_oid",
    ),
    Mutation(
        # Report an absent basic constraints as a stated CA:FALSE. RFC 5280
        # reads it that way for chain building, and it is still the certificate
        # not saying something rather than saying it.
        "docinfo: read absent basic constraints as a stated CA:FALSE",
        "src/docinfo.rs",
        """    let constraints: BasicConstraints = decode_extension(extensions, "2.5.29.19", unread)?;
    Some(constraints.ca)""",
        """    let constraints: Option<BasicConstraints> =
        decode_extension(extensions, "2.5.29.19", unread);
    Some(constraints.is_some_and(|constraints| constraints.ca))""",
        "a_certificate_stating_no_usage_and_one_stating_none_are_told_apart",
    ),
    Mutation(
        # Drop the depth bound, so a `/Kids` chain a document chose the length of
        # is followed to the end. The queue is the only thing between a hostile
        # field tree and however long it cares to be.
        "docinfo: follow a field tree to whatever depth it claims",
        "src/docinfo.rs",
        "            if depth < 8 {",
        "            if depth < usize::MAX as u32 {",
        "a_field_tree_is_walked_to_a_bounded_depth_and_the_refusal_is_counted",
    ),
    Mutation(
        # Refuse a too-deep tree in silence. A signature dropped without a word
        # reads as a document that has none, which is the reassuring direction.
        "docinfo: drop a too-deep field node without counting it",
        "src/docinfo.rs",
        """            } else {
                limits.unreadable += 1;
            }""",
        """            }""",
        "a_field_tree_is_walked_to_a_bounded_depth_and_the_refusal_is_counted",
    ),
    Mutation(
        # Read a BMPString as bytes. It comes out as text interleaved with nulls,
        # which reads as a mangled name rather than as a decoding bug --- so this
        # is the version that ships looking merely ugly.
        "docinfo: read a UTF-16 name as bytes",
        "src/docinfo.rs",
        "    let text = if attribute.value.tag().number().value() == 0x1e {",
        "    let text = if false {",
        "a_utf16_name_is_decoded_rather_than_shown_with_nulls",
    ),
    Mutation(
        # Print the serial with the bytes reversed. Endianness is invisible in a
        # hex string --- it looks exactly as much like a serial either way --- so
        # only a comparison against another tool's answer can see it.
        "docinfo: print a serial least-significant byte first",
        "src/docinfo.rs",
        "    for byte in bytes.iter().take(MAX_VALUE_CHARS / 2) {",
        "    for byte in bytes.iter().rev().take(MAX_VALUE_CHARS / 2) {",
        "every_field_of_a_certificate_whose_bytes_the_test_chose",
    ),
    Mutation(
        # Report `notBefore` as `notAfter`. Both are dates in the same format on
        # adjacent lines, which is what makes it survive a reading.
        "docinfo: report a certificate's validity from one end only",
        "src/docinfo.rs",
        "        until: certificate_date(&tbs.validity.not_after),",
        "        until: certificate_date(&tbs.validity.not_before),",
        "every_field_of_a_certificate_whose_bytes_the_test_chose",
    ),
]


def main() -> int:
    # Before anything prints. A redirected run is block-buffered otherwise, and
    # this harness takes the better part of an hour: on 2026-08-19 a full run's
    # output sat at three lines for forty minutes and its verdict was lost
    # entirely when the run was interrupted, which is the exact ambiguity
    # `live_output` exists to remove. The three window harnesses had the fix and
    # the three mutation harnesses did not.
    stream_results()
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true")
    # Same flag, same meaning as `mutate_viewer.py`'s. Added when the page
    # deletion work put twenty new mutations into a table of a hundred: the
    # whole table is what runs before a push, and re-proving ninety-odd
    # mutations that could not have moved is an hour of somebody waiting.
    # The control run and the name cross-check below still run in full.
    parser.add_argument(
        "--only", default="", help="run mutations whose name contains this"
    )
    # For the loop while a change is being made, where `--only` needs you to
    # know the mutation names and this needs you to know nothing. It selects the
    # mutations whose FILE the diff touched, which is sound as far as it goes:
    # every test these name is `#[cfg(test)]` in the file it mutates.
    #
    # **It does not go as far as the whole table, and the difference is not
    # scope but reach.** A change in `docmodel.rs` can decide what `save.rs`
    # does, so a mutation in an untouched file can stop being caught without
    # that file changing. This is the fast loop; the table is what runs before a
    # push, and the count of what it left out is printed rather than implied.
    parser.add_argument(
        "--since",
        default="",
        metavar="REF",
        help="only mutations in files changed since REF (plus the working tree)",
    )
    args = parser.parse_args()
    chosen = [m for m in MUTATIONS if args.only.lower() in m.name.lower()]
    if args.since:
        touched = changed_files(args.since)
        if touched is None:
            print(f"[FAIL] git could not diff against {args.since!r}")
            return 1
        scoped = [m for m in chosen if str(Path("src-tauri") / m.path) in touched]
        left = len(chosen) - len(scoped)
        rust = sorted(f for f in touched if f.endswith(".rs"))
        print(f"--- --since {args.since}: {len(rust)} Rust file(s) changed, "
              f"{len(scoped)} mutation(s) aimed at them, {left} NOT run")
        print("    a change elsewhere can still break a mutation in a file this "
              "missed --- run the whole table before pushing")
        chosen = scoped
    # Mutations for another platform. `--list` still shows them, marked: the
    # table is the same everywhere and what differs is which rows this machine
    # can execute, so a listing that silently dropped them would make two
    # machines look like they disagree about the table.
    elsewhere = [m for m in chosen if m.only_on and m.only_on != HERE]

    if args.list:
        for mutation in chosen:
            aside = f"   [{mutation.only_on} only]" if mutation in elsewhere else ""
            print(f"{mutation.name}  ->  expects: {mutation.expect}{aside}")
        return 0
    if not chosen:
        # Not exit 0. "Nothing to run" and "everything passed" are different
        # facts, and a caller reading only the status must not be told the
        # second when this is the first.
        if args.since:
            print(
                f"[FAIL] no mutation is aimed at anything that changed since "
                f"{args.since!r} -- this run proved nothing, which is not the "
                "same as a green table"
            )
        else:
            print(f"[FAIL] no mutation matches {args.only!r}")
        return 1

    for mutation in elsewhere:
        print(f"[SKIP] {mutation.name}: {mutation.only_on} only, and this is {HERE}")
    chosen = [m for m in chosen if m not in elsewhere]
    if not chosen:
        print(f"[FAIL] every mutation matching {args.only!r} is for another platform")
        return 1

    print("--- control: the suite must be green before anything is broken", flush=True)
    names, counted, out = run_tests()
    if counted is None:
        print("[FAIL] the control run produced no summary line, so nothing below is readable")
        print(out[-2000:])
        return 1
    if counted != 0 or names:
        print(f"[FAIL] the control run is not green: {counted} failed, {sorted(names)}")
        return 1
    print("[OK]   control green", flush=True)

    # The same cross-check the front-end harness carries, and for the same
    # reason: an `expect` naming a test that does not exist cannot go red, so the
    # run prints SURVIVED and the fault reads as a gap in the suite. Derived from
    # libtest's own list rather than from a hand-kept table.
    known = all_test_names()
    unknown = [m for m in chosen if not any(m.expect in name for name in known)]
    if unknown:
        for mutation in unknown:
            print(
                f"[FAIL] {mutation.name}: no test here is named {mutation.expect!r} -- "
                "it cannot go red, so this mutation would report SURVIVED"
            )
        return 1
    print(f"[OK]   every mutation names one of the {len(known)} tests", flush=True)

    problems = 0
    with tempfile.TemporaryDirectory(prefix="tpdf-mutate-rs-") as scratch:
        for mutation in chosen:
            target = CRATE / mutation.path
            # Copied aside and written *back*, never moved: docs/TRAPS.md
            # records a restore-by-move that left the mutated build in place.
            #
            # And written back rather than copied back, which is the same trap
            # arriving through a timestamp. `shutil.copy2` preserves the
            # backup's mtime, so the restored file ends up *older* than the
            # artifact cargo built from the mutated one -- and the next
            # `cargo test` finds nothing to rebuild and serves the mutation.
            # The file on disk is correct; the binary under test is not. The
            # backup stays a real file so that a harness that dies mid-run
            # leaves something to recover from.
            backup = Path(scratch) / f"{len(list(Path(scratch).iterdir()))}.bak"
            shutil.copy2(target, backup)
            try:
                # Bytes, decoded explicitly. `read_text` uses the locale codec,
                # and on Windows that is cp1252, which does not merely mangle
                # this file -- it *cannot read it*: search.rs holds `İ` and `ﬁ`
                # for the case-folding tests, whose UTF-8 encodings contain the
                # byte 0x81, and cp1252 leaves 0x81 undefined. So this raised
                # UnicodeDecodeError on the first mutation and the harness never
                # ran here at all.
                #
                # The newlines are normalised for matching only, because a
                # Windows checkout is CRLF while the anchors are written with
                # "\n" -- eight of them span lines. The file's own convention
                # goes back on the way out, and the restore below is bytes, as
                # docs/TRAPS.md requires.
                raw = target.read_bytes().decode("utf-8")
                crlf = "\r\n" in raw
                source = raw.replace("\r\n", "\n") if crlf else raw
                if source.count(mutation.before) != 1:
                    print(
                        f"[FAIL] {mutation.name}: its anchor appears "
                        f"{source.count(mutation.before)} times, so the mutation is not the "
                        "one described"
                    )
                    problems += 1
                    continue
                mutated = source.replace(mutation.before, mutation.after)
                if crlf:
                    mutated = mutated.replace("\n", "\r\n")
                target.write_bytes(mutated.encode("utf-8"))
                # The test this mutation names, first and alone. When it goes
                # red -- which is the whole table's expected outcome -- the
                # verdict is settled and the run stops there.
                names, counted, out = run_tests(mutation.expect)
                narrow = True
                if counted is not None and not names:
                    # It did not. Now the full set matters: "nothing noticed"
                    # and "something else noticed" are different findings, and
                    # the second one prints which tests went red instead.
                    names, counted, out = run_tests()
                    narrow = False
            finally:
                target.write_bytes(backup.read_bytes())

            if counted is None:
                # Almost always a compile error, which produces no failing-test
                # lines at all -- indistinguishable from a survivor without this.
                first = next(
                    (line for line in out.splitlines() if line.startswith("error")), ""
                )
                print(
                    f"[FAIL] {mutation.name}: no summary line -- the run did not finish"
                    + (f" ({first})" if first else "")
                )
                problems += 1
                continue
            if len(names) != counted:
                print(
                    f"[FAIL] {mutation.name}: {len(names)} failing test lines but the summary "
                    f"says {counted} -- this harness cannot read its own output"
                )
                problems += 1
                continue
            if not names:
                print(f"[FAIL] {mutation.name}: SURVIVED -- no test noticed")
                problems += 1
                continue
            hit = any(mutation.expect in name for name in names)
            mark = "[OK]  " if hit else "[FAIL]"
            # The scope is printed because the count means different things in
            # the two cases: `1 red` out of the one test named for the mutation
            # is not the same statement as `1 red` out of 607.
            scope = "the test named for it" if narrow else f"{len(known)} tests"
            print(
                f"{mark} {mutation.name}: {counted} red of {scope}"
                + ("" if hit else f", but NOT the expected one ({mutation.expect!r})")
            )
            if not hit:
                print(f"         red instead: {sorted(names)}")
                problems += 1

    print()
    # The skip count rides on the verdict rather than sitting in a line above it.
    # A run that could not execute part of its own table must not read, three
    # scrollbacks later, like one that executed all of it.
    aside = f", {len(elsewhere)} skipped as not runnable on {HERE}" if elsewhere else ""
    print(
        f"[OK] all {len(chosen)} mutations caught by the test named for them{aside}"
        if problems == 0
        else f"[FAIL] {problems} of {len(chosen)} mutations were not caught as described{aside}"
    )
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
