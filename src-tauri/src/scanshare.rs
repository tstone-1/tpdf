//! How much of a page is pictures: what decides whether a page is a scan.
//!
//! On its own, with nothing of the program's beneath it, because two sides ask
//! it: `render::run_pictures` measures a page in the worker and
//! `ocr_layer::wants_reading` decides with the answer. Kept inside `ocr_layer`,
//! the render module's one call pulled the OCR layer and the writers beneath it
//! into the module cycle.

/// The share of a page that pictures must cover for it to be read as a scan,
/// whatever text it has.
///
/// Until 2026-10-15 a page with one character was left alone, so a scan with a
/// typed fax line, a stamp or a page number got no layer at all. Measured that
/// day on pages built from a real document: four scans, plain or with 12 to 48
/// typed characters, are 100% picture; born-digital pages are 0 to 3.1%,
/// including one with 31 characters and a photo. Text alone could not tell
/// them apart (0.2 to 0.6% of the page under characters on the scans, 0.4% on
/// that born-digital page). Any value between the two groups separates them;
/// half is the middle.
pub const SCAN_SHARE: f32 = 0.5;

/// How many cells a side of the page is divided into to measure
/// [`picture_share`].
const GRID: usize = 100;

/// The share of `page_box` that the `pictures` cover, from 0 to 1.
///
/// Both in the page's own space, y up (`objects::Rect`). The union is measured
/// by the centres of a 100 x 100 grid, so two pictures over the same place
/// count once: some scanners write a page as strips, and a picture under
/// another is not more page.
#[must_use]
pub fn picture_share(page_box: [f32; 4], pictures: &[[f32; 4]]) -> f32 {
    let [left, bottom, right, top] = page_box;
    let (width, height) = (right - left, top - bottom);
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return 0.0;
    }
    let mut covered = 0usize;
    for row in 0..GRID {
        let y = bottom + (row as f32 + 0.5) * height / GRID as f32;
        for column in 0..GRID {
            let x = left + (column as f32 + 0.5) * width / GRID as f32;
            if pictures
                .iter()
                .any(|r| r[0] <= x && x <= r[2] && r[1] <= y && y <= r[3])
            {
                covered += 1;
            }
        }
    }
    covered as f32 / (GRID * GRID) as f32
}
