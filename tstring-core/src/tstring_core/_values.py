from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import Generic, TypeVar

from ._types import StructuredData

TData = TypeVar("TData", bound=StructuredData, covariant=True)


class ValueKind(str, Enum):
    TEXT = "text"
    DATA = "data"


@dataclass(frozen=True, slots=True)
class RenderResult(Generic[TData]):
    text: str
    data: TData
