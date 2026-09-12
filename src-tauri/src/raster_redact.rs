//! Redaction by replacement: only rendered, masked RGB pixels enter the new graph.
//! No source object, annotation, text layer, metadata or resource is copied.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use sha2::{Digest, Sha256};

use crate::docmodel::PageSource;
use crate::document::OpenDocument;
use crate::edits::Plan;
use crate::progressive::{self, Bindings, CancelToken, Placement, RawBitmap, RawDocument, RawPage};
use crate::save::{Job, Refusal};

const DPI: f64 = 300.0;
const STRIP: usize = 256;
const MAX_PIXELS: usize = 32_000_000;
const MAX_BYTES: usize = 512 * 1024 * 1024;
const MAX_DOCUMENT_PIXELS: usize = 1024 * 1024 * 1024 / 3;
const MAX_PAGES: usize = 4096;
const DEADLINE: Duration = Duration::from_secs(120);

/// RawDocument requires a static buffer. This private owner closes every PDFium
/// handle before releasing that buffer; neither field can escape this module.
struct OwnedPdf {
    raw: RawDocument,
    _bytes: Box<[u8]>,
}

impl OwnedPdf {
    fn open(bindings: Bindings, bytes: Vec<u8>, password: Option<&str>) -> Result<Self, String> {
        let bytes = bytes.into_boxed_slice();
        // SAFETY: the box does not move its allocation, and fields drop in
        // declaration order: RawDocument closes its pages/document before bytes.
        let view = unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) };
        let raw = RawDocument::open_bytes(bindings, view, password)
            .map_err(|e| format!("raster document could not be read: {e:?}"))?;
        Ok(Self { raw, _bytes: bytes })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pixels {
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
}

fn dimensions(width: f64, height: f64) -> Result<(usize, usize), String> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("raster redaction requires finite positive page dimensions".into());
    }
    let w = (width * DPI / 72.0).ceil();
    let h = (height * DPI / 72.0).ceil();
    if w > f64::from(u16::MAX) || h > f64::from(i32::MAX) || w * h > MAX_PIXELS as f64 {
        return Err("page exceeds the raster redaction pixel limit".into());
    }
    Ok((w as usize, h as usize))
}

fn pixel_bounds(points: &[(i32, i32)], width: usize, height: usize) -> Result<Pixels, String> {
    let left = points
        .iter()
        .map(|p| p.0)
        .min()
        .ok_or("missing region corners")?;
    let right = points
        .iter()
        .map(|p| p.0)
        .max()
        .ok_or("missing region corners")?;
    let top = points
        .iter()
        .map(|p| p.1)
        .min()
        .ok_or("missing region corners")?;
    let bottom = points
        .iter()
        .map(|p| p.1)
        .max()
        .ok_or("missing region corners")?;
    if right <= 0 || bottom <= 0 || left >= width as i32 || top >= height as i32 {
        return Err("a marked region lies outside its output page".into());
    }
    // Integer PageToDevice rounding and antialiasing are covered by a two-pixel
    // outward margin. Never mask less than the requested area.
    Ok(Pixels {
        left: left.saturating_sub(2).clamp(0, width as i32) as usize,
        top: top.saturating_sub(2).clamp(0, height as i32) as usize,
        right: right.saturating_add(2).clamp(0, width as i32) as usize,
        bottom: bottom.saturating_add(2).clamp(0, height as i32) as usize,
    })
}

fn map_area(
    page: &RawPage<'_>,
    area: [f32; 4],
    width: usize,
    height: usize,
) -> Result<Pixels, String> {
    if !area.iter().all(|n| n.is_finite()) || area[0] >= area[2] || area[1] >= area[3] {
        return Err("a marked region has invalid or zero dimensions".into());
    }
    let mut points = Vec::with_capacity(4);
    for (x, y) in [
        (area[0], area[1]),
        (area[0], area[3]),
        (area[2], area[1]),
        (area[2], area[3]),
    ] {
        let (mut dx, mut dy) = (0, 0);
        // SAFETY: the live page owns the handle; outputs are writable integers.
        let ok = unsafe {
            page.bindings().FPDF_PageToDevice(
                page.handle(),
                0,
                0,
                width as i32,
                height as i32,
                0,
                f64::from(x),
                f64::from(y),
                &mut dx,
                &mut dy,
            )
        };
        if ok == 0 {
            return Err("PDFium could not locate a marked region".into());
        }
        points.push((dx, dy));
    }
    pixel_bounds(&points, width, height)
}

