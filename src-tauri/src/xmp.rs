//! The XMP metadata packet: what a document says about itself in RDF/XML.
//!
//! A PDF may carry a second set of document properties beside `/Info`, as an
//! XML packet hanging off the catalog's `/Metadata`. One thing lives there and
//! nowhere else: a **conformance claim**. PDF/A, PDF/UA and PDF/X are declared
//! only in XMP, so a document saying it is archival says it here or not at all.
//!
//! That is all this reads, and the scope was decided by measurement rather than
//! by taste. Across 41 real PDFs, 24 carry XMP and **8 state a conformance
//! level** --- seven PDF/UA-1 and one PDF/A-3B. The obvious second feature, and
//! the one this module was first written for, was comparing XMP's title, author
//! and producer against `/Info`'s: PDF 2.0 deprecates `/Info` in favour of XMP,
//! so a disagreement means two readers see different things. It occurred
//! **zero** times in that corpus, and XMP stating a value `/Info` omits
//! occurred zero times as well. Three fields were parsed and then removed; the
//! measurement is the deliverable, and `docs/PLAN.md` keeps it.
//!
//! # What this is not
//!
//! A claim is a claim. Nothing here validates a document against PDF/A, and
//! `pdfaid:part` is one integer a producer wrote; a file may state PDF/A-3B and
//! violate it in twenty ways. The wording in `properties.ts` says *states*
//! throughout, for the reason the signature rows do.
//!
//! # Hostile input
//!
//! This is a **second markup parser** in the trust boundary, on bytes the
//! document chose --- see `docs/THREAT-MODEL.md` §T6.6. Four things bound it:
//!
//! - The packet is capped at [`MAX_PACKET`] before the parser sees it, and
//!   exceeding that is *reported* rather than read as a document with no XMP.
//! - Element nesting is capped at [`MAX_DEPTH`], so a packet nested a million
//!   deep costs a counter rather than a stack.
//! - Each value is capped at [`MAX_VALUE`].
//! - **Entities are never expanded.** `quick-xml` resolves only the five
//!   predefined entities and numeric character references; a `<!ENTITY>`
//!   declaration arrives as a `DocType` event and is dropped. That is what
//!   makes the billion-laughs attack inert, and it is asserted by a test rather
//!   than inherited from the crate's documentation.
//!
//! # Namespaces, not prefixes
//!
//! Properties are matched on the namespace **URI** and local name, which is
//! what RDF defines them by. The conventional prefixes (`dc`, `pdfaid`) are
//! arbitrary: a producer may bind `dc:` to something else, or declare the
//! Dublin Core namespace under any prefix it likes, and a prefix-matching
//! scanner is wrong on both. `NsReader` does the resolution.

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::NsReader;

/// Largest XMP packet this will parse.
///
/// Real packets run from a few hundred bytes to about 40 kB; one that reaches a
/// megabyte is not a document describing itself. Exceeding it sets
/// [`Xmp::unread`] rather than reporting an absent packet.
pub const MAX_PACKET: usize = 1024 * 1024;

/// Deepest element nesting the parse will follow.
const MAX_DEPTH: usize = 64;

/// Longest value kept from any one property.
const MAX_VALUE: usize = 4096;

/// PDF/A's identification namespace.
const NS_PDFAID: &str = "http://www.aiim.org/pdfa/ns/id/";
/// PDF/UA's.
const NS_PDFUAID: &str = "http://www.aiim.org/pdfua/ns/id/";
/// PDF/X's.
const NS_PDFXID: &str = "http://www.npes.org/pdfx/ns/id/";

/// What the packet says, and what could not be read of it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Xmp {
    /// The packet's length in bytes as stored, before any parse.
    pub bytes: usize,
    /// Conformance claims, as the document words them --- `PDF/A-3B`.
    ///
    /// Several are legal and do occur: a file may state PDF/A and PDF/UA at
    /// once. **Sorted**, not in declaration order: a producer's ordering of two
    /// `rdf:Description` blocks carries nothing, and sorting makes two files
    /// claiming the same pair read identically. It is also the only order this
    /// can honestly promise, since PDF/A is assembled from two properties after
    /// the packet is read and would otherwise always land last.
    pub conformance: Vec<String>,
    /// The packet was present and something in it could not be read.
    ///
    /// Distinct from an absent packet, which is [`Option::None`] one level up:
    /// *this document says nothing about itself* and *this document said
    /// something tpdf could not read* are different facts, and only one of them
    /// is reassuring.
    pub unread: bool,
}

