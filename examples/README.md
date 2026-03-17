# Examples

Runnable examples for the JSON, TOML, and YAML t-string backends.

| Example | Format | What it shows |
|---------|--------|---------------|
| `json_api_payload.py` | JSON | Basic interpolation — keys, values, nested dicts/lists, string fragments |
| `toml_app_config.py` | TOML | Table headers, dotted keys, multiline strings, datetime/date/time |
| `yaml_release_manifest.py` | YAML | Anchors, aliases, tags, block scalars |
| `yaml_docker_compose.py` | YAML | Template-returning builder functions for reusable configs |
| `json_validation_errors.py` | JSON | Error detection and injection safety (f-string vs t-string) |

## How to run

Requires Python 3.14 and `uv`.

```bash
# JSON examples
cd json-tstring
PYTHONPATH="$PWD/src:../tstring-core/src" uv run --group dev python ../examples/json_api_payload.py
PYTHONPATH="$PWD/src:../tstring-core/src" uv run --group dev python ../examples/json_validation_errors.py

# TOML examples
cd toml-tstring
PYTHONPATH="$PWD/src:../tstring-core/src" uv run --group dev python ../examples/toml_app_config.py

# YAML examples
cd yaml-tstring
PYTHONPATH="$PWD/src:../tstring-core/src" uv run --group dev python ../examples/yaml_release_manifest.py
PYTHONPATH="$PWD/src:../tstring-core/src" uv run --group dev python ../examples/yaml_docker_compose.py
```
