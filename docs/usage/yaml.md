# YAML

The `yaml-tstring` package provides `render_data`, `render_text`, and `render_result` for YAML templates.

## Docker Compose example

A realistic example building Docker Compose service definitions. The builder function returns a `Template` object — the caller renders at the point of use:

```python
--8<-- "examples/yaml_docker_compose.py"
```

### What to notice

- `compose_service_template()` returns a `Template`, not rendered text
- Quoted fragments build the image ref and port mapping strings
- `dict` values render as YAML mappings
- `list` values render as YAML sequences

## Release manifest example

This example demonstrates YAML-specific features: anchors, aliases, tags, and block scalars:

```python
--8<-- "examples/yaml_release_manifest.py"
```

### What to notice

- The **anchor name** is interpolated once and reused through an alias
- The release name uses **quoted-string fragment** interpolation
- The startup message uses **YAML block-scalar** assembly
- The local **tag** comes from static punctuation plus an interpolated suffix

## Interpolation contexts

| Context | Example | Description |
|---------|---------|-------------|
| Whole value | `key: {val}` | Any YAML-representable Python value |
| Map key | `{key}: value` | Must be `str` |
| String fragment | `"{name}-{env}"` | Inserted inside a quoted string |
| Anchor | `&{name}` | Must be `str`, non-empty, no whitespace |
| Alias | `*{name}` | Must be `str`, non-empty, no whitespace |
| Tag | `!{tag}` | Must be `str`, non-empty, no whitespace |
| Block scalar | `\|` / `>` with `{fragments}` | Scalar assembly |

## Supported types

- `str`, `int`, `float`, `bool`, `None`
- `list` (rendered as YAML sequences)
- `dict` (rendered as YAML mappings)

!!! warning
    Although YAML 1.2.2 Core Schema supports `.inf` and `.nan`, this library rejects `float("inf")` and `float("nan")` to keep output portable across parsers. Anchor/tag fragments must be non-empty and whitespace-free.

## Profile

| Profile | Description | Default |
|---------|-------------|---------|
| `1.2.2` | YAML 1.2.2 | Yes |

```python
from yaml_tstring import render_data

data = render_data(template, profile="1.2.2")
```
