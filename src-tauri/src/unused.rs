//! Drops the pictures no remaining page draws.
//!
//! **The measurement this was written from.** A five-page document kept every
//! page's pictures in one `/XObject` dictionary that all five pages named.
//! Extracting two pages wrote a file of 1,017,555 bytes from a source of
//! 1,023,780: the three dropped pages were gone and their pictures were not,
//! because the two kept pages still named the shared dictionary and the
//! dictionary still named everything. `sweep::collect` was right about what is
//! reachable. Reachable through a resource name nothing draws is the gap, and
//! a reader who extracts two pages to send them on has said the other three
//! are not to go along.
//!
//! So this removes an `/XObject` entry when no page that reaches the
//! dictionary mentions its name in its content, and the caller sweeps afterwards.
//!
//! **Only `/XObject`, and only where nothing else can be drawing through the
//! dictionary.** A group of pages is left exactly as it was when any of these
//! holds, because each is a way a name can be used without a `Do` in a page's
//! own content:
//!
//! - a page's content does not decode completely, or is over the text
//!   editor's size bound, whose strict decoder this borrows;
//! - anything other than the group's own pages reaches the dictionary --- a
//!   form that shares its page's resources is the common shape;
//! - a form, a Type 3 font, a tiling pattern or an annotation's appearance on
//!   one of the pages carries no `/Resources` of its own, which by the
//!   specification means it draws through the page's.
//!
//! Fonts are left alone: the bytes and the other pages' pictures were both in
//! `/XObject`, and a font is named from more places than a picture is.
//!
//! **Run it on a swept document.** The referrer check below reads every object
//! in the map, so a dropped page that is still there counts as something else
//! reaching the dictionary and the group is left alone.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::sweep;

/// How far up the page tree `/Resources` is looked for.
const MAX_PARENTS: usize = 64;

/// Where a page's `/XObject` dictionary is written.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Home {
    /// It is this object.
    Own(ObjectId),
    /// It is inline in `/Resources`, which is this object.
    InResources(ObjectId),
    /// It is inline in an inline `/Resources` of this page or page-tree node.
    InHolder(ObjectId),
}

/// One page's way to its `/XObject` dictionary.
struct Found {
    home: Home,
    /// The object a reference to the home is allowed to sit in.
    holder: ObjectId,
}

/// What the pages sharing one dictionary draw, or `None` when it must be kept whole.
struct Group {
    drawn: Option<HashSet<Vec<u8>>>,
    holders: HashSet<ObjectId>,
}

/// Removes every `/XObject` entry no remaining page draws.
///
/// Returns how many entries went. The streams they named are still in the
/// object map; `sweep::collect` is what deletes them.
///
/// # Errors
///
/// An object nesting deeper than [`sweep::MAX_NESTING`].
pub fn prune(doc: &mut Document) -> Result<usize, String> {
    let mut groups: HashMap<Home, Group> = HashMap::new();
    for page in doc.get_pages().into_values() {
        let Some(found) = home_of(doc, page) else {
            continue;
        };
        let group = groups.entry(found.home).or_insert_with(|| Group {
            drawn: Some(HashSet::new()),
            holders: HashSet::new(),
        });
        group.holders.insert(found.holder);
        match (drawn_by(doc, page), group.drawn.as_mut()) {
            (Some(names), Some(all)) => all.extend(names),
            _ => group.drawn = None,
        }
    }

    // One pass over the map for every dictionary at once: who names it.
    let watched: HashSet<ObjectId> = groups
        .keys()
        .filter_map(|home| match home {
            Home::Own(id) | Home::InResources(id) => Some(*id),
            Home::InHolder(_) => None,
        })
        .collect();
    let mut strangers: HashSet<ObjectId> = HashSet::new();
    for (id, object) in &doc.objects {
        let mut named = Vec::new();
        sweep::references(object, &mut named)?;
        for target in named {
            if !watched.contains(&target) {
                continue;
            }
            let known = groups.iter().any(|(home, group)| {
                matches!(home, Home::Own(at) | Home::InResources(at) if *at == target)
                    && group.holders.contains(id)
            });
            if !known {
                strangers.insert(target);
            }
        }
    }
    let mut trailer = Vec::new();
    sweep::references(&Object::Dictionary(doc.trailer.clone()), &mut trailer)?;
    strangers.extend(trailer.into_iter().filter(|id| watched.contains(id)));

    let mut removed = 0;
    for (home, group) in groups {
        let Some(drawn) = group.drawn else {
            continue;
        };
        if let Home::Own(id) | Home::InResources(id) = home {
            if strangers.contains(&id) {
                continue;
            }
        }
        let Some(entries) = dictionary_at(doc, home) else {
            continue;
        };
        let unused: Vec<Vec<u8>> = entries
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| !drawn.contains(name))
            .collect();
        for name in unused {
            entries.remove(&name);
            removed += 1;
        }
    }
    Ok(removed)
}

