use super::*;

const RGB: &[u8] = include_bytes!("../textedit/images/synthetic-rgb.jpg");
const GRAY: &[u8] = include_bytes!("../textedit/images/synthetic-gray.jpg");
const PROGRESSIVE: &[u8] = include_bytes!("../textedit/images/synthetic-progressive.jpg");
const CMYK: &[u8] = include_bytes!("../textedit/images/synthetic-cmyk.jpg");

/// `jpeg` with a segment put straight after the start-of-image marker.
fn with_segment(jpeg: &[u8], marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    out.extend_from_slice(payload);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// An EXIF payload whose only entry is the orientation.
fn exif(orientation: u16, little: bool) -> Vec<u8> {
    let two = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let four = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut out = b"Exif\0\0".to_vec();
    out.extend_from_slice(if little { b"II" } else { b"MM" });
    out.extend_from_slice(&two(42));
    out.extend_from_slice(&four(8));
    out.extend_from_slice(&two(1));
    out.extend_from_slice(&two(0x0112));
    out.extend_from_slice(&two(3));
    out.extend_from_slice(&four(1));
    out.extend_from_slice(&two(orientation));
    out.extend_from_slice(&two(0));
    out.extend_from_slice(&four(0));
    out
}

/// A PNG of `width` by `height` whose samples are `pixels`.
fn png_of(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    pixels: &[u8],
    dots_per_metre: Option<u32>,
) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if color == png::ColorType::Indexed {
            encoder.set_palette(vec![255, 0, 0, 0, 0, 255]);
        }
        if let Some(dots) = dots_per_metre {
            encoder.set_pixel_dims(Some(png::PixelDimensions {
                xppu: dots,
                yppu: dots,
                unit: png::Unit::Meter,
            }));
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
    }
    out
}

fn own() -> Options {
    Options::default()
}

/// Where the unit square's corner `(u, v)` lands on the page.
fn lands(at: &Placement, u: f64, v: f64) -> (f64, f64) {
    let [a, b, c, d, e, f] = at.matrix;
    (a * u + c * v + e, b * u + d * v + f)
}

#[test]
fn every_orientation_puts_the_stored_corners_where_exif_says() {
    // A stored picture 40 wide and 20 high. For each orientation: where the
    // stored top-left and top-right corners are shown, as fractions of the
    // page from its bottom-left. Written from the EXIF table (which side row 0
    // and column 0 are on), not from the matrices.
    type Corner = (f64, f64);
    let table: [(u8, Corner, Corner); 8] = [
        (1, (0.0, 1.0), (1.0, 1.0)),
        (2, (1.0, 1.0), (0.0, 1.0)),
        (3, (1.0, 0.0), (0.0, 0.0)),
        (4, (0.0, 0.0), (1.0, 0.0)),
        (5, (0.0, 1.0), (0.0, 0.0)),
        (6, (1.0, 1.0), (1.0, 0.0)),
        (7, (1.0, 0.0), (1.0, 1.0)),
        (8, (0.0, 0.0), (0.0, 1.0)),
    ];
    for (orientation, top_left, top_right) in table {
        let at = placement(40, 20, orientation, None, own());
        let page = if orientation >= 5 {
            (20.0, 40.0)
        } else {
            (40.0, 20.0)
        };
        assert_eq!(at.page, page, "orientation {orientation}");
        let on_page = |(x, y): (f64, f64)| (x * page.0, y * page.1);
        assert_eq!(
            lands(&at, 0.0, 1.0),
            on_page(top_left),
            "{orientation}: top-left"
        );
        assert_eq!(
            lands(&at, 1.0, 1.0),
            on_page(top_right),
            "{orientation}: top-right"
        );
    }
}

#[test]
fn a_page_is_the_pictures_size_at_the_resolution_its_file_states() {
    assert_eq!(placement(600, 300, 1, None, own()).page, (600.0, 300.0));
    assert_eq!(
        placement(600, 300, 1, Some((300.0, 300.0)), own()).page,
        (144.0, 72.0)
    );
    // Pixels that are not square keep the shape the file means.
    assert_eq!(
        placement(600, 300, 1, Some((300.0, 150.0)), own()).page,
        (144.0, 144.0)
    );
    // A resolution that was asked for wins over the file's.
    let asked = Options {
        dpi: Some(100),
        ..own()
    };
    assert_eq!(
        placement(600, 300, 1, Some((300.0, 300.0)), asked).page,
        (432.0, 216.0)
    );
}

