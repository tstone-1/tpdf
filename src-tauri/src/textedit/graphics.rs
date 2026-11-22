//! Accept only graphics-state entries that keep text normally blended. Constant
//! alpha is admitted: an edit keeps the state, so a replacement is painted with
//! exactly the alpha of the text it replaces. Print overprint and stroke
//! adjustment are preserved, not simulated here. No fonts, blend modes, active
//! masks, transfer functions or other rendering effects are admitted.

use super::{dictionary, number};
use lopdf::{Dictionary, Document, Object};

#[cfg(test)]
mod tests;

/// Only the stroke settings changed by an operator or named ExtGState.
#[derive(Clone, Copy, Default)]
pub(super) struct StrokeSettings {
    pub width: Option<f64>,
    pub square_cap: Option<bool>,
    pub dashed: Option<bool>,
    pub miter_join: Option<bool>,
    pub miter_limit: Option<f64>,
}

/// Keep the geometry needed to bound joins while preserving every authored operator.
/// Dash arrays follow PDF 1.7 section 4.3.2; the 32-entry limit bounds validation.
pub(super) fn stroke(operator: &str, values: &[Object]) -> Result<StrokeSettings, String> {
    let invalid = || said!("unsupported line stroke state");
    let mut settings = StrokeSettings::default();
    match (operator, values) {
        ("J", [Object::Integer(cap @ 0..=2)]) => settings.square_cap = Some(*cap == 2),
        ("j", [Object::Integer(join @ 0..=2)]) => settings.miter_join = Some(*join == 0),
        ("M", [value]) if number(value)? >= 1. => settings.miter_limit = Some(number(value)?),
        ("d", [Object::Array(pattern), phase]) => {
            if pattern.len() > 32 || number(phase)? < 0. {
                return Err(invalid());
            }
            let mut positive = false;
            for value in pattern {
                let length = number(value)?;
                if length < 0. {
                    return Err(invalid());
                }
                positive |= length > 0.;
            }
            // Zero-length dashes are valid for dotted lines, but an all-zero
            // nonempty cycle cannot advance. An empty array restores solid lines.
            if !pattern.is_empty() && !positive {
                return Err(invalid());
            }
            settings.dashed = Some(!pattern.is_empty());
        }
        _ => return Err(invalid()),
    }
    Ok(settings)
}

/// ISO 32000-1 Table 58 and 10.6.2-10.6.3: flatness (`i`, `FL`) and smoothness
/// (`SM`) are device tolerances for approximating curves and shadings. They
/// choose no colour, coverage or geometry, and the same renderer draws the
/// preview and the saved copy, so they are preserved rather than simulated.
pub(super) fn tolerance(key: &[u8], value: &Object) -> Result<(), String> {
    let value = number(value)?;
    let limit = if key == b"SM" { 1. } else { 100. };
    if !(0. ..=limit).contains(&value) {
        return Err(said!("unsupported rendering tolerance"));
    }
    Ok(())
}

/// Validate a named graphics state and return the stroke settings it changes.
pub(super) fn normal(
    doc: &Document,
    resources: &Dictionary,
    name: &[u8],
) -> Result<StrokeSettings, String> {
    let invalid = || said!("unsupported external text graphics state");
    let states = dictionary(doc, resources.get(b"ExtGState").map_err(|_| invalid())?)?;
    let state = dictionary(doc, states.get(name).map_err(|_| invalid())?)?;
    let mut settings = StrokeSettings::default();
    for (key, value) in state {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"ExtGState" => {}
            (b"BM", Object::Name(name)) if name == b"Normal" => {}
            (b"ca" | b"CA", value) if (0. ..=1.).contains(&number(value)?) => {}
            // ISO 32000-1, Table 58. Keep the dictionary verbatim: OP also
            // sets nonstroking overprint when op is absent. Do not materialize
            // defaults or split these entries into independently applied state.
            (b"OP" | b"op" | b"SA", Object::Boolean(_)) => {}
            (b"OPM", Object::Integer(0 | 1)) => {}
            (b"SMask", Object::Name(name)) if name == b"None" => {}
            (b"AIS", Object::Boolean(false)) => {}
            (b"RI", Object::Name(name)) => super::colors::intent(name)?,
            (b"LW", value) => {
                super::clipping::line_width(value)?;
                settings.width = Some(number(value)?);
            }
            (b"LC", value) => {
                settings.square_cap = stroke("J", std::slice::from_ref(value))?.square_cap
            }
            (b"LJ", value) => {
                settings.miter_join = stroke("j", std::slice::from_ref(value))?.miter_join
            }
            (b"ML", value) => {
                settings.miter_limit = stroke("M", std::slice::from_ref(value))?.miter_limit
            }
            (b"FL" | b"SM", value) => tolerance(key, value)?,
            _ => return Err(invalid()),
        }
    }
    Ok(settings)
}
