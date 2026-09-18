//! Keep a TJ run's own kerning and word gaps where a replacement leaves the
//! text unchanged. Rewriting the whole run drops every kern, which widens a
//! kerned line (pdfTeX kerns most of them) and refuses same-length edits.
//!
//! The source array is cut into glyphs and displacements. The longest prefix
//! and suffix whose text the replacement repeats keep their original items
//! byte for byte; only the middle is encoded afresh. A kern between a kept
//! glyph and a changed one is dropped, since it belonged to the old pair. The
//! result is read back by the same parser the scan uses and is only used when
//! it reads as exactly the replacement; otherwise the caller rewrites the run.

use super::{array_text, fonts, number, GAP_EM};
use lopdf::Object;

#[cfg(test)]
mod tests;

enum Atom<'a> {
    Glyph(&'a [u8], String),
    // A displacement, and whether it reads as a word space.
    Shift(&'a Object, bool),
}

impl Atom<'_> {
    fn text(&self) -> &str {
        match self {
            Atom::Glyph(_, text) => text,
            Atom::Shift(_, true) => " ",
            Atom::Shift(_, false) => "",
        }
    }
}

fn atoms<'a>(values: &'a [Object], metrics: &fonts::Metrics) -> Option<Vec<Atom<'a>>> {
    let width = metrics.code_len();
    let gap_spaces = !metrics.writes_space();
    let mut atoms = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match value {
            Object::String(bytes, _) => {
                if bytes.len() % width != 0 {
                    return None;
                }
                for code in bytes.chunks_exact(width) {
                    atoms.push(Atom::Glyph(code, metrics.decode(code).ok()?));
                }
            }
            // Mirrors array_text: one displacement between two strings reads as
            // a space. A run of several numbers is left to the fallback.
            value => {
                let shift = -number(value).ok()?;
                let between = index > 0
                    && matches!(values[index - 1], Object::String(..))
                    && matches!(values.get(index + 1), Some(Object::String(..)));
                if !between && values.get(index + 1).is_some() {
                    return None;
                }
                atoms.push(Atom::Shift(value, gap_spaces && between && shift >= GAP_EM));
            }
        }
    }
    Some(atoms)
}

// Items for a run of kept atoms, merging adjacent glyphs into one string.
fn emit(atoms: &[Atom<'_>], items: &mut Vec<Object>) {
    for atom in atoms {
        match atom {
            Atom::Glyph(bytes, _) => match items.last_mut() {
                Some(Object::String(string, _)) => string.extend_from_slice(bytes),
                _ => items.push(Object::string_literal(bytes.to_vec())),
            },
            Atom::Shift(value, _) => items.push((*value).clone()),
        }
    }
}

/// The TJ items (without any leading adjustment) for `replacement`, keeping the
/// source's unchanged ends, with the advance and ink array_text measures for
/// them. `values` is the source array after its leading adjustment.
pub(super) fn kept(
    values: &[Object],
    replacement: &str,
    metrics: &fonts::Metrics,
    gap: f64,
    (size, spacing, word_spacing): (f64, f64, f64),
) -> Option<(Vec<Object>, f64, [f64; 2])> {
    let atoms = atoms(values, metrics)?;
    // Prefix: whole atoms whose text the replacement starts with. A kern is
    // kept only when the glyph after it is kept too.
    let mut prefix = 0;
    let mut prefix_chars = 0;
    let mut matched = 0;
    for (index, atom) in atoms.iter().enumerate() {
        let text = atom.text();
        if !replacement[matched..].starts_with(text) {
            break;
        }
        matched += text.len();
        if !text.is_empty() {
            prefix = index + 1;
            prefix_chars = matched;
        }
    }
    let mut suffix = atoms.len();
    let mut suffix_chars = 0;
    let mut matched = 0;
    for (index, atom) in atoms.iter().enumerate().rev() {
        let text = atom.text();
        let rest = &replacement[..replacement.len() - matched];
        if index < prefix
            || !rest.ends_with(text)
            || prefix_chars + matched + text.len() > replacement.len()
        {
            break;
        }
        matched += text.len();
        if !text.is_empty() {
            suffix = index;
            suffix_chars = matched;
        }
    }
    if prefix == 0 && suffix == atoms.len() {
        return None; // Nothing kept: the ordinary rewrite is the same thing.
    }
    let middle = &replacement[prefix_chars..replacement.len() - suffix_chars];
    let mut items = Vec::new();
    emit(&atoms[..prefix], &mut items);
    // A middle that begins or ends at a word boundary carries its spaces as
    // gaps in a font that writes none.
    let (lead, core, trail) = if metrics.writes_space() {
        ("", middle, "")
    } else {
        let core = middle.trim_matches(' ');
        let start = middle.len() - middle.trim_start_matches(' ').len();
        (&middle[..start], core, &middle[start + core.len()..])
    };
    for _ in lead.chars() {
        items.push(Object::Real(-gap as f32));
    }
    if !core.is_empty() {
        for item in metrics.items(core, gap).ok()? {
            match (items.last_mut(), item) {
                (Some(Object::String(string, _)), Object::String(bytes, _)) => {
                    string.extend_from_slice(&bytes)
                }
                (_, item) => items.push(item),
            }
        }
    }
    for _ in trail.chars() {
        items.push(Object::Real(-gap as f32));
    }
    let mut tail = Vec::new();
    emit(&atoms[suffix..], &mut tail);
    for item in tail {
        match (items.last_mut(), item) {
            (Some(Object::String(string, _)), Object::String(bytes, _)) => {
                string.extend_from_slice(&bytes)
            }
            (_, item) => items.push(item),
        }
    }
    // array_text wants a string first; a replacement that deletes the whole
    // kept start would begin with a displacement.
    if !matches!(items.first(), Some(Object::String(..))) {
        return None;
    }
    let (text, advance, bounds, lead, _) =
        array_text(&items, metrics, size, spacing, word_spacing).ok()?;
    (lead.is_none() && text == replacement).then_some((items, advance, bounds))
}
