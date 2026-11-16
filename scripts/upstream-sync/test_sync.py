import unittest
from sync import latest_release, version


class VersionTests(unittest.TestCase):
    def test_order(self):
        tags = ['v1.4.0-rc.9', 'v1.4.0-rc.10', 'v1.4.0', 'v1.3.10']
        self.assertEqual(sorted(tags, key=version), [tags[3], tags[0], tags[1], tags[2]])

    def test_only_supported_published_releases(self):
        releases = [{'tag_name': 'v1.4.0-rc.10', 'draft': False},
                    {'tag_name': 'v1.4.0', 'draft': True},
                    {'tag_name': 'beta', 'draft': False},
                    {'tag_name': 'v1.3.0', 'draft': False}]
        self.assertEqual(latest_release(releases)['tag_name'], 'v1.4.0-rc.10')
        self.assertIsNone(latest_release([{'tag_name': 'beta'}]))

class FlowTests(unittest.TestCase):
    def setUp(self):
        import os
        from unittest.mock import patch
        self.env = patch.dict(os.environ, {'GITHUB_REPOSITORY': 'owner/fork', 'DEFAULT_BRANCH': 'master'})
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_conflict_does_not_push(self):
        from unittest.mock import patch
        from types import SimpleNamespace
        import sync
        calls = []

        def result(stdout='', returncode=0):
            return SimpleNamespace(stdout=stdout, stderr='', returncode=returncode)

        def fake_gh(*args):
            if args[0] == 'api':
                return '[[{"tag_name":"v1.4.0-rc.10","html_url":"https://example.test/release"}]]'
            if args[:2] == ('pr', 'list'):
                return '[]'
            raise AssertionError(args)

        def fake_git(*args, **kwargs):
            calls.append(args)
            if args[:2] == ('rev-parse', 'FETCH_HEAD^{commit}'):
                return result('upstream-sha\n')
            if args[:2] == ('rev-parse', 'origin/master'):
                return result('base-sha\n')
            if args[:2] == ('merge-base', '--is-ancestor'):
                return result(returncode=1)
            if args[0] == 'merge' and '--no-ff' in args:
                return result(returncode=1)
            if args[:2] == ('diff', '--name-only'):
                return result('src/conflict.rs\n')
            return result()

        with patch.object(sync, 'gh', side_effect=fake_gh), patch.object(sync, 'git', side_effect=fake_git), patch.object(sync, 'notice') as issue:
            with self.assertRaisesRegex(RuntimeError, 'merge failed'):
                sync.main()
        self.assertIn('src/conflict.rs', issue.call_args.args[1])
        self.assertFalse(any(c[0] == 'push' for c in calls))

    def test_manual_changes_are_not_overwritten(self):
        from unittest.mock import patch
        from types import SimpleNamespace
        import sync
        calls = []
        pr = {'number': 42, 'headRefName': sync.BRANCH, 'headRefOid': 'manual',
              'body': '<!-- upstream-sync {"head":"old"} -->'}

        def fake_gh(*args):
            if args[0] == 'api':
                return '[[{"tag_name":"v1.4.0-rc.10","html_url":"https://example.test/release"}]]'
            if args[:2] == ('pr', 'list'):
                import json
                return json.dumps([pr])
            raise AssertionError(args)

        def fake_git(*args, **kwargs):
            calls.append(args)
            output = 'upstream-sha\n' if args[0] == 'rev-parse' else ''
            return SimpleNamespace(stdout=output, stderr='', returncode=1 if args[0] == 'merge-base' else 0)

        with patch.object(sync, 'gh', side_effect=fake_gh), patch.object(sync, 'git', side_effect=fake_git), patch.object(sync, 'notice') as issue:
            sync.main()
        issue.assert_called_once()
        self.assertFalse(any(c[0] == 'push' for c in calls))


if __name__ == '__main__':
    unittest.main()
