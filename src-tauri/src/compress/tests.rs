use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

use super::{
    apply, deflate_streams, drawn, scaled, side, Compress, Done, Pictures, Preset, Shown,
    MAX_PIXELS,
};

/// Samples no deflate can make much of: a photograph, as far as size goes.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x2545_f491u32;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

fn deflated(raw: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}

/// An image dictionary of 8-bit samples.
fn image(width: i64, height: i64, space: Object) -> Dictionary {
    dictionary! {
        "Type" => "XObject", "Subtype" => "Image",
        "Width" => width, "Height" => height,
        "ColorSpace" => space, "BitsPerComponent" => 8,
    }
}

/// A one-page document whose page draws `content` with these XObjects.
fn page_with(doc: &mut Document, content: &str, objects: &[(&str, ObjectId)]) -> ObjectId {
    let mut named = Dictionary::new();
    for (name, id) in objects {
        named.set(*name, *id);
    }
    let content = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "XObject" => named },
    });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    page
}

/// A document drawing one image, `dict` over `data`, 72 pt square.
fn one_picture(dict: Dictionary, data: Vec<u8>) -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(Stream::new(dict, data).with_compression(false));
    page_with(
        &mut doc,
        "q 72 0 0 72 100 100 cm /Im Do Q",
        &[("Im", picture)],
    );
    (doc, picture)
}

fn stream(doc: &Document, id: ObjectId) -> &Stream {
    doc.get_object(id).unwrap().as_stream().unwrap()
}

fn whole(doc: &Document, id: ObjectId, key: &[u8]) -> i64 {
    stream(doc, id).dict.get(key).unwrap().as_i64().unwrap()
}

fn filter(doc: &Document, id: ObjectId) -> String {
    match stream(doc, id).dict.get(b"Filter") {
        Ok(Object::Name(name)) => String::from_utf8_lossy(name).into_owned(),
        Ok(other) => format!("{other:?}"),
        Err(_) => "none".to_string(),
    }
}

const SCREEN: Compress = Compress::Pictures(Pictures {
    dpi: 110,
    quality: 60,
    jpeg: true,
});

#[test]
fn a_picture_is_shown_as_large_as_the_largest_matrix_it_is_drawn_under() {
    let mut doc = Document::with_version("1.7");
    let rgb = image(10, 10, "DeviceRGB".into());
    let picture = doc.add_object(Stream::new(rgb.clone(), vec![0; 300]));
    let inside = doc.add_object(Stream::new(rgb.clone(), vec![0; 300]));
    let unseen = doc.add_object(Stream::new(rgb, vec![0; 300]));
    // A block that draws its picture one unit square, under its own matrix.
    let block = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()],
            "Matrix" => vec![2.into(), 0.into(), 0.into(), 3.into(), 0.into(), 0.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "In" => inside } },
        },
        b"/In Do".to_vec(),
    ));
    page_with(
        &mut doc,
        "q 144 0 0 72 0 0 cm /Im Do Q\n\
         q 10 0 0 200 0 0 cm /Im Do Q\n\
         q 0 300 -40 0 0 0 cm /Im Do Q\n\
         q 10 0 0 10 0 0 cm /Blk Do Q",
        &[("Im", picture), ("Blk", block)],
    );
    let shown = drawn(&doc);
    assert_eq!(
        shown.get(&picture),
        Some(&Shown {
            width: 300.0,
            height: 200.0,
            page: 0
        }),
        "the widest and the tallest of three; the widest is turned, and read by its side"
    );
    assert_eq!(
        shown.get(&inside),
        Some(&Shown {
            width: 20.0,
            height: 30.0,
            page: 0
        }),
        "the block's matrix and the page's, multiplied"
    );
    assert!(
        !shown.contains_key(&unseen),
        "one nothing draws has no size"
    );
}

#[test]
fn a_side_is_scaled_only_past_the_slack_and_never_up() {
    // 144 pt is two inches: 150 pixels an inch is 300 pixels.
    assert_eq!(side(600, 144.0, 150.0), 300);
    // 360 pixels is 180 an inch, which is the limit times the slack.
    assert_eq!(side(360, 144.0, 150.0), 360);
    assert_eq!(side(361, 144.0, 150.0), 300);
    // Already coarser than the limit.
    assert_eq!(side(100, 144.0, 150.0), 100);
    // A size that is not known scales nothing.
    assert_eq!(side(600, 0.0, 150.0), 600);
    assert_eq!(side(600, f64::INFINITY, 150.0), 600);
    assert_eq!(side(600, f64::NAN, 150.0), 600);
    // Shown very small, it is still one pixel.
    assert_eq!(side(600, 0.001, 150.0), 1);
}

#[test]
fn scaling_averages_the_area_each_new_pixel_covers() {
    // 4 by 2 to 2 by 1: each new pixel is the mean of a 2 by 2 block.
    let data = [10, 20, 100, 200, 30, 40, 110, 190];
    assert_eq!(scaled(&data, (4, 2), (2, 1), 1, None), vec![25, 150]);
    // 3 to 2 across: the middle sample is shared half and half.
    assert_eq!(
        scaled(&[0, 90, 180], (3, 1), (2, 1), 1, None),
        vec![30, 150]
    );
    // Channels are averaged apart: two RGB pixels to one.
    assert_eq!(
        scaled(&[10, 200, 0, 30, 100, 50], (2, 1), (1, 1), 3, None),
        vec![20, 150, 25]
    );
    // Down only: two rows of two to one row.
    assert_eq!(
        scaled(&[0, 10, 100, 30], (2, 2), (2, 1), 1, None),
        vec![50, 20]
    );
}

#[test]
fn a_photograph_is_scaled_to_the_preset_and_stored_as_jpeg() {
    // 400 pixels over one inch.
    let raw = noise(400 * 400 * 3);
    let mut dict = image(400, 400, "DeviceRGB".into());
    dict.set("Filter", "FlateDecode");
    let (mut doc, picture) = one_picture(dict, deflated(&raw));
    let before = stream(&doc, picture).content.len();
    let done = apply(&mut doc, SCREEN);
    assert_eq!((done.pictures, done.pictures_changed), (1, 1));
    assert_eq!(
        (
            whole(&doc, picture, b"Width"),
            whole(&doc, picture, b"Height")
        ),
        (110, 110)
    );
    assert_eq!(filter(&doc, picture), "DCTDecode");
    assert!(!stream(&doc, picture).dict.has(b"DecodeParms"));
    let after = &stream(&doc, picture).content;
    assert!(after.len() * 4 < before, "{} against {before}", after.len());
    assert_eq!(&after[..2], &[0xff, 0xd8], "a JPEG");
    // The other presets ask for more pixels.
    for (preset, side) in [(Preset::Balanced, 150), (Preset::Print, 300)] {
        let mut dict = image(400, 400, "DeviceRGB".into());
        dict.set("Filter", "FlateDecode");
        let (mut doc, picture) = one_picture(dict, deflated(&raw));
        let _ = apply(&mut doc, Compress::Pictures(preset.into()));
        assert_eq!(whole(&doc, picture, b"Width"), side, "{preset:?}");
    }
}

