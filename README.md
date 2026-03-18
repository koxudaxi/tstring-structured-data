# T-strings for Structured Data

[![CI](https://github.com/koxudaxi/tstring-structured-data/actions/workflows/ci.yml/badge.svg)](https://github.com/koxudaxi/tstring-structured-data/actions/workflows/ci.yml)
[![PyPI - json-tstring](https://img.shields.io/pypi/v/json-tstring?label=json-tstring)](https://pypi.org/project/json-tstring/)
[![PyPI - toml-tstring](https://img.shields.io/pypi/v/toml-tstring?label=toml-tstring)](https://pypi.org/project/toml-tstring/)
[![PyPI - yaml-tstring](https://img.shields.io/pypi/v/yaml-tstring?label=yaml-tstring)](https://pypi.org/project/yaml-tstring/)
[![Python 3.14+](https://img.shields.io/badge/python-3.14%2B-blue)](https://docs.python.org/3/whatsnew/3.14.html)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

Parser-first JSON, TOML, and YAML backends for
[PEP 750 – Template Strings](https://peps.python.org/pep-0750/) t-strings.

T-strings (introduced in Python 3.14) give you f-string convenience with
structured access to interpolation values via
[`string.templatelib.Template`](https://docs.python.org/3/library/string.templatelib.html).
This project builds on that: write templates that look like the target format,
and get validated text or parsed Python data back.

## Packages

Pick the format you need:

| Package | Format |
|---------|--------|
| **json-tstring** | JSON |
| **toml-tstring** | TOML |
| **yaml-tstring** | YAML |

`tstring-core` (shared runtime) and `tstring-bindings` (native extension) are
pulled in automatically — you never need to install them directly.

## Installation

Requires Python 3.14+.

The Python packages depend on `tstring-bindings`, a native extension built with
PyO3. Release automation currently publishes wheels for Linux x86_64 GNU,
macOS Apple Silicon, and Windows x86_64. Other environments require a local
Rust 1.94.0 toolchain build.

Install the backend you need:

```bash
pip install json-tstring
pip install toml-tstring
pip install yaml-tstring
```

Or with uv:

```bash
uv add json-tstring
uv add toml-tstring
uv add yaml-tstring
```

Each package pulls in `tstring-core` and `tstring-bindings` automatically.

## Quick start

```python
from yaml_tstring import render_data, render_text

name = "api"
replicas = 3
labels = {"app": "api", "team": "platform"}
env = {"LOG_LEVEL": "info", "WORKERS": "4"}
ports = [8080, 8443]

template = t"""\
service:
  name: {name}
  replicas: {replicas}
  labels: {labels}
  env: {env}
  ports: {ports}
"""

# render_data: t-string -> parsed Python data
data = render_data(template)
# {'service': {'name': 'api',
#              'replicas': 3,
#              'labels': {'app': 'api', 'team': 'platform'},
#              'env': {'LOG_LEVEL': 'info', 'WORKERS': '4'},
#              'ports': [8080, 8443]}}

# render_text: t-string -> valid YAML text
text = render_text(template)
# service:
#   name: "api"
#   replicas: 3
#   labels:
#     "app": "api"
#     "team": "platform"
#   env:
#     "LOG_LEVEL": "info"
#     "WORKERS": "4"
#   ports:
#     - 8080
#     - 8443
```

JSON and TOML work the same way — `json_tstring.render_data()`,
`toml_tstring.render_text()`, etc.

## Rust backend API

The Rust crates also expose parser-first `check` and `format` entry points for
tools that already tokenize template strings themselves, such as LSP servers and
linters.

```rust
use tstring_json::{check_template, format_template};
use tstring_syntax::{TemplateInput, TemplateInterpolation, TemplateSegment};

let template = TemplateInput::from_segments(vec![
    TemplateSegment::StaticText("{\"name\": ".to_owned()),
    TemplateSegment::Interpolation(TemplateInterpolation {
        expression: "name".to_owned(),
        conversion: None,
        format_spec: String::new(),
        interpolation_index: 0,
        raw_source: Some("{name}".to_owned()),
    }),
    TemplateSegment::StaticText("}".to_owned()),
]);

check_template(&template)?;
let text = format_template(&template)?;
assert_eq!(text, "{\"name\": {name}}");
```

Available on each backend crate:

- `check_template_with_profile(...) -> BackendResult<()>`
- `check_template(...) -> BackendResult<()>`
- `format_template_with_profile(...) -> BackendResult<String>`
- `format_template(...) -> BackendResult<String>`

`format_*` requires every interpolation to include `raw_source`, because the
formatter preserves `{expr!r:spec}` verbatim instead of reconstructing it from
parsed fields. The formatter returns canonical JSON/TOML/YAML text; it does not
preserve comments or original whitespace.

## Setup

Requires Python 3.14, `uv`, and Rust 1.94.0.

```bash
uv sync --group dev
```

## Verify

```bash
cargo test --manifest-path rust/Cargo.toml --workspace --tests
uv run --group dev pytest -q
uv run --group dev ruff check .
uv run --group dev ty check json-tstring/src toml-tstring/src yaml-tstring/src tstring-core/src
```

## Layout

```
rust/           Rust workspace and native bindings
tstring-core/   shared Python runtime
json-tstring/   JSON wrapper
toml-tstring/   TOML wrapper
yaml-tstring/   YAML wrapper
examples/       runnable examples
conformance/    profile manifests and fixture slices
docs/           support matrix and conformance notes
```

## Profiles

Each backend accepts a `profile` argument. Defaults:

- JSON: `rfc8259`
- TOML: `1.1` (also supports `1.0`)
- YAML: `1.2.2`

See [docs/backend-support-matrix.md](docs/backend-support-matrix.md) and
[docs/spec-conformance-status.md](docs/spec-conformance-status.md) for details.

## Publishing

Python and Rust packages are published automatically via GitHub Actions
when a version tag is pushed.

## See also

- [PEP 750 – Template Strings](https://peps.python.org/pep-0750/)
- [`string.templatelib` — Template String Support](https://docs.python.org/3/library/string.templatelib.html)
- [What's New In Python 3.14](https://docs.python.org/3/whatsnew/3.14.html)
