"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import Mutation

ROWS: list[Mutation] = []

# --- signing with a certificate the reader has -------------------------------
#
# Phase 6 step 2, 2026-09-26. `sign_prepare.rs` is the worker's revision,
# `sign_cms.rs` the app process's checks, CMS and splice, `keystore.rs` the OS
# half. Each mutation removes one rule and names the test that is the only one
# built to see it.
ROWS += [
    Mutation(
        # Sign with whatever identity the caller names. The window's command
        # takes it from the webview, and before 26.9.21 its invisible path never
        # asked whether the certificate may sign a document at all.
        "sign: use a certificate the listing would not offer",
        "src/sign_cms.rs",
        "    usable(certificate, at)",
        "    Ok::<u64, String>(at)",
        "signing_refuses_a_certificate_the_listing_would_not_offer",
    ),
    Mutation(
        # Sign a document that was opened with a password, or on an empty one:
        # lopdf would encrypt the signature's own value with the document.
        "sign: sign a document that was encrypted and opened",
        "src/sign_prepare.rs",
        "    if prev.is_encrypted() || prev.was_encrypted() {",
        "    if prev.is_encrypted() {",
        "an_encrypted_document_is_refused_whether_or_not_it_was_opened",
    ),
    Mutation(
        "sign: sign over a certification that permits no change",
        "src/sign_prepare.rs",
        "    if certified_without_changes(&prev) {",
        "    if false {",
        "a_certification_that_permits_no_change_is_refused_and_the_others_are_not",
    ),
    Mutation(
        # Take the first placeholder found: a string in a cloned page that spells
        # one gets the range written into it, and the real hole is left alone.
        "sign: take a placeholder that occurs twice",
        "src/sign_prepare.rs",
        "    found.next().is_none().then_some(first)",
        "    Some(first)",
        "a_placeholder_the_document_already_spells_is_a_refusal_not_a_guess",
    ),
    Mutation(
        # Truncate a range too wide for the placeholder instead of refusing.
        "sign: let the offsets overflow the placeholder",
        "src/sign_prepare.rs",
        "    if text.len() + 1 > width {",
        "    if false {",
        "a_file_whose_offsets_outgrow_the_placeholder_is_refused",
    ),
    Mutation(
        # Rewrite the page as well as its own /Annots array: a change an
        # earlier signature's difference analysis must then account for.
        "sign: rewrite the page whose annotation list is its own object",
        "src/sign_prepare.rs",
        """    let changed = match site {
        Site::Object(array) => array,
        Site::Inline | Site::Absent => page,
    };""",
        """    incremental
        .opt_clone_object_to_new_document(page)
        .map_err(|e| e.to_string())?;
    let changed = match site {
        Site::Object(array) => array,
        Site::Inline | Site::Absent => page,
    };""",
        "a_list_that_is_its_own_object_is_rewritten_and_its_owner_is_not",
    ),
    Mutation(
        "sign: set /SigFlags without keeping the bits already there",
        "src/sign_prepare.rs",
        '    dict.set("SigFlags", Object::Integer(flags | SIG_FLAGS));',
        '    dict.set("SigFlags", Object::Integer(SIG_FLAGS));',
        "existing_sig_flags_are_kept_and_the_two_bits_added",
    ),
    Mutation(
        # Replace an inline list instead of appending to it: the fields and
        # annotations already there are dropped from the new revision.
        "sign: replace an inline list rather than append to it",
        "src/sign_prepare.rs",
        """        Site::Inline => dict
            .get_mut(key)
            .and_then(Object::as_array_mut)
            .map(|array| array.push(reference))
            .map_err(|e| e.to_string()),""",
        """        Site::Inline => {
            dict.set(key.to_vec(), Object::Array(vec![reference]));
            Ok(())
        }""",
        "every_shape_of_the_two_lists_gains_the_widget",
    ),
    Mutation(
        "sign: name the new field Signature1 whatever exists",
        "src/sign_prepare.rs",
        "        .find(|name| !taken.contains(name))",
        "        .find(|_| true)",
        "a_list_that_is_its_own_object_is_rewritten_and_its_owner_is_not",
    ),
    Mutation(
        # Leave the `<` out of the hole: the range then covers a byte of the
        # value's own string, which no verifier accepts.
        "sign: start the hole after its delimiter",
        "src/sign_prepare.rs",
        '    let first = was + contents + b"/Contents".len();',
        '    let first = was + contents + b"/Contents<".len();',
        "the_range_frames_exactly_the_reserved_hole_and_the_digest_is_its_bytes",
    ),
    Mutation(
        "sign: hash the hole along with the covered bytes",
        "src/sign_prepare.rs",
        "    hasher.update(&update[second - was..]);",
        "    hasher.update(&update[first - was..]);",
        "the_range_frames_exactly_the_reserved_hole_and_the_digest_is_its_bytes",
    ),
    Mutation(
        # Believe the worker's digest. A worker that described one document and
        # built another then gets the reader to sign what it built.
        "sign: believe the worker's digest",
        "src/sign_cms.rs",
        "    if unsigned.digest != digest {",
        "    if false {",
        "the_workers_digest_is_recomputed_rather_than_believed",
    ),
    Mutation(
        "sign: accept an update built against another length",
        "src/sign_cms.rs",
        "    if unsigned.built_against != was {",
        "    if false {",
        "an_update_built_against_other_bytes_is_refused",
    ),
    Mutation(
        "sign: accept a range whose hole is not the reservation",
        "src/sign_cms.rs",
        "        && second == first.saturating_add(RESERVED * 2 + 2)\n",
        "\n",
        "a_range_that_does_not_frame_its_hole_is_refused",
    ),
    Mutation(
        "sign: accept a hole that is not empty",
        "src/sign_cms.rs",
        "    if !zeros {",
        "    if false {",
        "a_hole_that_is_not_empty_is_refused",
    ),
    Mutation(
        # Hand back whatever was built. The key that is not the certificate's,
        # and the value over the wrong digest, are then written as signatures.
        "sign: skip the check of what was just signed",
        "src/sign_cms.rs",
        "        if verdict.verdict != crate::integrity::Verdict::Intact {",
        "        if false {",
        "a_signature_over_the_wrong_digest_is_broken_and_not_written",
    ),
    Mutation(
        "sign: splice the value one byte late",
        "src/sign_cms.rs",
        "? + 1 - built_against;",
        "? + 2 - built_against;",
        "the_value_is_spliced_as_uppercase_hex_at_the_start_of_the_hole",
    ),
    Mutation(
        "sign: write a CMS past the half kept for a timestamp",
        "src/sign_cms.rs",
        "    if blob.len() > STEP_TWO_LIMIT {",
        "    if false {",
        "a_blob_past_the_step_two_limit_is_refused",
    ),
    Mutation(
        # Leave out ESS signingCertificateV2: PAdES B-B's binding of the
        # signature to one certificate.
        "sign: leave the signing certificate unnamed",
        "src/sign_cms.rs",
        """    info.add_signed_attribute(signing_certificate(&signer, certificate)?)
        .map_err(|e| said!("the signing certificate could not be named: {e}"))?;""",
        "",
        "the_signed_attributes_are_exactly_the_three_pades_b_b_names",
    ),
    Mutation(
        "sign: offer an expired certificate",
        "src/sign_cms.rs",
        "    if now > until {",
        "    if false {",
        "a_certificate_is_offered_only_while_valid_for_signing",
    ),
    Mutation(
        # Refuse the qualified card that states nonRepudiation alone.
        "sign: accept only digitalSignature",
        "src/sign_cms.rs",
        "        if !usage.digital_signature() && !usage.non_repudiation() {",
        "        if !usage.digital_signature() {",
        "a_certificate_is_offered_only_while_valid_for_signing",
    ),
    Mutation(
        # Compare paths only: a hard link to the source reads as another file.
        "save: same_file ignores file identity",
        "src/save.rs",
        "        return a == b;\n    }\n    canonical_parent(a) == canonical_parent(b)",
        "        let _ = (a, b);\n    }\n    canonical_parent(a) == canonical_parent(b)",
        "a_hard_link_to_the_source_is_the_same_file_and_a_copy_is_not",
    ),
    Mutation(
        # The owner's Developer ID certificate: digitalSignature, code signing only.
        "sign: offer a certificate issued for code signing",
        "src/sign_cms.rs",
        "            .any(|purpose| DOCUMENT_PURPOSES.contains(&purpose.as_str()))",
        "            .any(|_| true)",
        "a_certificate_issued_for_code_signing_is_not_offered",
    ),
    Mutation(
        "sign: offer a certificate that is not for signing",
        "src/sign_cms.rs",
        "        if !usage.digital_signature() && !usage.non_repudiation() {",
        "        if false {",
        "a_certificate_is_offered_only_while_valid_for_signing",
    ),
    Mutation(
        "sign: sign with an RSA key under 2048 bits",
        "src/sign_cms.rs",
        "            if bits < MIN_RSA_BITS {",
        "            if false {",
        "only_the_keys_tpdf_signs_with_are_offered",
    ),
    Mutation(
        # Hand the worker a document it will die parsing, on Windows.
        "sign: prepare a document past the worker's bound",
        "src/sign_cms.rs",
        "    if len > crate::save::APPEND_MAX_BYTES {",
        "    if false {",
        "a_document_past_the_workers_bound_is_refused_and_one_at_it_is_not",
    ),
    Mutation(
        "sign: sign over unsaved edits",
        "src/sign_cms.rs",
        "    if dirty {",
        "    if false {",
        "unsaved_edits_refuse_before_anything_is_signed",
    ),
    Mutation(
        "sign: call every signature in the report the new one",
        "src/sign_cms.rs",
        "        .map(|(index, signature)| (ours == Some(index), signature))",
        "        .map(|(_, signature)| (true, signature))",
        "the_report_marks_the_new_signature_and_leaves_out_unsigned_fields",
    ),
    Mutation(
        # Keep an ECDSA half's leading zeros: a non-minimal DER INTEGER.
        "sign: keep leading zeros in a raw ECDSA half",
        "src/sign_cms.rs",
        "        let leading = half.iter().take_while(|b| **b == 0).count();",
        "        let leading = 0;",
        "a_raw_ecdsa_value_becomes_the_der_the_curve_crate_writes",
    ),
    Mutation(
        # Drop the sign byte: a half with its top bit set reads as negative.
        "sign: leave out the sign byte of a raw ECDSA half",
        "src/sign_cms.rs",
        "        if body[0] & 0x80 != 0 {",
        "        if false {",
        "a_raw_ecdsa_value_becomes_the_der_the_curve_crate_writes",
    ),
    Mutation(
        # The control `.cargo/audit.toml` cites: a private-key type named in a
        # shipped file.
        "sign: name a private-key type in a shipped file",
        "src/keystore.rs",
        "pub struct Identity {",
        "// SigningKey\npub struct Identity {",
        "no_private_key_type_is_named_outside_the_tests",
    ),
    Mutation(
        "keystore: name an identity by an uppercase hash",
        "src/keystore.rs",
        '    sha2::Sha256::digest(certificate)\n        .iter()\n        .map(|b| format!("{b:02x}"))',
        '    sha2::Sha256::digest(certificate)\n        .iter()\n        .map(|b| format!("{b:02X}"))',
        "an_identity_is_named_by_the_sha256_of_its_certificate",
    ),
    Mutation(
        # Ask the OS for a raw RSA signature, without the DigestInfo a
        # verifier checks. macOS only: the code under it is.
        "keystore: sign RSA without the SHA-256 DigestInfo",
        "src/keystore.rs",
        "            KeyKind::Rsa(_) => Algorithm::RSASignatureDigestPKCS1v15SHA256,",
        "            KeyKind::Rsa(_) => Algorithm::RSASignatureDigestPKCS1v15Raw,",
        "the_os_signs_a_revision_that_the_verifier_calls_intact",
        only_on="macos",
    ),
]

