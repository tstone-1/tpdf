"""The frontend mutation table's rows, joined in the order they run.

The parts are the table as it stood in `mutate_frontend.py` until 2026-10-15,
cut into pieces in order, so `ROWS` is that table row for row. A new row goes
at the end of the part for its area, or of the last part. A part that passes
the harness ceiling (`check_mutation_test_files.py`, `HARNESS_CEILING`) is split
in two and the second half added to `PARTS` right after the first.
"""

from . import (
    f01_viewer,
    f02_keyboard,
    f03_colour,
    f04_inserting,
    f05_latest,
    f06_compare_addtext,
)

#: Every part, in the order its rows run.
PARTS = (
    f01_viewer,
    f02_keyboard,
    f03_colour,
    f04_inserting,
    f05_latest,
    f06_compare_addtext,
)

ROWS = [row for part in PARTS for row in part.ROWS]
