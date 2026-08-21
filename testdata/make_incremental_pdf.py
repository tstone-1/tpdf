#!/usr/bin/env python3
"""Generates the corpus for the Phase 0 incremental-save spike (0.6).

The spike asks whether tpdf can append a real PDF update section that other
readers accept, what that costs against a full rewrite, and what it does to the
two things an append is supposed to protect: an existing digital signature and
an existing encryption dictionary (PLAN.md section 5).

The existing corpora do not cover it. `make_text_pdf.py` writes small
single-revision files with classic cross-reference tables; `make_hostile_pdf.py`
writes deliberate leaks. Neither has a document large enough for "appending to a
300 MB scan is near-instant" to mean anything, a document whose page dictionary
lives inside an object stream, or a signed document at all.

  scan-<N>p     N pages, each one uncompressed 300-dpi grayscale image. Sizes
                the append-versus-rewrite comparison across three orders of
                magnitude, up to the 300 MB the plan actually claims. The pixels
                are pseudo-random from a fixed seed, so nothing downstream can
                make the file cheap by compressing it away.
  xrefstream    775 pages behind an xref stream with every page dictionary
                packed into an /ObjStm. An update to this file may not append a
                classic xref table, and the object being edited is not at any
                byte offset of its own -- both are cases the small hand-written
                fixtures cannot reach. Derived from text-heavy.pdf via qpdf.
  signed        An approval signature over a two-page document. Appending must
                leave its ByteRange bytes untouched.
  certified-1   A certification signature with DocMDP permission 1: no changes
                permitted at all. The case PLAN.md section 5 classifies as
                Forbidden.
  certified-2   DocMDP permission 2: form filling and signing permitted, nothing
                else. The case that distinguishes "a signature is present" from
                "this particular edit is forbidden".
  certified-3   DocMDP permission 3: annotations permitted as well. Without this
                one the corpus could only show that every edit is refused, which
                would make "signed means forbidden" look proven when what is
                actually true is that it depends on what the edit touches.
  encrypted-pw  AES-256 behind a real user password, so opening it requires one.
                hostile-encrypted.pdf has an *empty* user password and therefore
                opens with no prompt, which exercises the opposite branch.

The signed fixtures need `pyhanko`, which is a *test oracle*, not a dependency
of tpdf -- it both writes the signatures and is the only implementation here
that can validate them. Run under `uv run --with pyhanko`. Without it the three
signed fixtures are skipped and the rest are still written.

Everything here is gitignored. Usage:
    uv run --with pyhanko --with pyhanko-certvalidator \\
        testdata/make_incremental_pdf.py [outdir]
"""

import argparse
import json
import os
import random
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from make_text_pdf import HEIGHT, WIDTH, Pdf, escape  # noqa: E402

# 300 dpi over US Letter. One page is 2550 * 3300 bytes = 8.0 MiB uncompressed,
# which is what a real grayscale scan of a page costs before anyone compresses
# it, and is why scanned documents reach hundreds of megabytes.
SCAN_DPI = 300
SCAN_W = int(8.5 * SCAN_DPI)
SCAN_H = int(11 * SCAN_DPI)

# Page counts for the size sweep. 40 pages lands on the 300 MB that PLAN.md
# section 5 names, and the two smaller ones bracket it an order of magnitude
# apart so the cost can be seen to scale rather than merely be large once.
SCAN_PAGES = (5, 20, 40)


