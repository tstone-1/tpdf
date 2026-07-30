#!/usr/bin/env python3
"""Keeps a harness's output readable while it is still running.

Every check script here prints results as it produces them, on purpose: a run
that stops partway should name the last thing it completed rather than leaving a
reader to guess whether it hung or finished. `BUILD.md` states that property.

**Redirecting to a file destroys it, and that is the default way these are run.**
Python switches stdout from line-buffered to 4 KB block-buffered the moment it is
not a tty, so `viewer_check.py > out.txt` writes *nothing* until the process
exits. A twelve-minute run then looks identical to one that died at startup ---
which is precisely the ambiguity printing-as-you-go was meant to remove, arriving
from the producer's end instead of the consumer's. Measured, not theorised: an
`open_check.py` run redirected to a file sat at **zero bytes** for its whole
duration.

One line each, called explicitly rather than left as an import side effect, since
a module that silently reconfigures stdout is worse than the problem. And code
rather than a note in a document: the same hazard is written down as a caution in
`AGENTS.md`, was read, and was then walked into anyway --- a rule that has to be
remembered at the right moment loses to four characters that cannot be forgotten.
"""

import sys


def stream_results() -> None:
    """Makes stdout and stderr line-buffered even when they are not a terminal.

    Safe to call more than once, and safe on a stream that is already a tty ---
    it is then a no-op in effect. Wrapped in a try because `reconfigure` exists
    only on a `TextIOWrapper`: a caller whose stdout has been replaced (a test
    harness capturing it, for instance) should not crash over buffering.
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(line_buffering=True)  # type: ignore[union-attr]
        except (AttributeError, ValueError):
            # Nothing to do and nothing worth reporting: the caller's output is
            # someone else's object now, and buffering is their business.
            pass
