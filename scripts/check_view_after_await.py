#!/usr/bin/env python3
"""After an `await`, `App.svelte` may be another document. Two rules hold that.

WHY THIS EXISTS
===============

`App.svelte` keeps the document the reader is working in as plain variables:
`viewer`, `edits`, `covered` and the rest. With two documents side by side the
variables hold the focused one and `Stage` (`src/lib/livedocument.ts`) holds the
other, and a press inside the other side swaps them. So a function that waits
for anything, a reply to an edit, a dialog, and then reads one of those
variables is reading whichever document is focused *by then*. Highlight on the
left, a press on the right before the reply, and the left selection's words
were filed under a mark of the right-hand document.

No unit test imports `App.svelte`, and the window checks drive one document at a
time unless a phase says otherwise, so the mistake is silent at every layer. The
rule was written down in `asDocument`'s doc comment and nothing enforced it.

RULE 1: NO SLOT NAME AFTER AN AWAIT, OUTSIDE A GUARD
====================================================

The names are read from the `liveSlots` table, so a slot added there is checked
from the commit that adds it. In every function that contains an `await`, a slot
name written after that `await`, or anywhere in a loop that contains it (the
second pass of the loop runs after the first one's wait), is a finding, and so
is one in a function handed to `.then(...)`, `.catch(...)` or `.finally(...)`,
unless it is inside one of the guards:

  * `asDocument(view, ...)`: runs as the named document, whichever is focused.
  * `documentTasks.run(...)` and `opens.run(...)`: `focusSide` refuses to move
    while either runs, so the variables stay the document they were.
  * `ALLOWED` below: a function named there, with the reason it is safe.

An `await` inside `asDocument(...)` is a finding of its own: the variables go
back to the focused document the moment the work waits. So is `applyEdit(run)`
after a wait without its second argument: it takes the document in the
variables when the view is left out, which is a read like any other.

Code that must *stop* when the reader has left, a question the palette is
about to ask, calls `stillIn(view, model)` in `App.svelte`, which this does not
look into.

RULE 2: EVERY TOP-LEVEL `let` IS A SLOT OR IS NAMED AS THE WINDOW'S
===================================================================

`liveSlots` is typed against `LiveDocument`, so a field without a slot does not
compile. The other direction has no compiler: a new `let` that is about one
document and never reaches `LiveDocument` is shared by both sides, and nothing
says so. Each top-level `let` is therefore either a slot or an entry of
`PER_WINDOW` below, with what makes it the window's. Adding a variable means
deciding which, in writing.

WHAT IT CANNOT SEE
==================

This reads text; it does not follow calls. A helper that reads the variables
and is called after an `await` is not seen (`say`, `refreshMenu` and
`applyEdit` are such helpers, and each is written to be called as the focused
document). A callback defined before the first `await` and run after it is not
seen. A property or an object key
with a slot's name is skipped by its spelling (`tab.edits`, `title: "..."`), and
a local, a parameter or a `catch` binding with a slot's name hides the slot in
the function that declares it.

Usage:
    scripts/check_view_after_await.py               # the planted cases, then App.svelte
    scripts/check_view_after_await.py --self-test   # the planted cases alone, each one named
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "src" / "App.svelte"

#: The calls whose argument runs as one known document.
GUARDS = ("asDocument(", "documentTasks.run(", "opens.run(")

#: Functions that read a slot after an `await` and are right to, with the reason.
#:
#: Every entry is a claim that focus cannot have moved, or that the function
#: checks for itself. An entry naming a function that no longer needs it, or no
#: longer exists, is a failure: a list that quietly stops applying becomes a
#: blanket permission.
ALLOWED: dict[str, str] = {
    "openDocument": "it is the mount: it parks, clears and fills the variables itself, under `opening`",
    "showPanes": "it carries out the plan that moves documents in and out of the variables, under `opening`",
    "boot": "every name is in a listener or a check's accessor, which runs later as the focused document",
}

#: Top-level `let`s that are about the window and not about one document.
#:
#: Grouped by what makes them the window's. A variable belongs here when both
#: sides share it on purpose; one that names a page, a mark, a search or
#: anything else of one file belongs in `LiveDocument` and `liveSlots`.
PER_WINDOW: dict[str, str] = {
    # The two page areas and what is drawn round them.
    "areaHosts": "the window's two page areas",
    "paneLayout": "what the markup draws of the two sides",
    "bodyHeld": "whether any document is mounted beside the focused one",
    "paneShare": "the divider between the two sides",
    "panesHost": "the element holding both page areas",
    "sidebarHost": "the one element every document's sidebar is built in",
    "startHost": "the blank page's element",
    "startRows": "the blank page's rows",
    "findField": "the one find field; its text is the slot `query`",
    "zoomMenu": "the header's zoom menu",
    "sidebarShown": "the reader's choice for the window, kept in the session",
    # Scrolling the two sides together is about the pair.
    "syncScroll": "whether the two sides scroll together",
    "following": "set while one side is moved to keep up with the other",
    "aloneUntil": "the Alt wheel's grace, by the clock",
    # The tab row.
    "tabCarried": "the tab being dragged",
    "tabDragEnded": "the click that follows a drag's release",
    "tabRows": "every tab, for the markup",
    "tabLabelPx": "the tab labels' size",
    "activeTab": "which tab is the reader's, for the markup",
    # What blocks the whole window while it runs.
    "opening": "an open or a tab change is running; focus does not move under it",
    "documentBusy": "a document task is running; focus does not move under it",
    "copyTaskBusy": "a copy task is running; focus does not move under it",
    "blockingTask": "the line shown over the window while a task blocks it",
    "recognising": "whether the blocking task is a recognition",
    "recognitionRun": "which recognition Stop is for; a task blocks the window",
    "findingHidden": "whether the blocking task is the hidden-text check",
    "committingPopup": "raised and lowered inside one synchronous commit",
    "textEditorGeneration": "counted up across documents on purpose, see `unmountDocument`",
    # What the reader picked, which goes with them from one document to the next.
    "armedField": "the kind of field the armed tool places",
    "armedGroup": "the group the next radio buttons join",
    "armedOptions": "the choices of the next dropdown",
    "fieldBorder": "a preference, kept between sessions",
    "markColor": "the colour the reader picked",
    "markNib": "the nib the reader picked, set on every mounted viewer",
    "redactionFill": "a preference, kept between sessions",
    "invertPages": "a preference, kept in the session and set on every mounted viewer",
    "diskChangeMode": "a preference, kept between sessions",
    "restoreTabs": "a preference, kept in the session",
    # Built once at launch, on `document.body`.
    "palette": "the one palette",
    "signatureDialog": "a dialog on `document.body`",
    "propertiesDialog": "a dialog on `document.body`; the answer it shows is the slot `properties`",
    "passwordDialog": "a dialog on `document.body`",
    "newPasswordDialog": "a dialog on `document.body`",
    "fieldPropertiesDialog": "a dialog on `document.body`",
    "compressDialog": "a dialog on `document.body`",
    "webLinkDialog": "a dialog on `document.body`",
    "contextMenu": "the one right-click menu",
    # The application.
    "session": "the session read at launch",
    "updateState": "the updater",
    "appVersion": "the running version",
    "toolState": "the toolbar, which describes the focused document and is rebuilt when focus moves",
    "toolStateKey": "the toolbar state last drawn",
    "menuInstalled": "whether there is a native menu bar",
    "menuPushed": "the enablement last pushed to the menu bar",
    "queuedPictures": "what a checks build answers the next open panel with",
}


def blanked(source: str) -> str:
    """`source` with comments and the text of strings replaced by spaces.

    The length and every newline are kept, so an offset here is an offset in
    the file. The code inside a template literal's `${...}` is kept, since a
    slot read there is a read. A regular expression is blanked when what
    precedes it cannot end an expression.
    """
    out = list(source)
    n = len(source)
    i = 0
    # Each entry is the brace depth a `${` was opened at inside a template.
    templates: list[int] = []
    depth = 0

    def blank(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    def template_text(start: int) -> int:
        """Blanks template text from `start`; stops after a `${` or the closing tick."""
        k = start
        while k < n:
            if source[k] == "\\":
                k += 2
                continue
            if source[k] == "`":
                blank(start, k)
                return k + 1
            if source.startswith("${", k):
                blank(start, k)
                templates.append(depth)
                return k + 2
            k += 1
        blank(start, n)
        return n

    last = ""  # the last character of code that is not white space
    last_word = ""
    while i < n:
        c = source[i]
        if source.startswith("//", i):
            j = source.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
            continue
        if source.startswith("/*", i):
            j = source.find("*/", i + 2)
            j = n if j < 0 else j + 2
            blank(i, j)
            i = j
            continue
        if c in "\"'":
            j = i + 1
            while j < n and source[j] != c and source[j] != "\n":
                j += 2 if source[j] == "\\" else 1
            blank(i + 1, j)
            i = j + 1
            last, last_word = c, ""
            continue
        if c == "`":
            i = template_text(i + 1)
            last, last_word = "`", ""
            continue
        if c == "/" and (last in "(,=:[!&|?{};+-*%<>~^" or last == "" or last_word in ("return", "typeof", "case")):
            j = i + 1
            in_class = False
            while j < n and source[j] != "\n":
                if source[j] == "\\":
                    j += 2
                    continue
                if source[j] == "[":
                    in_class = True
                elif source[j] == "]":
                    in_class = False
                elif source[j] == "/" and not in_class:
                    break
                j += 1
            blank(i + 1, j)
            i = j + 1
            last, last_word = "/", ""
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            if templates and templates[-1] == depth:
                templates.pop()
                i = template_text(i + 1)
                last, last_word = "`", ""
                continue
            depth -= 1
        if not c.isspace():
            if c.isalnum() or c in "_$":
                last_word = last_word + c if (last.isalnum() or last in "_$") and source[i - 1] == last else c
            else:
                last_word = ""
            last = c
        i += 1
    return "".join(out)


PAIRS = {"(": ")", "[": "]", "{": "}"}


def closing(code: str, at: int) -> int:
    """The offset of the bracket that closes the one at `at`."""
    stack = []
    for k in range(at, len(code)):
        c = code[k]
        if c in PAIRS:
            stack.append(PAIRS[c])
        elif c in ")]}":
            if not stack or stack.pop() != c:
                raise ValueError(f"unbalanced bracket at offset {k}")
            if not stack:
                return k
    raise ValueError(f"bracket at offset {at} is never closed")


def script_of(source: str) -> tuple[str, int]:
    """The component's script and the offset it starts at."""
    start = source.index('<script lang="ts">') + len('<script lang="ts">')
    return source[start:source.index("\n</script>", start)], start