def build_scan(pages: int, seed: int = 20260726) -> bytes:
    """Builds a synthetic scanned document: `pages` full-page grayscale images.

    The image data is deliberately not compressed and not compressible. A scan
    fixture that deflates to nothing would make a full rewrite look cheap for a
    reason no real document provides.
    """
    pdf = Pdf()
    rng = random.Random(seed)

    page_ids = [pdf.reserve() for _ in range(pages)]
    pages_id = pdf.reserve()
    kids = b" ".join(b"%d 0 R" % pid for pid in page_ids)

    for index, page_id in enumerate(page_ids):
        # `randbytes` rather than `urandom`: same incompressibility, an order of
        # magnitude faster, and seeded so the corpus is reproducible.
        image = pdf.stream(
            b"<< /Type /XObject /Subtype /Image"
            b" /Width %d /Height %d"
            b" /ColorSpace /DeviceGray /BitsPerComponent 8 >>" % (SCAN_W, SCAN_H),
            rng.randbytes(SCAN_W * SCAN_H),
            compress=False,
        )
        # A caption in a base-14 font, so the page has something a text
        # extractor can find and a reader can be seen to have rendered.
        content = pdf.stream(
            b"<< >>",
            b"q %d 0 0 %d 0 0 cm /Im0 Do Q\n"
            b"BT /F1 24 Tf 40 %d Td (%s) Tj ET"
            % (WIDTH, HEIGHT, HEIGHT - 40, escape(f"scan page {index + 1}")),
        )
        font = pdf.add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
        pdf.put(
            page_id,
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d]"
            b" /Resources << /XObject << /Im0 %d 0 R >> /Font << /F1 %d 0 R >> >>"
            b" /Contents %d 0 R >>" % (pages_id, WIDTH, HEIGHT, image, font, content),
        )

    pdf.put(pages_id, b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, pages))
    catalog = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id)
    return pdf.serialize(catalog)


def build_plain(pages: int, label: str, indirect_annots: bool = False) -> bytes:
    """Builds a small ordinary document, used as the base for the signatures.

    Small on purpose: a signature fixture is about the signature, and a large
    one would make every validation run slow for no added coverage.

    `indirect_annots` gives page 1 an `/Annots` that is a reference to its own
    array object rather than an array written inline in the page dictionary.
    Both shapes are common in the wild, and they are not interchangeable for a
    signed document: adding an annotation to an inline `/Annots` means rewriting
    the page dictionary, which a difference analysis sees as a structural change
    it cannot justify, while extending a referenced array touches nothing the
    page dictionary says.
    """
    pdf = Pdf()
    page_ids = [pdf.reserve() for _ in range(pages)]
    pages_id = pdf.reserve()
    kids = b" ".join(b"%d 0 R" % pid for pid in page_ids)
    font = pdf.add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    annots_id = pdf.add(b"[ ]") if indirect_annots else None

    for index, page_id in enumerate(page_ids):
        content = pdf.stream(
            b"<< >>",
            b"BT /F1 18 Tf 60 %d Td (%s) Tj ET\n"
            b"0.2 w 60 %d m %d %d l S"
            % (
                HEIGHT - 80,
                escape(f"{label} page {index + 1} of {pages}"),
                HEIGHT - 96,
                WIDTH - 60,
                HEIGHT - 96,
            ),
        )
        annots = (
            b" /Annots %d 0 R" % annots_id
            if annots_id is not None and index == 0
            else b""
        )
        pdf.put(
            page_id,
            b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d]"
            b" /Resources << /Font << /F1 %d 0 R >> >>"
            b"%s /Contents %d 0 R >>"
            % (pages_id, WIDTH, HEIGHT, font, annots, content),
        )

    pdf.put(pages_id, b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, pages))
    catalog = pdf.add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id)
    return pdf.serialize(catalog)


def build_xrefstream(outdir: str) -> "str | None":
    """Derives an xref-stream + /ObjStm document from text-heavy.pdf via qpdf.

    Hand-writing an 775-page object-stream document would be a generator in its
    own right, and qpdf produces a more representative one than we would: this
    is the shape every modern producer emits.
    """
    source = os.path.join(outdir, "text-heavy.pdf")
    if not os.path.exists(source):
        print(f"[SKIP] incr-xrefstream.pdf: {source} missing (run make_text_pdf.py)")
        return None
    target = os.path.join(outdir, "incr-xrefstream.pdf")
    subprocess.run(
        [
            "qpdf",
            "--object-streams=generate",
            # Keep the page content streams as they are, so the only difference
            # from text-heavy.pdf is the structure being tested.
            "--stream-data=preserve",
            source,
            target,
        ],
        check=True,
    )
    return target