#[test]
fn a_flat_drawing_is_scaled_and_stays_without_loss() {
    // Left half black, right half white: a screenshot, as far as size goes.
    let mut raw = Vec::new();
    for _ in 0..400 {
        raw.extend(std::iter::repeat_n(0u8, 200));
        raw.extend(std::iter::repeat_n(255u8, 200));
    }
    let (mut doc, picture) = one_picture(image(400, 400, "DeviceGray".into()), raw);
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_changed, 1);
    assert_eq!(filter(&doc, picture), "FlateDecode");
    assert_eq!(whole(&doc, picture, b"Width"), 110);
    // Read back through the predictor it was written with.
    let pixels = stream(&doc, picture)
        .decompressed_content_with_limit(1 << 20)
        .expect("decodes");
    assert_eq!(pixels.len(), 110 * 110);
    assert!(pixels
        .chunks(110)
        .all(|row| row[..55] == [0; 55] && row[55..] == [255; 55]));
}

#[test]
fn a_smooth_drawing_stays_lossless_though_a_jpeg_of_it_would_be_smaller() {
    // A soft gradient: deflate leaves a few percent of it and JPEG less still.
    // It is not photo-like, so it is not handed to JPEG, which would ring at
    // every edge a real drawing of this kind has.
    let raw: Vec<u8> = (0..110usize * 110)
        .map(|at| (((at % 110) * (at % 110) + (at / 110) * (at / 110)) / 96) as u8)
        .collect();
    let lossless = super::deflated_rows(&raw, 110).expect("deflates");
    let lossy = super::jpeg(&raw, 110, 110, 1, 60).expect("encodes");
    assert!(
        lossy.len() < lossless.len() && lossless.len() * 4 < raw.len(),
        "the fixture: {} as JPEG, {} deflated, {} raw",
        lossy.len(),
        lossless.len(),
        raw.len()
    );
    let (mut doc, picture) = one_picture(image(110, 110, "DeviceGray".into()), raw);
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 1);
    assert_eq!(filter(&doc, picture), "FlateDecode");
}

#[test]
fn a_picture_already_coarse_enough_is_not_scaled() {
    // 100 pixels over one inch, photo-like and stored without loss: it becomes
    // a JPEG of the same size.
    let raw = noise(100 * 100 * 3);
    let mut dict = image(100, 100, "DeviceRGB".into());
    dict.set("Filter", "FlateDecode");
    let (mut doc, picture) = one_picture(dict, deflated(&raw));
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_changed, 1);
    assert_eq!(whole(&doc, picture, b"Width"), 100);
    assert_eq!(filter(&doc, picture), "DCTDecode");

    // The same picture already a JPEG is left: encoding it again only loses.
    let jpeg = super::jpeg(&raw, 100, 100, 3, 90).expect("encodes");
    let mut dict = image(100, 100, "DeviceRGB".into());
    dict.set("Filter", "DCTDecode");
    let (mut doc, picture) = one_picture(dict, jpeg.clone());
    let done = apply(&mut doc, SCREEN);
    assert_eq!((done.pictures, done.pictures_changed), (1, 0));
    assert_eq!(stream(&doc, picture).content, jpeg);
}

#[test]
fn a_jpeg_drawn_too_fine_is_decoded_scaled_and_encoded_again() {
    let raw = noise(400 * 400);
    let jpeg = super::jpeg(&raw, 400, 400, 1, 90).expect("encodes");
    let mut dict = image(400, 400, "DeviceGray".into());
    dict.set("Filter", "DCTDecode");
    let (mut doc, picture) = one_picture(dict, jpeg);
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_changed, 1);
    assert_eq!(whole(&doc, picture, b"Width"), 110);
    assert_eq!(filter(&doc, picture), "DCTDecode");

    // A JPEG of three components under a one-component colour space is not
    // the picture its dictionary describes, and is left.
    let colour = super::jpeg(&noise(400 * 400 * 3), 400, 400, 3, 90).expect("encodes");
    let mut dict = image(400, 400, "DeviceGray".into());
    dict.set("Filter", "DCTDecode");
    let (mut doc, picture) = one_picture(dict, colour.clone());
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 0);
    assert_eq!(stream(&doc, picture).content, colour);
}

#[test]
fn what_could_change_by_more_than_resolution_is_left_as_it_is() {
    let raw = noise(400 * 400);
    let gray = || image(400, 400, "DeviceGray".into());
    let with = |key: &str, value: Object| {
        let mut dict = gray();
        dict.set(key, value);
        dict
    };
    let indexed = Object::Array(vec![
        "Indexed".into(),
        "DeviceRGB".into(),
        255.into(),
        Object::string_literal(vec![0u8; 768]),
    ]);
    let cases: Vec<(&str, Dictionary)> = vec![
        ("a stencil mask", with("ImageMask", true.into())),
        ("one bit a sample", with("BitsPerComponent", 1.into())),
        (
            "a decode array",
            with("Decode", vec![1.into(), 0.into()].into()),
        ),
        (
            "a colour key",
            with("Mask", vec![0.into(), 0.into()].into()),
        ),
        ("an indexed colour space", with("ColorSpace", indexed)),
        ("four components", with("ColorSpace", "DeviceCMYK".into())),
        (
            "a filter this does not decode",
            with("Filter", "JPXDecode".into()),
        ),
        (
            "two filters",
            with(
                "Filter",
                vec!["ASCIIHexDecode".into(), "FlateDecode".into()].into(),
            ),
        ),
        ("more pixels than the limit", {
            let mut dict = gray();
            dict.set("Width", i64::try_from(MAX_PIXELS).unwrap());
            dict.set("Height", 2);
            dict
        }),
        ("samples that are not the size it states", {
            let mut dict = gray();
            dict.set("Height", 500);
            dict
        }),
    ];
    for (what, dict) in cases {
        let (mut doc, picture) = one_picture(dict.clone(), raw.clone());
        let done = apply(&mut doc, SCREEN);
        assert_eq!(done.pictures_changed, 0, "{what}");
        let after = stream(&doc, picture);
        assert_eq!(after.content, raw, "{what}: its samples");
        assert_eq!(
            after.dict.get(b"Width").unwrap().as_i64().unwrap(),
            dict.get(b"Width").unwrap().as_i64().unwrap(),
            "{what}: its size"
        );
    }
    // The control: the plain picture those were made from is shrunk.
    let (mut doc, _) = one_picture(gray(), raw.clone());
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 1);

    // And one no page draws is left, though it could be shrunk.
    let mut doc = Document::with_version("1.7");
    let unseen = doc.add_object(Stream::new(gray(), raw.clone()).with_compression(false));
    page_with(&mut doc, "", &[("Im", unseen)]);
    assert_eq!(
        apply(&mut doc, SCREEN),
        Done {
            pictures: 0,
            pictures_changed: 0,
            streams_deflated: 0,
            pictures_unsized: 0,
        }
    );
    assert_eq!(stream(&doc, unseen).content, raw);
}

