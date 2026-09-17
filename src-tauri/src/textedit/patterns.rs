//! Preserve bounded axial shading artwork, never use a pattern to edit text.
//! ISO 32000-1 8.7.4: a type-2 pattern contains a shading dictionary, not a
//! content stream. Restrict its function to a nonrecursive type-2 interpolation.
use super::{colors, dictionary, number};
use lopdf::{Dictionary, Document, Object};

const INVALID: &str = "unsupported shading pattern on editable page";

#[derive(Clone, Copy)]
pub(super) enum Colour {
    Solid(usize),
    Pattern { selected: bool },
}

impl Colour {
    pub(super) fn set(
        &mut self,
        doc: &Document,
        resources: &Dictionary,
        operator: &str,
        values: &[Object],
        checked: &mut std::collections::BTreeSet<Vec<u8>>,
    ) -> Result<(), String> {
        match self {
            Self::Solid(count) => colors::values(values, *count),
            Self::Pattern { selected } => {
                let [Object::Name(name)] = values else {
                    return Err(INVALID.into());
                };
                if !matches!(operator, "scn" | "SCN") {
                    return Err(INVALID.into());
                }
                if !checked.contains(name) {
                    if checked.len() >= 32 {
                        return Err("too many shading patterns".into());
                    }
                    check(doc, resources, name)?;
                    checked.insert(name.clone());
                }
                *selected = true;
                Ok(())
            }
        }
    }

    fn ready(self) -> Result<(), String> {
        if matches!(self, Self::Pattern { selected: false }) {
            Err("painting with an unselected shading pattern".into())
        } else {
            Ok(())
        }
    }
}

pub(super) fn paint(operator: &str, fill: Colour, stroke: Colour) -> Result<(), String> {
    if matches!(operator, "f" | "F" | "f*" | "B" | "B*" | "b" | "b*") {
        fill.ready()?;
    }
    if matches!(operator, "S" | "s" | "B" | "B*" | "b" | "b*") {
        stroke.ready()?;
    }
    Ok(())
}

fn keys(dict: &Dictionary, allowed: &[&[u8]]) -> Result<(), String> {
    if dict
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn numbers<const N: usize>(dict: &Dictionary, key: &[u8]) -> Result<[f64; N], String> {
    let values = dict
        .get(key)
        .and_then(Object::as_array)
        .map_err(|_| INVALID)?;
    if values.len() != N {
        return Err(INVALID.into());
    }
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) {
        *out = number(value)?;
    }
    Ok(result)
}

fn check(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<(), String> {
    let patterns = dictionary(doc, resources.get(b"Pattern").map_err(|_| INVALID)?)?;
    let pattern = dictionary(doc, patterns.get(name).map_err(|_| INVALID)?)?;
    keys(pattern, &[b"Type", b"PatternType", b"Shading", b"Matrix"])?;
    if pattern.get(b"PatternType").and_then(Object::as_i64).ok() != Some(2)
        || pattern
            .get(b"Type")
            .is_ok_and(|v| v.as_name().ok() != Some(b"Pattern"))
    {
        return Err(INVALID.into());
    }
    if pattern.has(b"Matrix") {
        let m = numbers::<6>(pattern, b"Matrix")?;
        let determinant = m[0] * m[3] - m[1] * m[2];
        if determinant == 0. || !determinant.is_finite() {
            return Err(INVALID.into());
        }
    }
    let shading = dictionary(doc, pattern.get(b"Shading").map_err(|_| INVALID)?)?;
    keys(
        shading,
        &[
            b"ShadingType",
            b"ColorSpace",
            b"Coords",
            b"Domain",
            b"Extend",
            b"Function",
        ],
    )?;
    if shading.get(b"ShadingType").and_then(Object::as_i64).ok() != Some(2) {
        return Err(INVALID.into());
    }
    let components = match shading
        .get(b"ColorSpace")
        .and_then(Object::as_name)
        .map_err(|_| INVALID)?
    {
        b"DeviceGray" => 1,
        b"DeviceRGB" => 3,
        b"DeviceCMYK" => 4,
        _ => return Err(INVALID.into()),
    };
    let coords = numbers::<4>(shading, b"Coords")?;
    if coords[..2] == coords[2..] {
        return Err(INVALID.into());
    }
    if shading.has(b"Domain") && numbers::<2>(shading, b"Domain")? != [0., 1.] {
        return Err(INVALID.into());
    }
    if let Ok(extend) = shading.get(b"Extend") {
        if !matches!(extend, Object::Array(v) if matches!(v.as_slice(), [Object::Boolean(_), Object::Boolean(_)]))
        {
            return Err(INVALID.into());
        }
    }
    let function = dictionary(doc, shading.get(b"Function").map_err(|_| INVALID)?)?;
    keys(function, &[b"FunctionType", b"Domain", b"C0", b"C1", b"N"])?;
    if function.get(b"FunctionType").and_then(Object::as_i64).ok() != Some(2)
        || numbers::<2>(function, b"Domain")? != [0., 1.]
    {
        return Err(INVALID.into());
    }
    let exponent = number(function.get(b"N").map_err(|_| INVALID)?)?;
    if !(0.0..=128.).contains(&exponent) || exponent == 0. {
        return Err(INVALID.into());
    }
    for key in [b"C0", b"C1"] {
        colors::values(
            function
                .get(key)
                .and_then(Object::as_array)
                .map_err(|_| INVALID)?,
            components,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
