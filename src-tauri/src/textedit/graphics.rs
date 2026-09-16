//! Accept only graphics-state entries that retain opaque, normally blended text.
//! Print overprint and stroke adjustment are preserved, not simulated here.
//! No fonts, active masks, transfer functions or other rendering effects are admitted.

use super::{dictionary, number};
use lopdf::{Dictionary, Document, Object};

#[cfg(test)]
mod tests;

/// Stroke-only parameters cannot affect the supported fill-only text (Tr = 0).
/// Preserve the operators, including q/Q scoping, instead of recreating paths.
/// Dash arrays follow PDF 1.7 section 4.3.2; the 32-entry limit bounds validation.
pub(super) fn stroke(operator: &str, values: &[Object]) -> Result<(), String> {
    let invalid = || "unsupported line stroke state".to_string();
    match (operator, values) {
        ("J" | "j", [Object::Integer(0..=2)]) => Ok(()),
        ("M", [value]) if number(value)? >= 1. => Ok(()),
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
            Ok(())
        }
        _ => Err(invalid()),
    }
}

pub(super) fn normal(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<(), String> {
    let invalid = || "unsupported external text graphics state".to_string();
    let states = dictionary(doc, resources.get(b"ExtGState").map_err(|_| invalid())?)?;
    let state = dictionary(doc, states.get(name).map_err(|_| invalid())?)?;
    for (key, value) in state {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"ExtGState" => {}
            (b"BM", Object::Name(name)) if name == b"Normal" => {}
            (b"ca" | b"CA", value) if number(value)? == 1. => {}
            // ISO 32000-1, Table 58. Keep the dictionary verbatim: OP also
            // sets nonstroking overprint when op is absent. Do not materialize
            // defaults or split these entries into independently applied state.
            (b"OP" | b"op" | b"SA", Object::Boolean(_)) => {}
            (b"OPM", Object::Integer(0 | 1)) => {}
            (b"SMask", Object::Name(name)) if name == b"None" => {}
            (b"AIS", Object::Boolean(false)) => {}
            (b"RI", Object::Name(name)) => super::colors::intent(name)?,
            (b"LW", value) => super::clipping::line_width(value)?,
            (b"LC", value) => stroke("J", std::slice::from_ref(value))?,
            (b"LJ", value) => stroke("j", std::slice::from_ref(value))?,
            (b"ML", value) => stroke("M", std::slice::from_ref(value))?,
            _ => return Err(invalid()),
        }
    }
    Ok(())
}
