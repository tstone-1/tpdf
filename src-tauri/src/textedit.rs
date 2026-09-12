//! Conservative content-stream text editing, executed in the document worker.
//!
//! Supported text uses Helvetica/WinAnsi and printable ASCII, with explicit
//! positioning between shows. Font/leading setup may precede a text block.
//! Graphics, custom text state and implicit advances between shows are refused.
//! Addresses refer to decoded operators, never PDFium's text-object ordinals.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_CONTENT: usize = 1024 * 1024;
const MAX_OPERATIONS: usize = 4096;
pub(crate) const MAX_TEXT: usize = 4096;
pub(crate) const MAX_CHANGES: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub operator: u32,
    pub text: String,
    pub font: String,
    pub size: f64,
    /// Text matrix in the page's original user space, before crop and rotation.
    pub matrix: [f64; 6],
    pub advance: f64,
    /// Hit rectangle in the original displayed page, before journal crop and turns.
    pub display_rect: [f32; 4],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PageRuns {
    pub page: u32,
    /// Binds operator addresses to the exact decoded content that was inspected.
    pub revision: Vec<u8>,
    pub runs: Vec<Run>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub page: u32,
    pub revision: Vec<u8>,
    pub operator: u32,
    pub original: String,
    pub replacement: String,
}

fn dictionary<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Dictionary, String> {
    crate::encoding::resolve_dict(doc, value).map_err(|()| "invalid text resources".into())
}

fn resources(doc: &Document, page: ObjectId) -> Result<&Dictionary, String> {
    let mut id = page;
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        if !seen.insert(id) {
            break;
        }
        let node = doc.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = node.get(b"Resources") {
            return dictionary(doc, value);
        }
        id = node
            .get(b"Parent")
            .and_then(Object::as_reference)
            .map_err(|e| e.to_string())?;
    }
    Err("text resource inheritance exceeds its limit".into())
}

fn font(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<(), String> {
    let fonts = dictionary(doc, resources.get(b"Font").map_err(|e| e.to_string())?)?;
    let font = dictionary(doc, fonts.get(name).map_err(|e| e.to_string())?)?;
    for (key, expected) in [
        (b"Type".as_slice(), b"Font".as_slice()),
        (b"Subtype", b"Type1"),
        (b"BaseFont", b"Helvetica"),
        (b"Encoding", b"WinAnsiEncoding"),
    ] {
        if font.get(key).and_then(Object::as_name).ok() != Some(expected) {
            return Err("text editing currently requires Helvetica with WinAnsiEncoding".into());
        }
    }
    if font.iter().any(|(key, _)| {
        !matches!(
            key.as_slice(),
            b"Type" | b"Subtype" | b"BaseFont" | b"Encoding" | b"Name"
        )
    }) {
        return Err("custom font metrics or character mappings are not editable yet".into());
    }
    Ok(())
}

fn number(value: &Object) -> Result<f64, String> {
    let value = match value {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return Err("invalid text position or size".into()),
    };
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err("text position or size exceeds its limit".into());
    }
    Ok(value)
}

fn ascii(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > MAX_TEXT || !bytes.iter().all(|byte| (32..=126).contains(byte)) {
        return Err("text editing currently supports printable ASCII only".into());
    }
    std::str::from_utf8(bytes).map_err(|e| e.to_string())
}

