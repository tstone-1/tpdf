//! How often a redaction's verdict is withheld only because the document holds
//! a picture.
//!
//! ```text
//! cargo run --release --manifest-path src-tauri/Cargo.toml --example picture-verdict-probe -- \
//!     testdata [more folders or files ...]
//! ```
//!
//! `verify::scan` withholds *verified* for every picture in the written file:
//! its encoded bytes are scanned, its pixels are not read, and the reason says
//! text visible only as pixels needs OCR. Until 2026-11-19 that held only for a
//! picture stored with a lossy or fax filter (`DCTDecode`, `JPXDecode`,
//! `JBIG2Decode`, `CCITTFaxDecode`); a deflated or uncompressed picture was
//! searched as bytes and passed. This counts, per document, what the two rules
//! answer, and what the pictures that only the second rule withholds for are.
//!
//! **Counts and shapes only.** No page text and no picture content is printed,
//! so it can be pointed at a folder of somebody's own documents.
//!
//! **It parses in this process**, with `lopdf` under the scan's own decode
//! bound, which no command of the application or the tool may do. It is a
//! measurement over a corpus somebody chose, not a reader of documents.
//!
//! `--bare-pages` adds, under each document, the pages that draw no picture.
//!
//! Columns per document: pages, pages drawing a picture, image objects by
//! storage, inline images, size classes (pixels under 64 x 64, under
//! 256 x 256, larger), 1-bit pictures, soft masks, pictures drawn over half a
//! page or more, then `old`/`new`: whether the rule before and after
//! 2026-11-19 adds a picture reason. The scan run is the application's own
//! `verify::scan` with no needles, so `new` is its answer and not a copy of it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId};

/// The scan's own decode bound (`encoding::MAX_DECODE`, which is not public).
const MAX_DECODE: usize = 64 * 1024 * 1024;

/// The filters the rule before 2026-11-19 recognised a picture by.
const LOSSY_OR_FAX: &[&[u8]] = &[
    b"DCTDecode",
    b"JPXDecode",
    b"JBIG2Decode",
    b"CCITTFaxDecode",
];

/// A file is counted as real-world when its path names one of these, or the
/// folder the public sample was downloaded into.
const REAL_WORLD: &[&str] = &["text-heavy.pdf", "public", "private"];

#[derive(Default, Clone)]
struct Doc {
    name: String,
    real: bool,
    pages: usize,
    picture_pages: usize,
    by_storage: BTreeMap<&'static str, usize>,
    inline: usize,
    tiny: usize,
    small: usize,
    large: usize,
    one_bit: usize,
    masks: usize,
    half_page: usize,
    old: bool,
    new: bool,
    deferred_reasons: usize,
    /// Pages, counted from 1, that draw no picture.
    bare: Vec<usize>,
}

fn storage(filter: Option<&[u8]>) -> &'static str {
    match filter {
        None => "none",
        Some(b"DCTDecode") => "DCT",
        Some(b"JPXDecode") => "JPX",
        Some(b"JBIG2Decode") => "JBIG2",
        Some(b"CCITTFaxDecode") => "CCITT",
        Some(b"FlateDecode") => "Flate",
        Some(b"LZWDecode") => "LZW",
        Some(_) => "other",
    }
}

