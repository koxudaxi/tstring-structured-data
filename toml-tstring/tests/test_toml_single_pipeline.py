from __future__ import annotations

from string.templatelib import Template

import pytest

from toml_tstring import TemplateSemanticError, render_result, render_text


def test_toml_render_text_reports_duplicate_keys_from_document_state() -> None:
    with pytest.raises(TemplateSemanticError, match="Duplicate TOML key"):
        render_text(Template("value = 1\nvalue = 2\n"))


def test_toml_render_text_reports_table_scalar_path_conflicts() -> None:
    with pytest.raises(TemplateSemanticError, match="Conflicting TOML"):
        render_text(Template("value = 1\n[value]\nname = 2\n"))


def test_toml_render_text_reports_array_of_tables_conflicts() -> None:
    with pytest.raises(TemplateSemanticError, match="Conflicting TOML"):
        render_text(Template("value = 1\n[[value]]\nname = 2\n"))


def test_toml_result_prefers_earlier_document_conflicts_over_later_payload_errors() -> (
    None
):
    broken = "[1,,2]"
    template = t"[owner]\nname = 'Alice'\n[owner]\nvalue = {broken!s}\n"

    with pytest.raises(TemplateSemanticError, match="Duplicate TOML table"):
        render_text(template)

    with pytest.raises(TemplateSemanticError, match="Duplicate TOML table"):
        render_result(template)
