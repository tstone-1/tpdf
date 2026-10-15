"""What a row of the Rust mutation table is, and the anchors rows share.

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
    #: The one platform this mutation can run on, or None for all of them.
    #:
    #: `menu.rs` and `keylayout.rs` are macOS-only, so on Windows `cargo test`
    #: never compiles them and the tests they name do not exist. The guard below
    #: reports that correctly -- "it cannot go red, so this mutation would report
    #: SURVIVED" -- and then refuses the whole run over it, which is the right
    #: answer for an unknown name and the wrong one for an absent platform. It
    #: blocked every mutation in this table on Windows from the day `menu::` was
    #: added until 2026-08-19, and nothing said so, because the whole table had
    #: not been run there since.
    #:
    #: **Declared, never inferred.** A mutation with no `only_on` still refuses
    #: the run when its test cannot be found -- which is the case the guard was
    #: written for and the one that must stay loud. Skipping on a name the
    #: harness merely failed to see is how a mutation quietly stops being able
    #: to fail.
    only_on: str | None = None


# `Plan::is_appendable`'s body, which two mutations aim at and which has now moved
# twice. Named once so that the next clause added to the predicate re-aims both.
MUT_SUBTYPE_GUARD = (
    "    if dictionary\n"
    '        .get(b"Subtype")\n'
    "        .and_then(Object::as_name)\n"
    "        .is_err()\n"
    "    {"
)


# **The whole predicate, comment and all**, because the deletion clause added on
# 2026-08-30 carries six lines of comment inside it -- so an anchor naming only
# the three code lines no longer matches, and the two mutations that share this
# both went red in the gate on the commit that added it.
MUT_APPENDABLE = (
    "        (!self.marks.is_empty() || !self.notes.is_empty())\n"
    "            && self.redactions.is_empty()\n"
    "            // **A removal, and an append only adds.** Without this clause a plan\n"
    "            // that highlights one line and deletes one comment classifies as an\n"
    "            // append, writes the highlight, drops the deletion and reports\n"
    "            // success -- and a fixture holding only the deletion cannot see it,\n"
    "            // because with no marks and no note edits the first clause makes the\n"
    "            // answer false either way. `docs/TRAPS.md` has that.\n"
    "            && self.discards.is_empty()\n"
    "            && self.pages_are_the_file()"
)