#[test]
fn a_page_unit_scales_all_uses_including_a_soft_mask_also_drawn_directly() {
    let mut doc = Document::with_version("1.7");
    let mask = doc.add_object(Stream::new(
        image(400, 400, "DeviceGray".into()),
        noise(400 * 400),
    ));
    let mut img = image(400, 400, "DeviceRGB".into());
    img.set("SMask", mask);
    let parent = doc.add_object(Stream::new(img, noise(400 * 400 * 3)));
    let page = page_with(
        &mut doc,
        "q 72 0 0 72 0 0 cm /Im Do Q q 18 0 0 18 0 0 cm /G Do Q",
        &[("Im", parent), ("G", mask)],
    );
    doc.get_dictionary_mut(page).unwrap().set("UserUnit", 4);
    let _ = apply(&mut doc, SCREEN);
    assert_eq!(whole(&doc, parent, b"Width"), 400);
    assert_eq!(whole(&doc, mask, b"Width"), 400);
    assert_ne!(filter(&doc, mask), "DCTDecode");
}

#[test]
fn the_preview_ignores_a_larger_losslessly_reencoded_image() {
    let mut doc = Document::with_version("1.7");
    let flat = doc.add_object(Stream::new(
        image(500, 500, "DeviceGray".into()),
        vec![80; 500 * 500],
    ));
    let photo = doc.add_object(Stream::new(
        image(400, 400, "DeviceRGB".into()),
        noise(400 * 400 * 3),
    ));
    pages_with(
        &mut doc,
        &[
            ("q 475 0 0 475 0 0 cm /Flat Do Q", &[("Flat", flat)]),
            ("q 400 0 0 400 0 0 cm /Photo Do Q", &[("Photo", photo)]),
        ],
    );
    assert_eq!(
        super::estimate(&doc, SCREEN).unwrap().preview_pages[0].page,
        1
    );
}

#[test]
fn a_shared_images_preview_names_the_page_where_its_dpi_was_measured() {
    let (mut doc, image) = one_picture(image(600, 600, "DeviceGray".into()), noise(600 * 600));
    let first = doc.get_pages()[&1];
    let mut second = doc.get_dictionary(first).unwrap().clone();
    let pages = second.get(b"Parent").unwrap().as_reference().unwrap();
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 288 0 0 288 0 0 cm /Im Do Q".to_vec(),
    ));
    second.set("Contents", content);
    let second = doc.add_object(second);
    let root = doc.get_dictionary_mut(pages).unwrap();
    root.set(
        "Kids",
        vec![Object::Reference(first), Object::Reference(second)],
    );
    root.set("Count", 2);
    let estimate = super::estimate(&doc, SCREEN).unwrap();
    let focus = estimate.preview_pages[0];
    assert_eq!(
        (focus.page, focus.dpi_before, focus.dpi_after),
        (1, 150, 110)
    );
    assert_eq!(whole(&doc, image, b"Width"), 600);
}

#[test]
fn a_pattern_and_its_thumbnail_share_original_pixels_while_another_thumbnail_shrinks() {
    let mut doc = Document::with_version("1.7");
    let raw =
        Stream::new(image(400, 400, "DeviceGray".into()), noise(400 * 400)).with_compression(false);
    let logo = doc.add_object(raw.clone());
    let other = doc.add_object(raw);
    let page = page_with(
        &mut doc,
        "q 12 0 0 12 0 0 cm /Logo Do /Other Do Q /Pattern cs /P scn 0 0 256 256 re f",
        &[("Logo", logo), ("Other", other)],
    );
    let pattern = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"Pattern", "PatternType"=>1, "PaintType"=>1, "TilingType"=>1,
            "BBox"=>vec![0.into(),0.into(),256.into(),256.into()], "XStep"=>256, "YStep"=>256,
            "Resources"=>dictionary!{"XObject"=>dictionary!{"Logo"=>logo}},
        },
        b"q 256 0 0 256 0 0 cm /Logo Do Q".to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Pattern", dictionary! {"P"=>pattern});
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_changed, 1);
    assert_eq!(whole(&doc, logo, b"Width"), 400);
    assert!(whole(&doc, other, b"Width") < 400);
}

#[test]
fn a_type3_glyph_keeps_shared_pixels_at_their_original_resolution() {
    let mut doc = Document::with_version("1.7");
    let raw = Stream::new(image(256, 256, "DeviceGray".into()), noise(256 * 256));
    let logo = doc.add_object(raw.clone());
    let other = doc.add_object(raw);
    let page = page_with(
        &mut doc,
        "q 12 0 0 12 0 0 cm /Logo Do /Other Do Q BT /Glyph 256 Tf (A) Tj ET",
        &[("Logo", logo), ("Other", other)],
    );
    let charproc = doc.add_object(Stream::new(
        Dictionary::new(),
        b"1000 0 d0 q 1000 0 0 1000 0 0 cm /Im Do Q".to_vec(),
    ));
    let font = dictionary! {"Type"=>"Font","Subtype"=>"Type3","CharProcs"=>dictionary!{"A"=>charproc},
    "Resources"=>dictionary!{"XObject"=>dictionary!{"Im"=>logo}}};
    doc.get_dictionary_mut(page)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Font", dictionary! {"Glyph"=>font});
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 1);
    assert_eq!(whole(&doc, logo, b"Width"), 256);
    assert!(whole(&doc, other, b"Width") < 256);
}

