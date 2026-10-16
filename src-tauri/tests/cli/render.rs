//! Pixels are checked against the fixture's painted quadrants, not another
//! invocation of the same renderer. At 144 DPI both axes cross tile boundaries.
use super::{scratch, tool, Report};
use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;

fn fixture(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let tree = doc.new_object_id();
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        b"1 0 0 rg 10 20 300 400 re f 0 1 0 rg 310 420 300 400 re f 0 0 1 rg 310 20 300 400 re f"
            .to_vec(),
    ));
    let mut kids = Vec::new();
    for rotation in [0, 90, 180, 270] {
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type"=>"Page", "Parent"=>tree, "MediaBox"=>vec![0.into(),0.into(),650.into(),850.into()],
            "CropBox"=>vec![10.into(),20.into(),610.into(),820.into()], "Rotate"=>rotation,
            "Contents"=>stream, "Resources"=>dictionary! {},
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

fn pixels(path: &Path) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .read_info()
        .unwrap();
    let mut data = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut data).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    data.truncate(info.buffer_size());
    (info.width, info.height, data)
}

pub(super) fn renders(report: &mut Report) {
    let dir = scratch("render-pages");
    let source = dir.join("source.pdf");
    let output = dir.join("page.png");
    fixture(&source);
    let original = std::fs::read(&source).unwrap();
    let input = source.display().to_string();
    let destination = output.display().to_string();
    let run = |page: &str, dpi: &str, force: bool| {
        let mut args = vec![
            "render",
            &input,
            "-o",
            &destination,
            "--page",
            page,
            "--dpi",
            dpi,
            "--json",
        ];
        if force {
            args.push("--force");
        }
        tool(&args, &[])
    };
    let white = [255, 255, 255, 255];
    let red = [255, 0, 0, 255];
    let green = [0, 255, 0, 255];
    let blue = [0, 0, 255, 255];
    // Top-left, top-right, bottom-left, bottom-right after each saved rotation.
    let expected = [
        [white, green, red, blue],
        [red, white, blue, green],
        [blue, red, green, white],
        [green, blue, white, red],
    ];
    for (page, colors) in expected.iter().enumerate() {
        for dpi in [72, 144] {
            let (code, json, stderr) = run(&(page + 1).to_string(), &dpi.to_string(), true);
            report.check(
                &format!("render page {} at {dpi} DPI succeeds", page + 1),
                code == 0,
                &stderr,
            );
            if code != 0 {
                continue;
            }
            let (w, h, data) = pixels(&output);
            let factor = dpi / 72;
            let (ew, eh) = if page % 2 == 0 {
                (600 * factor, 800 * factor)
            } else {
                (800 * factor, 600 * factor)
            };
            let result: serde_json::Value = serde_json::from_str(&json).unwrap();
            report.check(
                "PNG dimensions and JSON honor saved crop, rotation and resolution",
                (w, h) == (ew, eh)
                    && result["width_px"] == w
                    && result["height_px"] == h
                    && result["page"] == page + 1
                    && result["dpi"] == dpi,
                &json,
            );
            // Sample corners and both sides of each tile seam. They are all well
            // inside filled regions, so antialiasing cannot justify a mismatch.
            let mut points = vec![(20, 20), (w - 20, 20), (20, h - 20), (w - 20, h - 20)];
            if w > 1024 {
                points.extend([(1023, 20), (1024, 20), (1023, h - 20), (1024, h - 20)]);
            }
            if h > 1024 {
                points.extend([(20, 1023), (20, 1024), (w - 20, 1023), (w - 20, 1024)]);
            }
            let correct = points.iter().all(|&(x, y)| {
                let quadrant = usize::from(y >= h / 2) * 2 + usize::from(x >= w / 2);
                let start = ((y * w + x) * 4) as usize;
                data[start..start + 4] == colors[quadrant]
            });
            report.check(
                "painted quadrants and tile seams have the expected RGBA pixels",
                correct,
                &format!("page {}, DPI {dpi}", page + 1),
            );
        }
    }
    let before = std::fs::read(&output).unwrap();
    let (code, _, stderr) = run("4", "144", true);
    report.check(
        "repeated rendering is byte-identical",
        code == 0 && std::fs::read(&output).unwrap() == before,
        &stderr,
    );
    for (page, dpi, force, status) in [
        ("1", "144", false, 3),
        ("99", "144", true, 3),
        ("1", "600", true, 3),
        ("0", "144", true, 2),
        ("1", "601", true, 2),
    ] {
        let (code, json, stderr) = run(page, dpi, force);
        let result: serde_json::Value = serde_json::from_str(&json).unwrap();
        report.check(
            "invalid or colliding renders preserve existing output",
            code == status
                && result["error"]["exit_code"] == status
                && std::fs::read(&output).unwrap() == before,
            &stderr,
        );
    }
    let encrypted = dir.join("encrypted.pdf");
    if std::process::Command::new("qpdf")
        .args([
            "--encrypt",
            "synthetic-user",
            "synthetic-owner",
            "256",
            "--",
        ])
        .arg(&source)
        .arg(&encrypted)
        .status()
        .is_ok_and(|s| s.success())
    {
        let encrypted_name = encrypted.display().to_string();
        for (secret, expected_status) in
            [(None, 3), (Some("wrong"), 3), (Some("synthetic-user"), 0)]
        {
            let mut args = vec![
                "render",
                &encrypted_name,
                "-o",
                &destination,
                "--page",
                "4",
                "--force",
                "--json",
            ];
            let mut env = Vec::new();
            if let Some(secret) = secret {
                args.extend(["--password-env", "TPDF_TEST_RENDER_PASSWORD"]);
                env.push(("TPDF_TEST_RENDER_PASSWORD", secret));
            }
            let (code, json, stderr) = tool(&args, &env);
            report.check(
                "encrypted renders require the password and match unencrypted pixels",
                code == expected_status && std::fs::read(&output).unwrap() == before,
                &format!("{stderr}; {json}"),
            );
        }
    } else {
        report.skip("encrypted page rendering", "qpdf is unavailable");
    }
    let (code, _, stderr) = tool(&["render", &input, "-o", &input, "--force", "--json"], &[]);
    report.check(
        "render never overwrites the input, even with force",
        code == 2 && std::fs::read(&source).unwrap() == original,
        &stderr,
    );
    // The Explorer handler's route (`shellext/`): the document on standard
    // input, the PNG alone on standard output, the page fitted into a box.
    let piped = |args: &[&str]| {
        use std::io::Write;
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_tpdf-cli"))
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the built tool runs");
        child.stdin.take().unwrap().write_all(&original).unwrap();
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap_or(-1),
            out.stdout,
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    for (page, fit, expect) in [
        ("1", "256x256", (192, 256)),
        ("2", "256x256", (256, 192)),
        ("1", "100x40", (30, 40)),
    ] {
        let (code, png, stderr) = piped(&["render", "-", "-o", "-", "--page", page, "--fit", fit]);
        report.check(
            &format!(
                "render - -o - --fit {fit} writes page {page} and nothing else to standard output"
            ),
            code == 0 && png.starts_with(b"\x89PNG\r\n\x1a\n"),
            &stderr,
        );
        if code != 0 {
            continue;
        }
        let piped_png = dir.join("piped.png");
        std::fs::write(&piped_png, &png).unwrap();
        let (w, h, data) = pixels(&piped_png);
        // Page 1 has red at the bottom left, page 2 (turned 90) at the top left.
        let colors = if page == "1" {
            [white, green, red, blue]
        } else {
            [red, white, blue, green]
        };
        let inside = |x: u32, y: u32| {
            let quadrant = usize::from(y >= h / 2) * 2 + usize::from(x >= w / 2);
            let start = ((y * w + x) * 4) as usize;
            data[start..start + 4] == colors[quadrant]
        };
        let (qx, qy) = (w / 4, h / 4);
        report.check(
            &format!("--fit {fit} sizes page {page} to the box and paints its quadrants"),
            (w, h) == expect
                && inside(qx, qy)
                && inside(w - qx, qy)
                && inside(qx, h - qy)
                && inside(w - qx, h - qy),
            &format!("{w} x {h}"),
        );
    }
    let (code, png, stderr) = piped(&["render", "-", "-o", "-", "--page", "9", "--fit", "64x64"]);
    report.check(
        "a page past the end is refused with exit 3 and nothing on standard output",
        code == 3 && png.is_empty(),
        &stderr,
    );
    report.check(
        "render leaves no staging directories",
        std::fs::read_dir(&dir).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tpdf-cli-")
        }),
        "",
    );
    std::fs::remove_dir_all(dir).unwrap();
}
