use super::*;

const RGB: &[u8] = include_bytes!("../textedit/images/synthetic-rgb.jpg");
const GRAY: &[u8] = include_bytes!("../textedit/images/synthetic-gray.jpg");
const PROGRESSIVE: &[u8] = include_bytes!("../textedit/images/synthetic-progressive.jpg");

fn encoded(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if color == png::ColorType::Indexed {
        encoder.set_palette(vec![255, 0, 0, 0, 0, 255]);
        encoder.set_trns(vec![255, 0]);
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    out
}

fn rgba(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let data: Vec<u8> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| pixel(x, y))
        .collect();
    encoded(
        width,
        height,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &data,
    )
}

#[test]
fn a_png_is_trimmed_to_what_it_shows_and_keeps_its_pixels() {
    // A 3 by 2 mark inside a transparent 10 by 8 image.
    let mark = |x: u32, y: u32| (4..7).contains(&x) && (3..5).contains(&y);
    let image = decode(&rgba(10, 8, |x, y| {
        if mark(x, y) {
            [10 * x as u8, 20 * y as u8, 7, 200]
        } else {
            [255, 255, 255, 0]
        }
    }))
    .unwrap();
    assert_eq!((image.width, image.height), (3, 2));
    assert_eq!(
        image.rgba,
        [
            [40, 60, 7, 200],
            [50, 60, 7, 200],
            [60, 60, 7, 200],
            [40, 80, 7, 200],
            [50, 80, 7, 200],
            [60, 80, 7, 200],
        ]
        .concat()
    );
    assert!(image.valid());
}

#[test]
fn a_large_image_is_scaled_to_fit_512_by_256_and_never_up() {
    for ((width, height), expected) in [
        ((1024, 256), (512, 128)),
        ((100, 600), (43, 256)),
        ((2000, 1500), (341, 256)),
        ((512, 256), (512, 256)),
        ((7, 5), (7, 5)),
    ] {
        let image = decode(&rgba(width, height, |_, _| [30, 60, 90, 255])).unwrap();
        assert_eq!((image.width, image.height), expected, "{width}x{height}");
        assert!(image.valid());
        assert!(image.rgba.chunks_exact(4).all(|p| p == [30, 60, 90, 255]));
    }
    // An average weighted by alpha: opaque red beside transparent blue is
    // half-transparent red, not purple.
    let image = decode(&rgba(1024, 2, |x, y| {
        if (x + y) % 2 == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 0]
        }
    }))
    .unwrap();
    assert_eq!((image.width, image.height), (512, 1));
    assert!(image.rgba.chunks_exact(4).all(|p| p == [255, 0, 0, 128]));
}

#[test]
fn every_png_colour_type_and_depth_is_read_as_straight_rgba() {
    use png::{
        BitDepth::{Eight, Sixteen},
        ColorType,
    };
    for (color, depth, data, expected) in [
        (
            ColorType::Rgb,
            Eight,
            vec![1, 2, 3, 4, 5, 6],
            vec![1, 2, 3, 255, 4, 5, 6, 255],
        ),
        (
            ColorType::Grayscale,
            Eight,
            vec![9, 200],
            vec![9, 9, 9, 255, 200, 200, 200, 255],
        ),
        (
            ColorType::GrayscaleAlpha,
            Eight,
            vec![9, 50, 200, 60],
            vec![9, 9, 9, 50, 200, 200, 200, 60],
        ),
        (
            ColorType::Indexed,
            Eight,
            vec![0, 0],
            vec![255, 0, 0, 255, 255, 0, 0, 255],
        ),
        (
            ColorType::Rgba,
            Sixteen,
            vec![
                0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xff, 0xff, 0, 1, 0, 2, 0, 3, 0x80, 0,
            ],
            vec![0x12, 0x56, 0x9a, 0xff, 0, 0, 0, 0x80],
        ),
    ] {
        let image = decode(&encoded(2, 1, color, depth, &data)).unwrap();
        assert_eq!(image.rgba, expected, "{color:?} {depth:?}");
    }
    // A palette's transparent entry is transparent, and trimmed away.
    let image = decode(&encoded(2, 1, png::ColorType::Indexed, Eight, &[1, 0])).unwrap();
    assert_eq!((image.width, image.rgba), (1, vec![255, 0, 0, 255]));
}

