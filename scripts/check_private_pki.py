"""Independently verify private-PKI integration artifacts, without network or OS trust changes.

Generate into a NEW directory, then check it (Windows: set the environment variable
with $env:TPDF_PKI_OUT and use an absolute Windows path):
  TPDF_PKI_OUT="$PWD/scratch/private-pki" cargo test --locked \
    --manifest-path src-tauri/Cargo.toml --test cli -- --filter 'sign --long-term'
  uv run --with pyhanko python scripts/check_private_pki.py scratch/private-pki

The DER files are public test roots, used only in this verifier's memory. No keys
are written or installed. Three positive cases must pass with embedded revocation
evidence alone. Removing that evidence must fail trust, and changing covered bytes
must invalidate both the document signature and the archive timestamp.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import logging
from pathlib import Path
import re

from asn1crypto import crl, ocsp, x509
from pyhanko.pdf_utils.reader import PdfFileReader
from pyhanko.sign.validation import (
    DocumentSecurityStore, KeyUsageConstraints, validate_pdf_signature, validate_pdf_timestamp,
)
from pyhanko_certvalidator import ValidationContext


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def check_case(directory: Path) -> dict:
    data = (directory / 'signed.pdf').read_bytes()
    roots = [x509.Certificate.load((directory / name).read_bytes())
             for name in ('signer-root.der', 'timestamp-root.der')]

    def read(blob: bytes):
        reader = PdfFileReader(io.BytesIO(blob))
        certificates, responses, lists = [], [], []
        if '/DSS' in reader.root:
            dss = DocumentSecurityStore.read_dss(reader)
            certificates = [x509.Certificate.load(ref.get_object().data) for ref in dss.certs.values()]
            responses = [ocsp.OCSPResponse.load(ref.get_object().data) for ref in dss.ocsps]
            lists = [crl.CertificateList.load(ref.get_object().data) for ref in dss.crls]

        def context():
            return ValidationContext(trust_roots=roots, allow_fetching=False,
                                     revocation_mode='hard-fail', other_certs=certificates,
                                     ocsps=responses, crls=lists)

        signatures = reader.embedded_signatures
        require(bool(signatures), 'no signatures found')
        statuses = []
        for signature in signatures:
            if signature.sig_object_type == '/DocTimeStamp':
                statuses.append(validate_pdf_timestamp(signature, validation_context=context()))
            else:
                # Synthetic certificates state no key usage; do not invent one.
                statuses.append(validate_pdf_signature(
                    signature, signer_validation_context=context(), ts_validation_context=context(),
                    key_usage_settings=KeyUsageConstraints(key_usage=set())))
        return reader, signatures, statuses, len(responses), len(lists)

    reader, signatures, statuses, responses, lists = read(data)
    require(len(signatures) == 2 and signatures[0].sig_object_type == '/Sig'
            and signatures[1].sig_object_type == '/DocTimeStamp', 'expected a signature followed by an archive timestamp')
    require((lists > 0 and responses == 0) if directory.name == 'crl'
            else (responses > 0 and lists == 0), 'revocation evidence does not match the fixture case')
    require(all(s.intact and s.valid and s.trusted for s in statuses), 'a signed revision is not intact and trusted')
    require(statuses[0].docmdp_ok and statuses[0].coverage.name == 'ENTIRE_REVISION'
            and statuses[1].coverage.name == 'ENTIRE_FILE', 'signed revision coverage or permitted changes differ')
    timestamp = statuses[0].timestamp_validity
    require(timestamp is not None and timestamp.intact and timestamp.valid and timestamp.trusted,
            'signature timestamp is not intact and trusted')

    # These controlled files have four revisions: input, signature, DSS, archive.
    ends = [match.end() for match in re.finditer(rb'(?m)^%%EOF(?:\r?\n|$)', data)]
    require(len(ends) == 4 and reader.total_revisions == 4, 'unexpected fixture revision layout')
    without_dss = data[:ends[1]]
    bare_reader, bare_signatures, bare_statuses, bare_ocsp, bare_crl = read(without_dss)
    require('/DSS' not in bare_reader.root and bare_ocsp + bare_crl == 0
            and len(bare_signatures) == 1 and bare_statuses[0].intact and bare_statuses[0].valid
            and not bare_statuses[0].trusted, 'missing-evidence control must remain intact but fail trust')

    target = b'0 0 m 10 10 l S'
    require(data.count(target) == 1, 'tamper control cannot locate its unique covered content')
    _, _, damaged, _, _ = read(data.replace(target, b'0 0 m 20 10 l S', 1))
    require(len(damaged) == 2 and all(not status.intact for status in damaged),
            'covered-byte tampering did not invalidate both signatures')
    return {'case': directory.name, 'ocsp': responses, 'crl': lists,
            'signature': statuses[0].summary(), 'archive': statuses[1].summary(),
            'missing_evidence_refused': True, 'tampering_refused': True,
            'files': {name: hashlib.sha256((directory / name).read_bytes()).hexdigest()
                      for name in ('signed.pdf', 'signer-root.der', 'timestamp-root.der', 'requests.json')}}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    # Negative controls deliberately provoke validation warnings; assertions
    # below decide the result, and unexpected exceptions still fail the run.
    logging.getLogger('pyhanko').setLevel(logging.CRITICAL)
    logging.getLogger('pyhanko_certvalidator').setLevel(logging.CRITICAL)
    results = [check_case(args.directory / name) for name in ('ocsp', 'crl', 'intermediate')]
    (args.directory / 'verification.json').write_text(json.dumps(results, indent=2) + '\n', encoding='utf-8')
    print('[OK] three private-PKI outputs passed independent offline validation and both negative controls')


if __name__ == '__main__':
    main()
