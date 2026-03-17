from __future__ import annotations

import importlib.util
import sys
from importlib import import_module
from pathlib import Path
from string.templatelib import Template
from types import ModuleType, SimpleNamespace
from typing import Any, cast, get_args

import pytest
from tstring_bindings.tstring_bindings import _render_toml_result_payload
from tstring_core import TemplateError, TemplateParseError

import toml_tstring

tstring_bindings = cast(Any, import_module("tstring_bindings"))
extension_module = cast(Any, import_module("tstring_bindings.tstring_bindings"))


def _optional_module(name: str) -> Any | None:
    try:
        return import_module(name)
    except ModuleNotFoundError:
        return None


json_tstring = _optional_module("json_tstring")
yaml_tstring = _optional_module("yaml_tstring")

REPO_ROOT = Path(__file__).resolve().parents[2]
TOML_VENDOR_ROOT = REPO_ROOT / "conformance" / "toml" / "vendor" / "toml-test" / "tests"


def _alias_values(alias: Any) -> tuple[str, ...]:
    value = getattr(alias, "__value__", alias)
    return get_args(value)


def test_exported_profile_aliases_exist() -> None:
    assert _alias_values(tstring_bindings.JsonProfile) == ("rfc8259",)
    assert _alias_values(tstring_bindings.TomlProfile) == ("1.0", "1.1")
    assert _alias_values(tstring_bindings.YamlProfile) == ("1.2.2",)
    if json_tstring is not None:
        assert _alias_values(json_tstring.JsonProfile) == ("rfc8259",)
    assert _alias_values(toml_tstring.TomlProfile) == ("1.0", "1.1")
    if yaml_tstring is not None:
        assert _alias_values(yaml_tstring.YamlProfile) == ("1.2.2",)


def test_top_level_public_exports_hide_extension_helpers() -> None:
    assert {
        "JsonProfile",
        "TomlProfile",
        "YamlProfile",
        "TemplateError",
        "TemplateParseError",
        "TemplateSemanticError",
        "UnrepresentableValueError",
        "render_json",
        "render_json_text",
        "render_toml",
        "render_toml_text",
        "render_yaml",
        "render_yaml_text",
    } == set(tstring_bindings.__all__)
    assert "_render_toml_result_payload" not in tstring_bindings.__all__


def test_public_wrappers_validate_profile_strings_in_python() -> None:
    with pytest.raises(ValueError, match="Unsupported JSON profile"):
        tstring_bindings.render_json_text(Template('{"value": 1}'), profile="draft")
    with pytest.raises(ValueError, match="Unsupported TOML profile"):
        tstring_bindings.render_toml_text(t"value = 1", profile="2.0")
    with pytest.raises(ValueError, match="Unsupported YAML profile"):
        tstring_bindings.render_yaml_text(t"value: 1", profile="1.1")

    if json_tstring is not None:
        with pytest.raises(ValueError, match="Unsupported JSON profile"):
            json_tstring.render_text(Template('{"value": 1}'), profile="draft")
    with pytest.raises(ValueError, match="Unsupported TOML profile"):
        toml_tstring.render_text(t"value = 1", profile="2.0")
    if yaml_tstring is not None:
        with pytest.raises(ValueError, match="Unsupported YAML profile"):
            yaml_tstring.render_text(t"value: 1", profile="1.1")


def test_public_wrappers_require_profile_to_be_keyword_only() -> None:
    cases: list[tuple[Any, Template, str]] = [
        (tstring_bindings.render_json, Template('{"value": 1}'), "rfc8259"),
        (tstring_bindings.render_json_text, Template('{"value": 1}'), "rfc8259"),
        (tstring_bindings.render_toml, t"value = 1", "1.0"),
        (tstring_bindings.render_toml_text, t"value = 1", "1.0"),
        (tstring_bindings.render_yaml, t"value: 1", "1.2.2"),
        (tstring_bindings.render_yaml_text, t"value: 1", "1.2.2"),
        (toml_tstring.render_data, t"value = 1", "1.0"),
        (toml_tstring.render_text, t"value = 1", "1.0"),
        (toml_tstring.render_result, t"value = 1", "1.0"),
    ]
    if json_tstring is not None:
        cases.extend(
            [
                (json_tstring.render_data, Template('{"value": 1}'), "rfc8259"),
                (json_tstring.render_text, Template('{"value": 1}'), "rfc8259"),
                (json_tstring.render_result, Template('{"value": 1}'), "rfc8259"),
            ]
        )
    if yaml_tstring is not None:
        cases.extend(
            [
                (yaml_tstring.render_data, t"value: 1", "1.2.2"),
                (yaml_tstring.render_text, t"value: 1", "1.2.2"),
                (yaml_tstring.render_result, t"value: 1", "1.2.2"),
            ]
        )

    for render, template, profile in cases:
        with pytest.raises(TypeError):
            render(template, profile)


def test_json_and_yaml_defaults_match_explicit_profiles() -> None:
    json_template = Template('{"value": 1}')
    yaml_template = t"value: 1"

    assert tstring_bindings.render_json_text(
        json_template
    ) == tstring_bindings.render_json_text(json_template, profile="rfc8259")
    if json_tstring is not None:
        assert json_tstring.render_text(json_template) == json_tstring.render_text(
            json_template, profile="rfc8259"
        )

    assert tstring_bindings.render_yaml_text(
        yaml_template
    ) == tstring_bindings.render_yaml_text(yaml_template, profile="1.2.2")
    if yaml_tstring is not None:
        assert yaml_tstring.render_text(yaml_template) == yaml_tstring.render_text(
            yaml_template, profile="1.2.2"
        )


