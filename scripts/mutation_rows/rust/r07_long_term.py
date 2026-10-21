"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# --- long-term validation data when signing (increment C2) ------------------
#
# `longterm.rs`, `sign_dss.rs`, `commands/sign.rs` and `cli/sign.rs`,
# 2026-09-28. Each removes one check the gathering, the `/DSS` revision or the
# refusal policy rests on, and names the test built so only that check fails
# it: the fake PKI in `integrity/test_tsa.rs` serves one fault at a time.
ROWS += [
    Mutation(
        "longterm: fetch revocation data over any scheme",
        "src/longterm.rs",
        '    let fine = matches!(url.scheme(), "http" | "https")',
        "    let fine = true",
        "a_certificate_that_publishes_nothing_is_refused_with_what_would_work",
    ),
    Mutation(
        "longterm: ask about a certificate that says it needs no check",
        "src/longterm.rs",
        "            if link.no_check {",
        "            if false && link.no_check {",
        "a_certificate_that_needs_no_check_is_not_asked_about",
    ),
    Mutation(
        # The walk is shared with the reader since 2026-09-28
        # (`revocation/chain.rs`): a root no longer ends it, so it loops.
        "longterm: ask about a root",
        "src/revocation/chain.rs",
        "        if let Some(anchor) = anchor_of(&current, candidates) {",
        "        if let Some(anchor) = anchor_of(&current, candidates).filter(|_| false) {",
        "a_b_lt_signature_is_written_and_reads_back_good_for_signer_and_authority",
    ),
    Mutation(
        # Found by the real run against DigiCert and Sectigo (docs/PLAN.md).
        "longterm: walk past a cross-certificate of a root the OS holds",
        "src/revocation/chain.rs",
        "    candidates.iter().copied().find(|candidate| {",
        "    candidates.iter().copied().filter(|_| false).find(|candidate| {",
        "a_cross_certificate_of_a_root_the_os_holds_ends_the_chain",
    ),
    Mutation(
        "longterm: carry a cross-certificate in place of its anchor",
        "src/revocation/chain.rs",
        "                anchor: Some(anchor.clone()),",
        "                anchor: Some(current.clone()),",
        "a_cross_certificate_of_a_root_the_os_holds_ends_the_chain",
    ),
    Mutation(
        "longterm: let a self-issued signer pass with nothing asked",
        "src/longterm.rs",
        "        if walked.links.is_empty() {",
        "        if false {",
        "a_self_issued_signer_publishes_nothing_and_says_so",
    ),
    Mutation(
        "longterm: take no issuer from the OS chain",
        "src/longterm.rs",
        "            above_signer.extend(os_chain(&der, &in_signature));",
        "            above_signer.extend(os_chain(&der, &in_signature).into_iter().take(0));",
        "an_issuer_nowhere_to_be_found_is_refused",
    ),
    Mutation(
        "longterm: plan a chain of any depth",
        "src/longterm.rs",
        "        if walked.dropped > 0 || subjects.len() > MAX_SUBJECTS {",
        "        if false {",
        "a_chain_deeper_than_any_real_one_is_refused",
    ),
    Mutation(
        # C1's note: the reader takes issuers only from the document.
        "longterm: leave the certificates out of the /DSS",
        "src/longterm.rs",
        "    let mut gathered = Gathered {\n        certificates,\n",
        "    let mut gathered = Gathered {\n        certificates: { let _ = certificates; Vec::new() },\n",
        "a_b_lt_signature_is_written_and_reads_back_good_for_signer_and_authority",
    ),
    Mutation(
        "longterm: make any number of requests",
        "src/longterm.rs",
        "        if requests > MAX_REQUESTS {",
        "        if false {",
        "the_requests_are_bounded_whatever_a_certificate_lists",
    ),
    Mutation(
        "longterm: gather for as long as the servers take",
        "src/longterm.rs",
        "        if left.is_zero() {",
        "        if false {",
        "the_whole_gathering_is_bounded_in_time",
    ),
    Mutation(
        "longterm: gather any amount of data",
        "src/longterm.rs",
        "        if gathered.bytes() + more > MAX_GATHERED {",
        "        if false {",
        "the_data_is_bounded_in_size_before_it_is_read",
    ),
    Mutation(
        "longterm: read an OCSP answer of any length",
        "src/longterm.rs",
        "    body: crate::revocation::MAX_RESPONSE_BYTES,",
        "    body: 16 * 1024 * 1024,",
        "a_responder_that_is_down_silent_or_too_long_leaves_nothing_answered",
    ),
    Mutation(
        # `tryLater` is not an answer, and the list is still asked.
        "longterm: take a responder's non-answer as its answer",
        "src/longterm.rs",
        "                Ok(OcspResponseStatus::Successful) => {}",
        "                Ok(_) => {}",
        "a_list_is_fetched_when_the_responder_does_not_answer",
    ),
    Mutation(
        # The OCSP response kept whatever it says: revoked, unknown, forged.
        "longterm: keep an OCSP response without judging it",
        "src/longterm.rs",
        "            room(&gathered, answer.len())?;\n            judged(subject, &candidates, &material, &by, &host, now)?;\n            gathered.responses.push(answer);",
        "            room(&gathered, answer.len())?;\n            gathered.responses.push(answer);",
        "a_revoked_signer_is_refused_and_no_list_is_shopped_for",
    ),
    Mutation(
        # Shopping: an answer that does not check out, and a list asked instead.
        "longterm: ask the list after an answer that does not check out",
        "src/longterm.rs",
        "            room(&gathered, answer.len())?;\n            judged(subject, &candidates, &material, &by, &host, now)?;\n            gathered.responses.push(answer);",
        "            room(&gathered, answer.len())?;\n            if judged(subject, &candidates, &material, &by, &host, now).is_err() {\n                continue;\n            }\n            gathered.responses.push(answer);",
        "an_answer_that_does_not_check_out_is_refused",
    ),
    Mutation(
        "longterm: keep a list without judging it",
        "src/longterm.rs",
        "            room(&gathered, answer.len())?;\n            judged(subject, &candidates, &material, &by, &host, now)?;\n            gathered.lists.push(answer);",
        "            room(&gathered, answer.len())?;\n            gathered.lists.push(answer);",
        # Re-aimed after its first run: for the signer and the authority the
        # worker's reading refuses a revoked list anyway (the named test went
        # on passing), so only a certificate above them reaches this alone.
        "a_revoked_intermediate_is_refused_before_anything_is_written",
    ),
    Mutation(
        "longterm: write a signature that does not read back intact",
        "src/longterm.rs",
        '        _ => return written("the new signature does not read as intact"),',
        "        _ => {}",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        "longterm: write a timestamp that does not read back intact",
        "src/longterm.rs",
        "    if stamp.integrity.as_ref().map(|i| i.verdict) != Some(Verdict::Intact) {",
        "    if false {",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        # `none` is B-T, which is not what the reader asked for.
        "longterm: write a signature whose revocation reads none",
        "src/longterm.rs",
        "        match revocation.standing {\n            Status::Good => {}",
        "        match revocation.standing {\n            Status::Good | Status::None => {}",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        "longterm: append a revision built against other bytes",
        "src/longterm.rs",
        "    if extended.built_against != bytes.len() {",
        "    if false {",
        "a_revision_built_against_other_bytes_is_refused",
    ),
    Mutation(
        "dss: write a /DSS with no revocation data in it",
        "src/sign_dss.rs",
        "    if gathered.responses.is_empty() && gathered.lists.is_empty() {",
        "    if false {",
        "nothing_to_add_or_too_much_is_refused",
    ),
    Mutation(
        "dss: take any amount of data",
        "src/sign_dss.rs",
        "    if gathered.bytes() > MAX_BYTES {",
        "    if false {",
        "nothing_to_add_or_too_much_is_refused",
    ),
    Mutation(
        "dss: drop what an earlier /DSS held",
        "src/sign_dss.rs",
        '    let arrays = [carried(b"Certs")?, carried(b"OCSPs")?, carried(b"CRLs")?];',
        "    let arrays = [Vec::new(), Vec::new(), Vec::new()];",
        "an_earlier_dss_is_kept_and_added_to",
    ),
    Mutation(
        # The reader who must never get a signature with a revoked certificate
        # is offered one without the data.
        "sign: hold a revoked signature for writing without the data",
        "src/commands/sign.rs",
        "        Err(Stopped::Unextended { why, .. }) if why.revoked() => {",
        "        Err(Stopped::Unextended { why, .. }) if false => {",
        "a_revoked_certificate_is_refused_and_nothing_is_held",
    ),
    Mutation(
        "sign: ask for long-term data without a timestamp",
        "src/commands/sign.rs",
        "    if long_term && authority.is_none() {",
        "    if false {",
        "long_term_data_is_refused_without_a_timestamp",
    ),
    Mutation(
        "sign: write the held signature without the data it waited for",
        "src/commands/sign.rs",
        "        (false, _) => bytes,",
        "        (false | true, _) => bytes,",
        "long_term_data_that_came_is_written_and_read_back_good",
    ),
    Mutation(
        "cli: take --long-term without --timestamp",
        "src/cli/sign.rs",
        "    if long_term && timestamp.is_none() {",
        "    if false {",
        "long_term_data_is_asked_for_only_with_a_timestamp",
    ),
    Mutation(
        "cli: drop --long-term",
        "src/cli/sign.rs",
        "        long_term,\n        lock,\n",
        "        long_term: false,\n        lock,\n",
        "long_term_data_is_asked_for_only_with_a_timestamp",
    ),
]

