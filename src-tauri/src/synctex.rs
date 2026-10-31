//! LaTeX forward search: from a line of a source file to the boxes it made on
//! a page (`--forward-search file.tex:LINE`).
//!
//! An editor runs tpdf with the line its cursor is on, and the window shows
//! the page and marks the lines that came from it. The map is the
//! `.synctex.gz` file a TeX engine writes beside the PDF when asked
//! (`-synctex=1`). It is input from outside, like the PDF, so this parser is
//! our own, in Rust, and bounded: at most 64 MiB read from disk and 512 MiB
//! once decompressed, no line longer than 64 KiB, and boxes nested at most
//! 100,000 deep. It reads text and builds no structure the file can make
//! large; nothing in it is run.
//!
//! ## What the file says
//!
//! After a header (`Unit:`, `Magnification:`, `X Offset:`, `Y Offset:`), each
//! output page is `{N` ... `}N`, and inside it boxes open and close: `(` an
//! hbox, a line of text, and `[` a vbox, each followed by
//! `tag,line:x,y:W,H,D`. Points inside them are `x`, `k`, `g` and `$` records,
//! `tag,line:x,y`. `tag` is a source file, named by an `Input:tag:path` line;
//! positions are in TeX's scaled points from the page's top-left corner, `y`
//! the baseline, `H` the height above it and `D` the depth below.
//!
//! **A line of text is an hbox tagged with the line where its paragraph ended**,
//! not the line its words came from: in the fixture the first line of the page
//! is `(1,4:` while every word in it is `x1,3:`. So a source line is found by
//! its point records, and what is marked is the hbox around them. A box record
//! that names the line itself is marked as it is. Vboxes are not marked: they
//! are a paragraph or a page, and would cover the line asked for along with
//! everything near it.
//!
//! A line that made nothing (a blank line, a `\newpage`) is answered with the
//! nearest line after it that did, or, failing that, before it, and the answer
//! says which line it used. Only the first page holding the line is marked.
//!
//! ## Which input is the source
//!
//! The file names inputs as the engine opened them, often absolute paths on
//! the machine that compiled the document. An input is the source when the two
//! paths are the same file; when none is, an input with the same file name is,
//! provided only one input has that name. A project moved or compiled
//! elsewhere is found that way, and two `intro.tex` files are refused rather
//! than guessed between.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

/// The most read from disk.
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// The most read once decompressed.
const MAX_TEXT: u64 = 512 * 1024 * 1024;
/// The longest line.
const MAX_LINE: u64 = 64 * 1024;
/// The deepest nesting of boxes.
const MAX_DEPTH: usize = 100_000;
/// The most rectangles kept for one line.
const MAX_BOXES: usize = 10_000;
/// Scaled points in a PostScript point: 65536 per TeX point, 72.27 TeX points
/// to the inch, 72 PostScript points to the inch.
const SP_PER_BP: f64 = 65_781.76;

/// A rectangle on a page, in points from its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Rect {
    pub x: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Where a source line went.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Forward {
    /// The page, counted from 1.
    pub page: u32,
    /// The line whose boxes these are: the one asked for, or the nearest that
    /// made something.
    pub line: u32,
    /// The boxes on that page, in the order the file has them.
    pub boxes: Vec<Rect>,
}

/// The SyncTeX file beside `pdf`: `name.synctex.gz`, or `name.synctex`.
pub fn beside(pdf: &Path) -> Option<PathBuf> {
    let stem = pdf.file_stem()?;
    let folder = pdf.parent().unwrap_or_else(|| Path::new(""));
    ["synctex.gz", "synctex"].iter().find_map(|ending| {
        let mut name = stem.to_os_string();
        name.push(".");
        name.push(ending);
        let path = folder.join(name);
        path.is_file().then_some(path)
    })
}

