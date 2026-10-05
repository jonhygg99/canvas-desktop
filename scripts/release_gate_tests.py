import unittest
import os
from unittest.mock import patch
import release_gate as gate


class ReleaseGateTests(unittest.TestCase):
    def test_tag_must_match_version_and_be_stable(self):
        gate.validate_tag("v0.7.0", "0.7.0")
        for tag in ["v0.6.0", "v0.7.0-beta", "main", "v0.7.0\n"]:
            with self.assertRaises(ValueError):
                gate.validate_tag(tag, "0.7.0")

    def test_only_latest_push_on_main_for_exact_sha_counts(self):
        base = dict(head_sha="abc", head_branch="main", event="push")
        runs = [dict(base, id=1), dict(base, id=2),
                dict(base, id=3, head_branch="feature"),
                dict(base, id=4, head_sha="other"),
                dict(base, id=5, event="pull_request")]
        self.assertEqual(gate.latest_main_run(runs, "abc")["id"], 2)

    def test_failed_ci_blocks_release(self):
        run = dict(id=1, head_sha="abc", head_branch="main", event="push",
                   status="completed", conclusion="failure", html_url="ci-url")
        with patch.object(gate, "api", return_value={"workflow_runs": [run]}):
            with self.assertRaisesRegex(ValueError, "CI no aprobada"):
                gate.wait_for_ci("owner/repo", "abc")

    def test_manual_workflow_on_main_cannot_publish_a_different_tag_commit(self):
        environment = dict(RELEASE_TAG="v0.7.0", GITHUB_SHA="different")
        with patch.dict(os.environ, environment), patch.object(gate.Path, "read_text", return_value='[workspace.package]\nversion="0.7.0"'), patch.object(gate, "command", side_effect=["abc", "", "abc"]), patch.object(gate.subprocess, "run"), patch.object(gate, "api") as api:
            with self.assertRaisesRegex(ValueError, "sobre el tag"):
                gate.main()
            api.assert_not_called()

    def test_pending_ci_waits_then_accepts_success(self):
        run = dict(id=1, head_sha="abc", head_branch="main", event="push")
        replies = [{"workflow_runs": [dict(run, status="in_progress")]},
                   {"workflow_runs": [dict(run, status="completed", conclusion="success")]}]
        with patch.object(gate, "api", side_effect=replies), patch.object(gate.time, "sleep"):
            self.assertEqual(gate.wait_for_ci("owner/repo", "abc"), 1)

    def test_prepared_artifact_must_be_successful_exact_and_unexpired(self):
        run = dict(id=7, head_sha="abc", head_branch="main", event="push", conclusion="success")
        for expired, expected in [(True, ""), (False, "7")]:
            replies = [{"workflow_runs": [dict(run, id=8, head_sha="other"), run]},
                       {"artifacts": [dict(name="canvas-desktop-windows-x64", expired=expired)]}]
            with patch.object(gate, "api", side_effect=replies):
                self.assertEqual(gate.prepared_run("owner/repo", "abc"), expected)


if __name__ == "__main__":
    unittest.main()
