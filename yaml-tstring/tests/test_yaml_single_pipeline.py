from __future__ import annotations

import pytest

from yaml_tstring import TemplateParseError, render_data, render_result, render_text


def test_yaml_result_prefers_text_visible_payload_parse_errors() -> None:
    broken = "[1,,2]"
    template = t"value: {broken!s}\nref: *missing\n"

    with pytest.raises(TemplateParseError, match="invalid formatted YAML payload"):
        render_text(template)

    with pytest.raises(TemplateParseError, match="invalid formatted YAML payload"):
        render_result(template)


def test_yaml_formatted_payload_alias_can_see_outer_anchor() -> None:
    payload = "*root"
    template = t"base: &root 1\ncopy: {payload!s}\n"

    assert render_text(template) == "base: &root 1\ncopy: *root"
    assert render_data(template) == {"base": 1, "copy": 1}


def test_yaml_formatted_payload_anchor_is_visible_to_later_outer_nodes() -> None:
    payload = "&inner 1"
    template = t"first: {payload!s}\nsecond: *inner\n"

    assert render_text(template) == "first: &inner 1\nsecond: *inner"
    assert render_data(template) == {"first": 1, "second": 1}


def test_yaml_formatted_payload_inherits_outer_tag_directives() -> None:
    payload = "!e!leaf 1"
    template = t"%TAG !e! tag:example.com,2020:\n---\nvalue: {payload!s}\n"

    result = render_result(template)

    assert result.text == "%TAG !e! tag:example.com,2020:\n---\nvalue: !e!leaf 1"
    assert result.data == {"value": 1}
