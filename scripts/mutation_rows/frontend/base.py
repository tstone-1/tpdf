"""What a row of the frontend mutation table is, and the anchors rows share.

Moved here from the harness so the row files can import it without importing
the harness; the harness re-exports `Mutation`.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Mutation:
    """One edit, and the test whose job it is to notice."""

    name: str
    path: str
    before: str
    after: str
    expect: str
