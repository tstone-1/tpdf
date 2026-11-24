//! *Copy a graphic* and *Save a graphic* through a sandboxed worker
//! (`graphic.rs`): the objects wholly inside a region, alone on a transparent
//! background, as PNG, as the picture stored, and as SVG.
//!
//! The page has what a word processor's export has around a logo: a white
//! rectangle under the whole page, a drawing placed as a Form XObject with a
//! matrix that stretches it, a picture with a mask, a JPEG, and a line of
//! text. The page-wide rectangle is the control for *alone*: it lies under
//! every region, and a graphic drawn with it would have no transparent pixel.
use super::{library_dir, scratch, wait, Report};
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
    // Above the SVG raster limit, even though the page placement is small.
    let large = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>2048, "Height"=>2048,
            "ColorSpace"=>"DeviceGray", "BitsPerComponent"=>1,
        },
        vec![0xaa; 2048 * 256],
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
q 64 0 0 64 400 480 cm /Large Do Q q 64 0 0 64 300 480 cm /Photo Do Q \
BT /F1 14 Tf 0 0 0 rg 100 300 Td (Words) Tj ET \
q 30 180 m 70 180 l 30 220 l h W n 0 g 30 180 40 40 re f Q \
q 0 G 0 w 100 180 m 140 220 l S Q \
q 2 M 2 w 180 180 m 195 218 l 200 180 l S Q \
q /Blend gs 0 0 1 rg 246 182 24 28 re f Q"
        .to_vec();
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    let page = doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![0.into(),0.into(),595.into(),842.into()],
        "Contents"=>stream,
        "Resources"=>dictionary! {
            "Font"=>dictionary! {"F1"=>font},
            "Pattern"=>dictionary! {},
            "ExtGState"=>dictionary! {"Blend"=>dictionary! {"BM"=>"Multiply"}},
            "XObject"=>dictionary! {"Logo"=>logo, "Pic"=>picture, "Photo"=>photo, "Large"=>large},
        },
    });
    let tiles = doc.add_object(Stream::new(
        dictionary! {
            "Type"=>"Pattern", "PatternType"=>1, "PaintType"=>1, "TilingType"=>1,
            "BBox"=>vec![Object::from(0), 0.into(), 8.into(), 8.into()],
            "XStep"=>8, "YStep"=>8, "Resources"=>dictionary! {},
        },
        b"1 0 0 rg 0 0 8 8 re f 0 0 1 rg 0 0 4 8 re f".to_vec(),
    ));
    let stripes = doc.add_object(Stream::new(
        dictionary! {},
        b"/Pattern cs /Tiles scn 30 30 100 100 re f".to_vec(),
    ));
    let patterned_page = doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![Object::from(0), 0.into(), 595.into(), 842.into()],
        "Contents"=>stripes, "Resources"=>dictionary! {"Pattern"=>dictionary! {"Tiles"=>tiles}},
    });
    let mut kids = vec![Object::Reference(page), Object::Reference(patterned_page)];
    // Equal physical rectangles, one in half as many native units.
    for (unit, content) in [(1, "0 g 10 10 144 72 re f"), (2, "0 g 10 10 72 36 re f")] {
        let drawing = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![Object::from(0),0.into(),595.into(),842.into()],
            "UserUnit"=>unit, "Contents"=>drawing, "Resources"=>dictionary! {},
        })));
    }
    doc.objects.insert(
        tree,
        dictionary! {"Type"=>"Pages", "Kids"=>kids, "Count"=>4}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>tree});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

/// A region from page-space `left, bottom, right, top`, at one pixel a point.
/// A click at page-space `x, y`, at one pixel a point.
fn click(x: i32, y: i32) -> GraphicRequest {
    region(x, y, x + 1, y + 1, GraphicFormat::Hit)
}

