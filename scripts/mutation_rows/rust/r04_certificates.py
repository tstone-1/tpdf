"""One part of the Rust mutation table.

Rows only. `__init__.py` beside this file joins the parts in order into the
harness's `MUTATIONS`; the harness says what a row is and how it runs.
"""

from .base import MUT_APPENDABLE, MUT_SUBTYPE_GUARD, Mutation

ROWS: list[Mutation] = []

# --- the certificate a signature was made with ----------------------------
ROWS += [
    Mutation(
        # Take the first certificate in the set instead of the one the signer
        # actually points at. The set is *unordered* and normally carries the
        # chain, so on any real document this names a certificate authority as
        # the signer roughly as often as it names the signer. Every fixture here
        # has a chain of one, which is why the check that catches this is the
        # `matched_signer` assertion and not the name.
        "docinfo: take the first certificate rather than the signer's",
        "src/certificate.rs",
        """    match (matched, certificates.as_slice()) {
        (Some(certificate), _) => Some((certificate, true)),
        (None, [only]) => Some((*only, false)),
        (None, _) => None,
    }""",
        """    Some((certificates[0], false))""",
        "each_signed_fixture_carries_its_own_certificate",
    ),
    Mutation(
        # Report a blob that could not be parsed as a document with no
        # certificate. The two readings are opposite --- absent is about the
        # file, unread is about tpdf --- and this is the direction that makes
        # tpdf's own failure look like the document's silence.
        "docinfo: report an unreadable certificate as an absent one",
        "src/docinfo.rs",
        """        None => {
            limits.certificates_unread += 1;
            None
        }""",
        """        None => None,""",
        "a_blob_that_is_not_der_is_reported_as_unread_rather_than_absent",
    ),
    Mutation(
        # The mirror: count an all-zero placeholder as a failure. A signature is
        # written by reserving a span and filling it, so this would put a "could
        # not read" notice on a document with nothing wrong with it.
        "docinfo: treat a reserved-but-empty placeholder as a parse failure",
        "src/docinfo.rs",
        """    if raw.iter().all(|byte| *byte == 0) {
        return None;
    }""",
        """    if raw.iter().all(|byte| *byte == 0) {
        *unread += 1;
        return None;
    }""",
        "an_untouched_placeholder_is_absent_rather_than_unread",
    ),
    Mutation(
        # Hand the padded blob to the decoder. The trailing zeros are not DER and
        # a decoder is entitled to reject them, so every certificate would read
        # as unparseable --- the failure that looks like a broken dependency.
        "docinfo: hand the parser the reserved span exactly as it arrived",
        "src/docinfo.rs",
        "    let Some(blob) = ber::to_definite_length(raw) else {",
        "    let Some(blob) = Some(raw.to_vec()) else {",
        "each_signed_fixture_carries_its_own_certificate",
    ),
    Mutation(
        # Drop the size bound, so an attacker-chosen megabyte of nested DER goes
        # to the parser. The blob is the most attacker-controlled thing in the
        # document and this is the only thing standing between it and a parser.
        "docinfo: parse a certificate blob of any size",
        "src/docinfo.rs",
        """    if blob.len() > bound {
        *unread += 1;
        return None;
    }""",
        "",
        "a_bound_a_valid_blob_exceeds_refuses_it_rather_than_parsing_it",
    ),
    Mutation(
        # Compare issuers by their common name rather than by their encoded
        # bytes. Two different authorities may use the same CN, and a DN is
        # equal by encoding, not by the one attribute a person reads.
        "docinfo: match a signer by common name rather than by encoded issuer",
        "src/certificate.rs",
        """                certificate.tbs_certificate.serial_number == both.serial_number
                    && certificate.tbs_certificate.issuer.to_der().ok() == both.issuer.to_der().ok()""",
        """                common_name(&certificate.tbs_certificate.issuer)
                    == common_name(&both.issuer)""",
        "a_certificate_from_the_right_issuer_but_the_wrong_serial_is_not_the_signer",
    ),
    Mutation(
        # Call every certificate self-issued. `self_issued` is one of exactly two
        # unhedged statements the signature section makes, so a wrong one is a
        # confident false claim about who vouched for a signer.
        "docinfo: call every certificate self-issued",
        "src/certificate.rs",
        "        self_issued: tbs.subject.to_der().ok() == tbs.issuer.to_der().ok(),",
        "        self_issued: true,",
        "a_certificate_somebody_else_issued_is_not_called_self_issued",
    ),
    Mutation(
        # List the signature fields in whatever order the queue pops them, which
        # is the reverse of document order. Every fact about every signature is
        # still correct; only which signature each belongs to is wrong, so a
        # reader is shown the right names against the wrong fields. Invisible on
        # any one-signature fixture, which was every signed fixture until
        # `incr-two-signers.pdf`.
        "docinfo: list the signature fields in the reverse of document order",
        # Re-aimed 2026-09-06: the traversal moved to `fields.rs`, where the
        # reversal is what `Order::Document` means. The test that catches it is
        # still `docinfo`'s, because `redact` walks the same tree in the other
        # order on purpose and cannot see this.
        "src/fields.rs",
        """    if bounds.order == Order::Document {
        queue.reverse();
    }""",
        "",
        "two_signers_are_told_apart_and_neither_is_reported_as_the_authority",
    ),
    Mutation(
        # Take the leaf's issuer as the signer's own name. Both leaves here are
        # issued by one root, so this reports "tpdf test root CA" for both
        # signatures --- the certificate-authority-as-signer mistake, which is
        # what the whole `SignerInfo.sid` walk exists to avoid.
        "docinfo: report the issuer's name as the subject's",
        "src/certificate.rs",
        "        subject_cn: common_name(&tbs.subject),",
        "        subject_cn: common_name(&tbs.issuer),",
        "two_signers_are_told_apart_and_neither_is_reported_as_the_authority",
    ),
    Mutation(
        # Stop recursing into `/Kids`, so a signature field grouped under a node
        # is walked straight past. The document then reports no signature at all,
        # which is indistinguishable from one that has none -- and PDFium does
        # exactly this, so the differential cannot catch it either.
        "docinfo: do not walk into a field node's kids",
        # Re-aimed 2026-09-06 with the traversal. Emptying the push loop is
        # what "walk past a node's kids" is now spelled as, and it is the same
        # edit: a signature under a `/Kids` node is never reached.
        "src/fields.rs",
        """        for kid in pushed {
            queue.push((Entry::Inline(kid), depth + 1, name.clone()));
        }""",
        """        for kid in pushed {
            let _ = (kid, depth, &name);
        }""",
        "a_signature_field_under_kids_is_found_rather_than_walked_past",
    ),
    Mutation(
        # Report the leaf's own `/T` rather than the fully qualified name. Every
        # fixture but one has unique leaf names, so this is right about all of
        # them and wrong about what it means: `/T` is unique among siblings only.
        "docinfo: name a field by its own /T alone",
        # Re-aimed 2026-09-06 with the traversal.
        "src/fields.rs",
        "            (true, Some(field)) => qualified_name(&prefix, &partial_name(doc, field)),",
        "            (true, Some(field)) => partial_name(doc, field),",
        "two_fields_with_the_same_leaf_name_are_told_apart_by_the_groups_above_them",
    ),
    Mutation(
        # Give an unnamed node a level of the name anyway, which puts an empty
        # component in the middle -- `top..Signature1`, a string no other reader
        # shows -- and gives a wholly unnamed chain the name ".".
        # Re-aimed 2026-09-06: `qualified_name` moved to `fields.rs` with the
        # traversal that calls it.
        "fields: make an unnamed field node a level of the name",
        "src/fields.rs",
        "        (_, true) => prefix.to_string(),",
        "        (_, true) => format!(\"{prefix}.\"),",
        "a_node_with_no_name_of_its_own_is_not_a_level_of_the_name",
    ),
    Mutation(
        # Hand each kid an empty prefix instead of its parent's name, so the
        # ancestry is walked and then thrown away one level at a time.
        "docinfo: forget the ancestry when descending into kids",
        # Re-aimed 2026-09-06 with the traversal.
        "src/fields.rs",
        "            queue.push((Entry::Inline(kid), depth + 1, name.clone()));",
        "            queue.push((Entry::Inline(kid), depth + 1, String::new()));",
        "a_signature_field_under_kids_is_found_rather_than_walked_past",
    ),
    Mutation(
        # Report an extension that would not decode as an absent one. The two
        # are opposite claims -- absent places no limit on the key, malformed
        # places an unknown one -- and absent is the reassuring branch.
        "docinfo: read a malformed extension as an absent one",
        "src/certificate.rs",
        """        Err(_) => {
            *unread += 1;
            None
        }""",
        "        Err(_) => None,",
        "an_extension_that_will_not_decode_is_counted_rather_than_read_as_absent",
    ),
    Mutation(
        # Swap the first two key usage bits. This SURVIVED when it was aimed at
        # the fixture test, and correctly: a real signing certificate sets both
        # bits, so permuting two selected entries yields the same list. The test
        # it names now sets one bit at a time, which is the only shape that can
        # tell a table from a permutation of it.
        "docinfo: name the first two key usage bits the wrong way round",
        "src/certificate.rs",
        """        (KeyUsages::DigitalSignature, "Digital signature"),
        (KeyUsages::NonRepudiation, "Non-repudiation"),""",
        """        (KeyUsages::NonRepudiation, "Digital signature"),
        (KeyUsages::DigitalSignature, "Non-repudiation"),""",
        "each_key_usage_bit_is_named_by_the_name_rfc_5280_gives_it",
    ),
    Mutation(
        # Reorder two whole rows, keeping every bit paired with its own name.
        # The single-bit half of that test cannot see this -- each of its
        # assertions is over a one-element list -- so it is the all-bits control
        # beneath them that catches it, which is what that control is for.
        "docinfo: list the key usage bits out of specification order",
        "src/certificate.rs",
        """        (KeyUsages::KeyEncipherment, "Key encipherment"),
        (KeyUsages::DataEncipherment, "Data encipherment"),""",
        """        (KeyUsages::DataEncipherment, "Data encipherment"),
        (KeyUsages::KeyEncipherment, "Key encipherment"),""",
        "each_key_usage_bit_is_named_by_the_name_rfc_5280_gives_it",
    ),
    Mutation(
        # Read only child elements, not attributes. Three of the eight claims in
        # a 41-document corpus are written as attributes, and a document that
        # claims nothing is the overwhelming majority -- so the silence this
        # produces looks exactly like an ordinary file.
        "xmp: read a conformance claim only when it is a child element",
        "src/xmp.rs",
        """                read_attributes(
                    &reader,
                    &element,
                    &mut out,
                    &mut pdfa_part,
                    &mut pdfa_conformance,
                );
                stack.push(property);""",
        "                stack.push(property);",
        "a_claim_written_as_attributes_is_read_as_the_same_claim",
    ),
    Mutation(
        # Match the conventional prefix instead of the namespace URI. Right
        # about every document a producer wrote conventionally, wrong in both
        # directions on the ones that did not.
        "xmp: identify a property by its prefix rather than its namespace",
        "src/xmp.rs",
        '        (NS_PDFAID, "part") => Some(Property::PdfaPart),',
        '        (_, "part") => Some(Property::PdfaPart),',
        "a_claim_is_identified_by_its_namespace_and_not_by_its_prefix",
    ),
    Mutation(
        # Take the first text event as the whole value. Correct for every value
        # with no entity in it, which is most of them, and silently truncating
        # for the rest.
        "xmp: take the first fragment of a value as the whole value",
        "src/xmp.rs",
        """                if let Some((_, _, buffer)) = &mut pending {
                    append(buffer, &text, &mut out.unread);
                }
            }
            Ok(Event::GeneralRef(reference)) => {""",
        """                if let Some((_, _, buffer)) = &mut pending {
                    if buffer.is_empty() {
                        append(buffer, &text, &mut out.unread);
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {""",
        "a_value_arriving_in_pieces_is_put_back_together",
    ),
    Mutation(
        # Resolve an entity this module does not know, which is the shape of the
        # attack the refusal exists to stop. The point is not that a resolver
        # would expand it -- it is that the refusing branch is load-bearing
        # rather than decoration.
        "xmp: resolve an entity this module does not know",
        "src/xmp.rs",
        """                let Ok(resolved) = quick_xml::escape::unescape(&spelled) else {
                    out.unread = true;
                    continue;
                };""",
        """                let resolved = quick_xml::escape::unescape(&spelled)
                    .unwrap_or(std::borrow::Cow::Borrowed("lol"));""",
        "a_billion_laughs_packet_is_neither_expanded_nor_followed",
    ),
    Mutation(
        # Drop the nesting bound, so a chain a document chose the depth of is
        # followed to the end.
        "xmp: follow a packet's nesting to whatever depth it claims",
        "src/xmp.rs",
        "                if stack.len() >= MAX_DEPTH {",
        "                if stack.len() >= usize::MAX {",
        "nesting_past_the_bound_stops_and_is_reported",
    ),
    Mutation(
        # Abandon an oversized packet in silence, so it reads as a document that
        # says nothing about itself.
        "xmp: drop an oversized packet without saying so",
        "src/xmp.rs",
        """    if packet.len() > MAX_PACKET {
        out.unread = true;
        return out;
    }""",
        """    if packet.len() > MAX_PACKET {
        return out;
    }""",
        "a_packet_larger_than_the_cap_is_reported_rather_than_dropped",
    ),
    Mutation(
        # Bound the finished string rather than the accumulation, so a document
        # can make this hold whatever it likes and then clip it.
        "xmp: let a value grow without bound and clip it at the end",
        "src/xmp.rs",
        """    if buffer.len() >= MAX_VALUE {
        *unread = true;
        return;
    }""",
        "",
        "a_value_assembled_past_the_cap_stops_accumulating_and_says_so",
    ),
    Mutation(
        # Report a malformed packet as a document that claims nothing, dropping
        # what was read before the damage as well as the notice.
        "xmp: read a malformed packet as a document claiming nothing",
        "src/xmp.rs",
        """            Err(_) => {
                out.unread = true;
                break;
            }""",
        """            Err(_) => {
                out = Xmp {
                    bytes: out.bytes,
                    ..Xmp::default()
                };
                break;
            }""",
        "a_packet_that_stops_making_sense_keeps_what_it_had_and_says_so",
    ),
    Mutation(
        # Require a conformance letter beside the part. PDF/A-4 dropped it, so
        # this reports nothing for the newest standard in the family.
        "xmp: require a conformance letter beside a PDF/A part",
        "src/xmp.rs",
        "    if !pdfa_part.is_empty() {",
        "    if !pdfa_part.is_empty() && !pdfa_conformance.is_empty() {",
        "a_part_with_no_conformance_letter_is_still_a_claim",
    ),
    Mutation(
        # Stop consulting the catalog's /Metadata, so every document reports no
        # packet -- which is true of most of them.
        "docinfo: report every document as carrying no metadata packet",
        "src/docinfo.rs",
        "        xmp: catalog.and_then(|c| read_xmp(document, c)),",
        "        xmp: catalog.and_then(|_| None),",
        "a_conformance_claim_in_the_metadata_stream_reaches_the_readout",
    ),
    Mutation(
        # Read only unfiltered streams. The specification prefers those and most
        # producers write them, so this is right about the common case and blind
        # to Acrobat's.
        "docinfo: read a metadata stream only when it carries no filter",
        "src/docinfo.rs",
        """    let packet = stream
        .decompressed_content_with_limit(crate::xmp::MAX_PACKET)
        .unwrap_or_else(|_| stream.content.clone());""",
        "    let packet = stream.content.clone();",
        "a_conformance_claim_in_the_metadata_stream_reaches_the_readout",
    ),
    Mutation(
        # Ask for the packet unbounded, which is what this was until 2026-09-01:
        # a 2,289-byte document declaring a 1 GiB /Metadata packet then costs the
        # worker 1,081 MB. The answer does not change -- xmp::scan discards
        # anything over MAX_PACKET either way -- so only a reading about COST can
        # catch this, which is why the test named here asserts on the packet
        # length reported and not on `unread`.
        "docinfo: ask for a metadata packet with no bound on its size",
        "src/docinfo.rs",
        "        .decompressed_content_with_limit(crate::xmp::MAX_PACKET)",
        "        .decompressed_content()",
        "a_metadata_packet_over_the_packet_bound_is_never_inflated",
    ),
    Mutation(
        # Bound at MAX_DECODE, the constant already in scope and the obvious
        # repair -- and 64x larger than any packet xmp::scan will look at. The
        # fixture is sized one byte past MAX_PACKET precisely so that this is
        # distinguishable from the bound above it.
        "docinfo: bound the metadata packet at the general decode ceiling",
        "src/docinfo.rs",
        "        .decompressed_content_with_limit(crate::xmp::MAX_PACKET)",
        "        .decompressed_content_with_limit(MAX_DECODE)",
        "a_metadata_packet_over_the_packet_bound_is_never_inflated",
    ),
    Mutation(
        # Read any CMS's encapsulated content as a TSTInfo. Every ordinary
        # signature then yields a "timestamp" made of whatever a
        # GeneralizedTime can be built from -- a plausible number attributed to
        # an authority, which is the worst outcome this module has.
        "docinfo: read any CMS content as a timestamp",
        "src/docinfo.rs",
        """    if signed.encap_content_info.econtent_type.to_string() != "1.2.840.113549.1.9.16.1.4" {
        return None;
    }""",
        "",
        "a_token_relabelled_as_something_else_is_not_read_as_a_timestamp",
    ),
    Mutation(
        # Read the fourth field as genTime. Off by one in a positional skip,
        # which is the failure the positional skip has to be bounded against.
        "docinfo: take the field before genTime as the attested time",
        "src/docinfo.rs",
        "    for _ in 0..4 {",
        "    for _ in 0..3 {",
        "the_time_a_timestamp_authority_attested_is_read",
    ),
    Mutation(
        # Drop the tag check, so a TSTInfo that is not a SEQUENCE has its value
        # walked as if it were one.
        "docinfo: read a TSTInfo without checking it is a sequence",
        "src/docinfo.rs",
        """    if sequence.tag() != der::Tag::Sequence {
        return None;
    }""",
        "",
        "a_tst_info_that_is_not_shaped_like_one_yields_no_time",
    ),
    Mutation(
        # Take the first of several values on a timestamp attribute. There is
        # nothing to choose between them, so this is a guess presented as an
        # authority's statement.
        "docinfo: guess when a timestamp attribute carries several values",
        "src/docinfo.rs",
        """    let [value] = attribute.values.as_slice() else {
        *unread += 1;
        return None;
    };""",
        """    let Some(value) = attribute.values.as_slice().first() else {
        *unread += 1;
        return None;
    };""",
        "a_timestamp_attribute_carrying_more_than_one_value_is_refused",
    ),
    Mutation(
        # Look for the timestamp in the SIGNED attributes. A token is minted
        # after the signature exists, so it cannot be inside what the signature
        # covers -- and every real signature would then report none.
        "docinfo: look for the timestamp among the signed attributes",
        "src/docinfo.rs",
        """    let attribute = signer
        .unsigned_attrs""",
        """    let attribute = signer
        .signed_attrs""",
        "the_time_a_timestamp_authority_attested_is_read",
    ),
    Mutation(
        # Report a token that would not parse as a signature nobody timestamped,
        # which is the ordinary case and therefore the reassuring one.
        "docinfo: read an unreadable timestamp token as an absent one",
        "src/docinfo.rs",
        """    let Some(timestamp) = parse_timestamp_token(&token) else {
        *unread += 1;
        return None;
    };""",
        """    let Some(timestamp) = parse_timestamp_token(&token) else {
        return None;
    };""",
        "a_token_that_will_not_parse_is_counted_rather_than_read_as_absent",
    ),
    Mutation(
        # Report a key usage naming nothing as no extension at all, which turns
        # "this key is for nothing" into "no limit was placed on this key".
        "docinfo: read an empty key usage as an absent one",
        "src/certificate.rs",
        """    Some(
        named
            .into_iter()
            .filter(|(bit, _)| usage.0.contains(*bit))
            .map(|(_, name)| name.to_string())
            .collect(),
    )""",
        """    let list: Vec<String> = named
        .into_iter()
        .filter(|(bit, _)| usage.0.contains(*bit))
        .map(|(_, name)| name.to_string())
        .collect();
    (!list.is_empty()).then_some(list)""",
        "a_certificate_stating_no_usage_and_one_stating_none_are_told_apart",
    ),
    Mutation(
        # Drop an extended key usage OID this module cannot name. The reader is
        # then shown the purposes we happen to know and told nothing about the
        # rest, which reads as the issuer having named only those.
        "docinfo: drop an extended key usage nobody here can name",
        "src/certificate.rs",
        "        other => return other.to_string(),",
        "        _ => \"\",",
        "an_extended_usage_this_module_cannot_name_is_shown_as_its_oid",
    ),
    Mutation(
        # Report an absent basic constraints as a stated CA:FALSE. RFC 5280
        # reads it that way for chain building, and it is still the certificate
        # not saying something rather than saying it.
        "docinfo: read absent basic constraints as a stated CA:FALSE",
        "src/certificate.rs",
        """    let constraints: BasicConstraints = decode_extension(extensions, "2.5.29.19", unread)?;
    Some(constraints.ca)""",
        """    let constraints: Option<BasicConstraints> =
        decode_extension(extensions, "2.5.29.19", unread);
    Some(constraints.is_some_and(|constraints| constraints.ca))""",
        "a_certificate_stating_no_usage_and_one_stating_none_are_told_apart",
    ),
    Mutation(
        # Drop the depth bound, so a `/Kids` chain a document chose the length of
        # is followed to the end. The queue is the only thing between a hostile
        # field tree and however long it cares to be.
        "docinfo: follow a field tree to whatever depth it claims",
        # Re-aimed 2026-09-06 with the traversal. `false` rather than a larger
        # number, because the bound is now a value the caller passes and the
        # mutation has to remove the test of it rather than the number in it.
        "src/fields.rs",
        "        if bounds.depth.is_some_and(|max| depth >= max) {",
        "        if false {",
        "a_field_tree_is_walked_to_a_bounded_depth_and_the_refusal_is_counted",
    ),
    Mutation(
        # Refuse a too-deep tree in silence. A signature dropped without a word
        # reads as a document that has none, which is the reassuring direction.
        "docinfo: drop a too-deep field node without counting it",
        # Re-aimed 2026-09-06 with the traversal. The bound still fires; only
        # the count of what it cut goes, which is the half that reads as a form
        # with nothing more in it.
        "src/fields.rs",
        "            cut.too_deep += 1;\n            continue;",
        "            continue;",
        "a_field_tree_is_walked_to_a_bounded_depth_and_the_refusal_is_counted",
    ),
    Mutation(
        # Read a BMPString as bytes. It comes out as text interleaved with nulls,
        # which reads as a mangled name rather than as a decoding bug --- so this
        # is the version that ships looking merely ugly.
        "docinfo: read a UTF-16 name as bytes",
        "src/certificate.rs",
        "    let text = if attribute.value.tag().number().value() == 0x1e {",
        "    let text = if false {",
        "a_utf16_name_is_decoded_rather_than_shown_with_nulls",
    ),
    Mutation(
        # Print the serial with the bytes reversed. Endianness is invisible in a
        # hex string --- it looks exactly as much like a serial either way --- so
        # only a comparison against another tool's answer can see it.
        "docinfo: print a serial least-significant byte first",
        "src/certificate.rs",
        "    for byte in bytes.iter().take(MAX_VALUE_CHARS / 2) {",
        "    for byte in bytes.iter().rev().take(MAX_VALUE_CHARS / 2) {",
        "every_field_of_a_certificate_whose_bytes_the_test_chose",
    ),
    Mutation(
        # Report `notBefore` as `notAfter`. Both are dates in the same format on
        # adjacent lines, which is what makes it survive a reading.
        "docinfo: report a certificate's validity from one end only",
        "src/certificate.rs",
        "        until: certificate_date(&tbs.validity.not_after),",
        "        until: certificate_date(&tbs.validity.not_before),",
        "every_field_of_a_certificate_whose_bytes_the_test_chose",
    ),
]

