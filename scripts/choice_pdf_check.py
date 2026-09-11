"""Read the synthetic mixed-form output with an independent PDF parser.

Usage: uv run --with pypdf scripts/choice_pdf_check.py scratch/choices-saved.pdf
Generate using TPDF_CHOICE_PROBE and the choices_and_radio_round_trip test.
The unedited TPDF_CHOICE_FIXTURE is the negative control: this check must reject it.
"""

import sys

from pypdf import PdfReader


def verify(path: str) -> None:
    reader = PdfReader(path, strict=True)
    assert len(reader.pages) == 2, "page count"
    fields = {}
    for page in reader.pages:
        for ref in page.get("/Annots", []):
            widget = ref.get_object()
            field = widget.get("/Parent", widget).get_object()
            fields[field.get("/T")] = field
    assert fields, "no fields found"
    combo, items, radio = (fields[name] for name in ("delivery_choice", "items", "delivery"))
    assert combo["/FT"] == "/Ch" and items["/FT"] == "/Ch", "choice type lost"
    assert combo["/V"] == "SAME", "dropdown export"
    assert list(combo["/I"]) == [1], "second duplicate export must retain its index"
    assert list(items["/V"]) == ["SAME", "OTHER"], "list exports"
    assert list(items["/I"]) == [0, 2], "list indices"
    assert radio["/V"] == "/Second", "radio group value"
    buttons = choices = 0
    for page_index, page in enumerate(reader.pages):
        for ref in page.get("/Annots", []):
            widget = ref.get_object()
            field = widget.get("/Parent", widget).get_object()
            name = field.get("/T")
            if name == "delivery":
                buttons += 1
                state = "/Second" if page_index == 1 else "/Off"
                assert widget["/AS"] == state, "radio sibling state"
                assert widget["/AP"]["/N"][state].get_object().get_data(), "missing radio artwork"
            elif name in ("delivery_choice", "items"):
                choices += 1
                content = widget["/AP"]["/N"].get_object().get_data()
                if name == "delivery_choice":
                    assert b"5365636f6e64206c6162656c" in content, "dropdown appearance must draw Second label"
                else:
                    assert b"4669727374206c6162656c" in content, "first list label missing"
                    assert b"5468697264206c6162656c" in content, "third list label missing"
                    assert content.count(b"0.8 0.87 1 rg") == 2, "both selected list rows must be shaded"
    assert buttons == 2 and choices == 2, "widget coverage"


if __name__ == "__main__":
    try:
        verify(sys.argv[1])
    except (AssertionError, KeyError) as error:
        print(f"[FAIL] mixed form: {error}")
        raise SystemExit(1) from error
    print("[PASS] independent parser: radio states, choice exports, indices and appearance streams")
