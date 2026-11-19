"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.

What the removal leaves behind and what a save over the reader's file checks
before the document is closed.
"""

from .base import Mutation

ROWS: list[Mutation] = []

ROWS += [
    # Saving over the reader's file: refused while the document is still open.
    Mutation("landing: rename over a file Windows will not replace", "src/save.rs", "    replaceable(&target)?;\n", "", "a_rewrite_over_a_read_only_file_is_refused_before_the_close", only_on="windows"),
    Mutation("landing: append to a file that cannot be written", "src/save.rs", "    openable_to_append(source)?;\n", "", "a_file_that_cannot_be_written_is_refused_before_the_document_is_closed"),
    Mutation("fill: stage beside the link the reader opened", "src/save.rs", "    let path = &resolved(path);\n    let mut plan = redaction_fill::output_plan(original_plan)?;", "    let mut plan = redaction_fill::output_plan(original_plan)?;", "the_redaction_fill_through_a_symlink_writes_the_document_the_link_names", only_on="macos"),
    # Importing pages: an action naming fields is a wall.
    Mutation("import: follow an action into the source's fields", "src/merge.rs", ".is_some_and(is_form_object)", ".is_some_and(|_| false)", "importing_one_page_of_a_form_does_not_bring_the_other_pages_answers"),
    Mutation("import: wall off the page's own merged field", "src/merge.rs", "                && !annotated.contains(&id)\n", "", "a_field_that_is_its_own_widget_comes_with_its_page"),
    # Form fields written out in place.
    Mutation("fields: stop the walk at an inline field", "src/redact.rs", "            return fields::Flow::Descend;\n        };\n\n        let kids: Vec<ObjectId> = field", "            return fields::Flow::Leaf;\n        };\n\n        let kids: Vec<ObjectId> = field", "a_field_under_an_inline_parent_is_reached"),
    Mutation("fields: leave an inline field in its array", "src/redact.rs", "            gone += prune_inline(array, &folded, &mut budget);", "            let _ = (array, &folded, &mut budget);", "an_inline_field_holding_taken_text_goes_with_its_widget"),
    Mutation("fields: the rewrite never asks about inline fields", "src/save.rs", "        done.fields += redact::drop_inline_fields(doc, &lines);\n", "", "a_redaction_takes_an_inline_field_holding_what_it_removed"),
    # Inline annotations that depend on a taken one.
    Mutation("annots: keep an inline reply to a taken note", "src/redact.rs", "                    && !names_taken(annot, b\"IRT\")\n", "", "an_inline_reply_to_a_taken_note_goes_with_it"),
    Mutation("annots: keep an inline popup of a taken note", "src/redact.rs", "                    && !names_taken(annot, b\"Parent\");", ";", "inline_dependents_of_a_taken_annotation_go_with_it"),
    Mutation("annots: keep the popup an inline note names", "src/redact.rs", "                    popups.extend(annot.get(b\"Popup\").and_then(Object::as_reference).ok());\n", "", "inline_dependents_of_a_taken_annotation_go_with_it"),
    Mutation("annots: inline pass told nothing was taken", "src/save.rs", "redact::take_inline_annots(doc, page, &redaction.areas, &mut taken);", "redact::take_inline_annots(doc, page, &redaction.areas, &mut Default::default());", "an_inline_reply_to_a_taken_note_goes_with_it"),
    # Shadow text inside a form, and around a page picture.
    Mutation("form text: keep the span's shadow text", "src/redact.rs", "    let carriers = clear_form_shadow_text(doc, page, id, &mut inside.operations, &positions)?;", "    let carriers = Cleared::default();", "text_taken_out_of_a_form_takes_the_spans_actual_text"),
    Mutation("form text: keep the structure element's copy", "src/redact.rs", "    let struct_carriers = clear_struct_shadow_text(doc, id, &carriers.mcids);", "    let struct_carriers = 0;", "an_inline_mcid_inside_a_form_reaches_the_forms_structure_element"),
    Mutation("form pictures: keep the span's shadow text", "src/redact.rs", "    let cleared = clear_form_shadow_text(doc, page, id, &mut inside.operations, &positions)?;", "    let cleared = Cleared::default();", "a_picture_taken_out_of_a_form_takes_the_spans_alt"),
    Mutation("form pictures: keep the structure element's copy", "src/redact.rs", "        inside.operations.remove(where_);\n    }\n    let in_structure = clear_struct_shadow_text(doc, id, &cleared.mcids);", "        inside.operations.remove(where_);\n    }\n    let in_structure = 0;", "a_named_lists_mcid_leads_to_the_forms_structure_element"),
    Mutation("form paths: keep the span's shadow text", "src/redact.rs", "    let cleared = clear_form_shadow_text(doc, page, id, &mut inside.operations, &edits.positions)?;", "    let cleared = Cleared::default();", "a_drawing_taken_out_of_a_form_takes_the_spans_expansion"),
    Mutation("page pictures: keep the span's shadow text", "src/redact.rs", "    let figure = clear_shadow_text(doc, page, &mut content.operations, &positions)?;", "    let figure = Cleared::default();", "a_picture_taken_off_a_page_takes_the_spans_alt"),
    Mutation("form names: resolved through the page", "src/redact.rs", "                    Lists::Form { page, form } => form_property_list(doc, page, form, &name),", "                    Lists::Form { page, .. } => property_list(doc, page, &name),", "a_named_list_in_the_forms_resources_carrying_shadow_text_is_refused"),
    Mutation("form names: a named list's MCID dropped", "src/redact.rs", "                    if let Ok(mcid) = shared.get(b\"MCID\").and_then(Object::as_i64) {\n                        cleared.mcids.push(mcid);\n                    }\n", "", "a_named_lists_mcid_leads_to_the_forms_structure_element"),
    Mutation("form structure: a form's StructParents unread", "src/redact.rs", "        Object::Stream(stream) => &stream.dict,", "        Object::Stream(_) => return None,", "an_inline_mcid_inside_a_form_reaches_the_forms_structure_element"),
    # Bounded walks.
    Mutation("number tree: walk a node named twice twice", "src/redact.rs", "            if !walk.seen.insert(*id) {", "            if !walk.seen.insert(*id) && false {", "a_number_tree_naming_one_node_twice_is_searched_once"),
    Mutation("number tree: go on past the bound", "src/redact.rs", "        if walk.visits >= MAX_TREE_NODES {\n            return None;\n        }\n", "", "a_number_tree_larger_than_the_bound_stops_at_it"),
    Mutation("outline count: count an entry reached twice twice", "src/redact.rs", "        if !walk.seen.insert(child) {", "        if !walk.seen.insert(child) && false {", "an_outline_entry_reached_twice_is_counted_once"),
    Mutation("outline count: no bound on the visits", "src/redact.rs", "        if walk.visits > MAX_OUTLINE_ITEMS {\n            break;\n        }\n        if !walk.seen", "        if !walk.seen", "an_outline_larger_than_the_bound_stops_at_it"),
    Mutation("outline: a cut walk not said", "src/redact.rs", "        if visits > MAX_OUTLINE_ITEMS {\n            cover.cut = true;\n            break;", "        if visits > MAX_OUTLINE_ITEMS {\n            break;", "an_outline_too_large_to_read_through_is_taken_whole"),
    Mutation("outline: a cut walk removed entry by entry", "src/save.rs", "        if cover.cut {", "        if false {", "an_outline_too_large_to_read_through_is_taken_whole"),
    Mutation("outline: an inline root not spliced", "src/redact.rs", "    let root = own_outline_root(doc);", "    let root = outline_root(doc);", "an_outline_written_into_the_catalog_keeps_the_entries_around_a_removed_one"),
    Mutation("outline: a title held as an object not read", "src/redact.rs", "            .get(b\"Title\")\n            .and_then(|object| doc.dereference(object).map(|(_, object)| object))\n            .and_then(Object::as_str)", "            .get(b\"Title\")\n            .and_then(Object::as_str)", "an_outline_title_held_as_its_own_object_is_matched"),
    # Metadata.
    Mutation("metadata: an inline trailer /Info kept", "src/save.rs", "            doc.trailer.remove(b\"Info\");\n", "", "an_info_written_into_the_trailer_is_removed"),
]
