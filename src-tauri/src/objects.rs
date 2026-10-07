//! What PDFium says is on a page: one entry per object, in its order.
//!
//! The bridge between the renderer and [`crate::redact`], which is deliberately
//! PDFium-free --- it takes a list of [`PageObject`] and decides what a region
//! covers, and this is what produces that list. Keeping the two apart is what
//! lets every rule in `redact.rs` be tested against objects a test wrote down,
//! with no library loaded and no document open.
//!
//! It lived in `examples/redact_probe.rs` until 2026-08-26, which is to say the
//! only thing that could produce a plan was a probe. Moving it here is what puts
//! `redact.rs` on a path a reader can reach.
//!
//! ## The text of a text object, and why it is not asked for directly
//!
//! `FPDFTextObj_GetText` exists and is exported by the vendored build. It is not
//! what this uses. The characters are taken from the page's own text page and
//! grouped by the object each one belongs to, through `FPDFText_GetTextObject`
//! --- which is the mapping `structure.rs` already makes for the tag tree, so the
//! astral-pair and dropped-character traps that surround `FPDFText_*` are paid
//! for once rather than twice. `RawTextPage::code` is a UTF-16 code *unit*, and
//! collecting units before decoding is what keeps a surrogate pair one character
//! rather than two replacement marks.
//!
//! ## An object PDFium will not measure is reported, never skipped
//!
//! It gets a bounding box of everything, so it overlaps every region and
//! `redact::covered` reports it as unhandled. The alternative is a redaction
//! that passes silently over whatever it could not see, which is the confident
//! lie `docs/PLAN.md` §6 opens by forbidding.

use std::collections::HashMap;

use pdfium_render::prelude::*;

use crate::progressive::RawPage;
use crate::text::RawTextPage;

/// A rectangle in PDF page space --- `[left, bottom, right, top]`, y upwards.
///
/// PDFium's own convention for object bounds, which is what the caller has when
/// it builds one of these, so nothing is converted on the way in. Note this is
/// **not** the display space `text::to_device` produces: a page with `/Rotate
/// 90` reports object bounds here unrotated, and a caller holding a reader's
/// selection has to map it back. `docs/TRAPS.md` has that entry more than once.
pub type Rect = [f32; 4];

/// One object PDFium found on the page, in the order it enumerated them.
#[derive(Debug, Clone, PartialEq)]
pub struct PageObject {
    /// Its bounding box, as PDFium reports it.
    pub bounds: Rect,
    /// What PDFium calls it: `text`, `image`, `path`, or anything else.
    ///
    /// A string rather than an enum because the only two questions asked of it
    /// are "is this text" and "what do I call it in a refusal", and an enum here
    /// would be a second vocabulary to keep in step with PDFium's.
    pub kind: String,
}

/// A text object drawn inside a Form XObject.
///
/// PDFium enumerates a form as **one** page object, so the text inside it is not
/// in the page's text-object list and [`crate::redact::remove_shows`] cannot
/// address it. It is the largest carrier a redaction cannot take that is made
/// of ordinary text ---
/// `docs/PLAN.md` §6 measured 9,310 of 154,095 realistic regions across 41 real
/// documents, three times the image count.
#[derive(Debug, Clone, PartialEq)]
pub struct FormText {
    /// Its box **in page space**, so it can be compared with a region directly.
    ///
    /// PDFium reports a form child's bounds in the *form's* own space --- measured
    /// on `form-xobject.pdf`, where a form placed at (60, 600) reports a child at
    /// (0.9, 19.9) --- so whoever builds one of these has already applied the
    /// form's matrix. Doing it here rather than in the comparison keeps
    /// [`crate::redact::covered`] free of a second coordinate system.
    pub bounds: Rect,
    /// What it draws, for the same reason [`PageObjects::text`]
    /// carries it: a caller has to be able to say what a removal would take.
    pub draws: String,
}

/// One Form XObject on a page, and the text inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct FormObject {
    /// Where the form itself sits in PDFium's enumeration of the page's objects.
    ///
    /// The same index [`crate::redact::Unhandled::at`] carries, and the index
    /// that identifies *which* form a removal is aimed at ---
    /// `remove_form_shows` finds the
    /// form's stream by counting `Do` operations in the page's content, and this
    /// is the position it counts to.
    pub at: usize,
    /// Its text objects, in PDFium's order --- which is the order the show
    /// operators appear in the form's own content stream, and the order a
    /// removal addresses them by.
    pub text: Vec<FormText>,
    /// What else is inside that this does not reach.
    ///
    /// **A nested form is the case that matters**: descending one level is a
    /// decision, and text a level further down has to be reported rather than
    /// silently missed. An image or a path inside a form is the same refusal the
    /// page level already makes, one level down.
    ///
    /// **"The same refusal" is what that last sentence claimed and not what the
    /// code did**, and it is kept because it names the defect exactly: the page
    /// level refuses an object *the region covers*, and this level refused every
    /// child of a form the region touched, wherever on the sheet it sat. A form
    /// is routinely a whole-page container --- a letterhead, a header band, a
    /// chart --- so a region over one line inside one reported every picture in
    /// the document's furniture. Measured over 40 real documents at 2,893
    /// word-sized regions: form children were 636 of the 1,131 refusals, 56%,
    /// and **every image refusal in the corpus** was one --- of which **174,
    /// 15.4% of all refusals, were about objects the region does not cover**.
    /// Each carries its box now, and [`crate::redact::covered`] asks the same
    /// question of it that it asks of a page object.
    pub unreachable: Vec<FormOther>,
    /// Its pictures, in PDFium's order, each with its box in page space.
    ///
    /// The order is the order the form's own content draws them in, which is
    /// what [`crate::redact::remove_form_images`] addresses them by. Until
    /// 2026-10-03 a picture inside a form was in
    /// [`unreachable`](Self::unreachable): on 64 real documents, 350 of 4,616
    /// word-sized regions touched one and could take nothing of it.
    pub images: Vec<Rect>,
    /// Its drawings, in PDFium's order, each with its box in page space.
    ///
    /// The order is the order the form's own content paints them in, which is
    /// what [`crate::redact::take_form_paths`] addresses them by. Until
    /// 2026-10-03 a drawing inside a form was in
    /// [`unreachable`](Self::unreachable): on 64 real documents, 630 of 4,616
    /// word-sized regions touched one and could take nothing of it.
    pub paths: Vec<Rect>,
}

