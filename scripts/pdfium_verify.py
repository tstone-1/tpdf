#!/usr/bin/env python3
"""Require the complete RTL observation of an unpatched upstream build.

python scripts/pdfium_verify.py --fixtures DIR --observed observed.json

The observation comes from pdfium_rtl_check.py, which deliberately exits 1
because it reports the two known limitations too. This verifier accepts only
the exact, independently defined outcome, never a generic nonzero exit or a
reduced fixture inventory: every ordinary case extracts its authored logical
text, and each known limitation extracts exactly the wrong text pinned below.

Until 8066-tpdf.1 this compared an unpatched control against a candidate
carrying scripts/pdfium_rtl.patch. PDFium reverted the change that caused the
regression (crbug.com/561066233, pdfium-review 158290, on main and in
chromium/8059), and on 2026-09-25 unpatched chromium/8066 matched the patched
8044 build on the nine ordinary cases in text, indices, character boxes and
pixels, and on the multilingual (68/68) and encoding (23/23) search corpora.
So the patch and the control build went.

The limitations are pinned to what upstream extracts rather than excused as
"any wrong text", so that either direction of change is a finding: a fix
upstream turns them into ordinary cases, and a different wrong order is a
behaviour change worth reading before it ships. The patched 8044 build got
them wrong the other way round: the Latin word in place, the Hebrew words
reversed. Upstream keeps the Hebrew phrase intact and moves the Latin word to
the far end, so a Hebrew phrase in such a line stays searchable.
"""
import argparse
import json
from pathlib import Path

ORDINARY = {
    "arabic.pdf", "arabic-forms.pdf", "arabic-shaped.pdf", "arabic-latin.pdf",
    "hebrew.pdf", "hebrew-latin.pdf", "arabic-numbers.pdf", "latin.pdf", "latin-hebrew.pdf",
}
# Known wrong extractions, pinned exactly (escaped, so the source stays ASCII).
LIMITATIONS = {
    "latin-prefix.pdf": "שלום עולם היום Hello",
    "latin-suffix.pdf": "Hello שלום עולם היום",
}


def verify(manifest, observed):
    cases = manifest["cases"]
    names = [case["file"] for case in cases]
    inventory = ORDINARY | set(LIMITATIONS)
    if len(names) != len(inventory) or set(names) != inventory:
        raise ValueError("Expected the complete, unique 11-case fixture inventory")
    if set(observed) != inventory:
        raise ValueError("Observation inventory differs from the 11 fixtures")
    for case in cases:
        name = case["file"]
        seen = observed[name]
        if not seen["chars"] or not seen["text"]:
            raise ValueError(f"{name}: empty observation")
        if "".join(chr(c[0]) for c in seen["chars"]) != seen["text"]:
            raise ValueError(f"{name}: text and character indices disagree")
        if name in LIMITATIONS:
            if seen["text"] == case["expected"]:
                raise ValueError(f"{name}: known limitation now extracts correctly; move it to ORDINARY")
            if seen["text"] != LIMITATIONS[name]:
                raise ValueError(f"{name}: known limitation changed")
        elif seen["text"] != case["expected"]:
            raise ValueError(f"{name}: extracted text differs from the authored text")
    return {"cases": len(inventory), "ordinary_correct": len(ORDINARY),
            "known_limitations_unchanged": len(LIMITATIONS)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--observed", type=Path, required=True)
    args = parser.parse_args()
    result = verify(json.loads((args.fixtures / "manifest.json").read_text(encoding="utf-8")),
                    json.loads(args.observed.read_text(encoding="utf-8")))
    print("[OK] " + json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