fn page_content(doc: &Document, id: ObjectId) -> Result<Vec<u8>, String> {
    let page = doc.get_dictionary(id).map_err(|e| e.to_string())?;
    let contents = page.get(b"Contents").map_err(|e| e.to_string())?;
    let contents = crate::encoding::resolve(doc, contents);
    let streams = match contents {
        Object::Array(values) => values.as_slice(),
        value => std::slice::from_ref(value),
    };
    if streams.len() > 128 {
        return Err("too many page content streams".into());
    }
    let mut bytes = Vec::new();
    for value in streams {
        let stream = crate::encoding::resolve(doc, value)
            .as_stream()
            .map_err(|e| e.to_string())?;
        if stream.dict.has(b"F") || stream.dict.has(b"DecodeParms") {
            return Err("external or parameterised content streams are not editable yet".into());
        }
        // The general page helper skips invalid references and falls back to
        // raw bytes on decode errors. Editing must never accept that partial view.
        if let Ok(filter) = stream.dict.get(b"Filter") {
            let filter = match filter {
                Object::Array(filters) if filters.len() == 1 => &filters[0],
                value => value,
            };
            if filter.as_name().ok() != Some(b"FlateDecode") {
                return Err("unsupported content stream filter".into());
            }
        }
        let remaining = MAX_CONTENT.saturating_sub(bytes.len() + 1);
        let decoded = if stream.dict.has(b"Filter") {
            let mut decoder = flate2::Decompress::new(true);
            let mut output = vec![0; remaining + 1];
            let status = decoder
                .decompress(
                    &stream.content,
                    &mut output,
                    flate2::FlushDecompress::Finish,
                )
                .map_err(|e| format!("invalid Flate content: {e}"))?;
            if status != flate2::Status::StreamEnd
                || decoder.total_in() != stream.content.len() as u64
                || decoder.total_out() > remaining as u64
            {
                return Err("incomplete or oversized Flate content".into());
            }
            output.truncate(decoder.total_out() as usize);
            output
        } else {
            if stream.content.len() > remaining {
                return Err("page content exceeds its limit".into());
            }
            stream.content.clone()
        };
        bytes.extend(decoded);
        bytes.push(b'\n');
        if bytes.len() > MAX_CONTENT {
            return Err("page content exceeds its limit".into());
        }
    }
    Ok(bytes)
}

// Td/T* translate the line matrix, not the text matrix advanced by Tj.
// Only diagonal matrices reach here; reject accumulated positions outside the
// same bound used for authored coordinates.
fn move_line(matrix: &mut [f64; 6], x: f64, y: f64) -> Result<(), String> {
    matrix[4] += x * matrix[0];
    matrix[5] += y * matrix[3];
    if matrix[4..]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return Err("text position exceeds its limit".into());
    }
    Ok(())
}