/// One thing inside a Form XObject that a removal cannot address.
///
/// Separate from [`crate::redact::Unhandled`] rather than a widening of it,
/// because the two carry different things for different readers: `Unhandled`
/// crosses the IPC
/// boundary and says *what* a region could not take, and a box is no use to the
/// panel that renders that sentence. This is the placement fact
/// [`crate::redact::covered`] needs in order to decide whether to report it at
/// all, and it never leaves
/// the worker.
#[derive(Debug, Clone, PartialEq)]
pub struct FormOther {
    /// Its box in the **page's** own space, the form's matrix already applied.
    ///
    /// [`FormText::bounds`]'s convention exactly, and for that field's reason:
    /// PDFium answers in the form's space, and applying the matrix there rather
    /// than here keeps [`crate::redact::covered`] free of a second coordinate system.
    ///
    /// A child PDFium enumerated and would not hand over is unmeasurable, which
    /// overlaps every region --- so an object that cannot be placed cannot be
    /// excluded either, and the destructive direction stays the default.
    pub bounds: Rect,
    /// What it is, in [`crate::redact::Unhandled::kind`]'s vocabulary.
    pub kind: String,
}

/// A rectangle with its corners the way round the comparison below assumes.
///
/// PDFium reports `[left, bottom, right, top]`, and a caller's region may arrive
/// with either pair the other way --- a drag upwards and to the left produces
/// exactly that --- which would otherwise make an ordinary rectangle overlap
/// nothing at all and redact nothing while reporting success.
///
/// One function rather than the same `min`/`max` pair written out twice: two
/// near-copies is the shape that drifts, and it drifted here first. The mutation
/// aimed at one copy survived, because the test that was meant to catch it
/// reversed *both* axes and the other copy rescued it.
#[must_use]
fn normalised(rect: Rect) -> Rect {
    [
        rect[0].min(rect[2]),
        rect[1].min(rect[3]),
        rect[0].max(rect[2]),
        rect[1].max(rect[3]),
    ]
}

/// Whether `inner` lies wholly inside `outer`, edges included.
///
/// What decides whether a drawing is removed: [`crate::redact::covered`] takes
/// a path only when the region holds all of it, so nothing outside the region
/// goes with it.
#[must_use]
pub fn contains(outer: Rect, inner: Rect) -> bool {
    let outer = normalised(outer);
    let inner = normalised(inner);
    outer[0] <= inner[0] && inner[2] <= outer[2] && outer[1] <= inner[1] && inner[3] <= outer[3]
}

/// Whether two page-space rectangles share any area.
///
/// Strict comparisons throughout: two rectangles sharing only an edge do not
/// overlap, so a region drawn flush against a line of text does not eat it.
///
/// `pub` for [`crate::ocr::control_from_page`], which has to ask the same
/// question of the same page: which words a region covers decides what the
/// removal takes *and* which words are left to read a control back from. Two
/// answers to that would let the gate certify against a word the removal was
/// supposed to have taken.
#[must_use]
pub fn overlaps(a: Rect, b: Rect) -> bool {
    let a = normalised(a);
    let b = normalised(b);
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// One glyph of a text object: what it draws, where its ink is, and where the
/// pen stood when it was drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// What it draws. Usually one character; a ligature draws two or three.
    pub draws: String,
    /// Its tight box in page space, or `None` for a glyph with no ink --- a
    /// space is the common one.
    pub bounds: Option<Rect>,
    /// How far along the object's baseline the pen stood, in thousandths of
    /// the font size: the unit a `TJ` array's numbers are in.
    pub pen: f32,
}

/// The glyphs of one text object, in the order its show operator draws them.
///
/// What a removal needs in order to take **part** of a show operator and leave
/// the rest where it was: the glyphs under a region are cut out of the string
/// and replaced by a gap as wide as they were, and the width is the distance
/// between two pens.
#[derive(Debug, Clone, PartialEq)]
pub struct TextGlyphs {
    /// One entry per glyph.
    pub glyphs: Vec<Glyph>,
    /// Where the **next** text object starts, measured along this object's
    /// baseline in the same unit. It is where the pen stands after the last
    /// glyph when the next show operator follows without moving it, and
    /// meaningless otherwise; only the writer can tell which.
    pub tail: Option<f32>,
}