fn region(left: i32, bottom: i32, right: i32, top: i32, format: GraphicFormat) -> GraphicRequest {
    GraphicRequest {
        page: 0,
        crop: None,
        turns: 0,
        scale: 1.0,
        rect: [left, HEIGHT - top, right - left, top - bottom],
        format,
        object: None,
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

/// Compare the worker's pending revision with the same edit saved and reopened.
fn pending_caption(dir: &Path) -> Result<(), String> {
    use tpdf_lib::textedit;
    let source = dir.join("caption.pdf");
    fixture(&source);
    let mut doc = Document::load(&source).map_err(|e| e.to_string())?;
    let page = doc.get_pages()[&1];
    doc.change_page_content(
        page,
        b"BT /F1 14 Tf 1 0 0 1 100 300 Tm (Words) Tj ET".to_vec(),
    )
    .map_err(|e| e.to_string())?;
    doc.save(&source).map_err(|e| e.to_string())?;
    let runs = textedit::scan(&doc, 0)?;
    let run = runs.runs.first().ok_or("no editable caption")?;
    let change = textedit::Change {
        page: 0,
        revision: runs.revision,
        operator: run.operator,
        original: run.text.clone(),
        replacement: "Other".into(),
        layout: None,
    };
    use tpdf_lib::{
        edits::Edits,
        render::{Backend, RenderService},
    };
    let service = RenderService::start_with(library_dir(), Backend::Worker);
    let edits = Edits::default();
    service.follow_text_edits(&edits);
    let open = |path: &Path| {
        wait(|reply| service.open(path.to_owned(), false, None, reply))
            .map(|info| info.id)
            .map_err(|why| why.reason)
    };
    let request = region(90, 270, 220, 330, GraphicFormat::Png);
    let png = |id| wait(|reply| service.graphic(id, request, reply)).map(|(bytes, _)| bytes);
    let id = open(&source)?;
    edits.open(id, 1, None);
    let original = png(id)?;
    edits.replace_text(id, 1, change.clone())?;
    let pending = png(id)?;
    textedit::write(&mut doc, &[change])?;
    let saved = dir.join("saved-caption.pdf");
    doc.save(&saved).map_err(|e| e.to_string())?;
    let reopened = png(open(&saved)?)?;
    let pixels = decoded(&pending).ok_or("pending caption had no pixels")?;
    if Some(&pixels) != decoded(&reopened).as_ref() || Some(&pixels) == decoded(&original).as_ref()
    {
        return Err(
            "pending caption differs from the saved revision or still shows the original".into(),
        );
    }
    Ok(())
}

/// Read the PNG back so sizing, painting and layer visibility are checked together.
fn layered_and_rotated(dir: &Path, angle: i64, visible: bool) -> Result<(), String> {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let layer = doc
        .add_object(dictionary! { "Type"=>"OCG", "Name"=>Object::string_literal("Alternative") });
    let body = doc.add_object(Stream::new(
        dictionary! {},
        b"0 0 1 rg 20 30 120 24 re f /OC /Alt BDC 1 0 0 rg 20 30 120 24 re f EMC".to_vec(),
    ));
    let page = doc.add_object(
        dictionary! { "Type"=>"Page", "Parent"=>tree, "Rotate"=>angle,
        "MediaBox"=>vec![Object::from(0),0.into(),200.into(),200.into()], "Contents"=>body,
        "Resources"=>dictionary! { "Properties"=>dictionary! { "Alt"=>layer } } },
    );
    doc.objects.insert(
        tree,
        dictionary! { "Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1 }.into(),
    );
    let mut default = dictionary! {};
    default.set(
        if visible { "ON" } else { "OFF" },
        vec![Object::Reference(layer)],
    );
    let root = doc.add_object(dictionary! { "Type"=>"Catalog", "Pages"=>tree,
    "OCProperties"=>dictionary! { "OCGs"=>vec![Object::Reference(layer)], "D"=>default } });
    doc.trailer.set("Root", root);
    let path = dir.join(format!("layer-{angle}-{visible}.pdf"));
    doc.save(&path).map_err(|e| e.to_string())?;
    let mut worker = Worker::spawn(&path, &library_dir())?;
    worker.call(&Request::Open {
        lazy_geometry: false,
    })?;
    let request = |format| GraphicRequest {
        rect: [0, 0, 200, 200],
        ..region(0, 0, 1, 1, format)
    };
    let (bytes, found) = export(&mut worker, request(GraphicFormat::Png))?;
    let (w, h, pixels) = decoded(&bytes).ok_or("invalid image")?;
    let size = if angle % 180 == 0 {
        (1000, 200)
    } else {
        (200, 1000)
    };
    let color = if visible {
        [255, 0, 0, 255]
    } else {
        [0, 0, 255, 255]
    };
    if (w, h) != size
        || (found.width, found.height) != size
        || pixels.chunks_exact(4).any(|px| px != color)
    {
        return Err(format!(
            "wrong dimensions or layer pixels for {angle} degrees"
        ));
    }
    if found.no_svg.is_none() || export(&mut worker, request(GraphicFormat::Svg)).is_ok() {
        return Err("offered raw SVG despite optional content".into());
    }
    let (x, y) = match angle {
        0 => (80, 158),
        90 => (42, 80),
        180 => (120, 42),
        _ => (158, 120),
    };
    let (_, hit) = export(
        &mut worker,
        GraphicRequest {
            rect: [x, y, 1, 1],
            ..request(GraphicFormat::Hit)
        },
    )?;
    if hit.object != Some(u32::from(visible)) {
        return Err(format!(
            "selected {:?} with layer visibility {visible}",
            hit.object
        ));
    }
    let (picked, _) = export(
        &mut worker,
        GraphicRequest {
            object: hit.object,
            ..request(GraphicFormat::Png)
        },
    )?;
    if decoded(&picked)
        .ok_or("invalid selected PNG")?
        .2
        .chunks_exact(4)
        .any(|px| px != color)
    {
        return Err("clicked graphic differed from the visible drawing".into());
    }
    // The underlying blue path has no optional-content mark. Selecting it
    // alone must still offer vector export on this very same layered page.
    let (svg, _) = export(
        &mut worker,
        GraphicRequest {
            object: Some(0),
            ..request(GraphicFormat::Svg)
        },
    )?;
    let (view_box, transform) = match angle {
        0 => ("0 0 120 24", "1 0 0 -1 -20 54"),
        90 => ("0 0 24 120", "0 1 1 0 -30 -20"),
        180 => ("0 0 120 24", "-1 0 0 1 140 -30"),
        _ => ("0 0 24 120", "0 -1 -1 0 54 140"),
    };
    let svg = String::from_utf8_lossy(&svg);
    if !svg.contains(&format!("viewBox=\"{view_box}\""))
        || !svg.contains(&format!("matrix({transform})"))
    {
        return Err(format!("wrong SVG orientation for {angle} degrees"));
    }
    if !svg.contains("#0000ff") {
        return Err("unmarked vector selection lost its blue drawing".into());
    }
    Ok(())
}

pub(super) fn exports_what_the_region_holds(report: &mut Report) {
    let dir = scratch("graphic");
    for angle in [0, 90, 180, 270] {
        for visible in [true, false] {
            let result = layered_and_rotated(&dir, angle, visible);
            report.check(
                "the rotated graphic preserves every visible pixel and refuses layered SVG",
                result.is_ok(),
                &format!("{result:?}"),
            );
        }
    }
    let caption = pending_caption(&dir);
    report.check(
        "export draws the accepted caption before saving",
        caption.is_ok(),
        &format!("{caption:?}"),
    );
    let source = dir.join("graphic.pdf");
    fixture(&source);
    let mut worker = match Worker::spawn(&source, &library_dir()) {
        Ok(worker) => worker,
        Err(why) => return report.check("a worker opens the graphic fixture", false, &why),
    };
    let _ = worker.call(&Request::Open {
        lazy_geometry: false,
    });

    for (page, viewbox) in [(2, "144 72"), (3, "72 36")] {
        let request = |format| GraphicRequest {
            page,
            ..region(5, 5, 160, 90, format)
        };
        let png = export(&mut worker, request(GraphicFormat::Png))
            .and_then(|(bytes, _)| decoded(&bytes).ok_or("invalid PNG".into()));
        let svg = export(&mut worker, request(GraphicFormat::Svg))
            .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned());
        report.check(
            "equal physical drawings export at 600 dpi, with a native SVG viewBox",
            png.as_ref().is_ok_and(|(w, h, rgba)| {
                (*w, *h) == (1200, 600)
                    && rgba.chunks_exact(4).filter(|p| p[3] == 255).count() > 700_000
            }) && svg.as_ref().is_ok_and(|s| {
                s.contains(&format!(
                    "width=\"144pt\" height=\"72pt\" viewBox=\"0 0 {viewbox}\""
                ))
            }),
            &format!(
                "page {page}: PNG {:?}, SVG {svg:?}",
                png.as_ref().map(|(w, h, _)| (w, h))
            ),
        );
    }
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
            && text.contains("<clipPath ")
            && text.contains("clip-path=")
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

    let large = export(&mut worker, region(395, 475, 470, 550, GraphicFormat::Svg));
    let embedded = large.as_ref().ok().and_then(|(bytes, _)| {
        use base64::Engine as _;
        let (_, data) = std::str::from_utf8(bytes)
            .ok()?
            .split_once("data:image/png;base64,")?;
        let (data, _) = data.split_once('"')?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?;
        let reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .ok()?;
        Some(reader.info().size())
    });
    report.check(
        "an SVG embeds the 2048-square picture at the 3.5-million-pixel limit",
        embedded == Some((1870, 1870)),
        &format!("embedded PNG size {embedded:?}"),
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

    for (left, reason) in [
        (25, "clipping path"),
        (95, "stroke"),
        (167, "stroke"),
        (235, "blend mode"),
    ] {
        let area = |format| region(left, 175, left + 50, 225, format);
        let svg = export(&mut worker, area(GraphicFormat::Svg));
        let png = export(&mut worker, area(GraphicFormat::Png))
            .ok()
            .and_then(|(bytes, _)| decoded(&bytes));
        report.check(
            reason,
            svg.is_err_and(|why| why.contains(reason))
                && png.is_some_and(|(_, _, pixels)| pixels.chunks_exact(4).any(|rgba| rgba[3] > 0)),
            "SVG must refuse the operation and PNG must paint it",
        );
    }

    let stripes = |format| GraphicRequest {
        page: 1,
        ..region(20, 20, 140, 140, format)
    };
    let description = export(&mut worker, stripes(GraphicFormat::Describe));
    let svg = export(&mut worker, stripes(GraphicFormat::Svg));
    let pixels = export(&mut worker, stripes(GraphicFormat::Png))
        .ok()
        .filter(|(_, found)| {
            found
                .no_svg
                .as_ref()
                .is_some_and(|why| why.contains("pattern"))
        })
        .and_then(|(bytes, _)| decoded(&bytes));
    report.check(
        "a tiled path offers PNG with both stripe colors",
        description.is_ok_and(|(_, found)| found.no_svg.is_some_and(|why| why.contains("pattern")))
            && svg.is_err_and(|why| why.contains("pattern"))
            && pixels.is_some_and(|(_, _, rgba)| {
                [[255, 0, 0, 255], [0, 0, 255, 255]]
                    .iter()
                    .all(|color| rgba.chunks_exact(4).any(|pixel| pixel == color))
            }),
        "both colors must survive and SVG must be refused",
    );

    let empty = export(&mut worker, region(400, 100, 450, 150, GraphicFormat::Png));
    report.check(
        "a region with nothing wholly inside it is refused, and says what to do",
        empty
            .as_ref()
            .is_err_and(|why| why.contains("drag around all of it")),
        &format!("{empty:?}"),
    );

    // A click on the drawing: the page's objects are the white, the drawing,
    // the picture, the JPEG and the text, so the drawing is index 1.
    let clicked = export(&mut worker, click(150, 720));
    let rect = clicked.as_ref().ok().and_then(|(_, found)| found.rect);
    report.check(
        "a click on the drawing selects it and answers its rectangle in the tile's pixels",
        clicked
            .as_ref()
            .is_ok_and(|(bytes, found)| bytes.is_empty() && found.object == Some(1))
            && rect.is_some_and(|[l, t, r, b]| {
                // 100..300 by 700..740 points, plus up to 2 of stroke, y down from 842.
                (97.0..=101.0).contains(&l)
                    && (99.0..=103.0).contains(&t)
                    && (299.0..=303.0).contains(&r)
                    && (141.0..=145.0).contains(&b)
            }),
        &format!("{clicked:?}"),
    );
    let mut chosen = region(0, 0, 1, 1, GraphicFormat::Png);
    chosen.object = Some(1);
    let by_index = export(&mut worker, chosen)
        .ok()
        .and_then(|(bytes, _)| decoded(&bytes));
    let by_region = png.as_ref().ok().and_then(|(bytes, _)| decoded(bytes));
    report.check(
        "the clicked object exported by its index is the drawing the region gave",
        by_index.is_some() && by_index == by_region,
        &format!(
            "{:?} / {:?}",
            by_index.as_ref().map(|p| (p.0, p.1)),
            by_region.as_ref().map(|p| (p.0, p.1))
        ),
    );
    let margin = export(&mut worker, click(500, 100));
    let words = export(&mut worker, click(110, 305));
    report.check(
        "a click on the margin or on text selects nothing, though the page white lies under both",
        margin
            .as_ref()
            .is_ok_and(|(_, found)| found.object.is_none() && found.objects == 0)
            && words
                .as_ref()
                .is_ok_and(|(_, found)| found.object.is_none()),
        &format!("{margin:?} / {words:?}"),
    );
    let mut gone = region(0, 0, 1, 1, GraphicFormat::Png);
    gone.object = Some(99);
    let gone = export(&mut worker, gone);
    report.check(
        "an index the page does not have is refused",
        gone.as_ref()
            .is_err_and(|why| why.contains("no longer on the page")),
        &format!("{gone:?}"),
    );

    let page = export(&mut worker, region(0, 0, 595, 842, GraphicFormat::Describe));
    report.check(
        "a region around the whole page takes all ten objects, the page white included",
        page.as_ref()
            .is_ok_and(|(_, found)| found.objects == 10 && found.original.is_none()),
        &format!("{page:?}"),
    );
}
