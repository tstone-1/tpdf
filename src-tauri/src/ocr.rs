//! Reading text off pixels, and proving when there is none.
//!
//! `docs/PLAN.md` §9 said Phase 1 defines these interfaces and §8's enumeration
//! of Phase 1's remaining items did not list them, which is question 10 of §10.
//! This module is the answer to it. It defines the shape and the safety
//! properties; it deliberately implements **no engine**, because which engine
//! runs is a platform question and the part that has to be right is the part
//! above it.
//!
//! ## Two callers that look alike and are not
//!
//! OCR is wanted for two things, and conflating them is the trap this module is
//! built around.
//!
//! **Search and selection** want recall. A scanned page has no text objects, so
//! [`crate::text`] returns nothing for it and Ctrl-F finds nothing. Recognised
//! text fills that in. If the engine misses a word the feature is worse; nothing
//! is unsafe. An empty result is a poor answer.
//!
//! **Redaction verification** wants the opposite, and it is a safety gate.
//! `docs/PLAN.md` §6 step 4 renders the redacted regions and OCRs them "confirming
//! no legible text survives" --- and it is the *only* check that can say anything
//! about an image carrier, because step 3's byte scan cannot see into a
//! `/DCTDecode` stream and refusing every such stream would refuse every scanned
//! page in existence. Here an empty result is the *whole claim*, and an empty
//! result is also what a broken engine, a missing language pack, a crashed
//! worker, a blank region and the wrong page all produce.
//!
//! `AGENTS.md` states the rule this sits under: tpdf must never claim a redaction
//! is clean unless it can prove it, and if any check cannot complete the result is
//! "not verified", never "clean". So [`Legibility`] has three values and not two,
//! and the only route to [`Legibility::Illegible`] is through a control the engine
//! had to read on the same image --- see [`adjudicate`].
//!
//! ## Where this runs, which was measured rather than reasoned
//!
//! Not in the parser worker. Measured 2026-07-31 on macOS 26.5.2, running Vision's
//! `VNRecognizeTextRequest` against a control image under
//! [`crate::worker::SANDBOX_PROFILE`] applied post-launch exactly as
//! `worker_child.rs` applies it:
//!
//! | profile | result |
//! |---|---|
//! | the production profile (font directories only) | **killed by SIGTRAP** |
//! | `+ file-read-data` on all of `/System/Library` | ran, then failed with `nilError` |
//! | `+ file-read` allowed entirely | read the control string back |
//!
//! Vision needs general read authority, which is the single most valuable thing
//! that profile denies: a worker parsing a hostile document must not be able to
//! read the user's files. So OCR cannot be another [`crate::worker::Request`] on
//! the parser worker without giving up the containment the worker exists for.
//!
//! It does not have to be. The parser worker is contained because it parses
//! **attacker-authored structure** --- an object graph, filters, fonts, a
//! decompressor. An engine here consumes a fixed-size RGBA buffer that *we*
//! rendered: no format to parse, no lengths to trust, no recursion. That is a
//! categorically smaller surface, and it is the reason a laxer profile is a
//! considered trade rather than a concession.
//!
//! Two properties survive, and [`OCR_SANDBOX_PROFILE`] keeps them: no network, and
//! no writes. And it stays a *separate process* --- not for containment but because
//! the first rung above measured Vision hard-crashing its host. An engine that can
//! take the process down must not share one with unsaved annotations.
//!
//! ## Why the geometry is coarser than [`crate::text::PageText`]
//!
//! `PageText` carries one box per character index, because PDFium reports one and
//! the selection is a range of those indices. No OCR engine produces that: they
//! report words or lines. Splitting a word box into equal per-character slices
//! would put a number in the same field that means something weaker, and nothing
//! downstream could tell which kind it had. So [`RecognisedItem`] is explicitly a
//! span, and a caller that needs character precision on a scanned page does not
//! get to pretend it has it.
//!
//! The coordinate convention *is* shared: `left, top, right, bottom` in PDF points,
//! y increasing downwards, origin at the page's top-left. Two conventions in one
//! codebase is how `AGENTS.md`'s two-rotation-tables entry happened.

use std::fmt;

