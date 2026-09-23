//! Bounded grouping and paragraph/heading trees, preserved rather than regenerated.
//! Reject semantic overrides and layout attributes that a shorter edit could stale.

use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod annotation_tests;
#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod list_tests;
#[cfg(test)]
mod nested_list_tests;
#[cfg(test)]
mod nested_tests;
#[cfg(test)]
mod pinned_tests;
#[cfg(test)]
mod powerpoint_tests;
#[cfg(test)]
mod producer_tests;
#[cfg(test)]
mod refusal_tests;
#[cfg(test)]
mod role_tests;
#[cfg(test)]
mod span_tests;
#[cfg(test)]
mod table_tests;
mod tables;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tree_tests;

// Every text block owns content. Grouping elements have separate count/depth
// bounds because a chain can contain many containers and just one text block.
// Word and Acrobat forms tag every table cell and field label, so one page
// can own over a thousand MCIDs. Both bounds stay within the page's operator
// limit; unused (null) slots count toward them.
const MAX_CONTENT_ITEMS: usize = 4096;
const MAX_DOCUMENT_CONTENT: usize = 16384;
const MAX_NODES: usize = 4096;
// Acrobat and LiveCycle forms wrap each field in its own Div; the IRS W-4 has
// 262. Containers still share the MAX_NODES bound with every other element.
const MAX_CONTAINERS: usize = 1024;
const MAX_CONTAINER_DEPTH: usize = 8;
const INVALID: &str = "unsupported or inconsistent tagged text structure";
// Marks an MCID whose slot names an unreachable element. A PDF name cannot
// contain NUL (ISO 32000-1 7.3.5), so no content tag can equal it.
const ORPHAN: &[u8] = b"\0orphan";

// ISO 32000-1 14.8.4: these grouping elements carry child elements; paragraph-
// like blocks carry the marked content. Keep the two authorities separate.
// ISO 32000-1 14.8.4.3: grouping elements, which own child elements rather than
// content. Note joins them because a footnote or endnote holds ordinary blocks;
// one that owned marked content directly is still refused.
fn container(tag: &[u8]) -> bool {
    matches!(tag, b"Part" | b"Art" | b"Sect" | b"Div" | b"Note")
}

fn text_block(tag: &[u8]) -> bool {
    matches!(
        tag,
        b"P" | b"H" | b"H1" | b"H2" | b"H3" | b"H4" | b"H5" | b"H6"
    )
}

// ISO 32000-1 14.8.4, Tables 333-340: standard roles keep their meaning even
// when this editor does not support their content. Do not let RoleMap turn a
// Figure, table cell or inline span into a supported paragraph/container.
// This is the default PDF 1.7 namespace. Elements in the declared PDF 1.7 or
// 2.0 standard namespace are limited separately to the types both share.
// https://pdfa.org/download-area/cheat-sheets/StandardStructureElements.pdf
fn standard_role(tag: &[u8]) -> bool {
    text_block(tag)
        || container(tag)
        || matches!(tag, b"L" | b"LI" | b"Lbl" | b"LBody")
        || matches!(
            tag,
            b"Document"
                | b"BlockQuote"
                | b"Caption"
                | b"TOC"
                | b"TOCI"
                | b"Index"
                | b"NonStruct"
                | b"Private"
                | b"Table"
                | b"TR"
                | b"TH"
                | b"TD"
                | b"THead"
                | b"TBody"
                | b"TFoot"
                | b"Span"
                | b"Quote"
                | b"Note"
                | b"Reference"
                | b"BibEntry"
                | b"Code"
                | b"Link"
                | b"Annot"
                | b"Ruby"
                | b"RB"
                | b"RT"
                | b"RP"
                | b"Warichu"
                | b"WT"
                | b"WP"
                | b"Figure"
                | b"Formula"
                | b"Form"
        )
}

fn role<'a>(roles: Option<&'a Dictionary>, tag: &'a [u8]) -> Result<&'a [u8], String> {
    roles
        .and_then(|roles| roles.get(tag).ok())
        .map(name)
        .transpose()
        .map(|mapped| mapped.unwrap_or(tag))
}

