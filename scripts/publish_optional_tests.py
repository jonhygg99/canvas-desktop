import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import publish_optional as publish


class OptionalPublishTests(unittest.TestCase):
    def test_existing_identical_asset_is_skipped_and_missing_is_uploaded(self):
        with tempfile.TemporaryDirectory() as temp:
            asset = Path(temp) / "package.deb"
            manifest = Path(temp) / "SHA256SUMS-linux.txt"
            asset.write_bytes(b"package")
            manifest.write_bytes(b"hashes")
            calls = []

            def run(args, **kwargs):
                calls.append(args)
                if args[2] == "download":
                    (Path(args[-1]) / asset.name).write_bytes(b"package")

            with patch.object(publish.subprocess, "check_output", return_value=json.dumps({"assets": [{"name": asset.name}]})), patch.object(publish.subprocess, "run", side_effect=run):
                publish.upload_missing("v0.7.0", "owner/repo", [asset, manifest])
            uploads = [c for c in calls if c[2] == "upload"]
            self.assertEqual(len(uploads), 1)
            self.assertIn(str(manifest), uploads[0])
            self.assertNotIn("--clobber", uploads[0])

    def test_existing_different_asset_blocks_overwrite(self):
        with tempfile.TemporaryDirectory() as temp:
            asset = Path(temp) / "package.deb"
            asset.write_bytes(b"new")

            def run(args, **kwargs):
                self.assertEqual(args[2], "download")
                (Path(args[-1]) / asset.name).write_bytes(b"old")

            with patch.object(publish.subprocess, "check_output", return_value=json.dumps({"assets": [{"name": asset.name}]})), patch.object(publish.subprocess, "run", side_effect=run):
                with self.assertRaisesRegex(ValueError, "No se sobrescribe"):
                    publish.upload_missing("v0.7.0", "owner/repo", [asset])


if __name__ == "__main__":
    unittest.main()
