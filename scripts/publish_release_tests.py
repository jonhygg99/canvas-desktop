import json
import os
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch
import publish_release as publish


class PublishReleaseTests(unittest.TestCase):
    def execute(self, existing, fail_attestation=False, fail_upload=False, dry_run=False):
        calls = []

        def run(args, **kwargs):
            calls.append(args)
            if args[1:3] == ["attestation", "verify"] and fail_attestation:
                raise subprocess.CalledProcessError(1, args)
            if args[1:3] == ["release", "upload"] and fail_upload:
                raise subprocess.CalledProcessError(1, args)
            if args[1:3] == ["release", "view"]:
                return subprocess.CompletedProcess(args, 1 if existing is None else 0,
                                                   json.dumps({"isDraft": existing}))
            return subprocess.CompletedProcess(args, 0)

        environment = dict(RELEASE_TAG="v0.7.0", GITHUB_REPOSITORY="owner/repo", RELEASE_SHA="abc",
                           DRY_RUN="true" if dry_run else "false")
        with patch.dict(os.environ, environment), patch.object(publish, "checksums", return_value=[Path("setup.exe"), Path("SHA256SUMS.txt")]), patch.object(publish.subprocess, "run", side_effect=run):
            try:
                publish.main()
            except (ValueError, subprocess.CalledProcessError):
                return calls, False
        return calls, True

    def test_dry_run_checks_provenance_without_any_release_commands(self):
        calls, ok = self.execute(None, dry_run=True)
        self.assertTrue(ok)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][1:3], ["attestation", "verify"])

    def test_failed_provenance_never_creates_or_promotes_release(self):
        calls, ok = self.execute(None, fail_attestation=True)
        self.assertFalse(ok)
        self.assertEqual(len(calls), 1)
        self.assertIn("--source-digest", calls[0])
        self.assertIn("--signer-workflow", calls[0])

    def test_published_release_is_never_overwritten(self):
        calls, ok = self.execute(False)
        self.assertFalse(ok)
        self.assertFalse(any(c[1:3] in (["release", "upload"], ["release", "edit"]) for c in calls))

    def test_new_release_is_created_as_draft_before_promotion(self):
        calls, ok = self.execute(None)
        self.assertTrue(ok)
        self.assertIn("--draft", calls[-2])
        self.assertIn("--verify-tag", calls[-2])
        self.assertIn("--notes", calls[-2])
        self.assertIn("--generate-notes", calls[-2])
        self.assertIn("--draft=false", calls[-1])

    def test_existing_draft_can_resume(self):
        calls, ok = self.execute(True)
        self.assertTrue(ok)
        self.assertEqual(calls[-2][1:3], ["release", "upload"])

    def test_failed_upload_never_promotes_the_draft(self):
        calls, ok = self.execute(True, fail_upload=True)
        self.assertFalse(ok)
        self.assertFalse(any(c[1:3] == ["release", "edit"] for c in calls))


if __name__ == "__main__":
    unittest.main()