# --- indefinite lengths, and where a signature blob ends -------------------
ROWS += [
    Mutation(
        # Read the coverage from the first range pair only. On a signature the
        # answer is then the bytes before the blob and not the ones after it,
        # which is most of a document -- and the check that sees it derives the
        # same quantity from where the /Contents hex sits, by a route this code
        # does not take. That derivation replaced a number transcribed from
        # `qpdf --json`, which was a real third reader and a fact about one
        # machine's pyhanko: both CI runners build a file 31 bytes smaller.
        "docinfo: count only the first pair of a signature's byte range",
        "src/docinfo.rs",
        """        out.covered_bytes = numbers
            .chunks_exact(2)
            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())
            .fold(0_u64, u64::saturating_add);""",
        """        out.covered_bytes = numbers
            .chunks_exact(2)
            .map(|pair| u64::try_from(pair[1]).unwrap_or_default())
            .next()
            .unwrap_or_default();""",
        "the_docmdp_levels_of_four_documents_another_program_signed",
    ),
    Mutation(
        # Stop counting a blob whose structure will not walk. The two readings
        # are opposite --- absent is about the file, unread is about tpdf ---
        # and this is the direction that makes tpdf's own failure look like the
        # document's silence. It is the *walk's* half of that distinction; the
        # parser's half is a mutation of its own, because a blob that reaches
        # one never reaches the other.
        "docinfo: report a blob that will not walk as a signature with no certificate",
        "src/docinfo.rs",
        """    let Some(blob) = ber::to_definite_length(raw) else {
        *unread += 1;
        return None;
    };""",
        """    let Some(blob) = ber::to_definite_length(raw) else {
        return None;
    };""",
        "a_blob_whose_structure_will_not_walk_is_reported_as_unread",
    ),
    Mutation(
        # Accept the indefinite form on a primitive value. There is no marker to
        # read such a value up to, so the walk would run into whatever follows
        # and call it content --- X.690 forbids it for exactly that reason.
        "ber: let an indefinite length end a primitive value too",
        "src/ber.rs",
        """        if !constructed {
            return None;
        }""",
        "",
        "an_indefinite_primitive_is_refused",
    ),
    Mutation(
        # Trust a child to stay inside the length its parent declared. A child
        # is bounded by the input, not by its parent, so one may decode
        # perfectly and still overrun --- and the parent's length would then
        # describe fewer bytes than the parent holds.
        "ber: trust a child to stay inside the length its parent declared",
        "src/ber.rs",
        """                if cursor > end {
                    return None;
                }""",
        "",
        "a_child_that_overruns_its_parent_is_refused",
    ),
    Mutation(
        # Delete the nesting bound. This is the only copy of it: `emit` runs
        # after `measure` returned and deliberately carries no second guard, so
        # a mutation of that one would have survived.
        "ber: follow nesting to whatever depth the blob claims",
        "src/ber.rs",
        """    if depth > MAX_DEPTH {
        return None;
    }
    let head = header(raw, at)?;
    let body = at.checked_add(head.tag)?.checked_add(head.length)?;
    let mut content = 0usize;""",
        """    let head = header(raw, at)?;
    let body = at.checked_add(head.tag)?.checked_add(head.length)?;
    let mut content = 0usize;""",
        "nesting_is_followed_to_the_bound_and_refused_past_it",
    ),
    Mutation(
        # Bound nesting where no real signature fits. The unit test names the
        # constant, so it cannot see this at all; the fixture check can, because
        # a timestamped signature nests a chain inside a token inside a signer
        # and reaches about twenty-five.
        "ber: bound nesting at eight levels",
        "src/ber.rs",
        "pub const MAX_DEPTH: usize = 64;",
        "pub const MAX_DEPTH: usize = 8;",
        "a_der_signature_blob_is_returned_byte_for_byte",
    ),
    Mutation(
        # Write every length in the long form. Legal BER, not DER, and the
        # identity property that lets this sit in front of every signature
        # rather than only the ones that need it goes with it.
        "ber: write a length in the long form whatever its size",
        "src/ber.rs",
        "    if value < 0x80 {\n        field[0] = value as u8;",
        "    if value < 0x00 {\n        field[0] = value as u8;",
        "a_definite_encoding_comes_back_unchanged",
    ),
    Mutation(
        # Accept a length field of any width. `0xff` is reserved and anything
        # past four bytes is a length no blob here can have, so this is a walk
        # taking an attacker's word for how far to read.
        "ber: accept a length field of any width",
        "src/ber.rs",
        """    if count > MAX_LENGTH_BYTES {
        return None;
    }""",
        "",
        "a_length_field_past_the_bound_is_refused",
    ),
    Mutation(
        # Keep whatever follows the first value. That is the reserved padding,
        # and handing it to a decoder is what the scan this module replaced was
        # for --- a decoder is entitled to reject it.
        "ber: keep the bytes that follow the first value",
        "src/ber.rs",
        "    emit(raw, 0, 0, &mut out)?;",
        """    emit(raw, 0, 0, &mut out)?;
    out.extend_from_slice(&raw[span.input..]);""",
        "padding_after_the_first_value_is_dropped",
    ),
    Mutation(
        # Write the input length where the content length belongs. On a value
        # that was already definite the two agree, which is what makes this
        # survive a reading; on an indefinite one the header then claims the
        # marker bytes as content.
        "ber: write the length the input used rather than the one being written",
        "src/ber.rs",
        "    let (field, length) = length_field(span.content);",
        "    let (field, length) = length_field(span.input);",
        "an_indefinite_length_becomes_a_definite_one",
    ),
]

