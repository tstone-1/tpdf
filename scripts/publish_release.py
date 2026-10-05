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
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
REPO = 'tstone-1/tpdf'
PACKAGE_STEP = 'Test packaged CLI and installed Python API'
PLATFORMS = {'macOS (Apple Silicon)', 'Windows (x64)'}


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
    if len(jobs) != 2 or {job['name'] for job in jobs} != PLATFORMS:
        raise RuntimeError('Release run does not contain both platform jobs')
    for job in jobs:
        checks = [step for step in job['steps'] if step['name'] == PACKAGE_STEP]
        if (job['conclusion'] != 'success' or len(checks) != 1 or
                checks[0]['conclusion'] != 'success'):
            raise RuntimeError(f'{job["name"]}: packaged CLI/API check did not pass')


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tag')
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-rc\d+)?', args.tag):
        parser.error('expected a version tag, optionally followed by -rcN')
    sha = tag_commit(args.tag)
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
        nodes { databaseId tagName isDraft }
      }
    } }'''
    releases = gh('api', 'graphql', '-f', 'query=' + query)['data']['repository']['releases']['nodes']
    drafts = [release for release in releases if release['tagName'] == args.tag]
    if len(drafts) != 1 or not drafts[0]['isDraft']:
        raise RuntimeError('expected exactly one unpublished draft for this tag')
    print(f'[PASS] {args.tag} at {sha}: both packaged API checks passed in run {latest["databaseId"]}, '
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
