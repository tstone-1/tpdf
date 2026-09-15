//! Exact ligature Unicode sequences and their CFF glyph names. These low metric slots are
//! private: no control character is accepted as input or emitted as PDF text.

// Longest sequence first gives replacement encoding a deterministic choice.
// Source measurement never uses this choice: it reads the original PDF codes.
pub(super) const GLYPHS: [(&str, &str, u8); 4] = [
    ("f_f_i", "ffi", 1),
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
