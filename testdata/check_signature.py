#!/usr/bin/env python3
"""Reports what pyhanko makes of every signature in a PDF, one line per field.

The Phase 0 incremental-save spike needs an answer to a question none of the
other readers can give: after tpdf appends an update section to a signed
document, is the signature still cryptographically intact, is the document
reported as modified, and was the modification *permitted*. Those are three
different things, and PLAN.md section 5 retracted an earlier claim precisely
because the first was being read as the third.

Called by `incremental-save --mode signed`. Prints one line per signature:

    Signature1 intact=yes valid=yes coverage=ENTIRE_FILE modified=no docmdp=ok

`intact` is the CMS digest over the signed byte range. `coverage` is how much of
the file that byte range spans. `modified` is whether anything follows it, and
`docmdp` is whether the difference analysis judged those changes permissible
under the certification level.

When the difference analysis refuses, its reason is printed on the following
line, indented. Without it a `docmdp=VIOLATED` says only that something was
disallowed, and the interesting question is always *what* --- the same verdict
covers "you rewrote a signed page dictionary" and "this validator does not
implement the permission level you are relying on".

Usage:
    uv run --with pyhanko testdata/check_signature.py FILE
"""

import logging
import sys


class Collect(logging.Handler):
    """Keeps pyhanko's own warnings so they can be reported alongside a verdict.

    pyhanko logs "StandardDiffPolicy was not designed to support DocMDP level 3"
    rather than raising, so a caller that only reads the return value cannot tell
    an unimplemented policy from a rejected edit.
    """

    def __init__(self) -> None:
        """Starts with no records."""
        super().__init__()
        self.messages: "list[str]" = []

    def emit(self, record: logging.LogRecord) -> None:
        """Stores one formatted record."""
        self.messages.append(record.getMessage())


def describe(path: str) -> "list[str]":
    """Validates every signature in `path`, returning one summary line each."""
    from pyhanko.pdf_utils.reader import PdfFileReader
    from pyhanko.sign.validation import validate_pdf_signature

    with open(path, "rb") as handle:
        reader = PdfFileReader(handle, strict=False)
        signatures = reader.embedded_signatures
        if not signatures:
            return ["(no signatures)"]

        lines = []
        for sig in signatures:
            # No trust roots are supplied on purpose. The question here is not
            # whether a throwaway self-signed certificate chains to anything --
            # it does not, by construction -- but whether the bytes the
            # signature covers still hash to what it says they do.
            status = validate_pdf_signature(sig)
            coverage = getattr(status.coverage, "name", str(status.coverage))
            modified = status.modification_level
            modified_name = getattr(modified, "name", str(modified))
            docmdp = getattr(status, "docmdp_ok", None)
            lines.append(
                "{field} intact={intact} valid={valid} coverage={coverage} "
                "modification={modification} docmdp={docmdp}".format(
                    field=sig.field_name,
                    intact="yes" if status.intact else "no",
                    valid="yes" if status.valid else "no",
                    coverage=coverage,
                    modification=modified_name,
                    docmdp={True: "ok", False: "VIOLATED", None: "n/a"}[docmdp],
                )
            )
            if docmdp is False and status.diff_result is not None:
                reason = str(status.diff_result).strip().splitlines()
                lines.extend("      " + part.strip() for part in reason if part.strip())
        return lines


def main(argv: "list[str]") -> int:
    """Prints the summary for the file named on the command line."""
    if len(argv) != 2:
        print("usage: check_signature.py FILE", file=sys.stderr)
        return 2
    collected = Collect()
    logging.getLogger("pyhanko.sign.diff_analysis").addHandler(collected)
    try:
        lines = describe(argv[1])
    except Exception as error:  # noqa: BLE001 - the message is the result
        print(f"unreadable: {type(error).__name__}: {error}")
        return 0
    for line in lines:
        print(line)
    for message in dict.fromkeys(collected.messages):
        print("      policy: " + " ".join(message.split()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
