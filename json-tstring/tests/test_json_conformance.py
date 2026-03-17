from __future__ import annotations

from string.templatelib import Template

import pytest
from tstring_core import TemplateError
from tstring_core._conformance import load_conformance_suite

from json_tstring import render_data, render_text

JSON_SUITE = load_conformance_suite("json", "rfc8259")


@pytest.mark.parametrize(
    "case",
    JSON_SUITE.iter_cases("python"),
    ids=lambda case: case.case_id,
)
def test_json_conformance_cases(case) -> None:
    template = Template(case.input_text())

    if case.expected == "accept":
        data = render_data(template)
        rendered = render_text(template)
        assert render_data(template, profile="rfc8259") == data
        assert render_text(template, profile="rfc8259") == rendered
        assert rendered
        expected_json = case.expected_json()
        if expected_json is not None:
            assert data == expected_json
        return

    with pytest.raises(TemplateError):
        render_data(template)
    with pytest.raises(TemplateError):
        render_text(template)
