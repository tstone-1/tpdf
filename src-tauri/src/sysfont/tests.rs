//! The app process's half: reading the file the lookup names, and the one
//! round trip that supplies it to the worker. The lookup itself is replaced by
//! a closure naming a generated fixture, so nothing here depends on what the
//! machine has installed; `textedit::fonts::installed::tests` has the two
//! platform-gated checks against real system fonts.
use super::*;
use crate::textedit::{preview_layout, Layout, PageRuns};
use std::path::Path;

const NAME: &str = "TPDFInstalledSans";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/textedit/fonts/installed/full.ttf")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpdf-sysfont-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("font.ttf")
}

#[test]
fn a_found_font_is_read_whole_up_to_the_bound_and_refused_past_it() {
    let found = find_with(NAME, |name| {
        assert_eq!(name, NAME);
        Some((fixture(), Some(3)))
    })
    .unwrap();
    assert_eq!(found.bytes, std::fs::read(fixture()).unwrap());
    assert_eq!(found.index, Some(3));

    let path = scratch("bound");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(crate::textedit::MAX_INSTALLED as u64).unwrap();
    let at = find_with(NAME, |_| Some((path.clone(), None))).unwrap();
    assert_eq!(at.bytes.len(), crate::textedit::MAX_INSTALLED);
    file.set_len(crate::textedit::MAX_INSTALLED as u64 + 1)
        .unwrap();
    assert!(find_with(NAME, |_| Some((path.clone(), None))).is_none());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();

    assert!(find_with(NAME, |_| None).is_none());
    assert!(find_with(NAME, |_| Some((PathBuf::from("/no/such/font.ttf"), None))).is_none());
}

#[test]
fn a_name_that_cannot_be_a_postscript_name_is_never_looked_up() {
    for name in ["", "Two Words", "Bad(Name)", "Slash/Name", &"N".repeat(64)] {
        assert!(find_with(name, |_| panic!("looked up {name:?}")).is_none());
    }
}

/// The page the worker tests use: a document subset of the fixture font.
fn document() -> lopdf::Document {
    let bytes = include_bytes!("../textedit/fonts/installed/subset.ttf");
    let face = ttf_parser::Face::parse(bytes, 0).unwrap();
    let mut doc = crate::textedit::tests::fixture();
    let page = crate::pagetree::ordered_pages(&doc)[0];
    let content = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        b"BT /F1 12 Tf 40 180 Td (TITLE) Tj ET".to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", content);
    let widths = (32_u8..=84)
        .map(|code| {
            let width = face
                .glyph_index(char::from(code))
                .and_then(|id| face.glyph_hor_advance(id))
                .map_or(0., |advance| f64::from(advance) * 1000. / 2048.);
            lopdf::Object::Real(width as f32)
        })
        .collect::<Vec<_>>();
    let program = doc.add_object(lopdf::Stream::new(
        lopdf::dictionary! { "Length1" => bytes.len() as i64 },
        bytes.to_vec(),
    ));
    let descriptor = doc.add_object(lopdf::dictionary! {
        "Type" => "FontDescriptor", "FontName" => "ABCDEF+TPDFInstalledSans", "Flags" => 32,
        "FontBBox" => vec![0.into(), 0.into(), 1000.into(), 700.into()],
        "ItalicAngle" => 0, "Ascent" => 879, "Descent" => -195,
        "CapHeight" => 700, "StemV" => 80, "FontFile2" => program
    });
    let font = doc.add_object(lopdf::dictionary! {
        "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+TPDFInstalledSans",
        "Encoding" => "WinAnsiEncoding", "FirstChar" => 32, "LastChar" => 84,
        "Widths" => widths, "FontDescriptor" => descriptor
    });
    for page in crate::pagetree::ordered_pages(&doc) {
        doc.get_dictionary_mut(page).unwrap().set(
            "Resources",
            lopdf::dictionary! { "Font" => lopdf::dictionary! { "F1" => font } },
        );
    }
    doc
}