def slot_names(code: str) -> list[str]:
    """The keys of the `liveSlots` table."""
    start = code.index("const liveSlots")
    brace = code.index("{", code.index("=", start))
    body = code[brace:closing(code, brace)]
    return re.findall(r"^\s+([A-Za-z_]\w*): \{ get:", body, re.M)


class Function:
    def __init__(self, name: str, params: tuple[int, int], body: tuple[int, int]):
        self.name = name
        self.params = params
        self.body = body


def functions_in(code: str) -> list[Function]:
    """Every function in `code`, declared or an arrow, with where its body is."""
    found: list[Function] = []
    for match in re.finditer(r"\bfunction\b\s*([A-Za-z_$][\w$]*)?\s*(?:<[^(]*>)?\s*\(", code):
        open_paren = match.end() - 1
        close_paren = closing(code, open_paren)
        # Past the return type to the body's brace.
        k = close_paren + 1
        angle = 0
        while k < len(code) and not (code[k] == "{" and angle == 0):
            if code[k] == "<":
                angle += 1
            elif code[k] == ">" and code[k - 1] != "=":
                angle -= 1
            k += 1
        if k >= len(code):
            continue
        found.append(Function(match.group(1) or "(anonymous)", (open_paren, close_paren), (k, closing(code, k))))
    for match in re.finditer(r"=>", code):
        # The parameters: a bracketed list, perhaps with a return type, or one name.
        k = match.start() - 1
        while k >= 0 and code[k].isspace():
            k -= 1
        if k >= 0 and code[k] == ")":
            depth = 0
            j = k
            while j >= 0:
                if code[j] in ")]}":
                    depth += 1
                elif code[j] in "([{":
                    depth -= 1
                    if depth == 0:
                        break
                j -= 1
            params = (j, k)
        else:
            j = k
            while j >= 0 and (code[j].isalnum() or code[j] in "_$"):
                j -= 1
            params = (j + 1, k + 1)
        b = match.end()
        while b < len(code) and code[b].isspace():
            b += 1
        if b < len(code) and code[b] == "{":
            body = (b, closing(code, b))
        else:
            # An expression: up to the first separator that is not inside it.
            depth = 0
            e = b
            while e < len(code):
                c = code[e]
                if c in "([{":
                    depth += 1
                elif c in ")]}":
                    if depth == 0:
                        break
                    depth -= 1
                elif c in ",;" and depth == 0:
                    break
                e += 1
            body = (b, e)
        found.append(Function("(arrow)", params, body))
    return found


