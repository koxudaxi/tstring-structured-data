use std::collections::BTreeMap;

use num_bigint::BigInt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub token_index: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceSpan {
    #[must_use]
    pub fn point(token_index: usize, offset: usize) -> Self {
        let position = SourcePosition {
            token_index,
            offset,
        };
        Self {
            start: position.clone(),
            end: position,
        }
    }

    #[must_use]
    pub fn between(start: SourcePosition, end: SourcePosition) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub fn extend(&self, end: SourcePosition) -> Self {
        Self {
            start: self.start.clone(),
            end,
        }
    }

    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            start: self.start.clone(),
            end: other.end.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub span: Option<SourceSpan>,
    pub metadata: BTreeMap<String, String>,
}

impl Diagnostic {
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            severity: DiagnosticSeverity::Error,
            span,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Semantic,
    Unrepresentable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendError {
    pub kind: ErrorKind,
    pub message: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl BackendError {
    #[must_use]
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Parse, "tstring.parse", message, None)
    }

    #[must_use]
    pub fn parse_at(
        code: impl Into<String>,
        message: impl Into<String>,
        span: impl Into<Option<SourceSpan>>,
    ) -> Self {
        Self::new(ErrorKind::Parse, code, message, span.into())
    }

    #[must_use]
    pub fn semantic(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Semantic, "tstring.semantic", message, None)
    }

    #[must_use]
    pub fn semantic_at(
        code: impl Into<String>,
        message: impl Into<String>,
        span: impl Into<Option<SourceSpan>>,
    ) -> Self {
        Self::new(ErrorKind::Semantic, code, message, span.into())
    }

    #[must_use]
    pub fn unrepresentable(message: impl Into<String>) -> Self {
        Self::new(
            ErrorKind::Unrepresentable,
            "tstring.unrepresentable",
            message,
            None,
        )
    }

    #[must_use]
    pub fn unrepresentable_at(
        code: impl Into<String>,
        message: impl Into<String>,
        span: impl Into<Option<SourceSpan>>,
    ) -> Self {
        Self::new(ErrorKind::Unrepresentable, code, message, span.into())
    }

    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Some(primary) = self.diagnostics.first_mut() {
            primary.metadata.insert(key.into(), value.into());
        }
        self
    }

    fn new(
        kind: ErrorKind,
        code: impl Into<String>,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        let message = message.into();
        Self {
            kind,
            diagnostics: vec![Diagnostic::error(code, message.clone(), span)],
            message,
        }
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedStream {
    pub documents: Vec<NormalizedDocument>,
}

impl NormalizedStream {
    #[must_use]
    pub fn new(documents: Vec<NormalizedDocument>) -> Self {
        Self { documents }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NormalizedDocument {
    Empty,
    Value(NormalizedValue),
}

#[derive(Clone, Debug, PartialEq)]
pub enum NormalizedValue {
    Null,
    Bool(bool),
    Integer(BigInt),
    Float(NormalizedFloat),
    String(String),
    Temporal(NormalizedTemporal),
    Sequence(Vec<NormalizedValue>),
    Mapping(Vec<NormalizedEntry>),
    Set(Vec<NormalizedKey>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedEntry {
    pub key: NormalizedKey,
    pub value: NormalizedValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NormalizedKey {
    Null,
    Bool(bool),
    Integer(BigInt),
    Float(NormalizedFloat),
    String(String),
    Temporal(NormalizedTemporal),
    Sequence(Vec<NormalizedKey>),
    Mapping(Vec<NormalizedKeyEntry>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedKeyEntry {
    pub key: NormalizedKey,
    pub value: NormalizedKey,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NormalizedFloat {
    Finite(f64),
    PosInf,
    NegInf,
    NaN,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizedTemporal {
    OffsetDateTime(NormalizedOffsetDateTime),
    LocalDateTime(NormalizedLocalDateTime),
    LocalDate(NormalizedDate),
    LocalTime(NormalizedTime),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedOffsetDateTime {
    pub date: NormalizedDate,
    pub time: NormalizedTime,
    pub offset_minutes: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedLocalDateTime {
    pub date: NormalizedDate,
    pub time: NormalizedTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizedDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizedTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
}

impl NormalizedFloat {
    #[must_use]
    pub fn finite(value: f64) -> Self {
        debug_assert!(value.is_finite());
        Self::Finite(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateInterpolation {
    pub expression: String,
    pub conversion: Option<String>,
    pub format_spec: String,
    pub interpolation_index: usize,
    pub raw_source: Option<String>,
}

impl TemplateInterpolation {
    #[must_use]
    pub fn expression_label(&self) -> &str {
        if self.expression.is_empty() {
            "slot"
        } else {
            &self.expression
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticTextToken {
    pub text: String,
    pub token_index: usize,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpolationToken {
    pub interpolation: TemplateInterpolation,
    pub interpolation_index: usize,
    pub token_index: usize,
    pub span: SourceSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateToken {
    StaticText(StaticTextToken),
    Interpolation(InterpolationToken),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamItem {
    Char {
        ch: char,
        span: SourceSpan,
    },
    Interpolation {
        interpolation: TemplateInterpolation,
        interpolation_index: usize,
        span: SourceSpan,
    },
    Eof {
        span: SourceSpan,
    },
}

impl StreamItem {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Char { .. } => "char",
            Self::Interpolation { .. } => "interpolation",
            Self::Eof { .. } => "eof",
        }
    }

    #[must_use]
    pub fn char(&self) -> Option<char> {
        match self {
            Self::Char { ch, .. } => Some(*ch),
            _ => None,
        }
    }

    #[must_use]
    pub fn interpolation(&self) -> Option<&TemplateInterpolation> {
        match self {
            Self::Interpolation { interpolation, .. } => Some(interpolation),
            _ => None,
        }
    }

    #[must_use]
    pub fn interpolation_index(&self) -> Option<usize> {
        match self {
            Self::Interpolation {
                interpolation_index,
                ..
            } => Some(*interpolation_index),
            _ => None,
        }
    }

    #[must_use]
    pub fn span(&self) -> &SourceSpan {
        match self {
            Self::Char { span, .. } | Self::Interpolation { span, .. } | Self::Eof { span } => span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateSegment {
    StaticText(String),
    Interpolation(TemplateInterpolation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateInput {
    pub segments: Vec<TemplateSegment>,
}

impl TemplateInput {
    #[must_use]
    pub fn from_segments(segments: Vec<TemplateSegment>) -> Self {
        Self { segments }
    }

    #[must_use]
    pub fn from_parts(strings: Vec<String>, interpolations: Vec<TemplateInterpolation>) -> Self {
        debug_assert_eq!(strings.len(), interpolations.len() + 1);

        let mut segments = Vec::with_capacity(strings.len() + interpolations.len());
        for (interpolation_index, interpolation) in interpolations.into_iter().enumerate() {
            let text = strings[interpolation_index].clone();
            if !text.is_empty() {
                segments.push(TemplateSegment::StaticText(text));
            }
            segments.push(TemplateSegment::Interpolation(interpolation));
        }

        let tail = strings.last().cloned().unwrap_or_default();
        if !tail.is_empty() || segments.is_empty() {
            segments.push(TemplateSegment::StaticText(tail));
        }

        Self { segments }
    }

    #[must_use]
    pub fn tokenize(&self) -> Vec<TemplateToken> {
        let mut tokens = Vec::new();

        for (token_index, segment) in self.segments.iter().enumerate() {
            match segment {
                TemplateSegment::StaticText(text) => {
                    let end = text.chars().count();
                    tokens.push(TemplateToken::StaticText(StaticTextToken {
                        text: text.clone(),
                        token_index,
                        span: SourceSpan::between(
                            SourcePosition {
                                token_index,
                                offset: 0,
                            },
                            SourcePosition {
                                token_index,
                                offset: end,
                            },
                        ),
                    }));
                }
                TemplateSegment::Interpolation(interpolation) => {
                    tokens.push(TemplateToken::Interpolation(InterpolationToken {
                        interpolation: interpolation.clone(),
                        interpolation_index: interpolation.interpolation_index,
                        token_index,
                        span: SourceSpan::point(token_index, 0),
                    }));
                }
            }
        }

        tokens
    }

    #[must_use]
    pub fn flatten(&self) -> Vec<StreamItem> {
        let mut items = Vec::new();

        for token in self.tokenize() {
            match token {
                TemplateToken::StaticText(token) => {
                    for (offset, ch) in token.text.chars().enumerate() {
                        items.push(StreamItem::Char {
                            ch,
                            span: SourceSpan::between(
                                SourcePosition {
                                    token_index: token.token_index,
                                    offset,
                                },
                                SourcePosition {
                                    token_index: token.token_index,
                                    offset: offset + 1,
                                },
                            ),
                        });
                    }
                }
                TemplateToken::Interpolation(token) => {
                    items.push(StreamItem::Interpolation {
                        interpolation: token.interpolation,
                        interpolation_index: token.interpolation_index,
                        span: token.span,
                    });
                }
            }
        }

        let eof_span = items
            .last()
            .map_or_else(|| SourceSpan::point(0, 0), |item| item.span().clone());
        items.push(StreamItem::Eof { span: eof_span });
        items
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Diagnostic, DiagnosticSeverity, ErrorKind, SourcePosition, SourceSpan, StreamItem,
        TemplateInput, TemplateInterpolation, TemplateSegment, TemplateToken,
    };

    #[test]
    fn span_helpers_compose() {
        let base = SourceSpan::between(
            SourcePosition {
                token_index: 0,
                offset: 0,
            },
            SourcePosition {
                token_index: 0,
                offset: 3,
            },
        );
        let extended = base.extend(SourcePosition {
            token_index: 0,
            offset: 5,
        });
        let merged = base.merge(&SourceSpan::point(2, 0));
        assert_eq!(extended.end.offset, 5);
        assert_eq!(merged.end.token_index, 2);
    }

    #[test]
    fn tokenize_and_flatten_templates_preserve_structure() {
        let template = TemplateInput::from_segments(vec![
            TemplateSegment::StaticText("{\"name\": ".to_owned()),
            TemplateSegment::Interpolation(TemplateInterpolation {
                expression: "value".to_owned(),
                conversion: None,
                format_spec: String::new(),
                interpolation_index: 0,
                raw_source: Some("{value}".to_owned()),
            }),
            TemplateSegment::StaticText("}".to_owned()),
        ]);

        let tokens = template.tokenize();
        assert_eq!(tokens.len(), 3);
        assert!(matches!(tokens[0], TemplateToken::StaticText(_)));
        assert!(matches!(tokens[1], TemplateToken::Interpolation(_)));
        assert!(matches!(tokens[2], TemplateToken::StaticText(_)));

        let items = template.flatten();
        assert_eq!(
            items
                .iter()
                .take(5)
                .map(StreamItem::kind)
                .collect::<Vec<_>>(),
            vec!["char", "char", "char", "char", "char"]
        );
        assert_eq!(items.last().map(StreamItem::kind), Some("eof"));
    }

    #[test]
    fn from_parts_preserves_interpolation_metadata() {
        let extracted = TemplateInput::from_parts(
            vec!["hello ".to_owned(), String::new()],
            vec![TemplateInterpolation {
                expression: "value".to_owned(),
                conversion: Some("r".to_owned()),
                format_spec: ">5".to_owned(),
                interpolation_index: 0,
                raw_source: Some("{value!r:>5}".to_owned()),
            }],
        );

        assert_eq!(extracted.segments.len(), 2);
        let TemplateSegment::Interpolation(interpolation) = &extracted.segments[1] else {
            panic!("expected interpolation segment");
        };
        assert_eq!(interpolation.expression, "value");
        assert_eq!(interpolation.conversion.as_deref(), Some("r"));
        assert_eq!(interpolation.format_spec, ">5");
        assert_eq!(interpolation.interpolation_index, 0);
        assert_eq!(interpolation.expression_label(), "value");
    }

    #[test]
    fn diagnostics_capture_code_and_span() {
        let span = SourceSpan::point(3, 2);
        let diagnostic = Diagnostic::error("json.parse", "unexpected token", Some(span.clone()));
        assert_eq!(diagnostic.code, "json.parse");
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostic.span, Some(span));
        let error = super::BackendError::parse_at(
            "json.parse",
            "unexpected token",
            Some(SourceSpan::point(1, 0)),
        );
        assert_eq!(error.kind, ErrorKind::Parse);
        assert_eq!(error.diagnostics.len(), 1);
        assert_eq!(error.diagnostics[0].code, "json.parse");
    }
}
