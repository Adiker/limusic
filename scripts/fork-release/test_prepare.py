import json
import os
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import prepare


class PreparationTests(unittest.TestCase):
    def test_rejects_existing_tag_at_another_commit_before_release(self):
        sha = 'a' * 40
        upstream = 'b' * 40
        calls = []
        def result(stdout='', code=0):
            return SimpleNamespace(stdout=stdout, stderr='', returncode=code)
        def fake(*args, **kwargs):
            calls.append(args)
            if args[:2] == ('git', 'rev-parse'):
                return result(upstream + '\n')
            if args[:2] == ('gh', 'api') and args[2].endswith('/pulls'):
                return result(json.dumps([{'merged_at':'today', 'base':{'ref':'master'},
                    'merge_commit_sha':sha, 'head':{'ref':'codex/sync-upstream-rc'}}]))
            if args[:2] == ('git', 'show'):
                return result('{"version":"1.4.0-rc.10"}')
            if args[:2] == ('gh', 'api'):
                return result('{"draft":false,"prerelease":true}')
            if args[:2] == ('git', 'ls-remote'):
                return result('c' * 40 + '\trefs/tags/v1.4.0-rc.10\n')
            return result()
        env = {'GITHUB_REPOSITORY':'owner/fork', 'DEFAULT_BRANCH':'master', 'EVENT_SHA':sha}
        with patch.dict(os.environ, env, clear=True), patch.object(prepare, 'run', side_effect=fake):
            with self.assertRaisesRegex(RuntimeError, 'Refusing to replace'):
                prepare.main()
        self.assertFalse(any(args[:2] == ('git', 'push') for args in calls))
        self.assertFalse(any(args[:3] == ('gh', 'release', 'create') for args in calls))


if __name__ == '__main__':
    unittest.main()
