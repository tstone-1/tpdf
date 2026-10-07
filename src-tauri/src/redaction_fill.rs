//! Black appearances for areas whose content-removal result was already checked.
//!
//! Annotation appearances start in their own graphics state, so a page's final
//! transform, clipping path or transparency cannot turn the fill into a hole.
//! They are visual output only: callers must verify removal before adding them.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};

use crate::docmodel::PageSource;
use crate::edits::{PageView, Plan, PlannedRedaction};
use crate::save::Refusal;

/// The colour a redaction's boxes are filled with.
///
/// Black unless the reader chose another. **White is allowed and is the one
/// that hides the redaction itself**: on white paper a reader of the copy
/// cannot see that anything was taken out, which is sometimes what is wanted
/// and never what should happen by accident, so it is never the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fill {
    /// The colour a redaction is expected to be.
    #[default]
    Black,
    /// Paper-coloured on white paper.
    White,
    /// For a draft somebody else reviews: the boxes stand out from black text.
    Red,
}

impl Fill {
    /// The colour as `rg` takes it.
    #[must_use]
    pub fn rgb(self) -> [f32; 3] {
        match self {
            Self::Black => [0.0, 0.0, 0.0],
            Self::White => [1.0, 1.0, 1.0],
            Self::Red => [0.83, 0.16, 0.16],
        }
    }

    /// The name a reader types or a panel sends: `black`, `white` or `red`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "black" => Some(Self::Black),
            "white" => Some(Self::White),
            "red" => Some(Self::Red),
            _ => None,
        }
    }
}

/// The plan the fill pass runs under: every page of the file the removal pass
/// wrote, as that file has them, and the regions at the places they are now.
///
/// **Built from nothing, and every field is written out.** This was a clone of
/// the reader's plan with the fields the fill must not repeat cleared one by
/// one, and a field added to [`Plan`] after that list was written was carried
/// into the fill without anybody deciding it should be: the form answers
/// first, then the field changes and the new fields. Each made the removal
/// pass's work be asked for a second time, of a file that had already had it
/// done. A literal with no `..` turns the next new field into a compile error
/// on this function, where the decision has to be made, and the same holds
/// for the region below.
///
/// What the fill needs is short: where the pages are, and where the regions
/// are on them. It adds appearances and removes, moves and answers nothing.
pub(crate) fn output_plan(original: &Plan) -> Result<Plan, Refusal> {
    let count = u32::try_from(original.pages.len())
        .map_err(|_| Refusal::from("Too many pages for the redaction fill"))?;
    let mut redactions = Vec::new();
    for (slot, page) in original.pages.iter().enumerate() {
        for redaction in &original.redactions {
            if page.source == PageSource::Baseline(redaction.source) {
                redactions.push(PlannedRedaction {
                    lines: Vec::new(),
                    show_cuts: Vec::new(),
                    source: u32::try_from(slot)
                        .map_err(|_| Refusal::from("Too many pages for the redaction fill"))?,
                    areas: redaction.areas.clone(),
                    // Everything that names content to take. The removal pass
                    // took it; asked for again, each of these would be looked
                    // for in a page that pass already changed.
                    shows: Vec::new(),
                    text_objects: 0,
                    taking: Vec::new(),
                    images: Vec::new(),
                    image_objects: 0,
                    paths: Vec::new(),
                    path_objects: 0,
                    cuts: Vec::new(),
                    form_shows: Vec::new(),
                    form_text_objects: Vec::new(),
                    form_paths: crate::redact::FormPathsPlanned::default(),
                    form_images: Vec::new(),
                    form_image_objects: Vec::new(),
                });
            }
        }
    }
    Ok(Plan {
        baseline: count,
        // The caller binds the pass to the bytes the verification read.
        opened_as: None,
        pages: (0..count)
            .map(|source| PageView {
                id: u64::from(source),
                source: PageSource::Baseline(source),
                turns: 0,
                crop: None,
            })
            .collect(),
        redactions,
        marks: Vec::new(),
        notes: Vec::new(),
        discards: Vec::new(),
        // **The file this plan is pointed at already holds the inserted
        // pages**, which the removal pass wrote into it --- so this pass must
        // not be told to insert them a second time, and must not be made to
        // depend on the other file still being where it was. `pages` above is
        // already every output page as a baseline one, so `save::import_pages`
        // has nothing to place; what an empty list takes away is
        // `save::held_sources`, which would reopen every source file and
        // refuse the black fill if one had been touched or moved in the
        // meantime --- a refusal about an insert, on a pass that inserts
        // nothing, arriving after the words are already gone.
        sources: Vec::new(),
        // Addressed by `Edit::source` into the list above, so one left here
        // would name a file this plan no longer has. A plan reaching here
        // carries none in any case --- `save::rewrite` refuses text edits
        // beside a redaction.
        text_edits: Vec::new(),
        // **The removal pass already wrote the answers.** Carried here,
        // `forms::write` runs again on a file the removal changed --- and when
        // the region covered a field the reader had filled, that field is
        // gone, so the fill refuses with *the form field is no longer in this
        // document*, after the words have gone and with the verification's
        // answer discarded.
        forms: Vec::new(),
        // The same twice more. A field the reader removed is gone from the
        // file the first pass wrote, so removing it again finds no such widget
        // and refuses the fill; one the reader added is in that file already.
        field_edits: Vec::new(),
        new_fields: Vec::new(),
        // Sorted, shrunk and layered once, by the pass that wrote the file.
        tab_order: false,
        compress: crate::compress::Compress::No,
        text_layers: Vec::new(),
        // What the file the first pass wrote has, put back: that pass set or
        // removed the password, and this one writes the same file again.
        protection: crate::protect::Protection::Keep,
    })
}

