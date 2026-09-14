//! Accept only graphics-state entries that retain opaque, normally blended text.
//! No fonts, masks, transfer functions or other rendering effects are admitted.

use super::{dictionary, number};
use lopdf::{Dictionary, Document, Object};

#[cfg(test)]
mod tests;

pub(super) fn normal(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<(), String> {
    let invalid = || "unsupported external text graphics state".to_string();
    let states = dictionary(doc, resources.get(b"ExtGState").map_err(|_| invalid())?)?;
    let state = dictionary(doc, states.get(name).map_err(|_| invalid())?)?;
    for (key, value) in state {
        match (key.as_slice(), value) {
            (b"Type", Object::Name(name)) if name == b"ExtGState" => {}
            (b"BM", Object::Name(name)) if name == b"Normal" => {}
            (b"ca" | b"CA", value) if number(value)? == 1. => {}
            (b"RI", Object::Name(name)) => super::colors::intent(name)?,
            _ => return Err(invalid()),
        }
    }
    Ok(())
}