def sign(source: bytes, out_path: str, certify: "int | None") -> bool:
    """Signs `source` with a throwaway self-signed key, writing `out_path`.

    `certify` is the DocMDP permission level, or None for an ordinary approval
    signature. Returns False when pyhanko is not installed.
    """
    try:
        import io

        from pyhanko.sign import fields, signers
        from pyhanko.sign.fields import MDPPerm
    except ImportError:
        return False

    from asn1crypto import keys as asn1_keys
    from asn1crypto import x509 as asn1_x509
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import rsa
    from datetime import datetime, timedelta, timezone

    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name(
        [x509.NameAttribute(x509.NameOID.COMMON_NAME, "tpdf spike 0.6 test signer")]
    )
    now = datetime.now(timezone.utc)
    cert = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - timedelta(days=1))
        .not_valid_after(now + timedelta(days=3650))
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=True,
                key_encipherment=False,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=False,
                crl_sign=False,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )

    signer = signers.SimpleSigner(
        signing_cert=asn1_x509.Certificate.load(
            cert.public_bytes(serialization.Encoding.DER)
        ),
        # pyhanko works in asn1crypto types throughout, so the key has to cross
        # over as DER rather than as a `cryptography` object.
        signing_key=asn1_keys.PrivateKeyInfo.load(
            key.private_bytes(
                serialization.Encoding.DER,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        ),
        cert_registry=None,
    )

    meta = signers.PdfSignatureMetadata(
        field_name="Signature1",
        certify=certify is not None,
        docmdp_permissions=MDPPerm(certify) if certify is not None else None,
    )

    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter

    writer = IncrementalPdfFileWriter(io.BytesIO(source))
    fields.append_signature_field(
        writer,
        fields.SigFieldSpec(sig_field_name="Signature1", on_page=0, box=(60, 60, 260, 120)),
    )
    with open(out_path, "wb") as handle:
        signers.sign_pdf(writer, meta, signer=signer, output=handle)
    return True



def _chain(common_name: str, issuer=None, not_before=None, not_after=None):
    """A certificate and its key, self-issued or signed by `issuer`.

    Returns `(cert, key)`. Passing `issuer` makes this a leaf under that
    authority, which is what gives a signature blob a `certificates` set of more
    than one -- the shape every fixture here lacked until 2026-08-21, and the one
    that makes "take the first certificate rather than the signer's" a mistake a
    test can see.
    """
    from datetime import datetime, timedelta, timezone

    from cryptography import x509
    from cryptography.hazmat.primitives import hashes
    from cryptography.hazmat.primitives.asymmetric import rsa

    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(x509.NameOID.COMMON_NAME, common_name)])
    now = datetime.now(timezone.utc)
    authority = issuer is not None
    builder = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(issuer[0].subject if authority else name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(not_before or now - timedelta(days=1))
        .not_valid_after(not_after or now + timedelta(days=3650))
        .add_extension(
            x509.BasicConstraints(ca=not authority, path_length=None), critical=True
        )
    )
    return builder.sign(issuer[1] if authority else key, hashes.SHA256()), key