/// The SBPL profile an OCR worker applies to itself after `exec`.
///
/// Reads are allowed, because the rung ladder in the module docs measured that
/// Vision does not run without them. Network and writes stay denied, which is
/// what a recogniser has no business doing either way.
///
/// This is deliberately a *different constant* from
/// [`crate::worker::SANDBOX_PROFILE`] rather than a relaxation flag on it. One
/// constant standing for two policies is already a trap in `docs/TRAPS.md`, and
/// the whole point here is that the two boundaries are not the same boundary.
#[cfg(target_os = "macos")]
pub const OCR_SANDBOX_PROFILE: &str = "\
(version 1)
(allow default)
(deny network*)
(deny file-write*)
";

/// Which engine produced a result, so a stored recognition can be invalidated
/// when the engine behind it changes.
///
/// A platform engine is a black box that moves with the OS: a Windows Update or
/// a macOS point release can change what it reads without anything in this
/// repository changing. That is tolerable for search and it is exactly why the
/// verification gate below insists on a control every single time rather than
/// trusting a recorded capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineId {
    /// Short stable name, e.g. `"vision"` or `"windows-ocr"`.
    pub name: &'static str,
    /// Whatever the platform will tell us about its version. Free-form because
    /// no two platforms agree on what a version is.
    pub build: String,
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.build)
    }
}

/// A borrowed view of pixels to recognise.
///
/// Borrowed rather than owned because the pixels are already in the tile mapping
/// the renderer wrote to; copying 4 MB to hand it to a recogniser would cost more
/// than the whole boundary crossing that put it there (`BUILD.md`, `latency-bench`).
#[derive(Debug, Clone, Copy)]
pub struct Pixels<'a> {
    /// Row-major RGBA, 4 bytes per pixel, length `width * height * 4`.
    pub rgba: &'a [u8],
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Pixels per PDF point, so results can be reported in points without a
    /// second request carrying the scale.
    pub scale: f32,
}

impl Pixels<'_> {
    /// Whether the buffer is the length the dimensions claim.
    ///
    /// A recogniser handed a short buffer reads uninitialised or out-of-bounds
    /// memory, and the caller that built it is the only code that can be wrong
    /// about this --- so it is checked at the boundary rather than trusted.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        let want = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4));
        want == Some(self.rgba.len()) && self.width > 0 && self.height > 0 && self.scale > 0.0
    }
}

/// One span of recognised text --- a word or a line, whichever the engine reports.
#[derive(Debug, Clone, PartialEq)]
pub struct RecognisedItem {
    /// What the engine read.
    pub text: String,
    /// `left, top, right, bottom` in PDF points, y down, origin at the page's
    /// top-left --- the same convention as [`crate::text::PageText::boxes`].
    pub rect: [f32; 4],
    /// The engine's own confidence, where it reports one.
    ///
    /// `None` is not "unconfident": `Windows.Media.Ocr` returns no per-word
    /// confidence at all. Treating an absent value as a low one would silently
    /// make every Windows result filterable away, so the gate below treats it as
    /// the conservative case instead.
    pub confidence: Option<f32>,
}

/// Why a recognition could not be produced.
///
/// Every variant means *the check did not happen*. None of them may be folded
/// into an empty result, which is the entire reason this is not
/// `Result<Vec<RecognisedItem>, String>` with an empty vec on the sad path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecogniseError {
    /// No engine on this platform, or no data for the requested language.
    /// A real state on Windows, where OCR needs an installed language pack.
    Unavailable(String),
    /// The engine process died. Measured as a real mode: Vision aborts under a
    /// profile it dislikes rather than returning an error.
    Crashed(String),
    /// The engine exceeded its deadline and was killed.
    TimedOut(String),
    /// The image was outside what the engine accepts.
    Rejected(String),
    /// The caller handed over a buffer inconsistent with its dimensions.
    MalformedInput(String),
}

impl fmt::Display for RecogniseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, detail) = match self {
            Self::Unavailable(d) => ("no engine available", d),
            Self::Crashed(d) => ("the engine crashed", d),
            Self::TimedOut(d) => ("the engine timed out", d),
            Self::Rejected(d) => ("the engine rejected the image", d),
            Self::MalformedInput(d) => ("the image buffer is malformed", d),
        };
        write!(f, "{kind}: {detail}")
    }
}

