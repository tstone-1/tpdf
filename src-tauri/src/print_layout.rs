//! Where each page of a print job goes on the paper.
//!
//! The Windows print path draws every page itself (`print_win.rs`), so the
//! choices a print dialog usually leaves to the printer --- how large a page is
//! drawn, and how many pages share a sheet --- are made here, once, and both
//! the preview and the printer read the same answer. The preview draws what
//! [`lay_out`] returns, and `print_win::spool_sheets` prints it, so a sheet
//! that looks right on screen is the sheet that comes out.
//!
//! The settings the preview sends ([`Settings`]) and the printer it describes
//! back ([`Device`]) are here too: the commands that carry them exist on every
//! platform, though only Windows answers them, since macOS and Linux keep their
//! system dialogs and those offer the same choices.
//!
//! Everything is in PDF points, measured from the top-left corner of the
//! paper's *printable area*: the part of the sheet the printer can put ink
//! on. A placement may reach past that area (actual size, or a percentage,
//! on a page larger than the paper); the printer clips it there, and the
//! preview clips it the same way.

use crate::render::PageSize;

/// The paper a job is laid out on, in points.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    /// The whole sheet.
    pub width: f32,
    pub height: f32,
    /// The printable area, from the sheet's top-left corner.
    pub left: f32,
    pub top: f32,
    pub printable_width: f32,
    pub printable_height: f32,
}

/// How large a page is drawn when it has a sheet to itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "percent")]
pub enum Scale {
    /// At its real size, and smaller when it does not fit. What tpdf did
    /// before there was a choice, and still the default.
    Shrink,
    /// As large as fits the printable area, smaller or larger.
    Fit,
    /// At its real size whatever the paper; what does not fit is cut off.
    Actual,
    /// At this percentage of its real size, centred; what does not fit is
    /// cut off.
    Percent(u16),
    /// At this percentage of its real size, across as many sheets as it
    /// takes, each overlapping the next by [`POSTER_OVERLAP`] so they can be
    /// trimmed and taped together. One page to a sheet only.
    Poster(u16),
}

/// The pages-per-sheet a reader can choose. Each has a fixed grid, the
/// longer side of the paper taking the larger count.
pub const PER_SHEET: [u8; 6] = [1, 2, 4, 6, 9, 16];

/// The largest percentage a reader can type. A page drawn larger than this
/// is a page nobody wants on one sheet.
pub const MOST_PERCENT: u16 = 400;

/// Space kept round a page sharing its sheet with others, in points, so two
/// pages next to each other do not read as one.
const CELL_GAP: f32 = 4.0;

/// How much of a poster each sheet repeats from the one beside it, in
/// points: a quarter of an inch, enough to trim one edge and glue.
pub const POSTER_OVERLAP: f32 = 18.0;

/// The most sheets one page may take as a poster. A page at 400% of A0 on A4
/// would take 256 sheets, which nobody wants by mistake.
pub const MOST_POSTER_SHEETS: usize = 64;

/// Printing on both sides of the paper, and which edge it turns on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TwoSided {
    Off,
    /// Turned on the long edge: a portrait document reads like a book.
    Long,
    /// Turned on the short edge: a portrait document reads like a notepad.
    Short,
}

/// What the preview's controls say, sent each time one changes.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The printer by its Windows name; the default printer when `None`.
    pub printer: Option<String>,
    /// A paper by the driver's number for it; the printer's own when `None`.
    pub paper: Option<i16>,
    pub landscape: Option<bool>,
    pub copies: u16,
    pub two_sided: Option<TwoSided>,
    /// `false` for black and white; the printer's own when `None`.
    pub colour: Option<bool>,
    pub per_sheet: u8,
    pub scale: Scale,
    /// Whether a page turns a quarter when its shape is the other way round
    /// from the space it is drawn in.
    pub rotate: bool,
    /// Whether the comments, highlights, stamps and drawings print. Form
    /// fields print either way.
    pub marks: bool,
    /// The job's pages to print, zero based and in order; every page when `None`.
    pub pages: Option<Vec<u32>>,
}

