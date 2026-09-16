//! Combine individually positioned glyphs without crossing state or line changes.
use super::Inspection;
use std::collections::BTreeMap;

pub(super) fn collect(page: &mut Inspection) {
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut runs: Vec<super::Run> = Vec::new();
    let mut last_operator = None;
    for next in std::mem::take(&mut page.runs.runs) {
        let join = runs
            .last()
            .zip(last_operator)
            .and_then(|(first, previous)| {
                let between =
                    &page.content.operations[previous as usize + 1..next.operator as usize];
                let [position] = between else { return None };
                if position.operator != "Td"
                    || first.text.is_empty()
                    || next.text.is_empty()
                    || position.operands.len() != 2
                    || super::number(&position.operands[1]).ok()? != 0.
                    || first.font != next.font
                    || first.size != next.size
                    || first.matrix[..4] != next.matrix[..4]
                    || page.continued.contains(&previous)
                    || page.continued.contains(&next.operator)
                    || first.text.chars().count() + next.text.chars().count() > super::MAX_TEXT
                {
                    return None;
                }
                let dx = next.matrix[4] - first.matrix[4];
                let dy = next.matrix[5] - first.matrix[5];
                let offset = if first.matrix[0] != 0. {
                    if dy != 0. {
                        return None;
                    }
                    dx / first.matrix[0]
                } else {
                    if dx != 0. {
                        return None;
                    }
                    dy / first.matrix[1]
                };
                // Explicit glyph placement may carry small kerning differences.
                // A larger gap denotes a separate field rather than inferred text.
                if offset <= 0.
                    || offset - first.advance < -first.size * 0.25
                    || offset - first.advance > first.size * 0.75
                {
                    return None;
                }
                if let Some((clips, original)) = page.compound_run_clips.get(&first.operator) {
                    let (_, next_bounds) = page.compound_run_clips.get(&next.operator)?;
                    let envelope = union(*original, *next_bounds);
                    if clips.iter().any(|clip| clip.contains(envelope).is_err()) {
                        return None;
                    }
                }
                let space = offset - first.advance > first.size * 0.18
                    && !first.text.ends_with(' ')
                    && !next.text.starts_with(' ');
                if first.text.chars().count() + next.text.chars().count() + usize::from(space)
                    > super::MAX_TEXT
                {
                    return None;
                }
                Some((offset, space))
            });
        last_operator = Some(next.operator);
        if let Some((offset, space)) = join {
            let first = runs.last_mut().unwrap();
            if let Some((_, next_bounds)) = page.compound_run_clips.get(&next.operator) {
                let next_bounds = *next_bounds;
                let (_, bounds) = page.compound_run_clips.get_mut(&first.operator).unwrap();
                *bounds = union(*bounds, next_bounds);
            }
            let bounds = page.horizontal_bounds[&next.operator];
            let original = page.horizontal_bounds.get_mut(&first.operator).unwrap();
            original[0] = original[0].min(offset + bounds[0]);
            original[1] = original[1].max(offset + bounds[1]);
            if space {
                first.text.push(' ');
            }
            first.text.push_str(&next.text);
            first.advance = offset + next.advance;
            for dimension in 0..2 {
                first.display_rect[dimension] =
                    first.display_rect[dimension].min(next.display_rect[dimension]);
                first.display_rect[dimension + 2] =
                    first.display_rect[dimension + 2].max(next.display_rect[dimension + 2]);
            }
            groups
                .entry(first.operator)
                .or_default()
                .push(next.operator);
        } else {
            runs.push(next);
        }
    }
    page.runs.runs = runs;
    page.groups = groups;
}

fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

#[cfg(test)]
mod tests {
    use crate::textedit::{self, Change};

    #[test]
    fn positioned_glyphs_edit_as_one_line_and_preserve_adjacent_content() {
        let mut doc = textedit::tests::with_content(
            b"BT /F1 12 Tf 40 180 Td (A) Tj 8 0 Td (B) Tj 8 0 Td (C) Tj 0 -30 Td (SECOND) Tj ET",
        );
        let before = textedit::scan(&doc, 0).unwrap();
        assert_eq!(
            before
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>(),
            ["ABC", "SECOND"]
        );
        let page = crate::pagetree::ordered_pages(&doc)[0];
        let original = textedit::page_content(&doc, page).unwrap();
        textedit::write(
            &mut doc,
            &[Change {
                layout: None,
                page: 0,
                revision: before.revision,
                operator: before.runs[0].operator,
                original: "ABC".into(),
                replacement: "CA".into(),
            }],
        )
        .unwrap();
        let after = textedit::scan(&doc, 0).unwrap();
        assert_eq!(after.runs.last().unwrap(), &before.runs[1]);
        assert_eq!(
            after
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "CASECOND"
        );
        let saved = textedit::page_content(&doc, page).unwrap();
        let suffix = b"0 -30 Td (SECOND) Tj ET";
        let offset = original
            .windows(suffix.len())
            .position(|part| part == suffix)
            .unwrap();
        let saved_offset = saved
            .windows(suffix.len())
            .position(|part| part == suffix)
            .unwrap();
        assert_eq!(
            saved[saved_offset..].trim_ascii_end(),
            original[offset..].trim_ascii_end()
        );
        assert_ne!(saved, original);
    }

    #[test]
    fn grouping_stops_at_columns_styles_and_cursor_dependent_shows() {
        for middle in ["80 0 Td", "8 -1 Td", "1 g 8 0 Td", "8 0 Td /F1 11 Tf", ""] {
            let doc = textedit::tests::with_content(
                format!("BT /F1 12 Tf 40 180 Td (A) Tj {middle} (B) Tj ET").as_bytes(),
            );
            assert_eq!(textedit::scan(&doc, 0).unwrap().runs.len(), 2, "{middle}");
        }
    }

    #[test]
    fn positioned_spaces_are_inferred_without_joining_columns() {
        let doc = textedit::tests::with_content(
            b"BT /F1 12 Tf 40 180 Td (A) Tj 11.5 0 Td (B) Tj 80 0 Td (C) Tj ET",
        );
        let mapped = textedit::scan(&doc, 0).unwrap();
        assert_eq!(
            mapped
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>(),
            ["A B", "C"]
        );
    }
}