def sign_twice(source: bytes, out_path: str) -> bool:
    """Signs `source` by two different signers, each carrying a chain.

    Every other signed fixture here is one signature whose blob holds one
    self-issued certificate, which makes four separate things untestable: the
    per-signature pairing in `signature-probe`, walking `/AcroForm /Fields` when
    it has more than one entry, picking the signer's certificate out of a set
    with something else in it, and a first signature whose byte range stops short
    because a second was appended after it.

    Both leaves are issued by one root, so the two blobs differ in the leaf and
    agree in the root -- a reader that takes the wrong element of the set reports
    the same name for both signatures.
    """
    try:
        import io

        from pyhanko.sign import fields, signers
    except ImportError:
        return False

    from asn1crypto import keys as asn1_keys
    from asn1crypto import x509 as asn1_x509
    from cryptography.hazmat.primitives import serialization
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter

    root = _chain("tpdf test root CA")

    def as_signer(cert, key):
        # It moved out of `pyhanko.sign.general` at some version; the
        # certvalidator registry is where it lives now.
        from pyhanko_certvalidator.registry import SimpleCertificateStore

        store = SimpleCertificateStore()
        store.register(
            asn1_x509.Certificate.load(root[0].public_bytes(serialization.Encoding.DER))
        )
        return signers.SimpleSigner(
            signing_cert=asn1_x509.Certificate.load(
                cert.public_bytes(serialization.Encoding.DER)
            ),
            signing_key=asn1_keys.PrivateKeyInfo.load(
                key.private_bytes(
                    serialization.Encoding.DER,
                    serialization.PrivateFormat.PKCS8,
                    serialization.NoEncryption(),
                )
            ),
            cert_registry=store,
        )

    current = source
    for index, who in enumerate(("First Signer", "Second Signer"), start=1):
        cert, key = _chain(who, issuer=root)
        field = f"Signature{index}"
        writer = IncrementalPdfFileWriter(io.BytesIO(current))
        fields.append_signature_field(
            writer,
            fields.SigFieldSpec(
                sig_field_name=field,
                on_page=0,
                box=(60, 60 + 70 * index, 260, 120 + 70 * index),
            ),
        )
        out = io.BytesIO()
        signers.sign_pdf(
            writer,
            signers.PdfSignatureMetadata(field_name=field),
            signer=as_signer(cert, key),
            output=out,
        )
        current = out.getvalue()

    with open(out_path, "wb") as handle:
        handle.write(current)
    return True



# The instant the dummy TSA attests, pinned so a test can assert it.
#
# Chosen rather than observed, which is the whole point: `datetime.now()` in a
# generator produces a fixture whose values a test may only assert the *shape*
# of -- learned once already here, when a serial and a date were pinned out of
# generated bytes and went red the first time anyone regenerated them.
TIMESTAMP_AT = "2026-08-21T12:00:00+00:00"

# The TSA's own certificate validity, pinned for the same reason.
TSA_FROM = "2026-01-01T00:00:00+00:00"
TSA_UNTIL = "2036-01-01T00:00:00+00:00"


def sign_with_timestamp(source: bytes, out_path: str) -> bool:
    """Signs `source` and has a dummy TSA attest the time.

    A signature's `/M` is whatever the signer's own computer clock said. It is
    free text in the signature dictionary, nothing checks it, and a machine with
    a wrong clock -- or a signer who wants a different date -- writes whatever it
    likes there. An **RFC 3161 timestamp token** is a third party's statement
    about when the signature existed, carried as an unsigned attribute on the
    `SignerInfo` under OID 1.2.840.113549.1.9.16.2.14.

    No fixture here had one, so the whole path was reached by nothing.
    `DummyTimeStamper` is pyhanko's offline TSA: it mints a real token with a
    real `TSTInfo` and signs it with a key made here, so the *structure* is
    exactly what a public TSA emits and the *trust* is nil. Nothing in tpdf
    verifies a timestamp, so that costs nothing -- but do not reach for this file
    to test anything about whether a time is to be believed.

    `fixed_dt` pins `genTime`, so a test may assert the instant rather than its
    shape.
    """
    try:
        import io

        from pyhanko.sign import fields, signers
        from pyhanko.sign.timestamps import DummyTimeStamper
    except ImportError:
        return False

    from datetime import datetime

    from asn1crypto import keys as asn1_keys
    from asn1crypto import x509 as asn1_x509
    from cryptography.hazmat.primitives import serialization

    def crossover(pair):
        """pyhanko works in asn1crypto types, so the pair has to cross over."""
        cert, key = pair
        return (
            asn1_x509.Certificate.load(cert.public_bytes(serialization.Encoding.DER)),
            asn1_keys.PrivateKeyInfo.load(
                key.private_bytes(
                    serialization.Encoding.DER,
                    serialization.PrivateFormat.PKCS8,
                    serialization.NoEncryption(),
                )
            ),
        )

    signer_cert, signer_key = crossover(_chain("tpdf timestamped signer"))
    tsa_cert, tsa_key = crossover(
        _chain(
            "tpdf dummy timestamp authority",
            not_before=datetime.fromisoformat(TSA_FROM),
            not_after=datetime.fromisoformat(TSA_UNTIL),
        )
    )

    signer = signers.SimpleSigner(
        signing_cert=signer_cert, signing_key=signer_key, cert_registry=None
    )
    stamper = DummyTimeStamper(
        tsa_cert=tsa_cert,
        tsa_key=tsa_key,
        fixed_dt=datetime.fromisoformat(TIMESTAMP_AT),
    )

    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter

    writer = IncrementalPdfFileWriter(io.BytesIO(source))
    fields.append_signature_field(
        writer,
        fields.SigFieldSpec(
            sig_field_name="Signature1", on_page=0, box=(60, 60, 260, 120)
        ),
    )
    meta = signers.PdfSignatureMetadata(field_name="Signature1")
    with open(out_path, "wb") as handle:
        signers.sign_pdf(
            writer, meta, signer=signer, timestamper=stamper, output=handle
        )
    return True


