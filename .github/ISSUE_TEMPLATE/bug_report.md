---
name: Bug report
about: Report a problem in the JSON, TOML, YAML, or bindings layers
title: ""
labels: bug
assignees: ""
---

## Summary

Describe the bug clearly and concisely.

## Reproduction

Provide a minimal template and the exact call that triggers the issue.

```python
from json_tstring import render_text

value = "example"
print(render_text(t'{"value": {value}}'))
```

## Expected behavior

Describe what you expected to happen.

## Environment

- OS:
- Python version:
- Package version(s):
- Backend/profile:

## Additional context

Include traceback output, rendered text, or links to related issues if helpful.