impl Settings {
    /// How the settings lay the job out.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout {
            per_sheet: self.per_sheet,
            scale: self.scale,
            rotate: self.rotate,
        }
    }
}

/// How a job is laid out: pages to a sheet, the scale, and whether pages turn
/// to fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub per_sheet: u8,
    pub scale: Scale,
    pub rotate: bool,
}

impl Layout {
    /// One page to a sheet, shrunk when it does not fit, never turned: what
    /// tpdf printed before there was a choice, and what the system dialog's
    /// path still prints.
    pub const PLAIN: Self = Self {
        per_sheet: 1,
        scale: Scale::Shrink,
        rotate: false,
    };
}

/// A paper a printer offers.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PaperChoice {
    pub id: i16,
    pub name: String,
}

/// A printer as it is set up for this print: what it offers and what is chosen.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub printer: String,
    pub papers: Vec<PaperChoice>,
    pub paper: i16,
    pub landscape: bool,
    pub copies: u16,
    /// The most copies the driver makes by itself; tpdf repeats the pages
    /// for more.
    pub most_copies: u16,
    /// `None` when the printer cannot print on both sides.
    pub two_sided: Option<TwoSided>,
    /// `None` when the printer prints only black.
    pub colour: Option<bool>,
}

/// One page placed on a sheet: the job's page `page`, drawn `width` by
/// `height` points, its top-left corner at `x`, `y` in the printable area.
///
/// `turned` is a quarter turn anticlockwise, so the page's top is at the
/// sheet's left edge, as a printer turns a landscape page; `width` and
/// `height` are then of the turned page, as it lands on the sheet.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Placement {
    pub page: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub turned: bool,
}

/// Columns and rows for `per_sheet` pages on an area `wide` or not.
///
/// # Errors
///
/// A count that is not in [`PER_SHEET`].
pub fn grid(per_sheet: u8, wide: bool) -> Result<(u32, u32), String> {
    let (long, short) = match per_sheet {
        1 => (1, 1),
        2 => (2, 1),
        4 => (2, 2),
        6 => (3, 2),
        9 => (3, 3),
        16 => (4, 4),
        other => return Err(format!("{other} pages to a sheet is not a choice")),
    };
    Ok(if wide { (long, short) } else { (short, long) })
}

