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


# How long both checks wait before they start the old copy a second time. The
# application asks once per launch and never again (`src/lib/update.ts`), so a
# copy that was served the previous `latest.json` cannot find the offer however
# long it is watched; only a new start asks again. On 26.11.1, 26.11.4 and
# 26.11.9 the first run, about ten minutes after publishing, was answered
# `current` while the script's own read already named the new version, and the
# run started again by hand passed. A minute is that rerun's wait without the
# ten minutes of install and uninstall around it.
SECOND_START_S = 60


def served_old(log: Path) -> bool:
    """Whether the copy's last launch check was answered `current`.

    Only that answer earns a second start. A failed check, a stall or no line
    at all is a finding, and starting again would hide it.
    """
    launches = [line for line in update_lines(log) if "launch check" in line]
    return bool(launches) and launches[-1].rstrip().endswith(": current")


def explain(log: Path) -> str:
    """One detail for the failed offer check: the lines, or why there are none."""
    lines = update_lines(log)
    if lines:
        return " | ".join(lines)
    if not log.exists():
        return f"no log at {log}"
    return "the log has no [update] line (releases up to 26.11.4 write none)"