fn mask(rgb: &mut [u8], width: usize, y: usize, rows: usize, areas: &[Pixels]) {
    for area in areas {
        for row in area.top.max(y)..area.bottom.min(y + rows) {
            let start = ((row - y) * width + area.left) * 3;
            let end = ((row - y) * width + area.right) * 3;
            rgb[start..end].fill(0);
        }
    }
}

fn render_strip(
    page: &RawPage<'_>,
    width: usize,
    height: usize,
    y: usize,
    rows: usize,
) -> Result<Vec<u8>, String> {
    let mut rgba = vec![255; width * rows * 4];
    let mut bitmap = RawBitmap::borrowed(page.bindings(), &mut rgba, width as u16, rows as u16)?;
    let placement = Placement {
        start_x: 0,
        start_y: -(y as i32),
        size_x: width as i32,
        size_y: height as i32,
        turns: 0,
    };
    let progress = progressive::render(
        &mut bitmap,
        page,
        placement,
        Some(Duration::from_millis(20)),
        &CancelToken::new(),
    );
    if !progress.outcome.is_done() {
        return Err("a raster redaction render did not complete".into());
    }
    let mut rgb = Vec::with_capacity(width * rows * 3);
    for pixel in bitmap.pixels().chunks_exact(4) {
        if pixel[3] != 255 {
            return Err("a raster redaction pixel was not opaque".into());
        }
        rgb.extend_from_slice(&pixel[..3]);
    }
    Ok(rgb)
}

struct PageProof {
    width: usize,
    height: usize,
    areas: Vec<Pixels>,
}

fn load(bytes: &[u8], password: Option<&str>) -> Result<Document, String> {
    Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            max_decompressed_size: Some(MAX_BYTES),
            password: password.map(str::to_string),
            ..Default::default()
        },
    )
    .map_err(|e| format!("could not read raster PDF: {e}"))
}

fn same_object(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::Real(a), Object::Integer(b)) | (Object::Integer(b), Object::Real(a)) => {
            f64::from(*a) == *b as f64
        }
        (Object::Array(a), Object::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_object(a, b))
        }
        (Object::Dictionary(a), Object::Dictionary(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_ok_and(|other| same_object(value, other)))
        }
        _ => a == b,
    }
}

/// Requires exact generated objects and exact losslessly decoded pixel digests.
/// This is checked on the serialized bytes, including encrypted round trips.
fn verify_graph(
    bytes: &[u8],
    password: Option<&str>,
    expected: &Document,
    images: &BTreeMap<ObjectId, [u8; 32]>,
) -> Result<(), String> {
    let actual = load(bytes, password)?;
    if actual.is_encrypted() {
        return Err("raster output could not be unlocked for verification".into());
    }
    let allowed_trailer: &[&[u8]] = &[b"Root", b"Size", b"ID"];
    if actual
        .trailer
        .iter()
        .any(|(key, _)| !allowed_trailer.contains(&key.as_slice()))
        || actual.trailer.get(b"Root").ok() != expected.trailer.get(b"Root").ok()
        || actual.trailer.get(b"ID").ok() != expected.trailer.get(b"ID").ok()
        || actual.trailer.get(b"Size").and_then(Object::as_i64).ok()
            != Some(i64::from(expected.max_id) + 1)
    {
        return Err("unexpected raster output trailer".into());
    }
    // lopdf removes the encryption dictionary on a successful decryption.
    let encrypted_id = expected
        .trailer
        .get(b"Encrypt")
        .ok()
        .and_then(|v| v.as_reference().ok());
    let expected_count = expected.objects.len() - usize::from(encrypted_id.is_some());
    if actual.objects.len() != expected_count {
        return Err(format!(
            "raster output object inventory changed: expected {}, read {}",
            expected_count,
            actual.objects.len()
        ));
    }
    for (&id, object) in &expected.objects {
        if Some(id) == encrypted_id {
            continue;
        }
        let got = actual.get_object(id).map_err(|e| e.to_string())?;
        if let Some(hash) = images.get(&id) {
            let stream = got.as_stream().map_err(|e| e.to_string())?;
            let allowed: &[&[u8]] = &[
                b"Type",
                b"Subtype",
                b"Width",
                b"Height",
                b"ColorSpace",
                b"BitsPerComponent",
                b"Filter",
                b"Length",
            ];
            if stream
                .dict
                .iter()
                .any(|(key, _)| !allowed.contains(&key.as_slice()))
            {
                return Err("unexpected raster image carrier".into());
            }
            let wanted = object.as_stream().map_err(|e| e.to_string())?;
            for (key, value) in &wanted.dict {
                if key != b"Length" && stream.dict.get(key).ok() != Some(value) {
                    return Err("raster image geometry or encoding changed".into());
                }
            }
            let data = if stream.dict.has(b"Filter") {
                stream
                    .decompressed_content_with_limit(MAX_BYTES)
                    .map_err(|e| e.to_string())?
            } else {
                stream.content.clone()
            };
            let found: [u8; 32] = Sha256::digest(&data).into();
            if &found != hash {
                return Err("raster pixels changed during serialization".into());
            }
        } else if matches!(object, Object::Stream(_)) {
            // Content streams are generated from fixed numeric transforms and
            // image names; their decrypted bytes must agree exactly.
            let wanted = object.as_stream().map_err(|e| e.to_string())?;
            let found = got.as_stream().map_err(|e| e.to_string())?;
            if wanted.content != found.content || found.dict.iter().any(|(key, _)| key != b"Length")
            {
                return Err("raster drawing commands changed".into());
            }
        } else if !same_object(object, got) {
            return Err("raster page graph changed".into());
        }
    }
    Ok(())
}

