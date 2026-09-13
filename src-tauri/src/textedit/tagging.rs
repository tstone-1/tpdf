//! A bounded multi-page Document/P tree, preserved rather than regenerated.
//! Reject semantic overrides and layout attributes that a shorter edit could stale.

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

// Every paragraph owns at least one item, so this also bounds pages and elements.
const MAX_CONTENT_ITEMS: usize = 128;
const INVALID: &str = "unsupported or inconsistent tagged text structure";

fn keys(dict: &Dictionary, allowed: &[&[u8]]) -> Result<(), String> {
    if dict
        .iter()
        .any(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        return Err(INVALID.into());
    }
    Ok(())
}

fn get<'a>(dict: &'a Dictionary, key: &[u8]) -> Result<&'a Object, String> {
    dict.get(key).map_err(|_| INVALID.into())
}

fn reference(value: &Object) -> Result<ObjectId, String> {
    value.as_reference().map_err(|_| INVALID.into())
}

fn node(doc: &Document, id: ObjectId) -> Result<&Dictionary, String> {
    doc.get_object(id)
        .and_then(Object::as_dict)
        .map_err(|_| INVALID.into())
}

fn array(value: &Object) -> Result<&[Object], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .map_err(|_| INVALID.into())
}

fn name(value: &Object) -> Result<&[u8], String> {
    value.as_name().map_err(|_| INVALID.into())
}

fn integer(value: &Object) -> Result<i64, String> {
    value.as_i64().map_err(|_| INVALID.into())
}

fn element(
    dict: &Dictionary,
    parent: ObjectId,
    pages: &BTreeSet<ObjectId>,
) -> Result<ObjectId, String> {
    // In particular: no ActualText, Alt, E, title, class, or attribute revision.
    keys(dict, &[b"Type", b"S", b"P", b"Pg", b"K", b"A"])?;
    let page = reference(get(dict, b"Pg")?)?;
    if name(get(dict, b"Type")?)? != b"StructElem"
        || reference(get(dict, b"P")?)? != parent
        || !pages.contains(&page)
    {
        return Err(INVALID.into());
    }
    if let Ok(attributes) = dict.get(b"A") {
        let attributes = attributes.as_dict().map_err(|_| INVALID)?;
        keys(attributes, &[b"O", b"Placement", b"EndIndent"])?;
        if name(get(attributes, b"O")?)? != b"Layout"
            || name(get(attributes, b"Placement")?)? != b"Block"
        {
            return Err(INVALID.into());
        }
        if let Ok(indent) = attributes.get(b"EndIndent") {
            // An authored paragraph allocation constraint, not an ink bound.
            // Replacing a fitting line preserves its position and this indent.
            if name(get(dict, b"S")?)? == b"Document" {
                return Err(INVALID.into());
            }
            super::number(indent)?;
        }
    }
    Ok(page)
}

#[derive(Default)]
pub(super) struct Tags {
    // One authored tag name per MCID. Empty means an ordinary untagged page.
    names: Vec<Vec<u8>>,
    seen: BTreeSet<usize>,
    active: Option<Option<usize>>, // None outside; Some(None) is an artifact.
    shows: usize,
}