/// One character PDFium placed for a text object, before grouping into glyphs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    /// A UTF-16 code unit, as `FPDFText_GetUnicode` reports it.
    pub unit: u16,
    /// The pen position, in page space.
    pub origin: (f64, f64),
    /// The tight box, in page space: `[left, bottom, right, top]`.
    pub bounds: Option<[f64; 4]>,
}

/// How far a pen may step backwards and still count as moving forwards, in
/// thousandths of the font size. A negative kern between two glyphs is
/// ordinary; this is for the noise of a position that went through a matrix.
const PEN_SLACK: f32 = 0.5;

/// How far a pen may sit off the baseline, in thousandths of the font size.
/// Past it the object is not a row of glyphs along one line --- vertical
/// writing is the case --- and a gap along the baseline would not replace one.
const BASELINE_SLACK: f64 = 5.0;

/// Groups a text object's characters into glyphs and measures each pen.
///
/// `matrix` and `size` are the object's own, as PDFium reports them: the
/// matrix is the text matrix times the page's, **with the horizontal scaling
/// folded into its first row**, and the size is the operand of `Tf`. Measured
/// on nine combinations of `cm`, `Tm`, `Tf`, `Tz`, `Tc` and `Ts`, 2026-10-07:
/// one em along the baseline is `size * hypot(a, b)` page units in every one.
///
/// Characters that share an origin are one glyph: a ligature's letters, and
/// the two halves of a surrogate pair.
///
/// `None` when the object cannot be described this way, which a caller takes
/// as "remove it whole": a size or a matrix that is zero or not finite, a pen
/// off the baseline, or pens that do not run forwards (right-to-left text,
/// which PDFium hands over in reading order).
pub(crate) fn glyphs_from(
    placed: &[Placed],
    matrix: [f32; 6],
    size: f32,
    next_start: Option<(f32, f32)>,
) -> Option<TextGlyphs> {
    let (a, b) = (f64::from(matrix[0]), f64::from(matrix[1]));
    let along = a.hypot(b);
    let em = f64::from(size) * along;
    if !(em.is_finite() && em > 0.0 && along > 0.0) {
        return None;
    }
    let (ux, uy) = (a / along, b / along);
    let (ex, ey) = (f64::from(matrix[4]), f64::from(matrix[5]));
    // A point's place along the baseline and off it, in thousandths of an em.
    let measure = |x: f64, y: f64| {
        let (dx, dy) = (x - ex, y - ey);
        (
            (dx * ux + dy * uy) / em * 1000.0,
            (dy * ux - dx * uy) / em * 1000.0,
        )
    };

    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut units: Vec<u16> = Vec::new();
    let mut last: Option<(f64, f64)> = None;
    let mut off_line = 0f64;
    let close = |glyphs: &mut Vec<Glyph>, units: &mut Vec<u16>| {
        if let Some(open) = glyphs.last_mut() {
            open.draws = String::from_utf16_lossy(units);
        }
        units.clear();
    };
    for (index, one) in placed.iter().enumerate() {
        if last != Some(one.origin) {
            close(&mut glyphs, &mut units);
            let (pen, off) = measure(one.origin.0, one.origin.1);
            if index == 0 {
                off_line = off;
            }
            if !pen.is_finite() || (off - off_line).abs() > BASELINE_SLACK {
                return None;
            }
            let pen = pen as f32;
            if glyphs
                .last()
                .is_some_and(|before| pen < before.pen - PEN_SLACK)
            {
                return None;
            }
            glyphs.push(Glyph {
                draws: String::new(),
                bounds: None,
                pen,
            });
            last = Some(one.origin);
        }
        units.push(one.unit);
        let open = glyphs.last_mut()?;
        if let Some([left, bottom, right, top]) = one.bounds {
            let this = [left as f32, bottom as f32, right as f32, top as f32];
            open.bounds = Some(match open.bounds {
                Some(have) => [
                    have[0].min(this[0]),
                    have[1].min(this[1]),
                    have[2].max(this[2]),
                    have[3].max(this[3]),
                ],
                None => this,
            });
        }
    }
    close(&mut glyphs, &mut units);
    let end = glyphs.last()?.pen;
    let tail = next_start
        .map(|(x, y)| measure(f64::from(x), f64::from(y)).0 as f32)
        .filter(|tail| tail.is_finite() && *tail >= end - PEN_SLACK);
    Some(TextGlyphs { glyphs, tail })
}

/// A page's objects, and what the text ones draw.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageObjects {
    /// Every object, in PDFium's enumeration order.
    ///
    /// The order [`crate::redact::covered`] counts text ordinals in, and the
    /// order `redact::remove_shows` addresses show operators by. Nothing
    /// connects the two but that order --- see `redact.rs`, which refuses when
    /// the counts disagree rather than removing the wrong words.
    pub all: Vec<PageObject>,
    /// What each **text** object draws, in text-object order.
    ///
    /// Indexed by the same ordinals [`crate::redact::Plan::shows`] holds, so a
    /// caller can say what a removal would take without a second walk. Empty
    /// strings are ordinary: a text object PDFium places no characters against
    /// draws nothing this can read.
    pub text: Vec<String>,
    /// The glyphs of each of the **page's own** text objects, by the same
    /// ordinals as the first entries of [`text`](Self::text). `None` for an
    /// object [`glyphs_from`] cannot describe. Text inside a form has no entry:
    /// a removal there still takes the whole show operator.
    pub glyphs: Vec<Option<TextGlyphs>>,
    /// The Form XObjects on this page, and the text inside each.
    ///
    /// One entry per `form` object in [`all`](Self::all) --- always, even when the
    /// descent found nothing --- so a caller can tell a form this looked inside
    /// from a form it could not, and `redact::covered` can refuse the second.
    pub forms: Vec<FormObject>,
}