/// Lays `pages` of a job whose pages measure `sizes` out on `paper`.
///
/// Every sheet holds `per_sheet` pages in reading order, left to right and
/// then down, and the last one may hold fewer. With one page to a sheet the
/// page is drawn as `scale` says and centred; with more, each page is drawn as
/// large as fits its cell and centred in it, and `scale` does not apply. A
/// poster takes as many sheets as its page needs, each holding one part.
///
/// With `rotate`, a page whose shape is the other way round from its space (the
/// printable area, or its cell) is turned to match; a square page or space is
/// never turned, and a poster is not.
///
/// # Errors
///
/// A page the job does not have, a paper with no printable area, a count not
/// in [`PER_SHEET`], a percentage of zero or over [`MOST_PERCENT`], or a
/// poster sharing its sheets or needing more than [`MOST_POSTER_SHEETS`].
pub fn lay_out(
    sizes: &[PageSize],
    pages: &[u32],
    paper: &Paper,
    layout: Layout,
) -> Result<Vec<Vec<Placement>>, String> {
    let Layout {
        per_sheet,
        scale,
        rotate,
    } = layout;
    let (area_w, area_h) = (paper.printable_width, paper.printable_height);
    if !(area_w > 0.0 && area_h > 0.0) {
        return Err("the paper has no printable area".into());
    }
    if let Scale::Percent(percent) | Scale::Poster(percent) = scale {
        if percent == 0 || percent > MOST_PERCENT {
            return Err(format!(
                "a scale of {percent}% is not between 1% and {MOST_PERCENT}%"
            ));
        }
    }
    if let Scale::Poster(percent) = scale {
        if per_sheet != 1 {
            return Err("a poster takes one page to a sheet".into());
        }
        let mut sheets = Vec::new();
        for &page in pages {
            let size = page_size(sizes, page)?;
            let tiles = poster(size, page, percent, area_w, area_h).map_err(|count| {
                format!(
                    "page {} as a poster at {percent}% takes {count} sheets, more than the \
                     {MOST_POSTER_SHEETS} tpdf prints of one page",
                    page + 1,
                )
            })?;
            sheets.extend(tiles.into_iter().map(|tile| vec![tile]));
        }
        return Ok(sheets);
    }
    let (cols, rows) = grid(per_sheet, area_w > area_h)?;
    #[allow(clippy::cast_precision_loss)]
    let (cell_w, cell_h) = (area_w / cols as f32, area_h / rows as f32);
    let mut sheets = Vec::new();
    for chunk in pages.chunks(usize::from(per_sheet)) {
        let mut sheet = Vec::with_capacity(chunk.len());
        for (slot, &page) in chunk.iter().enumerate() {
            let size = page_size(sizes, page)?;
            let (x, y, w, h, scale) = if per_sheet == 1 {
                (0.0, 0.0, area_w, area_h, scale)
            } else {
                #[allow(clippy::cast_possible_truncation)]
                let (col, row) = (slot as u32 % cols, slot as u32 / cols);
                #[allow(clippy::cast_precision_loss)]
                let (x, y) = (col as f32 * cell_w, row as f32 * cell_h);
                (
                    x + CELL_GAP,
                    y + CELL_GAP,
                    (cell_w - 2.0 * CELL_GAP).max(1.0),
                    (cell_h - 2.0 * CELL_GAP).max(1.0),
                    Scale::Fit,
                )
            };
            let turned = rotate
                && !square(size.width_pt, size.height_pt)
                && !square(w, h)
                && (size.width_pt > size.height_pt) != (w > h);
            let drawn = if turned {
                PageSize {
                    width_pt: size.height_pt,
                    height_pt: size.width_pt,
                }
            } else {
                size
            };
            let placed = place(drawn, x, y, w, h, scale);
            sheet.push(Placement {
                page,
                turned,
                ..placed
            });
        }
        sheets.push(sheet);
    }
    Ok(sheets)
}

/// Page `page`'s size, refused when the job has no such page or it has no area.
fn page_size(sizes: &[PageSize], page: u32) -> Result<PageSize, String> {
    let size = *sizes
        .get(page as usize)
        .ok_or_else(|| format!("page {} is not in this job", page + 1))?;
    if !(size.width_pt > 0.0 && size.height_pt > 0.0) {
        return Err(format!("page {} has no size", page + 1));
    }
    Ok(size)
}

/// Whether a box is square to within half a point, which no turn improves.
fn square(width: f32, height: f32) -> bool {
    (width - height).abs() < 0.5
}

/// A page of `size` drawn into the box at `x`, `y`, `w` by `h`, centred.
fn place(size: PageSize, x: f32, y: f32, w: f32, h: f32, scale: Scale) -> Placement {
    let fit = (w / size.width_pt).min(h / size.height_pt);
    let factor = match scale {
        Scale::Shrink => fit.min(1.0),
        Scale::Fit => fit,
        Scale::Actual => 1.0,
        Scale::Percent(percent) | Scale::Poster(percent) => f32::from(percent) / 100.0,
    };
    let (width, height) = (size.width_pt * factor, size.height_pt * factor);
    Placement {
        page: 0,
        x: x + (w - width) / 2.0,
        y: y + (h - height) / 2.0,
        width,
        height,
        turned: false,
    }
}