/// What a caller asks of an engine.
#[derive(Debug, Clone)]
pub struct Options {
    /// BCP-47 tags, most preferred first. Empty means the engine's default.
    pub languages: Vec<String>,
    /// Whether the engine may use a language model to correct what it read.
    ///
    /// Off for verification, always. A corrector is a thing that turns marks it
    /// cannot read into plausible words, which is precisely the wrong bias when
    /// the question is whether anything is readable at all --- and it can also
    /// "repair" the control token into something else and fail the check for the
    /// wrong reason.
    pub language_correction: bool,
    /// Wall-clock budget. An engine with no deadline is an unbounded one, and
    /// this runs on a page the user is waiting for.
    pub deadline_ms: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            language_correction: false,
            deadline_ms: 10_000,
        }
    }
}

/// An OCR engine.
///
/// One method, because everything that makes this subsystem difficult lives
/// above it in [`adjudicate`] rather than inside an implementation. An engine is
/// allowed to be a thin platform binding and nothing more; it is *not* allowed to
/// decide that an empty page is a clean one.
pub trait Recogniser {
    /// Which engine this is.
    fn id(&self) -> EngineId;

    /// Read what text there is, or say why that could not be done.
    ///
    /// Returning `Ok(vec![])` is a positive claim that the engine ran and found
    /// nothing. An implementation that cannot distinguish that from failure must
    /// return an error.
    ///
    /// # Errors
    ///
    /// See [`RecogniseError`]; every variant means the recognition did not happen.
    fn recognise(
        &self,
        pixels: Pixels<'_>,
        options: &Options,
    ) -> Result<Vec<RecognisedItem>, RecogniseError>;
}

// ------------------------------------------------------------------ the gate

/// A token composited into the probe image, which the engine must read back
/// before an empty result may be believed.
///
/// The band carrying it is *appended* to the region under test rather than drawn
/// over it, so the pixels being judged are untouched.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    /// The string drawn into the band.
    pub token: String,
    /// The point size it was drawn at.
    pub size_pt: f32,
    /// Where the band sits in the probe image, same convention as
    /// [`RecognisedItem::rect`]. Items inside it are the control; items outside
    /// it are survivors.
    pub band: [f32; 4],
}

/// A control could not be constructed, which is itself a reason not to certify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlTooEasy(String);

impl fmt::Display for ControlTooEasy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Control {
    /// Builds a control no easier to read than the text the redaction removed.
    ///
    /// This is the whole idea, and it is the part that is easy to get wrong in a
    /// way that looks fine: a control set in 48 pt proves an engine can read 48 pt.
    /// It says nothing about the 6 pt footnote that was redacted, so a gate built
    /// on it would certify a page whose small print survived. The size therefore
    /// comes from the *shortest* box the redaction covered --- box height in points
    /// being the closest thing to a glyph size available without asking the font.
    ///
    /// # Errors
    ///
    /// If no usable box was supplied. Refusing is correct: with nothing to size
    /// against there is no honest control, and no control means no certification.
    pub fn no_easier_than(
        redacted_boxes: &[[f32; 4]],
        token: impl Into<String>,
        band: [f32; 4],
    ) -> Result<Self, ControlTooEasy> {
        let smallest = redacted_boxes
            .iter()
            .map(|b| b[3] - b[1])
            .filter(|h| h.is_finite() && *h > 0.0)
            .fold(f32::INFINITY, f32::min);
        if !smallest.is_finite() {
            return Err(ControlTooEasy(
                "no redacted box had a usable height, so there is nothing to size a control \
                 against; refusing rather than picking one"
                    .into(),
            ));
        }
        let token = token.into();
        if token.trim().is_empty() {
            return Err(ControlTooEasy(
                "an empty control token is read back by every engine and by none".into(),
            ));
        }
        Ok(Self {
            token,
            size_pt: smallest,
            band,
        })
    }

