use super::tests::{fixture, CONTENT};
use crate::textedit::{self, Change};
use lopdf::{dictionary, Dictionary, Object};

// Independent census from ISO 32000-1 Tables 333-340, including types that
// discovery does not yet admit. Test the scan/write paths, not the predicate.
const STANDARD: &[&str] = &[
    "Document",
    "Part",
    "Art",
    "Sect",
    "Div",
    "BlockQuote",
    "Caption",
    "TOC",
    "TOCI",
    "Index",
    "NonStruct",
    "Private",
    "P",
    "H",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
    "L",
    "LI",
    "Lbl",
    "LBody",
    "Table",
    "TR",
    "TH",
    "TD",
    "THead",
    "TBody",
    "TFoot",
    "Span",
    "Quote",
    "Note",
    "Reference",
    "BibEntry",
    "Code",
    "Link",
    "Annot",
    "Ruby",
    "RB",
    "RT",
    "RP",
    "Warichu",
    "WT",
    "WP",
    "Figure",
    "Formula",
    "Form",
];

#[test]
fn textedit_role_map_cannot_redefine_any_standard_type_even_when_unused() {
    for &role in STANDARD {
        for target in ["P", "Sect", role] {
            let (mut doc, ids) = fixture(CONTENT);
            let before = textedit::scan(&doc, 0).unwrap();
            let change = Change {
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            };
            let mut roles = dictionary! { "Standard" => "P" };
            roles.set(role, Object::Name(target.as_bytes().to_vec()));
            doc.get_dictionary_mut(ids[1])
                .unwrap()
                .set("RoleMap", roles);
            let objects = doc.objects.clone();
            // The original content never uses this new entry. A later grammar
            // refusal therefore cannot conceal a missing RoleMap guard.
            assert_eq!(
                textedit::scan(&doc, 0).unwrap_err(),
                "tagged RoleMap contains unsupported or conflicting roles",
                "{role} -> {target}"
            );
            assert!(textedit::write(&mut doc, &[change]).is_err());
            assert_eq!(doc.objects, objects);
        }
    }
}

#[test]
fn textedit_role_map_blocks_disguised_standard_content_but_preserves_custom_aliases() {
    for role in [
        "Figure",
        "Table",
        "Span",
        "Link",
        "SyntheticParagraph",
        "FigureCustom",
        "figure",
        "H7",
    ] {
        let content = std::str::from_utf8(CONTENT)
            .unwrap()
            .replace("/Standard", &format!("/{role}"));
        let (mut doc, ids) = fixture(content.as_bytes());
        for id in [ids[3], ids[4]] {
            doc.get_dictionary_mut(id)
                .unwrap()
                .set("S", Object::Name(role.as_bytes().to_vec()));
        }
        let mut roles = Dictionary::new();
        roles.set(role, Object::Name(b"P".to_vec()));
        doc.get_dictionary_mut(ids[1])
            .unwrap()
            .set("RoleMap", roles);
        if ["Figure", "Table", "Span", "Link"].contains(&role) {
            assert_eq!(
                textedit::scan(&doc, 0).unwrap_err(),
                "tagged RoleMap contains unsupported or conflicting roles",
                "{role}"
            );
            continue;
        }
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(before.runs.len(), 2);
        let original = doc.objects.clone();
        textedit::write(
            &mut doc,
            &[Change {
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "FIRST".into(),
                replacement: "IN".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs[0].text, "IN");
        assert_eq!(after.runs[1], before.runs[1]);
        for (id, object) in original {
            if id != ids[0] {
                assert_eq!(doc.objects[&id], object);
            }
        }
        // The saved stream still names the producer's custom role.
        let stream = doc.get_page_content(ids[0]);
        assert!(String::from_utf8(stream)
            .unwrap()
            .contains(&format!("/{role}")));
    }
}
