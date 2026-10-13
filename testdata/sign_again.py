#!/usr/bin/env python3
"""Appends a second signature, by a key made for the run, to a signed PDF.

    uv run --with pyhanko testdata/sign_again.py IN.pdf OUT.pdf

For `sign-probe --lock`, which needs a later change that a validator permits
after an ordinary signature and forbids after one that locked the document.
A further signature is that change: pyHanko's difference analysis classes it
as form filling, which an approval signature allows and DocMDP level 1 does
not. Two changes that look as if they would do, and were measured not to
(pyHanko 0.36.2):

- a page object rewritten for a new annotation reads as violated after *any*
  signature, so both documents turn red;
- a new empty, invisible signature field reads as `LTA_UPDATES` --- the place
  a document timestamp goes --- and is permitted after a lock, so both stay
  green.

The signature is invisible, in a new field called `Later`. The key and its
self-made certificate exist only in this process. **Written with pyHanko's
writer and not tpdf's**, because tpdf refuses to sign a locked document, which
is its own row in the probe.
"""

import datetime
import sys


def throwaway():
    """A signer with a fresh P-256 key and a self-made certificate."""
    from asn1crypto import keys as asn1_keys
    from asn1crypto import x509 as asn1_x509
    from cryptography import x509
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    from cryptography.x509.oid import NameOID
    from pyhanko.sign import signers
    from pyhanko_certvalidator.registry import SimpleCertificateStore

    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Later Signer")])
    now = datetime.datetime.now(datetime.timezone.utc)
    certificate = (
        x509.CertificateBuilder()
        .subject_name(name)
        .issuer_name(name)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(minutes=5))
        .not_valid_after(now + datetime.timedelta(days=1))
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
    return signers.SimpleSigner(
        signing_cert=asn1_x509.Certificate.load(
            certificate.public_bytes(serialization.Encoding.DER)
        ),
        signing_key=asn1_keys.PrivateKeyInfo.load(
            key.private_bytes(
                serialization.Encoding.DER,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        ),
        cert_registry=SimpleCertificateStore(),
    )


def main(argv: "list[str]") -> int:
    """Writes `argv[2]` as `argv[1]` with one more signature."""
    if len(argv) != 3:
        print("usage: sign_again.py IN.pdf OUT.pdf", file=sys.stderr)
        return 2
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
    from pyhanko.sign import signers

    with open(argv[1], "rb") as handle:
        writer = IncrementalPdfFileWriter(handle, strict=False)
        with open(argv[2], "wb") as out:
            signers.sign_pdf(
                writer,
                signers.PdfSignatureMetadata(field_name="Later"),
                signer=throwaway(),
                output=out,
            )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