#: Moving a mark: the command a reader's drag produces.
ROWS += [
    Mutation(
        # Move the rectangle and leave the strokes where they are. Every kind but
        # ink has no strokes, so this is invisible on five of the six -- and the
        # saved file then draws the line in the old place inside a `/Rect` in the
        # new one, which is the underline defect's shape once more.
        "displace: move a mark's rectangle and leave its strokes behind",
        "src/docmodel.rs",
        """        let strokes = self
            .strokes_of(mark)
            .iter()
            .map(|stroke| Stroke {
                points: stroke
                    .points
                    .iter()
                    .map(|p| Point {
                        x: p.x + dx,
                        y: p.y + dy,
                    })
                    .collect(),
            })
            .collect();""",
        "        let strokes = self.strokes_of(mark).to_vec();",
        "moving_a_mark_carries_its_rectangle_and_its_strokes_together",
    ),
    Mutation(
        # Move one corner of each rectangle rather than both, which is a resize
        # wearing a move's name -- the failure the delta-not-a-geometry argument
        # in `Doc::displace` is about, arriving from inside rather than over the
        # wire.
        "displace: move a rectangle's top-left corner and not its bottom-right",
        "src/docmodel.rs",
        """                right: q.right + dx,
                bottom: q.bottom + dy,""",
        """                right: q.right,
                bottom: q.bottom,""",
        "moving_a_mark_changes_where_it_is_and_nothing_else_about_it",
    ),
    Mutation(
        # Refuse anything that is not ink, which is `reink`'s rule next door and
        # is the wrong one here: geometry is geometry, and which kinds a reader
        # is offered the drag on is a product rule one layer up.
        "displace: refuse to move a mark that has no strokes",
        "src/docmodel.rs",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n        self.now.live_mark(mark)?;",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n"
        "        self.now.live_mark(mark)?;\n"
        "        let kind = self.mark(mark).map_or(MarkKind::Highlight, |m| m.kind);\n"
        "        if kind != MarkKind::Ink {\n"
        "            return Err(Refusal::ShapeMismatch(kind));\n"
        "        }",
        "every_kind_can_be_moved_including_the_one_that_cannot_be_erased",
    ),
    Mutation(
        # Write the new geometry into the body instead of journalling it. Every
        # visible behaviour is identical until an undo, which then puts the mark
        # back nowhere -- the whole reason a move is a command.
        "displace: change where a mark is without journalling it",
        "src/docmodel.rs",
        "            .collect();\n"
        "        let ink = self.issue_ink(Ink { strokes, quads });\n"
        "        self.apply(Command::Reink { mark, ink })\n"
        "    }\n"
        "\n"
        "    /// Resizes a signature",
        "            .collect();\n"
        "        if let Some(body) = self.marks.get_mut(&mark) {\n"
        "            body.quads = quads;\n"
        "            body.strokes = strokes;\n"
        "        }\n"
        "        Ok(())\n"
        "    }\n"
        "\n"
        "    /// Resizes a signature",
        "undoing_a_move_puts_the_mark_back_where_it_was",
    ),
    Mutation(
        # Spend a version before the liveness check, so a refused move leaves an
        # `Ink` body behind. Nothing a reader can see says so, which is why the
        # accounting observable exists.
        "displace: spend a version on a mark that is not there",
        "src/docmodel.rs",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n        self.now.live_mark(mark)?;",
        "    pub fn displace(&mut self, mark: MarkId, dx: f32, dy: f32) -> Result<(), Refusal> {\n"
        "        self.issue_ink(Ink {\n"
        "            strokes: Vec::new(),\n"
        "            quads: Vec::new(),\n"
        "        });\n"
        "        self.now.live_mark(mark)?;",
        "moving_a_mark_that_is_not_there_is_refused_before_a_version_is_spent",
    ),
    Mutation(
        # Substitute zero for a non-finite offset, which is `channel`'s answer to
        # the same shape of input and is the wrong one here: a drag that silently
        # did nothing is indistinguishable from a viewer that has stopped working.
        "displace: take a non-finite offset as no offset at all",
        "src/edits.rs",
        """        if !dx.is_finite() || !dy.is_finite() {
            return Err(format!("a mark cannot be moved by ({dx}, {dy})"));
        }""",
        """        let dx = if dx.is_finite() { dx } else { 0.0 };
        let dy = if dy.is_finite() { dy } else { 0.0 };""",
        "a_move_by_a_non_finite_offset_is_refused_rather_than_ignored",
    ),
]

#: Printing what the reader sees: the crop, the marks, and the view rotation.
ROWS += [
    Mutation(
        # Stop asking about the crop, which is the clause that was missing. A
        # cropped document then reports itself as the file on disk and the
        # printer is handed the uncropped original.
        "identity: read a cropped document as the file on disk",
        "src/edits.rs",
        "                theirs && turns % 4 == 0 && crop.is_none()",
        "                theirs && turns % 4 == 0 && (crop.is_none() || crop.is_some())",
        "a_cropped_document_is_not_the_file_on_disk",
    ),
    Mutation(
        # Never answer `Working`, which is the state printing was in: everything
        # that was not handed over byte for byte went through `build`, and
        # `build` has no marks and no crops.
        "print: send every job that is not the file through the range builder",
        "src/print.rs",
        """    if explicit || plan.is_none() {
        return Route::Range(job);
    }
    Route::Working""",
        "    Route::Range(job)",
        "a_cropped_or_marked_document_is_built_rather_than_handed_over",
    ),
    Mutation(
        # Drop the reader's own rotation on the working route. It SURVIVED when
        # first run: the route test asserts which producer is chosen and says
        # nothing about what it produces, and every rotation test beside it
        # drives the *other* producer. A reader viewing sideways would have
        # printed upright.
        "print: drop the view rotation from a job built by the save writer",
        "src/save.rs",
        "            Ok((slot, (page.turns % 4 + view % 4) % 4))",
        "            Ok((slot, page.turns))",
        "a_third_parser_sees_the_view_rotation_on_a_job_built_from_the_working_document",
    ),
    Mutation(
        # Let the view replace a page's own turn rather than composing with it.
        # This one SURVIVED too, and the fixture was the reason rather than the
        # assertion: with no page carrying an edit turn the two expressions are
        # the same number. The test rotates one page now.
        "print: let the view rotation replace a page's own instead of composing",
        "src/save.rs",
        "            Ok((slot, (page.turns % 4 + view % 4) % 4))",
        "            Ok((slot, view % 4))",
        "a_third_parser_sees_the_view_rotation_on_a_job_built_from_the_working_document",
    ),
]

#: Saving by appending an update section rather than rewriting the document.
ROWS += [
    Mutation(
        # Call every plan with a mark appendable, so a deletion, a move, a turn
        # or a crop is written as an update section --- edits an update section
        # cannot express, and which spike 0.6 never put to any parser.
        # Re-aimed 2026-08-26, when a redaction clause joined the predicate, and
        # again 2026-08-29, when note edits did and it was renamed
        # `only_adds_marks` -> `is_appendable`. Twice in three days is the
        # argument for the anchor gate rather than against the predicate: both
        # times the mutation was still exactly right and pointed at nothing.
        "append: append any plan that carries a mark",
        "src/edits.rs",
        MUT_APPENDABLE,
        "        (!self.marks.is_empty() || !self.notes.is_empty())\n            && self.redactions.is_empty()\n            && self.discards.is_empty()",
        "a_plan_that_only_adds_marks_is_appended_and_anything_else_is_rewritten",
    ),
    Mutation(
        # Report the file as put back without truncating it. The reader is then
        # told their document is untouched while it carries a half-written
        # revision, which is worse than either honest outcome.
        "append: say the file was put back without cutting it back",
        "src/save.rs",
        "        match file.set_len(appended.was) {",
        "        match Ok::<(), std::io::Error>(()) {",
        "an_append_that_cannot_be_read_back_puts_the_file_back_as_it_was",
    ),
    Mutation(
        # Append without checking the length. The update names byte offsets into
        # the previous revision, so writing it after any other length produces a
        # cross-reference pointing at the wrong bytes -- a file that opens and is
        # wrong, which is the worst of the three outcomes.
        #
        # Aimed at the comparison rather than at a call site, because since
        # 2026-08-22 both save paths reach it through `Fingerprint`: the length
        # half of `agrees_with_metadata` and of `agrees_with` is this one line.
        "append: write the update after whatever length the file now has",
        "src/fingerprint.rs",
        "        if meta.len() != self.len {",
        "        if false {",
        "an_append_to_a_file_that_changed_length_is_refused_and_writes_nothing",
    ),
    Mutation(
        # Take the fingerprint as advisory. `stage_in_place` refuses a changed
        # file and the two paths no longer share a function, so a refusal written
        # once is a refusal on one of them.
        "append: proceed over a file that changed since it was opened",
        "src/save.rs",
        "    let verified = opened_as.agrees_with(source).map_err(Refusal::changed)?;",
        "    let verified = opened_as\n"
        "        .agrees_with(source)\n"
        '        .unwrap_or_else(|_| crate::fingerprint::Fingerprint::of(source).expect("fp"));',
        "an_append_is_refused_when_the_file_changed_since_it_was_opened",
    ),
    Mutation(
        # Accept any file that parses. It SURVIVED when first run: the rollback
        # test plants a trailer pointing at nothing, so `load` errors and the
        # count is never compared. The test that reaches this arm builds a real
        # update section whose catalog names an empty page tree -- a file that
        # opens, and is empty.
        #
        # Re-aimed 2026-08-26 when the read-back moved behind `save::Reread`:
        # the comparison is the same comparison, and the parse it used to be
        # written beside is in a worker now, so the arm names a count rather
        # than a parsed document.
        "append: accept a saved file that parses, whatever it has lost",
        "src/save.rs",
        "        Ok(pages) if pages == appended.pages => {}",
        "        Ok(_) => {}",
        "an_append_that_parses_and_has_lost_pages_is_also_put_back",
    ),
    Mutation(
        # Rewrite the page dictionary even when its /Annots is its own object.
        # The update is then larger than the edit, which is what `docs/PLAN.md`
        # §5 records as the one document-shape dependency worth carrying -- and
        # here it also breaks the file, because the cloned page keeps a reference
        # to an array the update never brought across.
        "append: bring the page across rather than its /Annots array",
        "src/save.rs",
        """            AnnotsSite::ArrayObject(array) => incremental
                .opt_clone_object_to_new_document(*array)""",
        """            AnnotsSite::ArrayObject(_) => incremental
                .opt_clone_object_to_new_document(site.page)""",
        "a_third_parser_reads_an_appended_document",
    ),
]

