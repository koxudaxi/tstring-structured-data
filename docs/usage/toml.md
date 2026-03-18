# TOML

The `toml-tstring` package provides `render_data`, `render_text`, and `render_result` for TOML templates.

## Application config example

This example builds a TOML configuration with interpolated table headers, keys, datetime values, and multiline strings:

```python
--8<-- "examples/toml_app_config.py"
```

### What to notice

- The service name is interpolated into table headers: `[services.{service_name}]`
- The region is interpolated into a TOML key position: `{region} = {environment}`
- The multiline welcome message starts life as a readable TOML string
- `datetime`, `date`, and `time` values render as TOML-native literals
- Bare scalar assembly like `{service_name}-{environment}` becomes a string

## Interpolation contexts

| Context | Example | Description |
|---------|---------|-------------|
| Whole value | `key = {val}` | Any TOML-representable Python value |
| Table header | `[{name}]` | Must be `str` |
| Key | `{key} = value` | Must be `str` |
| String fragment | `"hello {name}"` | Inserted inside a TOML string |

## Supported types

- `str`, `int`, `float`, `bool`
- `list` (rendered as TOML arrays)
- `dict` (rendered as inline tables or nested tables)
- `datetime`, `date`, `time` (rendered as TOML-native datetime literals)

!!! warning
    TOML has no null value — `None` is rejected. Offset-aware `time` values are also rejected.

## Profiles

| Profile | Description | Default |
|---------|-------------|---------|
| `1.1` | TOML 1.1 (inline table newlines, trailing commas, `\e`/`\xHH` escapes, times without seconds) | Yes |
| `1.0` | TOML 1.0 | No |

```python
from toml_tstring import render_data

# Use TOML 1.1 features (default)
data = render_data(template)

# Restrict to TOML 1.0 behavior
data = render_data(template, profile="1.0")
```
