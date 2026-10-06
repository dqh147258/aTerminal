#!/usr/bin/env python3
"""Fail-closed Linux server packaging and prerelease publication helpers."""
import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
DIST = ROOT / 'dist'
ARCHES = ('amd64', 'arm64')
TARGET = ROOT / '.server-publish-target.json'


class HubError(RuntimeError):
    def __init__(self, status):
        self.status = status
        super().__init__(f'DockerHub API returned HTTP {status}')


def run(*args, check=True):
    result = subprocess.run(args, check=check, text=True, capture_output=True, timeout=900)
    return result


def config(data=None):
    data = data if data is not None else json.loads((ROOT / 'deploy/server-release.json').read_text())
    assert type(data.get('publish')) is bool, 'publish must be a boolean'
    assert re.fullmatch(r'0|[1-9][0-9]*', data['version'].split('.')[0])
    assert re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-(alpha|beta|rc)\.[1-9][0-9]*', data['version']), 'Use an explicit prerelease version'
    if data['publish']:
        assert re.fullmatch(r'[a-z0-9][a-z0-9_-]*/[a-z0-9]+(?:[._-][a-z0-9]+)*', data['dockerhub_repository']), 'Confirm namespace/repository before publication'
        assert data['dockerhub_visibility'] in ('public', 'private'), 'Confirm repository visibility'
        fallback = data.get('dockerhub_fallback_repository', '')
        assert not fallback or re.fullmatch(r'[a-z0-9][a-z0-9_-]*/[a-z0-9]+(?:[._-][a-z0-9]+)*', fallback)
        assert type(data.get('allow_create_repository', False)) is bool
    return data