/// Which property an element or attribute names, where it names one.
#[derive(Clone, Copy, PartialEq)]
enum Property {
    PdfaPart,
    PdfaConformance,
    PdfuaPart,
    PdfxVersion,
}

/// The property a namespace and local name identify, if any.
fn property_of(namespace: &str, local: &str) -> Option<Property> {
    match (namespace, local) {
        (NS_PDFAID, "part") => Some(Property::PdfaPart),
        (NS_PDFAID, "conformance") => Some(Property::PdfaConformance),
        (NS_PDFUAID, "part") => Some(Property::PdfuaPart),
        (NS_PDFXID, "GTS_PDFXVersion") => Some(Property::PdfxVersion),
        _ => None,
    }
}

/// Reads one XMP packet.
///
/// Returns what it could read; a packet that is too large, too deep or
/// malformed part-way through yields whatever was read before that point with
/// [`Xmp::unread`] set. Nothing here fails outright, because a packet tpdf
/// cannot parse is still a packet whose *size* and *presence* are facts.
pub fn scan(packet: &[u8]) -> Xmp {
    let mut out = Xmp {
        bytes: packet.len(),
        ..Xmp::default()
    };
    if packet.len() > MAX_PACKET {
        out.unread = true;
        return out;
    }

    // A packet is wrapped in `<?xpacket ...?>` processing instructions, which
    // are events like any other and need no stripping.
    let mut reader = NsReader::from_reader(packet);
    // Deliberately NOT `trim_text(true)`. It trims each text event, and a value
    // containing an entity arrives as several -- so `PDF/X-4 &amp; later` came
    // back as `PDF/X-4&later`, the spaces on either side of the entity having
    // been trimmed as the ends of their own fragments. The whole value is
    // trimmed once, where it is stored.

    // The stack holds the property each open element names, and `pending`
    // accumulates the text of the innermost one that names a property. Text is
    // accumulated rather than taken from the first event, because a value
    // containing an entity arrives in pieces: `AT&amp;T` is three events, and
    // taking the first would report the value as `AT`.
    let mut stack: Vec<Option<Property>> = Vec::new();
    let mut pending: Option<(Property, usize, String)> = None;
    let mut buffer = Vec::new();
    let mut pdfa_part = String::new();
    let mut pdfa_conformance = String::new();

    loop {
        buffer.clear();
        // Read unresolved, then resolve against the reader. The resolved form
        // borrows the reader for as long as the event lives, which rules out
        // resolving the element's *attributes* in the same breath --- and the
        // attribute form of a property is 3 of the 8 claims in the wild.
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(element)) => {
                if stack.len() >= MAX_DEPTH {
                    out.unread = true;
                    break;
                }
                let (resolved, local) = reader.resolver().resolve_element(element.name());
                let property = named(resolved, local.as_ref());
                // Attribute form: `<rdf:Description pdfuaid:part="1"/>` says the
                // same thing as the element form and is a third of what occurs.
                read_attributes(
                    &reader,
                    &element,
                    &mut out,
                    &mut pdfa_part,
                    &mut pdfa_conformance,
                );
                stack.push(property);
                if let (Some(property), None) = (property, &pending) {
                    pending = Some((property, stack.len(), String::new()));
                }
            }
            Ok(Event::Empty(element)) => {
                read_attributes(
                    &reader,
                    &element,
                    &mut out,
                    &mut pdfa_part,
                    &mut pdfa_conformance,
                );
            }
            Ok(Event::End(_)) => {
                // Flush when the element that opened the accumulation closes,
                // which is the one at this depth.
                if pending
                    .as_ref()
                    .is_some_and(|(_, depth, _)| *depth == stack.len())
                {
                    let (property, _, value) = pending.take().expect("just checked");
                    store(
                        property,
                        clip(value.trim()),
                        &mut out,
                        &mut pdfa_part,
                        &mut pdfa_conformance,
                    );
                }
                stack.pop();
            }
            Ok(Event::Text(text)) => {
                // Text events carry **no** entities: quick-xml delivers every
                // `&...;` as a separate `GeneralRef`, measured rather than
                // assumed. So there is nothing to unescape here.
                let Ok(value) = text.decode() else {
                    out.unread = true;
                    continue;
                };
                if let Some((_, _, buffer)) = &mut pending {
                    append(buffer, &value, &mut out.unread);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                // **The whole entity-expansion defence is this arm.** An
                // unknown entity cannot produce text: `unescape` resolves the
                // five predefined names and character references and refuses
                // everything else, and nothing here supplies a resolver. So a
                // billion-laughs declaration is not bounded, it is inert --- and
                // the refusal is reported rather than silently dropping the
                // character, because a value quietly shortened is a value the
                // document did not state.
                let Ok(name) = reference.decode() else {
                    out.unread = true;
                    continue;
                };
                let spelled = format!("&{name};");
                let Ok(resolved) = quick_xml::escape::unescape(&spelled) else {
                    out.unread = true;
                    continue;
                };
                if let Some((_, _, buffer)) = &mut pending {
                    append(buffer, &resolved, &mut out.unread);
                }
            }
            Ok(_) => {}
            Err(_) => {
                out.unread = true;
                break;
            }
        }
    }

    if !pdfa_part.is_empty() {
        out.conformance
            .push(format!("PDF/A-{pdfa_part}{pdfa_conformance}"));
    }
    out.conformance.sort();
    out
}

