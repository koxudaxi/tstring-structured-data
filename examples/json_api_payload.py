"""Tutorial example: build a JSON API payload from a PEP 750 template."""

from __future__ import annotations

from _display import print_walkthrough
from json_tstring import render_result


def main() -> None:
    account_id = "acct-001"
    display_name = "Ada Lovelace"
    first_role = "admin"
    roles = ["admin", "editor"]
    feature_flags = {"beta_dashboard": True, "audit_log": True}
    trace_id = "req-2026-03-14"

    profile = {
        "name": display_name,
        "roles": roles,
        "features": feature_flags,
    }

    template = t"""\
{{
  "account-{account_id}": {{
    "profile": {profile},
    "summary": "{display_name}-{first_role}",
    "trace": "trace-{trace_id}",
    "status": active-{first_role}
  }}
}}
"""

    result = render_result(template)

    print_walkthrough(
        title="JSON",
        template=template,
        result=result,
        notes=[
            "The dynamic account id is used in a JSON key position.",
            "Nested dict/list values are rendered as native JSON.",
            'String fragments such as "{display_name}-{first_role}" stay readable.',
            'Bare scalar assembly such as "active-{first_role}" becomes a JSON string.',
        ],
    )


if __name__ == "__main__":
    main()