fn keys(dict: &Dictionary, allowed: &[&[u8]], context: &str) -> Result<(), String> {
    if let Some((key, _)) = dict
        .iter()
        .find(|(key, _)| !allowed.contains(&key.as_slice()))
    {
        // Only fixed PDF keywords may appear in a refusal. Unknown keys and
        // every value can carry document text, so never format them directly.
        let feature = match key.as_slice() {
            b"ActualText" => "ActualText",
            b"Alt" => "Alt",
            b"E" => "E",
            b"T" => "T",
            b"C" => "C",
            b"R" => "R",
            b"ID" => "ID",
            b"IDTree" => "IDTree",
            b"ClassMap" => "ClassMap",
            b"Kids" => "Kids",
            b"Limits" => "Limits",
            b"BBox" => "BBox",
            b"Stm" => "Stm",
            b"StmOwn" => "StmOwn",
            _ => "unrecognized",
        };
        return Err(format!(
            "unsupported {feature} metadata in tagged {context}"
        ));
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

const MAX_TREE_DEPTH: usize = 8;
const MAX_TREE_NODES: usize = 256;

// ISO 32000-1 7.9.7: a number tree's root holds Nums or Kids; intermediate
// nodes hold Kids with Limits; leaves hold Nums with Limits. Large Acrobat
// exports split the parent tree this way. Flatten it in key order, requiring
// each node's Limits to state its first and last key exactly, so the flat pair
// list is the same one a single-leaf tree would have supplied.
fn number_tree(doc: &Document, root: ObjectId) -> Result<Vec<&Object>, String> {
    let mut pairs = Vec::new();
    let mut visited = BTreeSet::new();
    // (node, depth, whether this node is the root)
    let mut pending = vec![(root, 0, true)];
    while let Some((id, depth, is_root)) = pending.pop() {
        if depth > MAX_TREE_DEPTH || !visited.insert(id) || visited.len() > MAX_TREE_NODES {
            return Err(INVALID.into());
        }
        let tree = node(doc, id)?;
        keys(
            tree,
            if is_root {
                &[b"Type", b"Nums", b"Kids"]
            } else {
                &[b"Type", b"Nums", b"Kids", b"Limits"]
            },
            "parent tree",
        )?;
        if tree
            .get(b"Type")
            .is_ok_and(|value| value.as_name().ok() != Some(b"ParentTree"))
            || tree.has(b"Nums") == tree.has(b"Kids")
            || (!is_root && !tree.has(b"Limits"))
        {
            return Err(INVALID.into());
        }
        if let Ok(nums) = tree.get(b"Nums") {
            let nums = array(crate::encoding::resolve(doc, nums))?;
            if nums.len() % 2 != 0 {
                return Err("tagged parent tree must contain one entry per page".into());
            }
            if pairs.len() + nums.len() > 2 * MAX_DOCUMENT_CONTENT {
                return Err(INVALID.into());
            }
            pairs.extend(nums.iter());
            if let Ok(limits) = tree.get(b"Limits") {
                let limits = array(crate::encoding::resolve(doc, limits))?;
                if limits.len() != 2
                    || nums.is_empty()
                    || integer(&limits[0])? != integer(&nums[0])?
                    || integer(&limits[1])? != integer(&nums[nums.len() - 2])?
                {
                    return Err(INVALID.into());
                }
            }
        } else {
            let kids = array(crate::encoding::resolve(doc, get(tree, b"Kids")?))?;
            if kids.is_empty() || kids.len() + pending.len() > MAX_TREE_NODES {
                return Err(INVALID.into());
            }
            // An intermediate node's Limits span its children's; each child's
            // own Limits are checked against its leaves when it is visited.
            if let Ok(limits) = tree.get(b"Limits") {
                let limits = array(crate::encoding::resolve(doc, limits))?;
                let first = array(crate::encoding::resolve(
                    doc,
                    get(node(doc, reference(&kids[0])?)?, b"Limits")?,
                ))?;
                let last = array(crate::encoding::resolve(
                    doc,
                    get(node(doc, reference(&kids[kids.len() - 1])?)?, b"Limits")?,
                ))?;
                if limits.len() != 2
                    || first.len() != 2
                    || last.len() != 2
                    || integer(&limits[0])? != integer(&first[0])?
                    || integer(&limits[1])? != integer(&last[1])?
                {
                    return Err(INVALID.into());
                }
            }
            for kid in kids.iter().rev() {
                pending.push((reference(kid)?, depth + 1, false));
            }
        }
    }
    // Key order across leaves is enforced by the caller's strictly increasing
    // key check, which now covers the concatenated leaves as well.
    Ok(pairs)
}

// What every element in one structure tree is checked against.
struct Scope<'a> {
    pages: &'a BTreeSet<ObjectId>,
    // The root's validated standard namespaces (ISO 32000-2 14.7.4).
    namespaces: &'a BTreeSet<ObjectId>,
    // The root's ClassMap, when it has one (ISO 32000-1 14.7.5.2).
    classes: Option<&'a Dictionary>,
}

// ISO 32000-2 Annex L: standard types whose meaning the PDF 2.0 namespace keeps
// from PDF 1.7. An element placed in that namespace is admitted only with one
// of these, so RoleMap (which only covers the default namespace) never applies.
fn common_to_both_namespaces(tag: &[u8]) -> bool {
    text_block(tag)
        || container(tag)
        || table_section(tag)
        || matches!(
            tag,
            b"Document"
                | b"L"
                | b"LI"
                | b"Lbl"
                | b"LBody"
                | b"Table"
                | b"TR"
                | b"TH"
                | b"TD"
                | b"Figure"
                | b"Link"
                | b"Span"
                | b"NonStruct"
                | b"TOC"
                | b"TOCI"
        )
}

// Returns the element's page and whether it pins its own content: whether it
// keeps metadata that describes the content as it now stands, so that content
// has to stay read-only for the metadata to remain true.
fn element(
    doc: &Document,
    dict: &Dictionary,
    parent: ObjectId,
    scope: &Scope,
    owns_text: bool,
    // The standard type the RoleMap gives this element; its attributes are
    // those of that type (PowerPoint maps Diagram and Chart to Figure).
    role: &[u8],
) -> Result<(Option<ObjectId>, bool), String> {
    // In particular: no ActualText, E or attribute revision. ActualText and E
    // replace the text an edit would produce, so they are refused outright.
    // Alt text replaces an element's content for assistive technology and a
    // non-empty title often repeats it; both are kept, and an element with
    // editable text that carries either is pinned rather than refused, so an
    // edit can never leave them describing the old wording.
    let tag = name(get(dict, b"S")?)?;
    let mut allowed: Vec<&[u8]> = vec![b"Type", b"S", b"P", b"Pg", b"K", b"A", b"Lang", b"T"];
    if tag == b"TH" {
        allowed.push(b"ID");
    }
    allowed.push(b"Alt");
    if scope.classes.is_some() && !matches!(tag, b"TD" | b"TH") {
        allowed.push(b"C");
    }
    allowed.push(b"NS");
    keys(dict, &allowed, "element")?;
    if let Ok(namespace) = dict.get(b"NS") {
        if !scope.namespaces.contains(&reference(namespace)?) || !common_to_both_namespaces(tag) {
            return Err("unsupported NS metadata in tagged element".into());
        }
    }
    // ISO 32000-1 Table 323: T is a human-readable title and Alt an alternate
    // description. Both are text strings; bound what is retained.
    for (key, limit) in [(b"T".as_slice(), 4096), (b"Alt", 65536)] {
        if dict
            .get(key)
            .is_ok_and(|value| !value.as_str().is_ok_and(|text| text.len() <= limit))
        {
            return Err(INVALID.into());
        }
    }
    // Alt stands in for everything the element contains, so it pins a
    // grouping element's descendants too. A container's title names the
    // section rather than repeating its text.
    let mut pins = dict.has(b"Alt")
        || (owns_text
            && dict
                .get(b"T")
                .is_ok_and(|value| value.as_str().is_ok_and(|text| !text.is_empty())));
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
        || page.is_some_and(|id| !scope.pages.contains(&id))
    {
        return Err(INVALID.into());
    }
    if let Ok(value) = dict.get(b"A") {
        // Cell attributes and identifiers are checked together with the IDTree
        // after this element's row/table ownership has been established.
        if matches!(tag, b"TD" | b"TH") {
            return Ok((page, pins));
        }
        // ISO 32000-1 14.7.5: an array holds several attribute objects, each
        // held to the same rules (PowerPoint writes two on a figure). Lists
        // keep their own singleton form. Revision numbers stay refused.
        match crate::encoding::resolve(doc, value) {
            Object::Array(objects) if role != b"L" => {
                if objects.is_empty() || objects.len() > 8 {
                    return Err(INVALID.into());
                }
                for object in objects {
                    pins |= attributes(doc, role, crate::encoding::resolve(doc, object))?;
                }
            }
            value => pins |= attributes(doc, role, value)?,
        }
    }
    // ISO 32000-1 14.7.5.2: a class names attribute objects kept in the
    // root's ClassMap. They apply exactly as the element's own A would, so
    // each is held to the same rules. A revision number may follow a name.
    if let Ok(value) = dict.get(b"C") {
        let classes = scope.classes.ok_or(INVALID)?;
        let names = match crate::encoding::resolve(doc, value) {
            value @ Object::Name(_) => std::slice::from_ref(value),
            Object::Array(values) if !values.is_empty() && values.len() <= 8 => values.as_slice(),
            _ => return Err(INVALID.into()),
        };
        let mut named = false;
        for entry in names {
            match entry {
                Object::Name(class) => {
                    let value =
                        crate::encoding::resolve(doc, classes.get(class).map_err(|_| INVALID)?);
                    let objects = match value {
                        Object::Array(values) if !values.is_empty() && values.len() <= 8 => {
                            values.as_slice()
                        }
                        value => std::slice::from_ref(value),
                    };
                    for object in objects {
                        pins |= attributes(doc, role, crate::encoding::resolve(doc, object))?;
                    }
                    named = true;
                }
                Object::Integer(revision) if named && *revision >= 0 => named = false,
                _ => return Err(INVALID.into()),
            }
        }
    }
    Ok((page, pins))
}

// One attribute object belonging to an element with this standard tag. Returns
// whether the object pins the element's content (see `element`).
fn attributes(doc: &Document, tag: &[u8], value: &Object) -> Result<bool, String> {
    // ISO 32000-1 14.8.5.6, Table 348: a form field's role and state for
    // assistive technology. A field's text is read-only, so none of it can go
    // stale; LiveCycle writes one on every check box.
    if tag == b"Form"
        && value.as_dict().is_ok_and(|entries| {
            entries.get(b"O").and_then(Object::as_name).ok() == Some(b"PrintField")
        })
    {
        let attributes = value.as_dict().map_err(|_| INVALID)?;
        keys(
            attributes,
            &[b"O", b"Role", b"checked", b"Checked", b"Desc"],
            "field attributes",
        )?;
        if attributes
            .get(b"Role")
            .is_ok_and(|role| !matches!(role.as_name(), Ok(b"rb" | b"cb" | b"pb" | b"tv" | b"lb")))
            || [b"checked".as_slice(), b"Checked"].iter().any(|key| {
                attributes
                    .get(key)
                    .is_ok_and(|state| !matches!(state.as_name(), Ok(b"on" | b"off" | b"neutral")))
            })
            || attributes
                .get(b"Desc")
                .is_ok_and(|text| !text.as_str().is_ok_and(|text| text.len() <= 65536))
        {
            return Err(INVALID.into());
        }
        return Ok(false);
    }
    // ISO 32000-1 Table 344: BBox is the element's own ink, not an authored
    // allocation, so retaining one is only sound while the content it
    // describes cannot move. A figure, link and field are already read-only;
    // a table that declares bounds makes its own text read-only too, which
    // is what the returned pin and `Tags::bounded` carry out.
    if matches!(tag, b"Figure" | b"Link" | b"Form" | b"Table") {
        let attributes = value.as_dict().map_err(|_| INVALID)?;
        // PowerPoint also writes the block indents and spacing on figures.
        // They are the element's authored allocation and move nothing.
        keys(
            attributes,
            &[
                b"O",
                b"BBox",
                b"Placement",
                b"Width",
                b"Height",
                b"StartIndent",
                b"EndIndent",
                b"SpaceBefore",
                b"SpaceAfter",
                b"WritingMode",
            ],
            "bounded attributes",
        )?;
        for key in [
            b"StartIndent".as_slice(),
            b"EndIndent",
            b"SpaceBefore",
            b"SpaceAfter",
        ] {
            if let Ok(value) = attributes.get(key) {
                super::number(value)?;
            }
        }
        default_writing_mode(attributes)?;
        // Table 343: the element's authored size, a number or Auto. It
        // describes read-only content, so it cannot become stale either.
        for key in [b"Width".as_slice(), b"Height"] {
            if attributes.get(key).is_ok_and(|value| {
                value.as_name().ok() != Some(b"Auto")
                    && !super::number(value).is_ok_and(|size| size.is_finite() && size >= 0.0)
            }) {
                return Err(INVALID.into());
            }
        }
        if name(get(attributes, b"O")?)? != b"Layout" {
            return Err(INVALID.into());
        }
        if attributes.get(b"Placement").is_ok_and(|value| {
            !value.as_name().is_ok_and(|name| {
                matches!(name, b"Block" | b"Inline" | b"Before" | b"Start" | b"End")
            })
        }) {
            return Err(INVALID.into());
        }
        if bounding_box(doc, attributes)? {
            return Ok(tag == b"Table");
        }
        return Ok(false);
    }
    if tag == b"L" {
        // A single List attribute dictionary, optionally wrapped as emitted
        // by Chromium. Numbering describes labels; it is not an ink bound.
        let attributes = match value {
            Object::Array(items) if items.len() == 1 => crate::encoding::resolve(doc, &items[0]),
            value => value,
        };
        let attributes = attributes.as_dict().map_err(|_| INVALID)?;
        keys(attributes, &[b"O", b"ListNumbering"], "list attributes")?;
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
        return Ok(false);
    }
    let attributes = value.as_dict().map_err(|_| INVALID)?;
    let leaf = matches!(tag, b"NonStruct" | b"Span");
    // ISO 32000-1 Table 343: LineHeight is the distance between adjacent
    // baselines. A fixed-position edit moves no line, so it stays true, and it
    // is the one layout attribute an inline leaf may carry (InDesign's classes).
    let allowed: &[&[u8]] = if leaf {
        &[b"O", b"LineHeight"]
    } else {
        &[
            b"O",
            b"Placement",
            b"StartIndent",
            b"EndIndent",
            b"SpaceBefore",
            b"SpaceAfter",
            b"TextIndent",
            b"TextAlign",
            b"LineHeight",
            b"WritingMode",
            b"BBox",
        ]
    };
    keys(attributes, allowed, "layout attributes")?;
    // PowerPoint records each list label's ink as a BBox. Like a table's, it
    // stays true only while the label's text does not change, so it pins it.
    if tag != b"Lbl" && attributes.has(b"BBox") {
        return Err("unsupported BBox metadata in tagged layout attributes".into());
    }
    let bounded = bounding_box(doc, attributes)?;
    // Table 343: a list label sits inline beside its body (PowerPoint says
    // so explicitly); every other block is placed as a block.
    let placement: &[u8] = if tag == b"Lbl" { b"Inline" } else { b"Block" };
    if name(get(attributes, b"O")?)? != b"Layout"
        || attributes.get(b"Placement").is_ok_and(
            |value| !matches!(value.as_name(), Ok(name) if name == b"Block" || name == placement),
        )
        || (leaf && !attributes.has(b"LineHeight"))
    {
        return Err(INVALID.into());
    }
    default_writing_mode(attributes)?;
    if let Ok(height) = attributes.get(b"LineHeight") {
        if !matches!(height.as_name(), Ok(b"Normal" | b"Auto")) && super::number(height)? < 0.0 {
            return Err(INVALID.into());
        }
    }
    for key in [
        b"StartIndent".as_slice(),
        b"EndIndent",
        b"SpaceBefore",
        b"SpaceAfter",
        b"TextIndent",
    ] {
        if let Ok(indent) = attributes.get(key) {
            // ISO 32000-1 14.8.5.4: authored block allocation constraints,
            // not ink bounds. A fitting fixed-position edit retains these
            // indents and inter-paragraph spacing without reflowing text.
            // TextIndent offsets only the first line from StartIndent;
            // its origin stays fixed even when that line gets shorter.
            if tag == b"Document" {
                return Err(INVALID.into());
            }
            super::number(indent)?;
        }
    }
    // Table 343: alignment within the block. Start is the default and holds
    // wherever the line ends. Centre, end and justified alignment describe
    // where the existing line sits relative to its width, which an edit that
    // changes the width would falsify, so those blocks keep their text.
    match attributes.get(b"TextAlign") {
        Err(_) => Ok(bounded),
        Ok(value) => match name(value)? {
            b"Start" => Ok(bounded),
            b"Center" | b"End" | b"Justify" => Ok(true),
            _ => Err(INVALID.into()),
        },
    }
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

// ISO 32000-1 Table 323: K is optional. Word exports empty text boxes with no
// K at all; they own nothing, exactly like an empty K array.
fn kids<'a>(doc: &'a Document, element: &'a Dictionary) -> Result<&'a [Object], String> {
    match element.get(b"K") {
        Ok(value) => children(doc, value),
        Err(_) => Ok(&[]),
    }
}

struct Group<'a> {
    id: ObjectId,
    page: Option<ObjectId>,
    tag: &'a [u8],
    items: Vec<&'a Object>,
    // Whether this group or an ancestor pins its content (see `element`).
    pinned: bool,
}

// Exactly one optional NonStruct/Span level below a paragraph, never a recursive
// tree walk. Empty exported cells/paragraphs are retained; the node budget is
// separate from the number of marked-content owners.
fn groups<'a>(
    doc: &'a Document,
    paragraph: Group<'a>,
    scope: &Scope,
    ids: &mut BTreeSet<ObjectId>,
    nested: &mut Vec<(&'a Object, ObjectId, bool)>,
) -> Result<Vec<Group<'a>>, String> {
    if paragraph.items.len() > MAX_NODES {
        return Err(INVALID.into());
    }
    let mut plain = Group {
        items: Vec::new(),
        ..paragraph
    };
    let mut groups = Vec::new();
    for item in paragraph.items {
        if let Object::Reference(id) = item {
            // Word and Acrobat nest a sublist inside the list body rather than
            // beside it. A list is a container, so it goes back to the walk,
            // which owns the depth and container bounds; claiming its id here
            // would make the walk refuse it as a second visit.
            // PowerPoint nests the pictures of a grouped drawing as figures in
            // a figure; each goes back to the walk the same way, read-only.
            if node(doc, *id).is_ok_and(|child| {
                get(child, b"S")
                    .and_then(name)
                    .is_ok_and(|tag| goes_back_to_the_walk(plain.tag, tag))
            }) {
                nested.push((item, plain.id, plain.pinned));
                continue;
            }
            if !ids.insert(*id) || ids.len() > MAX_NODES {
                return Err(INVALID.into());
            }
            let child = node(doc, *id)?;
            let (page, pins) = element(
                doc,
                child,
                plain.id,
                scope,
                !annotation_owner(name(get(child, b"S")?)?),
                name(get(child, b"S")?)?,
            )?;
            let tag = name(get(child, b"S")?)?;
            if plain.tag == b"Figure"
                || annotation_owner(plain.tag)
                || (!matches!(tag, b"NonStruct" | b"Span")
                    && !annotation_owner(tag)
                    && !(plain.tag == b"LI" && matches!(tag, b"Lbl" | b"LBody"))
                    && !(matches!(plain.tag, b"TD" | b"TH")
                        && (text_block(tag) || tag == b"Figure")))
            {
                return Err(INVALID.into());
            }
            // A label's or body's layout attributes were checked by `element`
            // like a paragraph's, and any pin they carry joins the group's.
            let items = kids(doc, child)?;
            if items.len() > MAX_NODES {
                return Err(INVALID.into());
            }
            let group = Group {
                id: *id,
                page,
                tag,
                items: items.iter().collect(),
                pinned: plain.pinned || pins,
            };
            // Cell -> paragraph -> optional Span/NonStruct leaf. Only the cell
            // branch recurses, so this adds one bounded level, not arbitrary trees.
            // LI -> LBody -> optional Link/Form leaf is the same single extra
            // level; LBody is a leaf target, so this cannot recurse further.
            if (matches!(plain.tag, b"TD" | b"TH") && text_block(tag))
                || (plain.tag == b"LI" && tag == b"LBody")
            {
                groups.extend(self::groups(doc, group, scope, ids, nested)?);
            } else {
                groups.push(group);
            }
        } else {
            plain.items.push(item);
        }
    }
    if !plain.items.is_empty() || groups.is_empty() {
        groups.push(plain);
    }
    Ok(groups)
}