def test_toml_default_matches_explicit_profile_1_1() -> None:
    template = Template("value = {\n  answer = 42,\n}")

    assert tstring_bindings.render_toml_text(
        template
    ) == tstring_bindings.render_toml_text(template, profile="1.1")
    assert toml_tstring.render_text(template) == toml_tstring.render_text(
        template, profile="1.1"
    )

    with pytest.raises(TemplateParseError):
        tstring_bindings.render_toml_text(template, profile="1.0")
    with pytest.raises(TemplateParseError):
        toml_tstring.render_text(template, profile="1.0")


@pytest.mark.parametrize(
    "relative_path",
    [
        "valid/datetime/no-seconds.toml",
        "valid/inline-table/newline.toml",
        "valid/string/escape-esc.toml",
        "valid/string/hex-escape.toml",
    ],
)
def test_toml_profile_boundaries_for_selected_1_1_fixtures(relative_path: str) -> None:
    source_text = (TOML_VENDOR_ROOT / relative_path).read_text(
        encoding="utf-8", newline=""
    )
    template = Template(source_text)

    assert toml_tstring.render_data(template, profile="1.1") is not None
    with pytest.raises(TemplateError):
        toml_tstring.render_data(template, profile="1.0")


def test_toml_formatted_interpolation_obeys_profile_boundaries() -> None:
    payload = "{\n  answer = 42,\n}"
    template = t"value = {payload!s}"

    assert toml_tstring.render_data(template, profile="1.1") == {
        "value": {"answer": 42}
    }
    with pytest.raises(TemplateParseError):
        toml_tstring.render_data(template, profile="1.0")


def test_extension_private_result_payload_helpers_remain_available() -> None:
    text, data = _render_toml_result_payload(t"value = 1", "1.0")

    assert text == "value = 1"
    assert data == {"value": 1}


def test_extension_contract_metadata_is_exposed() -> None:
    assert extension_module.__contract_version__ == 1
    assert {
        "TemplateError",
        "TemplateParseError",
        "TemplateSemanticError",
        "UnrepresentableValueError",
        "render_json",
        "render_json_text",
        "_render_json_result_payload",
        "render_toml",
        "render_toml_text",
        "_render_toml_result_payload",
        "render_yaml",
        "render_yaml_text",
        "_render_yaml_result_payload",
    } == set(extension_module.__contract_symbols__)
    assert not hasattr(extension_module, "TomlSemanticArtifact")
    assert not hasattr(extension_module, "DocumentContextPlan")


def test_extension_direct_errors_keep_stable_diagnostics_shape() -> None:
    with pytest.raises(Exception) as info:
        extension_module.render_toml_text(Template("value = [1,,2]\n"), "1.0")

    exc = cast(Any, info.value)
    assert exc.code == "toml.parse"
    assert exc.span == exc.diagnostics[0]["span"]
    assert isinstance(exc.diagnostics, tuple)
    assert exc.diagnostics
    assert exc.diagnostics[0]["code"] == "toml.parse"
    assert exc.diagnostics[0]["message"] == str(exc)
    assert exc.diagnostics[0]["severity"] == "error"
    assert isinstance(exc.diagnostics[0]["metadata"], dict)


def _load_module_from_path(module_name: str, path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location(module_name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    try:
        spec.loader.exec_module(module)
    finally:
        sys.modules.pop(module_name, None)
    return module


@pytest.mark.parametrize(
    ("runtime_path", "missing_symbol"),
    [
        (
            REPO_ROOT / "json-tstring" / "src" / "json_tstring" / "_runtime.py",
            "_render_json_result_payload",
        ),
        (
            REPO_ROOT / "toml-tstring" / "src" / "toml_tstring" / "_runtime.py",
            "_render_toml_result_payload",
        ),
        (
            REPO_ROOT / "yaml-tstring" / "src" / "yaml_tstring" / "_runtime.py",
            "_render_yaml_result_payload",
        ),
    ],
)
def test_format_runtime_rejects_stale_extension_contract_at_import_time(
    monkeypatch: pytest.MonkeyPatch,
    runtime_path: Path,
    missing_symbol: str,
) -> None:
    fake_extension = SimpleNamespace(
        __contract_version__=0,
        __contract_symbols__=(),
    )
    monkeypatch.setattr(
        tstring_bindings, "tstring_bindings", fake_extension, raising=False
    )
    with pytest.raises(ImportError, match="contract mismatch"):
        _load_module_from_path(f"_contract_mismatch_{runtime_path.stem}", runtime_path)

    fake_extension = SimpleNamespace(
        __contract_version__=1,
        __contract_symbols__=tuple(
            symbol
            for symbol in extension_module.__contract_symbols__
            if symbol != missing_symbol
        ),
    )
    for symbol in extension_module.__contract_symbols__:
        if symbol != missing_symbol:
            setattr(fake_extension, symbol, getattr(extension_module, symbol))
    monkeypatch.setattr(
        tstring_bindings, "tstring_bindings", fake_extension, raising=False
    )
    with pytest.raises(ImportError, match="required extension symbols"):
        _load_module_from_path(f"_missing_symbol_{runtime_path.stem}", runtime_path)
