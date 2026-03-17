from __future__ import annotations

from dataclasses import dataclass, field

from ._diagnostics import Diagnostic
from ._spans import SourceSpan


@dataclass(slots=True)
class TemplateNode:
    span: SourceSpan
    diagnostics: list[Diagnostic] = field(default_factory=list, kw_only=True)
