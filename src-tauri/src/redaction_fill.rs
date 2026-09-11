//! Black appearances for areas whose content-removal result was already checked.
//!
//! Annotation appearances start in their own graphics state, so a page's final
//! transform, clipping path or transparency cannot turn the fill into a hole.
//! They are visual output only: callers must verify removal before adding them.

use lopdf::{dictionary, Document, Object, ObjectId, Stream};

use crate::docmodel::PageSource;
use crate::edits::{PageView, Plan, PlannedRedaction};
use crate::save::Refusal;

/// Re-address absolute page rectangles after deletion/reordering and reset edits.
pub(crate) fn output_plan(original: &Plan) -> Result<Plan, Refusal> {
    let count = u32::try_from(original.pages.len())
        .map_err(|_| Refusal::from("Too many pages for the redaction fill"))?;
    let mut result = original.clone();
    result.baseline = count;
    result.opened_as = None;
    result.pages = (0..count)
        .map(|source| PageView {
            id: u64::from(source),
            source: PageSource::Baseline(source),
            turns: 0,
            crop: None,
        })
        .collect();
    result.marks.clear();
    result.notes.clear();
    result.discards.clear();
    result.redactions.clear();
    for (slot, page) in original.pages.iter().enumerate() {
        for redaction in &original.redactions {
            if page.source == PageSource::Baseline(redaction.source) {
                let mut mapped = redaction.clone();
                mapped.source = u32::try_from(slot)
                    .map_err(|_| Refusal::from("Too many pages for the redaction fill"))?;
                mapped.shows.clear();
                mapped.text_objects = 0;
                mapped.taking.clear();
                mapped.images.clear();
                mapped.image_objects = 0;
                mapped.form_shows.clear();
                mapped.form_text_objects.clear();
                result.redactions.push(mapped);
            }
        }
    }
    Ok(result)
}

/// Appends opaque appearances, after existing annotations, in absolute PDF space.
pub(crate) fn paint(
    doc: &mut Document,
    pages: &[ObjectId],
    redactions: &[PlannedRedaction],
) -> Result<(), Refusal> {
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
                    lopdf::content::Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
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
                "CA" => 1, "C" => vec![0.into(), 0.into(), 0.into()],
                "IC" => vec![0.into(), 0.into(), 0.into()],
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
            source,
            shows: vec![4],
            text_objects: 7,
            areas: vec![area],
            taking: vec!["SYNTHETIC REMOVAL".into()],
            images: vec![2],
            image_objects: 3,
            form_shows: vec![(1, 2)],
            form_text_objects: vec![(1, 3)],
        }
    }

    #[test]
    fn maps_reordered_pages_without_reapplying_turns_crops_or_removals() {
        let original = Plan {
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
            forms: Vec::new(),
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
            assert!(r.taking.is_empty() && r.form_text_objects.is_empty());
            assert_eq!((r.text_objects, r.image_objects), (0, 0));
        }
    }

    #[test]
    fn preserves_indirect_annotations_and_uses_independent_opaque_appearance() {
        let mut doc = Document::new();
        let existing = doc.add_object(dictionary! { "Subtype" => "Text" });
        let list = doc.add_object(vec![Object::Reference(existing)]);
        let page = doc.add_object(dictionary! { "Type" => "Page", "Annots" => list });
        paint(&mut doc, &[page], &[region(0, [-20.0, 30.0, 80.0, 90.0])]).unwrap();
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
        assert_eq!(ops[2].operands, vec![Object::Integer(0); 3]);
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
            assert!(paint(&mut doc, &[page], &[region(0, area)]).is_err());
            assert!(doc.get_dictionary(page).unwrap().get(b"Annots").is_err());
        }
        assert!(paint(
            &mut Document::new(),
            &[],
            &[region(0, [0.0, 0.0, 1.0, 1.0])]
        )
        .is_err());
    }
}
