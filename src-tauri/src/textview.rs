//! Unsaved text and unsaved field changes rendered from a worker-owned
//! revision, with the source kept intact.

use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::document::OpenDocument;
use crate::formedit::FieldEdit;
use crate::textedit::Change;

const MAX_PREVIEW_BYTES: usize = 64 * 1024 * 1024;

/// What a reader has pending that a page shows before it is saved.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct View {
    /// The replaced text.
    pub(crate) changes: Vec<Change>,
    /// The changes to the fields the file has: moved, resized, removed, or
    /// given other properties.
    pub(crate) fields: Vec<FieldEdit>,
}

impl View {
    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.fields.is_empty()
    }
}

/// A cheap shared link to the journal, installed before the application opens files.
#[derive(Clone, Default)]
pub(crate) struct Source(Arc<Mutex<Option<crate::edits::Edits>>>);

impl Source {
    pub(crate) fn follow(&self, edits: &crate::edits::Edits) {
        *self.0.lock().expect("text source lock") = Some(edits.clone());
    }

    /// The replacements addressed in the document open under this **render
    /// handle**, which may be a file the reader inserted pages from rather
    /// than one they opened. See `Edits::render_changes`.
    pub(crate) fn view(&self, doc: u32) -> View {
        let source = self.0.lock().expect("text source lock").clone();
        source
            .map(|edits| View {
                changes: edits.render_changes(doc),
                fields: edits.render_fields(doc),
            })
            .unwrap_or_default()
    }

    pub(crate) fn request(
        &self,
        doc: u32,
        request: crate::worker::Request,
    ) -> crate::worker::Request {
        if !request.supports_text_view() {
            return request;
        }
        let view = self.view(doc);
        if view.is_empty() {
            request
        } else {
            crate::worker::Request::TextView {
                changes: view.changes,
                fields: view.fields,
                request: Box::new(request),
            }
        }
    }
}

pub(crate) struct Preview {
    pub(crate) view: View,
    /// `None` when the view holds field changes only and could not be drawn:
    /// the pages are then read from the source, with each field where the
    /// file has it. Kept, so that the rewrite is not tried again per tile.
    pub(crate) document: Option<Box<OpenDocument>>,
}

impl Preview {
    /// # Errors
    ///
    /// A view with replaced text that cannot be written or opened. A view of
    /// field changes alone has no error: a page is still worth drawing when a
    /// moved field cannot be shown at its new place.
    pub(crate) fn build(source: &OpenDocument, view: &View) -> Result<Self, String> {
        let document = match revision(source.graph().parsed()?, view)? {
            None => None,
            Some(bytes) => {
                match OpenDocument::open_owned(source.pdfium().bindings(), bytes.into()) {
                    Ok(document) => Some(Box::new(document)),
                    Err(_) if view.changes.is_empty() => None,
                    Err(refusal) => return Err(refusal.reason),
                }
            }
        };
        Ok(Self {
            view: view.clone(),
            document,
        })
    }
}

/// The bytes a view is drawn from, or `None` for a view of field changes alone
/// that cannot be written: its pages are drawn from the source.
fn revision(source: &lopdf::Document, view: &View) -> Result<Option<Vec<u8>>, String> {
    match rewrite(source, view) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) if view.changes.is_empty() => Ok(None),
        Err(e) => Err(e),
    }
}

// Preview bytes never leave the parser worker. Encryption is still preserved by
// the independent save path; this copy only feeds PDFium in this same process.
//
// The field changes are the ones the journal holds, and the journal admits a
// change the writer then refuses: a text field resized whose appearance cannot
// be redrawn is refused at the save. Such a batch is left out whole, so the
// replaced text is still shown and the fields stay where the file has them.
fn rewrite(source: &lopdf::Document, view: &View) -> Result<Vec<u8>, String> {
    let mut document = source.clone();
    crate::textedit::write(&mut document, &view.changes)?;
    if !view.fields.is_empty() {
        let mut fielded = document.clone();
        match crate::formedit::apply(&mut fielded, &view.fields) {
            Ok(()) => document = fielded,
            Err(e) if view.changes.is_empty() => return Err(e),
            Err(_) => {}
        }
    }
    document.trailer.remove(b"Encrypt");
    document.encryption_state = None;
    let mut output = Limited(Vec::new());
    document.save_to(&mut output).map_err(|e| e.to_string())?;
    Ok(output.0)
}

struct Limited(Vec<u8>);

impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_PREVIEW_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("text preview exceeds its byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textview_rewrites_without_changing_the_original_and_bounds_output() {
        let source = crate::textedit::tests::fixture();
        let change = crate::textedit::tests::change(&source);
        let view = View {
            changes: vec![change],
            fields: Vec::new(),
        };
        let bytes = rewrite(&source, &view).unwrap();
        let rendered = crate::encoding::load(&bytes, None).unwrap();
        assert_eq!(
            crate::textedit::scan(&rendered, 0).unwrap().runs[0].text,
            "EDITED FIRST"
        );
        assert_eq!(
            crate::textedit::scan(&source, 0).unwrap().runs[0].text,
            "SYNTHETIC FIRST"
        );
        let mut output = Limited(vec![0; MAX_PREVIEW_BYTES - 1]);
        output.write_all(b"a").unwrap();
        assert!(output.write_all(b"b").is_err());
        assert_eq!(output.0.len(), MAX_PREVIEW_BYTES);
    }

    /// One page of 300 by 200 points with a text field `Name` and a checkbox
    /// `Agree` on it.
    fn form() -> lopdf::Document {
        use lopdf::{dictionary, Dictionary, Object};
        let mut doc = lopdf::Document::with_version("1.7");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
            "Resources" => Dictionary::new(),
        });
        doc.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Count" => 1, "Kids" => vec![Object::Reference(page)] }
                .into(),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let field = |name: &str, kind, rect| crate::formfields::NewField {
            text_size: None,
            default_value: None,
            options: Vec::new(),
            name: name.into(),
            kind,
            page: 0,
            rect,
            tooltip: None,
            required: false,
            max_length: None,
            border: true,
        };
        crate::formfields::add(
            &mut doc,
            &[
                field(
                    "Name",
                    crate::formfields::Kind::Text,
                    [20.0, 20.0, 100.0, 20.0],
                ),
                field(
                    "Agree",
                    crate::formfields::Kind::Checkbox,
                    [20.0, 60.0, 12.0, 12.0],
                ),
            ],
        )
        .expect("added");
        doc
    }

    fn rects(doc: &lopdf::Document) -> Vec<(String, [f32; 4])> {
        crate::forms::scan(doc)
            .expect("a form")
            .widgets
            .into_iter()
            .map(|widget| (widget.name, widget.display_rect))
            .collect()
    }

    fn edit(widget: (u32, u16)) -> FieldEdit {
        FieldEdit {
            value: None,
            widget,
            rect: None,
            name: None,
            remove: false,
            props: crate::formedit::Props::default(),
        }
    }

    #[test]
    fn a_view_draws_a_moved_field_at_its_new_place_and_a_removed_one_nowhere() {
        let source = form();
        let before = rects(&source);
        let widgets = crate::forms::scan(&source).unwrap().widgets;
        let to = [150.0, 100.0, 250.0, 120.0];
        assert_ne!(before[0].1, to, "the field starts somewhere else");
        let view = View {
            changes: Vec::new(),
            fields: vec![
                FieldEdit {
                    rect: Some(to),
                    ..edit(widgets[0].widget)
                },
                FieldEdit {
                    remove: true,
                    ..edit(widgets[1].widget)
                },
            ],
        };
        let bytes = revision(&source, &view).unwrap().expect("drawn");
        let drawn = crate::encoding::load(&bytes, None).unwrap();
        assert_eq!(rects(&drawn), vec![("Name".to_string(), to)]);
        assert_eq!(rects(&source), before, "the source is as it was");
    }

    #[test]
    fn field_changes_that_cannot_be_written_leave_the_page_drawn_without_them() {
        // No widget is object 999, which is what the writer refuses a batch for.
        let missing = vec![edit((999, 0))];
        assert!(crate::formedit::apply(&mut form(), &missing).is_err());
        // Alone: no revision, and no error either. The source is drawn.
        let alone = View {
            changes: Vec::new(),
            fields: missing.clone(),
        };
        assert_eq!(revision(&form(), &alone), Ok(None));
        // Beside replaced text: the text is drawn and the fields are left.
        let source = crate::textedit::tests::fixture();
        let both = View {
            changes: vec![crate::textedit::tests::change(&source)],
            fields: missing,
        };
        let bytes = revision(&source, &both).unwrap().expect("the text");
        let drawn = crate::encoding::load(&bytes, None).unwrap();
        assert_eq!(
            crate::textedit::scan(&drawn, 0).unwrap().runs[0].text,
            "EDITED FIRST"
        );
        // And text that cannot be written is still an error.
        let mut stale = crate::textedit::tests::change(&source);
        stale.revision = vec![0; 32];
        let refused = View {
            changes: vec![stale],
            fields: Vec::new(),
        };
        assert!(revision(&source, &refused).is_err());
    }
}
