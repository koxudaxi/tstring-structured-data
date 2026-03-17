from __future__ import annotations

from string.templatelib import Template
from typing import Literal, Protocol, TypeIs, TypeVar, overload

from ._errors import TemplateParseError
from ._tokens import tokenize_template
from ._types import StructuredData
from ._values import RenderResult

ReturnMode = Literal["data", "text"]
NodeT = TypeVar("NodeT")
DataT = TypeVar("DataT", bound=StructuredData)


class ParserFirstBackend(Protocol[NodeT, DataT]):
    def parse(self, template: Template) -> NodeT: ...

    def analyze(self, node: NodeT) -> None: ...

    def render(self, node: NodeT) -> RenderResult[DataT]: ...


@overload
def render_with_backend(
    template: Template,
    *,
    api_name: str,
    backend: ParserFirstBackend[NodeT, DataT],
    return_mode: Literal["data"],
) -> DataT: ...


@overload
def render_with_backend(
    template: Template,
    *,
    api_name: str,
    backend: ParserFirstBackend[NodeT, DataT],
    return_mode: Literal["text"],
) -> str: ...


def render_with_backend(
    template: Template,
    *,
    api_name: str,
    backend: ParserFirstBackend[NodeT, DataT],
    return_mode: ReturnMode,
) -> DataT | str:
    _ensure_template(template, api_name)
    tokenize_template(template)

    node = backend.parse(template)
    backend.analyze(node)
    rendered = backend.render(node)

    if return_mode == "text":
        return rendered.text
    return rendered.data


def _is_template(value: object) -> TypeIs[Template]:
    return isinstance(value, Template)


def _ensure_template(template: object, api_name: str) -> None:
    if _is_template(template):
        return

    message = (
        f"{api_name} require a PEP 750 Template object. "
        f"Got {type(template).__name__} instead."
    )
    raise TypeError(message)


__all__ = [
    "ParserFirstBackend",
    "ReturnMode",
    "TemplateParseError",
    "render_with_backend",
]