#[test]
fn a_transparency_mask_keeps_shared_pixels_while_an_independent_thumbnail_shrinks() {
    let mut doc = Document::with_version("1.7");
    let raw = Stream::new(image(256, 256, "DeviceGray".into()), noise(256 * 256));
    let shared = doc.add_object(raw.clone());
    let other = doc.add_object(raw);
    let page = page_with(
        &mut doc,
        "q 12 0 0 12 0 0 cm /Logo Do /Other Do Q q /Soft gs 0 0 256 256 re f Q",
        &[("Logo", shared), ("Other", other)],
    );
    let group = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),256.into(),256.into()],
            "Group"=>dictionary!{"S"=>"Transparency", "CS"=>"DeviceGray"},
            "Resources"=>dictionary!{"XObject"=>dictionary!{"MaskPixels"=>shared}},
        },
        b"q 256 0 0 256 0 0 cm /MaskPixels Do Q".to_vec(),
    ));
    let mask = doc.add_object(dictionary! {"S"=>"Luminosity", "G"=>group});
    doc.get_dictionary_mut(page)
        .unwrap()
        .get_mut(b"Resources")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "ExtGState",
            dictionary! {"Soft"=>dictionary!{"SMask"=>mask}},
        );
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 1);
    assert_eq!(whole(&doc, shared, b"Width"), 256);
    assert!(whole(&doc, other, b"Width") < 256);
}

#[test]
fn preview_accepts_indirect_image_dimensions() {
    let (mut doc, image) = one_picture(image(400, 400, "DeviceGray".into()), noise(400 * 400));
    let width = doc.add_object(Object::Integer(400));
    doc.get_object_mut(image)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("Width", width);
    let estimated = super::estimate(&doc, SCREEN).unwrap();
    assert_eq!(estimated.estimate.done.pictures_changed, 1);
    assert_eq!(estimated.preview_pages[0].page, 0);
}

#[test]
fn a_page_logo_also_used_by_a_stamp_keeps_its_resolution() {
    let (mut doc, id) = one_picture(image(400, 400, "DeviceGray".into()), noise(400 * 400));
    let page = doc.get_pages()[&1];
    let form = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Resources" => dictionary! { "XObject" => dictionary! { "Logo" => id } },
            "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()],
        },
        b"/Logo Do".to_vec(),
    ));
    let stamp = doc.add_object(dictionary! { "Subtype" => "Stamp", "Rect" => vec![0.into(), 0.into(), 360.into(), 360.into()], "AP" => dictionary! { "N" => form } });
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![Object::Reference(stamp)]);
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_unsized, 1);
    assert_eq!(whole(&doc, id, b"Width"), 400);
}

#[test]
fn a_change_only_to_opacity_counts_the_parent_and_selects_a_preview() {
    let pixels = vec![80; 72 * 72];
    let mut parent = image(72, 72, "DeviceGray".into());
    parent.set("Filter", "DCTDecode");
    let (mut doc, id) = one_picture(parent, super::jpeg(&pixels, 72, 72, 1, 80).unwrap());
    let mask = doc.add_object(Stream::new(
        image(400, 400, "DeviceGray".into()),
        noise(400 * 400),
    ));
    doc.get_object_mut(id)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("SMask", mask);
    let estimate = super::estimate(&doc, SCREEN).unwrap();
    assert_eq!(estimate.estimate.done.pictures_changed, 1);
    let focus = estimate.preview_pages[0];
    assert_eq!(
        (focus.page, focus.dpi_before, focus.dpi_after),
        (0, 400, 110)
    );
}

#[test]
fn resampling_color_uses_only_the_visible_part_of_each_pixel() {
    let colors = [255, 255, 255, 0, 0, 0];
    let opacity = [255, 0];
    assert_eq!(scaled(&colors, (2, 1), (1, 1), 3, Some(&opacity)), [255; 3]);
    assert_eq!(scaled(&opacity, (2, 1), (1, 1), 1, None), [128]);
    assert_eq!(scaled(&colors, (2, 1), (1, 1), 3, Some(&[0, 0])), [0; 3]);
}

#[test]
fn a_finer_mask_keeps_its_grid_when_its_nonuniform_picture_cannot_follow() {
    let (mut doc, parent) = one_picture(image(220, 220, "DeviceGray".into()), noise(220 * 220));
    let mask = doc.add_object(Stream::new(
        image(440, 440, "DeviceGray".into()),
        noise(440 * 440),
    ));
    doc.get_object_mut(parent)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("SMask", mask);
    let before = stream(&doc, mask).content.clone();
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_changed, 0);
    assert_eq!(whole(&doc, parent, b"Width"), 220);
    assert_eq!(whole(&doc, mask, b"Width"), 440);
    // Unfiltered data may be compressed without changing its samples.
    assert_eq!(
        super::samples(&doc, mask, &super::picture(&doc, mask, true).unwrap()).unwrap(),
        before
    );
}

#[test]
fn a_soft_mask_is_scaled_with_its_picture_and_never_stored_as_jpeg() {
    let mut doc = Document::with_version("1.7");
    let mask = doc.add_object(
        Stream::new(image(400, 400, "DeviceGray".into()), noise(400 * 400)).with_compression(false),
    );
    let mut dict = image(400, 400, "DeviceRGB".into());
    dict.set("SMask", mask);
    let picture = doc.add_object(Stream::new(dict, noise(400 * 400 * 3)).with_compression(false));
    page_with(&mut doc, "q 72 0 0 72 0 0 cm /Im Do Q", &[("Im", picture)]);
    let done = apply(&mut doc, SCREEN);
    assert_eq!((done.pictures, done.pictures_changed), (1, 1));
    assert_eq!(filter(&doc, picture), "DCTDecode");
    assert_eq!(whole(&doc, mask, b"Width"), 110);
    assert_eq!(
        filter(&doc, mask),
        "FlateDecode",
        "noise, and still not a JPEG"
    );

    // A mask blended against a matte keeps itself and its picture as they are.
    let mut doc = Document::with_version("1.7");
    let mut matte = image(400, 400, "DeviceGray".into());
    matte.set("Matte", vec![0.into(), 0.into(), 0.into()]);
    let raw_mask = noise(400 * 400);
    let mask = doc.add_object(Stream::new(matte, raw_mask.clone()).with_compression(false));
    let mut dict = image(400, 400, "DeviceRGB".into());
    dict.set("SMask", mask);
    let raw = noise(400 * 400 * 3);
    let picture = doc.add_object(Stream::new(dict, raw.clone()).with_compression(false));
    page_with(&mut doc, "q 72 0 0 72 0 0 cm /Im Do Q", &[("Im", picture)]);
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 0);
    assert_eq!(stream(&doc, picture).content, raw);
    assert_eq!(stream(&doc, mask).content, raw_mask);
}

#[test]
fn a_picture_whose_new_form_is_hardly_smaller_is_left() {
    // Already coarse and already deflated as well as it will be: the new form
    // would be the same bytes behind a predictor.
    let raw = vec![7u8; 100 * 100];
    let mut dict = image(100, 100, "DeviceGray".into());
    dict.set("Filter", "FlateDecode");
    let stored = {
        use std::io::Write as _;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&raw).unwrap();
        encoder.finish().unwrap()
    };
    let (mut doc, picture) = one_picture(dict, stored.clone());
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 0);
    assert_eq!(stream(&doc, picture).content, stored);
    assert!(!stream(&doc, picture).dict.has(b"DecodeParms"));
}

