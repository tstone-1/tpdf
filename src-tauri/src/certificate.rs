//! Reading certificates out of a CMS `SignedData`: which certificates a blob
//! carries, which of them is the signer's, and what that one says.
//!
//! ## Why this is its own module
//!
//! Four modules ask these questions of one signature. [`crate::docinfo`]
//! reports the certificate's rows; [`crate::integrity`] verifies under the
//! signer's key; [`crate::trust`] hands the signer and the rest of the set to
//! the operating system; [`crate::revocation`] looks for issuers among them.
//! They have to get **one** answer to "who signed" --- two copies of the match
//! would be two answers, and the one a verdict used could then differ from the
//! one the dialog names --- so the match lives once, here.
//!
//! Until 2026-10-05 it lived in `docinfo`, which also owns the report those
//! three verdicts are fields of. `docinfo` called them and they called back
//! into it for the certificate, so none of the four could be read, tested or
//! moved without the other three. This module is the part they all stand on,
//! and it stands on none of them: it names `cms`, `x509-cert` and `der`, and
//! nothing else in this crate. Keep it that way --- a `use crate::` appearing
//! here is the cycle coming back.
//!
//! ## Nothing here is a verdict
//!
//! Everything in this module is *reading*. No chain is built, no signature is
//! tested and no store is asked; [`Certificate`] says so at greater length.
//! `docinfo` re-exports the names it has always offered, so
//! `docinfo::Certificate` and `docinfo::parse_certificate` are still paths.

/// Longest reported value, in characters, before it is clipped.
///
/// One bound for a name out of a certificate and for an `/Info` value, which
/// is why `docinfo` clips by this constant too rather than by one of its own.
pub(crate) const MAX_VALUE_CHARS: usize = 512;

/// What the signing certificate says, as against what the signer typed.
///
/// Read out of the DER blob in `/Contents`. **Nothing here is verified.** No
/// chain is built, no issuer is looked up, no revocation list is consulted, and
/// the signature is never tested against the bytes it covers. A certificate is
/// a document like any other and states whatever its issuer put in it.
///
/// The reason it is worth reading at all is *provenance*, not validity.
/// [`crate::docinfo::Signature::name`] is free text the signer typed into the PDF; this is what
/// somebody put into a certificate and signed with a key. The two can disagree,
/// and on the fixtures here one of them is routinely empty while the other is
/// not --- `incr-signed.pdf` has no `/Name` and a certificate reading
/// `CN=tpdf spike 0.6 test signer`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Certificate {
    /// The subject's distinguished name, near enough RFC 4514 form.
    pub subject: String,
    /// The subject's common name on its own, empty when it has none.
    pub subject_cn: String,
    /// The issuer's distinguished name.
    pub issuer: String,
    /// The issuer's common name on its own.
    pub issuer_cn: String,
    /// The serial number as uppercase hex, no separators.
    pub serial: String,
    /// `notBefore`, formatted as every date in `docinfo` is.
    pub from: String,
    /// `notAfter`.
    pub until: String,
    /// Issuer and subject are the same name.
    ///
    /// A checked fact about two byte strings, and **not** a verdict: a
    /// self-issued certificate is how every root in every trust store starts,
    /// and it is also what an unvouched-for signer produces. Which of those
    /// this is cannot be decided without a trust store, which tpdf has not got.
    pub self_issued: bool,
    /// How many certificates the blob carried, this one included.
    pub chain: u32,
    /// The signer's certificate was identified by `SignerInfo.sid`.
    ///
    /// False when the blob held exactly one certificate and the identifier did
    /// not match it, in which case the only certificate present is reported
    /// because a set of one leaves nothing to choose between. A blob with
    /// several and no match reports no certificate at all rather than guessing.
    pub matched_signer: bool,
    /// The key usage extension, 2.5.29.15 --- what the *issuer* says this key
    /// may be used for, named in the order RFC 5280 §4.2.1.3 defines the bits.
    ///
    /// `None` is a certificate carrying no such extension, which places no
    /// limit at all; `Some` of an empty list is one that limits it to nothing.
    /// The two are different documents and are kept different here.
    pub key_usage: Option<Vec<String>>,
    /// The extended key usage extension, 2.5.29.37 --- the purposes the issuer
    /// named. Each is given its RFC 5280 name when it is one of the handful
    /// written out in [`purpose_name`], and as dotted digits otherwise, so an
    /// unrecognised purpose is reported rather than dropped.
    ///
    /// `None` and `Some(vec![])` differ for the same reason as above.
    pub extended_usage: Option<Vec<String>>,
    /// Basic constraints, 2.5.29.19: whether the certificate says it may issue
    /// others. `None` when the extension is absent.
    pub authority: Option<bool>,
    /// Extensions present but not decodable.
    ///
    /// Counted rather than swallowed, because a malformed key usage reported as
    /// an absent one reads as *"the issuer placed no limit"* --- which is the
    /// reassuring direction, and is a claim the certificate does not make.
    pub extensions_unread: u32,
}

