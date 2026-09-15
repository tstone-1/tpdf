//! Bounded grouping and paragraph/heading trees, preserved rather than regenerated.
//! Reject semantic overrides and layout attributes that a shorter edit could stale.

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod list_tests;
#[cfg(test)]
mod nested_tests;
#[cfg(test)]
mod tests;

// Every text block owns content. Grouping elements have separate count/depth
// bounds because a chain can contain many containers and just one text block.
const MAX_CONTENT_ITEMS: usize = 128;
const MAX_CONTAINERS: usize = 128;
const MAX_CONTAINER_DEPTH: usize = 8;
const INVALID: &str = "unsupported or inconsistent tagged text structure";

// ISO 32000-1 14.8.4: these grouping elements carry child elements; paragraph-
// like blocks carry the marked content. Keep the two authorities separate.
fn container(tag: &[u8]) -> bool {
    matches!(tag, b"Part" | b"Art" | b"Sect" | b"Div")
}

fn text_block(tag: &[u8]) -> bool {
    matches!(
        tag,
        b"P" | b"H" | b"H1" | b"H2" | b"H3" | b"H4" | b"H5" | b"H6"
    )
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
    doc: &Document,
    dict: &Dictionary,
    parent: ObjectId,
    pages: &BTreeSet<ObjectId>,
) -> Result<Option<ObjectId>, String> {
    // In particular: no ActualText, Alt, E, title, class, or attribute revision.
    keys(dict, &[b"Type", b"S", b"P", b"Pg", b"K", b"A", b"Lang"])?;
    let page = dict.get(b"Pg").ok().map(reference).transpose()?;
    if let Ok(language) = dict.get(b"Lang") {
        let bytes = language.as_str().map_err(|_| INVALID)?;
        if bytes.is_empty()
            || bytes.len() > 63
            || !bytes[0].is_ascii_alphabetic()
            || bytes.split(|&b| b == b'-').any(|part| {
                part.is_empty() || part.len() > 8 || !part.iter().all(u8::is_ascii_alphanumeric)
            })
        {
            return Err(INVALID.into());
        }
    }
    // ISO 32000-1 Table 323: Type is optional, but a supplied value must agree.
    if dict
        .get(b"Type")
        .is_ok_and(|value| value.as_name().ok() != Some(b"StructElem"))
        || reference(get(dict, b"P")?)? != parent
        || page.is_some_and(|id| !pages.contains(&id))
    {
        return Err(INVALID.into());
    }
    if let Ok(attributes) = dict.get(b"A") {
        if name(get(dict, b"S")?)? == b"L" {
            // A single List attribute dictionary, optionally wrapped as emitted
            // by Chromium. Numbering describes labels; it is not an ink bound.
            let attributes = crate::encoding::resolve(doc, attributes);
            let attributes = match attributes {
                Object::Array(items) if items.len() == 1 => {
                    crate::encoding::resolve(doc, &items[0])
                }
                value => value,
            };
            let attributes = attributes.as_dict().map_err(|_| INVALID)?;
            keys(attributes, &[b"O", b"ListNumbering"])?;
            if name(get(attributes, b"O")?)? != b"List"
                || !matches!(
                    name(get(attributes, b"ListNumbering")?)?,
                    b"None"
                        | b"Disc"
                        | b"Circle"
                        | b"Square"
                        | b"Decimal"
                        | b"UpperRoman"
                        | b"LowerRoman"
                        | b"UpperAlpha"
                        | b"LowerAlpha"
                )
            {
                return Err(INVALID.into());
            }
            return Ok(page);
        }
        if name(get(dict, b"S")?)? == b"NonStruct" {
            return Err(INVALID.into());
        }
        let attributes = crate::encoding::resolve(doc, attributes)
            .as_dict()
            .map_err(|_| INVALID)?;
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

// K may be a single child/item or an array. An indirect array is a container,
// not a structure element; other indirect values retain their object identity.
fn children<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a [Object], String> {
    let resolved = if let Object::Reference(id) = value {
        doc.get_object(*id).map_err(|_| INVALID)?
    } else {
        value
    };
    Ok(match resolved {
        Object::Array(values) => values.as_slice(),
        _ => std::slice::from_ref(value),
    })
}

struct Group<'a> {
    id: ObjectId,
    page: Option<ObjectId>,
    tag: &'a [u8],
    items: Vec<&'a Object>,
}

// Exactly one optional NonStruct level below a paragraph, never a recursive
// tree walk. Every group must own content, and total owns the allocation bound.
fn groups<'a>(
    doc: &'a Document,
    paragraph: Group<'a>,
    pages: &BTreeSet<ObjectId>,
    ids: &mut BTreeSet<ObjectId>,
    total: usize,
) -> Result<Vec<Group<'a>>, String> {
    if paragraph.items.len() > total {
        return Err(INVALID.into());
    }
    let mut plain = Group {
        items: Vec::new(),
        ..paragraph
    };
    let mut groups = Vec::new();
    for item in paragraph.items {
        if let Object::Reference(id) = item {
            if !ids.insert(*id) {
                return Err(INVALID.into());
            }
            let child = node(doc, *id)?;
            let page = element(doc, child, plain.id, pages)?;
            let tag = name(get(child, b"S")?)?;
            if tag != b"NonStruct" && !(plain.tag == b"LI" && matches!(tag, b"Lbl" | b"LBody")) {
                return Err(INVALID.into());
            }
            if plain.tag == b"LI" && child.has(b"A") {
                return Err(INVALID.into());
            }
            let items = children(doc, get(child, b"K")?)?;
            if items.len() > total {
                return Err(INVALID.into());
            }
            groups.push(Group {
                id: *id,
                page,
                tag,
                items: items.iter().collect(),
            });
        } else {
            plain.items.push(item);
        }
    }
    if !plain.items.is_empty() || groups.is_empty() {
        groups.push(plain);
    }
    Ok(groups)
}

