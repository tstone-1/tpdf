//! Opening a second file so its pages can be placed in the working document.
//!
//! **The file is opened the way the reader's own document is**, through the
//! render service, so it is parsed only in a sandboxed worker of its own and the
//! app process maps no parser for it --- `docs/THREAT-MODEL.md` §T6.20. What the
//! open yields is a render handle, and that handle is the whole of how an
//! imported page is drawn: every tile, text and search request for one names it
//! with the page of *that* file.
//!
//! What lives here is the part of `page_import_prepare` that can be wrong without a
//! render service to be wrong about: the words a reader is shown when the file
//! cannot be used, and the rule that a handle opened for an import is released
//! on every path out except the one that hands it to the model. The command
//! itself is in `commands/document.rs`, because it waits on the service.

use crate::progressive;

/// A render handle that is released when this is dropped, unless it is kept.
///
/// **The shape of the command is an open followed by four things that can each
/// refuse** --- the encryption check, the fingerprint, the page count and the
/// model --- and every one of those refusals leaves a sandboxed worker pool that
/// nothing names. Writing a release at each `return` is four places for the
/// fifth refusal to forget; a guard makes forgetting it impossible to write.
///
/// `release` is a closure rather than the render service so that the rule is
/// testable without one: the tests hand it a list to push onto.
pub struct Held<F: FnOnce(u32)> {
    id: u32,
    release: Option<F>,
}

impl<F: FnOnce(u32)> Held<F> {
    /// Holds `id`, to be released by `release` unless [`keep`](Self::keep) is called.
    pub fn new(id: u32, release: F) -> Self {
        Self {
            id,
            release: Some(release),
        }
    }

    /// The handle, for the requests made while it is held.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Hands the handle on: nothing is released, and the caller now owns it.
    ///
    /// The one path out that does not release, and it is taken only once the
    /// document's model holds the handle --- waiting for its pages
    /// (`edits::Edits::prepare_import`) or drawing them (`edits::Edits::import`).
    #[must_use]
    pub fn keep(mut self) -> u32 {
        self.release = None;
        self.id
    }
}

impl<F: FnOnce(u32)> Drop for Held<F> {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release(self.id);
        }
    }
}

/// What a reader is told when the file they chose would not open.
///
/// **A locked file is named as locked rather than passed on as the render
/// service's sentence.** That sentence was written for the document a reader is
/// opening, where the answer is a password prompt; here there is none, because
/// the save that would write these pages refuses an encrypted file anyway
/// (`docs/THREAT-MODEL.md` §T6.19), and a prompt that led to a refusal at save
/// would cost the reader everything they did in between.
///
/// Every other failure keeps the service's own reason, which names the cause
/// --- not a PDF, damaged, unreadable --- in words chosen in `progressive.rs`
/// and never taken from the file.
pub fn open_refused(refusal: &progressive::Refusal, name: &str) -> String {
    if refusal.locked {
        return locked(name);
    }
    format!("Could not insert pages from {name}: {}", refusal.reason)
}

/// What a reader is told when the file they chose is encrypted.
///
/// One sentence for both routes to it --- a file that asked for a password and
/// one that opened with the empty one --- because the reason is the same: a save
/// refuses to import from an encrypted file. Asked at insert rather than left to
/// the save, which would refuse after the reader had arranged the pages.
pub fn locked(name: &str) -> String {
    format!(
        "{name} is encrypted, and tpdf cannot insert pages from an encrypted file yet. \
         Save an unencrypted copy of it first."
    )
}

/// The file's name, for a message, without the directories above it.
///
/// A reader chose the file a moment ago and knows where it is; the directories
/// only make the sentence longer. The whole path is the fallback for a name
/// with no final component.
pub fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A file's page count, as the model counts pages.
///
/// # Errors
///
/// A count past what a page number can hold, which the model cannot represent.
pub fn page_count(count: usize) -> Result<u32, String> {
    u32::try_from(count)
        .map_err(|_| format!("a file of {count} pages is past what tpdf can insert"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A guard dropped without being kept releases its handle, once.
    ///
    /// The property every refusal in `page_import_prepare` leans on: each `?` after the
    /// open drops the guard, and the drop is the release.
    #[test]
    fn a_handle_nobody_kept_is_released_when_the_guard_goes() {
        let released = RefCell::new(Vec::new());
        {
            let held = Held::new(5, |id| released.borrow_mut().push(id));
            assert_eq!(held.id(), 5);
        }
        assert_eq!(*released.borrow(), vec![5]);
    }

    /// A kept handle is not released, and the id comes back.
    #[test]
    fn a_kept_handle_is_the_caller_s_and_is_not_released() {
        let released = RefCell::new(Vec::new());
        let held = Held::new(9, |id| released.borrow_mut().push(id));
        assert_eq!(held.keep(), 9);
        assert!(released.borrow().is_empty(), "keeping is not releasing");
    }

    /// A refusal on the way out of a function releases the handle.
    ///
    /// The shape the command has: the guard is taken, a later step refuses with
    /// `?`, and nothing but the drop is left to release it.
    #[test]
    fn a_refusal_after_the_open_releases_the_handle() {
        let released = RefCell::new(Vec::new());
        let run = || -> Result<u32, String> {
            let held = Held::new(3, |id| released.borrow_mut().push(id));
            let _pages = page_count(0)?;
            Err::<(), _>("the model refused".to_string())?;
            Ok(held.keep())
        };
        assert_eq!(run(), Err("the model refused".into()));
        assert_eq!(*released.borrow(), vec![3]);
    }

    /// A locked file is refused in words about encryption, not with a prompt.
    #[test]
    fn a_locked_file_is_refused_as_encrypted() {
        let refusal = progressive::Refusal {
            reason: "This document needs a password.".into(),
            locked: true,
        };
        let said = open_refused(&refusal, "other.pdf");
        assert!(said.contains("other.pdf"), "{said}");
        assert!(said.contains("encrypted"), "{said}");
        assert!(
            !said.contains("needs a password"),
            "the open prompt's sentence is for the reader's own document: {said}"
        );
    }

    /// Any other failure keeps the service's reason, which names the cause.
    #[test]
    fn any_other_failure_keeps_the_reason_the_service_gave() {
        let refusal = progressive::Refusal {
            reason: "The file is not a PDF.".into(),
            locked: false,
        };
        let said = open_refused(&refusal, "notes.txt");
        assert!(said.contains("notes.txt"), "{said}");
        assert!(said.contains("The file is not a PDF."), "{said}");
        assert!(!said.contains("encrypted"), "{said}");
    }

    #[test]
    fn a_page_count_is_the_model_s_number_or_a_refusal() {
        assert_eq!(page_count(3), Ok(3));
        assert_eq!(page_count(0), Ok(0));
        if let Ok(past) = usize::try_from(u64::from(u32::MAX) + 1) {
            assert!(page_count(past).is_err(), "past a page number is refused");
        }
    }

    #[test]
    fn the_name_is_the_last_component() {
        assert_eq!(
            display_name(std::path::Path::new("/a/b/report.pdf")),
            "report.pdf"
        );
        assert_eq!(display_name(std::path::Path::new("/")), "/");
    }
}
