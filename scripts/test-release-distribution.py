#!/usr/bin/env python3
"""Offline checks for the cross-platform publisher's fail-closed boundary."""
import importlib.util
import json
import os
import pathlib
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('publisher', pathlib.Path(__file__).with_name('release-server.py'))
PUBLISHER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PUBLISHER)
SHA = 'a' * 40
VERSION = '0.1.0-alpha.2'
REPOSITORY = 'dqh147258/aTerminal'
TARGETS = ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-unknown-linux-gnu', 'android']


class DistributionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        self.dist = self.root / 'dist'
        self.dist.mkdir()
        for name, value in [('ROOT', self.root), ('DIST', self.dist), ('TARGET', self.root / 'target.json')]:
            context = patch.object(PUBLISHER, name, value)
            context.start()
            self.addCleanup(context.stop)
        env = patch.dict(os.environ, {'GITHUB_SHA': SHA, 'GITHUB_REPOSITORY': REPOSITORY, 'GITHUB_REF': 'refs/heads/main'})
        env.start()
        self.addCleanup(env.stop)
        self.config = {'publish': True, 'version': VERSION, 'dockerhub_repository': 'yixifeng/aterminal', 'dockerhub_visibility': 'public'}
        self.clients = {'schema_version': 1, 'source_commit': SHA, 'version': VERSION, 'repository': REPOSITORY,
                        'platforms': list(TARGETS), 'assets': []}
        for index, target in enumerate(TARGETS):
            name = f'client-{index}.apk' if target == 'android' else f'client-{index}.zip'
            file = self.dist / name
            file.write_bytes(('verified-' + target).encode())
            self.clients['assets'].append({'name': name, 'size': file.stat().st_size, 'sha256': PUBLISHER.digest(file)})
        (self.dist / 'CLIENT-SHA256SUMS').write_text('fixture\n')
        for arch in PUBLISHER.ARCHES:
            base = f'aterminal-server-{VERSION}-linux-{arch}'
            for suffix in ('.json', '.tar.gz', '.tar.gz.sha256'):
                (self.dist / (base + suffix)).write_text('fixture')
        self.write_manifest()

    def write_manifest(self):
        (self.dist / 'client-assets.json').write_text(json.dumps(self.clients))

    def verify(self):
        # Archive/signature/provenance verification is delegated to the independently
        # tested collector; here we exercise the publisher's second validation layer.
        with patch.object(PUBLISHER, 'run') as command:
            clients = PUBLISHER.client_metadata(self.config)
            args = command.call_args.args
            self.assertIn('--verify-only', args)
            self.assertIn(SHA, args)
            self.assertIn(REPOSITORY, args)
            self.assertIn(VERSION, args)
        PUBLISHER.validate_distribution(self.config, clients)
        return clients

    def test_complete_distribution(self):
        self.verify()
        (self.dist / 'server-image.json').write_text('{}')
        (self.dist / 'SHA256SUMS').write_text('generated later')
        self.verify()

    def test_wrong_commit(self):
        self.clients['source_commit'] = 'b' * 40
        self.write_manifest()
        with self.assertRaises(AssertionError): self.verify()

    def test_wrong_version(self):
        self.clients['version'] = '0.1.0-alpha.1'
        self.write_manifest()
        with self.assertRaises(AssertionError): self.verify()

    def test_wrong_repository(self):
        self.clients['repository'] = 'someone/else'
        self.write_manifest()
        with self.assertRaises(AssertionError): self.verify()

    def test_missing_desktop_platform(self):
        self.clients['platforms'].remove('x86_64-pc-windows-msvc')
        self.write_manifest()
        with self.assertRaises(AssertionError): self.verify()

    def test_missing_direct_apk(self):
        item = self.clients['assets'][-1]
        (self.dist / item['name']).unlink()
        self.clients['assets'].pop()
        self.write_manifest()
        with self.assertRaises(AssertionError): self.verify()

    def test_mutated_asset(self):
        (self.dist / self.clients['assets'][0]['name']).write_bytes(b'changed')
        with self.assertRaises(AssertionError): self.verify()

    def test_unexpected_file(self):
        (self.dist / 'unrequested-file.txt').write_text('must not be published')
        with self.assertRaises(AssertionError): self.verify()

    def test_missing_server_archive(self):
        (self.dist / f'aterminal-server-{VERSION}-linux-amd64.tar.gz').unlink()
        with self.assertRaises(AssertionError): self.verify()

    def test_symlink_rejected(self):
        target = self.dist / 'CLIENT-SHA256SUMS'
        target.unlink()
        outside = self.root / 'outside'
        outside.write_text('fixture\n')
        target.symlink_to(outside)
        with self.assertRaises(AssertionError): self.verify()

    def test_bad_clients_stop_before_remote_calls(self):
        self.clients['source_commit'] = 'b' * 40
        self.write_manifest()
        with patch.object(PUBLISHER, 'metadata', return_value={}), patch.object(PUBLISHER, 'run'), patch.object(PUBLISHER, 'api') as api:
            with self.assertRaises(AssertionError): PUBLISHER.preflight(self.config)
            api.assert_not_called()

    def test_offline_verifier_failure_is_not_ignored(self):
        with patch.object(PUBLISHER, 'run', side_effect=RuntimeError('collector rejected inputs')):
            with self.assertRaises(RuntimeError): PUBLISHER.client_metadata(self.config)


if __name__ == '__main__':
    unittest.main()
