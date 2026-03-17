from __future__ import annotations

import warnings

from ._diagnostics import Diagnostic, DiagnosticSeverity
from ._errors import (
    TemplateError,
    TemplateParseError,
    TemplateSemanticError,
    UnrepresentableValueError,
)
from ._nodes import TemplateNode
from ._slots import FragmentGroup, Slot, SlotContext
from ._spans import SourcePosition, SourceSpan
from ._tokens import (
    InterpolationToken,
    StaticTextToken,
    StreamItem,
    TemplateToken,
    flatten_template,
    tokenize_template,
)
from ._types import JsonValue, StructuredData, TomlValue, YamlKey, YamlValue
from ._values import RenderResult, ValueKind

_DEPRECATED_ROOT_EXPORTS = frozenset(
    {"ParserFirstBackend", "ReturnMode", "render_with_backend"}
)


def __getattr__(name: str) -> object:
    if name not in _DEPRECATED_ROOT_EXPORTS:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")

    from . import _render

    value = getattr(_render, name)
    warnings.warn(
        f"tstring_core.{name} is deprecated and will be removed in a future release.",
        DeprecationWarning,
        stacklevel=2,
    )
    globals()[name] = value
    return value


__all__ = [
    "Diagnostic",
    "DiagnosticSeverity",
    "FragmentGroup",
    "InterpolationToken",
    "JsonValue",
    "RenderResult",
    "Slot",
    "SlotContext",
    "SourcePosition",
    "SourceSpan",
    "StaticTextToken",
    "StreamItem",
    "TemplateError",
    "TemplateNode",
    "TemplateParseError",
    "TemplateSemanticError",
    "TomlValue",
    "TemplateToken",
    "UnrepresentableValueError",
    "ValueKind",
    "YamlKey",
    "YamlValue",
    "StructuredData",
    "flatten_template",
    "tokenize_template",
]
