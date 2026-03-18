# Installation

Requires Python 3.14+ ([PEP 750](https://peps.python.org/pep-0750/) t-strings).

## Install a backend

Pick the format you need:

=== "JSON"

    ```bash
    pip install json-tstring
    ```

=== "TOML"

    ```bash
    pip install toml-tstring
    ```

=== "YAML"

    ```bash
    pip install yaml-tstring
    ```

Or with [uv](https://docs.astral.sh/uv/):

=== "JSON"

    ```bash
    uv add json-tstring
    ```

=== "TOML"

    ```bash
    uv add toml-tstring
    ```

=== "YAML"

    ```bash
    uv add yaml-tstring
    ```

Each package automatically pulls in `tstring-core` (shared runtime) and `tstring-bindings` (native extension).

## Platform wheels

Release automation publishes pre-built wheels for:

| Platform | Architecture |
|----------|-------------|
| Linux | x86_64 (GNU) |
| macOS | Apple Silicon (arm64) |
| Windows | x86_64 |

Other environments require a local Rust 1.94.0+ toolchain to build `tstring-bindings` from source.

## Installing multiple backends

You can install any combination of backends in the same environment:

```bash
pip install json-tstring toml-tstring yaml-tstring
```

They share the same `tstring-core` and `tstring-bindings` dependencies, so there is no conflict.
