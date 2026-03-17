# toml-tstring

TOML rendering for PEP 750 t-strings. Parsing and rendering happen in Rust;
this package is the Python wrapper.

Requires Python 3.14+.

This package depends on `tstring-bindings`, a native PyO3 extension. On
supported platforms, install from prebuilt wheels. Other environments require a
local Rust 1.94.0 toolchain build.

## API

```python
render_data(template, profile="1.1")  # -> Python data
render_text(template, profile="1.1")  # -> TOML text
render_result(template, profile="1.1")  # -> RenderResult (.text + .data)
```

Type alias: `TomlProfile = Literal["1.0", "1.1"]`

Parsed template structure is cached per process using `template.strings` +
profile as the key. Use `profile="1.0"` when you need the stricter TOML 1.0
behavior.

## How it works

The Python `Template` is converted to a Rust token stream and parsed into TOML
nodes -- assignments, key paths, headers, arrays, inline tables, literals, and
string families all become explicit nodes with interpolation preserved.
Rendering is driven by the parsed node type. The Rust `toml` crate handles
value materialization and normalization.

## Supported positions

- whole-value, key, dotted-key, table-header, and array-of-table interpolation
- string-fragment interpolation across all four TOML string types
- nested arrays, inline tables, and array-of-table sections
- integer forms (hex, binary, octal), special floats (`inf`, `nan`)
- `datetime`, `date`, and `time` values

## Limits

- `None` rejected (TOML has no null)
- offset `time` values rejected
- integers must fit signed 64-bit range
- values must be TOML-representable

## Verify

```bash
uv sync --group dev
uv run --group dev pytest
```

## See also

- [Project README](https://github.com/koxudaxi/tstring-structured-data#readme)
- [Backend support matrix](https://github.com/koxudaxi/tstring-structured-data/blob/main/docs/backend-support-matrix.md)