/// Where `line` of `source` went in `pdf`, by the SyncTeX file beside it.
///
/// # Errors
///
/// No SyncTeX file, one too large or not readable, no input that is the
/// source, two inputs it could be, or nothing at all from that file.
pub fn forward(pdf: &Path, source: &Path, line: u32) -> Result<Forward, String> {
    let map = beside(pdf).ok_or_else(|| {
        said!(
            "there is no SyncTeX file beside {path}; compile it with -synctex=1",
            path = pdf.display()
        )
    })?;
    let folder = map.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let inputs = inputs(&mut open(&map)?)?;
    let tags = source_tags(&inputs, &folder, source)?;
    scan(&mut open(&map)?, &tags, line)?.ok_or_else(|| {
        said!(
            "nothing on any page came from {name}",
            name = source.file_name().map_or_else(
                || source.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
        )
    })
}

/// The file's text, decompressed when it ends in `.gz`, held to the bounds.
fn open(map: &Path) -> Result<Box<dyn BufRead>, String> {
    let file = std::fs::File::open(map)
        .map_err(|e| said!("could not read {path}: {e}", path = map.display()))?;
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    if size > MAX_FILE {
        return Err(said!(
            "{path} is {size} MB, more than the {most} MB tpdf reads",
            path = map.display(),
            size = size / 1_048_576,
            most = MAX_FILE / 1_048_576
        ));
    }
    let raw = file.take(MAX_FILE);
    let text: Box<dyn Read> = if map.extension().is_some_and(|ext| ext == "gz") {
        Box::new(flate2::read::MultiGzDecoder::new(raw))
    } else {
        Box::new(raw)
    };
    Ok(Box::new(BufReader::new(text.take(MAX_TEXT + 1))))
}

/// Calls `each` with every line, held to [`MAX_LINE`] and [`MAX_TEXT`].
/// `each` returns false to stop.
fn lines(reader: &mut dyn BufRead, mut each: impl FnMut(&str) -> bool) -> Result<(), String> {
    let mut buffer = Vec::new();
    let mut total = 0u64;
    loop {
        buffer.clear();
        let read = (&mut *reader)
            .take(MAX_LINE + 1)
            .read_until(b'\n', &mut buffer)
            .map_err(|e| said!("the SyncTeX file could not be read: {e}"))?;
        if read == 0 {
            return Ok(());
        }
        total += read as u64;
        if total > MAX_TEXT {
            return Err(said!(
                "the SyncTeX file is more than {most} MB decompressed",
                most = MAX_TEXT / 1_048_576
            ));
        }
        if buffer.len() as u64 > MAX_LINE {
            return Err(said!("the SyncTeX file has a line longer than 64 KB"));
        }
        let text = String::from_utf8_lossy(&buffer);
        if !each(text.trim_end_matches(['\n', '\r'])) {
            return Ok(());
        }
    }
}

/// Every `Input:tag:path` line, in order.
fn inputs(reader: &mut dyn BufRead) -> Result<Vec<(u32, String)>, String> {
    let mut found = Vec::new();
    lines(reader, |line| {
        if let Some(rest) = line.strip_prefix("Input:") {
            if let Some((tag, path)) = rest.split_once(':') {
                if let Ok(tag) = tag.parse::<u32>() {
                    found.push((tag, path.to_string()));
                }
            }
        }
        true
    })?;
    Ok(found)
}

/// `path` with `.` left out and `..` taken back, without asking the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Whether `input`, a path the SyncTeX file chose, is the reader's `source`.
///
/// **The disk is asked about `source` alone.** `canonicalize` opens the path, and on
/// Windows `\\host\share\x` opens a connection to a host the file chose, in the
/// reader's name and from the unsandboxed app process. An input reached through a
/// link is then not found by path, and the file name in [`source_tags`] finds it.
fn same_file(input: &Path, source: &Path) -> bool {
    let input = lexical(input);
    input == lexical(source) || source.canonicalize().is_ok_and(|real| input == real)
}

/// The tags that are `source`: by path, or by file name when exactly one
/// input has it. See the module's last section.
fn source_tags(
    inputs: &[(u32, String)],
    folder: &Path,
    source: &Path,
) -> Result<BTreeSet<u32>, String> {
    let resolved = |path: &str| {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            folder.join(path)
        }
    };
    let by_path: BTreeSet<u32> = inputs
        .iter()
        .filter(|(_, path)| same_file(&resolved(path), source))
        .map(|(tag, _)| *tag)
        .collect();
    if !by_path.is_empty() {
        return Ok(by_path);
    }
    let name = source
        .file_name()
        .ok_or_else(|| said!("{path} names no file", path = source.display()))?;
    let named: Vec<&(u32, String)> = inputs
        .iter()
        .filter(|(_, path)| lexical(&resolved(path)).file_name() == Some(name))
        .collect();
    let paths: BTreeSet<PathBuf> = named
        .iter()
        .map(|(_, path)| lexical(&resolved(path)))
        .collect();
    match paths.len() {
        0 => Err(said!(
            "the SyncTeX file names no input called {name}",
            name = name.to_string_lossy()
        )),
        1 => Ok(named.iter().map(|(tag, _)| *tag).collect()),
        _ => Err(said!(
            "the SyncTeX file names {count} different inputs called {name}; give the source's \
             full path",
            count = paths.len(),
            name = name.to_string_lossy()
        )),
    }
}

