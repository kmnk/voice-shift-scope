"""Tests for the two checks in third_party_licenses.py.

Run with python3 -m unittest scripts/test_third_party_licenses.py (does not call cargo or npm).
"""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import third_party_licenses as tpl  # noqa: E402


def component(files, license="MIT"):
    return tpl.Component("crate", "example", "1.0.0", license, "https://crates.io/crates/example/1.0.0", files)


class CheckTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.license = Path(self.dir.name) / "LICENSE"
        self.license.write_text("MIT License\n\nCopyright (c) 2026 example\n", encoding="utf-8")

    def tearDown(self):
        self.dir.cleanup()

    def test_accepts_component_with_file_and_accepted_license(self):
        tpl.check([component([self.license], "MIT OR Apache-2.0")])

    def test_rejects_component_without_license_file(self):
        with self.assertRaises(tpl.LicenseError) as e:
            tpl.check([component([])])
        self.assertIn("example 1.0.0", str(e.exception))

    def test_rejects_license_not_accepted(self):
        with self.assertRaises(tpl.LicenseError) as e:
            tpl.check([component([self.license], "MIT AND GPL-3.0-only")])
        self.assertIn("GPL-3.0-only", str(e.exception))

    def test_finds_license_files_regardless_of_case_and_separator(self):
        for name in ["license-mit", "LICENSE_MIT", "COPYING", "NOTICE.txt", "README.md"]:
            (Path(self.dir.name) / name).write_text("x", encoding="utf-8")
        names = {p.name for p in tpl.license_files(Path(self.dir.name))}
        self.assertEqual(names, {"LICENSE", "license-mit", "LICENSE_MIT", "COPYING", "NOTICE.txt"})

    def test_license_names_reads_old_slash_form(self):
        self.assertEqual(tpl.license_names("MIT/Apache-2.0"), {"MIT", "Apache-2.0"})
        self.assertEqual(tpl.license_names("Apache-2.0 WITH LLVM-exception"), {"Apache-2.0", "LLVM-exception"})


if __name__ == "__main__":
    unittest.main()
