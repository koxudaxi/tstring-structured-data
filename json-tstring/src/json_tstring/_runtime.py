from __future__ import annotations

from string.templatelib import Template
from typing import Annotated, Literal, Protocol, TypeIs, cast

from tstring_bindings import tstring_bindings as _bindings
from tstring_core import JsonValue, RenderResult

type JsonProfile = Literal["rfc8259"]
type JsonTemplate = Annotated[Template, "json"]

_CONTRACT_VERSION = 1
_REQUIRED_SYMBOLS = {
    "render_json",
    "render_json_text",
    "_render_json_result_payload",
}


class _RenderJson(Protocol):
    def __call__(
        self, template: JsonTemplate, *, profile: JsonProfile
    ) -> JsonValue: ...


class _RenderJsonText(Protocol):
    def __call__(self, template: JsonTemplate, *, profile: JsonProfile) -> str: ...


class _RenderJsonResultPayload(Protocol):
    def __call__(
        self, template: JsonTemplate, *, profile: JsonProfile
    ) -> tuple[str, JsonValue]: ...


def _bind_extension() -> tuple[_RenderJson, _RenderJsonText, _RenderJsonResultPayload]:
    version = getattr(_bindings, "__contract_version__", None)
    if version != _CONTRACT_VERSION:
        raise ImportError(
            "json_tstring extension contract mismatch: "
            f"expected version {_CONTRACT_VERSION}, got {version!r}."
        )

    exported = set(getattr(_bindings, "__contract_symbols__", ()))
    missing = sorted(_REQUIRED_SYMBOLS - exported)
    if missing:
        raise ImportError(
            "json_tstring could not bind required extension symbols: "
            + ", ".join(missing)
        )

    return (
        cast(_RenderJson, _bindings.render_json),
        cast(_RenderJsonText, _bindings.render_json_text),
        cast(_RenderJsonResultPayload, _bindings._render_json_result_payload),
    )


_render_json, _render_json_text, _render_json_result_payload = _bind_extension()


def _is_template(value: object) -> TypeIs[Template]:
    return isinstance(value, Template)


def _validate_template(template: object, api_name: str) -> JsonTemplate:
    if _is_template(template):
        return template
    raise TypeError(
        f"{api_name} requires a PEP 750 Template object. "
        f"Got {type(template).__name__} instead."
    )


def _resolve_profile(profile: JsonProfile | str | None) -> JsonProfile:
    if profile is None or profile == "rfc8259":
        return "rfc8259"
    raise ValueError(
        f"Unsupported JSON profile {profile!r}. Supported profiles: 'rfc8259'."
    )


def render_data(
    template: JsonTemplate, *, profile: JsonProfile | str | None = None
) -> JsonValue:
    checked = _validate_template(template, "render_data")
    return _render_json(checked, profile=_resolve_profile(profile))


def render_text(
    template: JsonTemplate, *, profile: JsonProfile | str | None = None
) -> str:
    checked = _validate_template(template, "render_text")
    return _render_json_text(checked, profile=_resolve_profile(profile))


def render_result(
    template: JsonTemplate, *, profile: JsonProfile | str | None = None
) -> RenderResult[JsonValue]:
    checked = _validate_template(template, "render_result")
    text, data = _render_json_result_payload(checked, profile=_resolve_profile(profile))
    return RenderResult(text=text, data=data)


__all__ = [
    "JsonProfile",
    "RenderResult",
    "render_data",
    "render_result",
    "render_text",
]
