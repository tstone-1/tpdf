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
//! **The caller is [`crate::ocr_gate`]**, which is where the decisions this module
//! makes meet a rendered page: it captures the page's words before the removal,
//! chooses the scale, builds the probe image and turns a [`Legibility`] into a
//! sentence in `redact::Applied::why`. Wired into `redact_copy` and
//! `redact_document` on 2026-08-27.
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
//! **That table is executable as of 2026-08-27** --- `examples/ocr_sandbox_probe.rs`, three
//! rungs, 7/7 on OS build 25G83, with the SIGTRAP reproducing four weeks after it was first
//! seen. It also measures something the hand run did not: the rung that worked above allowed
//! reads and said nothing about **writes**, while [`OCR_SANDBOX_PROFILE`] denies
//! `file-write*` and `network*` --- and Vision reads under it. Until then the shipped constant
//! was inherited from a neighbouring rung rather than measured.
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
//! [`crate::ocr_worker`] is that process, built 2026-08-27. It maps no PDF parser, holds the
//! pixels in a mapping the parent wrote and the child cannot, and applies the profile before
//! it reads a single request. What it deliberately does **not** claim is that the app process
//! never maps the engine: `objc2-vision` links Vision rather than `dlopen`ing it, so every
//! binary linking [`crate::ocr_vision`] maps the framework at launch, called or not.
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
///
/// It carries a [`NotVerifiedCause`] as well as its sentence because this one
/// refusal has **four** distinct reasons and it is by far the commonest thing
/// the gate says: measured 2026-08-28 over 40 real documents, 850 of 943
/// unanswered regions, 90.1%. A single bucket holding nine tenths of the answer
/// is the same defect this attribution was built to remove, one level down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlTooEasy(String, NotVerifiedCause);

impl ControlTooEasy {
    /// Which of the four ways a control could not be chosen this was.
    #[must_use]
    pub fn cause(&self) -> NotVerifiedCause {
        self.1
    }
}

impl fmt::Display for ControlTooEasy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The point size a control has to be set at to be no easier than what went.
///
/// The *shortest* box the redaction covered, box height in points being the
/// closest thing to a glyph size available without asking the font.
///
/// One function rather than the same fold written out in [`Control::no_easier_than`]
/// and in [`control_from_page`]: two copies of a safety rule is the drift this
/// repository keeps recording, and here the drifted copy would be the one that
/// decides whether a page of small print may be called clean.
///
/// # Errors
///
/// If no box had a usable height. Refusing is correct: with nothing to size
/// against there is no honest control, and no control means no certification.
fn size_no_easier_than(boxes: &[[f32; 4]]) -> Result<f32, ControlTooEasy> {
    let smallest = boxes
        .iter()
        .map(|b| b[3] - b[1])
        .filter(|h| h.is_finite() && *h > 0.0)
        .fold(f32::INFINITY, f32::min);
    if !smallest.is_finite() {
        return Err(ControlTooEasy(
            "no redacted box had a usable height, so there is nothing to size a control \
             against; refusing rather than picking one"
                .into(),
            NotVerifiedCause::ControlNoSize,
        ));
    }
    Ok(smallest)
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
        let smallest = size_no_easier_than(redacted_boxes)?;
        let token = token.into();
        if token.trim().is_empty() {
            return Err(ControlTooEasy(
                "an empty control token is read back by every engine and by none".into(),
                NotVerifiedCause::ControlEmptyToken,
            ));
        }
        Ok(Self {
            token,
            size_pt: smallest,
            band,
        })
    }

    /// Whether a rect belongs to the control band, by its centre.
    ///
    /// This was strict containment first, and `ocr-probe` failed on the real engine because
    /// of it. Vision's reported box is not tight to the glyphs: handed a strip cropped to a
    /// span's own rectangle, it read the text back and placed it **1.5 pt above the strip it
    /// came from**, so `top >= band_top` was false and a control the engine had plainly read
    /// counted as a survivor. The gate then refused a redaction that was fine.
    ///
    /// A centre test keeps the property that matters --- position, not string, decides what
    /// the control is, so a token occurring elsewhere on the page cannot stand in for it ---
    /// while tolerating an engine whose idea of a bounding box is looser than the pixels it
    /// was given. Which every engine's is: they are detections, not measurements.
    fn contains(&self, rect: &[f32; 4]) -> bool {
        let [l, t, r, b] = *rect;
        let [bl, bt, br, bb] = self.band;
        let (cx, cy) = ((l + r) / 2.0, (t + b) / 2.0);
        cx >= bl && cx <= br && cy >= bt && cy <= bb
    }
}

