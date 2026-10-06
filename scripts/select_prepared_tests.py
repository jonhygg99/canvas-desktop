import unittest
from unittest.mock import patch
import select_prepared as prepared


class PreparedTests(unittest.TestCase):
    def test_ready_platform_is_reused_while_others_run(self):
        run = dict(id=7, head_sha="sha", head_branch="main", event="push", status="in_progress")
        replies = [{"workflow_runs": [run]}, {"artifacts": [dict(name="canvas-desktop-macos", expired=False)]}]
        with patch.object(prepared, "api", side_effect=replies):
            self.assertEqual(prepared.select("repo", "sha", "macos"), "7")

    def test_failed_platform_does_not_wait_for_other_platforms(self):
        run = dict(id=7, head_sha="sha", head_branch="main", event="push", status="in_progress")
        replies = [{"workflow_runs": [run]}, {"artifacts": []},
                   {"jobs": [dict(name="macos / build", status="completed", conclusion="failure")]}]
        with patch.object(prepared, "api", side_effect=replies), patch.object(prepared.time, "sleep") as sleep:
            self.assertEqual(prepared.select("repo", "sha", "macos"), "")
            sleep.assert_not_called()

    def test_expired_or_wrong_sha_artifacts_are_not_reused(self):
        run = dict(id=7, head_sha="sha", head_branch="main", event="push", status="completed")
        replies = [{"workflow_runs": [dict(run, id=8, head_sha="wrong"), run]},
                   {"artifacts": [dict(name="canvas-desktop-macos", expired=True)]}, {"jobs": []}]
        with patch.object(prepared, "api", side_effect=replies):
            self.assertEqual(prepared.select("repo", "sha", "macos"), "")
