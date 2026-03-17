# TOML Conformance Inputs

This directory combines:

- repo-owned audit cases for TOML profile behavior that is easy to compare at
  the Python wrapper layer
- a pinned snapshot slice from `toml-test`

The current snapshot metadata is recorded in
`vendor/toml-test/PROVENANCE.md`. Refresh the vendored snapshot with
`uv run python scripts/sync_conformance_vendor.py toml`.

The source of truth for supported TOML profiles is `profiles.toml`. Each
profile points at its own manifest under `profiles/<profile>/spec-map.toml`.