/// The property a resolved element name identifies.
fn named(resolved: ResolveResult, local: &[u8]) -> Option<Property> {
    let ResolveResult::Bound(namespace) = resolved else {
        return None;
    };
    let namespace = std::str::from_utf8(namespace.as_ref()).ok()?;
    let local = std::str::from_utf8(local).ok()?;
    property_of(namespace, local)
}

/// Reads the properties an element states as attributes rather than children.
fn read_attributes(
    reader: &NsReader<&[u8]>,
    element: &quick_xml::events::BytesStart,
    out: &mut Xmp,
    pdfa_part: &mut String,
    pdfa_conformance: &mut String,
) {
    for attribute in element.attributes().flatten() {
        let (resolved, local) = reader.resolver().resolve_attribute(attribute.key);
        let Some(property) = named(resolved, local.as_ref()) else {
            continue;
        };
        // `normalized_value` unescapes on the same terms as the text path ---
        // its own body reads *"resolve_predefined_entity returns only
        // non-recursive replacements, so depth=1 is enough"*, which is the
        // property this module rests on, stated by the crate at the call it
        // makes. XMP is XML 1.0; the packet's own declaration is a processing
        // instruction the reader does not act on.
        let Ok(value) = attribute.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
            out.unread = true;
            continue;
        };
        store(
            property,
            clip(value.trim()),
            out,
            pdfa_part,
            pdfa_conformance,
        );
    }
}

/// Keeps a value, where nothing has claimed that property yet.
///
/// First wins throughout. A packet may state a property more than once --- two
/// `rdf:Description` blocks are legal and occur --- and every reader shows the
/// first; taking the last would make the value depend on how many times the
/// producer repeated itself.
fn store(
    property: Property,
    value: String,
    out: &mut Xmp,
    pdfa_part: &mut String,
    pdfa_conformance: &mut String,
) {
    if value.is_empty() {
        return;
    }
    let once = |slot: &mut String| {
        if slot.is_empty() {
            *slot = value.clone();
        }
    };
    match property {
        Property::PdfaPart => once(pdfa_part),
        Property::PdfaConformance => once(pdfa_conformance),
        // PDF/UA and PDF/X each state one value, so they need no assembly and
        // are pushed as they are found.
        Property::PdfuaPart => {
            let claim = format!("PDF/UA-{value}");
            if !out.conformance.contains(&claim) {
                out.conformance.push(claim);
            }
        }
        Property::PdfxVersion => {
            if !out.conformance.contains(&value) {
                out.conformance.push(value);
            }
        }
    }
}

/// Appends to a value under accumulation, stopping at [`MAX_VALUE`].
///
/// The bound is here rather than only at the end, so a document cannot make
/// this hold a gigabyte and then clip it --- which is the shape of every
/// decompression bomb, and would be an odd thing to write into the module whose
/// job is refusing one.
fn append(buffer: &mut String, value: &str, unread: &mut bool) {
    if buffer.len() >= MAX_VALUE {
        *unread = true;
        return;
    }
    buffer.push_str(value);
}