#: Linux: the worker's boundary, signing from a token or a certificate file,
#: Tesseract's languages, printing to CUPS and the installed-font lookup.
#:
#: Added 2026-10-15. Until then these modules had been mutated only by hand
#: under WSL as each landed, so a release pass with `--since` could never check
#: them again. The hand mutations the commits name are here (`c27d6163`,
#: `c1569dda`, `c2b7f2c5`, `7f843537`, `30ee2e85`) where a unit test holds
#: them; the ones only a probe catches (`ocr-worker-probe`, `print-probe`)
#: stay with their probes. Every row is `only_on="linux"`: the modules are
#: compiled nowhere else. `worker_handover.rs` is the exception, run on both
#: unix platforms.
ROWS += [
    Mutation(
        # The file-system half of the boundary, which is the required half.
        "linux boundary: no Landlock",
        "src/sandbox_linux.rs",
        "    landlock(extra)?;\n    seccomp()",
        "    let _ = extra;\n    seccomp()",
        "a_contained_worker_reads_fonts_and_nothing_else",
        only_on="linux",
    ),
    Mutation(
        "linux boundary: no seccomp",
        "src/sandbox_linux.rs",
        "    landlock(extra)?;\n    seccomp()",
        "    landlock(extra)?;\n    let _ = seccomp;\n    Ok(())",
        "a_contained_worker_reads_fonts_and_nothing_else",
        only_on="linux",
    ),
    Mutation(
        # No font folder readable: PDFium on Linux then has no system font.
        "linux boundary: no font folder granted",
        "src/sandbox_linux.rs",
        "    for dir in FONT_DIRS {",
        '    for dir in ["/nonexistent-tpdf"] {',
        "a_contained_worker_reads_fonts_and_nothing_else",
        only_on="linux",
    ),
    Mutation(
        # An io_uring ring makes a socket without the `socket` call. The
        # list's length is kept, so the array still compiles.
        "linux boundary: let a worker make an io_uring ring",
        "src/sandbox_linux.rs",
        "    libc::SYS_io_uring_setup,\n",
        "    libc::SYS_socket,\n",
        "a_contained_worker_reads_fonts_and_nothing_else",
        only_on="linux",
    ),
    Mutation(
        "linux boundary: let a call through by its x32 number",
        "src/sandbox_linux.rs",
        "        out.push(op(IF_AT_LEAST, X32_BIT, count + 1, 0));",
        "        let _ = (IF_AT_LEAST, X32_BIT);",
        "the_filter_refuses_a_call_by_its_x32_number",
        only_on="linux",
    ),
    Mutation(
        # Each refusal lands one instruction early, on the allow.
        "linux boundary: jump each listed call to the allow",
        "src/sandbox_linux.rs",
        "            out.push(op(IF_EQUAL, number, count - at as u8, 0));",
        "            out.push(op(IF_EQUAL, number, count - at as u8 - 1, 0));",
        "the_filter_refuses_each_listed_call_and_allows_the_rest",
        only_on="linux",
    ),
    Mutation(
        "linux boundary: say TCP is ruled on a kernel without the network rule",
        "src/sandbox_linux.rs",
        "    line.push_str(if abi >= 4 {",
        "    line.push_str(if abi >= 3 {",
        "the_boundary_line_names_what_the_kernel_lacks",
        only_on="linux",
    ),
    Mutation(
        # Every descriptor the app holds without close-on-exec reaches each
        # worker already open, past every rule of the boundary.
        "worker spawn: leave inherited descriptors open in the worker",
        "src/worker_handover.rs",
        "    unsafe { cloexec_the_rest(&keep[..mappings.len()], minimum - 1) };",
        "    let _ = (keep, minimum, cloexec_the_rest);",
        "a_descriptor_opened_without_close_on_exec_does_not_reach_the_child",
        only_on="macos",
    ),
    Mutation(
        "worker spawn: leave inherited descriptors open in the worker (Linux)",
        "src/worker_handover.rs",
        "    unsafe { cloexec_the_rest(&keep[..mappings.len()], minimum - 1) };",
        "    let _ = (keep, minimum, cloexec_the_rest);",
        "a_descriptor_opened_without_close_on_exec_does_not_reach_the_child",
        only_on="linux",
    ),
    Mutation(
        # Linux's one-call path, started too high: it then returns having
        # marked nothing the test opened.
        "worker spawn: mark descriptors from far too high a number on Linux",
        "src/worker_handover.rs",
        "            let first = libc::c_uint::try_from(highest + 1).unwrap_or(libc::c_uint::MAX);",
        "            let first = libc::c_uint::try_from(highest + 100_000).unwrap_or(libc::c_uint::MAX);",
        "a_descriptor_opened_without_close_on_exec_does_not_reach_the_child",
        only_on="linux",
    ),
    Mutation(
        "pin prompt: take Ctrl+C as a character of the PIN",
        "src/pin_prompt.rs",
        "            0x03 => return Ok(None),",
        "            0x03 => {}",
        "ctrl_c_cancels_the_terminal_prompt",
        only_on="linux",
    ),
    Mutation(
        "pin prompt: backspace deletes nothing",
        "src/pin_prompt.rs",
        "                len = len.saturating_sub(1);",
        "                len = len.saturating_sub(0);",
        "the_terminal_prompt_ends_at_enter_and_edits_with_backspace",
        only_on="linux",
    ),
    Mutation(
        "pin prompt: no bound on what is typed",
        "src/pin_prompt.rs",
        "                if len == MOST {",
        "                if len == MOST && false {",
        "the_terminal_prompt_refuses_more_than_it_holds",
        only_on="linux",
    ),
    Mutation(
        # The defect the refusal replaced: adding over a damaged list lost
        # every earlier entry.
        "certificate files: read a damaged list as empty and write over it",
        "src/certfiles.rs",
        "    let mut list = load(store)\n"
        '        .map_err(|why| format!("{why} Nothing was added; move it aside to start a new list."))?;',
        "    let mut list = load(store).unwrap_or_default();",
        "a_damaged_list_is_left_as_it_is_and_nothing_is_added",
        only_on="linux",
    ),
    Mutation(
        "certificate files: sign with a remembered file that now holds another certificate",
        "src/certfiles.rs",
        "    if opened.certificate != entry.certificate() {",
        "    if false {",
        "an_added_file_is_listed_reopened_and_refused_once_replaced",
        only_on="linux",
    ),
    Mutation(
        # Decrypt without checking the MAC: a wrong password then fails later,
        # or not at all, and is never named as a wrong password.
        "certificate file: skip the MAC check",
        "src/pkcs12.rs",
        "        check_mac(mac, &auth, password)?;",
        "        let _ = (mac, password);",
        "ecdsa_files_open_and_sign_and_the_rest_are_refused_by_name",
        only_on="linux",
    ),
    Mutation(
        # An RSA key refused without the sentence pointing at a smart card.
        "certificate file: refuse an RSA key as merely unsupported",
        "src/pkcs12.rs",
        "        RSA => return Err(Refusal::Rsa),",
        '        RSA => return Err(Refusal::Unsupported("an RSA key".into())),',
        "an_rsa_key_is_refused_and_says_what_can_sign_instead",
        only_on="linux",
    ),
    Mutation(
        # OpenSSL writes the matching certificate first, so taking the first
        # passes by accident; the test puts the root first.
        "certificate file: take the first certificate as the signer",
        "src/pkcs12.rs",
        "        .position(|der| public_key_of(der).as_deref() == Some(public.as_slice()))",
        "        .position(|_| true)",
        "the_signer_is_the_certificate_that_matches_the_key_and_the_rest_is_the_chain",
        only_on="linux",
    ),
    Mutation(
        "token: ask for the PIN while the token is logged in",
        "src/pkcs11.rs",
        "    if info.login_required() && !logged_in {",
        "    if info.login_required() {",
        "a_token_lists_its_certificates_and_signs_after_its_pin",
        only_on="linux",
    ),
    Mutation(
        "token: sign an RSA digest without its DigestInfo",
        "src/pkcs11.rs",
        "            let mut data = SHA256_DIGEST_INFO.to_vec();",
        "            let mut data = Vec::new();",
        "a_token_lists_its_certificates_and_signs_after_its_pin",
        only_on="linux",
    ),
    Mutation(
        "token: say a wrong PIN in the library's words",
        "src/pkcs11.rs",
        "        Error::Pkcs11(RvError::PinIncorrect, _) => format!(",
        "        Error::Pkcs11(RvError::PinIncorrect, _) if false => format!(",
        "a_refused_login_is_said_in_words_a_reader_can_act_on",
        only_on="linux",
    ),
    Mutation(
        "tesseract: read with another language's data when the asked one has none",
        "src/ocr_tesseract.rs",
        "    crate::ocr::first_offered(asked, offered)\n        .and_then(|tag| code(&tag))",
        "    crate::ocr::first_offered(asked, offered)\n        .or_else(|| offered.first().cloned())\n        .and_then(|tag| code(&tag))",
        "a_language_with_no_data_is_refused_rather_than_read_with_another",
        only_on="linux",
    ),
    Mutation(
        # GTK numbers pages from 0 and CUPS from 1: every page one early.
        "print: hand CUPS the dialog's page numbers as they are",
        "src/print_linux.rs",
        "            let low = i64::from(start.max(0)) + 1;",
        "            let low = i64::from(start.max(0));",
        "the_dialogs_ranges_become_cups_page_ranges",
        only_on="linux",
    ),
    Mutation(
        "print: give CUPS the ranges in the order the dialog listed them",
        "src/print_linux.rs",
        "    spans.sort_unstable();\n",
        "",
        "the_dialogs_ranges_become_cups_page_ranges",
        only_on="linux",
    ),
    Mutation(
        "installed font: take a file fc-list lists under another name",
        "src/sysfont.rs",
        "        (postscript == name && file.is_absolute()).then(",
        "        (!postscript.is_empty() && file.is_absolute()).then(",
        "an_fc_list_line_counts_only_for_the_exact_name",
        only_on="linux",
    ),
    Mutation(
        # A hyphen unescaped ends the name for fontconfig at the hyphen.
        "installed font: leave a hyphen unescaped in the fontconfig pattern",
        "src/sysfont.rs",
        "        if matches!(c, '\\\\' | '-' | ':' | ',' | '=') {",
        "        if matches!(c, '\\\\' | ':' | ',' | '=') {",
        "the_fontconfig_pattern_escapes_its_syntax",
        only_on="linux",
    ),
]

