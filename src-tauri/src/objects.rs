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
use crate::redact::{FormObject, FormOther, FormText, PageObject};
use crate::text::RawTextPage;

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

    Ok(PageObjects {
        text: said,
        all,
        forms,
    })
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
