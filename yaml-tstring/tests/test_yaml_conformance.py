from __future__ import annotations

from pathlib import Path
from string.templatelib import Template

import pytest
from tstring_core import TemplateError
from tstring_core._conformance import load_conformance_suite

from yaml_tstring import render_data, render_text

YAML_SUITE = load_conformance_suite("yaml", "1.2.2")
REPO_ROOT = Path(__file__).resolve().parents[2]
YAML_VENDOR_ROOT = REPO_ROOT / "conformance" / "yaml" / "vendor" / "yaml-test-suite"


@pytest.mark.parametrize(
    "case",
    YAML_SUITE.iter_cases("python"),
    ids=lambda case: case.case_id,
)
def test_yaml_conformance_cases(case) -> None:
    template = Template(case.input_text())

    if case.expected == "accept":
        data = render_data(template)
        rendered = render_text(template)
        assert render_data(template, profile="1.2.2") == data
        assert render_text(template, profile="1.2.2") == rendered
        assert rendered
        expected_json = case.expected_json()
        if expected_json is not None:
            assert data == expected_json
        return

    with pytest.raises(TemplateError):
        render_data(template)
    with pytest.raises(TemplateError):
        render_text(template)


def _yaml_vendor_cases() -> list[tuple[str, str, bool]]:
    cases = []
    for case_dir in sorted(
        path
        for path in YAML_VENDOR_ROOT.iterdir()
        if path.is_dir() and (path / "in.yaml").exists()
    ):
        cases.append(
            (
                case_dir.name,
                (case_dir / "in.yaml").read_text(encoding="utf-8", newline=""),
                (case_dir / "error").exists(),
            )
        )
    return cases


@pytest.mark.parametrize(
    ("case_id", "source_text", "expects_error"),
    _yaml_vendor_cases(),
    ids=[case[0] for case in _yaml_vendor_cases()],
)
def test_yaml_full_vendor_suite(
    case_id: str, source_text: str, expects_error: bool
) -> None:
    template = Template(source_text)
    if expects_error:
        with pytest.raises(TemplateError):
            render_data(template)
        return

    render_data(template)