/// What PDFium calls an object of this type, for a refusal a person reads.
///
/// A string rather than an enum for `redact::PageObject::kind`'s reason: the
/// only questions asked of it are "is this text" and "what do I call it", and an
/// enum here would be a second vocabulary to keep in step with PDFium's.
fn kind_of(raw: i32) -> &'static str {
    match raw as u32 {
        FPDF_PAGEOBJ_TEXT => "text",
        FPDF_PAGEOBJ_PATH => "path",
        FPDF_PAGEOBJ_IMAGE => "image",
        FPDF_PAGEOBJ_SHADING => "shading",
        FPDF_PAGEOBJ_FORM => "form",
        _ => "unsupported",
    }
}

/// The box a rectangle gets when PDFium will not measure the object.
///
/// Everything, so it overlaps every region and is always reported. See the
/// module note: an object that cannot be placed cannot be excluded either.
const UNMEASURABLE: [f32; 4] = [f32::MIN, f32::MIN, f32::MAX, f32::MAX];

/// Reads a page's objects, and the text each text object draws.
///
/// # Errors
///
/// The page's text page not loading. A page with no objects on it is an empty
/// [`PageObjects`], not an error.
pub fn read(page: &RawPage<'_>) -> Result<PageObjects, String> {
    let text = RawTextPage::load(page)?;
    read_using(page, &text)
}

/// [`read`], for a caller that has already loaded the page's text.
///
/// Split for `structure::read_using`'s reason: loading a second text page means
/// PDFium building a second character index for the same page, which is work
/// proportional to the text on it for something already in hand.
pub fn read_using(page: &RawPage<'_>, text: &RawTextPage<'_>) -> Result<PageObjects, String> {
    let bindings = page.bindings();
    // SAFETY: the page handle is valid for the borrow.
    let count = unsafe { bindings.FPDFPage_CountObjects(page.handle()) };

    let mut all = Vec::new();
    // Text-object ordinal by object pointer, so the character walk below can put
    // each character on the right one. Built here rather than from a second
    // enumeration: two walks of the same list is the shape that drifts, and the
    // ordinals are the thing a removal addresses by.
    let mut ordinal_of: HashMap<usize, usize> = HashMap::new();
    let mut text_objects = 0usize;
    // The form objects, kept with their handles so the descent below is a second
    // pass rather than a nested one --- the page's own text has to occupy an
    // unbroken range of ordinals starting at zero, and interleaving the two
    // would put a form's children in the middle of it.
    let mut form_handles: Vec<(usize, FPDF_PAGEOBJECT)> = Vec::new();
    // The page's own text objects by ordinal, for the glyph walk at the end.
    let mut own_text: Vec<FPDF_PAGEOBJECT> = Vec::new();

    for index in 0..count.max(0) {
        // SAFETY: `index` is below the reported object count.
        let object = unsafe { bindings.FPDFPage_GetObject(page.handle(), index) };
        if object.is_null() {
            // Reported rather than dropped, and as unmeasurable rather than as
            // an error: an object PDFium enumerated and will not hand over is
            // exactly the case where dropping it silently under-redacts.
            all.push(PageObject {
                bounds: UNMEASURABLE,
                kind: "unsupported".to_string(),
            });
            continue;
        }
        // SAFETY: `object` is a valid page object owned by the page.
        let raw = unsafe { bindings.FPDFPageObj_GetType(object) };
        let kind = kind_of(raw);
        if kind == "text" {
            ordinal_of.insert(object as usize, text_objects);
            text_objects += 1;
            own_text.push(object);
        }
        if kind == "form" {
            form_handles.push((all.len(), object));
        }
        all.push(PageObject {
            bounds: bounds_of(page, object),
            kind: kind.to_string(),
        });
    }

    // The second pass. Every text child gets an ordinal above the page's own, so
    // one walk of the page's characters serves both -- which it can, because a
    // character inside a form is on the *page's* text page like every other.
    // Measured on `form-xobject.pdf`, whose 157 characters include both lines of
    // a form and one from a form nested inside a form, and whose
    // `FPDFText_GetTextObject` hands back the **inner** text object rather than
    // the form. That is what makes a pointer-keyed map work at all.
    let mut forms: Vec<FormObject> = Vec::new();
    let mut slot_of: Vec<(usize, usize)> = Vec::new();
    for (at, handle) in form_handles {
        let form = descend(
            page,
            handle,
            at,
            text_objects + slot_of.len(),
            &mut ordinal_of,
        );
        for ordinal in 0..form.text.len() {
            slot_of.push((forms.len(), ordinal));
        }
        forms.push(form);
    }

    let mut said = draws(page, text, &ordinal_of, text_objects + slot_of.len());
    let deep = said.split_off(text_objects.min(said.len()));
    for ((form, ordinal), drawn) in slot_of.into_iter().zip(deep) {
        forms[form].text[ordinal].draws = drawn;
    }

    let glyphs = glyphs_of(page, text, &ordinal_of, &own_text);
    Ok(PageObjects {
        text: said,
        glyphs,
        all,
        forms,
    })
}

