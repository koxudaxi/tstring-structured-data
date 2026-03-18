# Quick Start

Every backend exposes three functions with the same interface:

| Function | Returns |
|----------|---------|
| `render_data(template)` | Parsed Python objects (`dict`, `list`, `str`, `int`, etc.) |
| `render_text(template)` | Formatted text (`str`) valid in the target format |
| `render_result(template)` | `RenderResult` with both `.data` and `.text` |

## YAML example

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

# Parsed Python data
data = render_data(template)
# {'service': {'name': 'api', 'replicas': 3, ...}}

# Valid YAML text
text = render_text(template)
# service:
#   name: "api"
#   replicas: 3
#   ...
```

## JSON example

```python
from json_tstring import render_data, render_text

user = "Ada Lovelace"
roles = ["admin", "editor"]
features = {"beta_dashboard": True, "audit_log": True}

template = t"""\
{
  "user": {user},
  "roles": {roles},
  "features": {features}
}
"""

data = render_data(template)
# {'user': 'Ada Lovelace', 'roles': ['admin', 'editor'],
#  'features': {'beta_dashboard': True, 'audit_log': True}}

text = render_text(template)
# {
#   "user": "Ada Lovelace",
#   "roles": ["admin", "editor"],
#   ...
# }
```

## TOML example

```python
from toml_tstring import render_data, render_text

service_name = "billing"
owner = "platform-team"
retries = [1, 2, 5]

template = t"""\
[services.{service_name}]
owner = {owner}

[services.{service_name}.retry]
schedule = {retries}
"""

data = render_data(template)
# {'services': {'billing': {'owner': 'platform-team',
#               'retry': {'schedule': [1, 2, 5]}}}}

text = render_text(template)
# [services.billing]
# owner = "platform-team"
# ...
```

## See also

- [JSON](../usage/json.md), [TOML](../usage/toml.md), [YAML](../usage/yaml.md) usage
- [Error Handling](../usage/error-handling.md)
- [API Reference](../reference/api.md)
