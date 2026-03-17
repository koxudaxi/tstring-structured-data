"""Validation and error handling: t-string safety vs f-string fragility.

Demonstrates how t-string backends detect errors that f-strings silently
ignore, plus injection prevention analogous to SQL parameterized queries.
"""

from __future__ import annotations

import json

from json_tstring import render_data, render_text


def main() -> None:
    print("=== Error 1: Non-serializable value ===\n")

    class DatabaseConnection:
        def __init__(self, host: str):
            self.host = host

    conn = DatabaseConnection("db.example.com")
    try:
        render_text(t'{{"connection": {conn}}}')
    except Exception as exc:
        print(f"  {type(exc).__name__}: {exc}")
        print("  f-string would silently produce repr() junk.\n")

    print("=== Error 2: Invalid key type ===\n")

    key = 42
    try:
        render_text(t'{{{key}: "value"}}')
    except Exception as exc:
        print(f"  {type(exc).__name__}: {exc}")
        print("  JSON keys must be strings — t-strings enforce this.\n")

    print("=== Error 3: Non-finite float ===\n")

    value = float("inf")
    try:
        render_text(t'{{"metric": {value}}}')
    except Exception as exc:
        print(f"  {type(exc).__name__}: {exc}")
        print("  JSON forbids Infinity/NaN — t-strings reject them.\n")

    print("=== Injection safety: f-string vs t-string ===\n")

    # Malicious input that closes the string and injects a new key
    user_input = 'admin", "role": "superuser'

    # f-string: vulnerable — attacker overrides the role
    fstring_result = f'{{"role": "viewer", "username": "{user_input}"}}'
    parsed = json.loads(fstring_result)
    print(f"  f-string parsed role: {parsed.get('role')}")
    print("  ^^^ Injection succeeded: attacker controls the role!\n")

    # t-string: safe — value is properly escaped
    tstring_text = render_text(
        t'{{"role": "viewer", "username": {user_input}}}'
    )
    parsed = json.loads(tstring_text)
    print(f"  t-string parsed role: {parsed.get('role')}")
    print(f"  t-string parsed username: {parsed.get('username')}")
    print("  ^^^ Injection blocked: value properly escaped.")


if __name__ == "__main__":
    main()