def guard_spans(code: str) -> list[tuple[int, int, str]]:
    spans = []
    for guard in GUARDS:
        for match in re.finditer(r"(?<![\w.$])" + re.escape(guard), code):
            open_paren = match.end() - 1
            spans.append((open_paren, closing(code, open_paren), guard))
    return spans


def loop_bodies(code: str) -> list[tuple[int, int]]:
    """The span of every `for`, `while` and `do` loop, header included."""
    loops = []
    for match in re.finditer(r"(?<![\w.$])(for|while)\s*\(", code):
        head_end = closing(code, match.end() - 1)
        k = head_end + 1
        while k < len(code) and code[k].isspace():
            k += 1
        if k < len(code) and code[k] == "{":
            loops.append((match.start(), closing(code, k)))
        else:
            end = code.find(";", k)
            loops.append((match.start(), len(code) if end < 0 else end))
    for match in re.finditer(r"(?<![\w.$])do\s*\{", code):
        loops.append((match.start(), closing(code, match.end() - 1)))
    return loops


def declared_in(code: str, function: Function, name: str) -> bool:
    """Whether `function` declares a local, a parameter or a `catch` binding called `name`."""
    params = code[function.params[0]:function.params[1] + 1]
    # A parameter is a name followed by its type, a default, a comma or the end.
    if re.search(r"(?<![\w.$])" + re.escape(name) + r"\s*(?:\?)?\s*(?=[:,)=]|$)", params):
        return True
    body = code[function.body[0]:function.body[1] + 1]
    word = r"(?<![\w.$])" + re.escape(name) + r"(?![\w$])"
    if re.search(r"\b(?:const|let|var)\s+" + word, body):
        return True
    if re.search(r"\bcatch\s*\(\s*" + word, body):
        return True
    # `const a = 1, name = 2` and a destructuring pattern.
    for match in re.finditer(r"\b(?:const|let|var)\s+([^;]*?)(?:;|\n\s*\n)", body, re.S):
        declaration = match.group(1)
        if re.search(r"(?:^|,)\s*" + word + r"\s*(?:=|,|$)", declaration, re.M):
            return True
        pattern = re.match(r"\s*[\[{]", declaration)
        if pattern:
            opened = match.start(1) + declaration.index(declaration.strip()[0])
            inside = body[opened:closing(body, opened)]
            if re.search(word, inside):
                return True
    return False


