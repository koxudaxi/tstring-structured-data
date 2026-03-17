from __future__ import annotations

from pathlib import Path
from string.templatelib import Template

import pytest
from tstring_core import TemplateError
from tstring_core._conformance import load_conformance_suite

from toml_tstring import render_data, render_text

TOML_V1_0_SUITE = load_conformance_suite("toml", "1.0")
TOML_V1_1_SUITE = load_conformance_suite("toml", "1.1")
REPO_ROOT = Path(__file__).resolve().parents[2]
TOML_VENDOR_ROOT = REPO_ROOT / "conformance" / "toml" / "vendor" / "toml-test" / "tests"
TOML_OUT_OF_SCOPE_VALID_CASES = {
    "valid/datetime/no-seconds.toml",
    "valid/inline-table/newline-comment.toml",
    "valid/inline-table/newline.toml",
    "valid/string/escape-esc.toml",
    "valid/string/hex-escape.toml",
}


@pytest.mark.parametrize(
    "case",
    TOML_V1_0_SUITE.iter_cases("python"),
    ids=lambda case: case.case_id,
)
def test_toml_conformance_cases_v1_0(case) -> None:
    template = Template(case.input_text())

    if case.expected == "accept":
        data = render_data(template, profile="1.0")
        rendered = render_text(template, profile="1.0")
        assert rendered
        expected_json = case.expected_json()
        if expected_json is not None:
            assert data == expected_json
        return

    with pytest.raises(TemplateError):
        render_data(template, profile="1.0")
    with pytest.raises(TemplateError):
        render_text(template, profile="1.0")


@pytest.mark.parametrize(
    "case",
    TOML_V1_1_SUITE.iter_cases("python"),
    ids=lambda case: case.case_id,
)
def test_toml_conformance_cases_v1_1(case) -> None:
    template = Template(case.input_text())

    if case.expected == "accept":
        data = render_data(template, profile="1.1")
        rendered = render_text(template, profile="1.1")
        assert rendered
        expected_json = case.expected_json()
        if expected_json is not None:
            assert data == expected_json
        return

    with pytest.raises(TemplateError):
        render_data(template, profile="1.1")
    with pytest.raises(TemplateError):
        render_text(template, profile="1.1")


def _toml_vendor_cases() -> list[tuple[str, str, bool]]:
    cases = []
    for bucket in ("valid", "invalid"):
        for path in sorted((TOML_VENDOR_ROOT / bucket).rglob("*.toml")):
            relpath = path.relative_to(TOML_VENDOR_ROOT).as_posix()
            if "spec-1.1.0/" in relpath or relpath in TOML_OUT_OF_SCOPE_VALID_CASES:
                continue
            try:
                source_text = path.read_text(encoding="utf-8", newline="")
            except UnicodeDecodeError:
                continue
            cases.append((relpath, source_text, bucket == "invalid"))
    return cases


@pytest.mark.parametrize(
    ("relpath", "source_text", "expects_error"),
    _toml_vendor_cases(),
    ids=[case[0] for case in _toml_vendor_cases()],
)
def test_toml_vendor_suite_v1_0_scope(
    relpath: str, source_text: str, expects_error: bool
) -> None:
    template = Template(source_text)
    if expects_error:
        with pytest.raises(TemplateError):
            render_data(template, profile="1.0")
        return

    render_data(template, profile="1.0")
