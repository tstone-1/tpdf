"""Rust mutation rows: what the command-line tool reports and how it ends, the
values it reads off its command line, and what crosses the worker boundary
when a request cannot be read.

A partly placed match in `redact`, `verify --strict` on a list of signatures
that was cut, a report that could not be written, `--color`, `render -- -`,
a comment on one line, the worker's unreadable request, and the bound on the
codes read from a page.

Added 2026-11-19, each row seen red on the day it was written. Two more of the
same work need a worker and are in `tests/cli.rs`, which this table cannot
select; they were proved by hand: `redact.rs`'s refusal of a forced output
when nothing matched (`return Err(Failure::new(Exit::Refused, why));` after
`kept_output`, made `let _ = why;`, turns "nothing matched over an existing
output with --force is refused" red, in `--filter "redact removes what it
finds"`), and `edit.rs`'s comment listing calling `comment_line` (the call
replaced by a `format!` of the raw body turns "comments prints a comment whose
body breaks lines as one line" red, in `--filter "JSON edit plans"`).
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- redact: a match only partly placed on the page --------------------------------
    Mutation(
        "cli redact: a match with some characters unplaced counted as marked",
        "src/cli/redact.rs",
        "        } else if left > 0 {",
        "        } else if false && left > 0 {",
        "a_match_with_only_some_characters_placed_is_a_reason",
    ),
    Mutation(
        "cli redact: a match with no position counted as marked",
        "src/cli/redact.rs",
        "            marked_any |= !areas.is_empty();",
        "            marked_any = true;",
        "a_match_with_no_position_on_the_page_is_a_reason_and_a_placed_one_is_not",
    ),
    # --- verify --strict and a list of signatures that was cut -------------------------
    Mutation(
        "cli verify: --strict passes a cut list of signatures",
        "src/cli/verify.rs",
        "            && file.signatures_complete\n",
        "",
        "strict_fails_a_document_whose_list_of_signatures_was_cut",
    ),
    Mutation(
        "cli verify: the report says every list was read whole",
        "src/cli/verify.rs",
        "        signatures_complete: signatures_complete(&properties.limits),",
        "        signatures_complete: true,",
        "strict_fails_a_document_whose_list_of_signatures_was_cut",
    ),
    Mutation(
        "cli verify: a cut list of signatures not said on stderr",
        "src/cli/verify.rs",
        '                Some(format!("{program}: {}", incomplete_sentence(&file.path)))',
        "                None",
        "strict_fails_a_document_whose_list_of_signatures_was_cut",
    ),
    # --- How a run ends when its report cannot be written ------------------------------
    Mutation(
        "cli: a report that could not be written goes unrecorded",
        "src/cli.rs",
        "                self.failed = Some(std::io::Error::new(e.kind(), e.to_string()));",
        "                let _ = e;",
        "a_report_that_could_not_be_written_exits_3_with_one_line",
    ),
    Mutation(
        "cli: a closed pipe ends the run as a failed write",
        "src/cli.rs",
        "        Some(e) if e.kind() == std::io::ErrorKind::BrokenPipe => code,\n",
        "",
        "a_closed_pipe_ends_the_run_quietly_with_its_own_code",
    ),
    Mutation(
        "cli: a failed run's code replaced by the write's",
        "src/cli.rs",
        "        Some(_) if code > Exit::Strict.code() => code,\n",
        "",
        "a_closed_pipe_ends_the_run_quietly_with_its_own_code",
    ),
    # --- Values off the command line ---------------------------------------------------
    Mutation(
        "cli --color: a channel out of range accepted",
        "src/cli/args.rs",
        "                .filter(|channel| (0. ..=1.).contains(channel))\n",
        "",
        "what_cannot_be_written_is_a_usage_error",
    ),
    Mutation(
        "cli --color: the parts that parse taken as the colour",
        "src/cli/args.rs",
        "        .collect::<Option<Vec<f32>>>()?;",
        "        .flatten().collect::<Vec<f32>>();",
        "what_cannot_be_written_is_a_usage_error",
    ),
    Mutation(
        "cli render: an operand after -- read as it is written",
        "src/cli/render.rs",
        "            paths.push(super::args::operand(true, arg));",
        "            paths.push(arg.into());",
        "render_parser_requires_bounded_resolution_and_one_page",
    ),
    # --- A comment printed on one line -------------------------------------------------
    Mutation(
        "cli edit: a comment's body printed with its line breaks",
        "src/cli/edit.rs",
        "        super::verify::one_line(body)\n",
        "        body\n",
        "a_comment_is_one_line_whatever_its_body_holds",
    ),
    # --- The worker's unreadable request -----------------------------------------------
    Mutation(
        "worker: an unreadable request printed with its contents",
        "src/worker_child.rs",
        '        "[worker] unreadable request: {:?} error at line {}, column {}, in {} bytes\\n",\n'
        "        error.classify(),",
        '        "[worker] unreadable request: {:?} error at line {}, column {}, in {} bytes {line}\\n",\n'
        "        error.classify(),",
        "an_unreadable_request_is_reported_without_its_contents",
    ),
    Mutation(
        "worker: the reader of a request prints the error as serde words it",
        "src/worker_child.rs",
        "    serde_json::from_str::<Request>(line).map_err(|e| unreadable_request(line, &e))",
        '    serde_json::from_str::<Request>(line).map_err(|e| format!("[worker] unreadable request {line:?}: {e}\\n"))',
        "an_unreadable_request_is_reported_without_its_contents",
    ),
    # --- The codes read from one page --------------------------------------------------
    Mutation(
        "codes: a page with exactly the bound said to have more",
        "src/codes.rs",
        "    let more = found.len() > most;",
        "    let more = found.len() >= most;",
        "a_page_past_the_bound_keeps_the_first_codes_and_says_there_were_more",
    ),
]