// Table 344: an element's ink bounds, four ordered numbers. Whether it is
// present decides whether the element's content has to stay where it is.
fn bounding_box(doc: &Document, attributes: &Dictionary) -> Result<bool, String> {
    let Ok(bounds) = attributes.get(b"BBox") else {
        return Ok(false);
    };
    let bounds = array(crate::encoding::resolve(doc, bounds))?;
    if bounds.len() != 4 {
        return Err(INVALID.into());
    }
    let bounds = bounds
        .iter()
        .map(super::number)
        .collect::<Result<Vec<_>, _>>()?;
    if bounds[0] > bounds[2] || bounds[1] > bounds[3] {
        return Err(INVALID.into());
    }
    Ok(true)
}

// Table 343: LrTb (left to right, lines top to bottom) is the default writing
// mode, which PowerPoint writes out on every paragraph. Other modes describe
// text the editor would write in the wrong direction, and stay refused.
fn default_writing_mode(attributes: &Dictionary) -> Result<(), String> {
    match attributes.get(b"WritingMode") {
        Ok(value) if value.as_name().ok() != Some(b"LrTb") => Err(INVALID.into()),
        _ => Ok(()),
    }
}

// A child element the content walk returns to the tree walk, which owns the
// depth and container bounds: a sublist in a list body (Word, Acrobat) and a
// figure in a figure (PowerPoint's grouped drawings).
fn goes_back_to_the_walk(parent: &[u8], child: &[u8]) -> bool {
    matches!((parent, child), (b"LBody", b"L") | (b"Figure", b"Figure"))
}

