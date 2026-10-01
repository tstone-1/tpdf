//! `sign --text` and `--date-format`: the lines drawn are the command line's
//! own, read back from the signed document's appearance stream. And `--reason`,
//! `--location` and `--contact` on an invisible signature, read back from its
//! signature dictionary.
use super::{certificate, now, plain_pdf, scratch, signs, strings, Report, TestStore, SUBJECT};
use std::path::Path;

/// The strings the signature's appearance shows, in order, as Latin-1.
fn lines(path: &Path) -> Vec<String> {
    let doc = lopdf::Document::load(path).expect("signed document");
    let mut out = Vec::new();
    for object in doc.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        let form = stream
            .dict
            .get(b"Subtype")
            .and_then(lopdf::Object::as_name)
            .is_ok_and(|name| name == b"Form");
        if !form {
            continue;
        }
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        for line in String::from_utf8_lossy(&content).lines() {
            let Some(hex) = line.strip_suffix("> Tj").and_then(|l| l.strip_prefix('<')) else {
                continue;
            };
            out.push(
                (0..hex.len() / 2)
                    .map(|at| {
                        u8::from_str_radix(&hex[at * 2..at * 2 + 2], 16).expect("hex") as char
                    })
                    .collect(),
            );
        }
    }
    out
}

/// The `(x, y)` of every text matrix in the appearance, in the form's space.
fn text_matrices(path: &Path) -> Vec<(f64, f64)> {
    let doc = lopdf::Document::load(path).expect("signed document");
    let mut out = Vec::new();
    for object in doc.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        if !stream
            .dict
            .get(b"Subtype")
            .and_then(lopdf::Object::as_name)
            .is_ok_and(|name| name == b"Form")
        {
            continue;
        }
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        for line in String::from_utf8_lossy(&content).lines() {
            let Some(numbers) = line.strip_suffix(" Tm") else {
                continue;
            };
            let numbers: Vec<f64> = numbers
                .split(' ')
                .map(|n| n.parse().expect("a number"))
                .collect();
            assert_eq!(&numbers[..4], [1.0, 0.0, 0.0, 1.0], "an upright page");
            out.push((numbers[4], numbers[5]));
        }
    }
    out
}

/// The image's `[width, height, x, y]` in the form's space: its matrix on an
/// upright page, where the unit square is scaled and moved and not turned.
fn image_matrix(path: &Path) -> Option<[f64; 4]> {
    let doc = lopdf::Document::load(path).expect("signed document");
    for object in doc.objects.values() {
        let Ok(stream) = object.as_stream() else {
            continue;
        };
        let content = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        for line in String::from_utf8_lossy(&content).lines() {
            let Some(numbers) = line
                .strip_suffix(" cm /Signature Do Q")
                .and_then(|l| l.strip_prefix("q "))
            else {
                continue;
            };
            let n: Vec<f64> = numbers
                .split(' ')
                .map(|n| n.parse().expect("a number"))
                .collect();
            assert!(n[1].abs() < 1e-9 && n[2].abs() < 1e-9, "an upright page");
            return Some([n[0], n[3], n[4], n[5]]);
        }
    }
    None
}

/// The signature dictionary's text entries, `key` by `key`, as Latin-1.
fn noted(path: &Path, keys: &[&[u8]]) -> Vec<Option<String>> {
    let doc = lopdf::Document::load(path).expect("signed document");
    let signature = doc
        .objects
        .values()
        .filter_map(|object| object.as_dict().ok())
        .find(|dict| dict.get(b"ByteRange").is_ok())
        .expect("a signature dictionary");
    keys.iter()
        .map(|key| match signature.get(key) {
            Ok(lopdf::Object::String(bytes, _)) => Some(bytes.iter().map(|b| *b as char).collect()),
            _ => None,
        })
        .collect()
}

