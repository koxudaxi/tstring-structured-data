"""Tutorial example: build a YAML release manifest from a PEP 750 template."""

from __future__ import annotations

from _display import print_walkthrough
from yaml_tstring import render_result


def main() -> None:
    app_name = "checkout"
    environment = "staging"
    defaults_anchor = "base"
    startup_tag = "str"
    replicas = 3

    template = t"""\
defaults: &{defaults_anchor}
  image: "ghcr.io/example/{app_name}:1.2.3"
  replicas: {replicas}
  startup_message: |
    Starting {app_name}
    in {environment}

service:
  defaults_copy: *{defaults_anchor}
  name: "{app_name}-{environment}"
  mode: !{startup_tag} rolling
"""

    result = render_result(template)

    print_walkthrough(
        title="YAML",
        template=template,
        result=result,
        notes=[
            "The anchor name is interpolated once and reused through an alias.",
            "The release name uses quoted-string fragment interpolation.",
            "The startup message uses YAML block-scalar assembly.",
            "The local tag comes from static punctuation plus an interpolated suffix.",
        ],
    )


if __name__ == "__main__":
    main()
