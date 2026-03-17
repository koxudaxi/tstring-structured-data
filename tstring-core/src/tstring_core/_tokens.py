from __future__ import annotations

from dataclasses import dataclass
from string.templatelib import Interpolation, Template
from typing import Literal, cast

from ._spans import SourcePosition, SourceSpan

TemplateTokenKind = Literal["text", "interpolation"]
StreamItemKind = Literal["char", "interpolation", "eof"]


@dataclass(frozen=True, slots=True)
class StaticTextToken:
    text: str
    token_index: int
    span: SourceSpan
    kind: TemplateTokenKind = "text"


@dataclass(frozen=True, slots=True)
class InterpolationToken:
    interpolation: Interpolation
    interpolation_index: int
    token_index: int
    span: SourceSpan
    kind: TemplateTokenKind = "interpolation"

    @property
    def expression(self) -> str:
        return self.interpolation.expression or f"slot {self.interpolation_index}"


TemplateToken = StaticTextToken | InterpolationToken


@dataclass(frozen=True, slots=True)
class StreamItem:
    kind: StreamItemKind
    value: str | Interpolation | None
    span: SourceSpan
    interpolation_index: int | None = None

    @property
    def char(self) -> str | None:
        return cast("str | None", self.value) if self.kind == "char" else None

    @property
    def interpolation(self) -> Interpolation | None:
        return (
            cast("Interpolation | None", self.value)
            if self.kind == "interpolation"
            else None
        )


def tokenize_template(template: Template) -> list[TemplateToken]:
    tokens: list[TemplateToken] = []
    token_index = 0

    for interpolation_index, interpolation in enumerate(template.interpolations):
        text = template.strings[interpolation_index]
        if text:
            tokens.append(
                StaticTextToken(
                    text=text,
                    token_index=token_index,
                    span=SourceSpan.between(
                        SourcePosition(token_index=token_index, offset=0),
                        SourcePosition(token_index=token_index, offset=len(text)),
                    ),
                )
            )
            token_index += 1

        tokens.append(
            InterpolationToken(
                interpolation=interpolation,
                interpolation_index=interpolation_index,
                token_index=token_index,
                span=SourceSpan.point(token_index=token_index, offset=0),
            )
        )
        token_index += 1

    tail = template.strings[len(template.interpolations)]
    if tail or not tokens:
        tokens.append(
            StaticTextToken(
                text=tail,
                token_index=token_index,
                span=SourceSpan.between(
                    SourcePosition(token_index=token_index, offset=0),
                    SourcePosition(token_index=token_index, offset=len(tail)),
                ),
            )
        )

    return tokens


def flatten_template(template: Template) -> list[StreamItem]:
    items: list[StreamItem] = []

    for token in tokenize_template(template):
        if isinstance(token, StaticTextToken):
            for offset, char in enumerate(token.text):
                items.append(
                    StreamItem(
                        kind="char",
                        value=char,
                        span=SourceSpan.between(
                            SourcePosition(
                                token_index=token.token_index, offset=offset
                            ),
                            SourcePosition(
                                token_index=token.token_index,
                                offset=offset + 1,
                            ),
                        ),
                    )
                )
            continue

        items.append(
            StreamItem(
                kind="interpolation",
                value=token.interpolation,
                interpolation_index=token.interpolation_index,
                span=token.span,
            )
        )

    eof_span = items[-1].span if items else SourceSpan.point(token_index=0, offset=0)
    items.append(StreamItem(kind="eof", value=None, span=eof_span))
    return items
