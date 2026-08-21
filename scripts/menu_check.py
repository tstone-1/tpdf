#!/usr/bin/env python3
"""Reads the real macOS menu bar of a running tpdf and checks what is in it.

WHY THIS EXISTS
===============

On 2026-08-21 a reader sent a screenshot of the application menu with **two
items named "About tpdf"**, one above the other. Both were real: `menu.rs` added
`PredefinedMenuItem::about`, and `menubar.ts` leads the application section with
our own `app.about`, which answers the same question and is the only answer on
Windows. One opened the platform panel, the other wrote the version into the
header.

**Nothing in the repository could have seen it, in either language.** The
platform's items are built in `menu.rs` and never named there -- the label comes
from the OS -- while our titles are declared in `appcommands.ts` and arrive over
IPC as data. Neither side holds both lists, so no test compares a label against a
label; `menubar.test.ts` and `menu.rs`'s tests both check ids, accelerators and
the wire shape, which is everything except the string the reader reads.

The only place both lists exist at once is the menu bar itself, which is why this
check reads that rather than either source. It is the same argument
`examples/backend_probe.rs` makes about the dynamic linker's image table: an
observable outside our own code beats a claim made inside it.

WHAT IT CHECKS
==============

1. The read returned menus at all. An accessibility read that comes back empty
   looks exactly like a clean bar, so an empty scan is a failure here.
2. No two items in one menu carry the same name. That is the invariant the
   defect broke, and it is one neither language can state on its own.
3. Every menu `menubar.ts` declares is in the bar, in that order, and no menu is
   in the bar that it does not declare -- so a section that stops being built is
   a failure rather than a quieter bar.

WHAT IT DOES NOT CHECK
======================

What an item does. This is a labelling and structure check; behaviour is
`appcommands.test.ts`'s and the command probes'. It also says nothing about
Windows, which has no menu bar of this shape at all.

**It launches the app with `open`, deliberately**, rather than as a subprocess
with pipes. `docs/TRAPS.md` records what launching from a shell hid for a month:
a harness that captures output supplies a stdout and a stderr that a
double-clicked application does not have. The menu is built before any document
is opened, so this needs no fixture and no document.

Usage:
    scripts/menu_check.py                 # the release bundle
    scripts/menu_check.py <path-to.app>
    scripts/menu_check.py --self-test     # the duplicate rule, against the
                                          # measured before-and-after menus
"""

from __future__ import annotations

import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUNDLE = ROOT / "src-tauri/target/release/bundle/macos/tpdf.app"
MENUBAR_TS = ROOT / "src/lib/menubar.ts"

#: What System Events returns for a separator. Not an item, and not a duplicate
#: of the separator above it either.
SEPARATOR = "missing value"

#: The application menu as it was measured at 15:0x on 2026-08-21, before the
#: predefined About was removed, and as it was measured minutes afterwards.
#:
#: These are the control. The rule below has to call the first one a defect and
#: the second one clean, or it is decoration -- and keeping the real strings
#: means the control is the failure itself rather than a synthetic stand-in that
#: a rewrite of the rule could accidentally be shaped around.
BEFORE_FIX = [
    "About tpdf", SEPARATOR, "About tpdf", "Check for updates",
    "Install update and restart", SEPARATOR, "Services", SEPARATOR,
    "Hide tpdf", "Hide Others", "Show All", SEPARATOR,
    "Quit tpdf", "Quit and Keep Windows",
]
AFTER_FIX = [
    "About tpdf", "Check for updates", "Install update and restart",
    SEPARATOR, "Services", SEPARATOR,
    "Hide tpdf", "Hide Others", "Show All", SEPARATOR,
    "Quit tpdf", "Quit and Keep Windows",
]

#: One record per line, menu name first, then its items, tab separated. The
#: Apple menu is `menu bar item 1` and is the platform's alone, so the scan
#: starts at 2.
SCRIPT = """
tell application "System Events" to tell process "tpdf"
    set out to ""
    set n to count of menu bar items of menu bar 1
    repeat with i from 2 to n
        set mbi to menu bar item i of menu bar 1
        set out to out & (name of mbi)
        try
            -- The list is materialised before it is walked. `repeat with x in
            -- (name of every menu item of ...)` iterates a REFERENCE into the
            -- property, and reading an element of it raises -1700 -- inside the
            -- `try` that aborts the whole menu and leaves it looking empty,
            -- which is exactly how the first two runs of this script reported
            -- every menu in the bar as having no items. An instrument failure
            -- wearing the shape of a finding, and the reason the empty-menu
            -- check above is a failure rather than a skip.
            set nms to name of every menu item of menu 1 of mbi
            repeat with k from 1 to count of nms
                set v to item k of nms
                -- A separator's name is `missing value`, and coercing that to
                -- text raises -1700 as well.
                if v is missing value then
                    set out to out & tab & "missing value"
                else
                    set out to out & tab & v
                end if
            end repeat
        end try
        set out to out & linefeed
    end repeat
    return out
end tell
"""


def duplicates(items: list[str]) -> list[str]:
    """Names appearing more than once in one menu, separators excepted."""
    seen: dict[str, int] = {}
    for name in items:
        if name == SEPARATOR:
            continue
        seen[name] = seen.get(name, 0) + 1
    return sorted(name for name, n in seen.items() if n > 1)


