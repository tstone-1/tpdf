#!/usr/bin/env python3
"""Build and test a source-pinned PDFium candidate in a NEW disposable directory.

uv run --with-requirements scripts/pdfium-fixture-tools.txt scripts/build_pdfium.py \
    --work /path/to/empty/build --output /path/to/artifacts

Only native mac-arm64 and win-x64 are supported: those are tpdf's shipped
architectures. Requires full Xcode on macOS, or VS 2022 with the C++ workload
on Windows. --install-windows-sdk installs Microsoft's digest-checked SDK
installer on a disposable CI runner; it is never implicit on a user's machine.

Source, build patches and depot_tools are pinned; Clang/GN/Ninja follow DEPS.
The host Xcode/Windows SDK and MSVC CRT are recorded, not hermetically bundled.
This is a repeatable source build, not a claim of bit-identical compiler output.
Only archives that pass both the control/candidate differential and upstream
text tests are emitted. This script neither publishes nor changes vendor/pdfium.
"""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request

from pdfium_verify import verify

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "scripts/pdfium_build.json"
PATCH = ROOT / "scripts/pdfium_rtl.patch"


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def run(args, cwd, env=None, *, capture=False, timeout=7200):
    print("[..] " + str(args[0]) + " " + " ".join(map(str, args[1:3])), flush=True)
    result = subprocess.run(list(map(str, args)), cwd=cwd, env=env,
                            text=True, encoding="utf-8", errors="replace", timeout=timeout,
                            stdout=subprocess.PIPE if capture else None,
                            stderr=subprocess.STDOUT if capture else None)
    if capture and result.returncode:
        print(result.stdout, flush=True)
    result.check_returncode()
    return result.stdout or ""


def checkout(url, revision, path):
    # Refuse existing trees rather than resetting a developer's checkout.
    run(["git", "init", path], path.parent)
    run(["git", "fetch", "--depth", "1", url, revision], path)
    run(["git", "checkout", "--detach", "FETCH_HEAD"], path)
    actual = run(["git", "rev-parse", "HEAD"], path, capture=True).strip()
    if actual != revision:
        raise ValueError("Checkout does not match pinned revision")


def download(url, expected, dest):
    with urllib.request.urlopen(url, timeout=120) as response, dest.open("wb") as out:
        shutil.copyfileobj(response, out)
    if digest(dest) != expected:
        raise ValueError(f"Downloaded digest mismatch: {dest.name}")


def fixtures(work, pins, env):
    archive = work / "font.tar.bz2"
    download(pins["font_url"], pins["font_archive_sha256"], archive)
    # Extract only the two known regular members, never arbitrary archive paths.
    with tarfile.open(archive) as bundle:
        for member_name, local_name in [("ttf/DejaVuSans.ttf", "DejaVuSans.ttf"),
                                        ("LICENSE", "DejaVu-LICENSE.txt")]:
            member = bundle.getmember("dejavu-fonts-ttf-2.37/" + member_name)
            if not member.isfile() or member.size > 2_000_000:
                raise ValueError("Unexpected font archive member")
            with bundle.extractfile(member) as inp, (work / local_name).open("wb") as out:
                shutil.copyfileobj(inp, out)
    run([sys.executable, ROOT / "testdata/make_rtl_pdf.py", work / "fixtures",
         "--font", work / "DejaVuSans.ttf"], ROOT, env)


def canonical_archive(tree, dest):
    """Stable archive metadata; identical input bytes yield identical archives."""
    with dest.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode="w", format=tarfile.PAX_FORMAT) as bundle:
            for path in sorted(tree.rglob("*")):
                if path.is_symlink():
                    raise ValueError("Package contains a symlink")
                if not path.is_file():
                    continue
                info = tarfile.TarInfo(path.relative_to(tree).as_posix())
                info.size, info.mode, info.mtime = path.stat().st_size, 0o644, 0
                with path.open("rb") as inp:
                    bundle.addfile(info, inp)


def complete_licenses(source, stage, log):
    # The supplier at the pinned revision only warns on these two libraries.
    # Both are permissive; copy notices from the actual DEPS-pinned sources.
    supplements = {
        "dragonbox": ("third_party/dragonbox/src/LICENSE-Boost", "dragonbox.txt"),
        "harfbuzz": ("third_party/harfbuzz/src/COPYING", "harfbuzz.txt"),
    }
    for line in log.splitlines():
        if line.startswith("WARNING:"):
            match = re.fullmatch(r"WARNING: unknow library (\w+)", line)
            if not match or match[1] not in supplements:
                raise ValueError("License inventory incomplete: " + line)
            src, name = supplements[match[1]]
            shutil.copy2(source / src, stage / "licenses" / name)
    if not (stage / "licenses/pdfium.txt").is_file():
        raise ValueError("PDFium license is absent")
    for path in (stage / "licenses").iterdir():
        if not path.is_file() or not path.read_bytes().strip():
            raise ValueError("License inventory contains an empty or invalid notice")


