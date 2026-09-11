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
    Selection(Vec<usize>),
}

/// Display labels and export values are different parts of a choice option.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Choice {
    pub export: String,
    pub label: String,
}

/// Control semantics travel with each widget; answers still belong to the field.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Control {
    Text,
    Checkbox,
    Radio {
        index: usize,
        states: Vec<Vec<u8>>,
        unison: bool,
        no_toggle_off: bool,
    },
    Choice {
        options: Vec<Choice>,
        combo: bool,
        editable: bool,
        multiple: bool,
    },
    Unsupported,
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
    pub control: Control,
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

fn option_text(doc: &Document, value: &Object) -> Result<String, String> {
    let raw = doc
        .dereference(value)
        .map_err(|e| e.to_string())?
        .1
        .as_str()
        .map_err(|_| "A choice option is not a text string")?;
    if raw.len() > 16384 {
        return Err("This form contains an oversized option".into());
    }
    Ok(crate::annots::decode_text_string(raw))
}

fn choice_options(doc: &Document, id: ObjectId) -> Result<Vec<Choice>, String> {
    let Some(raw) = inherited(doc, id, b"Opt") else {
        return Ok(Vec::new());
    };
    let values = raw.as_array().map_err(|_| "Invalid choice options")?;
    if values.len() > 4096 {
        return Err("This form exceeds the option limit".into());
    }
    let mut bytes = 0;
    values
        .iter()
        .map(|raw| {
            let raw = doc.dereference(raw).map_err(|e| e.to_string())?.1;
            let choice = if let Ok(pair) = raw.as_array() {
                if pair.len() != 2 {
                    return Err("Invalid choice export/label pair".into());
                }
                Choice {
                    export: option_text(doc, &pair[0])?,
                    label: option_text(doc, &pair[1])?,
                }
            } else {
                let text = option_text(doc, raw)?;
                Choice {
                    export: text.clone(),
                    label: text,
                }
            };
            bytes += choice.export.len() + choice.label.len() + 32;
            if bytes > 1_048_576 {
                return Err("This form exceeds the option text limit".into());
            }
            Ok(choice)
        })
        .collect()
}

