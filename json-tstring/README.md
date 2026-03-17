# json-tstring

JSON rendering for PEP 750 t-strings. Parsing and rendering happen in Rust;
this package is the Python wrapper.

Requires Python 3.14+.

This package depends on `tstring-bindings`, a native PyO3 extension. On
supported platforms, install from prebuilt wheels. Other environments require a
local Rust 1.94.0 toolchain build.

## API

```python
render_data(template, profile="rfc8259")  # -> Python data
render_text(template, profile="rfc8259")  # -> JSON text
render_result(template, profile="rfc8259")  # -> RenderResult (.text + .data)
```

Type alias: `JsonProfile = Literal["rfc8259"]`

Parsed template structure is cached per process using `template.strings` +
profile as the key.

## How it works

The Python `Template` is converted to a Rust token stream, parsed into JSON
nodes (keeping interpolation visible in values, keys, and string fragments),
and rendered back to text or Python data. `serde_json` handles normalization.

## Supported positions

- whole-value, object-key, quoted-key-fragment, and string-fragment interpolation
- bare fragments promoted to JSON strings
- nested arrays and objects, top-level values

## Limits

- object keys must be `str`
- non-finite floats rejected
- values must be JSON-representable
- integers keep exact Python text (no silent `float` coercion)

## Verify

```bash
uv sync --group dev
uv run --group dev pytest
```

## See also

- [Project README](https://github.com/koxudaxi/tstring-structured-data#readme)
- [Backend support matrix](https://github.com/koxudaxi/tstring-structured-data/blob/main/docs/backend-support-matrix.md)
