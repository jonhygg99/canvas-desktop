import unittest
from pathlib import Path
from unittest.mock import patch
import ci_reuse


class CiReuseTests(unittest.TestCase):
    def test_only_identical_verified_tree_is_reused(self):
        pull = dict(merged_at="now", merge_commit_sha="merge", base=dict(ref="main"),
                    head=dict(sha="head", repo=dict(full_name="owner/repo")))
        for tree, conclusion, expired, expected in [
            ("tree", "success", False, "7"), ("different", "success", False, ""),
            ("tree", "failure", False, ""), ("tree", "success", True, "")]:
            replies = [[pull], {"workflow_runs": [dict(id=7, conclusion=conclusion, event="pull_request")]},
                       {"artifacts": [dict(name="verified-ci-tree", expired=expired)]}]
            def download(args, **kwargs):
                (Path(args[-1]) / "tree.txt").write_text(tree)
            with patch.object(ci_reuse, "api", side_effect=replies), patch.object(ci_reuse.subprocess, "run", side_effect=download):
                self.assertEqual(ci_reuse.verified_run("owner/repo", "merge", "tree"), expected)

    def test_unmerged_wrong_commit_wrong_base_and_fork_are_rejected(self):
        base = dict(merged_at="now", merge_commit_sha="merge", base=dict(ref="main"),
                    head=dict(sha="head", repo=dict(full_name="owner/repo")))
        for change in [dict(merged_at=None), dict(merge_commit_sha="other"),
                       dict(base=dict(ref="other")), dict(head=dict(sha="head", repo=dict(full_name="fork/repo")))]:
            with patch.object(ci_reuse, "api", return_value=[dict(base, **change)]) as api:
                self.assertEqual(ci_reuse.verified_run("owner/repo", "merge", "tree"), "")
                self.assertEqual(api.call_count, 1)