def top_level_names(code: str) -> dict[int, str]:
    """The offset and name of each top-level declaration that can hold a function."""
    names = {}
    depth = 0
    line_start = 0
    for line in code.splitlines(keepends=True):
        if depth == 0:
            match = re.match(r"\s*(?:export\s+)?(?:async\s+)?(?:function|const|let|var)\s+([A-Za-z_$][\w$]*)", line)
            if match:
                names[line_start] = match.group(1)
        for c in line:
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
        line_start += len(line)
    return names


def statement_starts(code: str) -> list[int]:
    """The offset of every line that begins a top-level statement."""
    starts = []
    depth = 0
    line_start = 0
    for line in code.splitlines(keepends=True):
        if depth == 0 and line.strip() and not line.strip().startswith((")", "]", "}")):
            starts.append(line_start)
        for c in line:
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
        line_start += len(line)
    return starts


def owner_of(code: str, names: dict[int, str], functions: list["Function"], at: int) -> str:
    """What a finding at `at` is reported under, and allowed by.

    The top-level declaration it is in. A statement that declares nothing, an
    `$effect(...)` for one, is known by the outermost named function inside it
    that holds `at`: a name has to be written down for it to be allowed.
    """
    start = max((begin for begin in statement_starts(code) if begin <= at), default=0)
    if start in names:
        return names[start]
    named = [f for f in functions if f.name not in ("(arrow)", "(anonymous)") and f.body[0] <= at <= f.body[1]]
    if named:
        return max(named, key=lambda f: f.body[1] - f.body[0]).name
    return f"the statement at line {1 + code.count(chr(10), 0, start)} of the script (give its function a name)"