    /// Whether a rect lies within the control band.
    fn contains(&self, rect: &[f32; 4]) -> bool {
        let [l, t, r, b] = *rect;
        let [bl, bt, br, bb] = self.band;
        l >= bl && t >= bt && r <= br && b <= bb
    }
}

/// The verdict of the redaction gate.
///
/// Three-valued on purpose. Two-valued is the defect: with only clean and dirty,
/// every failure has to be reported as one of them, and the one it gets reported
/// as is always clean, because a failure produces no findings.
#[derive(Debug, Clone, PartialEq)]
pub enum Legibility {
    /// Nothing legible survived, **and** the engine was shown to be able to read
    /// on this image at this size. Only this variant may be presented as a clean
    /// redaction.
    Illegible {
        /// The engine that said so, recorded because it can change under the OS.
        engine: EngineId,
    },
    /// Something was read outside the control band.
    Legible {
        /// What survived, so the user can be shown it rather than told a number.
        found: Vec<RecognisedItem>,
    },
    /// The check did not complete. Never means clean.
    NotVerified {
        /// What went wrong, in terms a user can act on.
        why: String,
    },
}

impl Legibility {
    /// Whether this verdict permits calling a redaction clean.
    ///
    /// Exists so that no caller writes `!= Legible`, which is the same bug as a
    /// two-valued verdict wearing a three-valued type.
    #[must_use]
    pub fn certifies(&self) -> bool {
        matches!(self, Self::Illegible { .. })
    }
}

/// Turns a recogniser's output into a verdict.
///
/// Pure, so the safety-critical decision is testable without an engine --- which
/// matters, because the engines are platform black boxes and this is the part
/// that must not be wrong.
///
/// The rules, in order:
///
/// 1. The engine failed at all --- `NotVerified`. It did not look.
/// 2. The control token was not read back --- `NotVerified`. The engine ran and
///    proved it cannot read text of this size in this image, so its silence about
///    the rest of the image carries no information.
/// 3. Anything read outside the band --- `Legible`.
/// 4. Otherwise --- `Illegible`.
///
/// Rule 3 has no confidence threshold and that is deliberate. The two errors are
/// not symmetric: a false `Legible` costs a human another look, a false
/// `Illegible` publishes the secret. Confidence still travels on each item so the
/// user can see that a hit was marginal, but it never suppresses one.
#[must_use]
pub fn adjudicate(
    engine: &EngineId,
    control: &Control,
    outcome: &Result<Vec<RecognisedItem>, RecogniseError>,
) -> Legibility {
    let items = match outcome {
        Ok(items) => items,
        Err(e) => {
            return Legibility::NotVerified {
                why: format!("{e}"),
            }
        }
    };

    let (in_band, outside): (Vec<_>, Vec<_>) = items
        .iter()
        .cloned()
        .partition(|i| control.contains(&i.rect));

    let control_seen = in_band
        .iter()
        .any(|i| normalise(&i.text).contains(&normalise(&control.token)));

    if !control_seen {
        return Legibility::NotVerified {
            why: format!(
                "the control token {:?}, drawn at {:.1} pt, was not read back from the probe \
                 image. The engine is not able to read text of that size here, so its finding \
                 nothing else says nothing about what survived.",
                control.token, control.size_pt
            ),
        };
    }

    if outside.is_empty() {
        Legibility::Illegible {
            engine: engine.clone(),
        }
    } else {
        Legibility::Legible { found: outside }
    }
}

/// Case- and space-insensitive comparison, so a control read as `"K7 QX2"`
/// still matches `"K7QX2"`.
///
/// Engines insert and drop spaces around glyph clusters freely; requiring an
/// exact match would fail the control on a working engine, and a control that
/// fails when things are fine gets switched off.
fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

// ------------------------------------------------------- the type-level rule

