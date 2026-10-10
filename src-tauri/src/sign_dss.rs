//! The worker's half of long-term validation data: the `/DSS` revision.
//!
//! ## What this writes
//!
//! Phase 6 step 3, increment C2 (`docs/PLAN.md` §9). A PAdES baseline B-LT
//! signature is a B-T signature whose document also carries the certificates
//! and revocation data a verifier needs once the certificates have expired
//! and the responders have stopped answering. ETSI EN 319 142-1 §5.4.2 puts
//! them in the **document security store**: a `/DSS` dictionary in the
//! catalog, with `/Certs`, `/OCSPs` and `/CRLs` arrays of streams --- each a
//! certificate, a full DER `OCSPResponse`, or a `CertificateList` --- appended
//! as an incremental revision **after** the signature's own, so every byte the
//! signature covers stays the byte it was made over.
//!
//! **No `/VRI`.** The same clause defines a `/VRI` dictionary, keyed by the
//! SHA-1 of each signature's `/Contents`, that says which of the data belongs
//! to which signature; it is optional there, and the baseline profile asks for
//! the validation data, not for the index. A reader --- tpdf's own
//! (`docinfo::read_dss`), pyHanko, Acrobat --- finds the data it needs by
//! matching certificates, which is what the index would have told it, and a
//! second copy of that relationship is one more thing to get wrong in a file.
//!
//! An existing `/DSS`, left by an earlier signature's writer, is kept: its
//! arrays are carried into the new dictionary and the new streams added after
//! them. Nothing an earlier writer put there is dropped.
//!
//! ## Where it runs, and what it checks before it answers
//!
//! **In a worker**, holding the signed bytes the app process has not written
//! yet: the revision is built with `lopdf` from a parse of the document, and
//! every earlier revision is the reader's document verbatim. The app process
//! sends those bytes and the DER blobs it fetched (`longterm.rs`), and never
//! parses the document itself.
//!
//! The worker then **reads its own output** --- the signed bytes and the new
//! revision together --- with `docinfo::scan`, the function the properties
//! dialog's answer comes from, and returns what it found beside the revision.
//! The app process decides from that whether anything is written
//! (`longterm::check`): the new signature intact, its timestamp intact, and the
//! signer's and the authority's revocation standings `good`.

use lopdf::{Dictionary, Document, IncrementalDocument, Object, Stream};

use crate::encoding::{resolve, MAX_DECODE};

/// The validation data a signing gathered, as the worker is sent it: DER.
///
/// **Crosses the worker boundary**, from the app process, which fetched every
/// response and list over the network and checked each against the
/// certificate it is about (`longterm::gather`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Gathered {
    /// Every certificate in the signer's chain and the timestamp authority's,
    /// each once, the roots included.
    pub certificates: Vec<Vec<u8>>,
    /// Full `OCSPResponse`s, as `/DSS /OCSPs` carries them.
    pub responses: Vec<Vec<u8>>,
    /// `CertificateList`s.
    pub lists: Vec<Vec<u8>>,
}

impl Gathered {
    /// How many bytes of DER it holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.certificates
            .iter()
            .chain(&self.responses)
            .chain(&self.lists)
            .map(Vec::len)
            .sum()
    }
}

/// The revision carrying the data, and what the worker read in the result.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extended {
    /// The update section: the new streams, the `/DSS`, the catalog pointing
    /// at it, a cross-reference and a trailer.
    pub update: Vec<u8>,
    /// How long the signed document it was built against was: the update's
    /// offsets are measured from it, and the app process checks it.
    pub built_against: usize,
    /// Every signature in the signed bytes and the update together, as
    /// `docinfo::scan` reads them.
    pub signatures: Vec<crate::docinfo::Signature>,
}