def line_of(code: str, at: int, first_line: int) -> int:
    return first_line + code.count("\n", 0, at)


def after_await(code: str, slots: list[str], first_line: int = 1) -> tuple[dict[str, list[str]], list[str]]:
    """Rule 1. Findings by top-level declaration, and the other failures."""
    functions = functions_in(code)
    guards = guard_spans(code)
    loops = loop_bodies(code)
    names = top_level_names(code)

    def guarded(at: int) -> bool:
        return any(a < at < b for a, b, _ in guards)

    def scope_of(at: int) -> Function | None:
        inside = [f for f in functions if f.body[0] <= at <= f.body[1]]
        return min(inside, key=lambda f: f.body[1] - f.body[0]) if inside else None

    other: list[str] = []
    # Where, in each function, code starts to run after a wait.
    starts: dict[int, tuple[Function, int]] = {}
    for match in re.finditer(r"(?<![\w.$])await(?![\w$])", code):
        at = match.start()
        work = [g for g in guards if g[2] == "asDocument(" and g[0] < at < g[1]]
        if work and scope_of(at) and scope_of(at).body[0] > work[0][0]:
            other.append(
                f"line {line_of(code, at, first_line)}: an `await` inside asDocument(...); "
                "the variables are the focused document's again after it"
            )
        if guarded(at):
            continue
        scope = scope_of(at)
        if scope is None:
            continue
        # What is awaited is called before the wait: `await viewer.text()`
        # reads `viewer` first. Its arguments are not excused, since a function
        # among them runs whenever the callee chooses.
        begin = re.compile(r"await\s+[\w$.?]*").match(code, at).end()
        for a, b in loops:
            if scope.body[0] <= a and a <= at <= b:
                begin = min(begin, a)
        key = scope.body[0]
        if key not in starts or begin < starts[key][1]:
            starts[key] = (scope, begin)

    # What a promise is chained with runs after its wait, all of it.
    for match in re.finditer(r"\.(?:then|catch|finally)\s*\(", code):
        open_paren = match.end() - 1
        if guarded(open_paren):
            continue
        close_paren = closing(code, open_paren)
        for f in functions:
            if open_paren < f.body[0] and f.body[1] <= close_paren and f.params[0] > open_paren:
                key = f.body[0]
                if key not in starts or f.body[0] < starts[key][1]:
                    starts[key] = (f, f.body[0])

    findings: dict[str, list[str]] = {}
    # An edit asked for after a wait names the view it is for: left out,
    # `applyEdit` takes the document in the variables, like any other read.
    for scope, begin in starts.values():
        for match in re.finditer(r"(?<![\w.$])applyEdit\(", code[:scope.body[1]]):
            at = match.start()
            if at < begin or guarded(at):
                continue
            inside = code[match.end():closing(code, match.end() - 1)]
            depth = 0
            commas = 0
            for c in inside:
                if c in "([{":
                    depth += 1
                elif c in ")]}":
                    depth -= 1
                elif c == "," and depth == 0:
                    commas += 1
            # A trailing comma after the one argument is not a second one.
            if commas == 0 or (commas == 1 and inside.rstrip().endswith(",")):
                findings.setdefault(owner_of(code, names, functions, at), []).append(
                    f"line {line_of(code, at, first_line)}: `applyEdit(...)` without the view it is for"
                )
    word = re.compile(r"(?<![\w$])(" + "|".join(re.escape(s) for s in slots) + r")(?![\w$])")
    seen: set[tuple[int, str]] = set()
    for scope, begin in starts.values():
        for match in word.finditer(code, begin, scope.body[1]):
            at, name = match.start(), match.group(1)
            if (at, name) in seen or guarded(at):
                continue
            before = code[:at].rstrip()
            if before.endswith("."):
                continue  # a property of something else
            if code[match.end():match.end() + 1] == ":" and code[match.end():match.end() + 2] != "::":
                continue  # an object key or a label
            enclosing = [f for f in functions if f.body[0] <= at <= f.body[1] or f.params[0] <= at <= f.params[1]]
            if any(declared_in(code, f, name) for f in enclosing):
                continue
            seen.add((at, name))
            findings.setdefault(owner_of(code, names, functions, at), []).append(
                f"line {line_of(code, at, first_line)}: `{name}`"
            )
    return findings, other