# --- the whole chain, read (2026-09-28) --------------------------------------
#
# `revocation/chain.rs`, `docinfo.rs`, `cli/verify.rs`, `words.rs` and
# `longterm.rs`: the reader judging every certificate above the signer's and
# the authority's. Each removes one rule of the walk or the combination, and
# names the test built so only that rule fails it.
ROWS += [
    Mutation(
        "chain: judge only the leaf",
        "src/revocation/chain.rs",
        "    for link in walked.links.iter().skip(1).filter(|link| !link.no_check) {",
        "    for link in walked.links.iter().skip(usize::MAX).filter(|link| !link.no_check) {",
        "an_intermediate_revoked_before_the_moment_revokes_the_chain_and_is_named",
    ),
    Mutation(
        "chain: judge a certificate that needs no check",
        "src/revocation/chain.rs",
        "    for link in walked.links.iter().skip(1).filter(|link| !link.no_check) {",
        "    for link in walked.links.iter().skip(1).filter(|_| true) {",
        "a_certificate_that_needs_no_check_is_walked_through_and_not_judged",
    ),
    Mutation(
        "chain: a revocation before the moment outweighs nothing",
        "src/revocation/chain.rs",
        "        Status::Revoked if !revocation.after_moment => 5,",
        "        Status::Revoked if !revocation.after_moment => 1,",
        "the_most_telling_answer_decides_and_the_leaf_breaks_a_tie",
    ),
    Mutation(
        "chain: nothing said about a certificate reads as good",
        "src/revocation/chain.rs",
        "        Status::None => 2,",
        "        Status::None => 0,",
        "an_intermediate_the_document_says_nothing_about_is_not_read_as_good",
    ),
    Mutation(
        "chain: the top breaks a tie",
        "src/revocation/chain.rs",
        "(weight(&c.revocation), std::cmp::Reverse(*at))",
        "(weight(&c.revocation), *at)",
        "the_most_telling_answer_decides_and_the_leaf_breaks_a_tie",
    ),
    Mutation(
        "chain: past the bound a good chain stands",
        "src/revocation/chain.rs",
        "    let (standing, after_moment, decided_by) = if dropped > 0 && reassures {",
        "    let (standing, after_moment, decided_by) = if false && reassures {",
        "a_chain_longer_than_the_bound_is_counted_and_cannot_read_good",
    ),
    Mutation(
        "chain: count nothing past the bound",
        "src/revocation/chain.rs",
        "            dropped += 1;",
        "            dropped += 0;",
        "a_chain_longer_than_the_bound_is_counted_and_cannot_read_good",
    ),
    Mutation(
        # Not "remove the seen check", which would hang the harness: a loop
        # with nothing to end it is the defect, and a test cannot time it out.
        "chain: call a loop a missing issuer",
        "src/revocation/chain.rs",
        "                end: End::Loop,",
        "                end: End::NoIssuer,",
        "two_authorities_certifying_each_other_end_the_walk_rather_than_loop_it",
    ),
    Mutation(
        "chain: the attested moment ignores the certificates above the authority's",
        "src/docinfo.rs",
        "        || timestamp\n            .revocation_chain",
        "        || false && timestamp\n            .revocation_chain",
        "only_an_intact_token_from_a_trusted_unrevoked_authority_attests_a_moment",
    ),
    Mutation(
        "chain: the scan keeps no chain for the signer",
        "src/docinfo.rs",
        "            out.revocation_chain = Some(chain);",
        "            out.revocation_chain = None.filter(|_: &crate::revocation::chain::Chain| true).or(None); let _ = chain;",
        "the_certificate_above_the_signers_is_judged_at_the_same_moment",
    ),
    Mutation(
        "chain: --strict ignores a revoked certificate above the signer's",
        "src/cli/verify.rs",
        "            c.certificates.iter().skip(1).any(|above| {",
        "            c.certificates.iter().skip(usize::MAX).any(|above| {",
        "a_certificate_above_the_signers_revoked_before_the_attested_time_fails_strict",
    ),
    Mutation(
        "keystore: a counted key counts nothing",
        "src/keystore.rs",
        "        self.count\n            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);",
        "",
        "a_counted_key_counts_each_request_and_passes_the_answer_on",
    ),
    Mutation(
        "sign: report a held signature as waiting for its timestamp",
        "src/commands/sign.rs",
        "            Stage::Sealed { .. } => Waiting::LongTerm,",
        "            Stage::Sealed { .. } => Waiting::Timestamp,",
        "long_term_data_that_did_not_come_is_held_and_can_be_left_out",
    ),
    Mutation(
        "cli: say the attested moment after the reason again",
        "src/words.rs",
        '            "not trusted{judged} — {why}. So nothing establishes that the key belongs to the \\',
        '            "not trusted — {why}{judged}. So nothing establishes that the key belongs to the \\',
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    Mutation(
        "cli: hide that an untrusted authority was judged at the archive's moment",
        "src/words.rs",
        '            format!("not trusted{judged} — {why}. So nothing establishes who attests this time.")',
        '            format!("not trusted — {why}. So nothing establishes who attests this time.")',
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    Mutation(
        "chain: name the leaf when an issuing certificate decides",
        "src/words.rs",
        "                    &issuing_certificate(judged)\n                )\n            ),",
        "                    leaf_holder(authority)\n                )\n            ),",
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    Mutation(
        "longterm: write a signature whose chain reads revoked",
        "src/longterm.rs",
        "            .find(|c| c.revocation.standing == Status::Revoked)",
        "            .find(|_| false)",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        "longterm: write a signature whose chain reads none",
        "src/longterm.rs",
        "            .find(|c| c.revocation.standing != Status::Good)",
        "            .find(|_| false)",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
    Mutation(
        "longterm: write a signature whose chain runs past the bound",
        "src/longterm.rs",
        "        if chain.dropped > 0 {",
        "        if false {",
        "the_check_before_writing_wants_good_for_both_and_names_a_revocation",
    ),
]

# --- archive timestamps, read (PAdES B-LTA, 2026-09-28) -----------------------
#
# `docinfo.rs`: a document timestamp later in the file attests a moment at
# which the timestamp authorities before it are judged. Each removes one rule
# of that, and names the test built so only that rule fails it.
ROWS += [
    Mutation(
        "archive: judge a token's authority now whatever an archive attests",
        "src/docinfo.rs",
        "    let trust = attested.then(|| match archive {",
        "    let trust = attested.then(|| match None::<u64> {",
        "an_archive_timestamp_lets_a_lapsed_authority_attest_the_time_it_stated",
    ),
    Mutation(
        # Not the sort key's first half alone: a document timestamp's end
        # already sorts before every other field's placeholder, so that edit
        # changes nothing (measured, 2026-09-28).
        "archive: read the fields in the document's order",
        "src/docinfo.rs",
        "    order.sort_by_key(|&at| match reach[at] {",
        "    order.sort_by_key(|&at| match None::<Option<(bool, u64)>>.unwrap_or(reach[at]).filter(|_| false) {",
        "an_archive_timestamp_lets_a_lapsed_authority_attest_the_time_it_stated",
    ),
    Mutation(
        "archive: a range reaching as far covers itself",
        "src/docinfo.rs",
        "        .filter(|archive| archive.end > end)",
        "        .filter(|archive| archive.end >= end)",
        "an_archive_moment_is_the_earliest_covering_one",
    ),
    Mutation(
        "archive: the latest covering archive is the moment",
        "src/docinfo.rs",
        "        .map(|archive| archive.at)\n        .min()",
        "        .map(|archive| archive.at)\n        .max()",
        "an_archive_moment_is_the_earliest_covering_one",
    ),
    Mutation(
        "archive: a document timestamp that is not intact still archives",
        "src/docinfo.rs",
        "    (intact && trusted && !revoked && (from..=until).contains(&at)).then_some(at)",
        "    (trusted && !revoked && (from..=until).contains(&at)).then_some(at)",
        "an_archive_timestamp_that_does_not_attest_is_no_archive",
    ),
    Mutation(
        "archive: an untrusted document timestamp still archives",
        "src/docinfo.rs",
        "    (intact && trusted && !revoked && (from..=until).contains(&at)).then_some(at)",
        "    (intact && !revoked && (from..=until).contains(&at)).then_some(at)",
        "an_archive_timestamp_that_does_not_attest_is_no_archive",
    ),
    Mutation(
        "archive: only an authority trusted now attests a moment",
        "src/docinfo.rs",
        "    let trusted = timestamp.trust.as_ref().is_some_and(|t| {",
        "    let trusted = timestamp.trust.as_ref().is_some_and(|t| t.standing == crate::trust::Standing::Trusted) && timestamp.trust.as_ref().is_some_and(|t| {",
        "an_archive_timestamp_lets_a_lapsed_authority_attest_the_time_it_stated",
    ),
    Mutation(
        "archive: seal a document timestamp that does not read back intact",
        "src/sign_cms.rs",
        "    if verdict.verdict != crate::integrity::Verdict::Intact {\n        return Err(format!(\n            \"tpdf's own check of the document timestamp",
        "    if false {\n        return Err(format!(\n            \"tpdf's own check of the document timestamp",
        "a_document_timestamp_is_sealed_only_when_it_reads_back_intact",
    ),
    Mutation(
        "archive: seal a document timestamp against other bytes",
        "src/sign_cms.rs",
        "    if built_against != original.len() {",
        "    if false {",
        "a_document_timestamp_is_sealed_only_when_it_reads_back_intact",
    ),
    Mutation(
        "archive: write the long-term data without an archive timestamp",
        "src/longterm.rs",
        "    archived(whole, worker, archive)",
        "    let _ = &archive;\n    Ok(whole)",
        "the_archive_timestamp_covers_the_signature_and_its_validation_data",
    ),
    Mutation(
        "archive: ask for a token over the first piece only",
        "src/tsa.rs",
        "    for piece in pieces {",
        "    for piece in pieces.iter().take(1) {",
        "an_archive_timestamp_is_asked_over_the_covered_range_and_checked_over_it",
    ),
    Mutation(
        "archive: take the authority's answer unchecked",
        "src/tsa.rs",
        "    accept_over(&answer, token::Target::Range(pieces), &digest, &nonce)",
        "    let _ = (&digest, &nonce);\n    Ok(answer)",
        "an_archive_timestamp_is_asked_over_the_covered_range_and_checked_over_it",
    ),
    Mutation(
        "archive: report the archive timestamp as an earlier signature",
        "src/sign_cms.rs",
        '            archive: signature.kind == "ETSI.RFC3161" && at.is_some_and(|at| index > at),',
        "            archive: false && at.is_some_and(|at| index > at),",
        "the_archive_timestamp_covers_the_signature_and_its_validation_data",
    ),
]

# --- the release audit for 26.9.22 (2026-09-28) ------------------------------
#
# `longterm.rs`, `cli/sign.rs` and `docinfo.rs`: fetching only for a
# timestamp authority this computer trusts, the reader's bounds on what the OS
# parses in the app process, whose failure a dead worker is, the command
# line's read-back after long-term signing, and /DSS counts that bound the
# decoding rather than only what is kept.
ROWS += [
    Mutation(
        "longterm: gather for an authority nobody vouched for",
        "src/longterm.rs",
        "    let vouched_chain = vouched(cms, now, vouch)?;",
        "    let vouched_chain = vouched(cms, now, vouch).unwrap_or_default();",
        "an_authority_this_computer_does_not_trust_is_refused_before_anything_is_fetched",
    ),
    Mutation(
        # A certificate added to the token's set, which passes for an issuer
        # by name and key and is on no chain the store accepted.
        "longterm: take a link the authority was not vouched for by",
        "src/longterm.rs",
        "            if at > 0 && !admitted.contains(&der) {",
        "            if false {",
        "a_certificate_injected_into_the_token_is_never_asked_about",
    ),
    Mutation(
        "trust: hand back the chain of an answer that is not trusted",
        "src/trust.rs",
        "    if trust.standing != Standing::Trusted {\n        chain.clear();",
        "    if false {\n        chain.clear();",
        "a_chain_is_handed_back_only_with_a_trusted_answer",
    ),
    Mutation(
        "longterm: take an authority the store could not judge as vouched for",
        "src/longterm.rs",
        "    if trust.standing == Standing::Trusted {",
        "    if trust.standing != Standing::Untrusted {",
        "a_token_carrying_more_certificates_than_the_os_is_handed_is_not_vouched_for",
    ),
    Mutation(
        "longterm: hand the OS every certificate the token carries",
        "src/longterm.rs",
        "            .take(crate::trust::MAX_CERTIFICATES)",
        "            .take(usize::MAX)",
        "the_os_is_handed_at_most_the_readers_bound_of_certificates",
    ),
    Mutation(
        "longterm: hand the OS a candidate of any size",
        "src/longterm.rs",
        "            .filter(|der| der.len() <= crate::trust::MAX_CERTIFICATE_BYTES)",
        "            .filter(|_| true)",
        "the_os_is_handed_at_most_the_readers_bound_of_certificates",
    ),
    Mutation(
        "longterm: hand the OS a leaf of any size",
        "src/longterm.rs",
        "    if leaf.len() > crate::trust::MAX_CERTIFICATE_BYTES {",
        "    if false {",
        "the_os_is_handed_at_most_the_readers_bound_of_certificates",
    ),
    Mutation(
        "longterm: call a dead worker a refusal",
        "src/longterm.rs",
        "        matches!(self, Refusal::Unreadable(_) | Refusal::Written(_))",
        "        matches!(self, Refusal::Unreadable(_))",
        "a_worker_that_dies_while_extending_is_tpdfs_failure",
    ),
    Mutation(
        "cli: exit 3 for tpdf's own long-term failure",
        "src/cli/sign.rs",
        "    let exit = if why.tpdf_failed() {",
        "    let exit = if false {",
        "a_long_term_failure_of_tpdfs_own_is_4_and_every_refusal_3",
    ),
    Mutation(
        # The rule is the window's too since the two share one tail, and lives
        # beside it.
        "sign: read back only the signer's revocation after long-term signing",
        "src/commands/sign.rs",
        "        if let Err(why) = crate::longterm::check(Some(ours)) {",
        "        if let Err(why) = crate::longterm::check(Some(ours)).or_else(|why| {\n"
        "            let signer_good = ours\n"
        "                .revocation\n"
        "                .as_ref()\n"
        "                .is_some_and(|r| r.standing == crate::revocation::Status::Good);\n"
        "            if signer_good { Ok(()) } else { Err(why) }\n"
        "        }) {",
        "the_command_lines_read_back_asks_what_the_check_before_writing_asks",
    ),
    Mutation(
        "sign: take a timestamp that did not read back as written",
        "src/commands/sign.rs",
        "    if timestamp && !stamped {",
        "    if false {",
        "a_copy_that_does_not_read_back_as_written_is_refused_in_the_window_too",
    ),
    Mutation(
        # The window's half of the one rule: a copy whose timestamp or
        # long-term data did not read back is reported as signed.
        "sign: report a copy that did not read back as signed, in the window",
        "src/commands/sign.rs",
        "            if let Some(why) = finished.read_back.sentence(&name, &finished.field) {",
        "            if let Some(why) = None::<String> {",
        "the_window_refuses_a_copy_whose_timestamp_did_not_read_back",
    ),
    Mutation(
        # The order both paths share: written before the long-term data came.
        "sign: write the signed copy before its long-term data is checked",
        "src/commands/sign.rs",
        "    let timestamped = cms.is_some();",
        "    let timestamped = cms.is_some();\n    let _ = write(&bytes);",
        "the_window_and_the_command_line_take_the_same_steps_in_the_same_order",
    ),
    Mutation(
        "seal: index a document timestamp's revision by a range nobody checked",
        "src/sign_cms.rs",
        "    if !framed(range, built_against, update.len()) {",
        "    if false {",
        "a_document_timestamp_whose_range_does_not_frame_its_value_is_refused",
    ),
    Mutation(
        # Substitution under a second certificate over the same key.
        "integrity: call a signature intact under a certificate it does not name",
        "src/integrity.rs",
        "        if let Err(why) = token::binds_when_stated(attributes.as_ref(), certificate.as_deref()) {",
        "        if let Err(why) = Ok::<(), Why>(()).and(Ok(drop((attributes, certificate)))) {",
        "a_signature_naming_another_certificate_than_the_one_it_carries_is_not_intact",
    ),
    Mutation(
        "integrity: require every signature to name its certificate",
        "src/integrity/token.rs",
        "    if !stated {\n        return Ok(());",
        "    if false {\n        return Ok(());",
        "a_signature_that_does_not_name_its_certificate_reads_as_before",
    ),
    Mutation(
        "appendix: count only the page objects an append wrote",
        "src/docinfo.rs",
        "    let mut touched = pages_reaching(&whole, &changed, &apart_from_signing);",
        "    let mut touched = std::collections::BTreeSet::new();",
        "a_content_stream_replaced_after_signing_touches_its_page_and_no_other",
    ),
    Mutation(
        "appendix: walk through a page to whatever names it",
        "src/docinfo.rs",
        "            if reaching.insert(*from) && !pages.contains(from) {",
        "            if reaching.insert(*from) {",
        "a_page_that_links_to_a_rewritten_page_is_not_touched_by_that",
    ),
    Mutation(
        "appendix: walk on from a rewritten page to whatever names it",
        "src/docinfo.rs",
        "        .filter(|id| !pages.contains(id))",
        "        .filter(|_| true)",
        "a_page_that_links_to_a_rewritten_page_is_not_touched_by_that",
    ),
    Mutation(
        # One branch written again would touch every page of the document.
        "appendix: reach the pages beside a branch through a node's kids",
        "src/docinfo.rs",
        "        match (tree.contains(id), object.as_dict()) {",
        "        match (false, object.as_dict()) {",
        "a_branch_of_the_tree_rewritten_touches_only_the_pages_under_it",
    ),
    Mutation(
        "appendix: ignore the resources a page inherits",
        "src/docinfo.rs",
        "const HANDED_DOWN: [&[u8]; 5] = [b\"Parent\", b\"Resources\", b\"MediaBox\", b\"CropBox\", b\"Rotate\"];",
        "const HANDED_DOWN: [&[u8]; 5] = [b\"Parent\", b\"Parent\", b\"MediaBox\", b\"CropBox\", b\"Rotate\"];",
        "what_a_page_inherits_touches_every_page_below_it",
    ),
    Mutation(
        "appendix: look no further up the page tree than a page's own parent",
        "src/docinfo.rs",
        "const HANDED_DOWN: [&[u8]; 5] = [b\"Parent\", b\"Resources\", b\"MediaBox\", b\"CropBox\", b\"Rotate\"];",
        "const HANDED_DOWN: [&[u8]; 5] = [b\"Resources\", b\"Resources\", b\"MediaBox\", b\"CropBox\", b\"Rotate\"];",
        "what_a_page_inherits_touches_every_page_below_it",
    ),
    Mutation(
        "appendix: miss a page taken out of the document",
        "src/docinfo.rs",
        "    out.pages_touched = touched.len() + moved;",
        "    out.pages_touched = touched.len();",
        "a_page_taken_out_of_the_document_is_counted",
    ),
    Mutation(
        "cli: pass --strict whatever follows the last signature",
        "src/cli/verify.rs",
        "            && after.get(at).is_none_or(|after| *after == After::Unchanged)",
        "            && after.get(at).is_none_or(|_| true)",
        "strict_fails_a_signature_followed_by_a_rewritten_page",
    ),
    Mutation(
        "cli: read an appendix nobody could read as nothing appended",
        "src/cli/verify.rs",
        "        Some(appendix) if appendix.unread => After::Unread,",
        "        Some(appendix) if false && appendix.unread => After::Unread,",
        "what_follows_the_last_intact_signature_is_judged_by_the_pages_it_touches",
    ),
    Mutation(
        "cli: fail --strict for a field listed on a page",
        "src/cli/verify.rs",
        "                .saturating_sub(appendix.pages_listing.len())",
        "                .saturating_sub(0)",
        "what_follows_the_last_intact_signature_is_judged_by_the_pages_it_touches",
    ),
    Mutation(
        "cli: judge what follows a signature that does not hold",
        "src/cli/verify.rs",
        "                    .is_some_and(|i| i.verdict == crate::integrity::Verdict::Intact)\n        })\n        .min_by_key(|s| s.appended_bytes);",
        "                    .is_some()\n        })\n        .min_by_key(|s| s.appended_bytes);",
        "what_follows_the_last_intact_signature_is_judged_by_the_pages_it_touches",
    ),
    # `verify --json`'s `appendix`, 2026-10-05: what a script reads to see why
    # `--strict` failed, so each count has to be the worker's own.
    Mutation(
        "cli: leave what was appended out of the report",
        "src/cli/verify.rs",
        "        appendix: signature.appendix.as_ref().map(appendix_report),",
        "        appendix: None,",
        "a_signature_reports_what_was_appended_after_it",
    ),
    Mutation(
        "cli: report no page touched after a signature",
        "src/cli/verify.rs",
        "        pages_touched: appendix.pages_touched,",
        "        pages_touched: 0,",
        "a_signature_reports_what_was_appended_after_it",
    ),
    Mutation(
        "cli: report an appendix nobody could read as one that was read",
        "src/cli/verify.rs",
        "        unread: appendix.unread,",
        "        unread: false,",
        "an_appendix_that_could_not_be_read_is_reported_as_unread",
    ),
    Mutation(
        # Every touched page would then count against `--strict` for a reader
        # of the report, tpdf's own archive timestamp included.
        "cli: report no page as rewritten only to list a field",
        "src/cli/verify.rs",
        "            .iter()\n            .map(|listed| ListedPage {",
        "            .iter()\n            .take(0)\n            .map(|listed| ListedPage {",
        "tpdfs_own_long_term_revisions_are_reported_and_touch_no_page_for_more",
    ),
    Mutation(
        "cli: report every listed field as a signature's",
        "src/cli/verify.rs",
        "                timestamp: listed.timestamp,",
        "                timestamp: false,",
        "a_signature_reports_what_was_appended_after_it",
    ),
    Mutation(
        "cli: word an appendix nobody could read as an empty one",
        "src/cli/verify.rs",
        "    if appendix.unread {\n        return \"something, but its contents could not be read\".into();",
        "    if false {\n        return \"something, but its contents could not be read\".into();",
        "an_appendix_that_could_not_be_read_is_reported_as_unread",
    ),
    Mutation(
        "cli: name validation data by its objects",
        "src/cli/verify.rs",
        "    let what = if appendix.catalog_gained.iter().any(|key| key == \"DSS\") {",
        "    let what = if false {",
        "tpdfs_own_long_term_revisions_are_reported_and_touch_no_page_for_more",
    ),
    Mutation(
        # The bare count is what says a page changed and tpdf does not know why.
        "cli: say a page only lists a field when another was rewritten for more",
        "src/cli/verify.rs",
        "numbers.split_last().filter(|_| listing.len() == touched)",
        "numbers.split_last().filter(|_| true)",
        "a_signature_reports_what_was_appended_after_it",
    ),
    Mutation(
        "cli: print no Appended row under a signature",
        "src/cli/verify.rs",
        "    if let Some(appendix) = &signature.appendix {",
        "    if let Some(appendix) = signature.appendix.as_ref().filter(|_| false) {",
        "the_text_names_what_was_appended_under_its_signature",
    ),
    Mutation(
        "sign: gather in the window without asking whether the authority is trusted",
        "src/commands/sign.rs",
        "                asking.vouch,",
        "                &|_: &[u8], _: u64| crate::longterm::Vouched {\n"
        "                    trust: crate::trust::Trust {\n"
        "                        standing: crate::trust::Standing::Trusted,\n"
        "                        ..crate::trust::Trust::default()\n"
        "                    },\n"
        "                    chain: Vec::new(),\n"
        "                },",
        "an_untrusted_authority_is_held_and_nothing_is_fetched",
    ),
    Mutation(
        "dss: decode one object as often as the arrays name it",
        "src/docinfo.rs",
        "                    if !seen.insert((kind, *id)) {",
        "                    if !seen.insert((kind, *id)) && false {",
        "the_dss_counts_bound_the_decoding_and_what_they_drop_is_counted",
    ),
    Mutation(
        "dss: decode every stream an array names",
        "src/docinfo.rs",
        "                if *count >= most(kind) {",
        "                if false {",
        "the_dss_counts_bound_the_decoding_and_what_they_drop_is_counted",
    ),
    # Dropping what no remaining page uses (`src/unused.rs`), 2026-10-02.
    Mutation(
        "unused: a rewrite that dropped a page prunes what is left",
        "src/save.rs",
        "    if !dropped.is_empty() && crate::unused::prune(&mut doc)? > 0 {",
        "    if false && !dropped.is_empty() && crate::unused::prune(&mut doc)? > 0 {",
        "extracting_a_page_leaves_the_other_pages_pictures_out_of_the_file",
    ),
    Mutation(
        "unused: a dictionary something else reaches is kept whole",
        "src/unused.rs",
        "            if strangers.contains(&id) {",
        "            if false {",
        "a_dictionary_something_else_reaches_is_left_whole",
    ),
    Mutation(
        "unused: only a name no page mentions goes",
        "src/unused.rs",
        "            .filter(|name| !drawn.contains(name))",
        "            .filter(|_| true)",
        "a_picture_no_page_draws_leaves_the_file_and_a_drawn_one_stays",
    ),
    Mutation(
        "unused: a form drawing through the page keeps every name",
        "src/unused.rs",
        "    if leans_on_page(doc, page) {",
        "    if false {",
        "a_form_without_resources_of_its_own_keeps_every_name",
    ),
    Mutation(
        "unused: content that does not decode keeps every name",
        "src/unused.rs",
        "    let data = crate::textedit::page_content(doc, page).ok()?;",
        "    let data = crate::textedit::page_content(doc, page).unwrap_or_default();",
        "content_that_does_not_decode_keeps_every_name",
    ),
    Mutation(
        "unused: an escaped name is the name it stands for",
        "src/unused.rs",
        "            if data[at] == b'#' {",
        "            if false {",
        "a_name_is_found_however_the_content_around_it_reads",
    ),
    Mutation(
        "default app: already the default is said as that",
        "src/defaultapp.rs",
        '        Some(handler) if same(handler) && already => Ok("tpdf already opens PDF documents.".into()),',
        '        Some(handler) if same(handler) => Ok("tpdf already opens PDF documents.".into()),',
        "the_sentence_is_read_from_what_the_system_names_afterwards",
    ),
    Mutation(
        "cli tool: removing says both links",
        "src/clitool.rs",
        'format!("Removed {LINK} and {ALIAS}. The application is unchanged.")',
        'format!("Removed {LINK}. The application is unchanged.")',
        "the_reader_is_told_what_is_there_afterwards_not_what_was_attempted",
    ),
]

# --- the text layer over a scanned page -------------------------------------
# `textlayer.rs` writes it, `ocr_layer.rs` decides what goes in it, and the plan
# carries it through `save::rewrite`. What these cannot see is whether PDFium
# reads the layer back as the words at their boxes: that is `textlayer-probe`,
# which was run against six of the `textlayer.rs` mutations below by hand on
# 2026-10-03 and went red on each.
ROWS += [
    Mutation("text layer: let a copy be the original bytes", "src/edits.rs", "        // and confusingly, without the highlights.\n        if !self.forms.is_empty() || !self.text_edits.is_empty() || !self.text_layers.is_empty() {", "        // and confusingly, without the highlights.\n        if !self.forms.is_empty() || !self.text_edits.is_empty() {", "a_text_layer_forces_a_rewrite_and_lands_on_the_page_it_names"),
    Mutation("text layer: allow a layer beside a mark to append", "src/edits.rs", "pub fn is_appendable(&self) -> bool {\n        if !self.forms.is_empty() || !self.text_edits.is_empty() || !self.text_layers.is_empty() {", "pub fn is_appendable(&self) -> bool {\n        if !self.forms.is_empty() || !self.text_edits.is_empty() {", "a_text_layer_beside_a_mark_is_not_written_as_an_append"),
    Mutation("text layer: omit the writer call", "src/save.rs", "    crate::textlayer::write(&mut doc, &plan.text_layers)?;", "    // text layer omitted", "a_text_layer_forces_a_rewrite_and_lands_on_the_page_it_names"),
    Mutation("text layer: set the type along the page's own axis", "src/textlayer.rs", "    let upright = Upright::of(\n        shown.turns,", "    let upright = Upright::of(\n        0,", "on_a_turned_page_the_type_runs_the_way_it_is_read"),
    Mutation("text layer: measure from the sheet's corner", "src/textlayer.rs", "[quad[0] + ox, quad[1] + oy, quad[2] + ox, quad[3] + oy],", "[quad[0], quad[1], quad[2], quad[3]],", "a_cropped_page_s_corner_is_where_the_layer_measures_from"),
    Mutation("text layer: leave the word at its natural width", "src/textlayer.rs", "let scaling = upright.width / natural * 100.0;", "let scaling = 100.0;", "a_word_is_set_at_its_box_and_scaled_to_its_width"),
    Mutation("text layer: put the baseline on the box's bottom", "src/textlayer.rs", "let baseline = size * f64::from(ASCENT) / em;", "let baseline = size;", "a_word_is_set_at_its_box_and_scaled_to_its_width"),
    Mutation("text layer: write no space after a word", "src/textlayer.rs", "        .chain(std::iter::once(&0x0020))\n", "", "a_word_is_set_at_its_box_and_scaled_to_its_width"),
    Mutation("text layer: paint the words", "src/textlayer.rs", 'String::from("q\\nBT\\n3 Tr\\n")', 'String::from("q\\nBT\\n0 Tr\\n")', "a_word_is_set_at_its_box_and_scaled_to_its_width"),
    Mutation("text layer: reuse a font name the page has", "src/textlayer.rs", "while fonts.has(name.as_bytes()) {", "while false {", "inherited_resources_are_copied_and_a_taken_name_is_not_reused"),
    Mutation("text layer: drop the page's own content", "src/textlayer.rs", "            _ => contents.push(Object::Reference(id)),", "            _ => {}", "the_layer_goes_before_the_page_s_own_content_and_keeps_it"),
    Mutation("text layer: drop a content array's parts", "src/textlayer.rs", "        Some(Object::Array(parts)) => contents.extend(parts),", "        Some(Object::Array(_)) => {}", "a_page_whose_content_is_an_array_keeps_every_part_after_the_layer"),
    Mutation("text layer: accept one page named twice", "src/textlayer.rs", "if !seen.insert(layer.page) {", "if !seen.insert(layer.page) && false {", "a_page_the_document_lacks_a_page_named_twice_and_an_oversized_layer_are_refused"),
    Mutation("text layer: accept any number of words", "src/textlayer.rs", "if layer.words.len() > MAX_WORDS {", "if false {", "a_page_the_document_lacks_a_page_named_twice_and_an_oversized_layer_are_refused"),
    Mutation("text layer: accept a word of any length", "src/textlayer.rs", "if units.len() > MAX_UNITS {", "if false {", "a_page_the_document_lacks_a_page_named_twice_and_an_oversized_layer_are_refused"),
    Mutation("text layer: write a word with no area", "src/textlayer.rs", "word.rect.iter().all(|v| v.is_finite()) && right > left && bottom > top;", "true;", "unusable_words_are_left_out_and_a_page_with_none_is_untouched"),
    Mutation("text layer: keep control characters", "src/textlayer.rs", "        .filter(|c| !c.is_control())\n        .collect::<String>()", "        .collect::<String>()", "unusable_words_are_left_out_and_a_page_with_none_is_untouched"),
    Mutation("text layer: a glyph with no height", "src/textlayer.rs", "be16(&mut glyf, &[descent, ascent - descent, 0, descent - ascent]); // y deltas", "be16(&mut glyf, &[0, 0, 0, 0]); // y deltas", "the_font_program_is_one_box_glyph_of_the_stated_size"),
    Mutation("ocr: count a word's place in characters", "src/ocr.rs", "units += ch.len_utf16();", "units += 1;", "a_character_outside_the_basic_plane_moves_the_next_word_by_two"),
    Mutation("ocr layer: every page has text", "src/ocr_layer.rs", ".any(|code| !char::from_u32(*code).is_some_and(char::is_whitespace))", ".any(|_| true)", "a_page_with_only_white_space_has_no_text"),
    Mutation("ocr layer: read a page at any resolution", "src/ocr_layer.rs", "if scale < MIN_SCALE {", "if scale < 0.0 {", "a_page_too_large_to_read_at_100_dpi_is_refused"),
    Mutation("ocr layer: ignore the longest side", "src/ocr_layer.rs", "WANTED_SCALE.min(by_area).min(by_edge);", "WANTED_SCALE.min(by_area);", "a_long_narrow_page_is_bounded_by_its_longer_side"),
    Mutation("ocr layer: keep an empty recognition", "src/ocr_layer.rs", "        if item.text.trim().is_empty() || words.iter().any(|word| repeats(word, &item)) {", "        if words.iter().any(|word| repeats(word, &item)) {", "a_recognition_with_no_words_makes_no_layer"),
    Mutation("ocr layer: any page reads back", "src/ocr_layer.rs", "    wanted == got\n", "    let _ = (wanted, got);\n    true\n", "a_missing_layer_and_a_wrong_character_do_not_read_back"),
    Mutation("ocr layer: ask for lines", "src/ocr_layer.rs", "        words: true,\n", "        words: false,\n", "the_engine_is_asked_for_words_with_its_corrector_on"),
    Mutation("ocr layer: turn the corrector off", "src/ocr_layer.rs", "        language_correction: true,\n", "        language_correction: false,\n", "the_engine_is_asked_for_words_with_its_corrector_on"),
    Mutation("ocr layer: drop the mark of a hyphen that ends a line", "src/ocr_layer.rs", "            if *code == LINE_END_HYPHEN {", "            if false {", "a_hyphen_that_ends_a_line_reads_back_as_the_hyphen_it_is"),
    Mutation("ocr layer: take the line-end mark for any character", "src/ocr_layer.rs", "const LINE_END_HYPHEN: u32 = 2;", "const LINE_END_HYPHEN: u32 = 3;", "a_hyphen_that_ends_a_line_reads_back_as_the_hyphen_it_is"),
    Mutation("ocr layer: write a word the engine reported twice, twice", "src/ocr_layer.rs", "        if item.text.trim().is_empty() || words.iter().any(|word| repeats(word, &item)) {", "        if item.text.trim().is_empty() {", "a_word_reported_twice_in_one_place_is_written_once"),
    Mutation("ocr layer: take any word in the same place for a repeat", "src/ocr_layer.rs", "    word.text == item.text && smaller > 0.0 && shared >= smaller * 0.5", "    smaller > 0.0 && shared >= smaller * 0.5", "a_word_reported_twice_in_one_place_is_written_once"),
    Mutation("ocr layer: ask for more than half of the box", "src/ocr_layer.rs", "    word.text == item.text && smaller > 0.0 && shared >= smaller * 0.5", "    word.text == item.text && smaller > 0.0 && shared > smaller * 0.5", "a_word_reported_twice_in_one_place_is_written_once"),
    Mutation("ocr layer: measure a repeat by the larger box", "src/ocr_layer.rs", "    let smaller = ((a.2 - a.0) * (a.3 - a.1)).min((b.2 - b.0) * (b.3 - b.1));", "    let smaller = ((a.2 - a.0) * (a.3 - a.1)).max((b.2 - b.0) * (b.3 - b.1));", "a_word_reported_twice_in_one_place_is_written_once"),
    Mutation("ocr layer: read no scan that has text", "src/ocr_layer.rs", "    !has_text(text) || pictures >= SCAN_SHARE", "    !has_text(text)", "a_scan_is_read_whatever_text_it_has_and_a_page_of_text_is_not"),
    Mutation("ocr layer: want more than half a page of pictures", "src/ocr_layer.rs", "    !has_text(text) || pictures >= SCAN_SHARE", "    !has_text(text) || pictures > SCAN_SHARE", "a_scan_is_read_whatever_text_it_has_and_a_page_of_text_is_not"),
    Mutation("ocr layer: count a picture under another again", "src/scanshare.rs", "    covered as f32 / (GRID * GRID) as f32\n", "    covered as f32 / (GRID * GRID) as f32 * pictures.len().max(1) as f32\n", "a_scan_is_read_whatever_text_it_has_and_a_page_of_text_is_not"),
    Mutation("ocr layer: measure across from the origin, not the page box", "src/scanshare.rs", "        let y = bottom + (row as f32 + 0.5) * height / GRID as f32;", "        let y = (row as f32 + 0.5) * height / GRID as f32;", "a_scan_is_read_whatever_text_it_has_and_a_page_of_text_is_not"),
    Mutation("ocr layer: write every word the engine read", "src/ocr_layer.rs", "                .partition(|item| already_there(item.rect, &centres, &item.text));", "                .partition(|_| false);", "a_word_already_on_the_page_is_not_written_again"),
    Mutation("ocr layer: drop a word for any character under it", "src/ocr_layer.rs", "(under.contains(&word) || word.contains(&under))", "true", "a_word_already_on_the_page_is_not_written_again"),
    Mutation("ocr layer: compare two readings one way round", "src/ocr_layer.rs", "(under.contains(&word) || word.contains(&under))", "under.contains(&word)", "a_word_already_on_the_page_is_not_written_again"),
    Mutation("ocr layer: compare two readings by their case", "src/ocr_layer.rs", "        .flat_map(char::to_lowercase)\n", "", "a_word_already_on_the_page_is_not_written_again"),
    Mutation("ocr layer: place a character with no box in the corner", "src/ocr_layer.rs", "        .filter(|(_, b)| b.iter().any(|v| *v != 0.0))", "", "a_word_already_on_the_page_is_not_written_again"),
    Mutation("ocr layer: say nothing was read when every word was there", "src/ocr_layer.rs", "                return Ok(Outcome::AlreadyThere);", "                return Ok(Outcome::Nothing);", "a_word_already_on_the_page_is_not_written_again"),
    Mutation("ocr layer: write the words another program read otherwise", "src/ocr_layer.rs", "    there > 0 && there as f32 >= ALREADY_READ * (there + new) as f32", "    there > 0 && new == 0", "a_scan_another_program_read_gets_nothing_where_its_engine_read_otherwise"),
    Mutation("ocr layer: want more than half the words already there", "src/ocr_layer.rs", "    there > 0 && there as f32 >= ALREADY_READ * (there + new) as f32", "    there > 0 && there as f32 > ALREADY_READ * (there + new) as f32", "a_scan_another_program_read_gets_nothing_where_its_engine_read_otherwise"),
    Mutation("ocr layer: leave a page alone when one word in four is there", "src/ocr_layer.rs", "pub const ALREADY_READ: f32 = 0.5;", "pub const ALREADY_READ: f32 = 0.25;", "a_scan_another_program_read_gets_nothing_where_its_engine_read_otherwise"),
    Mutation("ocr layer: call a page that read nothing already read", "src/ocr_layer.rs", "    there > 0 && there as f32 >= ALREADY_READ * (there + new) as f32", "    there as f32 >= ALREADY_READ * (there + new) as f32", "a_word_already_on_the_page_is_not_written_again"),
    # `objects::pictures`, which measures a picture inside forms inside forms,
    # needs a document and an engine, so its checks are in `tests/cli/ocr.rs`,
    # which this table cannot select. Four mutations were run there by hand on
    # 2026-10-15, each restored byte for byte, each red on the check named:
    # `depth < PICTURE_DEPTH` as `depth < 1`, the outer form's matrix left out
    # of `then`, the two matrices applied in the wrong order (all three: *two
    # forms deep is read*), and `has_its_layer` replaced by "every word is
    # there" (*a scan another program read is left alone*, tool and window).
    Mutation("ocr layer: leave the page's own text out of the read-back", "src/ocr_layer.rs", "        .chain(characters(before))\n", "", "a_scan_with_typed_words_reads_back_with_them"),
    Mutation("checked copy: never run the check", "src/save.rs", "    if let Some(check) = check {\n        // Removed on a refusal", "    if let Some(check) = check.filter(|_| false) {\n        // Removed on a refusal", "a_checked_copy_that_fails_its_check_leaves_nothing_and_replaces_nothing"),
    Mutation("checked copy: publish what failed its check", "src/save.rs", "            let _ = std::fs::remove_file(&staged);\n            return Err(why);\n", "            let _ = why;\n", "a_checked_copy_that_fails_its_check_leaves_nothing_and_replaces_nothing"),
    Mutation("checked copy: leave the staged file behind", "src/save.rs", "            let _ = std::fs::remove_file(&staged);\n            return Err(why);\n", "            return Err(why);\n", "a_checked_copy_that_fails_its_check_leaves_nothing_and_replaces_nothing"),
    Mutation("checked copy: write a layer into a changed source", "src/save.rs", "    let on_change = if check.is_some() {\n        OnChange::Refuse", "    let on_change = if check.is_none() {\n        OnChange::Refuse", "a_checked_copy_refuses_a_changed_source_where_a_plain_copy_proceeds"),
    Mutation("recognise: blame the engine for pages that had text", "src/commands/ocr.rs", "        (true, false, false) => \"Every page already has text", "        (false, true, true) => \"Every page already has text", "the_reason_nothing_was_added_names_the_one_cause_when_there_is_one"),
    Mutation("recognise: send the report in Rust's spelling", "src/commands/ocr.rs", "#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]\n#[serde(rename_all = \"camelCase\")]\npub struct Recognised {", "#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]\npub struct Recognised {", "the_report_reaches_the_window_in_its_own_spelling"),
    Mutation('drawings: take any drawing the region touches', 'src/redact.rs', 'object.kind == "path" && contains(region, object.bounds)', 'object.kind == "path" && overlaps(object.bounds, region)', 'a_drawing_the_region_holds_all_of_is_removed_and_one_reaching_beyond_is_left'),
    Mutation('drawings: name a path by its place among all objects', 'src/redact.rs', '        let path_ordinal = paths;', '        let path_ordinal = at;', 'a_drawing_the_region_holds_all_of_is_removed_and_one_reaching_beyond_is_left'),
    Mutation('drawings: ignore the right edge when asking what is inside', 'src/objects.rs', 'outer[0] <= inner[0] && inner[2] <= outer[2] && outer[1]', 'outer[0] <= inner[0] && outer[1]', 'a_drawing_flush_with_the_region_s_edges_is_inside_it'),
    Mutation('drawings: count a path that is ended without painting', 'src/redact.rs', 'let painted = operation.operator != "n" && points >= 2;', 'let painted = points >= 2;', 'a_path_is_counted_when_it_is_painted_and_has_two_points'),
    Mutation('drawings: count a painted lone point', 'src/redact.rs', 'let painted = operation.operator != "n" && points >= 2;', 'let painted = operation.operator != "n" && points >= 1;', 'a_path_is_counted_when_it_is_painted_and_has_two_points'),
    Mutation('drawings: count a move after a move as a second point', 'src/redact.rs', '                if !(points > 0 && last_is_move) {', '                if true {', 'a_path_is_counted_when_it_is_painted_and_has_two_points'),
    Mutation('drawings: forget that a path clips', 'src/redact.rs', '                clips = true;', '                clips = false;', 'a_path_is_counted_when_it_is_painted_and_has_two_points'),
    Mutation('drawings: stop painting and keep the outline', 'src/redact.rs', '}\n        positions.extend(path.operations.iter().copied());', '}\n        positions.extend(path.operations.last().copied());', 'removing_a_path_takes_its_outline_and_leaves_its_neighbours'),
    Mutation('drawings: remove by position on a count that disagrees', 'src/redact.rs', '    if painted.len() != path_objects {', '    if false {', 'a_disagreeing_path_count_refuses_only_a_real_removal'),
    Mutation('drawings: remove a path that also clips', 'src/redact.rs', '        if path.clips {\n            return Err(', '        if false {\n            return Err(', 'a_path_that_also_clips_is_refused_by_the_writer_and_left_by_the_planner'),
    Mutation('drawings: plan the removal of a path that clips', 'src/redact.rs', '            Some(Some(true)) => CLIP_PATH,', '            Some(Some(true)) => return true,', 'a_path_that_also_clips_is_refused_by_the_writer_and_left_by_the_planner'),
    Mutation('drawings: call an unplaced path a clipping one', 'src/redact.rs', '            Some(None) | None => UNPLACED_PATH,', '            Some(None) | None => CLIP_PATH,', 'paths_that_cannot_be_placed_are_all_left_and_say_so'),
    Mutation('drawings: answer for a page whose counts disagree', 'src/redact.rs', '    (painted.len() == expected).then(|| painted.iter()', '    true.then(|| painted.iter()', 'paths_that_cannot_be_placed_are_all_left_and_say_so'),
    Mutation("drawings: leave a figure's description behind", 'src/redact.rs', '    let cleared = clear_shadow_text(doc, page, &mut content.operations, &positions)?;', '    let cleared = clear_shadow_text(doc, page, &mut content.operations, &[])?;', 'a_figure_s_description_goes_with_the_drawing_it_describes'),
    Mutation('drawings: leave them out of the count a reader is shown', 'src/redact.rs', '    paths.dedup();\n    total += paths.len();', '    paths.dedup();', 'two_regions_holding_one_drawing_remove_it_once'),
    Mutation('drawings: remove one drawing once per region', 'src/redact.rs', '    paths.sort_unstable();\n    paths.dedup();\n    total += paths.len();', '    paths.sort_unstable();\n    total += paths.len();', 'two_regions_holding_one_drawing_remove_it_once'),
    Mutation("drawings: lose the page's path count in the merge", 'src/redact.rs', '        path_objects = plan.path_objects;\n', '', 'two_regions_holding_one_drawing_remove_it_once'),
    Mutation('drawings: the writer never asks for them', 'src/save.rs', '            &redaction.paths,\n            &redaction.cuts,', '            &[],\n            &redaction.cuts,', 'a_rewrite_takes_a_planned_drawing_out_of_the_page'),
    Mutation('drawings: say a rule is of kind path', 'src/redact.rs', '            ("path", None) => {', '            ("no such kind", None) => {', 'an_object_this_cannot_remove_makes_the_plan_incomplete'),
    Mutation('what is left: print the kind as the program spells it', 'src/redact.rs', '        let what = said(kind);', '        let what = kind;', 'a_repeated_object_says_why_it_stayed_and_an_ordinary_one_does_not'),
    Mutation('what is left: have no words for a drawing that clips', 'src/redact.rs', '        CLIP_PATH | UNPLACED_PATH => "a drawing",', '', 'a_repeated_object_says_why_it_stayed_and_an_ordinary_one_does_not'),
    Mutation('gate: refuse every region that held no text', 'src/ocr.rs', '    let from_page = covered.is_empty();', '    let from_page = false;', 'a_region_over_no_words_is_sized_from_the_smallest_word_left_that_can_be_a_control'),
    Mutation('gate: size a wordless region from a word too short to read back', 'src/ocr.rs', '            .filter(|word| longest_run(&word.text).chars().count() >= MIN_CONTROL_CHARS)\n            .map(|word| word.rect)', '            .map(|word| word.rect)', 'a_region_over_no_words_is_sized_from_the_smallest_word_left_that_can_be_a_control'),
    Mutation("gate: forget that the size was the page's", 'src/ocr.rs', '        size_pt,\n        from_page,\n    })', '        size_pt,\n        from_page: false,\n    })', 'a_region_over_no_words_is_sized_from_the_smallest_word_left_that_can_be_a_control'),
    Mutation('gate: note every page, sized from the page or not', 'src/ocr_gate.rs', '            choice.from_page.then(|| {', '            true.then(|| {', 'a_page_whose_regions_held_no_text_gets_a_note_naming_the_size'),
    Mutation('cli wording: drop the note from a clean verdict', 'src/words.rs', '            format!("{clean} Note: {}.", notes.join("; "))', '            clean', 'every_json_shape_and_the_wording_match_their_committed_samples'),
    Mutation('edge cut: keep the rule inside the region', 'src/pathcut.rs', '        let before = (low, high.min(region_a.0 - pull));', '        let before = (low, high);', 'a_rule_is_cut_at_both_edges_of_the_region_that_crosses_it'),
    Mutation('edge cut: ignore how far a cap reaches', 'src/pathcut.rs', '    let pull = if cap_reaches { half } else { 0.0 };', '    let pull = 0.0;', 'a_cap_that_reaches_past_its_end_is_pulled_back_by_that_reach'),
    Mutation('edge cut: cut a rule the region covers half the thickness of', 'src/pathcut.rs', '        if !(region_p.0 <= p - half + eps && p + half - eps <= region_p.1) {', '        if false {', 'a_region_that_takes_only_part_of_a_rule_s_thickness_is_not_cut'),
    Mutation('edge cut: skip the check of the result', 'src/pathcut.rs', '                    .any(|region| inks_inside(&polys, *half, *cap_reaches, *region, eps))', '                    .any(|_| false)', 'a_join_that_still_reaches_into_the_region_is_not_cut'),
    Mutation('edge cut: forget what Q restores', 'src/pathcut.rs', '                    state = saved;', '                    let _ = saved;', 'the_line_width_is_the_one_in_effect_where_the_path_is_painted'),
    Mutation('edge cut: multiply two matrices the other way', 'src/pathcut.rs', '                    (Some(matrix), Some(ctm)) => Some(concat(matrix, ctm)),', '                    (Some(matrix), Some(ctm)) => Some(concat(ctm, matrix)),', 'the_region_is_carried_into_the_space_the_path_is_drawn_in'),
    Mutation("edge cut: ignore a named state's line width", 'src/pathcut.rs', '                            state.width = resolved(doc, width).and_then(number);', '                            let _ = width;', 'the_line_width_is_the_one_in_effect_where_the_path_is_painted'),
    Mutation('edge cut: keep the old state when a named one cannot be found', 'src/pathcut.rs', '                    None => {\n                        state.width = None;\n                        state.cap_reaches = None;\n                        state.dashed = None;\n                    }', '                    None => {}', 'the_line_width_is_the_one_in_effect_where_the_path_is_painted'),
    Mutation('edge cut: cut a dashed rule', 'src/pathcut.rs', '            if state.dashed != Some(false) {', '            if false {', 'what_cannot_be_split_exactly_is_not_cut'),
    Mutation('edge cut: cut a hairline', 'src/pathcut.rs', '            let width = state.width.filter(|width| *width > 0.0)?;', '            let width = state.width?;', 'what_cannot_be_split_exactly_is_not_cut'),
    Mutation('edge cut: cut a line off the axes', 'src/pathcut.rs', '                    if (from.0 == to.0) == (from.1 == to.1) {', '                    if false {', 'what_cannot_be_split_exactly_is_not_cut'),
    Mutation('edge cut: cut under a skew', 'src/pathcut.rs', '    straight || turned\n', '    straight || turned || true\n', 'the_region_is_carried_into_the_space_the_path_is_drawn_in'),
    Mutation('edge cut: cut rectangles whose fill is not their union', 'src/pathcut.rs', '            if rects.len() > 1 && (paint.operator == "f*" || !one_way) {', '            if false {', 'several_filled_rectangles_are_cut_only_when_their_fill_is_their_union'),
    Mutation('edge cut: lose the piece above the region', 'src/pathcut.rs', '    if rect[3] - region[3] > eps {', '    if false {', 'a_filled_rectangle_loses_what_the_region_covers'),
    Mutation('edge cut: write a clockwise rectangle the other way', 'src/pathcut.rs', '                        if *clockwise {', '                        if false {', 'a_filled_rectangle_loses_what_the_region_covers'),
    Mutation('edge cut: answer for a path with an operator inside it', 'src/pathcut.rs', '            if last - first + 1 != path.len() {', '            if false {', 'a_path_whose_operators_are_not_next_to_each_other_is_not_cut'),
    Mutation('edge cut: open a closed run at its first corner', 'src/pathcut.rs', '        if starts && ends {', '        if false {', 'a_stroked_rectangle_keeps_its_corners_joined_around_a_cut'),
    Mutation("edge cut: cut the page's rule for a drawing inside a block", 'src/redact.rs', '                .is_some_and(|object| object.kind == "path");', '                .is_some();', 'a_drawing_inside_a_reusable_block_is_not_one_of_the_page_s_paths'),
    Mutation('edge cut: name a crossing path by its place among all objects', 'src/redact.rs', '        let ordinal = objects[..left.at]\n            .iter()\n            .filter(|object| object.kind == "path")\n            .count();', '        let ordinal = left.at;', 'a_drawing_that_crosses_the_region_is_cut_when_it_can_be'),
    Mutation('edge cut: plan no cut', 'src/redact.rs', '            pathcut::Verdict::Cut(_) => cuts.push(ordinal),', '            pathcut::Verdict::Cut(_) => {}', 'a_drawing_that_crosses_the_region_is_cut_when_it_can_be'),
    Mutation('edge cut: drop a rule that cannot be cut from the report', 'src/redact.rs', '            pathcut::Verdict::Unsupported => return true,', '            pathcut::Verdict::Unsupported => {}', 'a_rule_the_region_covers_part_of_the_thickness_of_is_still_reported'),
    Mutation('edge cut: keep reporting a rule whose ink is outside the region', 'src/redact.rs', 'gone.push(ordinal),\n            pathcut::Verdict::Outside => {}', 'gone.push(ordinal),\n            pathcut::Verdict::Outside => return true,', 'a_drawing_that_crosses_the_region_is_cut_when_it_can_be'),
    Mutation('edge cut: drop a curve from the report', 'src/redact.rs', '        let Some(Some(drawing)) = drawings.get(ordinal) else {\n            return true;\n        };', '        let Some(Some(drawing)) = drawings.get(ordinal) else {\n            return false;\n        };', 'a_drawing_that_crosses_the_region_is_cut_when_it_can_be'),
    Mutation('edge cut: forget a rule whose ink is all inside', 'src/redact.rs', '            pathcut::Verdict::Gone => gone.push(ordinal),', '            pathcut::Verdict::Gone => {}', 'a_rule_whose_ink_the_region_holds_goes_whole'),
    Mutation('edge cut: cut a path that is also removed', 'src/redact.rs', '        if wanted.binary_search(ordinal).is_err() {', '        if true {', 'a_path_that_is_both_removed_and_cut_is_removed'),
    Mutation('edge cut: delete a cut path instead of rewriting it', 'src/redact.rs', '            .position(|(range, _)| *range.end() == position)', '            .position(|(range, _)| *range.end() == position && false)', 'cutting_a_path_rewrites_it_where_it_stood_and_leaves_its_neighbours'),
    Mutation('edge cut: count a path that went whole as cut', 'src/redact.rs', '        cut: rewritten.len(),', '        cut: crossed.len(),', 'a_cut_whose_regions_together_hold_all_the_ink_removes_the_path'),
    Mutation('edge cut: cut a path another region removes', 'src/redact.rs', '    cuts.retain(|(ordinal, _)| paths.binary_search(ordinal).is_err());\n', '', 'two_regions_over_one_rule_cut_it_twice_and_count_it_once'),
    Mutation('edge cut: leave cuts out of the count a reader is shown', 'src/redact.rs', '    total += cut_paths(&cuts);\n', '', 'two_regions_over_one_rule_cut_it_twice_and_count_it_once'),
    Mutation('edge cut: lose the cuts in the merge', 'src/redact.rs', '        cuts,\n    };\n    PageAggregate {', '        cuts: Vec::new(),\n    };\n    PageAggregate {', 'two_regions_over_one_rule_cut_it_twice_and_count_it_once'),
    Mutation('edge cut: the writer never asks for them', 'src/save.rs', '            &redaction.cuts,\n            redaction.path_objects,', '            &[],\n            redaction.path_objects,', 'a_rewrite_cuts_a_planned_rule_at_the_region_s_edge'),
    Mutation('edge cut: the fill pass asks for them again', 'src/redaction_fill.rs', '                    cuts: Vec::new(),\n', '                    cuts: redaction.cuts.clone(),\n', 'maps_reordered_pages_without_reapplying_turns_crops_or_removals'),
    Mutation('compress: scale a picture already coarse enough', 'src/compress.rs', '    if at <= dpi * SLACK {', '    if false {', 'a_side_is_scaled_only_past_the_slack_and_never_up'),
    Mutation("compress: read a turned picture's width off the diagonal", 'src/compress.rs', '                        entry.width = entry.width.max(a.hypot(b));', '                        entry.width = entry.width.max(a.abs());', 'a_picture_is_shown_as_large_as_the_largest_matrix_it_is_drawn_under'),
    Mutation('compress: forget the matrix Q restores', 'src/compress.rs', '                        ctm = saved;', '                        let _ = saved;', 'a_picture_is_shown_as_large_as_the_largest_matrix_it_is_drawn_under'),
    Mutation("compress: ignore a block's own matrix", 'src/compress.rs', '                    .map_or(Some(IDENTITY), |matrix| matrix_of(matrix));', '                    .map_or(Some(IDENTITY), |_| Some(IDENTITY));', 'a_picture_is_shown_as_large_as_the_largest_matrix_it_is_drawn_under'),
    Mutation('compress: shrink a stencil mask', 'src/compress.rs', '    if flag(b"ImageMask") || whole(b"BitsPerComponent") != Some(8) {', '    if whole(b"BitsPerComponent") != Some(8) {', 'what_could_change_by_more_than_resolution_is_left_as_it_is'),
    Mutation('compress: shrink a picture of one bit a sample', 'src/compress.rs', '    if flag(b"ImageMask") || whole(b"BitsPerComponent") != Some(8) {', '    if flag(b"ImageMask") {', 'what_could_change_by_more_than_resolution_is_left_as_it_is'),
    Mutation('compress: shrink a picture with a decode array', 'src/compress.rs', '    if dict.has(b"Decode") || dict.has(b"Mask") || dict.has(b"Matte") {', '    if dict.has(b"Mask") || dict.has(b"Matte") {', 'what_could_change_by_more_than_resolution_is_left_as_it_is'),
    Mutation('compress: shrink a picture with a colour key', 'src/compress.rs', '    if dict.has(b"Decode") || dict.has(b"Mask") || dict.has(b"Matte") {', '    if dict.has(b"Decode") || dict.has(b"Matte") {', 'what_could_change_by_more_than_resolution_is_left_as_it_is'),
    Mutation('compress: scale a mask blended against a matte', 'src/compress.rs', '    if dict.has(b"Decode") || dict.has(b"Mask") || dict.has(b"Matte") {', '    if dict.has(b"Decode") || dict.has(b"Mask") {', 'a_soft_mask_is_scaled_with_its_picture_and_never_stored_as_jpeg'),
    Mutation('compress: take a JPEG of other components for the picture', 'src/compress.rs', '            if coded != picture.channels || decoder.output_buffer_size() != Some(wanted) {', '            if decoder.output_buffer_size() != Some(wanted) {', 'a_jpeg_drawn_too_fine_is_decoded_scaled_and_encoded_again'),
    Mutation('compress: encode a JPEG again without scaling it', 'src/compress.rs', '    if !scaling && picture.stored == Stored::Jpeg {', '    if false {', 'a_picture_already_coarse_enough_is_not_scaled'),
    Mutation('compress: hand every picture to JPEG', 'src/compress.rs', '        || (how.jpeg && lossless.len() as f64 > pixels.len() as f64 * PHOTO_SHARE);', '        || how.jpeg;', 'a_smooth_drawing_stays_lossless_though_a_jpeg_of_it_would_be_smaller'),
    Mutation('compress: store a photograph as JPEG when told not to', 'src/compress.rs', '        || (how.jpeg && lossless.len() as f64 > pixels.len() as f64 * PHOTO_SHARE);', '        || (lossless.len() as f64 > pixels.len() as f64 * PHOTO_SHARE);', 'settings_chosen_freely_are_followed_and_bounded'),
    Mutation('compress: store a soft mask as JPEG', 'src/compress.rs', '    let lossy = (photo && !is_mask)', '    let lossy = photo', 'a_soft_mask_is_scaled_with_its_picture_and_never_stored_as_jpeg'),
    Mutation('compress: keep a new form that is hardly smaller', 'src/compress.rs', '    if new_len * 10 > stream.content.len() * 9 {', '    if false {', 'a_picture_whose_new_form_is_hardly_smaller_is_left'),
    Mutation('compress: deflate an XMP packet', 'src/compress.rs', '        if metadata || !stream.allows_compression {', '        if !stream.allows_compression {', 'without_a_preset_no_picture_is_touched_and_plain_streams_are_deflated'),
    Mutation('compress: deflate again under two filters', 'src/compress.rs', '            array.len() == 1 && array[0].as_name().ok() == Some(b"FlateDecode")', '            array[0].as_name().ok() == Some(b"FlateDecode")', 'a_deflated_stream_is_deflated_again_only_when_that_is_smaller'),
    Mutation('compress: never deflate a deflated stream again', 'src/compress.rs', '    if !one {', '    if true {', 'a_deflated_stream_is_deflated_again_only_when_that_is_smaller'),
    Mutation('compress: accept any resolution', 'src/compress.rs', '        if !DPI_RANGE.contains(&self.dpi) {', '        if false {', 'settings_chosen_freely_are_followed_and_bounded'),
    Mutation('compress: accept any quality', 'src/compress.rs', '        if !(1..=100).contains(&self.quality) {', '        if false {', 'settings_chosen_freely_are_followed_and_bounded'),
    Mutation('compress: do the work when none was asked for', 'src/compress.rs', '        Compress::No => return done,', '        Compress::No => {}', 'without_a_preset_no_picture_is_touched_and_plain_streams_are_deflated'),
    Mutation('compress: sample the first window, not the one that differs most', 'src/compress.rs', '                if best.is_none_or(|(most, ..)| sum > most) {', '                if best.is_none() {', 'a_sample_is_the_part_of_the_page_that_differs_most'),
    Mutation('compress: sample a page that did not change', 'src/compress.rs', '        if most == 0 {', '        if false {', 'a_sample_is_the_part_of_the_page_that_differs_most'),
    Mutation('compress: look at the page that lost least', 'src/compress.rs', '        .max_by_key(|(_, _, old, new)| (old.0 * old.1 - new.0 * new.1, old.0 * old.1))?;', '        .min_by_key(|(_, _, old, new)| (old.0 * old.1 - new.0 * new.1, old.0 * old.1))?;', 'an_estimate_writes_nothing_and_names_the_page_that_loses_most'),
    Mutation('compress: the rewrite never makes the copy smaller', 'src/save.rs', '    let _ = crate::compress::apply(&mut doc, plan.compress);', '    let _ = crate::compress::apply(&mut doc, crate::compress::Compress::No);', 'a_smaller_copy_is_packed_shrinks_its_pictures_and_keeps_every_page'),
    Mutation('compress: write a smaller copy without object streams', 'src/save.rs', '        serialise_packed(&mut doc, "the document")?', '        serialise(&mut doc, "the document")?', 'a_smaller_copy_is_packed_shrinks_its_pictures_and_keeps_every_page'),
    Mutation('compress: call a plan for a smaller copy the file itself', 'src/edits.rs', '        if self.compress != crate::compress::Compress::No {\n            return false;\n        }\n', '', 'a_plan_that_asks_for_a_smaller_copy_is_neither_the_file_nor_an_append'),
    Mutation('compress: append a smaller copy', 'src/edits.rs', '            && self.compress == crate::compress::Compress::No\n', '', 'a_plan_that_asks_for_a_smaller_copy_is_neither_the_file_nor_an_append'),
    Mutation('compress: start a lone number from another preset', 'src/cli/compress.rs', '        let mut pictures = Pictures::from(preset.unwrap_or(Preset::Balanced));', '        let mut pictures = Pictures::from(preset.unwrap_or(Preset::Print));', 'a_preset_names_three_numbers_and_each_can_be_given_alone'),
    Mutation('compress: ignore --no-jpeg', 'src/cli/compress.rs', '        pictures.jpeg = !no_jpeg;', '        pictures.jpeg = true;', 'a_preset_names_three_numbers_and_each_can_be_given_alone'),
    Mutation('compress: round the share saved up', 'src/cli/compress.rs', '    (before - after) * 100 / before', '    ((before - after) * 100).div_ceil(before)', 'the_share_saved_is_rounded_down_and_never_negative'),
    Mutation('compress: the window sends settings unchecked', 'src/commands/compress.rs', '        Some(pictures) => Compress::Pictures(pictures.checked()?),', '        Some(pictures) => Compress::Pictures(pictures),', 'no_settings_is_lossless_and_settings_are_checked'),
]

#: The script a control proves. `ocr::hold_to_scripts` lets a clean verdict stand
#: only for the scripts the control word is written in; before it, a Latin
#: control read back beside a region still showing a line of Chinese certified
#: the region, because Vision returns no span at all for one.
ROWS += [
    Mutation(
        # Let a control vouch for every script. This is the gate as it stood:
        # the engine read "quartz", said nothing about the rest, and the rest
        # was called unreadable.
        "scripts: let a control vouch for a script it is not written in",
        "src/ocr.rs",
        "    if unproven.is_empty() {\n        return verdict;\n    }",
        "    if true {\n        return verdict;\n    }",
        "a_clean_verdict_stands_only_for_the_scripts_the_control_is_written_in",
    ),
    Mutation(
        # Leave the rule out of the gate's verdict. Every test of the rule
        # itself still passes, and no region is ever held to it.
        "scripts: leave the script rule out of the gate's verdict",
        "src/ocr_gate.rs",
        "            crate::ocr::hold_to_scripts(read, placed, &covered_by(page, region))",
        "            read",
        "a_latin_control_does_not_certify_a_region_that_held_another_script",
    ),
    Mutation(
        # Hold a region to every word on its page. Nothing unsafe comes of it,
        # and every page that mixes two scripts stops being verifiable.
        "scripts: hold a region to the words of the whole page",
        "src/ocr_gate.rs",
        "        .filter(|word| crate::objects::overlaps(word.rect, region))\n        .map(|word| word.text.as_str())",
        "        .map(|word| word.text.as_str())",
        "only_the_words_under_the_region_decide_its_script",
    ),
    Mutation(
        # Put every script without a name of its own in one bucket, so a Tamil
        # control vouches for Telugu.
        "scripts: read every unnamed script as one",
        "src/ocr.rs",
        "        other => Script::Other(other >> 7),",
        "        _ => Script::Other(0),",
        "a_character_is_of_one_script_or_of_none",
    ),
    Mutation(
        # Call a digit of any script scriptless, as the European ones are. A
        # Latin control then vouches for a number set in Arabic-Indic digits.
        "scripts: read a digit of another script as of none",
        "src/ocr.rs",
        "    if !ch.is_alphanumeric() {",
        "    if !ch.is_alphabetic() {",
        "a_character_is_of_one_script_or_of_none",
    ),
    Mutation(
        # Answer every request the plain way in the child. The gate asks for
        # any script, the flag crosses the pipe, and the engine is never told.
        "scripts: drop the any-script flag in the OCR child",
        "src/ocr_worker.rs",
        "    let read = if ask.any_script {",
        "    let read = if false {",
        "the_child_asks_the_engine_the_way_the_request_says",
    ),
    Mutation(
        # Slice a page's boxes by its character count, as before. The two
        # arrays come from a worker; a short one is a panic in the app process.
        "gate: index a page's boxes without checking there is one",
        "src/ocr_gate.rs",
        "        let Some(b) = page.boxes.get(i * 4..i * 4 + 4) else {\n            continue;\n        };",
        "        let b = &page.boxes[i * 4..i * 4 + 4];",
        "a_page_with_fewer_boxes_than_characters_places_the_words_it_can",
    ),
    Mutation(
        # Make a subroutine plain again at every call. The operation budget
        # still holds, and the work behind one operation is the Subr's length.
        "charstrings: make a subroutine plain at every call",
        "src/textedit/fonts/type1/program.rs",
        "                    let subr = subr.as_deref().ok_or(INVALID)?;\n                    if self.execute(subr, depth + 1)? {",
        "                    let subr = plain(None, subr.as_deref().ok_or(INVALID)?)?;\n                    if self.execute(&subr, depth + 1)? {",
        "a_glyph_that_calls_a_large_subroutine_many_times_is_parsed_in_bounded_work",
    ),
    Mutation(
        # Run a Subr too short to decrypt as an empty one, so the glyph that
        # calls it is offered with an outline nobody read.
        "type1: run a Subr shorter than lenIV as an empty one",
        "src/textedit/fonts/type1/program.rs",
        "                subrs[index] = plain(len_iv, cursor.binary()?).ok();",
        "                subrs[index] = plain(len_iv, cursor.binary()?).ok().or(Some(Vec::new()));",
        "a_subr_shorter_than_leniv_fails_only_the_glyph_that_calls_it",
    ),
]

# Forgetting remembered documents (2026-10-05): one place, or all of them, and
# nothing else the session holds.
ROWS += [
    Mutation("session: forgetting a document forgets nothing", "src/session.rs", "        self.places.retain(|kept| kept.path != path);\n", "", "forgetting_a_document_removes_its_place_and_no_other"),
    Mutation("session: forget every document whose path begins the same way", "src/session.rs", "        self.places.retain(|kept| kept.path != path);", "        self.places.retain(|kept| !kept.path.starts_with(path));", "forgetting_a_document_that_is_not_remembered_changes_nothing"),
    Mutation("session: forgetting a document closes its tab", "src/session.rs", "        self.places.retain(|kept| kept.path != path);", "        self.places.retain(|kept| kept.path != path);\n        self.tabs.retain(|kept| kept != path);", "forgetting_a_document_leaves_it_open_as_a_tab"),
    Mutation("session: clearing the places resets the whole session", "src/session.rs", "        self.places.clear();", "        *self = Self::default();", "clearing_the_places_keeps_the_preferences_and_the_tabs"),
    Mutation("session: clearing the places clears nothing", "src/session.rs", "        self.places.clear();\n", "", "clearing_the_places_keeps_the_preferences_and_the_tabs"),
    Mutation("session: nest the session inside the launch reply", "src/session.rs", "    #[serde(flatten)]\n    pub session: Session,", "    pub session: Session,", "the_launch_reply_carries_home_beside_the_session_s_own_keys"),
]

# The lines of a text box's words while they are typed (2026-10-05): one
# function turns a box into a wrap width, for the model's lines and for a
# draft's, and a draft is bounded as a note is.
ROWS += [
    Mutation(
        "text box: wrap a box at its whole width",
        "src/textbox.rs",
        "    let width = f64::from(right - left) - INSET * 2.0;",
        "    let width = f64::from(right - left);",
        "a_box_wraps_at_its_width_less_the_inset_on_both_sides_at_the_fixed_size",
    ),
    Mutation(
        "text box: take the inset off one side only",
        "src/textbox.rs",
        "    let width = f64::from(right - left) - INSET * 2.0;",
        "    let width = f64::from(right - left) - INSET;",
        "a_box_wraps_at_its_width_less_the_inset_on_both_sides_at_the_fixed_size",
    ),
    Mutation(
        "text box: wrap a box's words at another size",
        "src/textbox.rs",
        "    wrap(text, SIZE, width.max(1.0))",
        "    wrap(text, SIZE - 1.0, width.max(1.0))",
        "a_box_wraps_at_its_width_less_the_inset_on_both_sides_at_the_fixed_size",
    ),
    Mutation(
        # The draft takes a path of its own, with the inset left out: what a
        # reader sees while typing breaks somewhere the model will not.
        "draft: wrap a draft at the box's whole width",
        "src/edits.rs",
        "    Ok(textbox::box_lines(note, left, right))",
        "    Ok(textbox::wrap(note, textbox::SIZE, f64::from(right - left)))",
        "a_draft_breaks_where_the_model_breaks_the_same_note",
    ),
    Mutation(
        # The same drift from the other side: the model's lines leave the path
        # the draft takes.
        "edits: wrap a text box's own lines at its whole width",
        "src/edits.rs",
        "                    textbox::box_lines(model.note_of(id), left, right)",
        "                    textbox::wrap(model.note_of(id), textbox::SIZE, f64::from(right - left))",
        "a_draft_breaks_where_the_model_breaks_the_same_note",
    ),
    Mutation(
        "edits: wrap a text box's own lines between its top and bottom",
        "src/edits.rs",
        "                        .map_or((0.0, 0.0), |q| (q.left, q.right));",
        "                        .map_or((0.0, 0.0), |q| (q.top, q.bottom));",
        "a_draft_breaks_where_the_model_breaks_the_same_note",
    ),
    Mutation(
        "draft: wrap a draft of any length",
        "src/edits.rs",
        "    too_long(note)?;\n    Ok(textbox::box_lines(note, left, right))",
        "    Ok(textbox::box_lines(note, left, right))",
        "a_draft_past_the_note_bound_is_refused_and_one_at_it_is_not",
    ),
    Mutation(
        "draft: refuse a draft Helvetica cannot write",
        "src/edits.rs",
        "    too_long(note)?;\n    Ok(textbox::box_lines(note, left, right))",
        "    too_long(note)?;\n    if !textbox::encodable(note) {\n        return Err(String::new());\n    }\n    Ok(textbox::box_lines(note, left, right))",
        "a_draft_past_the_note_bound_is_refused_and_one_at_it_is_not",
    ),
    Mutation(
        "ocr language: take a language the machine does not offer",
        "src/ocr.rs",
        "            .find(|have| have.eq_ignore_ascii_case(want))\n            .cloned()",
        "            .find(|have| have.eq_ignore_ascii_case(want))\n            .cloned()\n            .or_else(|| Some(want.clone()))",
        "the_first_language_asked_for_that_is_offered_is_the_one_used",
    ),
    Mutation(
        "ocr language: compare tags by case",
        "src/ocr.rs",
        "            .find(|have| have.eq_ignore_ascii_case(want))",
        "            .find(|have| have == &want)",
        "the_first_language_asked_for_that_is_offered_is_the_one_used",
    ),
    Mutation(
        "ocr language: report every reading under the worker's first identity",
        "src/ocr_worker.rs",
        "            engine: engine.id_for(&ask.options).map_or_else(",
        "            engine: None::<EngineId>.map_or_else(",
        "a_reading_is_reported_under_the_identity_its_options_gave_it",
    ),
    Mutation(
        "ocr language: let an underscore into a tag",
        "src/ocr_layer.rs",
        "        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')",
        "        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')",
        "a_language_tag_is_letters_digits_and_hyphens_after_a_letter",
    ),
    Mutation(
        "ocr language: let a tag start with a digit",
        "src/ocr_layer.rs",
        "        && raw.starts_with(|c: char| c.is_ascii_alphabetic())",
        "        && raw.starts_with(|c: char| c.is_ascii_alphanumeric())",
        "a_language_tag_is_letters_digits_and_hyphens_after_a_letter",
    ),
    Mutation(
        "ocr language: let a tag be any length",
        "src/ocr_layer.rs",
        "    (2..=35).contains(&raw.len())",
        "    (2..=350).contains(&raw.len())",
        "a_language_tag_is_letters_digits_and_hyphens_after_a_letter",
    ),
    Mutation(
        "ocr language: send the engine a language it stopped offering",
        "src/ocr_layer.rs",
        "        None => Choice {\n            languages: Vec::new(),\n            unavailable: Some(asked.to_string()),",
        "        None => Choice {\n            languages: vec![asked.to_string()],\n            unavailable: Some(asked.to_string()),",
        "a_remembered_language_is_asked_for_only_while_the_machine_offers_it",
    ),
    Mutation(
        "ocr language: do not say which language was missing",
        "src/ocr_layer.rs",
        "            languages: Vec::new(),\n            unavailable: Some(asked.to_string()),",
        "            languages: Vec::new(),\n            unavailable: None,",
        "a_remembered_language_is_asked_for_only_while_the_machine_offers_it",
    ),
    Mutation(
        "ocr language: ask in the reader's spelling, not the engine's",
        "src/ocr_layer.rs",
        "        Some(tag) => Choice {\n            languages: vec![tag],",
        "        Some(_) => Choice {\n            languages: vec![asked.to_string()],",
        "a_remembered_language_is_asked_for_only_while_the_machine_offers_it",
    ),
    Mutation(
        "ocr language: send the missing language in Rust's spelling",
        "src/commands/ocr.rs",
        "    #[serde(skip_serializing_if = \"Option::is_none\")]\n    pub language_unavailable: Option<String>,",
        "    #[serde(skip_serializing_if = \"Option::is_none\", rename = \"language_unavailable\")]\n    pub language_unavailable: Option<String>,",
        "the_report_reaches_the_window_in_its_own_spelling",
    ),
    Mutation(
        "ocr language: send a null for no missing language",
        "src/commands/ocr.rs",
        "    #[serde(skip_serializing_if = \"Option::is_none\")]\n    pub language_unavailable: Option<String>,",
        "    pub language_unavailable: Option<String>,",
        "the_report_reaches_the_window_in_its_own_spelling",
    ),
    Mutation(
        # Inverted rather than `true`: on Windows `cfg!(windows)` is already
        # true, so `true` changed nothing there and survived on 2026-10-21.
        "ocr language: offer installing more languages on the wrong platforms",
        "src/commands/ocr.rs",
        "        installable: cfg!(windows),",
        "        installable: !cfg!(windows),",
        "only_windows_tells_the_reader_more_languages_can_be_installed",
    ),
    Mutation(
        "ocr language: forget the choice on load",
        "src/session.rs",
        "        .with_ocr_language(session.ocr_language)",
        "        .with_ocr_language(None)",
        "the_language_to_recognise_text_in_survives_a_round_trip",
    ),
    Mutation(
        "ocr language: write a null for the engine's own choice",
        "src/session.rs",
        "    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n    pub ocr_language: Option<String>,",
        "    #[serde(default)]\n    pub ocr_language: Option<String>,",
        "the_language_to_recognise_text_in_survives_a_round_trip",
    ),
    Mutation(
        "ocr language: keep whatever the session file says",
        "src/session.rs",
        "        self.ocr_language = language.filter(|tag| crate::ocr_layer::is_language_tag(tag));",
        "        self.ocr_language = language;",
        "a_stored_language_that_is_not_a_tag_is_the_engine_s_own_choice",
    ),
    Mutation(
        "ocr language: list no language on macOS",
        "src/ocr_vision.rs",
        "        Ok(listed.iter().map(|tag| tag.to_string()).collect())",
        "        Ok(listed.iter().map(|tag| tag.to_string()).take(0).collect())",
        "the_engine_lists_the_languages_it_can_be_asked_for",
    ),
]

# A page the recogniser refuses (26.10.8). `ocr_layer::outcome_of` decides which
# of the engine's errors is one page's own, and the two callers file the page
# and say so. What needs an engine --- that a refused page costs the page
# beside it nothing, and that the tool's walk and the window's file it alike
# --- is `tests/cli/ocr.rs`, which this table cannot select. Six mutations were
# run there by hand on 2026-10-06, each restored byte for byte, each red:
# `outcome_of` without its `Rejected` arm (the two checks that a copy is
# written); `Outcome::Refused` pushed to `nothing_read` in `cli/ocr.rs` and then
# in `commands/ocr.rs` (the check that holds the two to each other); and the
# three `refused.is_empty()` tests below turned off (the three wording checks).
ROWS += [
    Mutation(
        "ocr refused: fail the document for a page the engine rejected",
        "src/ocr_layer.rs",
        "        Err(RecogniseError::Rejected(_)) => Ok(Outcome::Refused),\n",
        "",
        "only_a_page_the_engine_rejected_is_passed_over",
    ),
    Mutation(
        "ocr refused: pass over a page whatever the engine reported",
        "src/ocr_layer.rs",
        "        Err(other) => Err(other),\n",
        "        Err(_) => Ok(Outcome::Refused),\n",
        "only_a_page_the_engine_rejected_is_passed_over",
    ),
    Mutation(
        "ocr refused: call a refused page an empty one",
        "src/ocr_layer.rs",
        "        Err(RecogniseError::Rejected(_)) => Ok(Outcome::Refused),\n",
        "        Err(RecogniseError::Rejected(_)) => Ok(Outcome::Nothing),\n",
        "only_a_page_the_engine_rejected_is_passed_over",
    ),
    Mutation(
        "ocr refused: put every layer on the first page",
        "src/ocr_layer.rs",
        "            Ok(match layer_of(page, new) {",
        "            Ok(match layer_of(0, new) {",
        "an_answer_is_a_layer_on_its_own_page_or_nothing",
    ),
    Mutation(
        "ocr refused: name every page however many there are",
        "src/ocr_layer.rs",
        "    const SHOWN: usize = 8;\n",
        "    const SHOWN: usize = 8000;\n",
        "pages_are_named_up_to_eight",
    ),
    Mutation(
        "ocr refused: say pages for one page",
        "src/ocr_layer.rs",
        '        1 => format!("page {head}"),\n',
        '        0 => format!("page {head}"),\n',
        "pages_are_named_up_to_eight",
    ),
    Mutation(
        "ocr refused: say what a refusal means in other words",
        "src/ocr_layer.rs",
        '    "which usually means a script it cannot read or a scan too unclear to tell the script";',
        '    "which means a script it cannot read";',
        "the_sentences_name_what_was_left_alone_and_why",
    ),
    Mutation(
        "ocr refused: the tool says nothing about a refused page",
        "src/cli/ocr.rs",
        "    if !report.refused.is_empty() {",
        "    if false {",
        "the_sentences_name_what_was_left_alone_and_why",
    ),
    Mutation(
        "ocr refused: the tool leaves refused pages out of the count",
        "src/cli/ocr.rs",
        "            + report.nothing_read.len()\n            + report.refused.len(),",
        "            + report.nothing_read.len(),",
        "the_sentences_name_what_was_left_alone_and_why",
    ),
    Mutation(
        "ocr refused: the tool writes no copy without naming the refusal",
        "src/cli/ocr.rs",
        "    if !read.refused.is_empty() {",
        "    if false {",
        "a_document_with_no_layer_says_when_pages_were_refused",
    ),
    Mutation(
        "ocr refused: the tool's report leaves the list out",
        "src/cli/report.rs",
        "    pub refused: Vec<u32>,\n    /// Number of source signatures",
        "    #[serde(skip)]\n    pub refused: Vec<u32>,\n    /// Number of source signatures",
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    Mutation(
        "ocr refused: the window writes no copy without naming the refusal",
        "src/commands/ocr.rs",
        "    if !report.refused.is_empty() {",
        "    if false {",
        "the_reason_nothing_was_added_names_the_pages_the_recogniser_refused",
    ),
    Mutation(
        "ocr refused: the window's reply leaves the list out",
        "src/commands/ocr.rs",
        "    pub refused: Vec<u32>,\n    /// Pages too large",
        "    #[serde(skip)]\n    pub refused: Vec<u32>,\n    /// Pages too large",
        "the_report_reaches_the_window_in_its_own_spelling",
    ),
    # A list item whose body holds its paragraph as a block of its own, the
    # shape LibreOffice writes (`docs/TEXTEDIT.md`, *Lists in paragraph styles
    # of their own*). `--only 'list paragraph:'` runs the set.
    Mutation(
        "list paragraph: refuse a paragraph in a list body",
        "src/textedit/tagging.rs",
        '            if plain.tag == b"LBody" && block {\n                paragraphs += 1;',
        '            if false {\n                paragraphs += 1;',
        "textedit_list_bodies_hold_one_paragraph_under_its_own_or_a_producer_name",
    ),
    Mutation(
        # Whatever a body holds is taken for its paragraph. A name the RoleMap
        # does not make a text block is still refused, as a leaf beside the
        # paragraph it was counted as, so what goes red is the body's own
        # inline leaf, the list every other producer writes.
        "list paragraph: any element in a list body is its paragraph",
        "src/textedit/tagging.rs",
        '            if plain.tag == b"LBody" && block {\n                paragraphs += 1;',
        '            if plain.tag == b"LBody" {\n                paragraphs += 1;',
        "textedit_list_bodies_still_validate_their_own_inline_leaves",
    ),
    Mutation(
        # The paragraph is claimed without being read, so a Span that holds its
        # words reaches the walk as a reference where marked content belongs.
        "list paragraph: its leaves are not read",
        "src/textedit/tagging.rs",
        '                || (plain.tag == b"LBody" && block)\n',
        "",
        "textedit_a_list_paragraph_holds_leaves_and_no_block",
    ),
    Mutation(
        "list paragraph: a second paragraph shares the body",
        "src/textedit/tagging.rs",
        "    paragraphs == 0 || (paragraphs == 1 && !own)",
        "    paragraphs == 0 || !own",
        "textedit_list_bodies_refuse_a_second_paragraph_and_words_beside_one",
    ),
    Mutation(
        "list paragraph: the body's words sit beside its paragraph",
        "src/textedit/tagging.rs",
        "    paragraphs == 0 || (paragraphs == 1 && !own)",
        "    paragraphs == 0 || paragraphs == 1",
        "textedit_list_bodies_refuse_their_own_words_beside_a_paragraph",
    ),
    Mutation(
        "list paragraph: an inline leaf beside it is not counted",
        "src/textedit/tagging.rs",
        '            leaves |= plain.tag == b"LBody" && !block;\n',
        "",
        "textedit_list_bodies_refuse_a_second_paragraph_and_words_beside_one",
    ),
    Mutation(
        "list paragraph: the body's own marked content is not counted",
        "src/textedit/tagging.rs",
        "body_holds_one_thing(paragraphs, leaves || !plain.items.is_empty())",
        "body_holds_one_thing(paragraphs, leaves)",
        "textedit_list_bodies_refuse_their_own_words_beside_a_paragraph",
    ),
    Mutation(
        # The paragraph becomes a block apart from its label, which a wrap would
        # then move without it. Nothing in `groups` knows the block; the walk
        # stamps the element it popped, and this stamps the claiming leaf.
        "list paragraph: its text is a block apart from its item",
        "src/textedit/tagging.rs",
        "                    blocks[mcid] = Some(block);",
        "                    blocks[mcid] = Some(if tag == b\"Standard\" { id } else { block });",
        "textedit_a_list_paragraph_belongs_to_its_item",
    ),
    # Acrobat's Fill & Sign: the marked sequence it appends to a page, and the
    # forms inside it. `--only 'fill and sign:'` runs the set.
    Mutation(
        "fill and sign: refuse the sequence it appends",
        "src/textedit/inspect.rs",
        '("BMC", [tag]) if !inside && tag.as_name().ok() == Some(FILL_SIGN) => {',
        '("BMC", [tag]) if false && tag.as_name().ok() == Some(FILL_SIGN) => {',
        "textedit_fill_and_sign_additions_are_kept_and_the_page_is_edited",
    ),
    Mutation(
        "fill and sign: the same tag on a tagged page is refused",
        "src/textedit/inspect.rs",
        '("BMC", [tag]) if !inside && tag.as_name().ok() == Some(FILL_SIGN) => {',
        '("BMC", [tag]) if false && tag.as_name().ok() == Some(FILL_SIGN) => {',
        "textedit_a_libreoffice_list_stays_editable_under_a_fill_and_sign_addition",
    ),
    Mutation(
        "fill and sign: the sequence opens inside a text object",
        "src/textedit/inspect.rs",
        '("BMC", [tag]) if !inside && tag.as_name().ok() == Some(FILL_SIGN) => {',
        '("BMC", [tag]) if tag.as_name().ok() == Some(FILL_SIGN) => {',
        "textedit_fill_and_sign_additions_are_kept_and_the_page_is_edited",
    ),
    Mutation(
        "fill and sign: the text it added is offered",
        "src/textedit/inspect.rs",
        "            || marked.placed > 0\n            || marked.filled;\n",
        "            || marked.placed > 0;\n",
        "textedit_fill_and_sign_additions_are_kept_and_the_page_is_edited",
    ),
    Mutation(
        "fill and sign: marked content opens inside it",
        "src/textedit/inspect.rs",
        '        if self.filled && matches!(op.operator.as_str(), "BDC" | "BMC") {',
        "        if false && self.filled {",
        "textedit_fill_and_sign_additions_are_kept_and_the_page_is_edited",
    ),
    Mutation(
        "fill and sign: a sequence left open is accepted",
        "src/textedit.rs",
        '    if marked.filled {\n        return Err("unterminated Fill & Sign marked content".into());\n    }\n',
        "",
        "textedit_fill_and_sign_additions_are_kept_and_the_page_is_edited",
    ),
    Mutation(
        "fill and sign: a form with its record is refused",
        "src/textedit/forms.rs",
        '(b"ADBE_FillSign", _) => {',
        '(b"ADBE_FillSign_", _) => {',
        "textedit_fill_and_sign_records_and_reversed_boxes_are_read_as_what_they_are",
    ),
    Mutation(
        "fill and sign: any dictionary under its key is its record",
        "src/textedit/forms.rs",
        'if data.get(b"Type").and_then(Object::as_name).ok() != Some(b"FillSignData") {',
        "if false {",
        "textedit_fill_and_sign_records_and_reversed_boxes_are_read_as_what_they_are",
    ),
    Mutation(
        "fill and sign: a form's box is read as its corners are written",
        "src/textedit/forms.rs",
        "        bounds[0].min(bounds[2]),\n        bounds[1].min(bounds[3]),\n        bounds[0].max(bounds[2]),\n        bounds[1].max(bounds[3]),\n",
        "        bounds[0],\n        bounds[1],\n        bounds[2],\n        bounds[3],\n",
        "textedit_fill_and_sign_records_and_reversed_boxes_are_read_as_what_they_are",
    ),
    Mutation(
        "fill and sign: a form's box with no area is accepted",
        "src/textedit/forms.rs",
        "    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {",
        "    if false {",
        "textedit_fill_and_sign_records_and_reversed_boxes_are_read_as_what_they_are",
    ),
    # A code of a symbolic TrueType subset whose character the path does not
    # write is kept read-only, where it used to refuse the font. `--only 'kept
    # code:'` runs the set.
    Mutation(
        "kept code: a character outside the repertoire refuses the map",
        "src/textedit/fonts/mapping.rs",
        "written(ch) || (keeps && char::from_u32(ch).is_some_and(|ch| !ch.is_control()))",
        "written(ch) || (false && keeps && char::from_u32(ch).is_some())",
        "textedit_mapping_keeps_a_character_outside_the_repertoire",
    ),
    Mutation(
        "kept code: a control character is kept",
        "src/textedit/fonts/mapping.rs",
        "written(ch) || (keeps && char::from_u32(ch).is_some_and(|ch| !ch.is_control()))",
        "written(ch) || (keeps && char::from_u32(ch).is_some())",
        "textedit_mapping_keeps_a_character_outside_the_repertoire",
    ),
    Mutation(
        "kept code: a code named twice is kept",
        "src/textedit/fonts/mapping.rs",
        "let Some(kept) = kept.as_mut().filter(|_| !twice) else {",
        "let Some(kept) = kept.as_mut().filter(|_| !twice || true) else {",
        "textedit_mapping_keeps_a_character_outside_the_repertoire",
    ),
    Mutation(
        "kept code: a kept code is offered under a second name",
        "src/textedit/fonts/mapping.rs",
        "\n                    || kept.as_ref().is_some_and(|kept| kept[code as usize]);",
        ";",
        "textedit_mapping_keeps_a_character_outside_the_repertoire",
    ),
    Mutation(
        "kept code: the other single-byte paths keep codes too",
        "src/textedit/fonts/mapping.rs",
        "    parse_single(stream, true, true, false, None)",
        "    parse_single(stream, true, true, false, Some(&mut [false; 256]))",
        "textedit_mapping_keeps_a_character_outside_the_repertoire",
    ),
    Mutation(
        # The strict map is what the font path reads: one bullet refuses the
        # font again, and every word set in it is read-only.
        "kept code: the font path reads the strict map",
        "src/textedit/fonts.rs",
        "            kept = unwritten;\n",
        "            if unwritten.contains(&true) {\n                return Err(invalid());\n            }\n",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
    Mutation(
        "kept code: the export's body font is read-only again",
        "src/textedit/fonts.rs",
        "            kept = unwritten;\n",
        "            if unwritten.contains(&true) {\n                return Err(invalid());\n            }\n",
        "textedit_a_libreoffice_list_in_its_own_paragraph_styles_is_offered",
    ),
    Mutation(
        "kept code: its width is not held to its glyph",
        "src/textedit/fonts.rs",
        "            if width <= 0. || width > 2000. || (width - advance).abs() > 1. {\n                return Err(\"embedded font widths disagree with its glyph metrics\".into());\n            }\n            let (overhang",
        "            let (overhang",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
    Mutation(
        # Skipped, the code has neither a slot nor a glyph, and a run showing
        # it refuses the page as an unmapped code where the font alone should
        # have gone read-only.
        "kept code: a code past the font's widths is skipped",
        "src/textedit/fonts.rs",
        "            if i64::from(code_byte) < first || i64::from(code_byte) > last {\n                return Err(invalid());\n            }\n",
        "            if i64::from(code_byte) < first || i64::from(code_byte) > last {\n                continue;\n            }\n",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
    Mutation(
        "kept code: its ink may reach anywhere",
        "src/textedit/fonts.rs",
        "    (left >= -250. && right <= width + 250. && bottom >= -500. && top <= 1000.)\n",
        "    (bottom.is_finite())\n",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
    Mutation(
        "kept code: its ink leaves the font's extent as it was",
        "src/textedit/fonts.rs",
        "            vertical_bounds = [vertical_bounds[0].min(low), vertical_bounds[1].max(high)];\n",
        "            let _ = (low, high);\n",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
    Mutation(
        "kept code: a glyph that draws nothing is not proved blank",
        "src/textedit/fonts.rs",
        "        return empty_glyph(face, glyph)?.then_some(([0.; 2], [0.; 2]));",
        "        return None;",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
    Mutation(
        # The run that shows a kept glyph is measured from nothing and offered
        # as text, the glyph read as whatever slot zero says.
        "kept code: nothing is measured for it",
        "src/textedit/fonts.rs",
        "        opaque: kept.contains(&true).then_some(opaque),",
        "        opaque: None,",
        "textedit_symbolic_codes_keep_a_character_they_cannot_write_read_only",
    ),
]

# Read-only text a wrap carries, and the clip that is the page (2026-10-06).
# `beneath: text in an ActualText span may move` left the table with this
# change: that text does move now, inside its span, and the first entry below
# is its reverse. Each was run with `--only 'carried:' --only 'page clip:'
# --only 'item:'` and was red in the test named.
ROWS += [
    Mutation(
        # The refusal as it was: a show an ActualText span describes stays.
        "carried: a show in an ActualText span stays where it is",
        "src/textedit/layout/wrap.rs",
        "    context_of(page, show).is_some() && !page.compound_run_clips.contains_key(&show)\n",
        "    context_of(page, show).is_some()\n"
        "        && !page.actual_text.contains_key(&show)\n"
        "        && !page.compound_run_clips.contains_key(&show)\n",
        "a_show_in_an_actualtext_span_moves_with_its_block_inside_its_span",
    ),
    Mutation(
        "carried: no position is recorded for read-only text",
        "src/textedit/inspect.rs",
        "            if carried {\n                self.held.insert(index as u32, context);",
        "            if false {\n                self.held.insert(index as u32, context);",
        "a_read_only_block_below_moves_down_with_the_wrap",
    ),
    Mutation(
        # Turned, patterned, layered and placed text would be drawn lower as
        # though only its position were its own.
        "carried: read-only text moves whatever keeps it where it is",
        "src/textedit/inspect.rs",
        "            carried: !fixed,\n",
        "            carried: true,\n",
        "a_block_below_that_cannot_be_moved_refuses_the_wrap",
    ),
    Mutation(
        "carried: only shows the writer may rewrite have a context to move by",
        "src/textedit/layout.rs",
        "    page.contexts.get(&show).or_else(|| page.held.get(&show))\n",
        "    page.contexts.get(&show)\n",
        "a_read_only_block_below_moves_down_with_the_wrap",
    ),
    Mutation(
        # The moved block is still in the hit list where it was, and the
        # paragraph's own line is refused for landing on it.
        "carried: read-only text that moves is still in the way",
        "src/textedit/layout.rs",
        "                                    (rect.map(f64::from), moves.contains(&id))\n",
        "                                    (rect.map(f64::from), false && moves.contains(&id))\n",
        "a_read_only_block_below_moves_down_with_the_wrap",
    ),
    Mutation(
        # The block has shows and no rectangle, so nothing lands on it and
        # nothing moves it.
        "carried: a read-only block below has no rectangle to be landed on",
        "src/textedit/layout.rs",
        "                .chain(&page.preserved)\n"
        "                .filter(|other| shows.contains(&other.operator) && !other.text.trim().is_empty())",
        "                .filter(|other| shows.contains(&other.operator) && !other.text.trim().is_empty())",
        "a_read_only_block_below_moves_down_with_the_wrap",
    ),
    Mutation(
        # Lowered and never checked: it is drawn off the page.
        "carried: a read-only line that moves is checked against nothing",
        "src/textedit/layout.rs",
        "                            .chain(&page.preserved)\n                            .find(|other| {",
        "                            .find(|other| {",
        "a_read_only_line_of_the_paragraph_moves_and_is_held_to_the_page",
    ),
    Mutation(
        "carried: text in the middle of a read-only line is left behind",
        "src/textedit/layout.rs",
        "        .chain(&page.preserved)\n"
        "        .filter(|other| below.contains(&other.operator) && !other.text.trim().is_empty())",
        "        .filter(|other| below.contains(&other.operator) && !other.text.trim().is_empty())",
        "text_that_stays_in_the_middle_of_a_read_only_line_refuses_the_wrap",
    ),
    Mutation(
        # Read-only text after the edit on its line would flow with it: a
        # hyphen its producer set at the line's end, moved into the line.
        "carried: read-only text after the edit flows onto its lines",
        "src/textedit/layout/wrap.rs",
        "                if !page.contexts.contains_key(&show)\n"
        "                    || page.actual_text.contains_key(&show)\n"
        "                    || page.compound_run_clips.contains_key(&show)\n"
        "                {\n"
        "                    return Err(Refused::NotApplicable);",
        "                if !carried(page, show) {\n                    return Err(Refused::NotApplicable);",
        "text_after_the_edit_that_cannot_be_moved_keeps_the_refusal",
    ),
    Mutation(
        "page clip: a clip a hair inside the page is a clip",
        "src/textedit/layout.rs",
        "const PAGE_CLIP: f64 = 0.5;",
        "const PAGE_CLIP: f64 = 0.0;",
        "a_line_that_a_clip_the_size_of_the_page_ends_wraps_at_the_page_edge",
    ),
    Mutation(
        "page clip: a clip well inside the page is the page",
        "src/textedit/layout.rs",
        "const PAGE_CLIP: f64 = 0.5;",
        "const PAGE_CLIP: f64 = 50.0;",
        "a_clip_within_half_a_point_of_the_page_edge_is_the_page",
    ),
    Mutation(
        "page clip: just past the allowance is still the page",
        "src/textedit/layout.rs",
        "const PAGE_CLIP: f64 = 0.5;",
        "const PAGE_CLIP: f64 = 0.7;",
        "a_clip_within_half_a_point_of_the_page_edge_is_the_page",
    ),
    Mutation(
        "page clip: every clip is the page",
        "src/textedit/layout.rs",
        "                    if kind != Room::Clip || (sheet - edge).abs() > PAGE_CLIP {",
        "                    if kind != Room::Clip {",
        "a_grown_box_stops_at_a_clip_the_document_has_in_force",
    ),
    Mutation(
        "page clip: the bounds a table states near the page edge are the page",
        "src/textedit/layout.rs",
        "                    if kind != Room::Clip || (sheet - edge).abs() > PAGE_CLIP {",
        "                    if (sheet - edge).abs() > PAGE_CLIP {",
        "a_clip_within_half_a_point_of_the_page_edge_is_the_page",
    ),
    Mutation(
        # Measured against the far edge of the page whichever way the line runs.
        "page clip: the allowance is measured from the wrong edge",
        "src/textedit/layout.rs",
        "                    if kind != Room::Clip || (sheet - edge).abs() > PAGE_CLIP {",
        "                    if kind != Room::Clip || (page[self.axis + 2] - edge).abs() > PAGE_CLIP {",
        "a_clip_within_half_a_point_of_the_page_edge_is_the_page",
    ),
    Mutation(
        "item: a one-line item wraps under its label",
        "src/textedit/layout/wrap.rs",
        "        .filter(|x| several || *x > label)\n",
        "",
        "textedit_a_libreoffice_list_item_wraps_and_carries_what_is_below",
    ),
    Mutation(
        "item: no show is a label",
        "src/textedit/inspect.rs",
        "        if marked.tags.label() {\n            self.labels.insert(index as u32);",
        "        if false {\n            self.labels.insert(index as u32);",
        "textedit_a_libreoffice_list_item_wraps_and_carries_what_is_below",
    ),
    Mutation(
        # Every show of the item would be its label, and no line would be left
        # to start a new one under.
        "item: every show of a list is a label",
        "src/textedit/tagging.rs",
        '            .is_some_and(|mcid| self.names[mcid] == b"Lbl")',
        "            .is_some()",
        "textedit_a_libreoffice_list_item_wraps_and_carries_what_is_below",
    ),
    Mutation(
        # A list set without a hanging indent starts its lower lines at its
        # label's left edge, and an item of several lines would then have
        # nowhere to start a new one.
        "item: an item of several lines is asked about its label too",
        "src/textedit/layout/wrap.rs",
        "        .filter(|x| several || *x > label)\n",
        "        .filter(|x| *x > label)\n",
        "textedit_a_list_item_of_several_lines_wraps_where_its_own_lines_start",
    ),
    Mutation(
        "item: the label itself is where a one-line item starts its lines",
        "src/textedit/layout/wrap.rs",
        "        .filter(|x| several || *x > label)\n",
        "        .filter(|x| several || *x >= label)\n",
        "textedit_a_one_line_list_item_wraps_under_its_words_not_its_label",
    ),
]

ROWS += [
    Mutation(
        # A wrap accepted while read-only text could not move is refused,
        # because the block now offered moves and has nowhere to go.
        "carried: a wrap is not tried again without the read-only blocks",
        "src/textedit/layout.rs",
        "                    Err(first) if plain != plan.beneath => attempt",
        "                    Err(first) if false => attempt",
        "a_wrap_a_read_only_block_cannot_follow_is_made_without_it",
    ),
    Mutation(
        # The second attempt would offer the read-only blocks again.
        "carried: the second attempt keeps the read-only blocks",
        "src/textedit/layout.rs",
        "                            page.contexts.contains_key(show) && !page.actual_text.contains_key(show)\n",
        "                            wrap::carried(page, *show)\n",
        "a_wrap_a_read_only_block_cannot_follow_is_made_without_it",
    ),
]

# A block of one line wraps at the measure the page's paragraphs show.
ROWS += [
    Mutation(
        # The line runs on to the paper's edge before it breaks.
        "measure: a one-line block wraps at the room its line had",
        "src/textedit/layout/wrap.rs",
        "        borrowed(page, run, &known, &leader).map_or(first, |measure| measure.min(first))",
        "        first",
        "textedit_a_libreoffice_list_item_wraps_and_carries_what_is_below",
    ),
    Mutation(
        # A line grown into the margin becomes every other line's measure.
        "measure: a block of one line is a measure too",
        "src/textedit/layout/wrap.rs",
        "        .filter(|(_, several, _)| *several)\n",
        "",
        "textedit_a_one_line_item_wraps_at_the_measure_of_the_paragraphs_beside_it",
    ),
    Mutation(
        # A line already past the measure is broken short of what it holds.
        "measure: a line past the measure is held to it",
        "src/textedit/layout/wrap.rs",
        "        .filter(|far| *far + tolerance >= run.advance)\n",
        "",
        "textedit_a_one_line_item_wraps_at_the_measure_of_the_paragraphs_beside_it",
    ),
    Mutation(
        # A wrapped one-line item sets its lines at the editor's 1.25 em.
        "measure: a one-line block wraps at the editor's own pitch",
        "src/textedit/layout/wrap.rs",
        "        borrowed_pitch(page, run, &known, &leader).unwrap_or(run.size * 1.25)",
        "        run.size * 1.25",
        "textedit_a_libreoffice_list_item_wraps_and_carries_what_is_below",
    ),
]

# A line that may break is broken where its ink fits.
ROWS += [
    Mutation(
        # The line is broken by its advance, and its ink is refused afterwards.
        "ink break: a wrapped line is broken by its advance alone",
        "src/textedit/layout.rs",
        "                            if shape.wrap {\n                                width.max(ink[1] + (-ink[0]).max(0.))",
        "                            if false {\n                                width.max(ink[1] + (-ink[0]).max(0.))",
        "textedit_a_wrapped_line_breaks_before_a_glyph_whose_ink_leaves_the_box",
    ),
]

# A line that grows past its paragraph's measure wraps there, or stays.
ROWS += [
    Mutation(
        # The words run on through the right margin.
        "margin: a line grown past its measure is never wrapped",
        "src/textedit/layout.rs",
        "    if (full || grown.is_some()) && placement.inherited == 0.",
        "    if full && placement.inherited == 0.",
        "textedit_a_line_grown_past_its_measure_wraps_there_or_stays",
    ),
    Mutation(
        # The run's own items are not kept for words that fit.
        "margin: a line within its measure is laid out afresh",
        "src/textedit/layout.rs",
        "            Ok(plan) if grown.is_some_and(|used| used <= plan.first + 0.001) => {}\n",
        "",
        "textedit_a_line_grown_past_its_measure_wraps_there_or_stays",
    ),
    Mutation(
        # An edit accepted before is refused.
        "margin: a wrap that fails refuses the words",
        "src/textedit/layout.rs",
        "                    Err(error) if full => return Err(error),",
        "                    Err(error) if true => return Err(error),",
        "textedit_a_line_grown_past_its_measure_wraps_there_or_stays",
    ),
    Mutation(
        # An edit accepted before is refused.
        "margin: a wrap that cannot be planned refuses the words",
        "src/textedit/layout.rs",
        "            Err(wrap::Refused::Blocked(reason)) if full => return Err(reason.into()),",
        "            Err(wrap::Refused::Blocked(reason)) if true => return Err(reason.into()),",
        "text_that_stays_in_the_middle_of_a_read_only_line_refuses_the_wrap",
    ),
    Mutation(
        # The text after the run leaves its line.
        "margin: a line a push moves along is wrapped",
        "src/textedit/layout.rs",
        "                && free.line.is_empty()\n",
        "",
        "a_line_pushed_by_one_edit_and_moved_down_by_a_wrap_is_refused",
    ),
    Mutation(
        # The share of the gutter a line may take is lost.
        "margin: a line beside a column wraps short of the gutter",
        "src/textedit/layout.rs",
        "                && stop == Room::Page\n",
        "",
        "a_line_stops_before_the_gutter_and_wraps_at_its_columns_measure",
    ),
    Mutation(
        # An untagged page's line breaks at a measure nothing states.
        "margin: blocks read off the lines are wrapped at their widest",
        "src/textedit/layout.rs",
        "                && !blocks::geometric_page(page) =>",
        "                =>",
        "an_untagged_paragraph_wraps_by_the_blocks_its_lines_give",
    ),
    Mutation(
        # The reader's box is taken from them.
        "margin: a box the reader wraps within is wrapped at the measure",
        "src/textedit/layout.rs",
        "            if !settings.wrap\n                && *used > width",
        "            if *used > width",
        "textedit_a_line_grown_past_its_measure_wraps_there_or_stays",
    ),
]

# --- which of the command-line tool's two commands has something to do -------
# `clitool::state_of`, read by the window to grey a command. The two state
# tests are `#[cfg(unix)]`; the Windows half reads the registry and is held by
# `userpath::tests` on Windows, where no mutation is run against the real PATH.
ROWS += [
    Mutation(
        # An installation from before the second name greys Install, which is
        # how it would have gained the name.
        "cli tool state: one name of the two counts as installed",
        "src/clitool.rs",
        "            .all(|path| plan(true, path, tool) == Step::Present),",
        "            .any(|path| plan(true, path, tool) == Step::Present),",
        "the_state_is_read_from_both_links_and_reading_changes_neither",
        only_on="macos",
    ),
    Mutation(
        # A link to an older copy greys Install, which is what repoints it.
        "cli tool state: another copy of tpdf counts as installed",
        "src/clitool.rs",
        "            .all(|path| plan(true, path, tool) == Step::Present),",
        "            .all(|path| plan(false, path, tool) == Step::Remove),",
        "the_state_is_read_from_both_links_and_reading_changes_neither",
        only_on="macos",
    ),
    Mutation(
        # One link left of two greys Uninstall, which is what removes it.
        "cli tool state: one name of the two counts as nothing to remove",
        "src/clitool.rs",
        "            .any(|path| plan(false, path, tool) != Step::Absent),",
        "            .all(|path| plan(false, path, tool) != Step::Absent),",
        "the_state_is_read_from_both_links_and_reading_changes_neither",
        only_on="macos",
    ),
    Mutation(
        # Uninstall is greyed over a file it would have explained.
        "cli tool state: somebody else's file counts as nothing there",
        "src/clitool.rs",
        "            .any(|path| plan(false, path, tool) != Step::Absent),",
        "            .any(|path| plan(false, path, tool) == Step::Remove),",
        "the_state_is_read_from_both_links_and_reading_changes_neither",
        only_on="macos",
    ),
    Mutation(
        # A failed read greys Uninstall instead of leaving it to say why.
        "cli tool state: a path that cannot be read counts as empty",
        "src/clitool.rs",
        "        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {",
        "        Err(_) => {",
        "a_path_that_cannot_be_read_greys_neither_command",
        only_on="macos",
    ),
    Mutation(
        # The window reads `occupied`; under another name it reads nothing and
        # greys Uninstall for ever.
        "cli tool state: the second answer crosses under another name",
        "src/clitool.rs",
        "    pub occupied: bool,\n}",
        '    #[serde(rename = "removable")]\n    pub occupied: bool,\n}',
        "the_state_crosses_to_the_window_under_the_names_it_reads",
    ),
]

# --- bookmarks kept through a page deletion ----------------------------------
# `outline_repair::keep_for`, and the order `pagetree::materialise` calls it in.
# `--only 'bookmarks:'` runs the set.
#
# Three edits to `chain` are left out because nothing can see them: taking out
# the lines that remove `/First` and `/Last` from an emptied parent, `/Prev`
# from a new first entry, or `/Next` from a new last one. The keys those lines
# remove name removed entries, and `pagetree::forget` takes every key that
# does. Measured on 2026-10-07: all three survive, as that predicts.
ROWS += [
    Mutation(
        'bookmarks: read the outline after the pages are dropped',
        'src/pagetree.rs',
        '        crate::outline_repair::keep_for(doc, &doomed)?;\n        drop_pages(doc, dropped)?;',
        '        drop_pages(doc, dropped)?;\n        crate::outline_repair::keep_for(doc, &doomed)?;',
        'materialising_a_deletion_keeps_the_bookmarks_of_the_pages_that_stay',
    ),
    Mutation(
        'bookmarks: do not repair the outline at all',
        'src/pagetree.rs',
        '        crate::outline_repair::keep_for(doc, &doomed)?;\n',
        '',
        'materialising_a_deletion_keeps_the_bookmarks_of_the_pages_that_stay',
    ),
    Mutation(
        'bookmarks: count a page a kept number also names as deleted',
        'src/pagetree.rs',
        '        .filter(|id| !kept.contains(id))\n        .collect()\n}',
        '        .collect()\n}',
        'a_page_a_kept_number_also_names_is_not_dropped',
    ),
    Mutation(
        'bookmarks: keep an entry that leads to a deleted page',
        'src/outline_repair.rs',
        '            Aim::Page(page) => doomed.contains(&page),',
        '            Aim::Page(_) => false,',
        'an_outline_with_nothing_left_is_dropped',
    ),
    Mutation(
        'bookmarks: remove an entry whatever page it leads to',
        'src/outline_repair.rs',
        '            Aim::Page(page) => doomed.contains(&page),',
        '            Aim::Page(_) => true,',
        'deleting_a_page_no_entry_leads_to_changes_nothing',
    ),
    Mutation(
        'bookmarks: keep a heading with nothing left under it',
        'src/outline_repair.rs',
        '            Aim::Nothing => !was.is_empty() && left.is_empty(),',
        '            Aim::Nothing => false,',
        'deleting_a_page_removes_only_the_entries_that_led_to_it',
    ),
    Mutation(
        'bookmarks: remove a heading that never had anything under it',
        'src/outline_repair.rs',
        '            Aim::Nothing => !was.is_empty() && left.is_empty(),',
        '            Aim::Nothing => left.is_empty(),',
        'deleting_a_page_removes_only_the_entries_that_led_to_it',
    ),
    Mutation(
        'bookmarks: keep the outline when an entry cannot be vouched for',
        'src/outline_repair.rs',
        '            Aim::Unsafe => return whole(doc),',
        '            Aim::Unsafe => false,',
        'an_entry_naming_its_page_by_number_drops_the_outline_whole',
    ),
    Mutation(
        'bookmarks: trust a page named by its number',
        'src/outline_repair.rs',
        '        Some(Object::Integer(_)) => Aim::Unsafe,',
        '        Some(Object::Integer(_)) => Aim::Other,',
        'an_entry_naming_its_page_by_number_drops_the_outline_whole',
    ),
    Mutation(
        'bookmarks: trust a name the walk gave up on',
        'src/outline_repair.rs',
        '        Err(links::GaveUp) => Aim::Unsafe,',
        '        Err(links::GaveUp) => Aim::Other,',
        'a_name_the_walk_gives_up_on_drops_the_outline_whole',
    ),
    Mutation(
        'bookmarks: drop what was under a removed entry',
        'src/outline_repair.rs',
        '            left.extend(under.get(id).into_iter().flatten());\n',
        '',
        'what_stays_moves_up_past_every_entry_that_goes',
    ),
    Mutation(
        'bookmarks: leave a moved entry naming its old parent',
        'src/outline_repair.rs',
        '        item.set("Parent", parent);\n        match at.checked_sub',
        '        match at.checked_sub',
        'what_stays_moves_up_past_every_entry_that_goes',
    ),
    Mutation(
        'bookmarks: do not write the chain forwards',
        'src/outline_repair.rs',
        '            Some(next) => item.set("Next", *next),',
        '            Some(_) => {}',
        'what_stays_moves_up_past_every_entry_that_goes',
    ),
    Mutation(
        'bookmarks: do not write the chain backwards',
        'src/outline_repair.rs',
        '            Some(prev) => item.set("Prev", *prev),',
        '            Some(_) => {}',
        'what_stays_moves_up_past_every_entry_that_goes',
    ),
    Mutation(
        "bookmarks: do not write the parent's first and last",
        'src/outline_repair.rs',
        '                node.set("First", *first);\n                node.set("Last", *last);\n',
        '',
        'what_stays_moves_up_past_every_entry_that_goes',
    ),
    Mutation(
        'bookmarks: leave the removed entries in the file',
        'src/outline_repair.rs',
        '    pagetree::forget(doc, &removed)?;\n',
        '',
        'what_stays_moves_up_past_every_entry_that_goes',
    ),
    Mutation(
        'bookmarks: do not recount what is left',
        'src/outline_repair.rs',
        '        redact::recount(doc, root, 0);\n',
        '        let _ = root;\n',
        'deleting_a_page_removes_only_the_entries_that_led_to_it',
    ),
    Mutation(
        'bookmarks: leave an emptied outline as an empty root',
        'src/outline_repair.rs',
        '    if top.is_empty() {\n',
        '    if false {\n',
        'an_outline_with_nothing_left_is_dropped',
    ),
    Mutation(
        'bookmarks: report an outline dropped and leave it in',
        'src/outline_repair.rs',
        'fn whole(doc: &mut Document) -> Result<Kept, String> {\n    pagetree::drop_outline(doc)?;\n',
        'fn whole(doc: &mut Document) -> Result<Kept, String> {\n    let _ = &doc;\n',
        'an_outline_that_is_not_a_tree_is_dropped_whole',
    ),
    Mutation(
        'bookmarks: do not read a destination inside an action',
        'src/outline_repair.rs',
        '            Ok(b"GoTo") => action',
        '            Ok(b"GoToX") => action',
        'deleting_a_page_removes_only_the_entries_that_led_to_it',
    ),
    Mutation(
        'bookmarks: do not look a name up',
        'src/outline_repair.rs',
        '        Object::Name(name) => named(doc, name),',
        '        Object::Name(_) => Aim::Other,',
        'deleting_a_page_removes_only_the_entries_that_led_to_it',
    ),
    Mutation(
        'bookmarks: do not look a string up',
        'src/outline_repair.rs',
        '        Object::String(bytes, _) => named(doc, bytes),',
        '        Object::String(..) => Aim::Other,',
        'a_name_in_the_name_tree_is_followed_to_its_page',
    ),
    Mutation(
        'bookmarks: do not read a named value written as a dictionary',
        'src/outline_repair.rs',
        '            Ok(Object::Array(array)) => page_of(array),',
        '            Ok(Object::Array(_)) => Aim::Other,',
        'a_name_in_the_name_tree_is_followed_to_its_page',
    ),
]

# --- text that is in a document and not on its pages --------------------------
# `hidden::judge`, the walk over the pages and its last line (`hidden/survey.rs`),
# which the tool and the window share. `--only 'hidden:'` runs the set, the
# window's own included (`--only 'hidden window:'`, `--only 'hidden survey:'`).
# What only a worker can show --- the exit code, the turned page, a sheet of
# more than one tile, the window's path giving what the tool gives --- is held
# by `tests/cli/hidden.rs` against a document written there, and is not in this
# table.
ROWS += [
    Mutation(
        'hidden: count a difference of exactly the tolerance as ink',
        'src/hidden.rs',
        '        self.high.max(other.high) - self.low.min(other.low) <= TOLERANCE\n',
        '        self.high.max(other.high) - self.low.min(other.low) < TOLERANCE\n',
        'the_tolerance_is_where_one_colour_ends',
    ),
    Mutation(
        'hidden: take a box with ink in it for one colour',
        'src/hidden.rs',
        '        self.high.max(other.high) - self.low.min(other.low) <= TOLERANCE\n',
        '        self.high.abs_diff(other.high) <= TOLERANCE\n',
        'visible_words_are_compared_and_not_reported',
    ),
    Mutation(
        'hidden: never match the strip beside a box',
        'src/hidden.rs',
        '        self.high.max(other.high) - self.low.min(other.low) <= TOLERANCE\n',
        '        self.high.max(other.high) < self.low.min(other.low)\n',
        'words_under_a_tight_black_rectangle_are_reported',
    ),
    Mutation(
        'hidden: call every uniform box hidden without looking beside it',
        'src/hidden.rs',
        '        if sides.into_iter().flatten().any(|side| inside.matches(side)) {',
        '        if sides.into_iter().flatten().count() > 0 {',
        'a_glyph_that_fills_its_box_is_visible',
    ),
    Mutation(
        'hidden: judge glyphs that fill their box and touch their neighbours',
        'src/hidden.rs',
        '        } else if !sound || fills_its_box(ch) {',
        '        } else if !sound {',
        'a_rule_made_of_touching_glyphs_is_not_judged',
    ),
    Mutation(
        'hidden: judge a soft hyphen as a character that paints',
        'src/hidden.rs',
        "            '\\u{ad}' | '\\u{200b}'..='\\u{200f}'",
        "            '\\u{200b}'..='\\u{200f}'",
        'characters_that_paint_nothing_are_neither_hidden_nor_visible',
    ),
    Mutation(
        'hidden: report one hidden character alone',
        'src/hidden.rs',
        'pub const MIN_RUN: usize = 2;',
        'pub const MIN_RUN: usize = 1;',
        'one_hidden_character_alone_is_not_reported',
    ),
    Mutation(
        'hidden: leave an unreported character counted as compared',
        'src/hidden.rs',
        '            out.judged -= run.hidden;\n',
        '',
        'one_hidden_character_alone_is_not_reported',
    ),
    Mutation(
        'hidden: report words outside the page as covered',
        'src/hidden.rs',
        '            return Verdict::OffPage;',
        '            return Verdict::Hidden;',
        'words_outside_the_page_are_reported_as_off_the_page',
    ),
    Mutation(
        'hidden: join words outside the page to words inside it',
        'src/hidden.rs',
        '                    || (run.hidden > 0 && run.off_page != off_page)\n',
        '',
        'words_outside_the_page_are_reported_as_off_the_page',
    ),
    Mutation(
        'hidden: run a finding on into the next line',
        'src/hidden.rs',
        '            .is_some_and(|last| rect[far] <= last[near] || rect[near] >= last[far])',
        '            .is_some_and(|_| false)',
        'a_finding_does_not_run_on_into_the_next_line',
    ),
    Mutation(
        "hidden: ignore the page's own turn",
        'src/hidden.rs',
        '        let turned = (own % 2 == 1) != (text.quarter_turns % 2 == 1);',
        '        let turned = own % 2 == 1;',
        'the_page_turn_and_the_character_turn_are_combined',
    ),
    Mutation(
        "hidden: read a turned character's left and right",
        'src/hidden.rs',
        '        let sides = if turned {',
        '        let sides = if !turned {',
        'a_turned_character_is_read_along_its_own_line',
    ),
    Mutation(
        'hidden: look for the characters at the scale of the page',
        'src/hidden.rs',
        '        let [left, top, right, bottom] = rect.map(|v| v * scale);',
        '        let [left, top, right, bottom] = rect;',
        'boxes_are_scaled_to_the_image',
    ),
    Mutation(
        'hidden: judge a box of two pixels',
        'src/hidden.rs',
        '        if x1 < x0 + MIN_SIDE || y1 < y0 + MIN_SIDE {',
        '        if x1 <= x0 || y1 <= y0 {',
        'what_cannot_be_judged_is_counted',
    ),
    Mutation(
        'hidden: let a finding run through a visible character',
        'src/hidden.rs',
        '            Verdict::Visible => {\n                out.judged += 1;\n                run.close(&mut out);',
        '            Verdict::Visible => {\n                out.judged += 1;',
        'a_visible_character_divides_two_findings',
    ),
    Mutation(
        "hidden: keep the spaces after a finding's last character",
        'src/hidden.rs',
        '                text: run.text[..run.solid].to_string(),',
        '                text: run.text.clone(),',
        'a_visible_character_divides_two_findings',
    ),
    Mutation(
        'hidden: judge with a buffer of the wrong size',
        'src/hidden.rs',
        '    let sound = pixels.len() == width.saturating_mul(height).saturating_mul(4) && scale > 0.0;',
        '    let sound = pixels.len() >= 4 && scale > 0.0;',
        'a_buffer_of_the_wrong_size_judges_nothing',
    ),
    Mutation(
        'hidden: leave the characters that could not be judged out of the last line',
        'src/hidden/survey.rs',
        '    if survey.unjudged > 0 {',
        '    if false {',
        'the_last_line_says_what_was_found_and_what_was_not_checked',
    ),
    Mutation(
        'hidden: leave the pages without text out of the last line',
        'src/hidden/survey.rs',
        '    if !survey.without_text.is_empty() {\n        line.push_str',
        '    if false {\n        line.push_str',
        'the_last_line_says_what_was_found_and_what_was_not_checked',
    ),
    Mutation(
        'hidden: leave the pages too large to compare out of the last line',
        'src/hidden/survey.rs',
        '    if !survey.not_compared.is_empty() {\n        line.push_str',
        '    if false {\n        line.push_str',
        'the_last_line_says_what_was_found_and_what_was_not_checked',
    ),
    Mutation(
        'hidden: compare a page at any scale, however small',
        'src/hidden/survey.rs',
        '    if scale < MIN_SCALE {',
        '    if scale <= 0.0 {',
        'a_page_is_compared_at_144_dpi_or_as_large_as_the_bounds_allow',
    ),
    Mutation(
        'hidden survey: compare a page that has no text',
        'src/hidden/survey.rs',
        '        if !ocr_layer::has_text(&text) {',
        '        if false {',
        'a_page_without_text_or_too_large_is_named_and_not_rendered',
    ),
    Mutation(
        'hidden survey: leave a page too large to compare out of the account',
        'src/hidden/survey.rs',
        '            out.not_compared.push(*n);\n',
        '',
        'a_page_without_text_or_too_large_is_named_and_not_rendered',
    ),
    Mutation(
        'hidden survey: ask for the page numbered as the selection counts',
        'src/hidden/survey.rs',
        '        let page = n.saturating_sub(1);',
        '        let page = *n;',
        'each_selected_page_is_compared_and_a_finding_names_its_page',
    ),
    Mutation(
        'hidden survey: put every finding on the first page',
        'src/hidden/survey.rs',
        '                page: *n,\n',
        '                page: 1,\n',
        'each_selected_page_is_compared_and_a_finding_names_its_page',
    ),
    Mutation(
        'hidden survey: count only the last page compared',
        'src/hidden/survey.rs',
        '        out.compared += judged.judged as u64;',
        '        out.compared = judged.judged as u64;',
        'each_selected_page_is_compared_and_a_finding_names_its_page',
    ),
    Mutation(
        'hidden survey: drop the characters that could not be judged',
        'src/hidden/survey.rs',
        '        out.unjudged += judged.unjudged as u64;\n',
        '',
        'each_selected_page_is_compared_and_a_finding_names_its_page',
    ),
    Mutation(
        'hidden survey: report no characters as hidden in a finding',
        'src/hidden/survey.rs',
        '                characters: found.hidden as u32,',
        '                characters: 0,',
        'each_selected_page_is_compared_and_a_finding_names_its_page',
    ),
    Mutation(
        'hidden survey: forget how many pages were selected',
        'src/hidden/survey.rs',
        '        selected: selection.len(),',
        '        selected: 0,',
        'each_selected_page_is_compared_and_a_finding_names_its_page',
    ),
    Mutation(
        'hidden survey: lose the mark on words outside the page',
        'src/hidden/survey.rs',
        '                off_page: found.off_page,',
        '                off_page: false,',
        'words_outside_the_page_keep_their_mark',
    ),
    Mutation(
        'hidden survey: walk on after being told to stop',
        'src/hidden/survey.rs',
        '        before(done, *n)?;',
        '        let _ = before(done, *n);',
        'the_walk_says_where_it_is_and_stops_when_told',
    ),
    Mutation(
        'hidden survey: send a passage to the window in another spelling',
        'src/hidden/survey.rs',
        '#[serde(rename_all = "camelCase")]\npub struct Passage {',
        'pub struct Passage {',
        'the_reply_reaches_the_window_in_its_own_spelling',
    ),
    Mutation(
        'hidden window: never ask whether to stop',
        'src/commands/hidden.rs',
        '        if cancelled() {\n            return Err(CANCELLED.into());',
        '        if false {\n            return Err(CANCELLED.into());',
        'a_stop_ends_the_walk_before_the_next_page_and_leaves_no_result',
    ),
    Mutation(
        'hidden window: report every page as the first',
        'src/commands/hidden.rs',
        '        progress(Progress { page, of: count });',
        '        progress(Progress { page: 1, of: count });',
        'every_page_is_walked_and_reported_before_it_is_read',
    ),
    Mutation(
        'hidden window: put every tile at the left edge',
        'src/commands/hidden.rs',
        '                let start = ((y as usize + row) * width as usize + x as usize) * 4;',
        '                let start = ((y as usize + row) * width as usize) * 4;',
        'a_page_is_put_together_from_its_tiles',
    ),
    Mutation(
        'hidden window: ask for a full tile at the right edge',
        'src/commands/hidden.rs',
        '            let tw = (width - x).min(side) as u16;',
        '            let tw = width.min(side) as u16;',
        'a_page_is_put_together_from_its_tiles',
    ),
    Mutation(
        'hidden window: take a tile that is larger than asked',
        'src/commands/hidden.rs',
        '            if part.len() != stride * usize::from(th) {',
        '            if part.len() < stride * usize::from(th) {',
        'a_tile_of_the_wrong_size_or_one_that_failed_ends_the_page',
    ),
    Mutation(
        'hidden window: call a document with unsaved changes saved',
        'src/commands/hidden.rs',
        '    !plan.is_identity()',
        '    plan.is_identity()',
        'a_document_with_changes_that_are_not_saved_is_told_so',
    ),
    Mutation(
        'hidden window: never say there were unsaved changes',
        'src/commands/hidden.rs',
        '            not_looked_at: NOT_LOOKED_AT.into(),\n            unsaved,',
        '            not_looked_at: NOT_LOOKED_AT.into(),\n            unsaved: false,',
        'the_reply_reaches_the_window_in_its_own_spelling',
    ),
    Mutation(
        'hidden window: leave out what the check never looks at',
        'src/commands/hidden.rs',
        '            not_looked_at: NOT_LOOKED_AT.into(),\n            unsaved,',
        '            not_looked_at: String::new(),\n            unsaved,',
        'the_reply_reaches_the_window_in_its_own_spelling',
    ),
    Mutation(
        "hidden window: send the reply in Rust's spelling",
        'src/commands/hidden.rs',
        '#[serde(rename_all = "camelCase")]\npub struct HiddenText {',
        'pub struct HiddenText {',
        'the_reply_reaches_the_window_in_its_own_spelling',
    ),
    Mutation(
        'hidden: list a passage outside the page as one inside it',
        'src/cli/hidden.rs',
        '    if found.off_page {',
        '    if false {',
        'a_passage_outside_the_page_is_listed_as_such',
    ),
]
