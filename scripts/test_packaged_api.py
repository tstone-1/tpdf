"""Failure controls for the release-only API test runner and its wiring."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from copy import deepcopy
from unittest.mock import patch

from check_packaged_api import ROOT, RUN_SUITE, one
from check_workflow_parity import authority, job_block, packaged_api, steps
from publish_release import PACKAGE_STEP, PLATFORMS, require_audited, require_passed
import publish_release


def passing_steps(platform):
    names = [PACKAGE_STEP]
    if platform == 'macOS (Apple Silicon)':
        names.extend(publish_release.MAC_STEPS)
    return [dict(name=name, conclusion='success') for name in names]


class PackageGateTests(unittest.TestCase):
    def suite(self, body, *, origin_inside=True):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tests = root / 'test_api.py'
            tests.write_text('import unittest\nclass ClientTests(unittest.TestCase):\n' + body,
                             encoding='utf-8')
            result = root / 'result.json'
            # No third-party code needed: supply only the module origin that
            # the child runner checks, then exercise real unittest execution.
            origin = root / 'tpdf_client/__init__.py' if origin_inside else root.parent / 'wrong-client.py'
            setup = ('import sys,types; '
                     f'sys.prefix={str(root)!r}; '
                     f'sys.modules["tpdf_client"]=types.SimpleNamespace(__file__={str(origin)!r});\n')
            process = subprocess.run([sys.executable, '-I', '-c', setup + RUN_SUITE,
                                      str(tests), str(result)], capture_output=True, text=True)
            report = json.loads(result.read_text()) if result.exists() else None
            return process, report

    @staticmethod
    def cases(statement='pass'):
        return ''.join(f'    def test_{i}(self): {statement}\n' for i in range(12))

    def test_gate_step_policy_and_permission_precedence(self):
        text = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        block = job_block(text, 'gates')
        baseline = steps(block)
        for key in ["if: runner.os == 'Linux'", 'continue-on-error: true']:
            changed = [line.replace('      - name: Quality gates', '      - name: Quality gates\n        ' + key)
                       for line in block]
            self.assertNotEqual(baseline, steps('\n'.join(changed).splitlines()))
        self.assertEqual(baseline, steps([line.replace('name: Quality gates', 'name: Build checks') for line in block]))
        for job, workflow, safe in [('write-all', 'read-all', False), ('read-all', 'write-all', True),
                                   ('{contents: write}', 'read-all', False), ('{contents: read}', 'write-all', True)]:
            self.assertEqual(not authority('ci', [f'    permissions: {job}', *block], f'permissions: {workflow}'), safe)

    def test_complete_suite_passes(self):
        process, report = self.suite(self.cases())
        self.assertEqual(process.returncode, 0, process.stderr)
        self.assertEqual(report, dict(tests=12, failures=0, errors=0, skipped=0))

    def test_empty_suite_is_not_success(self):
        process, _ = self.suite('    pass\n')
        self.assertNotEqual(process.returncode, 0)
        self.assertIn('incomplete API suite: 0', process.stderr)

    def test_skips_and_assertion_failures_fail_the_process(self):
        for statement, field in [('self.skipTest("control")', 'skipped'),
                                 ('self.fail("control")', 'failures')]:
            with self.subTest(field=field):
                process, report = self.suite(self.cases(statement))
                self.assertNotEqual(process.returncode, 0)
                self.assertEqual(report[field], 12)

    def test_checkout_import_is_rejected(self):
        process, _ = self.suite(self.cases(), origin_inside=False)
        self.assertNotEqual(process.returncode, 0)
        self.assertIn('client was not installed', process.stderr)

    def test_missing_and_ambiguous_packages_are_rejected(self):
        for paths in [[], [Path('one'), Path('two')]]:
            with self.assertRaisesRegex(RuntimeError, 'expected exactly one'):
                one(paths, 'package')

    def test_removed_or_weakened_workflow_step_is_rejected(self):
        text = (ROOT / '.github/workflows/release.yml').read_text(encoding='utf-8')
        self.assertEqual(packaged_api(text), [])
        command = 'run: python scripts/check_packaged_api.py'
        self.assertEqual(text.count(command), 1)
        for replacement in ['run: echo skipped',
                            'if: runner.os == "macOS"\n        ' + command,
                            'continue-on-error: true\n        ' + command]:
            with self.subTest(replacement=replacement):
                self.assertTrue(packaged_api(text.replace(command, replacement)))
        self.assertTrue(packaged_api(text.replace('  release:\n',
                        '  release:\n    continue-on-error: true\n')))
        self.assertTrue(packaged_api(text.replace(PACKAGE_STEP, 'Renamed package check')))

    def test_publication_requires_all_platform_and_signing_steps(self):
        run = dict(headSha='abc', headBranch='v26.9.22', status='completed',
                   conclusion='success', jobs=[
            dict(name=name, conclusion='success', steps=passing_steps(name))
            for name in sorted(PLATFORMS)])
        require_passed(run, 'v26.9.22', 'abc')
        for key, value in [('headSha', 'other'), ('headBranch', 'v26.9.21'),
                           ('status', 'in_progress'), ('conclusion', 'failure')]:
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                require_passed({**run, key: value}, 'v26.9.22', 'abc')
        for jobs in [[], run['jobs'][:1], [run['jobs'][0]] * 2]:
            with self.assertRaises(RuntimeError):
                require_passed({**run, 'jobs': jobs}, 'v26.9.22', 'abc')
        # A run from before Linux shipped, macOS and Windows only, is not enough.
        two = [job for job in run['jobs'] if job['name'] != 'Linux (x64)']
        self.assertEqual(len(two), 2)
        with self.assertRaises(RuntimeError):
            require_passed({**run, 'jobs': two}, 'v26.9.22', 'abc')
        for step in publish_release.MAC_STEPS:
            wrong = deepcopy(run)
            mac = next(job for job in wrong['jobs'] if job['name'] == 'macOS (Apple Silicon)')
            next(item for item in mac['steps'] if item['name'] == step)['conclusion'] = 'skipped'
            with self.assertRaisesRegex(RuntimeError, step):
                require_passed(wrong, 'v26.9.22', 'abc')
        for index in range(len(PLATFORMS)):
            for state in ['skipped', 'failure', 'cancelled']:
                wrong = deepcopy(run)
                wrong['jobs'][index]['steps'][0]['conclusion'] = state
                with self.assertRaises(RuntimeError):
                    require_passed(wrong, 'v26.9.22', 'abc')
            wrong = deepcopy(run)
            wrong['jobs'][index]['steps'] = []
            with self.assertRaises(RuntimeError):
                require_passed(wrong, 'v26.9.22', 'abc')

    def test_publish_guard_requires_the_newest_audit_run_to_have_passed(self):
        green = dict(databaseId=7, headSha='abc', event='push', status='completed',
                     conclusion='success', createdAt='2026-09-29T10:00:00Z')
        self.assertEqual(require_audited([green], 'abc')['databaseId'], 7)
        later = dict(green, databaseId=8, event='schedule', createdAt='2026-09-30T06:17:00Z')
        # A later green run is the one reported; an older red one does not refuse.
        self.assertEqual(
            require_audited([dict(green, conclusion='failure'), later], 'abc')['databaseId'], 8)
        refused = [
            [],
            [dict(green, headSha='other')],
            [dict(green, event='pull_request')],
            [dict(green, status='in_progress', conclusion=None)],
            [dict(green, status='queued', conclusion='')],
            [dict(green, conclusion='failure')],
            [dict(green, conclusion='cancelled')],
            # A newer run overrules an older green one, whatever started it.
            [green, dict(later, conclusion='failure')],
            [green, dict(later, status='in_progress', conclusion=None)],
        ]
        for runs in refused:
            with self.subTest(runs=runs), self.assertRaises(RuntimeError):
                require_audited(runs, 'abc')
        # An unfinished run is named as unfinished, also when GitHub already
        # reports a conclusion for it.
        with self.assertRaisesRegex(RuntimeError, 'not finished'):
            require_audited([dict(green, status='in_progress')], 'abc')

    def test_publish_entry_point_calls_the_guard_before_mutating(self):
        for mode in ['passed', 'failed', 'moved', 'dry-run', 'unaudited', 'audit-red', 'mismatch', 'rc-check', 'rc-publish', 'unsigned', 'missing-sig', 'substituted', 'stale']:
            tag = 'v26.9.22-rc1' if mode.startswith('rc-') else 'v26.9.22'
            packages = ['tpdf.app.tar.gz', 'tpdf_26.9.22_aarch64.dmg', 'tpdf_26.9.22_x64-setup.exe',
                        'tpdf_26.9.22_amd64.deb', 'tpdf_26.9.22_amd64.AppImage']
            evidence = [dict(package=p, sha256='a' * 64) for p in packages]
            calls = []
            reads = 0
            def fake_gh(*args):
                nonlocal reads
                calls.append(args)
                if args[0] == 'api' and '/git/ref/' in args[1]:
                    reads += 1
                    sha = 'moved' if mode == 'moved' and reads == 2 else 'abc'
                    return dict(object=dict(type='commit', sha=sha))
                if args == ('api', '-H', 'Accept: application/vnd.github.raw+json',
                            'repos/tstone-1/tpdf/contents/package.json?ref=abc'):
                    version = '26.9.21' if mode == 'mismatch' else '26.9.22'
                    return dict(version=version)
                if args[:2] == ('run', 'list') and 'audit.yml' in args:
                    if mode == 'unaudited':
                        return []
                    return [dict(databaseId=3, headSha='abc', event='push', status='completed',
                                 conclusion='failure' if mode == 'audit-red' else 'success',
                                 createdAt='2026-09-29')]
                if args[:2] == ('run', 'list'):
                    return [dict(databaseId=1, headBranch=tag, createdAt='2026-09-29')]
                if args[:2] == ('run', 'view'):
                    run = dict(headSha='abc', headBranch=tag, status='completed',
                        conclusion='failure' if mode == 'failed' else 'success', jobs=[
                            dict(name=name, conclusion='success',
                                 steps=passing_steps(name))
                            for name in PLATFORMS])
                    if mode == 'unsigned':
                        mac = next(job for job in run['jobs'] if job['name'] == 'macOS (Apple Silicon)')
                        mac['steps'][-1]['conclusion'] = 'skipped'
                    return run
                if args[:2] == ('api', 'graphql'):
                    self.assertIn('releaseAssets', args[-1])
                    assets = ['latest.json', 'tpdf_aarch64.app.tar.gz', 'tpdf_aarch64.app.tar.gz.sig',
                              'tpdf_26.9.22_aarch64.dmg', 'tpdf_26.9.22_amd64.deb',
                              'tpdf_26.9.22_amd64.AppImage', 'tpdf_26.9.22_x64-setup.exe',
                              'tpdf_26.9.22_x64-setup.exe.sig']
                    if mode in ('missing-sig', 'substituted'):
                        assets.pop()
                    if mode == 'substituted':
                        assets.append('unexpected.sig')
                    remote = dict(totalCount=len(assets), nodes=[dict(name=name) for name in assets])
                    return dict(data=dict(repository=dict(releases=dict(nodes=[
                        dict(databaseId=2, tagName=tag, isDraft=True, releaseAssets=remote)]))))
                if args == ('api', 'repos/tstone-1/tpdf/releases/2/assets?per_page=100'):
                    names = [*packages[1:], 'tpdf_aarch64.app.tar.gz', 'tpdf_aarch64.app.tar.gz.sig',
                             'tpdf_26.9.22_x64-setup.exe.sig', 'latest.json']
                    return [dict(name=p, digest='sha256:' + ('b' if mode == 'stale' and p.endswith('.deb') else 'a') * 64)
                            for p in names]
                if args[:3] == ('api', '--method', 'PATCH'):
                    return dict(draft=False)
                raise AssertionError(args)
            argv = ['publish_release.py', tag]
            if mode not in ['dry-run', 'rc-check']:
                argv.append('--publish')
            with self.subTest(mode=mode), patch.object(publish_release, 'gh', fake_gh), \
                    patch.object(publish_release, 'tested_packages', return_value=evidence) as reports, \
                    patch.object(sys, 'argv', argv), patch('builtins.print'):
                if mode == 'rc-publish':
                    with patch('sys.stderr'), self.assertRaises(SystemExit):
                        publish_release.main()
                    self.assertEqual(calls, [])
                elif mode in ['failed', 'moved', 'unaudited', 'audit-red', 'mismatch', 'unsigned', 'missing-sig', 'substituted', 'stale']:
                    with self.assertRaises(RuntimeError):
                        publish_release.main()
                else:
                    self.assertEqual(publish_release.main(), 0)
                if mode in ('passed', 'stale'):
                    reports.assert_called_once_with(1)
                mutations = [args for args in calls if args[:3] == ('api', '--method', 'PATCH')]
                self.assertEqual(len(mutations), int(mode == 'passed'))


if __name__ == '__main__':
    unittest.main()
