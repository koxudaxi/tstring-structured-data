# API Reference

All three packages expose the same three-function interface. Only the types and profile options differ.

## Functions

### `render_data`

Parse a t-string template and return Python objects.

=== "JSON"

    ```python
    from json_tstring import render_data

    render_data(template: Template, *, profile: JsonProfile | str | None = None) -> JsonValue
    ```

=== "TOML"

    ```python
    from toml_tstring import render_data

    render_data(template: Template, *, profile: TomlProfile | str | None = None) -> TomlValue
    ```

=== "YAML"

    ```python
    from yaml_tstring import render_data

    render_data(template: Template, *, profile: YamlProfile | str | None = None) -> YamlValue
    ```

Parameters:

| Parameter | Type | Description |
|-----------|------|-------------|
| `template` | `Template` | A PEP 750 template string (`t"..."`) |
| `profile` | `JsonProfile \| TomlProfile \| YamlProfile \| str \| None` | Spec version profile (see [Profile types](#profile-types)). `None` uses the default for the format |

Returns: Parsed Python data (`dict`, `list`, `str`, `int`, `float`, `bool`, `None`, or temporal types for TOML).

### `render_text`

Parse a t-string template and return valid formatted text.

=== "JSON"

    ```python
    from json_tstring import render_text

    render_text(template: Template, *, profile: JsonProfile | str | None = None) -> str
    ```

=== "TOML"

    ```python
    from toml_tstring import render_text

    render_text(template: Template, *, profile: TomlProfile | str | None = None) -> str
    ```

=== "YAML"

    ```python
    from yaml_tstring import render_text

    render_text(template: Template, *, profile: YamlProfile | str | None = None) -> str
    ```

Parameters: Same as `render_data`.

Returns: A `str` containing valid JSON, TOML, or YAML text.

### `render_result`

Parse a t-string template and return both data and text.

=== "JSON"

    ```python
    from json_tstring import render_result

    render_result(template: Template, *, profile: JsonProfile | str | None = None) -> RenderResult[JsonValue]
    ```

=== "TOML"

    ```python
    from toml_tstring import render_result

    render_result(template: Template, *, profile: TomlProfile | str | None = None) -> RenderResult[TomlValue]
    ```

=== "YAML"

    ```python
    from yaml_tstring import render_result

    render_result(template: Template, *, profile: YamlProfile | str | None = None) -> RenderResult[YamlValue]
    ```

Parameters: Same as `render_data`.

Returns: A `RenderResult` with `.data` and `.text` attributes.

## Types

### `RenderResult`

```python
from tstring_core import RenderResult

@dataclass(frozen=True, slots=True)
class RenderResult(Generic[TData]):
    text: str
    data: TData
```

| Attribute | Type | Description |
|-----------|------|-------------|
| `text` | `str` | Rendered text in the target format |
| `data` | `TData` | Parsed Python data |

### Profile types

| Type | Values | Package |
|------|--------|---------|
| `JsonProfile` | `Literal["rfc8259"]` | `json_tstring` |
| `TomlProfile` | `Literal["1.0", "1.1"]` | `toml_tstring` |
| `YamlProfile` | `Literal["1.2.2"]` | `yaml_tstring` |

### Value types

| Type | Definition | Package |
|------|-----------|---------|
| `JsonValue` | `dict \| list \| str \| int \| float \| bool \| None` | `tstring_core` |
| `TomlValue` | `dict \| list \| str \| int \| float \| bool \| datetime \| date \| time` | `tstring_core` |
| `YamlValue` | `dict \| list \| str \| int \| float \| bool \| None` | `tstring_core` |

## Exceptions

All exceptions are re-exported from `tstring-bindings`:

```
TemplateError
├── TemplateParseError
├── TemplateSemanticError
└── UnrepresentableValueError
```

| Exception | When |
|-----------|------|
| `TemplateParseError` | Template syntax is invalid for the target format |
| `TemplateSemanticError` | Valid syntax but invalid semantics (e.g., non-string JSON key) |
| `UnrepresentableValueError` | Python value can't be represented in the target format |

Errors carry `Diagnostic` objects with source spans and expression labels.

Unknown profile strings raise `ValueError` before the Rust backend is invoked.