#[test]
fn the_count_only_estimate_counts_drawn_pictures_in_the_working_document() {
    let (pictured, _) = one_picture(image(400, 400, "DeviceGray".into()), vec![0; 400 * 400]);
    assert_eq!(super::picture_count(&pictured).done.pictures, 1);
    let mut empty = Document::with_version("1.7");
    page_with(&mut empty, "", &[]);
    assert_eq!(super::picture_count(&empty).done.pictures, 0);
}

#[test]
fn without_a_preset_no_picture_is_touched_and_plain_streams_are_deflated() {
    let raw = noise(400 * 400);
    let mut dict = image(400, 400, "DeviceGray".into());
    dict.set("Filter", "FlateDecode");
    let stored = deflated(&raw);
    let (mut doc, picture) = one_picture(dict, stored);
    // A long content stream and an XMP packet, both stored plainly.
    let text = "BT /F1 12 Tf (the same line again) Tj ET\n".repeat(200);
    let plain = doc.add_object(Stream::new(dictionary! {}, text.clone().into_bytes()));
    let xmp = doc.add_object(Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        text.clone().into_bytes(),
    ));

    let mut untouched = doc.clone();
    assert_eq!(apply(&mut untouched, Compress::No), Done::default());
    assert_eq!(stream(&untouched, plain).content, text.as_bytes());

    let done = apply(&mut doc, Compress::Lossless);
    assert_eq!((done.pictures, done.pictures_changed), (0, 0));
    assert_eq!(whole(&doc, picture, b"Width"), 400);
    assert_eq!(filter(&doc, picture), "FlateDecode");
    assert_eq!(
        stream(&doc, picture)
            .decompressed_content_with_limit(1 << 20)
            .unwrap(),
        raw,
        "its samples are the ones it had"
    );
    assert_eq!(filter(&doc, plain), "FlateDecode");
    assert!(stream(&doc, plain).content.len() * 10 < text.len());
    assert_eq!(
        stream(&doc, plain)
            .decompressed_content_with_limit(1 << 20)
            .unwrap(),
        text.as_bytes()
    );
    assert_eq!(filter(&doc, xmp), "none", "an XMP packet stays readable");
    // The page's own content, the long stream: two. Not the packet.
    assert_eq!(
        done.streams_deflated, 1,
        "the page's short content does not shrink"
    );
}

#[test]
fn a_deflated_stream_is_deflated_again_only_when_that_is_smaller() {
    let text = "0 0 m 100 100 l S\n".repeat(500).into_bytes();
    let loosely = deflated(&text);
    let tightly = {
        use std::io::Write as _;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&text).unwrap();
        encoder.finish().unwrap()
    };
    assert!(tightly.len() < loosely.len(), "the fixture");
    let mut doc = Document::with_version("1.7");
    let flate = |data: &[u8]| Stream::new(dictionary! { "Filter" => "FlateDecode" }, data.to_vec());
    let loose = doc.add_object(flate(&loosely));
    let tight = doc.add_object(flate(&tightly));
    // Not deflate at all under the name: left as it is.
    let broken = doc.add_object(flate(b"not a zlib stream"));
    // Two filters, deflate first: what deflate gives is not the stream's
    // content but the next filter's input, and the pair is left alone.
    let two = doc.add_object(Stream::new(
        dictionary! { "Filter" => vec!["FlateDecode".into(), "ASCIIHexDecode".into()] },
        loosely.clone(),
    ));
    assert_eq!(deflate_streams(&mut doc), 0, "none was stored plainly");
    assert_eq!(stream(&doc, loose).content, tightly);
    assert_eq!(stream(&doc, tight).content, tightly);
    assert_eq!(stream(&doc, broken).content, b"not a zlib stream");
    assert_eq!(stream(&doc, two).content, loosely);
}

#[test]
fn settings_chosen_freely_are_followed_and_bounded() {
    let raw = noise(400 * 400 * 3);
    let shrunk = |how: Pictures| {
        let mut dict = image(400, 400, "DeviceRGB".into());
        dict.set("Filter", "FlateDecode");
        let (mut doc, picture) = one_picture(dict, deflated(&raw));
        let _ = apply(&mut doc, Compress::Pictures(how));
        (
            whole(&doc, picture, b"Width"),
            filter(&doc, picture),
            stream(&doc, picture).content.len(),
        )
    };
    let (width, kind, coarse) = shrunk(Pictures {
        dpi: 200,
        quality: 20,
        jpeg: true,
    });
    assert_eq!((width, kind.as_str()), (200, "DCTDecode"));
    let (_, _, fine) = shrunk(Pictures {
        dpi: 200,
        quality: 95,
        jpeg: true,
    });
    assert!(
        coarse * 2 < fine,
        "quality 20 is {coarse} bytes and 95 is {fine}"
    );
    // Without JPEG a photograph is scaled and stays lossless.
    let (width, kind, _) = shrunk(Pictures {
        dpi: 200,
        quality: 20,
        jpeg: false,
    });
    assert_eq!((width, kind.as_str()), (200, "FlateDecode"));

    let ok = Pictures {
        dpi: 150,
        quality: 75,
        jpeg: true,
    };
    assert_eq!(ok.checked(), Ok(ok));
    for (dpi, quality) in [(19, 75), (1201, 75), (150, 0), (150, 101)] {
        assert!(
            Pictures {
                dpi,
                quality,
                jpeg: true
            }
            .checked()
            .is_err(),
            "{dpi} {quality}"
        );
    }
    for (dpi, quality) in [(20, 1), (1200, 100)] {
        assert!(
            Pictures {
                dpi,
                quality,
                jpeg: true
            }
            .checked()
            .is_ok(),
            "{dpi} {quality}"
        );
    }
    // A preset is three numbers, and three numbers that are one name it.
    assert_eq!(Pictures::from(Preset::Balanced), ok);
    assert_eq!(ok.preset(), Some(Preset::Balanced));
    assert_eq!(Pictures { quality: 76, ..ok }.preset(), None);
    assert_eq!(Pictures { jpeg: false, ..ok }.preset(), None);
}

