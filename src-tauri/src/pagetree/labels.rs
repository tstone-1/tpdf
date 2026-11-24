//! Preserve position-based page labels when a writer changes the page sequence.

use std::collections::{BTreeMap, HashSet};

use lopdf::{Dictionary, Document, Object, StringFormat};

use crate::encoding::resolve;

fn invalid() -> String {
    said!("the document's page labels could not be remapped")
}

fn entry<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    dict.get(key)
        .ok()
        .map(|v| resolve(doc, v))
        .filter(|v| !matches!(v, Object::Null))
}

struct Range<'a> {
    style: Option<&'a [u8]>,
    prefix: Option<(&'a [u8], StringFormat)>,
    start: i64,
}

/// Flatten the number tree once. A page's range is the last key at or before
/// its original index. A cycle or ambiguous key must not produce a wrong label.
fn read<'a>(doc: &'a Document, root: &'a Object) -> Result<BTreeMap<u32, Range<'a>>, String> {
    let mut ranges = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let dict = resolve(doc, node).as_dict().map_err(|_| invalid())?;
        if let Some(kids) = entry(doc, dict, b"Kids") {
            if entry(doc, dict, b"Nums").is_some() {
                return Err(invalid());
            }
            for kid in resolve(doc, kids).as_array().map_err(|_| invalid())? {
                // Kids are indirect references. Visit each object at most once.
                if !seen.insert(kid.as_reference().map_err(|_| invalid())?) {
                    return Err(invalid());
                }
                pending.push(kid);
            }
            continue;
        }
        let nums = dict.get(b"Nums").map_err(|_| invalid())?;
        let nums = resolve(doc, nums).as_array().map_err(|_| invalid())?;
        let pairs = nums.chunks_exact(2);
        if !pairs.remainder().is_empty() {
            return Err(invalid());
        }
        for pair in pairs {
            let key = resolve(doc, &pair[0]).as_i64().map_err(|_| invalid())?;
            let key = u32::try_from(key).map_err(|_| invalid())?;
            let rule = resolve(doc, &pair[1]).as_dict().map_err(|_| invalid())?;
            let style = entry(doc, rule, b"S")
                .map(Object::as_name)
                .transpose()
                .map_err(|_| invalid())?;
            if style.is_some_and(|value| !matches!(value, b"D" | b"R" | b"r" | b"A" | b"a")) {
                return Err(invalid());
            }
            let prefix = match entry(doc, rule, b"P") {
                Some(Object::String(bytes, format)) => Some((bytes.as_slice(), *format)),
                Some(_) => return Err(invalid()),
                None => None,
            };
            let start = entry(doc, rule, b"St")
                .map(Object::as_i64)
                .transpose()
                .map_err(|_| invalid())?
                .unwrap_or(1);
            if start < 1 || ranges.contains_key(&key) {
                return Err(invalid());
            }
            ranges.insert(
                key,
                Range {
                    style,
                    prefix,
                    start,
                },
            );
        }
    }
    if !ranges.contains_key(&0) {
        return Err(invalid());
    }
    Ok(ranges)
}

pub(crate) fn remap(
    doc: &mut Document,
    sources: impl IntoIterator<Item = Option<u32>>,
) -> Result<(), String> {
    let Some(root) = doc
        .catalog()
        .ok()
        .and_then(|catalog| entry(doc, catalog, b"PageLabels"))
    else {
        return Ok(());
    };
    let ranges = read(doc, root)?;
    let mut prefix_budget = crate::encoding::MAX_DECODE;
    let mut nums = Vec::new();
    let mut previous = None;
    for (slot, source) in sources.into_iter().enumerate() {
        let key = source.and_then(|page| ranges.range(..=page).next_back().map(|(&key, _)| key));
        let numbered = key.is_some_and(|k| ranges[&k].style.is_some());
        if let Some((last_key, last_page)) = previous {
            if key == last_key && (!numbered || source.and_then(|n| n.checked_sub(1)) == last_page)
            {
                previous = Some((key, source));
                continue;
            }
        }
        previous = Some((key, source));
        let mut rule = Dictionary::new();
        if let (Some(key), Some(source)) = (key, source) {
            let range = &ranges[&key];
            if let Some(style) = range.style {
                rule.set("S", Object::Name(style.to_vec()));
                let start = range
                    .start
                    .checked_add(i64::from(source - key))
                    .ok_or_else(invalid)?;
                if start != 1 {
                    rule.set("St", start);
                }
            }
            // Readers including pypdf require /P to be a direct string. Bound
            // the total copied when page moves split a single prefixed range.
            if let Some((bytes, format)) = range.prefix {
                prefix_budget = prefix_budget.checked_sub(bytes.len()).ok_or_else(invalid)?;
                rule.set("P", Object::String(bytes.to_vec(), format));
            }
        }
        nums.push(Object::Integer(i64::try_from(slot).map_err(|_| invalid())?));
        nums.push(Object::Dictionary(rule));
    }
    let mut tree = Dictionary::new();
    tree.set("Nums", nums);
    doc.catalog_mut()
        .map_err(|_| invalid())?
        .set("PageLabels", tree);
    Ok(())
}