/// Pixels that have been through redaction and passed the gate.
///
/// `docs/PLAN.md` §6 requires that when OCR is applied to a flattened page it is
/// run "only on already-redacted pixels with the redacted regions masked out",
/// because OCR over a *pre-redaction* image reinstates the secret as an invisible
/// text layer --- a shadow-text carrier, which §6's own table lists as a thing
/// redaction must defeat.
///
/// A comment saying so is a comment somebody edits around. This is the same move
/// `worker.rs` makes with `PreWorker`/`WarmWorker`: the shadow-text builder will
/// accept only this type, and the only way to obtain one is to present an
/// [`Legibility::Illegible`] verdict. "OCR the pre-redaction image" then does not
/// compile rather than being forbidden in prose.
#[derive(Debug)]
pub struct RedactedPixels<'a> {
    pixels: Pixels<'a>,
    engine: EngineId,
}

impl<'a> RedactedPixels<'a> {
    /// Certifies pixels against a verdict.
    ///
    /// # Errors
    ///
    /// Any verdict other than [`Legibility::Illegible`], and an inconsistent
    /// buffer. Both are refusals to produce the witness, not warnings.
    pub fn certify(pixels: Pixels<'a>, verdict: &Legibility) -> Result<Self, String> {
        if !pixels.is_consistent() {
            return Err("the buffer does not match its dimensions".into());
        }
        match verdict {
            Legibility::Illegible { engine } => Ok(Self {
                pixels,
                engine: engine.clone(),
            }),
            Legibility::Legible { found } => Err(format!(
                "{} legible span(s) survive; these pixels are not redacted",
                found.len()
            )),
            Legibility::NotVerified { why } => {
                Err(format!("not verified, so not certified: {why}"))
            }
        }
    }

