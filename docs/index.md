# T-strings for Structured Data

[![CI](https://github.com/koxudaxi/tstring-structured-data/actions/workflows/ci.yml/badge.svg)](https://github.com/koxudaxi/tstring-structured-data/actions/workflows/ci.yml)
[![PyPI - json-tstring](https://img.shields.io/pypi/v/json-tstring?label=json-tstring)](https://pypi.org/project/json-tstring/)
[![PyPI - toml-tstring](https://img.shields.io/pypi/v/toml-tstring?label=toml-tstring)](https://pypi.org/project/toml-tstring/)
[![PyPI - yaml-tstring](https://img.shields.io/pypi/v/yaml-tstring?label=yaml-tstring)](https://pypi.org/project/yaml-tstring/)
[![Python 3.14+](https://img.shields.io/badge/python-3.14%2B-blue)](https://docs.python.org/3/whatsnew/3.14.html)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](https://github.com/koxudaxi/tstring-structured-data/blob/main/LICENSE)

**Parser-first JSON, TOML, and YAML backends for [PEP 750](https://peps.python.org/pep-0750/) template strings.**

T-strings (introduced in Python 3.14) give you f-string convenience with structured access to interpolation values via [`string.templatelib.Template`](https://docs.python.org/3/library/string.templatelib.html). This project builds on that: write templates that look like the target format, and get validated text or parsed Python data back.

## How it works

Templates are **parsed into an AST first**, interpolation values are validated and inserted into slots in the AST, then the AST is rendered to text or materialized to Python objects. This parse-first approach prevents structurally invalid output and injection by construction.

```python
from yaml_tstring import render_data

name = "api"
replicas = 3
labels = {"app": "api", "team": "platform"}

data = render_data(t"""\
service:
  name: {name}
  replicas: {replicas}
  labels: {labels}
""")
# {'service': {'name': 'api', 'replicas': 3,
#              'labels': {'app': 'api', 'team': 'platform'}}}
```

## Packages

Pick the format you need:

| Package | Format | Install |
|---------|--------|---------|
| **[json-tstring](https://pypi.org/project/json-tstring/)** | JSON | `pip install json-tstring` |
| **[toml-tstring](https://pypi.org/project/toml-tstring/)** | TOML | `pip install toml-tstring` |
| **[yaml-tstring](https://pypi.org/project/yaml-tstring/)** | YAML | `pip install yaml-tstring` |

`tstring-core` (shared runtime) and `tstring-bindings` (native extension) are pulled in automatically.

## Key features

- **Injection-safe** — values are inserted into the AST, not concatenated into strings
- **Type-preserving** — Python dicts, lists, ints, and floats render as native format values
- **Dual output** — `render_data()` returns Python objects, `render_text()` returns formatted text
- **Profile-based** — select spec versions (`rfc8259`, `1.0`/`1.1`, `1.2.2`) per call
- **Parse cache** — same template structure reuses parsed AST (256-entry LRU per format)

## Next steps

- [Installation](getting-started/installation.md) — install your backend
- [Quick Start](getting-started/quick-start.md) — basic examples for each format
- [Architecture](architecture.md) — how the parser-first pipeline works