def top_level_lets(code: str) -> list[str]:
    """The names of the script's top-level `let`s, each declarator counted."""
    found = []
    depth = 0
    for line in code.splitlines():
        if depth == 0:
            match = re.match(r"\s*let\s+(.*)", line)
            if match:
                rest = match.group(1)
                # Declarators are split at commas that are not inside brackets.
                level = 0
                piece = ""
                pieces = []
                for c in rest:
                    if c in "([{<":
                        level += 1
                    elif c in ")]}>":
                        level -= 1
                    if c == "," and level == 0:
                        pieces.append(piece)
                        piece = ""
                    else:
                        piece += c
                pieces.append(piece)
                for index, declarator in enumerate(pieces):
                    name = re.match(r"\s*([A-Za-z_$][\w$]*)", declarator)
                    # After the first, only a piece that reads as `name =` or
                    # `name:` is a declarator; the rest is an initialiser that
                    # went on over the line.
                    if name and (index == 0 or re.match(r"\s*[A-Za-z_$][\w$]*\s*(?:[:=;]|$)", declarator)):
                        found.append(name.group(1))
        for c in line:
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
    return found


def check(source: str, allowed: dict[str, str], per_window: dict[str, str], first_line: int = 1) -> list[str]:
    """Every failure in one script, as the lines to print."""
    code = blanked(source)
    failures: list[str] = []
    try:
        slots = slot_names(code)
    except ValueError as why:
        return [f"could not read the liveSlots table ({why})"]
    # An empty scan passes exactly like a clean one, so each is refused.
    if not slots:
        return ["no slots found in the liveSlots table -- the scan is broken"]
    try:
        findings, other = after_await(code, slots, first_line)
    except ValueError as why:
        return [f"could not read the functions ({why})"]
    if not re.search(r"(?<![\w.$])await(?![\w$])", code):
        failures.append("no `await` found in the script -- the scan is broken")
    failures += other
    for owner in sorted(findings):
        if owner in allowed:
            continue
        where = ", ".join(findings[owner])
        failures.append(
            f"{owner} touches a document's variable after an `await`: {where}. "
            "Run it through asDocument(view, ...) with the view taken at entry, "
            "or name the function in ALLOWED with the reason"
        )
    for owner in sorted(set(allowed) - set(findings)):
        failures.append(f"{owner} is in ALLOWED and touches no document variable after an `await` any more")

    lets = top_level_lets(code)
    if not lets:
        failures.append("no top-level `let` found -- the scan is broken")
    for name in lets:
        if name not in slots and name not in per_window:
            failures.append(
                f"`let {name}` is neither a slot of liveSlots nor named in PER_WINDOW: "
                "say whether it is one document's or the window's"
            )
    for name in sorted(set(per_window) & set(slots)):
        failures.append(f"`{name}` is a slot and is also named in PER_WINDOW")
    for name in sorted(set(per_window) - set(lets)):
        failures.append(f"`{name}` is named in PER_WINDOW and is not a top-level `let` any more")
    for name in sorted(set(slots) - set(lets)):
        failures.append(f"`{name}` is a slot and no top-level `let` declares it -- the scan is broken")
    return failures


