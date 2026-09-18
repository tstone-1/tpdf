//! CID-keyed CFF programs (FontFile3 /CIDFontType0C), what xdvipdfmx, LuaTeX
//! and Typst embed. Under Identity-H the PDF code is the CID; the program's
//! charset maps each CID to a glyph, FDSelect names the glyph's Private dict,
//! and a charstring's width is relative to that dict's nominalWidthX.
//! ttf-parser reads the charset and outlines of such a font, not its widths.
//! No PostScript is executed and only literal declarations are recognized.

use super::profile::{dict, index, permissions, Dict};
use crate::textedit::fonts::unicode::Glyph;
use std::collections::BTreeMap;
use ttf_parser::{cff::Table, GlyphId};

#[cfg(test)]
mod tests;

const INVALID: &str = "unsupported CID-keyed CFF program";
const MAX_FDS: usize = 256;
// Type 2 charstrings allow 48 operands (Adobe TN 5177, Appendix B).
const MAX_STACK: usize = 48;

pub(in crate::textedit::fonts) struct Program<'a> {
    table: Table<'a>,
    glyphs: BTreeMap<u16, GlyphId>,
    widths: Vec<Option<f64>>,
}

fn integer(value: f64, range: std::ops::RangeInclusive<f64>) -> Option<usize> {
    (value.fract() == 0. && range.contains(&value)).then_some(value as usize)
}

fn string<'a>(strings: &[&'a [u8]], sid: f64) -> Option<&'a [u8]> {
    // Adobe, Identity and permission strings are not standard strings.
    strings.get(integer(sid, 391.0..=65535.0)? - 391).copied()
}

