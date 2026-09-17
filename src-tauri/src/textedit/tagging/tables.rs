//! Preserve header identity and associations, without reconstructing a table.
use super::{array, get, integer, keys, name, reference, INVALID};
use crate::encoding::resolve;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

const ID_TREE_ERROR: &str = "unsupported IDTree metadata in tagged structure root";
const MAX_CONTENT_ITEMS: usize = 128;

fn identifier(value: &Object) -> Result<&[u8], String> {
    let bytes = value.as_str().map_err(|_| INVALID)?;
    if bytes.is_empty() || bytes.len() > 127 {
        return Err(INVALID.into());
    }
    Ok(bytes)
}

#[derive(Default)]
pub(super) struct Tables {
    index: BTreeMap<Vec<u8>, ObjectId>,
    headers: BTreeMap<Vec<u8>, ObjectId>, // identifier -> owning table
    links: Vec<(ObjectId, Vec<u8>)>,
}

impl Tables {
    pub(super) fn read(doc: &Document, root: &Dictionary) -> Result<Self, String> {
        let mut result = Self::default();
        if let Ok(tree) = root.get(b"IDTree") {
            let mut visited = BTreeSet::new();
            read_names(doc, tree, 0, &mut visited, &mut result.index, &mut 0)
                .map_err(|_| ID_TREE_ERROR.to_string())?;
        }
        Ok(result)
    }

