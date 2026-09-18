//! Validate the CFF metadata that ttf-parser intentionally skips. No PostScript
//! is executed; only literal font-embedding declarations are recognized.

use std::collections::{BTreeMap, BTreeSet};
const INVALID: &str = "unsupported CFF program metadata";

pub(super) fn index<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<Vec<&'a [u8]>> {
    let count = u16::from_be_bytes(bytes.get(*pos..*pos + 2)?.try_into().ok()?) as usize;
    *pos += 2;
    if count == 0 {
        return Some(Vec::new());
    }
    if count > 4096 {
        return None;
    }
    let size = usize::from(*bytes.get(*pos)?);
    *pos += 1;
    if !(1..=4).contains(&size) {
        return None;
    }
    let mut offsets = Vec::with_capacity(count + 1);
    for _ in 0..=count {
        let value = bytes
            .get(*pos..*pos + size)?
            .iter()
            .fold(0_usize, |n, b| n * 256 + usize::from(*b));
        *pos += size;
        offsets.push(value);
    }
    if offsets[0] != 1 || offsets.windows(2).any(|w| w[0] > w[1]) {
        return None;
    }
    let start = *pos;
    *pos = start.checked_add(offsets[count] - 1)?;
    bytes.get(..*pos)?;
    offsets
        .windows(2)
        .map(|w| bytes.get(start + w[0] - 1..start + w[1] - 1))
        .collect()
}

pub(super) fn number(data: &[u8], pos: &mut usize) -> Option<f64> {
    let first = *data.get(*pos)?;
    *pos += 1;
    let value = match first {
        32..=246 => f64::from(first) - 139.,
        247..=254 => {
            let second = *data.get(*pos)?;
            *pos += 1;
            if first <= 250 {
                f64::from(first - 247) * 256. + f64::from(second) + 108.
            } else {
                -(f64::from(first - 251) * 256. + f64::from(second) + 108.)
            }
        }
        28 => {
            let value = i16::from_be_bytes(data.get(*pos..*pos + 2)?.try_into().ok()?);
            *pos += 2;
            f64::from(value)
        }
        29 => {
            let value = i32::from_be_bytes(data.get(*pos..*pos + 4)?.try_into().ok()?);
            *pos += 4;
            f64::from(value)
        }
        30 => {
            let mut text = String::new();
            'real: loop {
                let byte = *data.get(*pos)?;
                *pos += 1;
                for (part, nibble) in [byte >> 4, byte & 15].into_iter().enumerate() {
                    match nibble {
                        0..=9 => text.push(char::from(b'0' + nibble)),
                        10 => text.push('.'),
                        11 => text.push('e'),
                        12 => text.push_str("e-"),
                        14 => text.push('-'),
                        15 => {
                            if part == 0 && byte & 15 != 15 {
                                return None;
                            }
                            break 'real;
                        }
                        _ => return None,
                    }
                    if text.len() > 64 {
                        return None;
                    }
                }
            }
            text.parse().ok()?
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

pub(super) type Dict = BTreeMap<u16, Vec<f64>>;

// A DICT's operators and operands, each operator at most once.
pub(super) fn dict(data: &[u8]) -> Option<Dict> {
    let mut result = BTreeMap::new();
    let mut operands = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        if data[pos] >= 28 && data[pos] != 31 && data[pos] != 255 {
            operands.push(number(data, &mut pos)?);
            if operands.len() > 48 {
                return None;
            }
            continue;
        }
        let mut op = u16::from(data[pos]);
        pos += 1;
        if op == 12 {
            op = 1200 + u16::from(*data.get(pos)?);
            pos += 1;
        }
        if result.insert(op, std::mem::take(&mut operands)).is_some() {
            return None;
        }
    }
    operands.is_empty().then_some(result)
}

// The Top DICT of a one-font program; `validate` or `cid::parse` checks it.
pub(super) fn top(bytes: &[u8]) -> Option<Dict> {
    let mut pos = usize::from(*bytes.get(2)?);
    index(bytes, &mut pos)?;
    let tops = index(bytes, &mut pos)?;
    dict(tops.first()?)
}

pub(super) fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 4
        || bytes[0] != 1
        || bytes[1] != 0
        || bytes[2] < 4
        || !(1..=4).contains(&bytes[3])
    {
        return Err(INVALID.into());
    }
    let mut pos = usize::from(bytes[2]);
    let names = index(bytes, &mut pos).ok_or(INVALID)?;
    let tops = index(bytes, &mut pos).ok_or(INVALID)?;
    let strings = index(bytes, &mut pos).ok_or(INVALID)?;
    if names.len() != 1
        || names[0].is_empty()
        || names[0].len() > 127
        || tops.len() != 1
        || tops[0].len() > 4096
    {
        return Err(INVALID.into());
    }
    let top = tops[0];
    let mut pos = 0;
    let mut operands = Vec::new();
    let mut seen = BTreeSet::new();
    while pos < top.len() {
        if top[pos] >= 28 {
            operands.push(number(top, &mut pos).ok_or(INVALID)?);
            if operands.len() > 48 {
                return Err(INVALID.into());
            }
            continue;
        }
        let mut op = u16::from(top[pos]);
        pos += 1;
        if op == 12 {
            op = 1200 + u16::from(*top.get(pos).ok_or(INVALID)?);
            pos += 1;
        }
        if !seen.insert(op) {
            return Err(INVALID.into());
        }
        let valid = match op {
            0..=4 | 13 | 15..=17 | 1200 | 1222 => {
                operands.len() == 1 && operands[0] >= 0. && operands[0].fract() == 0.
            }
            5 => operands.len() == 4,
            14 => !operands.is_empty() && operands.iter().all(|v| *v >= 0. && v.fract() == 0.),
            18 => operands.len() == 2 && operands.iter().all(|v| *v >= 0. && v.fract() == 0.),
            1201 => operands == [0.] || operands == [1.],
            1202..=1204 => operands.len() == 1,
            1205 | 1208 => operands == [0.], // Filled outlines, no stroke width.
            1206 => operands == [2.],        // Type 2 charstrings only.
            1207 => operands == [0.001, 0., 0., 0.001, 0., 0.],
            1221 if operands.len() == 1 && operands[0] >= 391. && operands[0].fract() == 0. => {
                let value = strings.get((operands[0] - 391.) as usize).ok_or(INVALID)?;
                permissions(value)?;
                true
            }
            _ => false, // No synthetic bases, CID fonts, blends or unknown semantics.
        };
        if !valid {
            return Err(INVALID.into());
        }
        operands.clear();
    }
    if !operands.is_empty() {
        return Err(INVALID.into());
    }
    Ok(())
}

