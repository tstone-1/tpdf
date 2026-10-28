//! Preserve bounded axial shading artwork, never use a pattern to edit text.
//! ISO 32000-1 8.7.4: a type-2 pattern contains a shading dictionary, not a
//! content stream. Restrict its function to a nonrecursive type-2 interpolation
//! or a bounded type-0 sampled function, which is checked and never evaluated.
use super::{colors, dictionary, number};
use lopdf::{Dictionary, Document, Object};

fn invalid() -> String {
    said!("unsupported shading pattern on editable page")
}

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
                    return Err(invalid());
                };
                if !matches!(operator, "scn" | "SCN") {
                    return Err(invalid());
                }
                if !checked.contains(name) {
                    if checked.len() >= 32 {
                        return Err(said!("too many shading patterns"));
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
            Err(said!("painting with an unselected shading pattern"))
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
        return Err(invalid());
    }
    Ok(())
}

fn numbers<const N: usize>(dict: &Dictionary, key: &[u8]) -> Result<[f64; N], String> {
    let values = dict
        .get(key)
        .and_then(Object::as_array)
        .map_err(|_| invalid())?;
    if values.len() != N {
        return Err(invalid());
    }
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) {
        *out = number(value)?;
    }
    Ok(result)
}

fn check(doc: &Document, resources: &Dictionary, name: &[u8]) -> Result<(), String> {
    let patterns = dictionary(doc, resources.get(b"Pattern").map_err(|_| invalid())?)?;
    let pattern = dictionary(doc, patterns.get(name).map_err(|_| invalid())?)?;
    keys(pattern, &[b"Type", b"PatternType", b"Shading", b"Matrix"])?;
    if pattern.get(b"PatternType").and_then(Object::as_i64).ok() != Some(2)
        || pattern
            .get(b"Type")
            .is_ok_and(|v| v.as_name().ok() != Some(b"Pattern"))
    {
        return Err(invalid());
    }
    if pattern.has(b"Matrix") {
        let m = numbers::<6>(pattern, b"Matrix")?;
        let determinant = m[0] * m[3] - m[1] * m[2];
        if determinant == 0. || !determinant.is_finite() {
            return Err(invalid());
        }
    }
    let shading = dictionary(doc, pattern.get(b"Shading").map_err(|_| invalid())?)?;
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
        return Err(invalid());
    }
    let components = match shading
        .get(b"ColorSpace")
        .and_then(Object::as_name)
        .map_err(|_| invalid())?
    {
        b"DeviceGray" => 1,
        b"DeviceRGB" => 3,
        b"DeviceCMYK" => 4,
        _ => return Err(invalid()),
    };
    let coords = numbers::<4>(shading, b"Coords")?;
    if coords[..2] == coords[2..] {
        return Err(invalid());
    }
    if shading.has(b"Domain") && numbers::<2>(shading, b"Domain")? != [0., 1.] {
        return Err(invalid());
    }
    if let Ok(extend) = shading.get(b"Extend") {
        if !matches!(extend, Object::Array(v) if matches!(v.as_slice(), [Object::Boolean(_), Object::Boolean(_)]))
        {
            return Err(invalid());
        }
    }
    let function = shading.get(b"Function").map_err(|_| invalid())?;
    if let Ok(stream) = crate::encoding::resolve(doc, function).as_stream() {
        return sampled(stream, components);
    }
    let function = dictionary(doc, function)?;
    keys(function, &[b"FunctionType", b"Domain", b"C0", b"C1", b"N"])?;
    if function.get(b"FunctionType").and_then(Object::as_i64).ok() != Some(2)
        || numbers::<2>(function, b"Domain")? != [0., 1.]
    {
        return Err(invalid());
    }
    let exponent = number(function.get(b"N").map_err(|_| invalid())?)?;
    if !(0.0..=128.).contains(&exponent) || exponent == 0. {
        return Err(invalid());
    }
    for key in [b"C0", b"C1"] {
        colors::values(
            function
                .get(key)
                .and_then(Object::as_array)
                .map_err(|_| invalid())?,
            components,
        )?;
    }
    Ok(())
}

// A sampled function's sample count, and the most bits one sample may take.
const MAX_SAMPLES: i64 = 4096;
const SAMPLE_BITS: [i64; 8] = [1, 2, 4, 8, 12, 16, 24, 32];

/// ISO 32000-1 7.10.2: a type 0 (sampled) function of one input, as the
/// shading's colour function. PowerPoint writes one for every gradient: 512
/// eight-bit RGB samples, Flate-compressed. It is preserved and never
/// evaluated; what is checked is that it is exactly what its dictionary says
/// -- its shapes, and a decoded length equal to the one its size, outputs and
/// bits per sample make -- so a reader that does evaluate it finds no surprise.
fn sampled(stream: &lopdf::Stream, components: usize) -> Result<(), String> {
    let function = &stream.dict;
    keys(
        function,
        &[
            b"FunctionType",
            b"Domain",
            b"Range",
            b"Size",
            b"BitsPerSample",
            b"Order",
            b"Encode",
            b"Decode",
            b"Length",
            b"Filter",
        ],
    )?;
    let integer = |key: &[u8]| {
        function
            .get(key)
            .and_then(Object::as_i64)
            .map_err(|_| invalid())
    };
    let [size] = function
        .get(b"Size")
        .and_then(Object::as_array)
        .map_err(|_| invalid())?
        .as_slice()
    else {
        return Err(invalid());
    };
    let size = size.as_i64().map_err(|_| invalid())?;
    let bits = integer(b"BitsPerSample")?;
    if integer(b"FunctionType")? != 0
        || numbers::<2>(function, b"Domain")? != [0., 1.]
        || !(1..=MAX_SAMPLES).contains(&size)
        || !SAMPLE_BITS.contains(&bits)
        || function
            .get(b"Order")
            .is_ok_and(|order| !matches!(order.as_i64(), Ok(1 | 3)))
    {
        return Err(invalid());
    }
    // Range has two numbers per output, and the outputs are the colour
    // space's components; Decode, when present, has the same shape.
    for key in [b"Range".as_slice(), b"Decode"] {
        let Ok(values) = function.get(key) else {
            if key == b"Range" {
                return Err(invalid());
            }
            continue;
        };
        let values = values.as_array().map_err(|_| invalid())?;
        if values.len() != 2 * components {
            return Err(invalid());
        }
        for pair in values.chunks_exact(2) {
            let [low, high] = [number(&pair[0])?, number(&pair[1])?];
            if key == b"Range" && low > high {
                return Err(invalid());
            }
        }
    }
    if function.has(b"Encode") {
        numbers::<2>(function, b"Encode")?;
    }
    let length = usize::try_from(size * components as i64 * bits)
        .map_err(|_| invalid())?
        .div_ceil(8);
    // Exactly that many bytes: a longer stream is refused by the decoding
    // bound, a shorter one by the comparison.
    let samples = super::filters::decode(stream, length).map_err(|_| invalid())?;
    if samples.len() < length {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