/// Writes a new raster-only document, preserving the input encryption policy.
pub fn rewrite(document: &OpenDocument, plan: &Plan) -> Result<Vec<u8>, Refusal> {
    rewrite_inner(document, plan).map_err(Into::into)
}

fn rewrite_inner(document: &OpenDocument, plan: &Plan) -> Result<Vec<u8>, String> {
    let started = Instant::now();
    if !plan.text_edits.is_empty() {
        return Err("save text edits before applying redactions".into());
    }
    if plan.redactions.is_empty() || plan.redactions.iter().any(|r| r.areas.is_empty()) {
        return Err("no complete redaction regions were supplied".into());
    }
    if plan.pages.is_empty() || plan.pages.len() > MAX_PAGES {
        return Err("raster redaction page limit exceeded".into());
    }
    let mut clean = plan.clone();
    clean.redactions.clear();
    let bytes = document
        .graph()
        .rewrite(&clean, Job::Save)
        .map_err(|e| e.message)?;
    if bytes.len() > MAX_BYTES {
        return Err("working PDF exceeds raster redaction size limit".into());
    }
    let password = document.graph().password();
    let mut source_graph = load(&bytes, password)?;
    if source_graph.is_encrypted() {
        return Err("cannot preserve an encryption policy without its key".into());
    }
    let encryption = source_graph.encryption_state.take();
    let page_boxes = crate::pagetree::displayed_boxes_from(&source_graph, plan.pages.len())?;
    drop(source_graph);
    let working = OwnedPdf::open(document.pdfium().bindings(), bytes, password)?;
    if working.raw.page_count() as usize != plan.pages.len() {
        return Err("raster working page count disagrees with the plan".into());
    }
    let mut regions = vec![Vec::new(); plan.pages.len()];
    for redaction in &plan.redactions {
        let slots: Vec<_> = plan
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.source == PageSource::Baseline(redaction.source))
            .map(|(slot, _)| slot)
            .collect();
        if slots.len() != 1 {
            return Err("a marked source does not name exactly one output page".into());
        }
        regions[slots[0]].extend_from_slice(&redaction.areas);
    }
    let mut out = Document::with_version("1.7");
    // A traditional xref table introduces no extra stream object into the
    // otherwise closed image/page/content inventory.
    out.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    let pages_id = out.new_object_id();
    let mut kids = Vec::new();
    let mut images = BTreeMap::new();
    let mut proofs = Vec::new();
    let mut total = 0usize;
    let mut encoded_total = 0usize;
    for (slot, areas) in regions.iter().enumerate() {
        if started.elapsed() > DEADLINE {
            return Err("raster redaction exceeded its time limit".into());
        }
        let page = working.raw.page_cropped(slot as u32, None, &|index| {
            page_boxes.get(index as usize).copied()
        })?;
        let (pw, ph) = (f64::from(page.width_pt()), f64::from(page.height_pt()));
        let (width, height) = dimensions(pw, ph)?;
        total = total
            .checked_add(width * height)
            .ok_or("raster size overflow")?;
        if total > MAX_DOCUMENT_PIXELS {
            return Err("document exceeds raster redaction pixel budget".into());
        }
        let masks = areas
            .iter()
            .map(|&area| map_area(&page, area, width, height))
            .collect::<Result<Vec<_>, _>>()?;
        let mut resources = lopdf::Dictionary::new();
        let mut commands = String::new();
        for y in (0..height).step_by(STRIP) {
            if started.elapsed() > DEADLINE {
                return Err("raster redaction exceeded its time limit".into());
            }
            let rows = STRIP.min(height - y);
            let mut rgb = render_strip(&page, width, height, y, rows)?;
            mask(&mut rgb, width, y, rows, &masks);
            let hash: [u8; 32] = Sha256::digest(&rgb).into();
            let mut image = Stream::new(
                dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>width as i64, "Height"=>rows as i64, "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8},
                rgb,
            );
            image.compress().map_err(|e| e.to_string())?;
            encoded_total = encoded_total
                .checked_add(image.content.len())
                .ok_or("raster stream size overflow")?;
            if encoded_total > MAX_BYTES / 4 {
                return Err("compressed raster images exceed the memory budget".into());
            }
            let image_id = out.add_object(image);
            images.insert(image_id, hash);
            let name = format!("I{y}");
            resources.set(name.clone(), image_id);
            let strip_h = ph * rows as f64 / height as f64;
            let bottom = ph * (height - y - rows) as f64 / height as f64;
            commands.push_str(&format!(
                "q {pw:.9} 0 0 {strip_h:.9} 0 {bottom:.9} cm /{name} Do Q\n"
            ));
        }
        let content = out.add_object(Stream::new(dictionary! {}, commands.into_bytes()));
        let page_id = out.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages_id, "MediaBox"=>vec![0.into(),0.into(),Object::Real(pw as f32),Object::Real(ph as f32)], "Resources"=>dictionary!{"XObject"=>resources}, "Contents"=>content});
        kids.push(Object::Reference(page_id));
        proofs.push(PageProof {
            width,
            height,
            areas: masks,
        });
        working.raw.evict_page(slot as u32);
    }
    out.objects.insert(
        pages_id,
        dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>plan.pages.len() as i64}.into(),
    );
    let catalog = out.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages_id});
    out.trailer.set("Root", catalog);
    // Keep plaintext expected objects for comparison after decrypting output.
    let expected = out.clone();
    if let Some(state) = &encryption {
        out.encrypt(state)
            .map_err(|e| format!("could not preserve raster output encryption: {e}"))?;
    }
    let mut result = Vec::new();
    out.save_to(&mut result).map_err(|e| e.to_string())?;
    if result.len() > MAX_BYTES {
        return Err("raster output exceeds the file size limit".into());
    }
    // The encryption dictionary is the only extra object the serializer owns.
    let mut expected = expected;
    expected.max_id = out.max_id;
    if let Ok(id) = out.trailer.get(b"ID") {
        expected.trailer.set("ID", id.clone());
    }
    if let Ok(value) = out.trailer.get(b"Encrypt") {
        let id = value.as_reference().map_err(|e| e.to_string())?;
        expected
            .objects
            .insert(id, out.get_object(id).map_err(|e| e.to_string())?.clone());
        expected.trailer.set("Encrypt", id);
    }
    verify_graph(&result, password, &expected, &images)?;
    let verified = OwnedPdf::open(document.pdfium().bindings(), result.clone(), password)?;
    if verified.raw.page_count() as usize != proofs.len() {
        return Err("PDFium reported a different raster output page count".into());
    }
    for (slot, proof) in proofs.iter().enumerate() {
        let page = verified.raw.page_cropped(slot as u32, None, &|_| None)?;
        if dimensions(f64::from(page.width_pt()), f64::from(page.height_pt()))?
            != (proof.width, proof.height)
            || !crate::text::extract(&page)?.codes.is_empty()
        {
            return Err("raster output page geometry or text layer is inconsistent".into());
        }
        if proof.areas.is_empty() {
            verified.raw.evict_page(slot as u32);
            continue;
        }
        for y in (0..proof.height).step_by(STRIP) {
            if started.elapsed() > DEADLINE {
                return Err("raster verification exceeded its time limit".into());
            }
            let rows = STRIP.min(proof.height - y);
            let rgb = render_strip(&page, proof.width, proof.height, y, rows)?;
            for area in &proof.areas {
                // The outward mask margin handles pixel interpolation at edges.
                let inset_x = usize::from(area.right - area.left > 2);
                let inset_y = usize::from(area.bottom - area.top > 2);
                let inside = Pixels {
                    left: area.left + inset_x,
                    top: area.top + inset_y,
                    right: area.right - inset_x,
                    bottom: area.bottom - inset_y,
                };
                for row in inside.top.max(y)..inside.bottom.min(y + rows) {
                    let start = ((row - y) * proof.width + inside.left) * 3;
                    let end = ((row - y) * proof.width + inside.right) * 3;
                    if rgb[start..end].iter().any(|&n| n != 0) {
                        return Err("PDFium found non-black pixels inside a redacted region".into());
                    }
                }
            }
        }
        verified.raw.evict_page(slot as u32);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_bound_allocations_and_reject_invalid_pages() {
        assert_eq!(dimensions(72.0, 144.0).unwrap(), (300, 600));
        for width in [0.0, -1.0, f64::NAN, f64::INFINITY, 14_400.0] {
            assert!(dimensions(width, 144.0).is_err());
        }
    }

    #[test]
    fn all_corners_and_outward_margin_define_the_mask() {
        assert_eq!(
            pixel_bounds(&[(30, 10), (10, 30), (20, 40), (40, 20)], 100, 100).unwrap(),
            Pixels {
                left: 8,
                top: 8,
                right: 42,
                bottom: 42
            }
        );
        assert!(pixel_bounds(&[(101, 101), (110, 110)], 100, 100).is_err());
    }

    #[test]
    fn strip_mask_removes_only_intersecting_rgb_pixels() {
        let mut rgb = vec![77; 5 * 3 * 3];
        mask(
            &mut rgb,
            5,
            10,
            3,
            &[Pixels {
                left: 1,
                top: 11,
                right: 4,
                bottom: 15,
            }],
        );
        for row in 0..3 {
            for col in 0..5 {
                assert_eq!(
                    rgb[(row * 5 + col) * 3],
                    if row >= 1 && (1..4).contains(&col) {
                        0
                    } else {
                        77
                    }
                );
            }
        }
    }

    #[test]
    fn serialized_graph_checks_pixels_trailer_and_unexpected_carriers() {
        let mut doc = Document::with_version("1.7");
        doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
        let root = doc.add_object(dictionary! {"Type"=>"Catalog"});
        doc.trailer.set("Root", root);
        let pixels = vec![0, 0, 0];
        let image = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8},pixels.clone()));
        let hashes = BTreeMap::from([(image, Sha256::digest(&pixels).into())]);
        let expected = doc.clone();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(verify_graph(&bytes, None, &expected, &hashes).is_ok());
        let mut bad = expected.clone();
        bad.get_object_mut(image)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content[0] = 255;
        bytes.clear();
        bad.save_to(&mut bytes).unwrap();
        assert!(verify_graph(&bytes, None, &expected, &hashes)
            .unwrap_err()
            .contains("pixels changed"));
        let mut bad = expected.clone();
        bad.trailer.set("Info", root);
        bytes.clear();
        bad.save_to(&mut bytes).unwrap();
        assert!(verify_graph(&bytes, None, &expected, &hashes)
            .unwrap_err()
            .contains("trailer"));
        let mut bad = expected.clone();
        bad.add_object(dictionary! {"Hidden"=>Object::string_literal("SYNTHETIC")});
        bytes.clear();
        bad.save_to(&mut bytes).unwrap();
        assert!(verify_graph(&bytes, None, &expected, &hashes).is_err());
        assert!(same_object(&Object::Real(125.0), &Object::Integer(125)));
        assert!(!same_object(&Object::Real(125.5), &Object::Integer(125)));
    }
}