/// A box or point record: `tag,line[,column]:x,y[:W[,H,D]]`.
struct Record {
    tag: u32,
    line: u32,
    x: i64,
    y: i64,
    size: Option<(i64, i64, i64)>,
}

fn record(text: &str) -> Option<Record> {
    let mut parts = text.split(':');
    let mut place = parts.next()?.split(',');
    let tag = place.next()?.trim().parse().ok()?;
    let line = place.next()?.trim().parse().ok()?;
    let mut at = parts.next()?.split(',');
    let x = at.next()?.trim().parse().ok()?;
    let y = at.next()?.trim().parse().ok()?;
    let size = parts.next().and_then(|dims| {
        let mut dims = dims.split(',').map(|n| n.trim().parse::<i64>().ok());
        let w = dims.next()??;
        let h = dims.next().flatten().unwrap_or(0);
        let d = dims.next().flatten().unwrap_or(0);
        Some((w, h, d))
    });
    Some(Record {
        tag,
        line,
        x,
        y,
        size,
    })
}

/// How scaled points become points.
#[derive(Clone, Copy)]
struct Scale {
    unit: f64,
    magnification: f64,
    x_offset: f64,
    y_offset: f64,
}

impl Scale {
    fn x(&self, sp: i64) -> f64 {
        (sp as f64 * self.unit * self.magnification / 1000.0 + self.x_offset) / SP_PER_BP
    }
    fn y(&self, sp: i64) -> f64 {
        (sp as f64 * self.unit * self.magnification / 1000.0 + self.y_offset) / SP_PER_BP
    }
    fn length(&self, sp: i64) -> f64 {
        sp as f64 * self.unit * self.magnification / 1000.0 / SP_PER_BP
    }
    /// A box's rectangle, or `None` for one with no area.
    fn rect(&self, record: &Record) -> Option<Rect> {
        let (w, h, d) = record.size?;
        if w == 0 || h + d <= 0 {
            return None;
        }
        let left = self.x(record.x).min(self.x(record.x) + self.length(w));
        Some(Rect {
            x: left,
            top: self.y(record.y) - self.length(h),
            width: self.length(w).abs(),
            height: self.length(h + d),
        })
    }
}

/// The boxes kept for one line.
#[derive(Default)]
struct Bucket {
    line: Option<u32>,
    page: u32,
    boxes: Vec<Rect>,
}

impl Bucket {
    /// Keeps `rect` on `page` for `line` when that line is the one this bucket
    /// is collecting, or one closer to the asked line than it (`better`).
    fn offer(&mut self, line: u32, page: u32, rect: Rect, better: impl Fn(u32, u32) -> bool) {
        match self.line {
            Some(held) if held == line => {}
            Some(held) if !better(line, held) => return,
            _ => {
                self.line = Some(line);
                self.page = page;
                self.boxes.clear();
            }
        }
        // Only the first page the line is on, and each box once in a row:
        // every word of a line names the same hbox.
        if page != self.page || self.boxes.last() == Some(&rect) || self.boxes.len() >= MAX_BOXES {
            return;
        }
        self.boxes.push(rect);
    }
}