SELF_TEST_HEAD = """
  let viewer: Viewer | null = null;
  let edits: Edits | null = null;
  let covered = new Map<number, string>();
  let openView = -1;
  let markColor = $state(DEFAULT);
  const liveSlots: Slots<LiveDocument> = {
    openView: { get: () => openView, set: (value) => { openView = value; } },
    viewer: { get: () => viewer, set: (value) => { viewer = value; } },
    edits: { get: () => edits, set: (value) => { edits = value; } },
    covered: { get: () => covered, set: (value) => { covered = value; } },
  };
"""

#: Each case: what it proves, the script, the allow-list, and a word every
#: failure it must produce contains (none for a script that must pass).
SELF_TESTS: list[tuple[str, str, dict[str, str], list[str]]] = [
    (
        "a slot read after an await is refused",
        """
  async function mark(text: string): Promise<void> {
    const before = edits?.state.marks.length ?? 0;
    await applyEdit((e) => e.mark());
    if ((edits?.state.marks.length ?? 0) > before) covered.set(1, text);
  }
""",
        {},
        ["mark touches"],
    ),
    (
        "the same work through asDocument passes",
        """
  async function mark(text: string): Promise<void> {
    const view = openView;
    const before = edits?.state.marks.length ?? 0;
    const after = await applyEdit((e) => e.mark());
    asDocument(view, () => { if (after && after.marks.length > before) covered.set(1, text); });
  }
""",
        {},
        [],
    ),
    (
        "a read at the top of a loop that waits is after the wait",
        """
  async function each(): Promise<void> {
    for (const page of pages) {
      const model = edits;
      await send(page, model);
    }
  }
""",
        {},
        ["each touches"],
    ),
    (
        "a task and a queued open hold the document still",
        """
  async function save(): Promise<void> {
    return documentTasks.run(async () => {
      await settle();
      await edits?.save();
    });
  }
  function show(): Promise<void> {
    return opens.run(async () => { await settle(); viewer?.focus(); });
  }
""",
        {},
        [],
    ),
    (
        "a comment, a string, a property and a key are not reads",
        """
  async function quiet(tab: Tab): Promise<void> {
    await settle();
    // edits is not read here, and neither is viewer.
    say("the viewer and its edits");
    say(`covered ${tab.edits.state.dirty}`);
    open({ viewer: null, edits: tab.edits });
  }
""",
        {},
        [],
    ),
    (
        "a read inside a template's expression is a read",
        """
  async function loud(): Promise<void> {
    await settle();
    say(`now ${edits?.doc}`);
  }
""",
        {},
        ["loud touches"],
    ),
    (
        "a local or a parameter with a slot's name is not the slot",
        """
  async function local(viewer: Viewer): Promise<void> {
    await settle();
    const edits = viewer.edits();
    try { await edits.save(); } catch (covered) { say(String(covered)); }
  }
""",
        {},
        [],
    ),
    (
        "an allowed function passes, and an entry nothing needs is refused",
        """
  async function mark(): Promise<void> {
    await settle();
    viewer?.focus();
  }
""",
        {"mark": "focus cannot move here", "gone": "no longer there"},
        ["gone is in ALLOWED"],
    ),
    (
        "an await inside asDocument is refused",
        """
  function late(): void {
    asDocument(openView, async () => { await settle(); viewer?.focus(); });
  }
""",
        {},
        ["inside asDocument"],
    ),
    (
        "a let that is neither a slot nor the window's is refused",
        """
  let pickedMark = -1;
  function nothing(): void {}
  async function waits(): Promise<void> { await settle(); }
""",
        {},
        ["`let pickedMark`"],
    ),
    (
        "what is awaited is read before the wait, and its arguments are not excused",
        """
  async function reads(): Promise<void> {
    const text = await viewer?.text();
    say(text);
  }
  async function hands(): Promise<void> {
    await ask(() => viewer?.text());
  }
""",
        {},
        ["hands touches"],
    ),
    (
        "a function handed to .then runs after the wait",
        """
  const actions = {
    order: () => {
      void send().then(() => { viewer?.focus(); });
    },
    fine: () => {
      const view = openView;
      void send().then(() => asDocument(view, () => viewer?.focus()));
    },
  };
""",
        {},
        ["actions touches"],
    ),
    (
        "an edit asked for after a wait names its view",
        """
  async function crops(): Promise<void> {
    const view = openView;
    const box = await measure();
    await applyEdit((e) => e.crop(box));
  }
  async function cropsThere(): Promise<void> {
    const view = openView;
    const box = await measure();
    await applyEdit((e) => e.crop(box), view);
    await applyEdit(
      (e) => e.crop(box),
      view,
    );
  }
  async function first(): Promise<void> {
    await applyEdit((e) => e.undo());
  }
""",
        {},
        ["crops touches"],
    ),
    (
        "a statement that declares nothing is known by the function named inside it",
        """
  $effect(() => {
    void (async function boot() {
      await settle();
      window.addEventListener("focus", () => viewer?.focus());
    })();
  });
""",
        {"boot": "listeners run later, as the focused document"},
        [],
    ),
    (
        "and has to be given a name before it can be allowed",
        """
  $effect(() => {
    void (async () => {
      await settle();
      viewer?.focus();
    })();
  });
""",
        {},
        ["give its function a name"],
    ),
]