#: Redaction: the two predicates that could ship an unredacted file, and the
#: writer's own guards. The first two are the ones to read --- each of them
#: hands the reader a document that looks redacted and is not.
ROWS += [
    Mutation(
        # Call a plan that only redacts "the file". `is_identity` is what lets
        # the print path hand the original bytes over, so this prints a redacted
        # document with every word in it -- and nothing fails, because the file
        # it printed is a perfectly good file.
        "redact: call a plan that only redacts the file on disk",
        "src/edits.rs",
        "        self.marks.is_empty() && self.redactions.is_empty() && self.pages_are_the_file()",
        "        self.marks.is_empty() && self.pages_are_the_file()",
        "a_plan_that_only_redacts_is_neither_the_file_nor_an_append",
    ),
    Mutation(
        # A note edit that writes no body. The append still builds, the file is
        # still valid, and the comment still says what it said -- which is the
        # shape of failure a reader reports as "my edit did not save" and no
        # length or fingerprint check can see.
        "notes: build the append without writing the new body",
        "src/save/marks.rs",
        '    dictionary.set("Contents", text_string(&note.body));',
        '    let _ = &note.body;',
        "a_comment_out_of_the_file_is_overridden_by_its_object",
    ),
    Mutation(
        # `/M` left as the file had it. Every viewer shows that date, so the
        # reader's own words appear over somebody else's timestamp.
        "notes: leave the modification date the file had",
        "src/save/marks.rs",
        '    dictionary.set("M", text_string(&note.made));',
        '    let _ = &note.made;',
        "a_comment_out_of_the_file_is_overridden_by_its_object",
    ),
    Mutation(
        # The write skipped entirely. Aimed at the CALL rather than at the body,
        # because a guard is only covered when a mutation removes the call ---
        # `docs/TRAPS.md` has that one, from a deploy that locked an account.
        "notes: never call the note writer at all",
        "src/save.rs",
        "    write_note_edits(&mut incremental, &plan.notes)?;",
        "    let _ = &plan.notes;",
        "a_comment_out_of_the_file_is_overridden_by_its_object",
    ),
    Mutation(
        # The same omission on the other writer, and it is where the defect
        # actually was: a reader who edits a comment and then deletes a page
        # leaves the append path, and until this call existed the rewrite
        # dropped the edit and reported a successful save. Aimed at the CALL for
        # the reason the append's twin is.
        "notes: never call the note writer on the rewrite path",
        "src/save.rs",
        "    rewrite_note_edits(&mut doc, &plan.notes)?;",
        "    let _ = &plan.notes;",
        "a_rewrite_writes_the_new_body_over_the_old",
    ),
    Mutation(
        # The `/Subtype` guard, reached through the second caller. Without it a
        # plan naming a page object writes `/Contents` onto the page, where the
        # key is its content stream: the save reports success and the document is
        # destroyed. Shared between the two writers precisely so that adding a
        # caller could not lose it.
        "notes: let a note edit name something that is not an annotation",
        "src/save/marks.rs",
        MUT_SUBTYPE_GUARD,
        "    if false {",
        "a_rewrite_refuses_a_note_edit_that_names_a_page",
    ),
    Mutation(
        # The latest version kept rather than the first. Undo of the second
        # rewrite would then leave the second body in place, which is the one
        # thing the journal exists to make impossible -- and every other rewrite
        # test has one version per object, so this is the only one that can see
        # it.
        "rewrite: keep the first body an object was given, not the latest",
        "src/docmodel.rs",
        "                self.rewrites.insert(object, Rewritten { page, edit });",
        "                self.rewrites.entry(object).or_insert(Rewritten { page, edit });",
        "undoing_the_first_rewrite_leaves_the_model_with_nothing_to_say",
    ),
    Mutation(
        # A deleted page leaves its rewrites behind. The plan then carries an
        # edit for a comment the written file does not contain, and the reader is
        # told the save failed for a reason about a document they cannot see.
        "rewrite: leave a deleted page's rewrites in the model",
        "src/docmodel.rs",
        "                self.rewrites.retain(|_, held| held.page != page);",
        "                let _ = page;",
        "deleting_a_page_takes_the_rewrites_on_it_and_no_others",
    ),
    Mutation(
        # The page checked only on the replay path. The refusal still arrives,
        # from `Working::apply`, so nothing a reader sees changes -- and a body
        # has been issued into a table nothing can ever reach again.
        "rewrite: issue the body before the page is checked",
        "src/docmodel.rs",
        "        self.now.live(page)?;\n        // Before the body is issued, for the liveness check's reason above.\n        self.now.opened_files_page(page)?;\n        let edit = self.issue_rewrite(NoteEdit { body, made });",
        "        let edit = self.issue_rewrite(NoteEdit { body, made });",
        "a_rewrite_on_a_page_that_is_gone_says_which_of_the_two_it_is",
    ),
    Mutation(
        # Every rewrite reported for every page. The bodies are right and the
        # order is nonsense, which is what a walk over the map rather than over
        # the reading order would produce.
        "rewrite: report every rewrite against every page",
        "src/docmodel.rs",
        "                    .filter(move |(_, held)| held.page == *page)",
        "                    .filter(move |(_, _held)| true || *page == *page)",
        "rewrites_come_back_in_reading_order_and_by_object",
    ),
    Mutation(
        # A discarded redo tail keeps its bodies. Nothing about any document
        # changes, which is the whole reason `rewrite_bodies` exists to be read.
        "rewrite: keep the bodies of a discarded redo tail",
        "src/docmodel.rs",
        "                Command::Rewrite { edit, .. } => {\n                    self.rewrites.remove(&edit);\n                }",
        "                Command::Rewrite { .. } => {}",
        "a_discarded_rewrite_takes_its_body_with_it",
    ),
    Mutation(
        # A subset carries every rewrite, including ones on pages it did not
        # take. The extra edits land on objects the written file sweeps out, so
        # the reader pays for a save that changes nothing it could show them.
        "notes: carry every rewrite into a subset plan",
        "src/edits.rs",
        "        .filter(|(page, _, _)| kept.contains(page))",
        "        .filter(|(_page, _, _)| true)",
        "a_subset_takes_the_rewrites_on_the_pages_it_names_and_leaves_the_rest",
    ),
    Mutation(
        # Object 0 accepted. It is the head of the free list and can never be an
        # indirect object, so the model journals an edit that no writer can ever
        # apply -- and the reader has an undo step for it.
        "notes: accept a comment with no object of its own",
        "src/edits.rs",
        "        if object.0 == 0 {\n            return Err(\"that comment has no object to write over\".to_string());",
        "        if false {\n            return Err(\"that comment has no object to write over\".to_string());",
        "a_comment_with_no_object_of_its_own_is_refused_here_and_spends_nothing",
    ),
    Mutation(
        # Let a highlight answer a comment. `/IRT` on a text-markup annotation
        # is not a thread, it is a stray key -- and the mark would be written
        # with it, so every reader that threads would show a highlight nested
        # under a note.
        "reply: let a mark that is not a comment answer one",
        "src/docmodel.rs",
        "        if mark.reply_to.is_some() && mark.kind != MarkKind::Note {\n"
        "            return Err(Refusal::ReplyMismatch(mark.kind));\n"
        "        }",
        "",
        "only_a_comment_may_answer_a_comment",
    ),
    Mutation(
        # Drop the parent on the way into the model. The reply is made, it is
        # saved, and it answers nobody -- and the file is perfectly valid, so
        # nothing downstream complains.
        "reply: forget the comment a reply names",
        "src/edits.rs",
        "                    reply_to: want\n"
        "                        .reply_to\n"
        "                        .map(|(number, generation)| ObjectId::new(number, generation)),",
        "                    reply_to: None,",
        "a_reply_carries_the_comment_it_answers_all_the_way_to_the_plan",
    ),
    Mutation(
        # Drop the parent on the way out to the writer. The other end of the
        # same crossing, and its own mutation because a single test asserting
        # both would pass with either half working.
        "reply: forget the comment a reply names, on the way out",
        "src/edits.rs",
        "                    reply_to: body\n"
        "                        .reply_to\n"
        "                        .map(|object| (object.number(), object.generation())),",
        "                    reply_to: None,",
        "a_reply_carries_the_comment_it_answers_all_the_way_to_the_plan",
    ),
    Mutation(
        # Write no `/IRT` at all. The reply becomes an unrelated second note by
        # another author, which is exactly what `pdfium-render` not exposing
        # the key means nobody would notice through that reader.
        "reply: write a reply that names nothing",
        "src/save/marks.rs",
        '        dictionary.set("IRT", Object::Reference((number, generation)));',
        "",
        "a_reply_is_written_as_one_and_reads_back_as_one",
    ),
    Mutation(
        # Say the relationship is a group rather than a reply. A reader that
        # honours `/RT` then shows the two comments as one annotation instead of
        # as a thread, and `annots.rs` -- which never reads the key -- agrees
        # with itself either way.
        "reply: call the relationship a group",
        "src/save/marks.rs",
        '        dictionary.set("RT", Object::Name(b"R".to_vec()));',
        '        dictionary.set("RT", Object::Name(b"Group".to_vec()));',
        "a_reply_is_written_as_one_and_reads_back_as_one",
    ),
    Mutation(
        # Let a reply name anything at all. The guard the writer has that the
        # model cannot: a plan naming a page, a font or the catalog would thread
        # a reply onto it.
        "reply: let a reply name something that is not an annotation",
        "src/save/marks.rs",
        '            return Err("that comment is not an annotation".into());\n'
        "        }\n"
        "    }\n"
        "    Ok(RepliesChecked)",
        "        }\n    }\n    Ok(RepliesChecked)",
        "a_reply_naming_something_that_is_not_an_annotation_is_refused_on_both_paths",
    ),
    Mutation(
        # Check the replies after the page surgery instead of before it. The
        # ordering finding: `materialise` unlinks a dropped page's annotations
        # and `sweep::collect` deletes them, so the refusal stops being about
        # the plan and becomes a true sentence about the file being built.
        "reply: check a reply's parent after the pages have been removed",
        "src/save.rs",
        "    let replies = check_replies(&doc, &plan.marks)?;",
        "    let replies = check_replies(&doc, &[])?;",
        "a_reply_naming_something_that_is_not_an_annotation_is_refused_on_both_paths",
    ),
    Mutation(
        # The mirror, one predicate over: route a redaction to the append. An
        # update section adds objects and never touches a content stream, so the
        # file is written, is bigger, and has nothing taken out of it.
        "redact: let a plan carrying a redaction be appended",
        "src/edits.rs",
        MUT_APPENDABLE,
        "        (!self.marks.is_empty() || !self.notes.is_empty())\n            && self.discards.is_empty()\n            && self.pages_are_the_file()",
        "a_plan_that_only_redacts_is_neither_the_file_nor_an_append",
    ),
    Mutation(
        # Remove from a page named twice. The second call runs against a stream
        # the first already changed, so its ordinals name different operators.
        "redact: remove twice from a page the plan names twice",
        "src/save.rs",
        "        if seen.contains(&redaction.source) {",
        "        if false {",
        "a_page_named_twice_by_the_redaction_plan_is_refused",
    ),
    Mutation(
        # Take a page the plan does not keep as the last one it does. Removing
        # text from a page nobody marked is the confident wrong answer.
        "redact: clamp a redaction naming a page past the end",
        "src/save.rs",
        "        let Some(page) = pages.get(redaction.source as usize) else {",
        "        let Some(page) = pages.get((redaction.source as usize).min(pages.len().saturating_sub(1))) else {",
        "a_redaction_naming_a_page_that_is_not_kept_is_refused",
    ),
    Mutation(
        # Act on each entry as it is checked rather than checking them all
        # first. A refusal half way through then leaves a document with some
        # pages redacted and some not, and the caller serialises it.
        "redact: act on each redaction as it is validated",
        "src/save.rs",
        "        targets.push((*page, redaction));",
        "        let took = crate::redact::remove_shows(doc, *page, &redaction.shows, redaction.text_objects).map_err(Refusal::from)?;\n        let _ = took;\n        targets.push((*page, redaction));",
        "a_page_named_twice_by_the_redaction_plan_is_refused",
    ),
]

#: The OCR gate's control chooser. Every one of these hands the gate a control
#: that proves less than it claims, and the verdict it produces is *Illegible* ---
#: which is the one verdict `docs/PLAN.md` §6 lets a caller present as clean.
ROWS += [
    Mutation(
        # Choose the control from the words the region covered. Those are the
        # words the removal was supposed to take, so reading one back proves the
        # removal failed -- and the gate reads it as proof the engine can see.
        "control: choose from the words the region covered",
        "src/ocr.rs",
        "            !regions\n                .iter()",
        "            regions\n                .iter()",
        "a_word_the_region_covers_is_never_the_control",
    ),
    Mutation(
        # Size the control against the largest covered box instead of the
        # smallest. A control in 12 pt proves an engine reads 12 pt and says
        # nothing about the 6 pt footnote in the same region.
        "control: size against the largest box the region covered",
        "src/ocr.rs",
        "        .fold(f32::INFINITY, f32::min);",
        "        .fold(f32::NEG_INFINITY, f32::max);",
        "control_is_sized_to_the_smallest_redacted_box_not_the_largest",
    ),
    Mutation(
        # Let any surviving word be the control however large it is set. The
        # heading at the top of the page then certifies a page of small print.
        "control: accept a word of any size",
        "src/ocr.rs",
        "        .filter(|word| word.rect[3] - word.rect[1] <= size_pt + CONTROL_HEIGHT_SLACK_PT)",
        "        .filter(|word| word.rect[3] - word.rect[1] >= 0.0 || size_pt >= 0.0)",
        "a_word_set_larger_than_what_went_is_refused",
    ),
    Mutation(
        # Widen the slack until a word a whole point taller passes. The rule
        # survives and the number stops meaning anything, which is the shape a
        # test written in the constant's own units cannot see -- so the test
        # aimed at this one carries a measured height instead.
        "control: widen the height slack to a whole point",
        "src/ocr.rs",
        "const CONTROL_HEIGHT_SLACK_PT: f32 = 0.01;",
        "const CONTROL_HEIGHT_SLACK_PT: f32 = 1.0;",
        "a_word_one_point_taller_than_what_went_does_not",
    ),
    Mutation(
        # Accept a control of any length. A two-character fragment is what an
        # engine emits from noise, and one matching by accident certifies a page
        # nothing was read on.
        "control: accept a token of any length",
        "src/ocr.rs",
        "        if chars < MIN_CONTROL_CHARS {",
        "        if false {",
        "a_word_too_short_to_be_a_control_is_refused",
    ),
    Mutation(
        # Break a tie by the bottom-most word. Nothing is unsafe and the page
        # stops yielding the same control twice, so every test that says WHICH
        # control a page gives becomes a statement about iteration order.
        "control: break a tie by the bottom-most word",
        "src/ocr.rs",
        "                        && (word.rect[1] < have.rect[1]",
        "                        && (word.rect[1] > have.rect[1]",
        "a_tie_goes_to_the_topmost_and_then_the_leftmost",
    ),
    Mutation(
        # Take the whole line as the token. `adjudicate` asks whether ONE
        # recognised span contains it, so a 52-character line is read back only
        # when the engine happens to return that line in one piece -- measured
        # failing on text-base14 and passing on text-marked, i.e. a gate that
        # refuses a correct redaction depending on the font.
        "control: take the whole line as the control token",
        "src/ocr.rs",
        "        token: longest_run(&chosen.text).to_string(),",
        "        token: chosen.text.trim().to_string(),",
        "the_token_is_a_word_and_not_the_line_it_sits_on",
    ),
    Mutation(
        # Rank candidates by the length of the whole line rather than by its
        # longest word. A line of six three-letter words then beats a single
        # readable one, and the control it yields is a fragment.
        "control: rank by the whole line rather than its longest word",
        "src/ocr.rs",
        "        let chars = longest_run(&word.text).chars().count();",
        "        let chars = word.text.trim().chars().count();",
        "a_line_of_short_words_does_not_outrank_one_long_one",
    ),
    Mutation(
        # Take the band from the crop. The crop is on the page and the band is
        # in the probe image, so every recognised item falls outside it, the
        # control is never seen and a working gate refuses everything.
        "control: place the band where the crop is",
        "src/ocr.rs",
        "            size_pt: self.size_pt,\n            band,",
        "            size_pt: self.size_pt,\n            band: self.crop,",
        "placed_takes_the_band_from_its_caller_and_nothing_else",
    ),
]

