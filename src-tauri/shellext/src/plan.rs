//! The decisions the handler makes that need no Windows API: the limits, the
//! command line `tpdf-cli` is started with, the sizes asked for and accepted,
//! and which page a key moves to. Compiled on every platform so its tests run
//! on any machine; everything that touches Windows is in the other modules.

use std::time::Duration;

/// The most of Explorer's stream the handler reads. A larger document gets
/// Explorer's generic icon (`docs/THREAT-MODEL.md` §T6.47).
pub const MAX_DOCUMENT: u64 = 256 * 1024 * 1024;

/// The largest PNG accepted back from `tpdf-cli`. A page at the largest size
/// asked for ([`MAX_SIDE`] squared, RGBA) is 64 MiB uncompressed, and a PNG of
/// it is smaller, so a longer answer is not one `tpdf-cli` wrote.
pub const MAX_PNG: usize = 64 * 1024 * 1024;

/// The longest side the handler asks for. Explorer's largest thumbnail is
/// 2560 pixels, and a preview pane is a window.
pub const MAX_SIDE: u32 = 4096;

/// How long a thumbnail may take, `tpdf-cli` and its worker together.
pub const THUMBNAIL_TIME: Duration = Duration::from_secs(10);

/// How long one page of the preview pane may take.
pub const PREVIEW_TIME: Duration = Duration::from_secs(20);

/// The memory `tpdf-cli` and its worker may commit together, enforced by the
/// job object they run in.
pub const JOB_MEMORY: usize = 1024 * 1024 * 1024;

/// The box a page is fitted into, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    pub width: u32,
    pub height: u32,
}

/// The arguments after `tpdf-cli.exe`: the document on standard input, the
/// PNG on standard output. A list, never a command line a shell reads.
pub fn render_args(fit: Fit, page: u32) -> Vec<String> {
    vec![
        "render".into(),
        "-".into(),
        "-o".into(),
        "-".into(),
        "--fit".into(),
        format!("{}x{}", fit.width, fit.height),
        "--page".into(),
        page.to_string(),
    ]
}

/// The box for a thumbnail of `cx` pixels, which Explorer asks for as the
/// longer side. `None` for a size outside 1 to [`MAX_SIDE`].
pub fn thumbnail_fit(cx: u32) -> Option<Fit> {
    (1..=MAX_SIDE).contains(&cx).then_some(Fit {
        width: cx,
        height: cx,
    })
}

/// Pixels kept clear around the page in the preview pane.
pub const MARGIN: i32 = 8;

/// Pixels under the page for the page number.
pub const LABEL: i32 = 24;

/// The box for a page in a preview pane whose client area is `width` by
/// `height`: the area less the margins and the label, held to [`MAX_SIDE`].
/// `None` when nothing that small is worth asking for.
pub fn preview_fit(width: i32, height: i32) -> Option<Fit> {
    let width = width - 2 * MARGIN;
    let height = height - 2 * MARGIN - LABEL;
    if width < 16 || height < 16 {
        return None;
    }
    Some(Fit {
        width: (width as u32).min(MAX_SIDE),
        height: (height as u32).min(MAX_SIDE),
    })
}

/// Whether a decoded picture is one `tpdf-cli` could have answered `fit`
/// with: not empty, and inside the box.
pub fn fits(width: u32, height: u32, fit: Fit) -> bool {
    width > 0 && height > 0 && width <= fit.width && height <= fit.height
}

/// Whether `bytes` start as a PNG does. Checked before WIC is handed them, so
/// that a `tpdf-cli` that wrote something else is refused by name.
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_PNG && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// Where a picture of `width` by `height` goes in a pane of `pane_width` by
/// `pane_height`: centred in the area above the label, never above or left
/// of the margin.
pub fn placed(pane_width: i32, pane_height: i32, width: i32, height: i32) -> (i32, i32) {
    let area_height = pane_height - LABEL;
    let x = ((pane_width - width) / 2).max(MARGIN);
    let y = ((area_height - height) / 2).max(MARGIN);
    (x, y)
}