/// The boxes `line` of the inputs `tags` made, or the nearest line's.
fn scan(
    reader: &mut dyn BufRead,
    tags: &BTreeSet<u32>,
    wanted: u32,
) -> Result<Option<Forward>, String> {
    let mut scale = Scale {
        unit: 1.0,
        magnification: 1000.0,
        x_offset: 0.0,
        y_offset: 0.0,
    };
    let mut page = 0u32;
    // One entry per open box: the hbox's rectangle, or `None` for a vbox.
    let mut open: Vec<Option<Rect>> = Vec::new();
    let mut after = Bucket::default();
    let mut before = Bucket::default();
    let mut deep = false;
    lines(reader, |line| {
        let Some(kind) = line.chars().next() else {
            return true;
        };
        let rest = &line[kind.len_utf8()..];
        match kind {
            '{' => page = rest.trim().parse().unwrap_or(page),
            // The asked line found, and its page finished: nothing later is kept.
            '}' => return !(after.line == Some(wanted) && after.page == page),
            '(' | '[' => {
                if open.len() >= MAX_DEPTH {
                    deep = true;
                    return false;
                }
                let found = record(rest);
                let rect = if kind == '(' {
                    found.as_ref().and_then(|r| scale.rect(r))
                } else {
                    None
                };
                open.push(rect);
                if let (Some(found), Some(rect), '(') = (found, rect, kind) {
                    if tags.contains(&found.tag) {
                        keep(&mut after, &mut before, wanted, found.line, page, rect);
                    }
                }
            }
            ')' | ']' => {
                open.pop();
            }
            'v' | 'h' | 'x' | 'k' | 'g' | '$' => {
                let Some(found) = record(rest) else {
                    return true;
                };
                if !tags.contains(&found.tag) {
                    return true;
                }
                // A void box with an area is marked as it is; anything else,
                // by the hbox it is in.
                let own = if matches!(kind, 'v' | 'h') {
                    scale.rect(&found)
                } else {
                    None
                };
                let enclosing = open.iter().rev().find_map(|rect| *rect);
                if let Some(rect) = own.or(enclosing) {
                    keep(&mut after, &mut before, wanted, found.line, page, rect);
                }
            }
            _ => {
                let number = |prefix: &str| {
                    line.strip_prefix(prefix)
                        .and_then(|n| n.trim().parse::<f64>().ok())
                };
                if let Some(unit) = number("Unit:") {
                    scale.unit = unit;
                } else if let Some(magnification) = number("Magnification:") {
                    scale.magnification = magnification;
                } else if let Some(offset) = number("X Offset:") {
                    scale.x_offset = offset;
                } else if let Some(offset) = number("Y Offset:") {
                    scale.y_offset = offset;
                }
            }
        }
        true
    })?;
    if deep {
        return Err(said!(
            "the SyncTeX file nests boxes more than {MAX_DEPTH} deep"
        ));
    }
    let chosen = if after.line.is_some() { after } else { before };
    Ok(chosen.line.map(|line| Forward {
        page: chosen.page,
        line,
        boxes: chosen.boxes,
    }))
}

/// Offers a box to the bucket its line belongs in: the asked line or after
/// it, nearest first; or before it, nearest first.
fn keep(after: &mut Bucket, before: &mut Bucket, wanted: u32, line: u32, page: u32, rect: Rect) {
    if line >= wanted {
        after.offer(line, page, rect, |new, held| new < held);
    } else {
        before.offer(line, page, rect, |new, held| new > held);
    }
}

#[cfg(test)]
mod tests {
    use super::{beside, forward, inputs, lexical, scan, source_tags, Rect, MAX_LINE};
    use std::collections::BTreeSet;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("testdata")
            .join("synctex")
            .join(name)
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn a_line_of_the_main_file_is_the_box_its_words_are_in() {
        // `First paragraph, on line three`: words `x1,3`, in the hbox `(1,4:`
        // at 8799518,8865054 sp, 22609920 wide, 455111 high, 127431 deep.
        let found = forward(&fixture("main.pdf"), &fixture("main.tex"), 3).expect("found");
        assert_eq!(
            (found.page, found.line, found.boxes.len()),
            (1, 3, 1),
            "{found:?}"
        );
        let rect = found.boxes[0];
        assert!(near(rect.x, 8_799_518.0 / 65_781.76), "{rect:?}");
        assert!(
            near(rect.top, (8_865_054.0 - 455_111.0) / 65_781.76),
            "{rect:?}"
        );
        assert!(near(rect.width, 22_609_920.0 / 65_781.76), "{rect:?}");
        assert!(
            near(rect.height, (455_111.0 + 127_431.0) / 65_781.76),
            "{rect:?}"
        );
    }

