from __future__ import annotations

from string.templatelib import Template
from typing import Annotated, Protocol, cast

from . import tstring_bindings as _bindings
from ._profiles import (
    JsonProfile,
    TomlProfile,
    YamlProfile,
    resolve_json_profile,
    resolve_toml_profile,
    resolve_yaml_profile,
)
from ._types import JsonValue, TomlValue, YamlValue

type JsonTemplate = Annotated[Template, "json"]
type TomlTemplate = Annotated[Template, "toml"]
type YamlTemplate = Annotated[Template, "yaml"]


class _RenderJson(Protocol):
    def __call__(self, template: JsonTemplate, profile: JsonProfile) -> JsonValue: ...


class _RenderJsonText(Protocol):
    def __call__(self, template: JsonTemplate, profile: JsonProfile) -> str: ...


class _RenderToml(Protocol):
    def __call__(self, template: TomlTemplate, profile: TomlProfile) -> TomlValue: ...


class _RenderTomlText(Protocol):
    def __call__(self, template: TomlTemplate, profile: TomlProfile) -> str: ...


class _RenderYaml(Protocol):
    def __call__(self, template: YamlTemplate, profile: YamlProfile) -> YamlValue: ...


class _RenderYamlText(Protocol):
    def __call__(self, template: YamlTemplate, profile: YamlProfile) -> str: ...


class _BindingsContract(Protocol):
    __contract_version__: int
    __contract_symbols__: tuple[str, ...]
    TemplateError: type[Exception]
    TemplateParseError: type[Exception]
    TemplateSemanticError: type[Exception]
    UnrepresentableValueError: type[Exception]
    render_json: _RenderJson
    render_json_text: _RenderJsonText
    render_toml: _RenderToml
    render_toml_text: _RenderTomlText
    render_yaml: _RenderYaml
    render_yaml_text: _RenderYamlText


_CONTRACT_VERSION = 1
_REQUIRED_SYMBOLS = {
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
}


def _require_extension_contract() -> _BindingsContract:
    version = getattr(_bindings, "__contract_version__", None)
    if version != _CONTRACT_VERSION:
        raise ImportError(
            "tstring_bindings extension contract mismatch: "
            f"expected version {_CONTRACT_VERSION}, got {version!r}."
        )

    exported = set(getattr(_bindings, "__contract_symbols__", ()))
    missing = sorted(_REQUIRED_SYMBOLS - exported)
    if missing:
        raise ImportError(
            "tstring_bindings extension is missing required symbols: "
            + ", ".join(missing)
        )
    return cast(_BindingsContract, _bindings)


_EXTENSION = _require_extension_contract()
TemplateError = _EXTENSION.TemplateError
TemplateParseError = _EXTENSION.TemplateParseError
TemplateSemanticError = _EXTENSION.TemplateSemanticError
UnrepresentableValueError = _EXTENSION.UnrepresentableValueError
_render_json = _EXTENSION.render_json
_render_json_text = _EXTENSION.render_json_text
_render_toml = _EXTENSION.render_toml
_render_toml_text = _EXTENSION.render_toml_text
_render_yaml = _EXTENSION.render_yaml
_render_yaml_text = _EXTENSION.render_yaml_text


def render_json(
    template: JsonTemplate, *, profile: JsonProfile | str | None = None
) -> JsonValue:
    return _render_json(template, resolve_json_profile(profile))


def render_json_text(
    template: JsonTemplate, *, profile: JsonProfile | str | None = None
) -> str:
    return _render_json_text(template, resolve_json_profile(profile))


def render_toml(
    template: TomlTemplate, *, profile: TomlProfile | str | None = None
) -> TomlValue:
    return _render_toml(template, resolve_toml_profile(profile))


def render_toml_text(
    template: TomlTemplate, *, profile: TomlProfile | str | None = None
) -> str:
    return _render_toml_text(template, resolve_toml_profile(profile))


def render_yaml(
    template: YamlTemplate, *, profile: YamlProfile | str | None = None
) -> YamlValue:
    return _render_yaml(template, resolve_yaml_profile(profile))


def render_yaml_text(
    template: YamlTemplate, *, profile: YamlProfile | str | None = None
) -> str:
    return _render_yaml_text(template, resolve_yaml_profile(profile))


__all__ = [
    "JsonProfile",
    "TomlProfile",
    "TemplateError",
    "TemplateParseError",
    "TemplateSemanticError",
    "UnrepresentableValueError",
    "YamlProfile",
    "render_json",
    "render_json_text",
    "render_toml",
    "render_toml_text",
    "render_yaml",
    "render_yaml_text",
]