// ------------------------------------------------- choosing that control

/// The fewest characters a control token may have.
///
/// **Measured against 41 real documents rather than picked.** Treating every
/// text object's own box as the region a reader would draw over that line ---
/// 154,095 of them --- and asking whether any surviving word on the same page
/// qualifies as a control, the coverage runs 71.9% at two characters, 68.5% at
/// three, **58.3% at four**, 45.9% at six and 35.5% at eight. There is no flat
/// part to sit on: every character costs coverage, so the value is a judgement
/// about what a token has to be, taken with the price in front of it.
///
/// Four, because [`adjudicate`] matches the control by asking whether one
/// recognised span *contains* the token. A two- or three-character token is a
/// fragment an engine can emit from noise, and a fragment matching by accident
/// certifies a page nothing was read on. Four is a short whole word, and going
/// further buys nothing that argument does not already have while costing 12.4
/// points of coverage at six.
///
/// A region with no control is reported *not verified*, which is what every
/// region gets today --- so this constant trades how often the gate can speak,
/// never how often it is right.
pub const MIN_CONTROL_CHARS: usize = 4;

/// How much taller than the smallest covered box a control word may still be.
///
/// Box heights arrive as floats out of PDFium and two lines set in the same font
/// on the same page routinely differ in the last digit. Without the slack a
/// control word from the very line below the redacted one is refused as *easier*
/// on a difference no reader could see, which throws away the best control on
/// the page for a rounding error.
const CONTROL_HEIGHT_SLACK_PT: f32 = 0.01;

/// A word on the page, as the control chooser sees it.
///
/// Deliberately not [`RecognisedItem`]: that is what an engine *read*, and this
/// is what the document *says*. A control chosen from an engine's own output
/// would be the engine agreeing with itself, which is a shape this repository
/// has recorded from several other directions.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlWord {
    /// Its box, same convention as everything else here: `left, top, right,
    /// bottom` in PDF points, y increasing **downwards**.
    ///
    /// PDFium reports a page object's bounds the other way up, so a caller
    /// holding [`crate::redact::PageObject::bounds`] has to flip them. Only the
    /// tie-break turns on it --- the height rule and the overlap test read the
    /// same either way --- so a caller that gets it wrong still gets an honest
    /// control, just the bottom-most of two equally long words rather than the
    /// topmost. It is stated because a test that says *which* control a page
    /// yields would otherwise be asserting the caller's convention.
    pub rect: [f32; 4],
    /// What it draws.
    pub text: String,
}

/// A control chosen from a page, before anyone knows where its band will land.
///
/// **Two coordinate systems, kept apart on purpose.** [`crop`](Self::crop) is on
/// the *page*, because that is what has to be rendered; a [`Control`]'s band is
/// in the *probe image*, because that is where [`adjudicate`] partitions items.
/// The band is not the crop moved --- the probe image is the region under test
/// with this strip appended below it, so the band's `top` is the region's
/// height. Returning a `Control` from here would mean guessing that offset, and
/// `docs/TRAPS.md` carries more than one entry about a rectangle produced in one
/// space and read in another.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlChoice {
    /// Where on the page the control band is cropped from.
    pub crop: [f32; 4],
    /// The text drawn there, which the engine has to read back.
    pub token: String,
    /// The size it has to be no easier than --- see [`size_no_easier_than`].
    pub size_pt: f32,
}