// ISO 32000-1 14.8.4.2: a table of contents and its entries group elements.
fn toc(tag: &[u8]) -> bool {
    matches!(tag, b"TOC" | b"TOCI")
}

// ISO 32000-1 14.8.4.3.4: optional row groups between a table and its rows.
fn table_section(tag: &[u8]) -> bool {
    matches!(tag, b"THead" | b"TBody" | b"TFoot")
}

// ISO 32000-1 14.7.4.3 and 14.8.4.4: Link and Form elements own annotations
// through object references. Only the literal standard names are admitted.
fn annotation_owner(tag: &[u8]) -> bool {
    matches!(tag, b"Link" | b"Form")
}

// An OBJR must name an annotation on a known page whose StructParent entry
// points back at this element. Each parent-tree entry is claimed exactly once;
// the annotation itself is preserved unchanged and its owner's text read-only.
fn annotation(
    doc: &Document,
    objr: &Dictionary,
    tag: &[u8],
    owner: ObjectId,
    page: Option<ObjectId>,
    pages: &BTreeSet<ObjectId>,
    objects: &mut BTreeMap<i64, ObjectId>,
) -> Result<(), String> {
    keys(objr, &[b"Type", b"Pg", b"Obj"], "object reference")?;
    if !annotation_owner(tag) {
        return Err(INVALID.into());
    }
    let page = match objr.get(b"Pg") {
        Ok(value) => reference(value)?,
        Err(_) => page.ok_or(INVALID)?,
    };
    let target = reference(get(objr, b"Obj")?)?;
    let dict = node(doc, target)?;
    let subtype = name(get(dict, b"Subtype")?)?;
    let key = integer(get(dict, b"StructParent")?)?;
    let listed = crate::encoding::resolve(doc, get(node(doc, page)?, b"Annots")?)
        .as_array()
        .is_ok_and(|annots| annots.iter().any(|a| a.as_reference().ok() == Some(target)));
    if !pages.contains(&page)
        || subtype
            != if tag == b"Link" {
                b"Link".as_slice()
            } else {
                b"Widget"
            }
        || dict
            .get(b"P")
            .is_ok_and(|p| p.as_reference().ok() != Some(page))
        || !listed
        || objects.remove(&key) != Some(owner)
    {
        return Err("tagged annotation and parent tree disagree on ownership".into());
    }
    Ok(())
}