fn choice_value(
    doc: &Document,
    id: ObjectId,
    options: &[Choice],
    editable: bool,
) -> Result<Value, String> {
    let values = match inherited(doc, id, b"V") {
        None => Vec::new(),
        Some(Object::Array(values)) => {
            if values.len() > 4096 {
                return Err("This form exceeds the selection limit".into());
            }
            values
                .iter()
                .map(|v| option_text(doc, v))
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(value) => vec![option_text(doc, value)?],
    };
    // I disambiguates duplicate export values, but V takes precedence over stale I.
    if let Some(Object::Array(indices)) = inherited(doc, id, b"I") {
        if indices.len() > 4096 {
            return Err("This form exceeds the selection limit".into());
        }
        let indices: Option<Vec<usize>> = indices
            .iter()
            .map(|i| i.as_i64().ok().and_then(|n| usize::try_from(n).ok()))
            .collect();
        if let Some(indices) = indices {
            if indices.windows(2).all(|p| p[0] < p[1])
                && indices.len() == values.len()
                && indices
                    .iter()
                    .zip(&values)
                    .all(|(i, v)| options.get(*i).is_some_and(|o| &o.export == v))
            {
                return Ok(Value::Selection(indices));
            }
        }
    }
    let mut selected = Vec::new();
    for value in &values {
        if options.iter().filter(|o| &o.export == value).count() > 1 {
            return Err(
                "A choice with duplicate export values has no valid selection indices".into(),
            );
        }
        let Some(index) = options.iter().position(|o| &o.export == value) else {
            if value.is_empty() && values.len() == 1 {
                return Ok(Value::Selection(Vec::new()));
            }
            if editable && values.len() == 1 {
                return Ok(Value::Text(value.clone()));
            }
            return Err("The saved choice is absent from its options".into());
        };
        selected.push(index);
    }
    selected.sort_unstable();
    selected.dedup();
    Ok(Value::Selection(selected))
}

fn button_state(doc: &Document, id: ObjectId) -> Result<Vec<u8>, String> {
    let normal = doc
        .get_dictionary(id)
        .ok()
        .and_then(|w| w.get(b"AP").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .and_then(|a| a.get(b"N").ok())
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .ok_or("A radio button has no appearance states")?;
    let states: Vec<_> = normal
        .iter()
        .filter(|(k, _)| k.as_slice() != b"Off")
        .collect();
    if states.len() != 1 || states[0].0.len() > 1024 {
        return Err("A radio button has ambiguous appearance states".into());
    }
    for state in [b"Off".as_slice(), states[0].0.as_slice()] {
        let ap = normal
            .get(state)
            .map_err(|_| "A radio button is missing an appearance")?;
        doc.dereference(ap)
            .map_err(|e| e.to_string())?
            .1
            .as_stream()
            .map_err(|_| "A radio button appearance is not a stream")?;
    }
    Ok(states[0].0.clone())
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
            let radio = kind == Some(b"Btn") && flags & (1 << 15) != 0 && flags & (1 << 16) == 0;
            let choice = kind == Some(b"Ch");
            let control = if text {
                Control::Text
            } else if checkbox {
                Control::Checkbox
            } else if radio {
                Control::Radio {
                    index: 0,
                    states: Vec::new(),
                    unison: flags & (1 << 25) != 0,
                    no_toggle_off: flags & (1 << 14) != 0,
                }
            } else if choice {
                Control::Choice {
                    options: choice_options(doc, object)?,
                    combo: flags & (1 << 17) != 0,
                    editable: flags & (1 << 18) != 0 && flags & (1 << 17) != 0,
                    multiple: flags & (1 << 21) != 0,
                }
            } else {
                Control::Unsupported
            };
            if inherited(doc, object, b"V")
                .and_then(|v| v.as_str().ok())
                .is_some_and(|raw| raw.len() > 16384)
            {
                return Err("This form contains an oversized answer".into());
            }
            let value = if let Control::Choice {
                options, editable, ..
            } = &control
            {
                choice_value(doc, object, options, *editable)?
            } else if radio {
                Value::Selection(Vec::new())
            } else if checkbox {
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
                    Value::Selection(indices) => indices.len() * 8,
                };
            if let Control::Choice { options, .. } = &control {
                text_bytes += options
                    .iter()
                    .map(|o| o.export.len() + o.label.len() + 32)
                    .sum::<usize>();
            }
            if text_bytes > 1_048_576 {
                return Err("This form exceeds the text limit".into());
            }
            let annotation_flags = w.get(b"F").and_then(Object::as_i64).unwrap_or(0);
            let reason = if !text && !checkbox && !radio && !choice {
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
                control,
                multiline: text && flags & (1 << 12) != 0,
                max_length: (text && max > 0).then_some(max as usize),
                reason,
            });
            if result.widgets.len() > 4096 {
                return Err("This form exceeds the widget limit".into());
            }
        }
    }
    let mut groups: BTreeMap<ObjectId, Vec<usize>> = BTreeMap::new();
    for (i, widget) in result.widgets.iter().enumerate() {
        if matches!(widget.control, Control::Radio { .. }) {
            groups.entry(widget.object).or_default().push(i);
        }
    }
    for group in groups.values_mut() {
        // Page moves must not change the meaning of an answer already journalled.
        group.sort_by_key(|i| result.widgets[*i].widget);
        let states = group
            .iter()
            .map(|i| button_state(doc, result.widgets[*i].widget))
            .collect::<Result<Vec<_>, _>>();
        let states = match states {
            Ok(states) => states,
            Err(reason) => {
                for i in group.iter() {
                    result.widgets[*i].reason = Some(reason.clone());
                }
                continue;
            }
        };
        text_bytes += group.len() * states.iter().map(|s| s.len() * 4 + 16).sum::<usize>();
        if text_bytes > 1_048_576 {
            return Err("This form exceeds the text limit".into());
        }
        let selected = group
            .iter()
            .enumerate()
            .find_map(|(index, i)| {
                let widget = &result.widgets[*i];
                let state = doc
                    .get_dictionary(widget.widget)
                    .ok()?
                    .get(b"AS")
                    .ok()?
                    .as_name()
                    .ok()?;
                (state == states[index]).then_some(index)
            })
            .or_else(|| {
                let value = inherited(doc, result.widgets[group[0]].object, b"V")?
                    .as_name()
                    .ok()?;
                states.iter().position(|s| s == value)
            });
        for (index, i) in group.iter().enumerate() {
            let widget = &mut result.widgets[*i];
            widget.value = Value::Selection(selected.into_iter().collect());
            if let Control::Radio {
                index: at,
                states: all,
                ..
            } = &mut widget.control
            {
                *at = index;
                *all = states.clone();
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
    match (&widget.control, value) {
        (
            Control::Radio {
                states,
                no_toggle_off,
                ..
            },
            Value::Selection(indices),
        ) => {
            if indices.len() > 1
                || indices.iter().any(|i| *i >= states.len())
                || (*no_toggle_off && indices.is_empty())
            {
                return Err("Choose one available radio button".into());
            }
            return Ok(());
        }
        (
            Control::Choice {
                options,
                multiple,
                combo,
                ..
            },
            Value::Selection(indices),
        ) => {
            if (!multiple || *combo) && indices.len() > 1
                || !indices.windows(2).all(|p| p[0] < p[1])
                || indices.iter().any(|i| *i >= options.len())
            {
                return Err("Choose available options without duplicates".into());
            }
            for index in indices {
                if !crate::textbox::encodable(&options[*index].label)
                    || options[*index].label.contains(['\r', '\n'])
                {
                    return Err("This option uses characters that cannot be saved visibly".into());
                }
            }
            choice_layout(widget, indices)?;
            return Ok(());
        }
        (Control::Choice { editable: true, .. }, Value::Text(_)) => {}
        (Control::Text, Value::Text(_)) | (Control::Checkbox, Value::Checked(_)) => {}
        _ => return Err("The answer does not match the field type".into()),
    }
    match (&widget.control, value) {
        (_, Value::Text(text)) => {
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
        (_, Value::Checked(_)) => Ok(()),
        _ => Err("The answer does not match the field type".into()),
    }
}

fn choice_layout(widget: &Widget, indices: &[usize]) -> Result<(f64, Vec<String>), String> {
    let Control::Choice { options, combo, .. } = &widget.control else {
        unreachable!()
    };
    if *combo {
        return text_layout(
            widget,
            indices
                .first()
                .map(|i| options[*i].label.as_str())
                .unwrap_or(""),
        );
    }
    // A list remains a list on paper, with selected rows shaded. TI names the
    // first visible row so another editor starts at the same place.
    let top = indices.first().copied().unwrap_or(0);
    let height = widget.rect[3] - widget.rect[1];
    let size = 12.0_f64.min((height - 4.0) / 1.2);
    if size < 4.0 {
        return Err("The options do not fit visibly in this field".into());
    }
    let count = ((height - 2.0) / (size * 1.2)).floor() as usize;
    let lines: Vec<_> = options
        .iter()
        .skip(top)
        .take(count)
        .map(|o| o.label.clone())
        .collect();
    if lines.iter().any(|s| !crate::textbox::encodable(s)) {
        return Err("This list uses characters that cannot be saved visibly".into());
    }
    Ok((size, lines))
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

fn write_text_appearance(
    doc: &mut Document,
    widget: &Widget,
    size: f64,
    lines: &[String],
    selected: &[usize],
) -> Result<(), String> {
    let width = widget.rect[2] - widget.rect[0];
    let height = widget.rect[3] - widget.rect[1];
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let mut body = format!("q 1 1 1 rg 0 0 {width} {height} re f 0 0 {width} {height} re W n\n");
    let list = matches!(widget.control, Control::Choice { combo: false, .. });
    let top = selected.first().copied().unwrap_or(0);
    for (i, line) in lines.iter().enumerate() {
        let y = height - 2.0 - size - i as f64 * size * 1.2;
        if list && selected.contains(&(top + i)) {
            body.push_str(&format!(
                "0.8 0.87 1 rg 0 {} {width} {} re f\n",
                y - size * 0.2,
                size * 1.2
            ));
        }
        let hex: String = line.chars().map(|ch| format!("{:02x}", ch as u8)).collect();
        body.push_str(&format!(
            "0 0 0 rg BT /F0 {size} Tf 1 0 0 1 2 {y} Tm <{hex}> Tj ET\n"
        ));
    }
    body.push('Q');
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
    Ok(())
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
        let control = widgets[0].control.clone();
        let mut on_name = None;
        for widget in widgets {
            let width = widget.rect[2] - widget.rect[0];
            let height = widget.rect[3] - widget.rect[1];
            match &change.value {
                Value::Text(text) => {
                    let (size, lines) = text_layout(widget, text)?;
                    write_text_appearance(doc, widget, size, &lines, &[])?;
                }
                Value::Selection(indices) => match &widget.control {
                    Control::Choice { .. } => {
                        let (size, lines) = choice_layout(widget, indices)?;
                        write_text_appearance(doc, widget, size, &lines, indices)?;
                    }
                    Control::Radio {
                        index,
                        states,
                        unison,
                        ..
                    } => {
                        let checked = indices.first().is_some_and(|selected| {
                            selected == index || (*unison && states[*selected] == states[*index])
                        });
                        // scan validated both streams. Retain the authored artwork.
                        doc.get_dictionary_mut(widget.widget)
                            .map_err(|e| e.to_string())?
                            .set(
                                "AS",
                                Object::Name(if checked {
                                    states[*index].clone()
                                } else {
                                    b"Off".to_vec()
                                }),
                            );
                    }
                    _ => unreachable!(),
                },
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
            Object::Name(match control {
                Control::Text => b"Tx".to_vec(),
                Control::Choice { .. } => b"Ch".to_vec(),
                _ => b"Btn".to_vec(),
            }),
        );
        field.set("Ff", flags);
        field.set(
            "V",
            match &change.value {
                Value::Text(text) => pdf_string(text),
                Value::Selection(indices) => match &control {
                    Control::Radio { states, .. } => Object::Name(
                        indices
                            .first()
                            .map(|i| states[*i].clone())
                            .unwrap_or_else(|| b"Off".to_vec()),
                    ),
                    Control::Choice { options, .. } => {
                        let values: Vec<_> = indices
                            .iter()
                            .map(|i| pdf_string(&options[*i].export))
                            .collect();
                        if values.len() == 1 {
                            values[0].clone()
                        } else {
                            Object::Array(values)
                        }
                    }
                    _ => unreachable!(),
                },
                Value::Checked(checked) => Object::Name(if *checked {
                    on_name.unwrap_or_else(|| b"Yes".to_vec())
                } else {
                    b"Off".to_vec()
                }),
            },
        );
        if let Control::Choice { combo, .. } = control {
            if let Value::Selection(indices) = &change.value {
                field.set(
                    "I",
                    indices
                        .iter()
                        .map(|i| Object::Integer(*i as i64))
                        .collect::<Vec<_>>(),
                );
                if !combo {
                    field.set("TI", indices.first().copied().unwrap_or(0) as i64);
                }
                if indices.is_empty() {
                    field.remove(b"V");
                    field.remove(b"I");
                }
            } else {
                field.remove(b"I");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Mixed controls with export/label pairs and duplicate exports, on two pages.
    pub(crate) fn mixed_fixture() -> (Document, ObjectId, ObjectId, ObjectId) {
        let (mut doc, _, _) = fixture();
        let pages = crate::pagetree::ordered_pages(&doc);
        for page in &pages {
            doc.get_dictionary_mut(*page)
                .unwrap()
                .set("MediaBox", vec![0.into(), 0.into(), 400.into(), 500.into()]);
        }
        let radio = doc.add_object(dictionary! { "FT" => "Btn", "Ff" => 1 << 15, "T" => Object::string_literal("delivery"), "V" => "First" });
        let mut buttons = Vec::new();
        for (i, state) in ["First", "Second"].iter().enumerate() {
            let off = appearance(
                &mut doc,
                20.0,
                20.0,
                b"q 0 0 0 RG 1 w 1 1 18 18 re S Q".to_vec(),
                Dictionary::new(),
            );
            let on = appearance(
                &mut doc,
                20.0,
                20.0,
                b"q 0 0 0 RG 1 w 1 1 18 18 re S 0 0 0 rg 5 5 10 10 re f Q".to_vec(),
                Dictionary::new(),
            );
            let mut states = dictionary! { "Off" => off };
            states.set(*state, on);
            let widget = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "Parent" => radio, "F" => 4, "Rect" => vec![20.into(), (150 + i as i64 * 30).into(), 40.into(), (170 + i as i64 * 30).into()], "AP" => dictionary! { "N" => states }, "AS" => if i == 0 { "First" } else { "Off" } });
            buttons.push(widget);
            doc.get_dictionary_mut(pages[i])
                .unwrap()
                .get_mut(b"Annots")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(widget.into());
        }
        doc.get_dictionary_mut(radio).unwrap().set(
            "Kids",
            buttons
                .iter()
                .copied()
                .map(Object::Reference)
                .collect::<Vec<_>>(),
        );
        let options = vec![
            Object::Array(vec![pdf_string("SAME"), pdf_string("First label")]),
            Object::Array(vec![pdf_string("SAME"), pdf_string("Second label")]),
            Object::Array(vec![pdf_string("OTHER"), pdf_string("Third label")]),
        ];
        let combo = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "FT" => "Ch", "T" => Object::string_literal("delivery_choice"), "Ff" => 1 << 17, "F" => 4, "Rect" => vec![60.into(), 150.into(), 220.into(), 180.into()], "Opt" => options.clone(), "V" => pdf_string("SAME"), "I" => vec![0.into()] });
        let list = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "FT" => "Ch", "T" => Object::string_literal("items"), "Ff" => 1 << 21, "F" => 4, "Rect" => vec![60.into(), 200.into(), 220.into(), 280.into()], "Opt" => options, "V" => pdf_string("SAME"), "I" => vec![0.into()] });
        let options = vec![pdf_string("Alpha"), pdf_string("Beta"), pdf_string("Gamma")];
        let single = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "FT" => "Ch", "T" => Object::string_literal("single_item"), "F" => 4, "Rect" => vec![60.into(), 220.into(), 220.into(), 290.into()], "Opt" => options.clone(), "V" => pdf_string("Alpha") });
        let custom = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Widget", "FT" => "Ch", "T" => Object::string_literal("custom_choice"), "Ff" => (1 << 17) | (1 << 18), "F" => 4, "Rect" => vec![60.into(), 320.into(), 220.into(), 350.into()], "Opt" => options, "V" => pdf_string("Alpha") });
        doc.get_dictionary_mut(pages[1])
            .unwrap()
            .get_mut(b"Annots")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .extend([single.into(), custom.into()]);
        doc.get_dictionary_mut(pages[0])
            .unwrap()
            .get_mut(b"Annots")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .extend([combo.into(), list.into()]);
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        doc.get_dictionary_mut(root)
            .unwrap()
            .get_mut(b"AcroForm")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"Fields")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .extend([
                radio.into(),
                combo.into(),
                list.into(),
                single.into(),
                custom.into(),
            ]);
        if let Ok(path) = std::env::var("TPDF_CHOICE_FIXTURE") {
            doc.save(path).unwrap();
        }
        (doc, radio, combo, list)
    }

    #[test]
    fn choices_and_radio_round_trip_exports_indices_and_appearances() {
        let (mut doc, radio, combo, list) = mixed_fixture();
        let before = scan(&doc).unwrap();
        assert_eq!(before.widgets.len(), 9);
        for widget in before.widgets.iter().filter(|w| w.object == radio) {
            assert_eq!(widget.value, Value::Selection(vec![0]));
        }
        let changes = [
            Change {
                object: radio,
                value: Value::Selection(vec![1]),
            },
            Change {
                object: combo,
                value: Value::Selection(vec![1]),
            },
            Change {
                object: list,
                value: Value::Selection(vec![0, 2]),
            },
        ];
        write(&mut doc, &changes).unwrap();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let saved = Document::load_mem(&bytes).unwrap();
        let form = scan(&saved).unwrap();
        for change in &changes {
            let widgets: Vec<_> = form
                .widgets
                .iter()
                .filter(|w| w.object == change.object)
                .collect();
            assert!(!widgets.is_empty());
            for widget in widgets {
                assert_eq!(widget.value, change.value);
            }
        }
        assert_eq!(
            saved
                .get_dictionary(radio)
                .unwrap()
                .get(b"V")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Second"
        );
        for w in form.widgets.iter().filter(|w| w.object == radio) {
            let Control::Radio { index, .. } = w.control else {
                panic!("radio kind lost")
            };
            assert_eq!(
                saved
                    .get_dictionary(w.widget)
                    .unwrap()
                    .get(b"AS")
                    .unwrap()
                    .as_name()
                    .unwrap(),
                if index == 1 {
                    b"Second".as_slice()
                } else {
                    b"Off"
                }
            );
        }
        assert_eq!(
            option_text(
                &saved,
                saved.get_dictionary(combo).unwrap().get(b"V").unwrap()
            )
            .unwrap(),
            "SAME"
        );
        assert_eq!(
            saved
                .get_dictionary(combo)
                .unwrap()
                .get(b"FT")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Ch"
        );
        let ap = saved
            .get_dictionary(combo)
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
        assert!(
            String::from_utf8_lossy(&stream.content).contains("5365636f6e64206c6162656c"),
            "appearance must draw the label, not SAME"
        );
        assert_eq!(
            saved
                .get_dictionary(list)
                .unwrap()
                .get(b"I")
                .unwrap()
                .as_array()
                .unwrap(),
            &vec![0.into(), 2.into()]
        );
        if let Ok(path) = std::env::var("TPDF_CHOICE_PROBE") {
            std::fs::write(path, bytes).unwrap();
        }
        // A second artifact isolates foreign-reader handling of duplicate exports.
        // Do not change the actual document's exports to work around a reader.
        if let Ok(path) = std::env::var("TPDF_CHOICE_UNIQUE_PROBE") {
            let options = vec![
                Object::Array(vec![pdf_string("FIRST"), pdf_string("First label")]),
                Object::Array(vec![pdf_string("SECOND"), pdf_string("Second label")]),
                Object::Array(vec![pdf_string("OTHER"), pdf_string("Third label")]),
            ];
            for id in [combo, list] {
                let field = doc.get_dictionary_mut(id).unwrap();
                field.set("Opt", options.clone());
                field.remove(b"V");
                field.remove(b"I");
            }
            write(&mut doc, &changes).unwrap();
            doc.save(path).unwrap();
        }
    }

    #[test]
    fn choice_values_override_stale_indices_and_support_custom_combo_text() {
        let (mut doc, _, combo, list) = mixed_fixture();
        doc.get_dictionary_mut(combo)
            .unwrap()
            .set("V", pdf_string("OTHER"));
        assert_eq!(
            scan(&doc)
                .unwrap()
                .widgets
                .iter()
                .find(|w| w.object == combo)
                .unwrap()
                .value,
            Value::Selection(vec![2])
        );
        doc.get_dictionary_mut(combo)
            .unwrap()
            .set("Ff", (1 << 17) | (1 << 18));
        write(
            &mut doc,
            &[Change {
                object: combo,
                value: Value::Text("Custom answer".into()),
            }],
        )
        .unwrap();
        assert!(!doc.get_dictionary(combo).unwrap().has(b"I"));
        assert_eq!(
            scan(&doc)
                .unwrap()
                .widgets
                .iter()
                .find(|w| w.object == combo)
                .unwrap()
                .value,
            Value::Text("Custom answer".into())
        );
        write(
            &mut doc,
            &[Change {
                object: list,
                value: Value::Selection(vec![]),
            }],
        )
        .unwrap();
        assert_eq!(
            scan(&doc)
                .unwrap()
                .widgets
                .iter()
                .find(|w| w.object == list)
                .unwrap()
                .value,
            Value::Selection(vec![])
        );
    }

    #[test]
    fn ambiguous_choices_and_unbounded_options_are_refused() {
        let (mut doc, _, combo, _) = mixed_fixture();
        doc.get_dictionary_mut(combo).unwrap().remove(b"I");
        assert!(scan(&doc).unwrap_err().contains("selection indices"));
        doc.get_dictionary_mut(combo)
            .unwrap()
            .set("Opt", vec![pdf_string("option"); 4097]);
        assert!(scan(&doc).unwrap_err().contains("option limit"));
    }

    #[test]
    fn radio_answers_survive_page_moves_and_support_duplicate_states() {
        let (mut doc, radio, _, _) = mixed_fixture();
        let pages = doc
            .catalog()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_dictionary_mut(pages)
            .unwrap()
            .get_mut(b"Kids")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .reverse();
        write(
            &mut doc,
            &[Change {
                object: radio,
                value: Value::Selection(vec![1]),
            }],
        )
        .unwrap();
        assert_eq!(
            doc.get_dictionary(radio)
                .unwrap()
                .get(b"V")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Second"
        );
        let widgets: Vec<_> = scan(&doc)
            .unwrap()
            .widgets
            .into_iter()
            .filter(|w| w.object == radio)
            .collect();
        let second = widgets
            .iter()
            .find(|w| matches!(w.control, Control::Radio { index: 1, .. }))
            .unwrap()
            .widget;
        let normal = doc
            .get_dictionary_mut(second)
            .unwrap()
            .get_mut(b"AP")
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"N")
            .unwrap()
            .as_dict_mut()
            .unwrap();
        let ap = normal.remove(b"Second").unwrap();
        normal.set("First", ap);
        for unison in [false, true] {
            doc.get_dictionary_mut(radio)
                .unwrap()
                .set("Ff", (1 << 15) | if unison { 1 << 25 } else { 0 });
            write(
                &mut doc,
                &[Change {
                    object: radio,
                    value: Value::Selection(vec![1]),
                }],
            )
            .unwrap();
            let on = widgets
                .iter()
                .filter(|w| {
                    doc.get_dictionary(w.widget)
                        .unwrap()
                        .get(b"AS")
                        .unwrap()
                        .as_name()
                        .unwrap()
                        == b"First"
                })
                .count();
            assert_eq!(on, if unison { 2 } else { 1 });
        }
    }

    #[test]
    fn choice_validation_refuses_bad_indices_wrong_kinds_and_unrenderable_labels() {
        let (mut doc, radio, combo, list) = mixed_fixture();
        for (object, value) in [
            (radio, Value::Selection(vec![0, 1])),
            (combo, Value::Selection(vec![3])),
            (combo, Value::Selection(vec![0, 1])),
            (list, Value::Selection(vec![2, 0])),
            (list, Value::Selection(vec![1, 1])),
            (list, Value::Text("made up".into())),
        ] {
            assert!(write(&mut doc, &[Change { object, value }]).is_err());
        }
        doc.get_dictionary_mut(combo)
            .unwrap()
            .set("Opt", vec![pdf_string("漢")]);
        doc.get_dictionary_mut(combo).unwrap().remove(b"V");
        assert!(write(
            &mut doc,
            &[Change {
                object: combo,
                value: Value::Selection(vec![0])
            }]
        )
        .unwrap_err()
        .contains("characters"));
        doc.get_dictionary_mut(radio)
            .unwrap()
            .set("Ff", (1 << 15) | (1 << 14));
        assert!(write(
            &mut doc,
            &[Change {
                object: radio,
                value: Value::Selection(vec![])
            }]
        )
        .unwrap_err()
        .contains("radio"));
    }

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
