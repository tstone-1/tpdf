//! Why something did not happen, and what the reader can do about it.
//!
//! `docs/TRAPS.md` records *a refusal flattened to a string across a process
//! boundary loses the action that answers it* three times over, each one a
//! boundary further out: a correct sentence arrives, the save is correctly
//! refused, and the reader is told a thing they cannot act on. Nothing goes red,
//! because the message is right.
//!
//! [`Failure`] is the shape that does not lose it. A message a human reads and a
//! fact a program acts on are different things, and packing the second into the
//! first is how a window comes to match on wording.
//!
//! ## The wire is two booleans, and the type is not
//!
//! `save_document` and `redact_copy` have answered `{message, reopen, changed}`
//! since the save path had refusals to report, and `ipc.ts` mirrors those three
//! fields. So [`Failure`] serialises to exactly that, by hand, from an [`Action`]
//! that has more to say than two booleans can carry --- rather than carrying the
//! booleans *and* an action, which would be two copies of one distinction and is
//! the shape this repository has already watched drift.
//!
//! The consequence worth stating: several actions serialise identically today.
//! That is a limit of the wire and not of the model, and it is where a richer
//! answer goes when a window is ready to offer one.
//!
//! ## It knows about no model
//!
//! `From<docmodel::Refusal>` is in `edits.rs` and `refused_by` is in `save.rs`,
//! each beside the refusal it converts. That is not tidiness: this module has no
//! `use crate::` at all, so it depends on nothing and joins no cycle --- and the
//! first version of it did, through one call to `edits::describe`, which put the
//! type every command answers with inside the twenty-module knot it is supposed
//! to sit outside.

use serde::ser::{Serialize, SerializeStruct, Serializer};

/// What the reader can do about a failure.
///
/// Ordered roughly by how much it costs them, which is also the order the window
/// should prefer when two could apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    /// Nothing was touched. The reader carries on reading.
    #[default]
    Carry,
    /// Something the reader can change and try again --- a mark covering
    /// nothing, a page they cannot delete because it is the last one, a comment
    /// their own reply answers.
    ///
    /// Distinct from [`Carry`](Action::Carry) because it is the difference
    /// between *this did not happen* and *this did not happen and here is what
    /// to do*, which is the whole of the trap this module is named after.
    Amend,
    /// The wire and the model disagree. No reader caused it and none can fix it.
    ///
    /// Kept apart from [`Amend`](Action::Amend) so a window never invites
    /// somebody to correct a defect on the sending side.
    Report,
    /// The file changed on disk since it was opened, and reloading answers it.
    ///
    /// **The one action that costs the reader their edits**, which is why it is
    /// carried rather than re-derived: reloading is right for a document that
    /// was replaced underneath and wrong for every other refusal, and a window
    /// that offers it wrongly throws away work in exchange for nothing.
    Reload,
    /// The document is closed. The file has to be opened again.
    Reopen,
    /// Closed, and the file changed too.
    ReopenChanged,
}

impl Action {
    /// Whether the caller must open the document again.
    #[must_use]
    pub fn reopen(self) -> bool {
        matches!(self, Self::Reopen | Self::ReopenChanged)
    }

    /// Whether reloading the file would answer this.
    #[must_use]
    pub fn changed(self) -> bool {
        matches!(self, Self::Reload | Self::ReopenChanged)
    }
}

/// A message for a reader, and what they can do about it.
///
/// `Debug` so a test that expected an operation to land can print what it got
/// instead; nothing in the application formats one, because the window reads the
/// serialised form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The sentence a reader is shown.
    pub message: String,
    /// What answers it.
    pub action: Action,
}

impl Failure {
    /// Nothing was touched: no file written, no document closed.
    #[must_use]
    pub fn refused(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action: Action::Carry,
        }
    }

    /// The document is closed, whatever became of the file.
    #[must_use]
    pub fn after_close(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action: Action::Reopen,
        }
    }

    /// Whether the caller must open the document again.
    ///
    /// A question rather than a field, so [`Action`] is the only place the
    /// answer lives. The wire still carries it as `reopen`, which is what
    /// `ipc.ts` reads.
    #[must_use]
    pub fn reopen(&self) -> bool {
        self.action.reopen()
    }

    /// Whether reloading the file would answer this.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.action.changed()
    }
}

/// Serialised as `{message, reopen, changed}`, by hand and on purpose.
///
/// The two booleans are what `save_document` and `redact_copy` have always
/// answered and what `ipc.ts` mirrors, and they are *derived* here rather than
/// stored: one source of truth, so the type cannot come to disagree with the
/// wire about a refusal.
impl Serialize for Failure {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("Failure", 3)?;
        out.serialize_field("message", &self.message)?;
        out.serialize_field("reopen", &self.action.reopen())?;
        out.serialize_field("changed", &self.action.changed())?;
        out.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire is three fields with the names it has always had.
    ///
    /// A fixture rather than a round trip: `ipc.ts` declares these names by
    /// hand, and what this pins is the *bytes*, which is the only thing that
    /// side can be wrong about.
    #[test]
    fn a_failure_serialises_to_the_three_fields_the_window_reads() {
        let json = serde_json::to_string(&Failure::refused("it did not")).expect("serialises");
        assert_eq!(
            json, r#"{"message":"it did not","reopen":false,"changed":false}"#,
            "the shape `save_document` and `redact_copy` have always answered"
        );

        let closed = serde_json::to_string(&Failure::after_close("and it is gone")).expect("ok");
        assert_eq!(
            closed,
            r#"{"message":"and it is gone","reopen":true,"changed":false}"#
        );

        let both = serde_json::to_string(&Failure {
            message: "it moved underneath".into(),
            action: Action::ReopenChanged,
        })
        .expect("ok");
        assert_eq!(
            both,
            r#"{"message":"it moved underneath","reopen":true,"changed":true}"#
        );
    }

    /// Every action's two booleans, so no pair is reachable by accident.
    ///
    /// The table is the point: `Reload` and `Reopen` differ in *which* boolean,
    /// and a window that reads the wrong one either offers Reload for a refusal
    /// reloading cannot fix or withholds it from the one it can.
    #[test]
    fn each_action_answers_the_two_questions_the_wire_asks() {
        for (action, reopen, changed) in [
            (Action::Carry, false, false),
            (Action::Amend, false, false),
            (Action::Report, false, false),
            (Action::Reload, false, true),
            (Action::Reopen, true, false),
            (Action::ReopenChanged, true, true),
        ] {
            assert_eq!(action.reopen(), reopen, "{action:?} reopen");
            assert_eq!(action.changed(), changed, "{action:?} changed");
        }
    }
}