fn inspect(doc: &Document, page: u32) -> Result<(ObjectId, Content, PageRuns), String> {
    if doc
        .catalog()
        .map_err(|e| e.to_string())?
        .has(b"StructTreeRoot")
    {
        return Err("tagged text is not editable yet".into());
    }
    let pages = crate::pagetree::ordered_pages(doc);
    let id = *pages
        .get(page as usize)
        .ok_or("text page is not in this document")?;
    if pages.iter().filter(|&&other| other == id).count() != 1 {
        return Err("a repeated page object is not editable".into());
    }
    let bytes = page_content(doc, id)?;
    let content = Content::decode_strict(&bytes).map_err(|e| e.to_string())?;
    if content.operations.len() > MAX_OPERATIONS {
        return Err("text operator count exceeds its limit".into());
    }
    let resources = resources(doc, id)?;
    let mut result = PageRuns {
        page,
        revision: Sha256::digest(&bytes).to_vec(),
        runs: Vec::new(),
    };
    // Never skip unknown operators: graphics and text state can change a Tj's
    // meaning without changing its string. Tf and TL persist across BT/ET;
    // the text/line matrices reset at BT. Every accepted show has a position
    // independent of the preceding show's advance, so shorter edits cannot
    // move following text.
    let mut inside = false;
    let mut positioned = false;
    let mut selected_font = None;
    let mut leading = 0.0;
    let mut matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for (index, op) in content.operations.iter().enumerate() {
        match (op.operator.as_str(), op.operands.as_slice()) {
            ("cm", values) if !inside && values.len() == 6 => {
                for (value, expected) in values.iter().zip([1., 0., 0., 1., 0., 0.]) {
                    if number(value)? != expected {
                        return Err("transformed page content is not editable yet".into());
                    }
                }
            }
            ("BT", []) if !inside => {
                inside = true;
                positioned = false;
                matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
            }
            ("ET", []) if inside => inside = false,
            ("Tf", [name, size]) if inside => {
                let name = name.as_name().map_err(|e| e.to_string())?;
                font(doc, resources, name)?;
                let size = number(size)?;
                if !(0.0..=1000.0).contains(&size) || size == 0.0 {
                    return Err("unsupported text size".into());
                }
                selected_font = Some((name, size));
            }
            ("TL", [value]) if inside => leading = number(value)?,
            ("Tm", values) if inside && values.len() == 6 => {
                for (dest, value) in matrix.iter_mut().zip(values) {
                    *dest = number(value)?;
                }
                if matrix[0] <= 0.0 || matrix[3] <= 0.0 || matrix[1] != 0.0 || matrix[2] != 0.0 {
                    return Err("rotated or skewed text is not editable yet".into());
                }
                positioned = true;
            }
            ("Td", [x, y]) if inside => {
                move_line(&mut matrix, number(x)?, number(y)?)?;
                positioned = true;
            }
            ("T*", []) if inside => {
                move_line(&mut matrix, 0.0, -leading)?;
                positioned = true;
            }
            ("Tj", [_]) if inside && positioned => {}
            _ => return Err("unsupported text state or positioning between shows".into()),
        }
        if op.operator != "Tj" {
            continue;
        }
        let (name, size) = selected_font.ok_or("text has no explicit font")?;
        let text = ascii(op.operands[0].as_str().map_err(|e| e.to_string())?)?;
        positioned = false;
        let geometry = crate::pagetree::displayed_page(doc, id);
        let advance = crate::textbox::advance(text, size);
        let x = matrix[4] - f64::from(geometry.origin.0);
        let y = matrix[5] - f64::from(geometry.origin.1);
        let display_rect = crate::text::to_device(
            geometry.turns,
            geometry.width,
            geometry.height,
            [
                x,
                y - size * matrix[3] * 0.25,
                x + advance * matrix[0],
                y + size * matrix[3],
            ],
        );
        if display_rect.iter().any(|v| !v.is_finite()) {
            return Err("text bounds exceed the display range".into());
        }
        result.runs.push(Run {
            display_rect,
            operator: index as u32,
            text: text.into(),
            font: String::from_utf8_lossy(name).into_owned(),
            size,
            matrix,
            advance,
        });
    }
    if inside {
        return Err("unterminated text block".into());
    }
    Ok((id, content, result))
}

/// Discover a complete supported page, or explain why it cannot be edited yet.
///
/// # Errors
/// Unsupported content, invalid resources, or exhausted parsing limits.
pub fn scan(doc: &Document, page: u32) -> Result<PageRuns, String> {
    inspect(doc, page).map(|(_, _, runs)| runs)
}