pub(super) fn permissions(bytes: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| INVALID)?;
    let tokens: Vec<_> = text.split_ascii_whitespace().collect();
    if !(tokens.len() == 3
        || (tokens.len() == 6
            && tokens[3] == "/OrigFontType"
            && matches!(tokens[4], "/OpenType" | "/Type1")
            && tokens[5] == "def"))
        || tokens[0] != "/FSType"
        || tokens[2] != "def"
    {
        return Err(INVALID.into());
    }
    let rights: u16 = tokens[1].parse().map_err(|_| INVALID)?;
    if rights & !0x108 != 0 {
        return Err("embedded CFF font does not permit this editable use".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(top: &[u8]) -> Vec<u8> {
        assert!(top.len() < 255);
        let mut bytes = vec![1, 0, 4, 4, 0, 1, 1, 1, 2, b'A'];
        bytes.extend([0, 1, 1, 1, top.len() as u8 + 1]);
        bytes.extend(top);
        bytes.extend([0, 0]);
        bytes
    }

    #[test]
    fn textedit_cff_metadata_rejects_ambiguous_partial_and_nondefault_dicts() {
        for top in [
            vec![],
            vec![139, 12, 5],
            vec![141, 12, 6],
            vec![28, 0, 0, 12, 5],
            vec![29, 0, 0, 0, 2, 12, 6],
            vec![
                30, 0x0a, 0x00, 0x1f, 139, 139, 30, 0x0a, 0x00, 0x1f, 139, 139, 12, 7,
            ],
        ] {
            assert!(validate(&font(&top)).is_ok(), "{top:?}");
        }
        for top in [
            vec![139],
            vec![12],
            vec![139, 12, 5, 139, 12, 5],
            vec![140, 12, 5],
            vec![140, 12, 6],
            vec![139, 12, 20],
            vec![139, 12, 30],
            vec![139, 12, 7],
            vec![30, 0xdf],
            vec![30, 0x1b, 0xff],
            vec![255],
            vec![28, 0],
            vec![29, 0, 0, 0],
        ] {
            assert!(validate(&font(&top)).is_err(), "{top:?}");
        }
        let bytes = font(&[]);
        for len in 0..bytes.len() {
            assert!(validate(&bytes[..len]).is_err(), "prefix {len}");
        }
        let mut bytes = font(&[]);
        bytes[5] = 2; // More than one name, without a matching index.
        assert!(validate(&bytes).is_err());
        assert!(validate(&font(&[139; 49])).is_err());
        let mut pos = 0;
        assert!(index(&[0xff, 0xff], &mut pos).is_none());
        let mut pos = 0;
        assert!(index(&[0, 1, 1, 2, 1, 0], &mut pos).is_none());
    }

    #[test]
    fn textedit_cff_metadata_only_accepts_editable_literal_permissions() {
        for value in [
            "/FSType 0 def",
            "/FSType 8 def",
            "/FSType 256 def",
            "/FSType 264 def /OrigFontType /OpenType def",
            "/FSType 8 def /OrigFontType /Type1 def",
        ] {
            assert!(permissions(value.as_bytes()).is_ok());
        }
        for value in [
            "/FSType 2 def",
            "/FSType 4 def",
            "/FSType 512 def",
            "/FSType 65536 def",
            "/FSType -1 def",
            "/FSType 8.0 def",
            "",
            "/FSType",
            "/FSType 8 def evil",
            "/FSType 8 def /OrigFontType /Other def",
            "/FontMatrix [2 0 0 2 0 0] def",
        ] {
            assert!(permissions(value.as_bytes()).is_err(), "{value}");
        }
        assert!(permissions(&[255]).is_err());
    }
}
