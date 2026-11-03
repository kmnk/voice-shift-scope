#!/usr/bin/env python3
"""Makes THIRD-PARTY-LICENSES.txt for the release packages.

Copies the license files shipped by the third-party components in the exe (Rust crates, the screen's npm packages, third_party/, and the WebView2 loader)
into one text file without changing their content.

Usage: python3 scripts/third_party_licenses.py --out <output path>

Calls cargo and npm. In a sandbox, run it with CARGO_HOME set (AGENTS.md). Needs frontend/node_modules.
Stops with exit code 1 if either of these holds:
- a component has no license file at all (add one under packaging/licenses/crates/)
- a license name not decided as accepted appears in a component's license expression (review ACCEPTED)
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

# SPDX names of accepted licenses. Lists only those in the current dependencies.
# Before adding one, check that the release packages can meet its conditions (notices, providing source, and so on).
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
    kind: str  # "crate", "npm", or "bundled"
    name: str
    version: str
    license: str
    source: str
    files: list[Path] = field(default_factory=list)


class LicenseError(Exception):
    pass


def license_names(expr: str) -> set[str]:
    """Takes the license and exception names out of an SPDX expression. The old "/" form also counts as OR."""
    tokens = re.findall(r"[A-Za-z0-9.+\-]+", expr.replace("/", " OR "))
    return {t for t in tokens if t not in ("AND", "OR", "WITH")}


def license_files(directory: Path) -> list[Path]:
    if not directory.is_dir():
        return []
    return sorted(p for p in directory.iterdir() if p.is_file() and LICENSE_FILE.match(p.name))


def check(components: list[Component]) -> None:
    """The two checks. Raises LicenseError if either holds."""
    problems = []
    for c in components:
        if not c.files:
            problems.append(f"no license file: {c.kind} {c.name} {c.version}"
                            f" (for a crate, put it in {FALLBACK_DIR.relative_to(ROOT)}/{c.name}-{c.version}/)")
        unknown = license_names(c.license) - ACCEPTED
        if unknown:
            problems.append(f"license not decided as accepted: {c.kind} {c.name} {c.version}: "
                            f"{', '.join(sorted(unknown))} (expression {c.license!r})")
    if problems:
        raise LicenseError("\n".join(problems))


def run(args: list[str], cwd: Path) -> str:
    exe = shutil.which(args[0])
    if exe is None:
        raise LicenseError(f"{args[0]} not found")
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
            continue  # unused ones, and our own crates
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
        Component("bundled", "Microsoft WebView2 loader (built in by webview2-com-sys)", "1.0.3650.58",
                  "BSD-3-Clause", "https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3650.58",
                  license_files(ROOT / "packaging" / "licenses" / "webview2-loader")),
    ]


def render(components: list[Component], app_version: str) -> str:
    """Writes each identical file once, listing the components that use it."""
    groups: dict[str, tuple[str, list[str]]] = {}
    for c in sorted(components, key=lambda c: (c.kind, c.name.lower(), c.version)):
        for f in c.files:
            text = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip("\n")
            digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
            label = f"{c.name} {c.version} ({c.license}, {f.name}) {c.source}"
            groups.setdefault(digest, (text, []))[1].append(label)
    out = [
        "THIRD-PARTY LICENSES",
        "",
        f"Third-party software included in voice-shift-scope {app_version}, and its license texts.",
        "Lists each component's name, version, license expression, and source location, followed by the content of the license files the components ship.",
        f"Number of components: {len(components)}. Source is available from each component's URL (including the MPL-2.0 components).",
        "",
    ]
    for i, (text, users) in enumerate(sorted(groups.values(), key=lambda g: g[1][0].lower()), 1):
        out.append("=" * 78)
        out.append(f"[{i}] Used by the following components")
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
    print(f"{args.out}: {len(components)} components", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
