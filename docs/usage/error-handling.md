# Error Handling

T-string backends catch errors that f-strings silently ignore and block injection by construction.

## Validation errors

The full example demonstrates three kinds of validation errors and injection prevention:

```python
--8<-- "examples/json_validation_errors.py"
```

## Error hierarchy

All backends share the same error hierarchy from `tstring-bindings`:

```
TemplateError
├── TemplateParseError       — template syntax is invalid
├── TemplateSemanticError    — valid syntax but invalid semantics (e.g., wrong key type)
└── UnrepresentableValueError — Python value can't be represented in the target format
```

Errors carry `Diagnostic` objects with source spans and expression labels for precise error reporting.

## Common errors

### Non-serializable values

```python
from json_tstring import render_text

conn = SomeObject()
render_text(t'{{"connection": {conn}}}')
# => UnrepresentableValueError
```

F-strings would silently produce `repr()` output — invalid JSON.

### Invalid key types

```python
from json_tstring import render_text

key = 42
render_text(t'{{{key}: "value"}}')
# => TemplateSemanticError
```

JSON keys must be strings. T-strings enforce this at render time.

### Non-finite floats

```python
from json_tstring import render_text

value = float("inf")
render_text(t'{{"metric": {value}}}')
# => UnrepresentableValueError
```

JSON (RFC 8259) forbids `Infinity` and `NaN`. This library also rejects them for YAML to keep output portable.

## Injection prevention

T-strings prevent injection the same way SQL parameterized queries prevent SQL injection:

```python
import json
from json_tstring import render_text

# Malicious input
user_input = 'admin", "role": "superuser'

# f-string: VULNERABLE — attacker overrides the role
fstring = f'{{"role": "viewer", "username": "{user_input}"}}'
json.loads(fstring).get("role")  # => "superuser" (injected!)

# t-string: SAFE — value is properly escaped
tstring = render_text(t'{{"role": "viewer", "username": {user_input}}}')
json.loads(tstring).get("role")  # => "viewer" (correct)
```

Values are inserted into the parsed AST, not concatenated into strings, so the attacker cannot break out of the value slot.