/// Page `page` of `size` at `percent`, cut into the sheets of a poster, left
/// to right and then down. Each sheet's printable area of `area_w` by
/// `area_h` repeats [`POSTER_OVERLAP`] of its neighbour's, and the page is
/// centred on the whole. The count of sheets instead, when it is more than
/// [`MOST_POSTER_SHEETS`].
fn poster(
    size: PageSize,
    page: u32,
    percent: u16,
    area_w: f32,
    area_h: f32,
) -> Result<Vec<Placement>, usize> {
    let factor = f32::from(percent) / 100.0;
    let (width, height) = (size.width_pt * factor, size.height_pt * factor);
    // A sheet no wider than the overlap would never move on; one point is
    // still a poster of very many sheets, which the caller refuses.
    let (step_w, step_h) = (
        (area_w - POSTER_OVERLAP).max(1.0),
        (area_h - POSTER_OVERLAP).max(1.0),
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = |length: f32, area: f32, step: f32| -> u32 {
        if length <= area {
            1
        } else {
            // Bounded, so a poster that would take millions of sheets is
            // counted and refused rather than built.
            (((length - area) / step).ceil() as u32).min(1024) + 1
        }
    };
    let (cols, rows) = (count(width, area_w, step_w), count(height, area_h, step_h));
    #[allow(clippy::cast_precision_loss)]
    let (covered_w, covered_h) = (
        (cols - 1) as f32 * step_w + area_w,
        (rows - 1) as f32 * step_h + area_h,
    );
    let (start_x, start_y) = ((covered_w - width) / 2.0, (covered_h - height) / 2.0);
    let sheets = cols as usize * rows as usize;
    if sheets > MOST_POSTER_SHEETS {
        return Err(sheets);
    }
    let mut tiles = Vec::with_capacity(sheets);
    for row in 0..rows {
        for col in 0..cols {
            #[allow(clippy::cast_precision_loss)]
            tiles.push(Placement {
                page,
                x: start_x - col as f32 * step_w,
                y: start_y - row as f32 * step_h,
                width,
                height,
                turned: false,
            });
        }
    }
    Ok(tiles)
}

#[cfg(test)]
mod tests {
    use super::{
        grid, lay_out, Layout, Paper, Scale, CELL_GAP, MOST_POSTER_SHEETS, POSTER_OVERLAP,
    };
    use crate::render::PageSize;

    const A4: PageSize = PageSize {
        width_pt: 595.0,
        height_pt: 842.0,
    };

    /// A4 paper with a 10-point unprintable edge all round.
    fn paper() -> Paper {
        Paper {
            width: 595.0,
            height: 842.0,
            left: 10.0,
            top: 10.0,
            printable_width: 575.0,
            printable_height: 822.0,
        }
    }

    /// A layout with pages never turned, as every test but the turning ones has.
    fn at(per_sheet: u8, scale: Scale) -> Layout {
        Layout {
            per_sheet,
            scale,
            rotate: false,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn shrink_reduces_a_page_that_does_not_fit_and_leaves_one_that_does() {
        let small = PageSize {
            width_pt: 200.0,
            height_pt: 100.0,
        };
        let sheets =
            lay_out(&[A4, small], &[0, 1], &paper(), at(1, Scale::Shrink)).expect("laid out");
        assert_eq!(sheets.len(), 2);
        let big = sheets[0][0];
        // Limited by the width: 575 / 595.
        assert!(close(big.width, 575.0), "{big:?}");
        assert!(close(big.x, 0.0), "{big:?}");
        let little = sheets[1][0];
        assert!(
            close(little.width, 200.0) && close(little.height, 100.0),
            "{little:?}"
        );
        // Centred in the printable area.
        assert!(
            close(little.x, 187.5) && close(little.y, 361.0),
            "{little:?}"
        );
    }

    #[test]
    fn fit_enlarges_a_small_page_to_the_printable_area() {
        let small = PageSize {
            width_pt: 100.0,
            height_pt: 100.0,
        };
        let sheets = lay_out(&[small], &[0], &paper(), at(1, Scale::Fit)).expect("laid out");
        let placed = sheets[0][0];
        assert!(
            close(placed.width, 575.0) && close(placed.height, 575.0),
            "{placed:?}"
        );
    }

    #[test]
    fn actual_size_and_a_percentage_keep_their_size_and_may_overhang() {
        let a3 = PageSize {
            width_pt: 842.0,
            height_pt: 1191.0,
        };
        let actual = lay_out(&[a3], &[0], &paper(), at(1, Scale::Actual)).expect("laid out")[0][0];
        assert!(close(actual.width, 842.0), "{actual:?}");
        assert!(
            actual.x < 0.0,
            "an A3 page at actual size reaches past A4: {actual:?}"
        );
        let half =
            lay_out(&[a3], &[0], &paper(), at(1, Scale::Percent(50))).expect("laid out")[0][0];
        assert!(
            close(half.width, 421.0) && close(half.height, 595.5),
            "{half:?}"
        );
    }

    #[test]
    fn a_percentage_out_of_range_is_refused() {
        assert!(lay_out(&[A4], &[0], &paper(), at(1, Scale::Percent(0))).is_err());
        assert!(lay_out(&[A4], &[0], &paper(), at(1, Scale::Percent(401))).is_err());
    }

    #[test]
    fn four_to_a_sheet_reads_left_to_right_then_down() {
        let sheets =
            lay_out(&[A4; 5], &[0, 1, 2, 3, 4], &paper(), at(4, Scale::Shrink)).expect("laid out");
        assert_eq!(sheets.len(), 2, "five pages at four to a sheet");
        assert_eq!(sheets[1].len(), 1);
        let s = &sheets[0];
        assert_eq!(s.iter().map(|p| p.page).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(
            s[1].x > s[0].x && close(s[1].y, s[0].y),
            "second is right of the first"
        );
        assert!(
            s[2].y > s[0].y && close(s[2].x, s[0].x),
            "third is under the first"
        );
        // Each fits its quarter, less the gap.
        let cell_w = 575.0 / 2.0 - 2.0 * CELL_GAP;
        assert!(s.iter().all(|p| p.width <= cell_w + 0.01), "{s:?}");
    }

    #[test]
    fn two_to_a_portrait_sheet_are_stacked_and_to_a_landscape_one_side_by_side() {
        assert_eq!(grid(2, false), Ok((1, 2)));
        assert_eq!(grid(2, true), Ok((2, 1)));
        assert_eq!(grid(6, false), Ok((2, 3)));
        assert_eq!(grid(16, true), Ok((4, 4)));
        assert!(grid(3, true).is_err());
    }

    #[test]
    fn a_scale_does_not_apply_to_pages_sharing_a_sheet() {
        let shrink = lay_out(&[A4; 2], &[0, 1], &paper(), at(2, Scale::Shrink)).expect("laid out");
        let actual = lay_out(&[A4; 2], &[0, 1], &paper(), at(2, Scale::Actual)).expect("laid out");
        assert_eq!(shrink, actual);
    }

    #[test]
    fn a_page_the_job_does_not_have_is_refused() {
        assert!(lay_out(&[A4], &[1], &paper(), at(1, Scale::Shrink)).is_err());
    }

    #[test]
    fn the_pages_are_printed_in_the_order_asked() {
        let sheets = lay_out(&[A4; 3], &[2, 0], &paper(), at(1, Scale::Shrink)).expect("laid out");
        assert_eq!(sheets.iter().map(|s| s[0].page).collect::<Vec<_>>(), [2, 0]);
    }

    const A4_LANDSCAPE: PageSize = PageSize {
        width_pt: 842.0,
        height_pt: 595.0,
    };

    fn turning(per_sheet: u8, scale: Scale) -> Layout {
        Layout {
            rotate: true,
            ..at(per_sheet, scale)
        }
    }

    #[test]
    fn a_landscape_page_turns_to_fill_a_portrait_sheet_and_a_portrait_one_does_not() {
        let sheets = lay_out(
            &[A4_LANDSCAPE, A4],
            &[0, 1],
            &paper(),
            turning(1, Scale::Fit),
        )
        .expect("laid out");
        let (wide, tall) = (sheets[0][0], sheets[1][0]);
        assert!(wide.turned, "{wide:?}");
        // Drawn as large as the turned page fits: the area's width across its
        // 595-point side.
        assert!(
            close(wide.width, 575.0) && wide.height > wide.width,
            "{wide:?}"
        );
        assert!(!tall.turned, "{tall:?}");
        // Without the choice the landscape page lies across the sheet, smaller.
        let flat =
            lay_out(&[A4_LANDSCAPE], &[0], &paper(), at(1, Scale::Fit)).expect("laid out")[0][0];
        assert!(!flat.turned && flat.width > flat.height, "{flat:?}");
        assert!(flat.width * flat.height < wide.width * wide.height);
    }

    #[test]
    fn a_page_turns_to_fit_its_cell_and_a_square_one_never_turns() {
        // Two to a portrait sheet: each cell is wider than tall, so a
        // portrait page turns and a landscape one does not.
        let sheets = lay_out(
            &[A4, A4_LANDSCAPE],
            &[0, 1],
            &paper(),
            turning(2, Scale::Shrink),
        )
        .expect("laid out");
        assert!(
            sheets[0][0].turned && !sheets[0][1].turned,
            "{:?}",
            sheets[0]
        );
        let square = PageSize {
            width_pt: 500.0,
            height_pt: 500.0,
        };
        let placed =
            lay_out(&[square], &[0], &paper(), turning(1, Scale::Fit)).expect("laid out")[0][0];
        assert!(!placed.turned, "{placed:?}");
    }

    #[test]
    fn a_poster_covers_its_page_with_overlapping_sheets() {
        // A4 at 200% is 1190 x 1684 points; each sheet steps 575 - 18 across
        // and 822 - 18 down, so three across and three down.
        let sheets = lay_out(&[A4], &[0], &paper(), at(1, Scale::Poster(200))).expect("laid out");
        assert_eq!(sheets.len(), 9);
        assert!(sheets.iter().all(|sheet| sheet.len() == 1));
        let first = sheets[0][0];
        assert!(
            close(first.width, 1190.0) && close(first.height, 1684.0),
            "{first:?}"
        );
        let (second, below) = (sheets[1][0], sheets[3][0]);
        assert!(
            close(first.x - second.x, 575.0 - POSTER_OVERLAP),
            "{second:?}"
        );
        assert!(
            close(first.y - below.y, 822.0 - POSTER_OVERLAP),
            "{below:?}"
        );
        // The page is centred on the sheets: the margin left over is shared.
        let last = sheets[8][0];
        assert!(
            close(first.x, -(last.x + last.width - 575.0)),
            "{first:?} {last:?}"
        );
        // Every sheet shows some of the page.
        assert!(sheets.iter().all(|s| s[0].x < 575.0
            && s[0].x + s[0].width > 0.0
            && s[0].y < 822.0
            && s[0].y + s[0].height > 0.0));
    }

    #[test]
    fn a_poster_that_fits_one_sheet_is_one_sheet() {
        let sheets = lay_out(&[A4], &[0], &paper(), at(1, Scale::Poster(50))).expect("laid out");
        assert_eq!(sheets.len(), 1);
    }

    #[test]
    fn a_poster_is_one_page_to_a_sheet_and_not_too_many_sheets() {
        assert!(lay_out(&[A4], &[0], &paper(), at(2, Scale::Poster(200))).is_err());
        let a0 = PageSize {
            width_pt: 2384.0,
            height_pt: 3370.0,
        };
        let refused = lay_out(&[a0], &[0], &paper(), at(1, Scale::Poster(400))).unwrap_err();
        assert!(
            refused.contains(&MOST_POSTER_SHEETS.to_string()),
            "{refused}"
        );
        assert!(lay_out(&[a0], &[0], &paper(), at(1, Scale::Poster(100))).is_ok());
    }
}
