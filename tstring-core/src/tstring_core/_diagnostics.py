from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

from ._spans import SourceSpan


class DiagnosticSeverity(str, Enum):
    ERROR = "error"
    WARNING = "warning"
    INFO = "info"


@dataclass(frozen=True, slots=True)
class Diagnostic:
    code: str
    message: str
    span: SourceSpan | None = None
    severity: DiagnosticSeverity = DiagnosticSeverity.ERROR
