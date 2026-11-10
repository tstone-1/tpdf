"""Frontend mutation rows for suites that ran under mutation with no row aimed
at them: byte counts, the print progress line and the print preview, the
PowerPoint copy notice, the graphic export, the graphic click, the Rust
sentences in German, and the three suites that read source text (dimmed text,
"Sie" German, the landing page).

`check_mutation_test_files.py` refuses a suite in `TEST_FILES` that no row's
`expect` names, so each suite here keeps at least one row.

The last three read the tree rather than a module's behaviour, so their rows
edit the files they read: a component's opacity, a catalog's "Sie" half, and
the landing page's sentences and figures. Each is the defect that suite exists
to catch.
"""

from __future__ import annotations

from .base import Mutation

ROWS = [
    # --- bytes.ts ------------------------------------------------------------------
    Mutation("bytes: a count that is not a number printed as one", "src/lib/bytes.ts",
        "  if (!Number.isFinite(bytes) || bytes < 0) return text.unknown;",
        "  if (bytes < 0) return text.unknown;",
        "prints German with a decimal comma and a thousands point"),
    Mutation("bytes: a decimal printed at ten and above", "src/lib/bytes.ts",
        "  const rounded = Number(tenths) < 10 ? tenths : Math.round(value).toString();",
        "  const rounded = tenths;",
        "prints German with a decimal comma and a thousands point"),
    Mutation("bytes: German given a decimal point", "src/lib/bytes.ts",
        '  const shown = text.number === "de-DE" ? rounded.replace(".", ",") : rounded;',
        "  const shown = rounded;",
        "prints German with a decimal comma and a thousands point"),
    # --- printing.ts ---------------------------------------------------------------
    Mutation("printing: the last sheet's report reads one past the job", "src/lib/printing.ts",
        "  return TEXT().printing(Math.min(sent + 1, of), of);",
        "  return TEXT().printing(sent + 1, of);",
        "names the sheet being drawn, counted from 1"),
    Mutation("printing: a failure leaves the line saying Printing", "src/lib/printing.ts",
        '  if (state === "failed") return null;\n',
        "",
        "clears the line on a failure, which Windows reports in its own message"),
    Mutation("printing: a stop says sheets may print when none were sent", "src/lib/printing.ts",
        "    return sent === 0 ? TEXT().stoppedNothingSent : TEXT().stoppedSomeSent;",
        "    return TEXT().stoppedSomeSent;",
        "says a stopped print stopped, and whether the printer may have sheets already"),
    Mutation("printing: Stop offered once the job is over", "src/lib/printing.ts",
        '  return progress.state === "printing";',
        '  return progress.state !== "failed";',
        "offers Stop only while a print is being drawn"),
    # --- printpreview.ts -----------------------------------------------------------
    Mutation("print preview: a backwards range printed", "src/lib/printpreview.ts",
        "    if (last < first) return TEXT().backwards(piece);",
        "    if (false) return TEXT().backwards(piece);",
        "refuses a page the job does not have, a backwards range and words"),
    Mutation("print preview: two portrait pages to a sheet left upright", "src/lib/printpreview.ts",
        "  return perSheet === 2 || perSheet === 6 ? !wide : wide;",
        "  return wide;",
        "turns the sheet for two or six portrait pages"),
    Mutation("print preview: last page first ignored", "src/lib/printpreview.ts",
        "  if (choice.reverse) pages = [...pages].reverse();\n",
        "",
        "prints last page first, after the subset"),
    Mutation("print preview: a page drawn again for a few pixels", "src/lib/printpreview.ts",
        "      if (have === undefined || have < wanted * 0.9) return",
        "      if (have === undefined || have < wanted) return",
        "draws a page again when it is shown much larger, and not for a few pixels"),
    Mutation("print preview: settings kept before a printer is known", "src/lib/printpreview.ts",
        "  if (settings.printer === null) return false;\n  const kept: Kept = {",
        "  const kept: Kept = {",
        "keeps nothing until a printer is known"),
    # --- pagecopynotice.ts ---------------------------------------------------------
    Mutation("page copy notice: text said left out when none was", "src/lib/pagecopynotice.ts",
        '  return text.pageCopied(page) + (leftOut > 0 ? text.leftOut(leftOut) : "");',
        "  return text.pageCopied(page) + text.leftOut(leftOut);",
        "says only that the page was copied when nothing is left out"),
    Mutation("page copy notice: one piece of text counted as many", "src/lib/pagecopynotice.ts",
        '      count === 1\n        ? " One piece',
        '      count < 1\n        ? " One piece',
        "tells one from many in English"),
    # --- graphicexport.ts ----------------------------------------------------------
    Mutation("graphic export: SVG offered when it cannot be made", "src/lib/graphicexport.ts",
        '  if (found.no_svg === null) choices.push({ format: "svg"',
        '  choices.push({ format: "svg"',
        "leaves SVG out when the backend says it cannot be made"),
    Mutation("graphic export: the stored JPEG not offered", "src/lib/graphicexport.ts",
        '  if (found.original === "jpg") choices.push({ format: "original"',
        '  if (false) choices.push({ format: "original"',
        "offers the stored JPEG beside the PNG"),
    Mutation("graphic export: an upper-case ending not read", "src/lib/graphicexport.ts",
        "  const ending = /\\.([^./\\\\]+)$/.exec(path)?.[1]?.toLowerCase();",
        "  const ending = /\\.([^./\\\\]+)$/.exec(path)?.[1];",
        "reads the format from the chosen name's ending, in any case"),
    Mutation("graphic export: a saved JPEG described as a PNG", "src/lib/graphicexport.ts",
        '  if (found.extension === "jpg") return TEXT().savedJpeg(saved);\n',
        "",
        "says what was made, by what was made"),
    # --- viewer.ts: isGraphicClick ---------------------------------------------------
    Mutation("graphic click: taken with a tool chosen", "src/lib/viewer.ts",
        '  return (\n    tool === "none" &&\n    unit === "char" &&',
        '  return (\n    unit === "char" &&',
        "leaves a drag, text, a tool, a double click and a selection alone"),
    Mutation("graphic click: taken with a selection standing", "src/lib/viewer.ts",
        "    !press.text &&\n    !selected &&\n    Math.hypot(",
        "    !press.text &&\n    Math.hypot(",
        "leaves a drag, text, a tool, a double click and a selection alone"),
    # --- said.ts -------------------------------------------------------------------
    Mutation("said: the Sie entry never used", "src/lib/said.ts",
        '  return render((locale === "de-DE-formal" ? SIE[key] : undefined) ?? GERMAN[key]!, args);',
        "  return render(GERMAN[key]!, args);",
        "says Sie where the du sentence has a Sie entry"),
    Mutation("said: a sentence inside a sentence left in English", "src/lib/said.ts",
        "  for (const [name, value] of Object.entries(found.args)) args[name] = spoken(value, locale, depth + 1);",
        "  for (const [name, value] of Object.entries(found.args)) args[name] = value;",
        "renders a sentence from Rust in German, with the sentence inside it"),
    Mutation("said: a doubled brace kept doubled", "src/lib/said.ts",
        "      segments[segments.length - 1] += match[0][0]!;",
        "      segments[segments.length - 1] += match[0];",
        "recognises braces written double"),
    # --- contrast.test.ts reads every source's opacity -------------------------------
    Mutation("contrast: a shortcut's text dimmed below readable", "src/lib/contextmenu.ts",
        '      keys.style.cssText = "opacity:0.55;font-variant-numeric:tabular-nums;";',
        '      keys.style.cssText = "opacity:0.5;font-variant-numeric:tabular-nums;";',
        "is nowhere fainter than text can be read at"),
    # --- formal.test.ts reads every catalog's "Sie" half ------------------------------
    Mutation("formal: a du sentence left with no Sie entry", "src/lib/forward.ts",
        '  "de-DE-formal": {\n    notFileAndLine: (typed: string) =>\n'
        '      `tpdf sollte --forward-search „${typed}“ ausführen, das ist aber keine Datei mit Zeile: Geben Sie es als datei.tex:ZEILE an`,\n  },',
        '  "de-DE-formal": {\n  },',
        "every German entry that says du has a Sie entry"),
    Mutation("formal: a Sie entry that says du", "src/lib/diskwatch.ts",
        "wurde auf dem Datenträger geändert, während Sie ungespeicherte Änderungen haben. ",
        "wurde auf dem Datenträger geändert, während du ungespeicherte Änderungen hast. ",
        "a Sie entry says no du, differs from its German, and names a German key"),
    # --- sitepage.test.ts reads the landing pages against the README ------------------
    Mutation("landing page: a start-up figure the README does not have", "site/index.html",
        "From launch to the first page painted is 276 ms,",
        "From launch to the first page painted is 270 ms,",
        "says what the README says: is 276 ms"),
    Mutation("landing page: the German page names the English invoice", "site/de/index.html",
        "<em>RE-2026-0417</em>",
        "<em>INV-2026-0417</em>",
        "names the German invoice's number, which differs from the English one"),
]