# --- whether an issuer the OS trusts vouches for the signer -------------------
#
# `trust.rs`, 2026-09-27. `judge` is one pure function over evaluations, so most
# of these are aimed at the order of its questions and caught both by a
# scripted test and by the platform test that reaches the same branch through
# `SecTrust`. The platform ones are macOS only: the code under them is, and the
# Windows chain engine has not yet run anywhere (`docs/PLAN.md`, Phase 6).
ROWS += [
    Mutation(
        # Never ask again inside the signer's dates: an expired certificate
        # reads as a chain that failed, with its dates as the reason.
        "trust: never re-ask an expired certificate inside its dates",
        "src/trust.rs",
        "    } else if now > until || now < from {",
        "    } else if false {",
        "a_certificate_that_has_expired_since_reads_as_expired",
    ),
    Mutation(
        # Re-ask every failed chain at another moment, in its dates or not: a
        # chain failing now could then pass then and read as something else.
        "trust: re-ask a certificate that is in its dates",
        "src/trust.rs",
        "    } else if now > until || now < from {",
        "    } else if true {",
        "a_certificate_in_its_dates_is_asked_about_once",
    ),
    Mutation(
        # Re-ask at the present moment rather than the certificate's last one,
        # which is asking the same question twice.
        "trust: re-ask an expired certificate now rather than at its end",
        "src/trust.rs",
        "        let at = now.clamp(from, until);",
        "        let at = now;",
        "an_expired_certificate_is_asked_about_at_its_own_last_moment",
    ),
    Mutation(
        # Call a certificate not yet in force expired.
        "trust: read every out-of-date certificate as expired",
        "src/trust.rs",
        "        let standing = if now > until {",
        "        let standing = if true {",
        "a_certificate_not_yet_in_force_reads_as_not_yet_valid",
    ),
    Mutation(
        # Report the dates of a certificate nobody vouched for: "expired" on a
        # chain that was never trusted, which reads as nearly fine.
        "trust: report the dates of an unvouched certificate",
        "src/trust.rs",
        "        (standing, second)",
        "        (standing, Evaluation { passed: true, ..second })",
        "an_expired_certificate_nobody_vouched_for_is_untrusted_not_expired",
    ),
    Mutation(
        # Never ask what the certificate was issued for.
        "trust: trust a web server's certificate for signing",
        "src/trust.rs",
        "    if !serves {\n        let doubt = match purpose {\n            Purpose::Documents => Doubt::Purpose,\n            Purpose::Timestamping => Doubt::Timestamping,\n        };\n        return answer(Standing::Untrusted, Some(doubt));\n    }\n    answer(standing, None)",
        "    if false {\n        let doubt = match purpose {\n            Purpose::Documents => Doubt::Purpose,\n            Purpose::Timestamping => Doubt::Timestamping,\n        };\n        return answer(Standing::Untrusted, Some(doubt));\n    }\n    answer(standing, None)",
        "a_certificate_issued_only_for_web_servers_is_not_trusted_for_signing",
    ),
    Mutation(
        # The same rule through the scripted path, so it is not left to one OS.
        "trust: accept every extended key usage",
        "src/trust.rs",
        "crate::sign_cms::DOCUMENT_PURPOSES.contains(&named.to_string().as_str())",
        "!named.to_string().is_empty()",
        "the_purpose_is_asked_of_a_trusted_chain_and_an_expired_one",
    ),
    Mutation(
        # Drop the reason from an untrusted standing.
        "trust: say not trusted without saying why",
        "src/trust.rs",
        "        return answer(Standing::Untrusted, Some(evaluation.failure));\n    }\n    if !serves {\n        let doubt = match purpose {\n            Purpose::Documents => Doubt::Purpose,\n            Purpose::Timestamping => Doubt::Timestamping,\n        };\n        return answer(Standing::Untrusted, Some(doubt));\n    }\n    answer(standing, None)",
        "        return answer(Standing::Untrusted, None);\n    }\n    if !serves {\n        let doubt = match purpose {\n            Purpose::Documents => Doubt::Purpose,\n            Purpose::Timestamping => Doubt::Timestamping,\n        };\n        return answer(Standing::Untrusted, Some(doubt));\n    }\n    answer(standing, None)",
        "a_standing_that_is_not_trusted_says_why",
    ),
    Mutation(
        # Read a trust API that could not run as a chain it refused: tpdf's
        # failure reported as a fact about the document.
        "trust: call an evaluation that could not run untrusted",
        "src/trust.rs",
        """    let first = match evaluate(now) {
        Ok(evaluation) => evaluation,
        Err(_) => return Trust::unchecked(Doubt::Unavailable),""",
        """    let first = match evaluate(now) {
        Ok(evaluation) => evaluation,
        Err(_) => return answer(Standing::Untrusted, Some(Doubt::Rejected)),""",
        "an_evaluation_that_could_not_run_is_unchecked_never_untrusted",
    ),
    Mutation(
        # Offer the signer alone: the intermediate the signature carries is
        # never a candidate, and the chain stops short.
        "trust: leave the signature's other certificates out",
        "src/trust.rs",
        "            others.push(der);",
        "            drop(der);",
        "a_blob_is_read_for_its_signer_and_the_rest_of_its_set",
    ),
    Mutation(
        "trust: hand the OS a set of any size",
        "src/trust.rs",
        "    if all.len() > MAX_CERTIFICATES {",
        "    if false {",
        "a_set_larger_than_the_bound_is_not_handed_to_the_os",
    ),
    Mutation(
        # Ignore the test anchors: every chain then ends at a root the system
        # does not hold. The control that "trusted" is reachable at all.
        "trust: ignore the anchors the evaluation was given",
        "src/trust.rs",
        """        if let Anchors::Only(roots) = anchors {
            let roots = roots.iter().map(parse)""",
        """        if let Anchors::Only(roots) = Anchors::System {
            let roots = roots.iter().map(parse)""",
        "a_chain_to_an_anchor_is_trusted",
        only_on="macos",
    ),
    Mutation(
        # Leave the system's roots out of every evaluation: the store refuses
        # everything, which every refusal test passes.
        "trust: consult no system root",
        "src/trust.rs",
        """        trust.set_network_fetch_allowed(false).ok()?;
        Some(trust)""",
        """        trust.set_network_fetch_allowed(false).ok()?;
        trust.set_trust_anchor_certificates_only(true).ok()?;
        Some(trust)""",
        "a_root_this_mac_trusts_is_trusted_through_the_system_store",
        only_on="macos",
    ),
    Mutation(
        # Evaluate at the machine's clock whatever moment was asked for.
        "trust: ignore the moment the chain is evaluated at",
        "src/trust.rs",
        "            .set_trust_verify_date(&date)",
        "            .set_network_fetch_allowed(false)",
        "a_certificate_that_has_expired_since_reads_as_expired",
        only_on="macos",
    ),
    Mutation(
        # Read a chain that stops short as one that ends at an untrusted root.
        "trust: call a missing link an untrusted root",
        "src/trust.rs",
        "        Ok(if incomplete(chain) {",
        "        Ok(if false {",
        "a_missing_intermediate_is_a_missing_link_not_an_untrusted_root",
        only_on="macos",
    ),
    Mutation(
        "trust: call an untrusted root a refusal of another kind",
        "src/trust.rs",
        "                NOT_TRUSTED => Doubt::Root,",
        "                NOT_TRUSTED => Doubt::Rejected,",
        "a_self_signed_certificate_nobody_anchored_ends_at_an_untrusted_root",
        only_on="macos",
    ),
    Mutation(
        # Read a trust daemon that never answered as a chain it refused ---
        # which is what the first version did under a sandbox denying `trustd`.
        "trust: call an unreachable trust service a refusal",
        "src/trust.rs",
        "        if UNREACHED.contains(&code) {",
        "        if false {",
        "a_trust_service_that_did_not_answer_is_not_a_refusal",
        only_on="macos",
    ),
    Mutation(
        # Ask trust of every signature, altered and broken ones included.
        "docinfo: attribute an altered signature to its signer",
        "src/docinfo.rs",
        "    if attributable(&integrity) {\n        let extra = &judging.dss_certificates;",
        "    if true {\n        let extra = &judging.dss_certificates;",
        "trust_is_asked_only_of_a_signature_with_a_signer",
    ),
    Mutation(
        # Ask it of an intact one only: a SHA-1 signature's signer goes unasked.
        "docinfo: leave a weak signature's signer unasked",
        "src/docinfo.rs",
        "    matches!(integrity.verdict, Verdict::Intact | Verdict::Weak)",
        "    matches!(integrity.verdict, Verdict::Intact)",
        "trust_is_asked_only_of_a_signature_with_a_signer",
    ),
]

# --- a visible appearance for a digital signature ----------------------------
#
# Phase 6, 2026-09-27. `sign_prepare/appearance.rs` lays out and draws the
# widget's `/AP`; `sign_prepare.rs` places the widget and refuses what cannot
# be drawn honestly. Each mutation removes one rule and names the test built to
# see it. `sign-probe --visible` holds the rendered result against PDFium and
# PDFKit, and its own mutations are in BUILD.md.
ROWS += [
    Mutation(
        # An appearance dictionary on every widget, visible or not: the
        # invisible revision stops being the bytes it was.
        "sign visible: give the invisible widget an /AP as well",
        "src/sign_prepare.rs",
        "    let mut widget = widget(&field, signature, page);\n",
        '    let mut widget = widget(&field, signature, page);\n    widget.set("AP", Dictionary::new());\n',
        "the_invisible_revision_is_byte_for_byte_what_it_was",
    ),
    Mutation(
        "sign visible: put the widget on the first page whatever was chosen",
        "src/sign_prepare.rs",
        "        (None, placed) => placed.map_or(first, |(page, ..)| page),",
        "        (None, _) => first,",
        "a_visible_widget_has_the_rectangle_the_reader_placed_on_the_page_they_chose",
    ),
    Mutation(
        # Leave the crop box's origin off: right on a page whose crop box starts
        # at zero, and off by the inset on every other.
        "sign visible: map the rectangle without the crop box's origin",
        "src/sign_prepare/appearance.rs",
        "    Ok([page[0] + ox, page[1] + oy, page[2] + ox, page[3] + oy])",
        "    Ok(page)",
        "a_visible_widget_has_the_rectangle_the_reader_placed_on_the_page_they_chose",
    ),
    Mutation(
        "sign visible: draw without clipping to the rectangle",
        "src/sign_prepare/appearance.rs",
        '    let mut content = format!("q 0 0 {x1} {y1} re W n\\n");',
        '    let mut content = format!("q 0 0 {x1} {y1} re n\\n");',
        "the_appearance_names_the_signer_and_the_date_inside_its_rectangle",
    ),
    Mutation(
        "sign visible: leave the signer's name out of the words",
        "src/sign_prepare/appearance.rs",
        "        lines.push(name.to_string());\n",
        "        lines.push(String::new());\n",
        "the_appearance_names_the_signer_and_the_date_inside_its_rectangle",
    ),
    Mutation(
        # The image above the words in a wide box, beside them in a tall one:
        # both layouts fit, so only the placement test can tell.
        "sign visible: put the image beside the words in a tall box",
        "src/sign_prepare/appearance.rs",
        "        Some(_) if width >= height => (",
        "        Some(_) if width < height => (",
        "a_saved_image_is_drawn_beside_the_words_and_text_alone_without_one",
    ),
    Mutation(
        "sign visible: draw the words and image in the page's frame on a turned page",
        "src/sign_prepare/appearance.rs",
        "    let seen = Upright::of(turns, local);",
        "    let seen = Upright::of(0, local);",
        "a_turned_page_gets_an_appearance_that_reads_upright",
    ),
    Mutation(
        # Size the type to the width alone: three lines of it then overflow a
        # short box at the bottom.
        "sign visible: size the type without the height",
        "src/sign_prepare/appearance.rs",
        "        .min(th / (count * LEADING));",
        ";",
        "every_layout_keeps_its_ink_inside_the_box_and_apart_from_the_image",
    ),
    Mutation(
        # Put each baseline at the top of its line: every glyph's body then
        # hangs above the box.
        "sign visible: set a line's baseline at its top",
        "src/sign_prepare/appearance.rs",
        "            let baseline = top + (at as f64) * LEADING * size + ASCENT * size;",
        "            let baseline = top + (at as f64) * LEADING * size;",
        "every_layout_keeps_its_ink_inside_the_box_and_apart_from_the_image",
    ),
    Mutation(
        # Draw any name: `winansi_hex` then writes a space for every character
        # it cannot encode, and a different name goes on the page.
        "sign visible: draw a name WinAnsi cannot spell",
        "src/sign_prepare/appearance.rs",
        "        if !textbox::encodable(name) || name.chars().any(char::is_control) {",
        "        if name.chars().any(char::is_control) {",
        "a_name_that_cannot_be_drawn_honestly_is_refused",
    ),
    Mutation(
        "sign visible: accept a rectangle that is not on its page",
        "src/sign_prepare/appearance.rs",
        "    if !on_page {",
        "    if false {",
        "a_rectangle_off_its_page_too_small_or_on_no_page_is_refused",
    ),
    Mutation(
        "sign visible: accept a rectangle too small to read",
        "src/sign_prepare/appearance.rs",
        "    if f64::from(right - left) < MIN_SIDE || f64::from(bottom - top) < MIN_SIDE {",
        "    if false {",
        "a_rectangle_off_its_page_too_small_or_on_no_page_is_refused",
    ),
    Mutation(
        # Sign a certified document visibly: pyHanko then reads the new field as
        # a change the certification forbids.
        "sign visible: add a visible signature after a certification",
        "src/sign_prepare.rs",
        "    if visible.is_some() && certification(&prev) > 0 {",
        "    if false {",
        "a_visible_signature_after_a_certification_is_refused_and_an_invisible_one_is_not",
    ),
]

# --- configuring a visible signature's appearance ----------------------------
#
# 2026-09-27. The reader chooses the image, which of the three lines are drawn,
# and a reason and location that are also written as `/Reason` and
# `/Location`; a preview is drawn by the code that signs. Each mutation removes
# one rule and names the test built to see it. `sign-probe --visible --reason`
# holds the written file against pyHanko and the preview against PDFium, and
# its own mutations are in BUILD.md.
ROWS += [
    Mutation(
        "sign visible: draw the label whether or not it is on",
        "src/sign_prepare/appearance.rs",
        "    if options.label {\n",
        "    if true {\n",
        "each_line_is_drawn_only_when_it_is_on",
    ),
    Mutation(
        "sign visible: draw the name whether or not it is on",
        "src/sign_prepare/appearance.rs",
        "    if options.name {\n        lines.push(name.to_string());",
        "    if true {\n        lines.push(name.to_string());",
        "each_line_is_drawn_only_when_it_is_on",
    ),
    Mutation(
        "sign visible: draw the date whether or not it is on",
        "src/sign_prepare/appearance.rs",
        "    if options.date {\n",
        "    if true {\n",
        "each_line_is_drawn_only_when_it_is_on",
    ),
    Mutation(
        "sign visible: write the reason and never draw it",
        "src/sign_prepare/appearance.rs",
        '        lines.push(format!("Reason: {reason}"));',
        "        let _ = reason;",
        "each_line_is_drawn_only_when_it_is_on",
    ),
    Mutation(
        "sign visible: write the location and never draw it",
        "src/sign_prepare/appearance.rs",
        '        lines.push(format!("Location: {location}"));',
        "        let _ = location;",
        "each_line_is_drawn_only_when_it_is_on",
    ),
    Mutation(
        "sign visible: accept an appearance that draws nothing",
        "src/sign_prepare/appearance.rs",
        "    if !lines && visible.image.is_none() {",
        "    if false {",
        "a_visible_signature_that_would_draw_nothing_is_refused",
    ),
    Mutation(
        # The image alone keeps the half of the box the words would have had.
        "sign visible: leave the words' half empty when there are no words",
        "src/sign_prepare/appearance.rs",
        "        Some(_) if lines.is_empty() => ([0.0, 0.0, 0.0, 0.0], Some([0.0, 0.0, width, height])),\n",
        "",
        "a_visible_signature_that_would_draw_nothing_is_refused",
    ),
    Mutation(
        "sign visible: check the name even when it is not drawn",
        "src/sign_prepare/appearance.rs",
        "    if options.draws_name() {\n        let name = visible.name.as_str();",
        "    if true {\n        let name = visible.name.as_str();",
        "the_name_is_checked_only_when_it_is_drawn",
    ),
    Mutation(
        # `winansi_hex` then writes a space for every character it cannot
        # encode, and different words go on the page.
        "sign visible: draw a reason WinAnsi cannot spell",
        "src/sign_prepare/appearance.rs",
        "    if text.chars().any(char::is_control) || !textbox::encodable(text) {",
        "    if text.chars().any(char::is_control) {",
        "a_reason_or_location_that_cannot_be_drawn_is_refused",
    ),
    Mutation(
        "sign visible: never check the location",
        "src/sign_prepare/appearance.rs",
        '    check_note("location", options.location())?;\n',
        "",
        "a_reason_or_location_that_cannot_be_drawn_is_refused",
    ),
    Mutation(
        "sign visible: accept a reason of any length",
        "src/sign_prepare/appearance.rs",
        "    if text.chars().count() > MAX_NOTE_CHARS {",
        "    if false {",
        "a_reason_or_location_that_cannot_be_drawn_is_refused",
    ),
    Mutation(
        "sign visible: draw the reason and never write /Reason",
        "src/sign_prepare.rs",
        '        sig.set("Reason", text_string(reason));',
        "        let _ = reason;",
        "a_reason_and_location_are_text_strings_inside_the_range",
    ),
    Mutation(
        "sign visible: draw the location and never write /Location",
        "src/sign_prepare.rs",
        '        sig.set("Location", text_string(location));',
        "        let _ = location;",
        "a_reason_and_location_are_text_strings_inside_the_range",
    ),
    Mutation(
        # Every character as its code point's low byte: CJK becomes noise.
        "sign visible: write a text string as bytes with no UTF-16",
        "src/sign_prepare.rs",
        "        .map(|ch| safe(ch).then_some(ch as u8))",
        "        .map(|ch| Some(ch as u8))",
        "every_character_reads_back_from_its_text_string",
    ),
    Mutation(
        # 0x80..0xA0 are Latin-1 control and space codes and PDFDocEncoding
        # bullets, daggers and the euro sign.
        "sign visible: take PDFDocEncoding for Latin-1 from U+0080",
        "src/sign_prepare.rs",
        "(('\\u{a1}'..='\\u{ff}').contains(&ch) && ch != '\\u{ad}')",
        "(('\\u{80}'..='\\u{ff}').contains(&ch) && ch != '\\u{ad}')",
        "every_character_reads_back_from_its_text_string",
    ),
    Mutation(
        "sign visible: write the soft hyphen, which PDFDocEncoding lacks, as a byte",
        "src/sign_prepare.rs",
        " && ch != '\\u{ad}')",
        ")",
        "every_character_reads_back_from_its_text_string",
    ),
    Mutation(
        # "þÿ ok" as bytes FE FF ...: every reader takes it for UTF-16.
        "sign visible: write Latin-1 that starts like a byte-order mark as bytes",
        "src/sign_prepare.rs",
        "        bytes.filter(|b| !b.starts_with(&[0xFE, 0xFF]) && !b.starts_with(&[0xEF, 0xBB, 0xBF]));",
        "        bytes.filter(|b| !b.starts_with(&[0xEF, 0xBB, 0xBF]));",
        "a_reason_and_location_are_text_strings_inside_the_range",
    ),
    Mutation(
        # The form drawn where the rectangle sits on the page: correct on the
        # page, and a different stream from the preview's at every position
        # but the origin.
        "sign visible: draw the form at the rectangle's page position",
        "src/sign_prepare/appearance.rs",
        "    let local = [0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]];",
        "    let local = rect;",
        "a_preview_draws_the_stream_the_signing_writes",
    ),
    Mutation(
        "sign visible: preview the default lines whatever was chosen",
        "src/sign_prepare.rs",
        "        ..visible.clone()",
        "        options: Options::default(),\n        ..visible.clone()",
        "a_preview_draws_the_stream_the_signing_writes",
    ),
    Mutation(
        "sign visible: preview a rectangle of any size",
        "src/sign_prepare.rs",
        "    if !sized(width) || !sized(height) {",
        "    if false {",
        "a_preview_is_refused_where_the_signing_would_be",
    ),
    Mutation(
        "sign visible: drop the reader's choices on the way to the worker",
        "src/commands/sign.rs",
        "            options: self.options,",
        "            options: sign_prepare::Options::default(),",
        "a_placement_carries_the_readers_choices_to_the_worker",
    ),
]

