//! Drawings inside a Form XObject: what a region takes of them, what is cut at
//! its edge, what stays, and what the removal does to the form's stream.

use lopdf::content::Content;
use lopdf::{dictionary, Document, Object, ObjectId, Stream};

use super::{
    aggregate, covered, form_paths, leave_shared, settle_form_paths, take_form_paths, FormPaths,
    RegionPlan, SharedDraws, CLIP_PATH, UNPLACED_PATH,
};
use crate::objects::{FormObject, PageObject};

struct Fixture {
    doc: Document,
    page: ObjectId,
    forms: [ObjectId; 2],
}

/// What the first form paints. A point `(x, y)` of it lands on the page at
/// `(2x + 120, 2y + 240)`: the form's own matrix moves it by (10, 20) and the
/// page then doubles it and moves it by (100, 200).
///
/// 0. a rule along the bottom, stroked 3 wide by the *page's* `w`: 6 on the page
/// 1. a filled square
/// 2. a curve
/// 3. a square that paints and also sets the clip
/// 4. a rule stroked 4 wide by the form's own `ExtGState`: 8 on the page
///
/// The `re W n` between them clips and paints nothing, so it is not a path
/// object and has no ordinal.
const FIRST: &str = "0 0 m 100 0 l S\n\
                     10 10 30 30 re f\n\
                     0 50 m 20 80 60 80 80 50 c S\n\
                     5 5 20 20 re W n\n\
                     60 10 20 20 re W f\n\
                     /GS1 gs 0 40 m 100 40 l S\n";

fn fixture(page_draws: &str) -> Fixture {
    let mut doc = Document::with_version("1.7");
    let first = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 10.into(), 20.into()],
            "Resources" => dictionary! {
                "ExtGState" => dictionary! { "GS1" => dictionary! { "LW" => 4 } },
            },
        },
        FIRST.as_bytes().to_vec(),
    ));
    let second = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form" },
        b"0 0 m 50 0 l S\n".to_vec(),
    ));
    let content = doc.add_object(Stream::new(dictionary! {}, page_draws.as_bytes().to_vec()));
    let pages_id = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content,
        "Resources" => dictionary! {
            "XObject" => dictionary! { "Fm0" => first, "Fm1" => second },
        },
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    Fixture {
        doc,
        page,
        forms: [first, second],
    }
}

/// The page every test but the shared-form ones uses.
const PAGE: &str = "q 2 0 0 2 100 200 cm 3 w /Fm0 Do Q\n/Fm1 Do\n";

/// A form's content, one operation per entry, as it would be written.
fn written(doc: &Document, form: ObjectId) -> Vec<String> {
    let stream = doc.get_object(form).unwrap().as_stream().unwrap();
    let body = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    Content::decode(&body)
        .unwrap()
        .operations
        .into_iter()
        .map(|operation| {
            let mut words: Vec<String> = operation
                .operands
                .iter()
                .map(|operand| match operand {
                    Object::Integer(value) => value.to_string(),
                    Object::Real(value) => value.to_string(),
                    Object::Name(name) => format!("/{}", String::from_utf8_lossy(name)),
                    other => format!("{other:?}"),
                })
                .collect();
            words.push(operation.operator);
            words.join(" ")
        })
        .collect()
}

/// The page's objects and forms as PDFium would report them: two forms, the
/// first with the five paths above, each with its box on the page.
fn reported() -> (Vec<PageObject>, Vec<FormObject>) {
    let form = |bounds| PageObject {
        bounds,
        kind: "form".to_string(),
    };
    let objects = vec![
        form([114.0, 234.0, 326.0, 404.0]),
        form([0.0, -0.5, 50.0, 0.5]),
    ];
    let first = FormObject {
        at: 0,
        text: Vec::new(),
        unreachable: Vec::new(),
        images: Vec::new(),
        paths: vec![
            [114.0, 234.0, 326.0, 246.0],
            [140.0, 260.0, 200.0, 320.0],
            [120.0, 340.0, 280.0, 400.0],
            [240.0, 260.0, 280.0, 300.0],
            [112.0, 312.0, 328.0, 328.0],
        ],
    };
    let second = FormObject {
        at: 1,
        text: Vec::new(),
        unreachable: Vec::new(),
        images: Vec::new(),
        paths: vec![[0.0, -0.5, 50.0, 0.5]],
    };
    (objects, vec![first, second])
}