    pub(super) fn cell(
        &mut self,
        doc: &Document,
        cell: &Dictionary,
        id: ObjectId,
        table: ObjectId,
    ) -> Result<(), String> {
        let header = name(get(cell, b"S")?)? == b"TH";
        if let Ok(value) = cell.get(b"ID") {
            let key = identifier(value)?;
            if !header
                || self.index.get(key) != Some(&id)
                || self.headers.insert(key.to_vec(), table).is_some()
            {
                return Err("tagged header and IDTree disagree on ownership".into());
            }
        }
        let Ok(attributes) = cell.get(b"A") else {
            return Ok(());
        };
        let attributes = resolve(doc, attributes);
        let attributes = match attributes {
            Object::Array(items) => items.as_slice(),
            value => std::slice::from_ref(value),
        };
        if attributes.is_empty() || attributes.len() > 4 {
            return Err(INVALID.into());
        }
        let mut seen = BTreeSet::new();
        for value in attributes {
            let value = resolve(doc, value).as_dict().map_err(|_| INVALID)?;
            keys(
                value,
                &[b"O", b"RowSpan", b"ColSpan", b"Headers", b"Scope"],
                "table cell attributes",
            )?;
            if name(get(value, b"O")?)? != b"Table" {
                return Err(INVALID.into());
            }
            for key in [b"RowSpan".as_slice(), b"ColSpan"] {
                if let Ok(span) = value.get(key) {
                    if !(1..=128).contains(&integer(span)?) || !seen.insert(key) {
                        return Err(INVALID.into());
                    }
                }
            }
            if let Ok(scope) = value.get(b"Scope") {
                if !header
                    || !matches!(name(scope)?, b"Row" | b"Column" | b"Both")
                    || !seen.insert(b"Scope")
                {
                    return Err(INVALID.into());
                }
            }
            if let Ok(headers) = value.get(b"Headers") {
                let headers = array(resolve(doc, headers))?;
                // Header-to-header graphs need a separate cycle/meaning check.
                if !seen.insert(b"Headers") || headers.len() > 16 || (header && !headers.is_empty())
                {
                    return Err(INVALID.into());
                }
                let mut unique = BTreeSet::new();
                for value in headers {
                    let key = identifier(value)?;
                    if !unique.insert(key) {
                        return Err(INVALID.into());
                    }
                    self.links.push((table, key.to_vec()));
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), String> {
        // Checking only referenced headers misses stale or detached IDTree
        // entries. Every indexed identifier must belong to a visited TH.
        if self.headers.len() != self.index.len() {
            return Err("tagged header and IDTree disagree on ownership".into());
        }
        for (table, key) in &self.links {
            if self.headers.get(key) != Some(table) {
                return Err("tagged cell references an unknown or foreign header".into());
            }
        }
        Ok(())
    }
}

// ISO 32000-1 Tables 36, 322 and 323: byte-string keys, ordered name-tree
// leaves and exact Limits at every non-root node. Recursion is bounded before
// descending, independently of the node and identifier counts.
fn read_names(
    doc: &Document,
    value: &Object,
    depth: usize,
    visited: &mut BTreeSet<ObjectId>,
    index: &mut BTreeMap<Vec<u8>, ObjectId>,
    nodes: &mut usize,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if depth > 8 || *nodes >= 128 {
        return Err(INVALID.into());
    }
    *nodes += 1;
    if let Object::Reference(id) = value {
        if !visited.insert(*id) {
            return Err(INVALID.into());
        }
    } else if depth != 0 {
        return Err(INVALID.into());
    }
    let node = resolve(doc, value).as_dict().map_err(|_| INVALID)?;
    keys(
        node,
        if depth == 0 {
            &[b"Names", b"Kids"]
        } else {
            &[b"Names", b"Kids", b"Limits"]
        },
        "IDTree node",
    )?;
    if node.has(b"Names") == node.has(b"Kids") {
        return Err(INVALID.into());
    }
    let mut first = Vec::new();
    let mut last = Vec::new();
    if let Ok(names) = node.get(b"Names") {
        let names = array(resolve(doc, names))?;
        if names.is_empty()
            || names.len() % 2 != 0
            || names.len() / 2 + index.len() > MAX_CONTENT_ITEMS
        {
            return Err(INVALID.into());
        }
        for pair in names.chunks_exact(2) {
            let key = identifier(&pair[0])?;
            if key <= last.as_slice() || index.insert(key.to_vec(), reference(&pair[1])?).is_some()
            {
                return Err(INVALID.into());
            }
            if first.is_empty() {
                first = key.to_vec();
            }
            last = key.to_vec();
        }
    } else {
        let kids = array(resolve(doc, get(node, b"Kids")?))?;
        if kids.is_empty() || kids.len() > 128 {
            return Err(INVALID.into());
        }
        for kid in kids {
            let (low, high) = read_names(doc, kid, depth + 1, visited, index, nodes)?;
            if low <= last {
                return Err(INVALID.into());
            }
            if first.is_empty() {
                first = low;
            }
            last = high;
        }
    }
    if depth != 0 {
        let limits = array(resolve(doc, get(node, b"Limits")?))?;
        if limits.len() != 2 || identifier(&limits[0])? != first || identifier(&limits[1])? != last
        {
            return Err(INVALID.into());
        }
    }
    Ok((first, last))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn textedit_idtree_bounds_nodes_and_entries_independently() {
        for count in [128, 129] {
            let doc = Document::new();
            let names: Vec<_> = (0..count)
                .flat_map(|n| {
                    [
                        Object::string_literal(format!("SYNTHETIC_{n:03}")),
                        Object::Reference((n + 1, 0)),
                    ]
                })
                .collect();
            let root = dictionary! { "IDTree" => dictionary! { "Names" => names } };
            assert_eq!(Tables::read(&doc, &root).is_ok(), count == 128);
        }
        for extra in [false, true] {
            let mut doc = Document::new();
            let mut kids = Vec::new();
            for n in 0..64 {
                let key = Object::string_literal(format!("SYNTHETIC_{n:03}"));
                let limits = vec![key.clone(), key.clone()];
                let mut child = doc.add_object(dictionary! { "Names" => vec![key, Object::Reference((1000+n,0))], "Limits" => limits.clone() });
                // 64 leaves + 63 intermediates + the root = 128 nodes.
                if n != 0 || extra {
                    child = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(child)], "Limits" => limits });
                }
                kids.push(Object::Reference(child));
            }
            let tree = doc.add_object(dictionary! { "Kids" => kids });
            let root = dictionary! { "IDTree" => tree };
            assert_eq!(Tables::read(&doc, &root).is_ok(), !extra);
            let root = dictionary! { "IDTree" => doc.objects[&tree].clone() };
            assert_eq!(
                Tables::read(&doc, &root).is_ok(),
                !extra,
                "inline root counts too"
            );
        }
    }

    #[test]
    fn textedit_idtree_requires_ordered_keys_and_disjoint_children() {
        for split in [false, true] {
            for reverse in [false, true] {
                let mut doc = Document::new();
                let mut keys = ["SYNTHETIC_A", "SYNTHETIC_B"];
                if reverse {
                    keys.reverse();
                }
                let tree = if split {
                    let kids: Vec<_> = keys.into_iter().enumerate().map(|(n, key)| {
                        let key = Object::string_literal(key);
                        doc.add_object(dictionary! { "Names" => vec![key.clone(), Object::Reference((100+n as u32, 0))], "Limits" => vec![key.clone(), key] }).into()
                    }).collect();
                    dictionary! { "Kids" => kids }
                } else {
                    dictionary! { "Names" => vec![Object::string_literal(keys[0]), Object::Reference((100,0)), Object::string_literal(keys[1]), Object::Reference((101,0))] }
                };
                let root = dictionary! { "IDTree" => tree };
                assert_eq!(Tables::read(&doc, &root).is_ok(), !reverse);
            }
        }
    }
}
