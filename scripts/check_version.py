#!/usr/bin/env python3
"""Checks that the versions in Cargo.toml and tauri.conf.json match, and prints the version if they do.

Usage: python3 scripts/check_version.py [<tag>]
Given a tag (such as v0.1.0), also compares with the tag minus its v. An empty string or no tag skips the tag comparison.
If they match, prints the version to standard output and exits with 0; otherwise prints the reason and exits with 1.
Run in the first step of release.yml.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class VersionError(Exception):
    pass


def read_versions(root: Path) -> tuple[str, str]:
    cargo = (root / "Cargo.toml").read_text(encoding="utf-8")
    section = re.search(r"^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", cargo, re.MULTILINE | re.DOTALL)
    match = section and re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.MULTILINE)
    if not match:
        raise VersionError("no version in [workspace.package] of Cargo.toml")
    tauri = json.loads((root / "crates" / "voice-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
    if "version" not in tauri:
        raise VersionError("no version in tauri.conf.json")
    return match.group(1), tauri["version"]


def check(tag: str, cargo: str, tauri: str) -> str:
    if cargo != tauri:
        raise VersionError(f"Cargo.toml has {cargo} but tauri.conf.json has {tauri}")
    if tag:
        if not tag.startswith("v"):
            raise VersionError(f"tag {tag} does not start with v")
        if tag[1:] != cargo:
            raise VersionError(f"tag {tag} differs from {cargo} in Cargo.toml and tauri.conf.json")
    return cargo


def main(argv: list[str]) -> int:
    tag = argv[1] if len(argv) > 1 else ""
    try:
        print(check(tag, *read_versions(ROOT)))
    except VersionError as e:
        print(e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