/// Where `page` gets its `/XObject` dictionary from, if it has one.
fn home_of(doc: &Document, page: ObjectId) -> Option<Found> {
    let mut at = page;
    for _ in 0..MAX_PARENTS {
        let dictionary = doc.get_object(at).and_then(Object::as_dict).ok()?;
        if let Ok(resources) = dictionary.get(b"Resources") {
            let (holder, resources, inline) = match resources {
                Object::Reference(id) => (*id, doc.get_object(*id).ok()?.as_dict().ok()?, false),
                other => (at, other.as_dict().ok()?, true),
            };
            return match resources.get(b"XObject").ok()? {
                // The page or node `at` is what names an indirect `/Resources`,
                // so it is the allowed referrer of that object.
                Object::Reference(id) => Some(Found {
                    home: Home::Own(*id),
                    holder,
                }),
                Object::Dictionary(_) if inline => Some(Found {
                    home: Home::InHolder(holder),
                    holder,
                }),
                Object::Dictionary(_) => Some(Found {
                    home: Home::InResources(holder),
                    holder: at,
                }),
                _ => None,
            };
        }
        at = dictionary
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok()?;
    }
    None
}

/// The `/XObject` dictionary a [`Home`] names, to change.
fn dictionary_at(doc: &mut Document, home: Home) -> Option<&mut Dictionary> {
    match home {
        Home::Own(id) => doc.get_object_mut(id).ok()?.as_dict_mut().ok(),
        Home::InResources(id) => doc
            .get_object_mut(id)
            .ok()?
            .as_dict_mut()
            .ok()?
            .get_mut(b"XObject")
            .ok()?
            .as_dict_mut()
            .ok(),
        Home::InHolder(id) => doc
            .get_object_mut(id)
            .ok()?
            .as_dict_mut()
            .ok()?
            .get_mut(b"Resources")
            .ok()?
            .as_dict_mut()
            .ok()?
            .get_mut(b"XObject")
            .ok()?
            .as_dict_mut()
            .ok(),
    }
}

/// Every name `page`'s own content mentions, or `None` when that cannot be known.
///
/// **Every name, not the operand of every `Do`**, and no content parser. A
/// `Do` takes a name, so each drawn picture is in this set whatever else is;
/// a name that is a font's or a colour space's only keeps a picture of the
/// same name that nothing draws. Measured before choosing: `lopdf`'s
/// `Content::decode` reads `/Im2 Do ((` as one complete operation and returns
/// `Ok`, so a count of parsed `Do`s can fall short without saying so, and
/// falling short here deletes a picture a page draws.
fn drawn_by(doc: &Document, page: ObjectId) -> Option<HashSet<Vec<u8>>> {
    if leans_on_page(doc, page) {
        return None;
    }
    let has_content = doc
        .get_object(page)
        .and_then(Object::as_dict)
        .is_ok_and(|dictionary| dictionary.has(b"Contents"));
    if !has_content {
        return Some(HashSet::new());
    }
    let data = crate::textedit::page_content(doc, page).ok()?;
    names_in(&data)
}

/// Every `/Name` token in `data`, with `#xx` escapes decoded.
///
/// `None` on an escape that is not two hexadecimal digits: the name it stands
/// for is then unknown, and an unknown name may be the one that is drawn.
fn names_in(data: &[u8]) -> Option<HashSet<Vec<u8>>> {
    const DELIMITERS: &[u8] = b"()<>[]{}/%";
    let mut names = HashSet::new();
    let mut at = 0;
    while at < data.len() {
        if data[at] != b'/' {
            at += 1;
            continue;
        }
        at += 1;
        let mut name = Vec::new();
        while at < data.len()
            && !data[at].is_ascii_whitespace()
            && data[at] != 0
            && !DELIMITERS.contains(&data[at])
        {
            if data[at] == b'#' {
                let digits = data.get(at + 1..at + 3)?;
                let text = std::str::from_utf8(digits).ok()?;
                name.push(u8::from_str_radix(text, 16).ok()?);
                at += 3;
            } else {
                name.push(data[at]);
                at += 1;
            }
        }
        names.insert(name);
    }
    Some(names)
}