/// The DER half of `docinfo::read_certificate`, split out so a test can hand
/// it bytes.
///
/// Public for `signature-probe`, which parses the blob **PDFium** handed it and
/// compares the result against what `docinfo` produced from `lopdf`'s. Two
/// readers reaching the same certificate is a statement neither module's own
/// tests can make, and the failure it guards against --- picking a different
/// signature's blob, and so showing the wrong signer --- is the worst one here.
pub fn parse_certificate(der_bytes: &[u8]) -> Option<Certificate> {
    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;
    use der::{Decode, Encode};

    let info = ContentInfo::from_der(der_bytes).ok()?;
    let signed: SignedData = info.content.decode_as().ok()?;
    let chain = certificates_of(&signed).len();
    let chain = u32::try_from(chain).unwrap_or(u32::MAX);
    let (certificate, matched_signer) = signer_certificate(&signed)?;

    let tbs = &certificate.tbs_certificate;
    let subject = distinguished_name(&tbs.subject);
    let issuer = distinguished_name(&tbs.issuer);
    let extensions = tbs.extensions.as_deref().unwrap_or(&[]);
    let mut unread = 0u32;
    Some(Certificate {
        subject_cn: common_name(&tbs.subject),
        issuer_cn: common_name(&tbs.issuer),
        self_issued: tbs.subject.to_der().ok() == tbs.issuer.to_der().ok(),
        subject,
        issuer,
        serial: hex_of(tbs.serial_number.as_bytes()),
        from: certificate_date(&tbs.validity.not_before),
        until: certificate_date(&tbs.validity.not_after),
        key_usage: key_usage(extensions, &mut unread),
        extended_usage: extended_usage(extensions, &mut unread),
        authority: authority(extensions, &mut unread),
        chain,
        matched_signer,
        extensions_unread: unread,
    })
}

