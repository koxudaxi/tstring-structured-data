from __future__ import annotations

from string.templatelib import Template
from typing import Annotated, Literal, Protocol, TypeIs, cast

from tstring_bindings import tstring_bindings as _bindings
from tstring_core import RenderResult, YamlValue

type YamlProfile = Literal["1.2.2"]
type YamlTemplate = Annotated[Template, "yaml"]

_CONTRACT_VERSION = 1
_REQUIRED_SYMBOLS = {
    "render_yaml",
    "render_yaml_text",
    "_render_yaml_result_payload",
}


class _RenderYaml(Protocol):
    def __call__(
        self, template: YamlTemplate, *, profile: YamlProfile
    ) -> YamlValue: ...


class _RenderYamlText(Protocol):
    def __call__(self, template: YamlTemplate, *, profile: YamlProfile) -> str: ...


class _RenderYamlResultPayload(Protocol):
    def __call__(
        self, template: YamlTemplate, *, profile: YamlProfile
    ) -> tuple[str, YamlValue]: ...


def _bind_extension() -> tuple[_RenderYaml, _RenderYamlText, _RenderYamlResultPayload]:
    version = getattr(_bindings, "__contract_version__", None)
    if version != _CONTRACT_VERSION:
        raise ImportError(
            "yaml_tstring extension contract mismatch: "
            f"expected version {_CONTRACT_VERSION}, got {version!r}."
        )

    exported = set(getattr(_bindings, "__contract_symbols__", ()))
    missing = sorted(_REQUIRED_SYMBOLS - exported)
    if missing:
        raise ImportError(
            "yaml_tstring could not bind required extension symbols: "
            + ", ".join(missing)
        )

    return (
        cast(_RenderYaml, _bindings.render_yaml),
        cast(_RenderYamlText, _bindings.render_yaml_text),
        cast(_RenderYamlResultPayload, _bindings._render_yaml_result_payload),
    )


_render_yaml, _render_yaml_text, _render_yaml_result_payload = _bind_extension()


def _is_template(value: object) -> TypeIs[Template]:
    return isinstance(value, Template)


def _validate_template(template: object, api_name: str) -> YamlTemplate:
    if _is_template(template):
        return template
    raise TypeError(
        f"{api_name} requires a PEP 750 Template object. "
        f"Got {type(template).__name__} instead."
    )


def _resolve_profile(profile: YamlProfile | str | None) -> YamlProfile:
    if profile is None or profile == "1.2.2":
        return "1.2.2"
    raise ValueError(
        f"Unsupported YAML profile {profile!r}. Supported profiles: '1.2.2'."
    )


def render_data(
    template: YamlTemplate, *, profile: YamlProfile | str | None = None
) -> YamlValue:
    checked = _validate_template(template, "render_data")
    return _render_yaml(checked, profile=_resolve_profile(profile))


def render_text(
    template: YamlTemplate, *, profile: YamlProfile | str | None = None
) -> str:
    checked = _validate_template(template, "render_text")
    return _render_yaml_text(checked, profile=_resolve_profile(profile))


def render_result(
    template: YamlTemplate, *, profile: YamlProfile | str | None = None
) -> RenderResult[YamlValue]:
    checked = _validate_template(template, "render_result")
    text, data = _render_yaml_result_payload(checked, profile=_resolve_profile(profile))
    return RenderResult(text=text, data=data)


__all__ = [
    "RenderResult",
    "YamlProfile",
    "render_data",
    "render_result",
    "render_text",
]