def build_nested_field(blob: bytes) -> bytes:
    """A document whose signature field hangs two levels down an `/AcroForm` tree.

    `/AcroForm /Fields` is a *tree*: an entry may be a field, or a node whose
    `/Kids` hold fields, and a fully qualified field name is the `/T` values
    joined down the chain. Producers that group fields --- Acrobat among them ---
    write it that way, and every signature fixture here put its field directly in
    `/Fields`, so `read_signatures`'s recursion and its depth bound were reached
    by nothing.

    **The signature is structurally real and cryptographically meaningless.** The
    `/Contents` blob is copied from an already-signed fixture, so both readers
    parse a genuine certificate out of it and can be compared; the `/ByteRange`
    describes a span of this file that the blob was never computed over. Nothing
    in tpdf verifies a signature, so this fixture cannot mislead it --- but do not
    reach for this file to test anything about validity, because there is none.
    """
    pdf = Pdf()
    page_id = pdf.reserve()
    pages_id = pdf.reserve()
    parent_id = pdf.reserve()
    middle_id = pdf.reserve()
    leaf_id = pdf.reserve()
    sig_id = pdf.reserve()

    font = pdf.add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    content = pdf.stream(
        b"<< >>",
        b"BT /F1 18 Tf 60 %d Td (%s) Tj ET"
        % (HEIGHT - 80, escape("a signature field nested under /Kids")),
    )
    pdf.put(
        page_id,
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R "
        b"/Annots [ %d 0 R ] >>"
        % (pages_id, WIDTH, HEIGHT, font, content, leaf_id),
    )
    pdf.put(
        pages_id,
        b"<< /Type /Pages /Kids [ %d 0 R ] /Count 1 >>" % page_id,
    )

    # Two nodes above the field, so a walk that handles one level and stops is
    # distinguishable from one that recurses.
    pdf.put(parent_id, b"<< /T (top) /Kids [ %d 0 R ] >>" % middle_id)
    pdf.put(middle_id, b"<< /T (group) /Parent %d 0 R /Kids [ %d 0 R ] >>" % (parent_id, leaf_id))
    pdf.put(
        leaf_id,
        b"<< /FT /Sig /T (Signature1) /Parent %d 0 R /V %d 0 R "
        b"/Type /Annot /Subtype /Widget /F 4 /P %d 0 R /Rect [60 60 260 120] >>"
        % (middle_id, sig_id, page_id),
    )
    pdf.put(
        sig_id,
        b"<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached "
        b"/Name (Nested Signer) /M (D:20260821120000+02'00') "
        b"/ByteRange [0 400 900 500] /Contents <" + blob.hex().upper().encode("ascii") + b"> >>",
    )

    catalog = pdf.add(
        b"<< /Type /Catalog /Pages %d 0 R "
        b"/AcroForm << /Fields [ %d 0 R ] /SigFlags 3 >> >>" % (pages_id, parent_id)
    )
    return pdf.serialize(catalog)


