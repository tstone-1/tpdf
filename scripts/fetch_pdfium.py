#!/usr/bin/env python3
"""Installs the pinned PDFium build into `vendor/pdfium`.

`vendor/pdfium/` is gitignored -- a 7.7 MB dylib does not belong in the object
store -- so a fresh clone has no PDFium and every spike binary fails to bind at
runtime. This script is what closes that gap, and it exists in preference to a
README instruction because the pin has to be enforced rather than described.

Two properties are load-bearing, and both are why the download is not just a
`curl`:

**The digest is checked before anything is extracted.** The archive is fetched
to a temporary file, hashed, and compared against the table below; a mismatch
aborts without touching `vendor/`. `docs/THREAT-MODEL.md` treats every PDF as
hostile input, which is not a serious position if the parser itself arrives
unverified over the network.

**The asset must not be a V8 build.** `bblanchon/pdfium-binaries` publishes
`pdfium-<platform>` and `pdfium-v8-<platform>` for every release, differing by
one word in the URL. AGENTS.md records that the vendored build has zero `v8::`
symbols and zero `CXFA_` symbols, and `docs/THREAT-MODEL.md` promotes that from
a policy ("document JavaScript is disabled") to a property of the binary
("there is no engine to disable"). That claim survives exactly as long as nobody
fetches the other asset, so the name is asserted here rather than trusted.

Usage:
    scripts/fetch_pdfium.py                 # install if absent or wrong
    scripts/fetch_pdfium.py --check         # verify only, exit 1 if wrong
    scripts/fetch_pdfium.py --force         # reinstall even if correct
    scripts/fetch_pdfium.py --platform win-x64 --dest vendor/pdfium-win

Installing writes `VERSION.txt` (the upstream tag) and `SHA256.txt` beside the
extracted tree, so an install can be checked later without the network. Note the
archive itself carries neither -- it ships a `VERSION` file of
MAJOR/MINOR/BUILD/PATCH lines -- which is why the two are written here.

`SHA256.txt` has **two** lines, and the second is what makes `--check` a
statement about the library rather than about the stamp:

    <sha256>  pdfium-win-x64.tgz      the archive, compared against PINS
    <sha256>  bin/pdfium.dll          the extracted library, re-hashed on check

Only the first existed until 2026-08-02, and the only other fact `--check` had
about the tree was that *something* matching `*pdfium*` sat in `lib/` or `bin/`.
On Windows `lib/pdfium.dll.lib` is an import library and satisfies that glob on
its own, so deleting or replacing `bin/pdfium.dll` -- the C++ blob that parses
every hostile document -- left the gate green. This file's own docstring already
promised otherwise; the promise is now kept rather than reworded. It is the trap
*"A directory that exists is not the library you need"* arriving inside the
script whose docstring names that very function as the one getting it wrong.

What the second line can and cannot say, stated so nobody reads more into it:
the library digest is not pinned in `PINS` -- only the archive is -- so it
catches a library that **changed after install**, which is deletion, corruption
or a swap, and it inherits its provenance from the archive check that admitted
those bytes. A stamp on its own is still worth nothing.

Bumping the pin means changing TAG and the whole PINS table together, then
re-running the checks AGENTS.md attaches to a PDFium bump: `remove_probe` for
the object-destroy segfault, and `worker_bench --mode engine` for the V8 and XFA
symbol scan. A digest cannot tell you the new build still behaves; only those
can.
"""

import argparse
import hashlib
import os
import platform
import shutil
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

# The pinned upstream release. Every Phase 0 measurement in AGENTS.md and
# docs/PLAN.md was taken against this build; changing it invalidates them.
TAG = "chromium/7881"

# asset name -> sha256 of the archive as published under TAG.
#
# mac-arm64 is the build every spike ran on: its digest matches the SHA256.txt
# of the working install, and the dylib inside it is byte-identical to the one
# in vendor/pdfium/lib. The other three were downloaded from the same release
# and hashed here; they are pinned so a future fetch is reproducible, not
# because any of them has been run.
PINS = {
    "mac-arm64": "52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40",
    "mac-x64": "6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b",
    "win-x64": "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08",
    "win-arm64": "d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84",
}

RELEASE_URL = "https://github.com/bblanchon/pdfium-binaries/releases/download"


def host_platform() -> str:
    """Returns this machine's key into PINS, or exits if it is not a target."""
    machine = platform.machine().lower()
    arm = machine in ("arm64", "aarch64")

    if sys.platform == "darwin":
        return "mac-arm64" if arm else "mac-x64"
    if sys.platform in ("win32", "cygwin"):
        return "win-arm64" if arm else "win-x64"

    sys.exit(
        f"[FAIL] tpdf targets macOS and Windows; this is {sys.platform}/{machine}.\n"
        f"       Pass --platform to install a cross-platform archive anyway."
    )


