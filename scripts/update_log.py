"""What the application under an update check wrote about its own checks.

Shared by `update_check.py` and `update_check_mac.py`, which both point
`TPDF_LOG_FILE` into their scratch folder. When the offer is missing, they print
these lines, so a failure carries its cause: `current` means the copy was served
the previous `latest.json`, `failed:` names the error, and a duration near 30000
ms is a request that stalled into `CHECK_TIMEOUT_MS` (`src/lib/update.ts`).

Releases up to 26.11.4 write no such line, so a check that installs one of them
as the old copy gets the second sentence of `explain` instead.
"""

from __future__ import annotations

from pathlib import Path


def update_lines(log: Path) -> list[str]:
    """The `[update]` lines of the log, oldest first; none when there is no log."""
    try:
        text = log.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    return [line for line in text.splitlines() if "[update] " in line]


def explain(log: Path) -> str:
    """One detail for the failed offer check: the lines, or why there are none."""
    lines = update_lines(log)
    if lines:
        return " | ".join(lines)
    if not log.exists():
        return f"no log at {log}"
    return "the log has no [update] line (releases up to 26.11.4 write none)"