#[test]
fn a_page_stays_within_what_a_page_may_be() {
    let wide = placement(30_000, 1000, 1, None, own());
    assert_eq!(wide.page, (14_400.0, 480.0));
    assert_eq!(lands(&wide, 1.0, 1.0), (14_400.0, 480.0));
    let tiny = placement(1, 2, 1, None, own());
    assert_eq!(tiny.page, (3.0, 6.0));
}

#[test]
fn paper_is_turned_to_the_picture_and_the_picture_is_never_enlarged() {
    let a4 = Options {
        paper: Paper::A4,
        dpi: None,
    };
    // Larger than the paper: scaled to fit, centred on the longer side.
    let portrait = placement(1000, 2000, 1, None, a4);
    assert_eq!(portrait.page, (595.0, 842.0));
    assert_eq!(lands(&portrait, 0.0, 0.0), (87.0, 0.0));
    assert_eq!(lands(&portrait, 1.0, 1.0), (508.0, 842.0));
    let landscape = placement(2000, 1000, 1, None, a4);
    assert_eq!(landscape.page, (842.0, 595.0));
    // A turned photograph is judged by the way it is shown.
    assert_eq!(placement(2000, 1000, 6, None, a4).page, (595.0, 842.0));
    // Smaller than the paper: its own size, in the middle.
    let small = placement(95, 42, 1, None, a4);
    assert_eq!(small.page, (842.0, 595.0));
    assert_eq!(lands(&small, 0.0, 0.0), (373.5, 276.5));
    assert_eq!(lands(&small, 1.0, 1.0), (468.5, 318.5));
    let letter = Options {
        paper: Paper::Letter,
        dpi: None,
    };
    assert_eq!(placement(100, 200, 1, None, letter).page, (612.0, 792.0));
}

#[test]
fn a_resolution_nobody_prints_at_is_refused() {
    for dpi in [0, 29, 2401] {
        let options = Options {
            dpi: Some(dpi),
            ..own()
        };
        assert!(options.checked().is_err(), "{dpi}");
        assert!(document(&[(RGB, "a.jpg")], options).is_err(), "{dpi}");
    }
    for dpi in [30, 2400] {
        let options = Options {
            dpi: Some(dpi),
            ..own()
        };
        assert!(options.checked().is_ok(), "{dpi}");
    }
}

#[test]
fn a_jpeg_goes_in_as_the_bytes_it_is() {
    for (bytes, gray) in [(RGB, false), (GRAY, true), (PROGRESSIVE, false)] {
        let found = picture(bytes).expect("a JPEG");
        assert!(found.jpeg);
        assert_eq!(found.gray, gray);
        assert_eq!(found.data, bytes);
        assert_eq!(found.orientation, 1);
        assert!(found.mask.is_none());
    }
}

#[test]
fn a_jpegs_orientation_is_read_in_either_byte_order() {
    for little in [true, false] {
        for orientation in 1..=8 {
            let turned = with_segment(RGB, 0xe1, &exif(orientation, little));
            assert_eq!(
                picture(&turned).expect("a JPEG").orientation,
                orientation as u8,
                "{orientation}, little-endian {little}"
            );
        }
        // A value outside the table is not an orientation.
        let odd = with_segment(RGB, 0xe1, &exif(9, little));
        assert_eq!(picture(&odd).expect("a JPEG").orientation, 1);
    }
    // A segment that only starts like EXIF is passed over, not read past.
    let cut = with_segment(RGB, 0xe1, b"Exif\0\0II*\0\xff\xff\xff\xff");
    assert_eq!(picture(&cut).expect("a JPEG").orientation, 1);
}

