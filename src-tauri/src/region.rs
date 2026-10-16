//! A region of a page saved as a PNG file (*Save a region as a picture*).
//!
//! The pixels are a tile, rendered and PNG-encoded by a worker as every tile
//! is (`commands::edit::region_save`); this module only writes them to the
//! place the reader chose. The webview names the region, never the bytes, so
//! the one file this command writes holds what a worker rendered.

use std::io::Write as _;
use std::path::Path;

/// The eight bytes every PNG file starts with (PNG specification, 5.2).
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Writes `png` to `path`: to a file beside it first, then renamed over it, so
/// a write that fails leaves whatever was at `path` as it was.
///
/// Refuses a name that does not end in `.png`, which the save panel's filter
/// gives everywhere but a Linux panel the reader typed a bare name into: a
/// PNG under another name is a file that opens in the wrong program, and
/// adding the ending here would write to a file the panel never asked about.
/// Refuses bytes that are not a PNG, which a worker's encoder never sends.
///
/// # Errors
///
/// The name, the bytes, or the write, in words a reader is shown.
pub fn write_png(path: &Path, png: &[u8]) -> Result<(), String> {
    let named_png = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
    if !named_png {
        return Err(format!(
            "The picture was not saved: {} does not end in .png.",
            path.display()
        ));
    }
    if !png.starts_with(&SIGNATURE) {
        return Err("The picture was not saved: the render did not come back as a PNG.".into());
    }
    let name = path.file_name().ok_or_else(|| {
        format!(
            "The picture was not saved: {} names no file.",
            path.display()
        )
    })?;
    let staged = path.with_file_name(format!(
        ".{}.tpdf-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let written = std::fs::File::create(&staged)
        .and_then(|mut file| {
            file.write_all(png)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&staged, path));
    written.map_err(|why| {
        let _ = std::fs::remove_file(&staged);
        format!("The picture was not saved to {}: {why}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn png(tail: &[u8]) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend_from_slice(tail);
        bytes
    }

    #[test]
    fn writes_the_bytes_and_leaves_nothing_beside_them() {
        let dir = TempDir::new("region-write");
        let path = dir.join("page 3.png");
        write_png(&path, &png(b"pixels")).expect("written");
        assert_eq!(std::fs::read(&path).expect("read"), png(b"pixels"));
        assert_eq!(dir.names(), ["page 3.png"]);
    }

    #[test]
    fn replaces_a_file_already_there() {
        let dir = TempDir::new("region-replace");
        let path = dir.join("old.PNG");
        std::fs::write(&path, b"old").expect("seed");
        write_png(&path, &png(b"new")).expect("written");
        assert_eq!(std::fs::read(&path).expect("read"), png(b"new"));
    }

    #[test]
    fn refuses_a_name_that_is_not_a_png_and_writes_nothing() {
        let dir = TempDir::new("region-name");
        for name in ["page", "page.pdf", "png"] {
            let refused = write_png(&dir.join(name), &png(b"x")).expect_err(name);
            assert!(refused.contains("does not end in .png"), "{refused}");
        }
        assert!(dir.names().is_empty(), "{:?}", dir.names());
    }

    #[test]
    fn refuses_bytes_that_are_not_a_png_and_keeps_the_old_file() {
        let dir = TempDir::new("region-bytes");
        let path = dir.join("kept.png");
        std::fs::write(&path, b"kept").expect("seed");
        let refused = write_png(&path, b"\x89PNG but not").expect_err("not a png");
        assert!(refused.contains("did not come back as a PNG"), "{refused}");
        assert_eq!(std::fs::read(&path).expect("read"), b"kept");
        assert_eq!(dir.names(), ["kept.png"]);
    }

    #[test]
    fn a_failed_write_removes_its_staged_file() {
        let dir = TempDir::new("region-fail");
        // A directory where the file would go: the rename over it fails.
        let path = dir.join("taken.png");
        std::fs::create_dir(&path).expect("dir");
        std::fs::write(path.join("inside"), b"x").expect("fill");
        let refused = write_png(&path, &png(b"x")).expect_err("rename over a directory");
        assert!(refused.contains("was not saved to"), "{refused}");
        assert_eq!(dir.names(), ["taken.png"]);
    }
}