fn draft(doc: &lopdf::Document, font: EditFont) -> Change {
    let page = crate::textedit::scan(doc, 0).unwrap();
    Change {
        page: 0,
        revision: page.revision,
        operator: page.runs[0].operator,
        original: page.runs[0].text.clone(),
        replacement: "TITLE Ab".into(),
        layout: Some(Layout {
            width: 250.,
            height: 20.,
            size: 12.,
            wrap: false,
            font,
            grow: false,
            installed: None,
        }),
    }
}

fn runs(doc: &lopdf::Document, change: &Change) -> PageRuns {
    PageRuns {
        preview: Some(preview_layout(doc, change).unwrap()),
        ..PageRuns::default()
    }
}

fn from_fixture(name: &str) -> Option<Found> {
    find_with(name, |_| Some((fixture(), None)))
}

/// The round trip `commands::read::runs_with_installed` makes, against the
/// real worker code: ask, supply what the worker named, ask again, and keep
/// the subset for the journal with nothing app-only left for the webview.
#[test]
fn the_name_the_worker_wants_is_supplied_once_and_the_subset_kept() {
    let doc = document();
    let mut change = draft(&doc, EditFont::Auto);
    let first = runs(&doc, &change);
    assert_eq!(first.preview.as_ref().unwrap().font, "Noto Sans");
    let mut asked = None;
    assert!(supply(&mut change, &first, |name| {
        asked = Some(name.to_owned());
        from_fixture(name)
    }));
    assert_eq!(asked.as_deref(), Some(NAME));
    let mut second = runs(&doc, &change);
    assert_eq!(
        second.preview.as_ref().unwrap().font,
        "TPDF Installed Sans (installed)"
    );
    // Supplied now, so it is not asked for twice.
    assert!(!supply(&mut change, &second, |_| panic!("asked again")));
    let kept = take(&mut second).expect("the subset");
    assert!(kept.program.len() < std::fs::read(fixture()).unwrap().len());
    let preview = second.preview.as_ref().unwrap();
    assert_eq!((&preview.wants, &preview.installed), (&None, &None));
    // Nothing app-only reaches the webview's JSON.
    let json = serde_json::to_string(&second).unwrap();
    assert!(
        !json.contains("\"wants\"") && !json.contains("\"installed\""),
        "{json}"
    );
}

#[test]
fn nothing_is_supplied_unless_automatic_mode_asked_and_the_font_is_found() {
    let doc = document();
    let auto = draft(&doc, EditFont::Auto);
    let wanted = runs(&doc, &auto);
    // Not found: nothing supplied, and the reply stands.
    let mut change = auto.clone();
    assert!(!supply(&mut change, &wanted, |_| None));
    assert_eq!(change, auto);
    // Another font mode: the worker asked for nothing, and a reply that did
    // ask is not acted on for a draft that is not automatic.
    let mut noto = draft(&doc, EditFont::NotoSans);
    let plain = runs(&doc, &noto);
    assert!(plain.preview.as_ref().unwrap().wants.is_none());
    assert!(!supply(&mut noto, &wanted, |_| panic!("looked up")));
    // No draft layout, or no preview: nothing to supply.
    let mut bare = auto.clone();
    bare.layout = None;
    assert!(!supply(&mut bare, &wanted, |_| panic!("looked up")));
    let mut change = auto.clone();
    assert!(!supply(&mut change, &PageRuns::default(), |_| panic!(
        "looked up"
    )));
}

#[test]
fn a_change_from_the_webview_loses_any_installed_font_it_names() {
    let doc = document();
    let mut change = draft(&doc, EditFont::Auto);
    change.layout.as_mut().unwrap().installed = Some(crate::textedit::Installed {
        name: NAME.into(),
        index: None,
        program: vec![1, 2, 3],
    });
    sanitise(&mut change);
    assert_eq!(change, draft(&doc, EditFont::Auto));
}

