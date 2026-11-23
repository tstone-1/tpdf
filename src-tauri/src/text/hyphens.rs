//! PDFium can replace a soft hyphen or a literal hyphen at the end of a
//! line with the same U+0002 marker. Read its text object by itself to see
//! which glyph the font maps there. Keep the original character indices.

use std::collections::{HashMap, HashSet};

use super::{RawPage, RawTextPage};

#[derive(Default)]
struct Marked {
    codes: Vec<u32>,
    /// (Index in this text object, scalar index in the displayed page).
    positions: Vec<(usize, usize)>,
}

pub(super) fn restore(
    page: &RawPage<'_>,
    original: &RawTextPage<'_>,
    positions: &[(u32, usize)],
    codes: &mut [u32],
) {
    if positions.is_empty() {
        return;
    }
    let bindings = page.bindings();
    // A text-edit preview has its own PDFium document over the edited bytes.
    // Only the separately loaded page changes here, including its active flags.
    page.with_fresh_objects(|fresh| {
        // SAFETY: every text page below is closed before this callback returns.
        unsafe {
            let Some(text) = RawTextPage::load_handle(bindings, fresh) else {
                return;
            };
            if text.count() != original.count() {
                return;
            }
            let mut marked = HashMap::<_, Marked>::new();
            for &(at, _) in positions {
                let object = bindings.FPDFText_GetTextObject(text.handle(), at as i32);
                if !object.is_null()
                    && text.is_hyphen(at)
                    && text.char_box(at) == original.char_box(at)
                {
                    marked.entry(object).or_default();
                }
            }
            let mut objects = HashSet::new();
            let mut breaks = positions.iter().copied().peekable();
            for index in 0..text.count() {
                let code = text.code(index);
                if code != original.code(index) {
                    return;
                }
                let object = bindings.FPDFText_GetTextObject(text.handle(), index as i32);
                let output = breaks.next_if(|(at, _)| *at == index);
                if !object.is_null() {
                    objects.insert(object);
                }
                if let Some(run) = marked.get_mut(&object) {
                    if let Some((_, output)) = output {
                        run.positions.push((run.codes.len(), output));
                    }
                    run.codes.push(code);
                }
            }
            drop(text);
            // Each independent reading visits the page's objects. Bound the
            // repeated walks; an unconfirmed marker keeps PDFium's reading.
            if objects.len().saturating_mul(marked.len()) > 1_000_000 {
                return;
            }
            for object in objects {
                if bindings.FPDFPageObj_SetIsActive(object, 0) == 0 {
                    return;
                }
            }
            for (object, run) in marked {
                if bindings.FPDFPageObj_SetIsActive(object, 1) == 0 {
                    return;
                }
                if let Some(single) = RawTextPage::load_handle(bindings, fresh) {
                    if single.count() as usize == run.codes.len() {
                        let mut recovered = Vec::new();
                        let mut breaks = run.positions.iter().copied().peekable();
                        for (i, &expected) in run.codes.iter().enumerate() {
                            let actual = single.code(i as u32);
                            if let Some((_, output)) = breaks.next_if(|(at, _)| *at == i) {
                                if matches!(actual, 0xad | 0x2d | 0x2010) {
                                    recovered.push((output, actual));
                                    continue;
                                }
                            }
                            if actual != expected {
                                recovered.clear();
                                break;
                            }
                        }
                        for (output, actual) in recovered {
                            codes[output] = actual;
                        }
                    }
                }
                if bindings.FPDFPageObj_SetIsActive(object, 0) == 0 {
                    return;
                }
            }
        }
    });
}
