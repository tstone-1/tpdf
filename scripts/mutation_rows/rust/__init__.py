"""The Rust mutation table's rows, joined in the order they run.

The parts are the table as it stood in `mutate_rust.py` until 2026-10-15, cut
into pieces in order, so `ROWS` is that table row for row. A new row goes at
the end of the part for its area, or of the last part. A part that passes the
harness ceiling (`check_mutation_test_files.py`, `HARNESS_CEILING`) is split in
two and the second half added to `PARTS` right after the first.
"""

from . import (
    r01_tagging,
    r02_redact,
    r03_cropping,
    r04_certificates,
    r05_importer,
    r06_signing,
    r07_long_term,
    r08_reviews,
)

#: Every part, in the order its rows run.
PARTS = (
    r01_tagging,
    r02_redact,
    r03_cropping,
    r04_certificates,
    r05_importer,
    r06_signing,
    r07_long_term,
    r08_reviews,
)

ROWS = [row for part in PARTS for row in part.ROWS]