/// A worker, as far as these requests go: the preview of the last change
/// with a layout, from the real writer (`render::text_edit_runs` picks the
/// same one). Counts the requests.
fn worker<'a>(
    doc: &'a lopdf::Document,
    asked: &'a std::cell::Cell<usize>,
) -> impl Fn(Vec<Change>) -> std::future::Ready<Result<PageRuns, String>> + 'a {
    move |changes: Vec<Change>| {
        asked.set(asked.get() + 1);
        std::future::ready((|| {
            let preview = match changes.iter().rev().find(|change| change.layout.is_some()) {
                Some(change) => Some(preview_layout(doc, change)?),
                None => None,
            };
            Ok(PageRuns {
                preview,
                ..PageRuns::default()
            })
        })())
    }
}

/// What `text_replace` journals: the subset, not the file, and not whatever
/// the webview put in the change; and the journalled change, sent again the
/// way a tile or the save sends it, sets the same text in the same font
/// without a lookup.
#[test]
fn the_journal_keeps_the_subset_the_worker_built_and_it_needs_no_lookup_again() {
    let doc = document();
    let asked = std::cell::Cell::new(0);
    let mut change = draft(&doc, EditFont::Auto);
    change.layout.as_mut().unwrap().installed = Some(crate::textedit::Installed {
        name: NAME.into(),
        index: None,
        program: vec![0; 16],
    });
    let recorded = tauri::async_runtime::block_on(replacement(
        change,
        Vec::new(),
        worker(&doc, &asked),
        from_fixture,
    ))
    .unwrap();
    assert_eq!(asked.get(), 2, "asked, supplied, asked again");
    let kept = recorded
        .layout
        .as_ref()
        .unwrap()
        .installed
        .as_ref()
        .expect("the subset");
    let file = std::fs::read(fixture()).unwrap();
    assert!(kept.program.len() < file.len() && kept.program != vec![0; 16]);
    asked.set(0);
    let (runs, again) = tauri::async_runtime::block_on(ask_with_installed(
        vec![recorded.clone()],
        worker(&doc, &asked),
        |_| panic!("the journal's subset needs no lookup"),
    ))
    .unwrap();
    assert_eq!(asked.get(), 1);
    assert_eq!(
        runs.preview.unwrap().font,
        "TPDF Installed Sans (installed)"
    );
    assert_eq!(again.as_ref(), Some(kept));
    // Not installed on this computer: the reply that named the font reaches
    // the webview without the name, the draft is journalled without a font,
    // and it is set in Noto.
    asked.set(0);
    let (runs, none) = tauri::async_runtime::block_on(ask_with_installed(
        vec![draft(&doc, EditFont::Auto)],
        worker(&doc, &asked),
        |name| {
            assert_eq!(name, NAME, "the worker named it");
            None
        },
    ))
    .unwrap();
    assert_eq!((none, asked.get()), (None, 1));
    let json = serde_json::to_string(&runs).unwrap();
    assert!(!json.contains("\"wants\""), "{json}");
    asked.set(0);
    let plain = tauri::async_runtime::block_on(replacement(
        draft(&doc, EditFont::Auto),
        Vec::new(),
        worker(&doc, &asked),
        |_| None,
    ))
    .unwrap();
    assert_eq!(plain.layout.unwrap().installed, None);
    assert_eq!(asked.get(), 1);
}

#[test]
fn a_batch_replaces_the_runs_earlier_edit_and_leaves_out_one_that_restores_it() {
    let doc = document();
    let mut earlier = draft(&doc, EditFont::Auto);
    earlier.replacement = "TITLE".into();
    let mut named = draft(&doc, EditFont::Auto);
    named.layout.as_mut().unwrap().installed = Some(crate::textedit::Installed {
        name: NAME.into(),
        index: None,
        program: vec![1],
    });
    let (changes, drafted) = batch(vec![earlier.clone()], named);
    assert!(drafted);
    assert_eq!(changes, vec![draft(&doc, EditFont::Auto)]);
    let mut restore = draft(&doc, EditFont::Auto);
    restore.replacement = restore.original.clone();
    restore.layout = None;
    let (changes, drafted) = batch(vec![earlier], restore);
    assert!(!drafted && changes.is_empty());
}