/// Every X.509 certificate in a `SignedData`'s `certificates` set.
pub(crate) fn certificates_of(
    signed: &cms::signed_data::SignedData,
) -> Vec<&x509_cert::Certificate> {
    use cms::cert::CertificateChoices;

    signed
        .certificates
        .as_ref()
        .map(|set| {
            set.0
                .iter()
                .filter_map(|choice| match choice {
                    CertificateChoices::Certificate(certificate) => Some(certificate),
                    CertificateChoices::Other(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The signer's certificate, and whether `SignerInfo.sid` identified it.
///
/// **One implementation for the two readers of it** --- the certificate rows
/// and [`crate::integrity`], which verifies under this certificate's key. Two
/// copies of the match would be two answers to "who signed", and the one the
/// verdict used could then differ from the one the dialog names.
///
/// `(certificate, false)` is the set-of-one case: reported for reading, and
/// refused by the verdict, which needs the key the signature *names*.
pub(crate) fn signer_certificate(
    signed: &cms::signed_data::SignedData,
) -> Option<(&x509_cert::Certificate, bool)> {
    use cms::signed_data::SignerIdentifier;
    use der::Encode;

    let certificates = certificates_of(signed);
    if certificates.is_empty() {
        return None;
    }

    let wanted = signed
        .signer_infos
        .0
        .as_slice()
        .first()
        .map(|info| &info.sid);
    let matched = wanted.and_then(|sid| {
        certificates.iter().copied().find(|certificate| match sid {
            SignerIdentifier::IssuerAndSerialNumber(both) => {
                certificate.tbs_certificate.serial_number == both.serial_number
                    && certificate.tbs_certificate.issuer.to_der().ok() == both.issuer.to_der().ok()
            }
            SignerIdentifier::SubjectKeyIdentifier(key) => {
                subject_key_identifier(certificate).is_some_and(|ski| ski == key.0.as_bytes())
            }
        })
    });

    // A set of one leaves nothing to choose between, so an identifier that does
    // not match it is a disagreement about naming rather than an ambiguity ---
    // report the certificate and say the match failed. Several with no match is
    // a genuine ambiguity and reports nothing.
    match (matched, certificates.as_slice()) {
        (Some(certificate), _) => Some((certificate, true)),
        (None, [only]) => Some((*only, false)),
        (None, _) => None,
    }
}

/// One extension's octets, by OID, and whether it was there at all.
///
/// The OIDs are written out at each call site rather than pulled from
/// `const_oid`'s database, which is the same choice [`subject_key_identifier`]
/// makes and for the same reason: the dependency stays to what is parsed.
fn extension_bytes<'a>(extensions: &'a [x509_cert::ext::Extension], oid: &str) -> Option<&'a [u8]> {
    extensions
        .iter()
        .find(|extension| extension.extn_id.to_string() == oid)
        .map(|extension| extension.extn_value.as_bytes())
}

/// Decodes one extension, counting a present-but-malformed one.
///
/// The count is what stops a malformed extension reading as an absent one. For
/// key usage those are opposite claims --- absent places no limit, malformed
/// places an unknown one --- and absent is the reassuring branch.
fn decode_extension<T>(
    extensions: &[x509_cert::ext::Extension],
    oid: &str,
    unread: &mut u32,
) -> Option<T>
where
    // Not `Decode<'static>`: that bound is satisfiable here only by leaking the
    // bytes, which on attacker-chosen input is a leak an attacker sizes. The
    // three types decoded here own everything they keep, so they decode from
    // any lifetime and the borrow ends with the call.
    T: for<'a> der::Decode<'a>,
{
    let bytes = extension_bytes(extensions, oid)?;
    match T::from_der(bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            *unread += 1;
            None
        }
    }
}

/// What the issuer says this key is for --- 2.5.29.15.
fn key_usage(extensions: &[x509_cert::ext::Extension], unread: &mut u32) -> Option<Vec<String>> {
    use x509_cert::ext::pkix::{KeyUsage, KeyUsages};

    let usage: KeyUsage = decode_extension(extensions, "2.5.29.15", unread)?;
    // Listed in the order RFC 5280 numbers the bits, so two certificates with
    // the same usage read the same way round.
    let named = [
        (KeyUsages::DigitalSignature, "Digital signature"),
        (KeyUsages::NonRepudiation, "Non-repudiation"),
        (KeyUsages::KeyEncipherment, "Key encipherment"),
        (KeyUsages::DataEncipherment, "Data encipherment"),
        (KeyUsages::KeyAgreement, "Key agreement"),
        (KeyUsages::KeyCertSign, "Certificate signing"),
        (KeyUsages::CRLSign, "CRL signing"),
        (KeyUsages::EncipherOnly, "Encipher only"),
        (KeyUsages::DecipherOnly, "Decipher only"),
    ];
    Some(
        named
            .into_iter()
            .filter(|(bit, _)| usage.0.contains(*bit))
            .map(|(_, name)| name.to_string())
            .collect(),
    )
}

/// The purposes the issuer named --- 2.5.29.37.
fn extended_usage(
    extensions: &[x509_cert::ext::Extension],
    unread: &mut u32,
) -> Option<Vec<String>> {
    use x509_cert::ext::pkix::ExtendedKeyUsage;

    let usage: ExtendedKeyUsage = decode_extension(extensions, "2.5.29.37", unread)?;
    Some(
        usage
            .0
            .iter()
            .map(|oid| purpose_name(&oid.to_string()))
            .collect(),
    )
}

/// An extended key usage OID's name, or the OID itself.
///
/// Only the purposes RFC 5280 §4.2.1.12 defines are named, plus the wildcard.
/// Anything else is returned as dotted digits: a purpose nobody here has heard
/// of is a fact about the certificate, and dropping it would be the one outcome
/// that reads as *"the issuer named nothing"*.
fn purpose_name(oid: &str) -> String {
    match oid {
        "2.5.29.37.0" => "Any purpose",
        "1.3.6.1.5.5.7.3.1" => "TLS server",
        "1.3.6.1.5.5.7.3.2" => "TLS client",
        "1.3.6.1.5.5.7.3.3" => "Code signing",
        "1.3.6.1.5.5.7.3.4" => "Email protection",
        "1.3.6.1.5.5.7.3.8" => "Time stamping",
        "1.3.6.1.5.5.7.3.9" => "OCSP signing",
        other => return other.to_string(),
    }
    .to_string()
}

/// Whether the certificate says it may issue others --- 2.5.29.19.
fn authority(extensions: &[x509_cert::ext::Extension], unread: &mut u32) -> Option<bool> {
    use x509_cert::ext::pkix::BasicConstraints;

    let constraints: BasicConstraints = decode_extension(extensions, "2.5.29.19", unread)?;
    Some(constraints.ca)
}

/// The subject key identifier extension's octets, when the certificate has one.
fn subject_key_identifier(certificate: &x509_cert::Certificate) -> Option<Vec<u8>> {
    use der::{asn1::OctetString, Decode};

    certificate
        .tbs_certificate
        .extensions
        .as_ref()?
        .iter()
        // 2.5.29.14, the subject key identifier, written out rather than pulled
        // from a constant database so the dependency stays to what is parsed.
        .find(|extension| extension.extn_id.to_string() == "2.5.29.14")
        .and_then(|extension| OctetString::from_der(extension.extn_value.as_bytes()).ok())
        .map(|octets| octets.as_bytes().to_vec())
}

/// A distinguished name, near enough RFC 4514: `CN=Someone, O=Something`.
///
/// Written out rather than taken from `RdnSequence`'s own `Display`, because
/// that one escapes for round-tripping and this string is read by a person.
pub(crate) fn distinguished_name(name: &x509_cert::name::Name) -> String {
    let mut parts: Vec<String> = Vec::new();
    for rdn in name.0.iter() {
        for attribute in rdn.0.as_slice() {
            let value = attribute_text(attribute);
            if value.is_empty() {
                continue;
            }
            parts.push(format!(
                "{}={}",
                short_oid(&attribute.oid.to_string()),
                value
            ));
        }
    }
    parts.join(", ")
}

/// The common name alone, which is what a person reads as "who signed this".
pub(crate) fn common_name(name: &x509_cert::name::Name) -> String {
    for rdn in name.0.iter() {
        for attribute in rdn.0.as_slice() {
            if attribute.oid.to_string() == "2.5.4.3" {
                return attribute_text(attribute);
            }
        }
    }
    String::new()
}

/// The short label for the attribute types a certificate actually uses.
///
/// Anything else keeps its numeric form, which is honest --- a made-up
/// abbreviation would read as a standard one.
fn short_oid(oid: &str) -> &str {
    match oid {
        "2.5.4.3" => "CN",
        "2.5.4.6" => "C",
        "2.5.4.7" => "L",
        "2.5.4.8" => "ST",
        "2.5.4.10" => "O",
        "2.5.4.11" => "OU",
        "2.5.4.5" => "SERIALNUMBER",
        "1.2.840.113549.1.9.1" => "E",
        other => other,
    }
}

/// One name attribute's text.
///
/// A directory string is one of five ASN.1 string types and the encoding is the
/// issuer's choice. `BMPString` is UTF-16BE and is what Windows certificate
/// authorities emit, so it is decoded rather than shown as interleaved nulls;
/// the rest carry their text as bytes. Anything undecodable comes back lossily
/// rather than empty, because a mangled name still tells a reader who it is not.
pub(crate) fn attribute_text(attribute: &x509_cert::attr::AttributeTypeAndValue) -> String {
    use der::Tagged as _;

    let bytes = attribute.value.value();
    // Tag 0x1E is BMPString.
    let text = if attribute.value.tag().number().value() == 0x1e {
        let wide: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&wide)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    clip_text(text.trim())
}

/// `notBefore` / `notAfter`, in the shape `docinfo::format_date` produces.
pub(crate) fn certificate_date(time: &x509_cert::time::Time) -> String {
    let at = time.to_date_time();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minutes(),
        at.seconds()
    )
}

/// Uppercase hex, which is how every other tool prints a serial.
pub(crate) fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes.iter().take(MAX_VALUE_CHARS / 2) {
        let _ = write!(out, "{byte:02X}");
    }
    out
}

/// Bounds one string, on the same rule the `/Info` values use.
fn clip_text(text: &str) -> String {
    if text.chars().count() <= MAX_VALUE_CHARS {
        return text.to_string();
    }
    text.chars().take(MAX_VALUE_CHARS).collect()
}

/// A time, formatted as every date in `docinfo` is.
#[must_use]
pub fn format_time(at: u64) -> String {
    match der::DateTime::from_unix_duration(std::time::Duration::from_secs(at)) {
        Ok(at) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            at.year(),
            at.month(),
            at.day(),
            at.hour(),
            at.minutes(),
            at.seconds()
        ),
        Err(_) => String::new(),
    }
}
