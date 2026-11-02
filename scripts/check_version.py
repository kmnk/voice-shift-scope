#!/usr/bin/env python3
"""Cargo.toml と tauri.conf.json の版が揃っているかを確かめ、揃っていれば版を出す。

使い方: python3 scripts/check_version.py [<tag>]
tag（例 v0.1.0）を渡すと、tag から v を除いたものとも比べる。空の文字列か省略なら tag とは比べない。
揃っていれば版を標準出力に出して終了コード 0、揃っていなければ理由を出して終了コード 1。
release.yml の最初の step で動かす。
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
        raise VersionError("Cargo.toml の [workspace.package] に version が無い")
    tauri = json.loads((root / "crates" / "voice-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
    if "version" not in tauri:
        raise VersionError("tauri.conf.json に version が無い")
    return match.group(1), tauri["version"]


def check(tag: str, cargo: str, tauri: str) -> str:
    if cargo != tauri:
        raise VersionError(f"Cargo.toml は {cargo}、tauri.conf.json は {tauri} で違う")
    if tag:
        if not tag.startswith("v"):
            raise VersionError(f"tag {tag} が v で始まらない")
        if tag[1:] != cargo:
            raise VersionError(f"tag {tag} と、Cargo.toml・tauri.conf.json の {cargo} が違う")
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