#[test]
fn an_estimate_writes_nothing_and_names_the_page_that_loses_most() {
    use super::{estimate, Focus};
    // Two pages. The second draws the picture that loses the most pixels:
    // 600 across one inch against 300.
    let mut doc = Document::with_version("1.7");
    let small = doc.add_object(
        Stream::new(image(300, 300, "DeviceRGB".into()), noise(300 * 300 * 3))
            .with_compression(false),
    );
    let large = doc.add_object(
        Stream::new(image(600, 600, "DeviceGray".into()), noise(600 * 600)).with_compression(false),
    );
    let pages = doc.new_object_id();
    let mut kids = Vec::new();
    for (name, picture) in [("A", small), ("B", large)] {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("q 72 0 0 72 0 0 cm /{name} Do Q").into_bytes(),
        ));
        let mut named = Dictionary::new();
        named.set(name, picture);
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "XObject" => named },
        })));
    }
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);

    let before = doc.clone();
    let estimated = super::estimate(&doc, SCREEN).expect("estimates");
    assert_eq!(doc.objects, before.objects, "the document is as it was");
    let done = estimated.estimate.done;
    assert_eq!((done.pictures, done.pictures_changed), (2, 2));
    assert_eq!(
        estimated.preview_pages.first().copied(),
        Some(Focus {
            page: 1,
            dpi_before: 600,
            dpi_after: 110
        })
    );
    assert!(
        estimated.estimate.sample.is_none(),
        "drawing a page is the caller's"
    );

    // The size is the size of the copy the same settings write, and the bytes
    // are that copy.
    let mut copy = doc.clone();
    let _ = apply(&mut copy, SCREEN);
    let written = crate::save::serialise_packed(&mut copy, "the copy").expect("serialises");
    assert_eq!(estimated.estimate.bytes_after, written.len() as u64);
    assert_eq!(estimated.bytes.len(), written.len());

    // Nowhere to look when no picture changes.
    let lossless = estimate(&doc, Compress::Lossless).expect("estimates");
    assert!(lossless.preview_pages.is_empty());
    assert_eq!(lossless.estimate.done.pictures, 2);
    assert_eq!(lossless.estimate.done.pictures_changed, 0);
}

#[test]
fn a_pdfa_one_declaration_keeps_the_classic_xref_and_metadata() {
    use lopdf::xref::XrefType;
    for part in [1, 2] {
        let mut doc = Document::with_version("1.4");
        doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        page_with(&mut doc, "q Q", &[]);
        let packet = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:id="http://www.aiim.org/pdfa/ns/id/" id:part="{part}" id:conformance="B"/>
            </rdf:RDF>"#
        );
        let id = doc.add_object(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            packet.as_bytes().to_vec(),
        ));
        doc.catalog_mut().unwrap().set("Metadata", id);
        let estimated = super::estimate(&doc, Compress::Lossless).unwrap();
        assert_eq!(estimated.estimate.done.pictures, 0);
        let copy = Document::load_mem(&estimated.bytes).unwrap();
        assert_eq!(
            matches!(
                copy.reference_table.cross_reference_type,
                XrefType::CrossReferenceTable
            ),
            part == 1
        );
        assert_eq!(stream(&copy, id).content, packet.as_bytes());
        if part == 1 {
            assert_eq!(copy.version, "1.4");
        }
    }
}

#[test]
fn a_sample_is_the_part_of_the_page_that_differs_most() {
    use super::{Focus, Sample};
    let focus = Focus {
        page: 4,
        dpi_before: 300,
        dpi_after: 110,
    };
    // 800 by 500, white. The copy differs a little near the top left and a
    // lot in a patch near the lower right.
    let (width, height) = (800u32, 500u32);
    let before = vec![255u8; (width * height * 4) as usize];
    let mut after = before.clone();
    let mut paint = |x0: u32, y0: u32, side: u32, value: u8| {
        for y in y0..y0 + side {
            for x in x0..x0 + side {
                let at = ((y * width + x) * 4) as usize;
                after[at..at + 3].copy_from_slice(&[value; 3]);
            }
        }
    };
    paint(20, 20, 40, 250);
    paint(600, 300, 100, 0);
    let sample = Sample::of_page(&before, &after, width, height, focus, 200).expect("differs");
    assert_eq!((sample.width, sample.height), (320, 320));
    assert_eq!((sample.page, sample.zoom_percent), (5, 200));
    assert_eq!((sample.dpi_before, sample.dpi_after), (300, 110));
    assert_eq!(sample.before.len(), 320 * 320 * 3);
    assert!(sample.before.iter().all(|value| *value == 255));
    // The whole dark patch is in it: 100 by 100 pixels of three zeros.
    assert_eq!(
        sample.after.iter().filter(|value| **value == 0).count(),
        100 * 100 * 3
    );

    // Side by side: before, eight white pixels, after.
    let (across, down, pixels) = sample.side_by_side().expect("its size is its pixels");
    assert_eq!((across, down), (648, 320));
    assert_eq!(pixels.len(), 648 * 320 * 4);
    assert_eq!(sample.before_rgba().len(), 320 * 320 * 4);
    assert_eq!(
        &sample.after_rgba()[..4],
        &[sample.after[0], sample.after[1], sample.after[2], 255]
    );
    let dark = pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[..3] == [0, 0, 0])
        .count();
    assert_eq!(dark, 100 * 100, "the after half holds the patch");
    assert!(pixels
        .chunks_exact(648 * 4)
        .all(|row| row[..320 * 4].iter().all(|v| *v == 255)));

    // Two drawings that do not differ, or are not the size stated, give none.
    assert!(Sample::of_page(&before, &before, width, height, focus, 200).is_none());
    assert!(Sample::of_page(&before, &after, width, height + 1, focus, 200).is_none());
    // A single-pixel detail on any row or column is visible in the preview.
    let blank = vec![255; 64 * 64 * 4];
    for (x, y) in [(30, 30), (30, 31), (31, 30), (31, 31)] {
        let mut line = blank.clone();
        line[(y * 64 + x) * 4] = 0;
        let found = Sample::of_page(&blank, &line, 64, 64, focus, 200).unwrap();
        assert_eq!(found.after[(y * 64 + x) * 3], 0);
    }
    // A page smaller than the window is the whole page.
    let tiny =
        Sample::of_page(&[0, 0, 0, 255], &[9, 9, 9, 255], 1, 1, focus, 200).expect("differs");
    assert_eq!((tiny.width, tiny.height, tiny.after), (1, 1, vec![9, 9, 9]));
}

#[test]
fn a_preset_is_named_by_one_word() {
    for (word, preset) in [
        ("screen", Preset::Screen),
        ("balanced", Preset::Balanced),
        ("print", Preset::Print),
    ] {
        assert_eq!(Preset::named(word), Some(preset));
    }
    assert_eq!(Preset::named("Screen"), None);
    assert_eq!(Preset::named(""), None);
    // Finer and better as they go.
    assert!(Preset::Screen.dpi() < Preset::Balanced.dpi());
    assert!(Preset::Balanced.dpi() < Preset::Print.dpi());
    assert!(Preset::Screen.quality() < Preset::Balanced.quality());
    assert!(Preset::Balanced.quality() < Preset::Print.quality());
}