/// The glyphs of each of the page's own text objects.
///
/// One walk of the page's characters, each put on its object by
/// `FPDFText_GetTextObject` as [`draws`] does. A character PDFium made up ---
/// the space it puts between two words that have none --- belongs to no show
/// operator and is left out.
fn glyphs_of(
    page: &RawPage<'_>,
    text: &RawTextPage<'_>,
    ordinal_of: &HashMap<usize, usize>,
    own_text: &[FPDF_PAGEOBJECT],
) -> Vec<Option<TextGlyphs>> {
    let bindings = page.bindings();
    let mut placed: Vec<Vec<Placed>> = vec![Vec::new(); own_text.len()];
    // An object with a character PDFium would not place cannot be cut.
    let mut unplaced = vec![false; own_text.len()];
    for index in 0..text.count() {
        // SAFETY: the text page outlives this loop and `index` is in range.
        let object = unsafe { bindings.FPDFText_GetTextObject(text.handle(), index as i32) };
        if object.is_null() {
            continue;
        }
        let Some(ordinal) = ordinal_of.get(&(object as usize)).copied() else {
            continue;
        };
        if ordinal >= placed.len() {
            // A form's child: its ordinals continue above the page's own.
            continue;
        }
        // SAFETY: as above.
        if unsafe { bindings.FPDFText_IsGenerated(text.handle(), index as i32) } != 0 {
            continue;
        }
        let (mut x, mut y) = (0f64, 0f64);
        // SAFETY: two writable doubles, and PDFium bounds-checks the index.
        let ok =
            unsafe { bindings.FPDFText_GetCharOrigin(text.handle(), index as i32, &mut x, &mut y) };
        let unit = u16::try_from(text.code(index));
        match (ok != 0 && x.is_finite() && y.is_finite(), unit) {
            (true, Ok(unit)) => placed[ordinal].push(Placed {
                unit,
                origin: (x, y),
                bounds: text.char_box(index),
            }),
            _ => unplaced[ordinal] = true,
        }
    }
    let starts: Vec<[f32; 6]> = own_text
        .iter()
        .map(|object| matrix_of(page, *object))
        .collect();
    own_text
        .iter()
        .enumerate()
        .map(|(ordinal, object)| {
            if unplaced[ordinal] {
                return None;
            }
            let mut size = 0f32;
            // SAFETY: a writable float, and `object` is a text object the page owns.
            let ok = unsafe { bindings.FPDFTextObj_GetFontSize(*object, &mut size) };
            if ok == 0 {
                return None;
            }
            let next = starts.get(ordinal + 1).map(|m| (m[4], m[5]));
            glyphs_from(&placed[ordinal], starts[ordinal], size, next)
        })
        .collect()
}

/// One Form XObject's text children, in page space.
///
/// **Descends exactly one level**, and says so: a child that is itself a form
/// goes into [`FormObject::unreachable`] rather than being followed. Following it
/// would need the matrices to compose and a removal to address a stream two deep,
/// and neither is measured --- reporting what cannot be reached is the answer
/// `docs/PLAN.md` §6 requires, and is what stops a region over a nested form
/// being certified.
///
/// `first` is the ordinal the first text child takes in `ordinal_of`, which
/// continues above the page's own so that [`draws`] can be called once.
fn descend(
    page: &RawPage<'_>,
    form: FPDF_PAGEOBJECT,
    at: usize,
    first: usize,
    ordinal_of: &mut HashMap<usize, usize>,
) -> FormObject {
    let bindings = page.bindings();
    // SAFETY: `form` is a valid page object of type FORM, owned by the page.
    let count = unsafe { bindings.FPDFFormObj_CountObjects(form) };
    let matrix = matrix_of(page, form);
    let mut out = FormObject {
        at,
        text: Vec::new(),
        unreachable: Vec::new(),
        images: Vec::new(),
        paths: Vec::new(),
    };
    for index in 0..count.max(0) {
        // `c_ulong` rather than a width: PDFium takes this index as `unsigned
        // long`, which is 64-bit on macOS and 32-bit on Windows, so `as u64`
        // compiles on one platform and not the other. `scripts/check_windows.py`
        // is what said so, in about eight seconds.
        //
        // SAFETY: `index` is below the reported child count.
        let child = unsafe { bindings.FPDFFormObj_GetObject(form, index as std::os::raw::c_ulong) };
        // A child PDFium enumerated and will not hand over is reported for the
        // same reason a page object is: dropping it silently under-redacts.
        let kind = if child.is_null() {
            "unsupported"
        } else {
            // SAFETY: `child` is a valid page object owned by the form.
            kind_of(unsafe { bindings.FPDFPageObj_GetType(child) })
        };
        if kind == "image" && !child.is_null() {
            // A picture is addressed by its place among the form's pictures,
            // as the form's text is by its place among the text.
            out.images.push(through(matrix, bounds_of(page, child)));
            continue;
        }
        if kind == "path" && !child.is_null() {
            // And a drawing by its place among the form's drawings.
            out.paths.push(through(matrix, bounds_of(page, child)));
            continue;
        }
        if kind != "text" {
            // Placed, and not only named. Every child can be measured --- a
            // nested form has a box of its own even though its contents are not
            // followed --- and without one, `redact::covered` had to report
            // every child of a form the region merely touched. A child that
            // cannot be handed over is unmeasurable, which overlaps everything
            // and is therefore still always reported.
            out.unreachable.push(FormOther {
                bounds: if child.is_null() {
                    UNMEASURABLE
                } else {
                    through(matrix, bounds_of(page, child))
                },
                kind: kind.to_string(),
            });
            continue;
        }
        ordinal_of.insert(child as usize, first + out.text.len());
        out.text.push(FormText {
            bounds: through(matrix, bounds_of(page, child)),
            draws: String::new(),
        });
    }
    out
}

