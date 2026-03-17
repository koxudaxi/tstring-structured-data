#!/usr/bin/env python3

import json
import sys
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path


def workspace_version() -> str:
    cargo_toml = Path("rust/Cargo.toml")
    with cargo_toml.open("rb") as handle:
        data = tomllib.load(handle)
    return data["workspace"]["package"]["version"]


def crate_is_visible(crate_name: str, version: str) -> bool:
    request = urllib.request.Request(
        f"https://crates.io/api/v1/crates/{crate_name}",
        headers={"User-Agent": "tstring-structured-data-release-check"},
    )
    with urllib.request.urlopen(request, timeout=15) as response:
        payload = json.load(response)
    return any(entry["num"] == version for entry in payload.get("versions", []))


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: wait_for_crate.py <crate-name>", file=sys.stderr)
        return 2

    crate_name = sys.argv[1]
    version = workspace_version()
    deadline = time.time() + 900

    while time.time() < deadline:
        try:
            if crate_is_visible(crate_name, version):
                print(f"{crate_name} {version} is visible on crates.io")
                return 0
        except urllib.error.URLError as err:
            print(f"crates.io lookup failed: {err}", file=sys.stderr)

        print(f"waiting for {crate_name} {version} to appear on crates.io")
        time.sleep(15)

    print(f"timed out waiting for {crate_name} {version} to appear on crates.io", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