fn kinds(plan: &super::Plan) -> Vec<(usize, &str)> {
    plan.unhandled
        .iter()
        .map(|left| (left.at, left.kind.as_str()))
        .collect()
}

/// A region's plan, settled against the fixture's document.
fn settled(f: &Fixture, region: [f32; 4]) -> super::Plan {
    let (objects, forms) = reported();
    let mut plan = covered(&objects, &forms, region);
    leave_shared(&mut plan, &SharedDraws::unknown(0, 2), &objects, &forms);
    let facts = form_paths(&f.doc, f.page, &[5, 1]);
    settle_form_paths(&mut plan, &forms, &facts, region);
    plan
}

#[test]
fn each_path_a_form_paints_is_read_in_the_forms_own_state() {
    let f = fixture(PAGE);
    let facts = form_paths(&f.doc, f.page, &[5, 1]);
    let first = facts[0].as_ref().expect("the counts agree");
    assert_eq!(
        first.iter().map(|path| path.clips).collect::<Vec<_>>(),
        [false, false, false, true, false]
    );
    assert_eq!(
        first
            .iter()
            .map(|path| path.drawing.is_some())
            .collect::<Vec<_>>(),
        [true, true, false, false, true],
        "a rule and a square can be cut; a curve and a path that clips cannot"
    );
    assert_eq!(facts[1].as_ref().map(Vec::len), Some(1));

    // A count that disagrees answers nothing for that form and still answers
    // the other; a number of forms that disagrees answers nothing at all.
    let facts = form_paths(&f.doc, f.page, &[4, 1]);
    assert!(facts[0].is_none() && facts[1].is_some());
    assert_eq!(
        form_paths(&f.doc, f.page, &[5, 1, 2]),
        vec![None, None, None]
    );
    // A form PDFium found no paths in is not read.
    assert_eq!(form_paths(&f.doc, f.page, &[0, 1])[0], Some(Vec::new()));
}

#[test]
fn a_rule_in_a_form_is_cut_where_the_page_draws_it() {
    let f = fixture(PAGE);
    // The bottom rule runs from (120, 240) to (320, 240) on the page and is 6
    // thick there. A region over all of that thickness cuts it.
    let plan = settled(&f, [200.0, 230.0, 240.0, 250.0]);
    assert_eq!(plan.form_cuts, vec![(0, 0)]);
    assert!(plan.is_complete(), "{:?}", plan.unhandled);

    // The width is the page's `3 w`, doubled by the page's matrix. A region 4
    // thick covers a rule drawn with the default width of 1 and not this one.
    let plan = settled(&f, [200.0, 238.0, 240.0, 242.0]);
    assert!(plan.form_cuts.is_empty());
    assert_eq!(kinds(&plan), [(0, "path")]);

    // The upper rule's width is the form's own `/GS1`, 4 and so 8 on the page:
    // 316 to 324. A region that would cover a rule 6 thick does not cover it.
    let plan = settled(&f, [300.0, 317.0, 320.0, 323.0]);
    assert!(plan.form_cuts.is_empty());
    assert_eq!(kinds(&plan), [(0, "path")]);
    let plan = settled(&f, [300.0, 314.0, 320.0, 326.0]);
    assert_eq!(plan.form_cuts, vec![(0, 4)]);
    assert!(plan.is_complete());
}

