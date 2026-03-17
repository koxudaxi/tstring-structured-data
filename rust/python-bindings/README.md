# tstring-bindings

Native Python bindings for the `tstring-structured-data` backend family.

Requires Python 3.14+.

This package is a native PyO3 extension. Release automation currently publishes
wheels for Linux x86_64 GNU, macOS Apple Silicon, and Windows x86_64. Other
environments require a local Rust 1.94.0 toolchain build.

## Public API

The public Python import is `tstring_bindings`.

Supported public functions:

- `render_json(template, profile="rfc8259")`
- `render_json_text(template, profile="rfc8259")`
- `render_toml(template, profile="1.1")`
- `render_toml_text(template, profile="1.1")`
- `render_yaml(template, profile="1.2.2")`
- `render_yaml_text(template, profile="1.2.2")`

Exported profile aliases:

- `JsonProfile = Literal["rfc8259"]`
- `TomlProfile = Literal["1.0", "1.1"]`
- `YamlProfile = Literal["1.2.2"]`

Unknown profile strings raise `ValueError` in the public Python wrapper layer.
The Rust extension also rejects unsupported profile strings defensively.

## Internal Surface

This package also ships the extension submodule
`tstring_bindings.tstring_bindings`, which is used internally by:

- `json-tstring`
- `toml-tstring`
- `yaml-tstring`
- `tstring-core`

That extension submodule is retained for packaging compatibility and internal
imports, but it is not part of the public contract. Its underscore
result-payload helpers remain private implementation details for the wrapper
packages.

## See also

- [Project README](https://github.com/koxudaxi/tstring-structured-data#readme)
- [Architecture](https://github.com/koxudaxi/tstring-structured-data/blob/main/docs/architecture.md)
