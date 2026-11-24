"""Verify the remote runner's reusable checkout before replacing its tracked files."""
import os
from pathlib import Path
import subprocess
import sys


def git(folder: Path, *args: str) -> bytes:
    return subprocess.check_output(["git", "-C", folder, *args])


def verify(clone: Path, tree: Path) -> bool:
    def reported(folder: Path, arg: str) -> Path:
        return Path(os.fsdecode(git(folder, "rev-parse", "--path-format=absolute", arg).rstrip(b"\r\n"))).resolve()

    clone = clone.resolve(strict=True)
    expected = clone.with_name(clone.name + "-check")
    if tree.resolve(strict=True) != expected or reported(tree, "--show-toplevel") != expected:
        return False
    common = reported(clone, "--git-common-dir")
    admin = reported(tree, "--absolute-git-dir")
    if reported(tree, "--git-common-dir") != common or admin.parent != common / "worktrees":
        return False
    backlink = os.fsdecode((admin / "gitdir").read_bytes().rstrip(b"\r\n"))
    if (admin / backlink).resolve() != expected / ".git":
        return False
    for record in git(clone, "worktree", "list", "--porcelain", "-z").split(b"\0\0"):
        fields = record.split(b"\0")
        if fields[0].startswith(b"worktree ") and Path(os.fsdecode(fields[0][9:])).resolve() == expected:
            return b"detached" in fields and not any(field.startswith(b"prunable") for field in fields)
    return False


if __name__ == "__main__":
    try:
        valid = verify(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"cannot verify the check worktree: {error}")
    if not valid:
        sys.exit("the check directory is not this clone's registered detached worktree")