/// What a document that is already signed holds, as the worker reads it
/// before any long-term validation data is added: the question
/// `longterm::existing` plans from.
///
/// **Crosses the worker boundary, toward the app process.** The app process
/// fetches revocation data and so has to know which certificates to ask
/// about; they are in each signature's CMS, which this hands over as the DER
/// it is. It is the document's data --- anybody's --- and the app process
/// reads it with the readers and the bounds it reads a timestamp token from
/// the network with (`longterm::existing`, `docs/THREAT-MODEL.md` §T10).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Survey {
    /// Why this document can take no validation data at all, as a sentence:
    /// it does not parse as a file a revision can be appended to, it is
    /// encrypted, or its signatures are more than the answer carries. `None`
    /// when it can. **A field and not an error**, so that a document's own
    /// refusal is told apart from a worker that died or did not answer,
    /// which is the only thing the call itself fails for.
    pub refused: Option<String>,
    /// Every signature field, as `docinfo::scan` reads the document now.
    pub signatures: Vec<crate::docinfo::Signature>,
    /// Each **signed** field's value, DER: its full name, and the CMS --- for
    /// a document timestamp, the token. In the order of `signatures`.
    pub values: Vec<(String, Vec<u8>)>,
    /// The certificates the document's `/DSS` already carries, DER: candidate
    /// issuers the reader of the result will have.
    pub store: Vec<Vec<u8>>,
    /// Whether every signature field was reached and every value read. When
    /// it is not, nothing can be said to cover *the document's* signatures.
    pub complete: bool,
    /// How many entries the `/DSS` holds already, as its `/Certs`, `/OCSPs`
    /// and `/CRLs` arrays list them: the reader takes a bounded number of
    /// each (`revocation::MAX_DSS_CERTIFICATES`, `MAX_RESPONSES`,
    /// `MAX_LISTS`), so what is added past that could not be read back.
    #[serde(default)]
    pub held: [usize; 3],
    /// Whether the reader already leaves some of the `/DSS` out, or cannot
    /// read some of it: every answer then reads as not checked.
    #[serde(default)]
    pub store_cut: bool,
    /// The DocMDP level the catalog's `/Perms` certification grants, 1 to 3,
    /// or zero: what `sign_prepare` refuses a further signature field by. A
    /// certification need not be a field of the form, so the fields'
    /// own levels do not answer for it.
    #[serde(default)]
    pub certified: u8,
}

impl Survey {
    /// How many bytes of DER it holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.values
            .iter()
            .map(|(_, value)| value.len())
            .chain(self.store.iter().map(Vec::len))
            .sum()
    }

    /// What every refusal of an encrypted document says, whichever process
    /// found it out: the worker that opened it, or the one that was started
    /// over it with no password and answered that it is locked.
    #[must_use]
    pub fn encrypted() -> Self {
        Survey::refusing(
            "this document is encrypted, and tpdf adds long-term validation data only to a \
             document that is not"
                .into(),
        )
    }

    /// A document that can take no validation data, and why.
    fn refusing(why: String) -> Self {
        Survey {
            refused: Some(why),
            ..Survey::default()
        }
    }
}

/// What `signed` --- a document somebody has already signed --- holds:
/// its signatures as the properties dialog reads them, and their values.
///
/// `pages` is what the scan reports as the page count, which nothing here
/// reads.
///
/// Refused, in [`Survey::refused`]: a document that does not parse strictly,
/// which could take no revision; an encrypted one, whose `/DSS` would be
/// written in the clear beside encrypted objects; and signatures whose
/// values together are over [`MAX_BYTES`], which the answer could not carry
/// back.
#[must_use]
pub fn survey(signed: &[u8], pages: u32) -> Survey {
    survey_within(signed, pages, MAX_BYTES)
}

