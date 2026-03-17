# YAML Conformance Inputs

This directory combines:

- repo-owned audit notes for YAML 1.2.2 work
- a pinned data-release slice from `yaml-test-suite`

The vendored files are copied from the `data-2022-01-17` release branch so the
checked-in harness uses the stable directory-oriented fixture format documented
by the upstream project. Provenance for that snapshot lives in
`vendor/yaml-test-suite/PROVENANCE.md`, and the copied upstream license text is
kept in `vendor/yaml-test-suite/LICENSE`. Refresh the vendored snapshot with
`uv run python scripts/sync_conformance_vendor.py yaml`.

The source of truth for supported YAML profiles is `profiles.toml`. Each
profile points at its own manifest under `profiles/<profile>/spec-map.toml`.
