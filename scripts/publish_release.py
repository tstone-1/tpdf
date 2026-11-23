#!/usr/bin/env python3
"""Check a release's exact tag/run before allowing its draft to become public.

python scripts/publish_release.py vYY.M.MICRO            # read-only check
python scripts/publish_release.py vYY.M.MICRO --publish  # explicitly publish

Publication also needs the newest `audit.yml` run on the tag's commit to have
completed successfully. A missing, unfinished or failed audit refuses it.

Requires gh authenticated as a repository owner. Direct GitHub owner actions
can bypass this helper; the normal release procedure uses it instead of an
unconditional `gh release edit --draft=false`.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
REPO = 'tstone-1/tpdf'
PACKAGE_STEP = 'Test packaged CLI and installed Python API'
MAC_STEPS = ('Notarize and staple the macOS DMG',
             'Verify the macOS build is signed, notarized and stapled')
PLATFORMS = {'macOS (Apple Silicon)', 'Windows (x64)', 'Linux (x64)'}


def gh(*args):
    result = subprocess.run(['gh', *args], cwd=ROOT, capture_output=True,
                            text=True, check=True, timeout=120)
    return json.loads(result.stdout)


def tag_commit(tag):
    reference = gh('api', f'repos/{REPO}/git/ref/tags/{tag}')['object']
    for _ in range(8):
        if reference['type'] == 'commit':
            return reference['sha']
        if reference['type'] != 'tag':
            break
        reference = gh('api', f'repos/{REPO}/git/tags/{reference["sha"]}')['object']
    raise RuntimeError('tag does not resolve to a commit')


def require_passed(run, tag, sha):
    if (run['headSha'] != sha or run['headBranch'] != tag or
            run['status'] != 'completed' or run['conclusion'] != 'success'):
        raise RuntimeError('the latest Release run for this tag/commit has not passed')
    jobs = [job for job in run['jobs'] if job['name'] in PLATFORMS]
    if len(jobs) != len(PLATFORMS) or {job['name'] for job in jobs} != PLATFORMS:
        raise RuntimeError('Release run does not contain every platform job')
    for job in jobs:
        required = [PACKAGE_STEP]
        if job['name'] == 'macOS (Apple Silicon)' and '-rc' not in tag:
            required.extend(MAC_STEPS)
        if job['conclusion'] != 'success':
            raise RuntimeError(f'{job["name"]}: job did not pass')
        for name in required:
            checks = [step['conclusion'] for step in job['steps'] if step['name'] == name]
            if checks != ['success']:
                raise RuntimeError(f'{job["name"]}: {name} did not pass')


def require_audited(runs, sha):
    """Refuses unless the newest Audit run on this commit completed green.

    The newest, not any: the audit's answer changes when an advisory is
    published, so a later scheduled run on the same commit overrules an earlier
    green one. Pull-request runs are not counted, since they audit a merge
    commit. An empty list is a refusal, never a pass.
    """
    counted = [run for run in runs
               if run['headSha'] == sha and run['event'] != 'pull_request']
    if not counted:
        raise RuntimeError('no Audit run exists for this commit')
    latest = max(counted, key=lambda run: (run['createdAt'], run['databaseId']))
    if latest['status'] != 'completed':
        raise RuntimeError('the latest Audit run for this commit has not finished')
    if latest['conclusion'] != 'success':
        raise RuntimeError('the latest Audit run for this commit did not pass')
    return latest


def tested_packages(run_id):
    """Read only the package evidence uploaded by this release run."""
    with tempfile.TemporaryDirectory() as temp:
        subprocess.run(['gh', 'run', 'download', str(run_id), '--repo', REPO,
                        '--pattern', 'packaged-api-*', '--dir', temp], check=True, timeout=120)
        reports = [json.loads(path.read_text(encoding='utf-8'))
                   for path in Path(temp).glob('packaged-api-*/packaged-api.json')]
    if sorted(report['platform'] for report in reports) != ['darwin', 'linux', 'win32']:
        raise RuntimeError('incomplete platform package evidence')
    return [package for report in reports for package in report['packages']]


def download_asset(asset, destination):
    """Read the selected draft asset by ID, including before its public URL exists."""
    with destination.open('wb') as output:
        subprocess.run(['gh', 'api', '-H', 'Accept: application/octet-stream',
                        f'repos/{REPO}/releases/assets/{asset["id"]}'],
                       stdout=output, check=True, timeout=300)
    with destination.open('rb') as received:
        if 'sha256:' + hashlib.file_digest(received, 'sha256').hexdigest() != asset['digest']:
            raise RuntimeError(f'{destination.name} differs from its listed digest')


def check_updater(folder, version, platforms, public_key):
    try:
        manifest = json.loads((folder / 'latest.json').read_bytes())
    except ValueError as error:
        raise RuntimeError('draft latest.json is unreadable') from error
    offers = manifest.get('platforms') if isinstance(manifest, dict) else None
    if (not isinstance(offers, dict) or manifest.get('version') != version
            or offers.keys() != platforms.keys()):
        raise RuntimeError('draft updater manifest has the wrong version or platforms')
    keyfile = folder / 'updater.pub'
    keyfile.write_bytes(base64.b64decode(public_key, validate=True))
    for name in sorted(set(platforms.values())):
        signature = (folder / (name + '.sig')).read_text(encoding='ascii').strip()
        url = f'https://github.com/{REPO}/releases/latest/download/{name}'
        for platform, package in platforms.items():
            if package != name:
                continue
            offer = offers[platform]
            if (not isinstance(offer, dict) or offer.get('url') != url
                    or not signature or offer.get('signature') != signature):
                raise RuntimeError(f'{platform}: updater URL or signature differs from {name}')
        sigfile = folder / (name + '.minisig')
        sigfile.write_bytes(base64.b64decode(signature, validate=True))
        # -Q prints only the authenticated trusted comment after verifying the package.
        verified = subprocess.run(['minisign', '-V', '-Q', '-p', str(keyfile), '-x', str(sigfile),
                                   '-m', str(folder / name)], capture_output=True, text=True, timeout=120)
        if verified.returncode:
            raise RuntimeError(f'invalid updater signature for {name}: {verified.stderr.strip()}')
        versions = [field[8:] for field in verified.stdout.rstrip('\r\n').split('\t')
                    if field.startswith('version:')]
        if versions != [version]:
            raise RuntimeError(f'{name} was not signed for updater version {version}')


def require_assets(draft, version, packages, sha):
    installer = f'tpdf_{version}_x64-setup.exe'
    updater = 'tpdf_aarch64.app.tar.gz'
    expected = {'latest.json', installer, installer + '.sig', updater, updater + '.sig',
                f'tpdf_{version}_aarch64.dmg', f'tpdf_{version}_amd64.deb',
                f'tpdf_{version}_amd64.AppImage'}
    # The macOS bundler calls its updater tpdf.app.tar.gz locally. The release
    # action adds the architecture to that name when it uploads the same bytes.
    digests = {}
    for package in packages:
        name = package['package']
        if name == 'tpdf.app.tar.gz':
            name = updater
        digest = package['sha256']
        if name in digests or not re.fullmatch(r'[a-f0-9]{64}', digest):
            raise RuntimeError(f'invalid package evidence for {name}')
        digests[name] = 'sha256:' + digest
    tested = expected - {'latest.json', installer + '.sig', updater + '.sig'}
    if set(digests) != tested:
        raise RuntimeError('package evidence does not cover the five release packages')
    assets = draft['releaseAssets']
    actual = {item['name'] for item in assets['nodes']}
    if actual != expected or assets['totalCount'] != len(expected):
        raise RuntimeError(f'draft asset mismatch: missing {sorted(expected - actual)}, '
                           f'unexpected {sorted(actual - expected)}, total {assets["totalCount"]}')

    # REST reports the digest of the bytes stored on GitHub for each asset.
    stored = gh('api', f'repos/{REPO}/releases/{draft["databaseId"]}/assets?per_page=100')
    if (len(stored) != len(expected) or {a['name'] for a in stored} != expected
            or any(a.get('digest') != digests[a['name']] for a in stored if a['name'] in tested)):
        raise RuntimeError('draft assets differ from the tested release packages')

    platforms = {'darwin-aarch64': updater, 'darwin-aarch64-app': updater,
                 'windows-x86_64': installer, 'windows-x86_64-nsis': installer}
    config = gh('api', '-H', 'Accept: application/vnd.github.raw+json',
                f'repos/{REPO}/contents/src-tauri/tauri.conf.json?ref={sha}')
    with tempfile.TemporaryDirectory() as temp:
        folder = Path(temp)
        for asset in stored:
            name = asset['name']
            if name in (updater, installer, 'latest.json') or name.endswith('.sig'):
                download_asset(asset, folder / name)
        check_updater(folder, version, platforms, config['plugins']['updater']['pubkey'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tag')
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-rc\d+)?', args.tag):
        parser.error('expected a version tag, optionally followed by -rcN')
    rehearsal = '-rc' in args.tag
    if rehearsal and args.publish:
        parser.error('release candidates are for rehearsal only')
    sha = tag_commit(args.tag)
    if not rehearsal:
        package = gh('api', '-H', 'Accept: application/vnd.github.raw+json',
                     f'repos/{REPO}/contents/package.json?ref={sha}')
        version = package['version']
        if args.tag != f'v{version}':
            raise RuntimeError('stable tag differs from package.json at the tagged commit')
    runs = gh('run', 'list', '--repo', REPO, '--workflow', 'release.yml',
              '--commit', sha, '--event', 'push', '--limit', '100',
              '--json', 'databaseId,headBranch,createdAt')
    matching = [run for run in runs if run['headBranch'] == args.tag]
    if not matching:
        raise RuntimeError('no Release run exists for this tag and commit')
    latest = max(matching, key=lambda run: (run['createdAt'], run['databaseId']))
    run = gh('run', 'view', str(latest['databaseId']), '--repo', REPO,
             '--json', 'headSha,headBranch,status,conclusion,jobs')
    require_passed(run, args.tag, sha)
    audit = require_audited(
        gh('run', 'list', '--repo', REPO, '--workflow', 'audit.yml',
           '--commit', sha, '--limit', '100',
           '--json', 'databaseId,headSha,event,status,conclusion,createdAt'), sha)
    # Resolve drafts by id, since tag-based REST lookup can miss them. Refuse
    # duplicates instead of silently publishing whichever lookup found first.
    query = '''{ repository(owner: "tstone-1", name: "tpdf") {
      releases(first: 100, orderBy: {field: CREATED_AT, direction: DESC}) {
        nodes { databaseId tagName isDraft releaseAssets(first: 10) { totalCount nodes { name } } }
      }
    } }'''
    releases = gh('api', 'graphql', '-f', 'query=' + query)['data']['repository']['releases']['nodes']
    drafts = [release for release in releases if release['tagName'] == args.tag]
    if len(drafts) != 1 or not drafts[0]['isDraft']:
        raise RuntimeError('expected exactly one unpublished draft for this tag')
    if not rehearsal:
        require_assets(drafts[0], version, tested_packages(latest['databaseId']), sha)
    print(f'[PASS] {args.tag} at {sha}: all {len(PLATFORMS)} packaged API checks passed in run {latest["databaseId"]}, '
          f'audit run {audit["databaseId"]} passed')
    if args.publish:
        # Re-read the tag immediately before mutation; never publish against a
        # check of a previous target after somebody moved it.
        if tag_commit(args.tag) != sha:
            raise RuntimeError('tag moved during publication checks')
        gh('api', '--method', 'PATCH', f'repos/{REPO}/releases/{drafts[0]["databaseId"]}',
           '-F', 'draft=false')
        print(f'[PUBLISHED] https://github.com/{REPO}/releases/tag/{args.tag}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
