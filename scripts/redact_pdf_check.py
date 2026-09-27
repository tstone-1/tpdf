"""Read a file `tpdf redact` wrote with an independent PDF parser, against what should be gone.

Usage:
    TPDF_REDACT_PROBE=/tmp/tpdf-redact cargo test --manifest-path src-tauri/Cargo.toml --test cli
    uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/contacts-redacted.pdf \
        /tmp/tpdf-redact/contacts-expected.json
    # The negative control: the unredacted input must be rejected.
    uv run --with pypdf scripts/redact_pdf_check.py /tmp/tpdf-redact/contacts.pdf \
        /tmp/tpdf-redact/contacts-expected.json --expect-fail

pypdf extracts each page's text with its own content-stream interpreter and its
own font decoding, so a string it does not find is two readers agreeing that the
text layer lost it, not one reader agreeing with itself. It is also asked for
every decoded content stream's raw bytes, which is where a black rectangle drawn
over surviving text would give itself away: the words would still be operands of
a `Tj`. The expected file names the strings that must be `gone` and the controls
that must be `kept` --- the second half is what makes a run that removed
everything fail.
"""

import json
import sys

from pypdf import PdfReader


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    expect_fail = "--expect-fail" in sys.argv[1:]
    if len(args) != 2:
        print(__doc__)
        return 2
    path, expected_path = args
    with open(expected_path, encoding="utf-8") as f:
        expected = json.load(f)

    reader = PdfReader(path)
    text = "\n".join(page.extract_text() or "" for page in reader.pages)
    raw = b""
    for page in reader.pages:
        contents = page.get_contents()
        if contents is not None:
            raw += contents.get_data()

    problems = []
    for word in expected["gone"]:
        if word in text:
            problems.append(f"pypdf's text still has {word!r}")
        if word.encode("latin-1", "ignore") in raw:
            problems.append(f"a content stream still draws {word!r}")
    for word in expected["kept"]:
        if word not in text:
            problems.append(f"pypdf's text lost the control {word!r}")

    for problem in problems:
        print(f"[FAIL] {problem}")
    if expect_fail:
        if problems:
            print(f"[PASS] rejected, as it must be: {len(problems)} problem(s)")
            return 0
        print("[FAIL] the unredacted input was accepted, so this check cannot fail")
        return 1
    if problems:
        return 1
    print(
        f"[PASS] {len(expected['gone'])} string(s) gone and {len(expected['kept'])} "
        f"control(s) kept across {len(reader.pages)} page(s)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