impl ControlChoice {
    /// The [`Control`] for a probe image whose control band landed at `band`.
    ///
    /// The token and the size come across unchanged; only the rectangle is the
    /// caller's to supply, because only the caller knows how it composited the
    /// probe image.
    #[must_use]
    pub fn placed(&self, band: [f32; 4]) -> Control {
        Control {
            token: self.token.clone(),
            size_pt: self.size_pt,
            band,
        }
    }
}

/// Chooses a control out of the text the redaction leaves behind.
///
/// `docs/PLAN.md` §6 step 4 renders the redacted regions and OCRs them, and it
/// may only call the result clean when the engine was shown to read *on this
/// image at this size* --- which is what a control is for. `docs/TRAPS.md`'s
/// entry *a control that is easier than the check certifies nothing* is the
/// failure this exists to make unreachable: the size rule and the "did the
/// removal take it" rule are enforced here rather than left to whoever calls
/// [`Control::no_easier_than`], which takes both on trust.
///
/// Three properties decide a candidate, and each of them is a way the gate could
/// otherwise certify a page it had proved nothing about:
///
/// 1. **No region covers it.** A word the removal was supposed to take is not
///    evidence that the engine can read: it is evidence the removal failed. The
///    test is [`crate::redact::overlaps`], the same one that decided which words
///    the removal took, so the two cannot come to disagree about a word.
/// 2. **It is set no larger than the smallest box the regions covered.** A
///    control in 12 pt proves nothing about a 6 pt footnote that survived.
/// 3. **It draws at least [`MIN_CONTROL_CHARS`] characters**, because
///    [`adjudicate`] matches the token against a recognised span by containment
///    and a fragment matches by accident.
///
/// The longest qualifying word wins; ties go to the topmost and then the
/// leftmost, so the same page always yields the same control and a test can say
/// which.
///
/// # Errors
///
/// If nothing qualifies, and the reason says which of the three rules ran out ---
/// a reader who is told *not verified* can act on "every line left on the page is
/// bigger than what you removed" and cannot act on "no control".
pub fn control_from_page(
    words: &[ControlWord],
    regions: &[[f32; 4]],
) -> Result<ControlChoice, ControlTooEasy> {
    let covered: Vec<[f32; 4]> = words
        .iter()
        .filter(|word| {
            regions
                .iter()
                .any(|region| crate::redact::overlaps(word.rect, *region))
        })
        .map(|word| word.rect)
        .collect();
    let size_pt = size_no_easier_than(&covered)?;

    let survivors: Vec<&ControlWord> = words
        .iter()
        .filter(|word| {
            !regions
                .iter()
                .any(|region| crate::redact::overlaps(word.rect, *region))
        })
        .collect();
    if survivors.is_empty() {
        return Err(ControlTooEasy(
            "the regions cover every word on this page, so there is nothing left for the \
             engine to read back and nothing here can be certified"
                .into(),
            NotVerifiedCause::ControlNoSurvivor,
        ));
    }

    let small: Vec<&&ControlWord> = survivors
        .iter()
        .filter(|word| word.rect[3] - word.rect[1] <= size_pt + CONTROL_HEIGHT_SLACK_PT)
        .collect();
    if small.is_empty() {
        return Err(ControlTooEasy(
            format!(
                "every word left on this page is set larger than the {size_pt:.1} pt that was \
                 removed, so reading one back would say nothing about the small print"
            ),
            NotVerifiedCause::ControlAllLarger,
        ));
    }

    // Longest wins; a tie goes to the topmost and then the leftmost. Ordered by
    // hand rather than by sorting a key, because two of the three run the other
    // way: more characters is better and a *smaller* top is.
    let mut best: Option<&ControlWord> = None;
    for word in &small {
        let chars = longest_run(&word.text).chars().count();
        if chars < MIN_CONTROL_CHARS {
            continue;
        }
        let better = match best {
            None => true,
            Some(have) => {
                let mine = have.text.trim().chars().count();
                chars > mine
                    || (chars == mine
                        && (word.rect[1] < have.rect[1]
                            || (word.rect[1] == have.rect[1] && word.rect[0] < have.rect[0])))
            }
        };
        if better {
            best = Some(word);
        }
    }
    let chosen = best.ok_or_else(|| {
        ControlTooEasy(
            format!(
                "no word left on this page draws {MIN_CONTROL_CHARS} characters at \
                 {size_pt:.1} pt or smaller, and a shorter control is read back by accident"
            ),
            NotVerifiedCause::ControlTooShort,
        )
    })?;

    Ok(ControlChoice {
        crop: chosen.rect,
        token: longest_run(&chosen.text).to_string(),
        size_pt,
    })
}

