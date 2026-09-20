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
    scripts/mutate_rust.py --resume         # reuse a killed run's verdicts
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from live_output import stream_results  # noqa: E402
import mutation_resume  # noqa: E402
import mutation_since  # noqa: E402

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

#: And it builds that target directory *without debug info*, which is worth 7x.
#:
#: Every mutation touches one file, so cargo re-codegens the crate and relinks
#: the test binary --- and with full debug info that binary is 33 MB and the
#: cycle costs 22 to 30 s. Measured 2026-08-27, interleaved against the same
#: command in a second target directory built with `debug = 0`:
#:
#:     round 1:  debug= 22.6s   nodebug=  3.9s
#:     round 2:  debug= 30.1s   nodebug=  3.7s
#:     round 3:  debug= 27.0s   nodebug=  3.3s
#:
#: For the 508-mutation table that is 5.6 hours against well under one, and the
#: target directory is 1.7 GB rather than 14 GB. The cold build is 178 s, paid
#: once.
#:
#: It is safe because `debug` is debug *information* only: `debug_assertions`
#: and overflow checks are separate knobs and are untouched, so every test runs
#: exactly the program it ran before. What is given up is line numbers in a
#: panic backtrace, and nothing here reads one --- the harness identifies a
#: caught mutation by the test *name* libtest prints, which is unaffected.
#:
#: The figure this replaces is `BUILD.md`'s 1.77 s per mutation, measured on
#: 2026-08-21 when the crate was much smaller. That number did not go stale
#: because anybody changed the harness; it went stale because the crate grew,
#: which is exactly the kind of expiry nothing goes red about.
CARGO_ENV = {
    **os.environ,
    "CARGO_TARGET_DIR": str(MUT_TARGET),
    "CARGO_PROFILE_DEV_DEBUG": "0",
}

#: Which platform this is, in the vocabulary `Mutation.only_on` uses.
HERE = "macos" if sys.platform == "darwin" else "windows" if sys.platform == "win32" else "linux"

#: Only these modules' tests are run, so an unrelated failure elsewhere cannot be
#: read as a mutation being caught. libtest takes several filters and ORs them,
#: but only after `--`: `cargo test --lib a:: b::` is cargo's own argument error,
#: which is worth knowing because it looks like the feature being unsupported.
FILTERS = [
    "textedit::",
    "textview::",
    "forms::",
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
    # Added 2026-08-26 with the verification module, at the same time as the
    # mutations rather than after them. Seventh time this list has needed an
    # entry, and the first time it was written without the guard having to say
    # so -- which is what the six notes above are for.
    "verify::",
    # Added 2026-08-31 with `save_outside.rs`, in the same edit that moved the
    # two read-back deadline mutations into it. Eighth time, and the first where
    # the list had to change for a *move* rather than for new code: `save::` is
    # not a prefix of `save_outside::` --- libtest filters on substring, and
    # "save::" does not occur in "save_outside::tests::..." --- so the two
    # mutations would have named tests the harness could no longer see, and
    # reported SURVIVED for a deadline that works.
    "save_outside::",
    # Added 2026-08-31 with `save_order.rs`, in the same edit that took the save
    # sequence out of `save_document`. Ninth time, and the second in a day where
    # the substring rule is what makes the entry necessary rather than tidy:
    # "save::" does not occur in "save_order::tests::...", and "save_outside::"
    # does not either, so without this line the five mutations aimed at the
    # ordering would name tests the harness cannot see and report SURVIVED for a
    # sequence that works. `scripts/check_mutation_anchors.py` cannot say so --
    # it reads anchors and test names out of the sources and knows nothing about
    # this list -- which is why the note is here rather than trusted to a gate.
    "save_order::",
    # Added 2026-09-06 with `fields.rs`, in the same edit that moved the two
    # field-tree walks into it. It is subsumed by `tests::` above and is written
    # anyway, because that entry matches only because this module happens to
    # call its test module `tests` -- a filter that depends on nobody renaming a
    # private module is not a filter. Tenth entry; the eight notes above are all
    # the same incident.
    "fields::",
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


# `Plan::is_appendable`'s body, which two mutations aim at and which has now moved
# twice. Named once so that the next clause added to the predicate re-aims both.
MUT_SUBTYPE_GUARD = (
    "    if dictionary\n"
    '        .get(b"Subtype")\n'
    "        .and_then(Object::as_name)\n"
    "        .is_err()\n"
    "    {"
)

# **The whole predicate, comment and all**, because the deletion clause added on
# 2026-08-30 carries six lines of comment inside it -- so an anchor naming only
# the three code lines no longer matches, and the two mutations that share this
# both went red in the gate on the commit that added it.
MUT_APPENDABLE = (
    "        (!self.marks.is_empty() || !self.notes.is_empty())\n"
    "            && self.redactions.is_empty()\n"
    "            // **A removal, and an append only adds.** Without this clause a plan\n"
    "            // that highlights one line and deletes one comment classifies as an\n"
    "            // append, writes the highlight, drops the deletion and reports\n"
    "            // success -- and a fixture holding only the deletion cannot see it,\n"
    "            // because with no marks and no note edits the first clause makes the\n"
    "            // answer false either way. `docs/TRAPS.md` has that.\n"
    "            && self.discards.is_empty()\n"
    "            && self.pages_are_the_file()"
)

MUTATIONS = [
    Mutation('preserved forms: bypass graphics state validation', 'src/textedit/forms.rs', 'super::graphics::normal(doc, resources, name)?', '{ let _ = (doc, resources, name); }', 'textedit_preserved_forms_validate_normal_graphics_states_without_hidden_carriers'),
    Mutation('preserved forms: lose operation bound', 'src/textedit/forms.rs', 'if budget.operations > MAX_FORM_OPERATIONS {', 'if false {', 'textedit_preserved_forms_refuse_cycles_external_carriers_and_unbalanced_state'),
    Mutation('preserved forms: ignore unbalanced state', 'src/textedit/forms.rs', 'if inside || stack != 0 {', 'if false {', 'textedit_preserved_forms_refuse_cycles_external_carriers_and_unbalanced_state'),
    Mutation('preserved forms: omit text collision bounds', 'src/textedit/forms.rs', 'has_text = true;', 'has_text = false;', 'textedit_layout_reserves_transformed_form_text_but_not_plain_graphics'),
    Mutation('preserved forms: accept external carriers', 'src/textedit/forms.rs', 'for (key, value) in &form.dict {', 'for (key, value) in form.dict.iter().take(0) {', 'textedit_preserved_forms_refuse_cycles_external_carriers_and_unbalanced_state'),
    Mutation('indexed images: ignore invalid samples', 'src/textedit/images.rs', '|| high_index.is_some_and(|high| decoded.iter().any(|&sample| sample > high))', '|| high_index.is_some_and(|_| false)', 'textedit_indexed_images_preserve_palette_and_refuse_invalid_samples'),
    Mutation('invisible signatures: reject empty widgets', 'src/forms.rs', 'kind != Some(b"Sig") && (rect[2] == rect[0] || rect[3] == rect[1])', 'rect[2] == rect[0] || rect[3] == rect[1]', 'forms_preserve_invisible_signature_widgets_without_offering_them_for_editing'),
    Mutation('figures: admit editable span children', 'src/textedit/tagging.rs', 'if plain.tag == b"Figure"', 'if false', 'textedit_figure_cannot_expose_text_through_a_span_child'),
    Mutation('descenders: reject half-em outlines', 'src/textedit/fonts.rs', 'bottom * unit >= -500.', 'bottom * unit >= -250.', 'textedit_truetype_descenders_extend_hit_bounds_and_keep_a_finite_limit'),
    Mutation('CJK fallback: disable automatic selection', 'src/textedit/fonts/fallback.rs', 'Ok(4 + (style & 1))', 'Ok(style)', 'cjk_fallback_subsets_and_reopens_with_new_characters'),
    Mutation('CJK fallback: embed the entire program', 'src/textedit/fonts/fallback.rs', 'let bytes = self.subset.as_deref().unwrap_or_else(|| data(self.style));', 'let bytes = data(self.style);', 'cjk_fallback_subsets_and_reopens_with_new_characters'),
    Mutation('CJK fallback: reuse different glyph subsets', 'src/textedit/fonts/fallback.rs', '.entry((self.style, self.program_key.clone()))', '.entry((self.style, Vec::new()))', 'cjk_program_cache_distinguishes_glyph_sets_and_reuses_identical_sets'),
    Mutation('CJK fallback: omit embedding rights', 'src/textedit/fonts/fallback_subset.rs', 'tables.insert(*b"OS/2", rights.to_vec());', 'let _ = rights;', 'cjk_fallback_subsets_and_reopens_with_new_characters'),
    Mutation('CJK fallback: retain source glyph indices', 'src/textedit/fonts/fallback.rs', 'font.glyphs = glyphs;', 'let _ = glyphs;', 'cjk_fallback_subsets_and_reopens_with_new_characters'),
    Mutation('Type3: ignore declared advance', 'src/textedit/fonts/type3.rs', 'if width != advance {', 'if false {', 'type3_rejects_program_side_effects_bad_metrics_maps_and_limits_atomically'),
    Mutation('Type3: trust declared ink box', 'src/textedit/fonts/type3.rs', 'for point in points.chunks_exact(2) {', 'for point in points.chunks_exact(2).take(0) {', 'type3_single_byte_word_spacing_and_curve_bounds_are_measured'),
    Mutation('Type3: lose one-byte encoding', 'src/textedit/fonts/unicode.rs', 'single_byte: true,', 'single_byte: false,', 'type3_edit_delete_and_layout_preserve_fonts_and_following_text'),
    Mutation('Type3: omit code-32 word spacing', 'src/textedit/fonts/unicode.rs', 'if self.single_byte && *code == 32 {', 'if false {', 'type3_single_byte_word_spacing_and_curve_bounds_are_measured'),
    Mutation('Type3: admit forged header operator', 'src/textedit/fonts/type3.rs', 'if token != b"d1" {', 'if token != b"d1" && token != b"dX" {', 'type3_header_is_literal_bounded_and_preserves_comments'),
    Mutation('Type3: lose glyph operation bound', 'src/textedit/fonts/type3.rs', 'content.operations.len() > *ops_left', 'false', 'type3_header_is_literal_bounded_and_preserves_comments'),
    Mutation('partial editing: leave stale logical text', 'src/textedit/actual.rs', 'dict.set("ActualText", Object::string_literal(bytes));', 'let _ = (dict, bytes);', 'actual_text_edits_update_logical_text_and_preserve_following_positions'),
    Mutation('partial editing: offer alternate logical text', 'src/textedit/actual.rs', '.filter(|_| self.visible == self.logical);', ';', 'alternate_actual_text_preserves_span_and_refuses_forged_edits_and_overlap'),
    Mutation('partial editing: omit preserved collision checks', 'src/textedit/layout.rs', '.chain(&page.preserved)', '', 'alternate_actual_text_preserves_span_and_refuses_forged_edits_and_overlap'),
    Mutation('partial editing: reject paragraph cells', 'src/textedit/tagging.rs', '&& (text_block(tag) || tag == b"Figure")))', '&& tag == b"Figure"))', 'textedit_tables_preserve_merged_cells_and_paragraph_leaf_ownership'),
    Mutation('partial editing: reject merged cell spans', 'src/textedit/tagging/tables.rs', '!(1..=128).contains(&integer(span)?)', 'integer(span)? != 1', 'textedit_tables_preserve_merged_cells_and_paragraph_leaf_ownership'),
    Mutation('partial editing: expose pattern text', 'src/textedit.rs', '|| !matches!(fill_components, patterns::Colour::Solid(_))', '', 'textedit_pattern_text_is_preserved_beside_editable_solid_text'),
    Mutation('local text preservation: reject unused parent index', 'src/textedit/tagging.rs', 'if page_dict.has(b"StructParent")', 'if page_dict.has(b"StructParents") || page_dict.has(b"StructParent")', 'textedit_unused_parent_index_preserves_plain_text_edits_and_metadata'),
    Mutation('local text preservation: skip shading validation', 'src/textedit/patterns.rs', 'check(doc, resources, name)?;', 'let _ = (doc, resources);', 'textedit_shading_resource_refusals_are_atomic'),
    Mutation('local text preservation: ignore preserved collisions', 'src/textedit/layout.rs', '.chain(&page.preserved)', '', 'preserved_skew_text_blocks_layout_collisions'),
    Mutation('refusal kinds: font: skip carrier count', 'src/textedit/fonts.rs', 'if carriers.next().is_some() {', 'if false {', 'textedit_font_refusals_separate_missing_conflicting_and_invalid_programs'),
    Mutation('refusal kinds: font: misidentify PostScript', 'src/textedit/fonts.rs', 'b"FontFile" => return type1::embedded(doc, font),', 'b"FontFile" => return Err("unsupported embedded CFF font".into()),', 'textedit_font_refusals_identify_program_carriers_without_echoing_values'),
    Mutation('refusal kinds: font: misidentify OpenType', 'src/textedit/fonts.rs', 'OpenType programs in Type1 fonts are not editable yet', 'unsupported embedded CFF font', 'textedit_font_refusals_identify_program_carriers_without_echoing_values'),
    Mutation('refusal kinds: parent tree: miss object entries', 'src/textedit/tagging.rs', 'crate::encoding::resolve(doc, pair[1]).as_dict().is_ok()', 'false', 'textedit_parent_tree_refusals_identify_unclaimed_annotation_entries'),
    Mutation('refusal kinds: parent tree: misclassify arrays', 'src/textedit/tagging.rs', 'crate::encoding::resolve(doc, pair[1]).as_dict().is_ok()', 'crate::encoding::resolve(doc, pair[1]).as_array().is_ok()', 'textedit_parent_tree_refusals_identify_unclaimed_annotation_entries'),
    Mutation('headers: skip index ownership', 'src/textedit/tagging/tables.rs', 'self.index.get(key) != Some(&id)', 'false', 'textedit_headers_validate_idtree_in_both_directions'),
    Mutation('headers: skip reverse index coverage', 'src/textedit/tagging/tables.rs', 'self.headers.len() != self.index.len()', 'false', 'textedit_headers_validate_idtree_in_both_directions'),
    Mutation('headers: skip header link ownership', 'src/textedit/tagging/tables.rs', 'self.headers.get(key) != Some(table)', 'false', 'textedit_headers_refuse_unknown_foreign_duplicate_and_recursive_links'),
    Mutation('headers: allow recursive header links', 'src/textedit/tagging/tables.rs', '(header && !headers.is_empty())', 'false', 'textedit_headers_refuse_unknown_foreign_duplicate_and_recursive_links'),
    Mutation('headers: allow duplicate links', 'src/textedit/tagging/tables.rs', '!unique.insert(key)', '{ let _ = unique.insert(key); false }', 'textedit_headers_refuse_unknown_foreign_duplicate_and_recursive_links'),
    Mutation('headers: allow oversized IDs', 'src/textedit/tagging/tables.rs', 'bytes.len() > 127', 'false', 'textedit_headers_bound_identifiers_scope_and_attributes'),
    Mutation('headers: allow unknown scope', 'src/textedit/tagging/tables.rs', '!matches!(name(scope)?, b"Row" | b"Column" | b"Both")', 'false', 'textedit_headers_bound_identifiers_scope_and_attributes'),
    Mutation('headers: omit tree depth limit', 'src/textedit/tagging/tables.rs', 'depth > 8', 'false', 'textedit_headers_validate_name_tree_shape_order_limits_and_depth'),
    Mutation('headers: omit tree node limit', 'src/textedit/tagging/tables.rs', '*nodes >= 128', 'false', 'textedit_idtree_bounds_nodes_and_entries_independently'),
    Mutation('headers: omit tree entry limit', 'src/textedit/tagging/tables.rs', 'names.len() / 2 + index.len() > MAX_CONTENT_ITEMS', 'false', 'textedit_idtree_bounds_nodes_and_entries_independently'),
    Mutation('headers: omit leaf ordering', 'src/textedit/tagging/tables.rs', 'key <= last.as_slice()', 'false', 'textedit_idtree_requires_ordered_keys_and_disjoint_children'),
    Mutation('headers: omit child ordering', 'src/textedit/tagging/tables.rs', 'if low <= last {', 'if false {', 'textedit_idtree_requires_ordered_keys_and_disjoint_children'),
    Mutation('headers: omit upper Limits', 'src/textedit/tagging/tables.rs', 'identifier(&limits[1])? != last', 'false', 'textedit_headers_validate_name_tree_shape_order_limits_and_depth'),
    Mutation('tables: allow unbounded spans', 'src/textedit/tagging/tables.rs', '!(1..=128).contains(&integer(span)?)', 'false', 'textedit_tables_refuse_spans_metadata_and_attribute_ambiguity'),
    Mutation('tables: allow repeated attributes', 'src/textedit/tagging/tables.rs', '!seen.insert(key)', '{ let _ = seen.insert(key); false }', 'textedit_tables_refuse_spans_metadata_and_attribute_ambiguity'),
    Mutation('tables: allow orphan rows', 'src/textedit/tagging.rs', 'if parent_tag != b"Table" && !(role == b"TR" && table_section(parent_tag)) {', 'if false {', 'textedit_tables_require_rows_and_cells_in_their_own_containers'),
    Mutation('tables: allow orphan cells', 'src/textedit/tagging.rs', 'name(get(node(doc, parent_id)?, b"S")?)? != b"TR"', 'false', 'textedit_tables_require_rows_and_cells_in_their_own_containers'),
    Mutation('tables: ignore child roles', 'src/textedit/tagging.rs', '} else if role == b"TR" {\n                                matches!(tag, b"TD" | b"TH")', '} else if role == b"TR" {\n                                true', 'textedit_tables_require_rows_and_cells_in_their_own_containers'),
    Mutation('tables: allow text in border content', 'src/textedit/tagging.rs', 'matches!(self.names[mcid].as_slice(), b"Table" | b"Figure")', 'matches!(self.names[mcid].as_slice(), b"Figure")', 'textedit_tables_refuse_invalid_hierarchy_and_container_text'),
    Mutation('tagged first indent: omit supported attribute', 'src/textedit/tagging.rs', '            b"TextIndent",\n            b"TextAlign",\n', '            b"TextAlign",\n', 'textedit_tagged_layout_spacing_and_source_spaces_survive_a_fitting_edit'),
    Mutation('tagged first indent: skip numeric validation', "src/textedit/tagging.rs", 'if let Ok(indent) = attributes.get(key) {', 'if let Ok(indent) = attributes.get(key) {\n                if key == b"TextIndent" { continue; }', 'textedit_tagged_layout_spacing_rejects_invalid_values_and_document_scope'),
    Mutation('tagged first indent: accept document scope', 'src/textedit/tagging.rs', 'if tag == b"Document" {', 'if tag == b"Document" && key != b"TextIndent" {', 'textedit_tagged_layout_spacing_rejects_invalid_values_and_document_scope'),
    Mutation('tag containers: refuse neutral grouping', 'src/textedit/tagging.rs', 'b"NonStruct" | b"L" | b"Table" | b"TR"', 'b"L" | b"Table" | b"TR"', 'textedit_containers_and_headings_preserve_roles_order_and_graph'),
    Mutation('nested lists: discard child dispatch', 'src/textedit/tagging.rs', 'pending.push((item, id, depth, bounded));', '// dispatch omitted', 'textedit_nested_lists_keep_each_body_label_and_structure_object'),
    Mutation('nested lists: reset depth on each item', 'src/textedit/tagging.rs', 'pending.push((item, id, depth, bounded));', 'pending.push((item, id, 0, bounded));', 'textedit_nested_lists_share_the_grouping_depth_bound'),
    Mutation('nested lists: use enclosing list as parent', 'src/textedit/tagging.rs', 'pending.push((item, id, depth, bounded));', 'pending.push((item, parent_id, depth, bounded));', 'textedit_nested_lists_keep_each_body_label_and_structure_object'),
    Mutation('nested lists: ignore pending siblings', 'src/textedit/tagging.rs', 'if items.len() + pending.len() > MAX_NODES {\n                    return Err("tagged list frontier exceeds its limit".into());', 'if items.len() > MAX_NODES {\n                    return Err("tagged list frontier exceeds its limit".into());', 'textedit_nested_lists_bound_pending_children_before_visiting_them'),
    Mutation('nested lists: omit list frontier limit', 'src/textedit/tagging.rs', 'if items.len() + pending.len() > MAX_NODES {\n                    return Err("tagged list frontier exceeds its limit".into());', 'if false {\n                    return Err("tagged list frontier exceeds its limit".into());', 'textedit_nested_lists_bound_pending_children_before_visiting_them'),
    Mutation('lists: refuse list grouping', 'src/textedit/tagging.rs', 'b"NonStruct" | b"L" | b"Table" | b"TR"', 'b"NonStruct" | b"Table" | b"TR"', 'textedit_lists_preserve_labels_numbering_structure_and_other_items'),
    Mutation('lists: reject list item blocks', 'src/textedit/tagging.rs', 'b"LI" | b"TD" | b"TH" | b"Table" | b"Figure"', 'b"TD" | b"TH" | b"Table" | b"Figure"', 'textedit_lists_preserve_labels_numbering_structure_and_other_items'),
    Mutation('lists: omit numbered attribute owner', 'src/textedit/tagging.rs', 'name(get(attributes, b"O")?)? != b"List"', 'false', 'textedit_lists_refuse_ambiguous_numbering_and_stale_attributes'),
    Mutation('lists: accept unknown numbering', 'src/textedit/tagging.rs', 'b"None"\n                    | b"Disc"', 'b"Unknown" | b"None"\n                    | b"Disc"', 'textedit_lists_refuse_ambiguous_numbering_and_stale_attributes'),
    Mutation('lists: ignore list child role', 'src/textedit/tagging.rs', 'if !matches!(\n                            name(get(node(doc, reference(item)?)?, b"S")?)?,\n                            b"LI" | b"L"\n                        ) {', 'if false {', 'textedit_lists_refuse_wrong_roles_owners_cycles_and_semantic_overrides'),
    Mutation('lists: ignore list item parent role', 'src/textedit/tagging.rs', 'name(get(node(doc, parent_id)?, b"S")?)? != b"L"', 'false', 'textedit_lists_refuse_wrong_roles_owners_cycles_and_semantic_overrides'),
    Mutation('lists: allow standard list role remap', 'src/textedit/tagging.rs', '|| matches!(tag, b"L" | b"LI" | b"Lbl" | b"LBody")', '|| false', 'textedit_lists_refuse_wrong_roles_owners_cycles_and_semantic_overrides'),

    Mutation('tag containers: omit depth bound', 'src/textedit/tagging.rs', 'depth >= MAX_CONTAINER_DEPTH', 'false', 'textedit_container_depth_is_bounded_with_a_valid_boundary'),
    Mutation('tag containers: omit count bound', 'src/textedit/tagging.rs', 'containers > MAX_CONTAINERS', 'false', 'textedit_container_count_is_bounded_independently_of_depth_and_content'),
    Mutation('tag containers: omit frontier bound', 'src/textedit/tagging.rs', 'if items.len() + pending.len() > MAX_NODES {\n                    return Err("tagged grouping frontier exceeds its limit".into());', 'if false {\n                    return Err("tagged grouping frontier exceeds its limit".into());', 'textedit_container_frontier_is_bounded_before_children_are_visited'),
    Mutation('tag containers: accept layout attributes', 'src/textedit/tagging.rs', '|| (!matches!(role, b"L" | b"Table") && child.has(b"A"))', '|| false', 'textedit_containers_refuse_cycles_aliases_attributes_and_wrong_owners_atomically'),
    Mutation('tag containers: discard immediate parent', 'src/textedit/tagging.rs', 'element(doc, child, parent_id, &scope, owns_text, role)?', 'element(doc, child, root_id, &scope, owns_text, role)?', 'textedit_containers_keep_interleaved_page_owners_and_nested_leaves'),
    Mutation('tag containers: refuse grouping elements', 'src/textedit/tagging.rs', 'if container(role)\n                || matches!(role, b"NonStruct" | b"L" | b"Table" | b"TR")\n                || table_section(role)\n                || toc(role)\n            {', 'if false {', 'textedit_containers_and_headings_preserve_roles_order_and_graph'),
    Mutation('tag containers: refuse level six headings', 'src/textedit/tagging.rs', 'b"H5" | b"H6"', 'b"H5"', 'textedit_containers_and_headings_preserve_roles_order_and_graph'),
    Mutation('tag containers: permit standard grouping remaps', 'src/textedit/tagging.rs', 'text_block(tag)\n        || container(tag)\n        || matches!(tag, b"L"', 'text_block(tag)\n        || false\n        || matches!(tag, b"L"', 'textedit_containers_refuse_cycles_aliases_attributes_and_wrong_owners_atomically'),
    Mutation('tag containers: permit standard heading remaps', 'src/textedit/tagging.rs', 'text_block(tag)\n        || container(tag)\n        || matches!(tag, b"L"', 'false\n        || container(tag)\n        || matches!(tag, b"L"', 'textedit_containers_refuse_cycles_aliases_attributes_and_wrong_owners_atomically'),
    Mutation('tag metadata: require explicit element type', 'src/textedit/tagging.rs', '.is_ok_and(|value| value.as_name().ok() != Some(b"StructElem"))', '.map_or(true, |value| value.as_name().ok() != Some(b"StructElem"))', 'textedit_tagged_metadata_representations_preserve_the_original_graph'),
    Mutation('tag metadata: ignore supplied element type', 'src/textedit/tagging.rs', '.is_ok_and(|value| value.as_name().ok() != Some(b"StructElem"))', '.is_ok_and(|_| false)', 'textedit_tagged_metadata_references_keep_validation_and_atomic_refusal'),
    Mutation('tag metadata: skip attribute resolution', 'src/textedit/tagging.rs', '        match crate::encoding::resolve(doc, value) {\n            Object::Array(objects) if role != b"L" => {', '        match value {\n            Object::Array(objects) if role != b"L" => {', 'textedit_tagged_metadata_representations_preserve_the_original_graph'),
    Mutation('tag metadata: skip role map resolution', 'src/textedit/tagging.rs', '.get(b"RoleMap")\n            .ok()\n            .map(|value| crate::encoding::resolve(doc, value).as_dict())', '.get(b"RoleMap")\n            .ok()\n            .map(Object::as_dict)', 'textedit_tagged_metadata_representations_preserve_the_original_graph'),
    Mutation('tag metadata: skip parent number array resolution', 'src/textedit/tagging.rs', 'let nums = array(crate::encoding::resolve(doc, nums))?;', 'let nums = array(nums)?;', 'textedit_tagged_metadata_representations_preserve_the_original_graph'),
    Mutation('TD leading: omit leading update', 'src/textedit.rs', 'leading = -y;', 'let _ = y;', 'textedit_td_sets_signed_leading_and_resets_both_text_origins'),
    Mutation('TD leading: reverse leading sign', 'src/textedit.rs', 'leading = -y;', 'leading = y;', 'textedit_td_sets_signed_leading_and_resets_both_text_origins'),
    Mutation('TD leading: omit line movement', 'src/textedit.rs', 'move_line(&mut matrix, x, y)?;', 'if op.operator != "TD" { move_line(&mut matrix, x, y)?; }', 'textedit_td_sets_signed_leading_and_resets_both_text_origins'),
    Mutation('TD leading: retain advanced text cursor', 'src/textedit.rs', 'move_line(&mut matrix, x, y)?;\n                positioned = true;\n                cursor = 0.0;', 'move_line(&mut matrix, x, y)?;\n                positioned = true;\n                if op.operator != "TD" { cursor = 0.0; }', 'textedit_td_sets_signed_leading_and_resets_both_text_origins'),
    Mutation('TD leading: retain continuation marker', 'src/textedit.rs', 'move_line(&mut matrix, x, y)?;\n                positioned = true;', 'move_line(&mut matrix, x, y)?;\n                if op.operator != "TD" { positioned = true; }', 'textedit_td_sets_signed_leading_and_resets_both_text_origins'),
    Mutation('continued: omit discovery deletion check', 'src/textedit.rs', 'continuation_adjustment(run, 0.)?;', 'let _ = run;', 'textedit_continued_discovery_requires_representable_deletion'),
    Mutation('large word spacing: restore positive relative ceiling', 'src/textedit/fonts.rs', 'word_spacing > 1_000_000.', 'word_spacing > size * 0.25', 'textedit_large_word_spacing_preserves_scaled_continuations_and_ink_bounds'),
    Mutation('large word spacing: omit positive value ceiling', 'src/textedit/fonts.rs', 'word_spacing > 1_000_000.', 'false', 'textedit_large_word_spacing_keeps_original_code_semantics_and_finite_limits'),
    Mutation('large word spacing: omit combined cursor ceiling', 'src/textedit/fonts.rs', 'cursor > 1_000_000.', 'false', 'textedit_large_word_spacing_keeps_original_code_semantics_and_finite_limits'),
    Mutation('JPEG: omit preserved image validation', 'src/textedit/images.rs', 'jpeg::check(&jpeg, width, height, components)?;', 'let _ = (jpeg, components);', 'textedit_jpeg_refuses_bad_envelopes_and_incomplete_streams_atomically'),
    Mutation('JPEG: omit framing validation', 'src/textedit/images/jpeg.rs', 'framing(input)?;', 'let _ = input;', 'textedit_jpeg_refuses_bad_envelopes_and_incomplete_streams_atomically'),
    Mutation('JPEG: permit empty entropy scans', 'src/textedit/images/jpeg.rs', 'if !samples {', 'if false && !samples {', 'textedit_jpeg_refuses_bad_envelopes_and_incomplete_streams_atomically'),
    Mutation('JPEG: raise encoded byte ceiling', 'src/textedit/images/jpeg.rs', 'input.len() > 2 * super::super::MAX_CONTENT', 'input.len() > 4 * super::super::MAX_CONTENT', 'textedit_jpeg_bounds_valid_encoded_metadata_and_refuses_lossless_headers'),
    Mutation('JPEG: omit progressive scan ceiling', 'src/textedit/images/jpeg.rs', 'if scans > 64 {', 'if false {', 'textedit_jpeg_framing_bounds_scans_and_requires_complete_marker_segments'),
    Mutation('JPEG: return after headers alone', 'src/textedit/images/jpeg.rs', 'let mut pixels = vec![0; bytes];', 'if true { return Ok(()); }\n    let mut pixels = vec![0; bytes];', 'textedit_jpeg_refuses_bad_envelopes_and_incomplete_streams_atomically'),
    Mutation('JPEG: forget shared image cost', 'src/textedit/images.rs', 'return Ok(bytes);', 'return Ok(0);', 'textedit_jpeg_consumes_shared_image_budget_before_decoding'),
    Mutation('reversed clips: reject signed dimensions', 'src/textedit/clipping.rs', '\n    if width == 0. || height == 0. {', '\n    if width <= 0. || height <= 0. {', 'textedit_reversed_clips_preserve_geometry_and_authored_bytes'),
    Mutation('reversed clips: leave horizontal corners unsorted', 'src/textedit/clipping.rs', 'next[0].min(next[2])', 'next[0]', 'textedit_reversed_clip_bounds_normalize_every_corner_after_transform'),
    Mutation('reversed clips: leave vertical corners unsorted', 'src/textedit/clipping.rs', 'next[1].min(next[3])', 'next[1]', 'textedit_reversed_clip_bounds_normalize_every_corner_after_transform'),
    Mutation('CID ligatures: refuse declared sequences', 'src/textedit/fonts/mapping.rs', 'let slot = super::ligatures::target(bytes).ok_or_else(invalid)?;', 'let slot = super::ligatures::target(&[]).ok_or_else(invalid)?;', 'textedit_cid_ligature_mapping_accepts_only_unique_exact_sequences'),
    Mutation('CID ligatures: accept duplicate source', 'src/textedit/fonts/mapping.rs', 'result.insert(first, slot).is_some() || !unicode.insert(slot)', '{ result.insert(first, slot); false } || !unicode.insert(slot)', 'textedit_cid_ligature_mapping_accepts_only_unique_exact_sequences'),
    Mutation('CID ligatures: accept duplicate target', 'src/textedit/fonts/mapping.rs', 'result.insert(first, slot).is_some() || !unicode.insert(slot)', 'result.insert(first, slot).is_some() || { unicode.insert(slot); false }', 'textedit_cid_ligature_mapping_accepts_only_unique_exact_sequences'),
    Mutation('CID ligatures: undercount expansion', 'src/textedit/fonts.rs', '                    characters += sequence.len();\n                } else {', '                    characters += 1;\n                } else {', 'textedit_cid_ligatures_measure_source_glyphs_and_bound_expansion'),
    Mutation('CID ligatures: reencode source geometry', 'src/textedit/fonts.rs', 'self.spaced_slots(&slots, size, spacing, word_spacing)?\n        } else {', 'self.spaced_layout(&text, size, spacing, word_spacing)?\n        } else {', 'textedit_cid_ligatures_measure_source_glyphs_and_bound_expansion'),
    Mutation('composite: ignore indirect descendant arrays', 'src/textedit/fonts/composite.rs', 'crate::encoding::resolve(doc, font.get(b"DescendantFonts").map_err(|_| INVALID)?)', 'font.get(b"DescendantFonts").map_err(|_| INVALID)?', 'textedit_composite_indirect_descendants_preserve_resources_and_refuse_cycles'),
    Mutation('inline tags: route MCIDs through spacer grammar', 'src/textedit.rs', '                if !properties.as_dict().is_ok_and(|dict| dict.has(b"MCID")) =>', '                if true =>', 'textedit_inline_tags_preserve_continuations_and_every_unedited_operator'),
    Mutation('inline tags: refuse closing marker inside text', 'src/textedit.rs', '("EMC", []) => tags.end()?,', '("EMC", []) if !inside => tags.end()?,', 'textedit_inline_tags_balance_independently_of_text_objects_and_keep_spacers'),
    Mutation('inline tags: reset cursor at a tag boundary', 'src/textedit.rs', '("BDC", [tag, properties]) => tags.begin(tag, Some(properties))?,', '("BDC", [tag, properties]) => { tags.begin(tag, Some(properties))?; cursor = 0.; },', 'textedit_inline_tags_preserve_continuations_and_every_unedited_operator'),
    Mutation('inline tags: accept semantic overrides', 'src/textedit/tagging.rs', 'keys(properties, &[b"MCID"], "marked-content properties")?;', '// unchecked properties', 'textedit_inline_tags_refuse_invalid_semantics_and_balance_atomically'),
    Mutation('inline separators: offer protected text for editing', 'src/textedit.rs', 'spacer.text(&text)?;\n            continue;', 'spacer.text(&text)?;', 'textedit_inline_separators_preserve_bytes_and_positions_without_edit_targets'),
    Mutation('inline separators: skip sequence validation', 'src/textedit.rs', 'spacer.step(&op.operator)?;', 'let _ = spacer;', 'textedit_inline_separators_refuse_semantics_nesting_and_unbounded_sequences'),
    Mutation('inline separators: skip space validation', 'src/textedit.rs', 'spacer.text(&text)?;', 'let _ = spacer;', 'textedit_inline_separators_refuse_semantics_nesting_and_unbounded_sequences'),
    Mutation('inline separators: allow semantic properties', 'src/textedit/spacers.rs', '|| dict.len() != 1', '|| false', 'textedit_inline_separators_refuse_semantics_nesting_and_unbounded_sequences'),
    Mutation('inline separators: remove count bound', 'src/textedit/spacers.rs', 'if !(1..=32).contains(&count) {', 'if false {', 'textedit_inline_separators_refuse_semantics_nesting_and_unbounded_sequences'),
    Mutation('inline separators: accept other Unicode controls', 'src/textedit/spacers.rs', 'matches!(unit[1], 7 | 9)', 'true', 'textedit_inline_separators_refuse_semantics_nesting_and_unbounded_sequences'),
    Mutation('refusals: drop inline tag identity', 'src/textedit/refusal.rs', 'return format!("inline {name} marked content is not editable yet");', 'return "inline marked content is not editable yet".into();', 'textedit_refusals_distinguish_operator_context_without_echoing_values'),
    Mutation('tag refusals: hide known metadata', 'src/textedit/tagging.rs', 'b"IDTree" => "IDTree",', 'b"IDTree" => "unrecognized",', 'textedit_tagged_refusals_identify_metadata_without_echoing_document_data'),
    Mutation('tag refusals: echo unknown metadata', 'src/textedit/tagging.rs', '_ => "unrecognized",', '_ => std::str::from_utf8(key).unwrap_or("unrecognized"),', 'textedit_tagged_refusals_identify_metadata_without_echoing_document_data'),
    Mutation('tag refusals: hide ownership mismatch', 'src/textedit/tagging.rs', 'return Err("tagged content and parent tree disagree on ownership".into());', 'return Err(INVALID.into());', 'textedit_tagged_refusals_separate_roles_parent_tree_and_ownership'),
    Mutation('tag refusals: hide missing structure tree', 'src/textedit/tagging.rs', 'return Err("marked content has no supported structure tree".into());', 'return Err(INVALID.into());', 'textedit_tagged_refusals_distinguish_marked_content_context'),
    Mutation('role map: omit standard role guard', 'src/textedit/tagging.rs', '|| standard_role(key)', '|| false', 'textedit_role_map_cannot_redefine_any_standard_type_even_when_unused'),
    Mutation('role map: omit unsupported figure role', 'src/textedit/tagging.rs', '| b"Figure"\n                | b"Formula"', '| b"Formula"', 'textedit_role_map_cannot_redefine_any_standard_type_even_when_unused'),
    Mutation('role map: reject custom aliases', 'src/textedit/tagging.rs', '|| standard_role(key)', '|| !key.is_empty()', 'textedit_role_map_blocks_disguised_standard_content_but_preserves_custom_aliases'),
    Mutation('refusals: echo unrecognized document token', 'src/textedit/refusal.rs', '_ => "unrecognized content operator is not editable yet",', '_ => name,', 'textedit_refusals_distinguish_operator_context_without_echoing_values'),
    Mutation('continued: discard the text cursor', 'src/textedit.rs', 'cursor += advance;', 'cursor = 0.;', 'textedit_continued_shows_keep_followers_fixed_when_shortened_or_deleted'),
    Mutation('continued: discard dependent show tracking', 'src/textedit.rs', 'continued.insert(previous_show.ok_or("text has no preceding position")?);', 'let _ = previous_show;', 'textedit_continued_shows_keep_followers_fixed_when_shortened_or_deleted'),
    Mutation('continued: omit spacing compensation', 'src/textedit.rs', 'let wanted = (replacement_advance - run.advance) * 1000. / run.size;', 'let wanted = 0_f64;', 'textedit_continued_shows_keep_followers_fixed_when_shortened_or_deleted'),
    Mutation('continued: ignore PDF numeric precision', 'src/textedit.rs', 'if emitted > 0. || drift > 0.000_001 {', 'if emitted > 0. {', 'textedit_continued_precision_and_accumulated_bounds_fail_atomically'),
    Mutation('continued: omit cursor bound', 'src/textedit.rs', '!cursor.is_finite() || cursor > 1_000_000.0', 'false', 'textedit_continued_precision_and_accumulated_bounds_fail_atomically'),
    Mutation('continued: forget cursor reset at new line', 'src/textedit.rs', 'move_line(&mut matrix, 0.0, -leading)?;\n                positioned = true;\n                cursor = 0.0;', 'move_line(&mut matrix, 0.0, -leading)?;\n                positioned = true;', 'textedit_continued_shows_follow_transformed_axes_and_reset_at_line_moves'),
    Mutation('text direction: reverse angle', 'src/text.rs', 'turns.round().rem_euclid(4.) as u8', '(-turns.round()).rem_euclid(4.) as u8', 'text_character_turns_accept_float_quarters_without_promoting_skew'),
    Mutation('text direction: round arbitrary skew', 'src/text.rs', '(turns - turns.round()).abs() < 0.0001', 'true', 'text_character_turns_accept_float_quarters_without_promoting_skew'),
    Mutation('ligature: measure source using re-encoding', 'src/textedit/fonts.rs', 'self.spaced_glyphs(&glyphs, size, spacing, word_spacing)?\n        } else if let Some(Codes::Double(codes))', 'self.spaced_layout(&text, size, spacing, word_spacing)?\n        } else if let Some(Codes::Double(codes))', 'textedit_ligatures_measure_original_glyphs_and_encode_longest_existing_match'),
    Mutation('ligature: truncate expanded text limit', 'src/textedit/fonts.rs', '                characters += sequence.len();\n            } else {', '                characters += 1;\n            } else {', 'textedit_ligatures_refuse_overflow_expansion_and_stale_edits_atomically'),
    Mutation('ligature: drop exact target sequence', 'src/textedit/fonts/ligatures.rs', '.eq(text.bytes().flat_map(|ch| [0, ch]))', '.eq(text.bytes().take(2).flat_map(|ch| [0, ch]))', 'textedit_ligatures_require_matching_maps_and_existing_outlines'),
    Mutation('ligature: ignore duplicate mappings', 'src/textedit/fonts/mapping.rs', 'if result[first as usize].is_some() || unicode[slot as usize] {', 'if false {', 'textedit_ligatures_require_matching_maps_and_existing_outlines'),
    Mutation('ligature: relax padded header', 'src/textedit/fonts/mapping.rs', 'if low == &[0, 0] && high == &[255, 255]', 'if low.len() == 2 && high.len() == 2', 'textedit_cff_padded_header_keeps_single_byte_sources_and_exact_wrapper'),
    Mutation('ligature: named map permits semantic disagreement', 'src/textedit/fonts/mapping.rs', 'usize::from(slot) != code || matches!(code, 0x80 | 0xA0 | 0xAD) || code < 32', 'matches!(code, 0x80 | 0xA0 | 0xAD) || code < 32', 'textedit_named_unicode_requires_identity_and_agreeing_legacy_glyphs'),
    Mutation('ligature: discard stroke component state', 'src/textedit.rs', 'let mut stroke_components = patterns::Colour::Solid(1);', 'let mut stroke_components = patterns::Colour::Solid(3);', 'textedit_stroke_colours_restore_independent_state_and_preserve_operators'),
    Mutation('ligature: skip later rectangle validation', 'src/textedit/clipping.rs', 'for rect in &ops[..count] {', 'for rect in &ops[..1] {', 'textedit_painted_rectangles_refuse_invalid_geometry_and_incomplete_paths_atomically'),
    Mutation('ligature: reject reversed paint rectangles', 'src/textedit/clipping.rs', '\n    if width == 0. || height == 0. {', '\n    if width <= 0. || height <= 0. {', 'textedit_painted_rectangles_preserve_paint_state_and_surrounding_operators'),

    Mutation('simple overhang: omit measured bounds', 'src/textedit/fonts.rs', 'horizontal_overhangs: Some(horizontal_overhangs),', 'horizontal_overhangs: None,', 'textedit_simple_overhang_tracks_unicode_ink_and_preserves_resources'),
    Mutation('simple overhang: index by PDF code', 'src/textedit/fonts.rs', 'horizontal_overhangs[byte as usize]', 'horizontal_overhangs[code_byte as usize]', 'textedit_simple_overhang_tracks_unicode_ink_and_preserves_resources'),
    Mutation('simple overhang: omit left bound', 'src/textedit/fonts.rs', 'left * unit >= -250.', 'true', 'textedit_simple_overhang_quarter_em_boundaries'),
    Mutation('simple overhang: omit right bound', 'src/textedit/fonts.rs', 'right * unit <= width + 250.', 'true', 'textedit_simple_overhang_quarter_em_boundaries'),
    Mutation('CFF: omit font dispatch', 'src/textedit.rs', 'return fonts::type1(doc, font);', 'return Err("CFF disabled".into());', 'textedit_cff_maps_ascii_by_glyph_name_and_preserves_resources'),
    Mutation('CFF: trust internal encoding', 'src/textedit/fonts/cff.rs', 'let Some(&glyph) = names.get(name) else {', 'let Some(glyph) = face.glyph_index(code as u8) else {', 'textedit_cff_maps_ascii_by_glyph_name_and_preserves_resources'),
    Mutation('CFF: omit metadata validation', 'src/textedit/fonts/cff.rs', 'profile::validate(&bytes)?;', '// metadata unchecked', 'textedit_cff_refuses_unvalidated_program_semantics_and_permissions'),
    Mutation('CFF: ignore embedding restrictions', 'src/textedit/fonts/cff/profile.rs', 'if rights & !0x108 != 0 {', 'if false {', 'textedit_cff_metadata_only_accepts_editable_literal_permissions'),
    Mutation('CFF: ignore width disagreement', 'src/textedit/fonts/cff.rs', '|| (width - advance).abs() > 1.', '|| false', 'textedit_cff_requires_matching_pdf_font_contract'),
    Mutation('CFF: trust width-only space', 'src/textedit/fonts/cff.rs', 'match super::outlines::cff_bounds(&face, glyph) {', 'match if code == 32 { Ok(None) } else { super::outlines::cff_bounds(&face, glyph) } {', 'textedit_cff_missing_glyph_and_malformed_space_are_not_offered'),
    Mutation('CFF: drop ink overhang', 'src/textedit/fonts/cff.rs', 'overhangs[slot] = [left.min(0.), (right - width).max(0.)];', 'overhangs[slot] = [0., 0.];', 'textedit_cff_bounds_replacement_ink_and_preserves_failed_document'),
    Mutation('CFF Unicode: extend unverified CID mappings', 'src/textedit/fonts/mapping.rs', 'target.filter(|_| ch <= 0xff || ch == 0x2013)', 'target', 'textedit_cff_unicode_mapping_stays_bounded_and_font_specific'),
    Mutation('CFF Unicode: confuse minus with hyphen', 'src/textedit.rs', 'Some(0x80) // Internal metric slot only; never a literal WinAnsi byte.', 'Some(0x2d) // Wrong alias.', 'textedit_dash_slots_are_bijective_and_count_characters'),
    Mutation('CFF Unicode: substitute a different space glyph', 'src/textedit/fonts/cff.rs', 'let name = encoding.names[code];', 'let name = if slot == 160 { "space" } else { encoding.names[code] };', 'textedit_cff_unicode_requires_exact_glyph_names_widths_and_maps'),
    Mutation('CFF Unicode: offer NBSP without reader agreement', 'src/textedit/fonts/cff/encoding.rs', 'slot == 0xa0 ||', 'false ||', 'textedit_cff_unicode_missing_maps_glyphs_and_unmapped_fonts_never_guess'),
    Mutation('CFF Unicode: lose minus ink overhang', 'src/textedit/fonts/cff.rs', 'overhangs[slot] = [left.min(0.), (right - width).max(0.)];', 'overhangs[slot] = if slot == 128 { [0., 0.] } else { [left.min(0.), (right - width).max(0.)] };', 'textedit_cff_unicode_roundtrip_preserves_codes_ink_and_resources'),
    Mutation('CFF Unicode: extend unverified TrueType mappings', 'src/textedit/fonts/mapping.rs', 'cff && matches!', 'true && matches!', 'textedit_cff_unicode_mapping_stays_bounded_and_font_specific'),
    Mutation('CFF Unicode: forget blank NBSP', 'src/textedit/fonts/cff.rs', 'Ok(None) if matches!(slot, 32 | 160)', 'Ok(None) if slot == 32', 'textedit_cff_unicode_roundtrip_preserves_codes_ink_and_resources'),
    Mutation('CFF mapping: index ink by PDF code', 'src/textedit/fonts/cff.rs', 'overhangs[slot] = [left.min(0.), (right - width).max(0.)];', 'overhangs[code] = [left.min(0.), (right - width).max(0.)];', 'textedit_cff_remapped_metrics_follow_glyphs_at_both_code_boundaries'),
    Mutation('CFF mapping: ignore encoding differences', 'src/textedit/fonts/cff/encoding.rs', 'slots[code] = slot;', 'let _ = slot;', 'textedit_cff_custom_encoding_roundtrips_original_codes_and_word_spacing'),
    Mutation('CFF mapping: permit duplicate code assignments', 'src/textedit/fonts/cff/encoding.rs', 'if changed[code] {', 'if false {', 'textedit_cff_custom_encoding_refuses_malformed_or_ambiguous_differences'),
    Mutation('CFF mapping: permit duplicate glyph aliases', 'src/textedit/fonts/cff.rs', 'if result[slot].is_some() {', 'if false {', 'textedit_cff_custom_encoding_refuses_malformed_or_ambiguous_differences'),
    Mutation('CFF mapping: ignore Unicode disagreements', 'src/textedit/fonts/cff/encoding.rs', 'if target.is_some() && target != slot {', 'if false {', 'textedit_cff_tounicode_must_match_glyphs_and_never_guesses_missing_entries'),
    Mutation('CFF mapping: infer absent Unicode entries', 'src/textedit/fonts/cff/encoding.rs', '*slot = *target;', 'if target.is_some() { *slot = *target; }', 'textedit_cff_tounicode_must_match_glyphs_and_never_guesses_missing_entries'),
    Mutation('CFF mapping: forget remapped space', 'src/textedit/fonts/cff.rs', 'Ok(None) if matches!(slot, 32 | 160)', 'Ok(None) if code == 32', 'textedit_cff_custom_encoding_roundtrips_original_codes_and_word_spacing'),
    Mutation('CFF: allow duplicate metadata', 'src/textedit/fonts/cff/profile.rs', 'if !seen.insert(op) {', 'if !seen.insert(op) && false {', 'textedit_cff_metadata_rejects_ambiguous_partial_and_nondefault_dicts'),
    Mutation('span leaves: refuse literal span', 'src/textedit/tagging.rs', '!matches!(tag, b"NonStruct" | b"Span")', 'tag != b"NonStruct"', 'textedit_span_languages_and_structure_survive_edits_across_pages'),
    Mutation('span leaves: accept layout attributes', 'src/textedit/tagging.rs', 'let leaf = matches!(tag, b"NonStruct" | b"Span");', 'let leaf = tag == b"NonStruct";', 'textedit_spans_refuse_overrides_layout_nesting_and_wrong_owners_atomically'),
    Mutation('nested tags: allow any leaf role', 'src/textedit/tagging.rs', 'if plain.tag == b"Figure"\n                || annotation_owner(plain.tag)\n                || (!matches!(tag, b"NonStruct" | b"Span")\n                    && !annotation_owner(tag)\n                    && !(plain.tag == b"LI" && matches!(tag, b"Lbl" | b"LBody"))\n                    && !(matches!(plain.tag, b"TD" | b"TH")\n                        && (text_block(tag) || tag == b"Figure")))\n            {', 'if false {', 'textedit_nested_ownership_cycles_and_extra_levels_are_refused_atomically'),
    Mutation('nested tags: inherit an ancestor page', 'src/textedit/tagging.rs', '                name(get(child, b"S")?)?,\n            )?;', '                name(get(child, b"S")?)?,\n            )?;\n            let page = page.or(plain.page);', 'textedit_nested_optional_container_pages_and_scalar_children_are_explicit'),
    Mutation('nested tags: raise language limit', 'src/textedit/tagging.rs', 'bytes.len() > 63', 'bytes.len() > 64', 'textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text'),
    Mutation('nested tags: raise language component limit', 'src/textedit/tagging.rs', 'part.len() > 8', 'part.len() > 9', 'textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text'),
    Mutation('nested tags: skip language character check', 'src/textedit/tagging.rs', '!part.iter().all(u8::is_ascii_alphanumeric)', 'false', 'textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text'),
    Mutation('nested tags: allow stale next parent key', 'src/textedit/tagging.rs', 'next <= previous', 'next < previous', 'textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text'),
    Mutation('nested tags: ignore parent tree type', 'src/textedit/tagging.rs', 'value.as_name().ok() != Some(b"ParentTree")', 'false', 'textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text'),
    Mutation('nested tags: allow leaf layout attributes', 'src/textedit/tagging.rs', 'let leaf = matches!(tag, b"NonStruct" | b"Span");', 'let leaf = false;', 'textedit_nested_metadata_is_bounded_and_never_overrides_replacement_text'),
    Mutation('nested tags: raise total leaf limit', 'src/textedit/tagging.rs', 'entries.len() > MAX_CONTENT_ITEMS', 'entries.len() > MAX_CONTENT_ITEMS + 1', 'textedit_nested_total_content_limit_covers_all_leaf_groups'),
    Mutation('nested tags: discard direct paragraph content', 'src/textedit/tagging.rs', 'if !plain.items.is_empty() || groups.is_empty() {', 'if groups.is_empty() {', 'textedit_nested_mixed_leaf_ownership_keeps_each_authored_tag'),

    Mutation('stream patch: skip discovery boundary validation', 'src/textedit.rs', 'streams::rewrite(&bytes, &content, &BTreeSet::new())?;', '// discovery boundary unchecked', 'textedit_stream_discovery_refuses_unpatchable_nesting'),
    Mutation('stream patch: reserialize untouched operations', 'src/textedit.rs', 'streams::rewrite_expanded(&bytes, &content, &patched, &expansions)', '{ let _: &std::collections::BTreeMap<usize, Vec<lopdf::content::Operation>> = &expansions; content.encode().map_err(|e: lopdf::Error| e.to_string()) }', 'textedit_stream_patch_preserves_coordinates_comments_and_untouched_text_bytes'),
    Mutation('stream patch: ignore changed untouched operands', 'src/textedit/streams.rs', 'original.operations[0].operands != next.operands', 'false', 'textedit_stream_patch_refuses_disagreement_and_bounds_work'),
    Mutation('stream patch: raise container nesting limit', 'src/textedit/streams.rs', 'if depth > 32 {', 'if depth > 33 {', 'textedit_stream_patch_refuses_disagreement_and_bounds_work'),
    Mutation('stream patch: raise string nesting limit', 'src/textedit/streams.rs', 'if nesting > 32 {', 'if nesting > 33 {', 'textedit_stream_patch_refuses_disagreement_and_bounds_work'),
    Mutation('stream patch: ignore operator identity', 'src/textedit/streams.rs', '!same_operator && !compensated_show', 'false', 'textedit_stream_patch_refuses_disagreement_and_bounds_work'),
    Mutation('stream patch: omit trailing source bytes', 'src/textedit/streams.rs', 'output.extend_from_slice(&bytes[copied..]);', '// trailing source omitted', 'textedit_stream_patch_preserves_coordinates_comments_and_untouched_text_bytes'),

    Mutation('browser state: skip external state validation', 'src/textedit.rs', 'let width = graphics::normal(doc, resources, name)?;', 'let width: Option<f64> = None;', 'textedit_graphics_state_refuses_effects_bad_types_and_later_resets_atomically'),
    Mutation('browser state: admit unknown effects', 'src/textedit/graphics.rs', '_ => return Err(invalid()),', '_ => {}', 'textedit_graphics_state_refuses_effects_bad_types_and_later_resets_atomically'),
    Mutation('browser state: admit arbitrary blend mode', 'src/textedit/graphics.rs', 'if name == b"Normal"', 'if !name.is_empty()', 'textedit_graphics_state_refuses_effects_bad_types_and_later_resets_atomically'),
    Mutation('browser state: raise named state bound', 'src/textedit.rs', 'if graphics_states.len() >= 32 {', 'if graphics_states.len() >= 33 {', 'textedit_graphics_state_limits_names_and_refuses_missing_or_malformed_resources'),
    Mutation('browser state: skip stroke colour validation', 'src/textedit.rs', 'stroke_components = patterns::Colour::Solid(components);\n                colors::values(values, components)?;', 'stroke_components = patterns::Colour::Solid(components);', 'textedit_stroke_setters_validate_values_without_enabling_stroke_text'),
    Mutation('browser state: overwrite active fill space', 'src/textedit.rs', 'stroke_components = patterns::Colour::Solid(components);', 'stroke_components = patterns::Colour::Solid(components); fill_components = stroke_components;', 'textedit_normal_graphics_state_preserves_fill_geometry_and_saved_operators'),

    Mutation('glyph clip: omit quadratic control point', 'src/textedit/fonts/outlines.rs', 'self.point(x1, y1);\n        self.point(x, y);', 'self.point(x, y);', 'textedit_outline_envelope_includes_curve_controls_and_rejects_nonfinite_points'),
    Mutation('glyph clip: omit cubic control points', 'src/textedit/fonts/outlines.rs', 'self.point(x1, y1);\n        self.point(x2, y2);', '// controls omitted', 'textedit_outline_envelope_includes_curve_controls_and_rejects_nonfinite_points'),
    Mutation('glyph clip: omit simple bottom union', 'src/textedit/fonts.rs', 'vertical_bounds[0] = vertical_bounds[0].min(bottom * unit);', 'vertical_bounds[0] = 0.;', 'textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements'),
    Mutation('glyph clip: omit simple top union', 'src/textedit/fonts.rs', 'vertical_bounds[1] = vertical_bounds[1].max(top * unit);', 'vertical_bounds[1] = 700.;', 'textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements'),
    Mutation('glyph clip: omit composite bottom union', 'src/textedit/fonts/composite.rs', 'vertical_bounds[0] = vertical_bounds[0].min(bottom * unit);', 'vertical_bounds[0] = 0.;', 'textedit_composite_glyph_envelope_covers_fractional_and_unused_replacements'),
    Mutation('glyph clip: omit composite top union', 'src/textedit/fonts/composite.rs', 'vertical_bounds[1] = vertical_bounds[1].max(top * unit);', 'vertical_bounds[1] = 700.;', 'textedit_composite_glyph_envelope_covers_fractional_and_unused_replacements'),
    Mutation('glyph clip: truncate fractional outline points', 'src/textedit/fonts/outlines.rs', 'let (x, y) = (f64::from(x), f64::from(y));', 'let (x, y) = (f64::from(x as i16), f64::from(y as i16));', 'textedit_outline_bounds_preserve_fractional_components_and_ignore_header_boxes'),
    Mutation('glyph clip: reuse full em envelope', 'src/textedit.rs', "if clipping::contains(clip, ink_bounds).is_err() {", "if clipping::contains(clip, bounds).is_err() {", 'textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements'),
    Mutation('glyph clip: shrink lower glyph envelope', 'src/textedit.rs', 'let ink_bounds = text_bounds(\n                page_matrix,\n                [\n                    horizontal[0],\n                    bottom * size / 1000.,', 'let ink_bounds = text_bounds(\n                page_matrix,\n                [\n                    horizontal[0],\n                    bottom * size / 10000.,', 'textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements'),
    Mutation('glyph clip: shrink upper glyph envelope', 'src/textedit.rs', 'let ink_bounds = text_bounds(\n                page_matrix,\n                [\n                    horizontal[0],\n                    bottom * size / 1000.,\n                    horizontal[1],\n                    top * size / 1000.,', 'let ink_bounds = text_bounds(\n                page_matrix,\n                [\n                    horizontal[0],\n                    bottom * size / 1000.,\n                    horizontal[1],\n                    top * size / 10000.,', 'textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements'),

    Mutation('reflected: reject intermediate page reflection', 'src/textedit.rs', 'page_transform = compose_affine(page_transform, next)?;', 'if next[0] < 0. || next[3] < 0. { return Err("mutation".into()); } page_transform = compose_affine(page_transform, next)?;', 'textedit_reflections_cancel_before_hitboxes_and_preserve_other_operators'),
    Mutation('reflected: reject intermediate text reflection', 'src/textedit.rs', 'compose_affine([1., 0., 0., 1., 0., 0.], matrix)?;', 'if matrix[0] < 0. || matrix[3] < 0. { return Err("mutation".into()); } compose_affine([1., 0., 0., 1., 0., 0.], matrix)?;', 'textedit_reflections_cancel_before_hitboxes_and_preserve_other_operators'),
    Mutation('reflected: accept mirrored horizontal text', 'src/textedit.rs', '|| page_matrix[0] * page_matrix[3] - page_matrix[1] * page_matrix[2] <= 0.0', '|| (page_matrix[0] > 0. && page_matrix[3] < 0.)', 'preserved_text_matrices_allow_neighbour_edits_without_offering_skew_or_mirrors'),
    Mutation('reflected: accept mirrored vertical text', 'src/textedit.rs', '|| page_matrix[0] * page_matrix[3] - page_matrix[1] * page_matrix[2] <= 0.0', '|| (page_matrix[0] < 0. && page_matrix[3] > 0.)', 'preserved_text_matrices_allow_neighbour_edits_without_offering_skew_or_mirrors'),
    Mutation('reflected: skip horizontal clip normalization', 'src/textedit/clipping.rs', 'next[0].min(next[2])', 'next[0]', 'textedit_reflections_cancel_before_hitboxes_and_preserve_other_operators'),
    Mutation('reflected: skip vertical clip normalization', 'src/textedit/clipping.rs', 'next[1].min(next[3])', 'next[1]', 'textedit_reflections_cancel_before_hitboxes_and_preserve_other_operators'),
    Mutation('reflected: remove horizontal underflow guard', 'src/textedit.rs', '|| result[0] * result[3] - result[1] * result[2] == 0.', '|| (result[0] * result[3] - result[1] * result[2] == 0. && result[0] != 0.)', 'preserved_transform_bounds_cover_both_axes_and_singular_composition'),
    Mutation('reflected: remove vertical underflow guard', 'src/textedit.rs', '|| result[0] * result[3] - result[1] * result[2] == 0.', '|| (result[0] * result[3] - result[1] * result[2] == 0. && result[3] != 0.)', 'preserved_transform_bounds_cover_both_axes_and_singular_composition'),

    Mutation('cid: decode little endian', 'src/textedit/fonts.rs', '.get(&u16::from_be_bytes([pair[0], pair[1]]))', '.get(&u16::from_le_bytes([pair[0], pair[1]]))', 'textedit_composite_roundtrip_preserves_program_mapping_and_other_page'),
    Mutation('cid: encode little endian', 'src/textedit/fonts.rs', 'result.extend(code.to_be_bytes());', 'result.extend(code.to_le_bytes());', 'textedit_composite_roundtrip_preserves_program_mapping_and_other_page'),
    Mutation('cid: accept odd strings', 'src/textedit/fonts.rs', 'bytes.len() % 2 != 0 ||', 'false ||', 'textedit_composite_code_lengths_notdef_and_glyph_bounds'),
    # Input CID count and expanded character count enforce the same bound.
    # Remove both for the end-to-end length property; either alone still refuses.
    Mutation('cid: omit both string limits', 'src/textedit/fonts.rs', '        if let Codes::Double(codes) = codes {\n            if bytes.len() % 2 != 0 || bytes.len() / 2 > super::MAX_TEXT {\n                return Err("invalid or oversized two-byte text".into());\n            }\n            let mut text = String::new();\n            let mut characters = 0;\n            for pair in bytes.chunks_exact(2) {\n                let slot = *codes\n                    .get(&u16::from_be_bytes([pair[0], pair[1]]))\n                    .ok_or("text contains an unmapped font code")?;\n                if let Some(sequence) = ligatures::text(slot) {\n                    text.push_str(sequence);\n                    characters += sequence.len();\n                } else {\n                    text.push(super::slot_character(slot));\n                    characters += 1;\n                }\n                if characters > super::MAX_TEXT {\n                    return Err("expanded two-byte text exceeds its limit".into());\n                }\n            }\n            return Ok(text);\n        }\n', '        if let Codes::Double(codes) = codes {\n            if bytes.len() % 2 != 0 || false {\n                return Err("invalid or oversized two-byte text".into());\n            }\n            let mut text = String::new();\n            let mut characters = 0;\n            for pair in bytes.chunks_exact(2) {\n                let slot = *codes\n                    .get(&u16::from_be_bytes([pair[0], pair[1]]))\n                    .ok_or("text contains an unmapped font code")?;\n                if let Some(sequence) = ligatures::text(slot) {\n                    text.push_str(sequence);\n                    characters += sequence.len();\n                } else {\n                    text.push(super::slot_character(slot));\n                    characters += 1;\n                }\n                if false {\n                    return Err("expanded two-byte text exceeds its limit".into());\n                }\n            }\n            return Ok(text);\n        }\n', 'textedit_composite_code_lengths_notdef_and_glyph_bounds'),
    Mutation('cid: infer glyph from Unicode', 'src/textedit/fonts/composite.rs', 'let glyph = GlyphId(code);', 'let glyph = GlyphId(u16::from(ch));', 'textedit_composite_roundtrip_preserves_program_mapping_and_other_page'),
    Mutation('cid: ignore default width', 'src/textedit/fonts/composite.rs', '.copied().unwrap_or(default)', '.copied().unwrap_or(1000.)', 'textedit_composite_code_lengths_notdef_and_glyph_bounds'),
    Mutation('cid: skip width agreement', 'src/textedit/fonts/composite.rs', '(width - advance).abs() > 1.', 'false', 'textedit_composite_refusals_leave_every_object_unchanged'),
    Mutation('cid: skip glyph map identity', 'src/textedit/fonts/composite.rs', 'Object::Name(name) if name == b"Identity" => None,', 'Object::Name(_) => None,', 'textedit_composite_refusals_leave_every_object_unchanged'),
    Mutation('cid: skip font permissions', 'src/textedit/fonts/composite.rs', 'super::face(&bytes, false)?', 'ttf_parser::Face::parse(&bytes, 0).map_err(|_| INVALID)?', 'textedit_composite_checks_embedding_rights_and_program_format'),
    Mutation('cid: raise width table bound', 'src/textedit/fonts/composite.rs', 'const MAX_WIDTHS: usize = 4096;', 'const MAX_WIDTHS: usize = 4097;', 'textedit_composite_width_table_bounds_and_defaults'),
    Mutation('cid: accept duplicate codes', 'src/textedit/fonts/mapping.rs', 'result.insert(code, ch).is_some()', '{ result.insert(code, ch); false }', 'textedit_cid_mapping_rejects_ambiguity_expansion_and_wrong_width'),
    Mutation('dash: truncate CID Unicode targets', 'src/textedit/fonts/mapping.rs', 'char::from_u32(ch).and_then(super::super::character_slot)', 'char::from_u32(ch & 255).and_then(super::super::character_slot)', 'textedit_mapped_dash_uses_unicode_targets_not_low_bytes'),
    Mutation('dash: admit CID controls', 'src/textedit/fonts/mapping.rs', 'char::from_u32(ch).and_then(super::super::character_slot)', 'char::from_u32(ch).and_then(super::super::character_slot).or_else(|| u8::try_from(ch).ok())', 'textedit_cid_mapping_latin1_boundaries_and_ranges'),
    Mutation('dash: refuse existing CID Latin-1', 'src/textedit/fonts/mapping.rs', 'char::from_u32(ch).and_then(super::super::character_slot)', 'char::from_u32(ch).and_then(super::super::character_slot).filter(|_| ch <= 126 || ch == 0x2013)', 'textedit_cid_mapping_latin1_boundaries_and_ranges'),
    Mutation('stroke styles: skip validation call', 'src/textedit.rs', 'graphics::stroke(&op.operator, values)?', '()', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: accept invalid cap and join', 'src/textedit/graphics.rs', 'Object::Integer(0..=2)', 'Object::Integer(0..=3)', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: lower miter minimum', 'src/textedit/graphics.rs', 'number(value)? >= 1.', 'number(value)? >= 0.', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: omit dash array bound', 'src/textedit/graphics.rs', 'pattern.len() > 32', 'false', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: omit phase lower bound', 'src/textedit/graphics.rs', 'number(phase)? < 0.', 'number(phase)? < -1.', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: omit negative dash check', 'src/textedit/graphics.rs', 'if length < 0. {', 'if false {', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: accept all zero dash cycle', 'src/textedit/graphics.rs', 'if !pattern.is_empty() && !positive {', 'if false && !pattern.is_empty() && !positive {', 'textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically'),
    Mutation('stroke styles: refuse zero length dots', 'src/textedit/graphics.rs', 'if length < 0. {', 'if length <= 0. {', 'textedit_stroke_styles_preserve_scoped_operators_and_other_text'),
    Mutation('stroke styles: refuse boundary dash count', 'src/textedit/graphics.rs', 'pattern.len() > 32', 'pattern.len() >= 32', 'textedit_stroke_styles_preserve_scoped_operators_and_other_text'),

    Mutation('curves: omit first control point', 'src/textedit/clipping.rs', 'for pair in op.operands.chunks_exact(2) {', 'for pair in op.operands.chunks_exact(2).skip(1) {', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('curves: carry segments into new subpath', 'src/textedit/clipping.rs', '                segments = 0;', '                // segment count retained', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('curves: refuse coordinate boundary', 'src/textedit/clipping.rs', 'value.abs() > 1_000_000.', 'value.abs() >= 1_000_000.', 'textedit_curves_preserve_clip_and_validate_transformed_boundary'),
    Mutation('curves: forget closed subpath', 'src/textedit/clipping.rs', '                closed = true;', '                closed = false;', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('curves: skip transformed coordinate bounds', 'src/textedit/clipping.rs', 'if point\n        .iter()\n        .any(|value| !value.is_finite() || value.abs() > 1_000_000.)', 'if false', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('curves: accept extra coordinates', 'src/textedit/clipping.rs', 'if op.operands.len() != coordinates {', 'if op.operands.len() < coordinates {', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('curves: accept painting operands', 'src/textedit/clipping.rs', 'if drawn && op.operands.is_empty() =>', 'if drawn =>', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('stroked lines: discard clip', 'src/textedit.rs', "let consumed = clipping::path(&content.operations[index..], page_transform)?;", "let consumed = clipping::path(&content.operations[index..], page_transform)?; clip = None;", 'textedit_stroked_lines_cannot_replace_or_discard_a_clip'),
    Mutation('curves: consume following operator', 'src/textedit/clipping.rs', 'return Ok(index + 1);', 'return Ok(index + 2);', 'textedit_curves_preserve_complete_subpaths_and_following_text'),
    Mutation('stroked lines: count discard as tagged content', 'src/textedit.rs', 'if content.operations[index + consumed - 1].operator != "n" {', 'if true {', 'textedit_tagged_painted_content_preserves_structure_and_refuses_empty_items'),
    Mutation('curves: accept empty subpath', 'src/textedit/clipping.rs', 'if drawn && op.operands.is_empty() =>', 'if op.operands.is_empty() =>', 'textedit_curves_refuse_bad_control_points_and_partial_subpaths_atomically'),
    Mutation('painted rectangles: skip geometry validation', 'src/textedit/clipping.rs', '        rectangle(rect, ctm)?;', '    let _ = (rect, ctm);', 'textedit_painted_rectangles_refuse_invalid_geometry_and_incomplete_paths_atomically'),
    Mutation('painted rectangles: accept paint operands', 'src/textedit/clipping.rs', 'if !end.operands.is_empty() {', 'if false {', 'textedit_painted_rectangles_refuse_invalid_geometry_and_incomplete_paths_atomically'),
    Mutation('painted rectangles: discard an established clip', 'src/textedit.rs', 'path_until = index + rectangles_consumed;', 'path_until = index + rectangles_consumed; clip = None;', 'textedit_painted_rectangles_do_not_replace_or_discard_clipping'),
    Mutation('painted rectangles: consume a following operator', 'src/textedit.rs', 'path_until = index + rectangles_consumed;', 'path_until = index + rectangles_consumed + 1;', 'textedit_painted_rectangles_preserve_paint_state_and_surrounding_operators'),
    Mutation('painted rectangles: count discarded paths as tagged content', 'src/textedit.rs', 'if content.operations[index + rectangles_consumed - 1].operator != "n" {', 'if true {', 'textedit_tagged_painted_content_preserves_structure_and_refuses_empty_items'),
    Mutation('painted rectangles: omit tagged paint content', 'src/textedit.rs', 'if content.operations[index + rectangles_consumed - 1].operator != "n" {\n                        tags.paint();\n                    }', '// rectangle paint omitted', 'textedit_tagged_painted_content_preserves_structure_and_refuses_empty_items'),
    Mutation('overhang: omit replacement left boundary', 'src/textedit.rs', 'replacement_bounds[0] < original[0] || replacement_bounds[1] > original[1]', 'replacement_bounds[1] > original[1]', 'textedit_composite_overhang_replacements_stay_inside_original_ink_atomically'),
    Mutation('overhang: omit replacement right boundary', 'src/textedit.rs', 'replacement_bounds[0] < original[0] || replacement_bounds[1] > original[1]', 'replacement_bounds[0] < original[0]', 'textedit_composite_overhang_replacements_stay_inside_original_ink_atomically'),
    Mutation('overhang: discard measured excursions', 'src/textedit/fonts/composite.rs', '(left * unit).min(0.),\n                    (right * unit - width).max(0.),', '0., 0.,', 'textedit_composite_overhang_source_clips_follow_kerning_and_page_scale'),
    Mutation('overhang: remove left quarter em bound', 'src/textedit/fonts/composite.rs', 'left * unit >= -250.', 'true', 'textedit_composite_overhang_quarter_em_limits_have_boundary_controls'),
    Mutation('overhang: remove right quarter em bound', 'src/textedit/fonts/composite.rs', 'right * unit <= width + 250.', 'true', 'textedit_composite_overhang_quarter_em_limits_have_boundary_controls'),
    Mutation('cid: count kerning bytes as characters', 'src/textedit.rs', 'characters += fragment.chars().count();', 'characters += bytes.len();', 'textedit_composite_kerning_limit_counts_characters_across_strings'),

    Mutation('tagged indent: validate only end indent', 'src/textedit/tagging.rs', 'if let Ok(indent) = attributes.get(key) {', 'if let Ok(indent) = attributes.get(b"EndIndent") {', 'textedit_tagged_layout_spacing_rejects_invalid_values_and_document_scope'),
    Mutation('tagged indent: skip numeric validation', 'src/textedit/tagging.rs', 'super::number(indent)?;', '// unchecked indent', 'textedit_tagged_layout_spacing_rejects_invalid_values_and_document_scope'),
    Mutation('tagged indent: accept document scope', 'src/textedit/tagging.rs', 'if tag == b"Document" {', 'if false {', 'textedit_tagged_layout_spacing_rejects_invalid_values_and_document_scope'),
    Mutation('tagged indent: trim stale source text', 'src/textedit.rs', 'change.original != run.text', 'change.original.trim_end() != run.text.trim_end()', 'textedit_tagged_layout_spacing_and_source_spaces_survive_a_fitting_edit'),
    Mutation('tagged flow: allow empty role names', 'src/textedit/tagging.rs', 'key.is_empty()', 'false', 'textedit_tagged_empty_role_name_cannot_hide_duplicate_items'),
    Mutation('tagged flow: ignore explicit item page', 'src/textedit/tagging.rs', '(reference(get(mcr, b"Pg")?)?, integer(get(mcr, b"MCID")?)?)', '(paragraph_page.ok_or(INVALID)?, integer(get(mcr, b"MCID")?)?)', 'textedit_tagged_flowing_paragraph_preserves_every_item_and_page'),
    Mutation('tagged flow: ignore MCR type', 'src/textedit/tagging.rs', 'if name(get(mcr, b"Type")?)? != b"MCR" {', 'if false {', 'textedit_tagged_flowing_items_refuse_bad_ownership_without_mutation'),
    Mutation('tagged flow: ignore MCR field whitelist', 'src/textedit/tagging.rs', 'keys(mcr, &[b"Type", b"Pg", b"MCID"], "content reference")?;', '// fields unchecked', 'textedit_tagged_flowing_items_refuse_bad_ownership_without_mutation'),
    Mutation('tagged flow: reject preserved empty paragraph', 'src/textedit/tagging.rs', '            {\n                for item in items {\n                    if let Object::Dictionary(objr) = item {', '            {\n                if items.is_empty() {\n                    return Err(INVALID.into());\n                }\n                for item in items {\n                    if let Object::Dictionary(objr) = item {', 'textedit_tagged_flowing_items_refuse_bad_ownership_without_mutation'),
    Mutation('tagged flow: relax total item limit', 'src/textedit/tagging.rs', 'entries.len() > MAX_CONTENT_ITEMS', 'entries.len() > MAX_CONTENT_ITEMS + 1', 'textedit_tagged_limits_have_valid_boundary_controls'),
    Mutation("tagged flow: accept another element's slot", 'src/textedit/tagging.rs', 'if mcid >= names.len() || reference(&entries[mcid])? != id {', 'if mcid >= names.len() {', 'textedit_tagged_requires_both_parent_directions_and_bounded_unique_ids'),
    Mutation('number tree: ignore leaf Limits', 'src/textedit/tagging.rs', '|| integer(&limits[1])? != integer(&nums[nums.len() - 2])?', '', 'textedit_parent_number_tree_refuses_inconsistent_limits_order_and_cycles'),
    Mutation('number tree: allow Limits on the root', 'src/textedit/tagging.rs', 'if is_root {\n                &[b"Type", b"Nums", b"Kids"]', 'if false {\n                &[b"Type", b"Nums", b"Kids"]', 'textedit_parent_number_tree_refuses_inconsistent_limits_order_and_cycles'),
    Mutation('number tree: relax depth', 'src/textedit/tagging.rs', 'if depth > MAX_TREE_DEPTH ||', 'if depth > MAX_TREE_DEPTH + 1 ||', 'textedit_parent_number_tree_refuses_inconsistent_limits_order_and_cycles'),
    Mutation('annotations: accept a claim by another element', 'src/textedit/tagging.rs', '|| objects.remove(&key) != Some(owner)', '|| objects.remove(&key).is_none()', 'textedit_link_annotation_ownership_must_agree_in_both_directions'),
    Mutation('annotations: skip the page Annots listing', 'src/textedit/tagging.rs', '        || !listed\n', '', 'textedit_link_annotation_ownership_must_agree_in_both_directions'),
    Mutation('annotations: ignore the annotation subtype', 'src/textedit/tagging.rs', '        || subtype\n            != if tag == b"Link" {', '        || false && subtype\n            != if tag == b"Link" {', 'textedit_link_annotation_ownership_must_agree_in_both_directions'),
    Mutation('annotations: leave entries unclaimed', 'src/textedit/tagging.rs', 'if !objects.is_empty() {', 'if false {', 'textedit_link_annotation_ownership_must_agree_in_both_directions'),
    Mutation('annotations: offer link text for editing', 'src/textedit/tagging.rs', 'annotation_owner(&self.names[mcid])\n                    || self.names[mcid] == ORPHAN', 'self.names[mcid] == ORPHAN', 'textedit_link_text_stays_read_only_while_neighbouring_text_is_edited'),
    Mutation('orphans: offer orphaned text for editing', 'src/textedit/tagging.rs', 'annotation_owner(&self.names[mcid])\n                    || self.names[mcid] == ORPHAN', 'annotation_owner(&self.names[mcid])', 'textedit_orphaned_parent_tree_slots_stay_read_only_and_cannot_hide_reachable_owners'),
    Mutation('orphans: let a reachable element skip its slot', 'src/textedit/tagging.rs', '                if ids.contains(&id) {\n                    return Err(INVALID.into());', '                if false {\n                    return Err(INVALID.into());', 'textedit_orphaned_parent_tree_slots_stay_read_only_and_cannot_hide_reachable_owners'),
    Mutation('orphans: count used orphans as owned content', 'src/textedit/tagging.rs', '.filter(|&&mcid| self.names[mcid] != ORPHAN)', '.filter(|_| true)', 'textedit_orphaned_parent_tree_slots_stay_read_only_and_cannot_hide_reachable_owners'),
    Mutation('null slots: let an empty tag claim an unowned slot', 'src/textedit/tagging.rs', '.filter(|owner| !owner.is_empty());', ';', 'textedit_null_parent_tree_slots_are_unowned_and_cannot_be_used'),
    Mutation('artifacts: accept an artifact tag on an owned MCID', 'src/textedit/tagging.rs', 'if owner.is_none() || tag == b"Artifact" || !self.seen.insert(mcid) {', 'if owner.is_none() || !self.seen.insert(mcid) {', 'textedit_tagged_refusals_distinguish_marked_content_context'),
    Mutation('artifacts: accept any artifact property', 'src/textedit/tagging.rs', '                _ => false,\n            };\n            if !valid {', '                _ => true,\n            };\n            if !valid {', 'textedit_artifact_property_lists_keep_headers_read_only_inside_and_outside_text'),
    Mutation('alt: admit alternate text on paragraphs', 'src/textedit/tagging.rs', 'let mut pins = dict.has(b"Alt")\n', 'let mut pins = false\n', 'textedit_alternate_text_and_titles_pin_the_content_they_describe'),
    Mutation('titles: admit titles on editable text', 'src/textedit/tagging.rs', '        || (owns_text\n            && dict\n                .get(b"T")', '        || (false\n            && dict\n                .get(b"T")', 'textedit_alternate_text_and_titles_pin_the_content_they_describe'),
    Mutation('namespaces: accept any namespace URI', 'src/textedit/tagging.rs', '                    || !matches!(\n                        get(namespace, b"NS")?.as_str().map_err(|_| INVALID)?,\n                        b"http://iso.org/pdf2/ssn" | b"http://iso.org/pdf/ssn"\n                    )\n', '', 'textedit_standard_namespaces_are_accepted_and_others_refused'),
    Mutation('namespaces: accept PDF 2.0-only types', 'src/textedit/tagging.rs', '|| !common_to_both_namespaces(tag)', '', 'textedit_pdf2_namespace_only_admits_types_whose_meaning_is_shared'),
    Mutation('table sections: resolve headers to the section', 'src/textedit/tagging.rs', '                if table_section(name(get(node(doc, table)?, b"S")?)?) {\n                    table = reference(get(node(doc, table)?, b"P")?)?;', '                if false {\n                    table = reference(get(node(doc, table)?, b"P")?)?;', 'textedit_headers_link_across_table_sections_of_one_table'),
    Mutation('figures: accept any size value', 'src/textedit/tagging.rs', '                value.as_name().ok() != Some(b"Auto")\n                    && !super::number(value)', '                false\n                    && !super::number(value)', 'textedit_word_lists_nest_directly_and_cells_hold_lists_and_figures'),
    Mutation('WinAnsi: select glyphs by PDF code', 'src/textedit/fonts.rs', 'u32::from(super::slot_character(byte))', 'u32::from(code_byte)', 'textedit_winansi_punctuation_uses_unicode_glyphs_and_writes_winansi_codes'),
    Mutation('WinAnsi: require Mac maps to agree above ASCII', 'src/textedit/fonts.rs', '.filter(|table| code < 128 || table.is_unicode())', '.filter(|_| true)', 'textedit_winansi_punctuation_uses_unicode_glyphs_and_writes_winansi_codes'),
    Mutation('WinAnsi: accept the space alias', 'src/textedit/fonts/mapping.rs', 'matches!(code, 0x80 | 0xA0 | 0xAD) || code < 32', 'matches!(code, 0x80 | 0xAD) || code < 32', 'textedit_winansi_mapping_must_restate_winansi_and_unicode_cmaps_must_agree'),
    Mutation('continued: store the whole compensation in one f32', 'src/textedit.rs', 'let whole = wanted.trunc();', 'let whole = 0_f64;', 'textedit_continued_discovery_requires_representable_deletion'),
    Mutation('tagged pages: select the first page map', 'src/textedit/tagging.rs', '            .remove(&page)\n            .map(|(_, names)| names)', '            .remove(&pages[0])\n            .map(|(_, names)| names)', 'textedit_untagged_page_in_a_tagged_document_edits_as_plain_text'),
    Mutation('tagged pages: bind paragraphs to the requested page', 'src/textedit/tagging.rs', 'by_page.get_mut(&owner).ok_or(INVALID)?', 'by_page.get_mut(&page).ok_or(INVALID)?', 'textedit_tagged_pages_keep_local_ids_and_shared_streams_isolated'),
    Mutation('tagged pages: accept unsorted number tree', 'src/textedit/tagging.rs', 'if key <= previous || !(0..=1_000_000).contains(&key) {', 'if !(0..=1_000_000).contains(&key) {', 'textedit_tagged_multi_page_parent_keys_and_owners_must_agree'),
    Mutation('tagged pages: ignore trailing number tree key', 'src/textedit/tagging.rs', 'if page_entries.len() != page_keys.len()\n            || page_entries', 'if false\n            && page_entries', 'textedit_parent_tree_refusals_identify_unclaimed_annotation_entries'),

    Mutation('tagged: omit paragraph count limit', 'src/textedit/tagging.rs', 'entries.len() > MAX_CONTENT_ITEMS', 'entries.len() > MAX_CONTENT_ITEMS + 1', 'textedit_tagged_limits_have_valid_boundary_controls'),
    Mutation('tagged: ignore semantic dictionary fields', 'src/textedit/tagging.rs', '.find(|(key, _)| !allowed.contains(&key.as_slice()))', '.find(|_| false)', 'textedit_tagged_refuses_semantic_overrides_and_stale_layout_attributes'),
    Mutation('tagged: ignore parent element reference', 'src/textedit/tagging.rs', '|| reference(get(dict, b"P")?)? != parent', '|| false', 'textedit_tagged_requires_both_parent_directions_and_bounded_unique_ids'),
    Mutation('tagged: ignore reverse parent tree reference', 'src/textedit/tagging.rs', '|| reference(&entries[mcid])? != id', '|| false', 'textedit_tagged_requires_both_parent_directions_and_bounded_unique_ids'),
    Mutation('tagged: offer artifact text for editing', 'src/textedit/tagging.rs', '            None => !self.names.is_empty(),', '            None => false,', 'textedit_tagged_artifact_and_unmarked_text_remain_read_only'),
    Mutation('tagged: omit marker completeness call', 'src/textedit.rs', 'tags.finish()?;', '// finish omitted', 'textedit_tagged_requires_balanced_unique_markers_and_paragraph_text'),
    Mutation('tagged: allow repeated marked content id', 'src/textedit/tagging.rs', '|| !self.seen.insert(mcid)', '|| { self.seen.insert(mcid); false }', 'textedit_tagged_requires_balanced_unique_markers_and_paragraph_text'),
    Mutation('tagged: ignore page parent key', 'src/textedit/tagging.rs', 'let key = integer(key)?;', 'let key: i64 = 0;', 'textedit_tagged_requires_both_parent_directions_and_bounded_unique_ids'),

    Mutation('clip: omit text containment call', 'src/textedit.rs', "if clipping::contains(clip, ink_bounds).is_err() {", "if false {", 'textedit_clip_intersections_contain_every_side_of_text'),
    Mutation('clip: discard saved clip', 'src/textedit.rs', "states.push((\n                    selected_font,\n                    leading,\n                    page_transform,\n                    fill_components,\n                    clip,\n                    spacing,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", "states.push((\n                    selected_font,\n                    leading,\n                    page_transform,\n                    fill_components,\n                    None,\n                    spacing,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", 'textedit_clip_transform_is_fixed_at_creation_and_restored_by_q'),
    Mutation('clip: allow unproven glyph bounds', 'src/textedit.rs', '.vertical_bounds\n                .ok_or(', '.vertical_bounds.or(Some([0., 700.]))\n                .ok_or(', 'textedit_clips_require_proven_outline_bounds_not_only_advances'),
    Mutation('clip: replace intersection with union', 'src/textedit/clipping.rs', 'old[2].min(next[2])', 'old[2].max(next[2])', 'textedit_clip_intersections_contain_every_side_of_text'),
    Mutation('clip: omit rectangle horizontal scale', 'src/textedit/clipping.rs', '(x + width) * ctm[0] + ctm[4]', '(x + width) + ctm[4]', 'textedit_clip_transform_is_fixed_at_creation_and_restored_by_q'),
    Mutation('clip: accept painted path ending', 'src/textedit/clipping.rs', '|| end.operator != "n"', '|| false', 'textedit_clips_refuse_partial_compound_painted_and_unbounded_paths'),
    Mutation('clip: omit transformed coordinate limit', 'src/textedit/clipping.rs', 'if next.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {', 'if false {', 'textedit_clips_refuse_partial_compound_painted_and_unbounded_paths'),
    Mutation('clip: accept negative line width', 'src/textedit/clipping.rs', 'if super::number(value)? < 0. {', 'if super::number(value)? < -100. {', 'textedit_clips_refuse_partial_compound_painted_and_unbounded_paths'),

    Mutation('dash: wrong reverse Unicode', 'src/textedit.rs', 'WINANSI_EXTRA.iter().find(|(extra, _)| *extra == slot) {\n        ch', 'WINANSI_EXTRA.iter().find(|(extra, _)| *extra == slot) {\n        char::from(slot)', 'textedit_dash_slots_are_bijective_and_count_characters'),
    Mutation('dash: retain two-byte UTF8 bound', 'src/textedit.rs', 'text.len() > MAX_TEXT * 3', 'text.len() > MAX_TEXT * 2', 'textedit_dash_slots_are_bijective_and_count_characters'),
    Mutation('dash: write unproven unmapped slot', 'src/textedit/fonts.rs', 'super::decode_text(&bytes)?;', '// unmapped slot unchecked', 'textedit_symbolic_dash_roundtrip_keeps_original_font_codes_and_ink'),
    Mutation('mapped: write Unicode instead of font codes', 'src/textedit.rs', 'Ok((metrics.items(replacement, gap)?, advance, bounds))', 'Ok((vec![Object::string_literal(encode_text(replacement)?)], advance, bounds))', 'textedit_symbolic_codes_round_trip_without_changing_font_resources'),
    Mutation('mapped: decode bytes without font mapping', 'src/textedit/fonts.rs', 'let Some(slot) = codes[code as usize] else {', 'let Some(slot) = Some(code) else {', 'textedit_symbolic_codes_round_trip_without_changing_font_resources'),
    Mutation('mapped: permit competing font cmaps', 'src/textedit/fonts.rs', 'if custom && cmap.subtables.len() != 1 {', 'if false {', 'textedit_symbolic_codes_require_unambiguous_glyph_selection_and_widths'),
    Mutation('mapped: omit text length bound', 'src/textedit/fonts.rs', 'if bytes.len() > super::MAX_TEXT {', 'if false {', 'textedit_symbolic_codes_refuse_unmapped_and_oversized_runs'),
    Mutation('mapped: permit duplicate Unicode values', 'src/textedit/fonts/mapping.rs', '|| unicode[ch as usize]', '|| false', 'textedit_mapping_refuses_ambiguous_partial_and_extended_data'),
    Mutation('mapped: permit duplicate source codes', 'src/textedit/fonts/mapping.rs', 'result[code as usize].is_some()', 'false', 'textedit_mapping_refuses_ambiguous_partial_and_extended_data'),
    Mutation('mapped: ignore declared entry count', 'src/textedit/fonts/mapping.rs', 'block[1].operands.len() != *count as usize * stride {\n            return Err(invalid());\n        }\n        let byte', 'false {\n            return Err(invalid());\n        }\n        let byte', 'textedit_mapping_refuses_ambiguous_partial_and_extended_data'),
    Mutation('mapped: ignore wrapper operands', 'src/textedit/fonts/mapping.rs', 'actual.operands != expected.operands && !padded_range', 'false && !padded_range', 'textedit_mapping_refuses_ambiguous_partial_and_extended_data'),
    Mutation('single ranges: omit reversed-endpoint guard', 'src/textedit/fonts/mapping.rs', 'if last < first || !(u32::from(target)..=end_target).all(allowed) {', 'if !(u32::from(target)..=end_target).all(allowed) {', 'textedit_single_ranges_reject_bad_endpoints_counts_and_overlap'),
    Mutation('dash: accept control characters', 'src/textedit.rs', 'u8::try_from(ch as u32).ok().filter(|&byte| text_byte(byte))', 'u8::try_from(ch as u32).ok()', 'textedit_dash_slots_are_bijective_and_count_characters'),
    Mutation('single ranges: allow unproven Latin-1 targets', 'src/textedit/fonts/mapping.rs', '(32..=126).contains(&ch)\n                    || ch == 0x2013', '(32..=255).contains(&ch)\n                    || ch == 0x2013', 'textedit_mapping_refuses_ambiguous_partial_and_extended_data'),
    Mutation('single ranges: drop range endpoint', 'src/textedit/fonts/mapping.rs', 'for code in first..=last {\n                let ch = super::super::character_slot(', 'for code in first..last {\n                let ch = super::super::character_slot(', 'textedit_single_ranges_expand_exactly_and_mix_with_bfchar'),
    Mutation('mapped: raise decoding bound', 'src/textedit/fonts/mapping.rs', 'super::filters::decode(stream, MAX_MAP)?', 'super::filters::decode(stream, MAX_MAP * 2)?', 'textedit_mapping_bounds_plain_and_compressed_streams'),

    Mutation("textedit: offer notdef as a MacRoman glyph", "src/textedit/fonts.rs", "if glyph.0 == 0 {", "if false {", "textedit_macroman_format6_preserves_font_and_refuses_notdef"),
    Mutation("textedit: ignore present font permissions", "src/textedit/fonts.rs", "if rights & !0x108 != 0 {", "if false {", "textedit_macroman_requires_matching_encoding_and_honours_present_rights"),
    Mutation('textedit: forget saved fill colour space', 'src/textedit.rs', "states.push((\n                    selected_font,\n                    leading,\n                    page_transform,\n                    fill_components,\n                    clip,\n                    spacing,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", "states.push((selected_font, leading, page_transform, patterns::Colour::Solid(1), clip, spacing, word_spacing, stroke_components, compound_clips.clone(), (render, line_width)));", 'textedit_colours_restore_state_and_preserve_operators'),
    Mutation("textedit: omit colour component count", "src/textedit/colors.rs", "if values.len() != components {", "if false {", "textedit_colours_refuse_bad_components_and_unsupported_spaces"),
    Mutation("textedit: omit colour component range", "src/textedit/colors.rs", "if !(0.0..=1.0).contains(&number(value)?) {", "if false {", "textedit_colours_refuse_bad_components_and_unsupported_spaces"),
    Mutation("textedit: omit ICC header signature", "src/textedit/colors.rs", "|| &bytes[36..40] != b\"acsp\"", "|| false", "textedit_icc_checks_header_range_and_preserves_profile_bytes"),
    Mutation("textedit: raise ICC decode limit", "src/textedit/colors.rs", "filters::decode(profile, super::MAX_CONTENT)?", "filters::decode(profile, super::MAX_CONTENT * 2)?", "textedit_icc_decoding_and_colour_space_count_are_bounded"),
    Mutation("textedit: omit colour space count", "src/textedit.rs", "if colour_spaces.len() >= 32 {", "if false {", "textedit_icc_decoding_and_colour_space_count_are_bounded"),

    Mutation('textedit: reverse kerning adjustment', 'src/textedit.rs', 'advance -= value * size / 1000.0;', 'advance += value * size / 1000.0;', 'textedit_kerning_geometry_and_rewrite_preserve_other_shows'),
    Mutation('textedit: omit kerning text size', 'src/textedit.rs', 'advance -= value * size / 1000.0;', 'advance -= value / 1000.0;', 'textedit_kerning_geometry_and_rewrite_preserve_other_shows'),
    Mutation("textedit: omit kerning character bound", "src/textedit.rs", "if characters > MAX_TEXT {", "if false {", "textedit_kerning_bounds_total_characters_and_array_items"),
    Mutation("textedit: omit kerning item bound", "src/textedit.rs", "|| values.len() > MAX_TEXT", "|| false", "textedit_kerning_bounds_total_characters_and_array_items"),
    Mutation("textedit: omit accumulated kerning bound", "src/textedit.rs", "if !advance.is_finite() || advance.abs() > 1_000_000.0 {", "if false {", "textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays"),
    Mutation('textedit: write string instead of kerning array', 'src/textedit.rs', '} else if show.operator == "TJ" || items.len() > 1 {', '} else if items.len() > 1 {', 'textedit_kerning_geometry_and_rewrite_preserve_other_shows'),

    Mutation("textedit journal: retain a restored operand", "src/docmodel.rs", "self.text_edits.remove(&(page, operator));", "let _ = (page, operator);", "textedit_journal_restores_original_and_discards_abandoned_bodies"),
    Mutation("textedit journal: retain discarded redo bodies", "src/docmodel.rs", "self.text_versions.remove(&version);", "let _ = version;", "textedit_journal_restores_original_and_discards_abandoned_bodies"),
    Mutation("textedit journal: remove the history bound", "src/docmodel.rs", "self.text_versions.len() - discarded >= MAX_TEXT_VERSIONS", "self.text_versions.len() - discarded >= usize::MAX", "textedit_journal_bounds_history_but_reclaims_the_redo_tail"),
    Mutation("textedit journal: omit extraction filtering", "src/edits.rs", "page.source == PageSource::Baseline(change.page)", "true", "textedit_plans_follow_page_identity_and_filter_deleted_or_extracted_pages"),
    Mutation("textedit journal: accept a changed original", "src/docmodel.rs", "previous.revision != change.revision || previous.original != change.original", "previous.revision != change.revision", "textedit_journal_refuses_stale_or_unbounded_input_atomically"),
    Mutation("textedit: let print use the original bytes", "src/edits.rs", "pub fn is_identity(&self) -> bool {", "pub fn is_identity(&self) -> bool {\n        if !self.text_edits.is_empty() { return true; }", "textedit_reaches_save_copy_print_and_forbids_append"),
    Mutation("textedit: allow mixed edits to append", "src/edits.rs", "pub fn is_appendable(&self) -> bool {\n        if !self.forms.is_empty() || !self.text_edits.is_empty() {", "pub fn is_appendable(&self) -> bool {\n        if !self.forms.is_empty() {", "textedit_reaches_save_copy_print_and_forbids_append"),
    Mutation("text preview: omit the worker rewrite", "src/textview.rs", "crate::textedit::write(&mut document, changes)?;", "let _ = changes;", "textview_rewrites_without_changing_the_original_and_bounds_output"),
    Mutation("text preview: count retained bytes as free budget", "src/textview.rs", "MAX_PREVIEW_BYTES.saturating_sub(self.0.len())", "MAX_PREVIEW_BYTES.saturating_add(self.0.len())", "textview_rewrites_without_changing_the_original_and_bounds_output"),
    # Both passes reject partial input: strict decoding and the byte-preserving
    # writer's complete token walk. Weakening only the first is masked by the
    # second. Remove both for this end-to-end completion property; the stream
    # patch mutations separately exercise the writer's boundary checks.
    Mutation("textedit: bypass both content completion checks", "src/textedit.rs", '    let content = Content::decode_strict(&bytes).map_err(|e| e.to_string())?;\n    if content.operations.len() > MAX_OPERATIONS {\n        return Err("text operator count exceeds its limit".into());\n    }\n    // Discovery promises that deletion can use the byte-preserving writer too.\n    streams::rewrite(&bytes, &content, &BTreeSet::new())?;\n', '    let content = Content::decode(&bytes).map_err(|e| e.to_string())?;\n    if content.operations.len() > MAX_OPERATIONS {\n        return Err("text operator count exceeds its limit".into());\n    }\n    // Discovery promises that deletion can use the byte-preserving writer too.\n', "textedit_rejects_partial_or_undecodable_content"),
    Mutation("textedit: omit the writer call", "src/save.rs", "    crate::textedit::write(&mut doc, &plan.text_edits)?;", "    // text replacement omitted", "textedit_reaches_save_copy_print_and_forbids_append"),
    Mutation("textedit: keep unreachable old content", "src/save.rs", "        || !plan.text_edits.is_empty()", "        || false", "textedit_sweeps_old_streams_and_rejects_stale_or_redaction_plans"),
    Mutation("textedit: accept a stale content revision", "src/textedit.rs", "change.revision != runs.revision || change.original != run.text", "change.original != run.text", "textedit_rejects_invalid_batches_without_mutating_the_document"),

    Mutation("textedit: accept a missing embedded glyph", "src/textedit/fonts.rs", '.ok_or("the font has no validated glyph for this character")?', '.unwrap_or(600.)', "textedit_embedded_missing_glyph_and_own_width_overflow_leave_document_untouched"),
    Mutation("textedit: ignore conflicting Unicode cmaps", "src/textedit/fonts.rs", '.any(|table| table.glyph_index(code) != Some(glyph))', '.any(|table| { let _ = (table, code, glyph); false })', "textedit_embedded_requires_unicode_cmaps_to_agree"),
    Mutation('default encoding: use WinAnsi for omitted encoding', 'src/textedit.rs', 'None => Ok(metrics.standard_encoding()),', 'None => Ok(metrics),', 'textedit_default_helvetica_preserves_resources_and_mapped_text'),
    Mutation('default encoding: accept an explicit unsupported encoding', 'src/textedit.rs', '_ => Err("unsupported standard font encoding".into()),', '_ => Ok(metrics.standard_encoding()),', 'textedit_default_helvetica_refuses_explicit_or_custom_encodings'),
    Mutation('default encoding: interpret curly quote codes as ASCII', 'src/textedit/fonts.rs', 'if !matches!(code, 39 | 96) {', 'if true {', 'textedit_default_helvetica_maps_codes_and_widths'),
    Mutation('default encoding: retain widths for unavailable characters', 'src/textedit/fonts.rs', 'if !codes.contains(&Some(ch as u8)) {', 'if false {', 'textedit_default_helvetica_maps_codes_and_widths'),

    Mutation("textedit: treat a malformed space as blank", "src/textedit/fonts.rs", "empty_glyph(&face, glyph) == Some(true)", "true", "textedit_embedded_does_not_treat_a_broken_space_as_blank"),
    Mutation('textedit: discard restored text state', 'src/textedit.rs', "(\"Q\", []) if !inside => {\n                (\n                    selected_font,\n                    leading,\n                    page_transform,\n                    fill_components,\n                    clip,\n                    spacing,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips,\n                    (render, line_width),\n                ) =", '("Q", []) if !inside => {\n                let _ =', 'textedit_graphics_stack_restores_font_size_and_leading'),
    Mutation("textedit: allow unclosed graphics state", "src/textedit.rs", 'if !states.is_empty() {', 'if false {', "textedit_graphics_stack_requires_balanced_bounded_outer_saves"),
    Mutation("textedit: select last font instead of restored font", "src/textedit.rs", 'content.operations[*font_operator].operands[0]', 'content.operations[{ let _ = font_operator; content.operations[..change.operator as usize].iter().rposition(|op| op.operator == "Tf").unwrap() }].operands[0]', "textedit_graphics_restore_uses_the_active_font_for_replacement"),
    Mutation('rotation: omit line movement across the x axis', "src/textedit.rs", 'x * matrix[0] + y * matrix[2]', 'x * matrix[0]', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('rotation: omit line movement across the y axis', "src/textedit.rs", 'x * matrix[1] + y * matrix[3]', 'y * matrix[3]', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('rotation: omit rotated vertical page scale', "src/textedit.rs", 'inner[1] * outer[3]', 'inner[1]', 'textedit_rotation_composes_page_scales_and_preserves_replacement_placement'),
    Mutation('rotation: omit rotated horizontal page scale', "src/textedit.rs", 'inner[2] * outer[0]', 'inner[2]', 'textedit_rotation_composes_page_scales_and_preserves_replacement_placement'),
    Mutation('rotation: admit text skew', "src/textedit.rs", '(matrix[1] == 0. && matrix[2] == 0. &&', '(true &&', 'textedit_rotation_refuses_skew_mirrors_collapse_and_accumulated_overflow'),
    Mutation('rotation: lose negative x extent', "src/textedit.rs", 'ys[0].min(ys[1])', 'ys[0]', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('rotation: lose negative y extent', "src/textedit.rs", 'xb[0].min(xb[1])', 'xb[0]', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('rotation: lose positive x extent', "src/textedit.rs", 'ys[0].max(ys[1])', 'ys[1]', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('rotation: lose positive y extent', "src/textedit.rs", 'xb[0].max(xb[1])', 'xb[1]', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('rotation: omit glyph clip rotation', "src/textedit.rs", 'let ink_bounds = text_bounds(\n                page_matrix,', 'let ink_bounds = text_bounds(\n                [1., 0., 0., 1., page_matrix[4], page_matrix[5]],', 'textedit_simple_glyph_envelope_covers_fractional_and_unused_replacements'),

    Mutation("textedit: omit translated text position", "src/textedit.rs", 'compose_orthogonal(page_transform, shown_matrix)?', 'shown_matrix', "textedit_translated_hitboxes_match_absolute_positions_after_crop_and_rotation"),
    Mutation('textedit: forget saved page translation', 'src/textedit.rs', "states.push((\n                    selected_font,\n                    leading,\n                    page_transform,\n                    fill_components,\n                    clip,\n                    spacing,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", "states.push((selected_font, leading, [1., 0., 0., 1., 0., 0.], fill_components, clip, spacing, word_spacing, stroke_components, compound_clips.clone(), (render, line_width)));", 'textedit_page_translations_compose_restore_and_preserve_following_runs'),
    Mutation("textedit: accept collapsed page transforms", "src/textedit.rs", '|| result[0] * result[3] - result[1] * result[2] == 0.', '', "textedit_page_transforms_refuse_unbounded_matrices_and_preserve_restored_state"),

    Mutation("textedit: compose page translations in reverse order", "src/textedit.rs", 'inner[4] * outer[0] + inner[5] * outer[2] + outer[4]', 'outer[4] * inner[0] + outer[5] * inner[2] + inner[4]', "textedit_page_scales_compose_restore_and_preserve_following_runs"),
    Mutation('textedit: omit hitbox horizontal scale', 'src/textedit.rs', 'let xs = [left * a, right * a];', 'let xs = [left, right];', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation('textedit: omit hitbox vertical scale', 'src/textedit.rs', 'let yd = [bottom * d, top * d];', 'let yd = [bottom, top];', 'textedit_rotation_positions_and_hitboxes_cover_every_quarter_turn'),
    Mutation("textedit: allow unbounded composed scales", "src/textedit.rs", 'if result\n        .iter()\n        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)', 'if false', "textedit_page_scales_bound_composed_scales_and_positions"),

    Mutation("textedit: accept custom text rise", "src/textedit.rs", '("Ts", [value]) if number(value)? == 0.0', '("Ts", [_])', "textedit_default_setters_refuse_nondefault_and_malformed_operands"),
    Mutation("textedit: accept custom horizontal text scale", "src/textedit.rs", '("Tz", [value]) if number(value)? == 100.0', '("Tz", [_])', "textedit_default_setters_refuse_nondefault_and_malformed_operands"),
    Mutation("textedit: accept hidden or clipping text modes", "src/textedit.rs", 'Object::Integer(mode @ 0..=3)', 'Object::Integer(mode @ 0..=7)', "textedit_stroke_styles_refuse_invalid_state_even_before_resets_atomically"),
    Mutation("textedit: default setter positions a following show", "src/textedit.rs", '("Ts", [value]) if number(value)? == 0.0 => {}', '("Ts", [value]) if number(value)? == 0.0 => { positioned = true; }', "textedit_default_setters_do_not_position_a_following_show"),

    Mutation('image: skip image validation', 'src/textedit.rs', 'let image = images::check(doc, resources, name, MAX_IMAGES - image_bytes)?;', 'let image = images::Image { bytes: 0, stencil: false };', 'textedit_images_refuse_masks_forms_and_malformed_samples_atomically'),
    Mutation('image: reset per-page decoded budget', 'src/textedit.rs', 'images::check(doc, resources, name, MAX_IMAGES - image_bytes)?', 'images::check(doc, resources, name, MAX_IMAGES)?', 'textedit_images_share_decode_budget_and_bound_resource_names'),
    Mutation('image: omit distinct image name limit', 'src/textedit.rs', 'if image_names.len() >= 32 {', 'if false {', 'textedit_images_share_decode_budget_and_bound_resource_names'),
    Mutation('image: omit dimension ceiling', 'src/textedit/images.rs', 'if !(1..=8192).contains(&value) {', 'if value <= 0 {', 'textedit_images_share_decode_budget_and_bound_resource_names'),
    Mutation('image: allow missing samples', 'src/textedit/images.rs', 'if decoded.len() != samples', 'if false', 'textedit_images_refuse_masks_forms_and_malformed_samples_atomically'),
    Mutation('image: skip dictionary validation', 'src/textedit/images.rs', 'for (key, value) in &image.dict {', 'for (key, value) in image.dict.iter().take(0) {', 'textedit_images_refuse_masks_forms_and_malformed_samples_atomically'),
    Mutation('bounded attributes: refuse a link its own layout owner', 'src/textedit/tagging.rs', 'b"Figure" | b"Link" | b"Form" | b"Table"', 'b"Figure"', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('bounded attributes: skip the attribute owner', 'src/textedit/tagging.rs', '        if name(get(attributes, b"O")?)? != b"Layout" {\n            return Err(INVALID.into());\n        }', '', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('bounded attributes: accept any placement', 'src/textedit/tagging.rs', '            !value.as_name().is_ok_and(|name| {\n                matches!(name, b"Block" | b"Inline" | b"Before" | b"Start" | b"End")\n            })', 'false', 'textedit_bounded_tables_refuse_malformed_bounds_and_placements'),
    Mutation('bounded attributes: accept an inverted box', 'src/textedit/tagging.rs', 'if bounds[0] > bounds[2] || bounds[1] > bounds[3] {', 'if false {', 'textedit_bounded_tables_refuse_malformed_bounds_and_placements'),
    Mutation('bounded attributes: accept a box of any length', 'src/textedit/tagging.rs', '    if bounds.len() != 4 {\n        return Err(INVALID.into());\n    }', '', 'textedit_bounded_tables_refuse_malformed_bounds_and_placements'),
    Mutation('bounded attributes: accept any authored size', 'src/textedit/tagging.rs', '            if attributes.get(key).is_ok_and(|value| {\n                value.as_name().ok() != Some(b"Auto")\n                    && !super::number(value).is_ok_and(|size| size.is_finite() && size >= 0.0)\n            }) {', '            if false {', 'textedit_bounded_tables_refuse_malformed_bounds_and_placements'),
    Mutation('pins: titles pin grouping elements', 'src/textedit/tagging.rs', '        || (owns_text\n            && dict\n                .get(b"T")', '        || (true\n            && dict\n                .get(b"T")', 'textedit_alternate_text_and_titles_pin_the_content_they_describe'),
    Mutation('pins: forget a pin in the walk', 'src/textedit/tagging.rs', 'let bounded = bounded || pins;', 'let bounded = bounded;', 'textedit_alternate_text_and_titles_pin_the_content_they_describe'),
    Mutation('pins: forget a leaf pin', 'src/textedit/tagging.rs', 'pinned: plain.pinned || pins,', 'pinned: plain.pinned,', 'textedit_alternate_text_and_titles_pin_the_content_they_describe'),
    Mutation('pins: release a pinned list body sublist', 'src/textedit/tagging.rs', 'nested.push((item, plain.id, plain.pinned));', 'nested.push((item, plain.id, false));', 'textedit_pinned_list_bodies_keep_their_sublists'),
    Mutation('pins: accept an unbounded title', 'src/textedit/tagging.rs', '[(b"T".as_slice(), 4096), (b"Alt", 65536)]', '[(b"T".as_slice(), 4097), (b"Alt", 65537)]', 'textedit_alternate_text_and_titles_pin_the_content_they_describe'),
    Mutation('alignment: let a centred block stay editable', 'src/textedit/tagging.rs', '            b"Center" | b"End" | b"Justify" => Ok(true),', '            b"Center" | b"End" | b"Justify" => Ok(false),', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('alignment: accept any alignment', 'src/textedit/tagging.rs', '            _ => Err(INVALID.into()),\n        },\n    }\n}', '            _ => Ok(false),\n        },\n    }\n}', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('alignment: pin a start-aligned block', 'src/textedit/tagging.rs', '            b"Start" => Ok(bounded),', '            b"Start" => Ok(true),', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('line height: accept a negative height', 'src/textedit/tagging.rs', '&& super::number(height)? < 0.0', '&& super::number(height)? < -1e9', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('line height: refuse the named heights', 'src/textedit/tagging.rs', 'matches!(height.as_name(), Ok(b"Normal" | b"Auto"))', 'false', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('placement: require an explicit block placement', 'src/textedit/tagging.rs', '        || attributes.get(b"Placement").is_ok_and(\n', '        || attributes.get(b"Placement").map_or(true,\n', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('placement: accept any placement on a block', 'src/textedit/tagging.rs', '            |value| !matches!(value.as_name(), Ok(name) if name == b"Block" || name == placement),', '            |_| false,', 'textedit_alignment_pins_a_block_and_line_height_is_kept'),
    Mutation('leaves: accept a leaf without a line height', 'src/textedit/tagging.rs', '        || (leaf && !attributes.has(b"LineHeight"))\n', '', 'textedit_inline_leaves_accept_only_a_line_height_class'),
    Mutation('classes: accept a class with no ClassMap', 'src/textedit/tagging.rs', 'if scope.classes.is_some() && !matches!(tag, b"TD" | b"TH") {', 'if !matches!(tag, b"TD" | b"TH") {', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: skip class attributes', 'src/textedit/tagging.rs', '                    for object in objects {\n                        pins |= attributes(doc, role, crate::encoding::resolve(doc, object))?;\n                    }\n                    named = true;', '                    for object in objects {\n                        let _ = object;\n                    }\n                    named = true;', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept a revision before any name', 'src/textedit/tagging.rs', 'Object::Integer(revision) if named && *revision >= 0 => named = false,', 'Object::Integer(revision) if *revision >= 0 => named = false,', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept a negative revision', 'src/textedit/tagging.rs', 'Object::Integer(revision) if named && *revision >= 0 => named = false,', 'Object::Integer(_) if named => named = false,', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept consecutive revisions', 'src/textedit/tagging.rs', 'Object::Integer(revision) if named && *revision >= 0 => named = false,', 'Object::Integer(revision) if named && *revision >= 0 => {}', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept any number of names', 'src/textedit/tagging.rs', 'Object::Array(values) if !values.is_empty() && values.len() <= 8 => values.as_slice(),', 'Object::Array(values) if !values.is_empty() => values.as_slice(),', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept an empty class list', 'src/textedit/tagging.rs', 'Object::Array(values) if !values.is_empty() && values.len() <= 8 => values.as_slice(),', 'Object::Array(values) if values.len() <= 8 => values.as_slice(),', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept an unbounded ClassMap', 'src/textedit/tagging.rs', 'classes.is_some_and(|classes| classes.len() > 256)', 'classes.is_some_and(|classes| classes.len() > 257)', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('classes: accept a ClassMap that is not a dictionary', 'src/textedit/tagging.rs', '        let classes = root\n            .get(b"ClassMap")\n            .ok()\n            .map(|value| crate::encoding::resolve(doc, value).as_dict())\n            .transpose()\n            .map_err(|_| INVALID)?;', '        let classes = root\n            .get(b"ClassMap")\n            .ok()\n            .and_then(|value| crate::encoding::resolve(doc, value).as_dict().ok());', 'textedit_classes_apply_the_rules_of_the_elements_own_attributes'),
    Mutation('contents: refuse a table of contents', 'src/textedit/tagging.rs', '                || toc(role)\n            {', '            {', 'textedit_table_of_contents_groups_editable_entries'),
    Mutation('contents: accept any child of a table of contents', 'src/textedit/tagging.rs', '                        if !toc(name(get(node(doc, reference(item)?)?, b"S")?)?) {', '                        if false {', 'textedit_table_of_contents_groups_editable_entries'),
    Mutation('contents: accept an entry outside a table of contents', 'src/textedit/tagging.rs', 'if role == b"TOCI" && name(get(node(doc, parent_id)?, b"S")?)? != b"TOC" {', 'if false {', 'textedit_table_of_contents_groups_editable_entries'),
    Mutation('fields: skip the field attribute keys', 'src/textedit/tagging.rs', '            &[b"O", b"Role", b"checked", b"Checked", b"Desc"],', '            &[b"O", b"Role", b"checked", b"Checked", b"Desc", b"Placement"],', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('fields: accept any field role', 'src/textedit/tagging.rs', '.is_ok_and(|role| !matches!(role.as_name(), Ok(b"rb" | b"cb" | b"pb" | b"tv" | b"lb")))', '.is_ok_and(|_| false)', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('fields: accept any checked state', 'src/textedit/tagging.rs', '.is_ok_and(|state| !matches!(state.as_name(), Ok(b"on" | b"off" | b"neutral")))', '.is_ok_and(|_| false)', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('fields: accept any description', 'src/textedit/tagging.rs', '.is_ok_and(|text| !text.as_str().is_ok_and(|text| text.len() <= 65536))', '.is_ok_and(|_| false)', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('fields: accept field attributes on any owner', 'src/textedit/tagging.rs', '    if tag == b"Form"\n        && value.as_dict()', '    if true\n        && value.as_dict()', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('tolerances: accept any smoothness', 'src/textedit/graphics.rs', 'let limit = if key == b"SM" { 1. } else { 100. };', 'let limit = 100.;', 'textedit_rendering_tolerances_are_preserved_within_their_ranges'),
    Mutation('tolerances: accept a negative tolerance', 'src/textedit/graphics.rs', 'if !(0. ..=limit).contains(&value) {', 'if value > limit {', 'textedit_rendering_tolerances_are_preserved_within_their_ranges'),
    Mutation('tolerances: accept any flatness', 'src/textedit/graphics.rs', 'let limit = if key == b"SM" { 1. } else { 100. };', 'let limit = if key == b"SM" { 1. } else { 1e9 };', 'textedit_rendering_tolerances_are_preserved_within_their_ranges'),
    Mutation('tolerances: refuse the flatness operator', 'src/textedit.rs', '            ("i", [value]) => graphics::tolerance(b"FL", value)?,\n', '', 'textedit_rendering_tolerances_are_preserved_within_their_ranges'),
    Mutation('tolerances: read the flatness operator as smoothness', 'src/textedit.rs', 'graphics::tolerance(b"FL", value)', 'graphics::tolerance(b"SM", value)', 'textedit_rendering_tolerances_are_preserved_within_their_ranges'),
    Mutation('tolerances: refuse graphics-state tolerances', 'src/textedit/graphics.rs', '            (b"FL" | b"SM", value) => tolerance(key, value)?,\n', '', 'textedit_rendering_tolerances_are_preserved_within_their_ranges'),
    Mutation('legacy cmap: keep a Mac map only beside ToUnicode', 'src/textedit/fonts.rs', '&& !(table.platform_id == PlatformId::Macintosh && table.encoding_id == 0)', '&& !(font.has(b"ToUnicode") && table.platform_id == PlatformId::Macintosh && table.encoding_id == 0)', 'textedit_named_unicode_requires_identity_and_agreeing_legacy_glyphs'),
    Mutation('legacy cmap: accept a Mac map in any script', 'src/textedit/fonts.rs', '&& !(table.platform_id == PlatformId::Macintosh && table.encoding_id == 0)', '&& table.platform_id != PlatformId::Macintosh', 'textedit_named_unicode_requires_identity_and_agreeing_legacy_glyphs'),
    Mutation('type1: accept any header', 'src/textedit/fonts/type1/program.rs', 'if !(bytes.starts_with(b"%!PS-AdobeFont-1.") || bytes.starts_with(b"%!FontType1-1.")) {', 'if false {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept any matrix', 'src/textedit/fonts/type1/program.rs', '        || matrix.as_deref() != Some(&[0.001, 0., 0., 0.001, 0., 0.][..])\n', '', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept stroked outlines', 'src/textedit/fonts/type1/program.rs', '        || paint_type.is_some_and(|value| value != 0)\n', '', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept any font type', 'src/textedit/fonts/type1/program.rs', '    if font_type != Some(1)\n', '    if false\n', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept hexadecimal private data', 'src/textedit/fonts/type1/program.rs', 'if encrypted[..4].iter().all(u8::is_ascii_hexdigit) {', 'if false {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept any trailer', 'src/textedit/fonts/type1/program.rs', ".all(|b| b.is_ascii_whitespace() || *b == b'0')", '.all(|_| true)', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept a long lenIV', 'src/textedit/fonts/type1/program.rs', 'if value.fract() == 0. && (0. ..=4.).contains(&value) =>', 'if value.fract() == 0. && (0. ..=16.).contains(&value) =>', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: skip charstring decryption', 'src/textedit/fonts/type1/program.rs', 'Ok(decrypt(bytes, 4330)[skip..].to_vec())', 'Ok(bytes[skip..].to_vec())', 'textedit_type1_program_reads_widths_and_outline_hulls'),
    Mutation('type1: use the wrong eexec key', 'src/textedit/fonts/type1/program.rs', 'let private = decrypt(encrypted, 55665);', 'let private = decrypt(encrypted, 55666);', 'textedit_type1_program_reads_widths_and_outline_hulls'),
    Mutation('type1: accept a repeated Subrs index', 'src/textedit/fonts/type1/program.rs', 'if index >= count || std::mem::replace(&mut seen[index], true) {', 'if index >= count {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept a Subrs index out of range', 'src/textedit/fonts/type1/program.rs', 'if index >= count || std::mem::replace(&mut seen[index], true) {', 'if index >= count + 16 {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept a missing .notdef', 'src/textedit/fonts/type1/program.rs', 'if !names.contains(b".notdef".as_slice()) {', 'if false {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: skip the binary separator check', 'src/textedit/fonts/type1/program.rs', "if self.bytes.get(self.at) != Some(&b' ') {", 'if false {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept more glyphs than declared', 'src/textedit/fonts/type1/program.rs', 'if charstrings.len() > count {', 'if false {', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: accept a repeated glyph name', 'src/textedit/fonts/type1/program.rs', '|| !names.insert(name.to_vec())', '|| false && !names.insert(name.to_vec())', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('type1: read the custom encoding without its loop', 'src/textedit/fonts/type1/program.rs', '    if token == Token::Number(0.) {\n', '    if false {\n', 'textedit_type1_program_refuses_what_it_cannot_read_literally'),
    Mutation('charstrings: raise the stack limit', 'src/textedit/fonts/type1/program.rs', '                if self.stack.len() >= MAX_STACK {\n                    return Err(INVALID.into());\n                }\n                self.stack.push(value);', '                if self.stack.len() >= MAX_STACK + 1 {\n                    return Err(INVALID.into());\n                }\n                self.stack.push(value);', 'textedit_type1_interpreter_limits_have_boundary_controls'),
    Mutation('charstrings: raise the depth limit', 'src/textedit/fonts/type1/program.rs', 'if depth > MAX_DEPTH {', 'if depth > MAX_DEPTH + 1 {', 'textedit_type1_interpreter_limits_have_boundary_controls'),
    Mutation('charstrings: remove the work budget', 'src/textedit/fonts/type1/program.rs', 'if self.operations > MAX_OPERATIONS {', 'if false {', 'textedit_type1_interpreter_limits_have_boundary_controls'),
    Mutation('charstrings: accept a short flex', 'src/textedit/fonts/type1/program.rs', 'if points.len() != 7 {', 'if points.len() < 6 {', 'textedit_type1_interpreter_limits_have_boundary_controls'),
    Mutation('charstrings: accept unknown othersubrs', 'src/textedit/fonts/type1/program.rs', '(3, [subr]) => self.returns = vec![*subr],\n                        _ => return Err(INVALID.into()),', '(3, [subr]) => self.returns = vec![*subr],\n                        _ => {}', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('charstrings: allow a vertical advance', 'src/textedit/fonts/type1/program.rs', 'if wy != 0. || self.width.replace(wx).is_some() {', 'if self.width.replace(wx).is_some() {', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('charstrings: allow a second hsbw', 'src/textedit/fonts/type1/program.rs', '                    let [sbx, wx] = self.take()?;\n                    if self.width.replace(wx).is_some() {', '                    let [sbx, wx] = self.take()?;\n                    if false && self.width.replace(wx).is_some() {', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('charstrings: draw before hsbw', 'src/textedit/fonts/type1/program.rs', 'if self.width.is_none() || self.flex.is_some() {\n            return Err(INVALID.into());\n        }\n        if let Some(start)', 'if self.flex.is_some() {\n            return Err(INVALID.into());\n        }\n        if let Some(start)', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('charstrings: divide by zero', 'src/textedit/fonts/type1/program.rs', 'if b == 0. {', 'if false {', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('charstrings: end inside a flex', 'src/textedit/fonts/type1/program.rs', 'self.width.is_none() || self.flex.is_some() || !self.returns.is_empty()', 'self.width.is_none() || !self.returns.is_empty()', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('charstrings: pop with nothing returned', 'src/textedit/fonts/type1/program.rs', 'let value = self.returns.pop().ok_or(INVALID)?;', 'let value = self.returns.pop().unwrap_or(0.);', 'textedit_type1_glyphs_it_cannot_follow_are_not_offered'),
    Mutation('hull: forget the contour start', 'src/textedit/fonts/type1/program.rs', '        if let Some(start) = self.pending.take() {\n            self.include(start);\n        }\n', '        self.pending = None;\n', 'textedit_type1_program_reads_widths_and_outline_hulls'),
    Mutation('hull: count a move as ink', 'src/textedit/fonts/type1/program.rs', '        } else {\n            self.pending = Some(point);\n        }', '        } else {\n            self.include(point);\n        }', 'textedit_type1_program_reads_widths_and_outline_hulls'),
    Mutation('hull: skip curve control points', 'src/textedit/fonts/type1/program.rs', 'self.draw(&[first, second, end])', 'self.draw(&[end])', 'textedit_type1_interpreter_follows_curves_subrs_flex_and_hints'),
    Mutation('hull: skip flex points', 'src/textedit/fonts/type1/program.rs', 'self.draw(&points[1..])?;', 'self.draw(&points[6..])?;', 'textedit_type1_interpreter_follows_curves_subrs_flex_and_hints'),
    Mutation('type1 names: drop the WinAnsi base', 'src/textedit/fonts/type1.rs', '        Some(b"WinAnsiEncoding") => {\n            winansi(names);', '        Some(b"WinAnsiEncoding") => {', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1 names: drop the Standard base', 'src/textedit/fonts/type1.rs', '        Some(b"StandardEncoding") => {\n            standard(names);', '        Some(b"StandardEncoding") => {', 'textedit_type1_fonts_edit_through_their_glyph_names'),
    Mutation('type1 names: ignore the built-in encoding', 'src/textedit/fonts/type1.rs', 'program::Builtin::Custom(custom) => names.clone_from_slice(&custom[..]),', 'program::Builtin::Custom(_) => {}', 'textedit_type1_fonts_edit_through_their_glyph_names'),
    Mutation('type1 names: accept any base name', 'src/textedit/fonts/type1.rs', '        Some(_) => Err(invalid()),', '        Some(_) => Ok::<(), String>(()),', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1 names: let Differences repeat a code', 'src/textedit/fonts/type1.rs', 'if std::mem::replace(&mut changed[code], true) || name.len() > 127 {', 'if name.len() > 127 {', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1 names: accept any encoding key', 'src/textedit/fonts/type1.rs', '            b"BaseEncoding" | b"Differences" => {}\n            _ => return Err(invalid()),', '            _ => {}', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1: accept any flags', 'src/textedit/fonts/type1.rs', 'if !matches!(flags & (4 | 32), 4 | 32) {', 'if false {', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1: accept restricted rights', 'src/textedit/fonts/type1.rs', 'if program.rights.is_some_and(|rights| rights & !0x108 != 0) {', 'if false {', 'textedit_type1_rights_restrict_editing'),
    Mutation('type1: offer a code its map sends elsewhere', 'src/textedit/fonts/type1.rs', 'Some(unicode) => unicode[code] == Some(Some(slot)),', 'Some(unicode) => unicode[code] != Some(None),', 'textedit_type1_to_unicode_narrows_but_never_contradicts_names'),
    Mutation('type1: offer ligatures without a map', 'src/textedit/fonts/type1.rs', 'None => super::ligatures::text(slot).is_none(),', 'None => true,', 'textedit_type1_to_unicode_narrows_but_never_contradicts_names'),
    Mutation('type1: offer a glyph whose width disagrees', 'src/textedit/fonts/type1.rs', 'let offered = offered.filter(|_| (width - glyph.width).abs() <= 1.);', 'let offered = offered;', 'textedit_type1_to_unicode_narrows_but_never_contradicts_names'),
    Mutation('type1: drop the opaque table', 'src/textedit/fonts/type1.rs', '        opaque: Some(opaque),', '        opaque: None,', 'textedit_type1_opaque_glyphs_measure_read_only_text'),
    Mutation('type1: offer a glyph beyond the envelope', 'src/textedit/fonts/type1.rs', '                        && top <= 1000. =>', '                        && top <= 2000. =>', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1: forget opaque ink', 'src/textedit/fonts/type1.rs', '                vertical_bounds[1] = vertical_bounds[1].max(top);\n                opaque[code]', '                opaque[code]', 'textedit_type1_opaque_glyphs_measure_read_only_text'),
    Mutation('type1: accept two glyphs for one character', 'src/textedit/fonts/type1.rs', 'Some(existing) if existing != name => {', 'Some(existing) if false && existing != name => {', 'textedit_type1_encodings_flags_and_glyph_limits'),
    Mutation('type1 maps: accept any label', 'src/textedit/fonts/mapping.rs', 'if end <= begin || !labels(&ops[begin + 1..end]) {', 'if end <= begin {', 'textedit_type1_maps_narrow_to_the_repertoire_and_keep_every_code'),
    Mutation('type1 maps: accept extra label keys', 'src/textedit/fonts/mapping.rs', 'info.len() == 3', 'info.len() >= 3', 'textedit_type1_maps_narrow_to_the_repertoire_and_keep_every_code'),
    Mutation('type1 maps: accept a repeated code', 'src/textedit/fonts/mapping.rs', 'if result[code as usize].replace(slot).is_some() {', 'if false {', 'textedit_type1_maps_narrow_to_the_repertoire_and_keep_every_code'),
    Mutation('type1 maps: wrap a range', 'src/textedit/fonts/mapping.rs', 'let unit = start.checked_add(offset).ok_or_else(invalid)?;', 'let unit = start.wrapping_add(offset);', 'textedit_type1_maps_narrow_to_the_repertoire_and_keep_every_code'),
    Mutation('type1 maps: offer the soft hyphen', 'src/textedit/fonts/mapping.rs', '.filter(|&slot| !matches!(slot, 0xA0 | 0xAD))', '.filter(|&slot| slot != 0xA0)', 'textedit_type1_maps_narrow_to_the_repertoire_and_keep_every_code'),
    Mutation('gaps: write a space glyph that is not there', 'src/textedit/fonts.rs', "        if !text.contains(' ') || self.writes_space() {\n            return Ok(vec![", '        if true {\n            return Ok(vec![', 'textedit_type1_layout_keeps_the_original_font_and_writes_gaps'),
    Mutation('gaps: measure a space glyph that is not there', 'src/textedit/fonts.rs', "        if !text.contains(' ') || self.writes_space() {\n            return self.spaced_layout", '        if true {\n            return self.spaced_layout', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('gaps: accept an empty word', 'src/textedit/fonts.rs', '            if word.is_empty() {\n                return Err(GAP_SPACES.into());\n            }\n            if index > 0 {\n                items.push', '            if index > 0 {\n                items.push', 'textedit_type1_layout_keeps_the_original_font_and_writes_gaps'),
    Mutation('gaps: forget later words in the ink', 'src/textedit/fonts.rs', 'bounds = [bounds[0].min(cursor + left), bounds[1].max(cursor + right)];', 'bounds = [bounds[0], bounds[1]];', 'textedit_type1_gapped_layout_matches_the_written_words'),
    Mutation('gaps: measure the wrong gap', 'src/textedit/fonts.rs', 'let shift = gap * size / 1000.;', 'let shift = gap * size / 100.;', 'textedit_type1_gapped_layout_matches_the_written_words'),
    Mutation('opaque: forget word spacing on code 32', 'src/textedit/fonts.rs', '.map(|glyph| (glyph.width, glyph.overhang, code == 32))', '.map(|glyph| (glyph.width, glyph.overhang, false))', 'textedit_type1_opaque_glyphs_measure_read_only_text'),
    Mutation('opaque: decode an unmapped code', 'src/textedit/fonts.rs', '                if self.opaque(code).is_none() {\n                    return Err("text contains an unmapped font code".into());\n                }\n', '', 'textedit_cff_maps_ascii_by_glyph_name_and_preserves_resources'),
    Mutation('opaque: offer text the editor cannot write', 'src/textedit.rs', '            || text.contains(fonts::OPAQUE)\n', '', 'textedit_type1_to_unicode_narrows_but_never_contradicts_names'),
    Mutation('lead: refuse a leading adjustment', 'src/textedit.rs', '[lead @ (Object::Integer(_) | Object::Real(_)), rest @ ..] => (Some(lead), rest),', '[] => (None, values),', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('lead: ignore where the run starts', 'src/textedit.rs', '                cursor -= number(lead)? * size / 1000.0;\n', '                cursor -= 0.0 * number(lead)? * size / 1000.0;\n', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('lead: drop it on save', 'src/textedit.rs', '            show.operator = "TJ".into();\n            led(lead, items)\n        } else {', '            show.operator = "TJ".into();\n            led(None, items)\n        } else {', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('lead: accept an unbounded start', 'src/textedit.rs', '                if !cursor.is_finite() || cursor.abs() > 1_000_000.0 {\n                    return Err("kerning position exceeds its limit".into());\n                }\n                leads', '                leads', 'textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays'),
    Mutation('gaps: read gaps as spaces in any font', 'src/textedit.rs', 'let gap_spaces = !metrics.writes_space();', 'let gap_spaces = true;', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('gaps: read every kern as a space', 'src/textedit.rs', 'if gap_spaces && pending >= GAP_EM {', 'if gap_spaces && pending > 0.0 {', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('gaps: miss narrow spaces', 'src/textedit.rs', 'if gap_spaces && pending >= GAP_EM {', 'if gap_spaces && pending >= 400.0 {', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('gaps: forget the run mean', 'src/textedit.rs', '                gaps.insert(index as u32, found.total / f64::from(found.count));\n', '', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('gaps: keep a gap across a kern', 'src/textedit.rs', '            pending = 0.0;\n            characters += fragment', '            characters += fragment', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('layout: fall back for a space', 'src/textedit/layout.rs', '.all(|line| original_metrics.items(line, gap).is_ok());', '.all(|line| original_metrics.encode(line).is_ok());', 'textedit_type1_layout_keeps_the_original_font_and_writes_gaps'),
    Mutation("layout: keep a wrapped line's space", 'src/textedit/layout.rs', '        let text = if gap_spaces {', '        let text = if false {', 'textedit_type1_layout_keeps_the_original_font_and_writes_gaps'),
    Mutation('layout: write gaps as a Tj', 'src/textedit/layout.rs', 'operations.push(if items.len() > 1 {', 'operations.push(if false {', 'textedit_type1_layout_keeps_the_original_font_and_writes_gaps'),
    Mutation('layout: measure spaces as glyphs', 'src/textedit/layout.rs', ".gapped_layout(text.trim_end_matches(' '), size, spacing, word_spacing, gap)\n                        .map(", '.spaced_layout(text, size, spacing, word_spacing)\n                        .map(', 'textedit_type1_layout_keeps_the_original_font_and_writes_gaps'),
    Mutation('forms: accept any figure file name', 'src/textedit/forms.rs', '(b"PTEX.FileName", Object::String(bytes, _)) if bytes.len() <= 4096 => {}', '(b"PTEX.FileName", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('forms: accept a negative figure page', 'src/textedit/forms.rs', '(b"PTEX.PageNumber", Object::Integer(page)) if *page >= 0 => {}', '(b"PTEX.PageNumber", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('forms: accept any figure info', 'src/textedit/forms.rs', '            (b"PTEX.InfoDict", _) => {\n                dictionary(doc, value)?;\n            }', '            (b"PTEX.InfoDict", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('alpha: accept any number', 'src/textedit/graphics.rs', '(b"ca" | b"CA", value) if (0. ..=1.).contains(&number(value)?) => {}', '(b"ca" | b"CA", value) if number(value).is_ok() => {}', 'textedit_graphics_state_refuses_effects_bad_types_and_later_resets_atomically'),
    Mutation('alpha: refuse translucency', 'src/textedit/graphics.rs', '(b"ca" | b"CA", value) if (0. ..=1.).contains(&number(value)?) => {}', '(b"ca" | b"CA", value) if number(value)? == 1. => {}', 'textedit_normal_graphics_state_preserves_fill_geometry_and_saved_operators'),
    Mutation('kerning: never keep', 'src/textedit.rs', 'let kept = if source.operator == "TJ" && !grouped {', 'let kept = if false {', 'textedit_kerning_lets_a_kerned_line_take_an_edit_that_fits_only_with_its_kerns'),
    Mutation('kerning: keep a version that does not fit', 'src/textedit.rs', '                advance <= run.advance + 0.000_001\n                    && bounds[0] >= original[0]\n                    && bounds[1] <= original[1]', '                true', 'textedit_kerning_gives_way_to_the_rewrite_when_kept_kerns_do_not_fit'),
    Mutation('own size: lay out at the rounded size', 'src/textedit/layout.rs', '    if (requested - source * yscale).abs() <= SIZE_STEP * (1. + 1e-9) {', '    if false {', 'a_run_keeps_its_own_font_size_through_the_rounded_default_box'),
    Mutation('own size: exact step only', 'src/textedit/layout.rs', '<= SIZE_STEP * (1. + 1e-9) {', '<= SIZE_STEP {', 'a_requested_size_within_one_step_of_the_source_is_the_source_size'),
    Mutation('own size: snap only upwards', 'src/textedit/layout.rs', '    if (requested - source * yscale).abs() <= SIZE_STEP', '    if requested - source * yscale <= SIZE_STEP && requested >= source * yscale', 'a_requested_size_within_one_step_of_the_source_is_the_source_size'),
    Mutation('own size: snap any size', 'src/textedit/layout.rs', 'const SIZE_STEP: f64 = 0.001;', 'const SIZE_STEP: f64 = 0.01;', 'a_requested_size_within_one_step_of_the_source_is_the_source_size'),
    Mutation('source items: never in the layout', 'src/textedit/layout.rs', '        if fallback.is_none() && !change.replacement.is_empty() {', '        if false {', 'a_kerned_run_fits_its_own_default_box_unchanged_and_keeps_its_kerns'),
    Mutation('source items: keep kerns at another size', 'src/textedit/layout.rs', "    if size != run.size || replacement.contains('\\n') {", "    if replacement.contains('\\n') {", 'a_kerned_run_at_another_size_is_laid_out_again_without_its_kerns'),
    Mutation('source items: in a bundled font too', 'src/textedit/layout.rs', '        if fallback.is_none() && !change.replacement.is_empty() {', '        if !change.replacement.is_empty() {', 'textedit_a_partly_clipped_run_takes_its_own_text_in_the_default_box'),
    Mutation("source items: past the box's advance", 'src/textedit/layout.rs', '        advance <= width + 0.000_001\n            && bounds[0] >= original[0]', '        bounds[0] >= original[0]', 'kept_kerns_that_do_not_fit_the_box_give_way_to_the_rewrite'),
    Mutation("source items: ink before the source's", 'src/textedit/layout.rs', '            && bounds[0] >= original[0]\n            && bounds[1] <= original[1].max(width)', '            && bounds[1] <= original[1].max(width)', 'textedit_overhanging_source_ink_fits_its_own_default_box_unmoved'),
    Mutation('source items: ink past box and source', 'src/textedit/layout.rs', '            && bounds[1] <= original[1].max(width) + 0.000_001', '', 'textedit_overhanging_source_ink_fits_its_own_default_box_unmoved'),
    Mutation('source items: ink only within the box', 'src/textedit/layout.rs', '            && bounds[1] <= original[1].max(width) + 0.000_001', '            && bounds[1] <= width + 0.000_001', 'textedit_overhanging_source_ink_fits_its_own_default_box_unmoved'),
    Mutation('source items: inset like a new line', 'src/textedit/layout.rs', '            (*ink, 0.)\n', '            (*ink, (-ink[0]).max(0.))\n', 'textedit_overhanging_source_ink_fits_its_own_default_box_unmoved'),
    Mutation('source items: write the rewrite anyway', 'src/textedit/layout.rs', '            Some((items, _, _, _)) => items.clone(),', '            Some(_) => metrics.items(text, gap)?,', 'a_kerned_run_fits_its_own_default_box_unchanged_and_keeps_its_kerns'),
    Mutation('source items: skip the clip for any kept ink', 'src/textedit/layout.rs', '                if !kept.is_some_and(|(_, _, _, inside)| *inside) {', '                    if kept.is_none() {', 'textedit_a_partly_clipped_run_takes_its_own_text_in_the_default_box'),
    Mutation("source items: hold the source's own ink to the clip", 'src/textedit/layout.rs', '                if !kept.is_some_and(|(_, _, _, inside)| *inside) {', '                    if true {', 'textedit_a_partly_clipped_run_takes_its_own_text_in_the_default_box'),
    Mutation("source items: count box ink as the source's", 'src/textedit/layout.rs', '    let inside = bounds[1] <= original[1] + 0.000_001;', '    let inside = true;', 'textedit_a_partly_clipped_run_takes_its_own_text_in_the_default_box'),
    Mutation('source items: no second try', 'src/textedit/layout.rs', '        if outcome.is_ok() {\n            break;\n        }', '        break;', 'textedit_an_edit_the_source_positioning_cannot_place_is_laid_out_afresh'),
    # The room after a run, which a box the reader has not sized grows into.
    # The four `room` entries aim at the pure geometry; the rest at the writer.
    Mutation('room: no page edge', 'src/textedit/layout.rs', '    let mut free = at(edge([0., 0., page[0], page[1]], true));', '    let mut free = at(edge([-100., -100., page[0] + 100., page[1] + 100.], true));', 'the_room_after_a_run_reaches_the_page_edge_and_stops_short_of_a_neighbour'),
    Mutation('room: the leading edge is always the far one', 'src/textedit/layout.rs', 'rect[if (sign > 0.) == ahead { axis + 2 } else { axis }]', 'rect[axis + 2]', 'the_room_after_a_run_reaches_the_page_edge_and_stops_short_of_a_neighbour'),
    Mutation('room: ignore the clip', 'src/textedit/layout.rs', '        let limit = at(edge(clip, true));\n        if limit < free {', '        let limit = at(edge(clip, true));\n        if false {', 'the_room_after_a_run_stops_at_a_clip_and_a_nearer_neighbour_wins'),
    Mutation('room: ignore the neighbour', 'src/textedit/layout.rs', '        if near + 0.000_000_001 >= width && near < free {', '        if false {', 'the_room_after_a_run_reaches_the_page_edge_and_stops_short_of_a_neighbour'),
    Mutation("room: a neighbour's far edge faces us", 'src/textedit/layout.rs', '        let near = at(edge(other, false));', '        let near = at(edge(other, true));', 'the_room_after_a_run_reaches_the_page_edge_and_stops_short_of_a_neighbour'),
    Mutation('room: everything is on the line', 'src/textedit/layout.rs', '        if other[cross + 2].min(high) - other[cross].max(low) <= 0.1 {\n            continue;', '        if false {\n            continue;', 'the_room_after_a_run_ignores_what_is_not_on_its_line'),
    Mutation("room: the box's own cross span alone", 'src/textedit/layout.rs', '        zero[cross].min(own[cross]),\n        zero[cross + 2].max(own[cross + 2]),', '        zero[cross],\n        zero[cross + 2],', 'the_room_after_a_run_counts_a_neighbour_its_own_glyphs_reach'),
    Mutation('room: may shrink the box it was given', 'src/textedit/layout.rs', '    (free.max(width), stop)', '    (free, stop)', 'the_room_after_a_run_is_never_less_than_the_box_it_was_given'),
    Mutation('room: a neighbour behind the box still binds', 'src/textedit/layout.rs', '        if near + 0.000_000_001 >= width && near < free {', '        if near < free {', 'the_room_after_a_run_is_never_less_than_the_box_it_was_given'),
    Mutation('free width: trust a compound clip', 'src/textedit/layout.rs', '    if free > width\n        && context', '    if false\n        && context', 'a_run_under_a_compound_clip_gives_growth_up_rather_than_guessing_an_edge'),
    Mutation('grown box: never grow', 'src/textedit/layout.rs', '    let (ceiling, stop) = if settings.grow {', '    let (ceiling, stop) = if false {', 'a_box_the_reader_has_not_sized_grows_into_the_free_space_after_the_line'),
    Mutation('grown box: grow a box the reader sized', 'src/textedit/layout.rs', '    let (ceiling, stop) = if settings.grow {', '    let (ceiling, stop) = if true {', 'a_box_the_reader_has_not_sized_grows_into_the_free_space_after_the_line'),
    Mutation('grown box: say the box is too narrow', 'src/textedit/layout.rs', '    let (too_wide, too_wide_ink) = if settings.grow {', '    let (too_wide, too_wide_ink) = if false {', 'a_grown_box_stops_two_points_short_of_the_next_text_on_the_line'),
    Mutation('grown box: report the whole room as the box', 'src/textedit/layout.rs', '            width.max(used).min(ceiling),', '            ceiling,', 'the_box_reported_back_is_the_size_of_the_text_not_of_the_room_it_had'),
    Mutation('grown box: measure the room, not the text', 'src/textedit/layout.rs', '                used = used.max(advance.max(ink[1]));\n                (*ink, 0.)', '                used = ceiling;\n                (*ink, 0.)', 'the_box_reported_back_is_the_size_of_the_text_not_of_the_room_it_had'),
    Mutation('grown box: lay the line out to the box, not the room', 'src/textedit/layout.rs', '            line_breaks(\n                &change.replacement,\n                limit,', '            line_breaks(\n                &change.replacement,\n                width,', 'a_fallback_replacement_grows_into_the_free_space_as_well'),
    Mutation('grown box: hold the ink to the box, not the room', 'src/textedit/layout.rs', '                if ink[1] + inset > limit + 0.001 {', '                if ink[1] + inset > width + 0.001 {', 'a_fallback_replacement_grows_into_the_free_space_as_well'),
    Mutation('grown box: keep the source items to the box', 'src/textedit/layout.rs', '            (ceiling, true),', '            (width, true),', 'a_kerned_run_grown_into_the_free_space_keeps_the_kerns_it_had'),
    Mutation('own items: never keep kerns', 'src/textedit.rs', '    let kept = if source.operator == "TJ" && !grouped {', '    let kept = if false {', 'a_same_length_edit_in_the_default_box_keeps_kerns_and_leaves_neighbours'),
    Mutation("kerning: keep the changed pair's kern before", 'src/textedit/kerning.rs', '        if !text.is_empty() {\n            prefix = index + 1;', '        if true {\n            prefix = index + 1;', 'textedit_kerning_keeps_unchanged_ends_and_drops_the_changed_pairs_kern'),
    Mutation("kerning: keep the changed pair's kern after", 'src/textedit/kerning.rs', '        if !text.is_empty() {\n            suffix = index;', '        if true {\n            suffix = index;', 'textedit_kerning_keeps_unchanged_ends_and_drops_the_changed_pairs_kern'),
    Mutation('kerning: read every word gap as a kern', 'src/textedit/kerning.rs', 'atoms.push(Atom::Shift(value, gap_spaces && between && shift >= GAP_EM));', 'atoms.push(Atom::Shift(value, false));', 'textedit_type1_word_gaps_read_and_write_as_spaces'),
    Mutation('kerning: accept several numbers in a row', 'src/textedit/kerning.rs', '                if !between && values.get(index + 1).is_some() {\n                    return None;\n                }', '', 'textedit_kerning_keeps_unchanged_ends_and_drops_the_changed_pairs_kern'),
    Mutation('kerning: keep nothing at the end', 'src/textedit/kerning.rs', '    emit(&atoms[suffix..], &mut tail);', '    let _ = suffix;', 'textedit_kerning_keeps_unchanged_ends_and_drops_the_changed_pairs_kern'),
    Mutation('paths: require a segment before a moveto', 'src/textedit/clipping.rs', '            "m" => {\n                segments = 0;', '            "m" if index == 0 || segments > 0 => {\n                segments = 0;', 'textedit_paths_accept_movetos_that_start_nothing'),
    Mutation('paths: range-check rotated points as if diagonal', 'src/textedit/clipping.rs', '        x * ctm[0] + y * ctm[2] + ctm[4],\n        x * ctm[1] + y * ctm[3] + ctm[5],', '        x * ctm[0] + ctm[4],\n        y * ctm[3] + ctm[5],', 'textedit_rotated_paint_is_preserved_and_rotated_clips_are_refused'),
    Mutation('paths: skip rotated rectangle corners', 'src/textedit/clipping.rs', '                bounded(\n                    point(ctm, px, py),\n                    "rectangle coordinates exceed their limit",\n                )?;', '                let _ = (px, py);', 'textedit_rotated_paint_is_preserved_and_rotated_clips_are_refused'),
    Mutation('paths: accept an empty rotated rectangle', 'src/textedit/clipping.rs', '            if width == 0. || height == 0. {\n                return Err("empty rectangle is not editable".into());\n            }\n            for (px, py) in [', '            for (px, py) in [', 'textedit_rotated_paint_is_preserved_and_rotated_clips_are_refused'),
    Mutation('paths: clip under a rotation', 'src/textedit.rs', '                } else if !diagonal(page_transform) {\n                    return Err("non-diagonal clips are not editable yet".into());\n                } else {', '                } else {', 'textedit_rotated_paint_is_preserved_and_rotated_clips_are_refused'),
    Mutation('unembedded: accept any encoding', 'src/textedit/fonts.rs', '        || font.get(b"Encoding").and_then(Object::as_name).ok() != Some(b"WinAnsiEncoding")\n        || font.iter().any(|(key, _)| {\n            !matches!(\n                key.as_slice(),\n                b"Type"\n                    | b"Subtype"\n                    | b"BaseFont"\n                    | b"Encoding"\n                    | b"Name"\n                    | b"FirstChar"', '        || font.iter().any(|(key, _)| {\n            !matches!(\n                key.as_slice(),\n                b"Type"\n                    | b"Subtype"\n                    | b"BaseFont"\n                    | b"Encoding"\n                    | b"Name"\n                    | b"FirstChar"', 'textedit_unembedded_fonts_require_a_plain_winansi_contract'),
    Mutation('unembedded: accept symbolic fonts', 'src/textedit/fonts.rs', '            & (4 | 32)\n            != 32\n    {\n        // Both callers', '            & 0\n            != 0\n    {\n        // Both callers', 'textedit_unembedded_fonts_require_a_plain_winansi_contract'),
    Mutation('unembedded: offer a code with no width', 'src/textedit/fonts.rs', '        if width > 0. {\n            result[code as usize] = Some(width);\n            overhangs', '        if true {\n            result[code as usize] = Some(width);\n            overhangs', 'textedit_unembedded_fonts_edit_by_their_pdf_widths'),
    Mutation('unembedded: forget the font box', 'src/textedit/fonts.rs', '        vertical_bounds: Some([bottom.min(0.), top.max(0.)]),', '        vertical_bounds: None,', 'textedit_unembedded_font_box_bounds_read_only_text'),
    Mutation('unembedded: accept a reversed font box', 'src/textedit/fonts.rs', 'if left > right || bottom > top || [left, bottom, right, top]', 'if [left, bottom, right, top]', 'textedit_unembedded_fonts_require_a_plain_winansi_contract'),
    Mutation('unembedded: skip the dispatch', 'src/textedit.rs', '        && fonts::is_unembedded(doc, font)', '        && false', 'textedit_unembedded_fonts_edit_by_their_pdf_widths'),
    Mutation('unembedded: refuse a Type 1 without a program', 'src/textedit/fonts.rs', '    let Some(carrier) = carriers.next() else {\n        return unembedded(doc, font);', '    let Some(carrier) = carriers.next() else {\n        return Err("Type1 font has no embedded font program".into());', 'textedit_font_refusals_separate_missing_conflicting_and_invalid_programs'),
    Mutation('type1: narrow the read-only reach', 'src/textedit/fonts/type1.rs', 'const OPAQUE_REACH: f64 = 4000.;', 'const OPAQUE_REACH: f64 = 2000.;', 'textedit_type1_opaque_glyphs_measure_read_only_text'),
    Mutation('images: shrink the page budget', 'src/textedit.rs', 'const MAX_IMAGES: usize = 32 * 1024 * 1024;', 'const MAX_IMAGES: usize = 8 * 1024 * 1024;', 'textedit_a_screenshot_with_its_mask_fits_the_page_budget'),
    Mutation('render: no margin around stroked text', 'src/textedit.rs', '        let stroke = if matches!(render, 1 | 2) {', '        let stroke = if false {', 'textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink'),
    Mutation('render: margin ignores the CTM scale', 'src/textedit.rs', '            line_width / 2.\n                * (page_transform[0].abs() + page_transform[2].abs())\n                    .max(page_transform[1].abs() + page_transform[3].abs())', '            line_width / 2.', 'textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink'),
    Mutation('render: margin around invisible text', 'src/textedit.rs', '        let stroke = if matches!(render, 1 | 2) {', '        let stroke = if matches!(render, 1 | 2 | 3) {', 'textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink'),
    Mutation('render: margin misses the ink bounds', 'src/textedit.rs', '                ink_bounds[0] - stroke,\n                ink_bounds[1] - stroke,', '                ink_bounds[0],\n                ink_bounds[1],', 'textedit_compound_clips_contain_the_stroke_around_text'),
    Mutation('render: gs width is ignored', 'src/textedit.rs', '                if let Some(width) = graphics_states[name] {\n                    line_width = width;', '                if let Some(width) = graphics_states[name] {\n                    let _ = width;', 'textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink'),
    Mutation('render: w width is ignored', 'src/textedit.rs', '                clipping::line_width(value)?;\n                line_width = number(value)?;', '                clipping::line_width(value)?;', 'textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink'),
    Mutation('render: Q keeps the inner mode', 'src/textedit.rs', '                    compound_clips,\n                    (render, line_width),\n                ) = states', '                    compound_clips,\n                    _,\n                ) = states', 'textedit_stroked_and_invisible_text_is_edited_with_its_mode_and_ink'),
    Mutation('render: layout ignores the margin', 'src/textedit/layout.rs', '            ink[0] - context.stroke,', '            ink[0],', 'stroked_layouts_reserve_half_the_line_width'),
    Mutation('bmc: accept any tag inside a text object', 'src/textedit.rs', '("BMC", [tag]) if !inside || tag.as_name().ok() == Some(b"Artifact") => {', '("BMC", [tag]) => {', 'textedit_refusals_distinguish_operator_context_without_echoing_values'),
    Mutation('bmc: refuse Artifact inside a text object', 'src/textedit.rs', '("BMC", [tag]) if !inside || tag.as_name().ok() == Some(b"Artifact") => {', '("BMC", [tag]) if !inside => {', 'textedit_artifact_bmc_inside_a_text_object_is_read_only'),
    Mutation('glyphs: any one-contour glyph is empty', 'src/textedit/fonts.rs', 'data.get(0..2)? == [0, 1] && data.get(10..12)? == [0, 0]', 'data.get(0..2)? == [0, 1]', 'textedit_a_single_point_glyph_paints_nothing'),
    Mutation('glyphs: a single point paints', 'src/textedit/fonts.rs', 'Some(data.is_empty() || (data.get(0..2)? == [0, 1] && data.get(10..12)? == [0, 0]))', 'Some(data.is_empty())', 'textedit_a_single_point_glyph_paints_nothing'),
    Mutation('composite: descendant must repeat the Type0 name', 'src/textedit/fonts/composite.rs', '    let base = child\n        .get(b"BaseFont")\n        .and_then(Object::as_name)\n        .map_err(|_| INVALID)?;', '    let base = font\n        .get(b"BaseFont")\n        .and_then(Object::as_name)\n        .map_err(|_| INVALID)?;', 'textedit_composite_type0_name_may_differ_from_its_descendant'),
    Mutation('composite: accept a Type0 font without a name', 'src/textedit/fonts/composite.rs', '    font.get(b"BaseFont")\n        .and_then(Object::as_name)\n        .map_err(|_| INVALID)?;', '', 'textedit_composite_refusals_leave_every_object_unchanged'),
    Mutation('images: skip the Flate stage', 'src/textedit/images.rs', 'Some(filters::inflate(image)?)', 'Some(image.content.clone())', 'textedit_flate_wrapped_jpeg_is_checked_as_a_jpeg_and_kept'),
    Mutation('images: DCT after Flate is not a JPEG', 'src/textedit/images.rs', 'if first == b"FlateDecode" && second == b"DCTDecode" =>', 'if false =>', 'textedit_flate_wrapped_jpeg_is_checked_as_a_jpeg_and_kept'),
    Mutation('jpeg: any bytes after EOI', 'src/textedit/images/jpeg.rs', 'input[position..].iter().all(|&byte| byte == 0)', 'true', 'textedit_jpeg_framing_bounds_scans_and_requires_complete_marker_segments'),
    Mutation('jpeg: no padding after EOI', 'src/textedit/images/jpeg.rs', 'input[position..].iter().all(|&byte| byte == 0)', 'position == input.len()', 'textedit_flate_wrapped_jpeg_is_checked_as_a_jpeg_and_kept'),
    Mutation('jpeg: decode the padding too', 'src/textedit/images/jpeg.rs', '    let input = &input[..input\n', '    let _ = &input[..input\n', 'textedit_flate_wrapped_jpeg_is_checked_as_a_jpeg_and_kept'),
    Mutation('artifacts: artifact text on an untagged page is editable', 'src/textedit/tagging.rs', '            Some(None) => true,', '            Some(None) => !self.names.is_empty(),', 'textedit_artifacts_on_untagged_pages_are_read_only'),
    Mutation('artifacts: artifact text on a tagged page is editable', 'src/textedit/tagging.rs', '            Some(None) => true,', '            Some(None) => false,', 'textedit_tagged_artifact_and_unmarked_text_remain_read_only'),
    Mutation('artifacts: no tree for any marked content', 'src/textedit/tagging.rs', 'if self.names.is_empty() && (properties.is_some() || name(tag)? != b"Artifact") {', 'if false {', 'textedit_artifacts_on_untagged_pages_are_read_only'),
    Mutation('artifacts: an MCID artifact needs no tree', 'src/textedit/tagging.rs', 'if self.names.is_empty() && (properties.is_some() || name(tag)? != b"Artifact") {', 'if self.names.is_empty() && name(tag)? != b"Artifact" {', 'textedit_artifacts_on_untagged_pages_are_read_only'),
    Mutation('artifacts: untagged pages still refuse artifacts', 'src/textedit/tagging.rs', 'if self.names.is_empty() && (properties.is_some() || name(tag)? != b"Artifact") {', 'if self.names.is_empty() {', 'textedit_artifacts_on_untagged_pages_are_read_only'),
    Mutation('stencil: treat as an ordinary image', 'src/textedit/images.rs', 'if image.dict.get(b"ImageMask").and_then(Object::as_bool).ok() == Some(true) {', 'if false {', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: report an ordinary image', 'src/textedit/images.rs', '            stencil: true,', '            stencil: false,', 'textedit_stencil_masks_paint_only_a_ready_fill_colour'),
    Mutation('stencil: skip the fill colour', 'src/textedit.rs', '                if stencils.contains(name) {', '                if false {', 'textedit_stencil_masks_paint_only_a_ready_fill_colour'),
    Mutation('stencil: check the colour only at first use', 'src/textedit.rs', '                        if image.stencil {\n                            stencils.insert(name.clone());\n                        }', '                        if image.stencil {\n                            patterns::paint("f", fill_components, stroke_components)?;\n                        }', 'textedit_stencil_masks_paint_only_a_ready_fill_colour'),
    Mutation('stencil: forms paint stencils', 'src/textedit/forms.rs', '                    if image.stencil {\n                        return Err(INVALID.into());', '                    if false {\n                        return Err(INVALID.into());', 'textedit_preserved_forms_refuse_stencil_masks'),
    Mutation('stencil: any decode mapping', 'src/textedit/images/stencil.rs', 'if mapping != [0., 1.] && mapping != [1., 0.] {', 'if false {', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: only one polarity', 'src/textedit/images/stencil.rs', 'if mapping != [0., 1.] && mapping != [1., 0.] {', 'if mapping != [0., 1.] {', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: any bit depth', 'src/textedit/images/stencil.rs', '(b"BitsPerComponent", Object::Integer(1)) => {}', '(b"BitsPerComponent", _) => {}', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: rows round down', 'src/textedit/images/stencil.rs', 'let bytes = width.div_ceil(8) * height;', 'let bytes = width / 8 * height;', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: no page budget', 'src/textedit/images/stencil.rs', '    if bytes > remaining {\n        return Err("decoded images exceed', '    if false {\n        return Err("decoded images exceed', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: accept Group 3', 'src/textedit/images/stencil.rs', '    if !integer(b"K", 0).is_some_and(|k| k < 0)', '    if !integer(b"K", 0).is_some()', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: ignore Columns', 'src/textedit/images/stencil.rs', '        || integer(b"Columns", 1728) != Some(width as i64)\n', '', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: ignore Rows', 'src/textedit/images/stencil.rs', '        || (rows != Some(0) && rows != Some(height as i64))\n', '', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: ignore damaged rows', 'src/textedit/images/stencil.rs', '        || integer(b"DamagedRowsBeforeError", 0) != Some(0)\n', '', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: accept byte-aligned rows', 'src/textedit/images/stencil.rs', '(b"EncodedByteAlign" | b"EndOfLine", Object::Boolean(false)) => {}', '(b"EncodedByteAlign" | b"EndOfLine", _) => {}', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: accept unknown parameters', 'src/textedit/images/stencil.rs', '            _ => return Err(invalid()),\n        }\n    }\n    Ok(())', '            _ => {}\n        }\n    }\n    Ok(())', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: decode only the first row', 'src/textedit/images/stencil.rs', '    for _ in 0..height {\n        if !matches!(decoder.advance()', '    for _ in 0..height.min(1) {\n        if !matches!(decoder.advance()', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: accept an early end of block', 'src/textedit/images/stencil.rs', 'if !matches!(decoder.advance(), Ok(DecodeStatus::Incomplete)) {', 'if decoder.advance().is_err() {', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: skip the Group 4 decode', 'src/textedit/images/stencil.rs', '        group4(&image.content, width, height)?;', '        let _ = group4;', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('stencil: skip the unfiltered length', 'src/textedit/images/stencil.rs', 'if filters::decode(image, bytes)?.len() != bytes {', 'if filters::decode(image, bytes)?.len() > bytes {', 'textedit_ccitt_stencil_masks_are_decoded_whole_and_kept'),
    Mutation('standard: every font measures as Helvetica', 'src/textedit/fonts.rs', '            metrics.widths[slot] = Some(f64::from(*width));', '            let _ = (slot, width);', 'textedit_standard_times_roman_measures_and_edits'),
    Mutation('standard: any name is Helvetica', 'src/textedit/fonts.rs', 'standard::FONTS.iter().find(|(name, _)| *name == base)?;', 'standard::FONTS.first()?;', 'textedit_standard_fonts_refuse_symbolic_and_unknown_names'),
    Mutation('standard: read-only text reserves no font box', 'src/textedit.rs', 'if let Some(bounds) = standard_box(doc, resources, name) {', 'if let Some(bounds) = standard_box(doc, resources, name).filter(|_| false) {', 'textedit_rotated_standard_stamp_stays_read_only_with_its_font_box_reserved'),
    Mutation('standard: the font box does not reach past the advance', 'src/textedit.rs', '(left.abs().max(right.abs()), bottom, top)', '(0., bottom, top)', 'textedit_rotated_standard_stamp_stays_read_only_with_its_font_box_reserved'),
    Mutation('standard: every font has the last font box', 'src/textedit/fonts.rs', 'standard::BOXES.iter().find(|(name, _)| *name == base)?;', 'standard::BOXES.last()?;', 'textedit_rotated_standard_stamp_stays_read_only_with_its_font_box_reserved'),
    Mutation('powerpoint: accept any writing mode', 'src/textedit/tagging.rs', 'Ok(value) if value.as_name().ok() != Some(b"LrTb") => Err(INVALID.into()),', 'Ok(_) if false => Err(INVALID.into()),', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('powerpoint: every block may be inline', 'src/textedit/tagging.rs', 'let placement: &[u8] = if tag == b"Lbl" { b"Inline" } else { b"Block" };', 'let placement: &[u8] = b"Inline";', 'textedit_lists_refuse_ambiguous_numbering_and_stale_attributes'),
    Mutation('powerpoint: every block may carry bounds', 'src/textedit/tagging.rs', 'if tag != b"Lbl" && attributes.has(b"BBox") {', 'if false && attributes.has(b"BBox") {', 'textedit_lists_refuse_ambiguous_numbering_and_stale_attributes'),
    Mutation("powerpoint: a label's bounds do not pin it", 'src/textedit/tagging.rs', 'Err(_) => Ok(bounded),', 'Err(_) => Ok(false),', 'textedit_lists_refuse_ambiguous_numbering_and_stale_attributes'),
    Mutation('powerpoint: attribute arrays skip their entries', 'src/textedit/tagging.rs', '                if objects.is_empty() || objects.len() > 8 {\n                    return Err(INVALID.into());\n                }\n                for object in objects {', '                if objects.is_empty() || objects.len() > 8 {\n                    return Err(INVALID.into());\n                }\n                for object in objects.iter().take(0) {', 'textedit_attribute_arrays_hold_each_object_to_the_same_rules'),
    Mutation('powerpoint: attribute arrays of any length', 'src/textedit/tagging.rs', 'if objects.is_empty() || objects.len() > 8 {', 'if false {', 'textedit_attribute_arrays_hold_each_object_to_the_same_rules'),
    Mutation('powerpoint: attributes checked under the raw tag', 'src/textedit/tagging.rs', 'element(doc, child, parent_id, &scope, owns_text, role)?;', 'element(doc, child, parent_id, &scope, owns_text, tag)?;', 'textedit_figure_aliases_are_figures'),
    Mutation('powerpoint: a figure alias keeps its raw name', 'src/textedit/tagging.rs', 'tag: if role == b"Figure" { role } else { tag },', 'tag,', 'textedit_figure_aliases_are_figures'),
    Mutation('powerpoint: figure aliases refused', 'src/textedit/tagging.rs', '&& !container(role) && role != b"Figure" {', '&& !container(role) {', 'textedit_figure_aliases_are_figures'),
    Mutation('powerpoint: figures refuse figures', 'src/textedit/tagging.rs', 'matches!((parent, child), (b"LBody", b"L") | (b"Figure", b"Figure"))', 'matches!((parent, child), (b"LBody", b"L"))', 'textedit_figures_may_hold_figures'),
    Mutation('powerpoint: bounded spacing need not be numbers', 'src/textedit/tagging.rs', '            if let Ok(value) = attributes.get(key) {\n                super::number(value)?;\n            }', '            if let Ok(value) = attributes.get(key) {\n                let _ = value;\n            }', 'textedit_read_only_owners_keep_their_layout_attributes'),
    Mutation('layer: any properties resource is a layer', 'src/textedit.rs', 'Some(b"OCG" | b"OCMD") => Ok(()),', '_ => Ok(()),', 'textedit_optional_content_keeps_its_text_read_only'),
    Mutation('layer: text inside is editable', 'src/textedit.rs', '        let read_only = tags.read_only()\n            || layer\n', '        let read_only = tags.read_only()\n', 'textedit_optional_content_keeps_its_text_read_only'),
    Mutation('layer: marked content may open inside', 'src/textedit.rs', 'if layer && matches!(op.operator.as_str(), "BDC" | "BMC") {', 'if false && layer {', 'textedit_optional_content_keeps_its_text_read_only'),
    Mutation('backtrack: a backtracking run is editable', 'src/textedit.rs', '            || layer\n            || backtracks\n', '            || layer\n', 'textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays'),
    Mutation('backtrack: a string that ends early is not flagged', 'src/textedit.rs', '            backtracks |= advance < furthest;\n            furthest = furthest.max(advance);', '            furthest = furthest.max(advance);', 'textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays'),
    Mutation('backtrack: a trailing pull-back is not flagged', 'src/textedit.rs', '    backtracks |= advance < furthest;\n    Ok((text, advance, bounds, lead, gaps, backtracks))', '    Ok((text, advance, bounds, lead, gaps, backtracks))', 'textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays'),
    Mutation('backtrack: refuse a position behind the origin', 'src/textedit.rs', 'if !advance.is_finite() || advance.abs() > 1_000_000.0 {', 'if !advance.is_finite() || !(0.0..=1_000_000.0).contains(&advance) {', 'textedit_kerning_refuses_malformed_unbounded_and_retreating_arrays'),
    Mutation('text state: Tf only inside a text block', 'src/textedit.rs', '            ("Tf", [name, size]) => {', '            ("Tf", [name, size]) if inside => {', 'textedit_font_and_leading_may_be_set_before_the_text_block'),
    Mutation('text state: TL only inside a text block', 'src/textedit.rs', '            ("TL", [value]) => leading = number(value)?,', '            ("TL", [value]) if inside => leading = number(value)?,', 'textedit_font_and_leading_may_be_set_before_the_text_block'),
    Mutation('mapping: refuse CMap labels on the Unicode path', 'src/textedit/fonts/mapping.rs', 'blocks_with_header(stream, wide, false, true)', 'blocks_with_header(stream, wide, false, false)', 'unicode_cid_accepts_the_cmap_labels_context_writes'),
    Mutation('composite: read collection strings directly', 'src/textedit/fonts/composite.rs', '            .map(|value| crate::encoding::resolve(doc, value))', '            .map(|value| value)', 'textedit_composite_collection_strings_may_be_indirect'),
    Mutation('text cm: refuse one before the first show', 'src/textedit.rs', '("cm", values) if (!inside || previous_show.is_none()) && values.len() == 6 => {', '("cm", values) if !inside && values.len() == 6 => {', 'textedit_page_transforms_refuse_unbounded_matrices_and_preserve_restored_state'),
    Mutation('text cm: accept one after a show', 'src/textedit.rs', '("cm", values) if (!inside || previous_show.is_none()) && values.len() == 6 => {', '("cm", values) if values.len() == 6 => {', 'textedit_page_transforms_refuse_unbounded_matrices_and_preserve_restored_state'),
    Mutation('standard: a missing BaseFont is Helvetica', 'src/textedit.rs','        .map_err(|_| unsupported())?;\n    let metrics = fonts::Metrics::standard(base)', '        .unwrap_or(b"Helvetica".as_slice());\n    let metrics = fonts::Metrics::standard(base)', 'textedit_standard_fonts_refuse_symbolic_and_unknown_names'),
    Mutation('rounding: no allowance on the ink edge', 'src/textedit.rs', 'replacement_bounds[1] > original[1] + 0.000_001 {', 'replacement_bounds[1] > original[1] {', 'textedit_an_equal_width_edit_fits_despite_rounding'),
    Mutation('cff: forget the original ligature names', 'src/textedit/fonts/cff/encoding.rs', '                                .chain(\n                                    super::super::ligatures::GLYPHS\n                                        .map(|(_, text, slot)| (text, slot)),\n                                )\n', '', 'textedit_cff_ligatures_accept_adobe_original_names'),
    Mutation('mapping: any character sequence is a ligature', 'src/textedit/fonts/mapping.rs', '!text.chars().all(char::is_alphabetic)', 'false', 'unicode_cid_preserves_scalars_and_refuses_ambiguous_or_overflowing_maps'),
    Mutation('mapping: no letter ligatures', 'src/textedit/fonts/mapping.rs', '!text.chars().all(char::is_alphabetic)', 'true', 'textedit_cid_letter_ligatures_decode_and_encode_by_longest_match'),
    Mutation('cid: any registry', 'src/textedit/fonts/cff/cid.rs', '&& string(strings, values[0])? == b"Adobe"', '&& string(strings, values[0]).is_some()', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: any top matrix', 'src/textedit/fonts/cff/cid.rs', '1207 => values == &[0.001, 0., 0., 0.001, 0., 0.],', '1207 => values.len() == 6,', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: any FD matrix', 'src/textedit/fonts/cff/cid.rs', '1207 => values == &[1., 0., 0., 1., 0., 0.],', '1207 => values.len() == 6,', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: any top operator', 'src/textedit/fonts/cff/cid.rs', '            _ => false,\n        };\n        if !valid {\n            return None;\n        }\n    }\n    [15', '            _ => true,\n        };\n        if !valid {\n            return None;\n        }\n    }\n    [15', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: any private operator', 'src/textedit/fonts/cff/cid.rs', '            _ => false,\n        };\n        if !valid {\n            return None;\n        }\n    }\n    let width', '            _ => true,\n        };\n        if !valid {\n            return None;\n        }\n    }\n    let width', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: FDSelect names any dict', 'src/textedit/fonts/cff/cid.rs', '|| end <= first || fd >= dicts', '|| end <= first', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: FDSelect may skip glyphs', 'src/textedit/fonts/cff/cid.rs', 'if first != result.len() || end <= first', 'if end <= first', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: FDSelect may stop short', 'src/textedit/fonts/cff/cid.rs', '(result.len() == glyphs).then_some(result)', 'Some(result)', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: duplicate CIDs', 'src/textedit/fonts/cff/cid.rs', '|| glyphs.insert(cid, GlyphId(index)).is_some()', '|| {\n                glyphs.insert(cid, GlyphId(index));\n                false\n            }', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid: CIDs past the count', 'src/textedit/fonts/cff/cid.rs', 'if f64::from(cid) >= count || glyphs', 'if glyphs', 'textedit_cid_programs_refuse_what_they_cannot_state'),
    Mutation('cid width: ignore nominalWidthX', 'src/textedit/fonts/cff/cid.rs', 'Some(if has { nominal + first? } else { default })', 'Some(if has { first? } else { default })', 'textedit_cid_widths_come_before_the_first_clearing_operator'),
    Mutation('cid width: rmoveto always states one', 'src/textedit/fonts/cff/cid.rs', '21 => stack == 3,', '21 => stack >= 2,', 'textedit_cid_widths_come_before_the_first_clearing_operator'),
    Mutation('cid width: stems always state one', 'src/textedit/fonts/cff/cid.rs', '1 | 3 | 18 | 23 | 19 | 20 => stack % 2 == 1,', '1 | 3 | 18 | 23 | 19 | 20 => true,', 'textedit_cid_widths_come_before_the_first_clearing_operator'),
    Mutation('cid width: read past a subroutine call', 'src/textedit/fonts/cff/cid.rs', '                    _ => return None,\n                };\n                let expected', '                    _ => false,\n                };\n                let expected', 'textedit_cid_widths_come_before_the_first_clearing_operator'),
    Mutation('cid width: unbounded stack', 'src/textedit/fonts/cff/cid.rs', '        if stack > MAX_STACK {', '        if stack > 10_000 {', 'textedit_cid_widths_come_before_the_first_clearing_operator'),
    Mutation('cid width: endchar with any operands', 'src/textedit/fonts/cff/cid.rs', '14 => [0, 1, 4, 5].contains(&stack),', '14 => true,', 'textedit_cid_widths_come_before_the_first_clearing_operator'),
    Mutation('cid: .notdef is writable', 'src/textedit/fonts/cff/cid.rs', 'read_only: glyph.0 == 0,', 'read_only: false,', 'textedit_cid_notdef_is_read_only_and_absent_cids_are_not_offered'),
    Mutation('cid: a CID the subset left out refuses the font', 'src/textedit/fonts/cff/cid.rs', '        let Some(&glyph) = self.glyphs.get(&cid) else {\n            return Ok(None);', '        let Some(&glyph) = self.glyphs.get(&cid) else {\n            return Err(INVALID.into());', 'textedit_cid_notdef_is_read_only_and_absent_cids_are_not_offered'),
    Mutation('cid: read the CID as a glyph index', 'src/textedit/fonts/cff/cid.rs', '        let Some(&glyph) = self.glyphs.get(&cid) else {', '        let Some(&glyph) = Some(&GlyphId(cid)).filter(|g| g.0 < self.table.number_of_glyphs()) else {', 'textedit_cid_keyed_cff_maps_codes_through_its_charset'),
    Mutation('composite: CFF may carry a CIDToGIDMap', 'src/textedit/fonts/composite.rs', '        Err(_) if cff => None,', '        _ if cff => None,', 'textedit_cid_font_dictionaries_refuse_other_carriers'),
    Mutation('composite: CFF refuses ForceBold', 'src/textedit/fonts/composite.rs', 'let force_bold = if cff { 0 } else { 262144 };', 'let force_bold = 262144;', 'textedit_cid_font_dictionaries_refuse_other_carriers'),
    Mutation('composite: any FontFile3 subtype for CFF', 'src/textedit/fonts/composite.rs', 'if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"CIDFontType0C") {', 'if false {', 'textedit_cid_font_dictionaries_refuse_other_carriers'),
    Mutation('composite: CFF beside a TrueType program', 'src/textedit/fonts/composite.rs', 'if descriptor.has(b"FontFile") || descriptor.has(b"FontFile2") {', 'if descriptor.has(b"FontFile") {', 'textedit_cid_font_dictionaries_refuse_other_carriers'),
    Mutation('unicode: a disagreeing width is writable', 'src/textedit/fonts/unicode.rs', '&& width > 0. && (width - advance).abs() <= 1.;', '&& width > 0.;', 'textedit_cid_widths_follow_nominal_width_and_disagreement_is_read_only'),
    Mutation('unicode: an opaque glyph reads as its text', 'src/textedit/fonts/unicode.rs', '            if self.opaque.contains(code) {', '            if false {', 'textedit_cid_widths_follow_nominal_width_and_disagreement_is_read_only'),
    Mutation('unicode: drop an opaque glyph', 'src/textedit/fonts/unicode.rs', '                    if !agrees\n                        && left >= -reach', '                    if false\n                        && left >= -reach', 'textedit_cid_widths_follow_nominal_width_and_disagreement_is_read_only'),
    Mutation('unicode: a shared text is writable', 'src/textedit/fonts/unicode.rs', '    for text in &ambiguous {\n        reverse.remove(text);\n    }\n', '', 'textedit_shared_texts_take_the_glyph_their_run_shows'),
    Mutation('unicode: a run chooses nothing', 'src/textedit/fonts/unicode.rs', '                self.ambiguous.remove(&text);\n                self.reverse.insert(text, code);\n', '', 'textedit_shared_texts_take_the_glyph_their_run_shows'),
    Mutation('unicode: a run showing both glyphs chooses one', 'src/textedit/fonts/unicode.rs', 'if choice.is_some_and(|first| self.identities.get(&first) != Some(identity)) {', 'if false {', 'textedit_shared_texts_take_the_glyph_their_run_shows'),
    Mutation('unicode: one glyph under two codes is a choice', 'src/textedit/fonts/unicode.rs', 'Some(first) if identities.get(first) != Some(identity) => {', 'Some(_) => {', 'shared_texts_are_ambiguous_only_between_different_glyphs'),
    Mutation('unicode: a glyph at another width is the same choice', 'src/textedit/fonts/unicode.rs', 'identities.insert(code, (id, width.to_bits()));', 'identities.insert(code, (id, 0));', 'shared_texts_are_ambiguous_only_between_different_glyphs'),
    Mutation("textedit: write without the run's glyphs", 'src/textedit.rs', '.preferring(&shown(\n            content,', '.preferring(&shown(\n            &lopdf::content::Content { operations: Vec::new() },', 'textedit_shared_texts_take_the_glyph_their_run_shows'),
    Mutation('mapping: refuse a final comment', 'src/textedit/fonts/mapping.rs', "    bytes.push(b'\\n');\n", '', 'unicode_cid_accepts_the_cmap_typst_writes'),
    Mutation('mapping: vertical labels', 'src/textedit/fonts/mapping.rs', '(b"WMode", Object::Integer(0)) => true,', '(b"WMode", Object::Integer(_)) => true,', 'unicode_cid_labels_are_a_closed_grammar'),
    Mutation('mapping: labels may repeat', 'src/textedit/fonts/mapping.rs', 'if !valid || !seen.insert(key.clone()) {', 'if !valid {', 'unicode_cid_labels_are_a_closed_grammar'),
    Mutation('mapping: any CMap type', 'src/textedit/fonts/mapping.rs', '(b"CMapType", Object::Integer(kind)) => (0..=2).contains(kind),', '(b"CMapType", Object::Integer(_)) => true,', 'unicode_cid_labels_are_a_closed_grammar'),
    Mutation("cff builtin: ignore the program's encoding", 'src/textedit/fonts/cff.rs', 'encoding: encoding::builtin(bytes, &face)?,', 'encoding: super::type1::program::Builtin::Standard,', 'textedit_cff_builtin_encodings_name_glyphs_through_the_type1_rules'),
    Mutation('cff builtin: read Expert as Standard', 'src/textedit/fonts/cff/encoding.rs', '        _ => return Err(invalid()),\n    };\n    let byte', '        _ => return Ok(Builtin::Standard),\n    };\n    let byte', 'textedit_cff_builtin_encodings_name_glyphs_through_the_type1_rules'),
    Mutation('cff builtin: range codes start at the range', 'src/textedit/fonts/cff/encoding.rs', 'let last = first + byte(offset + 3 + range * 2)?;', 'let last = first;', 'textedit_cff_builtin_encodings_name_glyphs_through_the_type1_rules'),
    Mutation('dispatch: symbolic CFF takes the WinAnsi path', 'src/textedit/fonts.rs', 'Some(b"Type1C") if symbolic || !font.has(b"Encoding") =>', 'Some(b"Type1C") if !font.has(b"Encoding") =>', 'textedit_cff_builtin_encodings_name_glyphs_through_the_type1_rules'),
    Mutation('dispatch: an unencoded CFF takes the WinAnsi path', 'src/textedit/fonts.rs', 'Some(b"Type1C") if symbolic || !font.has(b"Encoding") =>', 'Some(b"Type1C") if symbolic =>', 'textedit_cff_builtin_encodings_name_glyphs_through_the_type1_rules'),
    Mutation('layout: count the space a line broke at as ink', 'src/textedit/layout.rs', "                let (advance, ink) = metrics.gapped_layout(\n                    text.trim_end_matches(' '),", '                let (advance, ink) = metrics.gapped_layout(\n                    text,', 'wrapping_fits_a_line_by_its_words_not_the_space_it_broke_at'),
    Mutation('truetype: refuse a program without OS/2', 'src/textedit/fonts.rs', '            return Err("embedded font does not permit this editable use".into());\n        }\n    }\n    Ok(face)', '            return Err("embedded font does not permit this editable use".into());\n        }\n    } else {\n        return Err(invalid());\n    }\n    Ok(face)', 'textedit_macroman_requires_matching_encoding_and_honours_present_rights'),
    Mutation('unicode: a zero width refuses the font', 'src/textedit/fonts/unicode.rs', 'if width < 0. {', 'if width <= 0. {', 'textedit_zero_width_marks_are_read_only'),
    Mutation('unicode: a zero-width mark is written', 'src/textedit/fonts/unicode.rs', '!read_only && width > 0. && ', '!read_only && ', 'textedit_zero_width_marks_are_read_only'),
    Mutation('unicode: any glyph may stand still', 'src/textedit/fonts/unicode.rs', 'if step < 0. || (step == 0. && !self.opaque.contains(code)) {', 'if step < 0. {', 'textedit_zero_width_marks_are_read_only'),
    Mutation('bounded table: forget the ink bounds it declared', 'src/textedit/tagging.rs', '            return Ok(tag == b"Table");\n', '            return Ok(false);\n', 'textedit_bounded_tables_keep_their_ink_bounds_and_stop_offering_cell_text'),
    Mutation('bounded table: look for the bounds on the wrong element', 'src/textedit/tagging.rs', '            return Ok(tag == b"Table");\n', '            return Ok(tag == b"TR");\n', 'textedit_bounded_tables_keep_their_ink_bounds_and_stop_offering_cell_text'),
    Mutation('bounded table: leave its content editable', 'src/textedit/tagging.rs', '                    if pinned {\n                        bounded_slots.entry(owner).or_default().insert(mcid);\n                    }', '', 'textedit_bounded_tables_keep_their_ink_bounds_and_stop_offering_cell_text'),
    Mutation('bounded table: offer its text anyway', 'src/textedit/tagging.rs', '|| self.bounded.contains(&mcid)', '|| false', 'textedit_bounded_tables_keep_their_ink_bounds_and_stop_offering_cell_text'),
    Mutation('bounded table: read the bounds from anywhere', 'src/textedit/tagging.rs', '            return Ok(tag == b"Table");\n        }\n        return Ok(false);', '            return Ok(tag == b"Table");\n        }\n        return Ok(tag == b"Table");', 'textedit_bounded_tables_keep_their_ink_bounds_and_stop_offering_cell_text'),
    Mutation('nested list: keep a sublist out of the list body', 'src/textedit/tagging.rs', 'matches!((parent, child), (b"LBody", b"L") | (b"Figure", b"Figure"))', 'matches!((parent, child), (b"Figure", b"Figure"))', 'textedit_lists_nested_in_a_list_body_keep_every_level_editable'),
    Mutation('nested list: defer any child of a list body', 'src/textedit/tagging.rs', 'matches!((parent, child), (b"LBody", b"L") | (b"Figure", b"Figure"))', 'matches!((parent, child), (b"LBody", _) | (b"Figure", b"Figure"))', 'textedit_list_bodies_still_validate_their_own_inline_leaves'),
    Mutation('nested list: defer a child of any element', 'src/textedit/tagging.rs', 'matches!((parent, child), (b"LBody", b"L") | (b"Figure", b"Figure"))', 'matches!(child, b"L" | b"Figure") || parent.is_empty()', 'textedit_paragraphs_refuse_a_list_among_their_content'),
    Mutation('nested list: drop the deferred sublists', 'src/textedit/tagging.rs', '            pending.extend(\n                deferred\n                    .into_iter()\n                    .map(|(item, parent, pinned)| (item, parent, depth, pinned)),\n            );', '', 'textedit_lists_nested_in_a_list_body_keep_every_level_editable'),
    Mutation('nested list: bound the deferred frontier', 'src/textedit/tagging.rs', '            if deferred.len() + pending.len() > MAX_NODES {\n                return Err("tagged sublist frontier exceeds its limit".into());\n            }', '', 'textedit_deferred_sublists_are_bounded_before_the_walk_visits_them'),
    Mutation('note: refuse a footnote its own blocks', 'src/textedit/tagging.rs', 'matches!(tag, b"Part" | b"Art" | b"Sect" | b"Div" | b"Note")', 'matches!(tag, b"Part" | b"Art" | b"Sect" | b"Div")', 'textedit_notes_group_their_blocks_and_accept_a_producer_role_for_them'),
    Mutation('form: accept a layer entry that is not one', 'src/textedit/forms.rs', '                if !matches!(\n                    group.get(b"Type").and_then(Object::as_name).ok(),\n                    Some(b"OCG") | Some(b"OCMD")\n                ) {\n                    return Err(INVALID.into());\n                }', '', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('form: accept private data that is not a dictionary', 'src/textedit/forms.rs', '            (b"PieceInfo", _) => {\n                dictionary(doc, value)?;\n            }', '            (b"PieceInfo", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('form: accept any modification stamp', 'src/textedit/forms.rs', '(b"LastModified", Object::String(bytes, _)) if bytes.len() <= 127 => {}', '(b"LastModified", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('form: refuse text state before a text object', 'src/textedit/forms.rs', '("Tc" | "Tw" | "Tz" | "TL" | "Tf" | "Tr" | "Ts", _) => {}', '("Tc" | "Tw" | "Tz" | "TL" | "Tf" | "Tr" | "Ts", _) if inside => {}', 'textedit_preserved_forms_accept_text_state_outside_a_text_object'),
    Mutation('form: accept positioning outside a text object', 'src/textedit/forms.rs', '("Td" | "TD" | "Tm" | "T*" | "Tj" | "TJ" | "\'" | "\\"", _) if inside => {}', '("Td" | "TD" | "Tm" | "T*" | "Tj" | "TJ" | "\'" | "\\"", _) => {}', 'textedit_preserved_forms_accept_text_state_outside_a_text_object'),
    Mutation('form: treat text state as painted text', 'src/textedit/forms.rs', '("BT", []) if !inside => {\n                inside = true;\n                has_text = true;\n            }', '("BT", []) if !inside => {\n                inside = true;\n                has_text = true;\n            }\n            ("Tc", _) => has_text = true,', 'textedit_preserved_forms_accept_text_state_outside_a_text_object'),
    Mutation('form: charge drawn images to the form content bound', 'src/textedit/forms.rs', 'let image = images::check(doc, resources, name, budget.remaining)?;', 'let image = images::check(doc, resources, name, budget.content.min(budget.remaining))?;', 'textedit_preserved_form_images_share_the_page_image_budget'),
    Mutation('form: stop charging form content to the page budget', 'src/textedit/forms.rs', '    budget.content -= bytes.len();\n    budget.remaining -= bytes.len();', '    budget.content -= bytes.len();', 'textedit_preserved_form_images_share_the_page_image_budget'),
    Mutation('form: one byte more form content', 'src/textedit/forms.rs', 'content: MAX_FORM_CONTENT,', 'content: MAX_FORM_CONTENT + 1,', 'textedit_preserved_forms_accept_large_figures_up_to_their_own_bounds'),
    Mutation('form: one byte less form content', 'src/textedit/forms.rs', 'content: MAX_FORM_CONTENT,', 'content: MAX_FORM_CONTENT - 1,', 'textedit_preserved_forms_accept_large_figures_up_to_their_own_bounds'),
    Mutation('form: refuse the last form operation', 'src/textedit/forms.rs', 'if budget.operations > MAX_FORM_OPERATIONS {', 'if budget.operations >= MAX_FORM_OPERATIONS {', 'textedit_preserved_forms_accept_large_figures_up_to_their_own_bounds'),
    Mutation('form: accept any transparency group', 'src/textedit/forms.rs', '(b"Group", _) => group(doc, value)?,', '(b"Group", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('form: accept a group that is not transparency', 'src/textedit/forms.rs', 'if group.get(b"S").and_then(Object::as_name).ok() != Some(b"Transparency") {', 'if false {', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('form: accept any group colour space', 'src/textedit/forms.rs', '                colors::space(doc, value)?;', '                let _ = colors::space;', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('form: accept any isolated or knockout flag', 'src/textedit/forms.rs', '(b"I" | b"K", Object::Boolean(_)) => {}', '(b"I" | b"K", _) => {}', 'textedit_preserved_forms_keep_layer_membership_and_private_data'),
    Mutation('soft mask: skip alpha validation', 'src/textedit/images.rs', 'bytes: bytes + samples(doc, None, mask, remaining - bytes)?,', 'bytes,', 'textedit_soft_masks_refuse_colour_palettes_and_masks_of_their_own'),
    Mutation('soft mask: reset the shared budget', 'src/textedit/images.rs', 'samples(doc, None, mask, remaining - bytes)?', 'samples(doc, None, mask, remaining)?', 'textedit_soft_masked_images_keep_both_streams_and_charge_the_page_budget'),
    Mutation('soft mask: follow a mask of a mask', 'src/textedit/images.rs', 'if mask.dict.has(b"SMask") {', 'if false {', 'textedit_soft_masks_refuse_colour_palettes_and_masks_of_their_own'),
    Mutation('soft mask: read alpha as colour', 'src/textedit/images.rs', 'None if space.as_name().ok() == Some(b"DeviceGray") => 1,', 'None if space.as_name().is_ok() => 1,', 'textedit_soft_masks_refuse_colour_palettes_and_masks_of_their_own'),
    Mutation('image: accept any sample mapping', 'src/textedit/images.rs', 'if mapping != default {', 'if false {', 'textedit_images_accept_only_the_default_sample_mapping'),
    Mutation('image: map an indexed sample range to its palette', 'src/textedit/images.rs', 'vec![0., 255.]', 'vec![0., 1.]', 'textedit_images_accept_only_the_default_sample_mapping'),
    Mutation('image: accept any decode parameters', 'src/textedit/images.rs', '(b"DecodeParms", _) => parameters(doc, value)?,', '(b"DecodeParms", _) => {}', 'textedit_images_accept_decode_parameters_that_select_no_prediction'),
    Mutation('image: accept any predictor', 'src/textedit/images.rs', 'b"Predictor" if value.as_i64().ok() == Some(1) => {}', 'b"Predictor" => {}', 'textedit_images_accept_decode_parameters_that_select_no_prediction'),
    Mutation('content: decode a parameterised stream', 'src/textedit/filters.rs', 'if stream.dict.has(b"DecodeParms") {', 'if false {', 'textedit_images_accept_decode_parameters_that_select_no_prediction'),
    Mutation('image: accept an untyped metadata packet', 'src/textedit/images.rs', 'if packet.dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"Metadata") {', 'if false {', 'textedit_image_metadata_packets_are_kept_and_must_declare_their_type'),
    Mutation('image: accept metadata that is not a packet', 'src/textedit/images.rs', '(b"Metadata", _) => {', '(b"Metadata", _) => if false {', 'textedit_image_metadata_packets_are_kept_and_must_declare_their_type'),
    Mutation('image: forget marked image content', 'src/textedit.rs', 'tags.paint();\n            }\n            ("w", [value])', '// paint omitted\n            }\n            ("w", [value])', 'textedit_tagged_painted_content_preserves_structure_and_refuses_empty_items'),

    Mutation('intent: skip direct intent validation', 'src/textedit.rs', '("ri", [Object::Name(name)]) => colors::intent(name)?,', '("ri", [Object::Name(_)]) => {},', 'textedit_rendering_intents_refuse_unknown_names_types_and_implicit_positions'),
    Mutation('intent: skip graphics state intent validation', 'src/textedit/graphics.rs', '(b"RI", Object::Name(name)) => super::colors::intent(name)?,', '(b"RI", Object::Name(_)) => {},', 'textedit_rendering_intents_refuse_unknown_names_types_and_implicit_positions'),
    Mutation('intent: setter positions a following show', 'src/textedit.rs', '("ri", [Object::Name(name)]) => colors::intent(name)?,', '("ri", [Object::Name(name)]) => { colors::intent(name)?; positioned = true; },', 'textedit_rendering_intents_refuse_unknown_names_types_and_implicit_positions'),

    Mutation('spacing: ignore nonzero character spacing', 'src/textedit/fonts.rs', 'if spacing == 0. && word_spacing == 0. {', 'if true {', 'textedit_spacing_measures_character_steps_and_tj_fragments'),
    Mutation('spacing: omit trailing character step', 'src/textedit/fonts.rs', 'cursor += step;', 'cursor += width;', 'textedit_spacing_measures_character_steps_and_tj_fragments'),
    Mutation('spacing: omit relative size limit', 'src/textedit/fonts.rs', '!spacing.is_finite() || spacing.abs() > size * 0.25', '!spacing.is_finite()', 'textedit_spacing_bounds_and_malformed_setters_remain_refused'),
    Mutation('spacing: allow nonpositive glyph steps', 'src/textedit/fonts.rs', 'if step <= 0. {', 'if false {', 'textedit_spacing_bounds_and_malformed_setters_remain_refused'),
    Mutation('spacing: omit accumulated advance limit', 'src/textedit/fonts.rs', 'cursor > 1_000_000.', 'false', 'textedit_spacing_bounds_and_malformed_setters_remain_refused'),
    Mutation('spacing: ignore spacing in kerning arrays', 'src/textedit.rs', 'metrics.source_layout(bytes, size, spacing, word_spacing)?', 'metrics.source_layout(bytes, size, 0., word_spacing)?', 'textedit_spacing_measures_character_steps_and_tj_fragments'),
    Mutation('spacing: ignore spacing when writing', 'src/textedit.rs', 'let (advance, bounds) = metrics.gapped_layout(replacement, size, spacing, word_spacing, gap)?;', 'let (advance, bounds) = metrics.gapped_layout(replacement, size, 0., word_spacing, gap)?;', 'textedit_spacing_replacement_advance_and_ink_are_both_checked_atomically'),
    Mutation('spacing: forget saved character spacing', 'src/textedit.rs', "                    clip,\n                    spacing,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", "                    clip,\n                    0.,\n                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", 'textedit_spacing_persists_across_blocks_and_restores_for_writing'),
    Mutation('spacing: reset spacing at each text block', 'src/textedit.rs', 'inside = true;\n                positioned = false;', 'inside = true;\n                spacing = 0.;\n                positioned = false;', 'textedit_spacing_persists_across_blocks_and_restores_for_writing'),

    Mutation('print state: accept nonboolean print flags', 'src/textedit/graphics.rs', '(b"OP" | b"op" | b"SA", Object::Boolean(_))', '(b"OP" | b"op" | b"SA", _)', 'textedit_print_graphics_state_refuses_masks_types_and_invalid_modes_atomically'),
    Mutation('print state: accept arbitrary overprint modes', 'src/textedit/graphics.rs', '(b"OPM", Object::Integer(0 | 1))', '(b"OPM", _)', 'textedit_print_graphics_state_refuses_masks_types_and_invalid_modes_atomically'),
    Mutation('print state: accept active soft mask names', 'src/textedit/graphics.rs', '(b"SMask", Object::Name(name)) if name == b"None"', '(b"SMask", Object::Name(_))', 'textedit_print_graphics_state_refuses_masks_types_and_invalid_modes_atomically'),
    Mutation('print state: accept active soft mask dictionaries', 'src/textedit/graphics.rs', '(b"SMask", Object::Name(name)) if name == b"None"', '(b"SMask", _)', 'textedit_print_graphics_state_refuses_masks_types_and_invalid_modes_atomically'),
    Mutation('print state: accept nondefault alpha source', 'src/textedit/graphics.rs', '(b"AIS", Object::Boolean(false))', '(b"AIS", _)', 'textedit_print_graphics_state_refuses_masks_types_and_invalid_modes_atomically'),

    Mutation('word spacing: omit negative relative size limit', 'src/textedit/fonts.rs', 'word_spacing < -size * 0.25', 'false', 'textedit_word_spacing_bounds_combined_backtracking_and_positioning'),
    Mutation('word spacing: use Unicode space for custom codes', 'src/textedit/fonts.rs', 'Some(Codes::Single(codes)) => codes[32],', 'Some(Codes::Single(_)) => Some(32),', 'textedit_word_spacing_uses_pdf_code_32_not_unicode_space'),
    Mutation('word spacing: apply to two-byte code 32', 'src/textedit/fonts.rs', 'Some(Codes::Double(_)) => None,', 'Some(Codes::Double(codes)) => codes.get(&32).copied(),', 'textedit_word_spacing_uses_pdf_code_32_not_unicode_space'),
    Mutation('word spacing: ignore kerning fragment spacing', 'src/textedit.rs', 'metrics.source_layout(bytes, size, spacing, word_spacing)?', 'metrics.source_layout(bytes, size, spacing, 0.)?', 'textedit_word_spacing_measures_combined_steps_and_kerning_fragments'),
    Mutation('word spacing: ignore replacement spacing', 'src/textedit.rs', 'let (advance, bounds) = metrics.gapped_layout(replacement, size, spacing, word_spacing, gap)?;', 'let (advance, bounds) = metrics.gapped_layout(replacement, size, spacing, 0., gap)?;', 'textedit_word_spacing_restores_state_and_checks_replacements_atomically'),
    Mutation('word spacing: forget saved spacing', 'src/textedit.rs', "                    word_spacing,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", "                    0.,\n                    stroke_components,\n                    compound_clips.clone(),\n                    (render, line_width),\n                ));", 'textedit_word_spacing_restores_state_and_checks_replacements_atomically'),
    Mutation('word spacing: reset at each text block', 'src/textedit.rs', 'inside = true;\n                positioned = false;', 'inside = true;\n                word_spacing = 0.;\n                positioned = false;', 'textedit_word_spacing_restores_state_and_checks_replacements_atomically'),

    Mutation("choices: retarget radio answers when pages move", "src/forms.rs", "        group.sort_by_key(|i| result.widgets[*i].widget);", "        // keep page order", "radio_answers_survive_page_moves_and_support_duplicate_states"),
    Mutation("choices: draw export values instead of labels", "src/forms.rs", "                .map(|i| options[*i].label.as_str())", "                .map(|i| options[*i].export.as_str())", "choices_and_radio_round_trip_exports_indices_and_appearances"),
    Mutation("choices: ignore the selected radio sibling", "src/forms.rs", "                            selected == index || (*unison && states[*selected] == states[*index])", "                            selected == index || (!*unison && states[*selected] != states[*index])", "choices_and_radio_round_trip_exports_indices_and_appearances"),
    Mutation("forms: skip answers in the save pipeline", "src/save.rs", "    crate::forms::write(&mut doc, &plan.forms)?;", "    // form answers omitted", "form_answers_force_a_rewrite_and_reach_the_saved_file"),
    Mutation("forms: write no appearances or values", "src/forms.rs", "    if changes.is_empty() {", "    if true {", "forms_round_trip_values_and_every_shared_widget_appearance"),
    Mutation("forms: lose current answers on replay", "src/docmodel.rs", "                self.forms.insert(object, version);", "                let _ = (object, version);", "forms_journal_undo_redo_and_redo_branch_are_independent_of_comments"),
    Mutation(
        # Show the engine a control no scale can render, as the gate did until
        # 2026-08-28. The region comes back `ControlUnread` -- true, and it
        # points at the engine, which is not where the problem is.
        "gate: render a control no scale can bring to the floor",
        "src/ocr_gate.rs",
        "    if wanted > MAX_SCALE {",
        "    if false {",
        "a_control_no_scale_can_render_is_refused_with_a_cause_of_its_own",
    ),
    Mutation(
        # Refuse the boundary too. A 2 pt control reaches exactly
        # MIN_CONTROL_PX at MAX_SCALE and is the smallest the gate can serve;
        # `>=` turns it into the largest it turns away.
        "gate: refuse the smallest control the ceiling can still render",
        "src/ocr_gate.rs",
        "    if wanted > MAX_SCALE {",
        "    if wanted >= MAX_SCALE {",
        "a_control_exactly_at_the_smallest_servable_size_is_served",
    ),
    Mutation(
        # File the refusal under the buffer's cause. Both are refusals from the
        # same function, the count still adds up, and a reader is sent to
        # `capacity` for a problem that is the control's size.
        "gate: report an unrenderable control as a probe image that will not fit",
        "src/ocr_gate.rs",
        "crate::ocr::NotVerifiedCause::ControlTooSmall,",
        "crate::ocr::NotVerifiedCause::ScaleRefused,",
        "a_control_no_scale_can_render_is_refused_with_a_cause_of_its_own",
    ),
    Mutation(
        # Merge the document as the file holds it rather than as the reader has
        # it. Every edit they made -- deleted pages, turns, crops, marks -- is
        # silently absent from the result, and the page count agrees with itself
        # for any plan that keeps every page. Only a plan that drops one can
        # tell.
        #
        # **Re-aimed 2026-09-01**, when the merge moved into a worker: it read
        # `source` by path, and the pure function that replaced it has no path
        # to read. Skipping the rewrite is the same fact expressed against the
        # bytes.
        "merge: read the source file instead of the document the reader has",
        "src/save.rs",
        "    let base = rewrite_update(base, plan, Job::Save, password)?;",
        "    let base = base.to_vec();",
        "the_open_documents_edits_reach_the_merge",
    ),
    Mutation(
        # Add the reader's view to a page turn the plan did not reduce. The
        # plan crosses a process boundary, so `PageView::turns` documenting
        # 0 to 3 is a contract the caller may break rather than a range this
        # side may lean on -- and unreduced, `page.turns + view % 4`
        # overflows `u8` from 253 up. Found by the fuzz target, whose own
        # generator had to reduce the value to reach anything behind it.
        #
        # **The answer is not what this mutation breaks**, which is why the
        # test it names asserts a rotation and a control rather than mere
        # survival: 256 is a multiple of 4, so the wrap gives the true
        # remainder in a build with overflow checks off. What it breaks is
        # every build that has them on, `cargo test` included -- so this
        # mutation kills by panic, and that panic is the defect.
        "save: add the view to an unreduced page turn",
        "src/save.rs",
        "            Ok((slot, (page.turns % 4 + view % 4) % 4))",
        "            Ok((slot, (page.turns + view % 4) % 4))",
        "a_turn_past_the_documented_range_is_reduced_rather_than_added",
    ),
    Mutation(
        # Write the parts over any file already sitting at their names. The
        # reader picked ONE name in a dialog and the platform asked about that
        # one; every other part is a path this module invented, so this
        # replaces files nobody was warned about. `write_atomically` finishes
        # with a rename, which does exactly that.
        "split: write over a part that already exists",
        "src/save.rs",
        "        if target.exists() {",
        "        if false {",
        "a_split_refuses_an_existing_part_and_writes_nothing",
    ),
    Mutation(
        # Number the parts from zero. `report-0.pdf` is not what any reader
        # expects, and the off-by-one is invisible in a page count -- every part
        # holds the right pages under the wrong name.
        "split: number the parts from zero",
        "src/save.rs",
        "    (1..=count)",
        "    (0..count)",
        "split_paths_number_from_one_and_never_use_the_chosen_name",
    ),
    Mutation(
        # Take the whole file name as the stem, so `report.pdf` becomes
        # `report.pdf-1.pdf`. `file_name` and `file_stem` differ only for a name
        # that has an extension, which every one here does.
        "split: build the part names from the whole file name",
        "src/save.rs",
        "        .file_stem()",
        "        .file_name()",
        "split_paths_keep_a_dot_that_is_inside_the_stem",
    ),
    Mutation(
        # Let a one-plan split through. It writes `name-1.pdf` and nothing else,
        # which is an extract wearing the wrong command's name -- and the reader
        # asked to split a document and got one file back.
        "split: accept a split that writes a single file",
        "src/save.rs",
        "    if plans.len() < 2 {",
        "    if false {",
        "a_split_into_one_file_is_refused",
    ),
    Mutation(
        # Let an encrypted document be merged in. `lopdf` writes plaintext and
        # drops the dictionary, so the merged file carries a
        # permission-restricted document's pages with the restrictions gone --
        # and nothing in the result says so.
        "merge: accept an encrypted document as an input",
        "src/save.rs",
        "        if incoming.was_encrypted() || incoming.is_encrypted() {",
        "        if false {",
        "an_encrypted_document_cannot_be_merged_in",
    ),
    Mutation(
        # Write a merge of nothing, which is a Save a copy the reader did not
        # ask for, under a name they chose for something else.
        "merge: allow a merge with no documents to merge in",
        "src/save.rs",
        "    if others.is_empty() {",
        "    if false {",
        "a_merge_of_no_documents_is_refused",
    ),
    Mutation(
        # Check only the open document, not the files going in. The destination
        # is the easier of the two for a reader to get wrong: the save dialog
        # opens in the directory they just picked the inputs from.
        "merge: guard the destination against the source alone",
        "src/save.rs",
        # Re-aimed after `cargo fmt` broke the call across three lines, which is
        # why the anchor is the multi-line form: a one-line anchor here was
        # correct when it was written and matched nothing an hour later.
        "    for input in [source]\n        .into_iter()\n        .chain(others.iter().map(PathBuf::as_path))\n    {",
        "    for input in [source].into_iter()\n    {",
        "a_merge_will_not_be_written_over_any_document_going_into_it",
    ),
    Mutation(
        # Report the plan's page count rather than the merged file's. The number
        # a reader is shown then describes what they had, not what was written
        # -- so a merge that dropped every incoming page reports success with a
        # plausible figure.
        # **Re-aimed 2026-09-01**, when the count moved into `merge_update` with
        # the parse that can take it. It is the worker's half; the coordinator's
        # is *report a page count of this process's own*, below.
        "merge: report the open document's page count as the merge's",
        "src/save.rs",
        "    let pages = merged.get_pages().len() as u32;",
        "    let pages = plan.pages.len() as u32;",
        "a_merge_holds_every_page_of_every_document",
    ),
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
        "    let ready = rewrite_ready(source, plan, OnChange::Proceed)?;",
        "    let ready = rewrite_ready(source, plan, OnChange::Refuse)?;",
        "a_copy_is_written_when_the_source_changed_and_reports_it",
    ),
    Mutation(
        # And the other direction, which is the dangerous one: let a save in
        # place proceed over a file that changed, because a copy may. The
        # asymmetry is the whole design and a single word carries it.
        "save: let a save in place tolerate a changed file, as a copy does",
        "src/save.rs",
        "    let ready = rewrite_ready(source, plan, OnChange::Refuse)?;",
        "    let ready = rewrite_ready(source, plan, OnChange::Proceed)?;",
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
        "src/save/marks.rs",
        "    for stroke in strokes {",
        "    for stroke in strokes.iter().take(1) {",
        "each_stroke_is_its_own_path_in_the_appearance_stream",
    ),
    Mutation(
        # Mitre the joins, which is the default. A hand-drawn corner turns at
        # whatever angle the hand made, and a mitre on a sharp one spikes out to
        # a point that reads as a rendering fault.
        "save: leave a drawing's joins mitred, as a box's are",
        "src/save/marks.rs",
        '        Paint::Path => (mark.width, "1 J 1 j "),',
        '        Paint::Path => (mark.width, ""),',
        "each_stroke_is_its_own_path_in_the_appearance_stream",
    ),
    Mutation(
        # Write `/InkList` on every kind. The appearance stream is unchanged, so
        # every pixel check still passes and every reader still draws the right
        # thing --- what breaks is the file's own account of what it holds.
        "save: write an /InkList on kinds that have no strokes",
        "src/save/marks.rs",
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
        "            quads = Stroke::bounds(&strokes, (width / 2.0) as f32)",
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
        # Off by one at the front of the range. A panel's "1" is the first sheet
        # and not an index, so this prints page 2 when the reader asked for 1 ---
        # a wrong page rather than a missing one, which is the shape that reaches
        # paper looking exactly like a job that worked.
        "print: read a page range's first number as an index rather than a page",
        "src/print.rs",
        "    Ok((first - 1..last).collect())",
        "    Ok((first..last).collect())",
        "a_page_range_names_the_sheets_between_its_ends",
    ),
    Mutation(
        # Clamp a range that runs off the end instead of refusing it. "3 to 99" on
        # a four-sheet job becomes "3 to 4", which is a plausible answer to a
        # question nobody asked --- and the only place it can be noticed is the
        # paper.
        "print: clamp a page range to the job instead of refusing it",
        "src/print.rs",
        "    if last > count {",
        "    if false {",
        "a_range_that_cannot_be_printed_is_refused_rather_than_clamped",
    ),
    Mutation(
        # Take a backwards range as empty rather than as a mistake. `4 to 2` then
        # spools nothing and reports success, which reads as a printer that did
        # not respond.
        "print: let a backwards page range through as an empty selection",
        "src/print.rs",
        "    if first > last {",
        "    if first > last && false {",
        "a_range_that_cannot_be_printed_is_refused_rather_than_clamped",
    ),
    Mutation(
        # Drop the border and draw only the word. A stamp is then a `/FreeText`
        # wearing a `/Stamp` subtype: every reader shows the word, `--mode
        # roundtrip` still reads the kind back, and only the one check that asks
        # for both halves can tell.
        "save: draw a stamp's word without its border",
        "src/save/marks.rs",
        '        out.push_str(&format!("{x} {y} {width} {height} re S\\n"));\n        if inner_w <= 0.0 || inner_h <= 0.0 {',
        "        if inner_w <= 0.0 || inner_h <= 0.0 {",
        "a_stamp_is_a_border_and_a_word_rather_than_either_alone",
    ),
    Mutation(
        # Say a word other than the one the stamp was made with. The stamp is
        # still bordered, still says something, and says the wrong thing --- which
        # is why the test asserts the encoded word rather than that any text was
        # drawn.
        #
        # **This drew the reader's *note* until the Paint arms were extracted on
        # 2026-08-26**, which was the same defect in its most tempting spelling.
        # `draw_stamp` is handed `stamp: Option<StampName>` rather than the mark,
        # so the note is not in scope there any more and that mutation stopped
        # compiling -- `error[E0425]: cannot find value 'note'`, which the harness
        # reports as "not caught" and is right to, since a build failure is not a
        # red test. Re-aimed rather than deleted: the compiler removed one
        # spelling of the defect and not the class, because a stamp can still be
        # made to say the wrong word. The trap index has the rule under a guard
        # the type system makes unexpressible.
        "save: draw a word other than the name a stamp was made with",
        "src/save/marks.rs",
        '        out.push_str(&format!("<{}> Tj\\n", winansi_hex(word)));',
        '        out.push_str(&format!("<{}> Tj\\n", winansi_hex("stamped")));',
        "a_stamp_is_a_border_and_a_word_rather_than_either_alone",
    ),
    Mutation(
        # Fix the size instead of computing it from the rectangle, which is what
        # a stamp dragged out large should get. `textbox::SIZE` is the value the
        # obvious implementation would reach for.
        "save: set every stamp at the text box's fixed size",
        "src/save/marks.rs",
        "        let size = (inner_w / unit).min(inner_h / STAMP_CAP).max(1.0);",
        "        let size = textbox::SIZE;",
        "a_stamp_fills_the_rectangle_it_was_dragged_out_at",
    ),
    Mutation(
        # Accept a stamp with no name, and a name on anything else. Both halves
        # are one rule and one `if`, so one mutation covers the pair --- the test
        # asserts each direction separately.
        "docmodel: let a kind and a stamp name disagree",
        "src/docmodel.rs",
        "        if mark.stamp.is_some() != (mark.kind == MarkKind::Stamp) {",
        "        if false {",
        "a_kind_and_a_stamp_name_that_disagree_are_refused_both_ways_round",
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
        "src/commands/app.rs",
        '    env!("CARGO_PKG_VERSION")',
        '    "26.0.0"',
        "the_version_files_agree_with_the_crate",
    ),
    Mutation(
        # Say nothing about a close that failed. The reader is told the save did
        # not land and not that their document is also gone, so the message they
        # act on is missing the half that decides what they do next.
        "save: drop the close's own failure from what the reader is told",
        "src/lib.rs",
        '            "{} --- and the document did not close cleanly: {also}",',
        '            "{}{also:.0}",',
        "a_failed_close_is_added_to_the_failure_the_reader_sees",
    ),
    Mutation(
        # Add the note whether or not the close failed, so every ordinary
        # refusal ends with a sentence about nothing going wrong.
        "save: append a close note to a save whose close was clean",
        "src/lib.rs",
        "    if let Err(also) = closed {",
        '    let also = closed.err().unwrap_or_else(|| "".into());\n    {',
        "a_clean_close_adds_nothing_to_a_failure",
    ),
    Mutation(
        # Rebuild the failure instead of decorating it, which drops `changed`.
        # That field is what lets the window offer Reload for a file that moved
        # under the reader, so this withdraws the one action that would help --
        # and the message, which is all anybody reads, looks perfect.
        "save: rebuild a decorated failure and lose the field Reload reads",
        "src/lib.rs",
        "fn with_close_note(mut why: SaveFailure, closed: Result<(), String>) -> SaveFailure {",
        "fn with_close_note(why: SaveFailure, closed: Result<(), String>) -> SaveFailure {\n"
        "    let mut why = SaveFailure::after_close(why.message);",
        "a_close_note_changes_the_sentence_and_not_the_fields",
    ),
    # The save sequence itself, which nothing could aim at until 2026-08-31: it
    # was the body of a Tauri command, so a mutation of it named no test that
    # could run. `save_order.rs` is that sequence with the round trips behind a
    # seam, and these six are the guards the extraction was for.
    Mutation(
        # Write a document nobody edited. The file that comes out is correct and
        # every object id in it has moved, which for a signed document means the
        # signature no longer covers anything -- in exchange for a save the
        # reader did not ask for.
        "save_order: save a document that has nothing to save",
        "src/save_order.rs",
        "    if !state.dirty {",
        "    if false {",
        "a_clean_document_is_refused_before_anything_is_touched",
    ),
    Mutation(
        # Pick the writer from the plan alone. `Plan::is_appendable` and
        # `mode_for_source` agree on every small file, so this is invisible until
        # the document is one the append cannot be prepared for: past
        # `APPEND_MAX_BYTES` the worker is refused the memory and aborts, and a
        # file that cannot be measured at all takes the arm with no bound over it.
        "save_order: choose the writer from the plan and not from the file",
        "src/save_order.rs",
        "    let mode = save::mode_for_source(&plan, source);",
        "    let mode = if plan.is_appendable() {\n"
        "        save::Mode::Append\n"
        "    } else {\n"
        "        save::Mode::Rewrite\n"
        "    };",
        "a_source_that_cannot_be_measured_takes_the_rewrite",
    ),
    Mutation(
        # Leave the reader's journal open across the write. Document numbers are
        # reused, so a journal still under a handle the service is free to hand
        # to another file is one document's edits applied to another's pages.
        "save_order: leave the model open while the file is written",
        "src/save_order.rs",
        "    saving.close_model();",
        "    let _ = &saving;",
        "the_document_is_closed_after_the_save_is_prepared_and_before_it_lands",
    ),
    Mutation(
        # Land the write without the reader's key. `lopdf` parses no objects at
        # all without one, so the append's read-back counts zero pages against
        # the two it expects and rolls back a save that was correct.
        "save_order: land the write without the reader's key",
        "src/save_order.rs",
        "    let landed = land(prepared, password).await?;",
        "    let landed = land(prepared, None).await?;",
        "the_password_is_asked_once_and_reaches_the_writer_and_the_landing",
    ),
    Mutation(
        # Say nothing about a close that failed. The reader is told the save did
        # not land and not that their document is also gone, which is the half
        # that decides what they do next.
        "save_order: drop the close from the failure the reader is handed",
        "src/save_order.rs",
        "    landed.map_err(|why| with_close_note(why, closed))",
        "    let _ = closed;\n    landed",
        "a_failed_close_is_named_in_a_failure_that_happened_after_it",
    ),
    Mutation(
        # Restate a writer's refusal instead of carrying it, which drops
        # `changed`. The message is perfect and the window withholds Reload --
        # the one action that helps a reader whose file moved underneath them.
        "save_order: restate a writer's refusal and lose the field Reload reads",
        "src/save_order.rs",
        "    .map_err(SaveFailure::refused_by)?;",
        "    .map_err(|why| SaveFailure::refused(why.message))?;",
        "a_save_refused_before_the_close_leaves_the_document_open",
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
        # Let an infinite corner through. `Quad::covers_area` excludes NaN and
        # not infinities, so the model accepts one -- and a region with an
        # infinite corner overlaps every object on the page, which is one drag
        # silently covering the document. Written as "reject only NaN", which is
        # the guard somebody would write believing serde had done the rest.
        "edits: accept a redaction with an infinite corner",
        "src/edits.rs",
        "        if let Some(bad) = area.iter().find(|v| !v.is_finite()) {",
        "        if let Some(bad) = area.iter().find(|v| v.is_nan()) {",
        "a_region_whose_geometry_is_not_finite_is_refused_rather_than_marked",
    ),
    Mutation(
        # Report a redaction's rectangle with two corners swapped. The row still
        # appears in the review list, on the right page, and the overlay draws a
        # rectangle -- somewhere else. Only an assertion on the numbers can see
        # it, which is why the reply test asserts the area rather than the count.
        "edits: transpose a redaction's corners on the way to the frontend",
        "src/edits.rs",
        "                area: [\n"
        "                    redaction.area.left,\n"
        "                    redaction.area.top,",
        "                area: [\n"
        "                    redaction.area.top,\n"
        "                    redaction.area.left,",
        "a_pending_redaction_reaches_the_reply_and_no_plan",
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
        "    turn_pages(&mut doc, &agreed_turns(&turns)?, &written)?;",
        "    turn_pages(&mut doc, &turns, &written)?;",
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
        "    let moved =\n        baselines.len() != plan.pages.len() || baselines.windows(2).any(|two| two[0] >= two[1]);",
        "    let moved = true;",
        "a_plan_in_document_order_leaves_the_page_tree_as_it_found_it",
    ),
    Mutation(
        # The print half of the same defect, and it was live in shipped code as
        # a documented property: a subset came out in document order whatever
        # order it was asked for.
        "print: hand the printer the pages in the file's order rather than the job's",
        "src/print.rs",
        "    let moved = wanted.windows(2).any(|two| two[0].number >= two[1].number);",
        "    let moved = false;",
        "a_job_prints_its_pages_in_the_order_it_lists_them",
    ),
    Mutation(
        # And its over-application, which no page a reader sees can distinguish.
        "print: rebuild the page tree for a job whose pages never moved",
        "src/print.rs",
        "    let moved = wanted.windows(2).any(|two| two[0].number >= two[1].number);",
        "    let moved = true;",
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
        "                let theirs = match source {\n                    PageSource::Baseline(n) => *n as usize == at,\n                    PageSource::Blank(_) => false,\n                    PageSource::Imported { .. } => false,\n                };",
        "                let theirs = {\n                    let _ = (source, at);\n                    true\n                };",
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
        "                    Slot::Kept(*pages.get(number as usize).ok_or_else(|| {",
        "                    Slot::Kept(*pages.get(number as usize + 1).ok_or_else(|| {",
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
        "|| baselines.windows(2).any(|two| two[0] >= two[1]);",
        "|| false;",
        "a_plan_whose_pages_have_moved_comes_out_in_the_order_the_reader_put_them",
    ),
    Mutation(
        # Number the ordinals by every object rather than by text objects. An
        # image between two lines then shifts the numbering, and the redaction
        # removes a line the reader did not mark while reporting success.
        "redact: count every page object when numbering the text ones",
        "src/redact.rs",
        "        let is_text = object.kind == \"text\";\n        let ordinal = text_ordinal;\n        if is_text {\n            text_ordinal += 1;\n        }",
        "        let is_text = object.kind == \"text\";\n        let ordinal = text_ordinal;\n        text_ordinal += 1;",
        "an_image_between_two_lines_does_not_shift_the_text_ordinals",
    ),
    Mutation(
        # Delete front-first. The second removal then lands on whatever moved
        # into the slot -- invisible with two lines, wrong with three.
        "redact: remove show operators front-first instead of back-first",
        "src/redact.rs",
        # Widened when `remove_images` arrived carrying the same three-line loop.
        # Third sibling in one session, and `docs/TRAPS.md` records the shape:
        # a near-copy makes an existing anchor ambiguous without the anchor
        # moving, and an ambiguous anchor is refused. The operand is what tells
        # the two apart -- one removes from the page's content, the other from a
        # decoded copy of it that is written back whole.
        "    for at in positions.into_iter().rev() {\n        content.operations.remove(at);\n    }\n\n    // **After the content stream",
        "    for at in positions.into_iter() {\n        content.operations.remove(at);\n    }\n\n    // **After the content stream",
        "removing_two_operators_removes_the_two_that_were_named",
    ),
    Mutation(
        # Proceed when PDFium's object count and the operator count disagree,
        # which is a redaction that removes the wrong words and says it worked.
        "redact: remove by position even when the two counts disagree",
        "src/redact.rs",
        # Widened when `remove_form_shows` arrived carrying the same guard one
        # level down. `docs/TRAPS.md` records the shape: a near-copy of a
        # function makes an existing anchor ambiguous without the anchor moving,
        # and an ambiguous anchor is refused, so the mutation stops being able to
        # fail. The page's message is what distinguishes them.
        "    if shows.len() != text_objects {\n        return Err(format!(\n            \"the page has",
        "    if false {\n        return Err(format!(\n            \"the page has",
        "a_count_that_disagrees_with_pdfium_refuses_and_removes_nothing",
    ),
    Mutation(
        # Treat a shared edge as an overlap, so a region drawn flush against a
        # line silently eats it.
        # Re-aimed 2026-09-06 with the page-object vocabulary: `overlaps` and
        # `normalised` moved to `objects.rs`, beside the code that produces the
        # boxes they compare. The test stays in `redact.rs`, which still reaches
        # both through the re-export, so only the file this names moved.
        "objects: treat two rectangles that only touch as overlapping",
        "src/objects.rs",
        "    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]",
        "    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]",
        "a_region_flush_against_a_line_does_not_eat_it",
    ),
    Mutation(
        # Say nothing about an object that cannot be removed. PLAN.md section 6's
        # deny-by-default rule inverted: the words go and the picture of the
        # words stays, and the plan calls itself complete.
        # Re-aimed 2026-08-26, when the finding became a kind and a position
        # rather than a sentence, and widened 2026-08-27, when a form's children
        # gained a push of their own and the shorter anchor matched both -- the
        # `} else {` is what makes this the page level's.
        "redact: pass over an image in the region without reporting it",
        "src/redact.rs",
        "        } else {\n            plan.unhandled.push(Unhandled {",
        "        } else {\n            drop(Unhandled {",
        "an_object_this_cannot_remove_makes_the_plan_incomplete",
    ),
    Mutation(
        # Forget the two quote forms, so a redaction passes over any line drawn
        # with one.
        "redact: stop recognising the quote text-showing operators",
        "src/redact.rs",
        "    matches!(operator, \"Tj\" | \"TJ\" | \"'\" | \"\\\"\")",
        "    matches!(operator, \"Tj\" | \"TJ\")",
        "the_two_quote_operators_are_show_operators",
    ),
    Mutation(
        # Leave the shadow text where it is. The glyphs go and `/ActualText`
        # keeps a verbatim copy of the words beside the hole they left --- which
        # is the redaction that reports success and removes nothing a reader
        # cares about.
        "redact: leave the shadow text on a span the removal emptied",
        "src/redact.rs",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &[])?;",
        "a_span_the_removal_touched_loses_its_copy_of_the_words",
    ),
    Mutation(
        # Clear the carriers after the deletion has renumbered the operations.
        # One span cannot tell the two orders apart; with two, the second span's
        # `EMC` has moved by the time the walk reaches it and its copy survives.
        "redact: clear the carriers after the removal has renumbered the operations",
        "src/redact.rs",
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;\n"
        "\n"
        "    for at in positions.into_iter().rev() {\n"
        "        content.operations.remove(at);\n"
        "    }",
        "    for at in positions.clone().into_iter().rev() {\n"
        "        content.operations.remove(at);\n"
        "    }\n"
        "\n"
        "    let carriers = clear_shadow_text(doc, page, &mut content.operations, &positions)?;",
        "two_spans_each_holding_a_removed_line_both_lose_their_shadow_text",
    ),
    Mutation(
        # Mark only the innermost open span. An outer `/ActualText` restates
        # everything inside it, so the removed line stays in the file one level
        # up.
        "redact: mark only the innermost span rather than every open one",
        "src/redact.rs",
        "            for frame in &mut open {\n                frame.1 = true;\n            }",
        "            if let Some(frame) = open.last_mut() {\n                frame.1 = true;\n            }",
        "an_enclosing_span_loses_its_shadow_text_as_well",
    ),
    Mutation(
        # Drop a span the stream never closed. Malformed input, and the carrier
        # then survives in exactly the file least likely to be looked at twice.
        "redact: drop a marked-content span the stream never closed",
        "src/redact.rs",
        "    for (start, inside) in open {",
        "    for (start, inside) in open.into_iter().take(0) {",
        "a_span_that_was_never_closed_still_loses_its_shadow_text",
    ),
    Mutation(
        # Never follow an /MCID into the structure tree. The words go from the
        # page and from its property list, and the element that owns that same
        # marked content keeps a verbatim copy nothing on the page mentions.
        "redact: leave the structure tree alone after a removal",
        "src/redact.rs",
        "    let struct_carriers = clear_struct_shadow_text(doc, page, &carriers.mcids);",
        "    let struct_carriers = clear_struct_shadow_text(doc, page, &[]);",
        "the_structure_element_a_removed_span_belongs_to_loses_its_shadow_text",
    ),
    Mutation(
        # Clear the element and stop. An ancestor restates everything beneath it,
        # so the removed line survives one level up.
        "redact: clear the element a span owns but not the ones above it",
        "src/redact.rs",
        "        for _ in 0..MAX_ANCESTORS {",
        "        for _ in 0..1 {",
        "an_ancestor_of_that_element_loses_its_shadow_text_as_well",
    ),
    Mutation(
        # Take every element the page's entry lists rather than the ones the
        # removal reached, which is the alternate text of every line on the page.
        "redact: strip the whole parent-tree entry rather than the spans reached",
        "src/redact.rs",
        "    for mcid in mcids {",
        "    for mcid in &(0..entries.len() as i64).collect::<Vec<i64>>() {",
        "the_element_for_a_line_nobody_redacted_keeps_its_shadow_text",
    ),
    Mutation(
        # Read the parent tree as one flat /Nums. Every document large enough for
        # a producer to balance the tree then reports no structure carrier at
        # all -- silently, because a miss and an untagged page look the same.
        "redact: read a parent tree's /Nums and never its /Kids",
        "src/redact.rs",
        "    let kids = node\n        .get(b\"Kids\")",
        "    let kids = node\n        .get(b\"NoKids\")",
        "a_parent_tree_written_as_kids_is_followed",
    ),
    Mutation(
        # Step the wave at the period the quad's aspect ratio asks for. The
        # trip count is width/half with both from a plan the writer did not
        # build, and the fuzzer's was two hundred million -- 6.2 GB of `String`
        # from a 2,937-byte input, in one pass. Fails fast rather than by
        # exhausting memory: the test's fixture asks for 200,049 segments,
        # which is large enough to cross the cap and small enough to finish.
        "marks: step a squiggle at whatever period the quad asks for",
        "src/save/marks.rs",
        "        if seen.width / half > MAX_WAVE_SEGMENTS {",
        "        if false {",
        "a_squiggle_across_an_extreme_quad_emits_a_bounded_number_of_segments",
    ),
    Mutation(
        # Accept a made page of any size that is a number. `make_blank_pages`
        # casts to `f32` for the /MediaBox, so anything past `f32::MAX` is
        # written as `inf` and the file is malformed with nothing saying so.
        "save: accept a made page wider than PDF permits",
        "src/save.rs",
        "                        || size.width > MAX_PAGE_POINTS",
        "                        || false",
        "a_made_page_of_a_size_no_reader_could_have_asked_for_is_refused",
    ),
    Mutation(
        # Trust the app-process guard. `edits.rs` refuses a non-finite page
        # size where the insert command receives it -- in the app process,
        # while this runs in the worker against a plan that may have crossed
        # the boundary or come out of a restored session.
        "save: trust that a made page's size is a finite number",
        "src/save.rs",
        "                    if !size.width.is_finite()",
        "                    if false",
        "a_made_page_of_a_size_no_reader_could_have_asked_for_is_refused",
    ),
    Mutation(
        # Leave the document's own description of itself. The words go from the
        # page and /Info /Title still says what the document is about.
        "save: leave /Info and the XMP packet on a redaction",
        "src/save.rs",
        "        done.metadata = strip_metadata(doc)?;",
        "        done.metadata = 0;",
        "a_redaction_removes_the_documents_own_description_of_itself",
    ),
    Mutation(
        # Strip on every rewrite rather than on a redaction. Every copy,
        # extract, split and merge silently loses its title and author, and the
        # two checks above stay green -- this is the SCOPE, and without a
        # control for it the condition is decoration.
        #
        # Since 2026-08-27 this guard covers the OUTLINE too, so the mutation
        # reddens `a_copy_that_is_not_a_redaction_keeps_its_outline` as well.
        # It names one of the two because a mutation names one test, and the
        # other says so in its own doc comment. A second entry with the same
        # anchor and an equivalent replacement would be padding: there is one
        # condition here, not two.
        "save: strip metadata on every save rather than on a redaction",
        "src/save.rs",
        "    if done.shows > 0 || done.annots > 0 || !redactions.is_empty() {",
        "    if true {",
        "a_copy_that_is_not_a_redaction_keeps_its_metadata",
    ),
    Mutation(
        # Take /Info and leave the XMP packet, which holds the same title in the
        # form PDF 2.0 prefers.
        "save: strip /Info and leave the XMP packet beside it",
        "src/save.rs",
        "        doomed.insert(metadata);",
        "        let _ = metadata;",
        "a_redaction_removes_the_documents_own_description_of_itself",
    ),
    Mutation(
        # Take the XMP packet and leave /Info, which is the half every reader
        # still shows in its properties panel.
        "save: strip the XMP packet and leave /Info beside it",
        "src/save.rs",
        "        doomed.insert(info);",
        "        let _ = info;",
        "a_redaction_removes_the_documents_own_description_of_itself",
    ),
    Mutation(
        # Take no annotation at all. The words go off the page and the comment
        # about them, which quotes them, stays -- displayed by every reader.
        "redact: leave every annotation where it is",
        "src/redact.rs",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) || true => {}",
        "an_annotation_over_the_region_is_taken",
    ),
    Mutation(
        # Take every annotation on the page. Passes the check above perfectly and
        # destroys every other comment the reader has.
        "redact: take every annotation on a page that has a region",
        "src/redact.rs",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) && false => {}",
        "an_annotation_away_from_the_region_is_left",
    ),
    Mutation(
        # Treat a shared edge as an overlap here rather than through `overlaps`,
        # which is the rule a second inline comparison would get wrong.
        "redact: test an annotation against the region without the strict rule",
        "src/redact.rs",
        "            Some(rect) if !areas.iter().any(|area| overlaps(rect, *area)) => {}",
        "            Some(rect)\n                if !areas.iter().any(|area| {\n                    !(rect[2] < area[0] || area[2] < rect[0] || rect[3] < area[1] || area[3] < rect[1])\n                }) => {}",
        "an_annotation_flush_against_the_region_is_left",
    ),
    Mutation(
        # Default an unreadable rectangle to somewhere far away instead of taking
        # the annotation. The plausible mistake, and it keeps what it cannot see.
        "redact: keep an annotation whose rectangle cannot be read",
        "src/redact.rs",
        "        match rect_of(doc, annot) {",
        "        match rect_of(doc, annot).or(Some([f32::MAX, f32::MAX, f32::MAX, f32::MAX])) {",
        "an_annotation_with_no_readable_rectangle_is_taken",
    ),
    Mutation(
        # Leave the popup. It is a separate object with its own `/Contents` in
        # most producers' output, so the note goes and the words stay.
        "redact: leave the popup of an annotation that is taken",
        "src/redact.rs",
        "                if let Ok(popup) = annot.get(b\"Popup\").and_then(Object::as_reference) {",
        "                if let Ok(popup) = annot.get(b\"Popup_\").and_then(Object::as_reference) {",
        "a_popup_goes_with_the_annotation_that_owns_it",
    ),
    Mutation(
        # One pass instead of a fixed point. A reply to a reply survives, and a
        # reply is a copy of the conversation about the words that went.
        "redact: collect replies in one pass rather than to a fixed point",
        "src/redact.rs",
        "        if !grew {\n            break;\n        }",
        "        let _ = grew;\n        break;",
        "a_chain_of_replies_goes_with_the_note_it_answers",
    ),
    Mutation(
        # Read `/Annots` only when it is written inline. `/Annots 12 0 R` is as
        # common, and a page written that way keeps every annotation on it.
        "redact: read /Annots only in its inline spelling",
        "src/redact.rs",
        "        .get(b\"Annots\")\n        .and_then(|object| doc.dereference(object).map(|(_, object)| object))\n        .and_then(Object::as_array)",
        "        .get(b\"Annots\")\n        .and_then(Object::as_array)",
        "an_annots_array_that_is_its_own_object_is_read",
    ),
    Mutation(
        # Remove the objects and leave every reference to them. The annotation is
        # gone and the AcroForm still names it, which is a dangling reference
        # where there was a comment -- and on a document where the second name is
        # a structure element, a reader that walks the tree still finds it.
        "save: remove a redacted annotation without dropping the references to it",
        "src/save.rs",
        "            crate::pagetree::forget(doc, &taken).map_err(Refusal::from)?;",
        "            for id in \u0026taken {\n                doc.objects.remove(id);\n            }",
        "a_redacted_annotation_loses_the_references_that_are_not_on_the_page",
    ),
    Mutation(
        # Prune the page's `/Annots` and leave the object. It is then reachable
        # from a structure element's `/OBJR` or an AcroForm's `/Fields`, written
        # out, and still carrying the comment about the words.
        "save: unlink a redacted annotation without removing the object",
        "src/save.rs",
        "            crate::pagetree::forget(doc, &taken).map_err(Refusal::from)?;",
        "            let _ = &taken;",
        "an_annotation_over_a_redacted_region_is_removed_and_its_neighbour_is_not",
    ),
    Mutation(
        # Count the key and leave it where it is. `Removed::carriers` then reports
        # the work as done while every word is still in the stream -- which is the
        # difference between an accounting observable and the thing it accounts
        # for, and the probe's own two assertions separate the same pair.
        "redact: count a shadow-text key without removing it",
        "src/redact.rs",
        "                    if properties.remove(key).is_some() {\n                        cleared.keys += 1;\n                    }",
        "                    if properties.has(key) {\n                        cleared.keys += 1;\n                    }",
        "a_span_the_removal_touched_loses_its_copy_of_the_words",
    ),
    Mutation(
        # Clear the famous key and leave its two siblings, which hold the same
        # words in the same dictionary for the same reason.
        "redact: clear only /ActualText and leave /Alt and /E",
        "src/redact.rs",
        "const SHADOW_TEXT: [&[u8]; 3] = [b\"ActualText\", b\"Alt\", b\"E\"];",
        "const SHADOW_TEXT: [&[u8]; 1] = [b\"ActualText\"];",
        "every_shadow_text_key_goes_not_just_the_famous_one",
    ),
    Mutation(
        # Slip the comparison so no subtree is ever skipped. `text-marked.pdf`'s
        # first kid says `/Limits [5 9]` while its `/Nums` claims key 0, so a
        # walk that stops honouring the limits finds the decoy's answer first --
        # a different, wrong element rather than none, which is what gives this
        # a failing case at all.
        "redact: search every kid of a number tree, ignoring /Limits",
        "src/redact.rs",
        "                if key < low || key > high {",
        "                if key < low && key > high {",
        "a_balanced_parent_tree_is_walked_through_its_kids",
    ),
    Mutation(
        # Read the flat `/Nums` and never descend. The function's own doc comment
        # says this "would find nothing on every document large enough to matter
        # -- silently, since a miss and an untagged page are the same answer",
        # and until the fixture carried a balanced tree nothing could say so.
        "redact: never descend a number tree's /Kids",
        "src/redact.rs",
        "    for kid in kids {",
        "    for kid in Vec::<Object>::new() {",
        "a_balanced_parent_tree_is_walked_through_its_kids",
    ),
    Mutation(
        # Never refuse a shared property list, so a file is written with the
        # words still in it -- or, worse, a later edit strips a dictionary other
        # pages share.
        "redact: pass over a shared property list that carries the words",
        "src/redact.rs",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|key| shared.has(key)) {",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|_key| false) {",
        "a_shared_property_list_carrying_the_words_refuses_and_removes_nothing",
    ),
    Mutation(
        # THE mutation this increment exists for: drop the outline entry
        # without splicing the chain it was in. `pagetree::forget` then takes
        # /Next off the entry before it, so a reader walks /First, /Next and
        # stops one entry early -- every later sibling unreachable, the file
        # valid, no parser complaining.
        "redact: remove an outline entry without joining its neighbours",
        "src/redact.rs",
        "        if let Some(at) = prev {\n            set_or_clear(doc, at, b\"Next\", next);\n        }",
        "        if let Some(at) = prev {\n            let _ = at;\n        }",
        "an_outline_removal_leaves_the_entries_around_it_reachable",
    ),
    Mutation(
        # The other half of the splice. A /Prev naming a deleted object is what
        # a reader walking BACKWARDS from /Last meets, and PDFium's own
        # FPDFBookmark_GetFirstChild/GetNextSibling never look at it -- so this
        # is the half no forward walk can see.
        "redact: leave an outline entry's successor pointing back at what went",
        "src/redact.rs",
        "        if let Some(at) = next {\n            set_or_clear(doc, at, b\"Prev\", prev);\n        }",
        "        if let Some(at) = next {\n            let _ = at;\n        }",
        "an_outline_removal_leaves_the_entries_around_it_reachable",
    ),
    Mutation(
        # Leave /Count saying what the outline used to hold. The /Size shape
        # from spike 0.4 one subsystem along: renders identically, structurally
        # wrong.
        "redact: leave the outline counting entries that are gone",
        "src/redact.rs",
        "            recount(doc, root, 0);",
        "            let _ = root;",
        "a_removal_leaves_the_outline_counting_what_is_left",
    ),
    Mutation(
        # Take every entry rather than the ones naming what went -- which is
        # what a page deletion correctly does and what this must not.
        "redact: take the whole outline rather than the entries that name it",
        "src/redact.rs",
        "        if folded.iter().any(|line| line.contains(&title)) {",
        "        if folded.iter().any(|line| !line.is_empty() || line.contains(&title)) {",
        "an_outline_entry_naming_something_else_survives_a_redaction",
    ),
    Mutation(
        # Match a title against nothing, so the carrier survives. The direction
        # that leaves the words on screen in tpdf's own sidebar.
        "redact: leave every outline entry whatever it names",
        "src/redact.rs",
        "        if folded.iter().any(|line| line.contains(&title)) {",
        "        if folded.iter().any(|line| line.contains(&title) && line.is_empty()) {",
        "a_redaction_takes_the_outline_entry_naming_what_it_removed",
    ),
    Mutation(
        # Take the matched entry and leave what hangs under it. A section's
        # subsections belong to the section, and a child's own title matches
        # nothing -- so only the subtree walk can reach it.
        "redact: take a matched outline entry without its subtree",
        "src/redact.rs",
        "        collect_subtree(doc, id, &mut all);",
        "        if !all.contains(&id) {\n            all.push(id);\n        }",
        "a_redaction_takes_the_outline_entry_naming_what_it_removed",
    ),
    Mutation(
        # Act on a one-character title. A bookmark called `1` is a substring of
        # almost any line, so this takes the outline off a document for the sake
        # of a chapter number.
        "redact: match an outline title too short to be distinctive",
        "src/redact.rs",
        "        if title.len() < MIN_OUTLINE_TITLE {",
        "        if title.is_empty() {",
        "a_very_short_outline_title_is_not_matched",
    ),
    Mutation(
        # Redact an XFA form rather than refusing it. Section 6 has said to
        # refuse since before any of this was written and nothing read it until
        # 2026-08-27: an XFA packet is a complete second copy of every answer,
        # so a redaction that takes the field values and leaves it has removed
        # nothing a reader could not recover.
        "redact: redact an XFA field form rather than refusing it",
        "src/save.rs",
        "    if !redactions.is_empty() && crate::redact::has_xfa(doc) {",
        "    if !redactions.is_empty() && false {",
        "a_redaction_of_an_xfa_form_is_refused_rather_than_half_done",
    ),
    Mutation(
        # Refuse a plain copy of an XFA form too. A serialisation makes no claim
        # about what it removed, so there is nothing for XFA to falsify -- and
        # refusing would make tpdf unable to open-and-save a whole class of
        # document for a promise it is not making.
        "redact: refuse a field form's copy as well as its redaction",
        "src/save.rs",
        "    if !redactions.is_empty() && crate::redact::has_xfa(doc) {",
        "    if crate::redact::has_xfa(doc) {",
        "a_copy_of_an_xfa_form_is_not_refused",
    ),
    Mutation(
        # Leave the field dictionary whose widgets all went. The gap measured
        # before this was built: the kid goes as an annotation and the parent
        # keeps /V, so nothing draws the value and every search finds it.
        "redact: keep a field whose widgets have all been removed",
        "src/redact.rs",
        "        let orphaned =\n            field.has(b\"Kids\")",
        "        let orphaned = false\n            \u0026\u0026 field.has(b\"Kids\")",
        "a_field_whose_widgets_all_went_does_not_keep_its_value",
    ),
    Mutation(
        # Treat a merged field -- no /Kids at all -- as orphaned. Every field
        # that is its own widget then goes on the first redaction, which is the
        # over-removal direction and empties the form.
        "redact: call a field with no kids at all an orphaned one",
        "src/redact.rs",
        "        let orphaned =\n            field.has(b\"Kids\")",
        "        let orphaned =\n            !field.has(b\"Kids\")",
        "a_field_naming_nothing_that_went_survives_a_redaction",
    ),
    Mutation(
        # Match a field value against nothing, so a field holding what went
        # survives wherever its widget sits -- which is section 6's "widgets
        # outside the redacted rectangle" left open.
        "redact: leave a field holding what went when its widget is elsewhere",
        "src/redact.rs",
        "            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.contains(&text))",
        "            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.is_empty())",
        "a_field_holding_what_went_goes_even_with_its_widget_elsewhere",
    ),
    Mutation(
        # Read only /V and not /DV. A default value is the same string in the
        # same dictionary, and a redaction that took the answer and left the
        # default it was pre-filled from has removed nothing.
        "redact: read a field's value and not the default beside it",
        "src/redact.rs",
        "        let carries = [b\"V\".as_slice(), b\"DV\".as_slice()].into_iter().any(|key| {",
        "        let carries = [b\"V\".as_slice()].into_iter().any(|key| {",
        "a_field_whose_default_holds_what_went_is_taken_too",
    ),
    Mutation(
        # Act on a field value too short to be distinctive. A form is full of
        # short answers, and a field holding `Yes` is a substring of almost any
        # line.
        "redact: match a field value too short to be distinctive",
        "src/redact.rs",
        "            text.len() >= MIN_FIELD_VALUE && folded.iter().any(|line| line.contains(&text))",
        "            folded.iter().any(|line| line.contains(&text))",
        "a_field_value_too_short_to_be_distinctive_is_not_matched",
    ),
    Mutation(
        # Take a matched field and leave its widgets. They are annotations on a
        # page, so the drawing stays where the reader can see it while the value
        # behind it has gone.
        "redact: take a matched field without the widgets under it",
        # Re-aimed 2026-09-06: `collect_field_subtree` is `fields::descend`,
        # which is the same loop `covered_fields` itself runs. The edit is the
        # same one -- take the field and leave everything under it.
        "src/redact.rs",
        """        crate::fields::descend(doc, &[id], &bounds, |node| {
            let Some(at) = node.id else {
                return crate::fields::Flow::Leaf;
            };
            if all.contains(&at) {
                return crate::fields::Flow::Leaf;
            }
            all.push(at);
            crate::fields::Flow::Descend
        });""",
        "        if !all.contains(&id) {\n            all.push(id);\n        }",
        "a_matched_field_takes_the_widgets_under_it",
    ),
    Mutation(
        # Keep an /AcroForm whose fields have all gone. It then reads as a
        # document that never had a form, while /DA, /DR and /NeedAppearances go
        # on describing fields that are not there.
        "redact: keep a form dictionary with no fields left in it",
        "src/redact.rs",
        "    if empty {\n        if let Ok(catalog) = doc.catalog_mut() {",
        "    if false {\n        if let Ok(catalog) = doc.catalog_mut() {",
        "a_form_with_nothing_left_in_it_goes_as_well",
    ),
    Mutation(
        # Refuse every named property list rather than only the ones carrying a
        # key. `/OC /MC0 BDC` is optional content on nearly every layered
        # drawing and carries no text, so this makes those pages unredactable.
        "redact: refuse every named property list, carrier or not",
        "src/redact.rs",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|key| shared.has(key)) {",
        "                    if let Some(key) = SHADOW_TEXT.into_iter().find(|_key| true) {",
        "a_shared_property_list_with_no_shadow_text_does_not_refuse",
    ),
    Mutation(
        # Trust the corners' order. A region arriving with either pair the other
        # way round then overlaps nothing at all, and the redaction quietly
        # removes nothing.
        # Re-aimed 2026-09-06 with the move to `objects.rs`.
        "objects: assume a region's corners arrive in the order PDFium uses",
        "src/objects.rs",
        "        rect[0].min(rect[2]),\n        rect[1].min(rect[3]),",
        "        rect[0],\n        rect[1],",
        "a_region_with_its_corners_the_other_way_round_still_overlaps",
    ),
    Mutation(
        # Normalise the region and not the object. The mirror, and the reason
        # there is one function rather than two copies: the first version of
        # this mutation survived because a test reversing both axes let the
        # other copy rescue it.
        # Re-aimed 2026-09-06 with the move to `objects.rs`.
        "objects: normalise the region a reader drew and not the object it covers",
        "src/objects.rs",
        "    let a = normalised(a);\n    let b = normalised(b);",
        "    let b = normalised(b);",
        "an_object_with_its_corners_the_other_way_round_is_still_found",
    ),
    Mutation(
        # Act on a repeated ordinal twice, so the second pass deletes whatever
        # moved into the slot.
        "redact: act on a repeated ordinal twice",
        "src/redact.rs",
        # Widened for the reason above: `remove_form_shows` deduplicates the same
        # way, and the line after it is what tells the two apart.
        "    positions.dedup();\n    let removed = positions.len();\n\n    // **Before the removal",
        "    positions.reverse();\n    positions.sort_unstable();\n    let removed = positions.len();\n\n    // **Before the removal",
        "the_same_ordinal_twice_removes_one_operator",
    ),
    Mutation(
        # Leave max_id where the graph left it, which is spike 0.4's defect:
        # /Size then claims objects the file does not contain. Nothing in this
        # process reads that -- only qpdf does -- so the assertion has to be on
        # the number rather than on any reader's verdict.
        "save: serialise a document whose /Size claims objects it does not hold",
        "src/save.rs",
        "    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);",
        "    doc.max_id = doc.max_id.max(doc.objects.keys().map(|id| id.0).max().unwrap_or(0));",
        "a_serialised_document_reports_the_size_its_objects_justify",
    ),
    Mutation(
        # Lower it one too far. The over-correction direction: /Size then fails
        # to cover the highest object written, which is the same defect
        # mirrored and equally invisible to every reader here.
        "save: declare a /Size that does not cover the highest object written",
        "src/save.rs",
        "    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0);",
        "    doc.max_id = doc.objects.keys().map(|id| id.0).max().unwrap_or(0).saturating_sub(1);",
        "no_object_is_written_at_or_past_the_size_that_was_declared",
    ),
    Mutation(
        # Accept a file that does not begin with a PDF header.
        "verify: let a file with no PDF header pass the structural check",
        "src/verify.rs",
        "    if !bytes.starts_with(b\"%PDF-\") {",
        "    if false {",
        "a_file_that_does_not_begin_with_a_pdf_header_is_refused",
    ),
    Mutation(
        # Accept a file with no %%EOF at all.
        "verify: let a file with no %%EOF pass the structural check",
        "src/verify.rs",
        "        0 => wrong.push(\"the file has no %%EOF marker\".to_string()),",
        "        0 => {}",
        "a_file_with_no_eof_marker_is_refused",
    ),
    Mutation(
        # Accept several revisions, which PLAN.md section 6 forbids for a
        # rewrite: an object an earlier revision held sits at its old offset,
        # addressable by nothing and invisible to a graph walk.
        "verify: let a rewrite hold more than one revision",
        "src/verify.rs",
        "    let eofs = count(bytes, b\"%%EOF\");",
        "    let eofs = count(bytes, b\"%%EOF\").min(1);",
        "a_second_revision_is_refused_and_the_count_is_reported",
    ),
    Mutation(
        # Ignore bytes past the last %%EOF -- content belonging to no object,
        # which is the other half of section 6's rule.
        "verify: ignore bytes trailing the last %%EOF",
        "src/verify.rs",
        "        if trailing > 0 {",
        "        if false {",
        "bytes_after_the_last_eof_are_refused_and_whitespace_is_not",
    ),
    Mutation(
        # Treat whitespace after %%EOF as trailing content. The over-refusal
        # direction, and the one that would fire on every file every writer
        # produces -- which is how the first draft of a /Size rule was caught.
        "verify: count the newline every writer ends a file with as trailing content",
        "src/verify.rs",
        "        let trailing = bytes[last + 5..]\n            .iter()\n            .filter(|byte| !byte.is_ascii_whitespace())",
        "        let trailing = bytes[last + 5..]\n            .iter()\n            .filter(|_byte| true)",
        "bytes_after_the_last_eof_are_refused_and_whitespace_is_not",
    ),
    Mutation(
        # Accept a startxref pointing past the end of the file.
        "verify: accept a startxref offset past the end of the file",
        "src/verify.rs",
        "            Some(offset) if offset >= bytes.len() => wrong.push(format!(",
        "            Some(offset) if false && offset >= bytes.len() => wrong.push(format!(",
        "a_startxref_pointing_past_the_end_of_the_file_is_refused",
    ),
    Mutation(
        # Accept a startxref with no number after it.
        "verify: accept a startxref with no offset after it",
        "src/verify.rs",
        "            None => wrong.push(\"the file's startxref has no offset after it\".to_string()),",
        "            None => {}",
        "a_startxref_with_no_number_after_it_is_refused",
    ),
    Mutation(
        # Accept a file with no startxref at all.
        "verify: accept a file with no startxref",
        "src/verify.rs",
        "        None => wrong.push(\"the file has no startxref\".to_string()),",
        "        None => {}",
        "a_file_with_no_startxref_is_refused",
    ),
    Mutation(
        # Complain about everything. The control's own control: without this the
        # corpus sweep passes for a check that cannot fail, which is the shape
        # the first /Size rule had in reverse.
        "verify: report every file as malformed",
        "src/verify.rs",
        "pub fn structure(bytes: &[u8]) -> Vec<String> {\n    let mut wrong = Vec::new();",
        "pub fn structure(bytes: &[u8]) -> Vec<String> {\n    let mut wrong = vec![String::from(\"planted\")];",
        "every_rewritten_fixture_is_structurally_sound",
    ),
    Mutation(
        # Never collect, which is what the save path did until 2026-08-26.
        # Extracting one page of eight then produces a one-page file carrying all
        # eight content streams --- `docs/THREAT-MODEL.md` residual risk 16, which
        # named the deletion and missed the extract.
        "save: leave a dropped page's content in the file",
        "src/save.rs",
        "    if !dropped.is_empty()\n        || moved\n        || redacted.annots > 0",
        "    if false\n        \u0026\u0026 moved\n        || redacted.annots > 0",
        "extracting_a_page_leaves_the_other_pages_out_of_the_file",
    ),
    Mutation(
        # Collect on every save, including one that dropped nothing. The two
        # leak checks stay green -- this is the *scope* that has to be able to
        # fail, and without a control for it the condition above is decoration.
        "save: sanitize a plain copy as well as a save that removed something",
        "src/save.rs",
        "    if !dropped.is_empty()\n        || moved\n        || redacted.annots > 0",
        "    if dropped.is_empty()\n        || !moved\n        || true\n        || redacted.annots > 0",
        "a_copy_that_drops_nothing_keeps_the_orphans_it_was_given",
    ),
    Mutation(
        # **Re-aimed on 2026-08-28, when the sequence moved to
        # `pagetree::materialise`.** The rule it used to mutate --- drop the
        # outline when pages go --- now lives in one place and has its own
        # mutation there, aimed at `pagetree`'s own test. Two mutations on one
        # line would make the anchor ambiguous, and an ambiguous anchor is
        # refused, so this one is aimed at what is still *this* caller's: whether
        # `save` tells the shared function anything was dropped at all. The rule
        # is covered once; each caller's wiring to it is covered where it is.
        "save: tell the page-tree writer that nothing was dropped",
        "src/save.rs",
        "    crate::pagetree::materialise(&mut doc, &dropped, moved.then_some(order.as_slice()))?;",
        "    crate::pagetree::materialise(&mut doc, &[], moved.then_some(order.as_slice()))?;",
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
        "                theirs && turns % 4 == 0 && crop.is_none()",
        "                (theirs || turns % 4 == 0) && crop.is_none()",
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
        # Let a print job rewrite an encrypted document. This is the regression
        # the encryption increment introduced and did not notice: `checked` used
        # to refuse every decrypted document, `print::route`'s `Working` arm
        # calls `print_bytes` directly and never reaches `print::build`'s own
        # guard, so removing that refusal removed the only one on this route.
        # The whole suite stayed green.
        # It moved into `rewrite_update` on 2026-09-01 with the parse it guards.
        "save: let a print job rewrite an encrypted document",
        "src/save.rs",
        "    if job.is_print() && checked.encryption.is_some() {",
        "    if false {",
        "a_print_job_from_an_encrypted_document_is_refused_whatever_the_rewrite_can_do",
    ),
    Mutation(
        # Refuse an encrypted *save* as well, which is the other operand of the
        # conjunction above and the half a single mutation cannot reach. A save
        # of an encrypted document keeps its encryption and must not be refused;
        # the print refusal reads as if it were about encryption alone.
        "save: refuse an encrypted save the way a print job is refused",
        "src/save.rs",
        "    if job.is_print() && checked.encryption.is_some() {",
        "    if checked.encryption.is_some() {",
        "a_rewrite_of_an_encrypted_document_stays_encrypted",
    ),
    Mutation(
        # Write a document `lopdf` decrypted on the way in back in the clear.
        # **Re-aimed rather than deleted on 2026-08-28**: the guard this used to
        # mutate -- a refusal keyed on `was_encrypted` -- is gone, because a
        # rewrite now re-encrypts instead of refusing. The class of defect did
        # not go with it. Dropping a document's encryption on save is still one
        # line away, and this is that line.
        #
        # Aimed at the fixture test rather than the synthetic one on purpose:
        # the synthetic document's encryption is fake, authentication fails on
        # it, and it therefore takes the `is_encrypted` arm. Two arms, two
        # mutations, because one fixture cannot reach both.
        "save: write a decrypted document back in the clear",
        "src/save.rs",
        "    if let Some(state) = &encryption {\n        doc.encrypt(state)",
        "    if let Some(state) = &encryption.filter(|_| false) {\n        doc.encrypt(state)",
        "a_really_encrypted_document_keeps_its_encryption_or_names_its_lock",
    ),
    Mutation(
        # Never take the state off the document, so `rewrite` has nothing to put
        # back. The mirror of the mutation above, at the other end of the value's
        # journey: one deletes the write, this one deletes the read, and a fix
        # for either alone leaves the other's defect intact.
        "save: keep no record of the encryption a password opened",
        "src/save.rs",
        "    let encryption = doc.encryption_state.take();",
        "    let encryption = None;\n    let _ = doc.encryption_state.take();",
        "a_rewrite_of_an_encrypted_document_stays_encrypted",
    ),
    Mutation(
        # Load without the reader's password. Every object then fails to parse,
        # and the document that comes back is EMPTY rather than refused -- which
        # is the failure shape that made the first two runs of this increment's
        # own spike report three passes over a document with nothing in it.
        "save: ignore the password the reader supplied",
        "src/save.rs",
        "            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    .map_err(|e| format!(\"could not parse the document: {e}\"))?;",
        "            password: None,\n            ..Default::default()\n        },\n    )\n    .map_err(|e| format!(\"could not parse the document: {e}\"))?;",
        "a_rewrite_of_an_encrypted_document_stays_encrypted",
    ),
    Mutation(
        # The other arm: a document nothing could unlock. `lopdf` parses no
        # objects for it, so a rewrite would write out an empty document.
        "save: rewrite a document nothing unlocked",
        "src/save.rs",
        "    if doc.is_encrypted() {\n        return Err(\n            \"This document is encrypted and tpdf could not unlock it, so it cannot be \\\n             rewritten. Open it with its password first.\"",
        "    if false {\n        return Err(\n            \"This document is encrypted and tpdf could not unlock it, so it cannot be \\\n             rewritten. Open it with its password first.\"",
        "an_encrypted_document_is_refused_rather_than_quietly_decrypted",
    ),
    Mutation(
        # Append to a document nothing unlocked. `lopdf` refuses this itself in
        # `check_incremental_save_supported`, so what this proves is that the
        # refusal a reader sees is ours and names a password rather than an
        # upstream issue number.
        "save: append to a document nothing unlocked",
        "src/save.rs",
        "    if prev.is_encrypted() {",
        "    if false {",
        "an_append_to_a_document_nobody_unlocked_is_refused",
    ),
    Mutation(
        # Build the update section without the reader's key. `lopdf` then parses
        # no objects, and the page walk below sees an empty document -- so the
        # refusal is about the baseline rather than about the password.
        "save: build an append without the password",
        "src/save.rs",
        "            max_decompressed_size: Some(MAX_DECODE),\n            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    .map_err(|e| format!(\"this document could not be parsed: {e}\"))?;",
        "            max_decompressed_size: Some(MAX_DECODE),\n            ..Default::default()\n        },\n    )\n    .map_err(|e| format!(\"this document could not be parsed: {e}\"))?;",
        "an_encrypted_document_can_be_appended_to_and_stays_encrypted",
    ),
    Mutation(
        # Accept a plan that does not describe the file on disk. The turns then
        # land on whichever pages happen to be in those positions.
        "save: accept a plan of the wrong length",
        "src/save.rs",
        "    let pages = ordered_pages(&doc);\n    if pages.len() != plan.baseline as usize {",
        "    let pages = ordered_pages(&doc);\n    if false {",
        "a_plan_that_does_not_match_the_file_on_disk_is_refused",
    ),
    Mutation(
        # Overwrite the open document. The journal then replays against a
        # baseline that no longer exists.
        "save: allow writing over the source",
        "src/save.rs",
        '    if same_file(source, out) {\n        return Err(\n'
        '            "tpdf cannot save over the document it is reading --- choose another name".into(),',
        '    if false {\n        return Err(\n'
        '            "tpdf cannot save over the document it is reading --- choose another name".into(),',
        "saving_over_the_open_document_is_refused",
    ),
    Mutation(
        # Put the save in place during the staging, which is the shape the write
        # had before it was split. The document is replaced while a worker still
        # has it mapped -- which succeeds on macOS, and leaves that worker serving
        # the file that used to be there.
        #
        # Written as a copy-and-remove rather than as an edit to the `stage` call
        # because the staging is a closure now: this reproduces both halves of the
        # old defect -- the source changes, and the path handed back is gone.
        # An earlier attempt handed the source back as the staged path and was
        # NOT caught: `resolved()` canonicalizes, so `/private/var/...` and
        # `/var/...` are unequal `PathBuf`s and the test's `assert_ne!` passed.
        "save: put a save in place during the staging rather than after the close",
        "src/save.rs",
        "    Ok(Staged { path, verified })",
        "    std::fs::copy(&path, &target).map_err(|e| e.to_string())?;\n    let _ = std::fs::remove_file(&path);\n    Ok(Staged { path, verified })",
        "staging_a_save_in_place_writes_beside_the_source_and_leaves_it_alone",
    ),
    Mutation(
        # Report the commit as done without renaming anything. The reader is told
        # their document was saved, the file on disk is the one they opened, and
        # the staged copy of their work sits beside it under a name nothing reads.
        "save: report a commit that never renamed anything",
        "src/save.rs",
        "    commit(staged, &resolved(source))\n}",
        "    let _ = (staged, source);\n    Ok(())\n}",
        "committing_a_staged_save_puts_the_edits_in_the_file_the_reader_opened",
    ),
    Mutation(
        # Stage before the guards run, which is the ordering the `reopen: false`
        # half of `SaveFailure` rests on. Every refusal then leaves a partial
        # file next to the reader's document under a name they never chose.
        "save: stage a save in place before its guards have run",
        "src/save.rs",
        "    if plan.opened_as.is_none() {",
        "    let early = stage(source, |_: &mut std::fs::File| Ok(()))?;\n    let _ = early;\n    if plan.opened_as.is_none() {",
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
        # Copy the staged file into place instead of renaming it. An interrupted
        # save then leaves a truncated PDF where the reader's file was.
        #
        # **Re-aimed 2026-09-01, and it had stopped being able to fail before
        # that.** It was aimed at `write_atomically`, which had three callers --
        # the copy, the split and the merge -- until the copy and the split moved
        # to the seam on 2026-09-01 and the merge followed. The test named below
        # goes through the COPY, so from the moment that path stopped calling
        # `write_atomically` the mutation could redden nothing, while the anchors
        # gate went on reporting the anchor present exactly once. That gate reads
        # the file; only a run reads the coverage. `docs/TRAPS.md` records the
        # shape under *A refactor orphans mutations nobody can see*.
        #
        # `commit` is the mechanism now, and it is stronger: four writing paths
        # end there rather than one.
        "save: write straight to the destination rather than renaming into it",
        "src/save.rs",
        "    std::fs::rename(staged, out).map_err(|e| {",
        "    std::fs::copy(staged, out).map(|_| ()).map_err(|e| {",
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
        # Re-aimed when the guard moved into `Prepared::new`, which is where a
        # query is now read once for a whole walk rather than once per page. The
        # edit is the same one: stop calling a whitespace-only query barren, so
        # the walk runs it and answers with every gap in the document.
        "match: run a query of only whitespace",
        "src/search.rs",
        "            needle.chars.iter().all(|ch| *ch == ' ')",
        "            false",
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
        "            before: slice_of(codes, start.saturating_sub(CONTEXT_CHARS)..start),",
        "            before: slice_of(codes, start..(start + CONTEXT_CHARS).min(codes.len())),",
        "a_hit_carries_the_words_on_either_side_of_it",
    ),
    Mutation(
        "context: run off the end of the page instead of clamping",
        "src/search.rs",
        "            after: slice_of(codes, stop..(stop + CONTEXT_CHARS).min(codes.len())),",
        "            after: slice_of(codes, stop..(stop + CONTEXT_CHARS)),",
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
        # The replacement is the *folded* needle rather than the query as typed,
        # because the walk no longer has the raw query in scope --- it holds a
        # `Prepared`. It is the same mutation: show what was searched for rather
        # than what the page says.
        "context: show the query instead of what the page says",
        "src/search.rs",
        "            hit: exact_of(codes, start..stop),",
        "            hit: needle.chars.iter().collect::<String>(),",
        "the_hit_is_the_page_text_and_not_the_query",
    ),
    Mutation(
        "context: collapse the whitespace inside the hit as well",
        "src/search.rs",
        "            hit: exact_of(codes, start..stop),",
        "            hit: slice_of(codes, start..stop),",
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
        # Same edit, aimed at `Prepared::search_page`, where the answer is now
        # built. The needle's length stands in for the query's, which is what
        # this module has left of it by then.
        "page: report the query's length rather than the page's",
        "src/search.rs",
        "            chars: codes.len() as u32,\n            problem: None,",
        "            chars: self.needle.chars.len() as u32,\n            problem: None,",
        "a_page_with_no_text_reports_it_rather_than_no_matches",
    ),
    Mutation(
        # A run's answers are indexed positionally by the caller, which continues
        # from the end of what came back. Out of order, it re-asks about pages it
        # already has and skips the ones it does not.
        "range: answer a run out of walk order",
        "src/render.rs",
        "        answers.push(answer);",
        "        answers.insert(0, answer);",
        "a_run_answers_the_pages_it_was_asked_about",
    ),
    Mutation(
        # Carry the tail between any two pages of a run rather than between
        # neighbours only. An unscoped scan wraps, so the page after the last is
        # the first, and this reports a phrase spanning the end of the document.
        "range: carry a tail across a gap in the pages",
        "src/render.rs",
        "        let adjacent = step > 0 && pages[step - 1] + 1 == page;",
        "        let adjacent = step > 0;",
        "a_run_does_not_carry_across_pages_that_are_not_neighbours",
    ),
    Mutation(
        # Answer every page asked about, whatever the reply weighs. A reply is
        # read under `MAX_REPLY_BYTES` and one that exceeds it leaves the stream
        # mid-line, which kills the worker -- so a document with a hit on every
        # line turns a search into a lost pool.
        "range: answer every page whatever the reply weighs",
        "src/render.rs",
        "        if spent >= budget {\n            break;\n        }",
        "",
        "a_run_stops_at_the_budget_rather_than_answering_every_page",
    ),
    Mutation(
        # Lose a run because a later page will not extract. One damaged page in
        # sixteen would take the hits on the fifteen before it with it.
        "range: lose the run when a later page will not extract",
        "src/render.rs",
        "            Err(_) => break,",
        "            Err(e) => return Err(e),",
        "a_damaged_page_ends_a_run_rather_than_losing_it",
    ),
    Mutation(
        # Answer a run that produced nothing with a page that has no hits, which
        # is the reassuring branch: a document that could not be read at all
        # would report itself as containing no matches.
        "range: answer an empty run with a page that has no hits",
        "src/render.rs",
        '        return Err("the search answered no pages at all".into());',
        "        return Ok(PageMatches { page: 0, matches: Vec::new(), chars: 0, problem: None, tail: None, more: Vec::new() });",
        "packing_no_pages_at_all_is_refused",
    ),
    Mutation(
        # Evict rather than decline. A scan touches every page once in the same
        # order, so eviction empties the cache of exactly the pages the next scan
        # asks for first -- a hit rate of zero rather than a partial one.
        "cache: evict what fits to make room for what does not",
        "src/textcache.rs",
        "        if self.chars + held.len() <= budget {\n            self.chars += held.len();\n            self.entries.insert(key, Arc::clone(&held));\n        }",
        "        while self.chars + held.len() > budget {\n            let Some(victim) = self.entries.keys().next().copied() else { break };\n            if let Some(gone) = self.entries.remove(&victim) { self.chars -= gone.len(); }\n        }\n        self.chars += held.len();\n        self.entries.insert(key, Arc::clone(&held));",
        "what_does_not_fit_is_declined_rather_than_evicting_what_does",
    ),
    Mutation(
        # Key on the page alone. The two extractions carry the same codes today,
        # so a caller that starts asking for cropped text would be served the
        # uncropped page's answer with nothing anywhere disagreeing.
        "cache: key on the page and ignore the crop",
        "src/textcache.rs",
        "    (page, crop.map(|c| c.map(f32::to_bits)))",
        "    let _ = crop;\n    (page, None)",
        "a_crop_is_part_of_the_key",
    ),
    Mutation(
        # Count the *asks* rather than the parses, by moving the counter outside
        # the cell that memoises it. Five parses and one give identical answers
        # and differ by 5.8 ms each on the 775-page document, so the count is the
        # only thing that can see the difference -- and it only can while it is
        # inside the closure that does the work.
        #
        # An earlier version of this mutation replaced the cell with a fresh
        # `OnceCell` per call, which is what "parse again for every question"
        # would literally be. It does not compile: the reference would outlive
        # the temporary. Recorded because the shape recurs -- a memo returning a
        # borrow cannot be un-memoised by an expression.
        "graph: count the questions rather than the parses",
        "src/docgraph.rs",
        "        self.parsed\n            .get_or_init(|| {",
        "        self.parses.set(self.parses.get() + 1);\n        self.parsed\n            .get_or_init(|| {",
        "one_parse_answers_every_question",
    ),
    Mutation(
        # Start the fingerprint at open, as it was. Hashing reads the file end to
        # end -- 452 ms cold on the 337 MB scan fixture -- and this puts that read
        # back on the wire while the first page is being drawn.
        "hash: start the fingerprint at open rather than at the first edit",
        "src/edits.rs",
        "                sources: HashMap::new(),\n                pending: None,\n            },\n        );\n    }",
        "                sources: HashMap::new(),\n                pending: None,\n            },\n        );\n        self.wake(doc);\n    }",
        "opening_a_document_does_not_start_the_hash",
    ),
    Mutation(
        # Leave the edit path to somebody else. `edits.rs` has no chokepoint, so
        # a trigger missing from one mutator is a document whose hash does not
        # start until the save that waits for it.
        "hash: do not start the fingerprint on an edit",
        "src/edits.rs",
        '        self.wake(doc);\n        let mut docs = self.docs.lock().expect("edits lock");\n        let open = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;\n        let model = &mut open.model;\n        model.apply_in(cmd, sweep).map_err(describe)?;',
        '        let mut docs = self.docs.lock().expect("edits lock");\n        let open = docs.get_mut(&doc).ok_or_else(|| unknown(doc))?;\n        let model = &mut open.model;\n        model.apply_in(cmd, sweep).map_err(describe)?;',
        "every_edit_starts_the_hash",
    ),
    Mutation(
        # One of the twelve that lock for themselves, so it has a wake of its
        # own and no chokepoint to inherit one from. Aimed here rather than only
        # at `command_in` because the two halves fail differently: this is the
        # half a test written against a delegating method cannot see.
        "hash: do not start the fingerprint on an annotation",
        "src/edits.rs",
        "    pub fn annotate(&self, doc: u32, want: NewMark, made: String) -> Result<EditState, String> {\n        // The reader is here: start the fingerprint if nothing has. See `wake`.\n        self.wake(doc);",
        "    pub fn annotate(&self, doc: u32, want: NewMark, made: String) -> Result<EditState, String> {",
        "every_edit_starts_the_hash",
    ),
    Mutation(
        # The other direction: wake on a read of the state as well. The frontend
        # asks for it the instant a document opens, so this is the deferral
        # undone while every test about the fingerprint still passes.
        "hash: start the fingerprint when the state is read",
        "src/edits.rs",
        "    pub fn state(&self, doc: u32) -> Result<EditState, String> {",
        "    pub fn state(&self, doc: u32) -> Result<EditState, String> {\n        self.wake(doc);",
        "reading_the_state_does_not_start_the_hash",
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
        "                let to = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };\n                self.order.insert(to, page);",
        "                let to = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor),\n                };\n                self.order.insert(to, page);",
        "a_page_moved_after_one_that_follows_it_lands_immediately_after_it",
    ),
    Mutation(
        # "No anchor" means the front. Sending it to the back instead is what a
        # reader would see as the drag going to the wrong end of the document.
        "docmodel: send an unanchored move to the back",
        "src/docmodel.rs",
        "                let to = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };\n                self.order.insert(to, page);",
        "                let to = match after {\n                    None => self.order.len(),\n                    Some(anchor) => self.position(anchor) + 1,\n                };\n                self.order.insert(to, page);",
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
        # Take a snapshot that has forgotten the pending redactions. Every short
        # journal is unaffected -- SNAPSHOT_EVERY is 32, so most documents never
        # take one -- and a rebuild through such a snapshot silently drops every
        # region marked before it. A redaction missing from the review list is
        # the one failure this subsystem must never have, and it would arrive
        # with the document looking entirely normal.
        "docmodel: snapshot a document without its pending redactions",
        "src/docmodel.rs",
        "            self.snapshots.insert(self.cursor, self.now.clone());",
        "            let mut snap = self.now.clone();\n"
        "            snap.redactions.clear();\n"
        "            self.snapshots.insert(self.cursor, snap);",
        "a_pending_redaction_survives_a_rebuild_across_a_snapshot_boundary",
    ),
    Mutation(
        # Accept a region that covers nothing. It reaches the review list, the
        # overlay draws a line, and applying it removes nothing -- so a reader
        # is shown a row that certifies the removal of nothing at all.
        "docmodel: accept a redaction covering no area",
        "src/docmodel.rs",
        "        if !redaction.area.covers_area() {",
        "        if false {",
        "a_region_covering_nothing_is_refused_rather_than_listed",
    ),
    Mutation(
        # Mark a region without checking the page is live. The redaction lands
        # in the table under a page nobody can see, and every refusal the
        # frontend depends on for a stale panel row stops happening.
        "docmodel: mark a region without checking the page exists",
        "src/docmodel.rs",
        "        self.now.live(redaction.page)?;",
        "        let _ = self.now.live(redaction.page);",
        "a_region_on_a_page_that_is_gone_says_which_of_the_two_it_is",
    ),
    Mutation(
        # Collapse the two redaction refusals, exactly as the page pair above is
        # collapsed. The model still refuses; what is lost is the distinction
        # between a row a reader has already removed and an id nobody issued,
        # which is what a stale review panel needs told apart.
        "docmodel: report a removed redaction as one that never existed",
        "src/docmodel.rs",
        "                Refusal::RedactionRemoved(redaction)",
        "                Refusal::NoSuchRedaction(redaction)",
        "removing_a_pending_redaction_twice_is_told_which_answer_it_is_getting",
    ),
    Mutation(
        # Delete a page and leave its pending redactions behind. The document is
        # identical -- the page is gone from the order either way -- and the
        # instruction survives, pointing at a page nobody can see. Only a
        # command naming it afterwards can tell.
        "docmodel: delete a page without tombstoning its redactions",
        "src/docmodel.rs",
        "                for redaction in self.redactions.remove(&page).unwrap_or_default() {\n"
        "                    self.redaction_graves.insert(redaction);\n"
        "                }",
        "                let _ = self.redactions.remove(&page);",
        "deleting_a_page_takes_its_pending_redactions_with_it",
    ),
    Mutation(
        # Keep the body of a redaction whose command was discarded. Nothing
        # about the working document differs -- the redaction is gone from the
        # page either way -- so only the accounting observable can see it.
        "docmodel: keep redaction bodies a discarded redo tail named",
        "src/docmodel.rs",
        "                Command::Redact { redaction, .. } => {\n"
        "                    self.redactions.remove(&redaction);\n"
        "                }",
        "                Command::Redact { .. } => {}",
        "a_discarded_redo_tail_drops_the_redaction_bodies_it_named",
    ),
    Mutation(
        # Answer with every page's redactions rather than the named page's. A
        # single-page document cannot tell, and neither can a review list read
        # whole -- only an assertion about which page a region is on.
        "docmodel: list every page's redactions for whichever page is asked",
        "src/docmodel.rs",
        "    pub fn redactions_on(&self, page: PageId) -> &[RedactionId] {\n"
        "        self.redactions\n"
        "            .get(&page)",
        "    pub fn redactions_on(&self, page: PageId) -> &[RedactionId] {\n"
        "        self.redactions\n"
        "            .values()\n"
        "            .next()",
        "a_region_marked_for_removal_lands_on_the_page_it_names",
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
        # Keep the components that parsed and drop the one that did not. The
        # array is then *shorter*, so `[/Red 0 0 0]` is read as a legal
        # DeviceRGB triple and the comment comes out black -- which is why the
        # test's fixture is four entries long rather than three.
        "annots: skip an unreadable colour component instead of declining the array",
        "src/annots.rs",
        "    if values.len() != array.len() {",
        "    if false {",
        "a_colour_with_a_value_that_is_not_a_number_is_declined_whole",
    ),
    Mutation(
        # Let an infinity or a `NaN` through to the clamp. An infinity clamps to
        # a bound and looks like a colour; a `NaN` clamps to `NaN` and reaches
        # serde, which refuses it -- so this one is a wrong colour on one input
        # and a failed reply on the other.
        "annots: clamp a colour component without checking it is finite",
        "src/annots.rs",
        "    if values.iter().any(|value| !value.is_finite()) {\n        return None;\n    }\n    let rgb = match values[..] {",
        "    if false {\n        return None;\n    }\n    let rgb = match values[..] {",
        "a_colour_with_a_value_that_is_not_a_number_is_declined_whole",
    ),
    Mutation(
        # Read DeviceGray into the red channel alone. Every gray then comes out
        # a shade of red, and 0.0 -- black -- is the one value that still looks
        # right, which is why the test uses 0.25.
        "annots: read a one-number colour into one channel",
        "src/annots.rs",
        "        [gray] => [gray, gray, gray],",
        "        [gray] => [gray, 0.0, 0.0],",
        "a_colour_names_its_space_by_how_many_numbers_it_has",
    ),
    Mutation(
        # Drop the black component from the CMYK conversion. Correct for every
        # colour with `k` of zero, which is most of a palette.
        "annots: convert a CMYK colour without its black component",
        "src/annots.rs",
        "            (1.0 - c) * (1.0 - k),",
        "            1.0 - c,",
        "a_colour_names_its_space_by_how_many_numbers_it_has",
    ),
    Mutation(
        # Read an array of any other length as black rather than as no colour.
        # An empty `/C` -- which §12.5.2 spells *no colour* -- then paints a
        # black comment, and so does a malformed two-number array.
        "annots: read a colour of an illegal length as black",
        "src/annots.rs",
        "        _ => return None,\n    };\n    // Legal values",
        "        _ => [0.0, 0.0, 0.0],\n    };\n    // Legal values",
        "an_array_that_is_not_one_of_the_four_legal_lengths_is_not_a_colour",
    ),
    Mutation(
        # Pass a component outside 0..1 through. Harmless for a value a producer
        # rounded past 1.0 and not for one a document chose, and either way it
        # is a number no consumer of this field expects.
        "annots: do not clamp a colour component to its legal range",
        "src/annots.rs",
        "    Some(rgb.map(|component| component.clamp(0.0, 1.0)))",
        "    Some(rgb)",
        "a_component_outside_the_legal_range_is_clamped_rather_than_declined",
    ),
    Mutation(
        # Never read `/C` at all. Every unit test above still passes, because
        # they call `color_of` directly -- this is the one that says the reader
        # is *wired in*, and the only test that can see it is the one that goes
        # through `scan`.
        "annots: do not read a colour onto the comment",
        "src/annots.rs",
        "        color: color_of(annot, document),",
        "        color: None,",
        "the_scan_reads_the_colour_onto_the_comment",
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
        '        err::PASSWORD => "This document is locked, and needs a password.".into(),',
        '        err::PASSWORD => "This file is not a PDF, or it is damaged beyond reading.".into(),',
        "each_reason_says_something_different",
    ),
    Mutation(
        # Keep the flag out of the reply. The reason still crosses and still
        # reads correctly, so what a reader loses is only the prompt --- they are
        # told, accurately, about a document they can no longer open.
        "worker_proto: stop sending whether a refusal is one a reader can answer",
        "src/worker_proto.rs",
        "    #[serde(default)]\n    pub locked: bool,",
        "    #[serde(default, skip_serializing)]\n    pub locked: bool,",
        "a_reply_is_locked_only_when_it_says_so",
    ),
    Mutation(
        # The parent's half of the same distinction, which is the copy that
        # drifts: `docs/TRAPS.md` records two copies of one distinction, and a
        # mutation of one surviving.
        "workers: drop the answerability when a worker's refusal reaches the engine",
        "src/workers.rs",
        "        locked: response.locked,",
        "        locked: false,",
        "a_locked_reply_reaches_the_engine_as_a_locked_refusal",
    ),
    Mutation(
        # Widen the one refusal a reader can answer to nearly every one. A
        # document that is not a PDF then asks for a password it has none for,
        # and the reader retypes into a dialog that can never accept anything.
        "progressive: treat almost every open failure as answerable",
        "src/progressive.rs",
        "            locked: code == err::PASSWORD,",
        "            locked: code != err::FORMAT,",
        "only_a_password_refusal_is_one_a_reader_can_answer",
    ),
    Mutation(
        # The other direction, through the widening `?` uses. Every ordinary
        # failure inside an open --- a page with no size, a path that is not
        # UTF-8 --- would then be shown as a locked document.
        "progressive: let a failure that arrived as prose claim it is locked",
        "src/progressive.rs",
        """        Self {
            reason,
            locked: false,
        }""",
        """        Self {
            reason,
            locked: true,
        }""",
        "a_refusal_widened_from_prose_is_not_locked",
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
        "        if let Ok(Object::Reference(child)) = dict.get(b\"First\") {\n            walk_outline(\n                document,\n                *child,\n                numbers,\n                geometry,\n                seen,\n                out,\n                limits,\n                urls,\n                depth - 1,\n            );\n        }\n        node = match dict.get(b\"Next\") {",
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
        # Accept an update section built against a different number of bytes than
        # the caller measured. The two used to be one number by construction, and
        # stopped being one when the parse moved into the worker: a stale mapping
        # or a file that moved between the check and the build now produces a
        # cross-reference pointing into a document nobody has, in a file that
        # still opens.
        "append: accept an update built against a different length",
        "src/save.rs",
        "    if update.built_against as u64 != ready.was {",
        "    if false {",
        "an_update_built_against_a_different_length_is_refused",
    ),
    Mutation(
        # Keep the call and let it pass whenever the length matches, which is
        # what `append_in_place` compared until 2026-08-22 while
        # `Appended::verified` held a full fingerprint nothing read. A file
        # replaced by a distinct revision of the same size is accepted, and this
        # update's byte offsets go into an object graph they were never computed
        # against.
        "save: accept any file of the right length before appending",
        "src/save.rs",
        "    appended\n        .verified\n        .agrees_with_metadata(&meta, source)",
        "    appended\n        .verified\n        .agrees_with_metadata(&meta, source)\n        .or_else(|e| if meta.len() == appended.was { Ok(()) } else { Err(e) })",
        "an_append_refuses_a_replacement_that_kept_the_length",
    ),
    Mutation(
        # Drop the guard entirely, so nothing is compared before the write. Its
        # sibling above keeps the call and weakens it; this one proves the call
        # is reached at all, which is the difference between a covered guard and
        # a guard whose caller nothing tests.
        "save: append without checking the file at all",
        "src/save.rs",
        "    appended\n        .verified\n        .agrees_with_metadata(&meta, source)\n        .map_err(|why| {",
        "    Ok::<(), String>(())\n        .map_err(|why: String| {",
        "an_append_refuses_a_replacement_that_kept_the_length",
    ),
    Mutation(
        # Ask the pathname which file this is instead of asking the handle, so
        # the identity check compares the replacement with itself and always
        # agrees. The save then reports success for edits that went somewhere
        # the reader cannot reach.
        "save: identify the file by name rather than by the handle written to",
        "src/save.rs",
        "    let writing_to = FileId::of(file).ok_or_else(|| {",
        "    let writing_to = FileId::at(source).ok_or_else(|| {",
        "an_append_writes_through_its_handle_and_says_so_when_the_name_moves",
    ),
    Mutation(
        # Write the update at the file offset the handle opens with, which is
        # zero, rather than seeking to the end. It overwrites the previous
        # revision's first bytes instead of adding to it -- and the reason this
        # line exists at all is that the handle is no longer in append mode:
        # append mode on Windows drops the access right `set_len` needs, so
        # every roll-back there would have failed with access denied.
        "save: write the update at the start of the file rather than the end",
        "src/save.rs",
        "    file.seek(std::io::SeekFrom::End(0))?;\n    file.write_all(body)?;",
        "    file.write_all(body)?;",
        "an_append_leaves_every_byte_of_the_previous_revision_where_it_was",
    ),
    Mutation(
        # Truncate whatever is at the staging name, which is what
        # `std::fs::write` did: it destroyed an unrelated file beside the
        # destination and followed a symlink planted at that path.
        "save: stage over whatever is already at the temporary name",
        "src/save.rs",
        "            .create_new(true)",
        "            .create(true)\n            .truncate(true)",
        "staging_never_writes_over_a_file_that_is_already_there",
    ),
    Mutation(
        # Go back to one staging name per destination. Two saves aimed at one
        # file share a temporary again, so the second truncates the first's bytes
        # and either can rename or delete the other's work.
        "save: give every save of a destination the same staging name",
        "src/save.rs",
        "    name.push(format!(\".{PARTIAL}-{}-{attempt}\", std::process::id()));",
        "    name.push(format!(\".{PARTIAL}\"));",
        "two_saves_to_one_destination_do_not_share_a_staging_file",
    ),
    Mutation(
        # Replace the page's `/Annots` rather than extending it. A page that had
        # no comments is unaffected, which is most pages and every fixture
        # written by hand -- and a page that had one loses it the moment a reader
        # highlights anything.
        "save: replace a page's /Annots instead of extending it",
        "src/save/marks.rs",
        "            array.push(Object::Reference(annotation));\n            doc.get_object_mut(page)",
        "            array.clear();\n            array.push(Object::Reference(annotation));\n            doc.get_object_mut(page)",
        "a_mark_is_written_whatever_shape_the_page_s_annots_is_in",
    ),
    Mutation(
        # Write the annotation object and leave it off the page. The file grows
        # by a perfectly well-formed annotation that is on no page, which every
        # reader reports as a document with no comments -- and which any check
        # counting objects passes.
        "save: write the mark object without listing it on the page",
        "src/save/marks.rs",
        "        let annotation = doc.add_object(dictionary);\n        attach(doc, page, annots, annotation)?;",
        "        let _annotation = doc.add_object(dictionary);",
        "a_marked_page_lists_the_mark_in_its_own_annots",
    ),
    Mutation(
        # Let a mark be written onto a page object that two page numbers share.
        # It appears on both, which is not what the reader asked for and not
        # something the file records as a mistake.
        "save: allow a mark on a page two numbers share",
        "src/save/marks.rs",
        # Re-aimed 2026-08-30: a mark is addressed by its position in the plan
        # now, so the count is over the plan's own page objects rather than over
        # the kept baseline numbers.
        "        if sheet.iter().filter(|id| **id == page).count() > 1 {",
        "        if false {",
        "a_mark_on_a_page_two_numbers_share_is_refused",
    ),
    Mutation(
        # Scope the shared-page refusal to the whole file rather than to this
        # mark's page. Every refusal test still passes; what breaks is the
        # ordinary case, where one malformed page makes the rest unmarkable.
        "save: refuse a mark anywhere in a file that has a shared page",
        "src/save/marks.rs",
        # Re-aimed 2026-08-30 with its sibling above, onto the same line and for
        # the same reason. The over-scoping is spelled differently and means
        # exactly what it did: refuse if *anything* in this document is shared.
        "        if sheet.iter().filter(|id| **id == page).count() > 1 {",
        "        if sheet.iter().any(|id| sheet.iter().filter(|other| *other == id).count() > 1) {",
        "a_mark_on_an_unshared_page_of_a_document_that_has_a_shared_one_is_written",
    ),
    Mutation(
        # Keep a quad that covers nothing. A `/QuadPoints` entry of zero area is
        # one some readers draw as nothing at all and others draw as a hairline,
        # so the mark's appearance stops being the document's.
        "save: keep quads that cover no area",
        "src/save/marks.rs",
        "        .filter(|quad| quad.covers_area())",
        "        .filter(|_| true)",
        "a_mark_whose_quads_all_collapse_is_refused_rather_than_written_empty",
    ),
    Mutation(
        # Let a plan carrying a mark call itself the file on disk. The print path
        # then hands over the original bytes, and a reader who highlighted a
        # document prints one without the highlights -- with nothing failing,
        # because what it printed is a perfectly good file.
        # Re-aimed 2026-08-26, when a redaction clause joined the predicate.
        "edits: call a plan with marks in it the file itself",
        "src/edits.rs",
        "        self.marks.is_empty() && self.redactions.is_empty() && self.pages_are_the_file()",
        "        self.redactions.is_empty() && self.pages_are_the_file()",
        "a_plan_carrying_a_mark_is_not_the_file_on_disk",
    ),
    Mutation(
        # Drop the marks a subset plan should carry. An extract of pages a reader
        # highlighted comes out unmarked, which looks like a feature nobody
        # implemented rather than one that was lost on the way.
        "edits: leave the marks out of a plan",
        "src/edits.rs",
        # Re-aimed 2026-08-30: the `filter_map` that dropped the pages tpdf made
        # is an `enumerate` now, because every page carries marks.
        "    pages\n        .iter()\n        .enumerate()",
        "    pages\n        .iter()\n        .take(0)\n        .enumerate()",
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
        "        self.now = self.rebuild(self.cursor);\n        true",
        "        self.next_mark = self.next_mark.saturating_sub(1);\n        self.now = self.rebuild(self.cursor);\n        true",
        "an_id_spent_by_an_undone_mark_is_never_issued_again",
    ),
    Mutation(
        # Spend the id before the preconditions are checked. Nothing visible
        # changes -- the mark is still refused -- which is the point: the ids
        # then run ahead of the marks, and the only thing that can see it is the
        # accounting observable.
        "docmodel: issue a mark's id before checking that it covers anything",
        "src/docmodel.rs",
        "        if empty {\n            return Err(Refusal::EmptyMark);\n        }\n        self.now.live(mark.page)?;",
        "        let id = MarkId(self.next_mark);\n        self.next_mark += 1;\n        let _ = id;\n        if empty {\n            return Err(Refusal::EmptyMark);\n        }\n        self.now.live(mark.page)?;",
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
        "                for mark in self.marks.remove(&page).unwrap_or_default() {\n                    self.forget_mark(mark);",
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
        # **Shares an anchor with the mutation below it, and that is a fact about
        # the code rather than a duplicate entry.** Deleting a page and removing
        # a mark were two cleanups doing different work; they are one
        # `forget_mark` since 2026-08-24, so there is one line to break and it
        # reddens both routes. Both entries are kept because each names a
        # *different* test, and what a mutation certifies is that its named test
        # can catch the loss --- folding them into one would leave one of those
        # two tests pinned by nothing. The cost is one duplicated run.
        "docmodel: keep a note when the page it is on is deleted",
        "src/docmodel.rs",
        "        self.mark_graves.insert(mark);\n        self.notes.remove(&mark);",
        "        self.mark_graves.insert(mark);",
        "a_marks_note_goes_with_it_and_comes_back_with_it",
    ),
    Mutation(
        # Keep the note when the mark is taken off the page. Same shape as the
        # entry above and a different arm: the map's keys are meant to be
        # exactly the live marks, and a leftover makes a document that was
        # annotated and un-annotated compare unequal to one that never was --
        # which is what a snapshot rebuild is checked against.
        # The other half of the pair above: same anchor, different test.
        "docmodel: keep a note when its mark is taken off the page",
        "src/docmodel.rs",
        "        self.mark_graves.insert(mark);\n        self.notes.remove(&mark);",
        "        self.mark_graves.insert(mark);",
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
        "src/save/marks.rs",
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
        "src/save/marks.rs",
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
        "src/save/marks.rs",
        "        MarkKind::Underline => (bottom, thickness),",
        "        MarkKind::Underline => (bottom - thickness / 2.0, thickness),",
        "a_line_stays_inside_the_quad_it_marks",
    ),
    Mutation(
        # And a strikeout drawn at the bottom, which is an underline with the
        # wrong subtype. Every check keyed on the subtype passes; only where the
        # rule sits tells them apart.
        "save: draw a strikeout where an underline goes",
        "src/save/marks.rs",
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
        "        let quads = Stroke::bounds(&strokes, (width / 2.0) as f32)",
        "        let quads = Stroke::bounds(&strokes, 1000.0)",
        "erasing_a_stroke_leaves_the_others_and_shrinks_the_rectangle",
    ),
    Mutation(
        # Leave the version behind when the mark goes. A mark removed and
        # restored by undo then comes back at whatever version an erasure left
        # it on rather than the one the journal says.
        "docmodel: let a removed drawing keep the version it was erased to",
        "src/docmodel.rs",
        "        self.inks.remove(&mark);\n        self.colors.remove(&mark);",
        "        self.colors.remove(&mark);",
        "a_removed_drawing_forgets_which_version_it_was_on",
    ),
    Mutation(
        # Read this process's counters instead of the child's. The mistake the
        # test exists for, and it is not hypothetical: `GetProcessMemoryInfo`
        # takes a handle, `GetCurrentProcess()` is the pseudo-handle for self,
        # and `tile_bench` passes exactly that on purpose two files away. A
        # worker's headroom would then be reported as the app's.
        "sandbox: read the parent's commit charge rather than the worker's",
        "src/sandbox_win.rs",
        "        let ok = unsafe { GetProcessMemoryInfo(self.process, &raw mut counters, counters.cb) };",
        "        let ok = unsafe { GetProcessMemoryInfo(windows_sys::Win32::System::Threading::GetCurrentProcess(), &raw mut counters, counters.cb) };",
        "a_contained_childs_peak_commit_is_readable_and_is_the_childs",
        only_on="windows",
    ),
    Mutation(
        # Lie about the struct's size. Win32 reads `cb` to decide which version
        # of the counters it was handed, so a zero is refused -- and the refusal
        # is the branch that produces `None`, which is what a caller reads as
        # "this platform cannot measure that" rather than as a broken call.
        "sandbox: hand GetProcessMemoryInfo a zero-length struct",
        "src/sandbox_win.rs",
        "        counters.cb = size_of_u32::<PROCESS_MEMORY_COUNTERS>();",
        "        counters.cb = 0;",
        "a_contained_childs_peak_commit_is_readable_and_is_the_childs",
        only_on="windows",
    ),
    Mutation(
        # Take the bound off. The plan condition still holds, so every marks-only
        # save goes back to being prepared in the worker whatever the document
        # weighs -- which is the state that shipped on 2026-08-22 and aborts a
        # worker above roughly 350 MB.
        "append: let a document of any size be prepared in the worker",
        "src/save.rs",
        "    if plan.is_appendable() && source_bytes <= APPEND_MAX_BYTES {",
        "    if plan.is_appendable() && source_bytes <= u64::MAX {",
        "a_marks_only_plan_is_rewritten_once_the_file_is_too_large_to_parse_twice",
    ),
    Mutation(
        # Move the bound to 256 MB from 256 MiB. A 5% change, in the direction
        # that is safe, and the point is that it is not silent: the value is
        # pinned so that it moves in the test as well or not at all.
        "append: move the size bound without saying so",
        "src/save.rs",
        "pub const APPEND_MAX_BYTES: u64 = 256 * 1024 * 1024;",
        "pub const APPEND_MAX_BYTES: u64 = 256 * 1000 * 1000;",
        "a_marks_only_plan_is_rewritten_once_the_file_is_too_large_to_parse_twice",
    ),
    Mutation(
        # Read an unmeasurable file as empty rather than as unbounded. It then
        # takes the append -- the arm with a memory bound over it -- on the one
        # input nothing knows the size of, which is the "could not check" and
        # "checked, fine" collapse this function exists to keep apart.
        "append: treat a file that cannot be measured as small",
        "src/save.rs",
        "        std::fs::metadata(source).map_or(u64::MAX, |m| m.len()),",
        "        std::fs::metadata(source).map_or(0, |m| m.len()),",
        "a_document_whose_size_cannot_be_read_is_rewritten",
    ),
    Mutation(
        # Point the hook at a file that is not there, which is what a rename
        # that forgets the config looks like. NOT spelled as a mistyped key:
        # `installerHooksTypo` is rejected by the build script's own schema
        # ("unknown field ... expected one of ... installerHooks"), so that
        # mutation never reaches a test and reports as a build failure.
        #
        # The bundler refuses this one too -- "failed to resolve `bundle >
        # windows > nsis > installerHooks`" -- but only when a bundle is
        # actually built, which is a CI leg and not a gate. The test answers in
        # seconds, on every machine.
        "installer: point the NSIS hook at a file that is not there",
        "tauri.windows.conf.json",
        '"installerHooks": "installer-hooks.nsh"',
        '"installerHooks": "installer-hooks-gone.nsh"',
        "the_windows_installer_clears_the_way_for_the_pdfium_directory",
    ),
    Mutation(
        # Leave the hook wired and take out the only thing it does. Everything
        # that can be checked from outside the installer -- the config, the
        # include, the macro definition -- still looks right.
        "installer: keep the hook but remove the deletion",
        "installer-hooks.nsh",
        '    Delete "$INSTDIR\\pdfium"',
        '    DetailPrint "$INSTDIR\\pdfium"',
        "the_windows_installer_clears_the_way_for_the_pdfium_directory",
    ),
    Mutation(
        # Prefer the page tree even where PDFium answered. Nothing visible moves
        # on any fixture -- the two agree wherever both answer -- so this is here
        # for the rule rather than for a symptom: the renderer's own reading is
        # what every downstream number is consistent with, and a second opinion
        # can only introduce a disagreement between the size a page reports and
        # the pixels it produces.
        "geometry: let the page tree override PDFium's own reading",
        "src/progressive.rs",
        "        (None, Some(from_tree)) => from_tree,",
        "        (_, Some(from_tree)) => from_tree,",
        "the_page_tree_decides_only_where_pdfium_has_no_media_box",
    ),
    Mutation(
        # Refuse to answer where PDFium could not. The page then keeps the box
        # PDFium computed for an inheriting page, which is the defect -- and a
        # document whose bytes cannot be re-read is meant to end up here, so the
        # arm has to stay reachable rather than be deleted.
        "geometry: decline the page tree's box even when it is the only one",
        "src/progressive.rs",
        "        (None, Some(from_tree)) => from_tree,",
        "        (None, Some(_)) => crop,",
        "the_page_tree_decides_only_where_pdfium_has_no_media_box",
    ),
    Mutation(
        # Return a short answer instead of refusing when the two parsers count
        # pages differently. The caller indexes positionally, so page 5's box
        # lands on page 4 -- a plausible page at a plausible size, with nothing
        # for a reader to notice.
        "geometry: return what the page tree found rather than refusing",
        "src/pagetree.rs",
        "    if pages.len() != page_count {",
        "    if false {",
        "a_page_count_the_two_parsers_disagree_about_is_refused",
    ),
    Mutation(
        # Stop undoing the display transpose. Every dimension is right and the
        # rectangle is on its side, which is the shape of error that survives a
        # size comparison and only a box comparison can see.
        "geometry: hand back the displayed rectangle as the page's own",
        "src/pagetree.rs",
        "        let (width, height) = if self.turns % 2 == 1 {\n            (self.height, self.width)\n        } else {\n            (self.width, self.height)\n        };",
        "        let (width, height) = (self.width, self.height);",
        "the_pages_own_box_is_the_same_rectangle_at_every_turn",
    ),
    # --------------------------------------------------------------
    # A mark on a page the document says is turned
    # --------------------------------------------------------------
    Mutation(
        # Stop swapping the box's sides at an odd quarter. This is the defect
        # itself, one level up from where it shipped: `wrap` is then handed the
        # box's height, which is what broke four words into eighteen lines.
        "turned marks: the reader's box keeps the page's own sides",
        "src/save/marks.rs",
        "            1 => Self {\n                width: h,\n                height: w,\n                origin: (quad[0], quad[1]),",
        "            1 => Self {\n                width: w,\n                height: h,\n                origin: (quad[0], quad[1]),",
        "an_upright_box_is_the_rectangle_the_reader_dragged",
    ),
    Mutation(
        # Anchor the reader's top-left at the wrong corner of the page-space
        # quad. Every size stays right and every mark moves by its own width,
        # which no size assertion can see -- the reason that test reads the
        # corners as well.
        "turned marks: the reader's corner is the page's",
        "src/save/marks.rs",
        "                origin: (quad[0], quad[1]),\n                right: (0.0, 1.0),",
        "                origin: (quad[0], quad[3]),\n                right: (0.0, 1.0),",
        "an_upright_box_is_the_rectangle_the_reader_dragged",
    ),
    Mutation(
        # Wrap to the page's width again, which is what shipped.
        "turned marks: a text box wraps to the page's width",
        "src/save/marks.rs",
        "        let width = seen.width - textbox::INSET * 2.0;",
        "        let width = (quad[2] - quad[0]) - textbox::INSET * 2.0;",
        "a_text_box_wraps_to_the_width_the_reader_dragged_however_the_page_is_turned",
    ),
    Mutation(
        # Keep the right lines and set them along the page's axis. The other
        # half of the same defect, and the half a line count cannot reach.
        "turned marks: type is set on the identity whatever the turn",
        "src/save/marks.rs",
        '            "{} {} {} {} {x} {y} Tm",\n            flat(self.right.0),\n            flat(self.right.1),\n            flat(-self.down.0),\n            flat(-self.down.1)',
        '            "{} {} {} {} {x} {y} Tm",\n            flat(1.0),\n            flat(0.0),\n            flat(0.0),\n            flat(1.0)',
        "type_runs_the_readers_way_on_a_turned_page",
    ),
    Mutation(
        # Stop normalising `-0.0`. The stream is still correct arithmetic and
        # the identity now prints as `1 0 -0 1`, which is the operator a reader
        # of the file has to squint at -- caught by the upright control rather
        # than by the turned assertion, which is why that control reads the
        # coefficients rather than merely counting them.
        "turned marks: a negated zero reaches the content stream",
        "src/save/marks.rs",
        "        let flat = |value: f64| if value == 0.0 { 0.0 } else { value };",
        "        let flat = |value: f64| value;",
        "type_runs_the_readers_way_on_a_turned_page",
    ),
    Mutation(
        # Put the rule back on the page's bottom edge. On a turned page that is
        # the left edge of the words.
        "turned marks: a rule sits on the page's bottom edge",
        "src/save/marks.rs",
        "        let (base, band) = line_rect(kind, 0.0, seen.height);\n        let [x, y, width, height] = seen.rect(",
        "        let (base, band) = line_rect(kind, 0.0, quad[3] - quad[1]);\n        let [x, y, width, height] = seen.rect(",
        "a_rule_sits_under_the_words_however_the_page_is_turned",
    ),
    Mutation(
        # Size a stamp's word from the page's box. The word is then set for a
        # rectangle of the other shape, which is a size error rather than a
        # placement one.
        "turned marks: a stamp is sized by the page's box",
        "src/save/marks.rs",
        "        let inner_w = seen.width - STAMP_INSET * 2.0;\n        let inner_h = seen.height - STAMP_INSET * 2.0;",
        "        let inner_w = (quad[2] - quad[0]) - STAMP_INSET * 2.0;\n        let inner_h = (quad[3] - quad[1]) - STAMP_INSET * 2.0;",
        "a_stamps_word_is_sized_by_the_box_the_reader_dragged",
    ),
    Mutation(
        # Rewrite in the coordinator, which is the code this replaced. Every
        # number a caller can see is identical on a good document -- the two
        # processes produce the same bytes -- so the only thing that can notice
        # is a test whose source does not parse here.
        #
        # It sits in `staged_rewrite` now rather than inline in
        # `stage_in_place`, because the copy and the split take the same step;
        # three tests can catch it and the one named below is the oldest.
        "save: rewrite in the coordinator instead of the worker",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = Here.write(source, len, out, plan, job, password, inputs)?;",
        "the_coordinator_does_not_parse_the_document_it_rewrites",
    ),
    Mutation(
        # Build the print job in the coordinator, which is what every print of
        # an edited document did until 2026-09-01. The bytes are identical
        # either way on a document that parses.
        "save: build the print job in the coordinator instead of the worker",
        "src/save.rs",
        # **Re-aimed 2026-09-01**, when the scratch dance both print routes do
        # moved into `into_scratch` and this call became the closure's body.
        "        staged_rewrite(\n            rewriter,\n            reading,",
        "        staged_rewrite(\n            &Here,\n            reading,",
        "the_coordinator_does_not_parse_the_document_it_prints",
    ),
    Mutation(
        # Tell the writer a print job is a save. The reader's own rotation is
        # dropped, so the pages come out the way the file holds them rather than
        # the way they are looking at them -- and every byte-level assertion
        # about the job still passes, because the writer is asked for bytes and
        # answers with them.
        "save: build a print job as if it were a save",
        "src/save.rs",
        "        Job::Print { view },",
        "        Job::Save,",
        "the_coordinator_does_not_parse_the_document_it_prints",
    ),
    Mutation(
        # Keep the scratch file the job was built in. It holds the reader's
        # document decrypted and reordered, in a shared directory, under a name
        # they never chose.
        "save: leave the print job's scratch file in the temporary directory",
        "src/save.rs",
        "    let _ = std::fs::remove_file(&at);\n    built",
        "    built",
        "the_coordinator_does_not_parse_the_document_it_prints",
    ),
    Mutation(
        # Parse a copy in the coordinator, which is what Save a copy, Extract
        # and Redact to a copy did until 2026-09-01. The bytes are identical
        # either way on a document that parses, so the only thing that can
        # notice is a source this process cannot read.
        "save: copy in the coordinator instead of the worker",
        "src/save.rs",
        "    let staged = stage(out, |writing| {\n        staged_rewrite(\n            rewriter,\n"
        "            &mut reading,\n            len,\n            writing,\n            plan,\n            Job::Save,",
        "    let staged = stage(out, |writing| {\n        staged_rewrite(\n            &Here,\n"
        "            &mut reading,\n            len,\n            writing,\n            plan,\n            Job::Save,",
        "the_coordinator_does_not_parse_the_document_it_copies",
    ),
    Mutation(
        # The same edit on the split, which is a separate call site and needs a
        # separate mutation: one worker per part, and nothing about the copy's
        # call site says anything about this one.
        "save: split in the coordinator instead of the worker",
        "src/save.rs",
        "            staged_rewrite(rewriter, &mut opened, len, into, plan, Job::Save, password)",
        "            staged_rewrite(&Here, &mut opened, len, into, plan, Job::Save, password)",
        "the_coordinator_does_not_parse_the_document_it_splits",
    ),
    Mutation(
        # Merge in the coordinator, which is what every merge did until
        # 2026-09-01 -- and this one parses more than the reader's own document:
        # every file they picked in a dialog is loaded too. The bytes are
        # identical either way on documents that parse, so only a source this
        # process cannot read can notice.
        "merge: merge in the coordinator instead of the worker",
        "src/save.rs",
        "        staged_merge(\n            merger,",
        "        staged_merge(\n            &Here,",
        "the_coordinator_does_not_parse_the_documents_it_merges",
    ),
    Mutation(
        # Start every incoming document at the beginning of the buffer. Each one
        # then reads the first file's bytes under its own name and length -- the
        # page count is plausible, every span is inside the buffer, and the
        # result is a document. Only an assertion that reads the BYTES each span
        # points at can see it.
        #
        # `at: 0` rather than something cleverer, and that is a lesson: the first
        # version read `incoming.first()` inside the `push` that gives `incoming`
        # its element type, so the mutated file did not compile and the harness
        # reported `no summary line` -- which reads exactly like a drifted
        # anchor. A mutation has to be a program.
        "merge: start every incoming document at the beginning of the buffer",
        "src/save.rs",
        "        incoming.push(Incoming {\n            at,",
        "        incoming.push(Incoming {\n            at: 0,",
        "the_coordinator_does_not_parse_the_documents_it_merges",
    ),
    Mutation(
        # Report the page count this process guessed rather than the one the
        # writer answered. Counting it here would mean parsing the merged
        # document, which is the parse that moved -- so the only honest source is
        # the reply.
        "merge: report a page count of this process's own",
        "src/save.rs",
        "        .map(|counted| pages = counted)",
        "        .map(|_| pages = 0)",
        "the_coordinator_does_not_parse_the_documents_it_merges",
    ),
    Mutation(
        # Build the page range in the coordinator, which is what every print of
        # a typed range did until 2026-09-01 -- and which residual risk 18 never
        # listed, because it names the paths that *write* a document and this
        # one writes nothing. The bytes are identical either way on a document
        # that parses, so only a source this process cannot read can notice.
        "save: build a page-range print job in the coordinator instead of the worker",
        "src/save.rs",
        "        staged_range(rewriter, reading, len, into, job)",
        "        staged_range(&Here, reading, len, into, job)",
        "the_coordinator_does_not_parse_the_document_it_prints_a_range_of",
    ),
    Mutation(
        # Send the writer every page instead of the range the reader typed. The
        # job is well formed, the scratch file is cleaned up, the length checks
        # out and the reader gets a document -- the whole of it. Only an
        # assertion that reads what CROSSED can see this, which is why the test
        # named below records the job the writer was handed rather than only the
        # bytes it gave back.
        "save: print every page instead of the range that was asked for",
        "src/save.rs",
        "        staged_range(rewriter, reading, len, into, job)",
        "        staged_range(\n            rewriter,\n            reading,\n            len,\n            into,\n            &crate::print::Job { pages: crate::print::Pages::All, turns: job.turns },\n        )",
        "the_coordinator_does_not_parse_the_document_it_prints_a_range_of",
    ),
    Mutation(
        # Write every part of a split through one plan. The count of parts is
        # right, the files all exist, and each of them is the first part -- so a
        # check that only counts files cannot see it.
        "save: write every part of a split from the first plan",
        "src/save.rs",
        "    for (plan, target) in plans.iter().zip(&targets) {",
        "    for (plan, target) in std::iter::repeat(&plans[0]).zip(&targets) {",
        "a_split_writes_each_page_once_and_in_order",
    ),
    Mutation(
        # Trust the length the writer reported. The bytes never reach this
        # process, so this comparison is the whole of what stands between a
        # short write in another process and a rename over the reader's only
        # copy.
        "save: trust the rewrite's own byte count",
        "src/save.rs",
        "    if landed != wrote as u64 {",
        "    if false {",
        "a_rewriter_that_overstates_what_it_wrote_is_refused",
    ),
    Mutation(
        # Ask the writer about the staged file rather than the source. Under
        # `Here` the number is a capacity hint that changes no answer; a worker
        # maps exactly this many bytes of the document it is rewriting.
        "save: ask the rewrite about the wrong file's length",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = rewriter.write(source, 0, out, plan, job, password, inputs)?;",
        "the_rewrite_is_asked_for_the_length_and_the_password",
    ),
    Mutation(
        # Drop the password on the way to the rewrite. `lopdf` parses no objects
        # at all for a document it cannot authenticate, so an encrypted save
        # would rewrite to an empty document rather than refusing.
        "save: rewrite without the reader's password",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = rewriter.write(source, len, out, plan, job, None, inputs)?;",
        "the_rewrite_is_asked_for_the_length_and_the_password",
    ),
    Mutation(
        # Keep the partial file when the write refuses. It is a copy of the
        # reader's document, possibly a truncated one, left in their directory
        # under a name they did not choose.
        "save: leave the staging file behind on a refusal",
        "src/save.rs",
        "            let _ = std::fs::remove_file(&partial);\n            return Err(why);",
        "            return Err(why);",
        "a_rewriter_that_refuses_says_so_without_a_disk_error_in_front_of_it",
    ),
    Mutation(
        # Flatten the refusal to "this cannot be reloaded". The sentence still
        # reaches the reader and the one action that answers it does not.
        "proto: drop whether a refusal is answerable by reloading",
        "src/worker_proto.rs",
        "            changed: why.changed,",
        "            changed: false,",
        "a_refusal_carries_whether_reloading_would_answer_it",
    ),
    Mutation(
        # Parse the written file in the coordinator again, which is the code
        # this replaced. Nothing about the save's outcome changes on a good
        # file -- both readers agree wherever both answer -- so the only thing
        # that can notice is a test asserting WHO was asked.
        "save: verify the append in the coordinator instead of the worker",
        "src/save.rs",
        "    match reread.pages(file, expected, password) {",
        "    match Here.pages(file, expected, password) {",
        "the_coordinator_does_not_parse_the_file_it_wrote",
    ),
    Mutation(
        # Ask about the update alone rather than the whole file. Harmless under
        # `Here`, where the number is a capacity hint that costs an allocation
        # and changes no answer; a worker maps exactly this many bytes, so it
        # would verify a prefix of the file it is meant to be checking.
        "save: ask the re-read about the update rather than the file",
        "src/save.rs",
        "    let expected = usize::try_from(appended.was).unwrap_or(0) + appended.update.len();",
        "    let expected = appended.update.len();",
        "the_re_read_is_asked_for_the_length_the_save_produced",
    ),
    Mutation(
        # Drop the password on the way to the re-read. `lopdf` parses no objects
        # at all for a document it cannot authenticate, so an encrypted append
        # is verified as having zero pages and a correct save is rolled back --
        # a refusal rather than a corruption, which is the safe direction and
        # still wrong.
        "save: re-read an encrypted append without its password",
        "src/save.rs",
        "            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    .map(|after| after.get_pages().len())",
        "            password: None,\n            ..Default::default()\n        },\n    )\n    .map(|after| after.get_pages().len())",
        "an_encrypted_document_can_be_appended_to_and_stays_encrypted",
    ),
    Mutation(
        # Classify a filter chain by its FIRST entry. `/Filter` is applied in
        # decoding order, so the last entry is the one that produces the
        # content: `[/ASCII85Decode /DCTDecode]` is an ASCII-armoured JPEG.
        # Reading the first calls it scannable, and the byte scan is then handed
        # a decoded JPEG to look for words in.
        "verify: classify a filter chain by its first entry",
        "src/verify.rs",
        "    let Some(last) = filters.last() else {",
        "    let Some(last) = filters.first() else {",
        "an_armoured_image_is_still_an_image",
    ),
    Mutation(
        # Stop recognising raster carriers at all. They then fall through to the
        # unrecognised arm, which is blind rather than deferred -- so the report
        # says no instrument can help where OCR would, and every scanned
        # document becomes uncertifiable for the wrong reason.
        "verify: stop recognising a raster carrier",
        "src/verify.rs",
        "    if IMAGE.contains(last) {\n        return Carrier::Image { filter: name() };\n    }",
        "    if false {\n        return Carrier::Image { filter: name() };\n    }",
        "the_raster_filters_are_deferred_to_a_different_instrument",
    ),
    Mutation(
        # Let a picture nobody read certify. THE one that would ship a lie: a
        # scanned document is nothing but image carriers, so a `deferred` list
        # that did not withhold the verdict would hand a reader the word
        # "verified" for a file where nothing read the only thing in it.
        "verify: certify a document whose pictures nobody read",
        "src/verify.rs",
        "        why.extend(self.deferred.iter().cloned());",
        "        // why.extend(self.deferred.iter().cloned());",
        "an_image_carrier_does_not_certify",
    ),
    Mutation(
        # Forget which filters are merely missing a decoder here. They become
        # "unrecognised", which is the deny-by-default arm and still withholds
        # certification -- so the verdict is unchanged and only the REASON is
        # wrong, telling a reader nobody has looked at a filter the format has
        # always had.
        "verify: conflate a missing decoder with an unknown filter",
        "src/verify.rs",
        '    const KNOWN: &[&[u8]] = &[b"ASCIIHexDecode", b"RunLengthDecode", b"Crypt"];',
        "    const KNOWN: &[&[u8]] = &[];",
        "a_filter_we_cannot_decode_is_blind_rather_than_deferred",
    ),
    Mutation(
        # Take the wrap away. Rust's `%` keeps the dividend's sign, so a page
        # written `/Rotate -90` comes back as a quarter-turn count of 255 -- and
        # the cast is what makes it silent, since `as u8` does not panic on a
        # negative in either profile.
        "pagetree: drop the wrap that makes a negative rotation a turn count",
        "src/pagetree.rs",
        "    (((degrees / 90) % 4 + 4) % 4) as u8",
        "    ((degrees / 90) % 4) as u8",
        "a_rotation_in_degrees_becomes_quarter_turns_clockwise",
    ),
    Mutation(
        # Round to the nearest quarter turn rather than truncating towards zero.
        # Every document that writes a multiple of 90 -- which is every document
        # that follows the specification -- is unaffected, and a page written
        # `/Rotate 45` is shown sideways.
        "pagetree: round a rotation to the nearest quarter turn",
        "src/pagetree.rs",
        "    (((degrees / 90) % 4 + 4) % 4) as u8",
        "    ((((degrees + 45) / 90) % 4 + 4) % 4) as u8",
        "a_rotation_that_is_not_a_multiple_of_ninety_truncates_towards_zero",
    ),
]

#: libtest prints `test <name> ... FAILED` per failure and a `test result:` line.
FAILED_TEST = re.compile(r"^test (\S+) \.\.\. FAILED$", re.M)
SUMMARY = re.compile(r"^test result: \w+\. \d+ passed; (\d+) failed", re.M)
#: `--list` prints `search::tests::a_match_is_found_where_it_is: test`.
LISTED_TEST = re.compile(r"^(\S+): test$", re.M)


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
        "        match chosen.get(&source) {\n            None => {",
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
        "src/save/marks.rs",
        # **The trailing `}` is what makes this unambiguous, and it was not
        # needed until the stamp arrived**: a stamp is bordered by the same
        # `re S` line, so the bare string now matches twice. The box's is the one
        # that ends its arm; the stamp's is followed by the word it draws.
        'out.push_str(&format!("{x} {y} {width} {height} re S\\n"));\n    }',
        'out.push_str(&format!("{x} {y} {width} {height} re f\\n"));\n    }',
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Stroke every kind. The check written for the box passes unchanged --
        # which is what its control is for.
        # Anchored on the wash's arm rather than on `re f`, which occurs twice:
        # the anchor gate requires exactly one occurrence, and a mutation that
        # could land in either of two places is one nobody can reason about.
        "save: stroke a wash rather than filling it",
        "src/save/marks.rs",
        '        let (x, y) = (quad[0], quad[1]);\n        let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);\n        out.push_str(&format!("{x} {y} {width} {height} re f',
        '        let (x, y) = (quad[0], quad[1]);\n        let (width, height) = (quad[2] - quad[0], quad[3] - quad[1]);\n        out.push_str(&format!("{x} {y} {width} {height} re S',
        "the_wash_and_the_rules_fill_rather_than_stroke",
    ),
    Mutation(
        # Write the text box's words as a UTF-8 literal instead of WinAnsi hex.
        # Every ASCII text box stays perfect and every German one draws `Ã¼` --
        # the encoding defect this would have shipped with, and the reason the
        # writer uses hex at all.
        "save: write a text box's words as UTF-8 rather than WinAnsi",
        "src/save/marks.rs",
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
        "src/save/marks.rs",
        "    if mark.kind == MarkKind::TextBox {",
        "    if false {",
        "a_text_box_carries_the_da_the_specification_requires_and_nothing_else_does",
    ),
    Mutation(
        # Give every mark a font in its appearance resources. Harmless-looking,
        # and it is the control for the `/Font` assertion: a check that only ever
        # says "the text box has one" passes equally if everything does.
        "save: put a font in every mark's appearance resources",
        "src/save/marks.rs",
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
        "src/save/marks.rs",
        "        MarkKind::Squiggly => Paint::Wave,",
        "        MarkKind::Squiggly => Paint::Line,",
        "a_squiggle_is_a_stroked_zigzag_in_a_band_taller_than_a_rule",
    ),
    Mutation(
        # Write the underline's subtype for a squiggle. Our own /AP draws the
        # right wave, so the mark looks correct in tpdf and is an underline to
        # everything else.
        "save: write /Underline for a squiggle",
        "src/save/marks.rs",
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
        "src/save/marks.rs",
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
        "src/save/marks.rs",
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
        "src/save/marks.rs",
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
        "src/save/marks.rs",
        '        MarkKind::Ellipse => b"Circle",',
        '        MarkKind::Ellipse => b"Square",',
        "each_kind_writes_its_own_subtype",
    ),
    Mutation(
        # Leave the ellipse's path open. The fourth arc ends where the first
        # began, so the shape is right and the join at three o'clock is a cap
        # instead -- a nick in a thick stroke rather than a missing curve.
        "save: leave an ellipse's path open rather than closing it",
        "src/save/marks.rs",
        '        out.push_str("h S',
        '        out.push_str("S',
        "an_ellipse_is_drawn_as_four_curves_and_not_as_a_rectangle",
    ),
    Mutation(
        # Stroke on the quad's own edge. Half of every side falls outside the
        # appearance stream's /BBox, which clips it -- so the box comes out with
        # hairline edges, which looks like a thin border and not like a bug.
        "save: stroke a box on its edge rather than inset by half the stroke",
        "src/save/marks.rs",
        "    let inset = OUTLINE_WIDTH / 2.0;",
        "    let inset = 0.0;",
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Never set the stroke colour. `rg` sets the fill's and does not imply
        # `RG`, so the box comes out black -- which reads as a colour that was
        # ignored rather than one that was never set.
        "save: set only the fill colour, so the stroke comes out black",
        "src/save/marks.rs",
        "{r} {g} {b} rg {r} {g} {b} RG {width} w {joins}",
        "{r} {g} {b} rg {width} w {joins}",
        "a_box_is_stroked_on_a_path_inset_by_half_its_own_width",
    ),
    Mutation(
        # Write /QuadPoints on every kind, as the writer did before a mark
        # existed that must not carry them. Most readers ignore an unlisted key
        # and one day something does not.
        "save: write text-markup quads on a box and a comment too",
        "src/save/marks.rs",
        "    if is_text_markup(mark.kind) {",
        "    if true {",
        "a_comment_carries_no_text_markup_keys_and_the_others_do",
    ),
    Mutation(
        # Give a box no appearance stream, as a comment correctly gets none.
        # Nothing synthesises a rectangle, so the annotation is in the file,
        # findable, removable -- and invisible.
        "save: leave a box's appearance to the reader, as a comment's is",
        "src/save/marks.rs",
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
        #
        # Aimed at `forget_mark` since 2026-08-24, where it used to name the
        # `Unannotate` arm. The two arms that end a mark --- removing the mark,
        # and deleting the page it sits on --- were separate cleanups doing
        # different amounts of work, and this mutation could only reach the one
        # that did more. One helper is one anchor, and it now reaches both.
        "docmodel: leave a removed mark's colour behind",
        "src/docmodel.rs",
        "        self.inks.remove(&mark);\n        self.colors.remove(&mark);",
        "        self.inks.remove(&mark);",
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

# --- printing an encrypted document ---------------------------------------
MUTATIONS += [
    Mutation(
        # Reserialise an encrypted document to print a selection of it. `lopdf`
        # writes every object in the clear, so the job -- which reaches a
        # printer, the platform's own PDF reader, and Print to PDF -- is a
        # decrypted copy of the reader's document.
        "print: build a selection from an encrypted document",
        "src/print.rs",
        "    if doc.was_encrypted() || doc.is_encrypted() {",
        "    if false {",
        "an_encrypted_document_is_printed_whole_or_refused",
    ),
    Mutation(
        # Refuse every document. The control: without it, a guard that never let
        # anything through would satisfy every refusal the test above asserts.
        "print: refuse every document as encrypted",
        "src/print.rs",
        "    if doc.was_encrypted() || doc.is_encrypted() {",
        "    if true {",
        "an_unencrypted_document_still_prints_a_selection",
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
        "    let encryption = read_encryption(document).or_else(|| encryption_from_state(document));",
        "    let encryption: Option<Encryption> = None;",
        "a_document_that_needs_a_password_says_so_rather_than_reporting_nothing",
    ),
    Mutation(
        # Drop the second route and keep the first. The trailer answers only for
        # a document nothing unlocked, so this reports every unprompted
        # permission-restricted file -- the commonest encrypted PDF there is --
        # as carrying no encryption at all.
        "docinfo: read the encryption only from the trailer",
        "src/docinfo.rs",
        "    let encryption = read_encryption(document).or_else(|| encryption_from_state(document));",
        "    let encryption = read_encryption(document);",
        "an_encrypted_document_reports_its_encryption_either_way",
    ),
    Mutation(
        # Report every document as encrypted. The control for the mutation
        # above: without it, a route that answered `Some` unconditionally would
        # satisfy every assertion the encrypted fixtures make.
        "docinfo: report an unencrypted document as encrypted",
        "src/docinfo.rs",
        "    let state = document.encryption_state.as_ref()?;",
        "    let Some(state) = document.encryption_state.as_ref() else {\n        return Some(Encryption::default());\n    };",
        "a_document_with_no_encryption_reports_none",
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
        "        out.covers_whole_file = end == Some(size) && numbers.first() == Some(&0);",
        "        out.covers_whole_file = end == Some(size);",
        "a_range_that_skips_the_start_of_the_file_is_not_whole_coverage",
    ),
    Mutation(
        # Count the container as an append. This is the defect the field was
        # added to fix, wearing the shape it had before: `size -
        # covered_bytes` is the uncovered total, and on a real DocuSign
        # contract 65,536 of its 74,637 bytes are the `/Contents` hex string
        # that no signature can cover. The panel then leads with 73 KB where
        # the honest number is 9 KB.
        # Tag the reply enum internally rather than adjacently. This is not a
        # style choice being reverted: serde cannot merge a tag into a payload
        # that is not a map, so `Reply::Content(Option<[f64; 4]>)` fails to
        # serialise AT RUNTIME with "cannot serialize tagged newtype variant
        # containing an optional". Content is the crop tool's ink bounds, so a
        # reader would meet it as a worker that cannot answer. Found this way,
        # before the code ran once.
        "proto: tag the reply enum internally, which cannot carry a bare payload",
        "src/worker_proto.rs",
        '#[serde(tag = "reply", content = "value", rename_all = "kebab-case")]',
        '#[serde(tag = "reply", rename_all = "kebab-case")]',
        "every_reply_variant_survives_the_wire_as_itself",
    ),
    Mutation(
        # Drop the tag entirely. `Content(Option<[f64; 4]>)` and
        # `CropBox([f32; 4])` are then the same four numbers on the wire, so serde
        # takes the first that fits and a crop box arrives as ink bounds --- the
        # right numbers carrying the wrong meaning, which is the failure mode the
        # untyped `serde_json::Value` had in a different costume.
        "proto: leave the reply enum untagged, so two payloads of four numbers collide",
        "src/worker_proto.rs",
        '#[serde(tag = "reply", content = "value", rename_all = "kebab-case")]',
        "#[serde(untagged)]",
        "every_reply_variant_survives_the_wire_as_itself",
    ),
    Mutation(
        # Send a payload on a reply that says it failed. Every caller checks `ok`
        # before reading the payload, so this is the reply that carries an answer
        # nobody will look at -- and the parent reports the worker's `error`
        # string, which is empty.
        "proto: mark a payload-bearing reply as a failure",
        "src/worker_proto.rs",
        "    pub fn reply(reply: Reply) -> Self {\n        Self {\n            ok: true,",
        "    pub fn reply(reply: Reply) -> Self {\n        Self {\n            ok: false,",
        "a_response_carries_its_reply_through_the_framing",
    ),
    Mutation(
        # Put a portable spike back on the macOS library directory. `lib/` exists
        # on Windows and holds the IMPORT library, so the path resolves, the
        # directory check passes and the bind fails much later pointing at
        # something that is right there. PDFIUM_SUBDIR's own note says this had
        # been rediscovered three times and named the grep that checks it; on
        # 2026-08-25 four probes had drifted back, which is what a rule with
        # nothing enforcing it does.
        # Count every id swept rather than every document actually released.
        # `close` leaves holes rather than removing entries, so a table of holes
        # is the ordinary shape here -- and this makes a fresh start claim it
        # released documents belonging to a webview that never existed. The one
        # line this count produces means "a page reloaded", which is exactly what
        # the miscount turns into a lie.
        #
        # A sibling mutation belongs here and is deliberately absent: dropping the
        # `slot.is_some()` filter that builds the id list. Running it showed it
        # SURVIVES, and correctly -- the filter keeps `close` from being asked
        # about holes, and `close` refuses a hole anyway, so the count is held
        # here rather than there. A variant, not a gap; recorded so the next
        # person to notice finds out it was measured.
        "workers: sweep the documents without closing any of them",
        "src/workers.rs",
        "            if self.close(doc).is_ok() {",
        "            if false {",
        "releasing_everything_empties_every_slot_and_reports_how_many",
    ),
    Mutation(
        "lib: hardcode the macOS library directory in a portable spike",
        "examples/crop_probe.rs",
        'PathBuf::from("vendor/pdfium").join(tpdf_lib::PDFIUM_SUBDIR)',
        'PathBuf::from("vendor/pdfium/lib")',
        "only_the_macos_spikes_hardcode_the_library_directory",
    ),
    Mutation(
        # Read the appendix without requiring the signed prefix to parse. With
        # no prefix to compare against, every object in the document reads as
        # ADDED -- so an ordinary validation-data append is reported as the
        # document having been rewritten from nothing, which is the alarming
        # direction to be wrong in. The guard is "both parses or neither".
        "docinfo: describe an appendix whose signed prefix would not parse",
        "src/docinfo.rs",
        "    let (Ok(whole), Ok(signed)) = (load(bytes), load(&bytes[..end])) else {",
        "    let signed = load(&bytes[..end]).unwrap_or_default();\n    let Ok(whole) = load(bytes) else {",
        "an_appendix_whose_signed_prefix_is_unparseable_is_reported_as_unread",
    ),
    Mutation(
        # Count a page like any other object. `pages_touched` is the one number
        # separating an append that rewrote something a reader sees from one that
        # did not, and without it the two rows read identically.
        "docinfo: stop counting the pages an appendix rewrote",
        "src/docinfo.rs",
        '        if kind == "Page" {',
        '        if kind == "Pages" {',
        "a_second_signature_is_reported_as_a_signature_rather_than_as_a_size",
    ),
    Mutation(
        # Slice past the end of the file. A `/ByteRange` written for bytes a
        # truncation removed gives an `end` beyond the buffer, and this bound is
        # what stands between that and a panic inside the worker.
        "docinfo: slice an appendix on an end past the file",
        "src/docinfo.rs",
        "    if end == 0 || end >= bytes.len() {",
        "    if end == 0 {",
        "an_appendix_outside_the_file_is_refused_rather_than_sliced",
    ),
    Mutation(
        "docinfo: report every uncovered byte as an append",
        "src/docinfo.rs",
        "        out.appended_bytes = end.map_or(0, |end| size.saturating_sub(end));",
        "        out.appended_bytes = size.saturating_sub(out.covered_bytes);",
        "what_was_appended_after_signing_is_counted_apart_from_the_container",
    ),
    Mutation(
        # Subtract without saturating. A range written for bytes a truncation
        # removed claims to end past the file, and an unsigned wrap then tells
        # the reader 18 exabytes were appended -- which is not a smaller error
        # than the one above, it is a different one.
        "docinfo: let a range past the end of the file wrap the append count",
        "src/docinfo.rs",
        "        out.appended_bytes = end.map_or(0, |end| size.saturating_sub(end));",
        "        out.appended_bytes = end.map_or(0, |end| size.wrapping_sub(end));",
        "a_range_reaching_past_the_end_of_the_file_reports_no_append",
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
        """    if raw.iter().all(|byte| *byte == 0) {
        return None;
    }""",
        """    if raw.iter().all(|byte| *byte == 0) {
        *unread += 1;
        return None;
    }""",
        "an_untouched_placeholder_is_absent_rather_than_unread",
    ),
    Mutation(
        # Hand the padded blob to the decoder. The trailing zeros are not DER and
        # a decoder is entitled to reject them, so every certificate would read
        # as unparseable --- the failure that looks like a broken dependency.
        "docinfo: hand the parser the reserved span exactly as it arrived",
        "src/docinfo.rs",
        "    let Some(blob) = ber::to_definite_length(raw) else {",
        "    let Some(blob) = Some(raw.to_vec()) else {",
        "each_signed_fixture_carries_its_own_certificate",
    ),
    Mutation(
        # Drop the size bound, so an attacker-chosen megabyte of nested DER goes
        # to the parser. The blob is the most attacker-controlled thing in the
        # document and this is the only thing standing between it and a parser.
        "docinfo: parse a certificate blob of any size",
        "src/docinfo.rs",
        """    if blob.len() > bound {
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
        # Re-aimed 2026-09-06: the traversal moved to `fields.rs`, where the
        # reversal is what `Order::Document` means. The test that catches it is
        # still `docinfo`'s, because `redact` walks the same tree in the other
        # order on purpose and cannot see this.
        "src/fields.rs",
        """    if bounds.order == Order::Document {
        queue.reverse();
    }""",
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
        # Re-aimed 2026-09-06 with the traversal. Emptying the push loop is
        # what "walk past a node's kids" is now spelled as, and it is the same
        # edit: a signature under a `/Kids` node is never reached.
        "src/fields.rs",
        """        for kid in pushed {
            queue.push((Entry::Inline(kid), depth + 1, name.clone()));
        }""",
        """        for kid in pushed {
            let _ = (kid, depth, &name);
        }""",
        "a_signature_field_under_kids_is_found_rather_than_walked_past",
    ),
    Mutation(
        # Report the leaf's own `/T` rather than the fully qualified name. Every
        # fixture but one has unique leaf names, so this is right about all of
        # them and wrong about what it means: `/T` is unique among siblings only.
        "docinfo: name a field by its own /T alone",
        # Re-aimed 2026-09-06 with the traversal.
        "src/fields.rs",
        "            (true, Some(field)) => qualified_name(&prefix, &partial_name(doc, field)),",
        "            (true, Some(field)) => partial_name(doc, field),",
        "two_fields_with_the_same_leaf_name_are_told_apart_by_the_groups_above_them",
    ),
    Mutation(
        # Give an unnamed node a level of the name anyway, which puts an empty
        # component in the middle -- `top..Signature1`, a string no other reader
        # shows -- and gives a wholly unnamed chain the name ".".
        # Re-aimed 2026-09-06: `qualified_name` moved to `fields.rs` with the
        # traversal that calls it.
        "fields: make an unnamed field node a level of the name",
        "src/fields.rs",
        "        (_, true) => prefix.to_string(),",
        "        (_, true) => format!(\"{prefix}.\"),",
        "a_node_with_no_name_of_its_own_is_not_a_level_of_the_name",
    ),
    Mutation(
        # Hand each kid an empty prefix instead of its parent's name, so the
        # ancestry is walked and then thrown away one level at a time.
        "docinfo: forget the ancestry when descending into kids",
        # Re-aimed 2026-09-06 with the traversal.
        "src/fields.rs",
        "            queue.push((Entry::Inline(kid), depth + 1, name.clone()));",
        "            queue.push((Entry::Inline(kid), depth + 1, String::new()));",
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
                    append(buffer, &text, &mut out.unread);
                }
            }
            Ok(Event::GeneralRef(reference)) => {""",
        """                if let Some((_, _, buffer)) = &mut pending {
                    if buffer.is_empty() {
                        append(buffer, &text, &mut out.unread);
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
        "        xmp: catalog.and_then(|c| read_xmp(document, c)),",
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
        .decompressed_content_with_limit(crate::xmp::MAX_PACKET)
        .unwrap_or_else(|_| stream.content.clone());""",
        "    let packet = stream.content.clone();",
        "a_conformance_claim_in_the_metadata_stream_reaches_the_readout",
    ),
    Mutation(
        # Ask for the packet unbounded, which is what this was until 2026-09-01:
        # a 2,289-byte document declaring a 1 GiB /Metadata packet then costs the
        # worker 1,081 MB. The answer does not change -- xmp::scan discards
        # anything over MAX_PACKET either way -- so only a reading about COST can
        # catch this, which is why the test named here asserts on the packet
        # length reported and not on `unread`.
        "docinfo: ask for a metadata packet with no bound on its size",
        "src/docinfo.rs",
        "        .decompressed_content_with_limit(crate::xmp::MAX_PACKET)",
        "        .decompressed_content()",
        "a_metadata_packet_over_the_packet_bound_is_never_inflated",
    ),
    Mutation(
        # Bound at MAX_DECODE, the constant already in scope and the obvious
        # repair -- and 64x larger than any packet xmp::scan will look at. The
        # fixture is sized one byte past MAX_PACKET precisely so that this is
        # distinguishable from the bound above it.
        "docinfo: bound the metadata packet at the general decode ceiling",
        "src/docinfo.rs",
        "        .decompressed_content_with_limit(crate::xmp::MAX_PACKET)",
        "        .decompressed_content_with_limit(MAX_DECODE)",
        "a_metadata_packet_over_the_packet_bound_is_never_inflated",
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
        # Re-aimed 2026-09-06 with the traversal. `false` rather than a larger
        # number, because the bound is now a value the caller passes and the
        # mutation has to remove the test of it rather than the number in it.
        "src/fields.rs",
        "        if bounds.depth.is_some_and(|max| depth >= max) {",
        "        if false {",
        "a_field_tree_is_walked_to_a_bounded_depth_and_the_refusal_is_counted",
    ),
    Mutation(
        # Refuse a too-deep tree in silence. A signature dropped without a word
        # reads as a document that has none, which is the reassuring direction.
        "docinfo: drop a too-deep field node without counting it",
        # Re-aimed 2026-09-06 with the traversal. The bound still fires; only
        # the count of what it cut goes, which is the half that reads as a form
        # with nothing more in it.
        "src/fields.rs",
        "            cut.too_deep += 1;\n            continue;",
        "            continue;",
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


# --- indefinite lengths, and where a signature blob ends -------------------
MUTATIONS += [
    Mutation(
        # Read the coverage from the first range pair only. On a signature the
        # answer is then the bytes before the blob and not the ones after it,
        # which is most of a document -- and the check that sees it derives the
        # same quantity from where the /Contents hex sits, by a route this code
        # does not take. That derivation replaced a number transcribed from
        # `qpdf --json`, which was a real third reader and a fact about one
        # machine's pyhanko: both CI runners build a file 31 bytes smaller.
        "docinfo: count only the first pair of a signature's byte range",
        "src/docinfo.rs",
        """        out.covered_bytes = numbers
            .chunks_exact(2)
            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())
            .fold(0_u64, u64::saturating_add);""",
        """        out.covered_bytes = numbers
            .chunks_exact(2)
            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())
            .next()
            .unwrap_or_default();""",
        "the_docmdp_levels_of_four_documents_another_program_signed",
    ),
    Mutation(
        # Stop counting a blob whose structure will not walk. The two readings
        # are opposite --- absent is about the file, unread is about tpdf ---
        # and this is the direction that makes tpdf's own failure look like the
        # document's silence. It is the *walk's* half of that distinction; the
        # parser's half is a mutation of its own, because a blob that reaches
        # one never reaches the other.
        "docinfo: report a blob that will not walk as a signature with no certificate",
        "src/docinfo.rs",
        """    let Some(blob) = ber::to_definite_length(raw) else {
        *unread += 1;
        return None;
    };""",
        """    let Some(blob) = ber::to_definite_length(raw) else {
        return None;
    };""",
        "a_blob_whose_structure_will_not_walk_is_reported_as_unread",
    ),
    Mutation(
        # Accept the indefinite form on a primitive value. There is no marker to
        # read such a value up to, so the walk would run into whatever follows
        # and call it content --- X.690 forbids it for exactly that reason.
        "ber: let an indefinite length end a primitive value too",
        "src/ber.rs",
        """        if !constructed {
            return None;
        }""",
        "",
        "an_indefinite_primitive_is_refused",
    ),
    Mutation(
        # Trust a child to stay inside the length its parent declared. A child
        # is bounded by the input, not by its parent, so one may decode
        # perfectly and still overrun --- and the parent's length would then
        # describe fewer bytes than the parent holds.
        "ber: trust a child to stay inside the length its parent declared",
        "src/ber.rs",
        """                if cursor > end {
                    return None;
                }""",
        "",
        "a_child_that_overruns_its_parent_is_refused",
    ),
    Mutation(
        # Delete the nesting bound. This is the only copy of it: `emit` runs
        # after `measure` returned and deliberately carries no second guard, so
        # a mutation of that one would have survived.
        "ber: follow nesting to whatever depth the blob claims",
        "src/ber.rs",
        """    if depth > MAX_DEPTH {
        return None;
    }
    let head = header(raw, at)?;
    let body = at.checked_add(head.tag)?.checked_add(head.length)?;
    let mut content = 0usize;""",
        """    let head = header(raw, at)?;
    let body = at.checked_add(head.tag)?.checked_add(head.length)?;
    let mut content = 0usize;""",
        "nesting_is_followed_to_the_bound_and_refused_past_it",
    ),
    Mutation(
        # Bound nesting where no real signature fits. The unit test names the
        # constant, so it cannot see this at all; the fixture check can, because
        # a timestamped signature nests a chain inside a token inside a signer
        # and reaches about twenty-five.
        "ber: bound nesting at eight levels",
        "src/ber.rs",
        "pub const MAX_DEPTH: usize = 64;",
        "pub const MAX_DEPTH: usize = 8;",
        "a_der_signature_blob_is_returned_byte_for_byte",
    ),
    Mutation(
        # Write every length in the long form. Legal BER, not DER, and the
        # identity property that lets this sit in front of every signature
        # rather than only the ones that need it goes with it.
        "ber: write a length in the long form whatever its size",
        "src/ber.rs",
        "    if value < 0x80 {\n        field[0] = value as u8;",
        "    if value < 0x00 {\n        field[0] = value as u8;",
        "a_definite_encoding_comes_back_unchanged",
    ),
    Mutation(
        # Accept a length field of any width. `0xff` is reserved and anything
        # past four bytes is a length no blob here can have, so this is a walk
        # taking an attacker's word for how far to read.
        "ber: accept a length field of any width",
        "src/ber.rs",
        """    if count > MAX_LENGTH_BYTES {
        return None;
    }""",
        "",
        "a_length_field_past_the_bound_is_refused",
    ),
    Mutation(
        # Keep whatever follows the first value. That is the reserved padding,
        # and handing it to a decoder is what the scan this module replaced was
        # for --- a decoder is entitled to reject it.
        "ber: keep the bytes that follow the first value",
        "src/ber.rs",
        "    emit(raw, 0, 0, &mut out)?;",
        """    emit(raw, 0, 0, &mut out)?;
    out.extend_from_slice(&raw[span.input..]);""",
        "padding_after_the_first_value_is_dropped",
    ),
    Mutation(
        # Write the input length where the content length belongs. On a value
        # that was already definite the two agree, which is what makes this
        # survive a reading; on an indefinite one the header then claims the
        # marker bytes as content.
        "ber: write the length the input used rather than the one being written",
        "src/ber.rs",
        "    let (field, length) = length_field(span.content);",
        "    let (field, length) = length_field(span.input);",
        "an_indefinite_length_becomes_a_definite_one",
    ),
]




#: Moving a mark: the command a reader's drag produces.
MUTATIONS += [
    Mutation(
        # Move the rectangle and leave the strokes where they are. Every kind but
        # ink has no strokes, so this is invisible on five of the six -- and the
        # saved file then draws the line in the old place inside a `/Rect` in the
        # new one, which is the underline defect's shape once more.
        "displace: move a mark's rectangle and leave its strokes behind",
        "src/docmodel.rs",
        """        let strokes = self
            .strokes_of(mark)
            .iter()
            .map(|stroke| Stroke {
                points: stroke
                    .points
                    .iter()
                    .map(|p| Point {
                        x: p.x + dx,
                        y: p.y + dy,
                    })
                    .collect(),
            })
            .collect();""",
        "        let strokes = self.strokes_of(mark).to_vec();",
        "moving_a_mark_carries_its_rectangle_and_its_strokes_together",
    ),
    Mutation(
        # Move one corner of each rectangle rather than both, which is a resize
        # wearing a move's name -- the failure the delta-not-a-geometry argument
        # in `Doc::displace` is about, arriving from inside rather than over the
        # wire.
        "displace: move a rectangle's top-left corner and not its bottom-right",
        "src/docmodel.rs",
        """                right: q.right + dx,
                bottom: q.bottom + dy,""",
        """                right: q.right,
                bottom: q.bottom,""",
        "moving_a_mark_changes_where_it_is_and_nothing_else_about_it",
    ),
    Mutation(
        # Refuse anything that is not ink, which is `reink`'s rule next door and
        # is the wrong one here: geometry is geometry, and which kinds a reader
        # is offered the drag on is a product rule one layer up.
        "displace: refuse to move a mark that has no strokes",
        "src/docmodel.rs",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n        self.now.live_mark(mark)?;",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n"
        "        self.now.live_mark(mark)?;\n"
        "        let kind = self.mark(mark).map_or(MarkKind::Highlight, |m| m.kind);\n"
        "        if kind != MarkKind::Ink {\n"
        "            return Err(Refusal::ShapeMismatch(kind));\n"
        "        }",
        "every_kind_can_be_moved_including_the_one_that_cannot_be_erased",
    ),
    Mutation(
        # Write the new geometry into the body instead of journalling it. Every
        # visible behaviour is identical until an undo, which then puts the mark
        # back nowhere -- the whole reason a move is a command.
        "displace: change where a mark is without journalling it",
        "src/docmodel.rs",
        "            .collect();\n"
        "        let ink = self.issue_ink(Ink { strokes, quads });\n"
        "        self.apply(Command::Reink { mark, ink })\n"
        "    }\n"
        "\n"
        "    /// Resizes a signature",
        "            .collect();\n"
        "        if let Some(body) = self.marks.get_mut(&mark) {\n"
        "            body.quads = quads;\n"
        "            body.strokes = strokes;\n"
        "        }\n"
        "        Ok(())\n"
        "    }\n"
        "\n"
        "    /// Resizes a signature",
        "undoing_a_move_puts_the_mark_back_where_it_was",
    ),
    Mutation(
        # Spend a version before the liveness check, so a refused move leaves an
        # `Ink` body behind. Nothing a reader can see says so, which is why the
        # accounting observable exists.
        "displace: spend a version on a mark that is not there",
        "src/docmodel.rs",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n        self.now.live_mark(mark)?;",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n"
        "        self.issue_ink(Ink {\n"
        "            strokes: Vec::new(),\n"
        "            quads: Vec::new(),\n"
        "        });\n"
        "        self.now.live_mark(mark)?;",
        "moving_a_mark_that_is_not_there_is_refused_before_a_version_is_spent",
    ),
    Mutation(
        # Substitute zero for a non-finite offset, which is `channel`'s answer to
        # the same shape of input and is the wrong one here: a drag that silently
        # did nothing is indistinguishable from a viewer that has stopped working.
        "displace: take a non-finite offset as no offset at all",
        "src/edits.rs",
        """        if !dx.is_finite() || !dy.is_finite() {
            return Err(format!("a mark cannot be moved by ({dx}, {dy})"));
        }""",
        """        let dx = if dx.is_finite() { dx } else { 0.0 };
        let dy = if dy.is_finite() { dy } else { 0.0 };""",
        "a_move_by_a_non_finite_offset_is_refused_rather_than_ignored",
    ),
]


#: Printing what the reader sees: the crop, the marks, and the view rotation.
MUTATIONS += [
    Mutation(
        # Stop asking about the crop, which is the clause that was missing. A
        # cropped document then reports itself as the file on disk and the
        # printer is handed the uncropped original.
        "identity: read a cropped document as the file on disk",
        "src/edits.rs",
        "                theirs && turns % 4 == 0 && crop.is_none()",
        "                theirs && turns % 4 == 0 && (crop.is_none() || crop.is_some())",
        "a_cropped_document_is_not_the_file_on_disk",
    ),
    Mutation(
        # Never answer `Working`, which is the state printing was in: everything
        # that was not handed over byte for byte went through `build`, and
        # `build` has no marks and no crops.
        "print: send every job that is not the file through the range builder",
        "src/print.rs",
        """    if explicit || plan.is_none() {
        return Route::Range(job);
    }
    Route::Working""",
        "    Route::Range(job)",
        "a_cropped_or_marked_document_is_built_rather_than_handed_over",
    ),
    Mutation(
        # Drop the reader's own rotation on the working route. It SURVIVED when
        # first run: the route test asserts which producer is chosen and says
        # nothing about what it produces, and every rotation test beside it
        # drives the *other* producer. A reader viewing sideways would have
        # printed upright.
        "print: drop the view rotation from a job built by the save writer",
        "src/save.rs",
        "            Ok((slot, (page.turns % 4 + view % 4) % 4))",
        "            Ok((slot, page.turns))",
        "a_third_parser_sees_the_view_rotation_on_a_job_built_from_the_working_document",
    ),
    Mutation(
        # Let the view replace a page's own turn rather than composing with it.
        # This one SURVIVED too, and the fixture was the reason rather than the
        # assertion: with no page carrying an edit turn the two expressions are
        # the same number. The test rotates one page now.
        "print: let the view rotation replace a page's own instead of composing",
        "src/save.rs",
        "            Ok((slot, (page.turns % 4 + view % 4) % 4))",
        "            Ok((slot, view % 4))",
        "a_third_parser_sees_the_view_rotation_on_a_job_built_from_the_working_document",
    ),
]


#: Saving by appending an update section rather than rewriting the document.
MUTATIONS += [
    Mutation(
        # Call every plan with a mark appendable, so a deletion, a move, a turn
        # or a crop is written as an update section --- edits an update section
        # cannot express, and which spike 0.6 never put to any parser.
        # Re-aimed 2026-08-26, when a redaction clause joined the predicate, and
        # again 2026-08-29, when note edits did and it was renamed
        # `only_adds_marks` -> `is_appendable`. Twice in three days is the
        # argument for the anchor gate rather than against the predicate: both
        # times the mutation was still exactly right and pointed at nothing.
        "append: append any plan that carries a mark",
        "src/edits.rs",
        MUT_APPENDABLE,
        "        (!self.marks.is_empty() || !self.notes.is_empty())\n            && self.redactions.is_empty()\n            && self.discards.is_empty()",
        "a_plan_that_only_adds_marks_is_appended_and_anything_else_is_rewritten",
    ),
    Mutation(
        # Report the file as put back without truncating it. The reader is then
        # told their document is untouched while it carries a half-written
        # revision, which is worse than either honest outcome.
        "append: say the file was put back without cutting it back",
        "src/save.rs",
        "        match file.set_len(appended.was) {",
        "        match Ok::<(), std::io::Error>(()) {",
        "an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was",
    ),
    Mutation(
        # Append without checking the length. The update names byte offsets into
        # the previous revision, so writing it after any other length produces a
        # cross-reference pointing at the wrong bytes -- a file that opens and is
        # wrong, which is the worst of the three outcomes.
        #
        # Aimed at the comparison rather than at a call site, because since
        # 2026-08-22 both save paths reach it through `Fingerprint`: the length
        # half of `agrees_with_metadata` and of `agrees_with` is this one line.
        "append: write the update after whatever length the file now has",
        "src/fingerprint.rs",
        "        if meta.len() != self.len {",
        "        if false {",
        "an_append_to_a_file_that_changed_length_is_refused_and_writes_nothing",
    ),
    Mutation(
        # Take the fingerprint as advisory. `stage_in_place` refuses a changed
        # file and the two paths no longer share a function, so a refusal written
        # once is a refusal on one of them.
        "append: proceed over a file that changed since it was opened",
        "src/save.rs",
        "    let verified = opened_as.agrees_with(source).map_err(Refusal::changed)?;",
        "    let verified = opened_as\n"
        "        .agrees_with(source)\n"
        '        .unwrap_or_else(|_| crate::fingerprint::Fingerprint::of(source).expect("fp"));',
        "an_append_is_refused_when_the_file_changed_since_it_was_opened",
    ),
    Mutation(
        # Accept any file that parses. It SURVIVED when first run: the rollback
        # test plants a trailer pointing at nothing, so `load` errors and the
        # count is never compared. The test that reaches this arm builds a real
        # update section whose catalog names an empty page tree -- a file that
        # opens, and is empty.
        #
        # Re-aimed 2026-08-26 when the read-back moved behind `save::Reread`:
        # the comparison is the same comparison, and the parse it used to be
        # written beside is in a worker now, so the arm names a count rather
        # than a parsed document.
        "append: accept a saved file that parses, whatever it has lost",
        "src/save.rs",
        "        Ok(pages) if pages == appended.pages => {}",
        "        Ok(_) => {}",
        "an_append_that_parses_and_has_lost_pages_is_also_put_back",
    ),
    Mutation(
        # Rewrite the page dictionary even when its /Annots is its own object.
        # The update is then larger than the edit, which is what `docs/PLAN.md`
        # §5 records as the one document-shape dependency worth carrying -- and
        # here it also breaks the file, because the cloned page keeps a reference
        # to an array the update never brought across.
        "append: bring the page across rather than its /Annots array",
        "src/save.rs",
        """            AnnotsSite::ArrayObject(array) => incremental
                .opt_clone_object_to_new_document(*array)""",
        """            AnnotsSite::ArrayObject(_) => incremental
                .opt_clone_object_to_new_document(site.page)""",
        "a_third_parser_reads_an_appended_document",
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
    # release selects affected behaviour, and re-proving ninety-odd mutations
    # that could not have moved is an hour of somebody waiting.
    # The control run and the name cross-check below still run in full.
    # **Repeatable, and every value must match something.** It was a plain
    # string option until 2026-09-01, so argparse kept the LAST of several and
    # silently discarded the rest --- three `--only` flags selected one mutation
    # and the run ended `[OK] all 1 mutations caught by the test named for
    # them`, which is a confident pass for two mutations that never executed.
    # The existing empty-selection guard could not see it, because the selection
    # was not empty. So the guard is per filter: a value matching nothing is
    # refused by name, whether or not the others matched.
    parser.add_argument(
        "--only",
        action="append",
        default=[],
        metavar="TEXT",
        help="run mutations whose name contains this; repeatable",
    )
    # For the loop while a change is being made, where `--only` needs you to
    # know the mutation names and this needs you to know nothing. It selects the
    # mutations whose FILE the diff touched, which is sound as far as it goes:
    # every test these name is `#[cfg(test)]` in the file it mutates.
    #
    # **It does not go as far as the whole table, and the difference is not
    # scope but reach.** A change in `docmodel.rs` can decide what `save.rs`
    # does, so a mutation in an untouched file can stop being caught without
    # that file changing. Include affected callers when selecting release scope
    # (BUILD.md); the count left out is printed rather than implied.
    parser.add_argument(
        "--since",
        default="",
        metavar="REF",
        help="only mutations in files changed since REF (plus the working tree)",
    )
    # Written for the kill this table keeps meeting: it is measured in tens of
    # minutes, and a process that dies loses every verdict it had proved *and*
    # leaves its current mutation in the tree, because the restore below is in a
    # `finally` and a `finally` does not survive a kill. See
    # `mutation_resume.py` for what makes a stored verdict reusable, and note
    # that the recovery half runs whether or not this flag is passed.
    parser.add_argument(
        "--resume",
        action="store_true",
        help="reuse verdicts from an earlier run, if the tracked tree has not moved",
    )
    args = parser.parse_args()
    unmatched = [
        f
        for f in args.only
        if not any(f.lower() in m.name.lower() for m in MUTATIONS)
    ]
    if unmatched:
        for f in unmatched:
            print(f"[FAIL] no mutation matches {f!r}")
        print(
            "[FAIL] a filter that selects nothing is a filter that proved nothing, "
            "and the mutations the other filters did select do not make up for it"
        )
        return 1
    chosen = [
        m
        for m in MUTATIONS
        if not args.only or any(f.lower() in m.name.lower() for f in args.only)
    ]
    if args.since:
        # The prefix is a string, not `Path("src-tauri") / m.path`, and that is
        # the whole of what was wrong with this for as long as it existed: git
        # reports forward slashes on every platform, `str(WindowsPath(...))`
        # produces backslashes, and the set membership test therefore matched
        # nothing on Windows. Measured 2026-08-25 -- `--since HEAD~1` over a
        # commit that changed `docinfo.rs`, which 48 mutations aim at, selected
        # **0**. It refused rather than passing, because the guard below treats
        # an empty selection as "proved nothing"; so the flag was useless here
        # rather than misleading, which is the difference that guard makes.
        chosen, code = mutation_since.apply(chosen, args.since, "src-tauri/")
        if code:
            return code
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

    # Before the control run, and before the fingerprint: a tree still holding
    # somebody else's mutation is neither green nor the tree these verdicts
    # would be about.
    resume = mutation_resume.Resume("rust", reuse=args.resume)
    code, lines = resume.recover()
    for line in lines:
        print(line, flush=True)
    if code:
        return code
    for line in resume.open(chosen):
        print(line, flush=True)

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

    def tally(ok: bool) -> int:
        """What one mutation adds to `problems`: 0 when it was caught, 1 otherwise.

        One line, and it exists because a reused verdict and one taken just now
        reach `problems` by different routes. Written at both, the rule would be
        two copies of itself --- the shape this repository keeps finding drifted,
        and the one a mutation of either half survives.
        """
        return 0 if ok else 1

    def say(mutation, ok: bool, *lines: str) -> int:
        """Print one mutation's verdict and file it, in that order.

        Every outcome below goes through this, so a run that is killed a moment
        later has already recorded everything it decided. The return value is
        what the caller adds to `problems`, which keeps the two from drifting.
        """
        for line in lines:
            print(line, flush=True)
        resume.record(mutation, ok, "\n".join(lines))
        return tally(ok)

    problems = 0
    for mutation in chosen:
        stored = resume.done(mutation)
        if stored is not None:
            # Marked, because a reused verdict and one taken just now are
            # the same claim about the tree and not the same run --- and a
            # transcript that cannot tell them apart is where somebody
            # reads "all caught" for a table that was never executed here.
            print(f"{stored.line}   [reused]", flush=True)
            problems += tally(stored.ok)
            continue
        target = CRATE / mutation.path
        # Bytes, decoded explicitly. `read_text` uses the locale codec,
        # and on Windows that is cp1252, which does not merely mangle
        # this file -- it *cannot read it*: search.rs holds `İ` and `ﬁ`
        # for the case-folding tests, whose UTF-8 encodings contain the
        # byte 0x81, and cp1252 leaves 0x81 undefined. So this raised
        # UnicodeDecodeError on the first mutation and the harness never
        # ran here at all.
        #
        # The newlines are normalised for matching only, because the
        # anchors are written with "\n" and eight of them span lines.
        # The file's own convention goes back on the way out, and the
        # restore below is bytes, as docs/TRAPS.md requires.
        #
        # **This said "a Windows checkout is CRLF", and since 2026-08-26
        # it is not**: `.gitattributes` pins `* text=auto eol=lf`, so
        # every text file checks out LF on every platform. Kept anyway,
        # and not as ceremony -- a checkout is not the only way a file
        # gets written, and a tool that rewrites one in text mode on
        # Windows produces CRLF whatever git checked out. The branch is
        # two lines and its absence cost this harness every multi-line
        # anchor once already.
        clean = target.read_bytes()
        raw = clean.decode("utf-8")
        crlf = "\r\n" in raw
        source = raw.replace("\r\n", "\n") if crlf else raw
        if source.count(mutation.before) != 1:
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: its anchor appears "
                f"{source.count(mutation.before)} times, so the mutation is not the "
                "one described",
            )
            continue
        mutated = source.replace(mutation.before, mutation.after)
        if crlf:
            mutated = mutated.replace("\n", "\r\n")
        payload = mutated.encode("utf-8")
        # Applied and taken off again by `mutation_resume.py`, which holds
        # the backup that outlives this process and enforces the three rules
        # this harness paid for one at a time: the record is written before
        # the bytes, the restore writes bytes rather than moving or copying
        # a file over them, and the restored file is left newer than the
        # mutation so cargo does not go on serving it. `docs/TRAPS.md` has
        # each of them. The anchor check above happens before any of it, so
        # the only path under the `finally` is one where a mutation is
        # really on the tree.
        resume.apply(mutation, target, clean, payload)
        try:
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
            put_back, notes = resume.restore()
        # A restore that could not put the file back stops the run. Carrying
        # on would test every later mutation against a file still holding
        # this one, and the verdicts would be about a tree nobody described.
        if not put_back:
            for line in notes:
                print(line, flush=True)
            return 1

        if counted is None:
            # Almost always a compile error, which produces no failing-test
            # lines at all -- indistinguishable from a survivor without this.
            first = next(
                (line for line in out.splitlines() if line.startswith("error")), ""
            )
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: no summary line -- the run did not finish"
                + (f" ({first})" if first else ""),
            )
            continue
        if len(names) != counted:
            problems += say(
                mutation,
                False,
                f"[FAIL] {mutation.name}: {len(names)} failing test lines but the summary "
                f"says {counted} -- this harness cannot read its own output",
            )
            continue
        if not names:
            problems += say(
                mutation, False, f"[FAIL] {mutation.name}: SURVIVED -- no test noticed"
            )
            continue
        hit = any(mutation.expect in name for name in names)
        mark = "[OK]  " if hit else "[FAIL]"
        # The scope is printed because the count means different things in
        # the two cases: `1 red` out of the one test named for the mutation
        # is not the same statement as `1 red` out of 607.
        scope = "the test named for it" if narrow else f"{len(known)} tests"
        verdict = [
            f"{mark} {mutation.name}: {counted} red of {scope}"
            + ("" if hit else f", but NOT the expected one ({mutation.expect!r})")
        ]
        if not hit:
            verdict.append(f"         red instead: {sorted(names)}")
        problems += say(mutation, hit, *verdict)

    print()
    # The skip count rides on the verdict rather than sitting in a line above it.
    # A run that could not execute part of its own table must not read, three
    # scrollbacks later, like one that executed all of it.
    aside = f", {len(elsewhere)} skipped as not runnable on {HERE}" if elsewhere else ""
    for line in resume.closing():
        print(line)
    print(
        f"[OK] all {len(chosen)} mutations caught by the test named for them{aside}"
        if problems == 0
        else f"[FAIL] {problems} of {len(chosen)} mutations were not caught as described{aside}"
    )
    return 0 if problems == 0 else 1



#: Redaction: the two predicates that could ship an unredacted file, and the
#: writer's own guards. The first two are the ones to read --- each of them
#: hands the reader a document that looks redacted and is not.
MUTATIONS += [
    Mutation(
        # Call a plan that only redacts "the file". `is_identity` is what lets
        # the print path hand the original bytes over, so this prints a redacted
        # document with every word in it -- and nothing fails, because the file
        # it printed is a perfectly good file.
        "redact: call a plan that only redacts the file on disk",
        "src/edits.rs",
        "        self.marks.is_empty() && self.redactions.is_empty() && self.pages_are_the_file()",
        "        self.marks.is_empty() && self.pages_are_the_file()",
        "a_plan_that_only_redacts_is_neither_the_file_nor_an_append",
    ),
    Mutation(
        # A note edit that writes no body. The append still builds, the file is
        # still valid, and the comment still says what it said -- which is the
        # shape of failure a reader reports as "my edit did not save" and no
        # length or fingerprint check can see.
        "notes: build the append without writing the new body",
        "src/save/marks.rs",
        '    dictionary.set("Contents", text_string(&note.body));',
        '    let _ = &note.body;',
        "a_comment_out_of_the_file_is_overridden_by_its_object",
    ),
    Mutation(
        # `/M` left as the file had it. Every viewer shows that date, so the
        # reader's own words appear over somebody else's timestamp.
        "notes: leave the modification date the file had",
        "src/save/marks.rs",
        '    dictionary.set("M", text_string(&note.made));',
        '    let _ = &note.made;',
        "a_comment_out_of_the_file_is_overridden_by_its_object",
    ),
    Mutation(
        # The write skipped entirely. Aimed at the CALL rather than at the body,
        # because a guard is only covered when a mutation removes the call ---
        # `docs/TRAPS.md` has that one, from a deploy that locked an account.
        "notes: never call the note writer at all",
        "src/save.rs",
        "    write_note_edits(&mut incremental, &plan.notes)?;",
        "    let _ = &plan.notes;",
        "a_comment_out_of_the_file_is_overridden_by_its_object",
    ),
    Mutation(
        # The same omission on the other writer, and it is where the defect
        # actually was: a reader who edits a comment and then deletes a page
        # leaves the append path, and until this call existed the rewrite
        # dropped the edit and reported a successful save. Aimed at the CALL for
        # the reason the append's twin is.
        "notes: never call the note writer on the rewrite path",
        "src/save.rs",
        "    rewrite_note_edits(&mut doc, &plan.notes)?;",
        "    let _ = &plan.notes;",
        "a_rewrite_writes_the_new_body_over_the_old",
    ),
    Mutation(
        # The `/Subtype` guard, reached through the second caller. Without it a
        # plan naming a page object writes `/Contents` onto the page, where the
        # key is its content stream: the save reports success and the document is
        # destroyed. Shared between the two writers precisely so that adding a
        # caller could not lose it.
        "notes: let a note edit name something that is not an annotation",
        "src/save/marks.rs",
        MUT_SUBTYPE_GUARD,
        "    if false {",
        "a_rewrite_refuses_a_note_edit_that_names_a_page",
    ),
    Mutation(
        # The latest version kept rather than the first. Undo of the second
        # rewrite would then leave the second body in place, which is the one
        # thing the journal exists to make impossible -- and every other rewrite
        # test has one version per object, so this is the only one that can see
        # it.
        "rewrite: keep the first body an object was given, not the latest",
        "src/docmodel.rs",
        "                self.rewrites.insert(object, Rewritten { page, edit });",
        "                self.rewrites.entry(object).or_insert(Rewritten { page, edit });",
        "undoing_the_first_rewrite_leaves_the_model_with_nothing_to_say",
    ),
    Mutation(
        # A deleted page leaves its rewrites behind. The plan then carries an
        # edit for a comment the written file does not contain, and the reader is
        # told the save failed for a reason about a document they cannot see.
        "rewrite: leave a deleted page's rewrites in the model",
        "src/docmodel.rs",
        "                self.rewrites.retain(|_, held| held.page != page);",
        "                let _ = page;",
        "deleting_a_page_takes_the_rewrites_on_it_and_no_others",
    ),
    Mutation(
        # The page checked only on the replay path. The refusal still arrives,
        # from `Working::apply`, so nothing a reader sees changes -- and a body
        # has been issued into a table nothing can ever reach again.
        "rewrite: issue the body before the page is checked",
        "src/docmodel.rs",
        "        self.now.live(page)?;\n        // Before the body is issued, for the liveness check's reason above.\n        self.now.opened_files_page(page)?;\n        let edit = self.issue_rewrite(NoteEdit { body, made });",
        "        let edit = self.issue_rewrite(NoteEdit { body, made });",
        "a_rewrite_on_a_page_that_is_gone_says_which_of_the_two_it_is",
    ),
    Mutation(
        # Every rewrite reported for every page. The bodies are right and the
        # order is nonsense, which is what a walk over the map rather than over
        # the reading order would produce.
        "rewrite: report every rewrite against every page",
        "src/docmodel.rs",
        "                    .filter(move |(_, held)| held.page == *page)",
        "                    .filter(move |(_, _held)| true || *page == *page)",
        "rewrites_come_back_in_reading_order_and_by_object",
    ),
    Mutation(
        # A discarded redo tail keeps its bodies. Nothing about any document
        # changes, which is the whole reason `rewrite_bodies` exists to be read.
        "rewrite: keep the bodies of a discarded redo tail",
        "src/docmodel.rs",
        "                Command::Rewrite { edit, .. } => {\n                    self.rewrites.remove(&edit);\n                }",
        "                Command::Rewrite { .. } => {}",
        "a_discarded_rewrite_takes_its_body_with_it",
    ),
    Mutation(
        # A subset carries every rewrite, including ones on pages it did not
        # take. The extra edits land on objects the written file sweeps out, so
        # the reader pays for a save that changes nothing it could show them.
        "notes: carry every rewrite into a subset plan",
        "src/edits.rs",
        "        .filter(|(page, _, _)| kept.contains(page))",
        "        .filter(|(_page, _, _)| true)",
        "a_subset_takes_the_rewrites_on_the_pages_it_names_and_leaves_the_rest",
    ),
    Mutation(
        # Object 0 accepted. It is the head of the free list and can never be an
        # indirect object, so the model journals an edit that no writer can ever
        # apply -- and the reader has an undo step for it.
        "notes: accept a comment with no object of its own",
        "src/edits.rs",
        "        if object.0 == 0 {\n            return Err(\"that comment has no object to write over\".to_string());",
        "        if false {\n            return Err(\"that comment has no object to write over\".to_string());",
        "a_comment_with_no_object_of_its_own_is_refused_here_and_spends_nothing",
    ),
    Mutation(
        # Let a highlight answer a comment. `/IRT` on a text-markup annotation
        # is not a thread, it is a stray key -- and the mark would be written
        # with it, so every reader that threads would show a highlight nested
        # under a note.
        "reply: let a mark that is not a comment answer one",
        "src/docmodel.rs",
        "        if mark.reply_to.is_some() && mark.kind != MarkKind::Note {\n"
        "            return Err(Refusal::ReplyMismatch(mark.kind));\n"
        "        }",
        "",
        "only_a_comment_may_answer_a_comment",
    ),
    Mutation(
        # Drop the parent on the way into the model. The reply is made, it is
        # saved, and it answers nobody -- and the file is perfectly valid, so
        # nothing downstream complains.
        "reply: forget the comment a reply names",
        "src/edits.rs",
        "                    reply_to: want\n"
        "                        .reply_to\n"
        "                        .map(|(number, generation)| ObjectId::new(number, generation)),",
        "                    reply_to: None,",
        "a_reply_carries_the_comment_it_answers_all_the_way_to_the_plan",
    ),
    Mutation(
        # Drop the parent on the way out to the writer. The other end of the
        # same crossing, and its own mutation because a single test asserting
        # both would pass with either half working.
        "reply: forget the comment a reply names, on the way out",
        "src/edits.rs",
        "                    reply_to: body\n"
        "                        .reply_to\n"
        "                        .map(|object| (object.number(), object.generation())),",
        "                    reply_to: None,",
        "a_reply_carries_the_comment_it_answers_all_the_way_to_the_plan",
    ),
    Mutation(
        # Write no `/IRT` at all. The reply becomes an unrelated second note by
        # another author, which is exactly what `pdfium-render` not exposing
        # the key means nobody would notice through that reader.
        "reply: write a reply that names nothing",
        "src/save/marks.rs",
        '        dictionary.set("IRT", Object::Reference((number, generation)));',
        "",
        "a_reply_is_written_as_one_and_reads_back_as_one",
    ),
    Mutation(
        # Say the relationship is a group rather than a reply. A reader that
        # honours `/RT` then shows the two comments as one annotation instead of
        # as a thread, and `annots.rs` -- which never reads the key -- agrees
        # with itself either way.
        "reply: call the relationship a group",
        "src/save/marks.rs",
        '        dictionary.set("RT", Object::Name(b"R".to_vec()));',
        '        dictionary.set("RT", Object::Name(b"Group".to_vec()));',
        "a_reply_is_written_as_one_and_reads_back_as_one",
    ),
    Mutation(
        # Let a reply name anything at all. The guard the writer has that the
        # model cannot: a plan naming a page, a font or the catalog would thread
        # a reply onto it.
        "reply: let a reply name something that is not an annotation",
        "src/save/marks.rs",
        '            return Err("that comment is not an annotation".into());\n'
        "        }\n"
        "    }\n"
        "    Ok(RepliesChecked)",
        "        }\n    }\n    Ok(RepliesChecked)",
        "a_reply_naming_something_that_is_not_an_annotation_is_refused_on_both_paths",
    ),
    Mutation(
        # Check the replies after the page surgery instead of before it. The
        # ordering finding: `materialise` unlinks a dropped page's annotations
        # and `sweep::collect` deletes them, so the refusal stops being about
        # the plan and becomes a true sentence about the file being built.
        "reply: check a reply's parent after the pages have been removed",
        "src/save.rs",
        "    let replies = check_replies(&doc, &plan.marks)?;",
        "    let replies = check_replies(&doc, &[])?;",
        "a_reply_naming_something_that_is_not_an_annotation_is_refused_on_both_paths",
    ),
    Mutation(
        # The mirror, one predicate over: route a redaction to the append. An
        # update section adds objects and never touches a content stream, so the
        # file is written, is bigger, and has nothing taken out of it.
        "redact: let a plan carrying a redaction be appended",
        "src/edits.rs",
        MUT_APPENDABLE,
        "        (!self.marks.is_empty() || !self.notes.is_empty())\n            && self.discards.is_empty()\n            && self.pages_are_the_file()",
        "a_plan_that_only_redacts_is_neither_the_file_nor_an_append",
    ),
    Mutation(
        # Remove from a page named twice. The second call runs against a stream
        # the first already changed, so its ordinals name different operators.
        "redact: remove twice from a page the plan names twice",
        "src/save.rs",
        "        if seen.contains(&redaction.source) {",
        "        if false {",
        "a_page_named_twice_by_the_redaction_plan_is_refused",
    ),
    Mutation(
        # Take a page the plan does not keep as the last one it does. Removing
        # text from a page nobody marked is the confident wrong answer.
        "redact: clamp a redaction naming a page past the end",
        "src/save.rs",
        "        let Some(page) = pages.get(redaction.source as usize) else {",
        "        let Some(page) = pages.get((redaction.source as usize).min(pages.len().saturating_sub(1))) else {",
        "a_redaction_naming_a_page_that_is_not_kept_is_refused",
    ),
    Mutation(
        # Act on each entry as it is checked rather than checking them all
        # first. A refusal half way through then leaves a document with some
        # pages redacted and some not, and the caller serialises it.
        "redact: act on each redaction as it is validated",
        "src/save.rs",
        "        targets.push((*page, redaction));",
        "        let took = crate::redact::remove_shows(doc, *page, &redaction.shows, redaction.text_objects).map_err(Refusal::from)?;\n        let _ = took;\n        targets.push((*page, redaction));",
        "a_page_named_twice_by_the_redaction_plan_is_refused",
    ),
]


#: The OCR gate's control chooser. Every one of these hands the gate a control
#: that proves less than it claims, and the verdict it produces is *Illegible* ---
#: which is the one verdict `docs/PLAN.md` §6 lets a caller present as clean.
MUTATIONS += [
    Mutation(
        # Choose the control from the words the region covered. Those are the
        # words the removal was supposed to take, so reading one back proves the
        # removal failed -- and the gate reads it as proof the engine can see.
        "control: choose from the words the region covered",
        "src/ocr.rs",
        "            !regions\n                .iter()",
        "            regions\n                .iter()",
        "a_word_the_region_covers_is_never_the_control",
    ),
    Mutation(
        # Size the control against the largest covered box instead of the
        # smallest. A control in 12 pt proves an engine reads 12 pt and says
        # nothing about the 6 pt footnote in the same region.
        "control: size against the largest box the region covered",
        "src/ocr.rs",
        "        .fold(f32::INFINITY, f32::min);",
        "        .fold(f32::NEG_INFINITY, f32::max);",
        "control_is_sized_to_the_smallest_redacted_box_not_the_largest",
    ),
    Mutation(
        # Let any surviving word be the control however large it is set. The
        # heading at the top of the page then certifies a page of small print.
        "control: accept a word of any size",
        "src/ocr.rs",
        "        .filter(|word| word.rect[3] - word.rect[1] <= size_pt + CONTROL_HEIGHT_SLACK_PT)",
        "        .filter(|word| word.rect[3] - word.rect[1] >= 0.0 || size_pt >= 0.0)",
        "a_word_set_larger_than_what_went_is_refused",
    ),
    Mutation(
        # Widen the slack until a word a whole point taller passes. The rule
        # survives and the number stops meaning anything, which is the shape a
        # test written in the constant's own units cannot see -- so the test
        # aimed at this one carries a measured height instead.
        "control: widen the height slack to a whole point",
        "src/ocr.rs",
        "const CONTROL_HEIGHT_SLACK_PT: f32 = 0.01;",
        "const CONTROL_HEIGHT_SLACK_PT: f32 = 1.0;",
        "a_word_one_point_taller_than_what_went_does_not",
    ),
    Mutation(
        # Accept a control of any length. A two-character fragment is what an
        # engine emits from noise, and one matching by accident certifies a page
        # nothing was read on.
        "control: accept a token of any length",
        "src/ocr.rs",
        "        if chars < MIN_CONTROL_CHARS {",
        "        if false {",
        "a_word_too_short_to_be_a_control_is_refused",
    ),
    Mutation(
        # Break a tie by the bottom-most word. Nothing is unsafe and the page
        # stops yielding the same control twice, so every test that says WHICH
        # control a page gives becomes a statement about iteration order.
        "control: break a tie by the bottom-most word",
        "src/ocr.rs",
        "                        && (word.rect[1] < have.rect[1]",
        "                        && (word.rect[1] > have.rect[1]",
        "a_tie_goes_to_the_topmost_and_then_the_leftmost",
    ),
    Mutation(
        # Take the whole line as the token. `adjudicate` asks whether ONE
        # recognised span contains it, so a 52-character line is read back only
        # when the engine happens to return that line in one piece -- measured
        # failing on text-base14 and passing on text-marked, i.e. a gate that
        # refuses a correct redaction depending on the font.
        "control: take the whole line as the control token",
        "src/ocr.rs",
        "        token: longest_run(&chosen.text).to_string(),",
        "        token: chosen.text.trim().to_string(),",
        "the_token_is_a_word_and_not_the_line_it_sits_on",
    ),
    Mutation(
        # Rank candidates by the length of the whole line rather than by its
        # longest word. A line of six three-letter words then beats a single
        # readable one, and the control it yields is a fragment.
        "control: rank by the whole line rather than its longest word",
        "src/ocr.rs",
        "        let chars = longest_run(&word.text).chars().count();",
        "        let chars = word.text.trim().chars().count();",
        "a_line_of_short_words_does_not_outrank_one_long_one",
    ),
    Mutation(
        # Take the band from the crop. The crop is on the page and the band is
        # in the probe image, so every recognised item falls outside it, the
        # control is never seen and a working gate refuses everything.
        "control: place the band where the crop is",
        "src/ocr.rs",
        "            size_pt: self.size_pt,\n            band,",
        "            size_pt: self.size_pt,\n            band: self.crop,",
        "placed_takes_the_band_from_its_caller_and_nothing_else",
    ),
]

#: The evidence an unread control carries. None of these is unsafe -- every one
#: leaves the verdict exactly as it was -- and that is the reason they are here:
#: a measurement nobody can falsify is one that gets quoted, and `docs/PLAN.md`
#: §6 has already had one increment ranked off a bucket with no denominator.
MUTATIONS += [
    Mutation(
        # Report the token as read nowhere. The band hypothesis then measures
        # zero on every corpus, which is the reassuring answer and the one that
        # closes the question by hiding it.
        "unread: report a token read outside the band as absent",
        "src/ocr.rs",
        "            .map(|i| control.overshoot(&i.rect))\n"
        "            .min_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])));",
        "            .map(|i| control.overshoot(&i.rect))\n"
        "            .min_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])))\n"
        "            .filter(|_| false);",
        "a_control_read_outside_its_band_is_recorded_as_outside_and_by_how_far",
    ),
    Mutation(
        # Report the furthest reading rather than the nearest. The count is
        # unchanged and the distance -- which is what says whether the repair is
        # a tolerance or something larger -- becomes the worst span in the image.
        "unread: report the furthest reading of the token, not the nearest",
        "src/ocr.rs",
        "            .min_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])));",
        "            .max_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])));",
        "the_nearest_reading_of_the_token_is_the_one_reported",
    ),
    Mutation(
        # Count every span as in-band. `items == 0` is what separates an engine
        # that read nothing from one that read the wrong things, and this leaves
        # the two counters equal on every refusal.
        "unread: count every span the engine returned as in-band",
        "src/ocr.rs",
        "                in_band: in_band.len(),",
        "                in_band: items.len(),",
        "an_engine_that_read_other_words_says_the_token_was_absent",
    ),
    Mutation(
        # Report the probe image with its two dimensions the wrong way round.
        # Every aspect then inverts, so a page-wide short image reads as a tall
        # narrow one and the band the measurement is about lands nowhere near
        # where it should. Nothing about the gate changes -- this is a reading
        # only, which is why it needs a test of its own.
        "geometry: report the probe image's shape transposed",
        "src/ocr_gate.rs",
        "        image_pt: (page.width_pt, height_pt),",
        "        image_pt: (height_pt, page.width_pt),",
        "the_reported_shape_is_the_page_wide_and_the_two_strips_tall",
    ),
    Mutation(
        # Leave out what `stack` adds. The image is then reported shorter than
        # it is and every aspect comes out wider, which biases the one axis the
        # 2026-08-28 measurement rests on toward its own conclusion.
        "geometry: report the probe image without the margins and the gap",
        "src/ocr_gate.rs",
        "    let height_pt = tallest + control_pt + padding;",
        "    let height_pt = tallest + control_pt;",
        "the_reported_shape_is_the_page_wide_and_the_two_strips_tall",
    ),
    Mutation(
        # Write the overshoot with `f32::max`, which is the obvious spelling and
        # returns the OTHER operand for a NaN -- so a rect with no centre reads
        # as inside the band, is not counted as a survivor, and can certify.
        # The only one of these five that is unsafe.
        "unread: compute the overshoot with a max that swallows a NaN",
        "src/ocr.rs",
        "        let axis = |c: f32, lo: f32, hi: f32| {\n"
        "            if c >= lo && c <= hi {\n"
        "                0.0\n"
        "            } else if c < lo {\n"
        "                lo - c\n"
        "            } else {\n"
        "                c - hi\n"
        "            }\n"
        "        };",
        "        let axis = |c: f32, lo: f32, hi: f32| (lo - c).max(c - hi).max(0.0);",
        "a_centre_that_is_not_a_number_is_outside_the_band",
    ),
]


#: A region over a picture. Two removals, and only the second one redacts:
#: deleting the `Do` stops the page drawing the image, and dropping the resource
#: entry is what leaves the stream unreachable so the sweep takes its bytes.
MUTATIONS += [
    Mutation(
        # Report a picture as unremovable again. Every region over one is then
        # refused, which is the behaviour that made a redaction over a scanned
        # page remove nothing.
        "image: refuse a picture instead of removing it",
        "src/redact.rs",
        "        } else if object.kind == \"image\" {\n            plan.images.push(image_ordinal);",
        "        } else if false {\n            plan.images.push(image_ordinal);",
        "a_region_over_an_image_names_it_rather_than_refusing",
    ),
    Mutation(
        # Advance the image ordinal only for the images a region covers. The
        # second picture on a page is then named as the first, and the removal
        # takes the wrong one.
        "image: number the pictures a region covers rather than the page's",
        "src/redact.rs",
        "        let image_ordinal = images;\n        if object.kind == \"image\" {\n            images += 1;\n        }",
        "        let image_ordinal = images;\n        if object.kind == \"image\" && overlaps(object.bounds, region) {\n            images += 1;\n        }",
        "an_image_ordinal_counts_images_and_not_every_object",
    ),
    Mutation(
        # Leave the resource entry. The page stops drawing the picture and every
        # byte of it stays in the file, reachable from the page -- a redaction
        # that removed the picture from view and nothing else.
        "image: stop drawing the picture and leave it in the file",
        "src/redact.rs",
        "    forget_xobjects(doc, page, &names)?;",
        "    let _ = &names;",
        "removing_an_image_takes_its_draw_and_its_resource_entry",
    ),
    Mutation(
        # Drop every image the page draws rather than the ones named. The check
        # above passes on it; only the control tells them apart.
        "image: drop every picture on the page",
        "src/redact.rs",
        "    let mut positions: Vec<usize> = Vec::with_capacity(ordinals.len());\n    let mut names: Vec<String> = Vec::new();\n    for &ordinal in ordinals {",
        "    let mut positions: Vec<usize> = Vec::with_capacity(ordinals.len());\n    let mut names: Vec<String> = Vec::new();\n    for &ordinal in &(0..drawn.len()).collect::<Vec<_>>() {",
        "removing_one_image_leaves_the_other_drawn_and_listed",
    ),
    Mutation(
        # Drop the correspondence guard. Nothing connects PDFium's image objects
        # to the page's `Do` operations but order.
        "image: remove whichever picture is at that position",
        "src/redact.rs",
        "    if drawn.len() != image_objects {",
        "    if false {",
        "an_image_count_that_disagrees_with_pdfium_removes_nothing",
    ),
    Mutation(
        # Sweep only for the carriers that were already listed. An image removal
        # then unlinks the stream and the rewrite writes it out anyway, which is
        # the defect `redact-apply-probe` found by grepping pixels.
        "image: leave the unlinked picture for the writer to emit",
        "src/save.rs",
        "        || redacted.images > 0\n        || discarded > 0\n        || !plan.text_edits.is_empty()\n    {\n        crate::sweep::collect(&mut doc)?;",
        "        || discarded > 0\n        || !plan.text_edits.is_empty()\n    {\n        crate::sweep::collect(&mut doc)?;",
        "a_rewrite_that_removed_a_picture_sweeps_it_out_of_the_file",
    ),
    Mutation(
        # Read the page even when no picture was marked. The correspondence
        # guard then refuses a call that was removing nothing, so an ordinary
        # text redaction on a page whose image count disagrees fails for a
        # removal it was not making.
        "image: read the page even when no picture was marked",
        "src/redact.rs",
        "    if ordinals.is_empty() {\n        return Ok(Removed {",
        "    if false {\n        return Ok(Removed {",
        "a_disagreeing_image_count_refuses_only_a_real_removal",
    ),
]

#: Text inside a Form XObject: reaching it, placing it, and refusing to touch a
#: form the document draws more than once. PDFium enumerates a form as ONE page
#: object, so every one of these is about a level `remove_shows` cannot see.
MUTATIONS += [
    Mutation(
        # Leave a form child's box in the form's own space. Measured on
        # `form-xobject.pdf`: a form placed at (60, 600) reports a child at
        # (0.9, 19.9), so the region covers nothing, the removal takes nothing,
        # and the save reports success.
        "form: leave a form's text in the form's own space",
        "src/objects.rs",
        "    let [a, b, c, d, e, f] = m;",
        "    let [a, b, c, d, e, f] = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];\n    let _ = m;",
        "a_translated_form_moves_its_text_by_exactly_the_translation",
    ),
    Mutation(
        # Take the transformed diagonal instead of bounding all four corners. A
        # quarter turn then gives a rectangle whose left is greater than its
        # right, which overlaps nothing at all.
        #
        # **Aimed at the fold rather than at the corner list, because the obvious
        # mutation is a no-op.** Duplicating two of the four corners still leaves
        # the extremes in `xs` and `ys`, and `min`/`max` over a list containing
        # them recovers the same rectangle -- so that edit landed, built, and
        # changed nothing. It is the bounding that has to go.
        "form: place a turned form from its diagonal",
        "src/objects.rs",
        "    [\n        xs.iter().copied().fold(f32::INFINITY, f32::min),\n        ys.iter().copied().fold(f32::INFINITY, f32::min),\n        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),\n        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),\n    ]",
        "    [xs[0], ys[0], xs[3], ys[3]]",
        "a_rotated_form_needs_all_four_corners",
    ),
    Mutation(
        # Put the unmeasurable box through the matrix. It becomes infinite rather
        # than everything, and an object PDFium would not place stops being
        # reported for every region -- which is a removal stepping over what it
        # could not see.
        "form: transform a child PDFium would not measure",
        "src/objects.rs",
        "    if rect == UNMEASURABLE {\n        return UNMEASURABLE;\n    }",
        "    if false {\n        return UNMEASURABLE;\n    }",
        "an_unmeasurable_child_stays_unmeasurable",
    ),
    Mutation(
        # Keep the columns outside the region instead of blanking them, which is
        # what the gate did until 2026-08-27: `strip` renders full-width rows, so
        # every word beside the region on those rows was read back as though the
        # removal had left it. 54 of 104 regions on 40 real documents.
        "mask: keep the columns beside the region",
        "src/ocr_gate.rs",
        "        row[..left * 4].fill(0xFF);\n        row[right * 4..].fill(0xFF);",
        "        let _ = (left, right);",
        "masking_keeps_the_region_s_own_columns_and_blanks_the_rest",
    ),
    Mutation(
        # Blank one side only. A region is two edges and a check that reads one
        # of them passes on this.
        "mask: blank only what is left of the region",
        "src/ocr_gate.rs",
        "        row[right * 4..].fill(0xFF);",
        "        let _ = right;",
        "masking_keeps_the_region_s_own_columns_and_blanks_the_rest",
    ),
    Mutation(
        # Round the region's edges inward rather than outward. A glyph whose ink
        # lands in the boundary pixel is then half erased, and half a glyph is
        # something an engine reads as something.
        "mask: round the region's edges inward",
        "src/ocr_gate.rs",
        "    let left = (rect[0].min(rect[2]) * scale).floor().max(0.0) as usize;",
        "    let left = (rect[0].min(rect[2]) * scale).ceil().max(0.0) as usize;",
        "masking_widens_to_whole_pixels_rather_than_clipping_the_region",
    ),
    Mutation(
        # Blank a strip whose columns miss the page instead of refusing. It then
        # reads as nothing, and reading nothing is the answer that certifies.
        "mask: blank a region that is not on the page at all",
        "src/ocr_gate.rs",
        "    if left >= right {\n        return Err(format!(",
        "    if false {\n        return Err(format!(",
        "a_region_beside_the_page_is_refused_rather_than_blanked",
    ),
    Mutation(
        # Take the rectangle's corners in the order given rather than by min and
        # max. A region dragged right to left then masks nothing.
        "mask: assume a region is dragged left to right",
        "src/ocr_gate.rs",
        "    let left = (rect[0].min(rect[2]) * scale).floor().max(0.0) as usize;\n    let right = ((rect[0].max(rect[2]) * scale).ceil().max(0.0) as usize).min(width_px as usize);",
        "    let left = (rect[0] * scale).floor().max(0.0) as usize;\n    let right = ((rect[2] * scale).ceil().max(0.0) as usize).min(width_px as usize);",
        "a_region_reversed_left_to_right_masks_the_same_columns",
    ),
    Mutation(
        # Report every child of a form the region touches, which is what this did
        # until 2026-08-27. A form is routinely a whole-page container, so a
        # region over one line inside a letterhead was refused for every picture
        # in it: 174 of 1,131 refusals across 40 real documents, 15.4%.
        "formchild: report every child of a form the region reaches",
        "src/redact.rs",
        "            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {",
        "            for other in &form.unreachable {\n                if true {",
        "a_form_child_the_region_misses_is_not_reported",
    ),
    Mutation(
        # The mirror, and the one that matters more: report nothing. A picture
        # sitting under the region is then silently not a reason, and the region
        # is certified complete when it is not.
        "formchild: report no child of a form at all",
        "src/redact.rs",
        "            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {",
        "            for other in &form.unreachable {\n                if !overlaps(other.bounds, region) {",
        "a_form_child_the_region_covers_is_still_reported",
    ),
    Mutation(
        # Take every line in a covered form, not the ones the region covers. A
        # region over one line of a form then removes the whole form, which is
        # content the reader did not mark.
        "form: take every line of a form the region reaches",
        "src/redact.rs",
        "                if overlaps(text.bounds, region) {\n                    plan.form_shows.push((at, ordinal));\n                }",
        "                let _ = overlaps(text.bounds, region);\n                plan.form_shows.push((at, ordinal));",
        "a_region_over_a_form_takes_only_the_lines_it_covers",
    ),
    Mutation(
        # Report a form as unhandled even when the descent read it. Every region
        # over a form is then refused, which is what the code did before this
        # carrier was reachable at all.
        "form: refuse a form whose text was read",
        "src/redact.rs",
        "        let inside = (object.kind == \"form\")\n            .then(|| forms.iter().find(|form| form.at == at))\n            .flatten();",
        "        let inside: Option<&FormObject> = None;\n        let _ = forms;",
        "a_region_over_a_forms_text_names_it_by_form_and_ordinal",
    ),
    Mutation(
        # Report what the descent could not reach only when some of the form's
        # text was also covered. A region over a form holding nothing but a
        # nested form then takes nothing and says nothing, which is the quiet
        # case becoming the silent one.
        "form: report the unreachable only beside a hit",
        "src/redact.rs",
        "            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {\n                    plan.unhandled.push(Unhandled {\n                        at,\n                        kind: other.kind.clone(),\n                        drawn: None,\n                    });\n                }\n            }",
        "            if !plan.form_shows.is_empty() {\n            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {\n                    plan.unhandled.push(Unhandled {\n                        at,\n                        kind: other.kind.clone(),\n                        drawn: None,\n                    });\n                }\n            }\n            }",
        "what_the_descent_could_not_reach_is_reported_even_when_nothing_was_covered",
    ),
    Mutation(
        # Stop counting the `Do` operations on this page. A form drawn twice here
        # is ONE reference in the object graph, so the graph count alone calls it
        # unshared and the removal changes a place the reader did not mark.
        "form: count only the object graph, not the page's draws",
        "src/redact.rs",
        "    (here > 1 || elsewhere > 1).then(|| here.max(elsewhere))",
        "    (elsewhere > 1).then(|| here.max(elsewhere))",
        "a_form_drawn_twice_on_one_page_is_refused",
    ),
    Mutation(
        # The mirror: stop counting references. A form another page draws is one
        # `Do` here, so the page count alone calls it unshared.
        "form: count only the page's draws, not the object graph",
        "src/redact.rs",
        "    let here = names.iter().filter(|other| *other == name).count();\n    let elsewhere = references_to(doc, id);",
        "    let here = names.iter().filter(|other| *other == name).count();\n    let elsewhere = 0;\n    let _ = references_to(doc, id);",
        "a_form_another_page_also_draws_is_refused",
    ),
    Mutation(
        # Remove ascending. An earlier removal moves every later index, so the
        # second ordinal names whatever slid into its place.
        "form: remove a form's lines in ascending order",
        "src/redact.rs",
        "    for where_ in positions.into_iter().rev() {\n        inside.operations.remove(where_);\n    }",
        "    for where_ in positions.into_iter() {\n        inside.operations.remove(where_);\n    }",
        "removing_two_lines_from_one_form_takes_both_and_keeps_the_rest",
    ),
    Mutation(
        # Drop the correspondence guard. Nothing connects PDFium's form objects
        # to the page's `Do` operations but order, so a disagreement removes text
        # from whichever form happens to be at that position.
        "form: remove from whichever form is at that position",
        "src/redact.rs",
        "    if names.len() != forms.len() {",
        "    if false {",
        "a_form_count_that_disagrees_with_pdfium_removes_nothing",
    ),
    Mutation(
        # The same guard one level down: the form's own show operators against
        # what PDFium counted inside it.
        "form: remove by position without checking the count",
        "src/redact.rs",
        "    if shows.len() != text_objects {\n        return Err(format!(\n            \"this form has {} text-showing operator(s) and PDFium reported {text_objects} text \\\n             object(s). Removing by position needs those to agree, so nothing was removed.\",",
        "    if false {\n        return Err(format!(\n            \"this form has {} text-showing operator(s) and PDFium reported {text_objects} text \\\n             object(s). Removing by position needs those to agree, so nothing was removed.\",",
        "a_show_count_that_disagrees_with_pdfium_removes_nothing",
    ),
]

#: The join between the redaction and the engine: which words a page has, how big
#: to render them, what image the engine is shown, and what a verdict is called.
#: Every one of these decides whether a region can be *certified*, which is the
#: one answer `docs/PLAN.md` §6 says must never be given by default.
MUTATIONS += [
    Mutation(
        # Drop the flush after the loop. A page's last word never leaves the
        # accumulator, so a one-word page yields nothing and the chooser refuses
        # a page that had a perfectly good control on it.
        "gate: forget the word the page ends on",
        "src/ocr_gate.rs",
        "    flush(&mut out, &mut text, &mut rect);\n    out\n}",
        "    let _ = (&mut text, &mut rect);\n    out\n}",
        "a_word_at_the_very_end_of_the_page_is_not_lost",
    ),
    Mutation(
        # Let a character PDFium gave no box for into the geometry. Its four
        # zeroes then pull the word's box to the page corner, and the control is
        # cropped from somewhere the word is not.
        "gate: place a word from a character that has no box",
        "src/ocr_gate.rs",
        "        if box_ == [0.0; 4] || !box_.iter().all(|v| v.is_finite()) {\n            continue;\n        }",
        "        if !box_.iter().all(|v| v.is_finite()) {\n            continue;\n        }",
        "a_character_with_no_box_stays_in_the_text_and_out_of_the_geometry",
    ),
    Mutation(
        # Drop every word the removal names, covered ones included. The covered
        # words are what set the size the control must be no easier than, so the
        # chooser then measures against a bigger box and picks a control that
        # proves less --- `docs/TRAPS.md`'s *a control that is easier than the
        # check certifies nothing*, arriving through the filter in front of it.
        "gate: let the survivor filter take the covered words too",
        "src/ocr_gate.rs",
        "            regions.iter().any(|r| crate::objects::overlaps(w.rect, *r))\n                || !gone.iter().any(|g| *g == w.text)",
        "            !gone.iter().any(|g| *g == w.text)",
        "a_word_a_region_covers_is_kept_even_when_the_removal_names_it",
    ),
    Mutation(
        # Take whatever scale the control's size asks for. A 40 pt heading then
        # renders below 1x and a 1 pt one at 16x, and the probe image is either
        # unreadable or too big to hand over.
        "gate: render at whatever scale the control asks for",
        "src/ocr_gate.rs",
        # Re-aimed 2026-08-28: the unclamped half of this rule became
        # `scale_wanted`, so a measurement could ask what a control would have
        # needed. The defect is the same one -- drop the clamp.
        "    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE);",
        "    let mut scale = scale_wanted(size_pt);",
        "the_scale_never_leaves_its_bounds",
    ),
    Mutation(
        # Never reduce. A probe image past the mapping is then refused outright
        # rather than rendered at a scale that fits, so a large page loses its
        # verification for a reason that had a remedy.
        "gate: refuse a big probe image rather than shrinking it",
        "src/ocr_gate.rs",
        "    while scale > MIN_SCALE && bytes_at(width_pt, probe_height_pt, scale) > capacity {\n        scale = (scale * 0.5).max(MIN_SCALE);\n    }",
        "    while false {\n        scale = (scale * 0.5).max(MIN_SCALE);\n    }",
        "a_probe_image_that_will_not_fit_at_the_chosen_scale_is_rendered_smaller",
    ),
    Mutation(
        # Butt the two strips together. This is the defect that shipped for an
        # afternoon: with no gap the engine read `quartz,` as `auartz,` on
        # `text-base14` and the gate refused a redaction that was fine.
        "gate: stack the strips with nothing between them",
        "src/ocr_gate.rs",
        "    let gap = (SEPARATION_PT * scale).round().max(2.0) as u32;",
        "    let gap = 0u32;",
        "the_two_strips_never_touch",
    ),
    Mutation(
        # Put the band's edge at the control's first row instead of in the middle
        # of the gap. `Control::contains` tests a centre and an engine's box is a
        # detection rather than a measurement, so a control reported a point high
        # then counts as a survivor and a clean region is reported legible.
        "gate: put the band edge at the control rather than in the gap",
        "src/ocr_gate.rs",
        "    let edge = margin + under + gap / 2;",
        "    let edge = margin + under + gap;",
        "the_band_edge_sits_in_the_middle_of_the_gap_not_at_the_control",
    ),
    Mutation(
        # Accept a strip that is not whole rows. The row count is then wrong, so
        # the band lands somewhere else in the image and the partition decides the
        # verdict: a survivor counted as the control certifies the region.
        "gate: accept a strip that is not whole rows",
        "src/ocr_gate.rs",
        "        if strip.is_empty() || strip.len() % stride != 0 {",
        "        if false {",
        "a_strip_that_is_not_whole_rows_is_refused_before_it_is_stacked",
    ),
    Mutation(
        # Say nothing about a verdict that did not certify. `Applied::why` is
        # empty exactly when `verified`, so a region the gate could not check is
        # then reported to the reader as a clean redaction.
        "gate: stay silent about a region that could not be checked",
        "src/ocr_gate.rs",
        "        Legibility::NotVerified { why, .. } => Some(format!(",
        "        Legibility::NotVerified { why, .. } => (false).then(|| format!(",
        "a_not_verified_verdict_carries_its_own_reason_through",
    ),
    Mutation(
        # Let the error path drift from the rule it duplicates. `judge` builds its
        # own `NotVerified` when no engine answered, rather than fabricating an
        # `EngineId` for `adjudicate` to ignore --- which is a second *caller* of
        # rule 1 and must not become a second copy of it.
        "gate: let the engine-failure reason drift from adjudicate's",
        "src/ocr_gate.rs",
        "    Legibility::NotVerified {\n        why: format!(\"{e}\"),\n        cause: NotVerifiedCause::EngineError,\n        evidence: None,\n    }\n}",
        "    Legibility::NotVerified {\n        why: format!(\"the engine failed: {e}\"),\n        cause: NotVerifiedCause::EngineError,\n        evidence: None,\n    }\n}",
        "the_error_path_says_what_adjudicate_would",
    ),
    Mutation(
        # Choose the render scale from the smallest box a region covered rather
        # than from the control word the engine is actually shown. This is the
        # code as it stood until 2026-08-28, and it is invisible from every
        # verdict: the gate still runs, still refuses, and still gives its
        # reason. What changes is that 34 of 38 unverifiable regions in a
        # 40-document corpus were shown a control below `MIN_CONTROL_PX` -- the
        # floor this very call exists to clear.
        "gate: scale the probe image from the covered box, not the control",
        "src/ocr_gate.rs",
        # Re-aimed 2026-08-28 when this call gained a `map_err` to carry its
        # own cause. The defect is unchanged -- size the render from the box
        # rather than from the control the engine is shown.
        "let scale = scale_for(control_pt, page.width_pt, height_pt, capacity)",
        "let scale = scale_for(choice.size_pt, page.width_pt, height_pt, capacity)",
        "the_scale_clears_the_floor_for_the_control_and_not_for_the_box",
    ),
    Mutation(
        # Raise the scale by dropping the floor's clamp instead of by measuring
        # the right height. The test above passes either way; only the control
        # beside it, which requires an 8 pt control to stay at exactly 2x, can
        # tell a fix from a blanket magnification.
        "gate: reach the floor by raising every scale rather than the right one",
        "src/ocr_gate.rs",
        # Re-aimed 2026-08-28, same split as above.
        "    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE);",
        "    let mut scale = scale_wanted(size_pt).clamp(MIN_SCALE, MAX_SCALE) * 2.0;",
        "a_control_no_smaller_than_its_box_is_scaled_the_same_as_before",
    ),
    Mutation(
        # Report every way of failing to choose a control as the same cause. The
        # sentence a reader sees is unchanged, so nothing in the application
        # differs -- and the measurement that decides the next increment loses
        # the split it was built to make: 90% of the gate's refusals are this
        # one refusal, and which of its four reasons fired is the whole question.
        "ocr: collapse the four control refusals onto one cause",
        "src/ocr.rs",
        "            NotVerifiedCause::ControlAllLarger,",
        "            NotVerifiedCause::ControlTooShort,",
        "a_word_one_point_taller_than_what_went_does_not",
    ),
    Mutation(
        # Give two causes the same label. A report keyed by it then shows one row
        # where there were two, its total still adds up, and the step that
        # stopped being reported reads exactly like a step that stopped failing.
        "ocr: label two causes the same",
        "src/ocr.rs",
        "            Self::Stack => \"strips would not stack\",",
        "            Self::Stack => \"region is not on the page\",",
        "every_cause_has_its_own_label",
    ),
    Mutation(
        # The copy-paste a hand-written list invites: one cause listed twice and
        # another not at all. The array is fixed-length, so the count still
        # compiles -- and the missing cause's row is never printed, which is not
        # the same as being printed as a zero.
        "ocr: list one cause twice in ALL and drop another",
        "src/ocr.rs",
        "        Self::Stack,\n        Self::EngineError,",
        "        Self::Stack,\n        Self::Stack,",
        "all_lists_every_cause_once",
    ),
    Mutation(
        # Return a row range for a rectangle that is off the page. It is empty, so
        # the engine is handed no rows, reads nothing, and reading nothing is the
        # answer that certifies.
        "gate: hand back an empty row range for an area off the page",
        "src/ocr_gate.rs",
        "    (bottom > top).then_some((top, bottom - top))",
        "    Some((top, bottom.saturating_sub(top)))",
        "a_rectangle_entirely_off_the_page_covers_no_rows_at_all",
    ),
]

#: The OCR worker's own guards. Its input is a pipe and a shared mapping, so
#: every one of these is about numbers that arrived from somewhere else.
MUTATIONS += [
    Mutation(
        # Do the size arithmetic in u32, where 65536 x 65536 x 4 is ZERO. Every
        # length comparison below it then passes and the engine is handed an
        # empty slice described as four gigapixels.
        "ocrworker: size a frame with u32 arithmetic",
        "src/ocr_worker.rs",
        "    let pixels = usize::try_from(width)\n        .ok()?\n        .checked_mul(usize::try_from(height).ok()?)?;\n    pixels.checked_mul(4)",
        "    Some(width.wrapping_mul(height).wrapping_mul(4) as usize)",
        "a_four_gigapixel_frame_is_a_number_too_big_rather_than_zero",
    ),
    Mutation(
        # Resolve any engine name to the first one this build knows. A child from
        # a different build then reports an identity that is not its own, and
        # nothing downstream can invalidate a recognition against an engine that
        # never ran.
        "ocrworker: resolve any engine name to a known one",
        "src/ocr_worker.rs",
        "            .find(|known| **known == self.name)",
        "            .find(|known| !known.is_empty())",
        "an_unknown_engine_name_is_refused_and_named_in_the_refusal",
    ),
    Mutation(
        # Make the reply untagged. `docs/TRAPS.md` records untagged silently
        # swapping two variants of the same shape; here a bare payload becomes a
        # legal reply, which is the encoding this one is chosen against.
        "ocrworker: make the reply untagged",
        "src/ocr_worker.rs",
        "pub enum Said {",
        "#[serde(untagged)]\npub enum Said {",
        "a_bare_payload_is_not_a_reply",
    ),
    Mutation(
        # Refuse an image exactly as large as the buffer. Nothing is unsafe and
        # the largest region a reader can redact stops being verifiable, silently
        # -- the reader is told the file could not be proved clean.
        "ocrworker: refuse an image exactly the size of the buffer",
        "src/ocr_worker.rs",
        "    if pixels.rgba.len() > capacity {",
        "    if pixels.rgba.len() >= capacity {",
        "an_image_larger_than_the_buffer_is_refused_before_it_is_copied",
    ),
    Mutation(
        # Copy an image in without checking it is its own dimensions. The child
        # then reads a frame whose length its own numbers imply and whose content
        # is somebody else's previous request.
        "ocrworker: copy in an image that is not its own dimensions",
        "src/ocr_worker.rs",
        "    if !pixels.is_consistent() {",
        "    if false {",
        "an_image_that_is_not_its_own_dimensions_is_refused_before_it_is_copied",
    ),
    Mutation(
        # Refuse a frame exactly as large as the mapping, in the child this time.
        # The two guards are separate on purpose and so are the mutations: one is
        # about what the parent copies in and one about what the child reads out.
        "ocrworker: refuse a frame exactly the size of the mapping",
        "src/ocr_worker.rs",
        "    if len > mapping.len() {",
        "    if len >= mapping.len() {",
        "a_frame_exactly_as_large_as_the_mapping_fits",
    ),
]

# The guards added on 2026-08-28, when an outside review found that the increment
# which taught the rewrite to preserve encryption had not been carried to four of
# its neighbours. Each of these was run before it was written down; none is here
# on the strength of an argument.
MUTATIONS += [
    Mutation(
        # Take the password off the reload of the merge's own base. `lopdf`
        # answers `Ok` with nothing parsed for a document it cannot authenticate,
        # so the merge fails at the catalog and blames this module's writer.
        "save: reload the merge base without the key that opened it",
        "src/save.rs",
        "            password: password.map(str::to_string),\n            ..Default::default()\n        },\n    )\n    // Not a refusal a reader can act on",
        "            ..Default::default()\n        },\n    )\n    // Not a refusal a reader can act on",
        "a_merge_whose_base_is_password_protected_keeps_its_encryption",
    ),
    Mutation(
        # Leave the merged document in the clear. Same class as the rewrite's own
        # encryption mutation and a different site: the incoming-file refusal
        # exists to stop exactly this, and the base can do it too.
        "save: write a merge of an encrypted base in the clear",
        "src/save.rs",
        "    if let Some(state) = &encryption {\n        merged.encrypt(state)",
        "    if let Some(state) = &encryption.filter(|_| false) {\n        merged.encrypt(state)",
        "a_merge_whose_base_is_password_protected_keeps_its_encryption",
    ),
    Mutation(
        # Print without the reader's key. The parse in front of the encryption
        # refusal then fails first, and the reader is told to open the document
        # with the password it is already open with.
        "save: check a print job without the password that opened the document",
        "src/save.rs",
        "    let checked = checked(original, plan, job.view(), password, inputs)?;",
        "    let checked = checked(original, plan, job.view(), None, inputs)?;",
        "a_print_job_from_a_locked_document_names_the_escape_that_exists",
    ),
    Mutation(
        # Let the verifier call a file clean when it decoded none of it. The
        # needle is compared against an empty graph, `found` stays empty, and the
        # verdict is `Verified` about bytes nothing read.
        "verify: verify a file whose encryption was never opened",
        "src/verify.rs",
        "            if doc.is_encrypted() {",
        "            if false {",
        "a_scan_that_decoded_no_object_is_not_verified",
    ),
    Mutation(
        # The same claim for the other subject: a document that parses cleanly
        # and holds nothing. Two rules, two fixtures --- the encrypted one leaves
        # ONE object behind, so it never reaches this arm.
        "verify: verify a file that parsed to no objects at all",
        "src/verify.rs",
        "            } else if doc.objects.is_empty() {",
        "            } else if false {",
        "a_scan_that_decoded_no_object_is_not_verified",
    ),
    Mutation(
        # Stage beside the name rather than beside the document. On a link whose
        # target is in another directory the temporary file lands on a filesystem
        # the rename may not be able to cross.
        "save: stage a rewrite beside the link rather than the document",
        "src/save.rs",
        "    let target = resolved(source);",
        "    let target = source.to_path_buf();",
        "saving_in_place_through_a_symlink_edits_the_document_the_link_names",
        only_on="macos",
    ),
    Mutation(
        # Rename onto the link. It becomes an ordinary file holding the new
        # bytes, and the document it named keeps the old ones --- so a page turn
        # and a highlight, on one file, end up in two different places.
        "save: commit a rewrite onto the link rather than the document",
        "src/save.rs",
        "    commit(staged, &resolved(source))",
        "    commit(staged, source)",
        "saving_in_place_through_a_symlink_edits_the_document_the_link_names",
        only_on="macos",
    ),
    Mutation(
        # Let the replacement take the umask's mode. A document kept at 0600 in a
        # shared directory comes back readable by everyone, with correct bytes and
        # the right page count.
        "save: give the replacement the umask's mode rather than the document's",
        "src/save.rs",
        "        #[cfg(unix)]\n        if let Ok(existing) = std::fs::metadata(out) {\n            let _ = file.set_permissions(existing.permissions());\n        }\n",
        "",
        "a_rewrite_keeps_the_documents_mode",
        only_on="macos",
    ),
    Mutation(
        # Time the read-back out without ending the worker. The refusal is still
        # correct; the process and the thread blocked on its pipe are leaked.
        "save: let a timed-out read-back leave its worker running",
        # `awaited` moved to `save_outside.rs` on 2026-08-31 with the rest of
        # `InWorker`'s behaviour, and the call it names lost its `crate::workers::`
        # prefix in the move, because naming that module in the import header is
        # the whole point of the move. Both halves of the anchor had to follow.
        "src/save_outside.rs",
        "            kill_pid(pid);\n",
        "",
        "a_read_back_that_never_answers_ends_the_worker",
        only_on="macos",
    ),
    Mutation(
        # Make the bound a thousand times longer, which is what having no bound
        # looks like from inside a test. Killed by the *upper* assertion only ---
        # a lower bound is satisfied by any longer wait, and this survived until
        # that assertion existed.
        "save: bound the read-back at a thousand times the deadline",
        # Moved with `awaited`. The line itself is unchanged --- it names nothing
        # outside the function --- so only the file it lives in moved.
        "src/save_outside.rs",
        "    match rx.recv_timeout(within) {",
        "    match rx.recv_timeout(within * 1000) {",
        "a_read_back_that_never_answers_ends_the_worker",
        only_on="macos",
    ),
]

# `redact::aggregate`, extracted from the Tauri command layer on 2026-08-28. The
# arithmetic did not change; what changed is that a test can reach it. Every one
# of these five was unreachable by any mutation while the code was a loop body
# inside a `#[tauri::command]`'s private helper.
MUTATIONS += [
    Mutation(
        # Image-only selections used to report zero despite removing image draws.
        "redact: omit image draws from the removal count",
        "src/redact.rs",
        "    total += images.len();",
        "    total += 0;",
        "image_draws_are_merged_and_counted_without_a_text_layer",
    ),
    Mutation(
        # Stop merging the operators two overlapping regions both name. The
        # reader is then told a page has more removals in it than it has.
        "redact: count one show operator once per region that names it",
        "src/redact.rs",
        "    shows.sort_unstable();\n    shows.dedup();\n    let mut total",
        "    shows.sort_unstable();\n    let mut total",
        "two_regions_over_one_line_are_one_removal",
    ),
    Mutation(
        # The same, one level down, inside a form. Its own mutation because it is
        # its own statement -- a fixture exercising both at once could not say
        # which of them a survivor had broken.
        "redact: count a form's show operator once per region that names it",
        "src/redact.rs",
        "    form_shows.sort_unstable();\n    form_shows.dedup();\n    total",
        "    form_shows.sort_unstable();\n    total",
        "text_inside_a_form_is_merged_and_counted_too",
    ),
    Mutation(
        # Report the model's page number rather than the reader's, which sends
        # somebody to look at the wrong sheet for an object that could not go.
        "redact: name a concern's page zero-based",
        "src/redact.rs",
        'format!("page {}: {}", page + 1, object.sentence())',
        'format!("page {}: {}", page, object.sentence())',
        "an_object_that_cannot_be_removed_names_the_readers_page_number",
    ),
    Mutation(
        # Let a region that takes no text contribute an empty needle. An empty
        # string is present in every file ever written, so the verification then
        # reports a leak on a redaction that is perfectly clean.
        "redact: take an empty string as something to verify",
        "src/redact.rs",
        "        if !taking.is_empty() {",
        "        if true {",
        "a_region_that_takes_no_text_adds_no_needle",
    ),
    Mutation(
        # Hand the OCR gate the plan's rectangles, which are in the page's own
        # space, instead of the reader's, which are in display space. Every
        # control then gets looked for somewhere else on the page.
        "redact: give the gate the page-space regions rather than the display ones",
        "src/redact.rs",
        "        regions: displayed,",
        "        regions: areas.clone(),",
        "the_gate_gets_the_display_space_regions_rather_than_the_plans",
    ),
]

# `pagetree::materialise`, the one page-selection sequence both writers use,
# extracted on 2026-08-28. `save::rewrite` and `print::build` each had their own
# copy, spelled differently, and `docs/TRAPS.md` records two shipped defects that
# came from the pair drifting.
MUTATIONS += [
    Mutation(
        # Leave the outline in place after a deletion. Its destinations then name
        # pages that are not in the file.
        "pagetree: keep the outline after deleting the pages it points at",
        "src/pagetree.rs",
        "    if !dropped.is_empty() {\n        drop_pages(doc, dropped)?;\n        drop_outline(doc)?;\n    }",
        "    if !dropped.is_empty() {\n        drop_pages(doc, dropped)?;\n    }",
        "materialising_a_deletion_drops_the_outline",
    ),
    Mutation(
        # Drop the outline unconditionally, which takes every bookmark out of a
        # document whose pages were merely rearranged --- and they would all still
        # have pointed at a page.
        "pagetree: drop the outline for a move as well as a deletion",
        "src/pagetree.rs",
        "    if !dropped.is_empty() {\n        drop_pages(doc, dropped)?;\n        drop_outline(doc)?;\n    }",
        "    drop_outline(doc)?;\n    if !dropped.is_empty() {\n        drop_pages(doc, dropped)?;\n    }",
        "materialising_a_move_keeps_the_outline",
    ),
    Mutation(
        # Reorder even when nothing moved. `reorder_pages` flattens the tree, so
        # every page loses what it inherited from the node it hung under --- for a
        # job that changed nothing.
        "pagetree: flatten the page tree for a plan that moved nothing",
        "src/pagetree.rs",
        "    if let Some(order) = reorder_to {\n        reorder_pages(doc, order)?;\n    }",
        "    reorder_pages(doc, reorder_to.unwrap_or(&doc.get_pages().into_values().collect::<Vec<_>>()))?;",
        "materialising_nothing_leaves_the_tree_alone",
    ),
    Mutation(
        # Ignore the anchor. Every inserted page lands at the front, which on a
        # one-page document is the same place it belonged and on any other is
        # not --- and a count of pages agrees either way.
        "docmodel: put an inserted page at the front whatever it was anchored to",
        "src/docmodel.rs",
        "                let at = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };",
        "                let at = 0;",
        "an_inserted_page_lands_behind_its_anchor_and_shows_nothing",
    ),
    Mutation(
        # Off by one the other way: the page lands in front of the one it was
        # anchored to. `Command::Move` has the identical mutation for the
        # identical reason, and both look like a drag landing one row out.
        "docmodel: put an inserted page in front of its anchor rather than behind it",
        "src/docmodel.rs",
        "                let at = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };",
        "                let at = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor),\n                };",
        "an_inserted_page_lands_behind_its_anchor_and_shows_nothing",
    ),
    Mutation(
        # Accept a page of no area at the entry point. The apply below still
        # refuses it, so the *refusal* is unchanged --- what changes is that the
        # id was spent on the way, which only the "spends no id" half can see.
        "docmodel: issue a page's id before checking that it has any area",
        "src/docmodel.rs",
        "        if !size.is_proper() {\n            return Err(Refusal::DegeneratePage(size));\n        }\n        if let Some(anchor) = after {",
        "        if let Some(anchor) = after {",
        "a_page_enclosing_no_area_is_refused_and_spends_no_id",
    ),
    Mutation(
        # And in the apply, which is the guard the public `Command::Insert`
        # meets. Its neighbour in `Doc::insert` refuses the same input first, so
        # only a test that goes through `Doc::apply` can see this one.
        "docmodel: let the apply take a page of no area",
        "src/docmodel.rs",
        "                if !size.is_proper() {\n                    return Err(Refusal::DegeneratePage(size));\n                }",
        "                if false {\n                    return Err(Refusal::DegeneratePage(size));\n                }",
        "a_page_of_no_area_is_refused_by_the_apply_as_well_as_by_the_entry_point",
    ),
    Mutation(
        # Number a made page from 1, as the baseline pages are. The second page
        # of the file and the first page tpdf makes then share an id, and every
        # command naming either reaches whichever the table happens to hold.
        "docmodel: issue a made page's id from the same run as the file's own",
        "src/docmodel.rs",
        "            next_page: u64::from(pages) + 1,",
        "            next_page: 1,",
        "a_made_page_is_never_given_a_baseline_page_s_id",
    ),
    # **Deleted 2026-08-30: "docmodel: let a mark onto a page tpdf made".** It
    # watched a guard that has been removed on purpose --- a mark on a page tpdf
    # made is written now, and the addressing that made it impossible is gone.
    # Deleted rather than re-aimed, for the reason `docs/TRAPS.md` records about
    # a seam that removes a class of defect: there is no guard left for a
    # mutation to delete. What replaced its coverage is `save: resolve a mark
    # against the file's pages rather than the plan's`, below, which is the
    # defect the old guard existed to make unreachable.
    Mutation(
        # And a redaction, which puts a row in the review list certifying the
        # removal of nothing from a page that has nothing on it.
        "docmodel: let a redaction onto a page tpdf made",
        "src/docmodel.rs",
        "        if let Some(page) = self.now.page(redaction.page) {\n            if let PageSource::Blank(_) = page.source {\n                return Err(Refusal::RedactionOnMadePage(redaction.page));\n            }\n        }",
        "        let _ = redaction.page;",
        "a_page_tpdf_made_takes_a_mark_and_no_redaction",
    ),
    Mutation(
        # Resolve a mark against the file's own pages rather than the plan's.
        # This is the addressing that made a mark on an inserted page impossible
        # --- a made page is in no file, so it has no number here at all. On a
        # plan that inserts, the mark's position names a page the baseline list
        # does not have and the save is refused; on one that only moves, it
        # names a different page and the mark is written onto it.
        "save: resolve a mark against the file's pages rather than the plan's",
        "src/save.rs",
        "    let sites = mark_sites(&doc, &order, &plan.marks)?;",
        "    let sites = mark_sites(&doc, &pages, &plan.marks)?;",
        "a_mark_on_a_page_tpdf_made_is_written_onto_that_page",
    ),
    Mutation(
        # Put the baseline page number in the plan instead of the position, which
        # is what this field held until the addressing was widened. Every plan
        # that is the file agrees with itself, so the only thing that can see it
        # is a plan whose pages have moved --- and the 0 for a page tpdf made is
        # the other half: it would put every mark on an inserted page onto the
        # first page of the document.
        "edits: address a mark by its baseline page rather than its position",
        "src/edits.rs",
        "            let at = u32::try_from(at).unwrap_or(u32::MAX);",
        "            let at = match view.source {\n                PageSource::Baseline(number) => number,\n                PageSource::Blank(_) => 0,\n            };",
        "a_marks_reply_names_the_page_by_identity_and_the_plan_by_position",
    ),
    Mutation(
        # And a crop, which the frontend cannot even measure for such a page ---
        # so the model would be holding a box nothing could have produced.
        "docmodel: let a crop onto a page tpdf made",
        "src/docmodel.rs",
        "                    if matches!(page_now.source, PageSource::Blank(_)) && to.is_some() {",
        "                    if false {",
        "a_page_tpdf_made_takes_no_crop_and_clearing_one_is_not_an_error",
    ),
    Mutation(
        # Refuse clearing one as well. The asymmetry is deliberate: a made page
        # has no crop to clear, so this refuses an operation with no effect ---
        # and the refusal is what a reader sees.
        "docmodel: refuse clearing a crop a made page never had",
        "src/docmodel.rs",
        "                    if matches!(page_now.source, PageSource::Blank(_)) && to.is_some() {",
        "                    if matches!(page_now.source, PageSource::Blank(_)) {",
        "a_page_tpdf_made_takes_no_crop_and_clearing_one_is_not_an_error",
    ),
    Mutation(
        # Read a page tpdf made as the file's own page at that slot. The plan
        # then claims to be the document on disk, and a save takes the append
        # path --- which cannot create a page, so the insert is silently absent
        # from a file the reader was told was written.
        "edits: read a page tpdf made as the file's own page at that slot",
        "src/edits.rs",
        "                    PageSource::Blank(_) => false,\n                    PageSource::Imported { .. } => false,",
        "                    PageSource::Blank(_) => true,\n                    PageSource::Imported { .. } => false,",
        "a_deletion_and_an_insert_together_are_still_not_the_file_on_disk",
    ),
    Mutation(
        # Leave an inserted page out of the reason to rebuild the tree. The page
        # object is created and never linked into `/Kids`, so the file comes out
        # with the pages it had and an orphan the sweep may or may not take.
        "save: do not rebuild the page tree for an inserted page",
        "src/save.rs",
        "    let moved =\n        baselines.len() != plan.pages.len() || baselines.windows(2).any(|two| two[0] >= two[1]);",
        "    let moved = baselines.windows(2).any(|two| two[0] >= two[1]);",
        "an_inserted_page_is_written_between_the_two_it_was_put_between",
    ),
    Mutation(
        # Make the page with no media box. It has no size, so every reader
        # guesses one --- and they do not all guess the same.
        "save: make a blank page with no media box",
        "src/save.rs",
        '                (\n                    "MediaBox",',
        '                (\n                    "NotABox",',
        "an_inserted_page_is_written_between_the_two_it_was_put_between",
    ),
    Mutation(
        # Give it the size of nothing. A zero media box is a page every reader
        # renders as empty and several refuse, and the dictionary still has
        # every key a check counting them would look for.
        "save: make a blank page of no size",
        "src/save.rs",
        "                        Object::Real(size.width as f32),\n                        Object::Real(size.height as f32),",
        "                        Object::Real(0.0),\n                        Object::Real(0.0),",
        "an_inserted_page_is_written_between_the_two_it_was_put_between",
    ),
    Mutation(
        # Spell a plan carrying a made page as the file pages it does have. The
        # list is one entry short, and every consumer of it reads a complete
        # selection of the wrong document.
        "print: list a plan with a page tpdf made as the file pages it does have",
        "src/print.rs",
        "        let PageSource::Baseline(number) = page.source else {\n            return Pages::Unlistable;\n        };",
        "        let PageSource::Baseline(number) = page.source else {\n            continue;\n        };",
        "a_plan_with_a_page_tpdf_made_cannot_be_listed_and_goes_to_the_working_writer",
    ),
]

# --- The importer taking a selection (2026-08-30) ---------------------------
#
# `merge::import` is the piece `docs/PLAN.md` names as carrying over into
# inserting pages from another file. What is new here is the *selection*: the
# whole-document path is `append`, unchanged, and its own tests are the control
# for it. So every mutation below is aimed at a clause that exists only because
# a caller may ask for some of a document rather than all of it.
#
# One clause deliberately has no mutation, and it is worth saying which rather
# than leaving a gap: the `!detached.contains_key(id)` filter that builds
# `left`. A page being taken is in `seen` and in the queue before the walk
# starts, so putting it in the barrier as well changes nothing any test could
# observe --- the filter is there so the set means what its name says, not so
# the walk behaves differently. `docs/TRAPS.md` records an unreachable guard as
# worth keeping and not worth a mutation.
MUTATIONS += [
    Mutation(
        # Detach every page rather than the ones asked for. `detached` is what
        # seeds the walk, so all three pages come across as objects while the
        # answer still names only the one the caller wanted --- a file reporting
        # one page and carrying the text of two others, which is the leak
        # `docs/TRAPS.md` records a shipped extract having.
        "merge: import every page rather than the ones asked for",
        "src/merge.rs",
        "    for &page in &taking {",
        "    for &page in &pages {",
        "only_the_pages_asked_for_come_across",
    ),
    Mutation(
        # Take the wall away. A `/Dest` from a page somebody asked for to a page
        # they did not then imports that page, its resources and its content
        # stream --- the same leak as above, arriving by reference rather than
        # through the seed list, which is why it needs its own mutation.
        "merge: follow a reference into a page nobody asked for",
        "src/merge.rs",
        "            if left.contains(&id) {\n                continue;\n            }\n",
        "",
        "a_page_left_behind_is_not_followed_into",
    ),
    Mutation(
        # Answer in the document's own page order. Every other assertion in the
        # module passes --- the ids are right, the count is right, nothing leaks
        # --- and a caller placing them puts page 3 where it asked for page 1.
        "merge: answer in the document's order rather than the order asked for",
        "src/merge.rs",
        "    taking\n        .iter()\n        .map(|&id| shifted_id(id, by))",
        "    pages\n        .iter()\n        .filter(|id| taking.contains(id))\n        .map(|&id| shifted_id(id, by))",
        "the_answer_is_in_the_order_asked_for",
    ),
    Mutation(
        # Serve a page asked for twice. Two positions then hold one page object,
        # which is the hazard `save::mark_sites` refuses a mark on and
        # `pagetree` refuses a deletion of --- reached here by building such a
        # document rather than by finding one.
        "merge: let one page be asked for twice",
        "src/merge.rs",
        "        if taking.contains(&id) {\n            return Err(format!(\"page {} was asked for twice\", at + 1));\n        }\n",
        "",
        "the_same_page_asked_for_twice_is_refused",
    ),
    Mutation(
        # Take the first page for a position the document does not have. The
        # import succeeds, the answer is the right length, and it names a page
        # nobody asked for.
        "merge: fall back to the first page for a position that is not there",
        "src/merge.rs",
        "        let id = *pages.get(at).ok_or_else(|| {",
        "        let id = *pages.get(at).or(pages.first()).ok_or_else(|| {",
        "a_position_the_document_does_not_have_is_refused",
    ),
]

# The nib a reader picks: the door it arrives through, the two derivations that
# pad by half of it, and the two readers that carry it out.
MUTATIONS += [
    Mutation(
        # Answer the default to everything. Every width a reader picks is thrown
        # away silently: the mark is taken, the file is valid, and the line is
        # the weight it always was. The control in the test is what catches it --
        # every other assertion there is satisfied by a door that clamps
        # correctly and by one that ignores its argument.
        "edits: answer the default nib to every width sent",
        "src/edits.rs",
        "    if value.is_finite() {\n        value.clamp(NIB_MIN, NIB_MAX)",
        "    if true {\n        INK_WIDTH",
        "a_nib_that_is_not_a_number_is_clamped_at_this_door_too",
    ),
    Mutation(
        # Take a non-finite width as sent, which is the fourth door standing
        # open: `1e40` is valid JSON, is `INFINITY` here, and `save.rs` writes it
        # as the three letters `inf` in the middle of a content stream. tpdf
        # would write a file no reader can parse and sign its name to it.
        "edits: let a non-finite nib through the door",
        "src/edits.rs",
        "fn nib(value: f64) -> f64 {\n    if value.is_finite() {",
        "fn nib(value: f64) -> f64 {\n    if true {",
        "a_nib_that_is_not_a_number_is_clamped_at_this_door_too",
    ),
    Mutation(
        # Derive the rectangle from the constant rather than from the mark. At
        # the default nib the two agree exactly, which is why the fixture has to
        # be a second width -- the sibling test that proves the pad exists at all
        # cannot see this.
        "edits: pad a drawing's rectangle by the default nib",
        "src/edits.rs",
        "        let width = nib(want.width);",
        "        let width = INK_WIDTH;",
        "the_rectangle_is_padded_by_half_the_nib_the_reader_chose",
    ),
    Mutation(
        # The same reach for the constant, on the second derivation: `reink` runs
        # after an eraser sweep, so a broad drawing would keep its ink and lose
        # the box around it the first time a reader rubbed one stroke out.
        "docmodel: pad an erased drawing by the default nib",
        "src/docmodel.rs",
        "        let width = self.mark(mark).map_or(INK_WIDTH, |m| m.width);",
        "        let width = INK_WIDTH;",
        "erasing_a_stroke_keeps_the_nib_the_drawing_was_made_with",
    ),
    Mutation(
        # The overlay's copy. PDFium draws the mark inside the tile at the width
        # the file carries and the overlay redraws ink on top from this; a
        # constant here puts the default weight over the reader's, which reads as
        # a rendering fault rather than as a wrong number.
        "edits: report the default nib to the overlay",
        "src/edits.rs",
        "                width: mark.width,\n                note: model.note_of(id).to_string(),",
        "                width: INK_WIDTH,\n                note: model.note_of(id).to_string(),",
        "the_reply_and_the_plan_both_carry_the_nib",
    ),
    Mutation(
        # The writer's copy, and the one that reaches a file. Together with the
        # mutation above this says the two readers are fed separately: a mark
        # that reached one with the chosen nib and the other with the default is
        # a drawing that changes weight the moment it is saved.
        "edits: plan the default nib for the writer",
        "src/edits.rs",
        "                    width: body.width,",
        "                    width: INK_WIDTH,",
        "the_reply_and_the_plan_both_carry_the_nib",
    ),
    Mutation(
        # Write the constant into the appearance stream. Every reader on earth
        # then draws the default, whatever the reader picked and whatever the
        # overlay showed them while they drew it.
        "save: write the default nib rather than the mark's",
        "src/save/marks.rs",
        '        Paint::Path => (mark.width, "1 J 1 j "),',
        '        Paint::Path => (crate::docmodel::INK_WIDTH, "1 J 1 j "),',
        "the_stroke_width_is_the_nib_the_reader_chose",
    ),
]

# Deleting a comment the file came with: the one edit that cannot be an append,
# the sweep clause that keeps its bytes out of the file, and the refusal replying
# made it need.
MUTATIONS += [
    Mutation(
        # Let a deletion append. An append only adds objects, so the highlight
        # beside it is written, the deletion is dropped, and the save reports
        # success -- and a fixture holding only the deletion cannot see it,
        # because with no marks and no note edits the first clause makes the
        # answer `Rewrite` either way.
        "edits: let a deleted comment be written as an append",
        "src/edits.rs",
        "            && self.discards.is_empty()\n",
        "",
        "a_plan_that_only_adds_marks_is_appended_and_anything_else_is_rewritten",
    ),
    Mutation(
        # Stop sweeping after a deletion. `pagetree::forget` removes the
        # annotation and every reference to it, which leaves its appearance
        # stream -- a drawing of the words the reader deleted -- reachable from
        # nothing and written out regardless. The page draws nothing and every
        # byte is still there, which is the picture's trap in the one place the
        # leftover is text somebody asked to be rid of.
        "save: leave a deleted comment's bytes in the file",
        "src/save.rs",
        "        || discarded > 0\n",
        "",
        "a_deleted_comment_leaves_the_page_and_leaves_no_bytes_behind",
    ),
    Mutation(
        # Take the deletion off the page without removing the object, which is
        # the smaller-looking implementation and is what the redaction path's own
        # comment argues against: a structure element's `/OBJR` or an AcroForm's
        # `/Fields` names the annotation too, so pruning the one list a caller
        # has in mind leaves it alive.
        "save: prune the page's list instead of forgetting the object",
        "src/save/marks.rs",
        "    let taken = doomed.len();\n    crate::pagetree::forget(doc, &doomed).map_err(Refusal::from)?;",
        "    let taken = doomed.len();\n    for id in &doomed {\n        doc.objects.remove(id);\n    }",
        "a_deleted_comment_leaves_the_page_and_leaves_no_bytes_behind",
    ),
    Mutation(
        # Accept a plan naming an object that is not an annotation, which would
        # let a caller delete a font, a page or the catalog out of the file.
        "save: delete whatever object a plan names",
        "src/save/marks.rs",
        "            return Err(\"that comment is not an annotation\".into());\n        }\n        doomed.insert(id);",
        "        }\n        doomed.insert(id);",
        "a_deletion_naming_something_that_is_not_an_annotation_is_refused",
    ),
    Mutation(
        # Let a comment one of the reader's own replies answers be deleted. The
        # reply's `/IRT` then points at nothing, which is a malformed thread in
        # every reader that draws one -- and no reader would see it until the
        # file was reopened somewhere else.
        "docmodel: delete a comment a reply of the reader's own answers",
        "src/docmodel.rs",
        "        if self.answered_by_a_reply(object) {\n            return Err(Refusal::ReplyAnswersIt(object));\n        }\n",
        "",
        "a_comment_a_reply_answers_cannot_be_discarded",
    ),
    Mutation(
        # Count every mark ever made rather than the live ones. A reader who
        # removes their reply is then still refused, for ever, and the refusal's
        # own advice -- delete the reply first -- stops working.
        "docmodel: count removed replies as replies",
        "src/docmodel.rs",
        "        self.now\n            .all_marks()\n            .into_iter()\n            .filter_map(|(_, id)| self.mark(id))",
        "        self.marks\n            .keys()\n            .copied()\n            .filter_map(|id| self.mark(id))",
        "removing_the_reply_makes_the_comment_deletable_again",
    ),
    Mutation(
        # Let a deleted page's discards outlive it. The model then hands the
        # writer a deletion for a comment on a page the plan does not carry.
        "docmodel: keep a deleted page's discards",
        "src/docmodel.rs",
        "                self.discards.retain(|_, on| *on != page);\n",
        "",
        "deleting_a_page_takes_the_discards_on_it_and_no_others",
    ),
    Mutation(
        # Throw the rewrite away when the comment is deleted, which is the
        # tempting single-map implementation. Undoing the deletion then brings
        # the comment back reading what the *file* said rather than what the
        # reader typed.
        "docmodel: drop a comment's edit when it is deleted",
        "src/docmodel.rs",
        "                self.discards.insert(object, page);",
        "                self.rewrites.remove(&object);\n                self.discards.insert(object, page);",
        "a_comment_can_be_rewritten_and_then_discarded_and_undo_keeps_the_edit",
    ),
]


MUTATIONS += [
    Mutation(
        # Step back one entry whatever gesture it belonged to, which is what
        # undo did until 2026-08-30. A sweep of the eraser across five drawings
        # is five commands, so the reader who let go once presses undo five
        # times -- and four comments in four files said otherwise, because a
        # sweep that stays inside one drawing really is one command and is the
        # sweep anyone writing a test reaches for.
        "docmodel: undo one entry whatever gesture it belonged to",
        "src/docmodel.rs",
        """        while let Some(with) = self.grouped(self.cursor) {
            if self.cursor == 0 || self.grouped(self.cursor - 1) != Some(with) {
                break;
            }
            self.cursor -= 1;
        }
""",
        "",
        "one_press_of_undo_puts_back_a_sweep_that_crossed_several_drawings",
    ),
    Mutation(
        # Redo one entry at a time while undo takes a whole gesture. The two
        # counts then disagree about how many presses the reader is from where
        # they were, which is worse than either behaviour on its own.
        "docmodel: redo one entry whatever gesture it belonged to",
        "src/docmodel.rs",
        """        while let Some(with) = self.grouped(self.cursor - 1) {
            if !self.can_redo() || self.grouped(self.cursor) != Some(with) {
                break;
            }
            self.step();
        }
""",
        "",
        "redo_returns_a_whole_sweep_in_one_press",
    ),
    Mutation(
        # Give every ungrouped entry one shared gesture, so that two commands
        # which belong to nothing group with each other. `Option` equality is
        # what makes this reachable: `None == None`, and the property that a
        # command outside a gesture stands alone is carried by the `while let`
        # rather than by any comparison.
        "docmodel: let two commands outside a gesture join each other",
        "src/docmodel.rs",
        "        self.journal[at].sweep\n",
        "        self.journal[at].sweep.or(SweepId::new(u64::MAX))\n",
        "a_command_belonging_to_no_gesture_joins_neither_neighbour",
    ),
    Mutation(
        # Drop the gesture between the door and the model. Every erasure then
        # stands alone, which is the state this whole increment replaced -- and
        # nothing about the reply changes, so only a check that presses undo
        # once and counts what came back can see it.
        "edits: forget which gesture an erasure belonged to",
        "src/edits.rs",
        "        model.apply_in(cmd, sweep).map_err(describe)?;\n",
        "        model.apply(cmd).map_err(describe)?;\n",
        "a_sweep_that_took_two_marks_whole_is_one_undo",
    ),
    Mutation(
        # Read the wire's zero as a gesture. Zero is what every caller but the
        # eraser sends, so this groups unrelated commands into one press of
        # undo -- the failure the non-zero type exists to make unspellable.
        "edits: treat the wire's no-gesture as gesture zero",
        "src/edits.rs",
        "    NonZeroU64::new(value)\n",
        "    Some(NonZeroU64::new(value).unwrap_or(NonZeroU64::MIN))\n",
        "two_erasures_outside_a_gesture_are_two_undos",
    ),
    Mutation(
        # Re-ink outside the gesture while the whole-mark removal stays inside
        # it. A sweep that clipped a drawing and took a highlight then costs two
        # presses, and only a check that crosses both callbacks can see it.
        "edits: re-ink outside the gesture the sweep named",
        "src/edits.rs",
        "            model.reink_in(id, keep, joins).map_err(describe)?;\n",
        "            model.reink(id, keep).map_err(describe)?;\n",
        "a_sweep_that_crossed_two_drawings_is_still_one_undo",
    ),
]

MUTATIONS += [
    Mutation(
        # Read a bottom-up BMP as top-down and the other way round. The image is
        # still the right size, still the right colours and still plausible --- it
        # is upside down, so a mark's rectangle is read from the wrong end of the
        # page and a coverage figure comes back small rather than wrong-looking.
        # A symmetric fixture cannot tell this from an identity, which is why
        # `three_rows` is not symmetric.
        "raster: read the rows in the other direction",
        "src/print_win.rs",
        "        let row = if self.top_down {\n            y\n        } else {\n            self.height - 1 - y\n        };",
        "        let row = if self.top_down {\n            self.height - 1 - y\n        } else {\n            y\n        };",
        "the_top_row_is_the_top_row_whichever_way_the_bmp_stores_it",
        only_on="windows",
    ),
    Mutation(
        # Walk the pixels without the row padding. Three 24-bit pixels are 9 bytes
        # in a 12-byte row, so this drifts three bytes further into the image on
        # every row and reads padding as colour -- which on a mostly-white page is
        # a small plausible count rather than an obvious error.
        "raster: ignore the four-byte row padding",
        "src/print_win.rs",
        "        let stride = (width * bytes_per_pixel).div_ceil(4) * 4;",
        "        let stride = width * bytes_per_pixel;",
        "a_row_is_read_past_the_padding_that_follows_it",
        only_on="windows",
    ),
    Mutation(
        # Take the bounds check off `pixel`. A rectangle that overhangs the page is
        # a question a check may legally ask, and this turns the answer into a
        # panic -- so a check's failure becomes a crash rather than a reading.
        "raster: let a pixel outside the image be read anyway",
        "src/print_win.rs",
        "        if x >= self.width || y >= self.height {\n            return [0xFF, 0xFF, 0xFF];\n        }",
        "        if false {\n            return [0xFF, 0xFF, 0xFF];\n        }",
        "a_pixel_outside_the_image_reads_white_rather_than_panicking",
        only_on="windows",
    ),
    Mutation(
        # Accept an indexed image. Its bytes are palette *indices*, so every
        # comparison downstream would be comparing index numbers as if they were
        # colours -- and two different colours with adjacent indices would read as
        # nearly the same pixel.
        "raster: compare an indexed image without its palette",
        "src/print_win.rs",
        "        if !(bpp == 24 || bpp == 32) {",
        "        if false {",
        "an_indexed_bmp_is_refused_rather_than_compared_without_its_palette",
        only_on="windows",
    ),
]

MUTATIONS += [
    Mutation(
        # Take the changed-file guard off the print path. The job is then built
        # from whatever is on disk now, with the reader's marks placed at
        # coordinates measured on the file still on their screen -- exactly the
        # defect the guard was written for, and the paper does not say so.
        "save: print a file that changed since it was opened",
        "src/save.rs",
        "    print_ready(source, plan)?;\n",
        "    let _ = print_ready(source, plan);\n",
        "a_print_job_over_a_changed_file_is_refused_or_says_so",
    ),
    Mutation(
        # The over-refusal direction: make `print_ready` refuse every job,
        # changed file or not. The guard letting an unchanged file through is
        # the half of it a reader relies on every day, and only the control can
        # see it go.
        "save: refuse a print job whether or not the file changed",
        "src/save.rs",
        "        .map(|_ready| ())\n",
        "        .and_then(|_ready| Err(Refusal::changed(\"changed on disk since you opened it\")))\n",
        "a_print_job_over_a_changed_file_is_refused_or_says_so",
    ),
    Mutation(
        # Let the umask choose the tile mapping's backing-file permissions
        # again. On a machine with the default umask 022 the file is briefly
        # 0644 in the shared temp directory; the test reads the mode through
        # the descriptor, because the path is unlinked on the next statement.
        "worker_shm: let the umask choose the backing file's permissions",
        "src/worker_shm.rs",
        "            .mode(0o600)\n",
        "",
        "the_backing_file_of_a_new_mapping_is_not_readable_by_anyone_else",
        only_on="macos",
    ),
]

# --- the print refusal, as the window receives it ------------------------
MUTATIONS += [
    Mutation(
        # Flatten the changed-file refusal back to its sentence, which is what
        # `print_document` did until 2026-09-01. It compiles, it refuses the
        # print, and the message the reader sees is word for word the right one
        # -- only the flag that lets the window put Save a copy and Reload
        # beside it is gone. Nothing but an assertion on the flag can see this:
        # `docs/TRAPS.md`, "A refusal flattened to a string across a process
        # boundary loses the action that answers it".
        "print: flatten the print refusal to its message again",
        "src/commands/print.rs",
        "            save::print_ready(source, plan)?;",
        "            save::print_ready(source, plan)\n"
        "                .map_err(|refused| save::Refusal::from(refused.message))?;",
        "a_print_of_a_replaced_file_rejects_with_the_flag_that_offers_reload",
    ),
    Mutation(
        # Rename the wire field the window branches on. The rejection is still
        # a two-field object carrying the right sentence and the right value,
        # and the frontend reads a name that is not there as absent -- so the
        # reader is told their file moved and offered nothing to do about it.
        # A rename is what a refactor of this struct looks like, which is why
        # the assertion is against a literal rather than a round trip.
        "save: rename the refusal field the window reads",
        "src/save.rs",
        "    /// The source changed on disk since the reader opened it.\n    pub changed: bool,",
        "    /// The source changed on disk since the reader opened it.\n"
        '    #[serde(rename = "fileChanged")]\n'
        "    pub changed: bool,",
        "the_wire_shape_of_a_refusal_is_a_message_and_a_changed_flag",
    ),
]

# --- the redaction read-back, the last parse to leave the coordinator -----
MUTATIONS += [
    Mutation(
        # Scan in the coordinator, which is what every redaction did until
        # 2026-09-01. The report is identical for any file that parses, so no
        # comparison of reports can see this -- only an assertion on WHO was
        # asked, over bytes this process could not have produced that answer
        # from.
        "verify: scan the written file in the coordinator instead of the verifier",
        "src/commands/redact.rs",
        "    scanning.scan(&mut file, len, needles, password)",
        "    let _ = scanning;\n"
        "    let mut bytes = Vec::new();\n"
        "    std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|e| e.to_string())?;\n"
        "    Ok(verify::scan(&bytes, needles, password))",
        "the_redaction_read_back_does_not_parse_the_file_it_wrote",
    ),
    Mutation(
        # Answer an empty report for a file that could not be opened. Every
        # field is zero, `found` is empty, and `Verdict::Verified` follows --
        # so a redaction whose output vanished between the rename and the read
        # would be certified clean. This is the exact shape `docs/PLAN.md` 6
        # forbids, and it is one `?` away at all times.
        "verify: certify a file that could not be opened at all",
        "src/commands/redact.rs",
        "    let mut file = std::fs::File::open(at)\n"
        '        .map_err(|why| format!("the redacted file could not be read back: {why}"))?;',
        "    let Ok(mut file) = std::fs::File::open(at) else {\n"
        "        return Ok(verify::Report::default());\n"
        "    };",
        "a_read_back_of_a_file_that_is_not_there_is_an_error",
    ),
    Mutation(
        # Take the cap off one arm. The report then carries a reason per object
        # and the reply it travels in stops fitting -- which the reader meets as
        # a verification that FAILED rather than as a file with a lot wrong with
        # it. Only the arm the fixture reaches is mutated: the three are
        # deliberately separate anchors, since a single one would be ambiguous.
        "verify: list every unrecognised object rather than the bounded number",
        "src/verify.rs",
        "                    Carrier::Unrecognised { filter } => {\n"
        "                        blind_objects += 1;\n"
        "                        if blind_objects <= MAX_OBJECT_REASONS {",
        "                    Carrier::Unrecognised { filter } => {\n"
        "                        blind_objects += 1;\n"
        "                        if blind_objects <= usize::MAX {",
        "a_report_lists_a_bounded_number_of_objects_and_counts_the_rest",
    ),
    Mutation(
        # Say nothing was left over. The list is still bounded and the verdict
        # is still a refusal, so every check but the arithmetic passes -- and a
        # reader is told a thousand objects are unaccounted for when the number
        # is twelve hundred. A cap that does not report what it dropped is a cap
        # that hides the worst files best.
        "verify: report nothing left over when the object list is shortened",
        "src/verify.rs",
        "                    blind_objects - MAX_OBJECT_REASONS",
        "                    0",
        "a_report_lists_a_bounded_number_of_objects_and_counts_the_rest",
    ),
]


# --- the ordering, the bounds and the handle a save's guards rest on ---------
MUTATIONS += [
    Mutation(
        # Send the answer and release the worker afterwards. Every path through
        # `InWorker` hands its worker a mapping of the file the coordinator is
        # about to resize: on Windows a file with a section open on it cannot be
        # shortened at all, so the roll-back after a refused read-back fails with
        # ERROR_USER_MAPPED_FILE and the reader keeps the update that was just
        # refused. Nothing about that is visible on macOS, where ftruncate
        # succeeds either way -- which is why the check is on the ordering.
        "save_outside: send the worker's answer before releasing its mapping",
        "src/save_outside.rs",
        "        let answer = ask(&mut resource);\n"
        "        // Before the send, never after. See the note above.\n"
        "        drop(resource);\n"
        "        let _ = tx.send(answer);",
        "        let answer = ask(&mut resource);\n"
        "        let _ = tx.send(answer);\n"
        "        drop(resource);",
        "a_worker_is_released_before_its_answer_is_sent",
    ),
    Mutation(
        # Signal outside the table's lock, which is where it was until an
        # overdue reply arriving in the gap was traced through: the blocked
        # thread reads `killed`, takes the entry and hands its worker to
        # `discard`, which reaps the child -- and the pid is free before the
        # signal is sent. Windows reissues one immediately.
        "workers: signal an overdue worker after releasing the in-flight table",
        # The first spelling of this mutation kept the second loop *inside* the
        # block that holds `calls`, so the lock was still held when it signalled
        # and the test rightly saw nothing: a variant, not the defect. The kill
        # has to land after the block's closing brace to be outside the lock.
        "src/workers.rs",
        "            for call in calls.iter_mut().filter(|call| overdue.contains(&call.pid)) {\n"
        "                call.killed = true;\n"
        "                kill(call.pid);\n"
        "            }\n"
        "            overdue\n"
        "        };\n",
        "            for call in calls.iter_mut().filter(|call| overdue.contains(&call.pid)) {\n"
        "                call.killed = true;\n"
        "            }\n"
        "            overdue\n"
        "        };\n"
        "        for pid in &overdue {\n"
        "            kill(*pid);\n"
        "        }\n",
        "an_overdue_worker_is_signalled_while_its_entry_is_still_held",
    ),
    Mutation(
        # Keep the claim after a spare that never warmed. `prewarm` returns
        # early whenever `warming` is set, so this is not one lost spare: no
        # spare is ever started again for the life of the process, and every
        # open from then on pays the link and the font walk on the reader's
        # thread. The only symptom is opens that are quietly slower.
        "workers: keep the spare slot claimed after a warm that timed out",
        "src/workers.rs",
        "    let answered = rx.recv_timeout(within).ok();\n"
        "    slot.lock().unwrap_or_else(|e| e.into_inner()).warming = None;",
        "    let answered = rx.recv_timeout(within).ok();\n"
        "    if answered.is_some() {\n"
        "        slot.lock().unwrap_or_else(|e| e.into_inner()).warming = None;\n"
        "    }",
        "a_spare_that_never_warms_gives_the_slot_back",
        # The stand-in for a silent spare is a real process, so the test is
        # `#[cfg(unix)]` and this mutation reports SURVIVED on Windows rather
        # than blocking the run there.
        only_on="macos",
    ),
    Mutation(
        # Take the ceiling off a merge's inputs, which is how it was: every
        # incoming file read into one buffer with nothing in the way, in a
        # process that then copies it. A file dialog takes as many files as
        # somebody cares to shift-click.
        "save: merge as many incoming bytes as the reader picked",
        "src/save.rs",
        "        if total > ceiling {",
        "        if false {",
        "more_incoming_bytes_than_the_ceiling_are_refused_with_something_to_do",
    ),
    Mutation(
        # Add the byte-range lengths plainly. Three of `i64::MAX` is a legal
        # array and a five-line file: the sum panics under debug assertions and
        # wraps in the shipped build, where it becomes a coverage figure the
        # panel prints as fact.
        "docinfo: add a signature's byte-range lengths without saturating",
        "src/docinfo.rs",
        "            .fold(0_u64, u64::saturating_add);",
        "            .sum();",
        "byte_range_lengths_that_cannot_be_added_saturate_rather_than_wrap",
    ),
    Mutation(
        # Walk the `/Parent` chain with the trip count as the only guard. A loop
        # in it then yields the same ancestors until the bound runs out, and
        # every lap decrements a `/Count`: one deleted page took a three-page
        # tree to -29.
        "pagetree: walk a page's ancestors without a visited set",
        "src/pagetree.rs",
        "            if !seen.insert(parent) {\n"
        "                break;\n"
        "            }\n"
        "            decrements.push(parent);",
        "            decrements.push(parent);",
        "a_cyclic_parent_chain_decrements_each_ancestor_once",
    ),
    Mutation(
        # Decrement the count plainly. `/Count -9223372036854775808` is as
        # writable as any other number, and this panics on it under debug
        # assertions and wraps to the largest count there is in release.
        "pagetree: decrement a node's count without saturating",
        "src/pagetree.rs",
        '                tree.set("Count", count.saturating_sub(1));',
        '                tree.set("Count", count - 1);',
        "a_count_at_the_bottom_of_its_range_saturates_rather_than_wrapping",
    ),
    Mutation(
        # Fingerprint the name rather than the handle it was handed. The
        # document is mapped for the workers on one thread while this hashes on
        # another, so this is a second lookup of one name -- and a file replaced
        # in that window by a revision with the same page count leaves the model
        # describing bytes nobody has seen, with every other guard agreeing.
        "fingerprint: hash the pathname again instead of the handle handed over",
        "src/fingerprint.rs",
        "    pub fn of_open(file: &File, what: &Path) -> Result<Fingerprint, String> {\n"
        "        let meta = file",
        "    pub fn of_open(_handed: &File, what: &Path) -> Result<Fingerprint, String> {\n"
        "        let file = &File::open(what)\n"
        '            .map_err(|e| format!("could not open {} to fingerprint it: {e}", what.display()))?;\n'
        "        let meta = file",
        "a_fingerprint_through_a_handle_is_of_the_file_that_was_opened",
    ),
]

# Web links, added 2026-09-07 with the feature. The four in `weburl.rs` are the
# allowlist and the three refusals beside it; the three in `webopen.rs` are the
# app process keeping the addresses away from the webview, which is the property
# `docs/THREAT-MODEL.md` T8 now rests on and which no behaviour a reader can see
# would report if it stopped holding.
MUTATIONS += [
    Mutation(
        # Open every scheme. The allowlist is the whole of the difference
        # between "tpdf opens web links" and "tpdf opens whatever a document
        # names", and on Windows the interesting schemes are the ones nobody
        # has heard of.
        "weburl: accept every scheme rather than http and https",
        "src/weburl.rs",
        '        if !matches!(url.scheme(), "http" | "https") {\n'
        "            return None;\n"
        "        }",
        "",
        "every_other_scheme_is_refused",
    ),
    Mutation(
        # Allow an embedded credential through. `https://bank.example@evil/` is
        # the one case where the host a reader picks out of the raw text is not
        # the host the request reaches.
        "weburl: allow a URL carrying a username or password",
        "src/weburl.rs",
        "        if !url.username().is_empty() || url.password().is_some() {\n"
        "            return None;\n"
        "        }",
        "",
        "an_embedded_credential_is_refused",
    ),
    Mutation(
        # Check for control characters only *after* the parse. `Url::parse`
        # strips tabs and newlines rather than refusing them, so the survivor of
        # this edit is a URL whose displayed form differs from the file's with
        # nothing reporting it.
        "weburl: look for control characters only after the parser has run",
        "src/weburl.rs",
        "        if raw.chars().any(is_unsafe_display) {\n"
        "            return None;\n"
        "        }\n\n"
        "        let url = Url::parse(raw).ok()?;",
        "        let url = Url::parse(raw).ok()?;",
        "a_control_character_is_refused_even_though_the_parser_strips_it",
    ),
    Mutation(
        # Show the whole path. The truncation is what keeps a reader's eye on
        # the host rather than on a reassuring string a stranger wrote.
        "weburl: show the path in full rather than cutting it",
        "src/weburl.rs",
        "    let mut out: String = rest.chars().take(MAX_REST_CHARS).collect();\n"
        "    if rest.chars().nth(MAX_REST_CHARS).is_some() {\n"
        "        out.push(ELLIPSIS);\n"
        "    }\n"
        "    out",
        "    rest.to_string()",
        "a_long_path_is_cut_and_says_so",
    ),
    Mutation(
        # Copy the addresses instead of taking them, so every URL in the
        # document continues to the webview. Nothing a reader can see changes:
        # links open, the dialog says the same thing, and the property T8 rests
        # on is gone.
        "webopen: copy the addresses out of the reply instead of draining them",
        "src/webopen.rs",
        "        let parsed: Addresses = std::mem::take(urls)\n            .iter()",
        "        let parsed: Addresses = urls\n            .iter()",
        "adopting_empties_the_list_the_frontend_would_receive",
    ),
    Mutation(
        # Drop the entries that do not parse rather than leaving a hole. The
        # list shortens, so every token after the hole names somebody else's
        # address -- the one failure here that opens the wrong page rather than
        # none.
        "webopen: drop an unparseable address instead of leaving a hole",
        "src/webopen.rs",
        "            .map(|raw| Web::parse(raw))\n            .collect();",
        "            .filter_map(|raw| Web::parse(raw))\n            .map(Some)\n            .collect();",
        "an_address_that_does_not_parse_leaves_a_hole_and_does_not_shift_the_rest",
    ),
    Mutation(
        # Forget only the links, leaving the outline's addresses behind under a
        # document number the service is about to reuse.
        "webopen: forget a document's links but not its outline",
        "src/webopen.rs",
        "        held.remove(&(document, Source::Links));\n"
        "        held.remove(&(document, Source::Outline));",
        "        held.remove(&(document, Source::Links));",
        "forgetting_a_document_makes_its_tokens_name_nothing",
    ),
    Mutation(
        # Give every web target token 0. One link works and every other one
        # opens the first link's address.
        "links: number every web target zero rather than by position",
        "src/links.rs",
        "    let token = urls.len() as u32;\n    urls.push(web.url);",
        "    let token = 0;\n    urls.push(web.url);",
        "each_web_target_indexes_its_own_address",
    ),
]

# Added 2026-09-07 with the shared-XObject fix. A letterhead drawn on every page
# used to make the *writer* refuse a redaction, which took the region's words
# with it -- so the arithmetic that decides which drawings are shared, and the
# translation from an image ordinal to the object a reader is told about, are
# where a silent wrong answer costs a redaction rather than a warning.
MUTATIONS += [
    Mutation(
        # Leave the picture and say nothing, which is the file that reads as
        # redacted and is not: the words go, the logo stays, and the verdict
        # calls it clean because nothing was reported.
        "shared: leave a repeated picture in the plan without reporting it",
        "src/redact.rs",
        '        left.push(Unhandled {\n'
        '            at: image_at.get(*ordinal).copied().unwrap_or(*ordinal),\n'
        '            kind: "image".to_string(),\n'
        '            drawn: Some(times),\n'
        '        });\n'
        "        false",
        "        let _ = times;\n        true",
        "a_shared_picture_is_left_and_reported_rather_than_removed",
    ),
    Mutation(
        # Call every picture shared. The control, and the direction that costs a
        # capability rather than a promise: no region would remove any picture
        # from any document, and every check about a repeated one still passes.
        "shared: treat an unrepeated picture as repeated",
        "src/redact.rs",
        "        let Some(times) = shared.images.get(*ordinal).copied().flatten() else {",
        "        let Some(times) = shared.images.get(*ordinal).copied().flatten().or(Some(1)) else {",
        "a_picture_that_is_not_shared_is_left_in_the_plan",
    ),
    Mutation(
        # Answer positionally when the positions are in doubt. `remove_images`
        # refuses on the same disagreement, so this is the edit that turns a
        # refusal a reader can act on into a picture silently left behind.
        "shared: answer from a draw count that disagrees with PDFium",
        "src/redact.rs",
        "    if names.len() != expected {\n        return vec![None; expected];\n    }",
        "    if false {\n        return vec![None; expected];\n    }",
        "an_image_count_that_disagrees_answers_nothing_at_all",
    ),
    Mutation(
        # Report nothing shared, ever. The control over `shared_draws` itself:
        # without it every check here could be satisfied by a function that
        # always says *drawn once*, which is what the module did before.
        "shared: report every drawing as repeated",
        "src/redact.rs",
        "                .and_then(|id| drawn_more_than_once(doc, id, names, name))",
        "                .and_then(|id| drawn_more_than_once(doc, id, names, name).or(Some(2)))",
        "a_picture_this_page_alone_draws_is_not_reported_as_shared",
    ),
    Mutation(
        # Ask about the page's forms with an empty name list. Every form comes
        # back unrepeated, so a shared header's text is planned for removal and
        # the writer refuses the whole redaction -- the defect, restored on the
        # half of it that is about forms.
        "shared: ask nothing about the page's forms",
        "src/redact.rs",
        "            &form_draws(doc, page, &content).unwrap_or_default(),",
        "            &[],",
        "a_form_a_second_page_also_names_is_counted",
    ),
    Mutation(
        # Name the picture by its ordinal among images rather than by its
        # position in the page's object list. Both are small integers and they
        # agree on a page whose only object is a picture, which is what most
        # fixtures are.
        "shared: report the image ordinal as the object a reader sees",
        "src/redact.rs",
        "            at: image_at.get(*ordinal).copied().unwrap_or(*ordinal),",
        "            at: *ordinal,",
        "a_finding_names_the_page_object_rather_than_the_image_ordinal",
    ),
    Mutation(
        # Report a shared form once per line of its text the region covers. The
        # count is right for a region over one line and wrong for every larger
        # one, which is the shape `Unhandled::at` exists to prevent.
        "shared: report a repeated form once per covered line",
        "src/redact.rs",
        '        if !left\n'
        '            .iter()\n'
        '            .any(|other| other.at == *at && other.kind == "form")\n'
        "        {",
        "        if true {",
        "a_shared_form_is_one_finding_however_many_of_its_lines_are_covered",
    ),
    Mutation(
        # Look up the first form for every one of them. One shared form on a
        # page then takes every other form's text out of the plan with it, and
        # a page with one form -- which is most of them -- cannot tell.
        "shared: read one form's repeat count for all of them",
        "src/redact.rs",
        "            .position(|form| form.at == *at)",
        "            .position(|_| true)",
        "a_shared_form_does_not_take_another_forms_lines_with_it",
    ),
    Mutation(
        # Append the findings in the order they were computed. A reader checks a
        # page from the top, and a shared picture reported after a shading
        # further down the sheet reads as a second page's finding.
        "shared: leave the findings in the order they were appended",
        "src/redact.rs",
        "    plan.unhandled.sort_by_key(|object| object.at);",
        "    let _ = &plan.unhandled;",
        "findings_stay_in_page_order_when_a_shared_one_joins_them",
    ),
    Mutation(
        # Say the one sentence for both reasons. A reader whose letterhead was
        # left is told tpdf cannot handle their picture, which sends them to
        # look for a different tool rather than at their own document.
        "shared: explain a repeated object as one tpdf cannot handle",
        "src/redact.rs",
        "        let Unhandled { at, kind, drawn } = self;",
        "        let Unhandled { at, kind, drawn: _ } = self;\n        let drawn = &None::<usize>;",
        "a_repeated_object_says_why_it_stayed_and_an_ordinary_one_does_not",
    ),
]

MUTATIONS += [
    Mutation(
        "raster: accept a snapshot whose digest differs from the opened source",
        "src/save_outside.rs",
        "    if digest != expected.digest {",
        "    if false && digest != expected.digest {",
        "raster_snapshot_binds_bytes_and_survives_source_changes",
    ),
    Mutation(
        "raster: allow overwriting the source with an image-only copy",
        "src/save.rs",
        '    if same_file(source, out) {\n        return Err(\n'
        '            "Choose a different name for the image-only copy; the original must be kept".into(),',
        '    if false {\n        return Err(\n'
        '            "Choose a different name for the image-only copy; the original must be kept".into(),',
        "raster_copy_never_overwrites_its_source",
    ),
]

MUTATIONS += [
    Mutation(
        "strict parsing: recover a damaged signed prefix",
        "src/docinfo.rs",
        "                // which objects belonged to the signed revision.\n"
        "                strict: true,",
        "                // which objects belonged to the signed revision.\n"
        "                strict: false,",
        "an_appendix_whose_signed_prefix_is_unparseable_is_reported_as_unread",
    ),
    Mutation(
        "strict parsing: repair a saved file instead of rejecting its broken table",
        "src/save.rs",
        "pub fn reread_pages(bytes: &[u8], password: Option<&str>) -> Result<usize, String> {\n"
        "    Document::load_mem_with_options(\n"
        "        bytes,\n"
        "        lopdf::LoadOptions {\n"
        "            strict: true,",
        "pub fn reread_pages(bytes: &[u8], password: Option<&str>) -> Result<usize, String> {\n"
        "    Document::load_mem_with_options(\n"
        "        bytes,\n"
        "        lopdf::LoadOptions {\n"
        "            strict: false,",
        "an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was",
    ),
]

MUTATIONS += [
    Mutation(
        "visual signature: discard transparency when writing its mask",
        "src/signature.rs",
        "            alpha.push(pixel[3]);",
        "            alpha.push(255);",
        "signature_pixels_alpha_and_placement_survive_append_and_rewrite",
    ),
    Mutation(
        "visual signature: accept an invisible raster",
        "src/signature.rs",
        "            && self.rgba.chunks_exact(4).any(|p| p[3] != 0)",
        "            && !self.rgba.is_empty()",
        "signature_rasters_reject_bad_lengths_limits_and_invisible_pixels",
    ),
]

MUTATIONS += [
    Mutation("boxed edit: lose following cursor", "src/textedit/layout.rs",
        "let adjustment = (-context.cursor_after * 1000. / run.size) as f32;",
        "let adjustment = 0_f32;",
        "layout_rotations_and_continued_shows_preserve_later_geometry"),
    Mutation("boxed edit: lose original line origin", "src/textedit/layout.rs",
        "    operations.extend(restore_line(",
        "    drop(restore_line(",
        "layout_rotations_and_continued_shows_preserve_later_geometry"),
    Mutation("line restore: skip the line moves", "src/textedit/layout.rs",
        '"Td" | "TD" | "T*" | "TL" => restored.push(op.clone()),',
        '"Td" | "TD" | "T*" | "TL" => {}',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: replay T* with the run's leading", "src/textedit/layout.rs",
        'restored.push(numeric("TL", &[leading]));',
        '',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: identity at a different origin", "src/textedit/layout.rs",
        '"BT" => Operation::new("Tm", [1, 0, 0, 1, 0, 0]',
        '"BT" => Operation::new("Tm", [1, 0, 0, 1, 0, 1]',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: identity for a Tm too", "src/textedit/layout.rs",
        '"Tm" => start.clone(),',
        '"Tm" => Operation::new("Tm", [1, 0, 0, 1, 0, 0].map(Object::Integer).to_vec()),',
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: no leading recorded at BT", "src/textedit.rs",
        "matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];\n                line_origin = (index, leading);",
        "matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];\n                line_origin = (index, 0.0);",
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("line restore: start at the block, not the Tm", "src/textedit.rs",
        "cursor = 0.0;\n                line_origin = (index, leading);",
        "cursor = 0.0;",
        "a_layout_leaves_the_following_line_matrix_exactly_as_the_source_had_it"),
    Mutation("boxed edit: omit wrapped line movement", "src/textedit/layout.rs",
        "let dy = first_baseline - index as f64 * line_height;",
        "let dy = first_baseline;",
        "wrapping_keeps_spaces_line_breaks_and_following_text_in_place"),
    Mutation("boxed edit: bypass document clipping", "src/textedit/layout.rs",
        'clipping::contains(context.clip, ink).map_err',
        'clipping::contains(None, ink).map_err',
        "fallback_deletion_and_clipped_growth_remain_editable_or_refuse_atomically"),
    Mutation("boxed edit: allow collisions with other lines", "src/textedit/layout.rs",
        "if intersection[2] > intersection[0] + 0.1",
        "if false && intersection[2] > intersection[0] + 0.1",
        "overflowing_clipped_colliding_and_invalid_layouts_refuse_atomically"),
    Mutation("boxed edit: suppress automatic font fallback", "src/textedit/layout.rs",
        "EditFont::Auto if encodable => None,",
        "EditFont::Auto if encodable || true => None,",
        "fallback_embeds_new_characters_and_reopens_for_another_edit"),
]

# Pages of another file, 2026-09-19: the model's import command and the rewrite
# that places the pages. Each is aimed at the one test whose job it is, and the
# three `pages_are_the_file` mutations above were re-aimed the same day, when
# that predicate became an exhaustive `match`.
MUTATIONS += [
    Mutation(
        # Accept an empty selection at the entry point. The apply takes it, so
        # a source record and a selection id are spent on a command that places
        # nothing --- the refusal this replaces is what keeps the tables empty.
        "docmodel: import: accept an empty selection",
        "src/docmodel.rs",
        "        if pages.is_empty() {\n            return Err(Refusal::EmptyImport);\n        }",
        "",
        "a_refused_import_spends_no_id_and_holds_no_body",
    ),
    Mutation(
        # Place a page the other file does not have. The model has no other way
        # to know the count, and the writer would refuse the whole save later.
        "docmodel: import: accept a page past the end of the file",
        "src/docmodel.rs",
        "        if let Some(&past) = pages.iter().find(|&&page| page >= source.pages) {",
        "        if let Some(&past) = pages.iter().find(|&&page| page > source.pages) {",
        "a_refused_import_spends_no_id_and_holds_no_body",
    ),
    Mutation(
        # Accept a page named twice in one selection. The writer imports a
        # selection in one walk and refuses the repeat, so the save fails for a
        # command the model took.
        "docmodel: import: accept the same page twice in one selection",
        "src/docmodel.rs",
        "        if let Some(&twice) = pages.iter().find(|&&page| !seen.insert(page)) {",
        "        if let Some(&twice) = pages.iter().find(|_| false) {",
        "a_refused_import_spends_no_id_and_holds_no_body",
    ),
    Mutation(
        # Put the imported pages at the front whatever they were anchored to.
        "docmodel: import: ignore the anchor",
        "src/docmodel.rs",
        "                let start = match after {\n                    None => 0,\n                    Some(anchor) => self.position(anchor) + 1,\n                };",
        "                let start = 0;",
        "an_import_places_its_pages_behind_the_anchor_in_the_order_asked",
    ),
    Mutation(
        # Keep a discarded import's selection. Nothing reads it, so no working
        # document differs; the accounting observable is what can say so.
        "docmodel: import: keep a discarded selection body",
        "src/docmodel.rs",
        "                Command::Import { selection, .. } => {\n                    self.selections.remove(&selection);\n                }",
        "                Command::Import { .. } => {}",
        "a_discarded_import_takes_its_selection_and_its_file_record_with_it",
    ),
    Mutation(
        # Keep a file record no selection names.
        "docmodel: import: keep a file record nothing names",
        "src/docmodel.rs",
        "        self.sources.retain(|id, _| named.contains(id));",
        "        let _ = named;",
        "a_discarded_import_takes_its_selection_and_its_file_record_with_it",
    ),
    Mutation(
        # Let a region be marked while a page of another file is in the
        # document. No step that proves a redaction clean can see that page.
        "docmodel: import: redact beside a page of another file",
        "src/docmodel.rs",
        "        if self.now.holds_imported_pages() {\n            return Err(Refusal::RedactionBesideImportedPages);\n        }",
        "",
        "a_redaction_is_refused_while_any_page_came_from_another_file",
    ),
    Mutation(
        # Import while regions are marked: the other direction of the same rule.
        "docmodel: import: import beside marked regions",
        "src/docmodel.rs",
        "        if !self.now.redactions.is_empty() {\n            return Err(Refusal::ImportBesideRedactions);\n        }",
        "",
        "an_import_is_refused_while_regions_are_marked_for_removal",
    ),
    Mutation(
        # Take a text replacement on a page of another file to the check that
        # compares baseline numbers, which answers with a sentence about text
        # having moved rather than about where the page came from.
        "docmodel: import: send text on an imported page to the baseline check",
        "src/docmodel.rs",
        "        if let PageSource::Imported { .. } = self.now.pages[&page].source {\n            return Err(Refusal::TextOnImportedPage(page));\n        }",
        "",
        "text_on_an_imported_page_is_refused",
    ),
    Mutation(
        # Let a comment of the opened file be deleted off a page of another. The
        # writer would delete whatever the opened file has at that number, on
        # some other page.
        "docmodel: import: accept a foreign comment on an imported page",
        "src/docmodel.rs",
        "            Some(PageSource::Imported { .. }) => Err(Refusal::ForeignCommentOnImportedPage(id)),",
        "            Some(PageSource::Imported { .. }) => Ok(()),",
        "a_foreign_comment_cannot_be_said_to_be_on_an_imported_page",
    ),
    Mutation(
        # Call a plan with a page of another file the file on disk. With a mark
        # beside it the save takes the append, which cannot write the page.
        "edits: read a page of another file as the file's own page",
        "src/edits.rs",
        "                    PageSource::Imported { .. } => false,\n                };",
        "                    PageSource::Imported { .. } => true,\n                };",
        "a_plan_with_a_page_of_another_file_is_never_appendable",
    ),
    Mutation(
        # Hand the rewriter nothing. The pages cannot be imported and every save
        # of such a document is refused.
        "save: import: rewrite without the other files",
        "src/save.rs",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, inputs)?;",
        "    let wrote = rewriter.write(source, len, out, plan, job, password, None)?;",
        "a_rewrite_places_pages_of_another_file_where_the_plan_puts_them",
    ),
    Mutation(
        # Import from a file that changed since the reader chose its pages.
        "save: import: skip the digest check",
        "src/save.rs",
        "        if digest != expected.digest {",
        "        if false {",
        "a_source_changed_since_its_pages_were_inserted_is_refused",
    ),
    Mutation(
        # Ask one walk for a page placed twice, which `merge::import` refuses.
        "save: import: put a repeated page in the same round",
        "src/save.rs",
        "                .find(|round| round.iter().all(|&(_, taken)| taken != page))",
        "                .find(|_| true)",
        "the_same_page_placed_twice_is_two_pages",
    ),
    Mutation(
        # Drop an imported page's crop: `agreed_crops` keys by baseline page.
        "save: import: forget an imported page's crop",
        "src/save.rs",
        "    crops.extend(",
        "    let _ = (",
        "a_mark_a_turn_and_a_crop_on_an_imported_page_land_on_it",
    ),
    Mutation(
        # Ask only the trailer whether the other file is encrypted, which
        # `lopdf` empties the moment the empty password opens it.
        "save: import: miss encryption the empty password opened",
        "src/save.rs",
        "        if document.was_encrypted() || document.is_encrypted() {\n            return Err(format!(\n                \"{label} is encrypted, and inserting",
        "        if document.is_encrypted() {\n            return Err(format!(\n                \"{label} is encrypted, and inserting",
        "a_source_an_empty_password_opens_is_refused_rather_than_decrypted",
    ),
    Mutation(
        # Leave the page count to `merge::import`, in the half that writes.
        "save: import: skip the other file's page count",
        "src/save.rs",
        "                    if number as usize >= has {",
        "                    if false {",
        "a_page_the_other_file_does_not_have_is_refused",
    ),
    Mutation(
        # Let a merge reach the rewrite's refusal about a file it was not given.
        "save: import: merge a document holding imported pages",
        "src/save.rs",
        "    if !plan.sources.is_empty() {\n        return Err(\n            \"save this document before merging it",
        "    if false {\n        return Err(\n            \"save this document before merging it",
        "a_merge_of_a_document_holding_imported_pages_says_why_it_is_refused",
    ),
]


# The render handles of the other files a document imports from, and the
# refusals of the command that opens them. What each of these costs is a
# sandboxed worker pool leaked, or a page drawn from nowhere --- neither of
# which a reader can see until the machine has a dozen of them.
MUTATIONS += [
    Mutation(
        # Close a document and hand back nothing, leaking a pool per file.
        "edits: sources: close without handing the handles back",
        "src/edits.rs",
        "        let mut held: Vec<u32> = open.sources.into_values().collect();",
        "        let mut held: Vec<u32> = Vec::new();",
        "closing_a_document_hands_back_every_file_it_imported_from",
    ),
    Mutation(
        # Release on undo, which the redo tail still needs.
        "edits: sources: release the handles when the import is undone",
        "src/edits.rs",
        "        let model = &mut open.model;\n        model.undo();",
        "        let model = &mut open.model;\n        model.undo();\n        open.sources.clear();",
        "undoing_an_import_keeps_the_handle_and_closing_releases_it",
    ),
    Mutation(
        # Hand back the first handle of a file imported again, which releases
        # the pool the pages on screen are drawn from.
        "edits: sources: hand back the handle in use for a file imported again",
        "src/edits.rs",
        "            Some(_) => Some(handle),",
        "            Some(&kept) => Some(kept),",
        "importing_the_same_file_again_hands_back_the_second_handle",
    ),
    Mutation(
        # Answer without the handles, so the frontend has nowhere to draw from.
        "edits: sources: leave the handles out of the reply",
        "src/edits.rs",
        "    state.sources = sources;\n",
        "",
        "an_import_answers_the_handle_its_pages_are_drawn_from",
    ),
    Mutation(
        # Hand an imported page's number to the text edit as a page of the
        # opened file --- which edits whatever page of it has that number.
        "read: text: edit an imported page as a page of the opened file",
        "src/commands/read.rs",
        "        Some(PageSource::Baseline(index)) => Ok(index),",
        "        Some(PageSource::Baseline(index) | PageSource::Imported { page: index, .. }) => Ok(index),",
        "an_imported_page_says_why_its_text_cannot_be_edited",
    ),
    Mutation(
        # A guard dropped without being kept releases nothing: every refusal
        # after the open leaks the pool it opened.
        "imports: release nothing when the guard is dropped",
        "src/imports.rs",
        "        if let Some(release) = self.release.take() {\n            release(self.id);\n        }",
        "        let _ = self.release.take();",
        "a_refusal_after_the_open_releases_the_handle",
    ),
    Mutation(
        # Keep and release anyway, which closes the pool the model just
        # recorded as the one its pages are drawn from.
        "imports: release a handle that was kept",
        "src/imports.rs",
        "        self.release = None;\n        self.id",
        "        self.id",
        "a_kept_handle_is_the_caller_s_and_is_not_released",
    ),
    Mutation(
        # Pass a locked file on as the open's own sentence, which offers a
        # password for a file the save would refuse anyway.
        "imports: word a locked file like any other failure",
        "src/imports.rs",
        "    if refusal.locked {\n        return locked(name);\n    }",
        "",
        "a_locked_file_is_refused_as_encrypted",
    ),
]


# The file an insert holds open while the reader names its pages, 2026-09-19.
# Every path out of the wait has to end it --- placed, replaced, cancelled,
# refused or closed --- and a stale answer must not end a newer one.
MUTATIONS += [
    Mutation(
        # Close a document and forget the file it was waiting on.
        "edits: pending: close without handing back the waiting file",
        "src/edits.rs",
        "        held.extend(open.pending.map(|pending| pending.handle));\n",
        "",
        "closing_a_document_releases_the_file_waiting_for_its_pages",
    ),
    Mutation(
        # Replace the waiting file and leave its pool to nobody.
        "edits: pending: replace without handing back the first file",
        "src/edits.rs",
        "            .map(|previous| previous.handle);",
        "            .map(|_| None).unwrap_or(None);",
        "a_second_prepare_replaces_the_first_and_hands_it_back",
    ),
    Mutation(
        # End whatever is waiting, whatever id the cancel names.
        "edits: pending: cancel a newer import than the one named",
        "src/edits.rs",
        "        let open = docs.get_mut(&doc)?;\n        if open.pending.as_ref().map(|waiting| waiting.id) != Some(pending) {\n            return None;\n        }",
        "        let open = docs.get_mut(&doc)?;",
        "a_cancel_hands_back_the_file_it_names_and_only_that_one",
    ),
    Mutation(
        # Commit whatever is waiting, whatever id the answer names.
        "edits: pending: commit an answer to another question",
        "src/edits.rs",
        "            if open.pending.as_ref().map(|waiting| waiting.id) != Some(pending) {\n                return Err(format!(",
        "            if open.pending.is_none() {\n                return Err(format!(",
        "a_commit_naming_an_import_nobody_is_waiting_on_is_refused",
    ),
    Mutation(
        # Take the file for a commit the model then refuses, and release nothing.
        "edits: pending: leak the file on a refused commit",
        "src/edits.rs",
        "        let held = crate::imports::Held::new(taken.handle, release.clone());",
        "        let held = crate::imports::Held::new(taken.handle, |_| {});",
        "a_refused_commit_releases_the_file",
    ),
    Mutation(
        # Keep a second pool for a file the document already draws from.
        "edits: pending: keep the spare handle of a file already held",
        "src/edits.rs",
        "            debug_assert_eq!(spare, kept);\n            release(spare);",
        "            debug_assert_eq!(spare, kept);",
        "committing_a_file_already_held_releases_the_second_handle",
    ),
]


if __name__ == "__main__":
    sys.exit(main())
