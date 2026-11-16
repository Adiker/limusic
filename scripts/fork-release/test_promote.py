import os
import unittest
from unittest.mock import patch

from promote import semver, validate_assets


class ReleaseValidationTests(unittest.TestCase):
    def setUp(self):
        self.tag = 'v1.4.0-rc.10'
        self.version = self.tag[1:]
        v = self.version
        self.files = [f'limusic_{v}_amd64.AppImage', f'limusic_{v}_amd64.AppImage.sig',
                      f'limusic_{v}_x64-setup.exe', f'limusic_{v}_x64-setup.exe.sig',
                      f'limusic_{v}_aarch64.dmg', f'limusic_{v}_aarch64.app.tar.gz',
                      f'limusic_{v}_aarch64.app.tar.gz.sig', 'latest.json',
                      'limusic-1.4.0rc.10-1-x86_64.pkg.tar.zst', 'limusic-1.4.0~rc.10-1.x86_64.rpm']
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

    def test_semver_never_moves_beta_backwards(self):
        self.assertLess(semver('1.4.0-rc.9'), semver('1.4.0-rc.10'))
        self.assertLess(semver('1.4.0-rc.10'), semver('1.4.0'))
        self.assertLess(semver('1.3.99'), semver('1.4.0-rc.1'))


if __name__ == '__main__':
    unittest.main()