/// The longest run of non-whitespace in a piece of drawn text.
///
/// **The token is a word and not the line it sits on, and that is a measurement
/// rather than a preference.** [`adjudicate`] asks whether *one* recognised span
/// contains the token, so a token spanning a whole line is only read back when
/// the engine happens to return that line as a single span. Measured on
/// 2026-08-27 with `ocr-probe`: handing Vision the whole 52-character line of
/// `text-base14.pdf` as the token produced `NotVerified` on a page where nothing
/// was wrong, while the same line on `text-marked.pdf` and `text-truetype.pdf`
/// read back perfectly --- a gate that refuses a correct redaction on one font
/// and accepts it on another.
///
/// One word inside the band is contained by whatever span the engine returns for
/// that line, however it chose to break it up. It is still matched by position
/// first, so a word occurring elsewhere on the page cannot stand in for it.
fn longest_run(text: &str) -> &str {
    text.split_whitespace()
        .max_by_key(|run| run.chars().count())
        .unwrap_or("")
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
        /// Which step could not be completed, in terms a *count* can be taken
        /// over. See [`NotVerifiedCause`].
        cause: NotVerifiedCause,
    },
}

/// Which step of the gate did not complete.
///
/// **It exists because a sentence cannot be tallied.** `redact-reach-probe`
/// bucketed these by `why.contains("control token")`, which is a second parser
/// over prose written for a human: reword the sentence and the bucket silently
/// reads zero, which is indistinguishable from that step never failing. The
/// measurement that wanted it --- `docs/PLAN.md` §6, *a third of the gate's
/// regions returning not verified for reasons nobody has bucketed* --- could
/// therefore attribute exactly one of them, and could not report a page-wide
/// refusal at all.
///
/// **One variant per step of the gate**, not a taxonomy of what went wrong
/// underneath. Two steps can fail for the same underlying reason --- a region
/// off the page is refused by [`super::ocr_gate`]'s `strip` and again by its
/// `mask_columns` --- and keeping those apart is the whole point, because a
/// count is only useful if it names a place to look.
///
/// The one variant with two construction sites is [`NotVerifiedCause::EngineError`],
/// and that is deliberate rather than an exception: [`adjudicate`]'s error arm and
/// [`super::ocr_gate::unanswered`] are two callers of one rule, which
/// `the_error_path_says_what_adjudicate_would` already requires to agree.
///
/// Nothing greps for these. The probe matches the variants exhaustively, so a
/// new step that forgets to be counted is `error[E0004]` rather than a bucket
/// silently reading zero --- which is the failure this type replaces. No count
/// of the variants is written in prose anywhere: [`NotVerifiedCause::ALL`] is
/// the authority, and the sentence above said *eight* for the half hour between
/// this type being written and its commonest variant being split into five.
///
/// `why` still carries the sentence, and no reader sees any of this: it never
/// leaves the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotVerifiedCause {
    /// No redacted box had a usable height, so there is nothing to size a
    /// control against. Page-wide.
    ControlNoSize,
    /// The regions cover every word on the page, so nothing survived for the
    /// engine to read back. Page-wide.
    ControlNoSurvivor,
    /// Every word left on the page is set larger than the smallest thing the
    /// redaction removed, so reading one back would say nothing about the small
    /// print. Page-wide.
    ControlAllLarger,
    /// Words survive at the right size and none of them draws
    /// [`MIN_CONTROL_CHARS`] characters, so any control would be short enough to
    /// be read back by accident. Page-wide.
    ControlTooShort,
    /// The caller offered an empty control token. Not reachable from
    /// [`control_from_page`], which never builds one; it belongs to
    /// [`Control::no_easier_than`]'s other caller.
    ControlEmptyToken,
    /// The probe image would not fit within the worker's pixel capacity at any
    /// scale that keeps the control legible. Page-wide.
    ScaleRefused,
    /// The control strip would not render. Page-wide.
    ControlStrip,
    /// The region's own strip would not render.
    RegionStrip,
    /// The region's columns could not be masked out of its strip, which is what
    /// happens when the region is not on the page at all.
    Mask,
    /// The region strip and the control strip could not be stacked into one
    /// probe image.
    Stack,
    /// The engine did not answer.
    EngineError,
    /// The engine answered and did not read the control token back, so its
    /// silence about the rest of the image carries no information.
    ControlUnread,
}