/// Whether something on `page` draws through the page's resources without the
/// page's content saying so: a stream that should carry `/Resources` and does not.
fn leans_on_page(doc: &Document, page: ObjectId) -> bool {
    let Some(resources) = resources_of(doc, page) else {
        return false;
    };
    let bare = |object: &Object| {
        doc.dereference(object)
            .ok()
            .and_then(|(_, object)| object.as_stream().ok())
            .is_some_and(|stream| !stream.dict.has(b"Resources"))
    };
    let entries = |key: &[u8]| -> Vec<&Object> {
        resources
            .get(key)
            .ok()
            .and_then(|value| doc.dereference(value).ok())
            .and_then(|(_, value)| value.as_dict().ok())
            .map(|dictionary| dictionary.iter().map(|(_, value)| value).collect())
            .unwrap_or_default()
    };
    // A form, or a tiling pattern: a stream with no `/Resources` of its own.
    // An image is a stream too, and never has any, so it is told apart by name.
    let form = |object: &&Object| {
        doc.dereference(object)
            .ok()
            .and_then(|(_, object)| object.as_stream().ok())
            .is_some_and(|stream| {
                !stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == b"Image")
                    && !stream.dict.has(b"Resources")
            })
    };
    if entries(b"XObject").iter().any(form) || entries(b"Pattern").iter().any(|o| bare(o)) {
        return true;
    }
    // A Type 3 font is a dictionary; its glyphs draw through its own
    // `/Resources`, or through the page's when it has none.
    let type3 = |object: &&Object| {
        doc.dereference(object)
            .ok()
            .and_then(|(_, object)| object.as_dict().ok())
            .is_some_and(|font| {
                font.get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|name| name == b"Type3")
                    && !font.has(b"Resources")
            })
    };
    if entries(b"Font").iter().any(type3) {
        return true;
    }
    appearances(doc, page).iter().any(bare)
}

/// The `/Resources` dictionary `page` uses, its own or an ancestor's.
fn resources_of(doc: &Document, page: ObjectId) -> Option<&Dictionary> {
    let mut at = page;
    for _ in 0..MAX_PARENTS {
        let dictionary = doc.get_object(at).and_then(Object::as_dict).ok()?;
        if let Ok(resources) = dictionary.get(b"Resources") {
            return doc.dereference(resources).ok()?.1.as_dict().ok();
        }
        at = dictionary
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok()?;
    }
    None
}