#[derive(Default)]
pub(super) struct Tags {
    // One authored tag name per MCID. Empty means an ordinary untagged page.
    names: Vec<Vec<u8>>,
    // The block element that owns each MCID: the paragraph, heading, list item
    // or cell whose text it is, above any Span or NonStruct leaf that claims
    // it. Two runs with the same entry are lines of one block, which is the
    // one thing a wrap needs that geometry cannot say (`layout::wrap`).
    blocks: Vec<Option<ObjectId>>,
    // MCIDs under an element whose authored ink bounds are kept on save, so
    // their text must stay where the bounds say it is (ISO 32000-1 Table 344).
    bounded: BTreeSet<usize>,
    seen: BTreeSet<usize>,
    active: Option<Option<usize>>, // None outside; Some(None) is an artifact.
    has_content: bool,
}

impl Tags {
    pub(super) fn read(doc: &Document, page: ObjectId, pages: &[ObjectId]) -> Result<Self, String> {
        let catalog = doc.catalog().map_err(|_| INVALID)?;
        let page_dict = node(doc, page)?;
        let Ok(root) = catalog.get(b"StructTreeRoot") else {
            // Some untagged exports retain the page's former parent-tree index.
            // Without a structure tree it has no target and changes no text
            // semantics. Preserve it; begin() still refuses structural marked
            // content without a tree, and inline ActualText keeps its own checks.
            if page_dict.has(b"StructParent")
                || page_dict.get(b"StructParents").is_ok_and(|value| {
                    !value
                        .as_i64()
                        .is_ok_and(|key| (0..=1_000_000).contains(&key))
                })
            {
                return Err(INVALID.into());
            }
            return Ok(Self::default());
        };
        // The grouping walk below is iterative, with independent depth/count bounds.
        if pages.is_empty() || pages.len() > 128 {
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
                b"IDTree",
                b"Namespaces",
                b"ClassMap",
            ],
            "structure root",
        )?;
        // Word 365 declares the PDF 2.0 standard namespace. Only the two
        // standard namespaces are admitted, with no role maps or schemas.
        let mut namespaces = BTreeSet::new();
        if let Ok(list) = root.get(b"Namespaces") {
            let list = array(crate::encoding::resolve(doc, list))?;
            if list.len() > 8 {
                return Err(INVALID.into());
            }
            for entry in list {
                let id = reference(entry)?;
                let namespace = node(doc, id)?;
                keys(namespace, &[b"Type", b"NS"], "namespace")?;
                if namespace
                    .get(b"Type")
                    .is_ok_and(|value| value.as_name().ok() != Some(b"Namespace"))
                    || !matches!(
                        get(namespace, b"NS")?.as_str().map_err(|_| INVALID)?,
                        b"http://iso.org/pdf2/ssn" | b"http://iso.org/pdf/ssn"
                    )
                    || !namespaces.insert(id)
                {
                    return Err("unsupported tagged namespace".into());
                }
            }
        }
        if name(get(root, b"Type")?)? != b"StructTreeRoot" {
            return Err(INVALID.into());
        }
        let mut tables = tables::Tables::read(doc, root)?;
        let roles = root
            .get(b"RoleMap")
            .ok()
            .map(|value| crate::encoding::resolve(doc, value).as_dict())
            .transpose()
            .map_err(|_| INVALID)?;
        if let Some(roles) = roles {
            if roles.len() > 128
                || roles.iter().any(|(key, value)| {
                    key.is_empty()
                        || key.len() > 127
                        || standard_role(key)
                        || !value.as_name().is_ok_and(standard_role)
                })
            {
                return Err("tagged RoleMap contains unsupported or conflicting roles".into());
            }
        }
        // The structure root may own several elements, including material added
        // alongside the producer's Document element by a later PDF editor.
        let paragraphs = children(doc, get(root, b"K")?)?;
        if paragraphs.is_empty() {
            return Err(INVALID.into());
        }
        // ISO 32000-1 14.7.4.4: StructParents indexes the number tree; the MCID
        // indexes its array. Require both directions to agree, not just /K.
        let nums = number_tree(doc, reference(get(root, b"ParentTree")?)?)?;
        let nums = nums.as_slice();
        // A parent tree indexes page MCID arrays and, through StructParent,
        // single annotations. Annotation entries reference one structure
        // element directly; they are claimed by an OBJR during the walk below.
        let mut page_entries = Vec::new();
        let mut objects = BTreeMap::new();
        let mut previous = -1;
        for pair in nums.chunks_exact(2) {
            let key = integer(pair[0])?;
            if key <= previous || !(0..=1_000_000).contains(&key) {
                return Err(INVALID.into());
            }
            previous = key;
            if crate::encoding::resolve(doc, pair[1]).as_dict().is_ok() {
                if objects.len() >= MAX_DOCUMENT_CONTENT {
                    return Err("tagged parent content is empty or exceeds its limit".into());
                }
                objects.insert(key, reference(pair[1])?);
            } else {
                page_entries.push((key, pair[1]));
            }
        }
        let mut page_keys = BTreeMap::new();
        for &id in pages {
            let dict = node(doc, id)?;
            // A tagged document can contain untagged pages, such as inserted
            // scans. They own no MCIDs; an element naming one is refused below.
            let Ok(key) = dict.get(b"StructParents") else {
                if dict.has(b"StructParent") {
                    return Err(INVALID.into());
                }
                continue;
            };
            let key = integer(key)?;
            if dict.has(b"StructParent")
                || !(0..=1_000_000).contains(&key)
                || page_keys.insert(key, id).is_some()
            {
                return Err(INVALID.into());
            }
        }
        // Diagnose a missing or surplus page entry before reading any entry.
        if page_entries.len() != page_keys.len()
            || page_entries
                .iter()
                .any(|(key, _)| !page_keys.contains_key(key))
        {
            return Err("tagged parent tree must contain one entry per page".into());
        }
        let mut by_page = BTreeMap::new();
        let mut total = 0;
        let mut slots = 0;
        for (key, value) in page_entries {
            let owner = page_keys
                .remove(&key)
                .ok_or("tagged parent tree must contain one entry per page")?;
            let entry = match value {
                Object::Reference(id) => doc.get_object(*id).map_err(|_| INVALID)?,
                value => value,
            };
            let entries = array(entry)?;
            // ISO 32000-1 14.7.4.4: a null slot is an MCID no element owns.
            // Word leaves these for skipped sequences; the content stream must
            // not use them, which begin() enforces through the empty name.
            total += entries
                .iter()
                .filter(|entry| !matches!(entry, Object::Null))
                .count();
            slots += entries.len();
            if entries.is_empty()
                || entries.len() > MAX_CONTENT_ITEMS
                || slots > MAX_DOCUMENT_CONTENT
            {
                return Err("tagged parent content is empty or exceeds its limit".into());
            }
            by_page.insert(
                owner,
                (
                    entries,
                    vec![Vec::new(); entries.len()],
                    vec![None; entries.len()],
                ),
            );
        }
        if let Ok(next) = root.get(b"ParentTreeNextKey") {
            let next = integer(next)?;
            if next <= previous || next > 1_000_001 {
                return Err(INVALID.into());
            }
        }
        // Every page that declares StructParents must have its entry.
        if !page_keys.is_empty() {
            return Err("tagged parent tree must contain one entry per page".into());
        }
        if paragraphs.len() > MAX_NODES {
            return Err(INVALID.into());
        }
        // Classes are checked where an element names them, against the rules
        // for that element's own attributes; one nobody names changes nothing.
        let classes = root
            .get(b"ClassMap")
            .ok()
            .map(|value| crate::encoding::resolve(doc, value).as_dict())
            .transpose()
            .map_err(|_| INVALID)?;
        if classes.is_some_and(|classes| classes.len() > 256) {
            return Err(INVALID.into());
        }
        let scope = Scope {
            pages: &page_ids,
            namespaces: &namespaces,
            classes,
        };
        let mut assigned = 0;
        let mut ids = page_ids.clone();
        ids.insert(root_id);
        let mut pending: Vec<_> = paragraphs
            .iter()
            .rev()
            .map(|child| (child, root_id, 0, false))
            .collect();
        let mut bounded_slots: BTreeMap<ObjectId, BTreeSet<usize>> = BTreeMap::new();
        let mut containers = 0;
        while let Some((child, parent_id, depth, bounded)) = pending.pop() {
            let id = reference(child)?;
            if !ids.insert(id) || ids.len() > MAX_NODES {
                return Err(INVALID.into());
            }
            let child = node(doc, id)?;
            let tag = name(get(child, b"S")?)?;
            let role = role(roles, tag)?;
            let owns_text = !(container(role)
                || annotation_owner(role)
                || matches!(role, b"Document" | b"Figure" | b"L" | b"Table" | b"TR"));
            let (paragraph_page, pins) = element(doc, child, parent_id, &scope, owns_text, role)?;
            // Metadata that describes this element's content as it stands
            // (a table's ink bounds, alternate text, a title, a centred line)
            // makes that content and everything below it read-only.
            let bounded = bounded || pins;
            let items = kids(doc, child)?;
            if role == b"Document" {
                if parent_id != root_id
                    || items.is_empty()
                    || items.len() + pending.len() > MAX_NODES
                {
                    return Err(INVALID.into());
                }
                pending.extend(items.iter().rev().map(|item| (item, id, depth, bounded)));
                continue;
            }
            // Some producers retain nameless, empty structure placeholders.
            // They may own no MCID and carry no attributes or semantics.
            if tag.is_empty() && items.is_empty() && !child.has(b"A") {
                continue;
            }
            // A figure alias (PowerPoint's Diagram and Chart) is read-only like
            // any figure; its group carries the mapped role below.
            if tag != role && !text_block(role) && !container(role) && role != b"Figure" {
                return Err("tagged element role is not editable yet".into());
            }
            let mut content_items = Vec::new();
            if container(role)
                || matches!(role, b"NonStruct" | b"L" | b"Table" | b"TR")
                || table_section(role)
                || toc(role)
            {
                containers += 1;
                // Bound the work list before copying child references into it.
                if items.len() + pending.len() > MAX_NODES {
                    return Err("tagged grouping frontier exceeds its limit".into());
                }
                if depth >= MAX_CONTAINER_DEPTH
                    || containers > MAX_CONTAINERS
                    || (!matches!(role, b"L" | b"Table") && child.has(b"A"))
                {
                    return Err(INVALID.into());
                }
                if role == b"L" {
                    // Word nests a sublist directly in its parent list.
                    for item in items {
                        if !matches!(
                            name(get(node(doc, reference(item)?)?, b"S")?)?,
                            b"LI" | b"L"
                        ) {
                            return Err(INVALID.into());
                        }
                    }
                }
                // ISO 32000-1 14.8.4.2: a table of contents holds entries and
                // nested tables of contents; an entry holds the blocks, which
                // own the content. LibreOffice and Word export one per index.
                if role == b"TOC" {
                    for item in items {
                        if !toc(name(get(node(doc, reference(item)?)?, b"S")?)?) {
                            return Err(INVALID.into());
                        }
                    }
                }
                if role == b"TOCI" && name(get(node(doc, parent_id)?, b"S")?)? != b"TOC" {
                    return Err(INVALID.into());
                }
                if role == b"TR" || table_section(role) {
                    // The structure root has no S; rows and sections never sit there.
                    let parent_tag = name(get(node(doc, parent_id)?, b"S")?)?;
                    if parent_tag != b"Table" && !(role == b"TR" && table_section(parent_tag)) {
                        return Err(INVALID.into());
                    }
                }
                if matches!(role, b"Table" | b"TR") || table_section(role) {
                    let mut descendants = 0;
                    for item in items {
                        if role == b"Table" && !matches!(item, Object::Reference(_)) {
                            content_items.push(item);
                        } else {
                            let tag = name(get(node(doc, reference(item)?)?, b"S")?)?;
                            if !(if role == b"Table" {
                                tag == b"TR" || table_section(tag)
                            } else if role == b"TR" {
                                matches!(tag, b"TD" | b"TH")
                            } else {
                                tag == b"TR"
                            }) {
                                return Err(INVALID.into());
                            }
                            descendants += 1;
                        }
                    }
                    if descendants == 0 && !items.is_empty() {
                        return Err(INVALID.into());
                    }
                }
                // Container Pg never supplies a descendant's page. Its own
                // identity is the immediate parent checked on every child.
                // A table that keeps its own ink bounds makes every descendant
                // read-only: an edit inside a cell would leave those bounds
                // describing content that is no longer there.
                pending.extend(
                    items
                        .iter()
                        .rev()
                        .filter(|item| role != b"Table" || matches!(item, Object::Reference(_)))
                        .map(|item| (item, id, depth + 1, bounded)),
                );
                if content_items.is_empty() {
                    continue;
                }
            }
            if !text_block(role)
                && !matches!(role, b"LI" | b"TD" | b"TH" | b"Table" | b"Figure")
                && !(tag == role && annotation_owner(role))
            {
                return Err("tagged element role is not editable yet".into());
            }
            if matches!(role, b"TD" | b"TH") {
                if name(get(node(doc, parent_id)?, b"S")?)? != b"TR" {
                    return Err(INVALID.into());
                }
                // Rows may sit in THead/TBody/TFoot; header links stay per table.
                let mut table = reference(get(node(doc, parent_id)?, b"P")?)?;
                if table_section(name(get(node(doc, table)?, b"S")?)?) {
                    table = reference(get(node(doc, table)?, b"P")?)?;
                }
                tables.cell(doc, child, id, table)?;
            }
            if role == b"LI"
                && (name(get(node(doc, parent_id)?, b"S")?)? != b"L" || child.has(b"A"))
            {
                return Err(INVALID.into());
            }
            if matches!(role, b"LI" | b"TD" | b"TH") {
                // Nested lists use the same iterative walk and container bounds.
                // LI and table cells own content; they do not add a container
                // level. Word places lists directly inside table cells.
                if items.len() + pending.len() > MAX_NODES {
                    return Err("tagged list frontier exceeds its limit".into());
                }
                for item in items.iter().rev() {
                    if matches!(item, Object::Reference(_))
                        && name(get(node(doc, reference(item)?)?, b"S")?)? == b"L"
                    {
                        pending.push((item, id, depth, bounded));
                    } else {
                        content_items.push(item);
                    }
                }
                content_items.reverse();
            } else if role != b"Table" {
                content_items.extend(items);
            }
            // The mapped role, so that text under a figure alias is refused
            // as figure text wherever a group's tag is read.
            // The element whose content this is, before `produced` below
            // shadows `id` with the leaf that claims each item. A link or a
            // field set inside a paragraph is one of those leaves, so its text
            // is the paragraph's too: read-only, it is text of the block a
            // wrap cannot move rather than text it would leave behind.
            let block = id;
            let paragraph = Group {
                id,
                page: paragraph_page,
                tag: if role == b"Figure" { role } else { tag },
                items: content_items,
                pinned: bounded,
            };
            let mut deferred = Vec::new();
            let produced = groups(doc, paragraph, &scope, &mut ids, &mut deferred)?;
            if deferred.len() + pending.len() > MAX_NODES {
                return Err("tagged sublist frontier exceeds its limit".into());
            }
            pending.extend(
                deferred
                    .into_iter()
                    .map(|(item, parent, pinned)| (item, parent, depth, pinned)),
            );
            for Group {
                id,
                page: paragraph_page,
                tag,
                items,
                pinned,
            } in produced
            {
                for item in items {
                    if let Object::Dictionary(objr) = item {
                        if objr
                            .get(b"Type")
                            .is_ok_and(|kind| kind.as_name().ok() == Some(b"OBJR"))
                        {
                            annotation(
                                doc,
                                objr,
                                tag,
                                id,
                                paragraph_page,
                                &page_ids,
                                &mut objects,
                            )?;
                            continue;
                        }
                    }
                    // ISO 32000-1 14.7.4.2: an integer uses the element's Pg;
                    // an MCR names a sequence on its own page. No external streams.
                    let (owner, mcid) = match item {
                        Object::Integer(mcid) => (paragraph_page.ok_or(INVALID)?, *mcid),
                        Object::Dictionary(mcr) => {
                            keys(mcr, &[b"Type", b"Pg", b"MCID"], "content reference")?;
                            if name(get(mcr, b"Type")?)? != b"MCR" {
                                return Err(INVALID.into());
                            }
                            (reference(get(mcr, b"Pg")?)?, integer(get(mcr, b"MCID")?)?)
                        }
                        _ => return Err(INVALID.into()),
                    };
                    let (entries, names, blocks) = by_page.get_mut(&owner).ok_or(INVALID)?;
                    let mcid = usize::try_from(mcid).map_err(|_| INVALID)?;
                    if mcid >= names.len() || reference(&entries[mcid])? != id {
                        return Err("tagged content and parent tree disagree on ownership".into());
                    }
                    // Acrobat can list one MCR several times in the same element.
                    // The slot names this element, and every earlier claim had
                    // to match its slot too, so a repeat is this element again.
                    if !names[mcid].is_empty() {
                        continue;
                    }
                    assigned += 1;
                    if assigned > total {
                        return Err(INVALID.into());
                    }
                    names[mcid] = tag.to_vec();
                    blocks[mcid] = Some(block);
                    if pinned {
                        bounded_slots.entry(owner).or_default().insert(mcid);
                    }
                }
            }
        }
        tables.finish()?;
        if !objects.is_empty() {
            return Err("tagged parent tree has annotation entries no element claims".into());
        }
        // Acrobat leaves parent-tree slots naming elements it has since deleted
        // or retagged as artifacts; nothing in the logical structure reaches
        // them. Their content is kept read-only under any tag. A reachable
        // element that fails to claim its slot is still inconsistent.
        for (entries, names, _) in by_page.values_mut() {
            for (slot, name) in entries.iter().zip(names.iter_mut()) {
                if !name.is_empty() || matches!(slot, Object::Null) {
                    continue;
                }
                let id = reference(slot)?;
                if ids.contains(&id) {
                    return Err(INVALID.into());
                }
                self::name(get(node(doc, id)?, b"S")?)?;
                *name = ORPHAN.to_vec();
            }
        }
        // Every non-null slot is now claimed, orphaned or refused above, so no
        // separate claimed-versus-total comparison is needed (it could not fail).
        let (names, blocks) = by_page
            .remove(&page)
            .map(|(_, names, blocks)| (names, blocks))
            .unwrap_or_default();
        Ok(Self {
            names,
            blocks,
            bounded: bounded_slots.remove(&page).unwrap_or_default(),
            ..Self::default()
        })
    }

    // ISO 32000-1 14.8.2.2, Table 330 (and PDF 2.0 subtypes): Word, Acrobat
    // and LiveCycle describe running headers, footers and watermarks with an
    // artifact property list. It names the artifact's own placement, which an
    // edit elsewhere cannot make stale; the artifact's text stays read-only.
    pub(super) fn artifact(&mut self, properties: &Object) -> Result<(), String> {
        const ARTIFACT: &str = "unsupported artifact properties";
        let dict = properties.as_dict().map_err(|_| ARTIFACT)?;
        for (key, value) in dict.iter() {
            let valid = match key.as_slice() {
                b"Type" => value.as_name().is_ok_and(|kind| {
                    matches!(
                        kind,
                        b"Pagination" | b"Layout" | b"Page" | b"Background" | b"Inline"
                    )
                }),
                b"Subtype" => value.as_name().is_ok_and(|kind| {
                    matches!(
                        kind,
                        b"Header"
                            | b"Footer"
                            | b"Watermark"
                            | b"PageNum"
                            | b"Bates"
                            | b"LineNum"
                            | b"Redaction"
                    )
                }),
                b"Attached" => value.as_array().is_ok_and(|edges| {
                    edges.len() <= 4
                        && edges.iter().all(|edge| {
                            edge.as_name().is_ok_and(|edge| {
                                matches!(edge, b"Top" | b"Bottom" | b"Left" | b"Right")
                            })
                        })
                }),
                b"BBox" => value.as_array().is_ok_and(|bounds| {
                    bounds.len() == 4
                        && bounds
                            .iter()
                            .all(|value| super::number(value).is_ok_and(f64::is_finite))
                }),
                // Acrobat's alternate description of the artifact itself.
                b"Contents" => value.as_str().is_ok_and(|text| text.len() <= 65536),
                _ => false,
            };
            if !valid {
                return Err(ARTIFACT.into());
            }
        }
        self.begin(&Object::Name(b"Artifact".to_vec()), None)
    }

    pub(super) fn begin(
        &mut self,
        tag: &Object,
        properties: Option<&Object>,
    ) -> Result<(), String> {
        // An artifact names no structure element, so an untagged page may
        // carry one (Acrobat stamps page numbers on scans this way).
        if self.names.is_empty() && (properties.is_some() || name(tag)? != b"Artifact") {
            return Err("marked content has no supported structure tree".into());
        }
        if self.active.is_some() {
            return Err("nested marked content is not editable yet".into());
        }
        let tag = name(tag)?;
        let mcid = if let Some(properties) = properties {
            let properties = properties.as_dict().map_err(|_| INVALID)?;
            keys(properties, &[b"MCID"], "marked-content properties")?;
            let mcid = usize::try_from(integer(get(properties, b"MCID")?)?).map_err(|_| INVALID)?;
            // An empty name is a null parent-tree slot, owned by nothing.
            let owner = self
                .names
                .get(mcid)
                .map(Vec::as_slice)
                .filter(|owner| !owner.is_empty());
            // The structure element, not the stream's descriptive tag, owns
            // the semantics (ISO 32000-1 14.7.4.2). Producers label sequences
            // loosely: Word writes Span or P for list bodies and figures, and
            // LiveCycle writes Content. Only Artifact contradicts an owner.
            // An Artifact on an unowned (null) or orphaned slot is an artifact,
            // Acrobat's form for retagged headers; it stays read-only.
            if tag == b"Artifact"
                && mcid < self.names.len()
                && owner.is_none_or(|owner| owner == ORPHAN)
            {
                None
            } else {
                if owner.is_none() || tag == b"Artifact" || !self.seen.insert(mcid) {
                    return Err("marked content repeats or disagrees with its structure tag".into());
                }
                Some(mcid)
            }
        } else {
            if tag != b"Artifact" {
                return Err("marked content without MCID must be an Artifact".into());
            }
            None
        };
        self.active = Some(mcid);
        self.has_content = false;
        Ok(())
    }

    pub(super) fn end(&mut self) -> Result<(), String> {
        let active = self.active.take().ok_or("unmatched marked-content end")?;
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
        if self
            .active
            .flatten()
            .is_some_and(|mcid| matches!(self.names[mcid].as_slice(), b"Table" | b"Figure"))
        {
            return Err("text outside a tagged table cell is not editable".into());
        }
        self.has_content = true;
        Ok(())
    }

    // Artifacts and later untagged additions keep their bytes and reserve their
    // glyph bounds, without blocking edits to properly owned paragraphs.
    pub(super) fn read_only(&self) -> bool {
        match self.active {
            // A link's rectangle and a field's widget are placed over this
            // text; changing it would leave them pointing at stale glyphs.
            Some(Some(mcid)) => {
                annotation_owner(&self.names[mcid])
                    || self.names[mcid] == ORPHAN
                    || self.bounded.contains(&mcid)
            }
            // An artifact is never the document's content, tagged or not.
            Some(None) => true,
            None => !self.names.is_empty(),
        }
    }

    /// The block element owning the marked content now open, where there is
    /// one: `None` outside marked content, in an artifact, and on an untagged
    /// page. An orphaned slot has no owner, and read-only text never asks.
    pub(super) fn block(&self) -> Option<ObjectId> {
        self.active
            .flatten()
            .and_then(|mcid| self.blocks.get(mcid).copied().flatten())
    }

    pub(super) fn finish(&self) -> Result<(), String> {
        // An orphaned slot need not appear in the content at all.
        let owned = self
            .names
            .iter()
            .filter(|name| !name.is_empty() && name.as_slice() != ORPHAN)
            .count();
        let used = self
            .seen
            .iter()
            .filter(|&&mcid| self.names[mcid] != ORPHAN)
            .count();
        if self.active.is_some() || used != owned {
            return Err(INVALID.into());
        }
        Ok(())
    }
}