impl NotVerifiedCause {
    /// A short stable label, for a report keyed by cause.
    ///
    /// **Here rather than in the probe that prints it, because two variants
    /// sharing a label merge two buckets into one and nothing says so** --- the
    /// count still adds up, and the step that stopped being reported reads as a
    /// step that stopped failing. `every_cause_has_its_own_label` is the check;
    /// it is the same shape as the arithmetic `[WARN]` the probe already runs
    /// over its own three counters.
    ///
    /// The match is exhaustive on purpose. A new step that forgets to be
    /// counted is `error[E0004]` here.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ControlNoSize => "nothing to size a control against",
            Self::ControlNoSurvivor => "the regions cover every word",
            Self::ControlAllLarger => "every surviving word is larger",
            Self::ControlTooShort => "no surviving word is long enough",
            Self::ControlEmptyToken => "the control token was empty",
            Self::ScaleRefused => "probe image will not fit",
            Self::ControlStrip => "control strip would not render",
            Self::RegionStrip => "region strip would not render",
            Self::Mask => "region is not on the page",
            Self::Stack => "strips would not stack",
            Self::EngineError => "the engine did not answer",
            Self::ControlUnread => "control not read back",
        }
    }

    /// Every variant, so a caller can report a zero rather than omit a row.
    ///
    /// A cause that never fired is the interesting reading and an absent row is
    /// not: `docs/TRAPS.md` records an empty answer from a whole-document scan
    /// being unable to say whether it looked.
    pub const ALL: [Self; 12] = [
        Self::ControlNoSize,
        Self::ControlNoSurvivor,
        Self::ControlAllLarger,
        Self::ControlTooShort,
        Self::ControlEmptyToken,
        Self::ScaleRefused,
        Self::ControlStrip,
        Self::RegionStrip,
        Self::Mask,
        Self::Stack,
        Self::EngineError,
        Self::ControlUnread,
    ];
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
                cause: NotVerifiedCause::EngineError,
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
            cause: NotVerifiedCause::ControlUnread,
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
            Legibility::NotVerified { why, .. } => {
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

    #[test]
    fn every_cause_has_its_own_label() {
        // Two variants sharing a label merge two buckets in a report keyed by
        // it, and the total still adds up -- so the step that stopped being
        // reported reads as a step that stopped failing.
        let mut seen = std::collections::BTreeSet::new();
        for cause in NotVerifiedCause::ALL {
            assert!(
                seen.insert(cause.label()),
                "two causes share the label {:?}",
                cause.label()
            );
        }
        assert_eq!(seen.len(), NotVerifiedCause::ALL.len());
    }

    #[test]
    fn all_lists_every_cause_once() {
        // `ALL` is written by hand, so a new variant that the `label` match
        // forces you to handle can still be left out of the list a report
        // iterates -- which is a row that is silently never printed rather than
        // printed as a zero. Nothing else in the tree would notice.
        for cause in NotVerifiedCause::ALL {
            assert_eq!(
                NotVerifiedCause::ALL
                    .iter()
                    .filter(|c| **c == cause)
                    .count(),
                1,
                "{cause:?} is in ALL more than once"
            );
        }
    }

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
        let why = e.expect_err("a zero-height box cannot size a control");
        assert_eq!(why.cause(), NotVerifiedCause::ControlNoSize);
    }

    #[test]
    fn an_empty_token_is_refused() {
        let e = Control::no_easier_than(&[[0.0, 0.0, 10.0, 10.0]], "   ", [0.0, 0.0, 1.0, 1.0]);
        let why = e.expect_err("an empty token is trivially 'read back'");
        assert_eq!(why.cause(), NotVerifiedCause::ControlEmptyToken);
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
            &Ok(vec![in_band("K7QX2"), outside("Aldebaran")]),
        );
        match v {
            Legibility::Legible { found } => {
                assert_eq!(found.len(), 1);
                assert_eq!(found[0].text, "Aldebaran");
            }
            other => panic!("surviving text was not reported legible: {other:?}"),
        }
    }

    #[test]
    fn a_survivor_without_confidence_still_counts() {
        // Windows OCR reports no per-word confidence. If absent confidence were
        // treated as low and filtered, every Windows survivor would vanish and
        // the gate would certify the page.
        let mut it = outside("Aldebaran");
        it.confidence = None;
        let v = adjudicate(&engine(), &control(), &Ok(vec![in_band("K7QX2"), it]));
        assert!(!v.certifies(), "an unscored survivor certified the page");
    }

    #[test]
    fn a_low_confidence_survivor_still_counts() {
        let mut it = outside("Aldebaran");
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
    fn a_control_box_looser_than_the_band_still_counts() {
        // The real failure `ocr-probe` hit. Vision, handed a strip cropped to a span's own
        // rectangle, reported that span 1.5 pt *above* the strip -- so strict containment
        // rejected a control the engine had obviously read, and the gate refused a clean
        // redaction. An engine's box is a detection, not a measurement.
        let c = control();
        let spilling = item("K7QX2", [0.0, 98.5, 200.0, 121.5]);
        let v = adjudicate(&engine(), &c, &Ok(vec![spilling]));
        assert!(
            v.certifies(),
            "a control overlapping its band by all but a hair was not counted: {v:?}"
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
                found: vec![outside("Aldebaran")],
            },
            Legibility::NotVerified {
                why: "engine crashed".into(),
                cause: NotVerifiedCause::EngineError,
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

    // ------------------------------------------------ choosing the control

    fn word(text: &str, rect: [f32; 4]) -> ControlWord {
        ControlWord {
            rect,
            text: text.into(),
        }
    }

    /// One region over one 6 pt word, and four survivors, **each of which only
    /// one rule can decide**.
    ///
    /// `heading` is long enough and is set at 20 pt, so only the size rule can
    /// refuse it. `no` is the right size and draws two characters, so only the
    /// length rule can. `readable` and `four` both qualify and differ only in
    /// length. Written this way because two rules that both refuse a word make
    /// each other unfalsifiable --- see `docs/TRAPS.md`.
    fn page() -> Vec<ControlWord> {
        vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("heading", [10.0, 30.0, 90.0, 50.0]),
            word("no", [10.0, 60.0, 30.0, 66.0]),
            word("readable", [10.0, 80.0, 70.0, 86.0]),
            word("four", [10.0, 100.0, 40.0, 106.0]),
        ]
    }

    /// Over `secret` and nothing else.
    const OVER_SECRET: [f32; 4] = [10.0, 10.0, 90.0, 20.0];

    #[test]
    fn the_longest_word_of_the_right_size_that_survived_is_the_control() {
        let chosen = control_from_page(&page(), &[OVER_SECRET]).expect("a control");
        assert_eq!(chosen.token, "readable");
        assert_eq!(chosen.crop, [10.0, 80.0, 70.0, 86.0]);
        assert!(
            (chosen.size_pt - 6.0).abs() < 0.001,
            "sized against the 6 pt box that went, not against the control: {}",
            chosen.size_pt
        );
    }

    /// The over-selection control. The covered word is the longest on the page
    /// and the right size, so the *only* thing that can keep it out of the
    /// answer is that a region covers it.
    #[test]
    fn a_word_the_region_covers_is_never_the_control() {
        let mut words = page();
        words[0] = word("compromised", [12.0, 12.0, 88.0, 18.0]);
        let chosen = control_from_page(&words, &[OVER_SECRET]).expect("a control");
        assert_eq!(
            chosen.token, "readable",
            "a word the removal was supposed to take is not evidence the engine can read"
        );
    }

    #[test]
    fn a_word_set_larger_than_what_went_is_refused() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("heading", [10.0, 30.0, 90.0, 50.0]),
        ];
        let why = control_from_page(&words, &[OVER_SECRET]).expect_err("refused");
        assert!(
            why.to_string().contains("larger"),
            "the reason has to say which rule ran out: {why}"
        );
    }

    #[test]
    fn a_word_too_short_to_be_a_control_is_refused() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("no", [10.0, 60.0, 30.0, 66.0]),
        ];
        let why = control_from_page(&words, &[OVER_SECRET]).expect_err("refused");
        assert!(
            why.to_string().contains("characters"),
            "the reason has to say which rule ran out: {why}"
        );
        // The sentence and the cause are two different artifacts -- one is read
        // by a person, the other counted by `redact-reach-probe` -- so they can
        // drift apart, and only this pins the second.
        assert_eq!(why.cause(), NotVerifiedCause::ControlTooShort);
    }

    #[test]
    fn a_page_whose_every_word_went_leaves_nothing_to_read_back() {
        let words = vec![word("secret", [12.0, 12.0, 88.0, 18.0])];
        let why = control_from_page(&words, &[OVER_SECRET]).expect_err("refused");
        assert!(
            why.to_string().contains("every word"),
            "the reason has to say which rule ran out: {why}"
        );
        assert_eq!(why.cause(), NotVerifiedCause::ControlNoSurvivor);
    }

    /// A region covering nothing has no box to size a control against, and
    /// that refusal is [`size_no_easier_than`]'s rather than a second copy of
    /// it here.
    #[test]
    fn a_region_over_no_words_has_no_size_to_measure_against() {
        let why = control_from_page(&page(), &[[500.0, 500.0, 520.0, 510.0]]).expect_err("refused");
        assert!(
            why.to_string().contains("nothing to size a control"),
            "{why}"
        );
    }

    /// The boundary the slack exists for. A survivor exactly as tall as the
    /// smallest covered box is *not* easier and must qualify.
    #[test]
    fn a_word_exactly_as_tall_as_what_went_still_qualifies() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("sameheight", [10.0, 30.0, 90.0, 36.0]),
        ];
        let chosen = control_from_page(&words, &[OVER_SECRET]).expect("a control");
        assert_eq!(chosen.token, "sameheight");
    }

    #[test]
    fn a_word_one_point_taller_than_what_went_does_not() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("sameheight", [10.0, 30.0, 90.0, 37.0]),
        ];
        let why = control_from_page(&words, &[OVER_SECRET]).expect_err("refused");
        assert_eq!(why.cause(), NotVerifiedCause::ControlAllLarger);
    }

    /// Determinism, so a test can say *which* control a page yields. Three
    /// words of equal length: the tie has to be broken by top and then by left,
    /// and the answer must not depend on the order they arrive in.
    #[test]
    fn a_tie_goes_to_the_topmost_and_then_the_leftmost() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("bbbb", [60.0, 40.0, 90.0, 46.0]),
            word("cccc", [10.0, 60.0, 40.0, 66.0]),
            word("aaaa", [10.0, 40.0, 40.0, 46.0]),
        ];
        let chosen = control_from_page(&words, &[OVER_SECRET]).expect("a control");
        assert_eq!(chosen.token, "aaaa", "topmost, then leftmost");
        let mut reversed = words.clone();
        reversed.reverse();
        assert_eq!(
            control_from_page(&reversed, &[OVER_SECRET])
                .expect("a control")
                .token,
            "aaaa",
            "the same page has to yield the same control whatever order it is read in"
        );
    }

    /// The two coordinate systems. `placed` may take the band from its caller
    /// and nothing else --- a band derived from the crop would be the page's
    /// rectangle read in the probe image's space.
    #[test]
    fn placed_takes_the_band_from_its_caller_and_nothing_else() {
        let chosen = control_from_page(&page(), &[OVER_SECRET]).expect("a control");
        let band = [0.0, 200.0, 300.0, 220.0];
        let control = chosen.placed(band);
        assert_eq!(control.band, band);
        assert_ne!(control.band, chosen.crop);
        assert_eq!(control.token, chosen.token);
        assert!((control.size_pt - chosen.size_pt).abs() < 0.001);
    }

    /// The token is a word, because [`adjudicate`] needs one recognised span to
    /// contain the whole of it. Measured: see [`longest_run`].
    #[test]
    fn the_token_is_a_word_and_not_the_line_it_sits_on() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("a longer line of prose", [10.0, 30.0, 90.0, 36.0]),
        ];
        let chosen = control_from_page(&words, &[OVER_SECRET]).expect("a control");
        assert_eq!(chosen.token, "longer");
    }

    /// The ranking half of the same rule. The line of short words is nearly
    /// three times as long as `readable` and holds nothing an engine could be
    /// asked to read back, so ranking by the line would pick it.
    #[test]
    fn a_line_of_short_words_does_not_outrank_one_long_one() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("aaa bbb ccc ddd eee fff", [10.0, 30.0, 90.0, 36.0]),
            word("readable", [10.0, 60.0, 70.0, 66.0]),
        ];
        let chosen = control_from_page(&words, &[OVER_SECRET]).expect("a control");
        assert_eq!(chosen.token, "readable");
    }

    /// And a page holding only short words is refused, however much text is on
    /// it: three characters is a fragment an engine emits from noise.
    #[test]
    fn a_line_whose_longest_word_is_too_short_is_refused() {
        let words = vec![
            word("secret", [12.0, 12.0, 88.0, 18.0]),
            word("aaa bbb ccc ddd eee fff", [10.0, 30.0, 90.0, 36.0]),
        ];
        let why = control_from_page(&words, &[OVER_SECRET]).expect_err("refused");
        assert!(why.to_string().contains("characters"), "{why}");
    }

    /// The chosen control has to survive [`adjudicate`], or the chooser is
    /// producing something the gate cannot use. Read back in the band: clean.
    #[test]
    fn a_chosen_control_read_back_in_its_band_certifies() {
        let chosen = control_from_page(&page(), &[OVER_SECRET]).expect("a control");
        let band = [0.0, 100.0, 200.0, 120.0];
        let control = chosen.placed(band);
        let read = Ok(vec![item(&chosen.token, [5.0, 102.0, 60.0, 118.0])]);
        assert!(adjudicate(&engine(), &control, &read).certifies());
    }
}
