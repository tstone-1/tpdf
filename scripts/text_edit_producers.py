#!/usr/bin/env python3
"""Survey locally generated synthetic PDFs without changing their bytes.

uv run --with pypdf scripts/text_edit_producers.py --probe <text-edit-probe> <pdf>...
The worker verdict is for page zero. Independent pypdf inventory covers every
page and follows only structure /K edges, never cyclic parent references.
Exit zero means the survey completed, not that every sample is editable.
This development tool parses synthetic exports only, outside the application.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess

from pypdf import PdfReader
from pypdf.generic import ContentStream


def inventory(path):
    reader = PdfReader(path)
    if not 1 <= len(reader.pages) <= 128:
        raise ValueError("synthetic sample page count exceeds bounds")
    root = reader.trailer["/Root"]
    roles, attributes, visited = Counter(), set(), set()

    def structure(obj, depth=0):
        if depth > 16 or len(visited) > 256:
            raise ValueError("synthetic structure exceeds bounds")
        obj = obj.get_object()
        if isinstance(obj, list):
            for child in obj:
                structure(child, depth + 1)
        elif isinstance(obj, dict):
            if id(obj) in visited:
                raise ValueError("duplicate or cyclic structure child")
            visited.add(id(obj))
            attributes.update(map(str, obj))
            if "/S" in obj:
                roles[str(obj["/S"])] += 1
            if "/K" in obj:
                structure(obj["/K"], depth + 1)

    if "/StructTreeRoot" in root:
        structure(root["/StructTreeRoot"])
    pages = []
    for page in reader.pages:
        fonts = []
        for name, ref in page.get("/Resources", {}).get("/Font", {}).items():
            font = ref.get_object()
            descendants = [child.get_object() for child in font.get("/DescendantFonts", [])]
            fonts.append({
                "resource": str(name), "subtype": str(font.get("/Subtype", "")),
                "encoding": str(font.get("/Encoding", "")),
                "to_unicode": "/ToUnicode" in font,
                "descendants": [{"subtype": str(child.get("/Subtype", "")),
                                 "cid_to_gid": str(child.get("/CIDToGIDMap", ""))}
                                for child in descendants],
            })
        content = page.get_contents()
        operators = Counter(op.decode("ascii") for _, op in ContentStream(content, reader).operations) if content else Counter()
        # No document text or metadata is emitted in the report.
        text = " ".join(page.extract_text().split())
        if text not in ("", "SYNTHETIC FIRST SYNTHETIC SECOND"):
            raise ValueError("expected the two synthetic lines or a blank control")
        pages.append({"fonts": fonts, "operators": dict(sorted(operators.items())),
                      "synthetic_text": bool(text)})
    return {"pages": pages, "tagged": "/StructTreeRoot" in root,
            "roles": dict(sorted(roles.items())), "structure_keys": sorted(attributes)}


def survey(path, probe):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    measured = inventory(path)
    run = subprocess.run([str(probe), "--inspect", str(path)], capture_output=True,
                         text=True, encoding="utf-8", timeout=60, check=True)
    verdict = json.loads(run.stdout)
    if verdict.get("page") != 0 or verdict.get("status") not in ("editable", "refused", "no_runs"):
        raise ValueError("invalid worker inspection verdict")
    if verdict["status"] == "refused" and not verdict.get("reason"):
        raise ValueError("worker refusal has no reason")
    if verdict["status"] in ("editable", "no_runs"):
        count = verdict.get("runs")
        if type(count) is not int or (count > 0) != (verdict["status"] == "editable") or count < 0:
            raise ValueError("worker status and run count disagree")
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        raise ValueError("inspection changed the source PDF")
    return {"sample": path.name, "sha256": digest, "worker": verdict, **measured}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("pdf", type=Path, nargs="+")
    args = parser.parse_args()
    reports = [survey(path.resolve(), args.probe.resolve()) for path in args.pdf]
    print(json.dumps(reports, indent=2))


if __name__ == "__main__":
    main()
