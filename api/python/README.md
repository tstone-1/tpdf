# tpdf-client

A Python client for the [tpdf](https://github.com/tstone-1/tpdf) command-line tool: read,
edit, fill, redact, sign and verify PDF documents from a script.

**This package alone does not read a PDF.** The client starts the `tpdf` command-line tool
and returns its JSON reports as dictionaries. The tool comes with the tpdf application for
macOS and Windows, which has to be installed separately; see the
[installation notes](https://github.com/tstone-1/tpdf#command-line-tool). There is no tool
for Linux, so the client finds nothing to start there. It has no dependencies of its own.

```
pip install tpdf-client
```

The import name is `tpdf_client`, not `tpdf`: another project on PyPI has that name.

```python
from tpdf_client import Tpdf

pdf = Tpdf()  # or Tpdf("/path/to/tpdf-cli", timeout=60)

print(pdf.info("report.pdf")["files"][0]["document"]["pages"])

report = pdf.redact("letter.pdf", "letter-redacted.pdf", texts=["Jane Doe"])
assert report["written"] and report["verified"]

pdf.merge(["cover.pdf", "report.pdf"], "combined.pdf")

# Highlights every match in a copy. Returns None, and writes nothing, when nothing matched.
pdf.mark_matches("report.pdf", "report-marked.pdf", texts=["North Pier"])

for file in pdf.search("a.pdf", "b.pdf", texts=["North Pier"])["files"]:
    for match in file["matches"]:
        print(file["path"], match["page"], match["hit"])
```

`pdf.run(command, *arguments)` reaches every command and returns a `Result`; its `typed`
property is the report, which the methods above return under the shape `tpdf_client.reports` names.

Every method is described in the
[tpdf README](https://github.com/tstone-1/tpdf#command-line-tool). Requires Python 3.10 or
newer. MIT licence.
