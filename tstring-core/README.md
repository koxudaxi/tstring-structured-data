# tstring-core

Shared Python compatibility layer for the Rust-first JSON, TOML, and YAML
t-string backends.

Requires Python 3.14+.

This package depends on `tstring-bindings`, a native PyO3 extension. On
supported platforms, install from prebuilt wheels. Other environments require a
local Rust 1.94.0 toolchain build.

## What It Provides

- shared error categories re-exported from the Rust bindings
- compatibility helper APIs for tokenization, spans, diagnostics, and slots
- a stable import surface for the Python wrapper packages and tests

## What It Does Not Provide

- JSON grammar rules
- TOML grammar rules
- YAML grammar rules
- backend-specific representability policies

Those responsibilities now live in the Rust workspace under `rust/`.

## Runtime Contract

Each backend exposed through the Python packages follows the same high-level
pipeline:

1. validate that the input is a PEP 750 `Template`
2. pass the template into the shared PyO3 bindings
3. parse backend-specific structure in Rust
4. run semantic checks in Rust
5. render text and backend-native Python data
6. materialize backend-native data from the same parsed/rendered structure

The shared layer is also responsible for keeping the Python-facing exception and
typing surface stable across the JSON, TOML, and YAML wrapper packages.

## See also

- [Project README](https://github.com/koxudaxi/tstring-structured-data#readme)
- [Architecture](https://github.com/koxudaxi/tstring-structured-data/blob/main/docs/architecture.md)
