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

/// The update section that gives `signed`'s catalog a `/DSS` holding
/// `gathered`, beside whatever `/DSS` it already has.
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
    let earlier = match catalog.get(b"DSS") {
        Err(_) => None,
        Ok(found) => Some(
            resolve(&prev, found)
                .as_dict()
                .map_err(|_| "this document's /DSS is not a dictionary".to_string())?
                .clone(),
        ),
    };
    let carried = |key: &[u8]| -> Result<Vec<Object>, String> {
        let Some(dss) = &earlier else {
            return Ok(Vec::new());
        };
        match dss.get(key) {
            Err(_) => Ok(Vec::new()),
            Ok(found) => resolve(&prev, found).as_array().cloned().map_err(|_| {
                format!(
                    "this document's /DSS /{} is not an array",
                    String::from_utf8_lossy(key)
                )
            }),
        }
    };
    let (mut certs, mut ocsps, mut crls) =
        (carried(b"Certs")?, carried(b"OCSPs")?, carried(b"CRLs")?);
    let mut dss = earlier.unwrap_or_default();

    let mut incremental = IncrementalDocument::create_from(signed.to_vec(), prev);
    let doc = &mut incremental.new_document;
    let mut add = |into: &mut Vec<Object>, items: &[Vec<u8>]| {
        for item in items {
            into.push(Object::Reference(
                doc.add_object(Stream::new(Dictionary::new(), item.clone())),
            ));
        }
    };
    add(&mut certs, &gathered.certificates);
    add(&mut ocsps, &gathered.responses);
    add(&mut crls, &gathered.lists);
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