/// Appends opaque appearances, after existing annotations, in absolute PDF space.
pub(crate) fn paint(
    doc: &mut Document,
    pages: &[ObjectId],
    redactions: &[PlannedRedaction],
    fill: Fill,
) -> Result<(), Refusal> {
    let colour: Vec<Object> = fill.rgb().into_iter().map(Object::Real).collect();
    for redaction in redactions {
        let page = *pages
            .get(redaction.source as usize)
            .ok_or_else(|| Refusal::from("The redaction fill addresses a missing page"))?;
        for &area in &redaction.areas {
            let [left, bottom, right, top] = area;
            let width = right - left;
            let height = top - bottom;
            if !area.iter().all(|n| n.is_finite())
                || !width.is_finite()
                || !height.is_finite()
                || width <= 0.0
                || height <= 0.0
            {
                return Err("The redaction fill has an invalid rectangle".into());
            }
            let current = doc.get_dictionary(page).map_err(|e| e.to_string())?;
            let mut annots = match current.get(b"Annots") {
                Ok(value) => doc
                    .dereference(value)
                    .map_err(|e| e.to_string())?
                    .1
                    .as_array()
                    .map_err(|e| e.to_string())?
                    .clone(),
                Err(lopdf::Error::DictKey(_)) => Vec::new(),
                Err(e) => return Err(e.to_string().into()),
            };
            let content = lopdf::content::Content {
                operations: vec![
                    lopdf::content::Operation::new("q", vec![]),
                    lopdf::content::Operation::new("gs", vec![Object::Name(b"Opaque".to_vec())]),
                    lopdf::content::Operation::new("rg", colour.clone()),
                    lopdf::content::Operation::new(
                        "re",
                        vec![0.into(), 0.into(), width.into(), height.into()],
                    ),
                    lopdf::content::Operation::new("f", vec![]),
                    lopdf::content::Operation::new("Q", vec![]),
                ],
            }
            .encode()
            .map_err(|e| e.to_string())?;
            let appearance = doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
                    "BBox" => vec![0.into(), 0.into(), width.into(), height.into()],
                    "Resources" => dictionary! { "ExtGState" => dictionary! {
                        "Opaque" => dictionary! { "Type" => "ExtGState", "ca" => 1,
                            "CA" => 1, "BM" => "Normal", "SMask" => "None" }
                    } },
                },
                content,
            ));
            let annotation = doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Square", "P" => page,
                "Rect" => area.into_iter().map(Object::Real).collect::<Vec<_>>(),
                "F" => 196, // Print, ReadOnly, Locked; never hidden or view-only.
                "CA" => 1, "C" => colour.clone(),
                "IC" => colour.clone(),
                "Border" => vec![0.into(), 0.into(), 0.into()],
                "AP" => dictionary! { "N" => appearance },
            });
            annots.push(annotation.into());
            doc.get_dictionary_mut(page)
                .map_err(|e| e.to_string())?
                .set("Annots", annots);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(source: u32, area: [f32; 4]) -> PlannedRedaction {
        PlannedRedaction {
            lines: Vec::new(),
            show_cuts: Vec::new(),
            form_paths: crate::redact::FormPathsPlanned {
                whole: vec![(1, 0)],
                cuts: vec![(1, 2, area)],
                objects: vec![(1, 3)],
            },
            source,
            shows: vec![4],
            text_objects: 7,
            areas: vec![area],
            taking: vec!["SYNTHETIC REMOVAL".into()],
            images: vec![2],
            image_objects: 3,
            paths: vec![1],
            path_objects: 5,
            cuts: vec![(0, area)],
            form_shows: vec![(1, 2)],
            form_images: vec![(1, 0)],
            form_image_objects: vec![(1, 4)],
            form_text_objects: vec![(1, 3)],
        }
    }

    /// The fill pass is pointed at the file the removal wrote, which already
    /// holds the inserted pages --- so the plan it runs under must name no other
    /// file.
    ///
    /// **Not about placing them twice**, which `pages` already rules out by
    /// being every output page as a baseline one. It is about
    /// `save::held_sources`, which reads and re-fingerprints every entry in
    /// `sources` before the rewrite begins: a source file moved or touched
    /// between the removal and the fill would refuse the black fill, naming an
    /// insert, after the words were already gone. The reader would be left with
    /// a redacted file and no fill over it.
    #[test]
    fn the_fill_pass_names_no_other_file_and_no_text_edit() {
        let source = PlannedSourceForTest::plan();
        assert!(
            !source.sources.is_empty() && !source.text_edits.is_empty(),
            "the fixture has to carry both, or this asserts nothing"
        );
        let mapped = output_plan(&source).unwrap();
        assert!(mapped.sources.is_empty(), "no other file to reopen");
        assert!(
            mapped.text_edits.is_empty(),
            "and nothing addressed into it"
        );
        // The pages are all the output's own, which is what leaves
        // `save::import_pages` nothing to place.
        assert!(mapped
            .pages
            .iter()
            .all(|page| matches!(page.source, PageSource::Baseline(_))));
        assert_eq!(mapped.redactions.len(), 1, "and the region still travels");
        assert_eq!(
            mapped.redactions[0].source, 1,
            "at the slot the written file puts its page at"
        );
    }

    /// The fixture for the test above: a plan as the redaction command builds
    /// one for a document holding a page of another file.
    struct PlannedSourceForTest;

    impl PlannedSourceForTest {
        fn plan() -> Plan {
            Plan {
                field_edits: Vec::new(),
                tab_order: false,
                baseline: 2,
                opened_as: None,
                text_layers: Vec::new(),
                protection: Default::default(),
                compress: Default::default(),
                new_fields: Vec::new(),
                pages: vec![
                    PageView {
                        id: 9,
                        source: PageSource::Imported {
                            source: crate::docmodel::SourceId::from_raw(1),
                            page: 0,
                        },
                        turns: 0,
                        crop: None,
                    },
                    PageView {
                        id: 1,
                        source: PageSource::Baseline(0),
                        turns: 0,
                        crop: None,
                    },
                ],
                marks: vec![],
                notes: vec![],
                discards: vec![],
                sources: vec![crate::edits::PlannedSource {
                    id: 1,
                    path: std::path::PathBuf::from("other.pdf"),
                    opened_as: None,
                }],
                forms: Vec::new(),
                text_edits: vec![crate::textedit::Edit::imported(
                    1,
                    crate::textedit::Change {
                        layout: None,
                        page: 0,
                        revision: Vec::new(),
                        operator: 0,
                        original: "before".into(),
                        replacement: "after".into(),
                    },
                )],
                redactions: vec![region(0, [30.0, 40.0, 60.0, 70.0])],
            }
        }
    }

    /// The fill pass writes no form answer: the removal pass wrote them.
    ///
    /// A region over a field the reader filled takes the field, so a second
    /// `forms::write` finds nothing to write into and refuses the fill.
    #[test]
    fn the_fill_pass_writes_no_form_answer_a_second_time() {
        let mut source = PlannedSourceForTest::plan();
        source.forms = vec![crate::forms::Change {
            object: (12, 0),
            value: crate::forms::Value::Text("an answer".into()),
        }];
        let mapped = output_plan(&source).unwrap();
        assert!(mapped.forms.is_empty(), "the removal pass already wrote it");
        assert_eq!(mapped.redactions.len(), 1, "the region still travels");
    }

    /// The fill pass changes no field, adds none, and repeats nothing else the
    /// removal pass was asked to do once.
    ///
    /// **Every field of the plan that is not the pages and the regions**, set
    /// to something and expected back empty. A removed field is the one a
    /// reader meets: the removal pass takes its widget out, the fill pass is
    /// told to take it out again, finds no such widget and refuses --- after
    /// the words are gone, with the verification's answer discarded. An added
    /// field is added twice, a tab order is sorted twice, pictures are shrunk
    /// twice and a text layer is written over the one already there.
    #[test]
    fn the_fill_plan_carries_no_field_changes() {
        let mut source = PlannedSourceForTest::plan();
        source.field_edits = vec![crate::formedit::FieldEdit {
            widget: (12, 0),
            rect: None,
            name: None,
            remove: true,
            props: Default::default(),
            value: None,
        }];
        source.new_fields = vec![crate::formfields::NewField {
            name: "Added".into(),
            kind: crate::formfields::Kind::Text,
            page: 0,
            rect: [20.0, 20.0, 100.0, 20.0],
            tooltip: None,
            required: false,
            max_length: None,
            border: true,
            options: Vec::new(),
            text_size: None,
            default_value: None,
        }];
        source.tab_order = true;
        source.compress = crate::compress::Compress::Lossless;
        source.protection = crate::protect::Protection::Set("a password".into());
        source.text_layers = vec![crate::textlayer::Layer {
            page: 0,
            words: Vec::new(),
        }];
        source.forms = vec![crate::forms::Change {
            object: (12, 0),
            value: crate::forms::Value::Text("an answer".into()),
        }];

        let mapped = output_plan(&source).unwrap();
        assert!(mapped.field_edits.is_empty(), "no field is changed twice");
        assert!(mapped.new_fields.is_empty(), "and none is added twice");
        assert!(!mapped.tab_order);
        assert_eq!(mapped.compress, crate::compress::Compress::No);
        assert_eq!(mapped.protection, crate::protect::Protection::Keep);
        assert!(mapped.text_layers.is_empty());
        // Everything, said once: the plan is what an empty plan over the
        // output's pages is, with the regions and nothing else.
        let mut bare = mapped.clone();
        bare.redactions.clear();
        assert!(
            bare.is_identity(),
            "without its regions the fill plan asks for nothing at all: {bare:?}"
        );
        assert_eq!(mapped.redactions.len(), 1, "and the region still travels");
    }

    #[test]
    fn maps_reordered_pages_without_reapplying_turns_crops_or_removals() {
        let original = Plan {
            field_edits: Vec::new(),
            tab_order: false,
            baseline: 3,
            opened_as: None,
            pages: vec![
                PageView {
                    id: 8,
                    source: PageSource::Baseline(2),
                    turns: 1,
                    crop: Some([10.0, 20.0, 100.0, 200.0]),
                },
                PageView {
                    id: 4,
                    source: PageSource::Baseline(0),
                    turns: 3,
                    crop: None,
                },
            ],
            marks: vec![],
            notes: vec![],
            discards: vec![],
            sources: Vec::new(),
            forms: Vec::new(),
            text_edits: Vec::new(),
            text_layers: Vec::new(),
            protection: Default::default(),
            compress: Default::default(),
            new_fields: Vec::new(),
            redactions: vec![
                region(0, [30.0, 40.0, 60.0, 70.0]),
                region(1, [1.0, 2.0, 3.0, 4.0]),
                region(2, [50.0, 60.0, 70.0, 80.0]),
            ],
        };
        let mapped = output_plan(&original).unwrap();
        assert_eq!(mapped.baseline, 2);
        assert_eq!(mapped.redactions.len(), 2);
        assert_eq!(mapped.redactions[0].source, 0);
        assert_eq!(mapped.redactions[0].areas, original.redactions[2].areas);
        assert_eq!(mapped.redactions[1].source, 1);
        assert_eq!(mapped.redactions[1].areas, original.redactions[0].areas);
        for (slot, page) in mapped.pages.iter().enumerate() {
            assert_eq!(
                page.source,
                PageSource::Baseline(u32::try_from(slot).unwrap())
            );
            assert_eq!(page.turns, 0);
            assert!(page.crop.is_none());
        }
        for r in mapped.redactions {
            assert!(r.shows.is_empty() && r.images.is_empty() && r.form_shows.is_empty());
            // The drawings went with the removal too. A cut asked for again
            // would be made against a page the first pass already changed.
            assert!(r.paths.is_empty() && r.cuts.is_empty() && r.path_objects == 0);
            assert!(r.taking.is_empty() && r.form_text_objects.is_empty());
            // And the pictures inside a block: asked for again, they would be
            // looked for in a block the first pass already took them out of.
            assert!(r.form_images.is_empty() && r.form_image_objects.is_empty());
            // And the drawings inside one.
            assert_eq!(r.form_paths, crate::redact::FormPathsPlanned::default());
            assert_eq!((r.text_objects, r.image_objects), (0, 0));
        }
    }

    #[test]
    fn preserves_indirect_annotations_and_uses_independent_opaque_appearance() {
        let mut doc = Document::new();
        let existing = doc.add_object(dictionary! { "Subtype" => "Text" });
        let list = doc.add_object(vec![Object::Reference(existing)]);
        let page = doc.add_object(dictionary! { "Type" => "Page", "Annots" => list });
        paint(
            &mut doc,
            &[page],
            &[region(0, [-20.0, 30.0, 80.0, 90.0])],
            Fill::Black,
        )
        .unwrap();
        let annots = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(annots.len(), 2);
        assert_eq!(annots[0].as_reference().unwrap(), existing);
        let fill = doc
            .get_dictionary(annots[1].as_reference().unwrap())
            .unwrap();
        assert_eq!(fill.get(b"F").unwrap().as_i64().unwrap(), 196);
        let ap = fill
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = doc.get_object(ap).unwrap().as_stream().unwrap();
        assert_eq!(
            stream.dict.get(b"BBox").unwrap().as_array().unwrap(),
            &vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Real(100.0),
                Object::Real(60.0)
            ]
        );
        let ops = lopdf::content::Content::decode(&stream.content)
            .unwrap()
            .operations;
        assert_eq!(
            ops.iter()
                .map(|op| op.operator.as_str())
                .collect::<Vec<_>>(),
            vec!["q", "gs", "rg", "re", "f", "Q"]
        );
        assert_eq!(numbers(&ops[2].operands), [0.0; 3]);
    }

    /// Numbers as numbers: a stream that was written `0` reads back as an
    /// integer whatever it was written from.
    fn numbers(objects: &[Object]) -> Vec<f32> {
        objects
            .iter()
            .map(|object| object.as_float().expect("a number"))
            .collect()
    }

    /// What `paint` writes for one region in `fill`: the colour the appearance
    /// fills with, and the two the annotation itself names.
    fn colours(fill: Fill) -> [Vec<f32>; 3] {
        let mut doc = Document::new();
        let page = doc.add_object(dictionary! { "Type" => "Page" });
        paint(
            &mut doc,
            &[page],
            &[region(0, [0.0, 0.0, 10.0, 10.0])],
            fill,
        )
        .unwrap();
        let annots = doc.get_dictionary(page).unwrap().get(b"Annots").unwrap();
        let annot = doc
            .get_dictionary(annots.as_array().unwrap()[0].as_reference().unwrap())
            .unwrap();
        let ap = annot.get(b"AP").unwrap().as_dict().unwrap();
        let stream = doc
            .get_object(ap.get(b"N").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        let ops = lopdf::content::Content::decode(&stream.content)
            .unwrap()
            .operations;
        [
            numbers(&ops[2].operands),
            numbers(annot.get(b"C").unwrap().as_array().unwrap()),
            numbers(annot.get(b"IC").unwrap().as_array().unwrap()),
        ]
    }

    /// The appearance is what a reader draws, and `/C` and `/IC` are what one
    /// that ignores appearances falls back to: all three say the same colour.
    #[test]
    fn the_boxes_are_the_colour_that_was_asked_for_in_all_three_places() {
        for (fill, rgb) in [
            (Fill::Black, [0.0, 0.0, 0.0]),
            (Fill::White, [1.0, 1.0, 1.0]),
            (Fill::Red, [0.83, 0.16, 0.16]),
        ] {
            let want = rgb.to_vec();
            assert_eq!(
                colours(fill),
                [want.clone(), want.clone(), want],
                "{fill:?}"
            );
        }
    }

    #[test]
    fn a_fill_is_black_unless_another_is_named() {
        assert_eq!(Fill::default(), Fill::Black);
        assert_eq!(Fill::named("black"), Some(Fill::Black));
        assert_eq!(Fill::named("white"), Some(Fill::White));
        assert_eq!(Fill::named("red"), Some(Fill::Red));
        assert_eq!(Fill::named("Red"), None, "the names are lower case");
        assert_eq!(Fill::named(""), None);
        // And the same names on the wire, which is what the window sends.
        assert_eq!(serde_json::to_string(&Fill::Red).unwrap(), "\"red\"");
        assert_eq!(
            serde_json::from_str::<Fill>("\"white\"").unwrap(),
            Fill::White
        );
    }

    #[test]
    fn refuses_invalid_geometry_and_missing_pages() {
        for area in [
            [0.0, 0.0, f32::NAN, 10.0],
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 0.0, 1.0, 0.0],
            [-f32::MAX, 0.0, f32::MAX, 1.0],
        ] {
            let mut doc = Document::new();
            let page = doc.add_object(dictionary! { "Type" => "Page" });
            assert!(paint(&mut doc, &[page], &[region(0, area)], Fill::Black).is_err());
            assert!(doc.get_dictionary(page).unwrap().get(b"Annots").is_err());
        }
        assert!(paint(
            &mut Document::new(),
            &[],
            &[region(0, [0.0, 0.0, 1.0, 1.0])],
            Fill::Black,
        )
        .is_err());
    }
}