/// A deflated picture whose predictor would not be undone is left as it is.
///
/// TIFF predictor differences have the same byte length as decoded samples.
/// Accept both direct parameter spellings, and leave unresolved parameters alone.
#[test]
fn a_deflated_picture_whose_predictor_would_not_be_undone_is_left() {
    // Left half black, right half white, each row differenced along itself.
    let mut differenced = Vec::new();
    for _ in 0..400 {
        let mut row = vec![0u8; 400];
        row[200] = 255;
        differenced.extend(row);
    }
    let stored = deflated(&differenced);
    // How a fixture spells its parameters, given the dictionary itself.
    type Spelled = fn(&mut Document, Dictionary) -> Object;
    let picture_with = |decode_parms: Spelled| {
        let mut doc = Document::with_version("1.7");
        let mut dict = image(400, 400, "DeviceGray".into());
        dict.set("Filter", "FlateDecode");
        let parms = dictionary! { "Predictor" => 2, "Colors" => 1, "Columns" => 400 };
        let value = decode_parms(&mut doc, parms);
        dict.set("DecodeParms", value);
        let picture = doc.add_object(Stream::new(dict, stored.clone()).with_compression(false));
        page_with(
            &mut doc,
            "q 72 0 0 72 100 100 cm /Im Do Q",
            &[("Im", picture)],
        );
        (doc, picture)
    };

    let readable: [Spelled; 2] = [
        |_, parms| parms.into(),
        |_, parms| vec![Object::Dictionary(parms)].into(),
    ];
    for spell in readable {
        let (mut doc, picture) = picture_with(spell);
        assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 1);
        assert_eq!(whole(&doc, picture, b"Width"), 110);
        let pixels = stream(&doc, picture)
            .decompressed_content_with_limit(1 << 20)
            .expect("decodes");
        assert!(pixels
            .chunks(110)
            .all(|row| row[..55] == [0; 55] && row[55..] == [255; 55]));
    }
    let cases: [(&str, Spelled); 3] = [
        ("an array containing a reference", |doc, parms| {
            vec![Object::Reference(doc.add_object(parms))].into()
        }),
        ("a reference", |doc, parms| doc.add_object(parms).into()),
        ("a predictor given by reference", |doc, mut parms| {
            let two = doc.add_object(Object::Integer(2));
            parms.set("Predictor", two);
            parms.into()
        }),
    ];
    for (what, decode_parms) in cases {
        let (mut doc, picture) = picture_with(decode_parms);
        let done = apply(&mut doc, SCREEN);
        assert_eq!(done.pictures_changed, 0, "{what}: it was scaled");
        assert_eq!(whole(&doc, picture, b"Width"), 400, "{what}: its size");
        assert!(
            stream(&doc, picture).dict.has(b"DecodeParms"),
            "{what}: and its parameters are still beside it"
        );
    }
}

/// Pages that each draw `content` with these XObjects, in order.
fn pages_with(doc: &mut Document, pages: &[(&str, &[(&str, ObjectId)])]) {
    let tree = doc.new_object_id();
    let mut kids = Vec::new();
    for (content, objects) in pages {
        let mut named = Dictionary::new();
        for (name, id) in *objects {
            named.set(*name, *id);
        }
        let content = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => tree, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "XObject" => named },
        })));
    }
    let count = i64::try_from(kids.len()).unwrap();
    doc.objects.insert(
        tree,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }.into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
    doc.trailer.set("Root", catalog);
}

/// A picture behind a PNG predictor, which is how this module stores every
/// picture it keeps without loss, is read and shrunk again.
///
/// The inflated bytes still carry a filter byte a row when they are held to
/// the limit, so a limit of the samples alone refused all of them.
#[test]
fn a_picture_behind_a_png_predictor_is_read_and_shrunk_again() {
    let lossless = Pictures {
        dpi: 110,
        quality: 60,
        jpeg: false,
    };
    let raw = noise(400 * 400);
    let mut dict = image(400, 400, "DeviceGray".into());
    let parms =
        dictionary! { "Predictor" => 15, "Colors" => 1, "BitsPerComponent" => 8, "Columns" => 400 };
    let stored = super::deflated_rows(&raw, 400).expect("deflates");
    dict.set("Filter", "FlateDecode");
    dict.set("DecodeParms", parms.clone());
    let (mut doc, picture) = one_picture(dict.clone(), stored.clone());
    dict.set("Filter", vec![Object::Name(b"FlateDecode".to_vec())]);
    dict.set("DecodeParms", vec![Object::Dictionary(parms)]);
    let (mut array_doc, array_picture) = one_picture(dict, stored);
    for (doc, id) in [(&mut doc, picture), (&mut array_doc, array_picture)] {
        assert_eq!(apply(doc, Compress::Pictures(lossless)).pictures_changed, 1);
        assert_eq!(whole(doc, id, b"Width"), 110);
        assert_eq!(filter(doc, id), "FlateDecode");
    }
    let decoded = |doc: &Document, id| {
        stream(doc, id)
            .decompressed_content_with_limit(1 << 20)
            .unwrap()
    };
    assert_eq!(decoded(&doc, picture), decoded(&array_doc, array_picture));

    // What it wrote, read once more for a coarser setting.
    let coarser = Pictures {
        dpi: 50,
        ..lossless
    };
    assert_eq!(
        apply(&mut doc, Compress::Pictures(coarser)).pictures_changed,
        1
    );
    assert_eq!(whole(&doc, picture, b"Width"), 50);
    let pixels = stream(&doc, picture)
        .decompressed_content_with_limit(1 << 20)
        .expect("decodes");
    assert_eq!(pixels.len(), 50 * 50);
}

/// A JPEG whose `/DecodeParms` says how its colours were coded is left as it
/// is; the same JPEG without them is the control and is shrunk.
#[test]
fn a_jpeg_with_decode_parameters_is_left_as_it_is() {
    let jpeg = super::jpeg(&noise(400 * 400 * 3), 400, 400, 3, 90).expect("encodes");
    let with_parms = |parms: Option<Dictionary>| {
        let mut dict = image(400, 400, "DeviceRGB".into());
        dict.set("Filter", "DCTDecode");
        if let Some(parms) = parms {
            dict.set("DecodeParms", parms);
        }
        one_picture(dict, jpeg.clone())
    };
    let (mut doc, _) = with_parms(None);
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 1, "the control");

    let (mut doc, picture) = with_parms(Some(dictionary! { "ColorTransform" => 0 }));
    assert_eq!(apply(&mut doc, SCREEN).pictures_changed, 0);
    let after = stream(&doc, picture);
    assert_eq!(after.content, jpeg);
    assert_eq!(whole(&doc, picture, b"Width"), 400);
    assert!(after.dict.has(b"DecodeParms"), "its parameters are kept");
}

