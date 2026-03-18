# Conformance Assets

This directory contains the reproducible evidence used for spec-origin
conformance work across the JSON, TOML, and YAML backends.

## Structure

- `conformance/<format>/profiles.toml` is the profile index for that format.
- `conformance/<format>/profiles/<profile>/spec-map.toml` is the per-profile
  manifest.
- `conformance/<format>/cases/` and `vendor/` remain shared asset directories
  addressed by manifest-relative paths.

Each per-profile `spec-map.toml` manifest is the source of truth for:

- the spec section or production being exercised
- the concrete case id
- the expected result (`accept` or `reject`)
- which execution layer must enforce it (`rust`, `python`, or `both`)
- optional notes for representability or provenance details

Each `profiles.toml` index is the source of truth for:

- which profiles are supported for that format
- which profile is the default
- where each per-profile manifest lives relative to `conformance/<format>`

## Current Status

The repository currently claims:

- JSON `rfc8259`
- TOML `1.0`
- TOML `1.1` additions shipped in this phase
- YAML `1.2.2`

## Verification

Python conformance checks run as part of the existing package `pytest` suites.

```bash
uv run --group dev pytest json-tstring/tests toml-tstring/tests yaml-tstring/tests
```

Rust conformance checks run as package integration tests:

```bash
PYO3_PYTHON="$PWD/.venv/bin/python3" cargo test --manifest-path rust/Cargo.toml -p tstring-json --tests
PYO3_PYTHON="$PWD/.venv/bin/python3" cargo test --manifest-path rust/Cargo.toml -p tstring-toml --tests
PYO3_PYTHON="$PWD/.venv/bin/python3" cargo test --manifest-path rust/Cargo.toml -p tstring-yaml --tests
```

## Vendored corpora

The TOML and YAML upstream corpora are synchronized by
`../scripts/sync_conformance_vendor.py`. See
the [Conformance Vendor Sync](https://tstring-structured-data.koxudaxi.dev/development/conformance-vendor-sync/) documentation for the update workflow.
