# Profiles

Each backend accepts an optional `profile` keyword argument to select the target spec version.

## Profile matrix

| Format | Profile | Default | Description |
|--------|---------|---------|-------------|
| JSON | `rfc8259` | Yes | RFC 8259 JSON |
| TOML | `1.1` | Yes | TOML 1.1 with inline table newlines, trailing commas, `\e`/`\xHH` escapes, times without seconds |
| TOML | `1.0` | No | TOML 1.0 strict |
| YAML | `1.2.2` | Yes | YAML 1.2.2 |

## Usage

When `profile` is omitted (or `None`), the default profile for that format is used:

```python
from json_tstring import render_data
from toml_tstring import render_data as render_toml_data
from yaml_tstring import render_data as render_yaml_data

# All use their default profiles
json_data = render_data(t'{"key": {value}}')           # rfc8259
toml_data = render_toml_data(t'key = {value}')          # 1.1
yaml_data = render_yaml_data(t'key: {value}')            # 1.2.2
```

To select a specific profile:

```python
from toml_tstring import render_data

# Use TOML 1.0 for compatibility
data = render_data(template, profile="1.0")
```

## TOML 1.1 additions

The `1.1` profile enables features beyond TOML 1.0:

- Inline table newlines and trailing commas
- Inline table newlines with comments
- Basic-string `\e` escape
- Basic-string `\xHH` escape
- Times and datetimes without seconds

If you need compatibility with TOML 1.0 tooling, pass `profile="1.0"` explicitly.

## Invalid profiles

Passing an unrecognized profile string raises `ValueError` immediately — before the Rust backend is invoked:

```python
from json_tstring import render_data

render_data(t'{"key": {value}}', profile="draft-07")
# => ValueError: unknown profile
```

## Conformance

Each profile's conformance claims are backed by per-profile manifests and test suites. See [Backend Support Matrix](../reference/backend-support-matrix.md) and [Spec Conformance](../reference/spec-conformance-status.md) for details.