def library_path(key: str) -> str:
    """Returns the archive-relative path of the loadable library.

    The two platforms do not agree, and the difference is easy to miss: macOS
    ships `lib/libpdfium.dylib`, while Windows ships the runtime DLL in `bin/`
    and puts only the import library `pdfium.dll.lib` in `lib/`. Anything that
    resolves a load path by joining `vendor/pdfium/lib` -- which is what
    `pdfium_library_dir()` in `src-tauri/src/lib.rs` does today -- finds nothing
    loadable on Windows.
    """
    return "bin/pdfium.dll" if key.startswith("win") else "lib/libpdfium.dylib"


def file_digest(path: Path) -> str:
    """Returns the sha256 of a file, streamed a megabyte at a time."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def read_stamp(dest: Path) -> "tuple[str, str | None, str | None] | None":
    """Reads SHA256.txt as (archive digest, library digest, library path).

    Returns None when there is no stamp or its first line is not a digest --
    i.e. when there is nothing here that claims to be an install at all.

    The last two are None for a stamp written before the library line existed.
    That case is honoured rather than rejected, and `check` says so out loud:
    treating an old stamp as "no install" would fail the gate on every machine
    holding a perfectly good tree fetched last month, which is a checker
    breaking working installs rather than finding broken ones.
    """
    stamp = dest / "SHA256.txt"
    if not stamp.is_file():
        return None

    lines = stamp.read_text(encoding="utf-8").splitlines()
    first = lines[0].split() if lines else []
    if not first or len(first[0]) != 64:
        return None

    second = lines[1].split() if len(lines) > 1 else []
    if len(second) == 2 and len(second[0]) == 64:
        return first[0], second[0], second[1]
    return first[0], None, None


def installed_complete(key: str, dest: Path) -> bool:
    """True when `dest` holds a pin-matching install with an intact library.

    Deliberately stricter than `check`: it also requires the library digest
    line, so re-running the installer over a stamp written before that line
    existed replaces the tree and records one, instead of leaving the weaker
    check in place for as long as the machine lives.
    """
    stamp = read_stamp(dest)
    if stamp is None:
        return False

    archive, recorded, name = stamp
    library = dest / library_path(key)
    if archive != PINS[key] or recorded is None or name != library_path(key):
        return False
    return library.is_file() and file_digest(library) == recorded


def download(url: str, target: Path) -> str:
    """Fetches `url` to `target`, returning its sha256. Streams; nothing buffers."""
    digest = hashlib.sha256()
    try:
        with urllib.request.urlopen(url, timeout=120) as response:  # noqa: S310
            with target.open("wb") as out:
                while chunk := response.read(1 << 20):
                    digest.update(chunk)
                    out.write(chunk)
    except urllib.error.HTTPError as exc:
        sys.exit(f"[FAIL] {exc.code} fetching {url}")
    except (urllib.error.URLError, TimeoutError) as exc:
        sys.exit(f"[FAIL] could not fetch {url}: {exc}")
    return digest.hexdigest()


def safe_extract(archive: Path, into: Path) -> None:
    """Extracts a tarball, refusing any member that escapes `into`.

    Python 3.12+ has `filter="data"`, which enforces this and more. It is used
    when present, but the explicit check runs regardless: this is the one place
    the script handles bytes it did not create, and the guarantee is cheap
    enough to state twice rather than to inherit from a version check.
    """
    with tarfile.open(archive, "r:gz") as tar:
        for member in tar.getmembers():
            name = Path(member.name)
            if name.is_absolute() or ".." in name.parts:
                sys.exit(f"[FAIL] archive member escapes its root: {member.name}")
            if member.issym() or member.islnk():
                link = Path(member.linkname)
                if link.is_absolute() or ".." in link.parts:
                    sys.exit(f"[FAIL] archive link escapes its root: {member.name}")

        if sys.version_info >= (3, 12):
            tar.extractall(into, filter="data")
        else:
            tar.extractall(into)  # noqa: S202 - members validated above


def install(key: str, dest: Path, force: bool) -> int:
    """Installs the pinned build for `key` into `dest`. Returns an exit code."""
    expected = PINS[key]
    asset = f"pdfium-{key}.tgz"

    # Asserted, not assumed -- see the module docstring. A v8 asset would sail
    # through the digest check, since it would be pinned to its own digest.
    if "v8" in asset:
        sys.exit(f"[FAIL] refusing a V8 build: {asset}")

    if not force and installed_complete(key, dest):
        print(f"[OK] pdfium {TAG} {key} already installed at {dest}")
        return 0

    url = f"{RELEASE_URL}/{TAG.replace('/', '%2F')}/{asset}"
    print(f"[..] fetching {asset} from {TAG}")

    with tempfile.TemporaryDirectory(prefix="tpdf-pdfium-") as tmp:
        archive = Path(tmp) / asset
        actual = download(url, archive)

        if actual != expected:
            print(f"[FAIL] digest mismatch for {asset}", file=sys.stderr)
            print(f"       expected {expected}", file=sys.stderr)
            print(f"       actual   {actual}", file=sys.stderr)
            print(
                "       Nothing was installed. Either the release was re-cut "
                "upstream\n       or the download is not what it claims to be; "
                "do not 'fix' this\n       by pasting the actual digest into "
                "PINS without finding out which.",
                file=sys.stderr,
            )
            return 1

        staging = Path(tmp) / "tree"
        safe_extract(archive, staging)

        library = staging / library_path(key)
        if not library.is_file():
            sys.exit(f"[FAIL] {asset} contains no {library_path(key)}")

        (staging / "VERSION.txt").write_text(f"{TAG}\n", encoding="utf-8")
        # Two lines: the archive, and the library extracted from it. The second
        # is hashed here, while the bytes are still the ones the pinned archive
        # produced and nothing else has been near them.
        (staging / "SHA256.txt").write_text(
            f"{expected}  {asset}\n{file_digest(library)}  {library_path(key)}\n",
            encoding="utf-8",
        )

        # Swap late: the old tree stays usable until the new one is complete.
        dest.parent.mkdir(parents=True, exist_ok=True)
        if dest.exists():
            shutil.rmtree(dest)
        shutil.move(str(staging), str(dest))

    size = (dest / library_path(key)).stat().st_size
    print(f"[OK] pdfium {TAG} {key} -> {dest} ({size:,} bytes)")
    return 0


def check(key: str, dest: Path) -> int:
    """Verifies the install matches the pin, without the network.

    Three questions, and until 2026-08-02 this asked only the first: is the
    recorded archive the pinned one, is the loadable library actually there,
    and is it still the library that was extracted. The second is asked about
    `library_path(key)` by name rather than about a directory or a glob, for
    the reason the module docstring gives.
    """
    stamp = read_stamp(dest)
    if stamp is None:
        print(f"[FAIL] no pdfium install at {dest}", file=sys.stderr)
        print("       Run scripts/fetch_pdfium.py", file=sys.stderr)
        return 1

    archive, recorded, name = stamp
    library = dest / library_path(key)

    if archive != PINS[key]:
        print(f"[FAIL] {dest} is not the pinned build", file=sys.stderr)
        print(f"       expected {PINS[key]} ({TAG} {key})", file=sys.stderr)
        print(f"       recorded {archive}", file=sys.stderr)
        return 1

    if not library.is_file():
        print(
            f"[FAIL] {dest} has a stamp and no library: {library_path(key)} "
            "is missing.\n"
            "       A stamp without the artefact it stamps is worth nothing. "
            "Run\n       scripts/fetch_pdfium.py --force",
            file=sys.stderr,
        )
        return 1

    if recorded is None:
        # An install predating the library line. The tree is the one the pinned
        # archive produced -- that was checked when it was fetched -- so this is
        # not a failure, but the check is the weaker one and has to say which
        # check it just ran. Refusing here would turn a good tree on a machine
        # that fetched it last month into a red gate.
        print(
            f"[WARN] {dest}/SHA256.txt predates the library digest line, so "
            "this run\n       verified that "
            f"{library_path(key)} exists, not that it is unaltered.\n"
            "       Run scripts/fetch_pdfium.py to re-fetch and record it.",
            file=sys.stderr,
        )
        print(f"[OK] pdfium {TAG} {key} present at {dest} (archive pin matches)")
        return 0

    if name != library_path(key):
        print(
            f"[FAIL] {dest} is stamped for {name}, but {key} loads "
            f"{library_path(key)}.\n"
            "       This tree was installed for a different platform.",
            file=sys.stderr,
        )
        return 1

    actual = file_digest(library)
    if actual != recorded:
        print(
            f"[FAIL] {library_path(key)} is not the library that was installed",
            file=sys.stderr,
        )
        print(f"       recorded {recorded}", file=sys.stderr)
        print(f"       on disk   {actual}", file=sys.stderr)
        print(
            "       The archive digest still matches the pin, so the stamp is "
            "the pinned\n       build and the library beside it is not. Run "
            "scripts/fetch_pdfium.py --force.",
            file=sys.stderr,
        )
        return 1

    print(f"[OK] pdfium {TAG} {key} verified at {dest} ({library_path(key)} unaltered)")
    return 0


def main() -> int:
    """Parses arguments and dispatches to install or check."""
    repo_root = Path(__file__).resolve().parent.parent

    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument(
        "--platform",
        choices=sorted(PINS),
        help="target to install (default: this machine)",
    )
    parser.add_argument(
        "--dest",
        type=Path,
        help="install directory (default: <repo>/vendor/pdfium)",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify an existing install and exit; download nothing",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="reinstall even when the pin already matches",
    )
    args = parser.parse_args()

    key = args.platform or host_platform()
    dest = args.dest or repo_root / "vendor" / "pdfium"
    dest = Path(os.path.abspath(dest))

    return check(key, dest) if args.check else install(key, dest, args.force)


if __name__ == "__main__":
    sys.exit(main())