fn int(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<i64> {
    let value = dict.get(key).ok()?;
    let (_, value) = doc.dereference(value).ok()?;
    match value {
        Object::Integer(n) => Some(*n),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

fn name_is(doc: &Document, dict: &Dictionary, key: &[u8], want: &[u8]) -> bool {
    dict.get(key)
        .and_then(|v| doc.dereference(v))
        .and_then(|(_, v)| v.as_name())
        .is_ok_and(|n| n == want)
}

fn resolve_dict<'a>(doc: &'a Document, value: &'a Object) -> Option<&'a Dictionary> {
    match doc.dereference(value).ok()?.1 {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The page's `/Resources`, inherited through `/Parent` as a viewer reads it.
fn resources(doc: &Document, page: ObjectId) -> Option<&Dictionary> {
    let mut node = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(value) = node.get(b"Resources") {
            return resolve_dict(doc, value);
        }
        let parent = node.get(b"Parent").ok()?.as_reference().ok()?;
        node = doc.get_dictionary(parent).ok()?;
    }
    None
}

fn media_area(doc: &Document, page: ObjectId) -> Option<f64> {
    let mut node = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(value) = node.get(b"MediaBox") {
            let (_, value) = doc.dereference(value).ok()?;
            let numbers: Vec<f64> = value
                .as_array()
                .ok()?
                .iter()
                .filter_map(|v| match doc.dereference(v).ok()?.1 {
                    Object::Integer(n) => Some(*n as f64),
                    Object::Real(r) => Some(f64::from(*r)),
                    _ => None,
                })
                .collect();
            if numbers.len() != 4 {
                return None;
            }
            return Some(((numbers[2] - numbers[0]) * (numbers[3] - numbers[1])).abs());
        }
        let parent = node.get(b"Parent").ok()?.as_reference().ok()?;
        node = doc.get_dictionary(parent).ok()?;
    }
    None
}

/// Whether a resource dictionary reaches a picture, through forms too.
fn reaches_picture(doc: &Document, res: &Dictionary, seen: &mut BTreeSet<ObjectId>) -> bool {
    let Some(xobjects) = res.get(b"XObject").ok().and_then(|v| resolve_dict(doc, v)) else {
        return false;
    };
    for (_, value) in xobjects.iter() {
        let Ok(id) = value.as_reference() else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let Ok(Object::Stream(stream)) = doc.get_object(id) else {
            continue;
        };
        if name_is(doc, &stream.dict, b"Subtype", b"Image") {
            return true;
        }
        if let Some(inner) = stream
            .dict
            .get(b"Resources")
            .ok()
            .and_then(|v| resolve_dict(doc, v))
        {
            if reaches_picture(doc, inner, seen) {
                return true;
            }
        }
    }
    false
}

fn number(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

fn multiply(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// The page's own content: whether it draws an inline image, and the largest
/// share of the page a picture `Do` covers there (forms are not entered).
fn page_content(doc: &Document, page: ObjectId, res: Option<&Dictionary>) -> (bool, f64) {
    let Ok(content) = doc.get_and_decode_page_content(page) else {
        return (false, 0.0);
    };
    let area = media_area(doc, page).unwrap_or(0.0);
    let xobjects = res
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|v| resolve_dict(doc, v));
    let mut ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack = Vec::new();
    let mut inline = false;
    let mut share: f64 = 0.0;
    for op in &content.operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(ctm),
            "cm" if op.operands.len() == 6 => {
                let m: Vec<f64> = op.operands.iter().filter_map(number).collect();
                if m.len() == 6 {
                    ctm = multiply([m[0], m[1], m[2], m[3], m[4], m[5]], ctm);
                }
            }
            "BI" | "ID" | "EI" => inline = true,
            "Do" => {
                let Some(Ok(name)) = op.operands.first().map(Object::as_name) else {
                    continue;
                };
                let Some(Ok(id)) = xobjects
                    .and_then(|x| x.get(name).ok())
                    .map(Object::as_reference)
                else {
                    continue;
                };
                if let Ok(Object::Stream(s)) = doc.get_object(id) {
                    if name_is(doc, &s.dict, b"Subtype", b"Image") && area > 0.0 {
                        let drawn = (ctm[0] * ctm[3] - ctm[1] * ctm[2]).abs();
                        share = share.max(drawn / area);
                    }
                }
            }
            _ => {}
        }
    }
    (inline, share)
}

fn measure(path: &Path, bytes: &[u8]) -> Result<Doc, String> {
    let doc = Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let shown = path.to_string_lossy().replace('\\', "/");
    let mut out = Doc {
        name: shown.clone(),
        real: REAL_WORLD
            .iter()
            .any(|w| shown.contains(&format!("/{w}")) || shown.ends_with(w)),
        ..Doc::default()
    };

    // Soft masks and stencil masks named by another picture.
    let mut masks: BTreeSet<ObjectId> = BTreeSet::new();
    for object in doc.objects.values() {
        if let Object::Stream(s) = object {
            for key in [b"SMask".as_slice(), b"Mask".as_slice()] {
                if let Ok(Object::Reference(id)) = s.dict.get(key) {
                    masks.insert(*id);
                }
            }
        }
    }
    for (id, object) in &doc.objects {
        let Object::Stream(stream) = object else {
            continue;
        };
        let filters = stream.filters().unwrap_or_default();
        if filters.last().is_some_and(|f| LOSSY_OR_FAX.contains(f)) {
            out.old = true;
        }
        if !name_is(&doc, &stream.dict, b"Subtype", b"Image") {
            continue;
        }
        *out.by_storage
            .entry(storage(filters.last().copied()))
            .or_default() += 1;
        if masks.contains(id) {
            out.masks += 1;
        }
        let w = int(&doc, &stream.dict, b"Width").unwrap_or(0);
        let h = int(&doc, &stream.dict, b"Height").unwrap_or(0);
        if w < 64 && h < 64 {
            out.tiny += 1;
        } else if w < 256 && h < 256 {
            out.small += 1;
        } else {
            out.large += 1;
        }
        let mask = matches!(stream.dict.get(b"ImageMask"), Ok(Object::Boolean(true)));
        if mask || int(&doc, &stream.dict, b"BitsPerComponent") == Some(1) {
            out.one_bit += 1;
        }
    }

    let pages = doc.get_pages();
    out.pages = pages.len();
    for (number, page) in &pages {
        let res = resources(&doc, *page);
        let mut seen = BTreeSet::new();
        let reached = res.is_some_and(|r| reaches_picture(&doc, r, &mut seen));
        let (inline, share) = page_content(&doc, *page, res);
        if reached || inline {
            out.picture_pages += 1;
        } else {
            out.bare.push(*number as usize);
        }
        if share >= 0.5 {
            out.half_page += 1;
        }
    }

    // The application's own rule, now: the scan with nothing to look for.
    let report = tpdf_lib::verify::scan(bytes, &[], None);
    out.deferred_reasons = report.deferred.len();
    out.inline = report
        .deferred
        .iter()
        .filter(|why| why.contains("inline image"))
        .count();
    out.new = !report.deferred.is_empty();
    Ok(out)
}

fn walk(path: &Path, into: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for entry in entries {
            walk(&entry, into);
        }
    } else if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        into.push(path.to_path_buf());
    }
}

