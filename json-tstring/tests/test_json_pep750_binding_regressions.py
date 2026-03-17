from __future__ import annotations

from string.templatelib import Template
from typing import Any

import pytest

# ruff: noqa: E501
import tstring_core

from json_tstring import TemplateParseError as JsonTemplateParseError
from json_tstring import render_data as render_json_data
from json_tstring import render_result as render_json_result
from json_tstring import render_text as render_json_text


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


def _assert_exception_metadata(exc: Any, *, expected_code: str, has_span: bool) -> None:
    assert exc.code == expected_code
    assert exc.span == exc.diagnostics[0]["span"]
    assert isinstance(exc.diagnostics, tuple)
    assert exc.diagnostics
    assert exc.diagnostics[0]["code"] == expected_code
    assert exc.diagnostics[0]["message"] == str(exc)
    assert exc.diagnostics[0]["severity"] == "error"
    assert isinstance(exc.diagnostics[0]["metadata"], dict)
    if has_span:
        assert exc.span is not None
    else:
        assert exc.span is None


def test_json_pep750_metadata_is_applied() -> None:
    value = 3.14159
    dual = DualRenderValue(repr_text="4", str_text="3")
    non_ascii = NonAsciiReprValue()

    template = t'{{"format": {value:.2f}, "repr": {dual!r}, "str": {dual!s}, "ascii": "{non_ascii!a}", "fragment": "pi={value:.2f}"}}'

    assert render_json_text(template) == (
        '{"format": 3.14, "repr": 4, "str": 3, "ascii": "cafe\\\\xe9", '
        '"fragment": "pi=3.14"}'
    )
    assert render_json_data(template) == {
        "format": 3.14,
        "repr": 4,
        "str": 3,
        "ascii": "cafe\\xe9",
        "fragment": "pi=3.14",
    }


def test_json_plain_interpolations_keep_structured_insertion_behavior() -> None:
    mapping = {"nested": [1, 2]}
    items = [1, 2]
    enabled = True
    count = 7
    ratio = 2.5

    assert render_json_data(
        t'{{"mapping": {mapping}, "items": {items}, "enabled": {enabled}, "count": {count}, "ratio": {ratio}}}'
    ) == {
        "mapping": {"nested": [1, 2]},
        "items": [1, 2],
        "enabled": True,
        "count": 7,
        "ratio": 2.5,
    }


def test_json_invalid_formatted_output_is_rejected_on_the_single_runtime_path() -> None:
    broken = BrokenRenderedValue()

    with pytest.raises(JsonTemplateParseError):
        render_json_text(t'{{"value": {broken!r}}}')


def test_json_metadata_path_materialization_reports_parse_errors_consistently() -> None:
    broken = BrokenRenderedValue()

    with pytest.raises(JsonTemplateParseError) as data_info:
        render_json_data(t"{broken!r}")
    _assert_exception_metadata(
        data_info.value, expected_code="json.parse", has_span=True
    )

    with pytest.raises(JsonTemplateParseError) as result_info:
        render_json_result(t'{{"value": {broken!r}}}')
    _assert_exception_metadata(
        result_info.value, expected_code="json.parse", has_span=True
    )


def test_json_binding_errors_expose_structured_diagnostics() -> None:
    with pytest.raises(JsonTemplateParseError) as parse_info:
        render_json_text(Template('{"value": ]'))
    _assert_exception_metadata(
        parse_info.value, expected_code="json.parse", has_span=True
    )


def test_tstring_core_deprecated_root_helpers_remain_compatible() -> None:
    import tstring_core._render as render_helpers

    assert "ParserFirstBackend" not in tstring_core.__all__
    assert "ReturnMode" not in tstring_core.__all__
    assert "render_with_backend" not in tstring_core.__all__

    with pytest.warns(DeprecationWarning, match="tstring_core.ParserFirstBackend"):
        assert tstring_core.ParserFirstBackend is render_helpers.ParserFirstBackend
    with pytest.warns(DeprecationWarning, match="tstring_core.ReturnMode"):
        assert tstring_core.ReturnMode == render_helpers.ReturnMode
    with pytest.warns(DeprecationWarning, match="tstring_core.render_with_backend"):
        assert tstring_core.render_with_backend is render_helpers.render_with_backend
