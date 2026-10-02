# tpdf-client

A Python client for the [tpdf](https://github.com/tstone-1/tpdf) command-line tool: read,
edit, fill, redact, sign and verify PDF documents from a script.

The client starts the `tpdf` command-line tool and returns its JSON reports as dictionaries.
It has no dependencies of its own. The tool itself comes with the tpdf application for
macOS and Windows, which has to be installed separately; see the
[installation notes](https://github.com/tstone-1/tpdf#command-line-tool).

```python
from tpdf import Tpdf

pdf = Tpdf()  # or Tpdf("/path/to/tpdf-cli", timeout=60)

print(pdf.info("report.pdf")["files"][0]["document"]["pages"])

report = pdf.redact("letter.pdf", "letter-redacted.pdf", texts=["Jane Doe"])
assert report["written"] and report["verified"]

pdf.merge(["cover.pdf", "report.pdf"], "combined.pdf")
```

Every method is described in the
[tpdf README](https://github.com/tstone-1/tpdf#command-line-tool). Requires Python 3.10 or
newer. MIT licence.
