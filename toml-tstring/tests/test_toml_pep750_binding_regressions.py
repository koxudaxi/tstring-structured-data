from __future__ import annotations

# ruff: noqa: E501
from datetime import UTC, datetime
from string.templatelib import Template
from typing import Annotated, get_args, get_origin, get_type_hints

import pytest

from toml_tstring import TemplateParseError, render_data, render_result, render_text


def _template_hint_value(hint: object) -> object:
    return getattr(hint, "__value__", hint)


class DualRenderValue:
    def __init__(self, *, repr_text: str, str_text: str) -> None:
        self._repr_text = repr_text
        self._str_text = str_text

    def __repr__(self) -> str:
        return self._repr_text

    def __str__(self) -> str:
        return self._str_text


class NonAsciiReprValue:
    def __repr__(self) -> str:
        return "cafe\u00e9"


class BrokenRenderedValue:
    def __repr__(self) -> str:
        return "[1, 2"


def test_toml_pep750_metadata_is_applied() -> None:
    value = 3.14159
    dual = DualRenderValue(repr_text="4", str_text="3")
    non_ascii = NonAsciiReprValue()

    template = t'format = {value:.2f}\nrepr = {dual!r}\nstr = {dual!s}\nascii = "{non_ascii!a}"\nfragment = "pi={value:.2f}"\n'

    assert render_text(template) == (
        'format = 3.14\nrepr = 4\nstr = 3\nascii = "cafe\\\\xe9"\nfragment = "pi=3.14"'
    )
    assert render_data(template) == {
        "format": 3.14,
        "repr": 4,
        "str": 3,
        "ascii": "cafe\\xe9",
        "fragment": "pi=3.14",
    }


def test_toml_plain_interpolations_keep_structured_insertion_behavior() -> None:
    mapping = {"nested": [1, 2]}
    items = [1, 2]
    enabled = True
    count = 7
    ratio = 2.5
    moment = datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC)

    assert render_data(
        t"mapping = {mapping}\nitems = {items}\nenabled = {enabled}\ncount = {count}\nratio = {ratio}\nmoment = {moment}\n"
    ) == {
        "mapping": {"nested": [1, 2]},
        "items": [1, 2],
        "enabled": True,
        "count": 7,
        "ratio": 2.5,
        "moment": moment,
    }


def test_toml_invalid_formatted_output_is_rejected_on_the_single_runtime_path() -> None:
    broken = BrokenRenderedValue()

    with pytest.raises(TemplateParseError):
        render_text(t"value = {broken!r}\n")


def test_toml_metadata_path_materialization_reports_parse_errors_consistently() -> None:
    broken = BrokenRenderedValue()

    with pytest.raises(TemplateParseError, match="invalid formatted TOML payload"):
        render_data(t"value = {broken!r}\n")

    with pytest.raises(TemplateParseError, match="invalid formatted TOML payload"):
        render_result(t"value = {broken!r}\n")


def test_toml_render_apis_expose_annotated_template_parameters() -> None:
    for render_api in (render_data, render_text, render_result):
        template_hint = _template_hint_value(
            get_type_hints(render_api, include_extras=True)["template"]
        )
        assert get_origin(template_hint) is Annotated
        assert get_args(template_hint) == (Template, "toml")
