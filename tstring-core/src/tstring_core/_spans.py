from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class SourcePosition:
    token_index: int
    offset: int


@dataclass(frozen=True, slots=True)
class SourceSpan:
    start: SourcePosition
    end: SourcePosition

    @classmethod
    def point(cls, token_index: int, offset: int) -> "SourceSpan":
        position = SourcePosition(token_index=token_index, offset=offset)
        return cls(start=position, end=position)

    @classmethod
    def between(cls, start: SourcePosition, end: SourcePosition) -> "SourceSpan":
        return cls(start=start, end=end)

    def extend(self, end: SourcePosition) -> "SourceSpan":
        return SourceSpan(start=self.start, end=end)

    def merge(self, other: "SourceSpan") -> "SourceSpan":
        return SourceSpan(start=self.start, end=other.end)