def signature_blob(path: str) -> "bytes | None":
    """The `/Contents` bytes of the first signature in an already-signed file."""
    import re

    try:
        with open(path, "rb") as handle:
            raw = handle.read()
    except OSError:
        return None
    found = re.search(rb"/Contents\s*<([0-9A-Fa-f]+)>", raw)
    if found is None:
        return None
    return bytes.fromhex(found.group(1).decode("ascii"))


def to_indefinite(der: bytes) -> bytes:
    """Rewrite every constructed value's length in BER's indefinite form.

    A signer that streams its output cannot know a value's length before it has
    written the value, so it writes `80` where the length belongs and a two-byte
    end-of-contents marker where the value stops. That is legal BER, it is not
    DER, and `der` refuses it -- which is why a real CAdES contract read as
    having no certificate at all until `ber.rs` was written.

    Converting rather than signing afresh is deliberate: the result differs from
    `incr-signed.pdf` in the *length encoding and nothing else*, so a check that
    reads the same certificate out of both is testing exactly that. pyHanko
    emits DER and has no switch for this, so there is no way to produce the pair
    other than by rewriting one into the other.
    """

    def convert(data: bytes, at: int) -> "tuple[bytes, int]":
        start = at
        tag = data[at]
        at += 1
        if tag & 0x1F == 0x1F:
            while data[at] & 0x80:
                at += 1
            at += 1
        identifier = data[start:at]
        length = data[at]
        at += 1
        if length & 0x80:
            count = length & 0x7F
            length = int.from_bytes(data[at : at + count], "big")
            at += count
        end = at + length
        if not tag & 0x20:
            return data[start:end], end
        body = b""
        cursor = at
        while cursor < end:
            piece, cursor = convert(data, cursor)
            body += piece
        return identifier + b"\x80" + body + b"\x00\x00", end

    converted, consumed = convert(der, 0)
    if consumed != len(der):
        raise ValueError("trailing bytes after the first value")
    return converted


def build_ber(source_path: str, out_path: str) -> bool:
    """`incr-signed.pdf` with its signature blob rewritten in indefinite form.

    The rewritten blob is padded back to the exact span the signer reserved, so
    every byte offset in the file is unchanged and `/ByteRange` and the xref
    stay correct. The signature does not verify, which is true of every fixture
    here and irrelevant: nothing in tpdf verifies one.
    """
    import re

    try:
        with open(source_path, "rb") as handle:
            raw = handle.read()
    except OSError:
        return False
    found = re.search(rb"/Contents\s*<([0-9A-Fa-f]+)>", raw)
    if found is None:
        return False
    digits = found.group(1)
    blob = bytes.fromhex(digits.decode("ascii"))
    last = max((index for index, byte in enumerate(blob) if byte), default=-1)
    if last < 0:
        return False
    rewritten = to_indefinite(blob[: last + 1])
    if len(rewritten) > len(blob):
        return False
    padded = rewritten + b"\x00" * (len(blob) - len(rewritten))
    encoded = padded.hex()
    if any(byte in b"ABCDEF" for byte in digits):
        encoded = encoded.upper()
    encoded = encoded.encode("ascii")
    assert len(encoded) == len(digits), "the reserved span must not move"
    with open(out_path, "wb") as handle:
        handle.write(raw[: found.start(1)] + encoded + raw[found.end(1) :])
    return True


