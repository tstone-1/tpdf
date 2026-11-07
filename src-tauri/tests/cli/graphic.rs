//! *Copy a graphic* and *Save a graphic* through a sandboxed worker
//! (`graphic.rs`): the objects wholly inside a region, alone on a transparent
//! background, as PNG, as the picture stored, and as SVG.
//!
//! The page has what a word processor's export has around a logo: a white
//! rectangle under the whole page, a drawing placed as a Form XObject with a
//! matrix that stretches it, a picture with a mask, a JPEG, and a line of
//! text. The page-wide rectangle is the control for *alone*: it lies under
//! every region, and a graphic drawn with it would have no transparent pixel.
use super::{library_dir, scratch, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use tpdf_lib::graphic::{GraphicFormat, GraphicFound, GraphicRequest};
use tpdf_lib::worker::{Reply, Request, Worker};

const JPEG: &[u8] = include_bytes!("../../src/textedit/images/synthetic-rgb.jpg");

/// Page height, for turning page space into the tile's top-down pixels.
const HEIGHT: i32 = 842;

fn fixture(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica", "Encoding"=>"WinAnsiEncoding",
    });
    // A red block on the left half and a blue diagonal on the right, in a
    // 100 x 40 box the placement stretches to 200 x 40.
    let logo = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Form",
            "BBox"=>vec![0.into(), 0.into(), 100.into(), 40.into()],
            "Matrix"=>vec![2.into(), 0.into(), 0.into(), 1.into(), 0.into(), 0.into()],
        },
        b"1 0 0 rg 0 0 50 40 re f 0 0 1 RG 4 w 60 5 m 95 35 l S".to_vec(),
    ));
    // Four by two pixels: green, with the left half transparent.
    let mask = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>4, "Height"=>2,
            "ColorSpace"=>"DeviceGray", "BitsPerComponent"=>8,
        },
        vec![0, 0, 255, 255, 0, 0, 255, 255],
    ));
    let picture = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>4, "Height"=>2,
            "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8, "SMask"=>mask,
        },
        [0u8, 200, 0].repeat(8),
    ));
    let photo = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>8, "Height"=>8,
            "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8, "Filter"=>"DCTDecode",
        },
        JPEG.to_vec(),
    ));
    let content = b"1 1 1 rg 0 0 595 842 re f \
q 1 0 0 1 100 700 cm /Logo Do Q \
q 80 0 0 40 100 500 cm /Pic Do Q \
q 64 0 0 64 300 480 cm /Photo Do Q \
BT /F1 14 Tf 0 0 0 rg 100 300 Td (Words) Tj ET"
        .to_vec();
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    let page = doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()],
        "Contents"=>stream,
        "Resources"=>dictionary! {
            "Font"=>dictionary! {"F1"=>font},
            "XObject"=>dictionary! {"Logo"=>logo, "Pic"=>picture, "Photo"=>photo},
        },
    });
    doc.objects.insert(
        tree,
        dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

/// A region from page-space `left, bottom, right, top`, at one pixel a point.
fn region(left: i32, bottom: i32, right: i32, top: i32, format: GraphicFormat) -> GraphicRequest {
    GraphicRequest {
        page: 0,
        crop: None,
        turns: 0,
        scale: 1.0,
        rect: [left, HEIGHT - top, right - left, top - bottom],
        format,
    }
}

fn export(worker: &mut Worker, request: GraphicRequest) -> Result<(Vec<u8>, GraphicFound), String> {
    let response = worker.call(&Request::Graphic(request))?;
    match response.reply {
        Some(Reply::Graphic(found)) if response.ok => {
            Ok((worker.tile.as_slice()[..response.bytes].to_vec(), found))
        }
        _ => Err(response.error),
    }
}

/// A PNG's size and RGBA pixels.
fn decoded(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut pixels).ok()?;
    pixels.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some((info.width, info.height, pixels))
}