# The command-line tool (`src/cli.rs`, `src/cli/`, `src/clitool.rs`). What
# needs a worker --- signing and verifying real documents, containment --- is
# `tests/cli.rs`, a harness-free binary this table cannot select; its
# mutations were proved by hand and are listed in `BUILD.md`.
ROWS += [
    Mutation(
        "cli: accept an output that names the input",
        "src/cli/sign.rs",
        "    if lexically_same(&input, &output) {",
        "    if false {",
        "an_output_that_names_the_input_is_refused",
    ),
    Mutation(
        "cli: accept a rectangle with no width",
        "src/cli/sign.rs",
        "        && w > 0.0\n",
        "\n",
        "a_rectangle_that_is_not_four_numbers_with_a_size_is_refused",
    ),
    Mutation(
        "cli: drop an appearance option given without --visible",
        "src/cli/sign.rs",
        "    if !visible {",
        "    if false {",
        "an_appearance_option_without_visible_is_refused_rather_than_dropped",
    ),
    Mutation(
        "cli: accept page 0",
        "src/cli/sign.rs",
        "fn page_number(text: &str) -> Result<u32, String> {\n    match text.trim().parse::<u32>() {\n        Ok(n) if n >= 1 => Ok(n),",
        "fn page_number(text: &str) -> Result<u32, String> {\n    match text.trim().parse::<u32>() {\n        Ok(n) => Ok(n),",
        "a_page_that_is_not_counted_from_one_is_refused",
    ),
    Mutation(
        "cli: count an expired certificate as a rival",
        "src/cli/identities.rs",
        "        .filter(|at| listed[*at].0.offer.is_ok())",
        "        .filter(|_| true)",
        "an_expired_certificate_beside_its_renewal_is_not_a_rival",
    ),
    Mutation(
        "cli: choose the first of two certificates that can sign",
        "src/cli/identities.rs",
        "        [one] => Ok(*one),",
        "        [one, ..] => Ok(*one),",
        "a_subject_two_usable_certificates_share_is_refused_and_both_are_listed",
    ),
    Mutation(
        "cli: match a SHA-256 only in lowercase",
        "src/cli/identities.rs",
        "        let wanted = wanted.to_ascii_lowercase();",
        "        let wanted = wanted.to_string();",
        "a_subject_two_usable_certificates_share_is_refused_and_both_are_listed",
    ),
    Mutation(
        "cli: judge certificates at the epoch rather than now",
        "src/cli/identities.rs",
        "                offer: sign_cms::usable(der, now),",
        "                offer: sign_cms::usable(der, 0),",
        "the_identities_report_sorts_by_the_choosers_rule_and_keeps_every_id",
    ),
    Mutation(
        "cli: let --strict pass whatever the verdicts",
        "src/cli/verify.rs",
        "        None if strict && !report.strict_passed => Exit::Strict,",
        "        None if false => Exit::Strict,",
        "strict_passes_only_intact_and_trusted_signatures_in_every_document",
    ),
    Mutation(
        "cli: pass --strict on an untrusted signature",
        "src/cli/verify.rs",
        "            .is_some_and(|t| matches!(t.standing, Standing::Trusted | Standing::TrustedAtTimestamp))",
        "            .is_some_and(|_| true)",
        "strict_passes_only_intact_and_trusted_signatures_in_every_document",
    ),
    Mutation(
        "cli: pass --strict on a document with no signature",
        "src/cli/verify.rs",
        "            && file.signatures.iter().any(|s| !s.document_timestamp)\n",
        "\n",
        "strict_passes_only_intact_and_trusted_signatures_in_every_document",
    ),
    Mutation(
        "cli: pass --strict on a document with nothing but a document timestamp",
        "src/cli/verify.rs",
        "            && file.signatures.iter().any(|s| !s.document_timestamp)\n",
        "            && !file.signatures.is_empty()\n",
        "strict_fails_a_document_with_only_a_document_timestamp",
    ),
    Mutation(
        "cli: report a worker's failure as a refusal",
        "src/cli/verify.rs",
        "            ErrorKind::Failed => Exit::Internal,",
        "            ErrorKind::Failed => Exit::Refused,",
        "a_document_that_could_not_be_read_outranks_every_verdict",
    ),
    Mutation(
        "cli: replace an existing output without --force",
        "src/cli/sign.rs",
        "    if !sign.force && sign.output.exists() {",
        "    if false {",
        "signing_refusals_that_need_no_worker_exit_before_one_is_asked_for",
    ),
    Mutation(
        "cli: sign over the input reached by a second name",
        "src/cli/sign.rs",
        "    if save::same_file(&sign.input, &sign.output) {",
        "    if false {",
        "signing_refusals_that_need_no_worker_exit_before_one_is_asked_for",
    ),
    Mutation(
        "cli: a word of the tool's own drifts from the application's",
        "src/words.rs",
        '    "Whether that key belongs to the person the certificate names was not checked.";',
        '    "Whether this key belongs to the person the certificate names was not checked.";',
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    Mutation(
        "cli: rename a JSON key without moving the schema",
        "src/cli/report.rs",
        "    pub claimed_time: String,",
        '    #[serde(rename = "time")]\n    pub claimed_time: String,',
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    Mutation(
        "cli: a registered command that nothing dispatches to",
        "src/cli.rs",
        "    redact::COMMAND,\n",
        "",
        "every_registered_command_is_reached_by_its_name_and_listed_in_help",
    ),
    Mutation(
        "cli: read the saved image from another application's store",
        "src/cli.rs",
        'pub const IDENTIFIER: &str = "com.timostein.tpdf";',
        'pub const IDENTIFIER: &str = "com.timostein.tpdf.cli";',
        "the_identifier_is_the_applications",
    ),
    Mutation(
        "cli render: omit the total pixel budget",
        "src/cli/render.rs",
        "f64::from(width) * f64::from(height) > 16_777_216.0",
        "false",
        "rendering_bounds_are_checked_before_allocation",
    ),
    Mutation(
        "cli render: accept a truncated raw tile",
        "src/save_outside.rs",
        "stated != expected || stated > capacity",
        "stated > expected || stated > capacity",
        "raw_tile_bytes_must_match_geometry_and_fit_the_mapping",
    ),
    Mutation('signature image: any width', 'src/signature_import.rs', '        || width > limits.side\n', '', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: any height', 'src/signature_import.rs', '        || height > limits.side\n', '', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: any number of pixels', 'src/signature_import.rs', '        || u64::from(width) * u64::from(height) > limits.pixels\n', '', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: a file of any size', 'src/signature_import.rs', '    if bytes.len() as u64 > limits.bytes {', '    if false {', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: an animated PNG', 'src/signature_import.rs', '                || kind == b"acTL"\n', '', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: bytes after the PNG', 'src/signature_import.rs', '                end = length == 0 && offset == bytes.len();', '                end = true;', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: a PNG without its end', 'src/signature_import.rs', '        return if data && end {', '        return if data {', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: bytes after the JPEG', 'src/signature_import.rs', '                Some(size) if scan && offset == bytes.len() => Ok(size),', '                Some(size) if scan => Ok(size),', 'what_the_chooser_refuses_is_refused_before_decoding'),
    Mutation('signature image: sixteen-bit samples refused', 'src/signature_import.rs', 'png::Transformations::STRIP_16 | png::Transformations::ALPHA', 'png::Transformations::ALPHA', 'every_png_colour_type_and_depth_is_read_as_straight_rgba'),
    Mutation('signature image: no alpha is added', 'src/signature_import.rs', 'png::Transformations::STRIP_16 | png::Transformations::ALPHA', 'png::Transformations::STRIP_16', 'every_png_colour_type_and_depth_is_read_as_straight_rgba'),
    Mutation('signature image: transparent margins are kept', 'src/signature_import.rs', '    (right > left).then_some([left, top, right, bottom])', '    (right > left).then_some([0, 0, width, height])', 'a_png_is_trimmed_to_what_it_shows_and_keeps_its_pixels'),
    Mutation('signature image: scaled up to fill', 'src/signature_import.rs', '        .min(f64::from(HEIGHT) / f64::from(h))\n        .min(1.);', '        .min(f64::from(HEIGHT) / f64::from(h));', 'a_large_image_is_scaled_to_fit_512_by_256_and_never_up'),
    Mutation('signature image: as tall as it is wide', 'src/signature_import.rs', 'const HEIGHT: u32 = 256;', 'const HEIGHT: u32 = 512;', 'a_large_image_is_scaled_to_fit_512_by_256_and_never_up'),
    Mutation('signature image: transparent pixels tint the average', 'src/signature_import.rs', '                    let a = u64::from(rgba[at + 3]);', '                    let a = u64::from(rgba[at + 3]).max(1);', 'a_large_image_is_scaled_to_fit_512_by_256_and_never_up'),
    Mutation('signature image: a picture scaled away to nothing is drawn', 'src/signature_import.rs', '    if !image.valid() {', '    if false {', 'a_file_the_header_passes_and_the_decoder_does_not_is_refused'),
    Mutation('sign: --image without --visible', 'src/cli/sign.rs', '        ("--image", image.is_some()),\n', '', 'an_image_file_is_a_visible_signatures_and_excludes_no_image'),
    Mutation('sign: --image and --no-image together', 'src/cli/sign.rs', '    if no_image && image.is_some() {', '    if false {', 'an_image_file_is_a_visible_signatures_and_excludes_no_image'),
    Mutation('sign: --hide without --visible', 'src/cli/sign.rs', '        ("--hide", hide.is_some()),\n', '', 'hide_names_what_is_written_and_not_drawn'),
    Mutation('sign: --hide of something not given', 'src/cli/sign.rs', '        if hidden && !given {', '        if false {', 'hide_names_what_is_written_and_not_drawn'),
    Mutation('sign: --hide naming nothing', 'src/cli/sign.rs', '    if hidden == Hidden::default() {', '    if false {', 'hide_names_what_is_written_and_not_drawn'),
    Mutation('sign: a hidden reason is drawn', 'src/sign_prepare/appearance.rs', '        self.reason().filter(|_| !self.hide_reason)', '        self.reason()', 'a_hidden_reason_or_location_is_written_and_not_drawn'),
    Mutation('sign: a hidden location is drawn', 'src/sign_prepare/appearance.rs', '        self.location().filter(|_| !self.hide_location)', '        self.location()', 'a_hidden_reason_or_location_is_written_and_not_drawn'),
    Mutation('sign: a hidden reason counts as something shown', 'src/sign_prepare/appearance.rs', '        || options.drawn_reason().is_some()', '        || options.reason().is_some()', 'a_hidden_reason_or_location_is_written_and_not_drawn'),
    Mutation('cli edit: the font used is left out of the report', 'src/cli/edit.rs', '                    if font.is_some() {\n                        let used', '                    if false {\n                        let used', 'a_named_font_sends_the_box_the_editor_opens_on_that_run'),
    Mutation('cli edit: the font is reported against the operation before', 'src/cli/edit.rs', '                            operation: index + 1,', '                            operation: index,', 'a_named_font_sends_the_box_the_editor_opens_on_that_run'),
    Mutation('cli edit: a reply that names no font passes', 'src/cli/edit.rs', '                        let used = validated.preview.ok_or_else(|| {\n                            OperationError::failed("worker did not name the font it used")\n                        })?;', '                        let Some(used) = validated.preview else {\n                            model.replace_text(1, id, change)?;\n                            return Ok(());\n                        };', 'a_named_font_sends_the_box_the_editor_opens_on_that_run'),
    Mutation('identity: a thumbprint is read as a subject', 'src/cli/identities.rs', '        40 if hex => Some("SHA-1"),\n', '', 'a_certificate_is_named_by_the_thumbprint_windows_shows_for_it'),
    Mutation('identity: a thumbprint is looked up among the SHA-256s', 'src/cli/identities.rs', '                entry.sha1 == wanted', '                entry.id == wanted', 'a_certificate_is_named_by_the_thumbprint_windows_shows_for_it'),
    Mutation('identity: an unusable certificate is reported under its SHA-256 twice', 'src/cli/identities.rs', '                sha1: entry.sha1.clone(),', '                sha1: entry.id.clone(),', 'a_certificate_is_named_by_the_thumbprint_windows_shows_for_it'),
    Mutation('identity: the listing prints the thumbprint as stored', 'src/cli/identities.rs', '            lines.push(format!("  {} --- {}", id.subject, id.why));\n            lines.push(format!("    {}", id.id));\n            lines.push(format!("    SHA-1 {}", id.sha1.to_ascii_uppercase()));', '            lines.push(format!("  {} --- {}", id.subject, id.why));\n            lines.push(format!("    {}", id.id));\n            lines.push(format!("    SHA-1 {}", id.sha1));', 'a_certificate_is_named_by_the_thumbprint_windows_shows_for_it'),
    Mutation('sign text: the standard lines are drawn beside a text', 'src/sign_prepare/appearance.rs', '    if !options.text.is_empty() {\n        return options', '    if false {\n        return options', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: the name line decides whether a text draws the name', 'src/sign_prepare/appearance.rs', '        if self.text.is_empty() {\n            self.name', '        if true {\n            self.name', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: a text is not checked', 'src/sign_prepare/appearance.rs', '    check_text(visible)?;\n', '', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: one line more than the bound', 'src/sign_prepare/appearance.rs', '    if options.text.len() > MAX_TEXT_LINES {', '    if options.text.len() > MAX_TEXT_LINES + 1 {', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: one line fewer than the bound', 'src/sign_prepare/appearance.rs', '    if options.text.len() > MAX_TEXT_LINES {', '    if options.text.len() >= MAX_TEXT_LINES {', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: a date format one character past the bound', 'src/sign_prepare/appearance.rs', '    if format.chars().count() > MAX_FORMAT_CHARS {', '    if format.chars().count() > MAX_FORMAT_CHARS + 1 {', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: a date format one character short of the bound', 'src/sign_prepare/appearance.rs', '    if format.chars().count() > MAX_FORMAT_CHARS {', '    if format.chars().count() >= MAX_FORMAT_CHARS {', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: a line of any length', 'src/sign_prepare/appearance.rs', '        if own.chars().count() > MAX_NOTE_CHARS {', '        if false {', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: a text with no words is drawn', 'src/sign_prepare/appearance.rs', '    if !drawn {\n        return Err(said!(\n            "the text has no words', '    if false {\n        return Err(said!(\n            "the text has no words', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: a doubled brace stays doubled', 'src/sign_prepare/appearance.rs', "            words.push('{');\n            rest = after;", '            words.push_str("{{");\n            rest = after;', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign text: the reason is left out of a line that asks for it', 'src/sign_prepare/appearance.rs', '            Part::Reason => out.push_str(options.reason().unwrap_or_default()),', '            Part::Reason => {}', 'a_text_is_drawn_in_place_of_the_standard_lines'),
    Mutation('sign date: minutes where the hours go', 'src/sign_prepare/appearance.rs', '        ("HH", part(8, 10)),', '        ("HH", part(10, 12)),', 'a_date_is_written_token_by_token'),
    Mutation('sign date: hours where the minutes go', 'src/sign_prepare/appearance.rs', '        ("mm", part(10, 12)),', '        ("mm", part(8, 10)),', 'a_date_is_written_token_by_token'),
    Mutation('sign date: a blank format writes nothing', 'src/sign_prepare/appearance.rs', '    let format = if format.trim().is_empty() {', '    let format = if false {', 'a_date_is_written_token_by_token'),
    Mutation('sign text: a second --text replaces the first', 'src/cli/sign.rs', '            (false, "--text") => text.extend(text_lines(value(arg, &mut rest)?)),', '            (false, "--text") => text = text_lines(value(arg, &mut rest)?),', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign text: --text passes without --visible', 'src/cli/sign.rs', '        ("--text", !text.is_empty()),\n', '', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign text: --lines and --hide pass beside a text', 'src/cli/sign.rs', '    for (flag, given) in [("--lines", lines.is_some()), ("--hide", hide)] {\n        if given {', '    for (flag, given) in [("--lines", lines.is_some()), ("--hide", hide)] {\n        if false && given {', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign text: a reason that was not given is asked for', 'src/cli/sign.rs', '                    if !given {\n                        return Err(format!(\n                            "the text asks for', '                    if false {\n                        return Err(format!(\n                            "the text asks for', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign text: a text with no words parses', 'src/cli/sign.rs', '    if !words {\n        return Err(\n            "`--text` was given no words', '    if false {\n        return Err(\n            "`--text` was given no words', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign date: a format passes with no date drawn', 'src/cli/sign.rs', '        if !date_drawn {', '        if false {', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign date: a text counts as drawing a date', 'src/cli/sign.rs', '                Part::Date => (date, words) = (true, true),', '                Part::Date => words = true,', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign date: the standard date line is ignored', 'src/cli/sign.rs', '        return Ok(lines.unwrap_or_default().date);', '        return Ok(false);', 'text_is_the_whole_wording_and_is_held_to_the_rest_of_the_line'),
    Mutation('sign notes: the contact is not written', 'src/sign_prepare.rs', '    if let Some(contact) = &details.contact {\n        sig.set("ContactInfo", text_string(contact));\n    }\n', '', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: an invisible signature drops its reason', 'src/sign_prepare.rs', '            reason: note("reason", &notes.reason)?,', '            reason: None,', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: an invisible signature drops its location', 'src/sign_prepare.rs', '            location: note("location", &notes.location)?,', '            location: None,', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation("sign notes: a visible signature takes the note's reason", 'src/sign_prepare.rs', '                reason: visible.options.reason().map(str::to_string),\n                location: visible.options.location().map(str::to_string),\n                contact,', '                reason: note("reason", &notes.reason)?,\n                location: visible.options.location().map(str::to_string),\n                contact,', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: a visible signature drops the contact', 'src/sign_prepare.rs', '                location: visible.options.location().map(str::to_string),\n                contact,', '                location: visible.options.location().map(str::to_string),\n                contact: None,', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: a visible signature is not checked', 'src/sign_prepare.rs', '        Some(visible) => {\n            appearance::check(visible)?;', '        Some(visible) => {', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: a note of any length', 'src/sign_prepare.rs', '        if text.chars().count() > MAX_NOTE_CHARS {\n            return Err(said!(\n                "the {what} is longer', '        if false {\n            return Err(said!(\n                "the {what} is longer', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: one character short of the bound', 'src/sign_prepare.rs', '        if text.chars().count() > MAX_NOTE_CHARS {\n            return Err(said!(\n                "the {what} is longer', '        if text.chars().count() >= MAX_NOTE_CHARS {\n            return Err(said!(\n                "the {what} is longer', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: a control character is carried', 'src/sign_prepare.rs', '        if text.chars().any(char::is_control) {\n            return Err(said!(\n                "the {what} has a control', '        if false {\n            return Err(said!(\n                "the {what} has a control', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign notes: a note is written untrimmed', 'src/sign_prepare.rs', '        let text = text.trim();\n        if text.chars().count() > MAX_NOTE_CHARS {', '        if text.chars().count() > MAX_NOTE_CHARS {', 'an_invisible_signature_carries_its_notes_and_a_visible_one_its_contact'),
    Mutation('sign lock: --lock is read and dropped', 'src/cli/sign.rs', '            (false, "--lock") => lock = true,', '            (false, "--lock") => lock = false,', 'lock_is_a_flag_of_sign_that_its_help_explains'),
    Mutation('sign lock: a locked document is signed again', 'src/sign_prepare.rs', 'if !details.document_timestamp && crate::docinfo::locked(&prev)? {', 'if false {', 'a_locked_document_takes_a_document_timestamp_and_no_further_signature'),
    Mutation('sign lock: a locked document takes no document timestamp', 'src/sign_prepare.rs', 'if !details.document_timestamp && crate::docinfo::locked(&prev)? {', 'if crate::docinfo::locked(&prev)? {', 'a_locked_document_takes_a_document_timestamp_and_no_further_signature'),
    Mutation('sign lock: a lock is written into a certified document', 'src/sign_prepare.rs', 'if details.lock && certification(&prev) > 0 {', 'if false {', 'a_lock_is_not_written_into_a_certified_document'),
    Mutation('sign lock: no transform in the signature', 'src/sign_prepare.rs', '    if details.lock && !details.document_timestamp {\n        sig.set("Reference", lock_reference(root));', '    if false {\n        sig.set("Reference", lock_reference(root));', 'a_locking_signature_says_so_on_the_field_and_inside_the_signature'),
    Mutation('sign lock: a document timestamp carries the transform', 'src/sign_prepare.rs', '    if details.lock && !details.document_timestamp {\n        sig.set("Reference", lock_reference(root));', '    if details.lock {\n        sig.set("Reference", lock_reference(root));', 'a_document_timestamp_is_never_a_lock'),
    Mutation('sign lock: no /Lock on a new field', 'src/sign_prepare.rs', '        if details.lock {\n            widget.set("Lock", field_lock());', '        if false {\n            widget.set("Lock", field_lock());', 'a_locking_signature_says_so_on_the_field_and_inside_the_signature'),
    Mutation('sign lock: no /Lock on a field the document has', 'src/sign_prepare.rs', '                .set("Lock", field_lock());', '                .set("NoLock", field_lock());', 'a_lock_in_a_field_the_document_has_is_written_on_that_field'),
    Mutation('sign lock: the revision never says it locks', 'src/sign_prepare.rs', 'locked: details.lock && !details.document_timestamp,', 'locked: false,', 'a_locking_signature_says_so_on_the_field_and_inside_the_signature'),
    Mutation('lock read: the more lenient of the two places', 'src/docinfo.rs', '        (a, b) => a.min(b),', '        (a, b) => a.max(b),', 'a_lock_is_the_stricter_of_what_the_field_and_the_signature_say'),
    Mutation('lock read: any level makes a document locked', 'src/docinfo.rs', 'locked |= lock_of(document, field, sig) == 1;', 'locked |= lock_of(document, field, sig) >= 1;', 'a_signed_field_that_locks_at_level_one_makes_the_document_locked'),
    Mutation('lock read: the scan reports no lock', 'src/docinfo.rs', '    out.lock = lock_of(document, field, sig);', '    out.lock = 0;', 'a_signed_field_that_locks_at_level_one_makes_the_document_locked'),
    Mutation('lock read-back: another level passes', 'src/commands/sign.rs', '    let unlocked = lock && ours.lock != 1;', '    let unlocked = lock && ours.lock == 0;', 'a_lock_that_does_not_read_back_is_not_a_signing_that_holds'),
    Mutation('lock read-back: never held to the lock', 'src/commands/sign.rs', '    let unlocked = lock && ours.lock != 1;', '    let unlocked = false;', 'a_lock_that_does_not_read_back_is_not_a_signing_that_holds'),
    # The lock needs no hashing, so a read-back out of budget still says it.
    Mutation('lock read-back: a budget answer hides a missing lock', 'src/commands/sign.rs', '            return if unlocked {', '            return if false {', 'a_copy_that_does_not_read_back_as_written_is_refused_in_the_window_too'),
    # A form past the walk is no answer, and a lock behind an inherited type is a lock.
    Mutation('lock read: a walk cut short reads as not locked', 'src/docinfo.rs', '    if cut.dropped > 0 || cut.too_deep > 0 {\n        return Err(said!(\n            "this form has more fields, or groups nested deeper, than tpdf reads, so it \\\n             cannot tell whether a signature locked', '    if false {\n        return Err(said!(\n            "this form has more fields, or groups nested deeper, than tpdf reads, so it \\\n             cannot tell whether a signature locked', 'a_lock_behind_an_inherited_type_or_past_the_walk_is_not_missed'),
    Mutation('lock read: the type read on the node alone', 'src/docinfo.rs', '        let typed = inherited(document, field, b"FT")', '        let typed = field.get(b"FT").ok()', 'a_lock_behind_an_inherited_type_or_past_the_walk_is_not_missed'),
    Mutation('lock read: groups walked only eight deep', 'src/docinfo.rs', 'pub(crate) fn locked(document: &Document) -> Result<bool, String> {\n    let bounds = fields::Bounds {\n        nodes: MAX_FIELD_NODES,\n        depth: Some(32),', 'pub(crate) fn locked(document: &Document) -> Result<bool, String> {\n    let bounds = fields::Bounds {\n        nodes: MAX_FIELD_NODES,\n        depth: Some(8),', 'a_form_too_deep_to_read_every_name_of_is_not_given_a_guessed_one'),
    Mutation('sign notes: --contact is read as the location', 'src/cli/sign.rs', '            (false, "--contact") => contact = Some(value(arg, &mut rest)?.clone()),', '            (false, "--contact") => location = Some(value(arg, &mut rest)?.clone()),', 'a_reason_a_location_and_a_contact_need_no_visible_signature'),
    Mutation('sign anchor: --rect and --anchor pass together', 'src/cli/sign.rs', '    if rect.is_some() {\n        return Err(\n            "`--rect` says where', '    if false {\n        return Err(\n            "`--rect` says where', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: an anchor needs no size', 'src/cli/sign.rs', '        size.ok_or("`--anchor` needs `--size w,h`: the signature\'s width and height in points")?;', '        size.unwrap_or([1.0, 1.0]);', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: an anchor that matches nothing parses', 'src/cli/sign.rs', '    if found.matches_nothing() {\n        return Err(format!(\n            "`--anchor` needs', '    if false {\n        return Err(format!(\n            "`--anchor` needs', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: --anchor passes without --visible', 'src/cli/sign.rs', '        ("--anchor", anchor.is_some()),\n', '', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: --size passes without --visible', 'src/cli/sign.rs', '        ("--size", size.is_some()),\n        ("--no-image", no_image),', '        ("--no-image", no_image),', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: --offset passes without an anchor', 'src/cli/sign.rs', '            ("--offset", offset.is_some()),\n            ("--size", size.is_some()),\n        ] {', '            ("--size", size.is_some()),\n        ] {', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: a size of nothing', 'src/cli/sign.rs', '(!size || (*a > 0.0 && *b > 0.0))', '(!size || (*a >= 0.0 && *b > 0.0))', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: an offset must be above zero', 'src/cli/sign.rs', '(!size || (*a > 0.0 && *b > 0.0))', '(*a > 0.0 && *b > 0.0)', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: an offset that is not finite', 'src/cli/sign.rs', '    let fine = a.is_finite() && b.is_finite() && (', '    let fine = (', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: the offset is dropped', 'src/cli/sign.rs', '        offset: offset.unwrap_or([0.0, 0.0]),', '        offset: [0.0, 0.0],', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: the match number is dropped', 'src/cli/sign.rs', '        text,\n        nth,\n', '        text,\n        nth: None,\n', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('sign anchor: accept match 0', 'src/cli/sign.rs', 'fn match_number(text: &str) -> Result<u32, String> {\n    match text.trim().parse::<u32>() {\n        Ok(n) if n >= 1 => Ok(n),', 'fn match_number(text: &str) -> Result<u32, String> {\n    match text.trim().parse::<u32>() {\n        Ok(n) => Ok(n),', 'an_anchor_says_where_a_visible_signature_goes_instead_of_a_rectangle'),
    Mutation('identity: the thumbprint is printed in capitals in the report', 'src/keystore.rs', '    sha1::Sha1::digest(certificate)\n        .iter()\n        .map(|b| format!("{b:02x}"))', '    sha1::Sha1::digest(certificate)\n        .iter()\n        .map(|b| format!("{b:02X}"))', 'a_certificate_is_named_by_the_thumbprint_windows_shows_for_it'),
    Mutation('user path: --add and --remove pass together', 'src/cli/path.rs', '        if what != Path::Show && what != asked {', '        if false {', 'path_shows_adds_or_removes_and_never_both'),
    Mutation('user path: --remove is read as --add', 'src/cli/path.rs', '            "--remove" => Path::Remove,', '            "--remove" => Path::Add,', 'path_shows_adds_or_removes_and_never_both'),
    Mutation('user path: a folder already there is added again', 'src/userpath.rs', "    if value.split(';').any(|entry| key(entry) == key(dir)) {\n        return None;\n    }\n    let kept", '    let kept', 'a_folder_is_added_last_and_once'),
    Mutation('user path: a trailing separator leaves an empty entry', 'src/userpath.rs', "    let kept = value.trim_end_matches(';');", '    let kept = value;', 'a_folder_is_added_last_and_once'),
    Mutation('user path: the folder is added first', 'src/userpath.rs', '        format!("{kept};{dir}")', '        format!("{dir};{kept}")', 'a_folder_is_added_last_and_once'),
    Mutation('user path: case tells two folders apart', 'src/userpath.rs', '        .to_lowercase()', '        .to_string()', 'a_folder_is_added_last_and_once'),
    Mutation('user path: a trailing backslash tells two folders apart', 'src/userpath.rs', "        .trim_end_matches(['\\\\', '/'])\n", '', 'a_folder_is_added_last_and_once'),
    Mutation('user path: quotes tell two folders apart', 'src/userpath.rs', '        .trim_matches(\'"\')\n', '', 'a_folder_is_added_last_and_once'),
    Mutation('user path: a folder that starts the same is the same', 'src/userpath.rs', "    if value.split(';').any(|entry| key(entry) == key(dir)) {\n        return None;\n    }\n    let kept", "    if value.split(';').any(|entry| key(entry).starts_with(&key(dir))) {\n        return None;\n    }\n    let kept", 'a_folder_is_added_last_and_once'),
    Mutation('user path: removing drops the entries around it', 'src/userpath.rs', '            .filter(|entry| key(entry) != key(dir))', '            .filter(|entry| key(entry) == key(dir))', 'a_folder_is_removed_and_every_other_entry_is_kept_as_written'),
    Mutation('user path: removing a folder that is not there rewrites the value', 'src/userpath.rs', "    if !value.split(';').any(|entry| key(entry) == key(dir)) {\n        return None;\n    }\n", '', 'a_folder_is_removed_and_every_other_entry_is_kept_as_written'),
    Mutation('user path: removing takes a folder that starts the same', 'src/userpath.rs', '            .filter(|entry| key(entry) != key(dir))', '            .filter(|entry| !key(entry).starts_with(&key(dir)))', 'a_folder_is_removed_and_every_other_entry_is_kept_as_written'),
    Mutation('signature headers: a longer side than the chooser', 'src/signature_import.rs', 'pub const MAX_SIDE: u32 = 8192;', 'pub const MAX_SIDE: u32 = 8193;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a shorter side than the chooser', 'src/signature_import.rs', 'pub const MAX_SIDE: u32 = 8192;', 'pub const MAX_SIDE: u32 = 8191;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: more pixels than the chooser', 'src/signature_import.rs', 'pub const MAX_PIXELS: u64 = 8 * 1024 * 1024;', 'pub const MAX_PIXELS: u64 = 8 * 1024 * 1024 + 4096;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: fewer pixels than the chooser', 'src/signature_import.rs', 'pub const MAX_PIXELS: u64 = 8 * 1024 * 1024;', 'pub const MAX_PIXELS: u64 = 8 * 1024 * 1024 - 1;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a larger file than the chooser', 'src/signature_import.rs', 'pub const MAX_BYTES: u64 = 10 * 1024 * 1024;', 'pub const MAX_BYTES: u64 = 10 * 1024 * 1024 + 1;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a smaller file than the chooser', 'src/signature_import.rs', 'pub const MAX_BYTES: u64 = 10 * 1024 * 1024;', 'pub const MAX_BYTES: u64 = 10 * 1024 * 1024 - 1;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a lossless JPEG', 'src/signature_import.rs', '            if !matches!(marker, 0xc0..=0xc2)\n', '            if !matches!(marker, 0xc0..=0xc3)\n', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: no progressive JPEG', 'src/signature_import.rs', '            if !matches!(marker, 0xc0..=0xc2)\n', '            if !matches!(marker, 0xc0..=0xc1)\n', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a twelve-bit JPEG', 'src/signature_import.rs', '                || bytes[offset + 2] != 8\n', '', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a second JPEG frame', 'src/signature_import.rs', '                || size.is_some()\n', '', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a JPEG scan before its frame', 'src/signature_import.rs', '            if size.is_none() {\n                return Err(invalid());\n            }\n            scan = true;', '            scan = true;', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a JPEG that changes its height late', 'src/signature_import.rs', '        if marker == 0xdc {', '        if false {', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a second PNG header', 'src/signature_import.rs', '                || (offset != 8 && kind == b"IHDR")\n', '', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a PNG end that carries data', 'src/signature_import.rs', '                end = length == 0 && offset == bytes.len();', '                end = offset == bytes.len();', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation('signature headers: a PNG without image data', 'src/signature_import.rs', '        return if data && end {', '        return if end {', 'the_shared_headers_are_read_as_the_chooser_reads_them'),
    Mutation(
        "cli tool: show the verbatim path",
        "src/clitool.rs",
        'path.strip_prefix(r"\\\\?\\").unwrap_or(path).to_owned()',
        "path.to_owned()",
        "a_shown_path_drops_the_verbatim_prefix_and_nothing_else",
    ),
    Mutation(
        "cli tool: show a share as a drive-less path",
        "src/clitool.rs",
        'format!(r"\\\\{share}")',
        "share.to_owned()",
        "a_shown_path_drops_the_verbatim_prefix_and_nothing_else",
    ),
    Mutation(
        "cli edit: flatten a worker failure into an ordinary refusal",
        "src/cli/edit.rs",
        "            self.0.exit,\n",
        "            Exit::Refused,\n",
        "worker_failures_keep_their_exit_code_and_operation_context",
    ),
    Mutation(
        "cli edit: accept an unsupported request schema",
        "src/cli/edit.rs",
        "if request.schema != 1 {",
        "if request.schema == 0 {",
        "edit_plan_rejects_unknown_fields_versions_and_unbounded_requests",
    ),
    Mutation(
        "cli edit: drop the layout a named font asks for",
        "src/cli/edit.rs",
        "                        layout,\n",
        "                        layout: None,\n",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli edit: build the box from another run",
        "src/cli/edit.rs",
        ".find(|run| run.operator == *operator)",
        ".find(|run| run.operator != *operator)",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli edit: ignore the font the plan names",
        "src/cli/edit.rs",
        "Layout::opened(run, *font)",
        "Layout::opened(run, Default::default())",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli edit: hint at a font on every refusal",
        "src/cli/edit.rs",
        "if font.is_none() && crate::textedit::wants_fallback(&message) =>",
        "if font.is_none() =>",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli edit: hint at a font when one was named",
        "src/cli/edit.rs",
        "if font.is_none() && crate::textedit::wants_fallback(&message) =>",
        "if crate::textedit::wants_fallback(&message) =>",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli edit: no hint for a font that forbids editing",
        "src/textedit.rs",
        "refusal == fonts::forbidden() || refusal == latin_only()",
        "refusal == latin_only()",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli edit: no hint for a character the font lacks",
        "src/textedit.rs",
        "refusal == fonts::forbidden() || refusal == latin_only()",
        "refusal == fonts::forbidden()",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "opened layout: a fixed box",
        "src/textedit.rs",
        "            font,\n            grow: true,",
        "            font,\n            grow: false,",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "opened layout: measure the width down the page",
        "src/textedit.rs",
        "round(run.advance * x).max(0.1)",
        "round(run.advance * y).max(0.1)",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "opened layout: ignore the run's minimum height",
        "src/textedit.rs",
        ".max(run.minimum_height.unwrap_or(0.))",
        ".max(0.)",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "opened layout: a line of one em",
        "src/textedit.rs",
        "(source_size * 1.25)",
        "(source_size * 1.0)",
        "a_named_font_sends_the_box_the_editor_opens_on_that_run",
    ),
    Mutation(
        "cli pages: overwrite a destination that appeared during processing",
        "src/cli/pages.rs",
        "std::fs::hard_link(staged, target).map_err(|e| e.to_string())",
        "std::fs::copy(staged, target).map(|_| ()).map_err(|e| e.to_string())",
        "publication_refuses_a_late_collision_and_cleans_only_owned_staging",
    ),
    Mutation(
        "cli pages: let --force write through a link or onto a directory",
        "src/cli/pages.rs",
        "        if !metadata.is_file() || !force {",
        "        if !force {",
        "force_does_not_write_through_a_symlinked_or_directory_output",
    ),
    Mutation(
        "cli publish: read back and publish a copy whose source changed during the write",
        "src/cli/pages.rs",
        "    let (changed, written) = write(staged)?;\n    if changed {",
        "    let (changed, written) = write(staged)?;\n    if changed && false {",
        "a_source_that_changed_during_the_write_publishes_nothing",
    ),
    Mutation(
        "cli publish: publish a copy whose source changed while it was read back",
        "src/cli/pages.rs",
        "    let read = read_back(written)?;\n    unchanged(sources)?;\n",
        "    let read = read_back(written)?;\n",
        "a_source_that_changed_during_the_write_publishes_nothing",
    ),
    Mutation(
        "cli publish: stage a copy of a source that had already changed",
        "src/cli/pages.rs",
        "    unchanged(sources)?;\n    let (changed, written) = write(staged)?;",
        "    let (changed, written) = write(staged)?;",
        "a_source_that_changed_during_the_write_publishes_nothing",
    ),
    Mutation(
        "cli publish: advise about edits a command line does not have",
        "src/cli/pages.rs",
        "        opened_as.compare_deeply(path).map_err(|fact| {",
        "        opened_as.agrees_with(path).map_err(|fact| {",
        "a_source_that_changed_during_the_write_publishes_nothing",
    ),
    Mutation(
        "cli publish: pass on the advice the writer gives a reader with a window",
        "src/cli/pages.rs",
        "    if why.changed {\n        source_changed()",
        "    if why.changed && false {\n        source_changed()",
        "a_source_that_changed_during_the_write_publishes_nothing",
    ),
    Mutation(
        "cli fill: publish a copy whose fields do not read back as asked",
        "src/cli/fill.rs",
        "            if !wrong.is_empty() {",
        "            if !wrong.is_empty() && false {",
        "a_fill_that_does_not_read_back_leaves_an_existing_output_as_it_was",
    ),
    Mutation(
        "cli: print a document's control characters to the terminal",
        "src/cli.rs",
        "    let acted_on = |c: char| c.is_control() && c != '\\n' && c != '\\t';",
        "    let acted_on = |_: char| false;",
        "plain_output_carries_no_escape_from_a_document_string",
    ),
    Mutation(
        "cli: take --help or --json for the option when it is another option's value",
        "src/cli/args.rs",
        "        if takes_value(word) {\n            words.next();\n        }\n",
        "        let _ = &takes_value;\n",
        "a_help_or_json_word_that_is_an_options_value_is_the_value",
    ),
    Mutation(
        "cli: read standard input for a `-` after the separator",
        "src/cli/args.rs",
        '    if after_separator && word == "-" {',
        '    if after_separator && word == "-" && false {',
        "a_dash_after_the_separator_is_a_file_and_not_standard_input",
    ),
    Mutation(
        "cli compress: take one name for the preview and the output",
        "src/cli/compress.rs",
        "            .is_some_and(|output| lexically_same(output, preview))",
        "            .is_some_and(|output| false && lexically_same(output, preview))",
        "an_output_that_names_another_file_of_the_line_is_refused",
    ),
    Mutation(
        "cli compress: replace the preview under another name",
        "src/cli/compress.rs",
        "                .is_some_and(|output| save::same_file(output, preview))",
        "                .is_some_and(|output| false && save::same_file(output, preview))",
        "an_output_that_names_another_file_of_the_line_is_refused",
    ),
    Mutation(
        "cli edit: take the plan file's name for the output",
        "src/cli/edit.rs",
        "        && lexically_same(Path::new(&command.request), &command.output)",
        "        && false\n        && lexically_same(Path::new(&command.request), &command.output)",
        "an_output_that_names_another_file_of_the_line_is_refused",
    ),
    Mutation(
        "cli edit: replace the plan file under another name",
        "src/cli/edit.rs",
        'if self.request != "-" && crate::save::same_file(',
        'if self.request != "-" && false && crate::save::same_file(',
        "an_output_that_names_another_file_of_the_line_is_refused",
    ),
    Mutation(
        "cli text: let --force write through a link or onto a directory",
        "src/cli/text.rs",
        "            check_target(std::slice::from_ref(&self.input), output, self.force)?;\n",
        "",
        "text_does_not_write_through_a_link_or_onto_a_directory",
    ),
    # `tpdf info` and `tpdf text`. What needs a worker --- the locked answer
    # reaching `verify` and `info` as `locked`, the password crossing to the
    # worker --- is `tests/cli.rs`, proved by hand and listed in `BUILD.md`.
    Mutation(
        "cli: accept page 0 in --pages",
        "src/cli/text.rs",
        "            Ok(n) if n >= 1 => Ok(n),",
        "            Ok(n) => Ok(n),",
        "a_page_list_is_read_as_the_palette_reads_one",
    ),
    Mutation(
        "cli: read a backwards page range as nothing",
        "src/cli/text.rs",
        "                if from > to {",
        "                if false {",
        "a_page_list_is_read_as_the_palette_reads_one",
    ),
    Mutation(
        "cli: read a page past the end",
        "src/cli/text.rs",
        "        Some(pages) => match pages.iter().find(|page| **page > count) {",
        "        Some(pages) => match pages.iter().find(|_| false) {",
        "a_page_past_the_end_is_refused_by_number",
    ),
    Mutation(
        "cli: accept a text -o that names the document",
        "src/cli/text.rs",
        "        if lexically_same(&input, out) {",
        "        if false {",
        "every_malformed_text_or_info_line_is_refused_with_its_reason",
    ),
    Mutation(
        "cli: replace an existing text output without --force",
        "src/cli/text.rs",
        # Re-aimed 2026-10-05. `text` had a check of its own before the shared
        # one, and since the shared one arrived removing it changed nothing:
        # the release's own run reported this entry as not caught.
        "            check_target(std::slice::from_ref(&self.input), output, self.force)?;",
        "            check_target(std::slice::from_ref(&self.input), output, true)?;",
        "text_refusals_that_need_no_worker_exit_before_one_is_asked_for",
    ),
    Mutation(
        "cli: read a password variable that is not set as no password",
        "src/cli/text.rs",
        "        Err(std::env::VarError::NotPresent) => Err(Failure::new(",
        "        Err(std::env::VarError::NotPresent) => Ok(None),\n        Err(std::env::VarError::NotUnicode(_)) if false => Err(Failure::new(",
        "a_password_variable_that_is_not_set_is_a_malformed_line",
    ),
    Mutation(
        "cli: call an empty page geometric",
        "src/cli/text.rs",
        "    let order = if text.codes.is_empty() {",
        "    let order = if false {",
        "a_page_says_which_order_it_was_read_in_and_whether_its_fonts_say_what_they_mean",
    ),
    Mutation(
        "cli: call a page whose fonts were not all examined stated",
        "src/cli/text.rs",
        "        Some(m) if m.certain() => Encoding::Stated,",
        "        Some(_) => Encoding::Stated,",
        "a_page_says_which_order_it_was_read_in_and_whether_its_fonts_say_what_they_mean",
    ),
    Mutation(
        "cli: keep PDFium's line breaks in a line",
        "src/cli/text.rs",
        "                .filter(|c| *c != '\\r' && *c != '\\n')",
        "                .filter(|_| true)",
        "a_line_break_pdfium_put_inside_a_line_is_not_kept",
    ),
    Mutation(
        "cli: forget the form feed after a page",
        "src/cli/text.rs",
        "        out.push('\\u{000C}');",
        "",
        "plain_text_ends_every_page_with_a_form_feed",
    ),
    Mutation(
        "cli: fail info on a merely locked document",
        "src/cli/info.rs",
        "            ErrorKind::Locked => Exit::Ok,",
        "            ErrorKind::Locked => Exit::Refused,",
        "a_locked_document_is_reported_by_info_and_exits_0",
    ),
    Mutation(
        "cli: miss an XFA form",
        "src/cli/info.rs",
        "            xfa: why == crate::forms::XFA_REFUSAL,",
        "            xfa: false,",
        "a_form_is_counted_by_field_and_an_xfa_refusal_is_named",
    ),
    Mutation(
        "cli: count widgets as fields",
        "src/cli/info.rs",
        "                .map(|w| w.object)",
        "                .map(|w| w.widget)",
        "a_form_is_counted_by_field_and_an_xfa_refusal_is_named",
    ),
    Mutation(
        "cli: report page sizes unrounded",
        "src/cli/info.rs",
        "    (f64::from(value) * 100.0).round() / 100.0",
        "    f64::from(value)",
        "page_sizes_are_counted_by_displayed_size_first_seen_first",
    ),
    # `tpdf fields` and `tpdf fill` (2026-09-27). The unit tests are
    # `cli::form_tests`; `tests/cli.rs`'s halves are run by hand (BUILD.md).
    Mutation(
        "cli: fill accepts an output that names the input",
        "src/cli/fill.rs",
        "    if lexically_same(&input, &output) {",
        "    if false {",
        "fields_and_fill_lines_parse_and_every_malformed_one_is_refused",
    ),
    Mutation(
        "cli: fill accepts an output that names the answers file",
        "src/cli/fill.rs",
        "        if lexically_same(path, &output) {",
        "        if false {",
        "fields_and_fill_lines_parse_and_every_malformed_one_is_refused",
    ),
    Mutation(
        "cli: fill guesses among options that share an export value",
        "src/cli/fill.rs",
        "        [one] => Ok(Some(*one)),",
        "        [one, ..] => Ok(Some(*one)),",
        "each_problem_is_refused_on_its_own_and_all_of_them_are_listed_together",
    ),
    Mutation(
        "cli: fill takes custom text in a dropdown that takes none",
        "src/cli/fill.rs",
        "                    None if *editable => Ok(Value::Text(wanted.clone())),",
        "                    None => Ok(Value::Text(wanted.clone())),",
        "each_problem_is_refused_on_its_own_and_all_of_them_are_listed_together",
    ),
    Mutation(
        "cli: fill leaves a list's repeated export value in",
        "src/cli/fill.rs",
        "                at.dedup();",
        "",
        "every_kind_of_answer_resolves_to_what_the_writer_takes",
    ),
    Mutation(
        "cli: fill resolves a repeated name to its last answer",
        "src/cli/fill.rs",
        "        if answers.0[..at].iter().any(|(earlier, _)| earlier == name) {",
        "        if false {",
        "each_problem_is_refused_on_its_own_and_all_of_them_are_listed_together",
    ),
    Mutation(
        "cli: fill picks the first of two fields that share a name",
        "src/cli/fill.rs",
        "            [one] => *one,",
        "            [one, ..] => *one,",
        "each_problem_is_refused_on_its_own_and_all_of_them_are_listed_together",
    ),
    Mutation(
        "cli: fill reports a field that cannot be filled as a wrong type",
        "src/cli/fill.rs",
        "        if let Some(reason) = group.reason() {",
        "        if let Some(reason) = group.reason().filter(|_| false) {",
        "every_reason_a_field_is_not_editable_is_named_and_refused",
    ),
    Mutation(
        "cli: fill skips the application's answer checks",
        "src/cli/fill.rs",
        "            .find_map(|w| forms::check(w, &value).err())",
        "            .find_map(|w| forms::check(w, &value).err().filter(|_| false))",
        "each_problem_is_refused_on_its_own_and_all_of_them_are_listed_together",
    ),
    Mutation(
        "cli: fill checks only a field's first widget",
        "src/cli/fill.rs",
        "            .find_map(|w| forms::check(w, &value).err())",
        "            .take(1).find_map(|w| forms::check(w, &value).err())",
        "an_answer_is_checked_against_every_widget_of_its_field",
    ),
    Mutation(
        "cli: fill rewrites a certified document",
        "src/cli/fill.rs",
        "        .filter(|s| s.signed || s.certification > 0)",
        "        .filter(|s| s.signed)",
        "a_signed_or_certified_document_or_one_whose_signatures_were_not_all_read_is_refused",
    ),
    Mutation(
        "cli: fill rewrites a document whose signatures were not all read",
        "src/cli/fill.rs",
        "    if limits.locked || limits.unreadable > 0 || limits.signatures_dropped > 0 {",
        "    if false {",
        "a_signed_or_certified_document_or_one_whose_signatures_were_not_all_read_is_refused",
    ),
    Mutation(
        "cli: fill never compares what was read back",
        "src/cli/fill.rs",
        "            if &got != want {",
        "            if false {",
        "a_written_form_reads_back_as_asked_and_a_difference_is_named",
    ),
    Mutation(
        "cli: fill ignores a field it was not asked to change",
        "src/cli/fill.rs",
        "        if asked.iter().any(|a| a.name == group.name) || !unique(&then, group.name) {",
        "        if true {",
        "a_written_form_reads_back_as_asked_and_a_difference_is_named",
    ),
    Mutation(
        "cli: fields lists every widget as a field",
        "src/cli/fields.rs",
        "        match out.iter_mut().find(|g| g.object == widget.object) {",
        "        match out.iter_mut().find(|_| false) {",
        "fields_are_grouped_by_field_with_their_answers_in_the_form_fill_takes",
    ),
    Mutation(
        "cli: fields never says which option is chosen",
        "src/cli/fields.rs",
        "                selected: chosen.contains(&i),",
        "                selected: false,",
        "fields_are_grouped_by_field_with_their_answers_in_the_form_fill_takes",
    ),
    Mutation(
        "cli: fields calls a comb field something else",
        "src/cli/fields.rs",
        "        forms::COMB => NotEditable::Comb,",
        "        forms::COMB => NotEditable::Other,",
        "every_reason_a_field_is_not_editable_is_named_and_refused",
    ),
    Mutation(
        "cli: forms reads the comb flag from the wrong bit",
        "src/forms.rs",
        "            } else if text && flags & (1 << 24) != 0 {",
        "            } else if text && flags & (1 << 23) != 0 {",
        "every_reason_a_field_is_not_editable_is_named_and_refused",
    ),
    Mutation(
        # Fixed 2026-09-27 on both sides: a first character with no box was lost.
        "reading: drop a first character that has no box",
        "src/reading.rs",
        "    if let Some(leading) = trailing.remove(&-1) {",
        "    if let Some(leading) = None::<Vec<usize>> {",
        "every_order_is_a_permutation_of_the_page",
    ),
    Mutation(
        "reading: ignore the document's tags",
        "src/reading.rs",
        "    if let Some(tagged) = usable_runs(&view) {",
        "    if let Some(tagged) = usable_runs(&view).filter(|_| false) {",
        "tags_that_cover_the_page_decide_its_order_and_the_geometry_does_not",
    ),
    Mutation(
        "reading: use tags that leave a visible character unclaimed",
        "src/reading.rs",
        "        if is_visible(view.codes[index]) && placed(char_quad(view, index)) {",
        "        if false {",
        "tags_that_leave_a_visible_character_unclaimed_are_not_used",
    ),
    Mutation(
        "reading: never cut between columns",
        "src/reading.rs",
        "    if columns.len() > 1 {",
        "    if false {",
        "interleaved_columns_read_one_column_after_the_other",
    ),
    Mutation(
        "reading: ignore the page's rotation",
        "src/reading.rs",
        "    let axes = axes_for(view.quarter_turns);",
        "    let axes = axes_for(0);",
        "a_turned_page_reads_as_the_upright_one_does",
    ),
    Mutation(
        "reading: give a comma a band of its own",
        "src/reading.rs",
        "    if shorter < ha.max(hb) * SHORT_MARK {",
        "    if false {",
        "a_mark_a_sliver_and_a_comma_stay_on_their_line",
    ),
    Mutation(
        "reading: give a combining mark a band of its own",
        "src/reading.rs",
        "        let mark = last >= 0 && combining(view.codes[index]);",
        "        let mark = false;",
        "a_mark_a_sliver_and_a_comma_stay_on_their_line",
    ),
    Mutation(
        "reading: compute on the widened f32 rather than the viewer's number",
        "src/reading.rs",
        "            boxes: text.boxes.iter().copied().map(webview_number).collect(),",
        "            boxes: text.boxes.iter().copied().map(f64::from).collect(),",
        "a_gap_is_measured_in_the_numbers_the_viewer_holds",
    ),
    Mutation(
        "reading: order a turned run in the page's direction",
        "src/reading.rs",
        "    if view.char_turns.is_empty() {\n        return aligned_fragments(view, axes, gap);",
        "    if true {\n        return aligned_fragments(view, axes, gap);",
        "text_turned_a_quarter_is_read_in_its_own_direction",
    ),
    Mutation(
        "reading: a restated rule drifts from reading.ts",
        "src/reading.rs",
        "const SLIVER_PT: f64 = 0.1;",
        "const SLIVER_PT: f64 = 0.01;",
        "every_json_shape_and_the_wording_match_their_committed_samples",
    ),
    # `tpdf redact`, added 2026-09-27 in the same edit as the command. The
    # regions restate the viewer's route from a hit to a rectangle and are held
    # to it by `regions.json` as well as by the unit test named here.
    Mutation(
        'cli: regions joins characters from different lines',
        'src/cli/regions.rs',
        '    shorter > 0.0 && overlap / shorter > 0.5',
        '    shorter > 0.0',
        'characters_on_one_line_join_one_run_and_a_new_line_starts_another',
    ),
    # The short-mark rule, added 2026-10-11: a hit on `4,250.00` was three
    # regions, each verified by what it took, and no file is free of a `4`.
    Mutation(
        'redact: explain a found piece of any length as too short',
        'src/redact.rs',
        '        .filter(|needle| needle.chars().count() <= SHORT_NEEDLE)',
        '        .filter(|_| true)',
        'a_found_piece_of_one_or_two_characters_is_explained_and_a_longer_one_is_not',
    ),
    Mutation(
        'redact: call a short piece on a marked page a removal that did not take',
        'src/redact.rs',
        '        .all(|needle| needle.chars().count() <= SHORT_NEEDLE)',
        '        .all(|_| false)',
        'only_short_pieces_on_a_marked_page_are_not_called_a_failed_removal',
    ),
    Mutation(
        'cli: regions gives a comma a run of its own',
        'src/cli/regions.rs',
        '    if overlap > 0.0 && shorter < ha.max(hb) * SHORT_MARK {',
        '    if false {',
        'a_comma_joins_the_run_it_touches_so_an_amount_is_one_run',
    ),
    Mutation(
        'cli: regions joins a short box that does not touch the run',
        'src/cli/regions.rs',
        '    if overlap > 0.0 && shorter < ha.max(hb) * SHORT_MARK {',
        '    if shorter < ha.max(hb) * SHORT_MARK {',
        'a_short_box_that_does_not_touch_and_a_letter_half_off_the_line_stay_apart',
    ),
    Mutation(
        'cli: regions joins any shorter box that touches the run',
        'src/cli/regions.rs',
        '    if overlap > 0.0 && shorter < ha.max(hb) * SHORT_MARK {',
        '    if overlap > 0.0 && shorter < ha.max(hb) {',
        'a_short_box_that_does_not_touch_and_a_letter_half_off_the_line_stay_apart',
    ),
    Mutation(
        'cli: regions keeps a character with width and no height',
        'src/cli/regions.rs',
        '    q.right > q.left && q.bottom > q.top',
        '    q.right > q.left || q.bottom > q.top',
        'a_character_with_no_extent_on_either_axis_is_not_placed',
    ),
    Mutation(
        'cli: regions joins a turned character to an upright run',
        'src/cli/regions.rs',
        '            if was == turns && on_same_line(*current, quad, turns % 2 == 1) {',
        '            if on_same_line(*current, quad, turns % 2 == 1) {',
        'a_turned_character_does_not_join_an_upright_run',
    ),
    Mutation(
        'cli: regions ignores the direction lines run in',
        'src/cli/regions.rs',
        '            if was == turns && on_same_line(*current, quad, turns % 2 == 1) {',
        '            if was == turns && on_same_line(*current, quad, false) {',
        'on_a_sideways_page_lines_are_told_apart_across',
    ),
    Mutation(
        'cli: regions keeps a sliver',
        'src/cli/regions.rs',
        '        if right - left < MIN_REDACTION_SIDE {',
        '        if false {',
        'a_run_thinner_than_half_a_point_either_way_is_not_a_region',
    ),
    Mutation(
        "cli: regions trusts the order of a run's sides",
        'src/cli/regions.rs',
        '        let (left, right) = (a.min(*c), a.max(*c));',
        '        let (left, right) = (*a, *c);',
        'sides_are_ordered_and_a_partial_run_is_dropped',
    ),
    Mutation(
        'cli: regions narrows a region by a cast, not as the IPC does',
        'src/cli/regions.rs',
        '    format!("{value}").parse().unwrap_or(f32::NAN)',
        '    value as f32',
        'a_region_is_narrowed_through_its_decimal_not_by_a_cast',
    ),
    Mutation(
        'cli: regions ends a hit over a break at its own end',
        'src/cli/regions.rs',
        '                    from: hit.start,\n                    to: None,',
        '                    from: hit.start,\n                    to: Some(hit.end),',
        'a_hit_over_a_page_break_is_one_half_on_each_page',
    ),
    Mutation(
        "cli: redact carries a page's tail to a page that is not the next",
        'src/cli/redact.rs',
        '        let adjacent = previous.as_ref().is_some_and(|(was, _)| *was + 1 == at);',
        '        let adjacent = previous.is_some();',
        'a_hit_over_a_page_break_is_found_between_neighbours_only',
    ),
    Mutation(
        'cli: redact forgets the page a hit over a break began on',
        'src/cli/redact.rs',
        '                found.texts.entry(was).or_insert(text);',
        '                let _ = (was, text);',
        'a_hit_over_a_page_break_is_found_between_neighbours_only',
    ),
    Mutation(
        'cli: redact skips a page that could not be read',
        'src/cli/redact.rs',
        '        let text = text_of(at).map_err(|why| format!("page {page} could not be read: {why}"))?;',
        '        let text = text_of(at).unwrap_or_default();',
        'a_page_that_cannot_be_read_stops_the_walk',
    ),
    Mutation(
        "cli: redact counts every query's hits as the first's",
        'src/cli/redact.rs',
        '            found.counts[index] += answer.matches.len();',
        '            found.counts[0] += answer.matches.len();',
        'each_query_is_counted_on_its_own_and_case_is_folded',
    ),
    Mutation(
        'cli: redact names the wrong query in a read-back reason',
        'src/cli/redact.rs',
        '            let query = queries.get(*by).map_or("", |(_, query, _)| query.as_str());',
        '            let query = queries.first().map_or("", |(_, query, _)| query.as_str());',
        'a_hit_still_in_the_written_file_is_a_reason_naming_its_page_and_query',
    ),
    Mutation(
        'cli: redact lets a signed document through',
        'src/cli/redact.rs',
        '    if invalidate {\n        return None;',
        '    if true {\n        return None;',
        'a_signed_document_is_refused_unless_invalidating_is_asked_for',
    ),
    Mutation(
        'cli: redact accepts a region on page 0',
        'src/cli/redact.rs',
        '        if spec.page == 0 {',
        '        if false {',
        'a_regions_file_that_is_not_one_is_refused_with_where',
    ),
    Mutation(
        'cli: redact accepts a key a regions file does not have',
        'src/cli/redact.rs',
        '#[serde(deny_unknown_fields)]\npub struct RegionSpec {',
        'pub struct RegionSpec {',
        'a_regions_file_that_is_not_one_is_refused_with_where',
    ),
    Mutation(
        'cli: redact needs -o for a dry run',
        'src/cli/redact.rs',
        '        None if !command.dry_run => {',
        '        None if true => {',
        'a_dry_run_needs_no_output_and_a_real_run_does',
    ),
    Mutation(
        'cli: redact runs a query that can match nothing',
        'src/cli/redact.rs',
        '    if compiled.matches_nothing() {',
        '    if false {',
        'a_query_that_can_match_nothing_is_refused_rather_than_run',
    ),
    Mutation(
        "cli: redact's barren query is not reported as one",
        'src/search.rs',
        '        self.barren\n',
        '        false\n',
        'a_query_that_can_match_nothing_is_refused_rather_than_run',
    ),
    Mutation(
        'cli: redact runs with nothing to remove',
        'src/cli/redact.rs',
        '    if command.texts.is_empty() && command.patterns.is_empty() && command.regions.is_empty() {',
        '    if false {',
        'nothing_to_remove_is_a_malformed_line',
    ),
    Mutation(
        'cli: redact calls a copy with reasons verified',
        'src/cli/redact.rs',
        '    let verified = why.is_empty();\n    (\n        verified,',
        '    let verified = true;\n    (\n        verified,',
        'a_copy_is_verified_and_exits_0_only_when_no_list_has_a_reason',
    ),
    Mutation(
        'cli: redact drops the reasons its own checks found',
        'src/cli/redact.rs',
        '    why.extend(unmarked);',
        '    let _ = unmarked;',
        'a_copy_is_verified_and_exits_0_only_when_no_list_has_a_reason',
    ),
    Mutation(
        'cli: redact calls a match with no position marked',
        'src/cli/redact.rs',
        '                        regions::regions_on(text, half.page, &[*half]).is_empty()',
        '                        false',
        'a_match_with_no_position_on_the_page_is_a_reason_and_a_placed_one_is_not',
    ),
    Mutation(
        'cli: redact words an unverified copy as verified',
        'src/words.rs',
        '    let verdict = if verified {',
        '    let verdict = if true {',
        'every_json_shape_and_the_wording_match_their_committed_samples',
    ),
    Mutation(
        "cli tool link: replace a link that is not tpdf's",
        "src/clitool.rs",
        "    if !ours(&target) {",
        "    if false {",
        "only_tpdfs_own_link_is_created_repointed_or_removed",
        only_on="macos",
    ),
    Mutation(
        # Write JSON as UTF-8. A PowerShell script reading it decodes through
        # the console's code page and gets `PrÃ¼fung` for `Prüfung`.
        "cli json: leave characters outside ASCII unescaped",
        "src/cli.rs",
        "        if c.is_ascii() {",
        "        if true {",
        "json_output_is_ascii_and_means_the_same_text",
    ),
    Mutation(
        "cli tool link: call any file of that name tpdf's",
        "src/clitool.rs",
        '            dir.ends_with("Contents/MacOS")',
        "            true",
        "only_tpdfs_own_link_is_created_repointed_or_removed",
        only_on="macos",
    ),
    Mutation(
        # The release audit's case: `/tmp/x/Contents/MacOS/tpdf-cli`, laid out
        # like a bundle in a folder that is not one.
        "cli tool link: call a bundle-shaped folder a bundle",
        "src/clitool.rs",
        '                    .is_some_and(|ext| ext == "app")',
        "                    .is_none_or(|_| true)",
        "only_tpdfs_own_link_is_created_repointed_or_removed",
        only_on="macos",
    ),
    Mutation(
        "cli tool link: report success whatever is there afterwards",
        "src/clitool.rs",
        '        (true, Step::Present) => said!(',
        '        (true, _) => said!(',
        "the_reader_is_told_what_is_there_afterwards_not_what_was_attempted",
    ),
]

# --- asking a timestamp authority when signing (Phase 6 step 3, increment B) ---
#
# `tsa.rs`, `sign_cms.rs` and `cli/sign.rs`, 2026-09-28. Each removes one check
# a token must pass before it is written, or one bound on the request, and
# names the test built so only that check can fail it: the fake authority in
# `tsa::tests` mints over what the request carried, one fault at a time.
ROWS += [
    Mutation(
        # Any status: an authority that said no, with a token beside it, is
        # read as having said yes.
        "tsa: take a token under a status that declined",
        "src/tsa.rs",
        "    if response.status.status > 1 {",
        "    if false {",
        "a_declining_status_is_refused_even_with_a_token_beside_it",
    ),
    Mutation(
        "tsa: refuse a grant with modifications",
        "src/tsa.rs",
        "    if response.status.status > 1 {",
        "    if response.status.status > 0 {",
        "granted_with_modifications_is_granted",
    ),
    Mutation(
        # Use the token whatever increment A's reader calls it.
        "tsa: use a token whose own signature fails",
        "src/tsa.rs",
        "    if verdict.verdict != Verdict::Intact {",
        "    if false {",
        "a_token_whose_own_signature_fails_is_refused",
    ),
    Mutation(
        "tsa: accept a weak token for a new signature",
        "src/tsa.rs",
        "    if verdict.verdict != Verdict::Intact {",
        "    if !matches!(verdict.verdict, Verdict::Intact | Verdict::Weak) {",
        "a_token_whose_own_signature_rests_on_sha1_is_refused_for_a_new_signature",
    ),
    Mutation(
        # The verdict accepts any hash; only this says it is the one asked.
        "tsa: never compare the imprint with the one asked for",
        "src/tsa.rs",
        "    if imprint.hashed_message.as_bytes() != asked {",
        "    if false {",
        "a_token_under_another_hash_than_the_one_asked_for_is_refused",
    ),
    Mutation(
        # A replayed answer to an earlier request over the same signature.
        "tsa: never compare the nonce",
        "src/tsa.rs",
        "    if nonce_of(&statement) != Some(unpadded(nonce)) {",
        "    if false {",
        "a_token_carrying_another_nonce_or_none_is_refused",
    ),
    Mutation(
        "tsa: accept a token that carries no nonce",
        "src/tsa.rs",
        "    if nonce_of(&statement) != Some(unpadded(nonce)) {",
        "    if nonce_of(&statement).is_some_and(|n| n != unpadded(nonce)) {",
        "a_token_carrying_another_nonce_or_none_is_refused",
    ),
    Mutation(
        "tsa: send the same nonce every time",
        "src/tsa.rs",
        '        .fill(&mut nonce)\n        .map_err(|_| Refusal::Unreachable(said!("the system\'s random source failed")))?;',
        '        .fill(&mut [0u8; 16])\n        .map_err(|_| Refusal::Unreachable(said!("the system\'s random source failed")))?;',
        "every_request_carries_a_nonce_of_its_own",
    ),
    Mutation(
        # Ask for a timestamp of something other than the signature's value.
        "tsa: ask for the imprint of other bytes",
        "src/tsa.rs",
        "    let digest: [u8; 32] = sha2_10::Sha256::digest(value).into();",
        "    let digest: [u8; 32] = sha2_10::Sha256::digest(&value[1..]).into();",
        "a_sound_answer_to_this_request_is_accepted",
    ),
    Mutation(
        "tsa: read an answer of any length",
        "src/tsa.rs",
        "        if answer.len() + chunk.len() > limits.body {",
        "        if false {",
        "an_answer_longer_than_the_bound_is_refused_whether_or_not_it_says_so",
    ),
    Mutation(
        # A signing that waits for a silent server for as long as it stays up.
        "tsa: wait for a server with no limit",
        "src/tsa.rs",
        "        .timeout(limits.total)\n",
        "\n",
        "a_server_that_never_answers_is_given_up_on_within_the_limit",
    ),
    Mutation(
        "tsa: follow a redirect to an address nobody chose",
        "src/tsa.rs",
        "        .redirect(reqwest::redirect::Policy::none())",
        "        .redirect(reqwest::redirect::Policy::limited(3))",
        "an_http_error_or_a_redirect_is_refused_and_the_redirect_is_not_followed",
    ),
    Mutation(
        "tsa: ask an authority over any scheme",
        "src/tsa.rs",
        '    if !matches!(url.scheme(), "http" | "https") {',
        "    if false {",
        "only_http_and_https_addresses_with_a_host_and_no_credentials_are_asked",
    ),
    Mutation(
        "tsa: ask an address with a password in it",
        "src/tsa.rs",
        "    if !url.username().is_empty() || url.password().is_some() {",
        "    if false {",
        "only_http_and_https_addresses_with_a_host_and_no_credentials_are_asked",
    ),
    Mutation(
        # The failure this increment exists to forbid: a reader who asked for
        # a timestamp handed a signature without one, in silence.
        "tsa: sign without a timestamp when the request fails",
        "src/tsa.rs",
        "    let token = ask(url, &value)?;",
        "    let Ok(token) = ask(url, &value) else {\n        return Ok(None);\n    };",
        "a_timestamp_asked_for_and_not_had_is_an_error_never_a_signature_without_one",
    ),
    Mutation(
        "sign: write a timestamp past the reserved span",
        "src/sign_cms.rs",
        "        if blob.len() > RESERVED {",
        "        if false {",
        "a_timestamp_past_the_reserved_span_is_refused",
    ),
    Mutation(
        # The token goes where the key's signature covers it: the signature
        # the OS made is no longer over its own signed attributes.
        "sign: put the token among the signed attributes",
        "src/sign_cms.rs",
        "        signer.unsigned_attrs =\n",
        "        signer.signed_attrs =\n",
        "stamping_changes_nothing_the_key_signed",
    ),
    Mutation(
        # The last check: the token as a reader of the written file finds it.
        "sign: never check the timestamp in the written bytes",
        "src/sign_cms.rs",
        "            if token.verdict != crate::integrity::Verdict::Intact {",
        "            if false {",
        "a_timestamp_that_does_not_check_out_in_the_written_bytes_is_not_written",
    ),
    Mutation(
        # `--timestamp` parsed and dropped: a signing the reader asked to be
        # timestamped goes out without one.
        "cli: drop --timestamp",
        "src/cli/sign.rs",
        "        timestamp,\n        long_term,\n",
        "        timestamp: None,\n        long_term,\n",
        "a_timestamp_authority_is_named_or_given_and_judged_before_anything_runs",
    ),
]

# --- revocation, and the signer judged at the attested time -----------------
#
# `revocation.rs`, `docinfo.rs`, `trust.rs` and `cli/verify.rs`, 2026-09-28
# (Phase 6 step 3, increment C1). Each removes one check a revocation answer or
# the attested moment rests on, and names the test built so only that check
# can fail it --- the responses, lists and certificates come from
# `integrity/test_tsa.rs`, one fault apiece.
ROWS += [
    Mutation(
        "revocation: accept an OCSP response whose signature fails",
        "src/revocation.rs",
        "                (true, Ok(true)) => return Ok(()),",
        "                (true, Ok(_)) => return Ok(()),",
        "a_response_whose_signature_fails_is_not_believed",
    ),
    Mutation(
        "revocation: accept any responder as authorised",
        "src/revocation.rs",
        "            let authorised = responder == issuer || delegated(responder, issuer, produced);",
        "            let authorised = true || delegated(responder, issuer, produced);",
        "a_responder_without_the_ocsp_signing_purpose_is_not_authorised",
    ),
    Mutation(
        "revocation: a delegated responder needs no OCSP-signing purpose",
        "src/revocation.rs",
        "    purpose\n        && (from..=until).contains(&at)",
        "    (purpose || true)\n        && (from..=until).contains(&at)",
        "a_responder_without_the_ocsp_signing_purpose_is_not_authorised",
    ),
    Mutation(
        "revocation: a delegated responder need not be in force",
        "src/revocation.rs",
        "    purpose\n        && (from..=until).contains(&at)",
        "    purpose\n        && ((from..=until).contains(&at) || true)",
        "a_response_from_a_responder_out_of_its_dates_is_not_authorised",
    ),
    Mutation(
        "revocation: a delegated responder need not be issued by the issuer",
        "src/revocation.rs",
        "            crate::integrity::signed_by(issuer, &responder.signature_algorithm, &tbs, value)\n                == Ok(true)",
        "            crate::integrity::signed_by(issuer, &responder.signature_algorithm, &tbs, value)\n                != Ok(false) || true",
        "a_responder_another_authority_delegated_is_not_authorised",
    ),
    Mutation(
        "revocation: match a CertID without its serial",
        "src/revocation.rs",
        "        if &id.serial_number != serial {\n            continue;\n        }",
        "",
        "a_response_about_another_certificate_says_nothing_about_this_one",
    ),
    Mutation(
        "revocation: match a CertID without its issuer name",
        "src/revocation.rs",
        "        if hash.digest(&[issuer_name]) != id.issuer_name_hash.as_bytes() {",
        "        if false {",
        "another_authoritys_response_about_the_same_serial_is_not_data_about_this_one",
    ),
    Mutation(
        "revocation: match a CertID without its issuer key",
        "src/revocation.rs",
        "        if hash.digest(&[key_bits(issuer)]) != id.issuer_key_hash.as_bytes() {",
        "        if false {",
        "a_response_under_the_issuers_name_and_another_key_is_not_about_this_certificate",
    ),
    Mutation(
        "revocation: find an issuer by its name alone",
        "src/revocation.rs",
        "            && crate::integrity::signed_by(candidate, &subject.signature_algorithm, &signed, value)\n                == Ok(true)",
        "            && (crate::integrity::signed_by(candidate, &subject.signature_algorithm, &signed, value)\n                == Ok(true) || true)",
        "an_issuer_is_found_by_its_key_and_not_by_its_name_alone",
    ),
    Mutation(
        "revocation: accept a list whose signature fails",
        "src/revocation.rs",
        "            Ok(true) => Ok(()),\n            Ok(false) => Err(Gap::Signature),",
        "            Ok(_) => Ok(()),",
        "a_list_whose_signature_fails_is_not_believed",
    ),
    Mutation(
        "revocation: read another issuer's list as this certificate's",
        "src/revocation.rs",
        "    if tbs.issuer.to_der().ok().as_deref() != Some(issuer_name) {",
        "    if false {",
        "a_list_naming_another_issuer_says_nothing_about_this_certificate",
    ),
    Mutation(
        "revocation: a list's scope covers everything",
        "src/revocation.rs",
        "    match in_scope(subject, tbs.crl_extensions.as_deref()) {",
        "    match Ok::<bool, Gap>(true) {",
        "a_list_whose_scope_leaves_the_certificate_out_says_nothing_about_it",
    ),
    Mutation(
        "revocation: any key may sign a list",
        "src/revocation.rs",
        "        if !signs_lists(issuer) {",
        "        if false {",
        "a_list_signed_by_a_key_not_permitted_to_sign_lists_is_not_authorised",
    ),
    Mutation(
        "revocation: a list names no serial",
        "src/revocation.rs",
        "        .find(|entry| &entry.serial_number == serial);",
        "        .find(|_| false);",
        "a_list_naming_the_certificate_is_revoked_with_its_reason",
    ),
    Mutation(
        "revocation: a list's first entry is this certificate",
        "src/revocation.rs",
        "        .find(|entry| &entry.serial_number == serial);",
        "        .find(|_| true);",
        "a_list_listing_every_certificate_but_this_one_is_not_a_revocation_of_it",
    ),
    Mutation(
        "revocation: every good is fresh",
        "src/revocation.rs",
        "        if !s.next_update.is_some_and(|next| next > moment.at) {",
        "        if false {",
        "a_good_that_does_not_reach_the_moment_is_stale",
    ),
    Mutation(
        "revocation: a nextUpdate at the moment is fresh",
        "src/revocation.rs",
        "        if !s.next_update.is_some_and(|next| next > moment.at) {",
        "        if !s.next_update.is_some_and(|next| next >= moment.at) {",
        "a_good_that_does_not_reach_the_moment_is_stale",
    ),
    Mutation(
        "revocation: a revocation after any moment is after the signature",
        "src/revocation.rs",
        "            after_moment: moment.basis == Basis::Attested && at > moment.at,",
        "            after_moment: at > moment.at,",
        "a_revocation_after_the_attested_moment_does_not_undo_the_signature",
    ),
    Mutation(
        "revocation: a revocation at the moment is after it",
        "src/revocation.rs",
        "            after_moment: moment.basis == Basis::Attested && at > moment.at,",
        "            after_moment: moment.basis == Basis::Attested && at >= moment.at,",
        "a_revocation_after_the_attested_moment_does_not_undo_the_signature",
    ),
    Mutation(
        "revocation: name the latest revocation rather than the earliest",
        "src/revocation.rs",
        "        .min_by_key(|(_, at, _)| *at);",
        "        .max_by_key(|(_, at, _)| *at);",
        "a_revocation_outweighs_a_good_and_the_earliest_revocation_is_the_one_named",
    ),
    Mutation(
        "revocation: judge the earliest good for freshness",
        "src/revocation.rs",
        "        .max_by_key(|s| s.this_update);",
        "        .min_by_key(|s| s.this_update);",
        "the_latest_good_is_the_one_judged_for_freshness",
    ),
    Mutation(
        "revocation: none stands beside data that would not read",
        "src/revocation.rs",
        "    if matches!(answer.standing, Status::Good | Status::None) {",
        "    if matches!(answer.standing, Status::Good) {",
        "data_that_would_not_read_turns_none_into_not_checked",
    ),
    Mutation(
        "revocation: a good stands beside data dropped at a bound",
        "src/revocation.rs",
        "        if dropped > 0 {\n            return Revocation::unchecked(moment, Gap::Bound);\n        }",
        "",
        "data_left_unread_at_a_bound_turns_good_and_none_into_not_checked_and_leaves_revoked",
    ),
    Mutation(
        "revocation: a good issued after expiry counts",
        "src/revocation.rs",
        "        if s.this_update > until && !s.keeps_expired_since.is_some_and(|since| since <= until) {",
        "        if false {",
        "a_good_issued_after_the_certificate_expired_counts_only_with_an_archive_cutoff",
    ),
    Mutation(
        "revocation: dates from the future are dates",
        "src/revocation.rs",
        "                let incoherent = s.this_update > now.saturating_add(SKEW)",
        "                let incoherent = false && s.this_update > now.saturating_add(SKEW)",
        "a_response_issued_in_the_future_has_no_dates",
    ),
    Mutation(
        "revocation: an unknown critical response extension is ignored",
        "src/revocation.rs",
        "        if critical_unknown(data.response_extensions.as_deref(), &[OCSP_NONCE]) {",
        "        if false {",
        "a_critical_extension_tpdf_does_not_know_is_unsupported",
    ),
    Mutation(
        "revocation: a delta list is read as a full one",
        "src/revocation.rs",
        "        if critical_unknown(tbs.crl_extensions.as_deref(), &[ISSUING_DISTRIBUTION_POINT]) {",
        "        if false {",
        "a_delta_list_is_unsupported",
    ),
    Mutation(
        "revocation: check a response's signature without charging the budget",
        "src/revocation.rs",
        "            charge(budget, response.signed.len())?;",
        "",
        "the_budget_is_charged_before_a_signature_is_checked",
    ),
    Mutation(
        "revocation: no count bound on responses",
        "src/revocation.rs",
        "        } else if self.responses.len() >= MAX_RESPONSES {",
        "        } else if false {",
        "data_left_unread_at_a_bound_turns_good_and_none_into_not_checked_and_leaves_revoked",
    ),
    Mutation(
        "revocation: no size bound on a list",
        "src/revocation.rs",
        "        if der.len() > MAX_LIST_BYTES {",
        "        if false {",
        "material_refuses_what_is_over_its_size_bounds",
    ),
    Mutation(
        "revocation: no count bound on lists",
        "src/revocation.rs",
        "        } else if self.lists.len() >= MAX_LISTS {",
        "        } else if false {",
        "material_refuses_what_is_over_its_size_bounds",
    ),
    Mutation(
        "revocation: no size bound on a basic response",
        "src/revocation.rs",
        "    pub fn basic(&mut self, der: Vec<u8>) {\n        if der.len() > MAX_RESPONSE_BYTES {",
        "    pub fn basic(&mut self, der: Vec<u8>) {\n        if false {",
        # SURVIVED its first run, aimed at the test that reaches `basic` only
        # through `response`, whose own check had already refused.
        "a_basic_response_over_its_bound_is_refused_on_the_way_in_too",
    ),
    Mutation(
        "revocation: ignore the CMS crls set",
        "src/revocation.rs",
        "        if let Some(choices) = &signed.crls {",
        "        if let Some(choices) = None::<&cms::revocation::RevocationInfoChoices> {",
        "a_signatures_own_cms_carries_lists_responses_and_the_adobe_attribute",
    ),
    Mutation(
        "revocation: ignore the Adobe archival attribute",
        "src/revocation.rs",
        "        if let Some(attribute) = archival {",
        "        if let Some(attribute) = archival.filter(|_| false) {",
        "a_signatures_own_cms_carries_lists_responses_and_the_adobe_attribute",
    ),
    Mutation(
        "revocation: read any other revocation format as a response",
        "src/revocation.rs",
        "                        if other.other_format.oid.to_string() == \"1.3.6.1.5.5.7.16.2\" {",
        "                        if !other.other_format.oid.to_string().is_empty() {",
        "a_signatures_own_cms_carries_lists_responses_and_the_adobe_attribute",
    ),
    Mutation(
        "lt: read a filtered /DSS stream raw",
        "src/docinfo.rs",
        "    if stream.dict.has(b\"Filter\") {",
        "    if false {",
        "a_compressed_dss_stream_is_decoded_and_a_vri_past_its_bound_is_counted",
    ),
    Mutation(
        "lt: walk every /VRI entry",
        "src/docinfo.rs",
        "            if at >= crate::revocation::MAX_VRI {",
        "            if false {",
        "a_compressed_dss_stream_is_decoded_and_a_vri_past_its_bound_is_counted",
    ),
    Mutation(
        "lt: judge the signer now even with an attested moment",
        "src/docinfo.rs",
        "                crate::trust::of_blob_at(blob, extra, purpose, at, judging.anchors)",
        "                crate::trust::of_blob_with(blob, extra, purpose, judging.now, judging.anchors)",
        "a_b_lt_signature_is_judged_at_the_attested_time_with_the_documents_own_data",
    ),
    Mutation(
        "lt: an untrusted authority attests a moment",
        "src/docinfo.rs",
        "    if !intact || !trusted || revoked {",
        "    if !intact || revoked {",
        "only_an_intact_token_from_a_trusted_unrevoked_authority_attests_a_moment",
    ),
    Mutation(
        "lt: a weak or broken token attests a moment",
        "src/docinfo.rs",
        "    if !intact || !trusted || revoked {",
        "    if !trusted || revoked {",
        "only_an_intact_token_from_a_trusted_unrevoked_authority_attests_a_moment",
    ),
    Mutation(
        "lt: a revoked authority attests a moment",
        "src/docinfo.rs",
        "    if !intact || !trusted || revoked {",
        "    if !intact || !trusted {",
        "an_authority_the_document_shows_revoked_leaves_the_signer_judged_now",
    ),
    Mutation(
        "lt: a time outside the authority's dates is attested",
        "src/docinfo.rs",
        "    (matched && (from..=until).contains(&at)).then_some(at)",
        "    matched.then_some(at)",
        "a_time_outside_the_authoritys_own_dates_is_not_used",
    ),
    Mutation(
        "lt: offer no /DSS certificate to the chain builder",
        "src/docinfo.rs",
        "        let extra = &judging.dss_certificates;",
        "        let extra: &[Vec<u8>] = &[];",
        "the_dss_certificates_complete_a_chain_the_signature_does_not_carry",
    ),
    Mutation(
        "lt: refuse a genTime with a fraction",
        "src/docinfo.rs",
        "    let (whole, fraction) = match body.split_once('.') {",
        "    let (whole, fraction) = match None::<(&str, &str)> {",
        "a_gen_time_with_fractional_seconds_is_read_to_the_second",
    ),
    Mutation(
        "lt: accept a fraction DER forbids",
        "src/docinfo.rs",
        "            || fraction.ends_with('0')",
        "            || false",
        "a_gen_time_with_fractional_seconds_is_read_to_the_second",
    ),
    Mutation(
        "trust: judge an attested moment without the signer's own dates",
        "src/trust.rs",
        "    if at < from || at > until {",
        "    if false {",
        "a_certificate_not_in_force_at_the_attested_moment_is_refused_before_asking",
    ),
    Mutation(
        "trust: an attested moment's chain need not pass",
        "src/trust.rs",
        "        Err(_) => return Trust::unchecked(Doubt::Unavailable),\n    };\n    if !evaluation.passed {\n        return answer(Standing::Untrusted, Some(evaluation.failure));\n    }\n    if !serves {\n        let doubt = match purpose {\n            Purpose::Documents => Doubt::Purpose,\n            Purpose::Timestamping => Doubt::Timestamping,\n        };\n        return answer(Standing::Untrusted, Some(doubt));\n    }\n    answer(Standing::TrustedAtTimestamp, None)",
        "        Err(_) => return Trust::unchecked(Doubt::Unavailable),\n    };\n    if false && !evaluation.passed {\n        return answer(Standing::Untrusted, Some(evaluation.failure));\n    }\n    if !serves {\n        let doubt = match purpose {\n            Purpose::Documents => Doubt::Purpose,\n            Purpose::Timestamping => Doubt::Timestamping,\n        };\n        return answer(Standing::Untrusted, Some(doubt));\n    }\n    answer(Standing::TrustedAtTimestamp, None)",
        "a_chain_refused_at_the_attested_moment_is_untrusted_with_that_reason_and_moment",
    ),
    Mutation(
        "trust: offer no extra certificate at all",
        "src/trust.rs",
        "        if added == MAX_CERTIFICATES {",
        "        if true {",
        "dss_certificates_are_offered_within_their_own_bound",
    ),
    Mutation(
        "trust: offer every extra certificate",
        "src/trust.rs",
        "        if added == MAX_CERTIFICATES {",
        "        if false {",
        "dss_certificates_are_offered_within_their_own_bound",
    ),
    Mutation(
        "cli: a revocation before the moment passes --strict",
        "src/cli/verify.rs",
        "            .is_some_and(|r| r.standing == crate::revocation::Status::Revoked && !r.after_moment)",
        "            .is_some_and(|r| false && r.standing == crate::revocation::Status::Revoked && !r.after_moment)",
        "a_revocation_after_the_attested_time_does_not_fail_strict_and_one_before_does",
    ),
    Mutation(
        "cli: a revocation after the moment fails --strict",
        "src/cli/verify.rs",
        "            .is_some_and(|r| r.standing == crate::revocation::Status::Revoked && !r.after_moment)",
        "            .is_some_and(|r| r.standing == crate::revocation::Status::Revoked)",
        "a_revocation_after_the_attested_time_does_not_fail_strict_and_one_before_does",
    ),
]