def self_test(quiet: bool = False) -> int:
    """Runs every planted case. `quiet` prints only what went wrong."""
    say = (lambda line: None) if quiet else print
    wrong = 0
    for what, body, allowed, expected in SELF_TESTS:
        failures = check(SELF_TEST_HEAD + body + "\n  async function waits2(): Promise<void> { await settle(); }\n",
                         allowed, {"markColor": "the reader's colour"})
        missing = [word for word in expected if not any(word in failure for failure in failures)]
        extra = [failure for failure in failures if not any(word in failure for word in expected)]
        if missing or extra:
            wrong += 1
            print(f"[FAIL] self-test: {what}")
            for word in missing:
                print(f"         expected a failure containing {word!r} and got none")
            for failure in extra:
                print(f"         unexpected: {failure}")
        else:
            say(f"[OK] self-test: {what}")
    # The two lists below it are held to the real file by `main`; here the
    # controls: a PER_WINDOW entry for a slot, and one for a variable that is gone.
    failures = check(SELF_TEST_HEAD + "\n  async function waits(): Promise<void> { await settle(); }\n",
                     {}, {"markColor": "x", "viewer": "x", "longGone": "x"})
    for word in ("`viewer` is a slot and is also named", "`longGone` is named in PER_WINDOW"):
        if not any(word in failure for failure in failures):
            wrong += 1
            print(f"[FAIL] self-test: expected a failure containing {word!r}")
    if wrong:
        print(f"[FAIL] {wrong} self-test case(s) did not behave as stated")
        return 2
    say("[OK] the self-test saw every planted defect refused and every repaired one pass")
    return 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    # The planted cases first, every time. They are text in this file and cost
    # nothing, and a scan that has stopped matching passes exactly like a
    # component with nothing wrong in it.
    if self_test(quiet=True):
        return 2
    try:
        source = APP.read_text(encoding="utf-8")
        script, start = script_of(source)
    except (OSError, ValueError) as why:
        print(f"[FAIL] could not read {APP.relative_to(ROOT)} ({why})")
        return 2
    first_line = 1 + source.count("\n", 0, start)
    failures = check(script, ALLOWED, PER_WINDOW, first_line)
    for failure in failures:
        print(f"[FAIL] {failure}")
    if failures:
        print(f"[FAIL] {len(failures)} place(s) where App.svelte may be reading another document")
        return 2
    code = blanked(script)
    print(
        f"[OK] no document variable is read after an await outside a guard "
        f"({len(slot_names(code))} slots, {len(ALLOWED)} functions allowed by name), and every "
        f"top-level let is a slot or the window's ({len(PER_WINDOW)} named)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