impl Tags {
    pub(super) fn read(doc: &Document, page: ObjectId, pages: &[ObjectId]) -> Result<Self, String> {
        let catalog = doc.catalog().map_err(|_| INVALID)?;
        let page_dict = node(doc, page)?;
        let Ok(root) = catalog.get(b"StructTreeRoot") else {
            if page_dict.has(b"StructParents") || page_dict.has(b"StructParent") {
                return Err(INVALID.into());
            }
            return Ok(Self::default());
        };
        // No recursive graph walk: this first grammar has exactly two levels.
        if pages.is_empty() || pages.len() > MAX_CONTENT_ITEMS {
            return Err(INVALID.into());
        }
        let page_ids: BTreeSet<_> = pages.iter().copied().collect();
        if page_ids.len() != pages.len() || !page_ids.contains(&page) {
            return Err(INVALID.into());
        }
        let root_id = reference(root)?;
        let root = node(doc, root_id)?;
        keys(root, &[b"Type", b"K", b"ParentTree", b"RoleMap"])?;
        if name(get(root, b"Type")?)? != b"StructTreeRoot" {
            return Err(INVALID.into());
        }
        let roles = root
            .get(b"RoleMap")
            .ok()
            .map(Object::as_dict)
            .transpose()
            .map_err(|_| INVALID)?;
        if let Some(roles) = roles {
            if roles.len() > 16
                || roles.iter().any(|(key, value)| {
                    key.is_empty()
                        || key.len() > 127
                        || key == b"Document"
                        || key == b"P"
                        || value.as_name().ok() != Some(b"P")
                })
            {
                return Err(INVALID.into());
            }
        }
        let [document] = array(get(root, b"K")?)? else {
            return Err(INVALID.into());
        };
        let document_id = reference(document)?;
        let document = node(doc, document_id)?;
        element(document, root_id, &page_ids)?;
        if name(get(document, b"S")?)? != b"Document" {
            return Err(INVALID.into());
        }
        let children = array(get(document, b"K")?)?;
        if children.is_empty() {
            return Err(INVALID.into());
        }
        // ISO 32000-1 14.7.4.4: StructParents indexes the number tree; the MCID
        // indexes its array. Require both directions to agree, not just /K.
        let parent = node(doc, reference(get(root, b"ParentTree")?)?)?;
        keys(parent, &[b"Nums"])?;
        let nums = array(get(parent, b"Nums")?)?;
        if nums.len() != pages.len() * 2 {
            return Err(INVALID.into());
        }
        let mut page_keys = BTreeMap::new();
        for &id in pages {
            let dict = node(doc, id)?;
            let key = integer(get(dict, b"StructParents")?)?;
            if dict.has(b"StructParent")
                || !(0..=1_000_000).contains(&key)
                || page_keys.insert(key, id).is_some()
            {
                return Err(INVALID.into());
            }
        }
        let mut by_page = BTreeMap::new();
        let mut previous = -1;
        let mut total = 0;
        for pair in nums.chunks_exact(2) {
            let key = integer(&pair[0])?;
            if key <= previous {
                return Err(INVALID.into());
            }
            previous = key;
            let owner = page_keys.remove(&key).ok_or(INVALID)?;
            let entries = array(&pair[1])?;
            total += entries.len();
            if entries.is_empty() || total > MAX_CONTENT_ITEMS {
                return Err(INVALID.into());
            }
            by_page.insert(owner, (entries, vec![Vec::new(); entries.len()]));
        }
        if children.len() > total || !page_keys.is_empty() {
            return Err(INVALID.into());
        }
        let mut assigned = 0;
        let mut ids = page_ids.clone();
        ids.extend([root_id, document_id]);
        for child in children {
            let id = reference(child)?;
            if !ids.insert(id) {
                return Err(INVALID.into());
            }
            let child = node(doc, id)?;
            let paragraph_page = element(child, document_id, &page_ids)?;
            let tag = name(get(child, b"S")?)?;
            if tag != b"P"
                && roles
                    .and_then(|r| r.get(tag).ok())
                    .and_then(|v| v.as_name().ok())
                    != Some(b"P")
            {
                return Err(INVALID.into());
            }
            let items = array(get(child, b"K")?)?;
            assigned += items.len();
            if items.is_empty() || assigned > total {
                return Err(INVALID.into());
            }
            for item in items {
                // ISO 32000-1 14.7.4.2: an integer uses the element's Pg;
                // an MCR names a sequence on its own page. No external streams.
                let (owner, mcid) = match item {
                    Object::Integer(mcid) => (paragraph_page, *mcid),
                    Object::Dictionary(mcr) => {
                        keys(mcr, &[b"Type", b"Pg", b"MCID"])?;
                        if name(get(mcr, b"Type")?)? != b"MCR" {
                            return Err(INVALID.into());
                        }
                        (reference(get(mcr, b"Pg")?)?, integer(get(mcr, b"MCID")?)?)
                    }
                    _ => return Err(INVALID.into()),
                };
                let (entries, names) = by_page.get_mut(&owner).ok_or(INVALID)?;
                let mcid = usize::try_from(mcid).map_err(|_| INVALID)?;
                if mcid >= names.len()
                    || !names[mcid].is_empty()
                    || reference(&entries[mcid])? != id
                {
                    return Err(INVALID.into());
                }
                names[mcid] = tag.to_vec();
            }
        }
        if assigned != total {
            return Err(INVALID.into());
        }
        let (_, names) = by_page.remove(&page).ok_or(INVALID)?;
        Ok(Self {
            names,
            ..Self::default()
        })
    }

    pub(super) fn begin(
        &mut self,
        tag: &Object,
        properties: Option<&Object>,
    ) -> Result<(), String> {
        if self.names.is_empty() || self.active.is_some() {
            return Err(INVALID.into());
        }
        let tag = name(tag)?;
        let mcid = if let Some(properties) = properties {
            let properties = properties.as_dict().map_err(|_| INVALID)?;
            keys(properties, &[b"MCID"])?;
            let mcid = usize::try_from(integer(get(properties, b"MCID")?)?).map_err(|_| INVALID)?;
            if self.names.get(mcid).map(Vec::as_slice) != Some(tag) || !self.seen.insert(mcid) {
                return Err(INVALID.into());
            }
            Some(mcid)
        } else {
            if tag != b"Artifact" {
                return Err(INVALID.into());
            }
            None
        };
        self.active = Some(mcid);
        self.shows = 0;
        Ok(())
    }

    pub(super) fn end(&mut self) -> Result<(), String> {
        let active = self.active.take().ok_or(INVALID)?;
        if active.is_some() && self.shows == 0 {
            return Err(INVALID.into());
        }
        Ok(())
    }

    pub(super) fn text(&mut self) -> Result<(), String> {
        if !self.names.is_empty() && !matches!(self.active, Some(Some(_))) {
            return Err("text outside a tagged paragraph is not editable".into());
        }
        self.shows += 1;
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), String> {
        if self.active.is_some() || self.seen.len() != self.names.len() {
            return Err(INVALID.into());
        }
        Ok(())
    }
}