#[derive(Default)]
pub(super) struct Tags {
    // One authored tag name per MCID. Empty means an ordinary untagged page.
    names: Vec<Vec<u8>>,
    seen: BTreeSet<usize>,
    active: Option<Option<usize>>, // None outside; Some(None) is an artifact.
    has_content: bool,
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
        // The grouping walk below is iterative, with independent depth/count bounds.
        if pages.is_empty() || pages.len() > MAX_CONTENT_ITEMS {
            return Err(INVALID.into());
        }
        let page_ids: BTreeSet<_> = pages.iter().copied().collect();
        if page_ids.len() != pages.len() || !page_ids.contains(&page) {
            return Err(INVALID.into());
        }
        let root_id = reference(root)?;
        let root = node(doc, root_id)?;
        keys(
            root,
            &[
                b"Type",
                b"K",
                b"ParentTree",
                b"RoleMap",
                b"ParentTreeNextKey",
            ],
        )?;
        if name(get(root, b"Type")?)? != b"StructTreeRoot" {
            return Err(INVALID.into());
        }
        let roles = root
            .get(b"RoleMap")
            .ok()
            .map(|value| crate::encoding::resolve(doc, value).as_dict())
            .transpose()
            .map_err(|_| INVALID)?;
        if let Some(roles) = roles {
            if roles.len() > 16
                || roles.iter().any(|(key, value)| {
                    key.is_empty()
                        || key.len() > 127
                        || key == b"Document"
                        || text_block(key)
                        || container(key)
                        || key == b"NonStruct"
                        || matches!(key.as_slice(), b"L" | b"LI" | b"Lbl" | b"LBody")
                        || !value
                            .as_name()
                            .is_ok_and(|tag| text_block(tag) || container(tag))
                })
            {
                return Err(INVALID.into());
            }
        }
        let [document] = children(doc, get(root, b"K")?)? else {
            return Err(INVALID.into());
        };
        let document_id = reference(document)?;
        let document = node(doc, document_id)?;
        element(doc, document, root_id, &page_ids)?;
        if name(get(document, b"S")?)? != b"Document" {
            return Err(INVALID.into());
        }
        let paragraphs = children(doc, get(document, b"K")?)?;
        if paragraphs.is_empty() {
            return Err(INVALID.into());
        }
        // ISO 32000-1 14.7.4.4: StructParents indexes the number tree; the MCID
        // indexes its array. Require both directions to agree, not just /K.
        let parent = node(doc, reference(get(root, b"ParentTree")?)?)?;
        keys(parent, &[b"Type", b"Nums"])?;
        if parent
            .get(b"Type")
            .is_ok_and(|value| value.as_name().ok() != Some(b"ParentTree"))
        {
            return Err(INVALID.into());
        }
        let nums = array(crate::encoding::resolve(doc, get(parent, b"Nums")?))?;
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
            let entry = match &pair[1] {
                Object::Reference(id) => doc.get_object(*id).map_err(|_| INVALID)?,
                value => value,
            };
            let entries = array(entry)?;
            total += entries.len();
            if entries.is_empty() || total > MAX_CONTENT_ITEMS {
                return Err(INVALID.into());
            }
            by_page.insert(owner, (entries, vec![Vec::new(); entries.len()]));
        }
        if let Ok(next) = root.get(b"ParentTreeNextKey") {
            let next = integer(next)?;
            if next <= previous || next > 1_000_001 {
                return Err(INVALID.into());
            }
        }
        if paragraphs.len() > total || !page_keys.is_empty() {
            return Err(INVALID.into());
        }
        let mut assigned = 0;
        let mut ids = page_ids.clone();
        ids.extend([root_id, document_id]);
        let mut pending: Vec<_> = paragraphs
            .iter()
            .rev()
            .map(|child| (child, document_id, 0))
            .collect();
        let mut containers = 0;
        while let Some((child, parent_id, depth)) = pending.pop() {
            let id = reference(child)?;
            if !ids.insert(id) {
                return Err(INVALID.into());
            }
            let child = node(doc, id)?;
            let paragraph_page = element(doc, child, parent_id, &page_ids)?;
            let tag = name(get(child, b"S")?)?;
            let role = roles
                .and_then(|r| r.get(tag).ok())
                .map(name)
                .transpose()?
                .unwrap_or(tag);
            let items = children(doc, get(child, b"K")?)?;
            if container(role) || role == b"NonStruct" || role == b"L" {
                containers += 1;
                // Bound the work list before copying child references into it.
                if items.len() + pending.len() > total {
                    return Err("tagged grouping frontier exceeds its limit".into());
                }
                if depth >= MAX_CONTAINER_DEPTH
                    || containers > MAX_CONTAINERS
                    || items.is_empty()
                    || (role != b"L" && child.has(b"A"))
                {
                    return Err(INVALID.into());
                }
                if role == b"L" {
                    for item in items {
                        if name(get(node(doc, reference(item)?)?, b"S")?)? != b"LI" {
                            return Err(INVALID.into());
                        }
                    }
                }
                // Container Pg never supplies a descendant's page. Its own
                // identity is the immediate parent checked on every child.
                pending.extend(items.iter().rev().map(|item| (item, id, depth + 1)));
                continue;
            }
            if !text_block(role) && role != b"LI" {
                return Err(INVALID.into());
            }
            if role == b"LI"
                && (name(get(node(doc, parent_id)?, b"S")?)? != b"L" || child.has(b"A"))
            {
                return Err(INVALID.into());
            }
            let paragraph = Group {
                id,
                page: paragraph_page,
                tag,
                items: items.iter().collect(),
            };
            for Group {
                id,
                page: paragraph_page,
                tag,
                items,
            } in groups(doc, paragraph, &page_ids, &mut ids, total)?
            {
                assigned += items.len();
                if items.is_empty() || assigned > total {
                    return Err(INVALID.into());
                }
                for item in items {
                    // ISO 32000-1 14.7.4.2: an integer uses the element's Pg;
                    // an MCR names a sequence on its own page. No external streams.
                    let (owner, mcid) = match item {
                        Object::Integer(mcid) => (paragraph_page.ok_or(INVALID)?, *mcid),
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
        self.has_content = false;
        Ok(())
    }

    pub(super) fn end(&mut self) -> Result<(), String> {
        let active = self.active.take().ok_or(INVALID)?;
        if active.is_some() && !self.has_content {
            return Err(INVALID.into());
        }
        Ok(())
    }

    // A tagged paragraph can own a painted background separately from its text.
    // Path construction, clipping, and discarded paths do not supply content.
    pub(super) fn paint(&mut self) {
        self.has_content = true;
    }

    pub(super) fn text(&mut self) -> Result<(), String> {
        if !self.names.is_empty() && !matches!(self.active, Some(Some(_))) {
            return Err("text outside a tagged paragraph is not editable".into());
        }
        self.has_content = true;
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), String> {
        if self.active.is_some() || self.seen.len() != self.names.len() {
            return Err(INVALID.into());
        }
        Ok(())
    }
}
