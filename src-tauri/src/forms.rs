//! Bounded AcroForm discovery and explicit appearances for filled fields.
//!
//! Runs only beside the worker's object graph. Values and appearances are
//! written together; readers never have to execute JavaScript or honour
//! NeedAppearances to see a saved answer. Unsupported controls stay read-only.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// A field answer, distinct from an annotation's comment body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Text(String),
    Checked(bool),
}

/// A pending answer, keyed by the terminal field rather than a widget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub object: ObjectId,
    pub value: Value,
}

/// One visible widget. Several widgets may share one field and answer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Widget {
    pub object: ObjectId,
    pub widget: ObjectId,
    pub page: u32,
    pub rect: [f64; 4],
    pub display_rect: [f32; 4],
    pub name: String,
    pub value: Value,
    pub multiline: bool,
    pub max_length: Option<usize>,
    pub reason: Option<String>,
}

/// A complete scan or an error; never a silently truncated list.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Form {
    pub widgets: Vec<Widget>,
}

fn inherited<'a>(doc: &'a Document, start: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut at = start;
    let mut seen = HashSet::new();
    for _ in 0..32 {
        if !seen.insert(at) {
            return None;
        }
        let dict = doc.get_dictionary(at).ok()?;
        if let Ok(value) = dict.get(key) {
            return doc.dereference(value).ok().map(|(_, v)| v);
        }
        at = dict.get(b"Parent").and_then(Object::as_reference).ok()?;
    }
    None
}

fn integer(doc: &Document, id: ObjectId, key: &[u8]) -> i64 {
    inherited(doc, id, key)
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0)
}

fn acroform(doc: &Document) -> Option<&Dictionary> {
    let o = doc.catalog().ok()?.get(b"AcroForm").ok()?;
    doc.dereference(o).ok()?.1.as_dict().ok()
}