/// Every appearance stream of every annotation on `page`.
fn appearances(doc: &Document, page: ObjectId) -> Vec<Object> {
    let mut out = Vec::new();
    let Some(annotations) = doc
        .get_object(page)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|dictionary| dictionary.get(b"Annots").ok())
        .and_then(|value| doc.dereference(value).ok())
        .and_then(|(_, value)| value.as_array().ok())
    else {
        return out;
    };
    for annotation in annotations {
        let Some(states) = doc
            .dereference(annotation)
            .ok()
            .and_then(|(_, value)| value.as_dict().ok())
            .and_then(|dictionary| dictionary.get(b"AP").ok())
            .and_then(|value| doc.dereference(value).ok())
            .and_then(|(_, value)| value.as_dict().ok())
        else {
            continue;
        };
        // `/N`, `/R` and `/D` are each a stream, or a dictionary of streams by state.
        for (_, state) in states.iter() {
            match doc.dereference(state).map(|(_, value)| value) {
                Ok(Object::Dictionary(by_state)) => {
                    out.extend(by_state.iter().map(|(_, value)| value.clone()));
                }
                Ok(_) => out.push(state.clone()),
                Err(_) => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::prune;
    use crate::sweep;
    use lopdf::{dictionary, Document, Object, ObjectId, Stream};

    /// Two pages sharing one `/XObject` dictionary that names three pictures;
    /// page 1 draws `Im1`, page 2 draws `Im2`, nothing draws `Im3`.
    fn shared(indirect: bool) -> (Document, [ObjectId; 3], [ObjectId; 2]) {
        let mut doc = Document::with_version("1.7");
        let image = |doc: &mut Document, pixel: u8| {
            doc.add_object(Stream::new(
                dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
                vec![pixel],
            ))
        };
        let images = [image(&mut doc, 1), image(&mut doc, 2), image(&mut doc, 3)];
        let names = dictionary! { "Im1" => images[0], "Im2" => images[1], "Im3" => images[2] };
        let names: Object = if indirect {
            doc.add_object(names).into()
        } else {
            names.into()
        };
        let resources = doc.add_object(dictionary! { "XObject" => names });
        let pages_id = doc.new_object_id();
        let mut pages = [(0, 0); 2];
        for (at, draw) in [&b"/Im1 Do"[..], &b"/Im2 Do"[..]].into_iter().enumerate() {
            let content = doc.add_object(Stream::new(dictionary! {}, draw.to_vec()));
            pages[at] = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
            });
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => 2,
                "Kids" => vec![pages[0].into(), pages[1].into()],
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        (doc, images, pages)
    }

    #[test]
    fn a_picture_no_page_draws_leaves_the_file_and_a_drawn_one_stays() {
        for indirect in [false, true] {
            let (mut doc, images, _) = shared(indirect);
            assert_eq!(prune(&mut doc).expect("prune"), 1, "indirect {indirect}");
            sweep::collect(&mut doc).expect("sweep");
            assert!(
                doc.get_object(images[2]).is_err(),
                "the undrawn picture is still there"
            );
            // The control: a version that removed everything would pass the line above.
            assert!(doc.get_object(images[0]).is_ok() && doc.get_object(images[1]).is_ok());
        }
    }

    #[test]
    fn a_dictionary_something_else_reaches_is_left_whole() {
        let (mut doc, images, pages) = shared(true);
        // A form that draws `Im3` through the same `/XObject` dictionary.
        let names = doc
            .get_object(pages[0])
            .and_then(Object::as_dict)
            .and_then(|page| page.get(b"Resources"))
            .and_then(Object::as_reference)
            .and_then(|id| doc.get_object(id))
            .and_then(Object::as_dict)
            .and_then(|resources| resources.get(b"XObject"))
            .and_then(Object::as_reference)
            .expect("the shared dictionary");
        let form = doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form",
            "Resources" => dictionary! { "XObject" => names } },
            b"/Im3 Do".to_vec(),
        ));
        doc.get_object_mut(names)
            .and_then(Object::as_dict_mut)
            .expect("names")
            .set("Fm", form);
        assert_eq!(prune(&mut doc).expect("prune"), 0);
        assert!(doc.get_object(images[2]).is_ok());
    }

    #[test]
    fn a_form_without_resources_of_its_own_keeps_every_name() {
        let (mut doc, _, pages) = shared(false);
        let form = doc.add_object(Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form" },
            b"/Im3 Do".to_vec(),
        ));
        let resources = doc
            .get_object(pages[0])
            .and_then(Object::as_dict)
            .and_then(|page| page.get(b"Resources"))
            .and_then(Object::as_reference)
            .expect("resources");
        doc.get_object_mut(resources)
            .and_then(Object::as_dict_mut)
            .and_then(|resources| resources.get_mut(b"XObject"))
            .and_then(Object::as_dict_mut)
            .expect("names")
            .set("Fm", form);
        assert_eq!(prune(&mut doc).expect("prune"), 0);
    }

    #[test]
    fn content_that_does_not_decode_keeps_every_name() {
        let (mut doc, _, pages) = shared(true);
        let content = doc
            .get_object(pages[1])
            .and_then(Object::as_dict)
            .and_then(|page| page.get(b"Contents"))
            .and_then(Object::as_reference)
            .expect("content");
        // Says Flate and is not: `lopdf` would hand back these raw bytes.
        doc.get_object_mut(content)
            .and_then(Object::as_stream_mut)
            .expect("stream")
            .dict
            .set("Filter", "FlateDecode");
        assert_eq!(prune(&mut doc).expect("prune"), 0);
    }

    #[test]
    fn a_name_is_found_however_the_content_around_it_reads() {
        let found = |data: &[u8]| super::names_in(data).expect("names");
        // After content no operator parser gets through, and written with an escape.
        assert!(found(b"(( /Im2 Do").contains(&b"Im2"[..]));
        assert!(found(b"/Im#32 Do").contains(&b"Im2"[..]));
        assert!(found(b"/Im2/Im3 Do").contains(&b"Im2"[..]));
        assert!(
            super::names_in(b"/Im#3 Do").is_none(),
            "a broken escape names something unknown"
        );
    }
}
