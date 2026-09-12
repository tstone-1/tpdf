//! Unsaved text rendered from a worker-owned revision, with the source kept intact.

use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::document::OpenDocument;
use crate::textedit::Change;

const MAX_PREVIEW_BYTES: usize = 64 * 1024 * 1024;

/// A cheap shared link to the journal, installed before the application opens files.
#[derive(Clone, Default)]
pub(crate) struct Source(Arc<Mutex<Option<crate::edits::Edits>>>);

impl Source {
    pub(crate) fn follow(&self, edits: &crate::edits::Edits) {
        *self.0.lock().expect("text source lock") = Some(edits.clone());
    }

    pub(crate) fn changes(&self, doc: u32) -> Vec<Change> {
        let source = self.0.lock().expect("text source lock").clone();
        source
            .map(|edits| edits.text_changes(doc))
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
        let changes = self.changes(doc);
        if changes.is_empty() {
            request
        } else {
            crate::worker::Request::TextView {
                changes,
                request: Box::new(request),
            }
        }
    }
}

pub(crate) struct Preview {
    pub(crate) changes: Vec<Change>,
    pub(crate) document: Box<OpenDocument>,
}

impl Preview {
    pub(crate) fn build(source: &OpenDocument, changes: &[Change]) -> Result<Self, String> {
        let bytes = rewrite(source.graph().parsed()?, changes)?;
        let document = OpenDocument::open_owned(source.pdfium().bindings(), bytes.into())
            .map_err(|refusal| refusal.reason)?;
        Ok(Self {
            changes: changes.to_vec(),
            document: Box::new(document),
        })
    }
}

// Preview bytes never leave the parser worker. Encryption is still preserved by
// the independent save path; this copy only feeds PDFium in this same process.
fn rewrite(source: &lopdf::Document, changes: &[Change]) -> Result<Vec<u8>, String> {
    let mut document = source.clone();
    crate::textedit::write(&mut document, changes)?;
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
        let bytes = rewrite(&source, &[change]).unwrap();
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
}