/// Reads widgets in page and annotation order without loading a PDFium page.
pub fn scan(doc: &Document) -> Result<Form, String> {
    let Some(form) = acroform(doc) else {
        return Ok(Form::default());
    };
    if form.has(b"XFA") {
        return Err("XFA forms are not supported".into());
    }
    let mut members = HashSet::new();
    let cut = crate::fields::walk(
        doc,
        &crate::fields::Bounds {
            nodes: 4096,
            depth: Some(32),
            dedup: true,
            names: false,
            order: crate::fields::Order::Document,
        },
        |node| {
            if let Some(id) = node.id {
                members.insert(id);
            }
            crate::fields::Flow::Descend
        },
    );
    if cut.dropped > 0 || cut.too_deep > 0 {
        return Err("This form exceeds the field-tree limit".into());
    }
    let mut result = Form::default();
    let mut visited = 0;
    let mut text_bytes = 0usize;
    for (page, id) in crate::pagetree::ordered_pages(doc).into_iter().enumerate() {
        let Ok(dict) = doc.get_dictionary(id) else {
            continue;
        };
        let Ok(annotations) = dict
            .get(b"Annots")
            .and_then(|o| doc.dereference(o).map(|(_, v)| v))
            .and_then(Object::as_array)
        else {
            continue;
        };
        for annotation in annotations {
            visited += 1;
            if visited > 20000 {
                return Err("This form exceeds the annotation limit".into());
            }
            let Ok(widget) = annotation.as_reference() else {
                continue;
            };
            let Ok(w) = doc.get_dictionary(widget) else {
                continue;
            };
            if w.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Widget") {
                continue;
            }
            let object = if w.has(b"T") || w.has(b"FT") {
                widget
            } else {
                w.get(b"Parent")
                    .and_then(Object::as_reference)
                    .map_err(|_| "A widget has no field")?
            };
            if !members.contains(&object) {
                return Err("A widget is outside the form field tree".into());
            }
            let flags = integer(doc, object, b"Ff");
            let kind = inherited(doc, object, b"FT").and_then(|v| v.as_name().ok());
            let text = kind == Some(b"Tx");
            let checkbox = kind == Some(b"Btn") && flags & ((1 << 15) | (1 << 16)) == 0;
            if inherited(doc, object, b"V")
                .and_then(|v| v.as_str().ok())
                .is_some_and(|raw| raw.len() > 16384)
            {
                return Err("This form contains an oversized answer".into());
            }
            let value = if checkbox {
                Value::Checked(
                    inherited(doc, object, b"V")
                        .and_then(|v| v.as_name().ok())
                        .is_some_and(|n| n != b"Off"),
                )
            } else {
                Value::Text(
                    inherited(doc, object, b"V")
                        .and_then(|v| v.as_str().ok())
                        .map(crate::annots::decode_text_string)
                        .unwrap_or_default(),
                )
            };
            let numbers = w
                .get(b"Rect")
                .and_then(Object::as_array)
                .map_err(|_| "A form widget has no rectangle")?;
            if numbers.len() != 4 {
                return Err("Invalid form widget rectangle".into());
            }
            let mut rect = [0.0; 4];
            for (out, value) in rect.iter_mut().zip(numbers) {
                *out = f64::from(
                    value
                        .as_float()
                        .map_err(|_| "Invalid form widget coordinate")?,
                );
            }
            if !rect.iter().all(|v| v.is_finite())
                || rect[2] <= rect[0]
                || rect[3] <= rect[1]
                || rect.iter().any(|v| v.abs() > 100000.0)
            {
                return Err("Invalid form widget bounds".into());
            }
            let mut names = Vec::new();
            let mut at = Some(object);
            let mut seen = HashSet::new();
            while let Some(id) = at {
                if !seen.insert(id) || seen.len() > 32 {
                    return Err("Cyclic form field ancestry".into());
                }
                let d = doc
                    .get_dictionary(id)
                    .map_err(|_| "Unreadable form field")?;
                if let Ok(raw) = d.get(b"T").and_then(Object::as_str) {
                    if raw.len() > 1024 {
                        return Err("This form contains an oversized field name".into());
                    }
                    names.push(crate::annots::decode_text_string(raw));
                }
                at = d.get(b"Parent").and_then(Object::as_reference).ok();
            }
            names.reverse();
            let name = names.join(".");
            text_bytes += name.len()
                + match &value {
                    Value::Text(s) => s.len(),
                    Value::Checked(_) => 0,
                };
            if text_bytes > 1_048_576 {
                return Err("This form exceeds the text limit".into());
            }
            let annotation_flags = w.get(b"F").and_then(Object::as_i64).unwrap_or(0);
            let reason = if !text && !checkbox {
                Some("This field type is not supported yet".into())
            } else if flags & 1 != 0 {
                Some("This field is read-only".into())
            } else if text && flags & ((1 << 13) | (1 << 20) | (1 << 24) | (1 << 25)) != 0 {
                Some("Password, file, comb and rich-text fields are not supported yet".into())
            } else if annotation_flags & (1 | 2 | 32) != 0 {
                Some("This field is hidden".into())
            } else {
                None
            };
            let max = integer(doc, object, b"MaxLen");
            let geometry = crate::pagetree::displayed_page(doc, id);
            let display_rect = crate::text::to_device(
                geometry.turns,
                geometry.width,
                geometry.height,
                [
                    rect[0] - f64::from(geometry.origin.0),
                    rect[1] - f64::from(geometry.origin.1),
                    rect[2] - f64::from(geometry.origin.0),
                    rect[3] - f64::from(geometry.origin.1),
                ],
            );
            result.widgets.push(Widget {
                display_rect,
                object,
                widget,
                page: page as u32,
                rect,
                name,
                value,
                multiline: text && flags & (1 << 12) != 0,
                max_length: (max > 0).then_some(max as usize),
                reason,
            });
            if result.widgets.len() > 4096 {
                return Err("This form exceeds the widget limit".into());
            }
        }
    }
    Ok(result)
}

/// Checks an answer before it can enter the journal or the saved document.
pub fn validate(widget: &Widget, value: &Value) -> Result<(), String> {
    if let Some(reason) = &widget.reason {
        return Err(reason.clone());
    }
    match (&widget.value, value) {
        (Value::Text(_), Value::Text(text)) => {
            if text.len() > 16384 {
                return Err("A form answer is limited to 16 KB".into());
            }
            if widget
                .max_length
                .is_some_and(|max| text.chars().count() > max)
            {
                return Err("This answer exceeds the field's maximum length".into());
            }
            if !crate::textbox::encodable(text) {
                return Err("This field supports Western European characters only".into());
            }
            if !widget.multiline && text.contains(['\r', '\n']) {
                return Err("This field accepts one line only".into());
            }
            text_layout(widget, text)?;
            Ok(())
        }
        (Value::Checked(_), Value::Checked(_)) => Ok(()),
        _ => Err("The answer does not match the field type".into()),
    }
}