def sha():
    value = os.environ['GITHUB_SHA']
    assert re.fullmatch('[0-9a-f]{40}', value), 'Expected exact source commit'
    return value


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def api(path, token=None, data=None, allow_missing=False):
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    body = None
    if data is not None:
        headers['Content-Type'] = 'application/json'
        body = json.dumps(data).encode()
    request = urllib.request.Request('https://hub.docker.com/v2/' + path, data=body, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if allow_missing and error.code == 404:
            return None
        raise HubError(error.code) from None


def metadata(c):
    result = {}
    for arch in ARCHES:
        path = DIST / f'aterminal-server-{c["version"]}-linux-{arch}.json'
        item = json.loads(path.read_text())
        assert item['source_commit'] == sha() and item['version'] == c['version']
        assert item['platform'] == 'linux/' + arch
        expected = f'aterminal-server-{c["version"]}-linux-{arch}.tar.gz'
        assert item['archive'] == expected
        assert digest(DIST / expected) == item['sha256'], 'Image archive checksum mismatch'
        result[arch] = item
    return result


def tags(c):
    return ['v' + c['version'], 'sha-' + sha()] + [f'v{c["version"]}-{arch}' for arch in ARCHES]


def preflight(c):
    TARGET.unlink(missing_ok=True)
    assert c['publish'], 'Publication is not enabled'
    metadata(c)
    # Preflight both the release and git tag. Network/authentication failures are never absence.
    tag = 'v' + c['version']
    for endpoint in (f'repos/{{owner}}/{{repo}}/releases/tags/{tag}', f'repos/{{owner}}/{{repo}}/git/ref/tags/{tag}'):
        check = run('gh', 'api', endpoint, check=False)
        assert check.returncode != 0 and '(HTTP 404)' in check.stderr, f'GitHub version already exists or cannot be verified: {tag}'
    username, secret = os.environ['DOCKERHUB_USERNAME'], os.environ['DOCKERHUB_TOKEN']
    assert username and secret, 'Configure existing DockerHub secrets'
    token = api('auth/token', data={'identifier': username, 'secret': secret})['access_token']
    selected = None
    candidates = [c['dockerhub_repository']]
    if c.get('dockerhub_fallback_repository'):
        candidates.append(c['dockerhub_fallback_repository'])
    for candidate in candidates:
        namespace, repository = candidate.split('/')
        base = f'namespaces/{namespace}/repositories/{repository}'
        try:
            repo = api(base, token, allow_missing=True)
            if repo is None:
                # Hub reports missing namespaces as HTTP 400 on creation; resolve
                # their existence first rather than weakening generic 400 handling.
                if api(f'namespaces/{namespace}/repositories?page_size=1', token, allow_missing=True) is None:
                    print(f'{candidate}: namespace is unavailable to current credentials')
                    continue
                if not c.get('allow_create_repository', False):
                    print(f'{candidate}: repository does not exist; creation is disabled')
                    continue
                # Only the explicitly approved repository under an existing authorized
                # namespace may be created. Never create accounts/namespaces or change visibility.
                try:
                    api(f'namespaces/{namespace}/repositories', token, data={
                        'name': repository, 'namespace': namespace, 'registry': 'docker.io',
                        'is_private': c['dockerhub_visibility'] == 'private',
                        'description': 'aTerminal coordination and encrypted relay server'})
                except HubError as error:
                    if error.status != 409:
                        raise
                repo = api(base, token)
            assert repo.get('namespace') == namespace and repo.get('name') == repository, 'DockerHub returned an unexpected repository identity'
            assert type(repo.get('is_private')) is bool, 'Repository visibility could not be verified'
            if repo['is_private'] != (c['dockerhub_visibility'] == 'private'):
                print(f'{candidate}: existing visibility differs from approval; it will not be changed')
                continue
            if repo.get('permissions', {}).get('write') is not True:
                print(f'{candidate}: current credentials do not have verified write permission')
                continue
            selected = candidate
            break
        except HubError as error:
            if error.status not in (401, 403, 404):
                raise
            print(f'{candidate}: unavailable with current credentials (HTTP {error.status})')
    assert selected, 'Neither approved DockerHub repository is available with the required permission and visibility'
    print('Verified publish destination: ' + selected)
    c = dict(c, dockerhub_repository=selected)
    for tag in tags(c):
        assert api(base + '/tags/' + tag, token, allow_missing=True) is None, f'Refusing to replace existing DockerHub tag {tag}; inspect partial publication before retrying'
    TARGET.write_text(json.dumps({'repository': selected, 'source_commit': sha(), 'version': c['version']}))
    print('Verified DockerHub write permission, approved visibility, absent version tags, and exact-commit image checksums')


def package(c, arch):
    image = json.loads(run('docker', 'image', 'inspect', f'aterminal-server:test-{arch}').stdout)[0]
    assert image['Architecture'] == arch and image['Os'] == 'linux'
    labels = image['Config']['Labels']
    assert labels['org.opencontainers.image.revision'] == sha()
    assert labels['org.opencontainers.image.version'] == c['version']
    archive = f'aterminal-server-{c["version"]}-linux-{arch}.tar.gz'
    item = {'version': c['version'], 'source_commit': sha(), 'platform': 'linux/' + arch,
            'local_image': f'aterminal-server:test-{arch}', 'archive': archive, 'sha256': digest(DIST / archive), 'image_id': image['Id'],
            'tests': ['server cargo tests', 'native hardened container smoke test']}
    (DIST / archive.replace('.tar.gz', '.json')).write_text(json.dumps(item, indent=2) + '\n')
    (DIST / (archive + '.sha256')).write_text(item['sha256'] + '  ' + archive + '\n')


def push(c):
    assert c['publish']
    target = json.loads(TARGET.read_text())
    assert target['source_commit'] == sha() and target['version'] == c['version']
    assert target['repository'] in (c['dockerhub_repository'], c.get('dockerhub_fallback_repository'))
    c = dict(c, dockerhub_repository=target['repository'])
    items = metadata(c)
    repo = c['dockerhub_repository']
    references = []
    # Validate both exported archives before the first registry mutation.
    for arch, item in items.items():
        run('docker', 'load', '-i', str(DIST / item['archive']))
        loaded = json.loads(run('docker', 'image', 'inspect', f'aterminal-server:test-{arch}').stdout)[0]
        assert loaded['Id'] == item['image_id'], 'Loaded image differs from tested image'
    for arch, item in items.items():
        tag = f'{repo}:v{c["version"]}-{arch}'
        run('docker', 'tag', item['image_id'], tag)
        run('docker', 'push', tag)
        raw = run('docker', 'buildx', 'imagetools', 'inspect', '--format', '{{json .Manifest}}', tag).stdout
        manifest_digest = json.loads(raw)['digest']
        assert re.fullmatch('sha256:[0-9a-f]{64}', manifest_digest)
        remote = json.loads(run('docker', 'buildx', 'imagetools', 'inspect', '--raw', repo + '@' + manifest_digest).stdout)
        assert remote['config']['digest'] == item['image_id'], 'Remote image differs from tested image'
        item['registry_digest'] = manifest_digest
        references.append(repo + '@' + manifest_digest)
    command = ['docker', 'buildx', 'imagetools', 'create']
    for tag in tags(c)[:2]:
        command.extend(['--tag', repo + ':' + tag])
    run(*command, *references)
    version_ref = repo + ':v' + c['version']
    manifest = json.loads(run('docker', 'buildx', 'imagetools', 'inspect', '--format', '{{json .Manifest}}', version_ref).stdout)
    immutable_ref = repo + '@' + manifest['digest']
    index = json.loads(run('docker', 'buildx', 'imagetools', 'inspect', '--raw', immutable_ref).stdout)
    actual = {(x['platform']['os'], x['platform']['architecture']): x['digest'] for x in index['manifests']}
    expected = {('linux', arch): item['registry_digest'] for arch, item in items.items()}
    assert actual == expected, 'Published platforms or digests do not match tested images'
    sha_manifest = json.loads(run('docker', 'buildx', 'imagetools', 'inspect', '--format', '{{json .Manifest}}', repo + ':sha-' + sha()).stdout)
    assert sha_manifest['digest'] == manifest['digest'], 'Source SHA tag differs from version manifest'
    result = {'version': c['version'], 'source_commit': sha(), 'repository': repo, 'visibility': c['dockerhub_visibility'],
              'digest': manifest['digest'], 'pull': repo + '@' + manifest['digest'], 'images': items}
    if result['visibility'] == 'public':
        # Use an empty client configuration to prove public pulls do not depend
        # on the publishing credentials, for both native-tested platforms.
        with tempfile.TemporaryDirectory() as anonymous_config:
            public_index = json.loads(run('docker', '--config', anonymous_config, 'buildx', 'imagetools', 'inspect', '--raw', result['pull']).stdout)
            assert public_index == index, 'Anonymous manifest differs from verified index'
            for arch, item in items.items():
                pull_ref = repo + '@' + item['registry_digest']
                run('docker', '--config', anonymous_config, 'pull', '--platform', 'linux/' + arch, pull_ref)
                # Per-platform manifest refs are unambiguous even with a multi-platform
                # containerd store and do not require inspect --platform (API 1.49).
                pulled = json.loads(run('docker', 'image', 'inspect', pull_ref).stdout)[0]
                assert pulled['Id'] == item['image_id'] and pulled['Architecture'] == arch, 'Anonymous digest pull differs from tested image'
    (DIST / 'server-image.json').write_text(json.dumps(result, indent=2) + '\n')
    print('Published and verified ' + result['pull'])


def release(c):
    assert c['publish'], 'Publication is not enabled'
    result = json.loads((DIST / 'server-image.json').read_text())
    assert result['source_commit'] == sha() and result['version'] == c['version']
    assert result['repository'] in (c['dockerhub_repository'], c.get('dockerhub_fallback_repository'))
    checksums = '\n'.join(digest(p) + '  ' + p.name for p in sorted(DIST.iterdir()) if p.is_file() and p.name != 'SHA256SUMS') + '\n'
    (DIST / 'SHA256SUMS').write_text(checksums)
    note = f'''aTerminal Server Linux prerelease v{c['version']}

Source commit: {sha()}

DockerHub: `{result['repository']}:v{c['version']}` ({result['visibility']})
Immutable pull: `docker pull {result['pull']}`
Platforms: linux/amd64 and linux/arm64, each built and smoke-tested on native runners.

Assets include the tested Docker image archives, per-platform source/checksum metadata, and SHA256SUMS. Load an offline image with `docker load -i <archive.tar.gz>` and run the `local_image` tag recorded in its platform JSON. Images run as UID/GID 10001, require an explicit admin token file, and persist SQLite under /data. See deploy/SERVER-RELEASE.md for safe startup and TLS requirements.

This is a server-only prerelease. It does not certify public-network/NAT, long-term performance, mobile signing or store release readiness. No production deployment is performed. Desktop/Android test artifacts are not copied from another commit into this release.
'''
    notes = ROOT / 'release-notes.txt'
    notes.write_text(note)
    run('gh', 'release', 'create', 'v' + c['version'], '--target', sha(), '--prerelease', '--latest=false',
        '--title', 'aTerminal Server v' + c['version'], '--notes-file', str(notes),
        *[str(p) for p in sorted(DIST.iterdir()) if p.is_file()])
    published = json.loads(run('gh', 'release', 'view', 'v' + c['version'], '--json', 'isPrerelease,isDraft,url,assets').stdout)
    assert published['isPrerelease'] and not published['isDraft']
    assert {p.name for p in DIST.iterdir() if p.is_file()} == {a['name'] for a in published['assets']}
    repository = os.environ['GITHUB_REPOSITORY']
    assert re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository)
    public_url = f'https://api.github.com/repos/{repository}/releases/tags/v{c["version"]}'
    with urllib.request.urlopen(urllib.request.Request(public_url, headers={'Accept': 'application/vnd.github+json'}), timeout=30) as response:
        public_release = json.load(response)
    assert public_release['prerelease'] and not public_release['draft']
    tag_url = f'https://api.github.com/repos/{repository}/git/ref/tags/v{c["version"]}'
    with urllib.request.urlopen(tag_url, timeout=30) as response:
        tag_ref = json.load(response)
    assert tag_ref['object']['type'] == 'commit' and tag_ref['object']['sha'] == sha(), 'Release tag does not point to the tested source commit'
    public_assets = {a['name']: a for a in public_release['assets']}
    assert set(public_assets) == {p.name for p in DIST.iterdir() if p.is_file()}
    for path in DIST.iterdir():
        if path.is_file():
            asset = public_assets[path.name]
            assert asset['size'] == path.stat().st_size
            assert asset.get('digest') == 'sha256:' + digest(path), 'GitHub asset checksum differs from tested distribution'
    # Independently fetch the small public manifest without GitHub credentials.
    manifest_asset = public_assets['server-image.json']['browser_download_url']
    with urllib.request.urlopen(manifest_asset, timeout=30) as response:
        assert hashlib.sha256(response.read()).hexdigest() == digest(DIST / 'server-image.json')
    print(published['url'])