#[test]
fn a_drawing_in_a_form_goes_whole_is_cut_or_stays_as_one_on_the_page_does() {
    let f = fixture(PAGE);
    // Holds the square and crosses the upper rule.
    let plan = settled(&f, [130.0, 250.0, 210.0, 330.0]);
    assert_eq!(plan.form_paths, vec![(0, 1)]);
    assert_eq!(plan.form_cuts, vec![(0, 4)]);
    assert!(plan.is_complete(), "{:?}", plan.unhandled);
    assert!(plan.form_crossing.is_empty());

    // Crosses the curve, which cannot be split.
    let plan = settled(&f, [150.0, 350.0, 170.0, 395.0]);
    assert!(plan.form_paths.is_empty() && plan.form_cuts.is_empty());
    assert_eq!(kinds(&plan), [(0, "path")]);

    // Holds the square that also clips: taking it would change what is drawn
    // after it.
    let plan = settled(&f, [235.0, 255.0, 285.0, 305.0]);
    assert!(plan.form_paths.is_empty());
    assert_eq!(kinds(&plan), [(0, CLIP_PATH)]);

    // Overlaps the lower rule's box and none of its ink: nothing to report.
    let plan = settled(&f, [200.0, 244.0, 240.0, 245.5]);
    assert!(plan.is_complete() && plan.form_cuts.is_empty() && plan.form_paths.is_empty());
}

#[test]
fn a_forms_drawings_are_all_reported_until_they_are_settled() {
    let (objects, forms) = reported();
    let region = [130.0, 250.0, 210.0, 330.0];
    let plan = covered(&objects, &forms, region);
    assert_eq!(kinds(&plan), [(0, "path"), (0, "path")]);
    assert_eq!(plan.form_crossing, vec![(0, 1, true), (0, 4, false)]);
    assert!(plan.form_paths.is_empty() && plan.form_cuts.is_empty());

    // A form whose paths cannot be addressed: both stay, and the one the
    // region holds all of says why.
    let mut plan = covered(&objects, &forms, region);
    settle_form_paths(&mut plan, &forms, &[None, None], region);
    assert_eq!(kinds(&plan), [(0, UNPLACED_PATH), (0, "path")]);
    assert!(plan.form_paths.is_empty() && plan.form_cuts.is_empty());
}

#[test]
fn a_form_drawn_more_than_once_keeps_its_drawings() {
    let f = fixture(PAGE);
    let (objects, forms) = reported();
    let region = [130.0, 250.0, 210.0, 330.0];
    let facts = form_paths(&f.doc, f.page, &[5, 1]);
    let shared = SharedDraws {
        forms: vec![Some(3), None],
        ..SharedDraws::unknown(0, 2)
    };

    let mut plan = covered(&objects, &forms, region);
    leave_shared(&mut plan, &shared, &objects, &forms);
    settle_form_paths(&mut plan, &forms, &facts, region);
    assert!(plan.form_paths.is_empty() && plan.form_cuts.is_empty());
    assert_eq!(kinds(&plan), [(0, "path"), (0, "path")]);

    // And in the other order.
    let mut plan = covered(&objects, &forms, region);
    settle_form_paths(&mut plan, &forms, &facts, region);
    assert_eq!((plan.form_paths.len(), plan.form_cuts.len()), (1, 1));
    leave_shared(&mut plan, &shared, &objects, &forms);
    assert!(plan.form_paths.is_empty() && plan.form_cuts.is_empty());
    assert_eq!(kinds(&plan), [(0, "path"), (0, "path")]);
}

#[test]
fn the_removal_takes_one_path_cuts_another_and_leaves_the_rest_of_the_form() {
    let mut f = fixture(PAGE);
    let before = written(&f.doc, f.forms[0]);
    let other = written(&f.doc, f.forms[1]);
    let region = [200.0, 230.0, 240.0, 250.0];
    let took = take_form_paths(
        &mut f.doc,
        f.page,
        &[(0, 5), (1, 1)],
        0,
        &[1],
        &[(0, region)],
    )
    .expect("taken");
    assert_eq!((took.removed.removed, took.cut), (1, 1));
    let after = written(&f.doc, f.forms[0]);
    // The rule is drawn in two pieces, in the form's own numbers: the region's
    // 200..240 on the page is 40..60 in the form.
    assert_eq!(
        &after[..5],
        ["0 0 m", "40 0 l", "60 0 m", "100 0 l", "S"],
        "{after:?}"
    );
    assert!(!after.contains(&"10 10 30 30 re".to_string()), "{after:?}");
    // Everything after the square is as it was.
    assert_eq!(&after[5..], &before[5..]);
    assert_eq!(
        written(&f.doc, f.forms[1]),
        other,
        "the other form is untouched"
    );
}