/// Layout is validated before journalling as well as immediately before writing.
fn text_layout(widget: &Widget, text: &str) -> Result<(f64, Vec<String>), String> {
    let width = widget.rect[2] - widget.rect[0];
    let height = widget.rect[3] - widget.rect[1];
    let size = 12.0_f64.min((height - 4.0).max(1.0));
    let lines = if widget.multiline {
        crate::textbox::wrap(text, size, (width - 4.0).max(1.0))
    } else {
        vec![text.to_string()]
    };
    let size = if !widget.multiline && !text.is_empty() {
        size.min(size * (width - 4.0).max(1.0) / crate::textbox::advance(text, size).max(1.0))
    } else {
        size
    };
    if size < 4.0 || lines.len() as f64 * size * 1.2 > height - 2.0 {
        return Err("The answer does not fit visibly in this field".into());
    }
    Ok((size, lines))
}

fn pdf_string(text: &str) -> Object {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

fn appearance(
    doc: &mut Document,
    width: f64,
    height: f64,
    body: Vec<u8>,
    resources: Dictionary,
) -> ObjectId {
    doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
            "BBox" => vec![0.into(), 0.into(), width.into(), height.into()],
            "Resources" => resources,
        },
        body,
    ))
}

/// Writes all widgets of a shared field, plus its value, in one rewrite.
pub fn write(doc: &mut Document, changes: &[Change]) -> Result<(), String> {
    if changes.is_empty() {
        return Ok(());
    }
    let form = scan(doc)?;
    let mut unique = BTreeMap::new();
    for change in changes {
        if unique.insert(change.object, &change.value).is_some() {
            return Err("Duplicate form answer".into());
        }
        let widgets: Vec<_> = form
            .widgets
            .iter()
            .filter(|w| w.object == change.object)
            .collect();
        if widgets.is_empty() {
            return Err("The form field is no longer in this document".into());
        }
        for widget in &widgets {
            validate(widget, &change.value)?;
        }
        let mut on_name = None;
        for widget in widgets {
            let width = widget.rect[2] - widget.rect[0];
            let height = widget.rect[3] - widget.rect[1];
            match &change.value {
                Value::Text(text) => {
                    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
                    let (size, lines) = text_layout(widget, text)?;
                    let mut body = format!("q 1 1 1 rg 0 0 {width} {height} re f 0 0 0 rg 0 0 {width} {height} re W n BT /F0 {size} Tf\n");
                    for (i, line) in lines.iter().enumerate() {
                        let encoded: Vec<u8> = line.chars().map(|ch| ch as u8).collect();
                        let hex: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
                        let y = height - 2.0 - size - i as f64 * size * 1.2;
                        body.push_str(&format!("1 0 0 1 2 {y} Tm <{hex}> Tj\n"));
                    }
                    body.push_str("ET Q");
                    let ap = appearance(
                        doc,
                        width,
                        height,
                        body.into_bytes(),
                        dictionary! { "Font" => dictionary! { "F0" => font } },
                    );
                    let w = doc
                        .get_dictionary_mut(widget.widget)
                        .map_err(|e| e.to_string())?;
                    w.set("AP", dictionary! { "N" => ap });
                    w.remove(b"AS");
                }
                Value::Checked(checked) => {
                    let existing = doc
                        .get_dictionary(widget.widget)
                        .ok()
                        .and_then(|w| w.get(b"AP").ok())
                        .and_then(|o| doc.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .and_then(|a| a.get(b"N").ok())
                        .and_then(|o| doc.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok());
                    let name = existing
                        .and_then(|d| {
                            d.iter()
                                .find(|(k, _)| k.as_slice() != b"Off")
                                .map(|(k, _)| k.clone())
                        })
                        .unwrap_or_else(|| b"Yes".to_vec());
                    if on_name.as_ref().is_some_and(|prior| prior != &name) {
                        return Err("Shared checkboxes have incompatible on states".into());
                    }
                    on_name = Some(name.clone());
                    let base = format!(
                        "q 1 1 1 rg 0 0 {width} {height} re f 0 0 0 RG 1 w 0.5 0.5 {} {} re S ",
                        width - 1.0,
                        height - 1.0
                    );
                    let off = appearance(
                        doc,
                        width,
                        height,
                        format!("{base} Q").into_bytes(),
                        Dictionary::new(),
                    );
                    let on = appearance(
                        doc,
                        width,
                        height,
                        format!(
                            "{base} 2 2 m {} {} l 2 {} m {} 2 l S Q",
                            width - 2.0,
                            height - 2.0,
                            height - 2.0,
                            width - 2.0
                        )
                        .into_bytes(),
                        Dictionary::new(),
                    );
                    let mut normal = dictionary! { "Off" => off };
                    normal.set(name.clone(), on);
                    let w = doc
                        .get_dictionary_mut(widget.widget)
                        .map_err(|e| e.to_string())?;
                    w.set("AP", dictionary! { "N" => normal });
                    w.set(
                        "AS",
                        Object::Name(if *checked { name } else { b"Off".to_vec() }),
                    );
                }
            }
        }
        let flags = integer(doc, change.object, b"Ff");
        let field = doc
            .get_dictionary_mut(change.object)
            .map_err(|e| e.to_string())?;
        // PDFKit reads a widget's immediate parent, not a grandparent, for FT.
        // Make inherited type and flags explicit when writing that terminal field.
        field.set(
            "FT",
            Object::Name(match change.value {
                Value::Text(_) => b"Tx".to_vec(),
                Value::Checked(_) => b"Btn".to_vec(),
            }),
        );
        field.set("Ff", flags);
        field.set(
            "V",
            match &change.value {
                Value::Text(text) => pdf_string(text),
                Value::Checked(checked) => Object::Name(if *checked {
                    on_name.unwrap_or_else(|| b"Yes".to_vec())
                } else {
                    b"Off".to_vec()
                }),
            },
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Two widgets share an inherited text field; the checkbox uses a custom state.
    pub(crate) fn fixture() -> (Document, ObjectId, ObjectId) {
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let parent = doc.add_object(
            dictionary! { "FT" => "Tx", "T" => Object::string_literal("ACME"), "Ff" => 0 },
        );
        let field = doc.add_object(dictionary! { "Parent" => parent, "T" => Object::string_literal("answer"), "V" => Object::string_literal("OLD"), "MaxLen" => 20 });
        let mut widgets = Vec::new();
        let mut page_ids = Vec::new();
        for _ in 0..2 {
            let widget = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "Parent" => field, "Rect" => vec![20.into(), 70.into(), 180.into(), 100.into()], "F" => 4 });
            widgets.push(widget);
            let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 220.into(), 150.into()], "Resources" => Dictionary::new(), "Annots" => vec![widget.into()] });
            page_ids.push(page);
        }
        doc.get_dictionary_mut(field).unwrap().set(
            "Kids",
            widgets
                .iter()
                .copied()
                .map(Object::Reference)
                .collect::<Vec<_>>(),
        );
        doc.get_dictionary_mut(parent)
            .unwrap()
            .set("Kids", vec![Object::Reference(field)]);
        let ap = appearance(&mut doc, 20.0, 20.0, b"q Q".to_vec(), Dictionary::new());
        let check = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "T" => Object::string_literal("consent"), "Rect" => vec![20.into(), 20.into(), 40.into(), 40.into()], "F" => 4, "V" => "Off", "AS" => "Off", "AP" => dictionary! { "N" => dictionary! { "Accepted" => ap, "Off" => ap } } });
        doc.get_dictionary_mut(page_ids[0])
            .unwrap()
            .get_mut(b"Annots")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(check.into());
        doc.objects.insert(pages, dictionary! { "Type" => "Pages", "Count" => 2, "Kids" => page_ids.iter().copied().map(Object::Reference).collect::<Vec<_>>() }.into());
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages, "AcroForm" => dictionary! { "Fields" => vec![parent.into(), check.into()] } });
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        if let Ok(path) = std::env::var("TPDF_FORM_FIXTURE") {
            std::fs::write(path, &bytes).unwrap();
        }
        (Document::load_mem(&bytes).unwrap(), field, check)
    }

    #[test]
    fn forms_round_trip_values_and_every_shared_widget_appearance() {
        let (mut doc, field, check) = fixture();
        let before = scan(&doc).unwrap();
        assert_eq!(before.widgets.len(), 3);
        assert_eq!(before.widgets[0].name, "ACME.answer");
        write(
            &mut doc,
            &[
                Change {
                    object: field,
                    value: Value::Text("Grüße".into()),
                },
                Change {
                    object: check,
                    value: Value::Checked(true),
                },
            ],
        )
        .unwrap();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let saved = Document::load_mem(&bytes).unwrap();
        let after = scan(&saved).unwrap();
        assert_eq!(
            saved
                .get_dictionary(field)
                .unwrap()
                .get(b"FT")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Tx",
            "the terminal field must expose its type to PDFKit"
        );
        for widget in after.widgets.iter().filter(|w| w.object == field) {
            assert_eq!(widget.value, Value::Text("Grüße".into()));
            let ap = saved
                .get_dictionary(widget.widget)
                .unwrap()
                .get(b"AP")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"N")
                .unwrap()
                .as_reference()
                .unwrap();
            let stream = saved.get_object(ap).unwrap().as_stream().unwrap();
            let content = stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone());
            assert!(String::from_utf8_lossy(&content).contains("4772fcdf65"));
        }
        assert_eq!(
            saved
                .get_dictionary(check)
                .unwrap()
                .get(b"V")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Accepted"
        );
        assert_eq!(
            saved
                .get_dictionary(check)
                .unwrap()
                .get(b"AS")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Accepted"
        );
        write(
            &mut doc,
            &[
                Change {
                    object: field,
                    value: Value::Text(String::new()),
                },
                Change {
                    object: check,
                    value: Value::Checked(false),
                },
            ],
        )
        .unwrap();
        assert_eq!(
            scan(&doc).unwrap().widgets[0].value,
            Value::Text(String::new())
        );
        assert_eq!(
            doc.get_dictionary(check)
                .unwrap()
                .get(b"V")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Off"
        );
        // Optional artifact for the independent PDFKit and PDFium checks.
        if let Ok(path) = std::env::var("TPDF_FORM_PROBE") {
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn forms_refuse_readonly_wrong_type_and_unrenderable_answers() {
        let (mut doc, field, _) = fixture();
        for value in [
            Value::Text("not supported: 漢".into()),
            Value::Text("x".repeat(21)),
            Value::Checked(true),
            Value::Text("two\nlines".into()),
        ] {
            assert!(write(
                &mut doc,
                &[Change {
                    object: field,
                    value
                }]
            )
            .is_err());
        }
        doc.get_dictionary_mut(field).unwrap().set("Ff", 1);
        assert!(write(
            &mut doc,
            &[Change {
                object: field,
                value: Value::Text("new".into())
            }]
        )
        .is_err());
    }

    #[test]
    fn forms_journal_undo_redo_and_redo_branch_are_independent_of_comments() {
        use crate::docmodel::{Doc, ObjectId as Id};
        let mut doc = Doc::open(2);
        let field = Id::new(99, 0);
        doc.fill(field, Value::Text("one".into())).unwrap();
        doc.fill(field, Value::Text("two".into())).unwrap();
        assert_eq!(doc.form_changes()[0].value, Value::Text("two".into()));
        doc.undo();
        assert_eq!(doc.form_changes()[0].value, Value::Text("one".into()));
        doc.undo();
        assert!(doc.form_changes().is_empty());
        doc.redo();
        assert_eq!(doc.form_changes()[0].value, Value::Text("one".into()));
        doc.fill(field, Value::Text("three".into())).unwrap();
        assert!(!doc.can_redo());
        doc.undo();
        assert_eq!(doc.form_changes()[0].value, Value::Text("one".into()));
    }

    #[test]
    fn forms_xfa_and_cyclic_ancestry_are_errors_not_empty_forms() {
        let (mut doc, field, _) = fixture();
        doc.get_dictionary_mut(field).unwrap().set("Parent", field);
        assert!(scan(&doc).is_err());
        let (mut doc, _, _) = fixture();
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        doc.get_dictionary_mut(root)
            .unwrap()
            .get_mut(b"AcroForm")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("XFA", Object::string_literal("synthetic"));
        assert!(scan(&doc).is_err());
    }
}
