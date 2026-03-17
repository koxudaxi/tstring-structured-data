"""Realistic example: Docker Compose service definitions as YAML.

Builder functions return ``Template`` objects.  The caller renders
with ``render_data()`` or ``render_text()`` at the point of use.
"""

from __future__ import annotations

from string.templatelib import Template

from _display import print_walkthrough
from yaml_tstring import render_result


def compose_service_template(
    *,
    service_name: str,
    image: str,
    image_tag: str,
    host_port: int,
    container_port: int,
    env_vars: dict[str, str],
    depends_on: list[str],
    replicas: int,
) -> Template:
    """Return a Docker Compose web service definition as a reusable Template."""
    return t"""\
services:
  {service_name}:
    image: "{image}:{image_tag}"
    ports:
      - "{host_port}:{container_port}"
    environment: {env_vars}
    depends_on: {depends_on}
    deploy:
      replicas: {replicas}
      restart_policy:
        condition: "on-failure"
"""


def main() -> None:
    tmpl = compose_service_template(
        service_name="api",
        image="ghcr.io/myorg/api-server",
        image_tag="v2.3.0",
        host_port=8080,
        container_port=8080,
        env_vars={"LOG_LEVEL": "info", "WORKERS": "4"},
        depends_on=["postgres", "redis"],
        replicas=3,
    )

    result = render_result(tmpl)

    print_walkthrough(
        title="YAML – Docker Compose",
        template=tmpl,
        result=result,
        notes=[
            "compose_service_template() returns a Template, not rendered text.",
            "Quoted fragments build image ref and port mapping strings.",
            "Dict env_vars renders as a YAML mapping.",
            "List depends_on renders as a YAML sequence.",
        ],
    )


if __name__ == "__main__":
    main()