    /// The certified pixels.
    #[must_use]
    pub fn pixels(&self) -> Pixels<'a> {
        self.pixels
    }

    /// Which engine certified them.
    #[must_use]
    pub fn engine(&self) -> &EngineId {
        &self.engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> EngineId {
        EngineId {
            name: "fake",
            build: "test".into(),
        }
    }

    /// The band sits below the region under test: y from 100 to 120.
    fn control() -> Control {
        Control::no_easier_than(
            &[[10.0, 10.0, 90.0, 16.0]],
            "K7QX2",
            [0.0, 100.0, 200.0, 120.0],
        )
        .expect("a 6 pt box is a usable control size")
    }

    fn item(text: &str, rect: [f32; 4]) -> RecognisedItem {
        RecognisedItem {
            text: text.into(),
            rect,
            confidence: Some(0.9),
        }
    }

    fn in_band(text: &str) -> RecognisedItem {
        item(text, [5.0, 102.0, 60.0, 118.0])
    }

    fn outside(text: &str) -> RecognisedItem {
        item(text, [10.0, 10.0, 90.0, 16.0])
    }

    #[test]
    fn control_is_sized_to_the_smallest_redacted_box_not_the_largest() {
        // 48 pt heading and 6 pt small print both redacted. Sizing to the heading
        // is the defect this guards: the engine would then only have to prove it
        // can read 48 pt.
        let c = Control::no_easier_than(
            &[[0.0, 0.0, 100.0, 48.0], [0.0, 60.0, 100.0, 66.0]],
            "K7QX2",
            [0.0, 100.0, 200.0, 120.0],
        )
        .expect("both boxes are usable");
        assert!(
            (c.size_pt - 6.0).abs() < f32::EPSILON,
            "expected the 6 pt box to set the size, got {}",
            c.size_pt
        );
    }

    #[test]
    fn a_control_with_no_usable_box_is_refused() {
        let e = Control::no_easier_than(&[[0.0, 0.0, 0.0, 0.0]], "K7QX2", [0.0, 0.0, 1.0, 1.0]);
        assert!(e.is_err(), "a zero-height box cannot size a control");
    }

    #[test]
    fn an_empty_token_is_refused() {
        let e = Control::no_easier_than(&[[0.0, 0.0, 10.0, 10.0]], "   ", [0.0, 0.0, 1.0, 1.0]);
        assert!(e.is_err(), "an empty token is trivially 'read back'");
    }

    #[test]
    fn nothing_read_at_all_is_not_verified_not_clean() {
        // The central case. An engine that returns nothing has produced exactly
        // what a perfect redaction produces, and the two must not be conflated.
        let v = adjudicate(&engine(), &control(), &Ok(vec![]));
        assert!(!v.certifies(), "an empty result certified a redaction");
        assert!(matches!(v, Legibility::NotVerified { .. }), "got {v:?}");
    }

    #[test]
    fn an_engine_failure_is_not_verified() {
        for e in [
            RecogniseError::Unavailable("no language pack".into()),
            RecogniseError::Crashed("SIGTRAP".into()),
            RecogniseError::TimedOut("10s".into()),
            RecogniseError::Rejected("too large".into()),
            RecogniseError::MalformedInput("short buffer".into()),
        ] {
            let v = adjudicate(&engine(), &control(), &Err(e.clone()));
            assert!(!v.certifies(), "{e:?} certified a redaction");
        }
    }

    #[test]
    fn the_control_alone_certifies() {
        let v = adjudicate(&engine(), &control(), &Ok(vec![in_band("K7QX2")]));
        assert!(v.certifies(), "got {v:?}");
    }

    #[test]
    fn survivors_outside_the_band_are_legible() {
        let v = adjudicate(
            &engine(),
            &control(),
            &Ok(vec![in_band("K7QX2"), outside("Nexperia")]),
        );
        match v {
            Legibility::Legible { found } => {
                assert_eq!(found.len(), 1);
                assert_eq!(found[0].text, "Nexperia");
            }
            other => panic!("surviving text was not reported legible: {other:?}"),
        }
    }

    #[test]
    fn a_survivor_without_confidence_still_counts() {
        // Windows OCR reports no per-word confidence. If absent confidence were
        // treated as low and filtered, every Windows survivor would vanish and
        // the gate would certify the page.
        let mut it = outside("Nexperia");
        it.confidence = None;
        let v = adjudicate(&engine(), &control(), &Ok(vec![in_band("K7QX2"), it]));
        assert!(!v.certifies(), "an unscored survivor certified the page");
    }

    #[test]
    fn a_low_confidence_survivor_still_counts() {
        let mut it = outside("Nexperia");
        it.confidence = Some(0.01);
        let v = adjudicate(&engine(), &control(), &Ok(vec![in_band("K7QX2"), it]));
        assert!(!v.certifies(), "a marginal survivor certified the page");
    }

    #[test]
    fn the_control_is_matched_through_spacing_and_case() {
        let v = adjudicate(&engine(), &control(), &Ok(vec![in_band("k7 qx2")]));
        assert!(
            v.certifies(),
            "a working engine failed its own control: {v:?}"
        );
    }

    #[test]
    fn text_matching_the_token_outside_the_band_does_not_stand_in_for_the_control() {
        // Position is what distinguishes the control from a survivor. If the
        // token were matched anywhere, a document that happens to contain it --- or
        // an engine reporting one box for the whole image --- would satisfy the
        // control it is supposed to have earned.
        let v = adjudicate(&engine(), &control(), &Ok(vec![outside("K7QX2")]));
        assert!(
            !v.certifies(),
            "the control was satisfied from outside its band"
        );
        assert!(matches!(v, Legibility::NotVerified { .. }), "got {v:?}");
    }

    #[test]
    fn certifying_pixels_requires_an_illegible_verdict() {
        let buf = vec![0u8; 4 * 4 * 4];
        let px = Pixels {
            rgba: &buf,
            width: 4,
            height: 4,
            scale: 2.0,
        };
        assert!(px.is_consistent());

        let clean = Legibility::Illegible { engine: engine() };
        assert!(RedactedPixels::certify(px, &clean).is_ok());

        for bad in [
            Legibility::Legible {
                found: vec![outside("Nexperia")],
            },
            Legibility::NotVerified {
                why: "engine crashed".into(),
            },
        ] {
            assert!(
                RedactedPixels::certify(px, &bad).is_err(),
                "{bad:?} produced a witness"
            );
        }
    }

    #[test]
    fn an_inconsistent_buffer_is_refused() {
        let buf = vec![0u8; 10];
        let px = Pixels {
            rgba: &buf,
            width: 4,
            height: 4,
            scale: 2.0,
        };
        assert!(!px.is_consistent(), "10 bytes is not 4x4 RGBA");
        let clean = Legibility::Illegible { engine: engine() };
        assert!(RedactedPixels::certify(px, &clean).is_err());
    }
}