    #[test]
    fn a_line_of_an_input_file_is_found_by_its_own_tag() {
        // chapter.tex line 3 is the third line of page 1, at 10437918 sp.
        let found = forward(&fixture("main.pdf"), &fixture("chapter.tex"), 3).expect("found");
        assert_eq!(
            (found.page, found.line, found.boxes.len()),
            (1, 3, 1),
            "{found:?}"
        );
        assert!(near(
            found.boxes[0].top,
            (10_437_918.0 - 455_111.0) / 65_781.76
        ));
        // And line 1 is the second line, not the main file's line 1.
        let first = forward(&fixture("main.pdf"), &fixture("chapter.tex"), 1).expect("found");
        assert!(
            near(first.boxes[0].top, (9_651_486.0 - 455_111.0) / 65_781.76),
            "{first:?}"
        );
    }

    #[test]
    fn a_line_on_the_second_page_is_on_page_two() {
        let found = forward(&fixture("main.pdf"), &fixture("main.tex"), 8).expect("found");
        assert_eq!((found.page, found.line), (2, 8), "{found:?}");
    }

    #[test]
    fn a_line_that_made_nothing_is_answered_with_the_next_that_did() {
        // main.tex's records name lines 3, 4, 6, 7, 8 and 9. Line 5 is the
        // `\input`, which made nothing of its own; a blank line can still
        // make something, as the line a paragraph ended on.
        let found = forward(&fixture("main.pdf"), &fixture("main.tex"), 5).expect("found");
        assert_eq!(found.line, 6, "{found:?}");
        let first = forward(&fixture("main.pdf"), &fixture("main.tex"), 1).expect("found");
        assert_eq!(first.line, 3, "{first:?}");
        // Past the end, the last line that made something.
        let past = forward(&fixture("main.pdf"), &fixture("main.tex"), 12).expect("found");
        assert_eq!((past.line, past.page), (9, 2), "{past:?}");
    }

    #[test]
    fn the_source_is_found_by_name_when_the_project_was_compiled_elsewhere() {
        // The fixture names `/tmp/stx/./chapter.tex`, the folder it was
        // compiled in; the source is here.
        let map = std::fs::File::open(fixture("main.synctex.gz")).expect("map");
        let listed = inputs(&mut std::io::BufReader::new(
            flate2::read::MultiGzDecoder::new(map),
        ))
        .expect("inputs");
        assert!(
            listed
                .iter()
                .any(|(tag, path)| *tag == 5 && path == "/tmp/stx/./chapter.tex"),
            "{listed:?}"
        );
        let tags =
            source_tags(&listed, Path::new("/elsewhere"), &fixture("chapter.tex")).expect("tags");
        assert_eq!(tags, BTreeSet::from([5]));
    }

    #[test]
    fn two_inputs_with_the_source_name_are_refused_not_guessed() {
        let listed = vec![
            (1, "/a/intro.tex".to_string()),
            (2, "/b/intro.tex".to_string()),
        ];
        let refused = source_tags(&listed, Path::new("/"), Path::new("/c/intro.tex")).unwrap_err();
        assert!(
            refused.contains("2 different inputs called intro.tex"),
            "{refused}"
        );
        // One file named twice, `./` or not, is one input.
        let once = vec![
            (1, "/a/./intro.tex".to_string()),
            (2, "/a/intro.tex".to_string()),
        ];
        assert_eq!(
            source_tags(&once, Path::new("/"), Path::new("/c/intro.tex")).expect("tags"),
            BTreeSet::from([1, 2])
        );
        // A path that is the source wins over a name.
        let by_path = vec![
            (1, "intro.tex".to_string()),
            (2, "/b/intro.tex".to_string()),
        ];
        assert_eq!(
            source_tags(&by_path, Path::new("/proj"), Path::new("/proj/intro.tex")).expect("tags"),
            BTreeSet::from([1])
        );
        assert!(
            source_tags(&listed, Path::new("/"), Path::new("/c/other.tex"))
                .unwrap_err()
                .contains("no input called other.tex")
        );
    }

    #[test]
    fn a_path_is_compared_without_its_dots() {
        assert_eq!(
            lexical(Path::new("/tmp/stx/./a/../main.tex")),
            PathBuf::from("/tmp/stx/main.tex")
        );
    }

    fn scanned(text: &str, line: u32) -> Option<super::Forward> {
        scan(
            &mut Cursor::new(text.as_bytes().to_vec()),
            &BTreeSet::from([1]),
            line,
        )
        .expect("scan")
    }

