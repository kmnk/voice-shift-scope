#!/usr/bin/env python3
"""配布物に入れる THIRD-PARTY-LICENSES.txt を作る。

exe に入る第三者の部品（Rust の crate、画面の npm の package、third_party/、WebView2 の loader）が
同梱するライセンスのファイルを、中身を変えずに 1 つのテキストに写す。

使い方: python3 scripts/third_party_licenses.py --out <出力のパス>

cargo と npm を呼ぶ。sandbox の中では CARGO_HOME を付けて動かす（AGENTS.md）。frontend/node_modules が要る。
次のどちらかに当たると、終了コード 1 で止まる。
- ライセンスのファイルが 1 つも見つからない部品がある（packaging/licenses/crates/ に置き足す）
- 受け入れると決めていないライセンスの名前が、部品のライセンスの式に出る（ACCEPTED を見直す）
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
BINARIES = ["voice-tauri", "voice-cli"]
FALLBACK_DIR = ROOT / "packaging" / "licenses" / "crates"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice)", re.IGNORECASE)

# 受け入れるライセンスの SPDX の名前。今の依存にあるものだけを並べる。
# 足すときは、そのライセンスの条件（表示・ソースの提供など）を配布物で満たせるかを先に確かめる。
ACCEPTED = {
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "ISC",
    "LLVM-exception",
    "MIT",
    "MIT-0",
    "MPL-2.0",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
}


@dataclass
class Component:
    kind: str  # "crate"・"npm"・"bundled"
    name: str
    version: str
    license: str
    source: str
    files: list[Path] = field(default_factory=list)


class LicenseError(Exception):
    pass


def license_names(expr: str) -> set[str]:
    """SPDX の式から、ライセンスと例外の名前を取り出す。古い書き方の "/" も OR とみなす。"""
    tokens = re.findall(r"[A-Za-z0-9.+\-]+", expr.replace("/", " OR "))
    return {t for t in tokens if t not in ("AND", "OR", "WITH")}


def license_files(directory: Path) -> list[Path]:
    if not directory.is_dir():
        return []
    return sorted(p for p in directory.iterdir() if p.is_file() and LICENSE_FILE.match(p.name))


def check(components: list[Component]) -> None:
    """2 つの検査。どちらかに当たれば LicenseError。"""
    problems = []
    for c in components:
        if not c.files:
            problems.append(f"ライセンスのファイルが無い: {c.kind} {c.name} {c.version}"
                            f"（crate なら {FALLBACK_DIR.relative_to(ROOT)}/{c.name}-{c.version}/ に置く）")
        unknown = license_names(c.license) - ACCEPTED
        if unknown:
            problems.append(f"受け入れると決めていないライセンス: {c.kind} {c.name} {c.version}: "
                            f"{', '.join(sorted(unknown))}（式は {c.license!r}）")
    if problems:
        raise LicenseError("\n".join(problems))


def run(args: list[str], cwd: Path) -> str:
    exe = shutil.which(args[0])
    if exe is None:
        raise LicenseError(f"{args[0]} が見つからない")
    return subprocess.run([exe, *args[1:]], cwd=cwd, check=True, capture_output=True,
                          text=True, encoding="utf-8").stdout


def crates() -> list[Component]:
    tree = run(["cargo", "tree", "--locked", "-e", "normal", "--target", TARGET,
                *sum((["-p", b] for b in BINARIES), []), "--prefix", "none", "-f", "{p}"], ROOT)
    used = set()
    for line in tree.splitlines():
        parts = line.split()
        if len(parts) >= 2:
            used.add((parts[0], parts[1].removeprefix("v")))
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1"], ROOT))
    components = []
    for p in metadata["packages"]:
        if (p["name"], p["version"]) not in used or p["source"] is None:
            continue  # 使われないもの、自作の crate
        files = license_files(Path(p["manifest_path"]).parent)
        if not files:
            files = license_files(FALLBACK_DIR / f"{p['name']}-{p['version']}")
        components.append(Component("crate", p["name"], p["version"], p.get("license") or "",
                                    f"https://crates.io/crates/{p['name']}/{p['version']}", files))
    return components


def npm_packages() -> list[Component]:
    tree = json.loads(run(["npm", "ls", "--omit=dev", "--all", "--json", "--long"], ROOT / "frontend"))
    found: dict[tuple[str, str], Component] = {}

    def walk(deps: dict | None) -> None:
        for name, info in (deps or {}).items():
            key = (name, info["version"])
            if key not in found:
                found[key] = Component("npm", name, info["version"], info.get("license") or "",
                                       f"https://www.npmjs.com/package/{name}/v/{info['version']}",
                                       license_files(Path(info["path"])))
            walk(info.get("dependencies"))

    walk(tree.get("dependencies"))
    return list(found.values())


def bundled() -> list[Component]:
    return [
        Component("bundled", "signalsmith-stretch", "1.4.0", "MIT",
                  "https://github.com/Signalsmith-Audio/signalsmith-stretch/tree/1.4.0",
                  license_files(ROOT / "third_party" / "signalsmith-stretch")),
        Component("bundled", "signalsmith-linear", "0.6.4", "MIT",
                  "https://github.com/Signalsmith-Audio/linear/tree/0.6.4",
                  license_files(ROOT / "third_party" / "signalsmith-linear")),
        Component("bundled", "Microsoft WebView2 loader（webview2-com-sys が組み込む）", "1.0.3650.58",
                  "BSD-3-Clause", "https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3650.58",
                  license_files(ROOT / "packaging" / "licenses" / "webview2-loader")),
    ]


def render(components: list[Component], app_version: str) -> str:
    """同じ中身のファイルは 1 回だけ書き、それを使う部品を並べる。"""
    groups: dict[str, tuple[str, list[str]]] = {}
    for c in sorted(components, key=lambda c: (c.kind, c.name.lower(), c.version)):
        for f in c.files:
            text = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip("\n")
            digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
            label = f"{c.name} {c.version}（{c.license}、{f.name}）{c.source}"
            groups.setdefault(digest, (text, []))[1].append(label)
    out = [
        "THIRD-PARTY LICENSES",
        "",
        f"voice-shift-scope {app_version} に含まれる第三者のソフトウェアと、そのライセンスの文。",
        "部品ごとの名前・版・ライセンスの式・ソースの場所を並べ、その下に部品が同梱するライセンスのファイルの中身を置く。",
        f"部品の数: {len(components)}。ソースは各部品の URL から得られる（MPL-2.0 の部品を含む）。",
        "",
    ]
    for i, (text, users) in enumerate(sorted(groups.values(), key=lambda g: g[1][0].lower()), 1):
        out.append("=" * 78)
        out.append(f"[{i}] 次の部品が使う")
        out.extend(f"  - {u}" for u in users)
        out.append("-" * 78)
        out.append(text)
        out.append("")
    return "\n".join(out) + "\n"


def app_version() -> str:
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], ROOT))
    return next(p["version"] for p in metadata["packages"] if p["name"] == "voice-tauri")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    try:
        components = crates() + npm_packages() + bundled()
        check(components)
    except LicenseError as e:
        print(e, file=sys.stderr)
        return 1
    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        f.write(render(components, app_version()))
    print(f"{args.out}: {len(components)} の部品", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