pub fn draws_the_text(report: &mut Report) {
    let now = now();
    let dir = scratch("sign-text");
    let s = |p: &Path| p.display().to_string();
    let plain = dir.join("plain.pdf");
    std::fs::write(&plain, plain_pdf()).expect("input");
    let store = TestStore {
        certificate: certificate(now),
        misdirected: false,
    };
    let line = |out: &Path, extra: &[&str]| {
        let mut args = strings(&[
            "sign",
            &s(&plain),
            "-o",
            &s(out),
            "--identity",
            SUBJECT,
            "--visible",
            "--no-image",
            "--rect",
            "40,40,300,120",
        ]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    };

    // Control: without --text the three standard lines.
    let standard = dir.join("standard.pdf");
    let (code, _, stderr) = signs(&line(&standard, &[]), &store, now);
    let drawn = lines(&standard);
    report.check(
        "control: without --text the standard lines are drawn",
        code == 0
            && drawn.len() == 3
            && drawn[0] == "Digitally signed by"
            && drawn[1] == SUBJECT
            && drawn[2].starts_with("Date: ")
            && drawn[2].ends_with(" UTC"),
        &format!("exit {code}, {drawn:?}: {stderr}"),
    );

    // The text, the date in the format given, and a reason written and drawn
    // only where the text puts it.
    let worded = dir.join("worded.pdf");
    let (code, _, stderr) = signs(
        &line(
            &worded,
            &[
                "--text",
                "Digitally signed\\n{date}",
                "--text",
                "{name}: {reason}",
                "--date-format",
                "at YYYY",
                "--reason",
                "Approved",
            ],
        ),
        &store,
        now,
    );
    let drawn = lines(&worded);
    let year = drawn
        .get(1)
        .and_then(|l| l.strip_prefix("at "))
        .is_some_and(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()));
    report.check(
        "--text draws its own lines, with the name, the reason and the date as formatted",
        code == 0
            && drawn.len() == 3
            && drawn[0] == "Digitally signed"
            && year
            && drawn[2] == format!("{SUBJECT}: Approved"),
        &format!("exit {code}, {drawn:?}: {stderr}"),
    );

    // The report says where the lines were drawn: each baseline and left edge
    // against the text matrix in the signed file's own appearance stream.
    let reported = dir.join("reported.pdf");
    let mut args = line(&reported, &["--text", "First\\nSecond line"]);
    args.push("--json".into());
    let (code, stdout, stderr) = signs(&args, &store, now);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let appearance = &json["appearance"];
    let number = |v: &serde_json::Value| v.as_f64().unwrap_or(f64::NAN);
    // `--rect 40,40,300,120`: the form's box is 300 by 120 with its origin at
    // the rectangle's bottom-left corner, so a form point (x, y) is at
    // (40 + x, 40 + 120 - y) from the page's top-left corner.
    let matrices = text_matrices(&reported);
    let bearing = number(&appearance["font_size"]) * 0.1;
    let agree = appearance["lines"].as_array().is_some_and(|lines| {
        lines.len() == 2
            && matrices.len() == 2
            && lines.iter().zip(&matrices).all(|(line, (x, y))| {
                (number(&line["baseline"]) - (160.0 - y)).abs() < 0.01
                    && (number(&line["rect"][0]) + bearing - (40.0 + x)).abs() < 0.01
            })
            && lines[0]["text"] == "First"
            && lines[1]["text"] == "Second line"
    });
    let whole = [40.0, 40.0, 300.0, 120.0];
    let rect_given = (0..4).all(|at| (number(&appearance["rect"][at]) - whole[at]).abs() < 1e-9);
    report.check(
        "the report's lines are where the signed file's appearance draws them",
        code == 0
            && agree
            && rect_given
            && appearance["page"] == 1
            && appearance["image"].is_null()
            && number(&appearance["font_size"]) > 0.0,
        &format!("exit {code}, {appearance}, drawn at {matrices:?}: {stderr}"),
    );
    // And the image: the saved one, beside the standard lines, against the
    // matrix that places it in the signed file.
    let pictured = dir.join("pictured.pdf");
    let mut args: Vec<String> = line(&pictured, &[])
        .into_iter()
        .filter(|a| a != "--no-image")
        .collect();
    args.push("--json".into());
    let (code, stdout, stderr) = signs(&args, &store, now);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    let image = &json["appearance"]["image"];
    let placed = image_matrix(&pictured);
    let agree = placed.is_some_and(|[w, h, x, y]| {
        [
            number(&image[0]) - (40.0 + x),
            number(&image[1]) - (160.0 - y - h),
            number(&image[2]) - w,
            number(&image[3]) - h,
        ]
        .iter()
        .all(|off| off.abs() < 0.01)
    });
    report.check(
        "the report's image is where the signed file's appearance draws it",
        code == 0 && agree && json["appearance"]["lines"].as_array().map(Vec::len) == Some(3),
        &format!("exit {code}, {image}, drawn at {placed:?}: {stderr}"),
    );

    // Control: an invisible signature reports no appearance.
    let unseen = dir.join("unseen.pdf");
    let args = strings(&[
        "sign",
        &s(&plain),
        "-o",
        &s(&unseen),
        "--identity",
        SUBJECT,
        "--json",
    ]);
    let (code, stdout, stderr) = signs(&args, &store, now);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_default();
    report.check(
        "control: an invisible signature reports no appearance",
        code == 0 && json["visible"] == false && json["appearance"].is_null(),
        &format!("exit {code}, {}: {stderr}", json["appearance"]),
    );

    // --date-format alone writes the standard date line in that format.
    let dated = dir.join("dated.pdf");
    let (code, _, stderr) = signs(
        &line(&dated, &["--lines", "date", "--date-format", "YYYY"]),
        &store,
        now,
    );
    let drawn = lines(&dated);
    report.check(
        "--date-format writes the standard date line in the format given",
        code == 0
            && drawn.len() == 1
            && drawn[0].len() == "Date: 2026".len()
            && drawn[0].starts_with("Date: 2"),
        &format!("exit {code}, {drawn:?}: {stderr}"),
    );

    // An invisible signature carries a reason, a location and a contact, and
    // draws nothing. Control: without them the dictionary has none.
    let keys: [&[u8]; 3] = [b"Reason", b"Location", b"ContactInfo"];
    let invisible = |out: &Path, extra: &[&str]| {
        let mut args = strings(&["sign", &s(&plain), "-o", &s(out), "--identity", SUBJECT]);
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    };
    let bare = dir.join("bare.pdf");
    let (bare_code, _, _) = signs(&invisible(&bare, &[]), &store, now);
    let carried = dir.join("carried.pdf");
    let (code, _, stderr) = signs(
        &invisible(
            &carried,
            &[
                "--reason",
                "Approved",
                "--location",
                "Hamburg",
                "--contact",
                "jane@example.com",
            ],
        ),
        &store,
        now,
    );
    let wanted = ["Approved", "Hamburg", "jane@example.com"].map(|t| Some(t.to_string()));
    report.check(
        "an invisible signature carries --reason, --location and --contact, and draws nothing",
        code == 0
            && bare_code == 0
            && noted(&carried, &keys) == wanted
            && noted(&bare, &keys) == [None, None, None]
            && lines(&carried).is_empty(),
        &format!(
            "exit {code}/{bare_code}, {:?}, bare {:?}: {stderr}",
            noted(&carried, &keys),
            noted(&bare, &keys)
        ),
    );
}