/// Validate the entire batch before changing any page. Shared streams are cloned.
///
/// # Errors
/// Unsupported, stale, duplicate, unchanged, or overflowing replacements.
pub fn write(doc: &mut Document, changes: &[Change]) -> Result<(), String> {
    if changes.len() > MAX_CHANGES {
        return Err("too many text replacements".into());
    }
    let mut prepared = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for change in changes {
        if !seen.insert((change.page, change.operator)) {
            return Err("duplicate text replacement".into());
        }
        ascii(change.replacement.as_bytes())?;
        if change.original == change.replacement {
            return Err("text replacement is unchanged".into());
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = prepared.entry(change.page) {
            entry.insert(inspect(doc, change.page)?);
        }
        let (_, content, runs) = prepared.get_mut(&change.page).ok_or("missing text page")?;
        let run = runs
            .runs
            .iter()
            .find(|run| run.operator == change.operator)
            .ok_or("text run no longer exists")?;
        if change.revision != runs.revision || change.original != run.text {
            return Err("text changed since this run was inspected".into());
        }
        if crate::textbox::advance(&change.replacement, run.size) > run.advance + 0.000_001 {
            return Err("replacement would exceed the original text advance".into());
        }
        content.operations[change.operator as usize].operands[0] =
            Object::string_literal(change.replacement.as_bytes());
    }
    let ready = prepared
        .into_values()
        .map(|(id, content, _)| {
            content
                .encode()
                .map(|bytes| (id, bytes))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (page, bytes) in ready {
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes));
        doc.get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|e| e.to_string())?
            .set("Contents", stream);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::dictionary;

    pub(crate) fn fixture() -> Document {
        let mut doc = Document::with_version("1.7");
        let root = doc.new_object_id();
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
        let contents = doc.add_object(Stream::new(Dictionary::new(), b"BT /F1 12 Tf 40 180 Td (SYNTHETIC FIRST) Tj ET\nBT /F1 12 Tf 40 140 Td (SYNTHETIC SECOND) Tj ET".to_vec()));
        let pages: Vec<Object> = (0..2)
            .map(|_| {
                doc.add_object(dictionary! {
                    "Type" => "Page", "Parent" => root, "Contents" => contents,
                })
                .into()
            })
            .collect();
        doc.objects.insert(
            root,
            dictionary! { "Type" => "Pages", "Kids" => pages, "Count" => 2,
                "MediaBox" => vec![0.into(), 0.into(), 300.into(), 240.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
            }
            .into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        doc.trailer.set("Root", catalog);
        doc
    }

    pub(crate) fn change(doc: &Document) -> Change {
        let runs = scan(doc, 0).unwrap();
        Change {
            page: 0,
            revision: runs.revision,
            operator: runs.runs[0].operator,
            original: runs.runs[0].text.clone(),
            replacement: "EDITED FIRST".into(),
        }
    }

    fn with_content(bytes: &[u8]) -> Document {
        let mut doc = fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.to_vec()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", stream);
        doc
    }

    #[test]
    fn textedit_reportlab_font_setup_and_multiline_positions_survive_shorter_edits() {
        // Produced independently by testdata/make_textedit_reportlab.py.
        let mut doc = with_content(b"1 0 0 1 0 0 cm BT /F1 12 Tf 14.4 TL ET\nBT 1 0 0 1 40 180 Tm 40 TL (SYNTHETIC FIRST) Tj T* (SYNTHETIC SECOND) Tj T* ET");
        let (_, before, mapped) = inspect(&doc, 0).unwrap();
        assert_eq!(
            mapped.runs.iter().map(|r| r.operator).collect::<Vec<_>>(),
            [8, 10]
        );
        assert_eq!(mapped.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        assert_eq!(mapped.runs[1].matrix, [1., 0., 0., 1., 40., 140.]);
        assert_eq!(mapped.runs[1].size, 12.);
        let mut edit = change(&doc);
        edit.replacement = "X".into();
        write(&mut doc, &[edit]).unwrap();
        let (_, after, saved) = inspect(&doc, 0).unwrap();
        assert_eq!(saved.runs[0].text, "X");
        assert_eq!(saved.runs[1], mapped.runs[1]);
        assert_eq!(before.operations.len(), after.operations.len());
        for (index, (a, b)) in before.operations.iter().zip(&after.operations).enumerate() {
            if index != 8 {
                assert_eq!(a.operator, b.operator, "changed operator {index}");
                assert_eq!(a.operands, b.operands, "changed operands {index}");
            }
        }
    }

    #[test]
    fn textedit_line_positions_use_scaled_line_matrix_and_reset_on_bt() {
        let doc = with_content(b"BT /F1 12 Tf 20 TL 2 0 0 3 40 180 Tm (FIRST) Tj 10 -10 Td (SECOND) Tj T* (THIRD) Tj ET BT 5 200 Td (FOURTH) Tj T* (FIFTH) Tj ET");
        let runs = scan(&doc, 0).unwrap().runs;
        assert_eq!(
            runs.iter()
                .map(|r| [r.matrix[4], r.matrix[5]])
                .collect::<Vec<_>>(),
            [[40., 180.], [60., 150.], [60., 90.], [5., 200.], [5., 180.]]
        );
        assert_eq!(runs[3].matrix[..4], [1., 0., 0., 1.]);
        assert!(runs.iter().all(|r| r.size == 12.));
    }

    #[test]
    fn textedit_accepts_single_flate_array_but_refuses_filter_chains() {
        let mut doc = fixture();
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let stream = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(stream)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .compress()
            .unwrap();
        for (filters, accepted) in [
            (vec![Object::Name(b"FlateDecode".to_vec())], true),
            (vec![], false),
            (vec![Object::Name(b"ASCII85Decode".to_vec())], false),
            (vec![Object::Name(b"FlateDecode".to_vec()); 2], false),
        ] {
            doc.get_object_mut(stream)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .dict
                .set("Filter", Object::Array(filters));
            assert_eq!(scan(&doc, 0).is_ok(), accepted);
        }
    }

    #[test]
    fn textedit_maps_operators_and_clones_shared_streams() {
        let mut doc = fixture();
        let (_, before, runs) = inspect(&doc, 0).unwrap();
        assert_eq!(
            runs.runs.iter().map(|r| r.operator).collect::<Vec<_>>(),
            [3, 8]
        );
        assert_eq!(runs.runs[0].matrix, [1., 0., 0., 1., 40., 180.]);
        let first = change(&doc);
        let second = Change {
            operator: 8,
            original: "SYNTHETIC SECOND".into(),
            replacement: "EDITED SECOND".into(),
            ..first.clone()
        };
        write(&mut doc, &[first, second]).unwrap();
        let (_, after, saved) = inspect(&doc, 0).unwrap();
        assert_eq!(saved.runs[0].text, "EDITED FIRST");
        assert_eq!(saved.runs[1].text, "EDITED SECOND");
        assert_eq!(scan(&doc, 1).unwrap().runs[0].text, "SYNTHETIC FIRST");
        for (a, b) in before.operations.iter().zip(after.operations.iter()) {
            if a.operator != "Tj" {
                assert_eq!(a.operator, b.operator);
                assert_eq!(a.operands, b.operands);
            }
        }
    }

    #[test]
    fn textedit_rejects_invalid_batches_without_mutating_the_document() {
        let doc = fixture();
        let valid = change(&doc);
        let variants = [
            (
                Change {
                    revision: vec![0; 32],
                    ..valid.clone()
                },
                "changed since",
            ),
            (
                Change {
                    original: "WRONG".into(),
                    ..valid.clone()
                },
                "changed since",
            ),
            (
                Change {
                    operator: 4,
                    ..valid.clone()
                },
                "no longer exists",
            ),
            (
                Change {
                    page: 9,
                    ..valid.clone()
                },
                "not in this document",
            ),
            (
                Change {
                    replacement: "Z".repeat(80),
                    ..valid.clone()
                },
                "exceed the original",
            ),
            (
                Change {
                    replacement: "\u{03b1}".into(),
                    ..valid.clone()
                },
                "ASCII only",
            ),
            (
                Change {
                    replacement: valid.original.clone(),
                    ..valid.clone()
                },
                "unchanged",
            ),
        ];
        for (bad, expected) in variants {
            let mut copy = doc.clone();
            // Put a valid edit on the other page first, so a late refusal also
            // proves validation does not partially apply a batch.
            let prior = Change {
                page: 1,
                ..valid.clone()
            };
            assert!(write(&mut copy, &[prior, bad])
                .unwrap_err()
                .contains(expected));
            assert_eq!(copy.objects, doc.objects);
            assert_eq!(copy.max_id, doc.max_id);
        }
        let mut copy = doc.clone();
        assert!(write(&mut copy, &[valid.clone(), valid])
            .unwrap_err()
            .contains("duplicate"));
        assert_eq!(copy.objects, doc.objects);
    }

    #[test]
    fn textedit_refuses_unsupported_state_and_font_semantics() {
        for bytes in [
            "BT /F1 12 Tf 40 180 Td [(SYNTHETIC)] TJ ET",
            "BT /F1 12 Tf 40 180 Td (ONE) Tj (TWO) Tj ET",
            "3 Tr BT /F1 12 Tf 40 180 Td (HIDDEN) Tj ET",
            "2 0 0 2 0 0 cm BT /F1 12 Tf 40 180 Td (SCALED) Tj ET",
            "BT /F1 12 Tf 0 1 -1 0 40 180 Tm (ROTATED) Tj ET",
            "/Span << /ActualText (OTHER) >> BDC BT /F1 12 Tf 40 180 Td (TEXT) Tj ET EMC",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj",
            "BT BT /F1 12 Tf 40 180 Td (TEXT) Tj ET ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET ET",
            "BT 40 180 Td (TEXT) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj 1 Tc T* (MORE) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj /F1 10 Tf (MORE) Tj ET",
            "BT /F1 12 Tf (TEXT) Tj ET",
            "BT /F1 12 Tf 1000000 0 0 1 40 180 Tm 2 0 Td (TEXT) Tj ET",
            "1 0 0 1 1 0 cm BT /F1 12 Tf 40 180 Td (TEXT) Tj ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj 1 T* ET",
            "BT /F1 12 Tf 40 180 Td (TEXT) Tj ET (OUTSIDE) Tj",
        ] {
            let mut doc = fixture();
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let stream = doc.add_object(Stream::new(Dictionary::new(), bytes.as_bytes().to_vec()));
            doc.get_dictionary_mut(id).unwrap().set("Contents", stream);
            assert!(scan(&doc, 0).is_err(), "accepted {bytes}");
        }
        for key in ["Widths", "ToUnicode", "FontDescriptor"] {
            let mut doc = fixture();
            let font = doc
                .objects
                .values_mut()
                .find_map(|o| o.as_dict_mut().ok().filter(|d| d.has(b"BaseFont")))
                .unwrap();
            font.set(key, Object::Null);
            assert!(scan(&doc, 0).unwrap_err().contains("custom font"));
        }
    }

    #[test]
    fn textedit_validates_empty_replacement_positioning_and_limits() {
        let mut doc = fixture();
        let mut edit = change(&doc);
        edit.replacement.clear();
        write(&mut doc, &[edit]).unwrap();
        assert_eq!(scan(&doc, 0).unwrap().runs[0].text, "");
        assert_eq!(scan(&doc, 0).unwrap().runs[1].matrix[5], 140.);
        assert!(
            write(&mut fixture(), &vec![change(&fixture()); MAX_CHANGES + 1])
                .unwrap_err()
                .contains("too many")
        );
        assert!(ascii(&vec![b'A'; MAX_TEXT + 1]).is_err());
    }

    #[test]
    fn textedit_rejects_partial_or_undecodable_content() {
        for kind in [
            "bad-reference",
            "bad-filter",
            "invalid-flate",
            "trailing-junk",
            "too-large",
        ] {
            let mut doc = fixture();
            let id = crate::pagetree::ordered_pages(&doc)[0];
            let original = doc
                .get_dictionary(id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .clone();
            match kind {
                "bad-reference" => doc
                    .get_dictionary_mut(id)
                    .unwrap()
                    .set("Contents", vec![original, Object::Reference((999999, 0))]),
                _ => {
                    let stream = doc
                        .get_object_mut(original.as_reference().unwrap())
                        .unwrap()
                        .as_stream_mut()
                        .unwrap();
                    match kind {
                        "bad-filter" => stream.dict.set("Filter", 7),
                        "invalid-flate" => stream.dict.set("Filter", "FlateDecode"),
                        "trailing-junk" => stream.content.extend(b"\n(unfinished"),
                        "too-large" => stream.content = vec![b' '; MAX_CONTENT + 1],
                        _ => unreachable!(),
                    }
                }
            }
            assert!(scan(&doc, 0).is_err(), "accepted {kind}");
        }
        let mut compressed = fixture();
        compressed.compress();
        assert_eq!(
            scan(&compressed, 0).unwrap().runs[0].text,
            "SYNTHETIC FIRST"
        );
    }
}
