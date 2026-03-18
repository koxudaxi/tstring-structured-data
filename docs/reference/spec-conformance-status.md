# Spec Conformance Status

Last updated: 2026-03-16

This repository makes profile-scoped conformance claims rather than
single-baseline format claims.

## Current Summary

| Format | Profile | Claim | Evidence basis |
| --- | --- | --- | --- |
| JSON | `rfc8259` | 100% | Repo manifest plus Python and Rust conformance runners |
| TOML | `1.0` | 100% | Repo manifest plus vendored `toml-test` fixtures in the TOML 1.0 scope |
| TOML | `1.1` | partial | Repo manifest covering the TOML 1.1 additions shipped in this phase |
| YAML | `1.2.2` | 100% | Repo manifest plus vendored `yaml-test-suite` coverage |

## Evidence Contract

The source of truth is now:

- `conformance/<format>/profiles.toml`
- `conformance/<format>/profiles/<profile>/spec-map.toml`

The `profiles.toml` index declares:

- supported profiles
- the default profile
- the per-profile manifest path relative to `conformance/<format>`

Python and Rust conformance runners both resolve manifests through that shared
contract.

## Scope Notes

- JSON currently exposes only the `rfc8259` profile.
- TOML now exposes both `1.0` and `1.1`, with `1.1` as the public default.
- TOML `1.0` remains the compatibility profile for repository-owned tests and
  examples that need the earlier behavior boundary.
- YAML currently exposes only the `1.2.2` profile.
- Host-language representability limits remain separate from format-spec
  conformance. For example, TOML still has no null value and JSON/YAML still
  reject non-finite floats in this runtime.

## Verification Commands

- `uv run --group dev pytest`
- `PYO3_PYTHON="$PWD/.venv/bin/python3" cargo test --manifest-path rust/Cargo.toml --all`
