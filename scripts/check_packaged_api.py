#!/usr/bin/env python3
"""Exercise an installed wheel against the CLI extracted from release packages.

Requires uv, plus hdiutil on macOS or 7z on Windows. No installer is
registered and no existing installation is changed. Run after Tauri bundling:
  python scripts/check_packaged_api.py --bundle-dir src-tauri/target/release/bundle
The release workflow also passes --updater to require the macOS updater archive.
All extraction, builds and virtual environments live outside the checkout.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def one(paths, description: str) -> Path:
    found = list(paths)
    if len(found) != 1:
        raise RuntimeError(f"expected exactly one {description}, found {len(found)}")
    return found[0].resolve()


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def run(argv, *, cwd: Path, env=None, timeout=180) -> subprocess.CompletedProcess:
    return subprocess.run([str(a) for a in argv], cwd=cwd, env=env, check=True,
                          timeout=timeout)


@contextmanager
def unpack(artifact: Path, destination: Path):
    destination.mkdir()
    if artifact.suffix == '.dmg':
        attached = subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse',
                                  '-plist', str(artifact)], check=True,
                                 capture_output=True, timeout=120)
        entities = plistlib.loads(attached.stdout)['system-entities']
        mounts = [Path(e['mount-point']) for e in entities if 'mount-point' in e]
        try:
            mount = one(mounts, 'mounted DMG volume')
            app = one(mount.glob('*.app'), 'DMG application')
            # Preserve executable modes, symlinks and code signatures.
            run(['ditto', app, destination / app.name], cwd=destination)
            yield destination
        finally:
            for mount in mounts:
                run(['hdiutil', 'detach', mount], cwd=destination)
    elif artifact.name.endswith('-setup.exe'):
        run(['7z', 'x', '-y', f'-o{destination}', artifact], cwd=destination)
        yield destination
    elif artifact.name.endswith('.app.tar.gz'):
        with tarfile.open(artifact) as archive:
            archive.extractall(destination, filter='data')
        yield destination
    else:
        raise RuntimeError(f'unsupported package: {artifact.name}')


# Executed with -I in the new environment. Loading only the test file avoids
# importing api/python/tpdf_client from the checkout. Assert that property explicitly.
RUN_SUITE = '''
import json, pathlib, runpy, sys, unittest
import tpdf_client
origin = pathlib.Path(tpdf_client.__file__).resolve()
if not origin.is_relative_to(pathlib.Path(sys.prefix).resolve()):
    raise RuntimeError(f"client was not installed in the isolated environment: {origin}")
namespace = runpy.run_path(sys.argv[1])
suite = unittest.defaultTestLoader.loadTestsFromTestCase(namespace['ClientTests'])
expected = suite.countTestCases()
if expected < 12:
    raise RuntimeError(f"incomplete API suite: {expected} tests")
result = unittest.TextTestRunner(verbosity=2).run(suite)
summary = dict(tests=result.testsRun, failures=len(result.failures),
               errors=len(result.errors), skipped=len(result.skipped))
pathlib.Path(sys.argv[2]).write_text(json.dumps(summary), encoding='utf-8')
if result.testsRun != expected or result.skipped or not result.wasSuccessful():
    raise SystemExit(1)
'''


def check_package(artifact: Path, work: Path, python: Path, tests: Path,
                  expected_version: str) -> dict:
    with unpack(artifact, work / 'payload') as payload:
        windows = sys.platform == 'win32'
        cli = one(payload.rglob('tpdf-cli.exe' if windows else 'tpdf-cli'), 'packaged CLI')
        app = one(payload.rglob('tpdf.exe' if windows else 'tpdf'), 'packaged application')
        if cli.parent != app.parent:
            raise RuntimeError('packaged CLI is not beside the application')
        engine = one(payload.rglob('pdfium.dll' if windows else 'libpdfium.dylib'), 'PDFium library')
        env = {k: v for k, v in os.environ.items() if not k.startswith(
            ('PYTHON', 'TPDF_', 'DYLD_', 'LD_LIBRARY_PATH'))}
        env['TPDF_TEST_CLI'] = str(cli)
        version = subprocess.run([str(cli), '--version'], cwd=work, env=env,
                                 capture_output=True, text=True, check=True, timeout=30).stdout.strip()
        if version != f'tpdf {expected_version}':
            raise RuntimeError(f'wrong packaged version: {version!r}')
        result = work / 'tests.json'
        run([python, '-I', '-c', RUN_SUITE, tests, result], cwd=work, env=env, timeout=300)
        # A positive suite alone could pass using a development-tree engine.
        # Remove ONLY the extracted copy, and demand that the same render test
        # fails. Restore it even on failure; the original archive is untouched.
        hidden = engine.with_name(engine.name + '.hidden')
        engine.rename(hidden)
        try:
            negative = subprocess.run([str(python), '-I', str(tests),
                'ClientTests.test_render_supports_repeatable_visual_assertions'],
                cwd=work, env=env, capture_output=True, text=True, timeout=60)
            if negative.returncode == 0 or 'Ran 1 test' not in negative.stderr or 'FAILED (errors=1)' not in negative.stderr:
                raise RuntimeError('missing-engine control did not fail in the render test:\n' + negative.stderr)
            if 'pdfium' not in negative.stderr.lower():
                raise RuntimeError('missing-engine control failed for an unrelated reason:\n' + negative.stderr)
        finally:
            hidden.rename(engine)
        return dict(package=artifact.name, sha256=digest(artifact),
                    cli_sha256=digest(cli), engine_sha256=digest(engine),
                    version=version, missing_engine_rejected=True,
                    **json.loads(result.read_text(encoding='utf-8')))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle-dir', type=Path, required=True)
    parser.add_argument('--updater', action='store_true')
    parser.add_argument('--report', type=Path, default=Path('scratch/packaged-api.json'))
    args = parser.parse_args()
    # A failed retry must not leave a previous passing report behind.
    args.report.unlink(missing_ok=True)
    bundle = args.bundle_dir.resolve()
    if sys.platform == 'darwin':
        artifacts = [one((bundle / 'dmg').glob('*.dmg'), 'DMG')]
        if args.updater:
            artifacts.append(one((bundle / 'macos').glob('*.app.tar.gz'), 'updater archive'))
    elif sys.platform == 'win32':
        artifacts = [one((bundle / 'nsis').glob('*-setup.exe'), 'NSIS setup')]
    else:
        raise RuntimeError('packaged API checks require macOS or Windows')
    version = json.loads((ROOT / 'package.json').read_text(encoding='utf-8'))['version']
    with tempfile.TemporaryDirectory(prefix='tpdf-packaged-api-') as temporary:
        work = Path(temporary).resolve()
        # Build from a copy: setuptools must not leave build/egg-info in source.
        source = work / 'client'
        shutil.copytree(ROOT / 'api/python', source,
                        ignore=shutil.ignore_patterns('__pycache__', '*.egg-info', 'build', 'dist'))
        # Pin both operations: otherwise uv can select a different managed
        # interpreter for the build (including an inaccessible Windows alias).
        run(['uv', 'build', '--python', sys.executable, '--wheel', '--out-dir', work / 'dist',
             '--build-constraint', ROOT / 'scripts/api-build-constraints.txt', source], cwd=work)
        wheel = one((work / 'dist').glob('*.whl'), 'client wheel')
        venv = work / 'venv'
        run(['uv', 'venv', '--python', sys.executable, venv], cwd=work)
        python = venv / ('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
        run(['uv', 'pip', 'install', '--python', python, '--no-index', '--no-deps', wheel], cwd=work)
        tests = work / 'tests' / 'test_api.py'
        tests.parent.mkdir()
        shutil.copy2(ROOT / 'api/python/test_api.py', tests)
        reports = []
        for index, artifact in enumerate(artifacts):
            package_work = work / f'package-{index}'
            package_work.mkdir()
            print(f'[CHECK] {artifact.name}', flush=True)
            reports.append(check_package(artifact, package_work, python, tests, version))
        report = dict(schema=1, platform=sys.platform, wheel=wheel.name,
                      wheel_sha256=digest(wheel), packages=reports)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'[PASS] {len(reports)} packages; installed Python client; zero skipped tests')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
