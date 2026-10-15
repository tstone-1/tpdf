"""The mutation harnesses' rows, one package per table.

`rust/` holds `mutate_rust.py`'s and `frontend/` holds `mutate_frontend.py`'s.
Each harness imports its package's `ROWS` as its `MUTATIONS`; every gate that
reads a table imports the harness, so none of them reads these files directly.
"""
