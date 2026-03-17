#!/usr/bin/env python3

from __future__ import annotations

import argparse
import io
import subprocess
import shutil
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class ExtraFile:
    repo: str
    ref: str
    source_path: str
    destination_path: str


@dataclass(frozen=True)
class VendorConfig:
    format_name: str
    repo: str
    vendor_dir: Path
    copy_paths: tuple[str, ...]
    copy_entire_archive: bool
    extra_files: tuple[ExtraFile, ...]

    def ref_from_snapshot(self, snapshot: str) -> str:
        prefixes = {
            "toml": "toml-test-",
            "yaml": "yaml-test-suite-",
        }
        prefix = prefixes[self.format_name]
        if not snapshot.startswith(prefix):
            raise ValueError(
                f"Unsupported snapshot {snapshot!r} for {self.format_name}; expected prefix {prefix!r}."
            )
        return snapshot.removeprefix(prefix)


CONFIGS = {
    "toml": VendorConfig(
        format_name="toml",
        repo="toml-lang/toml-test",
        vendor_dir=REPO_ROOT / "conformance" / "toml" / "vendor" / "toml-test",
        copy_paths=("LICENSE", "README.md", "tests"),
        copy_entire_archive=False,
        extra_files=(),
    ),
    "yaml": VendorConfig(
        format_name="yaml",
        repo="yaml/yaml-test-suite",
        vendor_dir=REPO_ROOT / "conformance" / "yaml" / "vendor" / "yaml-test-suite",
        copy_paths=(),
        copy_entire_archive=True,
        extra_files=(
            ExtraFile(
                repo="yaml/yaml-test-suite",
                ref="main",
                source_path="License",
                destination_path="LICENSE",
            ),
        ),
    ),
}


def load_snapshot(format_name: str) -> str:
    format_root = REPO_ROOT / "conformance" / format_name
    profiles = read_toml(format_root / "profiles.toml")
    manifest_paths = [
        format_root / profile["manifest_path"]
        for profile in profiles["profiles"].values()
    ]
    snapshots = {
        read_toml(manifest_path)["provenance"]["snapshot"] for manifest_path in manifest_paths
    }
    if len(snapshots) != 1:
        raise ValueError(
            f"Expected exactly one snapshot for {format_name}, found {sorted(snapshots)!r}."
        )
    return snapshots.pop()


def read_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def download_tarball(repo: str, ref: str) -> Path:
    url = f"https://codeload.github.com/{repo}/tar.gz/{ref}"
    print(f"Downloading {url}")
    request = urllib.request.Request(url, headers={"User-Agent": "tstring-structured-data-sync"})
    try:
        with urllib.request.urlopen(request) as response:
            payload = response.read()
    except urllib.error.URLError as err:
        print(f"Falling back to curl after download error: {err}")
        payload = subprocess.run(
            ["curl", "-L", "--fail", url],
            check=True,
            capture_output=True,
        ).stdout

    tempdir = Path(tempfile.mkdtemp(prefix="conformance-sync-"))
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as archive:
        archive.extractall(tempdir)

    children = [path for path in tempdir.iterdir()]
    if len(children) != 1:
        raise RuntimeError(f"Expected one extracted root for {repo}@{ref}, found {children!r}.")
    return children[0]


def copy_path(source: Path, destination: Path) -> None:
    if source.is_dir():
        shutil.copytree(source, destination)
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def sync_vendor(config: VendorConfig) -> None:
    snapshot = load_snapshot(config.format_name)
    ref = config.ref_from_snapshot(snapshot)
    upstream_root = download_tarball(config.repo, ref)

    if config.vendor_dir.exists():
        shutil.rmtree(config.vendor_dir)
    config.vendor_dir.mkdir(parents=True, exist_ok=True)

    if config.copy_entire_archive:
        for child in upstream_root.iterdir():
            copy_path(child, config.vendor_dir / child.name)
    else:
        for relative_path in config.copy_paths:
            copy_path(upstream_root / relative_path, config.vendor_dir / relative_path)

    for extra_file in config.extra_files:
        extra_root = download_tarball(extra_file.repo, extra_file.ref)
        copy_path(
            extra_root / extra_file.source_path,
            config.vendor_dir / extra_file.destination_path,
        )

    write_provenance(config, snapshot, ref)
    print(f"Synchronized {config.format_name} vendor data into {config.vendor_dir}")


def write_provenance(config: VendorConfig, snapshot: str, ref: str) -> None:
    canonical = f"https://github.com/{config.repo}"
    if config.format_name == "toml":
        body = (
            "# toml-test Provenance\n\n"
            f"- Upstream project: `{config.repo}`\n"
            f"- Canonical repository: <{canonical}>\n"
            f"- Snapshot basis: `{ref}`\n"
            "- Local policy: keep vendored files synchronized with "
            "`scripts/sync_conformance_vendor.py` and retain the upstream test layout under `tests/`\n"
            "- Redistribution note: upstream is MIT licensed; the copied license text is stored alongside this snapshot in `LICENSE`\n"
            f"- Snapshot label from manifests: `{snapshot}`\n"
        )
    else:
        body = (
            "# yaml-test-suite Provenance\n\n"
            f"- Upstream project: `{config.repo}`\n"
            f"- Canonical repository: <{canonical}>\n"
            f"- Snapshot basis: `{ref}`\n"
            "- Local policy: keep vendored files synchronized with "
            "`scripts/sync_conformance_vendor.py` and preserve the upstream case-directory layout\n"
            "- Redistribution note: upstream is MIT licensed; the copied license text is stored alongside this snapshot in `LICENSE`\n"
            f"- Snapshot label from manifests: `{snapshot}`\n"
        )

    (config.vendor_dir / "PROVENANCE.md").write_text(body, encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Synchronize vendored conformance fixtures from their upstream repositories."
    )
    parser.add_argument(
        "formats",
        nargs="*",
        choices=sorted(CONFIGS),
        default=sorted(CONFIGS),
        help="Formats to synchronize. Defaults to all vendored formats.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    for format_name in args.formats:
        sync_vendor(CONFIGS[format_name])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