// Top DICT keys of a CID-keyed font (Adobe TN 5176, Tables 9 and 10). Anything
// else, including a synthetic base, blends and Encoding, is refused.
fn top(top: &Dict, strings: &[&[u8]]) -> Option<()> {
    for (&op, values) in top {
        let valid = match op {
            0..=4 | 1200 | 1238 => values.len() == 1 && integer(values[0], 0.0..=65535.0).is_some(),
            5 => values.len() == 4,
            13 | 1231 | 1232 | 1235 => values.len() == 1,
            14 => !values.is_empty(),
            15 | 17 | 1236 | 1237 => {
                values.len() == 1 && integer(values[0], 1.0..=16777216.0).is_some()
            }
            1201 => values == &[0.] || values == &[1.],
            1202..=1204 => values.len() == 1,
            1205 | 1208 | 1233 => values == &[0.],
            1206 => values == &[2.],
            1207 => values == &[0.001, 0., 0., 0.001, 0., 0.],
            1221 => values.len() == 1 && permissions(string(strings, values[0])?).is_ok(),
            1230 => {
                values.len() == 3
                    && string(strings, values[0])? == b"Adobe"
                    && string(strings, values[1])? == b"Identity"
                    && values[2] == 0.
            }
            1234 => values.len() == 1 && integer(values[0], 1.0..=65536.0).is_some(),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    [15, 17, 1230, 1236, 1237]
        .iter()
        .all(|op| top.contains_key(op))
        .then_some(())
}

// One font dict and its Private dict: (defaultWidthX, nominalWidthX).
fn font_dict(bytes: &[u8], data: &[u8]) -> Option<(f64, f64)> {
    let fd = dict(data)?;
    for (&op, values) in &fd {
        let valid = match op {
            18 => values.len() == 2,
            1238 => values.len() == 1,
            // The Top DICT's 0.001 composed with this gives the glyph space.
            1207 => values == &[1., 0., 0., 1., 0., 0.],
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    let private = fd.get(&18)?;
    let size = integer(private[0], 0.0..=65535.0)?;
    let offset = integer(private[1], 1.0..=16777216.0)?;
    let private = dict(bytes.get(offset..offset.checked_add(size)?)?)?;
    for (&op, values) in &private {
        // Hints only, plus local subroutines and the two widths.
        let valid = match op {
            6..=9 | 1212 | 1213 => values.len() <= 28,
            10 | 11 | 20 | 21 | 1209..=1211 | 1218 | 1219 => values.len() == 1,
            19 => values.len() == 1 && integer(values[0], 1.0..=65535.0).is_some(),
            1214 | 1217 => values == &[0.] || values == &[1.],
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    let width = |op| {
        private
            .get(&op)
            .map_or(Some(0.), |v| v[0].is_finite().then_some(v[0]))
    };
    Some((width(20)?, width(21)?))
}

// Format 0 (one byte per glyph) or 3 (ranges), each naming an existing dict.
fn select(bytes: &[u8], offset: usize, glyphs: usize, dicts: usize) -> Option<Vec<usize>> {
    match *bytes.get(offset)? {
        0 => {
            let values = bytes.get(offset + 1..offset + 1 + glyphs)?;
            values
                .iter()
                .map(|&fd| (usize::from(fd) < dicts).then_some(usize::from(fd)))
                .collect()
        }
        3 => {
            let word = |at: usize| {
                bytes
                    .get(at..at + 2)
                    .map(|pair| usize::from(u16::from_be_bytes([pair[0], pair[1]])))
            };
            let ranges = word(offset + 1)?;
            if ranges == 0 || ranges > glyphs {
                return None;
            }
            let mut result = Vec::with_capacity(glyphs);
            for range in 0..ranges {
                let at = offset + 3 + range * 3;
                let (first, fd) = (word(at)?, usize::from(*bytes.get(at + 2)?));
                let end = word(at + 3)?; // The next first, or the sentinel.
                if first != result.len() || end <= first || fd >= dicts {
                    return None;
                }
                result.resize(end, fd);
            }
            (result.len() == glyphs).then_some(result)
        }
        _ => None,
    }
}

// The width a Type 2 charstring states before its first stack-clearing
// operator (TN 5177, 3.1): one operand more than the operator takes. A
// subroutine call before that point hides the width; such a glyph is not
// offered rather than interpreted.
pub(super) fn width(charstring: &[u8], (default, nominal): (f64, f64)) -> Option<f64> {
    let mut stack = 0_usize;
    let mut first = None;
    let mut pos = 0;
    loop {
        let byte = *charstring.get(pos)?;
        let (value, length) = match byte {
            28 => {
                let pair = charstring.get(pos + 1..pos + 3)?;
                (f64::from(i16::from_be_bytes([pair[0], pair[1]])), 3)
            }
            32..=246 => (f64::from(byte) - 139., 1),
            247..=250 => {
                let next = f64::from(*charstring.get(pos + 1)?);
                (f64::from(byte - 247) * 256. + next + 108., 2)
            }
            251..=254 => {
                let next = f64::from(*charstring.get(pos + 1)?);
                (-(f64::from(byte - 251) * 256.) - next - 108., 2)
            }
            255 => {
                let fixed = charstring.get(pos + 1..pos + 5)?;
                (
                    f64::from(i32::from_be_bytes([fixed[0], fixed[1], fixed[2], fixed[3]]))
                        / 65536.,
                    5,
                )
            }
            _ => {
                let has = match byte {
                    1 | 3 | 18 | 23 | 19 | 20 => stack % 2 == 1,
                    21 => stack == 3,
                    4 | 22 => stack == 2,
                    // endchar alone or with seac's four accent operands.
                    14 => stack == 1 || stack == 5,
                    _ => return None,
                };
                let expected = match byte {
                    21 => [2, 3].contains(&stack),
                    4 | 22 => [1, 2].contains(&stack),
                    14 => [0, 1, 4, 5].contains(&stack),
                    _ => true,
                };
                if !expected {
                    return None;
                }
                return Some(if has { nominal + first? } else { default });
            }
        };
        if stack == 0 {
            first = Some(value);
        }
        stack += 1;
        if stack > MAX_STACK {
            return None;
        }
        pos += length;
    }
}

pub(in crate::textedit::fonts) fn parse(bytes: &[u8]) -> Result<Program<'_>, String> {
    if bytes.len() < 4 || bytes[0] != 1 || bytes[2] < 4 || !(1..=4).contains(&bytes[3]) {
        return Err(INVALID.into());
    }
    let mut pos = usize::from(bytes[2]);
    let names = index(bytes, &mut pos).ok_or(INVALID)?;
    let tops = index(bytes, &mut pos).ok_or(INVALID)?;
    let strings = index(bytes, &mut pos).ok_or(INVALID)?;
    index(bytes, &mut pos).ok_or(INVALID)?; // Global subroutines, read by ttf-parser.
    if names.len() != 1 || names[0].is_empty() || names[0].len() > 127 || tops.len() != 1 {
        return Err(INVALID.into());
    }
    let dict_of = dict(tops[0]).ok_or(INVALID)?;
    top(&dict_of, &strings).ok_or(INVALID)?;
    let offset = |op: u16| dict_of[&op][0] as usize;
    let charstrings = index(bytes, &mut offset(17)).ok_or(INVALID)?;
    let fds = index(bytes, &mut offset(1236)).ok_or(INVALID)?;
    if charstrings.is_empty() || fds.is_empty() || fds.len() > MAX_FDS {
        return Err(INVALID.into());
    }
    let dicts = fds
        .iter()
        .map(|fd| font_dict(bytes, fd))
        .collect::<Option<Vec<_>>>()
        .ok_or(INVALID)?;
    let selected = select(bytes, offset(1237), charstrings.len(), dicts.len()).ok_or(INVALID)?;
    let table = Table::parse(bytes).ok_or(INVALID)?;
    if usize::from(table.number_of_glyphs()) != charstrings.len() {
        return Err(INVALID.into());
    }
    let count = dict_of.get(&1234).map_or(8720., |v| v[0]); // TN 5176's default.
    let mut glyphs = BTreeMap::new();
    for index in 0..table.number_of_glyphs() {
        // A charset never lists glyph 0, which is CID 0; a later glyph
        // claiming CID 0 is therefore a duplicate.
        let cid = table.glyph_cid(GlyphId(index)).ok_or(INVALID)?;
        if f64::from(cid) >= count || glyphs.insert(cid, GlyphId(index)).is_some() {
            return Err(INVALID.into());
        }
    }
    let widths = charstrings
        .iter()
        .zip(&selected)
        .map(|(charstring, &fd)| width(charstring, dicts[fd]))
        .collect();
    Ok(Program {
        table,
        glyphs,
        widths,
    })
}

impl Program<'_> {
    /// The glyph a CID selects; `None` when the program has no glyph for it or
    /// its width or outline cannot be established. xdvipdfmx's ToUnicode
    /// ranges also cover CIDs its subset left out. It maps CID 0, `.notdef`,
    /// to U+FFFF, and a page may show it (fontspec's manual shows six in a
    /// row), so that glyph is measured but only for read-only text.
    pub(in crate::textedit::fonts) fn glyph(&self, cid: u16) -> Result<Option<Glyph>, String> {
        let Some(&glyph) = self.glyphs.get(&cid) else {
            return Ok(None);
        };
        let Some(advance) = self.widths[usize::from(glyph.0)] else {
            return Ok(None);
        };
        Ok(
            match crate::textedit::fonts::outlines::cff_bounds(&self.table, glyph) {
                Ok(ink) => Some(Glyph {
                    id: glyph.0,
                    advance,
                    empty: ink.is_none(),
                    ink,
                    read_only: glyph.0 == 0,
                }),
                Err(_) => None,
            },
        )
    }
}
