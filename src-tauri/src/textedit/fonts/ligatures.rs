//! Exact ligature Unicode sequences and their CFF glyph names. These low metric slots are
//! private: no control character is accepted as input or emitted as PDF text.

// Source measurement never uses this choice: it reads the original PDF codes.
pub(super) const GLYPHS: [(&str, &str, u8); 5] = [
    ("f_f_i", "ffi", 1),
    ("f_f_l", "ffl", 5),
    ("f_f", "ff", 2),
    ("f_i", "fi", 3),
    ("f_l", "fl", 4),
];

pub(super) fn text(slot: u8) -> Option<&'static str> {
    GLYPHS
        .iter()
        .find(|(_, _, code)| *code == slot)
        .map(|(_, text, _)| *text)
}

pub(super) fn target(bytes: &[u8]) -> Option<u8> {
    GLYPHS
        .iter()
        .find(|(_, text, _)| {
            bytes
                .iter()
                .copied()
                .eq(text.bytes().flat_map(|ch| [0, ch]))
        })
        .map(|(_, _, slot)| *slot)
}

/// Choose the longest glyph at each character boundary that can still encode
/// the suffix. Each font maps at most three characters to one glyph. A failed
/// suffix remembers the missing character on the longest attempted spelling
/// so the caller can distinguish a missing glyph from an ambiguous one.
pub(super) fn encode<Code: Copy>(
    count: usize,
    glyph: impl Fn(std::ops::Range<usize>) -> Option<Code>,
) -> Result<Vec<Code>, usize> {
    // The empty suffix at count needs no glyph. Every other success records
    // the first glyph and the boundary after it.
    let mut steps = vec![Ok(None); count + 1];
    for start in (0..count).rev() {
        let mut step = Err(start);
        for (offset, &suffix) in steps[start + 1..=(start + 3).min(count)].iter().enumerate() {
            let end = start + 1 + offset;
            if let Some(code) = glyph(start..end) {
                match suffix {
                    Ok(_) => step = Ok(Some((code, end))),
                    Err(missing) if step.is_err() => step = Err(missing),
                    _ => {}
                }
            }
        }
        steps[start] = step;
    }
    let mut output = Vec::with_capacity(count);
    let mut start = 0;
    while let Some((code, end)) = steps[start]? {
        output.push(code);
        start = end;
    }
    Ok(output)
}