def self_test():
    base = {'version': '0.1.0-alpha.1', 'publish': False, 'dockerhub_repository': '', 'dockerhub_visibility': ''}
    config(base)
    good = dict(base, publish=True, dockerhub_repository='example/aterminal-server', dockerhub_visibility='private')
    config(good)
    for change in ({'version': 'latest'}, {'version': '0.1.0'}, {'version': '0.1.0-alpha.01'},
                   {'publish': 'false'}, {'dockerhub_repository': ''}, {'dockerhub_repository': '../oops'},
                   {'dockerhub_visibility': ''}):
        try:
            config(dict(good, **change))
        except (AssertionError, KeyError):
            pass
        else:
            raise AssertionError('Invalid release configuration accepted: ' + repr(change))
    print('PASS: explicit prerelease version, boolean publication, repository and visibility validation')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('command', choices=['config', 'self-test', 'package', 'preflight', 'push', 'release'])
    parser.add_argument('--arch', choices=ARCHES)
    args = parser.parse_args()
    if args.command == 'self-test':
        self_test()
        return
    c = config()
    if args.command == 'config':
        enabled = c['publish'] and (os.environ.get('GITHUB_EVENT_NAME') == 'workflow_dispatch' or 'deploy/server-release.json' in run('git', 'diff', '--name-only', 'HEAD^', 'HEAD').stdout.splitlines())
        with open(os.environ.get('GITHUB_OUTPUT', os.devnull), 'a') as stream:
            stream.write(f'version={c["version"]}\npublish={str(enabled).lower()}\n')
        print(f'Prerelease {c["version"]}; publication enabled: {c["publish"]}')
    elif args.command == 'package':
        assert args.arch
        package(c, args.arch)
    else:
        assert os.environ.get('GITHUB_REF') == 'refs/heads/main', 'Only main can publish'
        globals()[args.command](c)


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        # Subprocess output is not dumped: future tools may include credentials.
        sys.exit(f'Command failed ({error.returncode}): {error.cmd[0]}; inspect the relevant Actions step')
