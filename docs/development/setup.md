# Development Setup

## Prerequisites

- Python 3.14
- [uv](https://docs.astral.sh/uv/)
- [Rust 1.94.0+](https://www.rust-lang.org/tools/install) (for building `tstring-bindings`)
- git
- [prek](https://github.com/j178/prek) (for local pre-commit hooks)

## Install dependencies

The repository uses a root `uv` workspace plus an in-repo Rust workspace:

```bash
uv sync --group dev
```

`uv sync` installs the shared `tstring-bindings` workspace package as part of the normal dependency graph. Use `maturin develop` only when you are working on the bindings package in isolation.

## Install prek

```bash
uv tool install prek
```

Or run without a permanent installation:

```bash
uvx prek --version
```

## Install git hooks

From the repository root:

```bash
prek install
```

This repository uses a root `.pre-commit-config.yaml` with local hooks that run:

- `ruff format --check`
- `ruff check`
- `ty check`

The hooks are split by backend so changes usually only trigger the relevant checks.

## Run hooks manually

Run all hooks against the whole repository:

```bash
prek run --all-files
```

Run a single hook:

```bash
prek run json-backend-checks --all-files
prek run toml-backend-checks --all-files
prek run yaml-backend-checks --all-files
```

## Verification commands

### Python workspace

```bash
uv sync --group dev
uv run --group dev ruff format --check .
uv run --group dev ruff check .
uv run --group dev pytest
```

Conformance manifests and vendored fixture slices live under `conformance/`. The Python package test suites include their format-specific conformance cases automatically.

Optional bindings-only workflow:

```bash
uv run --group dev maturin develop -F extension-module --manifest-path rust/python-bindings/Cargo.toml
```

### Rust workspace

```bash
cargo fmt --manifest-path rust/Cargo.toml --all --check
cargo clippy --manifest-path rust/Cargo.toml --all-targets --all-features -- -D warnings
PYO3_PYTHON="$PWD/.venv/bin/python3" cargo test --manifest-path rust/Cargo.toml --all
```

The Rust integration test suites include parser-level conformance checks driven by the same manifests used by the Python tests.

## Guidelines

- Keep repository content in English only.
- Prefer small, reviewable commits.
- The authoritative backend behavior is protected by Python end-to-end tests and Rust crate tests — keep both layers covered when changing runtime behavior.