#[test]
fn a_removal_that_cannot_be_right_changes_nothing() {
    let region = [200.0, 230.0, 240.0, 250.0];
    let refused =
        |page: &str, forms: &[(usize, usize)], whole: &[usize], cuts: &[(usize, [f32; 4])]| {
            let mut f = fixture(page);
            let before = written(&f.doc, f.forms[0]);
            let why = take_form_paths(&mut f.doc, f.page, forms, 0, whole, cuts).unwrap_err();
            assert_eq!(written(&f.doc, f.forms[0]), before, "{why}");
            why
        };
    let both = [(0, 5), (1, 1)];
    // PDFium and the form's content disagree on how many paths there are.
    assert!(refused(PAGE, &[(0, 4), (1, 1)], &[1], &[]).contains("paints 5 path(s)"));
    // Or on how many forms the page draws.
    assert!(refused(PAGE, &[(0, 5)], &[1], &[]).contains("draws 2 form XObject(s)"));
    // The form is drawn a second time.
    let twice = "q 2 0 0 2 100 200 cm 3 w /Fm0 Do Q\n/Fm1 Do\n/Fm0 Do\n";
    assert!(refused(twice, &[(0, 5), (1, 1), (2, 5)], &[1], &[]).contains("draws 2 time(s)"));
    // The path also clips.
    assert!(refused(PAGE, &both, &[3], &[]).contains("also clips"));
    // The curve cannot be split.
    let over_curve = [150.0, 350.0, 170.0, 395.0];
    assert!(refused(PAGE, &both, &[], &[(2, over_curve)]).contains("cannot be split"));
    // There is no such path, or no such form.
    assert!(refused(PAGE, &both, &[9], &[]).contains("no path 9"));
    let mut f = fixture(PAGE);
    assert!(take_form_paths(&mut f.doc, f.page, &both, 7, &[1], &[])
        .unwrap_err()
        .contains("not one of the 2 form(s)"));
    // The control: the same call with nothing wrong is made.
    let mut f = fixture(PAGE);
    assert!(take_form_paths(&mut f.doc, f.page, &both, 0, &[1], &[(0, region)]).is_ok());
}

#[test]
fn the_pages_plan_carries_the_forms_drawings_of_every_region() {
    let one = [0.0, 0.0, 10.0, 10.0];
    let two = [20.0, 0.0, 30.0, 10.0];
    let region =
        |area: [f32; 4], whole: Vec<(usize, usize)>, cuts: Vec<(usize, usize)>| RegionPlan {
            area,
            form_paths: FormPaths {
                whole,
                cuts,
                objects: vec![(0, 5), (1, 1)],
            },
            ..RegionPlan::default()
        };
    let page = aggregate(
        0,
        vec![[0.0; 4]; 2],
        vec![
            region(one, vec![(0, 1)], vec![(0, 0), (0, 4)]),
            // Holds the same square, cuts the same rule, and holds all of a
            // path the first region only cut.
            region(two, vec![(0, 1), (0, 4)], vec![(0, 0)]),
        ],
        None,
    );
    let planned = &page.planned.form_paths;
    assert_eq!(planned.whole, vec![(0, 1), (0, 4)], "each once");
    assert_eq!(
        planned.cuts,
        vec![(0, 0, one), (0, 0, two)],
        "a rule two regions cross is cut twice; one that goes whole is not cut"
    );
    assert_eq!(planned.objects, vec![(0, 5), (1, 1)]);
    assert_eq!(page.shows, 3, "two paths removed and one cut");
    let summary = page.summary();
    assert_eq!((summary.paths, summary.cuts), (2, 1));
}
