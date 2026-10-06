import hashlib
from pathlib import Path
import tempfile
import unittest
from release_assets import checksums


class ReleaseAssetsTests(unittest.TestCase):
    def test_real_macos_name_normalized_without_changing_attested_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            name = "Canvas Desktop_0.7.0_aarch64.dmg"
            (root / name).write_bytes(b"dmg")
            assets = checksums(root, "macos", "0.7.0")
            self.assertEqual(assets[0].name, name.replace(" ", "."))
            self.assertEqual(assets[0].read_bytes(), b"dmg")
            self.assertIn(assets[0].name, assets[1].read_text())
            self.assertEqual(checksums(root, "macos", "0.7.0")[0].name, assets[0].name)
            with self.assertRaises(ValueError):
                checksums(root, "macos", "0.7.1")

    def test_exact_windows_asset_and_digest(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            name = "canvas-desktop_0.7.0_x64-setup.exe"
            (root / name).write_bytes(b"installer")
            assets = checksums(root, "windows-x64", "0.7.0")
            self.assertEqual(len(assets), 2)
            self.assertEqual(assets[1].read_text(), f"{hashlib.sha256(b'installer').hexdigest()}  {name}\n")

    def test_missing_wrong_architecture_empty_and_wrong_version_fail(self):
        for name, content in [(None, b""), ("canvas_0.7.0_arm64-setup.exe", b"x"),
                              ("canvas_0.6.0_x64-setup.exe", b"x"),
                              ("canvas_0.7.0_x64-setup.exe", b"")]:
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                if name:
                    (root / name).write_bytes(content)
                with self.assertRaises(ValueError):
                    checksums(root, "windows-x64", "0.7.0")


if __name__ == "__main__":
    unittest.main()
