# Backend Support Matrix

Last updated: 2026-03-16

This document tracks the current tested support boundary by
format/profile pair.

## Profile Matrix

| Format | Profile | Status | Default | Notes |
| --- | --- | --- | --- | --- |
| JSON | `rfc8259` | supported | yes | Current and only JSON profile in this phase |
| TOML | `1.0` | supported | no | Use when preserving pre-migration repo behavior matters |
| TOML | `1.1` | supported | yes | Enables the TOML 1.1 additions currently shipped here |
| YAML | `1.2.2` | supported | yes | Current and only YAML profile in this phase |

## Shared Public Surface

- `json_tstring.render_data`, `render_text`, `render_result`
- `toml_tstring.render_data`, `render_text`, `render_result`
- `yaml_tstring.render_data`, `render_text`, `render_result`
- `tstring_bindings.render_json`, `render_json_text`
- `tstring_bindings.render_toml`, `render_toml_text`
- `tstring_bindings.render_yaml`, `render_yaml_text`

Every supported public Python entrypoint accepts `profile=...`.
Unknown profile strings raise `ValueError` in Python before the Rust backend is
called. The Rust extension also rejects unsupported profile strings
defensively.

The extension submodule `tstring_bindings.tstring_bindings` remains present for
internal wrapper-package imports, but it is not part of the public contract.

## JSON `rfc8259`

### Supported and tested

- top-level scalar, object, and array values
- whole-value, object-key, quoted-key-fragment, and string-fragment interpolation
- RFC 8259 number forms, escape sequences, surrogate pairs, and representative examples
- JSON data normalization through `serde_json`

### Expected remaining failures

- object keys that do not resolve to `str`
- non-finite floats
- Python values that are not representable in JSON

### Out of scope

- JSON5-style extensions such as comments and trailing commas

## TOML `1.0`

### Supported and tested

- assignments, dotted keys, table headers, array-of-table headers, arrays, and inline tables
- basic, literal, multiline basic, and multiline literal strings
- TOML 1.0 numeric, string, date, time, and datetime forms exercised in tests
- interpolation in keys, headers, values, and string fragments

### Expected remaining failures

- `None`, because TOML has no null value
- offset-aware `time` values
- Python values that are not representable in TOML

## TOML `1.1`

### Additional support beyond `1.0`

- inline-table newlines and trailing commas
- inline-table newlines with comments
- basic-string `\e` escape support
- basic-string `\xHH` escape support
- times and datetimes without seconds

### Notes

- `1.1` is the public default profile for TOML in this repository phase.
- Callers that need the older repository behavior should pass `profile="1.0"`
  explicitly.

## YAML `1.2.2`

### Supported and tested

- block and flow mappings/sequences
- plain, single-quoted, double-quoted, and block scalars
- anchors, aliases, tags, directives, and explicit document markers
- multi-document streams
- interpolation in keys, values, scalar fragments, and metadata
- normalization through `saphyr`

### Expected remaining failures

- non-finite floats
- metadata fragments that are empty or contain whitespace
- Python values that are not representable in the current YAML backend surface

### Out of scope

- YAML 1.1 support
- non-data rendering guarantees beyond the current `render_text` surface
