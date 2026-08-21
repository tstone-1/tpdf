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



def _chain(common_name: str, issuer=None):
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
        .not_valid_before(now - timedelta(days=1))
        .not_valid_after(now + timedelta(days=3650))
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

    plain_path = os.path.join(args.outdir, "incr-encrypted-pw.pdf")
    base_plain = build_plain(2, "encrypted")
    unencrypted = plain_path + ".plain"
    with open(unencrypted, "wb") as handle:
        handle.write(base_plain)
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

    manifest_path = os.path.join(args.outdir, "incr-manifest.json")
    with open(manifest_path, "w", encoding="utf-8") as handle:
        json.dump(manifest, handle, indent=2, sort_keys=True)
    print(f"[OK] incr-manifest.json ({len(manifest)} fixtures)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
