//! `tpdf redact`'s rules that need no worker: the command line, the regions
//! file, the signature refusal, the walk over pages, the regions a hit becomes,
//! and what is printed. The command against real documents is `tests/cli.rs`.

use super::fill::SignedState;
use super::redact::{
    match_regions, outcome, parse, plain, region_list, search_pages, signed_refusal, still_found,
    unmarked, Found, Redact,
};
use super::report::{self, RedactedPage, SearchKind};
use super::Exit;
use crate::search::{Options, Prepared};
use crate::text::PageText;

fn argv(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

fn parsed(line: &str) -> Redact {
    parse(&argv(line)).unwrap_or_else(|why| panic!("`{line}` should parse: {why}"))
}

fn refused(line: &str) -> String {
    match parse(&argv(line)) {
        Ok(command) => panic!("`{line}` should be refused, and parsed as {command:?}"),
        Err(why) => why,
    }
}

#[test]
fn a_whole_redaction_line_parses_into_what_is_run() {
    let command = parsed(
        "in.pdf -o out.pdf --text Secret --text Other --pattern [0-9]+ --regions r.json \
         --case-sensitive --pages 2-3 --invalidate-signatures --password-env PW --force --json",
    );
    assert_eq!(command.input.to_str(), Some("in.pdf"));
    assert_eq!(
        command.output.as_deref().and_then(|p| p.to_str()),
        Some("out.pdf")
    );
    assert_eq!(command.texts, ["Secret", "Other"]);
    assert_eq!(command.patterns, ["[0-9]+"]);
    assert_eq!(command.regions.len(), 1);
    assert!(command.case_sensitive && command.invalidate_signatures);
    assert!(command.force && command.json && !command.dry_run);
    assert_eq!(command.pages, Some(vec![2, 3]));
    assert_eq!(command.password_env.as_deref(), Some("PW"));
}

#[test]
fn a_dry_run_needs_no_output_and_a_real_run_does() {
    assert_eq!(parsed("in.pdf --dry-run --text a").output, None);
    assert!(refused("in.pdf --text a").contains("-o <out.pdf>"));
    assert!(refused("in.pdf --dry-run --force --text a").contains("there is no `-o`"));
}

#[test]
fn nothing_to_remove_is_a_malformed_line() {
    assert!(refused("in.pdf -o out.pdf").contains("something to remove"));
}

#[test]
fn an_output_that_names_the_input_or_a_regions_file_is_refused() {
    assert!(refused("in.pdf -o ./in.pdf --text a").contains("names the input"));
    assert!(refused("in.pdf -o r.json --regions r.json").contains("regions file"));
}

#[test]
fn a_pattern_that_does_not_compile_is_refused_with_the_find_bars_reason() {
    let why = refused("in.pdf -o out.pdf --pattern (unclosed");
    assert!(why.starts_with("`--pattern (unclosed`:"), "{why}");
}

#[test]
fn a_query_that_can_match_nothing_is_refused_rather_than_run() {
    // A literal of only whitespace: the fold collapses it, and a search that
    // cannot match reads exactly like a clean document.
    let whitespace = vec![
        "in.pdf".to_string(),
        "-o".into(),
        "out.pdf".into(),
        "--text".into(),
        "   ".into(),
    ];
    assert!(parse(&whitespace)
        .expect_err("refused")
        .contains("can match nothing"));
    let empty = vec![
        "in.pdf".to_string(),
        "-o".into(),
        "out.pdf".into(),
        "--pattern".into(),
        String::new(),
    ];
    assert!(parse(&empty)
        .expect_err("refused")
        .contains("can match nothing"));
}

#[test]
fn every_other_malformed_redaction_line_is_refused_with_its_reason() {
    for (line, says) in [
        ("-o out.pdf --text a", "needs the document"),
        ("a.pdf b.pdf -o out.pdf --text a", "is a second"),
        (
            "in.pdf -o out.pdf --text a --whole-word",
            "has no option `--whole-word`",
        ),
        ("in.pdf -o out.pdf --text a --pages 3-1", "runs backwards"),
        ("in.pdf -o out.pdf --text", "needs a value"),
        (
            "in.pdf -o out.pdf --text a --password-env A=B",
            "cannot be one",
        ),
    ] {
        let why = refused(line);
        assert!(why.contains(says), "`{line}`: {why}");
    }
}

#[test]
fn a_regions_file_is_read_as_sign_rect_reads_a_rectangle() {
    let regions = region_list(
        r#"[{"page": 2, "rect": [72, 600, 220.5, 70]}, {"page": 1, "rect": [0, 0, 1, 1]}]"#,
        "r.json",
    )
    .expect("parses");
    assert_eq!(
        regions,
        vec![(2, [72.0, 600.0, 292.5, 670.0]), (1, [0.0, 0.0, 1.0, 1.0])]
    );
}

#[test]
fn a_regions_file_that_is_not_one_is_refused_with_where() {
    for (text, says) in [
        (r#"{"page": 1}"#, "not a JSON array"),
        (r#"[{"page": 1, "rect": [0, 0, 1]}]"#, "not a JSON array"),
        (
            r#"[{"page": 1, "rect": [0, 0, 1, 1], "why": "x"}]"#,
            "not a JSON array",
        ),
        (r#"[{"page": 0, "rect": [0, 0, 1, 1]}]"#, "region 1: page 0"),
        (
            r#"[{"page": 1, "rect": [0, 0, 1, 1]}, {"page": 1, "rect": [0, 0, 0, 1]}]"#,
            "region 2:",
        ),
        (r#"[{"page": 1, "rect": [-1, 0, 1, 1]}]"#, "region 1:"),
    ] {
        let why = region_list(text, "r.json").expect_err(text);
        assert!(why.contains(says), "{text}: {why}");
    }
}

#[test]
fn a_signed_document_is_refused_unless_invalidating_is_asked_for() {
    let signed = signed_refusal("a.pdf", Some(SignedState::Signed(2)), false).expect("refused");
    assert!(signed.contains("2 signatures") && signed.contains("--invalidate-signatures"));
    assert!(signed_refusal("a.pdf", Some(SignedState::Unknown), false).is_some());
    assert_eq!(
        signed_refusal("a.pdf", Some(SignedState::Signed(1)), true),
        None
    );
    assert_eq!(
        signed_refusal("a.pdf", Some(SignedState::Unknown), true),
        None
    );
    assert_eq!(signed_refusal("a.pdf", None, false), None);
}

/// A page of one line per string, each character a 6 x 10 pt box.
fn page(lines: &[&str]) -> PageText {
    let mut codes = Vec::new();
    let mut boxes = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        let top = 100.0 + 20.0 * row as f32;
        for (at, ch) in line.chars().enumerate() {
            let left = 72.0 + 6.0 * at as f32;
            codes.push(u32::from(ch));
            boxes.extend([left, top, left + 6.0, top + 10.0]);
        }
        codes.push(u32::from('\n'));
        boxes.extend([0.0; 4]);
    }
    PageText {
        codes,
        boxes,
        width_pt: 600.0,
        height_pt: 800.0,
        ..PageText::default()
    }
}

fn query(kind: SearchKind, text: &str) -> (SearchKind, String, Prepared) {
    let options = Options {
        regex: kind == SearchKind::Pattern,
        ..Options::default()
    };
    (
        kind,
        text.to_string(),
        Prepared::new(text, options).expect("compiles"),
    )
}

fn walk(queries: &[(SearchKind, String, Prepared)], pages: &[u32], texts: &[PageText]) -> Found {
    search_pages(queries, pages, |at| {
        texts
            .get(at as usize)
            .cloned()
            .ok_or_else(|| format!("no page {at}"))
    })
    .expect("every page reads")
}

#[test]
fn each_query_is_counted_on_its_own_and_case_is_folded() {
    let texts = [page(&["mail jane@example.com", "JANE again"])];
    let queries = [
        query(SearchKind::Text, "jane"),
        query(SearchKind::Pattern, r"[a-z]+@[a-z]+\.com"),
    ];
    let found = walk(&queries, &[1], &texts);
    assert_eq!(found.counts, vec![2, 1]);
    assert_eq!(found.by, vec![0, 0, 1]);
}

#[test]
fn a_hit_over_a_page_break_is_found_between_neighbours_only() {
    // Page 3 starts as page 2 does, so a tail carried from page 1 to page 3
    // would find the phrase there too.
    let texts = [
        page(&["ends with Rumpel"]),
        page(&["stilzchen starts"]),
        page(&["stilzchen again"]),
    ];
    let queries = [query(SearchKind::Text, "Rumpel stilzchen")];
    let joined = walk(&queries, &[1, 2], &texts);
    assert_eq!(joined.counts, vec![1]);
    assert_eq!(joined.matches[0].end_page, Some(1));
    // Both pages' text is kept, so both halves can be marked.
    assert!(joined.texts.contains_key(&0) && joined.texts.contains_key(&1));
    // `--pages 1,3`: page 1 does not run into page 3.
    let apart = walk(&queries, &[1, 3], &texts);
    assert_eq!(apart.counts, vec![0]);
}

#[test]
fn only_the_pages_a_hit_touches_are_kept() {
    let texts = [page(&["nothing here"]), page(&["Secret here"])];
    let found = walk(&[query(SearchKind::Text, "Secret")], &[1, 2], &texts);
    assert_eq!(found.texts.keys().copied().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn a_page_that_cannot_be_read_stops_the_walk() {
    let queries = [query(SearchKind::Text, "a")];
    let why = search_pages(&queries, &[1, 2], |at| {
        if at == 1 {
            Err("damaged".into())
        } else {
            Ok(page(&["a"]))
        }
    })
    .expect_err("stops");
    assert!(why.contains("page 2") && why.contains("damaged"), "{why}");
}

#[test]
fn a_hit_becomes_one_region_per_line_it_runs_on() {
    let texts = [page(&["one Secret", "phrase two"]), page(&["Rest end"])];
    let queries = [
        query(SearchKind::Text, "Secret phrase"),
        query(SearchKind::Text, "two Rest"),
    ];
    let found = walk(&queries, &[1, 2], &texts);
    let marked = match_regions(&found);
    // "Secret phrase" runs over two lines of page 1; "two Rest" over the break.
    assert_eq!(marked[&0].len(), 3, "{marked:?}");
    assert_eq!(marked[&1].len(), 1, "{marked:?}");
    // The first region is "Secret" on the first line: characters 4..10.
    assert_eq!(marked[&0][0], [96.0, 100.0, 132.0, 110.0]);
}

#[test]
fn a_hit_still_in_the_written_file_is_a_reason_naming_its_page_and_query() {
    let texts = [page(&["x"]), page(&["still Secret"])];
    let queries = [
        query(SearchKind::Text, "absent"),
        query(SearchKind::Text, "Secret"),
    ];
    let found = walk(&queries, &[1, 2], &texts);
    assert_eq!(
        still_found(&found, &queries),
        vec!["page 2: searching the written file for `Secret` still finds \"Secret\"".to_string()]
    );
}

#[test]
fn a_match_with_no_position_on_the_page_is_a_reason_and_a_placed_one_is_not() {
    let mut hidden = page(&["seen Secret", "gone Secret"]);
    // The second line's characters have no boxes, as PDFium reports a
    // character it did not place.
    let second = "seen Secret\n".chars().count();
    for value in &mut hidden.boxes[second * 4..] {
        *value = 0.0;
    }
    let found = walk(&[query(SearchKind::Text, "Secret")], &[1], &[hidden]);
    assert_eq!(found.counts, vec![2]);
    assert_eq!(
        unmarked(&found),
        vec![
            "page 1: \"Secret\" matched, but its characters have no position on the page, so it \
             could not be marked"
                .to_string()
        ]
    );
    assert_eq!(match_regions(&found)[&0].len(), 1);
}

#[test]
fn a_copy_is_verified_and_exits_0_only_when_no_list_has_a_reason() {
    assert_eq!(
        outcome(Vec::new(), Vec::new()),
        (true, Vec::new(), Exit::Ok)
    );
    let (verified, why, exit) = outcome(vec!["window".into()], vec!["unmarked".into()]);
    assert_eq!((verified, exit), (false, Exit::Strict));
    assert_eq!(why, ["window", "unmarked"]);
    // A match that could not be marked withholds the verdict on its own.
    assert_eq!(
        outcome(Vec::new(), vec!["unmarked".into()]),
        (false, vec!["unmarked".to_string()], Exit::Strict)
    );
}

fn redacted(dry_run: bool, written: bool, verified: Option<bool>) -> report::Redacted {
    report::Redacted {
        schema: report::SCHEMA,
        command: "redact".into(),
        input: "in.pdf".into(),
        output: Some("out.pdf".into()),
        dry_run,
        written,
        verified,
        reasons: Vec::new(),
        notes: Vec::new(),
        summary: None,
        regions: 0,
        removals: 0,
        signatures_invalidated: 0,
        searches: Vec::new(),
        pages: Vec::new(),
    }
}

#[test]
fn nothing_matched_says_so_and_a_dry_run_says_nothing_was_written() {
    assert!(plain(&redacted(false, false, None)).contains("Nothing matched"));
    let mut dry = redacted(true, false, None);
    dry.regions = 2;
    dry.removals = 1;
    let said = plain(&dry);
    assert!(said.contains("Dry run: 2 regions would take 1 removal. Nothing was written."));
}

#[test]
fn a_written_copy_ends_with_the_applications_sentence() {
    let mut done = redacted(false, true, Some(false));
    done.summary = Some("Redaction not verified. Redacted 1 region, 1 removal".into());
    done.signatures_invalidated = 1;
    let said = plain(&done);
    assert!(said.trim_end().ends_with("Redacted 1 region, 1 removal"));
    assert!(said.contains("The input's 1 signature no longer covers the written copy."));
}

/// A written run that could not be proved clean, and a dry run given no `-o`.
pub(super) fn redact_samples() -> (report::Redacted, report::Redacted) {
    let why = vec![
        "page 1: object 0 is of kind path and overlaps the region; only text is removed here"
            .to_string(),
    ];
    let pages = vec![
        RedactedPage {
            page: 1,
            hits: vec![
                "jane.doe@example.com".into(),
                "DE89 3704 0044 0532 0130 00".into(),
            ],
            regions: 2,
            text_removals: 2,
            form_text_removals: 0,
            image_removals: 0,
            path_removals: 2,
            path_cuts: 1,
            taking: vec![
                "Contact: jane.doe@example.com".into(),
                "Account DE89 3704 0044 0532 0130 00".into(),
            ],
            left: why.clone(),
        },
        RedactedPage {
            page: 3,
            hits: Vec::new(),
            regions: 1,
            text_removals: 0,
            form_text_removals: 0,
            image_removals: 1,
            path_removals: 0,
            path_cuts: 0,
            taking: Vec::new(),
            left: Vec::new(),
        },
    ];
    let searches = vec![
        report::Search {
            kind: SearchKind::Pattern,
            query: r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}".into(),
            matches: 1,
        },
        report::Search {
            kind: SearchKind::Text,
            query: "DE89 3704".into(),
            matches: 1,
        },
    ];
    let written = report::Redacted {
        schema: report::SCHEMA,
        command: "redact".into(),
        input: "contacts.pdf".into(),
        output: Some("contacts-redacted.pdf".into()),
        dry_run: false,
        written: true,
        verified: Some(false),
        reasons: why.clone(),
        notes: Vec::new(),
        summary: Some(super::words::after_redaction(3, 3, false, &why, &[], false)),
        regions: 3,
        removals: 3,
        signatures_invalidated: 1,
        searches: searches.clone(),
        pages: pages.clone(),
    };
    let dry = report::Redacted {
        output: None,
        dry_run: true,
        written: false,
        verified: None,
        summary: None,
        signatures_invalidated: 0,
        ..written.clone()
    };
    (written, dry)
}

#[test]
fn regions_stdin_is_single_use_and_the_separator_keeps_option_like_paths_literal() {
    let command = parsed("--regions - --dry-run -- --force");
    assert_eq!(command.input, std::path::PathBuf::from("--force"));
    assert!(!command.force);
    assert_eq!(command.regions, vec![std::path::PathBuf::from("-")]);
    assert!(refused("a.pdf --regions - --regions - --dry-run").contains("stdin only once"));
    assert!(refused("--regions - --dry-run -- a.pdf b.pdf").contains("is a second"));
    // A literal file named '-' is spelled './-'; stdin cannot alias an output.
    assert!(parse(&argv("a.pdf --regions - -o -")).is_ok());
    assert!(refused("a.pdf --regions ./- -o -").contains("regions file"));
}