def main(argv: "list[str] | None" = None) -> int:
    """Writes every fixture and a manifest describing what each one is for."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outdir", nargs="?", default="testdata")
    parser.add_argument(
        "--scan-pages",
        type=int,
        nargs="*",
        default=list(SCAN_PAGES),
        help="page counts for the synthetic scans (default: 5 20 80)",
    )
    args = parser.parse_args(argv)
    os.makedirs(args.outdir, exist_ok=True)

    manifest: "dict[str, dict[str, object]]" = {}

    for pages in args.scan_pages:
        name = f"incr-scan-{pages}p.pdf"
        path = os.path.join(args.outdir, name)
        # These reach several hundred megabytes and are deterministic, so an
        # existing one of the right size is reused rather than rewritten.
        if os.path.exists(path):
            size = os.path.getsize(path)
            print(f"[OK] {name} ({size / 1e6:.1f} MB, {pages} pages, already present)")
        else:
            data = build_scan(pages)
            with open(path, "wb") as handle:
                handle.write(data)
            size = len(data)
            print(f"[OK] {name} ({size / 1e6:.1f} MB, {pages} pages)")
        manifest[name] = {
            "role": "size sweep for append vs full rewrite",
            "pages": pages,
            "bytes": size,
            "xref": "table",
        }

    xref_path = build_xrefstream(args.outdir)
    if xref_path:
        manifest["incr-xrefstream.pdf"] = {
            "role": "xref stream and /ObjStm; an update may not append a table",
            "pages": 775,
            "bytes": os.path.getsize(xref_path),
            "xref": "stream",
        }
        print(f"[OK] incr-xrefstream.pdf ({os.path.getsize(xref_path) / 1e6:.1f} MB)")

    # qpdf is the only external program this script needs, and it is needed for
    # exactly one fixture. It used to be called with `check=True` and nothing
    # else, so a machine without it died here with a FileNotFoundError naming a
    # program rather than a fixture -- and died BEFORE the signed fixtures below,
    # which need only pyhanko. A hosted runner is such a machine, which is why
    # none of the signature work could be tested there. Skipping keeps the rest.
    plain_path = os.path.join(args.outdir, "incr-encrypted-pw.pdf")
    base_plain = build_plain(2, "encrypted")
    unencrypted = plain_path + ".plain"
    with open(unencrypted, "wb") as handle:
        handle.write(base_plain)
    try:
        subprocess.run(
            [
                "qpdf",
                "--encrypt",
                "--user-password=swordfish",
                "--owner-password=swordfish",
                "--bits=256",
                "--",
                unencrypted,
                plain_path,
            ],
            check=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError) as why:
        # The half-written input goes either way: leaving it turns a skipped
        # fixture into a stray file that looks like a fixture.
        os.remove(unencrypted)
        print(f"[SKIP] incr-encrypted-pw.pdf: qpdf did not run ({why})")
    else:
        os.remove(unencrypted)
        manifest["incr-encrypted-pw.pdf"] = {
            "role": "AES-256 behind a real user password",
            "pages": 2,
            "bytes": os.path.getsize(plain_path),
            "xref": "table",
            "password": "swordfish",
        }
        print(f"[OK] incr-encrypted-pw.pdf ({os.path.getsize(plain_path)} bytes)")

    inline = build_plain(2, "signed")
    indirect = build_plain(2, "signed", indirect_annots=True)
    signed_specs = [
        ("incr-signed.pdf", None, inline, "approval signature; append must not break it"),
        ("incr-certified-1.pdf", 1, inline, "DocMDP 1: no changes permitted"),
        ("incr-certified-2.pdf", 2, inline, "DocMDP 2: form filling and signing only"),
        ("incr-certified-3.pdf", 3, inline, "DocMDP 3: annotations permitted, /Annots inline"),
        (
            "incr-certified-3-indirect.pdf",
            3,
            indirect,
            "DocMDP 3 with /Annots as its own object, so an annotation can be "
            "added without rewriting the page dictionary",
        ),
    ]
    for name, certify, base, role in signed_specs:
        path = os.path.join(args.outdir, name)
        if not sign(base, path, certify):
            print(f"[SKIP] {name}: pyhanko not installed")
            continue
        manifest[name] = {
            "role": role,
            "pages": 2,
            "bytes": os.path.getsize(path),
            "xref": "table",
            "docmdp": certify,
        }
        print(f"[OK] {name} ({os.path.getsize(path)} bytes)")

    stamped_path = os.path.join(args.outdir, "incr-timestamped.pdf")
    if sign_with_timestamp(inline, stamped_path):
        manifest["incr-timestamped.pdf"] = {
            "role": "the only fixture whose signature carries an RFC 3161 "
            "timestamp token -- a third party's statement about when the "
            "signature existed, beside the /M the signer's own clock wrote. "
            "genTime is pinned to " + TIMESTAMP_AT + " so a test can assert "
            "the instant rather than its shape; the TSA is a dummy and "
            "nothing about the time is to be believed",
            "pages": 2,
            "bytes": os.path.getsize(stamped_path),
            "xref": "table",
            "docmdp": None,
            "signatures": 1,
            "timestamp": TIMESTAMP_AT,
            "timestamp_authority": "tpdf dummy timestamp authority",
        }
        print(f"[OK] incr-timestamped.pdf ({os.path.getsize(stamped_path)} bytes)")
    else:
        print("[SKIP] incr-timestamped.pdf: pyhanko not installed")

    two_path = os.path.join(args.outdir, "incr-two-signers.pdf")
    if sign_twice(inline, two_path):
        manifest["incr-two-signers.pdf"] = {
            "role": "two approval signatures by different signers, each blob "
            "carrying its leaf and the one root above it -- the only fixture "
            "where signature-probe's pairing, the /AcroForm /Fields walk past "
            "one entry, and picking the signer out of a set of two can fail",
            "pages": 2,
            "bytes": os.path.getsize(two_path),
            "xref": "table",
            "docmdp": None,
            "signatures": 2,
        }
        print(f"[OK] incr-two-signers.pdf ({os.path.getsize(two_path)} bytes)")
    else:
        print("[SKIP] incr-two-signers.pdf: pyhanko not installed")

    ber_path = os.path.join(args.outdir, "incr-ber.pdf")
    if build_ber(os.path.join(args.outdir, "incr-signed.pdf"), ber_path):
        manifest["incr-ber.pdf"] = {
            "role": "incr-signed.pdf with every constructed value in its "
            "signature blob rewritten in BER's indefinite-length form, and "
            "nothing else changed -- the pair is what makes a check of "
            "ber::to_definite_length discriminate. Real CAdES signers emit "
            "this and der refuses it, so before that module the certificate "
            "was unreadable",
            "pages": 1,
            "bytes": os.path.getsize(ber_path),
            "xref": "table",
            "docmdp": None,
            "signatures": 1,
        }
        print(f"[OK] incr-ber.pdf ({os.path.getsize(ber_path)} bytes)")
    else:
        print("[SKIP] incr-ber.pdf: incr-signed.pdf has no blob to rewrite")

    nested_path = os.path.join(args.outdir, "signed-nested-field.pdf")
    blob = signature_blob(os.path.join(args.outdir, "incr-signed.pdf"))
    if blob is None:
        print("[SKIP] signed-nested-field.pdf: incr-signed.pdf has no blob to borrow")
    else:
        with open(nested_path, "wb") as handle:
            handle.write(build_nested_field(blob))
        manifest["signed-nested-field.pdf"] = {
            "role": "the only fixture whose signature field hangs under /Kids, "
            "two levels down the /AcroForm field tree -- the recursion in "
            "read_signatures is reached by nothing else. Its /Contents is "
            "borrowed from incr-signed.pdf, so the certificate is real and the "
            "signature is cryptographically meaningless",
            "pages": 1,
            "bytes": os.path.getsize(nested_path),
            "xref": "table",
            "docmdp": None,
            "signatures": 1,
        }
        print(f"[OK] signed-nested-field.pdf ({os.path.getsize(nested_path)} bytes)")

    manifest_path = os.path.join(args.outdir, "incr-manifest.json")
    with open(manifest_path, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
    print(f"[OK] incr-manifest.json ({len(manifest)} fixtures)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