#[test]
fn a_jpegs_stated_resolution_is_used_when_it_is_plausible() {
    let jfif = |units: u8, x: u16, y: u16| {
        let mut payload = b"JFIF\0\x01\x02".to_vec();
        payload.push(units);
        payload.extend_from_slice(&x.to_be_bytes());
        payload.extend_from_slice(&y.to_be_bytes());
        payload.extend_from_slice(&[0, 0]);
        // Before the file's own JFIF segment, and the later one must not undo it.
        let mut out = RGB[..2].to_vec();
        out.extend_from_slice(&[0xff, 0xe0]);
        out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(&payload);
        // Skip the file's own APP0 if it has one.
        let rest = if RGB[2..4] == [0xff, 0xe0] {
            4 + usize::from(u16::from_be_bytes([RGB[4], RGB[5]]))
        } else {
            2
        };
        out.extend_from_slice(&RGB[rest..]);
        out
    };
    assert_eq!(
        picture(&jfif(1, 300, 300)).unwrap().dpi,
        Some((300.0, 300.0))
    );
    let per_cm = picture(&jfif(2, 100, 100)).unwrap().dpi.unwrap();
    assert!((per_cm.0 - 254.0).abs() < 1e-9, "{per_cm:?}");
    // No unit is an aspect ratio, and one dot an inch is a mistake.
    assert_eq!(picture(&jfif(0, 300, 300)).unwrap().dpi, None);
    assert_eq!(picture(&jfif(1, 1, 1)).unwrap().dpi, None);
}

#[test]
fn a_jpeg_tpdf_cannot_place_is_refused_in_words_about_it() {
    assert!(picture(CMYK).unwrap_err().contains("CMYK"));
    // Its tables and frame are whole and its picture data runs out: only
    // decoding it finds that. (The decoder forgives a missing end marker and
    // a few missing bytes, as readers do; forty is past what it forgives.)
    let cut = &RGB[..RGB.len() - 40];
    assert!(jpeg_facts(cut).is_ok(), "the control: the header reads");
    assert!(picture(cut).unwrap_err().contains("Exhausted data"));
    assert!(picture(&RGB[..12])
        .unwrap_err()
        .contains("not a complete JPEG"));
    assert!(picture(b"GIF89a")
        .unwrap_err()
        .contains("not a PNG or JPEG"));
}

#[test]
fn a_png_is_decoded_to_eight_bit_samples_whatever_it_was() {
    let rgb = png_of(
        2,
        1,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[1, 2, 3, 4, 5, 6],
        None,
    );
    let found = picture(&rgb).expect("a PNG");
    assert!(!found.jpeg && !found.gray && found.mask.is_none());
    assert_eq!((found.width, found.height), (2, 1));

    let inflate = |data: &[u8]| {
        use std::io::Read as _;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .unwrap();
        out
    };
    assert_eq!(inflate(&found.data), [1, 2, 3, 4, 5, 6]);

    // Sixteen bits a sample keep the high byte.
    let deep = png_of(
        1,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Sixteen,
        &[0xab, 0xcd],
        None,
    );
    let found = picture(&deep).expect("a PNG");
    assert!(found.gray);
    assert_eq!(inflate(&found.data), [0xab]);

    // A palette becomes the colours it names.
    let indexed = png_of(
        2,
        1,
        png::ColorType::Indexed,
        png::BitDepth::Eight,
        &[1, 0],
        None,
    );
    let found = picture(&indexed).expect("a PNG");
    assert_eq!(inflate(&found.data), [0, 0, 255, 255, 0, 0]);
}

#[test]
fn a_pngs_transparency_becomes_a_mask_only_when_something_shows_through() {
    let inflate = |data: &[u8]| {
        use std::io::Read as _;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .unwrap();
        out
    };
    let clear = png_of(
        2,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[1, 2, 3, 255, 4, 5, 6, 128],
        None,
    );
    let found = picture(&clear).expect("a PNG");
    assert_eq!(inflate(&found.data), [1, 2, 3, 4, 5, 6]);
    assert_eq!(inflate(found.mask.as_ref().expect("a mask")), [255, 128]);

    let opaque = png_of(
        2,
        1,
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Eight,
        &[9, 255, 8, 255],
        None,
    );
    let found = picture(&opaque).expect("a PNG");
    assert!(found.gray && found.mask.is_none());
    assert_eq!(inflate(&found.data), [9, 8]);
}