/// Shortens a value to [`MAX_VALUE`], on a character boundary.
fn clip(value: &str) -> String {
    if value.len() <= MAX_VALUE {
        return value.to_string();
    }
    let mut end = MAX_VALUE;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An XMP packet stating what the arguments say, in element form.
    fn packet(body: &str) -> Vec<u8> {
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
{body}
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
        )
        .into_bytes()
    }

    /// A PDF/A claim written as child elements, which is the commoner form.
    #[test]
    fn a_pdfa_claim_in_element_form_is_read() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>3</pdfaid:part>
   <pdfaid:conformance>B</pdfaid:conformance>
  </rdf:Description>"#,
        ));

        assert_eq!(xmp.conformance, vec!["PDF/A-3B"]);
        assert!(!xmp.unread);
    }

    /// The same claim written as attributes, which is 3 of the 8 in the wild.
    ///
    /// Measured, not assumed: across 41 real PDFs, 8 state a conformance level
    /// and **three of them do it in attribute form**. A reader that handles
    /// only child elements is right about five documents and silent about
    /// three, and silence here is indistinguishable from a document that claims
    /// nothing --- which is most documents, so nothing would look wrong.
    #[test]
    fn a_claim_written_as_attributes_is_read_as_the_same_claim() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
      rdf:about="" pdfuaid:part="1"/>"#,
        ));

        assert_eq!(xmp.conformance, vec!["PDF/UA-1"]);

        // And the same attribute on an element that is **not** self-closing,
        // which is a different code path and was covered by nothing until a
        // mutation deleting it survived. `<rdf:Description ... />` is an
        // `Event::Empty`; a description carrying both an attribute and children
        // is an `Event::Start`, and real producers write both.
        let with_children = scan(&packet(
            r#"  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      rdf:about="" pdfaid:part="2" pdfaid:conformance="A">
   <dc:format>application/pdf</dc:format>
  </rdf:Description>"#,
        ));
        assert_eq!(
            with_children.conformance,
            vec!["PDF/A-2A"],
            "an attribute on an element with children is read like any other"
        );
    }

    /// A document may state more than one standard, and both are listed.
    #[test]
    fn a_document_claiming_two_standards_is_reported_as_claiming_both() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>2</pdfaid:part>
   <pdfaid:conformance>A</pdfaid:conformance>
  </rdf:Description>
  <rdf:Description xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"
      rdf:about="" pdfuaid:part="1"/>"#,
        ));

        assert_eq!(xmp.conformance, vec!["PDF/A-2A", "PDF/UA-1"]);
    }

    /// The prefix is arbitrary; the namespace URI is what names the property.
    ///
    /// `pdfaid` is a convention, not a rule --- RDF identifies a property by its
    /// namespace URI, and a producer may bind that URI to any prefix. A scanner
    /// matching the string `pdfaid:part` is wrong in both directions, and this
    /// asserts both: an unconventional prefix bound to the right URI **is** a
    /// claim, and the conventional prefix bound to something else is **not**.
    #[test]
    fn a_claim_is_identified_by_its_namespace_and_not_by_its_prefix() {
        let renamed = scan(&packet(
            r#"  <rdf:Description xmlns:zz="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <zz:part>1</zz:part>
   <zz:conformance>B</zz:conformance>
  </rdf:Description>"#,
        ));
        assert_eq!(
            renamed.conformance,
            vec!["PDF/A-1B"],
            "the URI is what makes it a PDF/A claim"
        );

        let impostor = scan(&packet(
            r#"  <rdf:Description xmlns:pdfaid="http://example.invalid/not-pdfa/" rdf:about="">
   <pdfaid:part>3</pdfaid:part>
  </rdf:Description>"#,
        ));
        assert!(
            impostor.conformance.is_empty(),
            "and the familiar prefix over some other namespace is not one"
        );
    }

    /// A packet stating no conformance is read, and states nothing.
    ///
    /// The control for every test above: most documents with XMP claim no
    /// standard at all, so an implementation that reported one for everything
    /// would pass none of them and an implementation that reported one for
    /// nothing would pass only this.
    #[test]
    fn a_packet_claiming_nothing_is_read_and_claims_nothing() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/">
   <dc:title><rdf:Alt><rdf:li>A document</rdf:li></rdf:Alt></dc:title>
  </rdf:Description>"#,
        ));

        assert!(xmp.conformance.is_empty());
        assert!(!xmp.unread, "and nothing about it was unreadable");
        assert!(xmp.bytes > 0, "the packet's size is a fact either way");
    }

    /// Entities are never expanded, so the billion-laughs attack is inert.
    ///
    /// **Asserted rather than inherited from the crate's documentation**, which
    /// is the difference between a bound and a belief. `quick-xml` resolves the
    /// five predefined entities and character references; a custom `<!ENTITY>`
    /// needs `unescape_with` and a resolver, and this module calls neither. So
    /// the classic payload --- nine nested entities, each ten copies of the
    /// last, expanding to a gigabyte --- costs a `DocType` event that is
    /// dropped.
    ///
    /// The assertion that matters is not that it finishes: it is that no
    /// expansion happened, and the two outcomes are distinguishable. Had the
    /// entity expanded, `pdfaid:part` would be a gigabyte of `lol` clipped to
    /// [`MAX_VALUE`] and the claim would read `PDF/A-lollollol...`; what
    /// happens instead is that `unescape` refuses an entity it does not know,
    /// so the value is **dropped and the packet marked unread**. A test
    /// asserting only that it terminated would pass on an implementation that
    /// expanded to a gigabyte quickly.
    ///
    /// The control beneath it is what stops that being a blanket *"any
    /// ampersand breaks the parse"*: a predefined entity in the same position
    /// resolves and is read.
    #[test]
    fn a_billion_laughs_packet_is_neither_expanded_nor_followed() {
        let mut dtd = String::from("<!DOCTYPE x [\n<!ENTITY lol \"lol\">\n");
        for level in 1..=9 {
            dtd.push_str(&format!(
                "<!ENTITY lol{level} \"&lol{};&lol{};&lol{};&lol{};&lol{};&lol{};&lol{};&lol{};&lol{};&lol{};\">\n",
                level - 1, level - 1, level - 1, level - 1, level - 1,
                level - 1, level - 1, level - 1, level - 1, level - 1,
            ));
        }
        dtd = dtd.replace("&lol0;", "&lol;");
        dtd.push_str("]>\n");

        let bomb = format!(
            r#"{dtd}<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>&lol9;</pdfaid:part>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#
        );
        let started = std::time::Instant::now();
        let xmp = scan(bomb.as_bytes());
        let elapsed = started.elapsed();

        // The entity was not resolved, so what reached the property is the
        // reference itself. Had it expanded, this would be a gigabyte of "lol"
        // clipped to MAX_VALUE -- so the two outcomes are distinguishable, and
        // this asserts the one that means no expansion took place.
        assert!(
            xmp.conformance.is_empty(),
            "nothing expanded, so there is no gigabyte of lol to clip: {:?}",
            xmp.conformance
        );
        assert!(xmp.unread, "and the refusal is reported rather than silent");
        assert!(
            elapsed.as_secs() < 5,
            "and it took {elapsed:?}, which no expansion of this would"
        );

        // The control. A predefined entity in the same position resolves, so
        // the assertions above are about *custom* entities and not about the
        // parser giving up on any ampersand it meets.
        let ordinary = scan(&packet(
            r#"  <rdf:Description xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" rdf:about="">
   <pdfxid:GTS_PDFXVersion>PDF/X-4 &amp; later</pdfxid:GTS_PDFXVersion>
  </rdf:Description>"#,
        ));
        assert_eq!(ordinary.conformance, vec!["PDF/X-4 & later"]);
        assert!(!ordinary.unread);
    }

    /// A value split across text and entity events is assembled whole.
    ///
    /// The reason accumulation exists rather than taking the first text event.
    /// `quick-xml` delivers every `&...;` as its own event, so a value with an
    /// ampersand or a character reference in it arrives in three pieces or
    /// five; a reader taking the first would report `PDF/X-4` for a document
    /// stating `PDF/X-4 & later`, which is a *plausible* wrong answer and
    /// therefore the dangerous kind.
    #[test]
    fn a_value_arriving_in_pieces_is_put_back_together() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" rdf:about="">
   <pdfxid:GTS_PDFXVersion>caf&#233; &amp; caf&#233;</pdfxid:GTS_PDFXVersion>
  </rdf:Description>"#,
        ));

        assert_eq!(xmp.conformance, vec!["café & café"]);
        assert!(!xmp.unread);
    }

    /// A value built past the cap out of many fragments is stopped and counted.
    ///
    /// The bound has to be on the accumulation and not only on the finished
    /// string: clipping at the end means holding whatever the document chose to
    /// send first, which is the shape of every decompression bomb and an odd
    /// thing to build into the module whose job is refusing one.
    #[test]
    fn a_value_assembled_past_the_cap_stops_accumulating_and_says_so() {
        // Each `&amp;` is one character through a whole event, so this is the
        // cheap way for a packet to ask for a large string.
        let many = "&amp;".repeat(MAX_VALUE + 100);
        let xmp = scan(&packet(&format!(
            r#"  <rdf:Description xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" rdf:about="">
   <pdfxid:GTS_PDFXVersion>{many}</pdfxid:GTS_PDFXVersion>
  </rdf:Description>"#
        )));

        assert!(xmp.unread, "the reader is told the value was cut short");
        assert!(
            xmp.conformance[0].len() <= MAX_VALUE,
            "and it never grew past the cap: {}",
            xmp.conformance[0].len()
        );
    }

    /// A packet past the cap is reported as unread, not as absent.
    #[test]
    fn a_packet_larger_than_the_cap_is_reported_rather_than_dropped() {
        let mut huge = packet(
            r#"  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>1</pdfaid:part>
  </rdf:Description>"#,
        );
        huge.resize(MAX_PACKET + 1, b' ');

        let xmp = scan(&huge);
        assert!(xmp.unread, "the reader is told it could not read this");
        assert!(xmp.conformance.is_empty(), "and nothing is claimed from it");
        assert_eq!(xmp.bytes, MAX_PACKET + 1, "its size is still a fact");
    }

    /// Nesting past the bound stops the walk and says so.
    ///
    /// A `<a><a><a>...` chain is a stack the document chose the depth of. The
    /// bound has to be *reported*, because a packet abandoned in silence is a
    /// packet that claimed nothing, which is the reassuring reading.
    #[test]
    fn nesting_past_the_bound_stops_and_is_reported() {
        let deep = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">{}{}</x:xmpmeta>",
            "<x:a>".repeat(MAX_DEPTH + 10),
            "</x:a>".repeat(MAX_DEPTH + 10),
        );

        let xmp = scan(deep.as_bytes());
        assert!(xmp.unread);

        // And the control: the same shape one level inside the bound is read
        // without complaint, so this asserts a bound rather than a refusal.
        let shallow = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">{}{}</x:xmpmeta>",
            "<x:a>".repeat(MAX_DEPTH - 2),
            "</x:a>".repeat(MAX_DEPTH - 2),
        );
        assert!(!scan(shallow.as_bytes()).unread);
    }

    /// Malformed XML is reported as unread, keeping whatever was read first.
    #[test]
    fn a_packet_that_stops_making_sense_keeps_what_it_had_and_says_so() {
        let broken = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>2</pdfaid:part>
   <pdfaid:conformance>U</pdfaid:conformance>
  </rdf:Description>
 <<<< not xml at all"#;

        let xmp = scan(broken.as_bytes());
        assert_eq!(
            xmp.conformance,
            vec!["PDF/A-2U"],
            "what was read before the damage is still what the document said"
        );
        assert!(xmp.unread, "and the reader is told the rest was not read");
    }

    /// A value stated twice is taken once, from the front.
    #[test]
    fn a_property_stated_twice_is_read_from_the_first_statement() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>1</pdfaid:part>
   <pdfaid:conformance>A</pdfaid:conformance>
  </rdf:Description>
  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>4</pdfaid:part>
   <pdfaid:conformance>F</pdfaid:conformance>
  </rdf:Description>"#,
        ));

        assert_eq!(xmp.conformance, vec!["PDF/A-1A"]);
    }

    /// A part with no conformance letter is still a claim.
    ///
    /// PDF/A-4 dropped the conformance letter, and PDF/UA never had one, so a
    /// bare part is correct rather than damaged --- an implementation requiring
    /// both would report nothing for the newest standard in the family.
    #[test]
    fn a_part_with_no_conformance_letter_is_still_a_claim() {
        let xmp = scan(&packet(
            r#"  <rdf:Description xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" rdf:about="">
   <pdfaid:part>4</pdfaid:part>
  </rdf:Description>"#,
        ));

        assert_eq!(xmp.conformance, vec!["PDF/A-4"]);
    }

    /// A long value is clipped on a character boundary, not mid-codepoint.
    #[test]
    fn a_value_past_the_cap_is_clipped_where_a_character_ends() {
        let long = "ü".repeat(MAX_VALUE);
        let xmp = scan(&packet(&format!(
            r#"  <rdf:Description xmlns:pdfxid="http://www.npes.org/pdfx/ns/id/" rdf:about="">
   <pdfxid:GTS_PDFXVersion>{long}</pdfxid:GTS_PDFXVersion>
  </rdf:Description>"#
        )));

        let claim = &xmp.conformance[0];
        assert!(claim.len() <= MAX_VALUE, "clipped to the cap");
        assert!(
            claim.chars().all(|c| c == 'ü'),
            "and on a boundary, so no character was cut in half"
        );
    }
}