#: The evidence an unread control carries. None of these is unsafe -- every one
#: leaves the verdict exactly as it was -- and that is the reason they are here:
#: a measurement nobody can falsify is one that gets quoted, and `docs/PLAN.md`
#: §6 has already had one increment ranked off a bucket with no denominator.
ROWS += [
    Mutation(
        # Report the token as read nowhere. The band hypothesis then measures
        # zero on every corpus, which is the reassuring answer and the one that
        # closes the question by hiding it.
        "unread: report a token read outside the band as absent",
        "src/ocr.rs",
        "            .map(|i| control.overshoot(&i.rect))\n"
        "            .min_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])));",
        "            .map(|i| control.overshoot(&i.rect))\n"
        "            .min_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])))\n"
        "            .filter(|_| false);",
        "a_control_read_outside_its_band_is_recorded_as_outside_and_by_how_far",
    ),
    Mutation(
        # Report the furthest reading rather than the nearest. The count is
        # unchanged and the distance -- which is what says whether the repair is
        # a tolerance or something larger -- becomes the worst span in the image.
        "unread: report the furthest reading of the token, not the nearest",
        "src/ocr.rs",
        "            .min_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])));",
        "            .max_by(|a, b| (a[0] + a[1]).total_cmp(&(b[0] + b[1])));",
        "the_nearest_reading_of_the_token_is_the_one_reported",
    ),
    Mutation(
        # Count every span as in-band. `items == 0` is what separates an engine
        # that read nothing from one that read the wrong things, and this leaves
        # the two counters equal on every refusal.
        "unread: count every span the engine returned as in-band",
        "src/ocr.rs",
        "                in_band: in_band.len(),",
        "                in_band: items.len(),",
        "an_engine_that_read_other_words_says_the_token_was_absent",
    ),
    Mutation(
        # Report the probe image with its two dimensions the wrong way round.
        # Every aspect then inverts, so a page-wide short image reads as a tall
        # narrow one and the band the measurement is about lands nowhere near
        # where it should. Nothing about the gate changes -- this is a reading
        # only, which is why it needs a test of its own.
        "geometry: report the probe image's shape transposed",
        "src/ocr_gate.rs",
        "        image_pt: (page.width_pt, height_pt),",
        "        image_pt: (height_pt, page.width_pt),",
        "the_reported_shape_is_the_page_wide_and_the_two_strips_tall",
    ),
    Mutation(
        # Leave out what `stack` adds. The image is then reported shorter than
        # it is and every aspect comes out wider, which biases the one axis the
        # 2026-08-28 measurement rests on toward its own conclusion.
        "geometry: report the probe image without the margins and the gap",
        "src/ocr_gate.rs",
        "    let height_pt = tallest + control_pt + padding;",
        "    let height_pt = tallest + control_pt;",
        "the_reported_shape_is_the_page_wide_and_the_two_strips_tall",
    ),
    Mutation(
        # Write the overshoot with `f32::max`, which is the obvious spelling and
        # returns the OTHER operand for a NaN -- so a rect with no centre reads
        # as inside the band, is not counted as a survivor, and can certify.
        # The only one of these five that is unsafe.
        "unread: compute the overshoot with a max that swallows a NaN",
        "src/ocr.rs",
        "        let axis = |c: f32, lo: f32, hi: f32| {\n"
        "            if c >= lo && c <= hi {\n"
        "                0.0\n"
        "            } else if c < lo {\n"
        "                lo - c\n"
        "            } else {\n"
        "                c - hi\n"
        "            }\n"
        "        };",
        "        let axis = |c: f32, lo: f32, hi: f32| (lo - c).max(c - hi).max(0.0);",
        "a_centre_that_is_not_a_number_is_outside_the_band",
    ),
]

#: A region over a picture. Two removals, and only the second one redacts:
#: deleting the `Do` stops the page drawing the image, and dropping the resource
#: entry is what leaves the stream unreachable so the sweep takes its bytes.
ROWS += [
    Mutation(
        # Report a picture as unremovable again. Every region over one is then
        # refused, which is the behaviour that made a redaction over a scanned
        # page remove nothing.
        "image: refuse a picture instead of removing it",
        "src/redact.rs",
        "        } else if object.kind == \"image\" {\n            plan.images.push(image_ordinal);",
        "        } else if false {\n            plan.images.push(image_ordinal);",
        "a_region_over_an_image_names_it_rather_than_refusing",
    ),
    Mutation(
        # Advance the image ordinal only for the images a region covers. The
        # second picture on a page is then named as the first, and the removal
        # takes the wrong one.
        "image: number the pictures a region covers rather than the page's",
        "src/redact.rs",
        "        let image_ordinal = images;\n        if object.kind == \"image\" {\n            images += 1;\n        }",
        "        let image_ordinal = images;\n        if object.kind == \"image\" && overlaps(object.bounds, region) {\n            images += 1;\n        }",
        "an_image_ordinal_counts_images_and_not_every_object",
    ),
    Mutation(
        # Leave the resource entry. The page stops drawing the picture and every
        # byte of it stays in the file, reachable from the page -- a redaction
        # that removed the picture from view and nothing else.
        "image: stop drawing the picture and leave it in the file",
        "src/redact.rs",
        "    forget_xobjects(doc, page, &unused)?;",
        "    let _ = &unused;",
        "removing_an_image_takes_its_draw_and_its_resource_entry",
    ),
    Mutation(
        # Drop every image the page draws rather than the ones named. The check
        # above passes on it; only the control tells them apart.
        "image: drop every picture on the page",
        "src/redact.rs",
        "    let mut positions: Vec<usize> = Vec::with_capacity(ordinals.len());\n    let mut names: Vec<String> = Vec::new();\n    for &ordinal in ordinals {",
        "    let mut positions: Vec<usize> = Vec::with_capacity(ordinals.len());\n    let mut names: Vec<String> = Vec::new();\n    for &ordinal in &(0..drawn.len()).collect::<Vec<_>>() {",
        "removing_one_image_leaves_the_other_drawn_and_listed",
    ),
    Mutation(
        # Drop the correspondence guard. Nothing connects PDFium's image objects
        # to the page's `Do` operations but order.
        "image: remove whichever picture is at that position",
        "src/redact.rs",
        "    if drawn.len() != image_objects {",
        "    if false {",
        "an_image_count_that_disagrees_with_pdfium_removes_nothing",
    ),
    Mutation(
        # Sweep only for the carriers that were already listed. An image removal
        # then unlinks the stream and the rewrite writes it out anyway, which is
        # the defect `redact-apply-probe` found by grepping pixels.
        "image: leave the unlinked picture for the writer to emit",
        "src/save.rs",
        "        || (!matches!(job, Job::RedactionFill { .. }) \u0026\u0026 !plan.redactions.is_empty())\n",
        "",
        "a_rewrite_that_removed_a_picture_sweeps_it_out_of_the_file",
    ),
    Mutation(
        # The same clause, for the carrier it was written for: text taken off a
        # page of several streams leaves the old streams named by nothing, and
        # without the sweep every removed glyph is written out beside a page
        # that no longer draws it.
        "redact: leave the streams a page of several was taken off in the file",
        "src/save.rs",
        "        || (!matches!(job, Job::RedactionFill { .. }) \u0026\u0026 !plan.redactions.is_empty())\n",
        "",
        "a_text_redaction_on_a_page_of_several_streams_leaves_none_of_them",
    ),
    Mutation(
        # Point the page at one of the streams it had instead of the one that
        # was written. The read back is what refuses: the page does not say
        # what the removal wrote, so nothing may be reported removed.
        "redact: leave a page of several streams naming one of the old ones",
        "src/redact.rs",
        "                .set(\"Contents\", fresh);\n",
        "                .set(\"Contents\", streams[0]);\n",
        "a_page_whose_contents_is_a_reference_to_an_array_loses_the_line_too",
    ),
    Mutation(
        # Read a stream as far as it parses and write that back, which is what
        # every removal did: whatever follows the first unreadable token leaves
        # the page with the redaction.
        "redact: write back a content stream that was read in part",
        "src/redact.rs",
        "    Content::decode_strict(data).map_err(|_| {",
        "    Content::decode(data).map_err(|_| {",
        "a_content_stream_that_does_not_parse_to_its_end_is_refused",
    ),
    Mutation(
        # Compare a needle with the bytes a string is stored as and nothing
        # else. A note quoting the removed line in UTF-16BE then reads as clean.
        "verify: compare a needle with a text string's stored bytes only",
        "src/verify.rs",
        "            if !reads_as_stored(bytes) {",
        "            if false {",
        "a_needle_in_a_utf16_text_string_is_found",
    ),
    Mutation(
        # Take a byte above the ASCII range for the character it is not. A name
        # with an umlaut in it, stored in PDFDocEncoding, is then never decoded
        # and never matches its UTF-8 needle.
        "verify: take a PDFDocEncoded string for the text it holds",
        "src/verify.rs",
        "        .all(|byte| matches!(byte, 0x20..=0x7E | b'\\t' | b'\\n' | b'\\r'))",
        "        .all(|byte| matches!(byte, 0x20..=0xFD | b'\\t' | b'\\n' | b'\\r'))",
        "a_needle_outside_ascii_in_a_pdfdocencoded_string_is_found",
    ),
    Mutation(
        # Scale a deflated picture whatever its parameters look like. An array
        # of one, which `lopdf` does not read, leaves a TIFF predictor in the
        # samples, and the picture stored is not the one the page drew.
        "compress: scale a picture whose predictor was not undone",
        "src/compress.rs",
        "    if stored == Stored::Flate \u0026\u0026 !parms_are_read(dict) {",
        "    if false {",
        "a_deflated_picture_whose_predictor_would_not_be_undone_is_left",
    ),
    Mutation(
        # Believe any number in the parameters, written in place or not.
        "compress: read a predictor given by reference as none",
        "src/compress.rs",
        "            .all(|key| matches!(parms.get(key), Err(_) | Ok(Object::Integer(_)))),",
        "            .all(|_| true),",
        "a_deflated_picture_whose_predictor_would_not_be_undone_is_left",
    ),
    Mutation(
        # Hand the fill pass the field changes the removal pass already made.
        # A removed field is then looked for in a file that no longer has it,
        # and the fill is refused after the content is gone.
        "redaction_fill: change the form's fields again in the fill pass",
        "src/redaction_fill.rs",
        "        field_edits: Vec::new(),\n        new_fields: Vec::new(),\n        // Sorted",
        "        field_edits: original.field_edits.clone(),\n        new_fields: Vec::new(),\n        // Sorted",
        "a_redaction_beside_a_field_change_gets_its_black_fill",
    ),
    Mutation(
        # And the fields it added, which the file it wrote already holds.
        "redaction_fill: add the new fields again in the fill pass",
        "src/redaction_fill.rs",
        "        field_edits: Vec::new(),\n        new_fields: Vec::new(),\n        // Sorted",
        "        field_edits: Vec::new(),\n        new_fields: original.new_fields.clone(),\n        // Sorted",
        "the_fill_plan_carries_no_field_changes",
    ),
    Mutation(
        # Read the page even when no picture was marked. The correspondence
        # guard then refuses a call that was removing nothing, so an ordinary
        # text redaction on a page whose image count disagrees fails for a
        # removal it was not making.
        "image: read the page even when no picture was marked",
        "src/redact.rs",
        "    if ordinals.is_empty() {\n        return Ok(Removed {",
        "    if false {\n        return Ok(Removed {",
        "a_disagreeing_image_count_refuses_only_a_real_removal",
    ),
]

