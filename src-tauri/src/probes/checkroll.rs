//! The roll of check names a probe printed, and the one rule the names must obey.
//!
//! Included by `#[path]` from the probes that need it rather than compiled into
//! the library, which is what `src/probes/` is for --- these are example bodies,
//! not product code, and nothing here ships in the binary.
//!
//! **Why a roll at all.** Every probe prints `LABEL name:<52 detail`, and
//! `:<52` pads without truncating, so a name longer than that runs into its
//! detail with a single space between --- indistinguishable from the single
//! spaces inside the name. 30 of `search-probe`'s 75 lines are past the pad. A
//! reader that splits on runs of spaces silently drops those, which is not
//! hypothetical: it happened twice on 2026-08-16, once in `viewer_sweep.py`
//! (which then reported two corpora agreeing about a set that was wrong on both
//! sides) and once in an analysis of these very probes, which measured 60 of 75
//! names and reported a clean result over the subset.
//!
//! `scripts/mutate_viewer.py` itself is unaffected --- it tests `startswith`
//! against the whole remainder of the line and never needs the name. Anything
//! asking *what the names are* is affected, and the rule below is exactly that
//! question.

/// Whether no name is a prefix of another, printing the ones that are.
///
/// `mutate_viewer.py` decides a mutation was caught with
/// `line.startswith(expect)` over the failing checks, and refuses an
/// expectation matching more than one. That refusal is correct and arrives
/// late: it fires when somebody writes the mutation, not when the name is
/// added. So the constraint belongs on the names.
///
/// It has been broken once, in this probe: `query astral-alone` sat beside
/// `query astral-alone: indices address the hit`, and nothing could be aimed at
/// the first. `docs/TRAPS.md` records it under *"a check name that is a prefix
/// of another cannot be aimed at"*, where the rule was written down and
/// enforced by nothing.
fn no_name_shadows_another(names: &[String]) -> bool {
    let mut clean = true;
    for name in names {
        let shadowed: Vec<&String> = names
            .iter()
            .filter(|other| *other != name && other.starts_with(name.as_str()))
            .collect();
        if !shadowed.is_empty() {
            clean = false;
            println!(
                "[FAIL] {name:<52} is a prefix of {} other name(s); no mutation can aim at it",
                shadowed.len()
            );
            for other in shadowed {
                println!("       {other}");
            }
        }
    }
    clean
}

/// Prints the roll and checks the prefix rule, returning whether it holds.
///
/// Called before the summary line, so a transcript still ends on its verdict ---
/// `mutate_viewer.py` requires that line before it will believe a run happened,
/// and a reader takes the last one.
pub fn finish(names: &[String]) -> bool {
    println!("CHECK-NAMES-JSON {}", json_of(names));
    no_name_shadows_another(names)
}

/// The names as a JSON array.
///
/// Hand-rolled rather than pulling `serde_json` into an example for one line.
/// The escaping covers what a check name can contain: quotes, backslashes and
/// control characters. Non-ASCII is emitted as UTF-8, which JSON permits --- and
/// these names do carry it, since the search probe names its queries after the
/// text it looks for.
fn json_of(names: &[String]) -> String {
    let mut out = String::from("[");
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        for ch in name.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}