#[derive(Default)]
struct Sum {
    docs: usize,
    old: usize,
    new: usize,
    newly: usize,
    with_pictures: usize,
    newly_tiny_only: usize,
    newly_one_bit: usize,
    newly_half_page: usize,
    newly_masks: usize,
    newly_pictureless_pages: usize,
    pages: usize,
    picture_pages: usize,
    new_pages_without_picture: usize,
}

fn add(sum: &mut Sum, d: &Doc) {
    sum.docs += 1;
    sum.pages += d.pages;
    sum.picture_pages += d.picture_pages;
    let pictures: usize = d.by_storage.values().sum::<usize>() + d.inline;
    if pictures > 0 {
        sum.with_pictures += 1;
    }
    sum.old += usize::from(d.old);
    sum.new += usize::from(d.new);
    if d.new {
        sum.new_pages_without_picture += d.pages - d.picture_pages.min(d.pages);
    }
    if d.new && !d.old {
        sum.newly += 1;
        if d.small == 0 && d.large == 0 && d.inline == 0 {
            sum.newly_tiny_only += 1;
        }
        if d.one_bit > 0 {
            sum.newly_one_bit += 1;
        }
        if d.half_page > 0 {
            sum.newly_half_page += 1;
        }
        if d.masks > 0 {
            sum.newly_masks += 1;
        }
        sum.newly_pictureless_pages += d.pages - d.picture_pages.min(d.pages);
    }
}

fn print_sum(label: &str, s: &Sum) {
    println!(
        "{label}: {docs} documents, {pp} of {pages} pages draw a picture; {wp} documents hold \
         a picture",
        docs = s.docs,
        pp = s.picture_pages,
        pages = s.pages,
        wp = s.with_pictures
    );
    println!(
        "  withheld for a picture: before {old}, now {new}, newly {newly}",
        old = s.old,
        new = s.new,
        newly = s.newly
    );
    println!(
        "  pages of withheld documents that draw no picture: {n}",
        n = s.new_pages_without_picture
    );
    println!(
        "  of the newly withheld: only pictures under 64x64 px {t}, any 1-bit picture {b}, a \
         picture over half a page {h}, a soft or stencil mask {m}; their pages without a \
         picture {p}",
        t = s.newly_tiny_only,
        b = s.newly_one_bit,
        h = s.newly_half_page,
        m = s.newly_masks,
        p = s.newly_pictureless_pages
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: picture-verdict-probe <folder or file>...");
        std::process::exit(2);
    }
    let bare_pages = args.iter().any(|a| a == "--bare-pages");
    let mut files = Vec::new();
    for arg in args.iter().filter(|a| *a != "--bare-pages") {
        walk(Path::new(arg), &mut files);
    }
    let mut docs = Vec::new();
    let mut skipped = Vec::new();
    for file in &files {
        let Ok(bytes) = std::fs::read(file) else {
            skipped.push(file.clone());
            continue;
        };
        match measure(file, &bytes) {
            Ok(d) => docs.push(d),
            Err(_) => skipped.push(file.clone()),
        }
    }
    println!(
        "name | real | pages | picture pages | storage | inline | <64 | <256 | larger | 1-bit | \
         masks | >=half page | old | new | reasons"
    );
    for d in &docs {
        let storage: Vec<String> = d
            .by_storage
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect();
        println!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}",
            d.name,
            if d.real { "real" } else { "synthetic" },
            d.pages,
            d.picture_pages,
            if storage.is_empty() {
                "-".to_string()
            } else {
                storage.join(",")
            },
            d.inline,
            d.tiny,
            d.small,
            d.large,
            d.one_bit,
            d.masks,
            d.half_page,
            if d.old { "withheld" } else { "-" },
            if d.new { "withheld" } else { "-" },
            d.deferred_reasons
        );
        if bare_pages {
            let shown: Vec<String> = d.bare.iter().map(ToString::to_string).collect();
            println!("  bare: {}", shown.join(","));
        }
    }
    let mut all = Sum::default();
    let mut real = Sum::default();
    let mut synthetic = Sum::default();
    for d in &docs {
        add(&mut all, d);
        add(if d.real { &mut real } else { &mut synthetic }, d);
    }
    println!();
    println!(
        "{} files, {} measured, {} skipped (would not parse)",
        files.len(),
        docs.len(),
        skipped.len()
    );
    for s in &skipped {
        println!("  skipped: {}", s.to_string_lossy().replace('\\', "/"));
    }
    print_sum("all", &all);
    print_sum("real-world", &real);
    print_sum("synthetic", &synthetic);
}