def declared_menus() -> list[str]:
    """The menu titles `menubar.ts` declares, in bar order.

    Read out of the `MENU_LAYOUT` literal rather than from a list kept here: a
    second copy of the bar's own contents is the drift this check exists to
    catch, one level up.
    """
    source = MENUBAR_TS.read_text(encoding="utf-8")
    start = source.index("export const MENU_LAYOUT")
    body = source[start:]
    return re.findall(r'^\s{4}title: "([^"]+)",', body, re.M)


def read_menus() -> list[list[str]]:
    """The live bar, or an exit with the reason it could not be read."""
    done = subprocess.run(
        ["osascript", "-e", SCRIPT], capture_output=True, text=True, timeout=60
    )
    if done.returncode != 0:
        why = done.stderr.strip()
        # Told apart because the remedies differ and both read as "no menu":
        # -1719/-25211 is this terminal lacking accessibility permission, -1728
        # is no such process. Neither may be reported as a clean bar.
        if "-1719" in why or "-25211" in why or "assistive" in why:
            print(f"[FAIL] the menu could not be read: {why}")
            print("[FAIL] grant this terminal Accessibility in System Settings > Privacy")
        elif "-1728" in why:
            print(f"[FAIL] tpdf is not running, so there is no menu to read: {why}")
        else:
            print(f"[FAIL] the menu could not be read: {why}")
        sys.exit(2)
    return [line.split("\t") for line in done.stdout.splitlines() if line.strip()]


def check(menus: list[list[str]]) -> int:
    problems = 0

    # An empty read is the reassuring branch and the likeliest broken one.
    if not menus:
        print("[FAIL] the accessibility read returned no menus at all")
        return 2

    for menu in menus:
        name, items = menu[0], menu[1:]
        if not items:
            print(f"[FAIL] the {name} menu is empty")
            problems += 1
            continue
        repeated = duplicates(items)
        for label in repeated:
            print(f'[FAIL] the {name} menu carries "{label}" more than once')
            problems += 1
        if not repeated:
            print(f"[OK]   {name}: {len(items)} items, no repeated name")

    seen = [menu[0] for menu in menus]
    declared = declared_menus()
    if not declared:
        print("[FAIL] no menu titles could be read out of menubar.ts")
        return 2
    # Window is not `menubar.ts`'s and never appears there: `menu.rs` appends it
    # last and predefined throughout, because every item in it belongs to the
    # window manager rather than to the application. It is named here rather
    # than tolerated by a subset comparison -- a menu that stopped being built
    # would then be as clean as one that is there.
    expected = [*declared, "Window"]
    if seen != expected:
        print(f"[FAIL] the bar is {seen}")
        print(f"[FAIL] menubar.ts declares {declared}, and menu.rs appends Window")
        problems += 1
    else:
        print(
            f"[OK]   the bar is the {len(declared)} menus menubar.ts declares, "
            "in order, and the predefined Window"
        )

    if problems:
        print(f"[FAIL] {problems} problem(s) in the menu bar")
        return 2
    print(f"[OK] the menu bar is clean: {len(menus)} menus, no repeated name in any of them")
    return 0


def self_test() -> int:
    """The rule against the menu that broke, and against the one that fixed it."""
    bad = duplicates(BEFORE_FIX)
    good = duplicates(AFTER_FIX)
    ok = True
    if bad == ["About tpdf"]:
        print('[OK]   the rule calls the 2026-08-21 menu a defect: "About tpdf" twice')
    else:
        print(f"[FAIL] the rule did not catch the measured duplicate: {bad}")
        ok = False
    if good == []:
        print("[OK]   the rule calls the menu after the fix clean")
    else:
        print(f"[FAIL] the rule flags the fixed menu: {good}")
        ok = False
    # Separators repeat in both, and are not items. A rule that counted them
    # would fire on every menu in the bar and never on this defect.
    if SEPARATOR not in bad and SEPARATOR not in good:
        print("[OK]   separators are not counted, though both menus have three")
    else:
        print("[FAIL] separators are being counted as repeated items")
        ok = False
    return 0 if ok else 2


def main() -> int:
    args = [a for a in sys.argv[1:]]
    if "--self-test" in args:
        return self_test()

    app = Path(args[0]) if args else BUNDLE
    if not app.exists():
        print(f"[FAIL] no bundle at {app} --- npm run tauri build -- --bundles app")
        return 2

    subprocess.run(["open", str(app.resolve())], check=True)
    # The menu is set by the frontend once it has loaded, so the wait is for the
    # webview rather than for the process. Polled rather than slept through, so
    # a slow machine costs nothing and a broken launch fails on the last try
    # with the reason from `read_menus` rather than a bare timeout.
    for _ in range(20):
        time.sleep(1)
        done = subprocess.run(
            ["osascript", "-e", 'tell application "System Events" to tell process "tpdf" '
             'to count of menu bar items of menu bar 1'],
            capture_output=True, text=True,
        )
        if done.returncode == 0 and done.stdout.strip().isdigit() and int(done.stdout) > 2:
            break

    try:
        return check(read_menus())
    finally:
        subprocess.run(["osascript", "-e", 'tell application "tpdf" to quit'],
                       capture_output=True, text=True)


if __name__ == "__main__":
    sys.exit(main())
