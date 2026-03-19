# Editor Integration (t-linter)

[t-linter](https://github.com/koxudaxi/t-linter) is a linter, formatter, and LSP server for Python template strings (PEP 750 t-strings). It uses the same Rust backends (`tstring-json`, `tstring-toml`, `tstring-yaml`) as this project for `check` and `format` operations.

## Installation

```bash
pip install t-linter
```

Or with uv:

```bash
uv add t-linter
```

## CLI usage

### Check templates for errors

Validate t-string templates in your Python files:

```bash
t-linter check file.py
t-linter check src/
```

Output format options:

```bash
t-linter check file.py --format human    # default, human-readable
t-linter check file.py --format json     # machine-readable JSON
t-linter check file.py --format github   # GitHub Actions annotations
```

Use `--error-on-issues` to fail CI when problems are found:

```bash
t-linter check src/ --error-on-issues
```

### Format templates

Canonical formatting for JSON, YAML, and TOML template literals:

```bash
t-linter format file.py
t-linter format src/
```

Dry-run to see what would change without modifying files:

```bash
t-linter format --check file.py
```

Read from stdin:

```bash
cat file.py | t-linter format --stdin-filename file.py -
```

## LSP server

Start the built-in LSP server for real-time editor diagnostics and formatting:

```bash
t-linter lsp
```

The LSP provides:

- **Diagnostics** — syntax and semantic errors reported inline as you type
- **Formatting** — format-on-save or format-on-demand via your editor's formatting command

## VSCode integration

1. Install the binary:

    ```bash
    pip install t-linter
    ```

2. Install the [t-linter extension](https://marketplace.visualstudio.com/items?itemName=koxudaxi.t-linter) from the VSCode marketplace.

3. Recommended: set `"python.languageServer": "None"` in VSCode settings to avoid conflicts with other Python language servers.

4. Optionally configure `t-linter.serverPath` in settings if the binary is not on your `PATH`.

## Other editors

Any editor that supports the Language Server Protocol can use `t-linter lsp`. Configure your editor to start `t-linter lsp` as the language server for Python files.

## Configuration

Configure t-linter via `pyproject.toml`:

```toml
[tool.t-linter]
extend-exclude = ["generated", "vendor"]
ignore-file = ".t-linterignore"
```

## How it works with tstring-structured-data

t-linter and tstring-structured-data share the same Rust parsing and formatting backends:

- **tstring-structured-data** is the **runtime library** — `render_data()` and `render_text()` parse and render templates at runtime
- **t-linter** is the **developer tooling** — `check` validates templates and `format` canonicalizes them at development time

For JSON, YAML, and TOML templates, t-linter uses:

- **Tree-sitter** for fast syntax highlighting (low-latency, no full parse)
- **Rust backends** (`tstring-json`, `tstring-yaml`, `tstring-toml`) for strict `check` and `format` operations
