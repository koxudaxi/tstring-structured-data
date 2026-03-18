# JSON

The `json-tstring` package provides `render_data`, `render_text`, and `render_result` for JSON templates.

## API payload example

This example builds a JSON API payload with dynamic keys, nested structures, and string fragment interpolation:

```python
--8<-- "examples/json_api_payload.py"
```

### What to notice

- The dynamic account id is used in a JSON **key position**: `"account-{account_id}"`
- Nested `dict`/`list` values are rendered as native JSON objects and arrays
- String fragments like `"{display_name}-{first_role}"` stay readable
- Bare scalar assembly like `active-{first_role}` becomes a JSON string

## Interpolation contexts

| Context | Example | Description |
|---------|---------|-------------|
| Whole value | `{"key": {val}}` | Any JSON-serializable Python value |
| Object key | `{key}: ...` | Must be `str` |
| String fragment | `"hello {name}"` | Inserted inside a quoted string |

## Supported types

Values passed to JSON interpolation slots must be JSON-serializable:

- `str`, `int`, `float`, `bool`, `None`
- `list`, `tuple` (rendered as JSON arrays)
- `dict` (rendered as JSON objects)

JSON rejects `float("inf")`, `float("nan")`, and non-string keys.

## Profile

JSON currently supports one profile:

| Profile | Description | Default |
|---------|-------------|---------|
| `rfc8259` | RFC 8259 JSON | Yes |

```python
from json_tstring import render_data

data = render_data(t'{"key": {value}}', profile="rfc8259")
```