pub(super) fn exports_what_the_region_holds(report: &mut Report) {
    let dir = scratch("graphic");
    let source = dir.join("graphic.pdf");
    fixture(&source);
    let mut worker = match Worker::spawn(&source, &library_dir()) {
        Ok(worker) => worker,
        Err(why) => return report.check("a worker opens the graphic fixture", false, &why),
    };
    let _ = worker.call(&Request::Open {
        lazy_geometry: false,
    });

    // The drawing: 200 x 40 points at 100, 700, plus its stroke's half width.
    let around_logo = |format| region(90, 690, 320, 750, format);
    let found = export(&mut worker, around_logo(GraphicFormat::Describe));
    report.check(
        "a region around a drawing finds one object, with no stored picture and an SVG possible",
        found.as_ref().is_ok_and(|(bytes, found)| {
            bytes.is_empty()
                && found.objects == 1
                && found.original.is_none()
                && found.no_svg.is_none()
        }),
        &format!("{found:?}"),
    );

    let png = export(&mut worker, around_logo(GraphicFormat::Png));
    let pixels = png.as_ref().ok().and_then(|(bytes, _)| decoded(bytes));
    let counts = pixels.as_ref().map(|(w, h, rgba)| {
        let clear = rgba.chunks(4).filter(|p| p[3] == 0).count();
        let red = rgba
            .chunks(4)
            .filter(|p| p[3] == 255 && p[0] > 200 && p[1] < 50 && p[2] < 50)
            .count();
        let blue = rgba
            .chunks(4)
            .filter(|p| p[3] > 200 && p[2] > 200 && p[0] < 50)
            .count();
        let white = rgba
            .chunks(4)
            .filter(|p| p[3] == 255 && p[0] > 250 && p[1] > 250 && p[2] > 250)
            .count();
        (*w, *h, clear, red, blue, white)
    });
    report.check(
        "the drawing's PNG is the drawing alone: red and blue on a transparent background, no page white",
        counts.is_some_and(|(_, _, clear, red, blue, white)| clear > 0 && red > 0 && blue > 0 && white == 0),
        &format!("(width, height, clear, red, blue, white) = {counts:?}"),
    );
    // 600 dpi: 200 points plus up to 4 of stroke is about 1667 to 1700 pixels.
    report.check(
        "the drawing is rendered at 600 dpi, cropped to its own bounds",
        counts.is_some_and(|(w, h, ..)| (1660..=1710).contains(&w) && (330..=350).contains(&h)),
        &format!("{counts:?}, found {:?}", png.as_ref().map(|(_, f)| f)),
    );

    let svg = export(&mut worker, around_logo(GraphicFormat::Svg))
        .map(|(bytes, found)| (String::from_utf8_lossy(&bytes).into_owned(), found));
    let text = svg
        .as_ref()
        .map(|(text, _)| text.as_str())
        .unwrap_or_default();
    report.check(
        "the drawing's SVG has its two paths, their paint, and the form's stretch",
        svg.as_ref()
            .is_ok_and(|(_, found)| found.extension == "svg")
            && text.matches("<path ").count() == 2
            && text.contains("fill=\"#ff0000\"")
            && text.contains("stroke=\"#0000ff\" stroke-width=\"4\"")
            && text.contains("matrix(2 0 0 1 100 700)"),
        &format!("{svg:?}"),
    );

    // The masked picture: 80 x 40 points at 100, 500.
    let around_picture = |format| region(95, 495, 185, 545, format);
    let picture = export(&mut worker, around_picture(GraphicFormat::Png));
    let pixels = picture.as_ref().ok().and_then(|(bytes, _)| decoded(bytes));
    report.check(
        "a picture alone is saved at its own 4 x 2 pixels, its mask as transparency",
        picture
            .as_ref()
            .is_ok_and(|(_, found)| found.original.as_deref() == Some("png"))
            && pixels.as_ref().is_some_and(|(w, h, rgba)| {
                *w == 4 && *h == 2 && rgba[3] == 0 && rgba[12..16] == [0, 200, 0, 255]
            }),
        &format!("{:?} {pixels:?}", picture.as_ref().map(|(_, f)| f)),
    );

    // The JPEG: 64 points square at 300, 480.
    let around_photo = |format| region(295, 475, 370, 550, format);
    let photo = export(&mut worker, around_photo(GraphicFormat::Original));
    report.check(
        "a JPEG is saved as the bytes the document stores",
        photo.as_ref().is_ok_and(|(bytes, found)| {
            bytes == JPEG && found.extension == "jpg" && (found.width, found.height) == (8, 8)
        }),
        &format!("{:?}", photo.as_ref().map(|(b, f)| (b.len(), f))),
    );

    let words = |format| region(95, 290, 160, 320, format);
    let described = export(&mut worker, words(GraphicFormat::Describe));
    let refused = export(&mut worker, words(GraphicFormat::Svg));
    report.check(
        "text is not drawn as SVG: the description says why, and an SVG is refused",
        described.as_ref().is_ok_and(|(_, found)| {
            found
                .no_svg
                .as_deref()
                .is_some_and(|why| why.contains("text"))
        }) && refused.as_ref().is_err_and(|why| why.contains("text")),
        &format!("{described:?} / {refused:?}"),
    );

    let empty = export(&mut worker, region(400, 100, 450, 150, GraphicFormat::Png));
    report.check(
        "a region with nothing wholly inside it is refused, and says what to do",
        empty
            .as_ref()
            .is_err_and(|why| why.contains("drag around all of it")),
        &format!("{empty:?}"),
    );

    let page = export(&mut worker, region(0, 0, 595, 842, GraphicFormat::Describe));
    report.check(
        "a region around the whole page takes all five objects, the page white included",
        page.as_ref()
            .is_ok_and(|(_, found)| found.objects == 5 && found.original.is_none()),
        &format!("{page:?}"),
    );
}
