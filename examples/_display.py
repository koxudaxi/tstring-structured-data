"""Helpers for presenting tutorial examples in a source-like way."""

from __future__ import annotations

from string.templatelib import Template
from typing import Iterable

from tstring_core import RenderResult


def template_source(template: Template) -> str:
    parts = [template.strings[0]]

    for interpolation, trailing in zip(
        template.interpolations,
        template.strings[1:],
        strict=False,
    ):
        expression = interpolation.expression or "..."
        if interpolation.conversion:
            expression += f"!{interpolation.conversion}"
        if interpolation.format_spec:
            expression += f":{interpolation.format_spec}"

        parts.append("{" + expression + "}")
        parts.append(trailing)

    return "".join(parts)


def print_walkthrough(
    *,
    title: str,
    template: Template,
    result: RenderResult | None = None,
    rendered: str | None = None,
    data: object | None = None,
    notes: Iterable[str],
) -> None:
    if result is not None:
        rendered = result.text
        data = result.data

    if rendered is None or data is None:
        raise TypeError(
            "print_walkthrough() requires either result or both rendered and data."
        )

    print(f"{title} template:")
    print(template_source(template))

    print(f"\nRendered {title} text:")
    print(rendered)

    print("\nParsed Python data:")
    from pprint import pprint

    pprint(data, sort_dicts=False)

    print("\nWhat to notice:")
    for note in notes:
        print(f"- {note}")
