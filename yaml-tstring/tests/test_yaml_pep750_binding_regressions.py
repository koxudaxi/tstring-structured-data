from __future__ import annotations

# ruff: noqa: E501
from typing import Any

import pytest

from yaml_tstring import (
    TemplateParseError,
    UnrepresentableValueError,
    render_data,
    render_result,
    render_text,
)


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


class BlockFormattedValue:
    def __str__(self) -> str:
        return "nested:\n  - 1\n  - 2"


class BadStringValue:
    def __str__(self) -> str:
        raise ValueError("cannot stringify")


def _assert_exception_metadata(exc: Any, *, expected_code: str, has_span: bool) -> None:
    assert exc.code == expected_code
    assert exc.span == exc.diagnostics[0]["span"]
    assert isinstance(exc.diagnostics, tuple)
    assert exc.diagnostics[0]["code"] == expected_code
    assert exc.diagnostics[0]["message"] == str(exc)
    assert exc.diagnostics[0]["severity"] == "error"
    assert isinstance(exc.diagnostics[0]["metadata"], dict)
    if has_span:
        assert exc.span is not None
    else:
        assert exc.span is None


def test_yaml_pep750_metadata_is_applied() -> None:
    value = 3.14159
    dual = DualRenderValue(repr_text="4", str_text="3")
    non_ascii = NonAsciiReprValue()

    template = t'format: {value:.2f}\nrepr: {dual!r}\nstr: {dual!s}\nascii: "{non_ascii!a}"\nfragment: "pi={value:.2f}"\n'

    assert render_text(template) == (
        'format: 3.14\nrepr: 4\nstr: 3\nascii: "cafe\\\\xe9"\nfragment: "pi=3.14"'
    )
    assert render_data(template) == {
        "format": 3.14,
        "repr": 4,
        "str": 3,
        "ascii": "cafe\\xe9",
        "fragment": "pi=3.14",
    }


def test_yaml_plain_interpolations_keep_structured_insertion_behavior() -> None:
    mapping = {"nested": [1, 2]}
    items = [1, 2]
    enabled = True
    count = 7
    ratio = 2.5

    assert render_data(
        t"mapping: {mapping}\nitems: {items}\nenabled: {enabled}\ncount: {count}\nratio: {ratio}\n"
    ) == {
        "mapping": {"nested": [1, 2]},
        "items": [1, 2],
        "enabled": True,
        "count": 7,
        "ratio": 2.5,
    }


def test_yaml_plain_collection_interpolations_render_block_first_text() -> None:
    mapping = {"nested": [1, 2]}
    items = [1, 2]
    empty_mapping: dict[str, object] = {}
    empty_list: list[object] = []
    tag = "custom"
    anchor = "root"

    block_mapping = t"value: {mapping}\n"
    block_sequence = t"value: {items}\n"
    root_mapping = t"{mapping}\n"
    root_sequence = t"{items}\n"
    decorated = t"value: !{tag} &{anchor} {mapping}\n"
    flow = t"flow: [{mapping}]\nflow_map: {{k: {items}}}\n"
    empties = t"value_map: {empty_mapping}\nvalue_list: {empty_list}\n"

    assert render_text(block_mapping) == 'value:\n  "nested":\n    - 1\n    - 2'
    assert render_text(block_sequence) == "value:\n  - 1\n  - 2"
    assert render_text(root_mapping) == '"nested":\n  - 1\n  - 2'
    assert render_text(root_sequence) == "- 1\n- 2"
    assert render_text(decorated) == (
        'value: !custom &root\n  "nested":\n    - 1\n    - 2'
    )
    assert render_text(flow) == (
        'flow: [ { "nested": [ 1, 2 ] } ]\nflow_map: { k: [ 1, 2 ] }'
    )
    assert render_text(empties) == "value_map: {}\nvalue_list: []"

    assert render_data(block_mapping) == {"value": {"nested": [1, 2]}}
    assert render_data(block_sequence) == {"value": [1, 2]}
    assert render_data(root_mapping) == {"nested": [1, 2]}
    assert render_data(root_sequence) == [1, 2]
    assert render_data(decorated) == {"value": {"nested": [1, 2]}}

    result = render_result(decorated)
    assert result.text == render_text(decorated)
    assert result.data == render_data(decorated)


def test_yaml_invalid_formatted_output_is_rejected_on_the_single_runtime_path() -> None:
    broken = BrokenRenderedValue()

    with pytest.raises(TemplateParseError):
        render_text(t"value: {broken!r}\n")


def test_yaml_block_formatted_payloads_are_rejected_in_text_rendering() -> None:
    block = BlockFormattedValue()

    with pytest.raises(TemplateParseError, match="flow-safe formatted text"):
        render_text(t"value: {block!s}\n")

    with pytest.raises(TemplateParseError, match="flow-safe formatted text"):
        render_text(t"flow: [{block!s}]\n")

    with pytest.raises(TemplateParseError, match="flow-safe formatted text"):
        render_result(t"value: {block!s}\n")


def test_yaml_block_formatted_payloads_keep_render_data_behavior() -> None:
    block = BlockFormattedValue()

    assert render_data(t"{block!s}\n") == {"nested": [1, 2]}
    assert render_data(t"value: {block!s}\n") == {"value": {"nested": [1, 2]}}


def test_yaml_formatted_payloads_can_resolve_outer_anchor_context() -> None:
    alias = "*a"
    template = t"base: &a 1\nref: {alias!s}\n"

    assert render_text(template) == "base: &a 1\nref: *a"
    assert render_data(template) == {"base": 1, "ref": 1}

    result = render_result(template)
    assert result.text == "base: &a 1\nref: *a"
    assert result.data == {"base": 1, "ref": 1}


def test_yaml_binding_errors_expose_structured_diagnostics() -> None:
    bad = BadStringValue()

    with pytest.raises(UnrepresentableValueError) as unrepr_info:
        render_text(t'label: "hi-{bad}"')
    _assert_exception_metadata(
        unrepr_info.value,
        expected_code="yaml.unrepresentable.fragment",
        has_span=True,
    )
