# yaml-tstring

YAML rendering for PEP 750 t-strings. Parsing and rendering happen in Rust;
this package is the Python wrapper.

Requires Python 3.14+.

This package depends on `tstring-bindings`, a native PyO3 extension. On
supported platforms, install from prebuilt wheels. Other environments require a
local Rust 1.94.0 toolchain build.

## API

```python
render_data(template, profile="1.2.2")  # -> Python data (list for multi-doc)
render_text(template, profile="1.2.2")  # -> YAML text
render_result(template, profile="1.2.2")  # -> RenderResult (.text + .data)
```

Type alias: `YamlProfile = Literal["1.2.2"]`

Parsed template structure is cached per process using `template.strings` +
profile as the key.

## How it works

The Python `Template` is converted to a Rust token stream and parsed by an
interpolation-aware YAML scanner/parser. Block mappings, block sequences, flow
collections, scalar styles, anchors, aliases, and tags are all parsed
explicitly in Rust with interpolation nodes preserved. `saphyr` handles
data materialization and normalization.

## Supported positions

- mapping-key interpolation
- plain, single-quoted, double-quoted, and block scalar assembly
- anchor, alias, and tag interpolation (including verbatim `!<...>` tags)
- block and flow collections, multi-document streams
- directives, `%TAG` handles, explicit document markers
- merge keys, complex keys, trailing commas in flow collections
- YAML 1.2.2 escape sequences in double-quoted scalars

The full tested boundary is in the
[backend support matrix](https://github.com/koxudaxi/tstring-structured-data/blob/main/docs/backend-support-matrix.md).

## Limits

- non-finite floats rejected
- metadata fragments must be non-empty and whitespace-free
- integers keep exact Python text (no silent `float` coercion)
- values must be representable in the current YAML 1.2+ surface

## Verify

```bash
uv sync --group dev
uv run --group dev pytest
```

## See also

- [Project README](https://github.com/koxudaxi/tstring-structured-data#readme)
- [Backend support matrix](https://github.com/koxudaxi/tstring-structured-data/blob/main/docs/backend-support-matrix.md)