#: Text inside a Form XObject: reaching it, placing it, and refusing to touch a
#: form the document draws more than once. PDFium enumerates a form as ONE page
#: object, so every one of these is about a level `remove_shows` cannot see.
ROWS += [
    Mutation(
        # Leave a form child's box in the form's own space. Measured on
        # `form-xobject.pdf`: a form placed at (60, 600) reports a child at
        # (0.9, 19.9), so the region covers nothing, the removal takes nothing,
        # and the save reports success.
        "form: leave a form's text in the form's own space",
        "src/objects.rs",
        "    let [a, b, c, d, e, f] = m;",
        "    let [a, b, c, d, e, f] = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];\n    let _ = m;",
        "a_translated_form_moves_its_text_by_exactly_the_translation",
    ),
    Mutation(
        # Take the transformed diagonal instead of bounding all four corners. A
        # quarter turn then gives a rectangle whose left is greater than its
        # right, which overlaps nothing at all.
        #
        # **Aimed at the fold rather than at the corner list, because the obvious
        # mutation is a no-op.** Duplicating two of the four corners still leaves
        # the extremes in `xs` and `ys`, and `min`/`max` over a list containing
        # them recovers the same rectangle -- so that edit landed, built, and
        # changed nothing. It is the bounding that has to go.
        "form: place a turned form from its diagonal",
        "src/objects.rs",
        "    [\n        xs.iter().copied().fold(f32::INFINITY, f32::min),\n        ys.iter().copied().fold(f32::INFINITY, f32::min),\n        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),\n        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),\n    ]",
        "    [xs[0], ys[0], xs[3], ys[3]]",
        "a_rotated_form_needs_all_four_corners",
    ),
    Mutation(
        # Put the unmeasurable box through the matrix. It becomes infinite rather
        # than everything, and an object PDFium would not place stops being
        # reported for every region -- which is a removal stepping over what it
        # could not see.
        "form: transform a child PDFium would not measure",
        "src/objects.rs",
        "    if rect == UNMEASURABLE {\n        return UNMEASURABLE;\n    }",
        "    if false {\n        return UNMEASURABLE;\n    }",
        "an_unmeasurable_child_stays_unmeasurable",
    ),
    Mutation(
        # The copy that was to open for anybody still asks for the password.
        'protect: a removal keeps the encryption',
        'src/protect.rs',
        '        Protection::Remove => Ok(None),',
        '        Protection::Remove => Ok(source),',
        'keep_and_remove_resolve_without_a_key',
    ),
    Mutation(
        # Somebody else's restrictions, dropped for a reader who was never asked for a password.
        'protect: remove restrictions from a document that opens unasked',
        'src/protect.rs',
        '            if plain.is_ok_and(|doc| doc.is_encrypted()) {',
        '            if true {',
        'a_removal_needs_a_document_that_a_password_opens',
    ),
    Mutation(
        # The wrong sentence: a document with no encryption has no restrictions either.
        'protect: tell a plain document it opens without a password',
        'src/protect.rs',
        '        Protection::Remove if !had => {',
        '        Protection::Remove if false => {',
        'a_removal_needs_a_document_that_a_password_opens',
    ),
    Mutation(
        # Off by one at the limit revision 6 reads.
        'protect: refuse a password of exactly the longest length',
        'src/protect.rs',
        '    if password.len() > MAX_BYTES {',
        '    if password.len() >= MAX_BYTES {',
        'a_password_nobody_could_type_back_is_refused',
    ),
    Mutation(
        # A tab pasted into the field is a password nobody can type into another reader.
        'protect: accept a control character',
        'src/protect.rs',
        '    if password.chars().any(char::is_control) {',
        '    if false {',
        'a_password_nobody_could_type_back_is_refused',
    ),
    Mutation(
        # An empty user password opens for everybody: a protected copy that is not.
        'protect: accept an empty password',
        'src/protect.rs',
        '    if password.is_empty() {',
        '    if false {',
        'a_password_nobody_could_type_back_is_refused',
    ),
    Mutation(
        # The key stays all zeroes, so every protected copy is encrypted with the same one.
        'protect: one file key for every copy',
        'src/protect.rs',
        '    getrandom::fill(&mut key)\n        .map_err(|e| format!("tpdf could not get random bytes for the encryption key: {e}"))?;',
        '',
        'two_copies_get_two_keys',
    ),
    Mutation(
        # A copy written in the clear opens with any password, the new one included.
        'protect: do not ask whether the copy opens unasked',
        'src/protect.rs',
        '            if !load(None)?.is_encrypted() {',
        '            if false {',
        'a_copy_written_in_the_clear_is_not_one_with_a_password',
    ),
    Mutation(
        # A copy that opens and is missing a page is not the document.
        "protect: do not count the protected copy's pages",
        'src/protect.rs',
        '                .is_ok_and(|doc| !doc.is_encrypted() && doc.get_pages().len() == pages);',
        '                .is_ok_and(|doc| !doc.is_encrypted());',
        'a_copy_that_lost_a_page_is_refused',
    ),
    Mutation(
        # The same, for the copy with no password.
        "protect: do not count the unprotected copy's pages",
        'src/protect.rs',
        '            if plain.is_encrypted() || plain.was_encrypted() || plain.get_pages().len() != pages {',
        '            if plain.is_encrypted() || plain.was_encrypted() {',
        'a_copy_that_lost_a_page_is_refused',
    ),
    Mutation(
        # What lopdf writes alone: Preview takes the password and shows blank pages.
        'protect: leave the key length out of the crypt filter',
        'src/protect.rs',
        '                Ok(b"AESV3") => 32,',
        '                Ok(b"AESV3") => continue,',
        'a_new_password_states_its_key_length_and_identifies_the_file',
    ),
    Mutation(
        # qpdf warns about an encrypted file with no /ID.
        'protect: write no file identifier',
        'src/protect.rs',
        '    if !identified {',
        '    if false {',
        'a_new_password_states_its_key_length_and_identifies_the_file',
    ),
    Mutation(
        # The identifier is the file's, and other tools match revisions by it.
        'protect: replace the identifier a file has',
        'src/protect.rs',
        '    if !identified {',
        '    if true {',
        'a_new_password_states_its_key_length_and_identifies_the_file',
    ),
    Mutation(
        # A PDF 1.4 header on a file whose handler arrived with 1.7.
        'protect: leave an old header on a revision 6 file',
        'src/protect.rs',
        '    if doc.version.as_str() < "1.7" {',
        '    if false {',
        'a_new_password_states_its_key_length_and_identifies_the_file',
    ),
    Mutation(
        # A save that keeps the password must change nothing else about the file.
        'protect: give kept encryption a new header and identifier',
        'src/protect.rs',
        '    if !matches!(protection, Protection::Set(_)) {\n        return Ok(());\n    }\n    if doc.version',
        '    if doc.version',
        'a_new_password_states_its_key_length_and_identifies_the_file',
    ),
    Mutation(
        # A plan is printed by {:?} in refusals and test output.
        'protect: print the password in the debug form',
        'src/protect.rs',
        '            Protection::Set(_) => "Set(<password>)",',
        '            Protection::Set(password) => password,',
        'the_password_is_not_in_the_debug_form',
    ),
    Mutation(
        # The guard's call, rather than the guard.
        'save: do not ask whether a removal is allowed',
        'src/save.rs',
        '    crate::protect::allowed(&plan.protection, encryption.is_some(), original)?;\n',
        '',
        'a_password_is_not_removed_from_a_document_that_has_none_to_ask_for',
    ),
    Mutation(
        # The copy keeps what the source had, whatever was asked.
        'save: ignore the password the plan asks for',
        'src/save.rs',
        '    let encryption = crate::protect::resolve(&plan.protection, encryption)?;\n',
        '',
        'a_copy_without_the_password_opens_for_anybody',
    ),
    Mutation(
        # The call in the rewrite: shipped without it for five weeks.
        'save: do not complete the encryption a rewrite writes',
        'src/save.rs',
        '    crate::protect::finish(&mut doc, &plan.protection)?;\n',
        '',
        'a_rewrite_of_an_encrypted_document_stays_encrypted',
    ),
    Mutation(
        # The same call, in the second of the three places that encrypt.
        'save: do not complete the encryption a merge writes',
        'src/save.rs',
        '    crate::protect::finish(&mut merged, &plan.protection)?;\n',
        '',
        'a_merge_whose_base_is_password_protected_keeps_its_encryption',
    ),
    Mutation(
        # An identity plan is handed over byte for byte, with the old password.
        'plan: a password change is still the file',
        'src/edits.rs',
        '        if self.protection != crate::protect::Protection::Keep {\n            return false;\n        }\n',
        '',
        'a_plan_that_changes_the_password_is_neither_the_file_nor_an_append',
    ),
    Mutation(
        # An append writes under the encryption the file has.
        'plan: a password change can be an append',
        'src/edits.rs',
        '            && self.protection == crate::protect::Protection::Keep\n',
        '',
        'a_plan_that_changes_the_password_is_neither_the_file_nor_an_append',
    ),
    Mutation(
        # An option that would be read and then ignored.
        'cli protect: unprotect takes a new password',
        'src/cli/protect.rs',
        '            "--new-password-env" if set => {',
        '            "--new-password-env" => {',
        'unprotect_needs_the_password_and_takes_no_new_one',
    ),
    Mutation(
        # It would run as a removal.
        'cli protect: protect without a new password',
        'src/cli/protect.rs',
        '    if set && command.new_password_env.is_none() {',
        '    if false {',
        'protect_needs_a_variable_for_the_new_password',
    ),
    Mutation(
        # Refused late, by the worker, in a sentence about a locked document.
        'cli protect: unprotect without the password',
        'src/cli/protect.rs',
        '    if !set && command.password_env.is_none() {',
        '    if false {',
        'unprotect_needs_the_password_and_takes_no_new_one',
    ),
    Mutation(
        # The sentence a reader acts on when they send the copy to somebody.
        'cli protect: say the old password still opens it',
        'src/cli/protect.rs',
        '        (true, false) => format!(',
        '        (true, _) => format!(',
        'the_sentences_say_which_password_opens_the_copy',
    ),
    Mutation(
        # The window's check, before a worker is started.
        'window protect: read an empty password as acceptable',
        'src/commands/protect.rs',
        '            protect::acceptable(&new)?;\n',
        '',
        'an_empty_password_is_refused_rather_than_read_as_a_removal',
    ),
    Mutation(
        # The reader typed a password twice and got a copy with none.
        'window protect: remove a password when one was given',
        'src/commands/protect.rs',
        '            Protection::Set(new)\n',
        '            let _ = new;\n            Protection::Remove\n',
        'a_password_is_set_and_none_removes_it',
    ),
    Mutation(
        # The page keeps the stored shape, so a portrait photograph gets a landscape page.
        'images: show a turned photograph with its stored sides',
        'src/imagepages.rs',
        '    let turned = orientation >= 5;',
        '    let turned = false;',
        'every_orientation_puts_the_stored_corners_where_exif_says',
    ),
    Mutation(
        # Orientation 6 drawn as 8: every phone photograph upside down from the other.
        'images: turn a photograph the wrong way',
        'src/imagepages.rs',
        '        6 => [0.0, -h, w, 0.0, x, y + h],',
        '        6 => [0.0, h, -w, 0.0, x + w, y],',
        'every_orientation_puts_the_stored_corners_where_exif_says',
    ),
    Mutation(
        # A mirrored orientation drawn as stored.
        'images: do not mirror',
        'src/imagepages.rs',
        '        2 => [-w, 0.0, 0.0, h, x + w, y],',
        '        2 => [w, 0.0, 0.0, h, x, y],',
        'every_orientation_puts_the_stored_corners_where_exif_says',
    ),
    Mutation(
        # The documented default is one point a pixel.
        'images: take a silent picture at 96 DPI',
        'src/imagepages.rs',
        'pub const DEFAULT_DPI: f64 = 72.0;',
        'pub const DEFAULT_DPI: f64 = 96.0;',
        'a_page_is_the_pictures_size_at_the_resolution_its_file_states',
    ),
    Mutation(
        # A 300 DPI scan of A4 becomes a page a metre tall.
        'images: ignore the resolution a file states',
        'src/imagepages.rs',
        '        None => stated.unwrap_or((DEFAULT_DPI, DEFAULT_DPI)),',
        '        None => (DEFAULT_DPI, DEFAULT_DPI),',
        'a_page_is_the_pictures_size_at_the_resolution_its_file_states',
    ),
    Mutation(
        # Down to fit, never up.
        'images: enlarge a small picture to fill the paper',
        'src/imagepages.rs',
        '    let scale = (page.0 / w).min(page.1 / h).min(1.0);',
        '    let scale = (page.0 / w).min(page.1 / h);',
        'paper_is_turned_to_the_picture_and_the_picture_is_never_enlarged',
    ),
    Mutation(
        # A landscape picture shrunk onto a portrait page.
        'images: keep the paper upright for a landscape picture',
        'src/imagepages.rs',
        '        Some((short, long)) if w > h => (long, short),',
        '        Some((short, long)) if false => (long, short),',
        'paper_is_turned_to_the_picture_and_the_picture_is_never_enlarged',
    ),
    Mutation(
        # 14,400 points is the format's limit; readers clip or refuse beyond it.
        'images: make a page longer than the format allows',
        'src/imagepages.rs',
        '            let fit = (MAX_PAGE_PT / w.max(h)).min(1.0);',
        '            let fit = 1.0_f64;',
        'a_page_stays_within_what_a_page_may_be',
    ),
    Mutation(
        # --dpi 0 divides by zero into an infinite page.
        'images: accept any resolution',
        'src/imagepages.rs',
        '            Some(dpi) if !PLAUSIBLE_DPI.contains(&f64::from(dpi)) => Err(format!(',
        '            Some(dpi) if false => Err(format!(',
        'a_resolution_nobody_prints_at_is_refused',
    ),
    Mutation(
        # Half of all cameras write Motorola order.
        'images: read big-endian EXIF as little-endian',
        'src/imagepages.rs',
        "        [b'M', b'M', 0, 42] => false,",
        "        [b'M', b'M', 0, 42] => true,",
        'a_jpegs_orientation_is_read_in_either_byte_order',
    ),
    Mutation(
        # A value outside the table would fall through to the matrix of 1 with the sides of 5 to 8.
        'images: take any number as an orientation',
        'src/imagepages.rs',
        '            return u8::try_from(value).ok().filter(|v| (1..=8).contains(v));',
        '            return u8::try_from(value).ok();',
        'a_jpegs_orientation_is_read_in_either_byte_order',
    ),
    Mutation(
        # A page 2.54 times too large.
        'images: read dots per centimetre as dots per inch',
        'src/imagepages.rs',
        '                    2 => plausible(x * 2.54, y * 2.54),',
        '                    2 => plausible(x, y),',
        'a_jpegs_stated_resolution_is_used_when_it_is_plausible',
    ),
    Mutation(
        # A file cut short becomes a page that shows half a picture.
        'images: embed a JPEG without proving it decodes',
        'src/imagepages.rs',
        '    decoder.decode_into(&mut decoded).map_err(undecodable)?;\n',
        '',
        'a_jpeg_tpdf_cannot_place_is_refused_in_words_about_it',
    ),
    Mutation(
        # An alpha channel that hides nothing costs a second image per page.
        'images: give every transparent PNG a mask',
        'src/imagepages.rs',
        '        (samples, shows_through.then_some(opacity))',
        '        (samples, Some(opacity))',
        'a_pngs_transparency_becomes_a_mask_only_when_something_shows_through',
    ),
    Mutation(
        # The refusal must come from the header, before any room is given.
        'images: let the PNG decoder find out how large it is',
        'src/imagepages.rs',
        '    bounded(width, height)?;\n    let pixels = width as usize * height as usize;',
        '    let pixels = width as usize * height as usize;',
        'a_picture_too_large_is_refused_from_its_header',
    ),
    Mutation(
        # 30,000 by 30,000 is 900 megapixels.
        'images: bound the sides and not the area',
        'src/imagepages.rs',
        '    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {',
        '    if width > MAX_SIDE || height > MAX_SIDE {',
        'a_picture_too_large_is_refused_from_its_header',
    ),
    Mutation(
        # The count is the bound on how long one request runs.
        'images: take any number of pictures',
        'src/imagepages.rs',
        '    if images.len() > MAX_IMAGES {',
        '    if false {',
        'a_picture_that_cannot_be_used_is_named_and_stops_the_document',
    ),
    Mutation(
        # The picture is replaced by a document that contains it.
        'images: write the document over one of its pictures',
        'src/save.rs',
        '    if let Some(same) = images.iter().find(|image| same_file(image, out)) {',
        '    if let Some(same) = images.iter().find(|_| false) {',
        'a_document_from_pictures_is_not_written_over_one_of_them_or_from_none',
    ),
    Mutation(
        # landed_is on the fourth caller.
        'images: trust the length the writer reports',
        'src/save.rs',
        '        landed_is(writing, wrote)?;\n        pages = made;',
        '        pages = made;',
        'a_picture_writer_that_overstates_what_it_wrote_is_refused',
    ),
    Mutation(
        # A picture named --force.png cannot be given.
        'cli images: read every word as an option',
        'src/cli/images.rs',
        '            "--" => positional = true,',
        '            "--" => {}',
        'a_name_that_looks_like_an_option_is_a_picture_after_two_dashes',
    ),
    Mutation(
        # Refused later by the filesystem check, with exit 3 where a malformed line is 2.
        'cli images: let the output name a picture',
        'src/cli/images.rs',
        '        .any(|input| lexically_same(input, &command.output))',
        '        .any(|_| false)',
        'the_parser_takes_pictures_in_order_and_an_output_that_is_none_of_them',
    ),
    Mutation(
        # Keep the columns outside the region instead of blanking them, which is
        # what the gate did until 2026-08-27: `strip` renders full-width rows, so
        # every word beside the region on those rows was read back as though the
        # removal had left it. 54 of 104 regions on 40 real documents.
        "mask: keep the columns beside the region",
        "src/ocr_gate.rs",
        "        row[..left * 4].fill(0xFF);\n        row[right * 4..].fill(0xFF);",
        "        let _ = (left, right);",
        "masking_keeps_the_region_s_own_columns_and_blanks_the_rest",
    ),
    Mutation(
        # Blank one side only. A region is two edges and a check that reads one
        # of them passes on this.
        "mask: blank only what is left of the region",
        "src/ocr_gate.rs",
        "        row[right * 4..].fill(0xFF);",
        "        let _ = right;",
        "masking_keeps_the_region_s_own_columns_and_blanks_the_rest",
    ),
    Mutation(
        # Round the region's edges inward rather than outward. A glyph whose ink
        # lands in the boundary pixel is then half erased, and half a glyph is
        # something an engine reads as something.
        "mask: round the region's edges inward",
        "src/ocr_gate.rs",
        "    let left = (rect[0].min(rect[2]) * scale).floor().max(0.0) as usize;",
        "    let left = (rect[0].min(rect[2]) * scale).ceil().max(0.0) as usize;",
        "masking_widens_to_whole_pixels_rather_than_clipping_the_region",
    ),
    Mutation(
        # Blank a strip whose columns miss the page instead of refusing. It then
        # reads as nothing, and reading nothing is the answer that certifies.
        "mask: blank a region that is not on the page at all",
        "src/ocr_gate.rs",
        "    if left >= right {\n        return Err(format!(",
        "    if false {\n        return Err(format!(",
        "a_region_beside_the_page_is_refused_rather_than_blanked",
    ),
    Mutation(
        # Take the rectangle's corners in the order given rather than by min and
        # max. A region dragged right to left then masks nothing.
        "mask: assume a region is dragged left to right",
        "src/ocr_gate.rs",
        "    let left = (rect[0].min(rect[2]) * scale).floor().max(0.0) as usize;\n    let right = ((rect[0].max(rect[2]) * scale).ceil().max(0.0) as usize).min(width_px as usize);",
        "    let left = (rect[0] * scale).floor().max(0.0) as usize;\n    let right = ((rect[2] * scale).ceil().max(0.0) as usize).min(width_px as usize);",
        "a_region_reversed_left_to_right_masks_the_same_columns",
    ),
    Mutation(
        # Report every child of a form the region touches, which is what this did
        # until 2026-08-27. A form is routinely a whole-page container, so a
        # region over one line inside a letterhead was refused for every picture
        # in it: 174 of 1,131 refusals across 40 real documents, 15.4%.
        "formchild: report every child of a form the region reaches",
        "src/redact.rs",
        "            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {",
        "            for other in &form.unreachable {\n                if true {",
        "a_form_child_the_region_misses_is_not_reported",
    ),
    Mutation(
        # The mirror, and the one that matters more: report nothing. A picture
        # sitting under the region is then silently not a reason, and the region
        # is certified complete when it is not.
        "formchild: report no child of a form at all",
        "src/redact.rs",
        "            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {",
        "            for other in &form.unreachable {\n                if !overlaps(other.bounds, region) {",
        "a_form_child_the_region_covers_is_still_reported",
    ),
    Mutation(
        # Take every line in a covered form, not the ones the region covers. A
        # region over one line of a form then removes the whole form, which is
        # content the reader did not mark.
        "form: take every line of a form the region reaches",
        "src/redact.rs",
        "                if overlaps(text.bounds, region) {\n                    plan.form_shows.push((at, ordinal));\n                }",
        "                let _ = overlaps(text.bounds, region);\n                plan.form_shows.push((at, ordinal));",
        "a_region_over_a_form_takes_only_the_lines_it_covers",
    ),
    Mutation(
        # Report a form as unhandled even when the descent read it. Every region
        # over a form is then refused, which is what the code did before this
        # carrier was reachable at all.
        "form: refuse a form whose text was read",
        "src/redact.rs",
        "        let inside = (object.kind == \"form\")\n            .then(|| forms.iter().find(|form| form.at == at))\n            .flatten();",
        "        let inside: Option<&FormObject> = None;\n        let _ = forms;",
        "a_region_over_a_forms_text_names_it_by_form_and_ordinal",
    ),
    Mutation(
        # Report what the descent could not reach only when some of the form's
        # text was also covered. A region over a form holding nothing but a
        # nested form then takes nothing and says nothing, which is the quiet
        # case becoming the silent one.
        "form: report the unreachable only beside a hit",
        "src/redact.rs",
        "            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {\n                    plan.unhandled.push(Unhandled {\n                        at,\n                        kind: other.kind.clone(),\n                        drawn: None,\n                    });\n                }\n            }",
        "            if !plan.form_shows.is_empty() {\n            for other in &form.unreachable {\n                if overlaps(other.bounds, region) {\n                    plan.unhandled.push(Unhandled {\n                        at,\n                        kind: other.kind.clone(),\n                        drawn: None,\n                    });\n                }\n            }\n            }",
        "what_the_descent_could_not_reach_is_reported_even_when_nothing_was_covered",
    ),
    Mutation(
        # Stop counting the `Do` operations on this page. A form drawn twice here
        # is ONE reference in the object graph, so the graph count alone calls it
        # unshared and the removal changes a place the reader did not mark.
        "form: count only the object graph, not the page's draws",
        "src/redact.rs",
        "    } else {\n        here\n    };",
        "    } else {\n        1\n    };",
        "a_form_drawn_twice_on_one_page_is_refused",
    ),
    Mutation(
        # The mirror: stop counting references. A form another page draws is one
        # `Do` here, so the page count alone calls it unshared.
        "form: count only the page's draws, not the object graph",
        "src/redact.rs",
        "    let here = names.iter().filter(|other| *other == name).count();\n    let elsewhere = references_to(doc, id);",
        "    let here = names.iter().filter(|other| *other == name).count();\n    let elsewhere = 0;\n    let _ = references_to(doc, id);",
        "a_form_another_page_also_draws_is_refused",
    ),
    Mutation(
        # Remove ascending. An earlier removal moves every later index, so the
        # second ordinal names whatever slid into its place.
        "form: remove a form's lines in ascending order",
        "src/redact.rs",
        "    for where_ in positions.into_iter().rev() {\n"
        "        let stays = without_drawing(&inside.operations[where_]);",
        "    for where_ in positions.into_iter() {\n"
        "        let stays = without_drawing(&inside.operations[where_]);",
        "removing_two_lines_from_one_form_takes_both_and_keeps_the_rest",
    ),
    Mutation(
        # Drop the correspondence guard. Nothing connects PDFium's form objects
        # to the page's `Do` operations but order, so a disagreement removes text
        # from whichever form happens to be at that position.
        "form: remove from whichever form is at that position",
        "src/redact.rs",
        "    let text_objects = forms[which].1;\n\n    let data = doc\n        .get_page_content_with_limit(page, MAX_CONTENT_BYTES)\n        .map_err(|why| format!(\"the page's content stream could not be read: {why}\"))?;\n    let content = Content::decode(&data)\n        .map_err(|why| format!(\"the content stream will not decode: {why}\"))?;\n\n    let names = form_draws(doc, page, &content)?;\n    if names.len() != forms.len() {",
        "    let text_objects = forms[which].1;\n\n    let data = doc\n        .get_page_content_with_limit(page, MAX_CONTENT_BYTES)\n        .map_err(|why| format!(\"the page's content stream could not be read: {why}\"))?;\n    let content = Content::decode(&data)\n        .map_err(|why| format!(\"the content stream will not decode: {why}\"))?;\n\n    let names = form_draws(doc, page, &content)?;\n    if false {",
        "a_form_count_that_disagrees_with_pdfium_removes_nothing",
    ),
    Mutation(
        # The same guard one level down: the form's own show operators against
        # what PDFium counted inside it.
        "form: remove by position without checking the count",
        "src/redact.rs",
        "    if shows.len() != text_objects {\n        return Err(format!(\n            \"this form has {} text-showing operator(s) and PDFium reported {text_objects} text \\\n             object(s). Removing by position needs those to agree, so nothing was removed.\",",
        "    if false {\n        return Err(format!(\n            \"this form has {} text-showing operator(s) and PDFium reported {text_objects} text \\\n             object(s). Removing by position needs those to agree, so nothing was removed.\",",
        "a_show_count_that_disagrees_with_pdfium_removes_nothing",
    ),
]
