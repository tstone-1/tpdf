#!/usr/bin/env python3
"""Require the complete 8044 RTL differential, including known limitations.

python scripts/pdfium_verify.py --fixtures DIR --control control.json \
    --candidate candidate.json

The two observations come from separate pdfium_rtl_check.py processes. That
probe deliberately exits 1 on both engines because it reports the two known
limitations too. This verifier accepts only the exact, independently defined
failure sets, never a generic nonzero exit or a reduced fixture inventory.
"""
import argparse
from collections import Counter
import json
from pathlib import Path

REGRESSIONS = {
    "arabic.pdf", "arabic-forms.pdf", "arabic-shaped.pdf", "arabic-latin.pdf",
    "hebrew.pdf", "hebrew-latin.pdf", "arabic-numbers.pdf",
}
LIMITATIONS = {"latin-prefix.pdf", "latin-suffix.pdf"}
CONTROLS = {"latin.pdf", "latin-hebrew.pdf"}


def verify(manifest, control, candidate):
    cases = manifest["cases"]
    names = [case["file"] for case in cases]
    inventory = REGRESSIONS | LIMITATIONS | CONTROLS
    if len(names) != len(inventory) or set(names) != inventory:
        raise ValueError("Expected the complete, unique 11-case fixture inventory")
    if set(control) != inventory or set(candidate) != inventory:
        raise ValueError("Observation inventory differs from the 11 fixtures")
    wrong_before, wrong_after = set(), set()
    for case in cases:
        name = case["file"]
        before, after = control[name], candidate[name]
        if before["text"] != case["expected"]:
            wrong_before.add(name)
        if after["text"] != case["expected"]:
            wrong_after.add(name)
        for observation in (before, after):
            if not observation["chars"] or not observation["text"]:
                raise ValueError(f"{name}: empty observation")
            if "".join(chr(c[0]) for c in observation["chars"]) != observation["text"]:
                raise ValueError(f"{name}: text and character indices disagree")
        if before["pixels_sha256"] != after["pixels_sha256"]:
            raise ValueError(f"{name}: pixels changed")
        if Counter(map(json.dumps, before["chars"])) != Counter(map(json.dumps, after["chars"])):
            raise ValueError(f"{name}: character geometry changed")
        if name in LIMITATIONS and before != after:
            raise ValueError(f"{name}: known limitation changed")
    if wrong_before != REGRESSIONS | LIMITATIONS:
        raise ValueError(f"Control failed unexpected cases: {sorted(wrong_before)}")
    if wrong_after != LIMITATIONS:
        raise ValueError(f"Candidate failed unexpected cases: {sorted(wrong_after)}")
    return {"cases": len(inventory), "regressions_restored": len(REGRESSIONS),
            "known_limitations_unchanged": len(LIMITATIONS),
            "pixels_unchanged": True, "character_geometry_unchanged": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixtures", type=Path, required=True)
    parser.add_argument("--control", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    args = parser.parse_args()
    result = verify(json.loads((args.fixtures / "manifest.json").read_text()),
                    json.loads(args.control.read_text()), json.loads(args.candidate.read_text()))
    print("[OK] " + json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