/// [`survey`], with `bound` the most DER the answer may carry: split so a
/// test reaches the bound with a document of ordinary size.
fn survey_within(signed: &[u8], pages: u32, bound: usize) -> Survey {
    let document = match Document::load_mem_with_options(
        signed,
        lopdf::LoadOptions {
            // The strictness [`append`] will need of the same bytes: a
            // document it could not extend is refused here, before anything
            // is fetched for it.
            strict: true,
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    ) {
        Ok(document) => document,
        Err(e) => return Survey::refusing(format!("this document could not be parsed: {e}")),
    };
    if document.is_encrypted() || document.was_encrypted() {
        return Survey::encrypted();
    }
    // The `/DSS` as [`append`] will take it up: one it could not extend is
    // refused here, before anything is fetched for it.
    let held = match earlier(&document) {
        Ok(found) => found.arrays.map(|array| array.len()),
        Err(why) => return Survey::refusing(why),
    };
    let found = match crate::docinfo::scan_from(&document, signed, pages, None) {
        Ok(found) => found,
        Err(why) => return Survey::refusing(format!("this document could not be read: {why}")),
    };
    let values = crate::docinfo::signature_values(&document);
    let survey = Survey {
        refused: None,
        signatures: found.signatures,
        values: values.values,
        store: values.store,
        complete: values.complete,
        held,
        store_cut: values.store_cut,
        certified: crate::sign_prepare::certification(&document),
    };
    if survey.bytes() > bound {
        return Survey::refusing(format!(
            "this document's signatures and the certificates it already carries are {} bytes \
             together, more than the {bound} tpdf reads to add validation data",
            survey.bytes()
        ));
    }
    survey
}

/// The largest validation data the worker takes, in DER bytes.
///
/// The app process gathers at most `longterm::MAX_GATHERED`, which is this;
/// held here too, because the worker does not take the other side's word for
/// what it was sent.
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

/// Appends `gathered` to `signed` as a `/DSS` revision, reads the result, and
/// answers both.
///
/// `pages` is what the scan reports as the page count, which nothing here
/// reads.
///
/// # Errors
///
/// What [`append`] refuses, and a result the scan cannot parse at all.
pub fn extend(signed: &[u8], gathered: &Gathered, pages: u32) -> Result<Extended, String> {
    let update = append(signed, gathered)?;
    let mut whole = Vec::with_capacity(signed.len() + update.len());
    whole.extend_from_slice(signed);
    whole.extend_from_slice(&update);
    let found = crate::docinfo::scan(&whole, pages, None)?;
    Ok(Extended {
        update,
        built_against: signed.len(),
        signatures: found.signatures,
    })
}

/// A document's `/DSS` as it stands: the dictionary, and its `/Certs`,
/// `/OCSPs` and `/CRLs` arrays, each resolved to the array it is.
struct Earlier {
    dictionary: Option<Dictionary>,
    arrays: [Vec<Object>; 3],
}

/// [`Earlier`] of a parsed document; empty arrays where it has no `/DSS`.
///
/// # Errors
///
/// A `/DSS` that is not a dictionary, or one of the three that is not an
/// array: a shape [`append`] cannot extend without dropping what it holds.
fn earlier(document: &Document) -> Result<Earlier, String> {
    let dictionary = match document
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"DSS").ok())
    {
        None => None,
        Some(found) => Some(
            resolve(document, found)
                .as_dict()
                .map_err(|_| "this document's /DSS is not a dictionary".to_string())?
                .clone(),
        ),
    };
    let carried = |key: &[u8]| -> Result<Vec<Object>, String> {
        let Some(dss) = &dictionary else {
            return Ok(Vec::new());
        };
        match dss.get(key) {
            Err(_) => Ok(Vec::new()),
            Ok(found) => resolve(document, found).as_array().cloned().map_err(|_| {
                format!(
                    "this document's /DSS /{} is not an array",
                    String::from_utf8_lossy(key)
                )
            }),
        }
    };
    let arrays = [carried(b"Certs")?, carried(b"OCSPs")?, carried(b"CRLs")?];
    Ok(Earlier { dictionary, arrays })
}

/// What the streams `array` names hold, each within `bound` as the reader
/// decodes it (`docinfo::dss_content`). One that is no stream, or will not
/// decode, holds nothing anybody could find a duplicate of.
fn contents(document: &Document, array: &[Object], bound: usize) -> Vec<Vec<u8>> {
    array
        .iter()
        .filter_map(|item| resolve(document, item).as_stream().ok())
        .filter_map(|stream| crate::docinfo::dss_content(stream, bound))
        .collect()
}

