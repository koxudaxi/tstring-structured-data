from __future__ import annotations

from string.templatelib import Template
from typing import Annotated, Literal, Protocol, TypeIs, cast

from tstring_bindings import tstring_bindings as _bindings
from tstring_core import RenderResult, TomlValue

type TomlProfile = Literal["1.0", "1.1"]
type TomlTemplate = Annotated[Template, "toml"]

_CONTRACT_VERSION = 1
_REQUIRED_SYMBOLS = {
    "render_toml",
    "render_toml_text",
    "_render_toml_result_payload",
}


class _RenderToml(Protocol):
    def __call__(self, template: TomlTemplate, *, profile: TomlProfile) -> TomlValue: ...


class _RenderTomlText(Protocol):
    def __call__(self, template: TomlTemplate, *, profile: TomlProfile) -> str: ...


class _RenderTomlResultPayload(Protocol):
    def __call__(
        self, template: TomlTemplate, *, profile: TomlProfile
    ) -> tuple[str, TomlValue]: ...


def _bind_extension() -> tuple[_RenderToml, _RenderTomlText, _RenderTomlResultPayload]:
    version = getattr(_bindings, "__contract_version__", None)
    if version != _CONTRACT_VERSION:
        raise ImportError(
            "toml_tstring extension contract mismatch: "
            f"expected version {_CONTRACT_VERSION}, got {version!r}."
        )

    exported = set(getattr(_bindings, "__contract_symbols__", ()))
    missing = sorted(_REQUIRED_SYMBOLS - exported)
    if missing:
        raise ImportError(
            "toml_tstring could not bind required extension symbols: "
            + ", ".join(missing)
        )

    return (
        cast(_RenderToml, _bindings.render_toml),
        cast(_RenderTomlText, _bindings.render_toml_text),
        cast(_RenderTomlResultPayload, _bindings._render_toml_result_payload),
    )


_render_toml, _render_toml_text, _render_toml_result_payload = _bind_extension()


def _is_template(value: object) -> TypeIs[Template]:
    return isinstance(value, Template)


def _validate_template(template: object, api_name: str) -> TomlTemplate:
    if _is_template(template):
        return template
    raise TypeError(
        f"{api_name} requires a PEP 750 Template object. "
        f"Got {type(template).__name__} instead."
    )


def _resolve_profile(profile: TomlProfile | str | None) -> TomlProfile:
    if profile is None:
        return "1.1"
    if profile == "1.0":
        return "1.0"
    if profile == "1.1":
        return "1.1"
    raise ValueError(
        f"Unsupported TOML profile {profile!r}. Supported profiles: '1.0', '1.1'."
    )


def render_data(
    template: TomlTemplate, *, profile: TomlProfile | str | None = None
) -> TomlValue:
    checked = _validate_template(template, "render_data")
    return _render_toml(checked, profile=_resolve_profile(profile))


def render_text(
    template: TomlTemplate, *, profile: TomlProfile | str | None = None
) -> str:
    checked = _validate_template(template, "render_text")
    return _render_toml_text(checked, profile=_resolve_profile(profile))


def render_result(
    template: TomlTemplate, *, profile: TomlProfile | str | None = None
) -> RenderResult[TomlValue]:
    checked = _validate_template(template, "render_result")
    text, data = _render_toml_result_payload(checked, profile=_resolve_profile(profile))
    return RenderResult(text=text, data=data)


__all__ = [
    "RenderResult",
    "TomlProfile",
    "render_data",
    "render_result",
    "render_text",
]