/// A picture that content this could not follow may draw is not scaled to
/// the size another page draws it at: that size is only the smallest known.
///
/// Three ways content is not followed: a page whose content does not decode,
/// a block whose stream does not, and a block drawn deeper than the walk
/// goes. The control is a page that draws nothing, where the picture is
/// scaled to the one size it is drawn at.
#[test]
fn a_picture_drawn_where_content_could_not_be_followed_is_not_scaled() {
    let raw = noise(400 * 400);
    let gray =
        || Stream::new(image(400, 400, "DeviceGray".into()), raw.clone()).with_compression(false);
    let small = "q 72 0 0 72 0 0 cm /Im Do Q";
    // An inline image with no end cannot be stepped over, so the whole stream
    // fails, the picture drawn across the page before it included.
    let undecodable = "q 600 0 0 600 0 0 cm /Im Do Q\nBI /W 1 /H 1 /BPC 8 /CS /G ID X\n";
    assert!(
        lopdf::content::Content::decode(undecodable.as_bytes()).is_err(),
        "the fixture"
    );

    // The control.
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(gray());
    pages_with(
        &mut doc,
        &[("", &[("Im", picture)]), (small, &[("Im", picture)])],
    );
    let done = apply(&mut doc, SCREEN);
    assert_eq!((done.pictures_changed, done.pictures_unsized), (1, 0));
    assert_eq!(whole(&doc, picture, b"Width"), 110, "the control is scaled");

    // A page whose content does not decode.
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(gray());
    pages_with(
        &mut doc,
        &[
            (undecodable, &[("Im", picture)]),
            (small, &[("Im", picture)]),
        ],
    );
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_unsized, 1, "a page that does not decode");
    assert_eq!(
        whole(&doc, picture, b"Width"),
        400,
        "a page that does not decode"
    );

    // A block whose stream does not decode, drawn across the whole page.
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(gray());
    let block = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "Filter" => "NoSuchDecode",
            "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im" => picture } },
        },
        b"/Im Do".to_vec(),
    ));
    assert!(
        stream(&doc, block)
            .decompressed_content_with_limit(1 << 20)
            .is_err(),
        "the fixture"
    );
    pages_with(
        &mut doc,
        &[
            ("q 600 0 0 600 0 0 cm /Blk Do Q", &[("Blk", block)]),
            (small, &[("Im", picture)]),
        ],
    );
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_unsized, 1, "a block that does not decode");
    assert_eq!(
        whole(&doc, picture, b"Width"),
        400,
        "a block that does not decode"
    );

    // A block that draws itself, deeper than the walk follows.
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(gray());
    let block = doc.new_object_id();
    doc.objects.insert(
        block,
        Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 1.into(), 1.into()],
                "Resources" => dictionary! {
                    "XObject" => dictionary! { "Im" => picture, "Blk" => block },
                },
            },
            b"/Blk Do".to_vec(),
        )
        .into(),
    );
    pages_with(
        &mut doc,
        &[
            ("q 600 0 0 600 0 0 cm /Blk Do Q", &[("Blk", block)]),
            (small, &[("Im", picture)]),
        ],
    );
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_unsized, 1, "a block too deep");
    assert_eq!(whole(&doc, picture, b"Width"), 400, "a block too deep");

    // More drawing than the walk follows, before a page that draws it large.
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(gray());
    let many = "/None Do\n".repeat(super::MAX_DRAWS + 1);
    pages_with(
        &mut doc,
        &[
            (small, &[("Im", picture)]),
            (&many, &[]),
            ("q 600 0 0 600 0 0 cm /Im Do Q", &[("Im", picture)]),
        ],
    );
    let done = apply(&mut doc, SCREEN);
    assert_eq!(done.pictures_unsized, 1, "more drawing than is followed");
    assert_eq!(
        whole(&doc, picture, b"Width"),
        400,
        "more drawing than is followed"
    );
}

#[test]
fn a_picture_whose_stated_size_overflows_is_compared_without_overflowing() {
    assert_eq!(super::pixel_loss((8, 6), (4, 3)), (36, 48));
    assert_eq!(
        super::pixel_loss((i64::MAX, 2), (4, 3)),
        (i64::MAX - 12, i64::MAX)
    );
    assert_eq!(
        super::pixel_loss((i64::MAX, 2), (i64::MAX, 2)),
        (0, i64::MAX)
    );
}

#[test]
fn a_picture_that_was_only_deflated_is_excluded_from_the_preview() {
    use super::estimate;
    let mut doc = Document::with_version("1.7");
    let picture = doc.add_object(Stream::new(
        image(i64::MAX, 1 << 40, "DeviceGray".into()),
        vec![0; 4096],
    ));
    page_with(
        &mut doc,
        "q 72 0 0 72 100 100 cm /Im Do Q",
        &[("Im", picture)],
    );
    // The stream is smaller, but its pixels did not change.
    let mut copy = doc.clone();
    assert_eq!(apply(&mut copy, SCREEN).pictures_changed, 0);
    assert_eq!(filter(&copy, picture), "FlateDecode", "the fixture");
    let estimated = estimate(&doc, SCREEN).expect("estimates");
    assert_eq!(estimated.estimate.done.pictures_changed, 0);
    assert!(estimated.preview_pages.is_empty());
}

/// A sample's stated size is held to its pixels before a picture is made from
/// it: a worker that says more than it sent is refused, not followed.
#[test]
fn a_sample_whose_size_is_not_its_pixels_is_not_drawn() {
    use super::{Sample, SAMPLE_SIDE};
    let of = |width: u32, height: u32, before: usize, after: usize| Sample {
        width,
        height,
        before: vec![7; before],
        after: vec![9; after],
        ..Sample::default()
    };
    let full = (SAMPLE_SIDE * SAMPLE_SIDE * 3) as usize;
    assert!(of(SAMPLE_SIDE, SAMPLE_SIDE, full, full)
        .side_by_side()
        .is_ok());
    let (across, down, pixels) = of(2, 1, 6, 6).side_by_side().expect("the control");
    assert_eq!((across, down, pixels.len()), (12, 1, 48));
    for (sample, says) in [
        (of(2, 1, 5, 6), "holds 5 bytes"),
        (of(2, 1, 6, 9), "sample after holds 9 bytes"),
        (of(0, 1, 0, 0), "0 by 1"),
        (of(SAMPLE_SIDE + 1, 1, 963, 963), "which no sample is"),
        (of(100_000, 100_000, 3, 3), "which no sample is"),
        (of(u32::MAX, u32::MAX, 3, 3), "which no sample is"),
    ] {
        let why = sample.side_by_side().expect_err(says);
        assert!(why.contains(says), "{why}");
        assert_eq!(sample.checked().expect_err(says), why);
    }
}
