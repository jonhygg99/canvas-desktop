import hashlib
from pathlib import Path
import tempfile
import unittest
from release_assets import checksums


class ReleaseAssetsTests(unittest.TestCase):
    def test_real_macos_name_normalized_without_changing_attested_bytes(self):
        # macOS exige los dos DMG (Intel + Apple Silicon) desde 33987ad: el
        # fixture los crea los dos, o el propio test fallaría en la CI.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            arm = "Canvas Desktop_0.7.0_aarch64.dmg"
            intel = "Canvas Desktop_0.7.0_x86_64.dmg"
            (root / arm).write_bytes(b"arm")
            (root / intel).write_bytes(b"intel")
            assets = checksums(root, "macos", "0.7.0")
            self.assertEqual(len(assets), 3)
            self.assertEqual(assets[0].name, arm.replace(" ", "."))
            self.assertEqual(assets[1].name, intel.replace(" ", "."))
            self.assertEqual(assets[0].read_bytes(), b"arm")
            self.assertEqual(assets[1].read_bytes(), b"intel")
            manifest = assets[2].read_text()
            self.assertIn(assets[0].name, manifest)
            self.assertIn(assets[1].name, manifest)
            # Idempotente: reintentar no renombra dos veces.
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