/// The update section that gives `signed`'s catalog a `/DSS` holding
/// `gathered`, beside whatever `/DSS` it already has. An entry the `/DSS`
/// already holds byte for byte is not written again; nothing it holds is
/// rewritten or dropped.
///
/// # Errors
///
/// Nothing to add, more than [`MAX_BYTES`], a document that does not parse
/// strictly or was encrypted (a signed copy tpdf made is neither), a catalog
/// that is not an object of its own, or an existing `/DSS` of a shape this
/// cannot extend.
pub fn append(signed: &[u8], gathered: &Gathered) -> Result<Vec<u8>, String> {
    if gathered.responses.is_empty() && gathered.lists.is_empty() {
        return Err("there is no revocation data to add".into());
    }
    if gathered.bytes() > MAX_BYTES {
        return Err(format!(
            "the validation data is {} bytes, more than the {MAX_BYTES} tpdf adds",
            gathered.bytes()
        ));
    }
    let was = signed.len();
    let prev = Document::load_mem_with_options(
        signed,
        lopdf::LoadOptions {
            // An update needs a real `/Prev`, as `sign_prepare` says.
            strict: true,
            max_decompressed_size: Some(MAX_DECODE),
            ..Default::default()
        },
    )
    .map_err(|e| format!("the signed document could not be parsed: {e}"))?;
    if prev.is_encrypted() || prev.was_encrypted() {
        return Err("an encrypted document is not signed by tpdf, and gets no /DSS".into());
    }
    let root = prev
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| "the document's catalog is not an object of its own".to_string())?;
    let mut catalog = prev
        .get_object(root)
        .and_then(Object::as_dict)
        .map_err(|_| "the document's catalog is not a dictionary".to_string())?
        .clone();
    // What an earlier writer left: its arrays, each resolved to the array it
    // is, so the new dictionary carries them over by value.
    let Earlier {
        dictionary,
        arrays: [mut certs, mut ocsps, mut crls],
    } = earlier(&prev)?;
    // What those arrays' streams hold, so that nothing is written a second
    // time: a later run gathers the same certificates again, and the reader
    // takes a bounded number of streams of each kind.
    let mut there = [
        (&certs, crate::trust::MAX_CERTIFICATE_BYTES),
        (&ocsps, crate::revocation::MAX_RESPONSE_BYTES),
        (&crls, crate::revocation::MAX_LIST_BYTES),
    ]
    .map(|(array, bound)| contents(&prev, array, bound));
    let mut dss = dictionary.unwrap_or_default();

    let mut incremental = IncrementalDocument::create_from(signed.to_vec(), prev);
    let doc = &mut incremental.new_document;
    let mut add = |into: &mut Vec<Object>, there: &mut Vec<Vec<u8>>, items: &[Vec<u8>]| {
        for item in items {
            if there.contains(item) {
                continue;
            }
            there.push(item.clone());
            into.push(Object::Reference(
                doc.add_object(Stream::new(Dictionary::new(), item.clone())),
            ));
        }
    };
    let [certs_there, ocsps_there, crls_there] = &mut there;
    add(&mut certs, certs_there, &gathered.certificates);
    add(&mut ocsps, ocsps_there, &gathered.responses);
    add(&mut crls, crls_there, &gathered.lists);
    for (key, items) in [("Certs", certs), ("OCSPs", ocsps), ("CRLs", crls)] {
        if !items.is_empty() {
            dss.set(key, Object::Array(items));
        }
    }
    let dss = doc.add_object(dss);
    catalog.set("DSS", dss);
    doc.set_object(root, catalog);

    let mut sink = crate::save::Tail {
        skip: was,
        seen: 0,
        tail: Vec::with_capacity(gathered.bytes() + 4096),
    };
    // An earlier `/DSS` is carried into the new one key for key, and it is
    // the document's: one `lopdf` would leave out is a refusal.
    crate::save::written_whole(&incremental)?;
    incremental
        .save_to(&mut sink)
        .map_err(|e| format!("could not build the validation data's revision: {e}"))?;
    Ok(sink.tail)
}

#[cfg(test)]
mod tests;
