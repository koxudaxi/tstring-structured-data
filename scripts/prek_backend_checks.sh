#!/usr/bin/env bash

set -euo pipefail

package_dir="$1"
example_file="$2"

cd "$package_dir"

uv run --group dev ruff format --check . ../tstring-core
uv run --group dev ruff check . ../tstring-core
uv run --group dev ty check src tests ../tstring-core/src ../examples/_display.py "../examples/${example_file}"
