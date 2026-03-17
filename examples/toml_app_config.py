"""Tutorial example: build a TOML application config from a PEP 750 template."""

from __future__ import annotations

from datetime import UTC, date, datetime, time

from _display import print_walkthrough
from toml_tstring import render_result


def main() -> None:
    service_name = "billing"
    owner = "platform-team"
    region = "us-east-1"
    environment = "production"
    launch_at = datetime(2026, 3, 14, 9, 30, tzinfo=UTC)
    business_day = date(2026, 3, 14)
    office_hours = time(9, 30)
    retries = [1, 2, 5]

    template = t'''\
[services.{service_name}]
owner = {owner}
launch_at = {launch_at}
business_day = {business_day}
office_hours = {office_hours}
welcome = """Welcome {owner}
Running in {environment}
Region {region}"""
release_label = {service_name}-{environment}

[services.{service_name}.labels]
{region} = {environment}

[services.{service_name}.retry]
schedule = {retries}
'''

    result = render_result(template)

    print_walkthrough(
        title="TOML",
        template=template,
        result=result,
        notes=[
            "The service name is interpolated into table headers.",
            "The region is interpolated into a TOML key position.",
            "The multiline welcome message starts life as a readable TOML string.",
            "datetime, date, and time values render as TOML-native literals.",
            (
                "Bare scalar assembly such as {service_name}-{environment} "
                "becomes a string."
            ),
        ],
    )


if __name__ == "__main__":
    main()
