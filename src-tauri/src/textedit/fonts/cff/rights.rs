//! The embedding rights of an installed OpenType font, carried into the bare
//! CID-keyed CFF its subset is embedded as (FontFile3 /CIDFontType0C).
//!
//! A bare CFF has no OS/2 table, so the `fsType` would be lost on the way into
//! the PDF. Acrobat Distiller keeps it as a Top DICT `PostScript` entry,
//! `/FSType N def /OrigFontType /OpenType def`, and that is what this writes and
//! what `profile::permissions` reads when the document is opened again.
//!
//! Adding a string and an operator moves everything after the String INDEX, so
//! each absolute offset in the program is moved with it: the four in the Top
//! DICT (charset, CharStrings, FDArray, FDSelect) and the Private offset in
//! each Font DICT. `subsetter` writes all five as five-byte integers, so they
//! are patched in place without changing any length; a program written any
//! other way is refused rather than re-laid out. Its input is the worker's own
//! subset of the installed font, and `cid::parse` reads the result back before
//! it is used (`fallback::Font::installed`).
use super::profile::{index, number};

const INVALID: &str = "unsupported CFF program for an installed font";

/// Top DICT operators whose operand is an absolute offset into the program.
const OFFSETS: [u16; 4] = [15, 17, 1236, 1237];
/// The Top DICT `PostScript` operator (12 21).
const POSTSCRIPT: u16 = 1221;
/// The Font DICT `Private` operator: size, then offset.
const PRIVATE: u16 = 18;

/// One DICT operator, the byte ranges of its operands, and the operator's own.
struct Entry {
    op: u16,
    operands: Vec<std::ops::Range<usize>>,
}

fn entries(data: &[u8]) -> Option<Vec<Entry>> {
    let mut result = Vec::new();
    let mut operands = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        if data[pos] >= 28 && data[pos] != 31 && data[pos] != 255 {
            let start = pos;
            number(data, &mut pos)?;
            operands.push(start..pos);
            continue;
        }
        let mut op = u16::from(data[pos]);
        pos += 1;
        if op == 12 {
            op = 1200 + u16::from(*data.get(pos)?);
            pos += 1;
        }
        result.push(Entry {
            op,
            operands: std::mem::take(&mut operands),
        });
    }
    operands.is_empty().then_some(result)
}

/// The value of a five-byte integer operand (`29` and four bytes).
fn five(data: &[u8], range: &std::ops::Range<usize>) -> Option<usize> {
    let bytes = data.get(range.clone())?;
    let [29, a, b, c, d] = *bytes else {
        return None;
    };
    usize::try_from(i32::from_be_bytes([a, b, c, d])).ok()
}

fn put_five(data: &mut [u8], range: &std::ops::Range<usize>, value: usize) -> Option<()> {
    let value = i32::try_from(value).ok()?;
    let target = data.get_mut(range.clone())?;
    target.copy_from_slice(&[[29].as_slice(), &value.to_be_bytes()].concat());
    Some(())
}

/// A CFF INDEX of `items` with the smallest offset size that holds it.
fn write_index(items: &[&[u8]]) -> Option<Vec<u8>> {
    let count = u16::try_from(items.len()).ok()?;
    let mut result = count.to_be_bytes().to_vec();
    if count == 0 {
        return Some(result);
    }
    let last = 1 + items.iter().map(|item| item.len()).sum::<usize>();
    let size = match last {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x1_0000..=0xff_ffff => 3,
        _ => 4,
    };
    result.push(size as u8);
    let mut offset = 1_usize;
    for length in std::iter::once(0).chain(items.iter().map(|item| item.len())) {
        offset += length;
        let bytes = u32::try_from(offset).ok()?.to_be_bytes();
        result.extend(&bytes[4 - size..]);
    }
    for item in items {
        result.extend(*item);
    }
    Some(result)
}

/// `cff` with a `PostScript` entry declaring `rights` as its `fsType`, every
/// absolute offset moved by what that adds. `None` rights (a font without an
/// OS/2 table, which declares no restriction) leave the program unchanged.
pub(in crate::textedit::fonts) fn with_rights(
    cff: &[u8],
    rights: Option<u16>,
) -> Result<Vec<u8>, String> {
    build(cff, rights).ok_or_else(|| INVALID.into())
}

fn build(cff: &[u8], rights: Option<u16>) -> Option<Vec<u8>> {
    if cff.len() < 4 || cff[0] != 1 || cff[2] < 4 {
        return None;
    }
    let mut pos = usize::from(cff[2]);
    let names = index(cff, &mut pos)?;
    let names_end = pos;
    let tops = index(cff, &mut pos)?;
    let strings = index(cff, &mut pos)?;
    let strings_end = pos;
    let ([_], [top]) = (names.as_slice(), tops.as_slice()) else {
        return None;
    };
    // The offsets are five-byte integers in `subsetter`'s output, patched in
    // place below; any other encoding would change a length and is refused.
    // What this does not check (an offset pointing somewhere odd, a missing
    // entry, a repeated operator) `cid::parse` refuses in the result.
    let mut offsets = Vec::new();
    for entry in entries(top)? {
        if entry.op == POSTSCRIPT {
            return None;
        }
        if OFFSETS.contains(&entry.op) {
            let [range] = entry.operands.as_slice() else {
                return None;
            };
            offsets.push((entry.op, range.clone(), five(top, range)?));
        }
    }
    let Some(rights) = rights else {
        return Some(cff.to_vec());
    };
    let declaration = format!("/FSType {rights} def /OrigFontType /OpenType def");
    let mut top = top.to_vec();
    top.push(29);
    top.extend(i32::try_from(391 + strings.len()).ok()?.to_be_bytes());
    top.extend([12, (POSTSCRIPT - 1200) as u8]);
    let mut all = strings.clone();
    all.push(declaration.as_bytes());
    let strings = write_index(&all)?;
    // Everything from the Global Subr INDEX on moves by this much; the Top
    // DICT's own length does not depend on it.
    let delta = names_end + write_index(&[&top])?.len() + strings.len() - strings_end;
    for (_, range, value) in &offsets {
        put_five(&mut top, range, value + delta)?;
    }
    let mut result = cff[..names_end].to_vec();
    result.extend(write_index(&[&top])?);
    result.extend(strings);
    result.extend(&cff[strings_end..]);
    // Each Font DICT names its Private dict by an absolute offset.
    let (_, _, fd_array) = offsets.iter().find(|(op, _, _)| *op == 1236)?;
    let mut at = fd_array + delta;
    let dicts = index(&result, &mut at)?
        .iter()
        .map(|dict| {
            let start = dict.as_ptr() as usize - result.as_ptr() as usize;
            start..start + dict.len()
        })
        .collect::<Vec<_>>();
    for dict in dicts {
        let private = entries(&result[dict.clone()])?
            .into_iter()
            .find(|entry| entry.op == PRIVATE)?;
        let [_, offset] = private.operands.as_slice() else {
            return None;
        };
        let offset = offset.start + dict.start..offset.end + dict.start;
        let value = five(&result, &offset)?;
        put_five(&mut result, &offset, value + delta)?;
    }
    Some(result)
}
