"""Read a form `tpdf fill` wrote with an independent PDF parser, against its answers.

Usage:
    TPDF_FILL_PROBE=/tmp/tpdf-fill cargo test --manifest-path src-tauri/Cargo.toml --test cli
    uv run --with pypdf scripts/fill_pdf_check.py /tmp/tpdf-fill/every-control-filled.pdf \
        /tmp/tpdf-fill/every-control-answers.json
    # The negative control: the unfilled input must be rejected.
    uv run --with pypdf scripts/fill_pdf_check.py /tmp/tpdf-fill/every-control.pdf \
        /tmp/tpdf-fill/every-control-answers.json --expect-fail

pypdf walks the field tree itself (`PdfReader.get_fields`) and builds each fully
qualified name from the `/T` chain, so a name here agreeing with `tpdf fields`
is two readers agreeing, not one reader agreeing with itself. Each answer is
compared with the field's `/V` as pypdf decodes it: a string for text and
choices, a name for a checkbox (`true` is any state but `/Off`) and a radio
group, an array for a list that takes several. Every widget of an answered
field must also carry an `/AP` normal appearance, because an answer without one
is an answer some readers do not show.
"""

import json
import sys

from pypdf import PdfReader


def expected(value, found):
    """Whether pypdf's `/V` says what the answer asked for."""
    if isinstance(value, bool):
        state = str(found) if found is not None else "/Off"
        return (state != "/Off") == value
    if isinstance(value, list):
        got = found if isinstance(found, list) else ([] if found is None else [found])
        return [str(v) for v in got] == value
    if found is None:
        return value is None
    text = str(found)
    return text == value or text == "/" + value


def verify(pdf: str, answers_path: str) -> list:
    reader = PdfReader(pdf, strict=True)
    fields = reader.get_fields() or {}
    with open(answers_path, encoding="utf-8") as handle:
        answers = json.load(handle)
    wrong = []
    for name, value in answers.items():
        field = fields.get(name)
        if field is None:
            wrong.append(f"{name}: pypdf finds no such field among {sorted(fields)}")
            continue
        found = field.get("/V")
        if not expected(value, found):
            wrong.append(f"{name}: asked {value!r}, pypdf reads {found!r}")
    # Every widget of an answered field has a normal appearance.
    for page in reader.pages:
        for ref in page.get("/Annots", []) or []:
            widget = ref.get_object()
            if widget.get("/Subtype") != "/Widget":
                continue
            names = []
            node = widget
            while node is not None:
                if "/T" in node:
                    names.append(str(node["/T"]))
                parent = node.get("/Parent")
                node = parent.get_object() if parent is not None else None
            name = ".".join(reversed(names))
            if name in answers and "/N" not in (widget.get("/AP") or {}):
                wrong.append(f"{name}: a widget has no normal appearance")
    return wrong


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    expect_fail = "--expect-fail" in sys.argv
    wrong = verify(args[0], args[1])
    if expect_fail:
        if wrong:
            print(f"[PASS] control: pypdf rejects the unfilled form ({len(wrong)} differences)")
            return
        print("[FAIL] control: pypdf found the answers in a form nobody filled")
        raise SystemExit(1)
    if wrong:
        for line in wrong:
            print(f"[FAIL] {line}")
        raise SystemExit(1)
    print(f"[PASS] pypdf reads every answer in {args[0]} as given")


if __name__ == "__main__":
    main()
