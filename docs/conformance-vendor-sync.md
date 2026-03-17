# Conformance Vendor Sync

Use `scripts/sync_conformance_vendor.py` to refresh the vendored upstream
conformance corpora under `conformance/toml/vendor/` and
`conformance/yaml/vendor/`.

## What it does

- reads the pinned snapshot labels from the per-format manifest provenance
- downloads the matching upstream tarballs from GitHub
- recreates the vendored directories from scratch
- writes `PROVENANCE.md` inside each vendored snapshot

## Usage

Sync every vendored corpus:

```bash
uv run python scripts/sync_conformance_vendor.py
```

Sync a single corpus:

```bash
uv run python scripts/sync_conformance_vendor.py toml
uv run python scripts/sync_conformance_vendor.py yaml
```

## Update flow

1. Update the `provenance.snapshot` value in the relevant `spec-map.toml`.
2. Run `uv run python scripts/sync_conformance_vendor.py <format>`.
3. Run the conformance-facing test suites.
4. Review the regenerated `PROVENANCE.md` and vendored `LICENSE` files before committing.

Do not edit vendored upstream files by hand unless the change is also reflected
in the sync script or the manifest pin.
