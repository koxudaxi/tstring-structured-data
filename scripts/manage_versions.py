#!/usr/bin/env python3
"""Manage lockstep package versions across the monorepo."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
VERSION_RE = r"(?P<version>\d+\.\d+\.\d+)"


class Rule:
    def __init__(self, path: str, pattern: str, replacement: str, label: str) -> None:
        self.path = ROOT / path
        self.pattern = re.compile(pattern, re.MULTILINE)
        self.replacement = replacement
        self.label = label

    def read(self) -> str:
        return self.path.read_text(encoding="utf-8")

    def extract_all(self) -> list[str]:
        matches = [match.group("version") for match in self.pattern.finditer(self.read())]
        if not matches:
            raise ValueError(f"Pattern not found for {self.label} in {self.path}")
        return matches

    def replace(self, version: str) -> bool:
        content = self.read()
        updated, count = self.pattern.subn(
            self.replacement.format(version=version),
            content,
        )
        if count == 0:
            raise ValueError(f"Pattern not found for {self.label} in {self.path}")
        if updated != content:
            self.path.write_text(updated, encoding="utf-8")
            return True
        return False


RULES = [
    Rule("pyproject.toml", rf'^(version = "){VERSION_RE}(")$', r"\g<1>{version}\g<3>", "workspace version"),
    Rule(
        "pyproject.toml",
        rf'("json-tstring>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "workspace dependency json-tstring",
    ),
    Rule(
        "pyproject.toml",
        rf'("toml-tstring>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "workspace dependency toml-tstring",
    ),
    Rule(
        "pyproject.toml",
        rf'("tstring-core>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "workspace dependency tstring-core",
    ),
    Rule(
        "pyproject.toml",
        rf'("yaml-tstring>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "workspace dependency yaml-tstring",
    ),
    Rule("tstring-core/pyproject.toml", rf'^(version = "){VERSION_RE}(")$', r"\g<1>{version}\g<3>", "tstring-core version"),
    Rule(
        "tstring-core/pyproject.toml",
        rf'("tstring-bindings>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "tstring-core dependency tstring-bindings",
    ),
    Rule("json-tstring/pyproject.toml", rf'^(version = "){VERSION_RE}(")$', r"\g<1>{version}\g<3>", "json-tstring version"),
    Rule(
        "json-tstring/pyproject.toml",
        rf'("tstring-bindings>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "json-tstring dependency tstring-bindings",
    ),
    Rule(
        "json-tstring/pyproject.toml",
        rf'("tstring-core>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "json-tstring dependency tstring-core",
    ),
    Rule("toml-tstring/pyproject.toml", rf'^(version = "){VERSION_RE}(")$', r"\g<1>{version}\g<3>", "toml-tstring version"),
    Rule(
        "toml-tstring/pyproject.toml",
        rf'("tstring-bindings>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "toml-tstring dependency tstring-bindings",
    ),
    Rule(
        "toml-tstring/pyproject.toml",
        rf'("tstring-core>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "toml-tstring dependency tstring-core",
    ),
    Rule("yaml-tstring/pyproject.toml", rf'^(version = "){VERSION_RE}(")$', r"\g<1>{version}\g<3>", "yaml-tstring version"),
    Rule(
        "yaml-tstring/pyproject.toml",
        rf'("tstring-bindings>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "yaml-tstring dependency tstring-bindings",
    ),
    Rule(
        "yaml-tstring/pyproject.toml",
        rf'("tstring-core>=){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "yaml-tstring dependency tstring-core",
    ),
    Rule(
        "rust/python-bindings/pyproject.toml",
        rf'^(version = "){VERSION_RE}(")$',
        r"\g<1>{version}\g<3>",
        "tstring-bindings Python package version",
    ),
    Rule("rust/Cargo.toml", rf'^(version = "){VERSION_RE}(")$', r"\g<1>{version}\g<3>", "Rust workspace version"),
    Rule(
        "rust/json-tstring-rs/Cargo.toml",
        rf'(tstring-syntax = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-core-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust json-tstring dependency tstring-syntax",
    ),
    Rule(
        "rust/toml-tstring-rs/Cargo.toml",
        rf'(tstring-syntax = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-core-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust toml-tstring dependency tstring-syntax",
    ),
    Rule(
        "rust/yaml-tstring-rs/Cargo.toml",
        rf'(tstring-syntax = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-core-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust yaml-tstring dependency tstring-syntax",
    ),
    Rule(
        "rust/tstring-pyo3-bindings/Cargo.toml",
        rf'(tstring-json = \{{ version = "){VERSION_RE}(", path = "\.\./json-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust pyo3 dependency tstring-json",
    ),
    Rule(
        "rust/tstring-pyo3-bindings/Cargo.toml",
        rf'(tstring-syntax = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-core-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust pyo3 dependency tstring-syntax",
    ),
    Rule(
        "rust/tstring-pyo3-bindings/Cargo.toml",
        rf'(tstring-toml = \{{ version = "){VERSION_RE}(", path = "\.\./toml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust pyo3 dependency tstring-toml",
    ),
    Rule(
        "rust/tstring-pyo3-bindings/Cargo.toml",
        rf'(tstring-yaml = \{{ version = "){VERSION_RE}(", path = "\.\./yaml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust pyo3 dependency tstring-yaml",
    ),
    Rule(
        "rust/python-bindings/Cargo.toml",
        rf'(tstring-json = \{{ version = "){VERSION_RE}(", path = "\.\./json-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust bindings dependency tstring-json",
    ),
    Rule(
        "rust/python-bindings/Cargo.toml",
        rf'(tstring-pyo3-bindings = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-pyo3-bindings" \}})',
        r'\g<1>{version}\g<3>',
        "rust bindings dependency tstring-pyo3-bindings",
    ),
    Rule(
        "rust/python-bindings/Cargo.toml",
        rf'(tstring-syntax = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-core-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust bindings dependency tstring-syntax",
    ),
    Rule(
        "rust/python-bindings/Cargo.toml",
        rf'(tstring-toml = \{{ version = "){VERSION_RE}(", path = "\.\./toml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust bindings dependency tstring-toml",
    ),
    Rule(
        "rust/python-bindings/Cargo.toml",
        rf'(tstring-yaml = \{{ version = "){VERSION_RE}(", path = "\.\./yaml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "rust bindings dependency tstring-yaml",
    ),
    Rule(
        "rust/backend-e2e-tests/Cargo.toml",
        rf'(tstring-json = \{{ version = "){VERSION_RE}(", path = "\.\./json-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "backend e2e dependency tstring-json",
    ),
    Rule(
        "rust/backend-e2e-tests/Cargo.toml",
        rf'(tstring-syntax = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-core-rs" \}})',
        r'\g<1>{version}\g<3>',
        "backend e2e dependency tstring-syntax",
    ),
    Rule(
        "rust/backend-e2e-tests/Cargo.toml",
        rf'(tstring-toml = \{{ version = "){VERSION_RE}(", path = "\.\./toml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "backend e2e dependency tstring-toml",
    ),
    Rule(
        "rust/backend-e2e-tests/Cargo.toml",
        rf'(tstring-yaml = \{{ version = "){VERSION_RE}(", path = "\.\./yaml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "backend e2e dependency tstring-yaml",
    ),
    Rule(
        "rust/yaml-pyo3-tests/Cargo.toml",
        rf'(tstring-pyo3-bindings = \{{ version = "){VERSION_RE}(", path = "\.\./tstring-pyo3-bindings" \}})',
        r'\g<1>{version}\g<3>',
        "yaml pyo3 tests dependency tstring-pyo3-bindings",
    ),
    Rule(
        "rust/yaml-pyo3-tests/Cargo.toml",
        rf'(tstring-yaml = \{{ version = "){VERSION_RE}(", path = "\.\./yaml-tstring-rs" \}})',
        r'\g<1>{version}\g<3>',
        "yaml pyo3 tests dependency tstring-yaml",
    ),
    Rule(
        "rust/python-bindings/src/lib.rs",
        rf'(__version__", "){VERSION_RE}(")',
        r'\g<1>{version}\g<3>',
        "bindings module __version__",
    ),
]


def normalize_tag(raw: str) -> str:
    value = raw.strip()
    if value.startswith(("refs/tags/", "refs/heads/")):
        value = value.rsplit("/", 1)[-1]
    if value.startswith(("v", "V")):
        value = value[1:]
    if not re.fullmatch(VERSION_RE, value):
        raise ValueError(f"Expected semantic version like 0.1.1, got: {raw}")
    return value


def check_versions(expected: str | None) -> int:
    failures: list[str] = []
    actual_versions: set[str] = set()
    for rule in RULES:
        versions = rule.extract_all()
        unique_versions = sorted(set(versions))
        if len(unique_versions) != 1:
            failures.append(
                f"{rule.label}: found inconsistent versions {', '.join(unique_versions)} "
                f"in {rule.path.relative_to(ROOT)}"
            )
            continue
        actual = unique_versions[0]
        actual_versions.add(actual)
        if expected is not None and actual != expected:
            failures.append(f"{rule.label}: expected {expected}, found {actual} in {rule.path.relative_to(ROOT)}")
    if expected is None and len(actual_versions) != 1:
        failures.append(f"Expected one shared version, found: {', '.join(sorted(actual_versions))}")
    if failures:
        print("Version check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    version = expected or next(iter(actual_versions))
    print(f"All lockstep versions are aligned at {version}")
    return 0


def set_versions(version: str) -> int:
    changed = 0
    for rule in RULES:
        if rule.replace(version):
            changed += 1
    print(f"Updated lockstep version references to {version} across {changed} file edits")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Manage lockstep versions in the monorepo.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    check_parser = subparsers.add_parser("check", help="Validate that all managed versions are aligned.")
    check_parser.add_argument("--tag", help="Expected release tag or version, e.g. 0.1.1 or refs/tags/0.1.1")

    set_parser = subparsers.add_parser("set", help="Update all managed version references to one version.")
    set_parser.add_argument("version", help="Semantic version to write, e.g. 0.1.1")

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    if args.command == "check":
        expected = normalize_tag(args.tag) if args.tag else None
        return check_versions(expected)
    if args.command == "set":
        return set_versions(normalize_tag(args.version))
    parser.error(f"Unknown command: {args.command}")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
