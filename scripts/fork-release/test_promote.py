import os
import base64
import hashlib
import tempfile
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from nacl.signing import SigningKey

from promote import main, semver, tag_commit, validate_assets, verify_signature


class ReleaseValidationTests(unittest.TestCase):
    def setUp(self):
        self.tag = 'v1.4.0-rc.10'
        self.version = self.tag[1:]
        v = self.version
        self.files = [f'limusic_{v}_amd64.AppImage', f'limusic_{v}_amd64.AppImage.sig',
                      f'limusic_{v}_x64-setup.exe', f'limusic_{v}_x64-setup.exe.sig',
                      f'limusic_{v}_aarch64.dmg', f'limusic_{v}_aarch64.app.tar.gz',
                      f'limusic_{v}_aarch64.app.tar.gz.sig', 'latest.json',
                      'limusic-1.4.0rc.10-1-x86_64.pkg.tar.zst', 'limusic-1.4.0-rc.10-1.x86_64.rpm']
        self.assets = [{'name': n, 'state': 'uploaded', 'size': 1} for n in self.files]
        names = {'linux-x86_64': f'limusic_{v}_amd64.AppImage',
                 'windows-x86_64': f'limusic_{v}_x64-setup.exe',
                 'darwin-aarch64': f'limusic_{v}_aarch64.app.tar.gz'}
        self.manifest = {'version': v, 'platforms': {p: {'url': f'https://github.com/owner/fork/releases/download/{self.tag}/{n}', 'signature': 'sig'} for p, n in names.items()}}
        env = patch.dict(os.environ, {'GITHUB_REPOSITORY': 'owner/fork'})
        env.start()
        self.addCleanup(env.stop)

    def test_complete_rc(self):
        validate_assets(self.tag, self.assets, self.manifest)

    def test_incomplete_packages_block_promotion(self):
        for name in self.files:
            with self.subTest(name=name), self.assertRaises(ValueError):
                validate_assets(self.tag, [a for a in self.assets if a['name'] != name], self.manifest)

    def test_wrong_url_and_wrong_version_block_promotion(self):
        self.manifest['platforms']['linux-x86_64']['url'] = 'https://example.test/wrong'
        with self.assertRaises(ValueError):
            validate_assets(self.tag, self.assets, self.manifest)
        self.manifest['version'] = '1.4.0-rc.9'
        with self.assertRaises(ValueError):
            validate_assets(self.tag, self.assets, self.manifest)

    def test_rpm_from_another_rc_is_rejected(self):
        assets = [a for a in self.assets if not a['name'].endswith('.rpm')]
        assets.append({'name': 'limusic-1.4.0-rc.9-1.x86_64.rpm', 'state': 'uploaded', 'size': 1})
        with self.assertRaisesRegex(ValueError, 'RPM'):
            validate_assets(self.tag, assets, self.manifest)

    def test_semver_never_moves_beta_backwards(self):
        self.assertLess(semver('1.4.0-rc.9'), semver('1.4.0-rc.10'))
        self.assertLess(semver('1.4.0-rc.10'), semver('1.4.0'))
        self.assertLess(semver('1.3.99'), semver('1.4.0-rc.1'))

    def test_resolves_annotated_tag_to_commit(self):
        with patch('promote.run', side_effect=[
            SimpleNamespace(stdout=json.dumps({'object': {'type': 'tag', 'sha': 'tag-object'}})),
            SimpleNamespace(stdout=json.dumps({'object': {'type': 'commit', 'sha': 'commit-sha'}}))
        ]):
            self.assertEqual(tag_commit('owner/repo', 'v1.4.0'), 'commit-sha')

    def test_moved_tag_blocks_promotion_before_any_release_operation(self):
        with patch.dict(os.environ, {'RELEASE_TAG': 'v1.4.0', 'RELEASE_SHA': 'expected'}), \
             patch('promote.tag_commit', return_value='different'), patch('promote.run') as run:
            with self.assertRaisesRegex(RuntimeError, 'no longer points'):
                main()
            run.assert_not_called()

    def test_cryptographic_verification_and_tampering(self):
        private = SigningKey.generate()
        key_id = b'12345678'
        key = base64.b64encode(b'Ed' + key_id + bytes(private.verify_key)).decode()
        public = base64.b64encode(f'untrusted comment: test\n{key}\n'.encode()).decode()
        data = b'test artifact'
        signed = private.sign(hashlib.blake2b(data, digest_size=64).digest()).signature
        comment = 'timestamp:1'
        global_sig = private.sign(signed + comment.encode()).signature
        raw = (f'untrusted comment: signature\n'
               f'{base64.b64encode(b"ED" + key_id + signed).decode()}\n'
               f'trusted comment: {comment}\n{base64.b64encode(global_sig).decode()}\n')
        signature = base64.b64encode(raw.encode()).decode()
        with tempfile.TemporaryDirectory() as folder:
            asset = Path(folder) / 'artifact'
            asset.write_bytes(data)
            verify_signature(public, asset, signature)
            asset.write_bytes(b'tampered')
            with self.assertRaisesRegex(ValueError, 'cryptographic verification'):
                verify_signature(public, asset, signature)


if __name__ == '__main__':
    unittest.main()
