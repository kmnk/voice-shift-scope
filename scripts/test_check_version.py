"""Tests for check_version.py. Run with python3 -m unittest scripts/test_check_version.py."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_version as cv  # noqa: E402


def make_repo(root: Path, cargo: str, tauri: str) -> None:
    (root / "Cargo.toml").write_text(
        f'[workspace]\nmembers = ["crates/*"]\n\n[workspace.package]\nversion = "{cargo}"\nedition = "2024"\n\n'
        '[workspace.dependencies]\nthiserror = "2"\n', encoding="utf-8")
    conf = root / "crates" / "voice-tauri" / "tauri.conf.json"
    conf.parent.mkdir(parents=True)
    conf.write_text(json.dumps({"productName": "voice-shift-scope", "version": tauri}), encoding="utf-8")


class CheckVersionTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def test_reads_both_versions(self):
        make_repo(self.root, "0.1.0", "0.1.0")
        self.assertEqual(cv.read_versions(self.root), ("0.1.0", "0.1.0"))

    def test_accepts_matching_tag(self):
        self.assertEqual(cv.check("v0.1.0", "0.1.0", "0.1.0"), "0.1.0")

    def test_accepts_no_tag(self):
        self.assertEqual(cv.check("", "0.1.0", "0.1.0"), "0.1.0")

    def test_rejects_tag_that_differs(self):
        with self.assertRaises(cv.VersionError):
            cv.check("v9.9.9", "0.1.0", "0.1.0")

    def test_rejects_tauri_version_that_differs(self):
        make_repo(self.root, "0.1.0", "0.2.0")
        with self.assertRaises(cv.VersionError):
            cv.check("", *cv.read_versions(self.root))

    def test_rejects_tag_without_v(self):
        with self.assertRaises(cv.VersionError):
            cv.check("0.1.0", "0.1.0", "0.1.0")


if __name__ == "__main__":
    unittest.main()