#[test]
fn a_pngs_stated_resolution_is_read_from_its_dots_per_metre() {
    // 11811 dots a metre is 300 DPI.
    let stated = png_of(
        1,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[0],
        Some(11_811),
    );
    let dpi = picture(&stated).unwrap().dpi.expect("a resolution");
    assert!((dpi.0 - 300.0).abs() < 0.01, "{dpi:?}");
    let silent = png_of(
        1,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[0],
        None,
    );
    assert_eq!(picture(&silent).unwrap().dpi, None);
}

#[test]
fn a_picture_too_large_is_refused_from_its_header() {
    // A header claiming more than the limit, with no pixels behind it: the
    // refusal must come before anything is allocated for them.
    let mut huge = png_of(
        1,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[0],
        None,
    );
    huge[16..20].copy_from_slice(&(MAX_SIDE + 1).to_be_bytes());
    assert!(picture(&huge).unwrap_err().contains("megapixels"));
    assert!(bounded(MAX_SIDE, 1).is_ok());
    assert!(bounded(8000, 5001).is_err());
    assert!(bounded(8000, 5000).is_ok());
    assert!(bounded(0, 5).unwrap_err().contains("no pixels"));
}

#[test]
fn a_document_has_one_page_for_each_picture_in_order() {
    let small = png_of(
        2,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[1, 2, 3, 255, 4, 5, 6, 0],
        None,
    );
    let turned = with_segment(RGB, 0xe1, &exif(6, true));
    let mut doc = document(
        &[
            (&small, "small.png"),
            (&turned, "turned.jpg"),
            (GRAY, "gray.jpg"),
        ],
        own(),
    )
    .expect("a document");
    let bytes = crate::save::serialise(&mut doc, "the document").expect("serialised");
    let back = Document::load_mem(&bytes).expect("read back");
    let pages: Vec<lopdf::ObjectId> = back.get_pages().into_values().collect();
    assert_eq!(pages.len(), 3);

    let media = |page: lopdf::ObjectId| -> Vec<f32> {
        back.get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_float().unwrap())
            .collect()
    };
    let image = |page: lopdf::ObjectId| -> &Stream {
        let resources = back
            .get_dictionary(page)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap();
        let named = resources.get(b"XObject").unwrap().as_dict().unwrap();
        let id = named.get(b"Im0").unwrap().as_reference().unwrap();
        back.get_object(id).unwrap().as_stream().unwrap()
    };
    // Two pixels by one, grown to the smallest page there is.
    assert_eq!(media(pages[0]), [0.0, 0.0, 6.0, 3.0]);
    assert!(image(pages[0]).dict.has(b"SMask"));

    // The photograph lying on its side is a page standing up, and its bytes
    // are the file's.
    let facts = jpeg_facts(RGB).unwrap();
    assert_eq!(
        media(pages[1]),
        [0.0, 0.0, facts.height as f32, facts.width as f32]
    );
    assert_eq!(image(pages[1]).content, turned);
    assert_eq!(
        image(pages[1])
            .dict
            .get(b"Filter")
            .unwrap()
            .as_name()
            .unwrap(),
        b"DCTDecode"
    );
    assert_eq!(
        image(pages[2])
            .dict
            .get(b"ColorSpace")
            .unwrap()
            .as_name()
            .unwrap(),
        b"DeviceGray"
    );
    assert_eq!(image(pages[2]).content, GRAY);
}

#[test]
fn a_picture_that_cannot_be_used_is_named_and_stops_the_document() {
    let why = document(&[(RGB, "good.jpg"), (b"not a picture", "notes.txt")], own()).unwrap_err();
    assert!(
        why.starts_with("notes.txt cannot be used: it is not a PNG or JPEG"),
        "{why}"
    );
    assert!(document(&[], own()).unwrap_err().contains("at least one"));
    let many: Vec<(&[u8], &str)> = vec![(RGB, "a.jpg"); MAX_IMAGES + 1];
    assert!(document(&many, own()).unwrap_err().contains("at most 500"));
}