#[test]
fn a_jpeg_is_read_opaque_at_its_own_size() {
    for (name, bytes) in [("rgb", RGB), ("gray", GRAY), ("progressive", PROGRESSIVE)] {
        let (width, height) = dimensions(bytes).unwrap();
        let image = decode(bytes).unwrap();
        assert!(image.valid(), "{name}");
        assert!(image.rgba.chunks_exact(4).all(|p| p[3] == 255), "{name}");
        let scale = (512. / f64::from(width))
            .min(256. / f64::from(height))
            .min(1.);
        assert_eq!(
            (image.width, image.height),
            (
                (f64::from(width) * scale).round() as u32,
                (f64::from(height) * scale).round() as u32
            ),
            "{name}"
        );
    }
    // Not uniform: the pixels are the picture's.
    let image = decode(RGB).unwrap();
    assert!(image
        .rgba
        .chunks_exact(4)
        .any(|p| p[..3] != image.rgba[..3]));
}

#[test]
fn what_the_chooser_refuses_is_refused_before_decoding() {
    let good = rgba(4, 4, |_, _| [1, 2, 3, 255]);
    assert!(decode(&good).is_ok());
    let sized = |width: u32, height: u32| {
        let mut bytes = good.clone();
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    };
    let animated = {
        let mut bytes = good[..33].to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(b"acTL");
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&good[33..]);
        bytes
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("a PDF", b"%PDF-1.7\n%%EOF\n".to_vec()),
        ("too wide", sized(8193, 1)),
        ("too tall", sized(1, 8193)),
        ("too many pixels", sized(4096, 2049)),
        ("no width", sized(0, 4)),
        ("animated", animated),
        ("cut short", good[..good.len() - 5].to_vec()),
        ("without its end", good[..good.len() - 12].to_vec()),
        ("followed by more", [good.as_slice(), b"more"].concat()),
        ("a cut JPEG", RGB[..RGB.len() - 2].to_vec()),
        ("a JPEG followed by more", [RGB, b"more"].concat()),
        ("a JPEG with no scan", RGB[..20].to_vec()),
    ];
    for (name, bytes) in cases {
        assert_eq!(dimensions(&bytes).unwrap_err(), INVALID, "{name}");
        assert_eq!(decode(&bytes).unwrap_err(), INVALID, "{name}");
    }
    // The largest sizes that pass the header are still images to try.
    assert_eq!(dimensions(&sized(8192, 1)).unwrap(), (8192, 1));
    assert_eq!(dimensions(&sized(4096, 2048)).unwrap(), (4096, 2048));
    // The byte limit is the chooser's 10 MB: a sound PNG made that large by
    // a comment chunk is refused for its size alone. Control: one byte under.
    let commented = |total: usize| {
        let body = total - good.len() - 12;
        let mut bytes = good[..good.len() - 12].to_vec();
        bytes.extend_from_slice(&(body as u32).to_be_bytes());
        bytes.extend_from_slice(b"tEXt");
        bytes.resize(bytes.len() + body + 4, 0);
        bytes.extend_from_slice(&good[good.len() - 12..]);
        bytes
    };
    assert_eq!(dimensions(&commented(MAX_BYTES as usize)).unwrap(), (4, 4));
    assert_eq!(
        dimensions(&commented(MAX_BYTES as usize + 1)).unwrap_err(),
        INVALID
    );
}

#[test]
fn a_file_the_header_passes_and_the_decoder_does_not_is_refused() {
    // Damaged pixel data under a sound chunk grammar.
    let mut bytes = rgba(64, 64, |x, y| [x as u8, y as u8, 3, 255]);
    let data = bytes.windows(4).position(|w| w == b"IDAT").unwrap() + 4;
    for byte in &mut bytes[data + 2..data + 12] {
        *byte ^= 0x5a;
    }
    assert!(dimensions(&bytes).is_ok());
    assert_eq!(decode(&bytes).unwrap_err(), INVALID);
    // A header that states another size than the pixels have.
    let mut bytes = rgba(4, 4, |_, _| [1, 2, 3, 255]);
    bytes[19] = 5;
    assert_eq!(dimensions(&bytes).unwrap(), (5, 4));
    assert_eq!(decode(&bytes).unwrap_err(), INVALID);
    // Something to draw that scaling averages away: one faint pixel in a
    // large picture. The result would be a raster that shows nothing.
    let faint = rgba(2048, 1024, |x, y| [9, 9, 9, u8::from(x == 5 && y == 5)]);
    assert!(decode(&faint).is_ok());
    let faint = rgba(2048, 1024, |x, y| {
        [
            9,
            9,
            9,
            u8::from((x == 5 && y == 5) || (x == 2000 && y == 1000)),
        ]
    });
    assert_eq!(decode(&faint).unwrap_err(), CLEAR);
    // Nothing to draw.
    let clear = rgba(4, 4, |_, _| [9, 9, 9, 0]);
    assert!(decode(&clear)
        .unwrap_err()
        .contains("transparent everywhere"));
}