    #[test]
    fn a_vbox_is_not_marked_and_a_point_outside_any_hbox_is_not_either() {
        let text = "{1\n[1,5:0,0:100,100,0\nx1,5:10,10\n]\n}1\n";
        assert_eq!(scanned(text, 5), None);
    }

    #[test]
    fn an_hbox_naming_the_line_itself_is_marked_and_a_point_by_its_hbox() {
        let text = "Unit:65781.76\n{1\n(1,7:10,20:30,5,1\n)\n(1,9:0,40:50,6,2\nx1,8:3,40\n)\n}1\n";
        let own = scanned(text, 7).expect("found");
        assert_eq!(
            own.boxes,
            vec![Rect {
                x: 10.0,
                top: 15.0,
                width: 30.0,
                height: 6.0
            }]
        );
        let point = scanned(text, 8).expect("found");
        assert_eq!(
            point.boxes,
            vec![Rect {
                x: 0.0,
                top: 34.0,
                width: 50.0,
                height: 8.0
            }]
        );
    }

    #[test]
    fn only_the_first_page_with_the_line_is_marked() {
        // Line 3 on both pages, in boxes of different sizes. Asked for line 2,
        // which made nothing, the answer is line 3: the scan does not stop at
        // the end of page 1 for a line that is not the one asked for, so the
        // rule that keeps only the first page is what holds page 2 back.
        let text = "Unit:65781.76\n{1\n(1,3:0,10:5,1,0\n)\n}1\n{2\n(1,3:0,20:7,2,0\n)\n}2\n";
        let found = scanned(text, 2).expect("found");
        assert_eq!((found.page, found.line), (1, 3));
        assert_eq!(
            found.boxes,
            vec![Rect {
                x: 0.0,
                top: 9.0,
                width: 5.0,
                height: 1.0
            }]
        );
    }

    #[test]
    fn the_scale_and_offsets_in_the_header_are_applied() {
        let text = "Unit:2\nMagnification:2000\nX Offset:65781.76\nY Offset:0\n{1\n(1,1:65781.76,0:0,0,0\n)\n(1,1:16445,16445:16445,16445,0\n)\n}1\n";
        let found = scanned(text, 1).expect("found");
        // 16445 sp at unit 2 and magnification 2 is 65780 sp, about one point,
        // and the x offset adds one point more.
        let rect = found.boxes[0];
        assert!(
            near(rect.x, 2.0) && near(rect.width, 1.0) && near(rect.top, 0.0),
            "{rect:?}"
        );
    }

    #[test]
    fn a_line_longer_than_the_limit_is_refused() {
        let long = format!("Input:1:{}\n", "a".repeat(MAX_LINE as usize));
        let refused = inputs(&mut Cursor::new(long.into_bytes())).unwrap_err();
        assert!(refused.contains("longer than 64 KB"), "{refused}");
    }

    #[test]
    fn boxes_nested_too_deep_are_refused() {
        let text = "{1\n".to_string() + &"(1,1:0,0:1,1,0\n".repeat(super::MAX_DEPTH + 1);
        let refused =
            scan(&mut Cursor::new(text.into_bytes()), &BTreeSet::from([1]), 1).unwrap_err();
        assert!(refused.contains("more than 100000 deep"), "{refused}");
    }

    #[test]
    fn no_synctex_file_beside_the_pdf_says_how_to_make_one() {
        assert_eq!(
            beside(&fixture("main.pdf")),
            Some(fixture("main.synctex.gz"))
        );
        let refused = forward(&fixture("chapter.pdf"), &fixture("chapter.tex"), 1).unwrap_err();
        assert!(refused.contains("-synctex=1"), "{refused}");
    }

    /// A link to the source is not the source by path, because finding that out
    /// would open a path the file chose.
    #[cfg(unix)]
    #[test]
    fn a_path_in_the_map_is_not_looked_up_on_the_disk() {
        use super::same_file;
        let dir = std::env::temp_dir().join(format!("tpdf-synctex-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let source = dir.join("main.tex");
        std::fs::write(&source, b"x").expect("the source");
        let link = dir.join("link.tex");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&source, &link).expect("a link");
        assert!(same_file(&source, &source), "the control");
        assert!(same_file(&dir.join("./main.tex"), &source), "by path");
        assert!(
            same_file(&source.canonicalize().expect("real"), &source),
            "the source's real path"
        );
        assert!(!same_file(&link, &source));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