/// A form object's matrix, or the identity when PDFium will not report one.
///
/// The identity is the value that changes nothing, which is the right fallback
/// here for the reason it is the wrong one elsewhere: an unplaced form's children
/// then land where the form's own bounds say the form is, so a region over it
/// still covers them. Getting this wrong in the other direction --- refusing ---
/// would turn an unreadable matrix into text nobody can remove.
fn matrix_of(page: &RawPage<'_>, form: FPDF_PAGEOBJECT) -> [f32; 6] {
    let mut m = FS_MATRIX {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };
    // SAFETY: a writable matrix, and `form` is valid for the page's borrow.
    let ok = unsafe { page.bindings().FPDFPageObj_GetMatrix(form, &mut m) };
    let read = [m.a, m.b, m.c, m.d, m.e, m.f];
    if ok == 0 || !read.iter().all(|v| v.is_finite()) {
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    read
}

/// A rectangle in a form's space, brought into the page's.
///
/// **Not optional and not obvious**: PDFium reports a form child's bounds in the
/// *form's* own space. Measured on `form-xobject.pdf`, where a form placed at
/// (60, 600) reports a child at (0.9, 19.9) --- so a region compared against the
/// untransformed box covers nothing, and a redaction over that line removes
/// nothing while reporting success.
///
/// All four corners, because a matrix may rotate or skew and two corners then
/// describe a different rectangle from the one the glyphs occupy.
fn through(m: [f32; 6], rect: [f32; 4]) -> [f32; 4] {
    if rect == UNMEASURABLE {
        return UNMEASURABLE;
    }
    let [a, b, c, d, e, f] = m;
    let corners = [
        (rect[0], rect[1]),
        (rect[2], rect[1]),
        (rect[0], rect[3]),
        (rect[2], rect[3]),
    ];
    let mut xs = [0f32; 4];
    let mut ys = [0f32; 4];
    for (i, (x, y)) in corners.into_iter().enumerate() {
        xs[i] = a * x + c * y + e;
        ys[i] = b * x + d * y + f;
    }
    [
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    ]
}

/// One object's bounding box in the page's own space, y upwards.
///
/// PDFium's own convention, which is what `redact::Rect` documents itself as
/// being --- so nothing is converted on the way in, and the region a caller
/// compares against has to be brought into *this* space rather than the other
/// way round.
fn bounds_of(page: &RawPage<'_>, object: FPDF_PAGEOBJECT) -> [f32; 4] {
    let (mut left, mut bottom, mut right, mut top) = (0f32, 0f32, 0f32, 0f32);
    // SAFETY: four writable floats, and `object` is valid for the page's borrow.
    let ok = unsafe {
        page.bindings().FPDFPageObj_GetBounds(
            object,
            &mut left as *mut f32,
            &mut bottom as *mut f32,
            &mut right as *mut f32,
            &mut top as *mut f32,
        )
    };
    let measured = [left, bottom, right, top];
    if ok == 0 || !measured.iter().all(|value| value.is_finite()) {
        // A non-finite bound is refused for `progressive::normalised`'s reason:
        // it poisons every comparison it reaches, and an object that overlaps
        // nothing is an object a removal steps over.
        return UNMEASURABLE;
    }
    measured
}

/// What each text object draws, in text-object order.
///
/// Characters whose object is not in the map are dropped, and that is not a
/// silent loss: they belong to a Form XObject, whose own entry in
/// [`PageObjects::all`] is of kind `form` and which `redact::covered` reports as
/// unhandled the moment a region touches it.
fn draws(
    page: &RawPage<'_>,
    text: &RawTextPage<'_>,
    ordinal_of: &HashMap<usize, usize>,
    text_objects: usize,
) -> Vec<String> {
    if text_objects == 0 {
        return Vec::new();
    }
    let mut units: Vec<Vec<u16>> = vec![Vec::new(); text_objects];
    // The page's bindings, which is the same library `structure.rs` calls
    // `FPDFText_GetTextObject` through: a text page carries no bindings of its
    // own and a second handle to the library would be a second thing to keep in
    // step with the document's.
    let bindings = page.bindings();
    for index in 0..text.count() {
        // SAFETY: the text page outlives this loop and `index` is in range.
        let object = unsafe { bindings.FPDFText_GetTextObject(text.handle(), index as i32) };
        if object.is_null() {
            continue;
        }
        let Some(ordinal) = ordinal_of.get(&(object as usize)) else {
            continue;
        };
        let Some(into) = units.get_mut(*ordinal) else {
            continue;
        };
        // A UTF-16 code unit, not a scalar: `FPDFText_GetUnicode` is a UTF-16
        // API, so an astral character arrives as two indices. Collected as units
        // and decoded once, which is what keeps a surrogate pair one character
        // rather than two replacement marks. `docs/TRAPS.md` has the entry.
        let code = text.code(index);
        if let Ok(unit) = u16::try_from(code) {
            into.push(unit);
        }
    }
    units
        .into_iter()
        // Lossy for `outline::decode_title`'s reason: a lone surrogate from a
        // broken `/ToUnicode` must cost that one character, not the whole
        // object's text.
        .map(|wide| String::from_utf16_lossy(&wide))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_object_type_pdfium_reports_has_a_name() {
        // The names `redact::covered` puts in a refusal a person reads, and the
        // one case that matters is the last: an object type this build does not
        // know is `unsupported` rather than a panic or a silent `text`, and
        // calling it text is what would make a removal address the wrong show
        // operator.
        assert_eq!(kind_of(FPDF_PAGEOBJ_TEXT as i32), "text");
        assert_eq!(kind_of(FPDF_PAGEOBJ_PATH as i32), "path");
        assert_eq!(kind_of(FPDF_PAGEOBJ_IMAGE as i32), "image");
        assert_eq!(kind_of(FPDF_PAGEOBJ_SHADING as i32), "shading");
        assert_eq!(kind_of(FPDF_PAGEOBJ_FORM as i32), "form");
        assert_eq!(kind_of(0), "unsupported");
        assert_eq!(kind_of(99), "unsupported");
        assert_eq!(kind_of(-1), "unsupported");
    }

    /// Characters at `xs` along a horizontal baseline at y = 100.
    fn along(xs: &[f64]) -> Vec<Placed> {
        xs.iter()
            .enumerate()
            .map(|(at, x)| Placed {
                unit: b'a' as u16 + at as u16,
                origin: (*x, 100.0),
                bounds: Some([*x, 100.0, *x + 5.0, 108.0]),
            })
            .collect()
    }

    const PLAIN: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 20.0, 100.0];

    fn pens(text: &TextGlyphs) -> Vec<f32> {
        text.glyphs.iter().map(|glyph| glyph.pen.round()).collect()
    }

    /// The numbers are PDFium's for Helvetica `AB` at 12 points: `A` is 667
    /// thousandths wide, so `B` starts 8.004 points on.
    #[test]
    fn a_pen_is_measured_in_thousandths_of_the_font_size() {
        let text = glyphs_from(&along(&[20.0, 28.004]), PLAIN, 12.0, None).expect("placed");
        assert_eq!(pens(&text), [0.0, 667.0]);
        assert_eq!(text.glyphs[1].draws, "b");
        assert_eq!(text.glyphs[1].bounds, Some([28.004, 100.0, 33.004, 108.0]));
        assert_eq!(text.tail, None);
    }

    /// Each row is one measurement of 2026-10-07: the matrix PDFium reported
    /// and the distance it put between `A` and `B`. The pen is 667 in every
    /// one, because that is what the font says and nothing else changed.
    #[test]
    fn the_matrix_and_the_size_take_the_scale_out_of_a_pen() {
        let cases: [(&str, [f32; 6], f32, f64); 4] = [
            ("Tz 50", [0.5, 0.0, 0.0, 1.0, 20.0, 100.0], 12.0, 4.002),
            ("cm 2", [2.0, 0.0, 0.0, 2.0, 20.0, 100.0], 12.0, 16.008),
            ("Tm 3, Tf 4", [3.0, 0.0, 0.0, 3.0, 20.0, 100.0], 4.0, 8.004),
            (
                "Tm stretched 2",
                [2.0, 0.0, 0.0, 1.0, 20.0, 100.0],
                12.0,
                16.008,
            ),
        ];
        for (what, matrix, size, step) in cases {
            let text = glyphs_from(&along(&[20.0, 20.0 + step]), matrix, size, None).expect(what);
            assert_eq!(pens(&text), [0.0, 667.0], "{what}");
        }
    }

    #[test]
    fn a_turned_baseline_is_measured_along_itself() {
        // Thirty degrees: the second pen is 8.004 points along (0.866, 0.5).
        let turned = [0.866, 0.5, -0.5, 0.866, 40.0, 60.0];
        let placed = [
            Placed {
                unit: b'A' as u16,
                origin: (40.0, 60.0),
                bounds: None,
            },
            Placed {
                unit: b'B' as u16,
                origin: (40.0 + 8.004 * 0.866, 60.0 + 8.004 * 0.5),
                bounds: None,
            },
        ];
        let text = glyphs_from(&placed, turned, 12.0, None).expect("placed");
        assert_eq!(pens(&text), [0.0, 667.0]);
    }

    /// A ligature is one code and two characters, and both stand at one pen.
    #[test]
    fn characters_at_one_pen_are_one_glyph() {
        let mut placed = along(&[20.0, 30.0, 30.0, 45.0]);
        placed[2].bounds = Some([32.0, 98.0, 40.0, 110.0]);
        let text = glyphs_from(&placed, PLAIN, 10.0, None).expect("placed");
        assert_eq!(pens(&text), [0.0, 1000.0, 2500.0]);
        assert_eq!(text.glyphs[1].draws, "bc");
        assert_eq!(
            text.glyphs[1].bounds,
            Some([30.0, 98.0, 40.0, 110.0]),
            "the box of both"
        );
    }

    #[test]
    fn the_tail_is_where_the_next_object_starts_along_this_baseline() {
        let text =
            glyphs_from(&along(&[20.0, 30.0]), PLAIN, 10.0, Some((42.0, 100.0))).expect("placed");
        assert_eq!(text.tail.map(f32::round), Some(2200.0));
        // A next object that starts behind the last pen is on another line,
        // and is no tail.
        let behind =
            glyphs_from(&along(&[20.0, 30.0]), PLAIN, 10.0, Some((20.0, 86.0))).expect("placed");
        assert_eq!(behind.tail, None);
    }

    /// Each of these goes whole when a region touches it, which is what
    /// `None` means to `redact::cut_within`.
    #[test]
    fn an_object_that_is_not_a_row_of_glyphs_running_forwards_has_no_glyphs() {
        let row = along(&[20.0, 30.0, 40.0]);
        assert!(
            glyphs_from(&row, PLAIN, 10.0, None).is_some(),
            "the control"
        );

        // Right-to-left text arrives in reading order, pens running back.
        assert!(glyphs_from(&along(&[40.0, 30.0, 20.0]), PLAIN, 10.0, None).is_none());
        // A negative kern steps back by less than the slack allows: still a row.
        assert!(glyphs_from(&along(&[20.0, 30.0, 29.996]), PLAIN, 10.0, None).is_some());

        // A pen off the baseline: vertical writing.
        let mut down = row.clone();
        down[2].origin = (40.0, 90.0);
        assert!(glyphs_from(&down, PLAIN, 10.0, None).is_none());

        assert!(glyphs_from(&row, PLAIN, 0.0, None).is_none(), "no size");
        assert!(
            glyphs_from(&row, PLAIN, f32::NAN, None).is_none(),
            "no size"
        );
        let flat = [0.0, 0.0, 0.0, 1.0, 20.0, 100.0];
        assert!(glyphs_from(&row, flat, 10.0, None).is_none(), "no baseline");
        assert!(
            glyphs_from(&[], PLAIN, 10.0, None).is_none(),
            "no characters"
        );
    }

    #[test]
    fn an_unmeasurable_object_overlaps_every_region() {
        // The property the fallback exists for, asserted against `redact`'s own
        // overlap rule rather than by reading the constant back. An object
        // PDFium will not place has to be reported for every region, or a
        // removal steps over whatever it could not see.
        let objects = [PageObject {
            bounds: UNMEASURABLE,
            kind: "unsupported".to_string(),
        }];
        for region in [
            [0.0, 0.0, 1.0, 1.0],
            [500.0, 700.0, 560.0, 720.0],
            [-100.0, -100.0, -90.0, -90.0],
        ] {
            let plan = crate::redact::covered(&objects, &[], region);
            assert!(!plan.is_complete(), "region {region:?} reported complete");
        }
    }

    #[test]
    fn a_translated_form_moves_its_text_by_exactly_the_translation() {
        // The measurement this exists for: on `form-xobject.pdf` a form placed
        // at (60, 600) reports a child at (0.9, 19.9), and comparing a region
        // against the untransformed box covers nothing at all.
        let moved = through([1.0, 0.0, 0.0, 1.0, 60.0, 600.0], [0.9, 19.9, 250.0, 28.7]);
        assert_eq!(moved, [60.9, 619.9, 310.0, 628.7]);
    }

    #[test]
    fn the_identity_leaves_a_rectangle_where_it_was() {
        // The fallback's own property, and the control for the test above: a
        // form PDFium reports no matrix for must not move its children.
        let rect = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(through([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], rect), rect);
    }

    #[test]
    fn a_scaled_form_scales_its_text() {
        assert_eq!(
            through([2.0, 0.0, 0.0, 3.0, 0.0, 0.0], [1.0, 1.0, 2.0, 2.0]),
            [2.0, 3.0, 4.0, 6.0]
        );
    }

    #[test]
    fn a_rotated_form_needs_all_four_corners() {
        // A quarter turn: (x, y) -> (-y, x). Taking two corners would give
        // [-1, 1, -2, 2], whose left is greater than its right -- a rectangle
        // that overlaps nothing, so a redaction over it would remove nothing and
        // report success.
        let turned = through([0.0, 1.0, -1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 2.0, 2.0]);
        assert_eq!(turned, [-2.0, 1.0, -1.0, 2.0]);
        assert!(turned[0] < turned[2] && turned[1] < turned[3]);
    }

    #[test]
    fn an_unmeasurable_child_stays_unmeasurable() {
        // Through a matrix it would become garbage -- `f32::MIN * 2` is
        // infinite -- and an object that overlaps everything has to go on
        // overlapping everything, which is what makes it reported rather than
        // stepped over.
        assert_eq!(
            through([2.0, 0.0, 0.0, 2.0, 5.0, 5.0], UNMEASURABLE),
            UNMEASURABLE
        );
    }
}
