# Architecture

## Overview

[PEP 750](https://peps.python.org/pep-0750/) template strings for JSON, TOML, and YAML.
The template is parsed into an AST first, interpolation values are validated and inserted into slots in the AST, then the AST is rendered to text or materialized to Python objects. This parse-first approach prevents structurally invalid output and injection by construction.

```python
from yaml_tstring import render_data

name = "api"
replicas = 3
data = render_data(t"""\
service:
  name: {name}
  replicas: {replicas}
""")
# => {"service": {"name": "api", "replicas": 3}}
```

## Layers

```
┌─────────────────────────────────────────────────────────┐
│  Python API                                             │
│  json-tstring / toml-tstring / yaml-tstring             │
│  render_data() / render_text() / render_result()        │
└──────────────────────┬──────────────────────────────────┘
                       │ PEP 750 Template
┌──────────────────────▼──────────────────────────────────┐
│  Python Runtime (tstring-core)                          │
│  Tokenization, slots, error types, diagnostics          │
└──────────────────────┬──────────────────────────────────┘
                       │ Tokens + Python values
┌──────────────────────▼──────────────────────────────────┐
│  PyO3 Bindings                                          │
│  tstring-pyo3-bindings → python-bindings                │
│  BoundTemplate, LRU cache, pythonize                    │
└──────────────────────┬──────────────────────────────────┘
                       │ TemplateInput + Values
┌──────────────────────▼──────────────────────────────────┐
│  Rust Parsers                                           │
│  tstring-json / tstring-toml / tstring-yaml             │
│  ↕                                                      │
│  tstring-syntax (spans, diagnostics, NormalizedValue)   │
└─────────────────────────────────────────────────────────┘
```

### Python API (json-tstring, toml-tstring, yaml-tstring)

Thin wrappers over the Rust bindings. Validate the input is a `Template`, check the profile string, call into Rust, return the result. All three packages expose the same shape:

```python
render_data(template, profile=...) -> FormatValue
render_text(template, profile=...) -> str
render_result(template, profile=...) -> RenderResult[FormatValue]
```

### Python Runtime (tstring-core)

Shared across formats:

| Module | What it does |
| --- | --- |
| `_tokens.py` | Split templates into static text + interpolation tokens |
| `_slots.py` | Interpolation context enum (value, key, string_fragment) |
| `_errors.py` | Re-exports from `tstring_bindings` |
| `_spans.py` | Source positions and spans |
| `_diagnostics.py` | Diagnostic severity and messages |
| `_values.py` | `RenderResult` wrapper |
| `_types.py` | Type aliases (`JsonValue`, `TomlValue`, `YamlValue`) |

### PyO3 Bindings

Two crates:

- **tstring-pyo3-bindings** (internal) — `BoundTemplate` captures Python values; per-format modules (`json.rs`, `yaml.rs`, `toml.rs`) handle marshaling; converts `BackendError` to Python exceptions.
- **python-bindings** (public, ships as `tstring_bindings`) — LRU parse cache (256 entries per format), `pythonize` for `NormalizedValue` → Python, contract version check at import.

The wrapper packages validate an extension contract at import:

```python
__contract_version__ = 1
__contract_symbols__ = [
    "TemplateError", "TemplateParseError", "TemplateSemanticError",
    "UnrepresentableValueError",
    "render_json", "render_json_text", "_render_json_result_payload",
    "render_toml", "render_toml_text", "_render_toml_result_payload",
    "render_yaml", "render_yaml_text", "_render_yaml_result_payload",
]
```

### Rust Parsers

**tstring-syntax** defines shared types:

```rust
struct SourceSpan { start: SourcePosition, end: SourcePosition }

enum TemplateSegment { StaticText(String), Interpolation(TemplateInterpolation) }

// Format-agnostic normalized form — all parsers produce this
enum NormalizedValue {
    Null, Bool(bool), Integer(BigInt), Float(NormalizedFloat),
    String(String), Temporal(NormalizedTemporal),
    Sequence(Vec<NormalizedValue>), Mapping(Vec<NormalizedEntry>),
    Set(Vec<NormalizedKey>),
}

enum ErrorKind { Parse, Semantic, Unrepresentable }
```

Format parsers build interpolation-aware ASTs and normalize via third-party libraries:

| Crate | Spec | Normalization | AST Nodes |
| --- | --- | --- | --- |
| tstring-json | RFC 8259 | `serde_json` | Object, Array, String, Literal |
| tstring-toml | TOML 1.0/1.1 | `toml` | Document, Table, Array, Assignment |
| tstring-yaml | YAML 1.2.2 | `saphyr` | Stream, Document, Scalar, Mapping, Sequence |

## Data Flow

```
Template (Python t-string)
  → tokenize into static text + interpolations
  → Rust parser: build format-specific AST with interpolation slots
  → semantic analysis: validate + insert values into slots
  → normalize to NormalizedValue
  → render_data(): pythonize to Python objects
  → render_text(): serialize back to text
```

## Project Layout

```
rust/
  tstring-core-rs/            tstring-syntax crate (shared primitives)
  json-tstring-rs/            JSON parser
  yaml-tstring-rs/            YAML parser
  toml-tstring-rs/            TOML parser
  tstring-pyo3-bindings/      internal PyO3 adapter
  python-bindings/            public extension module
tstring-core/                 Python shared runtime
json-tstring/                 Python JSON package
toml-tstring/                 Python TOML package
yaml-tstring/                 Python YAML package
conformance/                  spec-map manifests and test fixtures
examples/                     runnable examples
```

## Design Decisions

### Why parse first?

String concatenation can produce syntactically broken output and is vulnerable to injection. Parsing the template structure before interpolation makes both problems impossible — invalid output cannot be constructed.

### Normalized intermediate representation

All three parsers normalize to `NormalizedValue` before handing off to Python. This keeps each parser independent while sharing the serialization path. `BigInt` preserves integer precision; `NormalizedTemporal` handles date/time types uniformly.

### Profiles

Format versions are expressed as profile strings (`"rfc8259"`, `"1.0"`, `"1.1"`, `"1.2.2"`). Adding a new spec version means adding a profile, not changing existing behavior.

| Format | Profiles | Default |
| --- | --- | --- |
| JSON | `rfc8259` | `rfc8259` |
| TOML | `1.0`, `1.1` | `1.1` |
| YAML | `1.2.2` | `1.2.2` |

### Parse cache

Template static structure (`template.strings`) is the cache key, so the same template with different interpolation values reuses the parsed AST. 256-entry LRU per format.

### Error model

```
TemplateError
├── TemplateParseError
├── TemplateSemanticError
└── UnrepresentableValueError
```

Errors carry `Diagnostic` objects with source spans and expression labels.

## Dependencies

### Python

```
json-tstring ──┐
toml-tstring ──┼──▶ tstring-core ──▶ tstring_bindings
yaml-tstring ──┘
```

### Rust

```
python-bindings
  └─ tstring-pyo3-bindings
       ├─ tstring-json ─── tstring-syntax + serde_json
       ├─ tstring-toml ─── tstring-syntax + toml
       └─ tstring-yaml ─── tstring-syntax + saphyr
```

Notable versions: pyo3 0.25.1, pythonize 0.25.0, serde_json 1.0.145 (arbitrary_precision), saphyr 0.0.6, toml 0.9.8, num-bigint 0.4.6.

## Interpolation Contexts

| Context | Example |
| --- | --- |
| `value` — whole value | `{"key": {val}}` |
| `key` — map key | `{key}: value` (YAML), `[{header}]` (TOML) |
| `string_fragment` — inside a string | `"hello {name}"` |

Allowed types differ per context; the parser validates this.

## Testing

Conformance tests are driven by shared manifests under `conformance/<format>/profiles/<profile>/spec-map.toml`. Both Python (`pytest`) and Rust (`cargo test`) runners use the same manifests.

Additional test layers: PEP 750 binding regressions, single-pipeline smoke tests, format-specific feature tests.

```bash
uv run --group dev pytest
PYO3_PYTHON="$PWD/.venv/bin/python3" cargo test --manifest-path rust/Cargo.toml --all
```

## Build & CI

- **Python build**: uv workspace + maturin for Rust extension
- **Rust build**: Cargo workspace
- **Lint**: ruff (Python), cargo fmt + clippy (Rust)
- **Type check**: ty

| Workflow | Trigger | What it does |
| --- | --- | --- |
| `ci.yml` | push, PR | test, lint, type check, build, twine check |
| `publish-python.yml` | tag push | wheel builds (Linux/macOS/Windows) → PyPI |
| `publish-rust.yml` | tag push | crates.io |

Distributed as five packages: `tstring-core` (pure Python), `json-tstring`, `toml-tstring`, `yaml-tstring`, and `tstring_bindings` (platform-specific wheel).

## Format Constraints

**JSON** — keys must be `str`; rejects `inf`/`nan`; integers keep exact text (no float coercion).

**TOML** — no `None` (TOML has no null); rejects offset-aware `time`; 64-bit signed integer range. 1.1 adds inline table newlines, trailing commas, `\e`/`\xHH` escapes, times without seconds.

**YAML** — rejects `inf`/`nan`; anchor/tag fragments must be non-empty and whitespace-free; supports multi-document streams, anchors, aliases, tags, directives.