/// A key or a turn of the wheel in the preview pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Next,
    Previous,
    First,
}

/// The page `step` asks for from `page`, or `None` when it asks for nothing
/// new. The handler does not know how many pages there are: a page past the
/// end is refused by `tpdf-cli`, and the pane keeps the page it shows.
pub fn stepped(page: u32, step: Step) -> Option<u32> {
    match step {
        Step::Next => page.checked_add(1),
        Step::Previous => (page > 1).then(|| page - 1),
        Step::First => (page != 1).then_some(1),
    }
}

/// What `tpdf-cli`'s exit code says, by the contract in its `cli.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// 0: standard output is the PNG.
    Done,
    /// 3: tpdf refused --- the document, or a page past its end.
    Refused,
    /// Anything else, or killed.
    Failed,
}

/// [`Ended`], from the exit code; `None` is a process that had none.
pub fn ended(code: Option<i32>) -> Ended {
    match code {
        Some(0) => Ended::Done,
        Some(3) => Ended::Refused,
        _ => Ended::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_reads_standard_input_and_writes_standard_output() {
        let args = render_args(
            Fit {
                width: 256,
                height: 200,
            },
            3,
        );
        assert_eq!(
            args,
            ["render", "-", "-o", "-", "--fit", "256x200", "--page", "3"]
        );
    }

    #[test]
    fn a_thumbnail_size_outside_the_limits_asks_for_nothing() {
        assert_eq!(thumbnail_fit(0), None);
        assert_eq!(thumbnail_fit(MAX_SIDE + 1), None);
        assert_eq!(
            thumbnail_fit(256),
            Some(Fit {
                width: 256,
                height: 256
            })
        );
        assert_eq!(thumbnail_fit(MAX_SIDE).map(|f| f.width), Some(MAX_SIDE));
    }

    #[test]
    fn a_preview_box_leaves_the_margins_and_the_label() {
        assert_eq!(
            preview_fit(400, 600),
            Some(Fit {
                width: 400 - 16,
                height: 600 - 16 - 24
            })
        );
        assert_eq!(preview_fit(30, 600), None);
        assert_eq!(preview_fit(400, 50), None);
        assert_eq!(
            preview_fit(100_000, 100_000),
            Some(Fit {
                width: MAX_SIDE,
                height: MAX_SIDE
            })
        );
    }

    #[test]
    fn a_picture_larger_than_asked_or_empty_is_refused() {
        let fit = Fit {
            width: 256,
            height: 256,
        };
        assert!(fits(256, 181, fit));
        assert!(!fits(257, 181, fit));
        assert!(!fits(181, 257, fit));
        assert!(!fits(0, 10, fit));
        assert!(!fits(10, 0, fit));
    }

    #[test]
    fn only_bytes_that_start_as_a_png_are_handed_to_the_decoder() {
        assert!(is_png(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_png(b"%PDF-1.7"));
        assert!(!is_png(b""));
        assert!(!is_png(&vec![0u8; MAX_PNG + 1]));
    }

    #[test]
    fn a_page_is_centred_above_the_label_and_never_off_the_margin() {
        assert_eq!(placed(400, 324, 200, 100), (100, 100));
        assert_eq!(placed(100, 100, 400, 400), (MARGIN, MARGIN));
    }

    #[test]
    fn keys_move_one_page_and_stop_at_the_first() {
        assert_eq!(stepped(1, Step::Next), Some(2));
        assert_eq!(stepped(1, Step::Previous), None);
        assert_eq!(stepped(5, Step::Previous), Some(4));
        assert_eq!(stepped(5, Step::First), Some(1));
        assert_eq!(stepped(1, Step::First), None);
        assert_eq!(stepped(u32::MAX, Step::Next), None);
    }

    #[test]
    fn the_exit_code_contract_is_read_as_cli_rs_writes_it() {
        assert_eq!(ended(Some(0)), Ended::Done);
        assert_eq!(ended(Some(3)), Ended::Refused);
        for other in [Some(1), Some(2), Some(4), Some(-1), None] {
            assert_eq!(ended(other), Ended::Failed);
        }
    }
}