def check_upstream_report(path, expected):
    report = json.loads(path.read_text(encoding="utf-8"))
    if (report.get("tests") != expected or report.get("failures") != 0
            or report.get("disabled") != 0 or report.get("errors", 0) != 0):
        raise ValueError("Unexpected upstream text test count or failures")
    tests = [test for suite in report["testsuites"] for test in suite["testsuite"]]
    if len(tests) != expected or any(t.get("status") != "RUN" or t.get("result") != "COMPLETED" for t in tests):
        raise ValueError("Upstream text tests were skipped or not completed")


def windows_bash(git):
    # PATH can resolve bash.exe to the WSL launcher, even inside a Git Bash
    # workflow step. Use the shell from this Git for Windows installation.
    for parent in Path(git).resolve().parents[:3]:
        shell = parent / "bin/bash.exe"
        if shell.is_file():
            return shell
    raise ValueError("Git Bash is absent from the Git for Windows installation")


def build(work, output, install_sdk):
    pins = json.loads(CONFIG.read_text(encoding="utf-8"))
    key = {("darwin", "arm64"): "mac-arm64", ("win32", "amd64"): "win-x64"}.get(
        (sys.platform, platform.machine().lower()))
    if key is None:
        raise ValueError("Build on a native mac-arm64 or win-x64 host")
    if work.exists() and any(work.iterdir()):
        raise ValueError("Build directory must be empty; no existing work is reset")
    if output.exists() and any(output.iterdir()):
        raise ValueError("Artifact directory must be empty; stale artifacts are not reusable")
    work.mkdir(parents=True, exist_ok=True)
    windows = key == "win-x64"
    target_os, target_cpu = key.split("-")
    env = dict(os.environ, DEPOT_TOOLS_UPDATE="0", DEPOT_TOOLS_WIN_TOOLCHAIN="0",
               VPYTHON_BYPASS="", GCLIENT_PY3="1")
    git = shutil.which("git")
    if not git:
        raise ValueError("Git is absent")
    shell = windows_bash(git) if windows else "bash"
    preflight = "command -v sed sort cp mkdir"
    if windows:
        preflight = "case $(uname -s) in MINGW*|MSYS*) ;; *) exit 1 ;; esac; " + preflight
    run([shell, "-eu", "-c", preflight], work, env)
    # No signing credentials or repository tokens are needed by any build step.
    builder, depot = work / "builder", work / "depot_tools"
    checkout("https://github.com/bblanchon/pdfium-binaries.git", pins["builder"], builder)
    checkout("https://chromium.googlesource.com/chromium/tools/depot_tools.git", pins["depot_tools"], depot)
    env["PATH"] = str(depot) + os.pathsep + env["PATH"]
    sdk = {}
    if windows:
        sdk_root = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Windows Kits/10"
        sdk_bin = sdk_root / "bin" / pins["windows_sdk_version"] / "x64"
        if install_sdk:
            installer = work / "winsdksetup.exe"
            download(pins["windows_sdk_url"], pins["windows_sdk_installer_sha256"], installer)
            run([installer, "/features", "OptionId.DesktopCPPx64", "OptionId.WindowsDesktopDebuggers",
                 "/ceip", "off", "/quiet", "/norestart"], work, env)
        if not (sdk_bin / "rc.exe").is_file():
            raise ValueError("Pinned Windows SDK is absent; use --install-windows-sdk on a disposable runner")
        env["PATH"] = str(sdk_bin) + os.pathsep + env["PATH"]
        env["WINDOWSSDKDIR"] = str(sdk_root)
        sdk["windows_sdk"] = pins["windows_sdk_version"]
        # depot_tools intentionally invokes git.bat, which Git for Windows does
        # not supply outside a depot_tools-configured shell.
        shim = work / "shims"
        shim.mkdir()
        # git_common.py parses the final line, which must start with a quote,
        # not @. Keep echo suppression on its own line (Windows experiment).
        (shim / "git.bat").write_text('@echo off\n"' + git + '" %*\n', encoding="utf-8")
        env["PATH"] = str(shim) + os.pathsep + env["PATH"]
    else:
        sdk["xcode"] = run(["xcodebuild", "-version"], work, env, capture=True).strip()
        sdk["macos_sdk"] = run(["xcrun", "--sdk", "macosx", "--show-sdk-version"], work, env, capture=True).strip()
    client = depot / ("gclient.bat" if windows else "gclient")
    run([client, "config", "--unmanaged", "https://pdfium.googlesource.com/pdfium.git",
         "--custom-var", "checkout_configuration=minimal"], work, env)
    run([client, "sync", "-r", pins["pdfium"], "--no-history", "--shallow"], work, env)
    source = work / "pdfium"
    if run(["git", "rev-parse", "HEAD"], source, env, capture=True).strip() != pins["pdfium"]:
        raise ValueError("PDFium checkout differs from source pin")
    run([client, "revinfo", "--actual", "--output-json", work / "dependencies.json"], work, env)
    patches = [("shared_library.patch", source), ("public_headers.patch", source),
               ("clang_rt.patch", source / "build"), (f"{target_os}/build.patch", source / "build")]
    applied = {}
    for name, directory in patches:
        patch = builder / "patches" / name
        run(["git", "apply", "--check", patch], directory, env)
        run(["git", "apply", patch], directory, env)
        applied[name] = digest(patch)
    if windows:
        resource = (builder / "patches/win/resources.rc").read_text(encoding="utf-8")
        resource = resource.replace("$VERSION_CSV", "0,0,8044,1").replace("$VERSION", "0.0.8044.1")
        resource = resource.replace("$YEAR", "2026").replace("compiled by github.com/bblanchon", "tpdf compatibility build")
        (source / "resources.rc").write_text(resource, encoding="utf-8")
    build_dir = source / "out/Release"
    build_dir.mkdir(parents=True)
    gn_args = '\n'.join([
        'clang_use_chrome_plugins = false', 'is_component_build = false', 'is_debug = false',
        'pdf_enable_v8 = false', 'pdf_enable_xfa = false', 'pdf_is_standalone = true',
        'pdf_use_partition_alloc = false', f'target_cpu = "{target_cpu}"',
        f'target_os = "{target_os}"', 'treat_warnings_as_errors = false',
    ]) + '\n'
    (build_dir / "args.gn").write_text(gn_args, encoding="utf-8")
    gn = source / ("buildtools/win/gn.exe" if windows else "buildtools/mac/gn")
    ninja = source / ("third_party/ninja/ninja.exe" if windows else "third_party/ninja/ninja")
    run([gn, "gen", "out/Release"], source, env)
    deps = run([gn, "desc", "out/Release", "//:pdfium", "deps", "--all"], source, env, capture=True)
    if "//core/" not in deps or re.search(r"//(?:v8|xfa)/", deps):
        raise ValueError("PDFium dependencies empty or contain V8/XFA")
    (work / "pdfium-deps.txt").write_text(deps, encoding="utf-8")
    fixtures(work, pins, env)
    filename = "pdfium.dll" if windows else "libpdfium.dylib"
    observations = {}
    for label in ("control", "candidate"):
        if label == "candidate":
            run(["git", "apply", "--check", PATCH], source, env)
            run(["git", "apply", PATCH], source, env)
        run([ninja, "-C", "out/Release", "-j", "4", "pdfium", "pdfium_embeddertests"], source, env)
        library_dir = work / label
        library_dir.mkdir()
        shutil.copy2(build_dir / filename, library_dir / filename)
        test_report = work / f"{label}-upstream.json"
        test_filter = "FPDFTextEmbedderTest.*"
        if windows:
            # This platform-specific test is disabled in the upstream source.
            test_filter += "-FPDFTextEmbedderTest.DISABLED_TextSearchLatinExtended"
        run([build_dir / ("pdfium_embeddertests.exe" if windows else "pdfium_embeddertests"),
             f"--gtest_filter={test_filter}", f"--gtest_output=json:{test_report}"], source, env)
        check_upstream_report(test_report, 61 if windows else 62)
        record = work / f"{label}.json"
        probe = subprocess.run([sys.executable, str(ROOT / "scripts/pdfium_rtl_check.py"),
                                "--lib", str(library_dir / filename), "--fixtures", str(work / "fixtures"),
                                "--record", str(record)], cwd=ROOT, env=env, timeout=120)
        if probe.returncode != 1:
            raise ValueError("Probe did not report the expected known limitations")
        observations[label] = json.loads(record.read_text(encoding="utf-8"))
    verdict = verify(json.loads((work / "fixtures/manifest.json").read_text(encoding="utf-8")),
                     observations["control"], observations["candidate"])
    stage = builder / "staging"
    stage.mkdir()
    libdir = stage / ("bin" if windows else "lib")
    libdir.mkdir()
    shutil.copy2(work / "candidate" / filename, libdir / filename)
    if windows:
        (stage / "lib").mkdir()
        shutil.copy2(build_dir / "pdfium.dll.lib", stage / "lib/pdfium.dll.lib")
    shutil.copytree(source / "public", stage / "include")
    shutil.copy2(builder / "LICENSE", stage / "LICENSE")
    shutil.copy2(build_dir / "args.gn", stage / "args.gn")
    license_env = dict(env, PDFium_SOURCE_DIR=source.as_posix(), PDFium_BUILD_DIR=build_dir.as_posix(), PDFium_ENABLE_V8="false")
    # Explicit flags: invoking bash does not apply the script's shebang -eu.
    license_log = run([shell, "-eu", "steps/08-licenses.sh"], builder, license_env, capture=True)
    complete_licenses(source, stage, license_log)
    # The engine archive is also distributed independently of the application.
    # It must carry the licence for tpdf's patch as well as PDFium's notices.
    shutil.copy2(ROOT / "LICENSE", stage / "licenses/tpdf.txt")
    clang = source / "third_party/llvm-build/Release+Asserts/bin" / ("clang-cl.exe" if windows else "clang")
    sdk["clang"] = run([clang, "--version"], source, env, capture=True).splitlines()[0]
    if windows:
        # The environment block is generated by GN from vcvarsall. Record the
        # actual selected toolset, without publishing its machine-local path.
        environment = (build_dir / "environment.x64").read_bytes().decode("utf-8", errors="replace")
        versions = set(re.findall(r"MSVC[\\/]+(14\.\d+\.\d+)", environment, re.IGNORECASE))
        if len(versions) != 1:
            raise ValueError("Could not identify the actual MSVC CRT toolset")
        sdk["msvc_crt"] = versions.pop()
    provenance = {"schema": 1, "version": pins["version"], "platform": key,
                  "pdfium": pins["pdfium"], "builder": pins["builder"], "depot_tools": pins["depot_tools"],
                  "patch_sha256": digest(PATCH), "packaging_patches": applied, "toolchain": sdk,
                  "inputs_sha256": {name: digest(ROOT / name) for name in (
                      "LICENSE", ".github/workflows/pdfium.yml", "scripts/build_pdfium.py",
                      "scripts/pdfium_build.json", "scripts/pdfium_verify.py",
                      "scripts/pdfium_rtl_check.py", "scripts/pdfium-fixture-tools.txt",
                      "testdata/make_rtl_pdf.py", "testdata/make_multilingual_pdf.py", "testdata/make_text_pdf.py")},
                  "gn_args": gn_args, "library_sha256": digest(libdir / filename),
                  "control_library_sha256": digest(work / "control" / filename), "verification": verdict,
                  "upstream_tests": 61 if windows else 62,
                  "runner_image": os.environ.get("ImageVersion"),
                  "repository_commit": os.environ.get("GITHUB_SHA"),
                  "workflow_run": os.environ.get("GITHUB_RUN_ID")}
    write_json(stage / "PROVENANCE.json", provenance)
    dependencies = json.loads((work / "dependencies.json").read_text(encoding="utf-8"))
    if len(dependencies) < 10 or dependencies.get("pdfium", {}).get("rev") != pins["pdfium"]:
        raise ValueError("Dependency provenance is empty or has the wrong source revision")
    write_json(stage / "DEPENDENCIES.json", dependencies)
    shutil.copy2(PATCH, stage / "tpdf-rtl.patch")
    output.mkdir(parents=True, exist_ok=True)
    asset = output / f"pdfium-{key}.tgz"
    canonical_archive(stage, asset)
    (output / (asset.name + ".sha256")).write_text(f"{digest(asset)}  {asset.name}\n", encoding="ascii")
    shutil.copy2(stage / "PROVENANCE.json", output / "PROVENANCE.json")
    for name in ("control.json", "candidate.json", "control-upstream.json", "candidate-upstream.json", "pdfium-deps.txt"):
        shutil.copy2(work / name, output / name)
    shutil.copytree(work / "fixtures", output / "fixtures")
    shutil.copy2(work / "DejaVu-LICENSE.txt", output / "DejaVu-LICENSE.txt")
    print(f"[OK] {key}: tested candidate archive {asset.name}, sha256 {digest(asset)}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--install-windows-sdk", action="store_true")
    args = parser.parse_args()
    build(args.work.resolve(), args.output.resolve(), args.install_windows_sdk)


if __name__ == "__main__":
    main()
